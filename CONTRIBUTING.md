# Contributing

Thanks for helping. Cantina is a macOS app; you need Rust (via
[rustup](https://rustup.rs)) and Node 22.

## Before your first pull request

Read the [real-time rules](https://cantina-docs.vibeproject.workers.dev/architecture/real-time/).
Code on the audio thread never allocates, locks, blocks or does I/O, and a
change that breaks that can sound fine on your machine and click on
someone else's. The [architecture](https://cantina-docs.vibeproject.workers.dev/architecture/)
pages say where everything lives.

## Checks

CI runs these on every pull request; run them first:

```sh
npm run check                                  # Biome, colour and contrast checks, TypeScript, tests
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Every public Rust item needs a doc comment. A change that alters a stock
pedal's sound regenerates the golden files with
`UPDATE_GOLDEN=1 cargo test -p cantina-dsp --test golden`.

## Pull requests

- One issue per pull request, closing it.
- UI changes include screenshots: `scripts/screenshot.sh <name> "<query>"`
  captures the UI in a browser against the stand-in engine.
- Docs changes go in `docs/` and get a preview URL on the pull request.
