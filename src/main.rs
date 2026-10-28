//! Command line: runs [`job::process`] over the input files, keeps the log, stops the batch on fatal errors.

mod cli;
mod ui;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget};

use cli::{ApiArgs, Cli, Command, RenderArgs, RunArgs};
use sonisub::batch::{self, Item, Selection, Totals};
use sonisub::cancel::{Cancelled, interrupt, interrupted};
use sonisub::job::{self, Outcome};
use sonisub::languages;
use sonisub::progress::Callback;
use sonisub::soniox::{self, Client, api_error};
use sonisub::usage::{self, Summary};

/// Prints the library's warnings (e.g. an upload that could not be deleted on Soniox) to stderr.
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("warning: {}", record.args());
        }
    }

    fn flush(&self) {}
}

fn main() -> ExitCode {
    if log::set_logger(&StderrLogger).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    let _ = ctrlc::set_handler(|| {
        if interrupt() {
            // Second Ctrl+C: give up on cleanup.
            std::process::exit(130);
        }
        eprintln!("\ninterrupted, cleaning up... (Ctrl+C again to quit immediately)");
    });

    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Purge { yes, api }) => purge(&api, yes).map(|_| ExitCode::SUCCESS),
        Some(Command::Usage { days, api }) => show_usage(&api, days).map(|_| ExitCode::SUCCESS),
        Some(Command::Languages { api }) => show_languages(&api).map(|_| ExitCode::SUCCESS),
        Some(Command::Render(args)) => render(&args).map(|_| ExitCode::SUCCESS),
        None => run(&cli.run),
    };
    result.unwrap_or_else(|e| {
        eprintln!("error: {e:#}");
        ExitCode::from(2)
    })
}

