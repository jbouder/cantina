---
title: "Tabs"
description: "How Cantina turns a recorded part into a tab, how it chooses strings and frets, and the tunings it reads."
---

A tab shows where to play each note: which string, which fret, and how.
Cantina writes one from a clip you recorded, and keeps it on that clip.
This guide grows with the tab features; for how transcription works and
how accurate it is, see the [transcription](/architecture/transcription/)
architecture page.

## How frets are chosen

The same note can be played in several places. An E above middle C is
the open first string, the 5th fret of the second, the 9th of the third,
and so on. Cantina chooses the way a player would:

- **Chords keep their shape.** The notes of a chord stay within a
  stretch of a few frets and on strings next to each other, so a G5 comes
  out as 3-5-5 on the lowest three strings, and an open G as 320003.
- **The hand moves as little as it can.** A riff or a scale stays in one
  position instead of jumping around the neck.
- **Open strings in low positions.** Near the nut, open strings are used
  where a player would use them; higher up they are avoided.
- **Ringing notes are left to ring.** A new note goes on another string
  when the one it could use is still sounding.
- **A repeated note stays put.** Pick the same A four times and it is
  the same fret on the same string each time.

Before choosing frets, Cantina corrects the mistakes transcription is
known to make: a note too low for the instrument (often a driven chord's
root, heard an octave low) moves up an octave, a quiet octave or twelfth
over a single note is dropped as the string's own harmonic, and a bend
heard as two notes becomes one bent note.

It also guesses a few techniques from the recording: a soft note straight
after another on the same string is a hammer-on (going up) or a pull-off
(going down), a short low note in a chug is palm-muted, and a note whose
pitch rose is a bend. Slides, vibrato and harmonics are left for you to
mark.

## Playing a passage somewhere else

Sometimes you play a passage higher up the neck than Cantina chose. Select
it and re-fit it to a region, such as the 7th to 10th fret, and Cantina
places those notes there wherever they reach, as one hand position,
without moving anything outside the selection.

## Tunings

A tab knows its instrument's tuning and capo, and frets are counted from
the capo:

- **Guitar** with 6, 7 or 8 strings, and **bass** with 4, 5 or 6.
- **Any tuning**: standard, drop D, DADGAD, open tunings, a step down,
  or a baritone in B. A note below the lowest string moves up an octave.
- **Capo**: with a capo at the 2nd fret, the open low string is fret 0
  and sounds F♯.

The tab file format is on the [tab schema](/reference/schemas/tab/) page.
