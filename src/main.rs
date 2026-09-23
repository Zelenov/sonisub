mod audio;
mod cli;
mod soniox;
mod srt;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::Value;

use cli::{ApiArgs, Cli, Command, RunArgs};
use soniox::{Client, RemoteGuard, api_error};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

fn main() -> ExitCode {
    let _ = ctrlc::set_handler(|| {
        if INTERRUPTED.swap(true, Ordering::Relaxed) {
            // Second Ctrl+C: give up on cleanup.
            std::process::exit(130);
        }
        eprintln!("\ninterrupted, cleaning up... (Ctrl+C again to quit immediately)");
    });

    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Purge { yes, api }) => purge(&api, yes).map(|_| ExitCode::SUCCESS),
        None => run(&cli.run),
    };
    result.unwrap_or_else(|e| {
        eprintln!("error: {e:#}");
        ExitCode::from(2)
    })
}

enum Outcome {
    Done,
    Skipped,
}

fn run(args: &RunArgs) -> Result<ExitCode> {
    if args.output.is_some() && args.inputs.len() > 1 {
        bail!("--output works with a single input; use --out-dir for several");
    }
    let needs_api = args.inputs.iter().any(|p| !is_json(p));
    let client = match (&args.api.api_key, needs_api) {
        (Some(k), _) if !k.trim().is_empty() => Some(Client::new(&args.api.api_url, k.trim())?),
        (_, true) => return Err(soniox::missing_key()),
        _ => None,
    };
    if let (Some(c), true) = (&client, needs_api) {
        c.check_auth()?;
    }
    if let Some(d) = &args.out_dir {
        std::fs::create_dir_all(d).with_context(|| format!("cannot create {}", d.display()))?;
    }

    let total = args.inputs.len();
    let (mut ok, mut failed, mut skipped) = (0, 0, 0);
    for (i, input) in args.inputs.iter().enumerate() {
        if interrupted() {
            break;
        }
        let tag = format!("[{}/{}]", i + 1, total);
        let name = input.file_name().map_or_else(|| input.display().to_string(), |n| n.to_string_lossy().into());
        eprintln!("{tag} {name}");
        let started = Instant::now();
        let out = srt_path(input, args);
        let log = args.log.clone().unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")).join("sonisub.log"));

        match process(input, &out, args, client.as_ref(), &tag) {
            Ok(Outcome::Done) => {
                ok += 1;
                log_line(&log, input, &format!("OK -> {} ({:.0}s)", out.display(), started.elapsed().as_secs_f64()));
            }
            Ok(Outcome::Skipped) => skipped += 1,
            Err(e) => {
                failed += 1;
                eprintln!("  error: {e:#}");
                log_line(&log, input, &format!("ERROR {e:#}"));
                eprintln!("  (logged to {})", log.display());
                if interrupted() {
                    break;
                }
                if let Some(api) = api_error(&e).filter(|a| a.is_fatal()) {
                    let rest = total - i - 1;
                    if rest > 0 {
                        eprintln!("stopping: {} — {rest} remaining file(s) not processed", api.error_type);
                    }
                    return Ok(ExitCode::from(2));
                }
            }
        }
    }
    if total > 1 {
        eprintln!("done: {ok} ok, {skipped} skipped, {failed} failed");
    }
    Ok(if interrupted() {
        ExitCode::from(130)
    } else if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn process(input: &Path, out: &Path, args: &RunArgs, client: Option<&Client>, tag: &str) -> Result<Outcome> {
    if !input.is_file() {
        bail!("file not found: {}", input.display());
    }
    if out.exists() && !args.force {
        eprintln!("  skip: {} exists (use --force to overwrite)", out.display());
        return Ok(Outcome::Skipped);
    }

    let transcript: Value = if is_json(input) {
        let text = std::fs::read_to_string(input)?;
        serde_json::from_str(&text).with_context(|| format!("{} is not a Soniox transcript", input.display()))?
    } else {
        transcribe(input, args, client.ok_or_else(soniox::missing_key)?, tag)?
    };

    if args.keep_json && !is_json(input) {
        let json_path = out.with_extension("soniox.json");
        std::fs::write(&json_path, serde_json::to_string_pretty(&transcript)?)?;
        eprintln!("  transcript: {}", json_path.display());
    }
    let (text, cues) = srt::build(&transcript, &args.seg, !args.no_diarization);
    if cues == 0 {
        eprintln!("  warning: no speech recognized, writing an empty .srt");
    }
    std::fs::write(out, text).with_context(|| format!("cannot write {}", out.display()))?;
    eprintln!("  {cues} cues -> {}", out.display());
    Ok(Outcome::Done)
}

fn transcribe(input: &Path, args: &RunArgs, client: &Client, tag: &str) -> Result<Value> {
    // Temp dir (and the audio inside) is removed when it goes out of scope, on success or error.
    let mut tmp = match &args.temp_dir {
        Some(d) => tempfile::Builder::new().prefix("sonisub-").tempdir_in(d),
        None => tempfile::Builder::new().prefix("sonisub-").tempdir(),
    }
    .context("cannot create temp dir")?;
    let stem = input.file_stem().map_or("audio".into(), |s| s.to_string_lossy().into_owned());
    let audio_path = tmp.path().join(format!("{stem}.flac"));

    let pb = bar(tag, "extract", "{prefix} {msg:9} [{bar:30.cyan/blue}] {percent:>3}%  {elapsed}");
    let audio = audio::extract(input, &audio_path, args.audio, &pb)?;
    pb.finish_with_message(format!("extracted {} ({})", fmt_dur(audio.duration_s), fmt_size(&audio.path)));
    if args.keep_audio {
        tmp.disable_cleanup(true);
        eprintln!("  audio kept: {}", audio.path.display());
    }

    let mut remote = RemoteGuard { client, file_id: None, transcription_id: None, warnings: Vec::new() };

    let pb = bar(tag, "upload", "{prefix} {msg:9} [{bar:30.cyan/blue}] {bytes}/{total_bytes}  {bytes_per_sec}");
    remote.file_id = Some(client.upload(&audio.path, &pb)?);
    pb.finish_with_message("uploaded");

    let cfg = soniox::transcription_config(
        &args.model,
        remote.file_id.as_deref().unwrap(),
        &args.lang,
        args.strict_lang,
        !args.no_diarization,
        args.context.as_deref(),
    );
    remote.transcription_id = Some(client.create(&cfg)?);
    let id = remote.transcription_id.clone().unwrap();

    let pb = ProgressBar::new_spinner().with_prefix(tag.to_string());
    pb.set_style(ProgressStyle::with_template("{prefix} {spinner} {msg}  {elapsed}").unwrap());
    pb.enable_steady_tick(Duration::from_millis(120));
    loop {
        if interrupted() {
            bail!("interrupted");
        }
        let st = client.status(&id)?;
        if st.status == "completed" {
            break;
        }
        pb.set_message(format!("transcribing ({})", st.status));
        sleep(Duration::from_secs_f64(args.poll.max(0.5)));
    }
    pb.set_message("downloading transcript");
    let transcript = client.transcript(&id)?;
    pb.finish_with_message("transcribed");

    remote.cleanup();
    Ok(transcript)
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

fn bar(tag: &str, msg: &str, template: &str) -> ProgressBar {
    let pb = ProgressBar::new(0).with_prefix(tag.to_string()).with_message(msg.to_string());
    pb.set_style(ProgressStyle::with_template(template).unwrap().progress_chars("=> "));
    pb
}

fn is_json(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))
}

fn srt_path(input: &Path, args: &RunArgs) -> PathBuf {
    if let Some(o) = &args.output {
        return o.clone();
    }
    // "clip.soniox.json" -> "clip.srt"
    let name = input.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let stem = name
        .strip_suffix(".soniox.json")
        .map(str::to_string)
        .unwrap_or_else(|| input.file_stem().map_or(name.clone(), |s| s.to_string_lossy().into_owned()));
    let dir = args.out_dir.clone().unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")).to_path_buf());
    dir.join(format!("{stem}.srt"))
}

fn log_line(log: &Path, input: &Path, msg: &str) {
    let ts = humantime::format_rfc3339_seconds(SystemTime::now());
    let line = format!("{ts}\t{}\t{msg}\n", input.display());
    let res = OpenOptions::new().create(true).append(true).open(log).and_then(|mut f| f.write_all(line.as_bytes()));
    if let Err(e) = res {
        eprintln!("  warning: cannot write log {}: {e}", log.display());
    }
}

fn fmt_dur(s: f64) -> String {
    let s = s.round() as u64;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn fmt_size(p: &Path) -> String {
    let b = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) as f64;
    if b > 1e6 { format!("{:.1} MB", b / 1e6) } else { format!("{:.0} KB", b / 1e3) }
}
