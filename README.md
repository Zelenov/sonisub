# sonisub

<img src="https://raw.githubusercontent.com/Zelenov/sonisub/main/docs/icon/sonisub-256.png" align="left" width="128" height="128" alt="sonisub icon">

Generate subtitles (`.srt`, WebVTT, ASS, TTML, YouTube's SBV and YTT, LRC), plain text transcripts, Fairlight ADR cue lists and
Premiere Pro transcripts for video and audio files with
[Soniox](https://soniox.com) speech-to-text.
One self-contained binary: no Python, no ffmpeg required.

<br clear="left">

```
sonisub clip.MP4
```

![sonisub in a terminal](docs/sonisub-screenshot.png)

## Download

Get the build for your system from [Releases](https://github.com/Zelenov/sonisub/releases/latest):

| System | File |
|---|---|
| Windows x64 | `sonisub-windows-x64-vX.Y.Z.zip` |
| Linux x64 | `sonisub-linux-x64-vX.Y.Z.tar.gz` |
| macOS (Apple Silicon) | `sonisub-macos-arm64-vX.Y.Z.tar.gz` |

Unpack it and put `sonisub` / `sonisub.exe` anywhere on `PATH`. On macOS, if the binary is blocked as
downloaded from the internet, run `xattr -d com.apple.quarantine sonisub` once.

## How it works

Pipeline per file:

1. **Extract** the audio track (MP4/MOV/M4A/MKV with AAC, plus MP3, WAV, FLAC, OGG...) with the built-in
   decoder, downmix to mono, resample to 16 kHz and encode FLAC into a temp dir.
   If the built-in decoder can't handle the file and `ffmpeg` is on `PATH`, it is used instead.
2. **Upload** to Soniox, **transcribe** (`stt-async-v3`, speaker diarization, language identification).
3. **Build subtitles**: cues break on sentence ends, pauses, speaker changes and at commas when a cue is full;
   lines are balanced; nothing exceeds `--max-line` × `--max-lines`.
4. **Clean up**: the temp audio and the Soniox file and transcription are deleted — also on errors and Ctrl+C.
   The transcript itself is kept next to the output as `clip.soniox.json` (`--no-json` to drop it): other
   formats and layouts are made from it later without paying again.

## Build from source

`cargo install --path .`, or copy `target/release/sonisub(.exe)` anywhere on `PATH`.

## API key

Set `SONIOX_API_KEY` (or pass `--api-key`). The key is checked before any file is processed.

## Examples

```sh
sonisub .                                 # every media file in this folder
sonisub E:/Kenya -r -F "*.MP4" -n         # subfolders too, only .MP4, show the plan and cost, change nothing
sonisub *.MP4                             # .srt next to every video, existing ones skipped
sonisub clip.MP4 -f -o subs/clip.srt      # overwrite, explicit output
sonisub *.MOV -d subs -l en --strict-lang # English only, into ./subs
sonisub clip.MP4 -c "Nairobi, Maasai, UAT, QA"   # context terms improve recognition
sonisub clip.MP4 -t srt,premiere          # also clip.premiere.json for Premiere Pro's Text panel
sonisub . -t premiere                     # only the Premiere transcript; saved .soniox.json files are reused
sonisub clip.MP4 -t srt,vtt,ass           # also clip.vtt (WebVTT) and clip.ass (a style per speaker)
sonisub clip.MP4 -t ass --karaoke         # each word lights up as it is spoken
sonisub clip.MP4 -t srt,sbv               # also clip.sbv for YouTube Studio
sonisub clip.MP4 -t lrc                   # clip.lrc for karaoke players and music apps
sonisub clip.MP4 -t srt,transcript        # also clip.txt, a plain text transcript to read and search
sonisub render clip.capt -t adr --fps 29.97  # a DaVinci Resolve Fairlight ADR cue list
sonisub clip.MP4 -t srt,capt              # also clip.capt, the whole transcript as small text
sonisub render clip.capt -t ytt           # YouTube: word by word, like its own captions
sonisub render clip.capt -t vtt,premiere  # other formats from a CAPT file or saved words, no Soniox
sonisub clip.MP4 --no-json                # don't keep clip.soniox.json
sonisub clip.MP4 -u                       # no length limit: one cue per sentence
sonisub clip.MP4 -s                       # "Speaker 1: ..." when the speaker changes
sonisub clip.MP4 --speakers-every         # "Speaker 1: ..." on every subtitle
sonisub clip.MP4 --speaker-names Eugene,Sasha
sonisub clip.soniox.json -f --max-line 32 # re-cut subtitles from a saved transcript, no API call
sonisub purge                             # list leftovers in the Soniox account (--yes deletes)
sonisub usage                             # what Soniox cost: last 30 days, today, price per hour
sonisub languages                         # language codes -l takes, as Soniox reports them
```

## Folders

A folder means all media files in it (mp4, mov, m4v, mkv, webm, m4a, mp3, wav, flac, ogg, opus, aac), sorted
by name; `-r` adds subfolders, `-F` filters names (`*.MP4`, `DJI_2025080?_*`, or plain text contained in the
name; repeatable, case-insensitive). Hidden and `._` files are ignored. With `-d` the subfolder structure is
kept under the output folder.

Before anything is uploaded, sonisub reads the headers and shows the plan:

```
165 file(s):
   157  to transcribe: 4:03:41 audio, ≈ $0.41 (at $0.10/h)
     4  no audio track
     4  .srt exists, skipped (--force to redo)
```

`-n` / `--dry-run` also lists every file and stops there. While running, one bar tracks the whole batch by
audio length (files sent, minutes done, cost so far, `eta ~4 min`), with the current file's stages under it;
Soniox gives no percentage, so transcribing shows `~60% (est.)` below 95% until done. Each finished file leaves one line:
`✓` subtitled, `·` skipped, `∅` no speech, `–` no audio, `✗` error; several files end with a summary. After
Ctrl+C the summary says what finished, what did not, and what was removed (the temp audio and the Soniox uploads).

`-q` / `--quiet`, or output that is not a terminal (a pipe, CI), draws no bars: only the result lines and the summary.

## Premiere Pro transcript

`-t premiere` writes `clip.premiere.json` in Adobe's transcript format
([schema](https://github.com/AdobeDocs/uxp-premiere-pro-samples/blob/main/sample-panels/premiere-api/assets/transcript_format_spec.json)):
every word with its time and confidence, sentence ends, speakers, language. In Premiere Pro (25.6+) it
replaces Premiere's own transcription for that clip — text-based editing, captions and search then work on the
Soniox text:

1. Open the clip in the Source Monitor.
2. Window > Text > Transcript, `…` menu > Import > Import Static Transcript, pick `clip.premiere.json`.

Premiere does not pick the file up by itself when the media is imported: transcripts live inside the `.prproj`.
To import many clips at once use the panel in [`premiere-plugin/`](premiere-plugin/README.md): select clips
or bins, press **Import transcripts**.

- A segment (paragraph in the Transcript panel) is one speaker and one language; silence longer than `--gap`
  starts a new one. Speakers are "Speaker N" or `--speaker-names`.
- Languages are mapped to Premiere's codes (`en` → `en-us`, `ru` → `ru-ru`, `pt` → `pt-br`, `zh` → `cmn-hans`...);
  a language Premiere doesn't know becomes `??-??`. Without language identification the first `-l` hint is used.
- Hesitations like "um", "uh", "эм" are tagged as fillers, so Premiere can remove them.
- Only missing outputs are written: `-t srt,premiere` on a folder that already has `.srt` and `.soniox.json`
  files adds the Premiere transcripts for free and leaves hand-edited `.srt` alone.

## WebVTT and ASS

`-t vtt` and `-t ass` write the same cues as the `.srt`. In `clip.ass` every speaker has a style of their own
(`Speaker 1`, or the name from `--speaker-names`) with its own text colour (white, yellow, cyan, green, pink,
orange, violet, red, then repeating), black outline, bottom centre. Every line names its speaker, so in Aegisub
you restyle or rename a person in one place. Without diarization all lines use the `Default` style. `--karaoke` adds word-by-word
highlight (`\k`).

## Other formats

`-t sbv` writes YouTube's SBV (`clip.sbv`), the same cues as the `.srt`, for YouTube Studio's subtitle upload.

`-t lrc` writes Enhanced LRC (`clip.lrc`): a line per cue with a time before every word, for karaoke players and music apps.

`-t transcript` (or `-t text`, `-t txt`) writes a plain text transcript (`clip.txt`): a paragraph per speaker turn, headed by
the name and the time, a new paragraph after a pause of over 5 s, at the first sentence end after a minute, or after two minutes
without one; sounds and voices a CAPT file marks as labels in
English or Russian (`--labels en|ru`; by default the transcript's language).

`-t ttml` writes TTML / IMSC 1.1 Text (`clip.ttml`), what broadcasters and streaming services ask for: speakers as
agents, places as regions (a rectangle exactly), italic and languages. For a delivery spec that counts characters a
line, add `-w --max-line 42`.

`-t adr` writes a DaVinci Resolve Fairlight ADR cue list (`clip.adr.csv`; Fairlight page > ADR > List > Import Cue
List): a row per breath of speech, joined per speaker, with the speaker's own sounds and voices as directions
(`--labels en|ru`); `--fps 23.976|24|25|29.97|30|50|59.94|60` and `--timeline-start 01:00:00:00` (where the clip
starts on the timeline, non-drop frame) match the timeline.

`-t ytt` writes YTT, YouTube's own timed text (`clip.ytt`, also called srv3): a time for every word, so YouTube can
show an uploaded subtitle word by word like its own captions, plus places and italic. Upload it in YouTube Studio as
a subtitle file.

`sonisub render` makes the same files without Soniox: from a CAPT file (`-t capt` writes `clip.capt`, the whole
transcript as small text you can read and edit, see [docs/capt.md](docs/capt.md)), or from words saved as JSON
(`[{"text": "Hello,", "start": 120, "end": 480, "speaker": "1"}, ...]`, times in milliseconds). It takes `-t`, `-o`,
`-f` and the layout options.

## How subtitles are cut

- A cue never mixes speakers and never runs past the end of a sentence (`Dr.`, `e.g.`, `3.5`, `...` are not ends).
- A sentence that doesn't fit (`--max-line` × `--max-lines`, `--max-duration`) is cut at punctuation
  (`, ; : — ... (`) first. Only a piece that still doesn't fit is cut at a pause, and only if that fails too,
  between words. Lines inside a cue break by the same preference.
- Silence longer than `--gap` always ends a cue; a sentence's short tail after such a pause stays with it.
- A cue is one line of up to `--max-line` × `--max-lines` characters (100 by default); `-w` / `--wrap`
  breaks it into up to `--max-lines` lines of `--max-line` characters instead.
- `-u` / `--max-line 0 --max-duration 0`: no limits, every cue is one whole sentence on one line.

## Nothing to transcribe

- Empty file, empty or foreign JSON: an error.
- No audio track, or a track without samples: reported as "no audio", not an error, nothing uploaded.
- Digital silence: detected locally, nothing uploaded, no `.srt`.
- Soniox finds no words: no `.srt`; the empty transcript is kept as `clip.soniox.json` — a marker.
- Any `clip.soniox.json` next to the output is used instead of calling Soniox again (`--force` re-transcribes),
  so a file is never paid for twice, with or without speech.

## Spending

After a run that sent audio to Soniox: `this run: 3:31 audio, $0.0057`, the exact cost from the Soniox usage
logs (each run tags its requests with a `client_reference_id`), or an estimate at your recent price if the
logs are late. `sonisub usage [--days N]` sums up to 91 days per model. Soniox has no API for the remaining
balance; see the console for that.

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
| `src/job.rs` | **one file**: media → audio → Soniox → every output, cleanup. `job::process(input, output, &Options, client)` |
| `src/audio.rs` | audio track → 16 kHz mono FLAC (built-in decoder or ffmpeg) |
| `src/soniox.rs` | Soniox REST client, error types, remote cleanup guard; `soniox::words` reads a transcript as words |
| `src/languages.rs` | language codes `-l` takes, from Soniox (`languages::fetch(client)`) |
| `captions/` | the [`captions`](captions/README.md) crate: words → cues → every subtitle format, and the CAPT format; no Soniox |
| `captions-wasm/` | `captions` in the browser (WebAssembly, an npm package); see below |
| `src/srt.rs`, `src/premiere.rs` | a Soniox transcript straight to SRT / Premiere Pro transcript, through `captions` |
| `src/cancel.rs` | Ctrl+C flag and per-job `CancelToken` |
| `src/main.rs`, `src/cli.rs`, `src/ui.rs` | command line only: arguments, the list of files, log, stopping on fatal errors, the batch bar and its ETA |

### As a library

```toml
sonisub = { version = "0.9", default-features = false }
```

Subtitle formats live in the [`captions`](captions/README.md) crate, re-exported as `sonisub::captions`:
`soniox::words(&transcript)` gives the words, `captions::vtt::build(&words, &layout)` and the others lay them out.
`soniox::capt(&transcript)` makes a CAPT file (`captions::capt::Doc`; `write`, `parse`, `to_words`).

`premiere::to_srt(&json, &layout)` turns a `clip.premiere.json` back into SRT text and the cue count
(`None` when it is not a Premiere transcript). Tested on transcripts sonisub wrote; transcripts from
other tools are read on a best-effort basis.

`default-features = false` drops the `cli` feature (the binary, `clap`, `ctrlc`). Per file:
`job::process(input, output, &Options { cancel: token.clone(), .. }, Some(&Client::new(soniox::DEFAULT_BASE, key)?.with_cancel(token)))`.
Cancelling the token stops extraction, upload, retries and polling within a fraction of a second, with the
error `cancel::Cancelled`; what was created on Soniox is still deleted. Requests other than uploads time out
after 60 s and are retried. Failed remote deletes are logged with the `log` crate and counted by
`Client::failed_deletes`. `reqwest::blocking::Client` must not be created or dropped on an async runtime
thread, so create the `Client` where the job runs. On Windows, ffmpeg and ffprobe run without a console window.

To show progress without a console, set `Options::on_progress` to `progress::Callback::new(|p| ..)`. Each
`Progress` names the stage (extracting, uploading, transcribing, downloading, writing), how far it is, and
`overall` for one bar (extract 15 %, upload 15 %, transcribe 60 %, download 5 %, write 5 %). While Soniox
transcribes, the fraction is an estimate from the audio length that stops at 95 % until Soniox is done; nothing
goes backwards. `fraction: None`: the media's length is not known yet, show a busy bar. `remaining: None` while
transcribing: the estimate has run out, say "finishing". The callback runs on the job's threads: send the value
over a channel to your UI.

### In the browser: captions-wasm

`captions-wasm` is the `captions` crate compiled to WebAssembly, for web pages and Node: an ES module with
TypeScript types that turns a CAPT file into every format above, byte for byte what `sonisub render` writes, and
gives a preview what it needs. No network, no Soniox. Its version is the `captions` version. Every release carries
it as `captions-wasm-<version>.tgz`; install it from the release:

```sh
npm i https://github.com/Zelenov/sonisub/releases/download/v0.9.1/captions-wasm-0.3.1.tgz
```

```js
import init, { parse, formats, render, layout } from "captions-wasm";
await init();
const { text, fileName } = render(capt, "srt", {}); // the .srt sonisub writes, and "clip.srt"
```

`parse(capt)` sums a file up or lists its problems by line; `formats()` lists every format with the options
`captions` takes for it (types, defaults, limits); `layout(capt, format, options)` gives the cues with every word
and piece (syllable) timed and placed, the ADR rows, or the Premiere transcript; `mergeSpeakers(capt, { 3: 1 })`
gives the file with speaker 3 made speaker 1. See
[captions-wasm/README.md](captions-wasm/README.md). To build it: `wasm-pack build --release --target web` in
`captions-wasm/` (needs [wasm-pack](https://github.com/drager/wasm-pack) and the `wasm32-unknown-unknown` target).

## Tests

```sh
cargo test                            # offline: subtitles, audio, the whole job against a mock Soniox
cargo test --test live -- --ignored   # real Soniox round trip (needs SONIOX_API_KEY)
cd captions-wasm && wasm-pack test --node   # captions-wasm gives what sonisub render writes
```

`tests/fixtures` is real Soniox output, made with Soniox TTS by `scripts/make-fixture.sh`:
`dialog` (English voices plus a Russian and a Spanish line), `punctuation` (dense punctuation, abbreviations,
numbers, a sentence without commas), `nospeech` (noise and music: Soniox's empty transcript), plus `noaudio.mp4`
and `silence.m4a`. Each has the video, the transcript and expected subtitles for the default, `-u` and `-s`
layouts, and `dialog`/`punctuation` the expected Premiere transcript. `dialog.capt` and the hand-written
`marks.capt` have every other output; `BLESS=1 cargo test --test outputs` rewrites those. Review the diff before
committing regenerated fixtures. `captions-wasm/tests/expected` is what `sonisub render` writes for the cases in
`captions-wasm/tests/cases`; `BLESS=1 cargo test --test wasm_render` rewrites them.

## Building for other platforms

```sh
cargo build --release                                   # this machine
cargo build --release --target x86_64-unknown-linux-gnu # needs the target + linker
cargo build --release --target aarch64-apple-darwin     # build on a Mac
```

TLS is rustls, so there is no OpenSSL dependency.

## Releases

`.github/workflows/release.yml` runs when `version.md` changes. The first line of `version.md` (`# 0.1.0`)
is the version and must match `version` in `Cargo.toml`; the section under it becomes the release notes.
The workflow runs the tests and builds Windows x64, Linux x64 and macOS arm64, and the browser package
`captions-wasm-<captions version>.tgz`.

- Push to `main`: a published release `vX.Y.Z` (skipped if that release already exists).
- Push to any other branch: a draft `vX.Y.Z-<branch>`, rebuilt on every push.
- From `main`, the crates are also published to crates.io, [`captions`](https://crates.io/crates/captions) first,
  then [`sonisub`](https://crates.io/crates/sonisub) (needs the `CARGO_REGISTRY_TOKEN` secret); a version already
  there is skipped. A change to `captions/` needs a new `version` in `captions/Cargo.toml`: the release fails if the
  published `captions` differs from the repository's.

## License

MIT, see [LICENSE](LICENSE).

To release: bump `version` in `Cargo.toml`, add a new `# X.Y.Z` section at the top of `version.md`, commit, push.
