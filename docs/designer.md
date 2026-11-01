# Pedal designer

The designer edits a pedal in the pedal format
([pedal-format.md](pedal-format.md)) while it runs in a chain, so every
change is heard on the input as it is made. **New design…** in the
pedal picker (the **+** slot at the end of a chain) starts a new pedal
(an input wired to an output) at the end of the chain and opens it;
**Edit…** on a designed pedal's card opens that one. Any stock pedal that ships in the format can be loaded as a
starting point from **Start from**.

## What is on screen

- **The canvas** (`ui/src/components/Designer.tsx`): one card per
  node, dragged to arrange (arrow keys move the focused one, Shift for
  single pixels). A card's right-hand port starts a wire; the next
  input port clicked ends it. Wires are the format's edges; dashed
  lines are signal bindings, labelled with the field they drive. An
  `lfo` has no output port, since it carries no audio: a field follows
  it from the side panel. Delete removes the selected node or wire.
- **The side panel**: the pedal's name; *Start from*; the problems the
  engine reported, if any; the selected node's id and fields; the
  parameters.
- **Fields** come from the kind catalog (`ui/src/lib/pedal-kinds.json`,
  written by a test in `crates/dsp/src/pedal/kinds.rs` from the one
  source, which fails when it is stale). Each field has a *source*:
  default, a constant (a fader, a pick or a switch), a parameter (with
  a map of the parameter's range onto the field, linear or log, or for
  a choice parameter a value per choice), or for modulatable fields a
  signal from another node with a map.
- **Parameters** are the pedal's knobs: id, label, unit, range and
  default, or choices, or a toggle. Each shows its live fader too, which
  sets the running pedal's value directly (`chain_set_param`), as the
  chain's card does.

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

## Saving, the library and sharing

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
