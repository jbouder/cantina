---
title: "Course format"
description: "A course is a *.course.json: exercises in the order to practice them, each with the tempo to reach before the next."
---

A course puts [exercises](/reference/exercise-format/) in an order, with
the tempo to reach on each before moving on, as a file of its own:
`<id>.course.json`. The [practice guide](/guides/practice/#courses) is
about following one; this page is the file.

## Schema

[`course.schema.json`](/schemas/course.schema.json) is a JSON Schema
generated from the types Cantina reads; the [course
schema](/reference/schemas/course/) page lists every field.

```json
{ "$schema": "https://cantina-docs.vibeproject.workers.dev/schemas/course.schema.json" }
```

## An example

```json
{
  "version": 1,
  "name": "Bass foundations",
  "description": "Root and fifth, a walking blues, octaves, then locking with a metal kick.",
  "instrument": "bass",
  "style": "Beginner",
  "steps": [
    { "exercise": "bass-root-and-fifth", "goal": 110, "note": "The root and the fifth hold up every song." },
    { "exercise": "bass-walking-blues", "goal": 120 }
  ]
}
```

## The fields

- **`version`**: 1.
- **`name`**, **`description`**, **`style`**: what the Course list shows.
- **`instrument`**: `guitar` or `bass`; every exercise in it is for the
  same instrument.
- **`steps`**: in order. Each has:
  - `exercise`: the exercise's file name less `.exercise.json`: yours in
    `~/Music/Cantina/Exercises` first, then the stock ones. An exercise
    may be in a course once.
  - `goal`: the tempo, 30 to 300, a whole pass must be played at before
    the next step. It usually sits on the exercise's own tempo ladder.
  - `note`: optional, a line on why the step comes here.

A step is done when the [practice log](/reference/schemas/practice-log/)
has a best tempo for its exercise at or above the goal. The next step is
the first one not done.

## Where courses go

| Folder | Holds |
| --- | --- |
| `courses/` in the app | the stock courses, read-only |
| `~/Music/Cantina/Courses/` | yours: a file written here is read within a second |

The stock courses are *Beginner guitar*, *Rhythm guitar*, *Metal rhythm*,
*Metal lead* and *Bass foundations*.

## Checking one

```sh
cantina-cli check metal-rhythm.course.json
```

An exercise that is not installed or is for the other instrument fails
the check; a goal outside the exercise's tempo ladder is a warning.
