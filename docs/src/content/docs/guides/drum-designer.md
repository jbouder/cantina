---
title: "Drum designer"
description: "Build a drum track's whole part in a window of its own: the kit's pads down one side, a large grid, an inspector and a transport that plays the part on loop."
---

The Beats tab is a strip under the timeline, and a drum part is a whole
song's worth of work. The **drum designer** gives it the window: the
kit's pads down the left, a large grid in the middle, an inspector on
the right and a transport of its own.

## Opening it

Open it on a drum track in any of three ways:

- **Design…** in the track's Beats tab.
- **View ▸ Drum Designer**, which opens it on the selected drum track,
  or the first one.
- **⌘B**.

It opens with the track's kit and patterns. Each pattern the track's
clips play becomes a **section**, named after the pattern, and the clips
in order become the part's **order**: two Verse clips of two bars each
followed by a Chorus clip read as *Verse ×4, Chorus ×2*. A track with no
clips gets a section for each of the project's patterns, and a project
with none an empty verse.

## A copy, heard on its own

The designer works on a **copy** of the part. While it is open the
engine hears the part instead of the project, as it hears a jam: the
part on a drum track of its own, through the track's kit, pad mixer and
fader, at the project's tempo and time signature, with the click if the
metronome is on. The track's pedals are not in the designer.

Nothing you do in the designer changes the project. **Back to project**
(or Esc, or ⌘B again) closes it, and the project is heard again with its
playhead where it was. If the part has changes that are not on the
track, the designer asks first: **Leave without placing** drops them.

## The transport

- **Loop section** plays the section you are editing round and round,
  long enough for its fill to come round.
- **Whole song** plays the order from the top and loops at its end.
- **Play** (Space) starts the loop from the top; Space again stops it.
  The readout shows the bar and beat, the tempo and the time signature.

## Sections and the order

Two lanes run across the top.

**Sections** lists the part's sections; the grid edits the one chosen.
Keys **1** to **9** choose the first nine. **+** adds a section: an intro,
verse, chorus, bridge, fill or ending, named after its kind, or one of
your own to name in the inspector. A new section is an empty pattern as
long as the one you were editing; a fill starts as one bar. Each section
has a pattern of its own length. In the inspector, **Duplicate** copies the section
as "Verse 2", and **Remove section** takes it out with its steps in the
order and any fills that came from it.

**Order** arranges the sections into a song. Each step plays a section a
number of times, as wide in the lane as it is long: *Verse ×4, Chorus
×2, Verse ×2, Ending ×1*. Under each step are its bars and how many of
them play a fill, and the total is at the end of the lane. **+** at the
end adds the section you are editing. Click a step to show it in the
inspector, where you can change what it plays and how many times, move it
earlier or later, remove it, or **Play from here**.

A section can play a **fill**: the first bar of another section played
in its place on the last bar of every 2, 4, 8 or 16, and on its last bar
in each step of the order with **At its end**, just as a pattern clip's
fills do on the timeline. Looping a section alone, the loop runs long
enough for its fill to come round.

### Changing what plays

While the part rolls, choosing another section to loop, **Whole song**,
or **Play from here** lands on the next bar line, as a jam's sections
do: what is playing carries on to the bar, and the new part starts on it.
Stopped, the change is where play starts next time.

Edits to the sections and the order are part of the designer's history,
so **⌘Z** takes them back like any other.

## The grid and the inspector

Click a cell to put a hit there, or click a hit to take it away; drag
across cells to paint or erase a run. With a cell focused, the arrows
move and Space or Return toggles it. The column under the playhead lights
while the section plays.

The inspector sets the section's **name** (the order and fills follow a
rename), its **kind** (intro, verse, chorus, bridge, fill, ending or your
own), its **length** in bars, its **swing** and its **fill**.

The designer keeps a history of its own: **⌘Z** and **⇧⌘Z**, or the
arrows in its toolbar, step back and forward through your edits in the
designer without touching the project's.
