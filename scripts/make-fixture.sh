#!/usr/bin/env bash
# Regenerates tests/fixtures from real Soniox output. Needs SONIOX_API_KEY, curl, ffmpeg and a built
# sonisub (cargo build --release). Review the diff of the .srt files before committing them.
#
#   dialog.*       two English voices + a Russian and a Spanish line: speakers, languages, pauses
#   punctuation.*  text dense with , ; : — ( ) " ... ?! abbreviations and numbers, plus a long
#                  sentence without any punctuation: how Soniox punctuates and how we cut it
#   nospeech.*     noise and music, no words: Soniox's real empty transcript (the no-speech marker)
#   noaudio.mp4    video without an audio track        (never reaches Soniox)
#   silence.m4a    digital silence                      (never reaches Soniox)
#
# For each TTS fixture: <name>.mp4 (AAC 48 kHz stereo, like camera files), <name>.soniox.json
# (real transcript), <name>.srt (default layout), <name>.unlimited.srt, <name>.speakers.srt, <name>.wrap.srt.
set -euo pipefail

cd "$(dirname "$0")/.."
out=tests/fixtures
sonisub=./target/release/sonisub
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
: "${SONIOX_API_KEY:?set SONIOX_API_KEY}"
mkdir -p "$out"

# tts <file.wav> <voice> <language> <text>
tts() {
  local body code
  body=$(VOICE="$2" LANG_="$3" TEXT="$4" python -c 'import json, os; print(json.dumps({
      "model": "tts-rt-v2", "voice": os.environ["VOICE"], "language": os.environ["LANG_"],
      "audio_format": "wav", "sample_rate": 48000, "text": os.environ["TEXT"]}))')
  code=$(curl -sS -o "$1" -w '%{http_code}' https://tts-rt.soniox.com/tts \
    -H "Authorization: Bearer $SONIOX_API_KEY" -H 'Content-Type: application/json' --data-binary "$body")
  [ "$code" = 200 ] || { echo "TTS failed ($code): $(cat "$1")" >&2; exit 1; }
}

# video <name> <language hints> <lines...>   each line: "voice|language|pause after, s|text"
video() {
  local name=$1 langs=$2 list="$work/$1.list" i=0
  shift 2
  : > "$list"
  echo "$name:"
  for line in "$@"; do
    IFS='|' read -r voice lang pause text <<< "$line"
    i=$((i + 1))
    tts "$work/$name.$i.wav" "$voice" "$lang" "$text"
    ffmpeg -v error -y -f lavfi -i "anullsrc=r=48000:cl=mono" -t "$pause" "$work/$name.$i.pause.wav"
    # Relative to the list file: absolute MSYS paths (/tmp/...) mean nothing to a native ffmpeg.
    printf "file '%s'\nfile '%s'\n" "$name.$i.wav" "$name.$i.pause.wav" >> "$list"
    echo "  $voice ($lang): $text"
  done
  ffmpeg -v error -y -f concat -safe 0 -i "$list" -ar 48000 -ac 1 "$work/$name.wav"
  mux "$work/$name.wav" "$out/$name.mp4"
  transcribe "$name" "$langs"
}

# mux <audio> <out.mp4>: half a second of lead-in silence, small test-pattern video.
mux() {
  ffmpeg -v error -y -f lavfi -i "testsrc2=size=320x180:rate=15" -i "$1" \
    -filter_complex "[1:a]adelay=500,aformat=channel_layouts=stereo[a]" -map 0:v -map "[a]" -shortest \
    -c:v libx264 -preset veryslow -crf 40 -pix_fmt yuv420p -c:a aac -b:a 96k -movflags +faststart "$2"
}

transcribe() {
  local name=$1 langs=$2
  rm -f "$out/$name.soniox.json" "$out/$name.srt" "$out/$name.premiere.json"
  "$sonisub" "$out/$name.mp4" -f -t srt,premiere -l "$langs" --log "$work/sonisub.log"
  "$sonisub" "$out/$name.soniox.json" -f -u -o "$out/$name.unlimited.srt" --log "$work/sonisub.log"
  "$sonisub" "$out/$name.soniox.json" -f -s -o "$out/$name.speakers.srt" --log "$work/sonisub.log"
  "$sonisub" "$out/$name.soniox.json" -f -w -o "$out/$name.wrap.srt" --log "$work/sonisub.log"
}

video dialog en,ru,es \
  "Grace|en|0.4|Hi! Today we're testing the subtitle generator." \
  "Daniel|en|0.5|Great. I've prepared a very long sentence, with plenty of commas, so we can check how the tool splits it into parts, without breaking a phrase in the middle, and without exceeding the line length." \
  "Grace|en|1.6|What happens if I pause... and then continue?" \
  "Andrei|ru|0.5|А теперь короткая фраза по-русски." \
  "Mateo|es|0.5|Y una frase corta en español." \
  "Daniel|en|0.9|Yes." \
  "Grace|en|0.8|Okay, that's the end of the test."

video punctuation en \
  "Grace|en|0.5|Well, here's the plan: first, we record; then, we transcribe; finally — and this is the tricky part — we cut the text into subtitles." \
  "Daniel|en|0.5|Wait... really? You're doing all of that by hand?!" \
  "Grace|en|0.5|No! Dr. Smith wrote a tool (a small one, written in Rust) that does it for us, e.g. for the videos from Kenya, Nairobi, and Mombasa." \
  "Daniel|en|0.5|He said: \"it just works\"; I'm not so sure." \
  "Grace|en|0.5|Version 3.5 costs \$12.50 per hour — about 20 cents a minute." \
  "Daniel|en|0.7|Hmm... well... okay. Fine. Deal!" \
  "Grace|en|0.5|And then we went down to the river and we sat there for a long time and we watched the boats go by until the sun went down and it got too cold to stay outside"

echo nospeech:
ffmpeg -v error -y -f lavfi -i "anoisesrc=color=brown:amplitude=0.05:d=6:r=48000" \
  -f lavfi -i "sine=f=330:d=6:r=48000" -f lavfi -i "sine=f=495:d=6:r=48000" \
  -filter_complex "[1:a]volume=0.15,tremolo=f=2[a];[2:a]volume=0.1,tremolo=f=3[b];[0:a][a][b]amix=inputs=3:normalize=0[m]" \
  -map "[m]" -ac 1 "$work/nospeech.wav"
mux "$work/nospeech.wav" "$out/nospeech.mp4"
rm -f "$out/nospeech.soniox.json"
"$sonisub" "$out/nospeech.mp4" -f -l en --log "$work/sonisub.log"

echo noaudio, silence:
ffmpeg -v error -y -f lavfi -i "testsrc2=size=320x180:rate=15" -t 3 -c:v libx264 -preset veryslow -crf 40 \
  -pix_fmt yuv420p "$out/noaudio.mp4"
ffmpeg -v error -y -f lavfi -i "anullsrc=r=48000:cl=stereo" -t 3 -c:a aac -b:a 64k "$out/silence.m4a"

ls -la "$out"