fn run(args: &RunArgs) -> Result<ExitCode> {
    let sel = Selection { filters: args.filters.clone(), recursive: args.recursive, out_dir: args.out_dir.clone() };
    let mut items = batch::collect(&args.inputs, &sel)?;
    if items.is_empty() {
        let filter =
            if args.filters.is_empty() { String::new() } else { format!(" matching {}", args.filters.join(", ")) };
        bail!(
            "no media files{filter} in {}",
            args.inputs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        );
    }
    let folder_mode = items.len() > 1 || args.inputs.iter().any(|p| p.is_dir());
    if let Some(o) = &args.output {
        if folder_mode {
            bail!("--output works with a single file; use --out-dir for folders and several files");
        }
        items[0].output = job::output_base(o);
    }

    let mut opts = args.job_options();
    let scanning = spinner(format!("reading {} file(s)...", items.len()), folder_mode && !args.quiet);
    let plan = batch::plan(&items, &opts);
    scanning.finish_and_clear();
    let totals = Totals::of(&plan);
    let needs_api = totals.transcribe > 0;

    let client = match &args.api.api_key {
        Some(k) if !k.trim().is_empty() => Some(Client::new(&args.api.api_url, k.trim())?),
        _ if needs_api && !args.dry_run => return Err(soniox::missing_key()),
        _ => None,
    };
    let price = match (&client, needs_api) {
        (Some(c), true) => recent_price(c),
        _ => usage::FALLBACK_USD_PER_HOUR,
    };
    if folder_mode || args.dry_run {
        eprint!("{}", plan_text(&plan, &totals, price, args.dry_run, &exists_label(&args.formats)));
    }
    if args.dry_run {
        return Ok(ExitCode::SUCCESS);
    }
    if let (Some(c), true) = (&client, needs_api) {
        c.check_auth()?;
    }

    // Tags this run's requests in Soniox usage logs, to report what it cost.
    let run_started = SystemTime::now();
    opts.reference =
        format!("sonisub-{}", run_started.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis());
    // Without a terminal indicatif draws nothing (no escape codes); --quiet draws nothing either.
    let multi =
        if args.quiet { MultiProgress::with_draw_target(ProgressDrawTarget::hidden()) } else { MultiProgress::new() };
    opts.progress = Some(multi.clone());
    let tracker = needs_api.then(|| Arc::new(ui::Tracker::new(&multi, &plan, price)));
    if let Some(t) = &tracker {
        let t = t.clone();
        opts.on_progress = Some(Callback::new(move |p| t.report(&p)));
    }
    // Result lines go above the bars; without a terminal they are plain lines.
    let say = |line: String| {
        if !multi.is_hidden() {
            let _ = multi.println(line);
        } else {
            eprintln!("{line}");
        }
    };

    let exists = exists_label(&args.formats);
    let mut stats = Stats::default();
    let (mut sent_files, mut sent_audio_s) = (0usize, 0.0);
    let total = plan.len();
    let run_began = Instant::now();
    for (i, planned) in plan.iter().enumerate() {
        if interrupted() {
            break;
        }
        let Item { input, output, name } = &planned.item;
        opts.prefix = if folder_mode { format!("  {}", ui::short(name, 28)) } else { String::new() };
        if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let log = args.log.clone().unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")).join("sonisub.log"));
        let started = Instant::now();
        let planned_s = match planned.action {
            batch::Action::Transcribe { audio_s } => Some(audio_s),
            _ => None,
        };
        if let (Some(t), Some(audio_s)) = (&tracker, planned_s) {
            t.start_file(audio_s);
        }

        let result = job::process(input, output, &opts, client.as_ref());
        if let Some(secs) = result.as_ref().ok().and_then(Outcome::uploaded_s) {
            sent_files += 1;
            sent_audio_s += secs;
        }
        let fatal = result.as_ref().err().and_then(api_error).filter(|a| a.is_fatal()).map(|a| a.error_type.clone());
        let (line, log_msg) = describe(name, &result, &mut stats, started.elapsed().as_secs_f64(), &exists);
        say(line);
        log_line(&log, input, &log_msg);
        if result.is_err() && !folder_mode {
            eprintln!("  (logged to {})", log.display());
        }

        if let (Some(t), Some(_)) = (&tracker, planned_s) {
            t.finish_file(result.as_ref().ok().and_then(Outcome::uploaded_s));
        }
        if interrupted() {
            break;
        }
        // Balance / budget / auth: every remaining file would fail the same way.
        if let Some(kind) = fatal {
            let rest = total - i - 1;
            if rest > 0 {
                say(format!("stopping: {kind} — {rest} remaining file(s) not processed"));
            }
            stats.fatal = true;
            break;
        }
    }
    if let Some(t) = tracker {
        t.finish();
    }
    if folder_mode || interrupted() {
        eprintln!(
            "{}",
            stats.summary(&exists, interrupted().then(|| cleanup_note(args.keep_audio)), total, run_began.elapsed())
        );
    }
    report_run_cost(client.as_ref(), &opts.reference, run_started, sent_audio_s, sent_files);
    Ok(if interrupted() {
        ExitCode::from(130)
    } else if stats.fatal {
        ExitCode::from(2)
    } else if stats.failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

#[derive(Default)]
struct Stats {
    written: usize,
    from_saved: usize,
    skipped: usize,
    no_speech: usize,
    no_audio: usize,
    failed: usize,
    /// The file Ctrl+C stopped.
    cancelled: usize,
    fatal: bool,
}

impl Stats {
    /// The end line. After Ctrl+C (`cleanup` says what the library removed) it says how much was finished.
    fn summary(&self, exists: &str, cleanup: Option<&str>, total: usize, took: Duration) -> String {
        let skipped = format!("skipped ({exists})");
        let parts = [
            (self.written, "processed"),
            (self.from_saved, "from saved transcripts"),
            (self.skipped, skipped.as_str()),
            (self.no_speech, "without speech"),
            (self.no_audio, "without audio"),
            (self.failed, "failed"),
        ];
        let text: Vec<String> = parts.iter().filter(|(n, _)| *n > 0).map(|(n, what)| format!("{n} {what}")).collect();
        let text = if text.is_empty() { "nothing to do".into() } else { text.join(", ") };
        let secs = took.as_secs();
        let time = if secs >= 5 { format!(" in {}", usage::fmt_minutes(secs * 1000)) } else { String::new() };
        if let Some(cleanup) = cleanup {
            let done = self.written + self.from_saved + self.skipped + self.no_speech + self.no_audio + self.failed;
            let rest = total.saturating_sub(done);
            format!("interrupted: {text}{time}, {rest} of {total} file(s) not finished; {cleanup}")
        } else {
            format!("done: {text}{time}")
        }
    }
}

/// What was removed after Ctrl+C: the job guards delete the uploads, and the temp audio unless it is kept.
fn cleanup_note(keep_audio: bool) -> &'static str {
    if keep_audio { "Soniox uploads removed (audio kept)" } else { "temp audio and Soniox uploads removed" }
}

/// ".srt exists", ".srt, .premiere.json exist": why a file is skipped.
fn exists_label(formats: &[job::Format]) -> String {
    let names: Vec<String> = formats
        .iter()
        .map(|f| f.path(Path::new("x.srt")).to_string_lossy().trim_start_matches('x').to_string())
        .collect();
    format!("{} exist{}", names.join(", "), if names.len() == 1 { "s" } else { "" })
}

/// One result line for the screen, one for the log.
fn describe(name: &str, result: &Result<Outcome>, stats: &mut Stats, secs: f64, exists: &str) -> (String, String) {
    match result {
        Ok(Outcome::Written { files, cues, cached, .. }) => {
            let from = if *cached {
                stats.from_saved += 1;
                ", from saved transcript"
            } else {
                stats.written += 1;
                ""
            };
            let what = if *cues > 0 { format!("{cues} cues") } else { "transcript".into() };
            let paths: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
            (format!("✓ {name} — {what}{from}"), format!("OK -> {} ({secs:.0}s){from}", paths.join(", ")))
        }
        Ok(Outcome::Skipped { .. }) => {
            stats.skipped += 1;
            (format!("· {name} — skipped, {exists} (--force to redo)"), format!("SKIPPED ({exists})"))
        }
        Ok(Outcome::NoAudio { reason }) => {
            stats.no_audio += 1;
            (format!("– {name} — {reason}"), format!("NO AUDIO ({reason})"))
        }
        Ok(Outcome::NoSpeech { marker, cached, uploaded_s }) => {
            stats.no_speech += 1;
            let note = match (marker, cached, uploaded_s) {
                (_, true, _) => "checked before (--force to try again)".to_string(),
                (_, false, None) => "silent, nothing uploaded".to_string(),
                (Some(m), false, Some(_)) => {
                    format!(
                        "marker saved: {}",
                        m.file_name().map_or_else(|| m.display().to_string(), |n| n.to_string_lossy().into())
                    )
                }
                (None, false, Some(_)) => "nothing recognised".to_string(),
            };
            (format!("∅ {name} — no speech, {note}"), format!("NO SPEECH ({note})"))
        }
        Err(e) if e.is::<Cancelled>() => {
            stats.cancelled += 1;
            (format!("⏹ {name} — interrupted"), "INTERRUPTED".into())
        }
        Err(e) => {
            stats.failed += 1;
            (format!("✗ {name} — {e:#}"), format!("ERROR {e:#}"))
        }
    }
}

/// What is about to happen, before anything is uploaded.
fn plan_text(plan: &[batch::Planned], t: &Totals, usd_per_hour: f64, detailed: bool, exists: &str) -> String {
    let mut out = String::new();
    if detailed {
        for p in plan {
            let name = &p.item.name;
            let what = match &p.action {
                batch::Action::Skip => "skip       ".to_string(),
                batch::Action::FromTranscript => "rebuild    ".to_string(),
                batch::Action::Cached { speech: true } => "saved      ".to_string(),
                batch::Action::Cached { speech: false } => "no speech  ".to_string(),
                batch::Action::NoAudio => "no audio   ".to_string(),
                batch::Action::Transcribe { audio_s: Some(s) } => {
                    format!("send {:>6} ", usage::fmt_minutes((s * 1000.0) as u64))
                }
                batch::Action::Transcribe { audio_s: None } => "send     ? ".to_string(),
            };
            out.push_str(&format!("  {what} {name}\n"));
        }
    }
    out.push_str(&format!("{} file(s):\n", plan.len()));
    if t.transcribe > 0 {
        let unknown =
            if t.unknown_length > 0 { format!(" + {} of unknown length", t.unknown_length) } else { String::new() };
        out.push_str(&format!(
            "  {:>4}  to transcribe: {} audio{unknown}, ≈ {} (at {}/h)\n",
            t.transcribe,
            usage::fmt_minutes((t.audio_s * 1000.0) as u64),
            usage::fmt_usd(usd_per_hour * t.audio_s / 3600.0),
            usage::fmt_usd(usd_per_hour)
        ));
    }
    for (n, what) in [
        (t.cached, "from saved transcripts, free"),
        (t.from_transcript, "rebuilt from transcripts, free"),
        (t.cached_silent, "checked before, no speech"),
        (t.no_audio, "no audio track"),
        (t.skip, &format!("{exists}, skipped (--force to redo)")),
    ] {
        if n > 0 {
            out.push_str(&format!(
                "  {n:>4}  {what}
"
            ));
        }
    }
    out
}

fn spinner(msg: String, visible: bool) -> ProgressBar {
    if !visible {
        return ProgressBar::hidden();
    }
    let pb = ProgressBar::new_spinner().with_message(msg);
    pb.enable_steady_tick(Duration::from_millis(120));
    pb
}

/// Transcription price per hour from the last 30 days of usage, for estimates.
fn recent_price(client: &Client) -> f64 {
    let now = SystemTime::now();
    usage::fetch(client, now - Duration::from_secs(30 * 86_400), now)
        .ok()
        .and_then(|logs| Summary::of(&logs).stt_usd_per_hour())
        .unwrap_or(usage::FALLBACK_USD_PER_HOUR)
}

/// "this run: 3:31 audio, $0.0057" — exact from the usage logs (they lag a few seconds, so wait a
/// little), otherwise estimated from the price seen in the last 30 days.
fn report_run_cost(client: Option<&Client>, reference: &str, started: SystemTime, uploaded_s: f64, uploads: usize) {
    let Some(client) = client.filter(|_| uploads > 0) else { return };
    let audio = usage::fmt_minutes((uploaded_s * 1000.0) as u64);
    let mut exact = None;
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(2));
        }
        let now = SystemTime::now();
        let Ok(logs) = client.usage_logs(started - Duration::from_secs(60), now + Duration::from_secs(60)) else {
            break;
        };
        let mine: Vec<_> = usage::of_run(&logs, reference).into_iter().cloned().collect();
        if mine.len() >= uploads {
            exact = Some(Summary::of(&mine));
            break;
        }
    }
    match exact {
        Some(s) => eprintln!("this run: {audio} audio, {}", usage::fmt_usd(s.cost_usd)),
        None => {
            let price = recent_price(client);
            eprintln!("this run: {audio} audio, ≈ {} (estimate)", usage::fmt_usd(price * uploaded_s / 3600.0));
        }
    }
}

