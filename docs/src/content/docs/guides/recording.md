---
title: "Recording and editing"
description: "Record takes, edit clips on the timeline, mix tracks, and set the tempo, grid and metronome."
---

The studio window is an arrangement on top and an editor for the selected
track below, with a divider between them that you can drag. The arrangement
is the transport, the track headers that make up the mixer, and the timeline.

![The studio: two tracks with takes on the timeline and the guitar's pedalboard below](../../../assets/guides/studio.png)

## Tracks

**Add guitar** adds an audio track with an empty pedalboard; **Add drums**
adds a [drum track](/guides/drums/). Click a track's name to select it and
show its editor below. The editor's header has the track's name, then its
tabs: **Beats** on a drum track, **Pedalboard**, **Clips**, and **Tab** when
a clip has one. Click the name to rename the track. The **⋯** key at the
right opens the track's actions: rename, bounce, move up, move down and
remove. When an editor is taller than its pane it scrolls, and a bar at
the right shows how far.

## Record a take

1. Arm the track: click the light beside its name, or choose **Track ▸ Arm for
   Recording**. An armed track plays your input through its pedals.
2. Press **Record**, R or **Transport ▸ Record**. The click counts in four beats, whether or not the
   metronome is on, and the clock shows the count. Then the transport rolls
   from the playhead and the take starts on that exact sample.
3. Press **Stop** or Space. The take lands on the track as a clip. Stopping
   during the count-in cancels it and records nothing. To start
   recording at once, uncheck **Transport ▸ Count In**.

Takes are written to the project's folder as they record, and the files are
never changed afterwards: every edit below is to the clips that point into
them. Takes are lined up with what you heard, so latency from the chain and
the interface does not push them late.

## Play and move around

- **Space** plays from the playhead or stops.
- **Click the ruler** to move the playhead, or press Home and End (**Transport ▸
  Go to Start** and **Go to End**) to jump to either end of the song.
- The clock shows bars and beats, then minutes and seconds.
- **Zoom** with a pinch or Ctrl and the scroll wheel, the **−** and **+**
  keys, or **Fit** to see every clip. Swipe sideways to scroll. While the
  transport rolls, the view pages to keep the playhead in sight.


## Loop a section

Press L, or choose **Transport ▸ Loop**, and playback goes round the loop
region: at its end it jumps back to its start on the exact sample, clips,
drums and the click with it. The region shows on the ruler, bright while
looping and faint while not.

- **Shift-drag on the ruler** to draw a loop. It snaps to the grid.
- **Drag either end** of the loop on the ruler to move it.
- **Transport ▸ Loop Selected Clip** loops the clip you have selected.
- With no loop drawn yet, L loops four bars from the bar the playhead is in.

Recording while looping records one pass: the take stops at the loop's end,
exactly, and lands on the track like any other. Setting and moving the loop
are edits, so ⌘Z undoes them, and the loop is saved with the project.

## Edit clips

| To | Do |
| --- | --- |
| Move a clip | Drag it, onto another track too. It snaps to the grid. |
| Trim a clip | Click it, then drag the handle on either edge. The audio stays where it was, and a trimmed edge can be dragged back out. |
| Split a clip | Alt-click it where it should split, or select it and press S to split at the playhead. |
| Duplicate a clip | Select it and press ⌘D. The copy lands in the first space after it that holds it, never over another clip, and is selected, so another ⌘D adds the next. |
| Cut, copy and paste a clip | Select it, then ⌘X or ⌘C with the timeline focused. ⌘V puts a copy on the selected track at the playhead, in the first space that holds it. |
| Delete a clip | Select it and press Delete, or use its button on the **Clips** tab. |
| Nudge a clip | Select it and press ← or → to move it one grid step. |

Right-click a clip for a menu of the same edits: split it where you clicked or
at the playhead, trim its start or end to the playhead, duplicate or delete it.
Drum clips work the same way.

Every edit can be undone with ⌘Z and redone with ⇧⌘Z (the Edit menu names the edit it will undo), or with the arrows
beside the project name. The **Clips** tab of the track's editor lists its
clips, for working from the keyboard, and its **Tab** button transcribes a
clip into a [tab](/guides/tabs/#transcribe-a-clip).

![The Clips tab, listing the selected track's clips](../../../assets/guides/clips.png)

## Tempo and the grid

Type a tempo in the **Tempo** box. The grid follows it, and the strip says
how fine it is: the grid gets finer as you zoom in, from 32 bars down to a
sixteenth note. **Snap** turns snapping off and on. Drum clips stay on their
bars when the tempo changes.

## Metronome

The metronome key in the transport strip (**Click**, or only its glyph in
a narrow window) turns the metronome on and off; K does the
same from the keyboard. While the transport rolls, it clicks on every beat
of the tempo, with a different sound on the first beat of each bar.

- The drop-down beside it picks the sound: **Standard**, **Woodblock**,
  **Cowbell**, **Drum** (a kick on the downbeat, a snare on the others) or
  **Hi-hat**.
- The slider sets its level, from -40 to +6 dB. Double-click it for -6 dB.
- With the metronome on, **Play** works on an empty project, so you can
  practice to it before anything is recorded.

The click goes to your speakers or headphones only. It is not on the output
meter, the stage, a recorded take or an exported mix. The metronome is a
setting of the app, not of the project, so it stays as you left it across
projects and launches, and undo does not touch it.

## Mix

Each track header is a channel strip:

- the name, which selects the track
- the **arm** light, or a drum mark on a drum track
- **mute** and **solo**
- a **gain** fader with its value in dB beside it, and a **pan** fader
  with its position (`C`, `L25`, `R10`)
- a thin meter along the bottom of what the track sends to the master

The **Master** strip is pinned along the bottom of the arrangement and
stays put while the tracks scroll. It has a mute, the overall gain and the
output meter. Double-click a fader to reset it.

The editor below the arrangement gets room for a pedal's controls, and the
arrangement takes the rest of the window, its empty lanes waiting for more
tracks. In a short window the arrangement still shows its tracks, up to
half the window. Drag the divider to move the split either way; a split
you drag is kept for the session.

When the mix is done, [export it](/guides/mixdown/) to a WAV file.

## Projects

A project is a folder in `~/Music/Cantina/` holding its takes, its settings
and every edit. **Save** writes it, and **Projects…** opens another or
starts a new one with a name of your choosing. If Cantina quits without saving, the next
launch offers to restore the unsaved edits.
