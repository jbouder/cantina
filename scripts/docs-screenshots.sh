#!/usr/bin/env bash
# Retakes every screenshot the README and the docs site embed.
#
#   scripts/docs-screenshots.sh            # all of them
#   scripts/docs-screenshots.sh jam        # only the ones whose path has "jam"
#
# Each line of the list below is an image under docs/src/assets/ and the
# query that draws it in the browser stand-in (ui/src/lib/engine.ts), at
# the default 1100 by 720 window and 2x. Add a line when a page gets a new
# image, and run this after a change to the UI so the docs show the app
# as it is. skins/recipe-ember.png is not here: it needs the guide's
# example skin, which is not in the tree (see docs/guides/skins.mdx).
set -euo pipefail

only="${1:-}"
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
port=1427
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/docs/src/assets"

shots=$(cat <<'LIST'
studio-classic.png|running&take&tab=pedalboard&chain=overdrive,chorus,tremolo,amp,cab
drums-neon.png|running&drums&beats&skin=neon
designer.png|running&design=overdrive&select=tone
guides/studio.png|running&armed&take
guides/start.png|start
guides/settings.png|settings
guides/instrument-profile.png|running&settings=instrument&calibrated=-9
guides/picker.png|running&armed&picker
guides/clips.png|running&take&tab=clips
guides/export.png|running&take&export
guides/rigs.png|running&tab=pedalboard&rigs&chain=gate,overdrive,amp,cab
guides/rigs-audition.png|running&tab=pedalboard&rigs&rigaudition&myrigs&broken-files&chain=gate,overdrive,amp,cab&skin=midnight
guides/designer.png|running&design=overdrive
guides/drums.png|running&drums&beats&feel&split=180
guides/drums-edit.png|running&drums&beats&gridselect=0,1,4,7&rowmute=hh-open&split=180
guides/feel.png|running&drums&beats&feel&split=180
guides/fills.png|running&drums&beats&tab=clips&fill
guides/grooves.png|running&drums&beats&grooves&split=180
guides/pad-mixer.png|running&drums&beats&padmixer&split=180
guides/pads.png|running&drums&beats&pads&split=180
guides/drum-designer.png|running&drums&beats&fill&drumdesign=playing&position=5.2
guides/drum-designer-tools.png|running&drums&beats&drumdesign&variations&variant=1&skin=hifi
guides/drum-designer-palette.png|running&drums&beats&drumdesign&grooves&palette&skin=midnight
guides/drum-designer-kit.png|running&drums&beats&kit=metal&drumdesign&workbench=open
guides/drum-designer-pads.png|running&drums&beats&drumdesign=playing&pads&position=0.9&skin=neon
guides/drum-designer-place.png|running&drums&beats&drumdesign&place&skin=toxic
guides/perform.png|running&perform&board&scene=bars&chain=overdrive,chorus,tremolo,amp,cab
guides/jam.png|running&jam
guides/jam-playing.png|running&jam=playing&jambacking&position=9&skin=hifi
guides/jam-save.png|running&jam&savejam&skin=toxic
guides/jam-keep.png|running&jam&kept&skin=metal
guides/jam-stage.png|running&jam=playing&jambacking&perform&position=50&skin=neon
guides/transcribe.png|running&take&tab=clips&transcribe=4
guides/tab-follow.png|running&take&tabbed&tab=tab&position=5.3
guides/tab-page.png|running&take&tabbed&tab=tab&tabpage&position=5.3&skin=midnight
guides/tab-edit.png|running&take&tabbed&tab=tab&tabedit&tabselect=11,12,13
guides/tab-practice.png|running&take&tabbed&tab=tab&practice&position=2.4&skin=toxic
guides/tab-export.png|running&take&tabbed&tab=tab&tabexport
guides/practice.png|running&exercise=playing&practicelog
guides/practice-course.png|running&exercise&practicelog&course=Metal%20rhythm&skin=midnight
guides/practice-lefty.png|running&exercise=playing&lefty&skin=hifi
guides/assistant-chat.png|running&take&assistant&chat
guides/assistant-droid.png|assistant&chat=empty&skin=midnight
guides/assistant-settings.png|settings=assistant&assistant
skins/classic.png|running&armed&take&skin=classic
skins/midnight.png|running&armed&take&skin=midnight
skins/metal.png|running&armed&take&skin=metal
skins/hifi.png|running&armed&take&skin=hifi
skins/neon.png|running&armed&take&skin=neon
skins/toxic.png|running&armed&take&skin=toxic
LIST
)

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

while IFS='|' read -r file query; do
  case "$file" in
    *"$only"*) ;;
    *) continue ;;
  esac
  mkdir -p "$(dirname "$out/$file")"
  # Software WebGL, so the stage's scenes draw without a GPU.
  "$chrome" --headless=new --use-angle=swiftshader --enable-unsafe-swiftshader \
    --hide-scrollbars \
    --window-size=1100,720 --force-device-scale-factor=2 \
    --run-all-compositor-stages-before-draw --virtual-time-budget=2000 \
    --screenshot="$out/$file" "http://localhost:$port/?$query" >/dev/null 2>&1
  echo "docs/src/assets/$file"
done <<<"$shots"
