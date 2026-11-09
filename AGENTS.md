# AGENTS.md — cantina

Guidance for AI agents working in this repository.

## What this is

A Mac desktop studio for guitar and bass players: a live pedal chain,
multitrack recording, drums, Audio Unit and CLAP hosting, a pedal designer
and a stage of visualizer scenes. A Rust engine on CoreAudio sits under a
React UI, joined by a Tauri 2 shell. macOS only. `PLAN.md` holds the
decisions and the milestones; the architecture lives on the docs site, in
`docs/src/content/docs/architecture/`, next to the code it describes.

## Layout

- `crates/engine` — the CoreAudio stream (`stream.rs`, `device.rs`), the
  real-time graph runtime (`graph.rs`, `runtime.rs`: graphs compiled off
  the audio thread, swapped in at a block boundary), transport and tempo
  map, metronome, WAV playback and recording on streaming threads
  (`player.rs`, `recorder.rs`), meters, analysis frames, the drum
  sequencer, latency compensation. `examples/`: `passthrough`,
  `measure_latency`, `make_irs`.
- `crates/dsp` — pure audio processing, no I/O: the `Processor` trait
  (`processor.rs`), the stock pedals, the pedal JSON interpreter
  (`pedal/`), the drum kit format and sampler, kit recipes, the offline
  renderer, and the JSON Schemas behind the `schema` feature
  (`schema.rs`). `tests/golden.rs` holds every stock pedal's sound.
  `examples/make_takes` renders the transcription reference takes.
- `crates/hosting` — Audio Units (`au.rs`) and CLAP (`clap.rs`) as
  `Processor`s, editor windows, the scanner (`scan.rs`, run in a child
  process).
- `crates/project` — the project model, `Command`s with inverses
  (`command.rs`), history and the journal (undo, redo and crash recovery
  are one mechanism), the versioned file format with migrations
  (`file.rs`), pattern files.
- `crates/cli` — `cantina-cli`: `check`, `render` (pedals, rigs,
  patterns), `kinds`, `schema`, `synth-kit`; `rig.rs` builds a rig's chain
  for the CLI and the app. The binary is `cantina-cli` because the app's is `cantina`.
- `crates/clap-fixture` — a tiny CLAP plugin the hosting tests load.
- `src-tauri/` — the shell. `engine.rs` owns the engine and the open
  project and applies every edit through `Engine::apply`; `commands.rs` is
  the Tauri commands; `library.rs` the watched folders under
  `~/Music/Cantina`; `frame.rs` the binary analysis channel.
- `ui/` — React 19 + TypeScript + Tailwind v4. `ui/src/lib/engine.ts` is
  the client for the shell's commands and the stand-in engine `npm run dev`
  runs against. `ui/src/lib/pedal-kinds.json` is the node catalog the
  designer reads, equal to `cantina-cli kinds --json` (a test holds them
  equal). `ui/src/skins/` holds Cantina's skin tokens and materials.
- `packages/scenes` — `@cantina/scenes`, the visualizer scenes, shared
  with music-visualizer. `packages/skins` — `@cantina/skins`, the six
  skins' shared palette.
- `pedals/`, `kits/`, `irs/`, `recipes/`, `rigs/` — stock pedals (JSON),
  drum kits, cabinet impulse responses, kit recipes, rigs (chains of stock
  pedals for a style, `*.rig.json`). `takes/` holds the
  hand-written ground truth for the transcription reference takes. Third-party audio is credited in
  each folder's `CREDITS.md`.
- `schemas/` — JSON Schemas for pedals, kits, kit recipes, patterns, tabs,
  rigs and projects, generated from the Rust types. Do not hand-edit.
- `.claude/skills/cantina-author/` — the skill for writing Cantina files.
  `references/formats.md` and `references/node-catalog.md` are generated
  by `scripts/skill-references.mjs`; do not hand-edit them.
- `docs/` — the docs site (Astro + Starlight), its own npm project.
  Guides, architecture, reference. `docs/screenshots/` holds the PR
  screenshots `scripts/screenshot.sh` captures.
- `scripts/` — colour and contrast checks, the scene harness, the
  screenshot script, the skill's reference generator and fixture runner.

## Rules

- **The audio thread is sacred.** Code that runs in the audio callback
  never allocates, locks, blocks or does I/O, and never calls anything
  that might: no `Vec::push` past capacity, `Box::new`, `String`,
  `format!`, `Mutex`, logging, `println!`, channel `recv` or file access.
  Allocate in `prepare` or the constructor; `process` only indexes. Cross
  threads with `rtrb` rings and atomics only. A failure in the callback is
  a counter, never a panic or a log line. A change that breaks this can
  sound fine on a fast machine and click on a slower one. Read
  `docs/src/content/docs/architecture/real-time.md` before touching
  `crates/engine`, `crates/dsp` or `crates/hosting`.
- **Graph edits are built off the audio thread.** Edit a `Graph`, compile
  it with every buffer preallocated, hand it over through the queue. The
  old schedule comes back and is dropped on the UI thread.
- **Every continuous parameter is smoothed** through a `Smoother`, so a
  step change never clicks.
- **One abstraction for every pedal.** Stock pedals, designed pedals and
  plugins all implement `Processor`. The chain, mixer, latency
  compensation and project file never ask which kind they hold; keep it
  that way.
- **Plugins get only what their format calls real-time safe** on the audio
  thread: CLAP's `process` and its events, an AU's render call. State,
  editors and parameter info go through the plugin's handle on the UI
  thread.
