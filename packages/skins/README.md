# @cantina/skins

The six player skins Cantina and
[music-visualizer](https://github.com/jbouder/music-visualizer) share:
Classic, Midnight, Brushed metal, Hi-fi, Neon and Toxic.

- `tokens.css` imports every skin's palette from `tokens/<id>.css`:
  Classic on `:root` as the fallback, the others under
  `:root[data-skin="<id>"]`. These are the tokens both apps draw with
  (frame, title bar, displays, keys, ink, lights, meters, sheet, focus,
  type).
- `index.ts` exports `SKINS`: each skin's id, name, whether its CSS
  pulses on `--beat`, and a `preview` of its colors for a picker swatch.

An app imports `tokens.css` before its own stylesheets and adds its
component-level tokens and rules on top; those stay in the app. Set
`data-skin` on `<html>` to switch.

`test/skins.test.ts` checks that every skin has a token file, that
Classic defines every token another skin overrides, and that each
preview matches its tokens.
