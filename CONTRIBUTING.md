# Contributing

Thanks for helping. Cantina is built for one guitar player first, and this
page is how a second person gets from a fresh clone to a merged pull
request. The [docs site](https://cantina-docs.vibeproject.workers.dev/)
has the guides and the [architecture](https://cantina-docs.vibeproject.workers.dev/architecture/)
pages, which say where everything lives and why.

## Setup

You need a Mac on macOS 13 or later, [Rust](https://rustup.rs) (stable,
with `~/.cargo/bin` on your `PATH`), Node 22 or newer, and Google Chrome
for the scene tests and screenshots.

```sh
git clone https://github.com/jbouder/cantina.git
cd cantina
npm install
npm run tauri dev        # the app, rebuilding as the code changes
```

- `npm run dev` serves the UI alone in a browser against a stand-in
  engine (`ui/src/lib/engine.ts`), which is enough for layout and skin
  work. Query parameters set it up: `?running&take&drums`, `?skin=neon`,
  `?settings` and more, listed in that file.
- The engine runs without the app too; see
  [try it without the app](https://cantina-docs.vibeproject.workers.dev/architecture/engine/#try-it-without-the-app).
- `cantina-cli` checks and renders pedals, patterns and kits from the
  command line: `cargo run -q -p cantina-cli -- render pedals/fuzz.pedal.json`.
- The docs site is its own npm project in `docs/`:
  `cd docs && npm install && npm run dev`.

`AGENTS.md` is the short version of this page, for coding agents and for
anyone who wants the rules on one screen.

## The real-time rules

Read [the real-time page](https://cantina-docs.vibeproject.workers.dev/architecture/real-time/)
before touching `crates/engine`, `crates/dsp` or `crates/hosting`. In
short:

- **Code on the audio thread never allocates, locks, blocks or does
  I/O**, and never calls anything that might: no `Vec::push` past
  capacity, `Box::new`, `String`, `format!`, `Mutex`, logging,
  `println!`, channel `recv` or file access. Allocate in `prepare` or
  the constructor; `process` only indexes.
- **Threads talk through `rtrb` rings and atomics.** A failure in the
  callback is a counter, never a panic or a log line.
- **Graph edits are built off the audio thread**, compiled with every
  buffer preallocated, and handed over through a queue; the old schedule
  comes back to be dropped on the UI thread.
- **Every continuous parameter goes through a `Smoother`**, so a step
  never clicks.
- **Plugins get only what their format calls real-time safe** on the
  audio thread; state, editors and parameter info go through their
  handle on the UI thread.

A change that breaks these can sound fine on a fast machine and click on
a slower one, so review looks for them first.

## The other rules

- **Every project edit is a `Command`** with an inverse, applied through
  `Engine::apply`, so it is undoable, journalled and reconciled into the
  live graph. A new shell command needs the `Engine` method, a
  `#[tauri::command]` in `src-tauri/src/commands.rs` registered in
  `lib.rs`, and the client in `ui/src/lib/engine.ts` with its browser
  stand-in.
- **One abstraction for every pedal**: stock pedals, designed pedals and
  plugins all implement `Processor`.
- **Audio files are never modified.** Edits are non-destructive.
- **File formats are versioned.** A change to `project.json` bumps its
  version and adds a migration; old files must still load.
- **Colours are skin tokens.** Components never carry a literal colour,
  radius, shadow or typeface.
- **Type and spacing are scales.** Four type sizes (`text-label`,
  `text-body`, `text-readout`, `text-display`) and eight spacing steps.
  Buttons come in three tiers with at most one primary per view, states
  come from ARIA, captions are the only capitals, and an editable field
  never uses the display material. The rules are in the docs under
  architecture, "The UI", and `npm run lint` checks them.
- **Every public Rust item has a doc comment.**
- **The docs stay true.** A change someone would notice updates its
  guide or architecture page in the same pull request.

## Adding a menu item or a shortcut

Menu items and keyboard shortcuts come from one registry,
`ui/src/lib/actions.json`. Add the action there and put it in a menu, then
bind it in `App.tsx`; the shell builds the native menu from the same file.
[Menus and shortcuts](https://cantina-docs.vibeproject.workers.dev/architecture/menus/)
has the details and the rules the tests hold it to.

## Testing

CI (`.github/workflows/ci.yml`) runs all of this on every pull request.
Run it first:

```sh
npm run check                                  # Biome, colour and contrast checks, TypeScript, Vitest
node scripts/skill-references.mjs --check      # the agent skill's generated references are current
npm run test:scenes                            # every visualizer scene draws, in headless Chrome
npm run build                                  # the UI; the shell embeds ui/dist
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/skill-fixtures.sh                      # every skill example checks and renders
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
(cd docs && npm run build)                     # for a docs change; fails on a broken link
```

Audio you cannot hear in review is proved with numbers. A DSP change
comes with tests and `cantina-cli render` figures (peak, RMS, DC,
latency), not a description of how it sounds.

Some files are generated. Regenerate them, never edit them by hand:

```sh
UPDATE_GOLDEN=1 cargo test -p cantina-dsp --test golden   # a stock pedal's sound changed on purpose
UPDATE_GOLDEN=1 cargo test -p cantina mixdown             # the mixdown changed on purpose
UPDATE_SCHEMAS=1 cargo test -p cantina-project schemas    # a file format changed
node scripts/skill-references.mjs                         # after the schemas or the node catalog
UPDATE_SHORTCUTS=1 npx vitest run shortcuts               # after a shortcut or menu item changed
```

A golden file that changes says so in the pull request, with why. If you
did not mean to change a sound, the change is wrong.

## Issues and pull requests

- **One issue per pull request**, closing it, on the issue's milestone.
  Issues say what and why (summary, motivation, acceptance criteria,
  what is out of scope); the templates ask for exactly that.
- **Branch from `main`**; pull requests are squash-merged once CI passes.
  Coding agents follow the per-issue loop in `AGENTS.md` (Working a
  milestone): a worktree and branch per issue, merged and removed before
  the next.
- **UI changes include screenshots**, taken with
  `scripts/screenshot.sh m<milestone>-<topic> "<query>"` against the
  stand-in engine, across more than one skin when the look changes. They
  go in `docs/screenshots/` and are linked from the pull request.
- **Docs changes** go in `docs/src/content/docs/`; each page has a
  `title` and a `description`, and the pull request gets a preview.

Releases are cut by a maintainer from a tag; [Releasing](https://cantina-docs.vibeproject.workers.dev/architecture/releasing/)
says how.

## License

By contributing you agree that your contribution is licensed under the
[MIT License](LICENSE).
