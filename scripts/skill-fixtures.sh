#!/usr/bin/env bash
# Checks and renders every example and fixture of the cantina-author
# skill with cantina-cli, so a change to a format or the CLI that breaks
# the skill's authoring path fails CI.
#
#   scripts/skill-fixtures.sh
#
# Pedals: check, then render over the DI and fail on clipping, a DC
# offset over 0.001 or silence (render exits 1 on silence or NaN).
# Patterns: check against the stock rock kit and render at 140 bpm.
# Kit recipes: check, synthesize and check the kit that comes out.
# Rigs: check, then render over their instrument's DI, judged as pedals are.
# Jams: check, with no warnings: every groove, kit and rig they name is stock.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
skill="$root/.claude/skills/cantina-author"
cargo build -q -p cantina-cli --manifest-path "$root/Cargo.toml"
cli="$root/target/debug/cantina-cli"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failed=0
fail() {
  echo "FAIL $1: $2" >&2
  failed=1
}

while IFS= read -r file; do
  rel="${file#"$root"/}"
  case "$file" in
    *.pedal.json | *.rig.json)
      "$cli" check "$file" >/dev/null || { fail "$rel" "check"; continue; }
      case "$file" in
        *.rig.json) what=(render rig "$file") ;;
        *) what=(render pedal "$file" --di) ;;
      esac
      if ! out="$("$cli" "${what[@]}" --json -o "$work/out.wav")"; then
        fail "$rel" "render: silent or not numbers"
        continue
      fi
      node -e '
        const s = JSON.parse(process.argv[1]);
        const bad = [];
        if (s.clipped > 0) bad.push(`${s.clipped} samples clip`);
        if (Math.abs(s.dcOffset) > 0.001) bad.push(`DC offset ${s.dcOffset}`);
        if (s.peakDb > 0) bad.push(`peak ${s.peakDb} dBFS`);
        if (bad.length) { console.error(bad.join("; ")); process.exit(1); }
        console.log(`ok ${process.argv[2]}: peak ${s.peakDb.toFixed(1)} dBFS, rms ${s.rmsDb.toFixed(1)} dBFS, latency ${s.latency}`);
      ' "$out" "$rel" || fail "$rel" "levels"
      ;;
    *.jam.json)
      # A jam names stock grooves, kits and rigs: check it, warnings and all.
      out="$("$cli" check "$file")" || { fail "$rel" "check"; continue; }
      if grep -q "^warning" <<<"$out"; then fail "$rel" "warnings: $out"; continue; fi
      echo "ok $rel"
      ;;
    *.pattern.json)
      "$cli" check "$file" --kit "$root/kits/rock" >/dev/null || { fail "$rel" "check"; continue; }
      "$cli" render pattern "$file" --kit "$root/kits/rock" --bpm 140 -o "$work/out.wav" >/dev/null \
        || { fail "$rel" "render"; continue; }
      echo "ok $rel"
      ;;
    *.recipe.json)
      "$cli" check "$file" >/dev/null || { fail "$rel" "check"; continue; }
      rm -rf "$work/kit"
      "$cli" synth-kit "$file" "$work/kit" >/dev/null || { fail "$rel" "synth-kit"; continue; }
      echo "ok $rel"
      ;;
  esac
done < <(find "$skill/examples" "$skill/fixtures" -name '*.json' | sort)

exit "$failed"
