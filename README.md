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
sonisub clip.soniox.json -f --max-line 32 # re-cut subtitles from a saved transcript, no API call
sonisub purge                             # list leftovers in the Soniox account (--yes deletes)
```

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

`tests/fixtures/dialog.*` is real Soniox output: a two-voice Russian/English dialog made with Soniox TTS
(`dialog.mp4`), its transcript (`dialog.soniox.json`) and the expected subtitles (`dialog.srt`).
`scripts/make-fixture.sh` regenerates all three; review the `.srt` diff before committing it.

## Building for other platforms

```sh
cargo build --release                                   # this machine
cargo build --release --target x86_64-unknown-linux-gnu # needs the target + linker
cargo build --release --target aarch64-apple-darwin     # build on a Mac
```

TLS is rustls, so there is no OpenSSL dependency.
