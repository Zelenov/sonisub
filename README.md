# sonisub

Generate `.srt` subtitles for video and audio files with [Soniox](https://soniox.com) speech-to-text.
One self-contained binary: no Python, no ffmpeg required.

```
sonisub clip.MP4
```

Pipeline per file:

1. **Extract** the audio track (MP4/MOV/M4A/MKV with AAC, plus MP3, WAV, FLAC, OGG...) with the built-in
   decoder, downmix to mono, resample to 16 kHz and encode FLAC into a temp dir.
   If the built-in decoder can't handle the file and `ffmpeg` is on `PATH`, it is used instead.
2. **Upload** to Soniox, **transcribe** (`stt-async-v3`, speaker diarization, language identification).
3. **Build subtitles**: cues break on sentence ends, pauses, speaker changes and at commas when a cue is full;
   lines are balanced; nothing exceeds `--max-line` × `--max-lines`.
4. **Clean up**: the temp audio and the Soniox file and transcription are deleted — also on errors and Ctrl+C.

## Install

```
cargo install --path .
```

or copy `target/release/sonisub(.exe)` anywhere on `PATH`.

## API key

Set `SONIOX_API_KEY` (or pass `--api-key`). The key is checked before any file is processed.

## Examples

```sh
sonisub *.MP4                             # .srt next to every video, existing ones skipped
sonisub clip.MP4 -f -o subs/clip.srt      # overwrite, explicit output
sonisub *.MOV -d subs -l en --strict-lang # English only, into ./subs
sonisub clip.MP4 -c "Nairobi, Maasai, UAT, QA"   # context terms improve recognition
sonisub clip.MP4 -j                       # also keep clip.soniox.json
sonisub clip.MP4 -u                       # no length limit: one cue per sentence
sonisub clip.MP4 -s                       # "Speaker 1: ..." when the speaker changes
sonisub clip.MP4 --speaker-names Eugene,Sasha
sonisub clip.soniox.json -f --max-line 32 # re-cut subtitles from a saved transcript, no API call
sonisub purge                             # list leftovers in the Soniox account (--yes deletes)
```

## How subtitles are cut

- A cue never mixes speakers and never runs past the end of a sentence (`Dr.`, `e.g.`, `3.5`, `...` are not ends).
- A sentence that doesn't fit (`--max-line` × `--max-lines`, `--max-duration`) is cut at punctuation
  (`, ; : — ... (`) first. Only a piece that still doesn't fit is cut at a pause, and only if that fails too,
  between words. Lines inside a cue break by the same preference.
- Silence longer than `--gap` always ends a cue; a sentence's short tail after such a pause stays with it.
- `-u` / `--max-line 0 --max-duration 0`: no limits, every cue is one whole sentence on one line.

## Nothing to transcribe

- Empty file, empty or foreign JSON: an error.
- No audio track, or a track without samples: reported as "no audio", not an error, nothing uploaded.
- Digital silence: detected locally, nothing uploaded, no `.srt`.
- Soniox finds no words: no `.srt`; the empty transcript is kept as `clip.soniox.json` — a marker.
- Any `clip.soniox.json` next to the output is used instead of calling Soniox again (`--force` re-transcribes),
  so a file is never paid for twice, with or without speech.

## Errors

Every processed file gets a line in `sonisub.log` next to it (`--log` to change): `OK -> out.srt` or `ERROR ...`
with the Soniox `error_type`, message and `request_id`.

Balance, budget and auth problems (`organization_balance_exhausted`, `*_monthly_budget_exhausted`,
`unauthenticated`, HTTP 401/402/403) stop the batch — the remaining files would fail the same way.
Network errors, 429 and 5xx are retried with backoff.

Exit codes: `0` ok, `1` some files failed, `2` fatal (key/balance/usage), `130` interrupted.

## Code layout

| Module | Does |
|---|---|
| `src/job.rs` | **one file**: media → audio → Soniox → `.srt`, cleanup. `job::process(input, output, &Options, client)` |
| `src/audio.rs` | audio track → 16 kHz mono FLAC (built-in decoder or ffmpeg) |
| `src/soniox.rs` | Soniox REST client, error types, remote cleanup guard |
| `src/srt.rs` | transcript tokens → cues → SRT text (`srt::Layout`) |
| `src/main.rs`, `src/cli.rs` | command line only: arguments, the list of files, log, stopping on fatal errors |

## Tests

```sh
cargo test                            # offline: subtitles, audio, the whole job against a mock Soniox
cargo test --test live -- --ignored   # real Soniox round trip (needs SONIOX_API_KEY)
```

`tests/fixtures` is real Soniox output, made with Soniox TTS by `scripts/make-fixture.sh`:
`dialog` (English voices plus a Russian and a Spanish line), `punctuation` (dense punctuation, abbreviations,
numbers, a sentence without commas), `nospeech` (noise and music: Soniox's empty transcript), plus `noaudio.mp4`
and `silence.m4a`. Each has the video, the transcript and expected subtitles for the default, `-u` and `-s`
layouts. Review the `.srt` diff before committing regenerated fixtures.

## Building for other platforms

```sh
cargo build --release                                   # this machine
cargo build --release --target x86_64-unknown-linux-gnu # needs the target + linker
cargo build --release --target aarch64-apple-darwin     # build on a Mac
```

TLS is rustls, so there is no OpenSSL dependency.
