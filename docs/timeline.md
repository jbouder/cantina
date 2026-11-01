# Timeline

The timeline is a Canvas, not DOM: tracks and clips with waveforms, a bar
and beat ruler, the playhead and the grid are drawn by
`ui/src/components/Timeline.tsx` from the project status and the meter
frames. Everything it decides (pixels to samples, the grid, snapping, hit
testing, what a move or trim does to a clip) is a pure function in
`ui/src/lib/timeline.ts`, tested on its own.

## Waveforms

A file's samples are reduced once, in a Web Worker
(`ui/src/lib/peaks.worker.ts`), into min and max per bin at five bin
sizes from 64 to 16384 samples (`ui/src/lib/peaks.ts`). Drawing a clip
reads the coarsest level whose bin is no larger than the zoom's samples
per pixel, so a column costs a few reads at any zoom and a long take is
never touched again after its cache is built. Caches are kept per file
for the page's life (`ui/src/lib/peakStore.ts`); the samples come from the
shell's `audio_samples` command as little-endian mono f32 over a binary
response, or from a synthesized pluck in a browser.

## Editing

- **Move**: drag a clip. Across tracks too. The start snaps to the grid.
- **Trim**: drag either edge. The left edge keeps the audio where it is
  (the offset into the file moves with the start); the right edge can grow
  up to the end of the file once its length is known from the peak cache.
- **Split**: Alt-click a clip at the pointer, or press S with a clip
  selected to split it at the playhead. The right part is a new clip.
- **Delete**: Delete or Backspace with a clip selected, or the clip's
  button in the track panel below the timeline, which lists the selected
  track's clips for the keyboard and assistive tech.
- **Nudge**: the arrow keys move the selected clip by one grid step, or
  scroll the view when nothing is selected.
- **Playhead**: click the ruler. It snaps when snapping is on.
- **Zoom**: pinch or Ctrl-wheel around the pointer, the − and + buttons or
  keys, Fit to see every clip. Swipe sideways to scroll. While rolling,
  the view pages to keep the playhead on screen.
- **Transport**: Space plays or stops, R records when a track is armed,
  anywhere but in a field or on a button. The strip above the lanes has
  the keys, one clock in bars and time, the tempo, snap and zoom.

Each edit becomes one command (`clip_move`, `clip_trim`, `clip_split`,
`clip_remove`) when the pointer is released; the drag is previewed on the
canvas without touching the project. Pattern clips on drum tracks go
through the same code: they are converted from ticks to samples for the
lane and back for the `pattern_clip_*` commands (see
[drums.md](drums.md)), and a clip only lands on a track of its own kind.

## Track headers

The column beside the lanes is the mixer: each track's header has its
name (click to select it for the editor below), the arm light or the
kit mark, mute and solo, a gain fader with its readout, a pan fader, and
a thin post-fader meter (`ui/src/components/TrackHeader.tsx`). The master
strip is pinned under the tracks with gain, mute and the output meter.
Double-click a fader to reset it. Gain and pan send at most one value per
control per animation frame while dragging, and are not echoed back in
the status, so the header keeps the value it last sent until a status
push carries a different one.

## The grid

The grid comes from the project's tempo: samples per beat is
`sampleRate · 60 / bpm`, a bar is `numerator` beats. The division shown
and snapped to is the finest of sixteenth, eighth, beat, bar, 2, 4, 8, 16
or 32 bars whose lines are at least 18 pixels apart at the current zoom;
the transport strip says which. Snap can be toggled off. The tempo is
editable in the strip and is a command like any other edit.

## Colors

The canvas reads its colors from the skin's `--tl-*` tokens (`tl-bg`,
`tl-lane`, `tl-grid`, `tl-bar`, `tl-ruler-fg`, `tl-clip`, `tl-clip-edge`,
`tl-clip-selected`, `tl-wave`, `tl-playhead`) with `getComputedStyle` on
each draw, so it reskins live like everything else and no literal color
appears in the component.
