# Cantina

A Mac desktop studio for guitar and bass players: plug in, play through a
pedal chain with no perceptible delay, layer tracks, build drum beats, run
Audio Unit and CLAP plugins, design your own pedals, and play on a stage of
music visualizer scenes.

**[Documentation](https://cantina-docs.vibeproject.workers.dev)** ·
[Getting started](https://cantina-docs.vibeproject.workers.dev/guides/getting-started/) ·
[Architecture](https://cantina-docs.vibeproject.workers.dev/architecture/) ·
[Contributing](CONTRIBUTING.md)

## Status

In development, with no packaged release yet. The studio, pedals, plugins,
drums, the pedal designer and the stage all work; packaging and signing are
next. Progress is tracked in the
[milestones](https://github.com/jbouder/cantina/milestones).

## Build and run

macOS 13 or later, [Rust](https://rustup.rs) and Node 22.

```sh
npm install
npm run tauri dev      # the app, reloading as the code changes
npm run tauri build    # target/release/bundle/macos/Cantina.app
```

## Test

```sh
npm run check          # UI: Biome, TypeScript and tests
cargo test --workspace # engine, DSP, hosting and project
```

## License

[MIT](LICENSE)
