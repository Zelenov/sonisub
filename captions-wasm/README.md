# captions-wasm

[captions](https://crates.io/crates/captions) in the browser: a CAPT transcript → SRT, WebVTT, ASS,
Premiere Pro, SBV, LRC, a text transcript, TTML, a Fairlight ADR cue list, YTT, and what a preview of
each needs. The same Rust code as the `sonisub` command line, compiled to WebAssembly: `render` gives
byte for byte what `sonisub render clip.capt -t <format>` writes. No network, no speech provider.

An ES module with TypeScript types (`wasm-pack --target web`). Its version is the `captions` version
it is built from. Each [sonisub release](https://github.com/Zelenov/sonisub/releases) carries it as
`captions-wasm-<version>.tgz`:

```sh
npm i https://github.com/Zelenov/sonisub/releases/download/v0.9.1/captions-wasm-0.3.1.tgz
```

```js
import init, { parse, formats, render, layout, mergeSpeakers } from "captions-wasm";

await init(); // in Node: await init({ module_or_path: fs.readFileSync(wasmPath) })
const capt = await (await fetch("/clip.capt")).text();

parse(capt);                        // { speakers, languages, duration, words, sounds, … } or throws
formats();                          // [{ name: "srt", extension, contentType, layout, options: [...] }, …]
render(capt, "srt", {});            // { text, fileName: "clip.srt", contentType, count }
render(capt, "ass", { karaoke: true, speakerLabels: true });
render(capt, "srt", { speakerLabels: true, speakerLabelsEvery: true }); // "Speaker 1: " on every cue
layout(capt, "ass", { karaoke: true }); // { kind: "cues", cues: [{ start, end, speaker, lines, position, style, words: [{ …, pieces }] }] }
mergeSpeakers(capt, { 3: 1 });     // the CAPT text with speaker 3 made speaker 1
```

- `parse(capt)`: the file's id, title, language, media, duration (ms), speakers, the languages of its
  words, how many words and sounds. A broken file throws an `Error` whose `errors` lists every problem
  with its line: `[{ line: 4, message: "a mark holds a time in hundredths, not “x”" }]`.
- `formats()`: every format with exactly the options its `captions` writer takes: `type` (`integer`,
  `number`, `boolean`, `string`, `string[]`, `map`, `enum`), `default` (from `captions`), `min`/`max`,
  `values` and a description. An option a format does not take is an error in `render` and `layout`.
- Speaker names in subtitles are three choices: off (the default), `speakerLabels: true` (when the
  speaker changes), and `speakerLabels: true, speakerLabelsEvery: true` (on every cue). Either way only
  with several speakers and `bySpeaker` on (a cue of mixed speakers has no one name); the label counts
  toward `maxLine`.
- `render(capt, format, options)` (`{}` for the defaults): the file's text, a name for it (the
  file's media name, else `captions`, with the format's extension), its content type and the number
  of cues.
- `layout(capt, format, options)`, laid out exactly as `render` lays it out:
  - subtitle formats (`srt`, `vtt`, `ass`, `sbv`, `lrc`, `ytt`, `ttml`): `{ kind: "cues", cues }`, each
    cue with `start`, `end` (ms), `speaker`, `prefix`, `lines`, `position` (`null` bottom, `"top"`,
    `"middle"`, `{ point }`, `{ rect }`), `words` (text, times, `line`, `italic`, `lang`, `voice`, and
    `pieces`: every syllable or token with its `start` and `end`), and `shown` (the lines piece by
    piece, each pointing at its word); ASS adds the speaker's `style` (name, colour, dimmed colour);
  - `adr`: `{ kind: "adr", fps, timelineStart, rows }`, each row with `id`, `start`, `end` (ms),
    `in`, `out` (timecodes), `character`, `dialogue`;
  - `premiere`: `{ kind: "premiere", transcript }`, Adobe's transcript JSON (segments, speakers);
  - `transcript`, `capt`: `{ kind: "text", text }`.

  Every kind has `speakers`: `[{ id, name }]`.
- `mergeSpeakers(capt, map)`: the CAPT text with speakers made one, for when recognition split a
  person in two: `{ 3: 1 }` makes every block and header line of speaker 3 speaker 1, so every format
  sees one speaker (their neighbouring lines join into cues, ASS has one style, ADR one character).
  Chains resolve (`{ 3: 2, 2: 1 }` makes both speaker 1). The other speakers keep their numbers
  (1, 2, 4 after merging 3, not 1, 2, 3), so their labels and names stay; colours go by order of
  appearance. The merged speaker keeps its name, else takes the source's. Throws for a broken file
  (with `errors`, as `parse`), a speaker mapped onto itself, or a number the file has no speaker for.
  Pass the result to `render` and `layout`.

MIT licensed.
