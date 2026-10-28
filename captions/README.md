# captions

Words with timing → subtitle cues → SRT, WebVTT, ASS, Premiere Pro transcript.

No network, no ffmpeg, no speech provider: give it words, it lays them out. It is the subtitle half of
[sonisub](https://github.com/Zelenov/sonisub) (Soniox speech-to-text), so it writes exactly what the
`sonisub` command line writes.

```toml
captions = "0.2"
```

```rust
use captions::{Layout, Word, ass, srt, vtt};

let words: Vec<Word> = serde_json::from_str(r#"[
    {"text": "Hello,", "start": 120, "end": 480, "speaker": "1"},
    {"text": "world.", "start": 520, "end": 900, "speaker": "1"}
]"#)?;
let layout = Layout::default();
let (srt, cues) = srt::build(&words, &layout);
let (vtt, _) = vtt::build(&words, &layout);
let (ass, _) = ass::build(&words, &layout, &ass::Options { karaoke: true, ..Default::default() });
```

- `Word`: text with its punctuation, start and end in milliseconds, optional speaker, language and
  confidence (serde).
- `Layout`: line length, lines per cue, cue duration (8 s by default), pause and speaker cuts, speaker labels.
  A cue never mixes speakers and never runs past a sentence end; a long sentence is cut at punctuation
  first, at pauses next, between words last. `cues::cues` gives the cues themselves.
- `srt`, `vtt`: the same cues in each format.
- `ass`: a style per speaker (`Speaker 1`, or the name from `ass::Options::names`, e.g. `{"1": "Anna"}`),
  each with its own text colour; every line names its speaker in `Name`. Words without a speaker use
  `Default`. `karaoke` lights up each word as it is spoken (`\k`).
- `premiere::build`: a Premiere Pro transcript (Import Static Transcript); `premiere::to_srt` reads one back.
- `capt`: the CAPT transcript format ([spec](https://github.com/Zelenov/sonisub/blob/main/docs/capt.md)), every piece
  of a word with its time, words, cut points, sentences, speakers, languages and confidence as small text.
  `capt::Doc::parse` reads a file (every problem with its line), `write` writes one, `from_spoken` makes one from a
  recogniser's words with their pieces, `from_words` from plain `Word`s, and `words` gives the `Word`s back for the
  writers above.

MIT licensed.
