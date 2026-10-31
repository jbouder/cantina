#!/usr/bin/env bash
# Captures PNGs of the UI for pull requests.
#
#   scripts/screenshot.sh <name> [query]...
#
# Starts the Vite dev server (the UI runs against its browser-side engine
# stand-in, see ui/src/lib/engine.ts), renders each query string with
# headless Chrome at the default window size, and writes
# docs/screenshots/<name>[-<query>].png. With no query it captures once.
#
#   scripts/screenshot.sh m2-scaffold
#   scripts/screenshot.sh m2-skins "skin=classic" "skin=neon"
set -euo pipefail

name="${1:?usage: screenshot.sh <name> [query]...}"
shift
queries=("$@")
if [ ${#queries[@]} -eq 0 ]; then
  queries=("")
fi

chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
port=1427
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/docs/screenshots"
mkdir -p "$out"

cd "$root"
npx vite --port "$port" --strictPort --logLevel silent &
vite=$!
trap 'kill $vite 2>/dev/null || true' EXIT

for _ in $(seq 1 50); do
  if curl -fs "http://localhost:$port/" >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done

for query in "${queries[@]}"; do
  file="$out/$name"
  if [ -n "$query" ]; then
    file="$file-$(echo "$query" | sed 's/[^A-Za-z0-9]/-/g; s/^skin-//')"
  fi
  file="$file.png"
  "$chrome" --headless=new --disable-gpu --hide-scrollbars \
    --window-size=1100,720 --force-device-scale-factor=2 \
    --virtual-time-budget=2000 \
    --screenshot="$file" "http://localhost:$port/?$query" >/dev/null 2>&1
  echo "$file"
done
