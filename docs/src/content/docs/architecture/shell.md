---
title: "Shell"
description: "The Tauri 2 shell in src-tauri: it owns the engine and the open project, turns UI commands into edits, and streams frames back to the UI."
sidebar:
  order: 9
---

`src-tauri/` is a Tauri 2 app that owns the engine handle and the open project, and exposes both to the React UI. The engine never reaches the web view directly.

## The window

`window_state.rs` remembers the main window between launches in
`window.json`, beside `settings.json`. With no file, which is a first
launch, the window opens zoomed to fill the screen, with the title bar
kept. After that it comes back as it was left: zoomed, or at its last size
and place. It never opens full screen: a window left full screen comes
back zoomed. The size and place are tracked only while the window is a
plain window, so unzooming returns to them. A saved size below the minimum is dropped, and so
is a place that is on no screen now, as when a second display was
unplugged. The state is written when the window loses focus and when it
closes.

## Commands in

The UI calls Tauri commands (`commands.rs`). Each one locks the shell's `Engine` for one call, does its work and returns the status the UI should show. Every edit to the project is a [`Command`](/architecture/project/#commands) applied through `Engine::apply`, which records it for undo, appends it to the journal, and brings the live graph in line: a parameter change goes over the audio thread's queue, and anything structural reconciles the graph's tracks and pedals against the project, rewires and commits a new schedule. Nodes that survive keep their state.

The project is the source of truth. A stream rebuild, after a device change or a new sample rate, drops the whole graph and builds it again from the project.

## Frames out

While a stream is open, a pump thread wakes 60 times a second, reads the meters, pedal readouts and transport, runs the analysis, and sends one binary [frame](/architecture/frames/) to each page that has drawn the last one. Status changes the engine makes on its own, such as a device going away, are pushed as events.

## What else it owns

- **Settings** (`settings.rs`): devices, sample rate, buffer size, the latency warning, editor window positions, the metronome, the recent projects, the assistant's endpoint and model, the instrument profile (which trims rigs' input gain) and the last jam, kept in `~/Library/Application Support/com.jbouder.cantina/settings.json`.
- **Projects** (`project.rs`): where projects live and opening, creating, renaming, duplicating and recovering them.
- **Designed pedals** (`pedals.rs`): the user's library in `~/Music/Cantina/Pedals` and the copies a project keeps.
- **Rigs** (`rigs.rs`): the stock rigs in the bundle's `rigs/` and the user's in `~/Music/Cantina/Rigs`. Applying one is a `Command::Batch` of removes and inserts, so it is one undo. Auditioning one builds its pedals with ids no project pedal has and syncs the graph with them in place of the track's chain, without touching the project; every swap fades the track's strip out, commits the new graph and fades it back in, so it never clicks.
- **Jam mode** (`jam.rs`, `engine_jam.rs`): while a jam is open, the graph is synced from a project the jam makes (the input through the jam's rig, and a drum track) instead of the open one, which is not touched; leaving syncs the project back. The jam's timeline only moves forward, with no loop: what plays where is a schedule of section and tempo changes on bar lines. A change goes on the next bar line at least two buffers and 20 ms ahead, as a new drum score and tempo map that differ from the old ones only from that bar on, so it lands on the bar to the sample whenever the swap reaches the audio thread. The last jam is kept in the settings.
- **Practice** (`engine_jam.rs`): practice is jam mode with an exercise. The jam's drums play the verse of the exercise's groove alone, muted when the groove is off; its backing is the exercise's phrase as a `TabLoop` on the tab voice, through the exercise's own rig, round and round or once to the end of the pass going on; its count-in is the exercise's bars. Tempo changes are the jam's, on the next bar line. The last exercise, tempo and options are kept in the settings.
- **Jams** (`jams.rs`): the stock jams in the bundle's `jams/` and the user's in `~/Music/Cantina/Jams`; opening one sets the jam playing, saving writes the jam as it is.
- **Exercises** (`exercises.rs`): the stock exercises in the bundle's `exercises/` and the user's in `~/Music/Cantina/Exercises`, listed by style, what they teach and name.
- **Courses** (`courses.rs`): the stock courses in the bundle's `courses/` and the user's in `~/Music/Cantina/Courses`, each with its steps' bests and the next step from the practice log. The assistant's `get_practice` tool reads them and the log, and never writes.
- **Grooves** (`grooves.rs`): the user's whole drum parts in `~/Music/Cantina/Grooves`, saved by the drum designer and listed in its palette and the jam's groove picker. A jam naming one plays its sections by kind (`GrooveFile::jam_patterns`). The stock grooves stay pattern files, found by `library::scan_grooves`.
- **Library folders** (`library.rs`): `Pedals`, `Kits`, `Patterns`, `Rigs`, `Jams`, `Exercises`, `Courses` and `Grooves` under `~/Music/Cantina`, watched twice a second and rescanned when they change.
- **Plugin editors**: native windows opened, ticked and closed on the main thread. See [hosting](/architecture/hosting/#editor-windows).
- **The menu bar** (`menu.rs`): built from the action registry the UI shares. See [menus and shortcuts](/architecture/menus/).
- **The assistant** (`assistant/`): the optional chat's provider requests and its key in the Keychain. See [assistant](/architecture/assistant/).
- **The plugin scanner**: the app binary run with `--scan-plugins` is the scanner's [child process](/architecture/hosting/#the-scanner).

## Adding a command

1. Add the method to `Engine` in `engine.rs`, going through `apply` if it edits the project.
2. Add a `#[tauri::command]` wrapper in `commands.rs` and register it in `lib.rs`.
3. Add it to the UI's engine client in `ui/src/lib/engine.ts`, including the browser stand-in, so `npm run dev` and the screenshots keep working.
4. If the assistant should be able to do it too, add a tool to `TOOLS` in `assistant/tools.rs` that builds the same `Command`. See [assistant](/architecture/assistant/#the-tools).
