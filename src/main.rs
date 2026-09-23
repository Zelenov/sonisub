//! Command line: runs [`job::process`] over the input files, keeps the log, stops the batch on fatal errors.

mod cli;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Instant, SystemTime};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;

use cli::{ApiArgs, Cli, Command, RunArgs};
use sonisub::cancel::{interrupt, interrupted};
use sonisub::job::{self, Outcome};
use sonisub::soniox::{self, Client, api_error};

fn main() -> ExitCode {
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
        None => run(&cli.run),
    };
    result.unwrap_or_else(|e| {
        eprintln!("error: {e:#}");
        ExitCode::from(2)
    })
}

fn run(args: &RunArgs) -> Result<ExitCode> {
    if args.output.is_some() && args.inputs.len() > 1 {
        bail!("--output works with a single input; use --out-dir for several");
    }
    let needs_api = args.inputs.iter().any(|p| !job::is_transcript(p));
    let client = match &args.api.api_key {
        Some(k) if !k.trim().is_empty() => Some(Client::new(&args.api.api_url, k.trim())?),
        _ if needs_api => return Err(soniox::missing_key()),
        _ => None,
    };
    if let (Some(c), true) = (&client, needs_api) {
        c.check_auth()?;
    }
    if let Some(d) = &args.out_dir {
        std::fs::create_dir_all(d).with_context(|| format!("cannot create {}", d.display()))?;
    }

    let mut opts = args.job_options();
    let total = args.inputs.len();
    let (mut ok, mut failed, mut skipped, mut empty) = (0, 0, 0, 0);
    for (i, input) in args.inputs.iter().enumerate() {
        if interrupted() {
            break;
        }
        opts.prefix = format!("[{}/{}]", i + 1, total);
        let name = input.file_name().map_or_else(|| input.display().to_string(), |n| n.to_string_lossy().into());
        eprintln!("{} {name}", opts.prefix);
        let output = args.output.clone().unwrap_or_else(|| job::default_output(input, args.out_dir.as_deref()));
        let log = args.log.clone().unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")).join("sonisub.log"));
        let started = Instant::now();

        match job::process(input, &output, &opts, client.as_ref()) {
            Ok(Outcome::Written { srt, cues, json, cached }) => {
                ok += 1;
                match (json, cached) {
                    (Some(j), true) => eprintln!("  using saved transcript {} (--force to transcribe again)", j.display()),
                    (Some(j), false) => eprintln!("  transcript: {}", j.display()),
                    _ => {}
                }
                eprintln!("  {cues} cues -> {}", srt.display());
                log_line(&log, input, &format!("OK -> {} ({:.0}s)", srt.display(), started.elapsed().as_secs_f64()));
            }
            Ok(Outcome::Skipped { srt }) => {
                skipped += 1;
                eprintln!("  skip: {} exists (use --force to overwrite)", srt.display());
            }
            Ok(Outcome::NoAudio { reason }) => {
                empty += 1;
                eprintln!("  no audio: {reason}, nothing to do");
                log_line(&log, input, &format!("NO AUDIO ({reason})"));
            }
            Ok(Outcome::NoSpeech { marker, cached }) => {
                empty += 1;
                let note = match (&marker, cached) {
                    (Some(m), true) => format!("checked before, see {} (--force to try again)", m.display()),
                    (Some(m), false) => format!("marker saved: {} - later runs skip this file", m.display()),
                    (None, _) => "audio is silent, nothing uploaded".to_string(),
                };
                eprintln!("  no speech, no .srt written: {note}");
                log_line(&log, input, &format!("NO SPEECH ({note})"));
            }
            Err(e) => {
                failed += 1;
                eprintln!("  error: {e:#}");
                log_line(&log, input, &format!("ERROR {e:#}"));
                eprintln!("  (logged to {})", log.display());
                if interrupted() {
                    break;
                }
                // Balance / budget / auth: every remaining file would fail the same way.
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
        eprintln!("done: {ok} ok, {empty} without speech, {skipped} skipped, {failed} failed");
    }
    Ok(if interrupted() {
        ExitCode::from(130)
    } else if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
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
