---
title: "Pedal designer"
description: "How the designer previews a pedal graph live in the chain, where designs are stored, and how to work on it in a browser."
sidebar:
  order: 10.5
---

The designer edits a pedal in the [pedal format](/reference/pedal-format/)
while it runs in a chain. Using it is in the
[designer guide](/guides/designer/); this page is how it works.

The canvas is `ui/src/components/Designer.tsx`. Its node menus and fields
come from the kind catalog, `ui/src/lib/pedal-kinds.json`, which a test in
`crates/dsp/src/pedal/kinds.rs` writes from the one source and fails when it
is stale.

## How the preview works

A designed pedal in a chain has the spec `pedal:<id>`. The shell keeps
the definitions in a map by id (`designs` in `src-tauri/src/engine.rs`),
and `build_processor` compiles one for that spec as it would build a
stock pedal or a plugin. `designer_new` puts an empty definition in the
map and adds the pedal to the chain like any other (an undoable
`InsertPedal`).

Every edit sends the whole definition and the parameter values to
`designer_update`. The shell parses and compiles it first. If that
fails, nothing changes: the problems come back, each naming the node
and field it is about, the panel lists them and the cards they concern
are outlined, and the last valid pedal keeps playing. If it compiles,
the definition replaces the old one, the catalog forgets what it knew
about the spec, the values (kept by parameter id across a change of
shape, defaults for new ones) go in as a `SetPedalValues` edit, and the
pedal's node is dropped from the live graph so the next sync builds it
again from the new definition: the swap lands on the next block
boundary, which is what "heard within one block" means here. Knob
moves on their own do not rebuild anything; they are parameter changes
over the audio thread's queue.

## The library and projects

**Save to library** writes the definition to `~/Music/Cantina/Pedals/`
(anything `CANTINA_PEDALS` names is listed too) as `<id>.pedal.json`,
with the id made from the pedal's name (`Warm Drive` becomes
`warm-drive`), and points a draft's chain entry at that id (an
undoable `SetPedalSpec`). Library pedals appear in the picker under
*Your pedals* with a *yours* badge; adding one copies its file into
the project's `pedals/` and the project plays from that copy, so a
song keeps sounding the same if the library pedal changes later.
Saving the project writes the copy of every designed pedal its chains
use, drafts included, and opening a project reads them back; a
project that refers to a pedal it has no copy of plays it as a
pass-through named *(missing)* rather than losing the chain.

**Export…** in the designer writes the definition to a file of your
choosing; **Import a file…** in the picker, or dropping a `.pedal.json`
on the window, copies the file into the library (validated; under the next
free id if a different pedal already holds its name) and adds it to
the selected track's chain. The file logic is `src-tauri/src/pedals.rs`,
with tests for the ids, the round trip and the import rules.

## In a browser

The stand-in opens the designer with `?design=<stock name>` (or
`new`), which puts such a pedal on the guitar track; `&select=<node>`
selects a node, `&broken` leaves the overdrive's tone unwired so the
problems show, and `?library` lists two pedals of your own in the
picker. The stand-in validates with `validate` in
`ui/src/lib/pedal.ts`, a copy of the dsp crate's structural rules with
the same messages, which the tests in `pedal.test.ts` cover.
