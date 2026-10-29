# CAPT 1

CAPT is a transcript as a small text file: pieces of words with their times, words, cut points,
sentences, speakers, languages and confidence. sonisub writes it (`-t capt`) and can make the other
formats from it (`sonisub render clip.capt -t srt,vtt,ass,sbv,lrc,transcript,ttml,adr,premiere`). The `captions` crate reads and
writes it (`captions::capt`).

```
CAPT 1
id:691529b1-72ac-4e08-8fe1-4c3a84d29b28
lang:en
speaker:1 name:Anna
speaker:2

00:00:00.870 --> 00:00:03.330 id:1qc62f speaker:1
{0}H{18}i,{24} / {36}tod{48}ay{54} {54}we{60}'{72}re{78} {78}test{90}ing{96}
{0..18} conf:0.97
{18..24} conf:0.88

00:00:03.870 --> 00:00:04.470 id:spfa65 speaker:2 lang:ru
{0}Да,{18} / {30}deploy{48}ment{60}
{30..60} lang:en

00:00:04.500 --> 00:00:05.700 id:p2r6kc sound:phone
```

## Lines

A file is UTF-8 text with `\n` line ends (a reader also takes `\r\n` and a byte order mark).

1. `CAPT 1` — the first line. The number changes only when old readers would misread a file.
2. The header: lines of options, up to the first block line (a line whose first word starts with a
   digit, followed by ` --> `). A line with a `speaker` key describes that speaker: its `name` and any other key
   belong to the speaker. Every other header line holds file options; several may share a line.
3. Blocks, each followed by a blank line or the end of the file. A block is:
   - a **block line**: `HH:MM:SS.mmm --> HH:MM:SS.mmm` (start and end in the media), then the
     block's options;
   - a **text line** (speech only): the words, with marks and cut points;
   - **span lines** `{from..to} options`: options for part of the text.

A block is one source of sound over a stretch of time: a speaker talking, or a sound. Blocks are
in order of start; blocks of different sources may overlap (a phone rings while Anna talks, Anna
laughs in the middle of her own sentence).

## Options

The same on every line: `key:value`, separated by single spaces.

- A key is a lowercase word. A reader skips keys it does not know and a writer keeps them, so a
  new option needs no new version.
- A value with several parts separates them by commas: `alt:con,0.20`, `pos:0.5,0.08`.
- A part with a space, a comma or a quote, or an empty part, is quoted: `name:"Anna Lee"`,
  `alt:"C:\\temp",0.4`. Inside quotes `\"` is a quote and `\\` a backslash; nothing else is
  escaped. A bare part is taken as written.
- A time of the file is `HH:MM:SS.mmm`, everywhere. A time inside a block is hundredths of a
  second after the block's start: marks and spans.
- A share is a decimal from 0 to 1 (`conf:0.68`, `pos:0.5,0.08`). Writers keep `conf` to two
  decimals, so confidence read back from a file can differ from the recogniser's in the third.
- Languages are ISO 639 codes (`en`, `ru`). Sounds and voices are lowercase codes from one
  vocabulary (below); a code is not a word to show, each output writes it in its own language.
- An option equal to its default is left out: the blocks of an English file carry no `lang:en`.

| Option | Header | Block | Span | Means |
|---|---|---|---|---|
| `id` | ✓ | ✓ | | the file's id: made once, never changed; the seed of Premiere's speaker ids (made from Soniox, it is the transcription id). A block's id: six characters of Crockford base32 (`0-9 a-z` without `i l o u`), unique in the file |
| `title` | ✓ | | | the name of the work |
| `media`, `duration` | ✓ | | | the source file and its length, so a program can check the file belongs to the video |
| `speaker`, `name` | ✓ | | | `speaker:1 name:Anna` names a speaker; one without a name is "Speaker N". A writer lists every speaker it knows before the first block |
| `speaker` | | ✓ | | who speaks or makes the sound; none for a sound of no one |
| `sound` | | ✓ | | the block is a sound, not speech: `sound:phone`. It has no text line |
| `lang` | ✓ | ✓ | ✓ | the language of the words; a block or span overrides the header |
| `pos` | | ✓ | ✓ | where on screen (below) |
| `voice` | | ✓ | ✓ | how the words are said: `laughter`, `whisper`, `phone`… |
| `style` | | | ✓ | `italic`: a title, a foreign word, a stressed word |
| `sentence` | | | ✓ | `end` or `continues` after the last covered word, against its punctuation |
| `conf` | | | ✓ | confidence of each covered piece; 1 where no span says otherwise |
| `alt` | | | ✓ | another reading of the span and its confidence; may repeat |

The rule behind the levels: what can change in the middle of speech is a span option. Another
speaker or a sound is another source, so `speaker` and `sound` are block options only.

## The text line

Exactly one line. Words are separated by a space; every word is
`{start}piece{time}piece…{end}`, each number the moment at that place in the text, in hundredths
after the block start, so times are kept to 10 ms. A piece is a token of the recogniser; the pause
between words shows as `{24} {36}`. Piece starts rise strictly within a block (a writer moves a
tie by 10 ms), and a word with a span on its last piece ends at least 10 ms after that piece starts.

