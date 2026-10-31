#!/usr/bin/env bash
# Measures what the frame stream costs the UI at idle.
#
#   scripts/measure-ui-cpu.sh [seconds]
#
# Launches the release build twice, once with CANTINA_SYNTHETIC_METERS=1
# (the shell pumps 60 frames a second with no audio stream behind them)
# and once without, samples the CPU of the shell process and its WebKit
# content process with top, and prints the averages. Needs
# `npm run tauri build` first. The window opens on screen; leave it alone
# while it samples.
set -euo pipefail

seconds="${1:-20}"
root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/target/release/bundle/macos/Cantina.app"
bin="$app/Contents/MacOS/cantina"
[ -x "$bin" ] || { echo "build the app first: npm run tauri build" >&2; exit 1; }

sample() {
  local label="$1"; shift
  # Through Launch Services so the window is actually on screen: a web
  # view in a hidden window stops its animation frames, and with them
  # the acknowledgements that let the next frame through.
  open -n --env "$1" "$app"
  sleep 4
  local pid
  pid=$(pgrep -n -f "Cantina.app/Contents/MacOS/cantina")
  # The content process is not a child of the shell; find the WebKit
  # process whose parent is the launch services daemon and that appeared
  # after our launch, i.e. the newest one.
  local web
  web=$(pgrep -n -f "com.apple.WebKit.WebContent" || true)
  # top's first sample is a lifetime average; the rest are one-second
  # deltas, which is what we want.
  local rows
  rows=$(top -l $((seconds + 1)) -s 1 -pid "$pid" -pid "${web:-$pid}" -stats pid,cpu 2>/dev/null \
    | awk -v a="$pid" -v w="${web:-0}" '
        $1 == "PID" { sample++; next }
        sample > 1 && $1 == a { sa += $2; na++ }
        sample > 1 && $1 == w { sw += $2; nw++ }
        END { printf "%.1f %.1f %d", (na ? sa / na : 0), (nw ? sw / nw : 0), na }')
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  read -r avg_app avg_web n <<<"$rows"
  printf "%-22s shell %5.1f%%   web content %5.1f%%   (%s one-second samples)\n" \
    "$label" "$avg_app" "$avg_web" "$n"
}

sample "idle, no frames" CANTINA_NO_SYNTHETIC=1
sample "idle, 60 frames/s" CANTINA_SYNTHETIC_METERS=1
