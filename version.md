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
