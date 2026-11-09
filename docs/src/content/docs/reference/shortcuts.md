---
title: "Keyboard shortcuts"
description: "Every keyboard shortcut in Cantina, by menu and by part of the window."
---

<!-- Generated from ui/src/lib/actions.json by ui/src/lib/shortcuts.ts. Do not edit: run UPDATE_SHORTCUTS=1 npx vitest run shortcuts. -->

The same list is in the app under **Help ▸ Keyboard Shortcuts**. Keys
without ⌘ do nothing while you are typing in a text field. The
[menus guide](/guides/menus/) says what each item does.

## Cantina

| Keys | Does |
| --- | --- |
| ⌘, | Settings… |

## File

| Keys | Does |
| --- | --- |
| ⌘N | New Project… |
| ⌘O | Open… |
| ⌘S | Save |
| ⇧⌘S | Duplicate… |
| ⇧⌘I | Import Audio… |
| ⌘E | Export Mixdown… |

## Edit

| Keys | Does |
| --- | --- |
| ⌘Z | Undo |
| ⇧⌘Z | Redo |
| ⌘D | Duplicate Clip |

## Transport

| Keys | Does |
| --- | --- |
| Space | Play |
| R | Record |
| L | Loop |
| K | Metronome |
| Home | Go to Start |
| End | Go to End |

## Track

| Keys | Does |
| --- | --- |
| ⌥⌘N | New Audio Track |

## View

| Keys | Does |
| --- | --- |
| J | Jam |
| P | Performance Mode |
| ⌘J | Assistant |
| ⌘= | Zoom In |
| ⌘− | Zoom Out |
| ⌘0 | Zoom to Fit |

## Timeline

With the timeline focused.

| Keys | Does |
| --- | --- |
| ⌫ | Deletes the selected clip |
| S | Splits the selected clip at the playhead |
| ← → | Nudges the selected clip a grid step, or scrolls with none selected |
| + − | Zooms in and out |
| Esc | Clears the selection |
| ⌥-click | Splits a clip where you click |
| ⇧-drag the ruler | Sets the loop |
| Drag a loop edge | Moves that end of the loop |

## Drum grid

With a step focused.

| Keys | Does |
| --- | --- |
| ← → ↑ ↓ | Moves around the grid |
| Space, Return | Adds or removes a hit |
| ⇧↑ ⇧↓ | Makes the hit 10% harder or softer |
| Home, End | Jumps to the ends of the row |
| ⇧← ⇧→ | Selects across steps; ⌥⇧↑ and ⌥⇧↓ across rows |
| ⌘C ⌘X ⌘V | Copies, cuts and pastes the selection or cell |
| D | Duplicates the selection after itself, or doubles the pattern |
| H | Halves the pattern |
| [ ] | Shifts the selection or row a step left or right |
| ⌫ | Clears the selection or cell |
| ⌥⌫ | Clears the row |
| M S | Mutes or solos the row while you listen |
| Esc | Clears the selection |
| ⇧-drag | Selects cells |
| Drag across | Paints or erases hits |
| C | Steps the chance of the hits: 100, 75, 50, 25% |
| R | Steps the ratchet: 1 to 4 hits in the step |
| F | Turns a flam on or off |

## Live pads

With a pad focused, in the Pads panel of the Beats tab.

| Keys | Does |
| --- | --- |
| A S D F G H J K L ; | Play the kit's first ten pads, in order |
| Z X C V B N | Play the next six |
| ⇧ with a pad key | Plays it as an accent, at full strength |
| ← → | In step entry, move the cursor a step |
| Esc | Stops recording or step entry |

## Jam

In the Jam view, outside a text field or menu.

| Keys | Does |
| --- | --- |
| Space | Plays the jam from the top, or stops it |
| 1–6 | Plays the intro, verse, chorus, bridge, fill or ending from the next bar |
| ↑ ↓ | Nudges the tempo a beat per minute from the next bar; ⇧ by five |
| T | Taps the tempo: two taps or more set it |
| Return | Keep that: keeps the last minutes of your playing as a take |
| P | Plays the jam on stage: the scene full screen with the jam's controls over it |

## Groove browser

With the groove list focused.

| Keys | Does |
| --- | --- |
| ↑ ↓ | Moves through the grooves |
| Page Up, Page Down | Moves eight at a time |
| Home, End | Jumps to the first or last groove |
| Space | Loops the groove on the track's kit, or stops it |
| Return | Adds the groove to the project |
| Esc | Stops the loop |

## Tab editor

With the tab focused and Edit on.

| Keys | Does |
| --- | --- |
| ← → ↑ ↓ | Moves the caret a grid step, or to the next string |
| 0–9 | Types a fret at the caret; two digits quickly for 10 to 24 |
| ⌥← ⌥→ | Moves the selected notes a grid step |
| ⌥↑ ⌥↓ | Moves the selected notes to the next string, keeping the pitch |
| ⇧← ⇧→ | Shortens or lengthens the selected notes |
| H P S B | Hammer-on, pull-off, slide, bend (½, full, 1½, off) |
| V M L | Vibrato, palm mute, let ring |
| N X T | Harmonic, dead note, tap |
| ⌫ | Deletes the selected notes |
| ⌘A | Selects every note |
| Esc | Clears the selection |
| ⇧-click | Adds a note to the selection or takes it away |

## Performance mode

While the stage is full screen.

| Keys | Does |
| --- | --- |
| ← → | Previous and next scene |
| B | Shows or hides the pedalboard |
| ↑ ↓ | Switches chains |
| 1–9 | Stomps the live chain's pedals |
| D | Sends the stage to the other display, or brings it back |
| Esc | Leaves performance mode |
