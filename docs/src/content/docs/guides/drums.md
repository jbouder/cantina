---
title: "Drums"
description: "Add a drum track, pick a kit, program a pattern in the step grid and arrange it on the timeline."
---

A drum track plays **patterns**, step grids you program, through a **kit** of
drum sounds. You place a pattern on the timeline as a clip, and it follows
the tempo.

![A drum track's Beats tab: the kit, the pattern, the step size, length and swing, and the grid](../../../assets/guides/drums.png)

## Add a drum track

**Add drums** above the track headers adds a drum track with the first kit
found. Select it to see its **Beats** tab below the timeline. A drum track has
no input to arm; it has a pedalboard like any other track, so a drum bus
can go through a compressor or a reverb.

## Kits

Pick a kit from **Kit**. Three come with Cantina:

| Kit | Sounds |
| --- | --- |
| Acoustic | Kick, snare, closed, pedal and open hats, three toms, crash and ride, each drum with a soft and a hard hit |
| Tight rock | A heavy kick, a hard snare, closed and short open hats, two toms, crash, ride and cowbell |
| Electronic | Two kicks, two snares, a clap, hats, two toms and percussion |

Every kit has the same names for the kick, snare, hats and toms, so a
pattern written on one plays on the others. The open hat is cut off by the
closed one, as on a real kit.

To make your own kit, copy one of the stock kits from `kits/` in the
repository to `~/Music/Cantina/Kits/`, swap the samples, and press
**Rescan**. The [kit format](/reference/kit-format/) describes every field.

## Program a pattern

A pattern belongs to the project, so any drum track can use it. **New**
makes one; **Rename** and **Remove** do what they say.

- **Click** a cell to add a hit at full strength, or to remove one.
- **Drag up or down** on a cell to set how hard it hits. The fill shows it.
- From the keyboard, the arrow keys move around the grid, Space or Enter
  toggles a cell, Shift with ↑ or ↓ changes the strength by 10%, and Home
  and End jump to the ends of a row.

Above the grid:

- **Steps** sets the size of a step: from quarter notes to 1/32, with
  triplets.
- **Length** is the number of steps before the pattern repeats, up to 128.
  The size in bars and beats is shown beside it.
- **Swing** delays every second step, up to a triplet feel at 100%.

A pattern is saved in the project as JSON; the
[pattern schema](/schemas/pattern.schema.json) describes it, and the
[kit format](/reference/kit-format/) describes kits.

**Loop** plays the pattern on its own, from the start of the timeline, while
the transport plays, and lights the column it is on. Edit as it plays; you
hear each change on the next pass.

## Arrange

**Place at playhead** puts the pattern on the track at the playhead. You can
also drag the button onto a drum track's lane and drop it where the pattern
should start.

A pattern clip moves, trims, splits and deletes like an audio clip (see
[Recording and editing](/guides/recording/#edit-clips)). Drag its right edge
to repeat the pattern for more bars. Clips stay on their bars when you
change the tempo, and editing a pattern changes every clip of it.

A pattern that is on the timeline cannot be removed until its clips are
deleted.

The stock kits' samples are public domain (CC0), recorded by their authors
on Freesound and gathered by the Sonic Pi project. `kits/CREDITS.md` in the
repository lists every file's source.
