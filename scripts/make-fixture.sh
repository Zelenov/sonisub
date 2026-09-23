#!/usr/bin/env bash
# Regenerates tests/fixtures/dialog.* from real Soniox output:
#   Soniox TTS lines -> dialog.mp4 (AAC 48 kHz stereo, like camera files)
#   -> sonisub -j -> dialog.soniox.json (real transcript) + dialog.srt (golden subtitles).
# Needs SONIOX_API_KEY, curl, ffmpeg and a built sonisub (cargo build --release).
set -euo pipefail

cd "$(dirname "$0")/.."
out=tests/fixtures
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
: "${SONIOX_API_KEY:?set SONIOX_API_KEY}"

# voice|language|pause after, seconds|text
lines=(
  "Yana|ru|0.4|Привет! Сегодня мы проверяем генератор субтитров."
  "Andrei|ru|0.5|Отлично. Я подготовил очень длинное предложение, в котором много запятых, чтобы проверить, как утилита разрезает реплики на части, не превышая длину строки."
  "Yana|ru|1.6|А что будет, если я сделаю паузу... и потом продолжу?"
  "Daniel|en|0.5|And now a short sentence in English."
  "Andrei|ru|0.9|Да."
  "Yana|ru|0.8|Всё, конец теста."
)

list="$work/list.txt"
: > "$list"
i=0
for line in "${lines[@]}"; do
  IFS='|' read -r voice lang pause text <<< "$line"
  i=$((i + 1))
  body=$(printf '{"model":"tts-rt-v2","voice":"%s","language":"%s","audio_format":"wav","sample_rate":48000,"text":"%s"}' "$voice" "$lang" "$text")
  code=$(curl -sS -o "$work/$i.wav" -w '%{http_code}' https://tts-rt.soniox.com/tts \
    -H "Authorization: Bearer $SONIOX_API_KEY" -H 'Content-Type: application/json' --data-binary "$body")
  [ "$code" = 200 ] || { echo "TTS failed ($code): $(cat "$work/$i.wav")" >&2; exit 1; }
  ffmpeg -v error -y -f lavfi -i "anullsrc=r=48000:cl=mono" -t "$pause" "$work/$i.pause.wav"
  # Relative to the list file: absolute MSYS paths (/tmp/...) mean nothing to a native ffmpeg.
  printf "file '%s'\nfile '%s'\n" "$i.wav" "$i.pause.wav" >> "$list"
  echo "  $i. $voice ($lang): $text"
done

ffmpeg -v error -y -f concat -safe 0 -i "$list" -ar 48000 -ac 1 "$work/dialog.wav"
# Leading half second of silence, like a real clip; small test-pattern video.
ffmpeg -v error -y -f lavfi -i "testsrc2=size=320x180:rate=15" -i "$work/dialog.wav" \
  -filter_complex "[1:a]adelay=500,aformat=channel_layouts=stereo[a]" -map 0:v -map "[a]" -shortest \
  -c:v libx264 -preset veryslow -crf 40 -pix_fmt yuv420p -c:a aac -b:a 96k -movflags +faststart "$out/dialog.mp4"

./target/release/sonisub "$out/dialog.mp4" -f -j --log "$work/sonisub.log"
ls -la "$out"
