---
title: "Real-time rules"
description: "The rules every line of code on the audio thread follows, how the engine keeps them, and the habits that make them easy to keep."
sidebar:
  order: 1
---

The audio callback has a few milliseconds to fill each buffer, and a missed deadline is a click the player hears. So code that runs on the audio thread follows four rules, with no exceptions:

1. **No allocation.** No `Vec::push` past capacity, no `Box::new`, no `String`, no `format!`, nothing that might reach the allocator.
2. **No locks.** No `Mutex`, no `RwLock`, nothing that can wait on another thread. A `try_lock` that never fails because nothing else takes the lock is allowed (see `NodeSlot` in the engine's graph).
3. **No blocking or I/O.** No files, no sockets, no logging, no `println!`, no channel `recv`, no sleeping.
4. **No calls into anything that might do the above.** That includes most of the standard library's collections, and any plugin call outside its processing function.

Read these before your first pull request. A change that breaks them may sound fine on a fast machine and fail on a slower one.

## How the engine keeps them

- **Graph edits are built off the audio thread.** The UI thread edits a `Graph`, compiles it into a schedule with every buffer preallocated, and hands it over through a queue. The audio thread swaps it in at a block boundary. The old schedule goes back the same way and is dropped on the UI thread.
- **Parameters travel through lock-free queues.** A knob change is a `ParamChange` pushed onto an `rtrb` ring and applied at the start of the next block.
- **Parameters are smoothed on the audio thread.** Every continuous parameter goes through a `Smoother`, so a step change never clicks.
- **Data comes out through atomics and rings.** Meters, scope samples, readouts and counters are written by the audio thread into atomics or a ring, and read by the UI thread whenever it likes.
- **Disk work has its own threads.** Playback reads ahead and recording writes behind on streaming threads, with rings between them and the callback.
- **The capture ring is allocated before play and only written.** A jam keeps the last minutes of its input, dry and through the rig, in a `CaptureRing`: every sample of those minutes allocated when the jam opens (or its length changes), off the audio thread, before the `Capture` nodes that fill it reach a graph. The audio thread does one relaxed atomic store per sample and counts the frames it wrote; it never waits. Keep that hands a `KeepJob` to the input's recorder's writer thread, which copies the span out to WAV files while the ring goes on filling, and checks as it goes that the ring has not come round over what it copied, leaving a block's room for the frames the audio thread is writing before it counts them. Where the transport started rolling is kept beside the samples under a seqlock, so the take can be put back on the bars it was played on.

## Habits that make it easy

- **Allocate in `prepare` or the constructor; `process` only indexes.** Scratch buffers, plugin buffer lists and event buffers are allocated with a capacity and cleared, never dropped and rebuilt, on the audio thread.
- **Cross threads with `rtrb` rings and atomics only.** Audio, parameter changes and counters all travel that way.
- **Treat a failure in the callback as a counter, not a return path.** The stream never panics or logs from the callback. A plugin that fails to render is bypassed and its failure counted.
- **Measure it.** `RuntimeStats`, `PlayerStats`, `RecorderStats` and the stream's `Stats` count blocks, underruns, dropped frames and failures. The passthrough example prints them when you type `s`.

## The plugin exception

Third-party plugins run on the audio thread too, and Cantina cannot check what they do. Plugin wrappers follow the rules themselves, and pass a plugin only what its format promises is real-time safe: CLAP's `process` and the parameter events delivered with it, and an Audio Unit's render call. Everything else a plugin offers, such as state, editors and parameter info, goes through the plugin's [handle](/architecture/processor/#the-handle) on the UI thread.
