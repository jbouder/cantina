Closes #

## What

<!-- What changed, for someone who has not read the issue. -->

## How

<!-- The approach, and anything a reviewer should look at first. -->

## Proof

<!-- Tests, and for audio the numbers: peak, RMS, DC, latency from
`cantina-cli render`. Say so if a golden file changed, and why. -->

## Checklist

- [ ] One issue, closed by this pull request, on its milestone
- [ ] Nothing on the audio thread allocates, locks, blocks or does I/O
- [ ] Every project edit is a `Command`; a new shell command has its stand-in in `ui/src/lib/engine.ts`
- [ ] Docs updated in `docs/src/content/docs/` if a user or contributor would notice
- [ ] Screenshots for UI changes (`scripts/screenshot.sh`), across more than one skin when the look changes
- [ ] What CI runs passes locally (see CONTRIBUTING.md)