- **Every project edit is a `Command`** with an inverse, applied through
  `Engine::apply`, so it is undoable, journalled and reconciled into the
  live graph. Never change the project or the graph around it. Adding a
  shell command: the `Engine` method, a `#[tauri::command]` wrapper in
  `commands.rs` registered in `lib.rs`, and the client in
  `ui/src/lib/engine.ts` **including the browser stand-in**, so
  `npm run dev` and the screenshots keep working.
- **Audio files are never modified.** A project is a directory; edits are
  non-destructive.
- **File formats are versioned.** A change to `project.json` bumps its
  `version` and adds a migration in `crates/project/src/file.rs`; old
  files must still load. A change to any format regenerates the schemas
  and the skill's references (below), and the docs' format reference.
- **A stock pedal's sound is pinned by golden files.** If a change alters
  it on purpose, regenerate them and say so in the PR; if not, the change
  is wrong.
- **Every public Rust item has a doc comment** (`missing_docs` is
  enforced through `cargo doc` with `-D warnings`).
- **Colours are skin tokens.** Components never carry a literal colour,
  radius, shadow or typeface; they use a token through a Tailwind or
  material utility (`surface-window`, `surface-button`, `surface-lcd`).
  `npm run lint` fails on a literal colour outside the skin files and on
  any skin below the contrast ratios. Every skin must define every token.
- **The docs stay true.** A change a user or contributor would notice
  updates its guide or architecture page in `docs/src/content/docs/` in the
  same PR. Pages are Markdown with a `title` and a `description`; the docs
  build fails on a broken link.
- **One issue per pull request**, closing it, on a milestone. Agents
  follow the loop in [Working a milestone](#working-a-milestone).
- **UI changes include screenshots**, taken with
  `scripts/screenshot.sh <name> "<query>"` against the stand-in engine
  (name them `m<milestone>-<topic>`, as the existing ones are), across
  more than one skin when the look changes.

## Commands

```sh
npm install
npm run tauri dev        # the app, rebuilding as the code changes
npm run dev              # the UI alone in a browser, against the stand-in engine
npm run tauri build      # target/release/bundle/macos/Cantina.app

npm run check            # Biome, colour and contrast checks, TypeScript, Vitest
npm run test:scenes      # draws every scene in headless Chrome
npm run format           # biome check --write

cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

cargo run -q -p cantina-cli -- check pedals/fuzz.pedal.json
cargo run -q -p cantina-cli -- render pedals/fuzz.pedal.json
```

Regenerating what is generated:

```sh
UPDATE_GOLDEN=1 cargo test -p cantina-dsp --test golden   # a stock pedal's sound changed on purpose
UPDATE_GOLDEN=1 cargo test -p cantina-cli rigs             # a stock rig's level changed on purpose
UPDATE_SCHEMAS=1 cargo test -p cantina-project schemas    # a file format changed
node scripts/skill-references.mjs                         # after the schemas or the node catalog
UPDATE_SHORTCUTS=1 npx vitest run shortcuts               # after a shortcut or menu item changed
scripts/skill-fixtures.sh                                 # every skill example checks and renders
```

The docs site: `cd docs && npm install && npm run dev`; `npm run build`
there before finishing a docs change.

CI (`.github/workflows/ci.yml`) is the gate: it runs everything, the UI
and Rust jobs in parallel, and a pull request merges only when it is
green. Locally, before pushing, run the checks for what the change
touches rather than the whole suite:

- **Any UI change**: `npm run check`.
- **Rust**: `cargo fmt --all`, clippy and the tests of the crates you
  changed (`cargo clippy -p <crate> --all-targets -- -D warnings`,
  `cargo test -p <crate>`); a change to a public item, `cargo doc` for
  that crate with `RUSTDOCFLAGS="-D warnings"`.
- **A format or the schemas**: regenerate as below, then
  `node scripts/skill-references.mjs --check` and
  `scripts/skill-fixtures.sh`.
- **Scenes** (`packages/scenes`): `npm run test:scenes`.
- **Docs**: `npm run build` in `docs/`.

Leave the rest to CI, and fix it on the branch if CI finds something.
Audio you cannot hear: prove a DSP change with tests and `cantina-cli
render` numbers (peak, RMS, DC, latency), not by assertion.

## Working a milestone

Coding agents work one milestone at a time, one issue at a time, and
finish each issue before starting the next:

1. **Pick the next open issue** in the milestone (`gh issue list
   --milestone "<name>"`), lowest number first unless one depends on
   another. Read it and its acceptance criteria, including the docs.
2. **Branch in a worktree** from an up-to-date `main`:
   `git fetch && git worktree add ../cantina-wt/<topic> -b
   m<milestone>/<topic> origin/main`. Work only in that worktree.
3. **Do the work** to the rules above: code, tests, docs and, for UI
   changes, screenshots.
4. **Run the checks for what changed** (above) locally until they pass;
   CI runs the rest.
5. **Open a pull request** that says `Closes #<issue>`, on the issue's
   milestone, in the shape the recent ones use (What, How, Proof,
   Screenshots). Say plainly what was not tested, such as on hardware.
6. **Wait for CI** (`gh pr checks --watch`). If it fails, fix it on the
   branch and push; never merge red.
7. **Squash-merge** (`gh pr merge --squash --delete-branch`), which
   closes the issue and deletes the remote branch.
8. **Clean up**: `git worktree remove ../cantina-wt/<topic>`, delete the
   local branch, and `git pull` on `main`.
9. **Move to the next issue.** When the milestone has none left, close
   it and stop for the next milestone.

Stop and ask a person instead of guessing when an issue needs a decision
it does not make, when a change would alter a stock pedal's sound or a
file format beyond what the issue asks, or when CI fails for a reason
outside the change.
