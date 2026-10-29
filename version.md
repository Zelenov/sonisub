# 0.9.1
## Changed
- `-t ytt`: six or more lines at once each get a window of their own (four and more used to be drawn on top of each other); a line lasts until its last word shows; control characters XML forbids are left out, so YouTube does not refuse the file.
- `-t adr`: when a long cue is cut, each row keeps its own Out (a speaker's sound no longer stretched the first row over the second); a sound heard alone stays within 8 s; a long sound and the same speaker's speech during it no longer overlap.
- `--fps` and `--timeline-start` without `-t adr` are refused instead of ignored; the run says the rate and start an ADR list is written at (`ADR at 25 fps from 01:00:00:00`).
- README: the YTT format is not yet checked on YouTube itself (#21); the ADR columns to check in Resolve's import preview.
- captions 0.3.1.

# 0.9.0
## Added
- Subtitles from a CAPT file follow what the file says: they cut at its cut points (a ` / ` moved by hand moves the cut), end sentences where `sentence:` says, show `style:italic` in italic (SRT, WebVTT, ASS), mark words in another language (WebVTT), and put a subtitle where `pos:` says (top, middle, a point or a rectangle).
- `-t sbv` writes YouTube's SBV subtitles (`clip.sbv`), the same cues as the `.srt`.
- `-t lrc` writes Enhanced LRC (`clip.lrc`): a line per cue, a time before every word, for karaoke players and music apps.
- `-t transcript` (also `-t text`, `-t txt`) writes a plain text transcript (`clip.txt`): a paragraph per speaker turn with the name and the time, cut after a pause of over 5 s, at the first sentence end after a minute, or after two minutes without one; whole sentences, so cross-talk and other people's sounds, and sounds of no one, come after the sentence they fall in; the speaker's own sounds in their paragraph, voices as labels, in English or Russian (`--labels en|ru`). Sounds and voices come from a CAPT file (Soniox reports none); a file of sounds only works too.
- `-t ttml` writes TTML / IMSC 1.1 Text (`clip.ttml`): speakers, places (regions, a rectangle exactly), italic, languages.
- `-t adr` writes a DaVinci Resolve Fairlight ADR cue list (`clip.adr.csv`): rows joined per speaker while the pause is short, the speaker's sounds and voices as directions (a voice once a row) in English or Russian (`--labels en|ru`); `--fps` 23.976 to 60, `--timeline-start` (drop-frame refused).
- `-t ytt` writes YTT, YouTube's timed text (`clip.ytt`, srv3): a time for every word, so YouTube can show the words one by one, places and italic.
- Library: `captions::sbv`, `captions::lrc`, `captions::transcript::build`, `captions::labels` (sound and voice labels, en/ru), `captions::ttml::build`, `captions::adr` (`build`, `Options`, `Rate`, `Timecode`), `captions::ytt::build`, `Doc::sounds()`.

## Changed
- The message after a file counts the cues of the first subtitles written, not the most of any format.
- Subtitles are always made from the transcript as CAPT reads it, so `-t srt`, `-t srt,capt` and `render clip.capt` give the same cues; WebVTT now marks a sentence in another language than the file's (`<lang ru>…</lang>`). The Premiere transcript keeps Soniox's words as they are.
- Library: `captions::Word` has `marks` and `Default` (build words with `..Word::default()`); `job::Options::ass` is now `job::Options::outputs` (`job::Outputs`: `ass`, `adr`, `labels`); `job::write_outputs` takes the words to lay out, the words for Premiere, the CAPT document and `&job::Outputs` instead of `&ass::Options`; `job::Format` is `#[non_exhaustive]` (a `match` needs a `_` arm). captions 0.3.0.

# 0.8.0
## Added
- `-t capt` writes `clip.capt`: the whole transcript as small text (about 1/15 of the Soniox JSON) with every piece of a word and its time, words, sentences, cut points, speakers, languages and confidence. It is readable and editable by hand; the format is in [docs/capt.md](docs/capt.md).
- `sonisub render clip.capt -t srt,vtt,ass,premiere` makes the other formats from a CAPT file, the same subtitles as from the transcript, with the speaker names its header gives; `--speaker-names` replace them, also in a CAPT it writes. A broken file is reported line by line.
- Library: `captions::capt` (captions 0.2.0) reads, writes and makes CAPT files (`Doc::parse`, `Doc::write`, `Doc::from_spoken`, `Doc::to_words`); `soniox::spoken` and `soniox::capt` make them from a Soniox transcript.

## Changed
- Library: `job::Format` has `Capt` (a `match` on it needs the new arm), and `job::write_outputs` takes `Option<&captions::capt::Doc>` after the words: the CAPT file to write for `Format::Capt` (`soniox::capt(&transcript)`, or `Doc::from_words(&words, None)`), `None` when it is not asked for.

# 0.7.0
## Added
- `-t vtt` writes WebVTT subtitles (`clip.vtt`) and `-t ass` ASS subtitles (`clip.ass`), the same cues as the `.srt`. In `.ass` every speaker has a style of their own with its own colour (`Speaker 1`, or the `--speaker-names` name), and every line names its speaker, so a person is restyled or renamed in one place in Aegisub. `--karaoke` lights up each word as it is spoken.
- `sonisub render words.json -t srt,vtt,ass,premiere` writes subtitles from saved words (a JSON list of words with timing), without Soniox.
- The subtitle half of sonisub is its own crate, [`captions`](https://crates.io/crates/captions) 0.1.0: words with timing → cues → SRT, WebVTT, ASS, Premiere Pro transcript, without Soniox. `sonisub::captions` re-exports it.

## Changed
- Library: the layout rules, `Word` and the writers moved to `captions` (`srt::Layout`, `srt::Word` are now `captions::Layout`, `captions::Word`; `Word` has serde). `srt::*` and `premiere::*` keep their names as before; `soniox::words(transcript)` is the new home of `srt::words`.
- Library: `job::Format` has `Vtt` and `Ass` (a `match` on it needs the new arms), and `job::Options` a new field `ass` (speaker names, karaoke); callers that build `Options` without `..Options::default()` add `ass: Default::default()`. `job::write_outputs` writes words in any format.

# 0.6.1
## Added
- `sonisub.exe` has an icon: Explorer, the taskbar and the file properties show it instead of the generic one. The README shows it too. Only the Windows command line build changes; the library and other systems build as before.

# 0.6.0
## Added
- The batch bar now moves while a file is worked on, not only when one finishes: extracting, uploading and transcribing count toward it by audio length. It shows `eta ~4 min`, computed from the audio left and the smoothed speed of the run (hidden until a file is done or 30 s have passed), and the cost so far. A single file gets the same bar.
- Transcribing shows `~60% (est.)` beside Soniox's real status, as a bar that stays under 95% until the transcript is ready.
- `-q` / `--quiet`: no bars, only result lines and the summary (output that is not a terminal already did this).
- The end summary of several files shows how long the run took; after Ctrl+C it says what finished, what did not, and what was removed. The file Ctrl+C stopped is reported as `⏹ interrupted`, not as failed.

## Changed
- A single file no longer leaves the `extracted … / uploaded / transcribed` lines on screen; its bar and the result line replace them. The cost estimate for one file now uses your recent price from Soniox, like a batch does.

# 0.5.1
## Added
- Library: `srt::words(transcript)` reads a Soniox transcript as words — text with its punctuation, start and end in milliseconds, speaker, language, confidence — the same words subtitles are built from. For programs that lay words out themselves (word-by-word captions, their own cue rules). No caller has to change.

# 0.5.0
## Added
- Library: `job::Options::on_progress` takes a `progress::Callback` that is told where a job is: extracting, uploading, transcribing (an estimate, held at 95 % until Soniox is done), downloading, writing. Each report has the stage's fraction and one `overall` fraction for a single bar. `None` (the default) reports nothing.

## Changed
- Library: `job::Options` has a new field `on_progress`; callers that build it without `..Options::default()` add `on_progress: None`.
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