- ` / ` (a slash with a space on both sides) is a **cut point**: outputs cut here first. A writer
  puts one where a sentence can be cut cleanly: after `, ; : — …`, before a word that starts with
  `— – ( « “ "`, and at a pause of 0.4 s or more.
- A text line may have no marks (a translation, a hand-written line): its words get times spread
  over the block by length.
- A backslash makes the next character plain text. Writers escape exactly `\{`, `\}`, `\\`, `\ `
  for a space inside a word (`New\ York`), and `\/` for a slash that is a whole word (`and \/ or`;
  `24/7` needs nothing). A line break, tab or other control character in a word or an option value is
  written as a space; other spaces, such as a no-break space, are kept. A text line without marks
  that would read as a block line starts with a backslash. Every mark lies within the block's start
  and end.

## Sentences

A writer makes one block per sentence: a block ends at a sentence end, at a change of speaker and
at a silence over 2 s (a sentence's short tail after such a silence stays with it). A person may
cut or join blocks in an editor. Sentences come from the text: one ends after a word ending in `.`
`?` `!` (not an abbreviation such as `Dr.`, not `...`), and a block that ends without one goes on in
the next block of the same speaker. `sentence:end|continues` on a word overrides the punctuation.

A word without a language takes its block's, else the file's.

## Spans

`{from..to} options`, after the text line: a stretch of time, `from` included and `to` excluded.
`from` is a mark where a piece starts; `to` a later mark, where a piece starts or a word ends. The
span covers every piece that starts at `from` or later and before `to`: `{0..18}` is "H", not
"i," which starts at 18; `{0..18}` and `{18..24}` meet without overlapping; a whole word is
`{word start..word end}`. `lang`, `pos`, `voice`, `style` and `sentence` cover whole words only.
Spans are tied to marks, not to word numbers, so they survive edits to other words.

## Position

`pos` is `bottom` (the default), `top`, `middle`, a point `x,y` (the text is centred on it) or a
rectangle `x,y,w,h` (the top left corner, then width and height; the text wraps inside it and
starts at its top; it must lie inside the frame). Numbers are parts of the frame from its top left
corner. A format that cannot say a rectangle writes the nearest thing it has: WebVTT and ASS keep
its width and top edge, SRT the nearest of the nine `{\anN}` places.

## Sounds and voices

| Code | As a sound | As a voice |
|---|---|---|
| `music` | music | |
| `singing` | singing | singing |
| `laughter` | laughs | laughing |
| `crying` | cries | crying |
| `applause`, `cheering` | applause, cheering | |
| `cough`, `sigh`, `breathing` | coughs, sighs, breathing | |
| `phone` | phone ringing | on the phone |
| `door`, `knock`, `noise` | door, knocking, noise | |
| `whisper`, `shout` | | whispering, shouting |
| `inaudible` | inaudible | |

The codes follow AudioSet classes. An unknown code is shown as written. Silence needs no block: it
is the time no block covers.

## What a reader reports

Every problem, each with its line: a first line other than `CAPT 1`; a quote never closed; a time
not `HH:MM:SS.mmm`; a `pos` that is not one of its forms or a rectangle outside the frame; a mark
that is not a number, earlier than the mark before it, or after the block's end; two pieces that
start at the same mark; a text line with marks on some words only; a block that starts before the
block above it; a block line with no blank line before it; a `{` never closed or a `}` that closes nothing; a sound
block with text; a line after the text that is not a span; a span that does not start where a piece starts, ends at no later mark, or puts a
whole-word option on part of a word. Problems are listed in order of line.

## What the outputs do with it

`sonisub render clip.capt -t …` reads everything above. Every subtitle output cuts at the file's
cut points (a ` / ` moved by hand moves the cut), ends sentences as `sentence:` says, and never lets
one subtitle span two places. Speaker names go to ASS styles, transcript headings, TTML agents and ADR characters for any id, and to `-s` labels and
Premiere only for numbered speakers (`speaker:1`); `--speaker-names` replaces them, also in a CAPT
file it writes.

| Output | Italic | Language | Place | Voice | Sounds |
|---|---|---|---|---|---|
| SRT | `<i>` | | `{\anN}`, the nearest of nine | | |
| WebVTT | `<i>` | `<lang xx>` | cue settings; a rectangle's width and top | | |
| ASS | `{\i1}` | | alignment and margins; a rectangle's width and top | | |
| SBV | | | | | |
| LRC | | | | | |
| Transcript (`.txt`) | | | | `[whispering]` | the speaker's in their paragraph; another's or no one's after the sentence it falls in |
| TTML / IMSC | `<span>` | `<span xml:lang>` | a region; a rectangle exactly (it grows if its text needs more lines) | | |
| Fairlight ADR (`.adr.csv`) | | | | `[whispering]`, once a row | the speaker's own, in their cue |
| Premiere | | | | | |

Only the transcript and the ADR cue list show sounds and voices; `alt` is kept in the file but no output shows it.
