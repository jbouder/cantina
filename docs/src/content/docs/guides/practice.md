---
title: "Practice"
description: "The Practice view: pick an exercise by style and by what it teaches, read its instructions and its tab, hear it through its own rig over its groove, and play along through the same rig, looped, counted in and a tempo rung at a time."
---

Practice is a room for exercises: chords, changes, scales, technique,
riffs and rhythm. Each [exercise](/reference/exercise-format/) is a
short phrase with a few sentences on how to play it, a tempo ladder from
a speed you can play today to the one the music is at, and the sound
around it: the rig it is heard through and a groove to play over.

Practice never edits the project. Like a [jam](/guides/jam/), the
exercise is heard *instead of* the project while the view is open, and
when you leave, the project is heard again with its playhead where it
was.

## Opening it

Choose **View ▸ Practice**, press **E**, or click **Practice** in the
start dialog. **Back to project**, **E** again or **View ▸ Arrange** goes
back to the project. The exercise you had last comes back the next time,
at the tempo you left it, with the loop and the groove as you left them.

## Picking an exercise

The left side lists every exercise: the stock ones, then yours from
`~/Music/Cantina/Exercises`, each with its start and goal tempo. Narrow
the list by **Style** (Beginner, Blues, Rock, Funk…) and by what it
**Teaches**: chords, changes, scales, technique, riffs or rhythm. Click
one to practice it; it starts at its start tempo.

## Reading it

The right side has a display of the pass, the bar, the beat and the
tempo, with the rung of the ladder you are on. Under it are the
instructions: what the hands do, what to listen for and what goes wrong.
Then the phrase as a tab, wrapped to fit, with a cursor that follows
while it plays. A chord exercise shows each strum as the strings it
plays.

## Chord diagrams and the fretboard

Above the tab, a chord exercise shows the chord sounding as a box
diagram, with the one coming next beside it, dimmer. A phrase of notes
shows a box only on a beat where two strings or more start together,
such as a power chord.

- **The box** has the strings up and down, the lowest on the left, and
  the frets across. A thick line at the top is the nut; a shape up the
  neck has its first fret's number beside it instead. A dot is a string
  held down, with the finger to use: 1 the index to 4 the little finger.
  A bar is one finger laid across several strings. `o` over a string is
  open and `×` is a string not played. A capo is named under the box.
- **The fretboard** is the neck in the exercise's tuning, drop tunings
  and a capo included, the highest string on top as tab reads. Every note
  of the phrase is a faint dot, so a scale shows as the shape it makes;
  the notes sounding are lit, and the ones coming next are rings. Stopped,
  the rings show where to start.

Left-handed players can turn on **Settings ▸ Fretboard ▸ Left-handed** to
draw the neck with the nut on the right, as they see it.

## Hearing it and playing along

**Play** (or **Space**) counts in the exercise's own bars, then plays the
phrase on the plucked strings that play tabs, through the exercise's rig,
over the verse of its groove. Your input goes through the same rig, so
you hear yourself as the example sounds. Pick another rig under **Your
rig** to play through that instead; the example keeps the exercise's.

- **Count in** counts the exercise's bars first; an exercise may have
  none.
- **Loop** plays the phrase round and round. Off, it plays once and
  stops; turned off while it plays, the pass going on finishes first.
  **L** turns it on and off.
- **Groove** plays the drums under it. Off, only the click keeps time.
  **G** turns it on and off.
- The click and its sound are the project's metronome.

## Climbing the tempo

**−** and **+** beside **Tempo**, or **↓** and **↑**, move a rung down or
up the exercise's ladder: from its start to its goal by its step, 5 bpm
for most. While it plays, the new tempo lands on the next bar line, so
the time never stumbles. Type a tempo to go straight to it.

The way to speed is small steps: play a few clean passes, go up a rung,
and drop back one when it gets away from you.

## The stock exercises and your own

The [exercise format](/reference/exercise-format/#the-stock-exercises)
page lists the stock exercises. To write your own, by hand or with a
[coding agent](/guides/agents/), save a `*.exercise.json` in
`~/Music/Cantina/Exercises`; it appears in the list within a second, and
one that does not load is listed with the reason.

See the [shortcut reference](/reference/shortcuts/#practice) for every
key.
