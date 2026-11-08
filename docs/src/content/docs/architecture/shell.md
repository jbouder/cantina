---
title: "Shell"
description: "The Tauri 2 shell in src-tauri: it owns the engine and the open project, turns UI commands into edits, and streams frames back to the UI."
sidebar:
  order: 9
---

`src-tauri/` is a Tauri 2 app that owns the engine handle and the open project, and exposes both to the React UI. The engine never reaches the web view directly.

## Commands in

The UI calls Tauri commands (`commands.rs`). Each one locks the shell's `Engine` for one call, does its work and returns the status the UI should show. Every edit to the project is a [`Command`](/architecture/project/#commands) applied through `Engine::apply`, which records it for undo, appends it to the journal, and brings the live graph in line: a parameter change goes over the audio thread's queue, and anything structural reconciles the graph's tracks and pedals against the project, rewires and commits a new schedule. Nodes that survive keep their state.

The project is the source of truth. A stream rebuild, after a device change or a new sample rate, drops the whole graph and builds it again from the project.

## Frames out

While a stream is open, a pump thread wakes 60 times a second, reads the meters, pedal readouts and transport, runs the analysis, and sends one binary [frame](/architecture/frames/) to each page that has drawn the last one. Status changes the engine makes on its own, such as a device going away, are pushed as events.

## What else it owns

- **Settings** (`settings.rs`): devices, sample rate, buffer size, the latency warning, editor window positions, the metronome and the recent projects, kept in `~/Library/Application Support/com.jbouder.cantina/settings.json`.
- **Projects** (`project.rs`): where projects live and opening, creating, renaming, duplicating and recovering them.
- **Designed pedals** (`pedals.rs`): the user's library in `~/Music/Cantina/Pedals` and the copies a project keeps.
- **Plugin editors**: native windows opened, ticked and closed on the main thread. See [hosting](/architecture/hosting/#editor-windows).
- **The menu bar** (`menu.rs`): built from the action registry the UI shares. See [menus and shortcuts](/architecture/menus/).
- **The plugin scanner**: the app binary run with `--scan-plugins` is the scanner's [child process](/architecture/hosting/#the-scanner).

## Adding a command

1. Add the method to `Engine` in `engine.rs`, going through `apply` if it edits the project.
2. Add a `#[tauri::command]` wrapper in `commands.rs` and register it in `lib.rs`.
3. Add it to the UI's engine client in `ui/src/lib/engine.ts`, including the browser stand-in, so `npm run dev` and the screenshots keep working.