fn show_usage(api: &ApiArgs, days: u64) -> Result<()> {
    let key = api.api_key.as_deref().filter(|k| !k.trim().is_empty()).ok_or_else(soniox::missing_key)?;
    let client = Client::new(&api.api_url, key.trim())?;
    let now = SystemTime::now();
    let logs = usage::fetch(&client, now - Duration::from_secs(days * 86_400), now)?;
    let day_start = now - Duration::from_secs(now.duration_since(SystemTime::UNIX_EPOCH)?.as_secs() % 86_400);
    let today: Vec<_> = logs
        .iter()
        .filter(|e| {
            e["end_time"]
                .as_str()
                .and_then(|t| humantime::parse_rfc3339_weak(t.trim_end_matches('Z')).ok())
                .is_some_and(|t| t >= day_start)
        })
        .cloned()
        .collect();
    print!("{}", usage::report(&Summary::of(&logs), &Summary::of(&today), days));
    Ok(())
}

/// `sonisub render`: subtitles from a CAPT file or saved words, no Soniox.
fn render(args: &RenderArgs) -> Result<()> {
    use sonisub::captions::capt::Doc;
    let input = &args.input;
    let text = std::fs::read_to_string(input).with_context(|| format!("cannot read {}", input.display()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let name = input.file_name().map_or_else(|| input.display().to_string(), |n| n.to_string_lossy().into_owned());
    let is_capt = input.extension().is_some_and(|e| e.eq_ignore_ascii_case("capt")) || text.starts_with("CAPT ");
    let (words, doc) = if is_capt {
        let doc = Doc::parse(text).map_err(|errors| {
            let list: Vec<String> = errors.0.iter().map(|e| format!("  {e}")).collect();
            anyhow!("{} is not a valid CAPT file:\n{}", input.display(), list.join("\n"))
        })?;
        (doc.to_words(), Some(doc))
    } else {
        let words: Vec<sonisub::captions::Word> = serde_json::from_str(text)
            .with_context(|| format!("{} is neither a CAPT file nor a JSON list of words", input.display()))?;
        (words, None)
    };
    let sounds = doc.as_ref().is_some_and(|d| d.sounds().next().is_some());
    if words.is_empty() && !(args.seg.sdh && sounds) {
        bail!("{} has no words", input.display());
    }
    let output = job::output_base(args.output.as_deref().unwrap_or(input));
    let targets = job::missing_outputs(&output, &args.formats, args.force);
    if targets.is_empty() {
        eprintln!("· {name} — skipped, {} (--force to redo)", exists_label(&args.formats));
        return Ok(());
    }
    let mut layout = args.seg.layout(true, doc.as_ref().and_then(|d| d.frame));
    let mut outputs = args.seg.outputs();
    let given = !layout.speaker_names.is_empty();
    // Saved words become a CAPT file without an id (the input's name only seeds the Premiere ids),
    // so every subtitle is made the same way as from a CAPT file.
    let mut doc = doc.unwrap_or_else(|| Doc::from_words(&words, None));
    // --vertical or --frame go into the CAPT written, so the next render lays it out the same.
    doc.frame = outputs.frame.or(doc.frame);
    if given {
        // --speaker-names win over a CAPT file's names, and go into the CAPT written.
        job::name_speakers(&mut doc, &layout, &outputs.ass);
    } else {
        layout.speaker_names = job::speaker_names(&doc);
    }
    outputs.ass.names = doc.speakers.iter().filter_map(|s| Some((s.id.clone(), s.name.clone()?))).collect();
    let seed = doc.id.clone().unwrap_or_else(|| name.clone());
    let language = if is_capt { doc.lang.clone() } else { None };
    let (files, cues) = job::write_outputs(&targets, &words, &doc, &layout, &outputs, language.as_deref(), &seed)?;
    let what = if cues > 0 { format!("{cues} cues") } else { "transcript".into() };
    let paths: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
    eprintln!("✓ {name} — {what} -> {}", paths.join(", "));
    Ok(())
}

fn show_languages(api: &ApiArgs) -> Result<()> {
    let key = api.api_key.as_deref().filter(|k| !k.trim().is_empty()).ok_or_else(soniox::missing_key)?;
    let client = Client::new(&api.api_url, key.trim())?;
    print!("{}", languages::report(&languages::fetch(&client)?));
    Ok(())
}

fn purge(api: &ApiArgs, yes: bool) -> Result<()> {
    let key = api.api_key.as_deref().filter(|k| !k.trim().is_empty()).ok_or_else(soniox::missing_key)?;
    let client = Client::new(&api.api_url, key.trim())?;
    let tr = client.list("transcriptions")?;
    let files = client.list("files")?;
    for (kind, items) in [("transcription", &tr), ("file", &files)] {
        for (id, desc) in items.iter() {
            println!("{kind:13} {id}  {desc}");
        }
    }
    println!("{} transcription(s), {} file(s)", tr.len(), files.len());
    if !yes {
        if !tr.is_empty() || !files.is_empty() {
            println!("dry run: add --yes to delete them");
        }
        return Ok(());
    }
    let mut errors = 0;
    for (id, _) in &tr {
        if let Err(e) = client.delete_transcription(id) {
            errors += 1;
            eprintln!("transcription {id}: {e:#}");
        }
    }
    for (id, _) in &files {
        if let Err(e) = client.delete_file(id) {
            errors += 1;
            eprintln!("file {id}: {e:#}");
        }
    }
    if errors > 0 {
        return Err(anyhow!("{errors} item(s) could not be deleted"));
    }
    println!("deleted");
    Ok(())
}

fn log_line(log: &Path, input: &Path, msg: &str) {
    let ts = humantime::format_rfc3339_seconds(SystemTime::now());
    let line = format!("{ts}\t{}\t{msg}\n", input.display());
    let res = OpenOptions::new().create(true).append(true).open(log).and_then(|mut f| f.write_all(line.as_bytes()));
    if let Err(e) = res {
        eprintln!("  warning: cannot write log {}: {e}", log.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_file_is_not_a_failure_and_counts_as_not_finished() {
        let mut stats = Stats::default();
        let (line, _) = describe("c.mp4", &Err(Cancelled.into()), &mut stats, 1.0, ".srt exists");
        assert_eq!(line, "⏹ c.mp4 — interrupted");
        assert_eq!((stats.failed, stats.cancelled), (0, 1));
        stats.written = 3;
        let s = stats.summary(".srt exists", Some(cleanup_note(false)), 10, Duration::from_secs(130));
        assert_eq!(
            s,
            "interrupted: 3 processed in 2:10, 7 of 10 file(s) not finished; temp audio and Soniox uploads removed"
        );
        let kept = stats.summary(".srt exists", Some(cleanup_note(true)), 10, Duration::ZERO);
        assert!(kept.ends_with("Soniox uploads removed (audio kept)"), "{kept}");
        assert_eq!(Stats::default().summary("x", None, 2, Duration::ZERO), "done: nothing to do");
    }
}
