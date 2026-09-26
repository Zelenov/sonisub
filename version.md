# 0.1.0
## Added
- First release: `.srt` subtitles for video and audio files with Soniox speech-to-text, in one binary with no Python or ffmpeg needed.
- Subtitles are cut at sentence ends, then punctuation, then pauses, and never mix speakers. `-u` gives one cue per sentence, `-s` / `--speaker-names` label speakers.
- The Soniox transcript is kept next to the output as `.soniox.json` (`--no-json` to drop it) and reused, so a file is never paid for twice. It can also be re-cut with other layout options without calling Soniox.
- `-t premiere` writes a Premiere Pro transcript (`clip.premiere.json`, Adobe's import format) for Text panel > Transcript > Import Static Transcript: words with timing and confidence, speakers, languages, filler tags. Only missing outputs are written, so a new format is made from saved transcripts for free.
- `sonisub usage` shows what Soniox cost; each run prints its own audio length and cost.
- `sonisub languages` lists the language codes `-l` takes, fetched from Soniox; `sonisub::languages::fetch` does the same in code.
- `sonisub purge` lists and deletes leftovers in the Soniox account.
- Builds for Windows x64, Linux x64 and macOS (Apple Silicon).
