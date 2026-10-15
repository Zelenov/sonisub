# NEXT
## Added
- Library: `job::Options::on_progress` takes a `progress::Callback` that is told where a job is: extracting, uploading, transcribing (an estimate, held at 95 % until Soniox is done), downloading, writing. Each report has the stage's fraction and one `overall` fraction for a single bar. `None` (the default) reports nothing; callers using `..Options::default()` need no change.

## Changed
- Cancelling during an upload now ends the job with `cancel::Cancelled`, like cancelling at any other stage, instead of a "request failed" error.

# 0.4.0
## Added
- Library: `premiere::to_srt(source, &layout)` reads a Premiere Pro transcript back as SRT text and the number of cues, laid out by the same `srt::Layout` rules as an SRT made from the same speech. Speaker names come from the file (`layout.speaker_labels` turns them on; `layout.speaker_names` is ignored). `None` for JSON that is not a Premiere transcript or has no words. Tested on transcripts sonisub wrote; others are read on a best-effort basis. No caller has to change.

# 0.3.0
## Added
- `-t premiere` writes a Premiere Pro transcript (`clip.premiere.json`, Adobe's import format) for Text panel > Transcript > Import Static Transcript: words with timing and confidence, speakers, languages, filler tags. `-t srt,premiere` writes both. Only missing outputs are written, so a new format is made from saved transcripts for free.
- `premiere-plugin/`: a Premiere Pro panel to import transcripts for many clips at once.
- `sonisub languages` lists the language codes `-l` takes, fetched from Soniox; `sonisub::languages::fetch` does the same in code.

## Changed
- The Soniox transcript is kept next to the output as `.soniox.json` by default (`--no-json` to drop it; `-j` / `--keep-json` is gone).
- `-o` takes any output extension (`clip.premiere.json` means base `clip`).
- Library: `job::Options` has `formats`; `Outcome::Written` / `Skipped` list `files` instead of one `srt` path.

# 0.2.0
## Added
- Usable as a library from other programs (frename uses it): a `cli` feature (on by default) holds the command line; build with `default-features = false` for the library alone.
- Per-job cancel (`cancel::CancelToken`) for extraction, uploads, retries and polling; what was created on Soniox is still deleted.

## Changed
- Requests to Soniox time out (60 s; uploads by size) instead of hanging on a dead connection, and a create that timed out is not sent twice.
- ffmpeg and ffprobe no longer open a console window on Windows.
- Soniox files that could not be deleted are reported through the log (the command line still prints the warning).

# 0.1.0
## Added
- First release: `.srt` subtitles for video and audio files with Soniox speech-to-text, in one binary with no Python or ffmpeg needed.
- Subtitles are cut at sentence ends, then punctuation, then pauses, and never mix speakers. `-u` gives one cue per sentence, `-s` / `--speaker-names` label speakers.
- A saved `.soniox.json` transcript is reused, so a file is never paid for twice. It can also be re-cut with other layout options without calling Soniox.
- `sonisub usage` shows what Soniox cost; each run prints its own audio length and cost.
- `sonisub purge` lists and deletes leftovers in the Soniox account.
- Builds for Windows x64, Linux x64 and macOS (Apple Silicon).
