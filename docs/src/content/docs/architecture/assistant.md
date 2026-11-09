---
title: "Assistant"
description: "How the optional chat assistant connects to a provider: an app setting, a key kept in the Keychain, and every request made from the shell so the key never reaches the web view."
sidebar:
  order: 9.7
---

The assistant is an opt-in chat with a model the player connects. It lives
in `src-tauri/src/assistant/`; the guide for players is
[The assistant](/guides/assistant/).

## Off by default

`AssistantSettings` in `settings.rs` holds `enabled`, `endpoint` and
`model`. It is an app setting like the metronome: in `settings.json`, not
in a project, and not undoable. While `enabled` is false the UI renders no
chat and every assistant command returns the `off` problem before any
request is built: `assistant::connection` checks the flag first.

## Where the key lives

The API key is a generic password in the login Keychain, service
`com.jbouder.cantina.assistant`, account `api-key` (`assistant/keychain.rs`,
through `security-framework`). It is read on first use and cached in
memory for the life of the process.

The key goes one way. The Settings field sends it once to
`assistant_set_key`, which writes it to the Keychain, and the field
clears. No command returns it: `EngineStatus.assistant` carries `hasKey`
and nothing else about it. It is never in the settings file, a project, the
journal or a log, and `Connection`'s `Debug` prints `…` in its place.

## Why requests go out from the shell

Every request is made in Rust (`assistant/provider.rs`, on `reqwest` with
the same rustls build the updater uses), not with `fetch` in the web view:

- **The key stays out of the web view.** A request made in the page would
  need the key in the page, where any script it runs could read it, and
  where it could end up in the web inspector or a crash report.
- **No CORS.** Most providers do not allow browser origins; a local
  Ollama or LM Studio usually does not either.
- **One place to classify failures.** `classify` turns an HTTP status and
  the provider's error body into a `Problem` the UI shows as it is: a bad
  URL, a bad key, an unknown model, a rate limit, a server error or no
  network.

The shell's `Engine` lock is held only to read the settings; the request
itself runs on Tauri's async runtime without it, so audio and editing
carry on while a provider is slow.

## The API

Any OpenAI-compatible chat completions API: `GET {endpoint}/models` lists
models and `POST {endpoint}/chat/completions` talks. Test connection lists
the models, checks the chosen one is among them, and, for an endpoint with
no list, sends a one-token chat instead. For `api.anthropic.com` the key
also goes in `x-api-key`.

## The conversation

`assistant/chat.rs` holds the thread and the loop. The thread is a list
of `Item`s, the same list the panel draws: the player's messages, the
model's replies, tool calls with their one-line summary and result, and
failures. It is kept in `assistant-thread.json` in the app's data folder,
rewritten (atomically) after every item, so it survives a relaunch and a
crash.

`converse` answers the last message:

1. Build the request from the thread: the system prompt, then each user
   message, each reply with the tool calls after it as one assistant
   message, and each call's result as a `tool` message. Failures are never
   sent; only the last `CONTEXT_ITEMS` are, cut at a user message.
2. Stream the answer (`Connection::chat`, server-sent events put back
   together by `StreamState`), passing text to the panel as it comes.
3. Run each tool the model called. A destructive tool first sends a
   `Confirm` event and waits for `assistant_confirm`; a tool that fails,
   or arguments that are not JSON, go back to the model as the result so
   it can try again.
4. Ask again, until a step calls no tool, a request fails, the player
   presses Stop, or `MAX_STEPS` is reached.

The panel gets `Text`, `Item`, `Confirm` and `Done` events over a Tauri
channel. Stop cancels through a `Cancel` that the stream and any pending
confirmation `select!` on; the reply written so far is kept and marked
stopped. One conversation runs at a time (`Session`).

A tool call that edits the project records the project's revision after
the edit. The card's Undo calls `assistant_undo`, which undoes only while
the project is still at that revision, so it can never undo something
else; otherwise Edit ▸ Undo steps back through the history as usual.

`converse` takes a `Model` and a `Toolbox`, so tests drive it with a
scripted model (`chat::script`) and no network or engine.

## The tools

`assistant/tools.rs` is the registry. Every tool is one entry in `TOOLS`:
a name, a description for the model, a JSON Schema for its arguments, an
optional question for the player, and the function that runs it.
`specs()` turns the table into the model's function definitions, and
`run()` checks the arguments against the compiled schema before calling
the tool, so a malformed call never reaches the project.

| Reads | Edits | Asks first |
| --- | --- | --- |
| `get_project`, `get_pattern`, `list_pedal_kinds`, `list_kits`, `get_engine` | `write_pedal`, `write_pattern`, `write_kit_recipe`, `set_tempo`, `add_track`, `add_drum_track`, `rename_track`, `set_mix`, `arm_track`, `add_pedal`, `remove_pedal`, `move_pedal`, `set_pedal_params`, `bypass_pedal`, `add_pattern`, `set_pattern`, `place_pattern`, `set_loop` | `remove_track`, `delete_pattern`, `delete_clip`, and a write that replaces a library file |

Tools run against a `Host`: the project, its revision, `apply`, and the
few things only the engine knows (pedal kinds and their parameters, the
kits and their pads, adding a pedal or a drum track, removing a track,
the engine report). `Engine` implements it in `engine.rs` with the same
methods the UI's commands call:

- **Every edit is a `Command` through `Engine::apply`**, so it is
  undoable, journalled and reconciled into the live graph, and the status
  is pushed to the window as for any edit. Nothing in `tools.rs` touches
  the graph or the project directly.
- **One tool call is one undo step.** A tool that changes several things
  (`set_mix`, or `add_pedal` with its knobs) builds one `Command::Batch`
  or one `InsertPedal` with its values, never several applies.
- **A failure is a result, not a crash.** An unknown id, a parameter that
  does not exist, a schema mismatch or an engine refusal comes back to the
  model as `{"error": …}`, naming what was wrong (and, for a parameter,
  what the pedal has), so it can call again.
- **Values are held to range.** Parameters are set by name in their own
  unit, a choice by its name, and clamped by `ParamInfo::clamp` as a knob
  would be.

### Authoring

`write_pedal`, `write_pattern` and `write_kit_recipe` bring the
cantina-author skill into the app (`assistant/author.rs`). The system
prompt includes `author::reference()`: the skill's generated
`formats.md` and `node-catalog.md`, its `design-rules.md` and its
examples, compiled in with `include_str!`, so they change when the
formats do (`scripts/skill-references.mjs --check` already fails CI on a
stale reference).

`author::write` drafts the file in a scratch folder and runs the
`cantina-cli` library's own code on it: `check`, then `render::render_pedal`
over the DI (judged: no clipping, not silent, no NaN, DC under 0.001; the
level is a warning), `render::render_pattern` through the kit, or
`synth::synth_kit` and a kit check. Only then is it written to the library
folder, atomically, and the library rescanned; nothing is saved on any
failure. A pedal or pattern is overwritten only with `replace`, which
asks the player first; a kit is never replaced, so its samples are never
touched. `HostTools` counts failed writes per message and refuses after
`author::MAX_TRIES`, telling the model to stop and explain.

Rendering and synthesis run under the engine lock, since they are tools;
they take tens of milliseconds and never touch the audio thread.

`HostTools` is the loop's `Toolbox` over the registry. It locks the
engine for each call only (the `Lock` trait: the `AppHandle` in the app,
a mutex around `tools::fake::FakeHost` in tests), so playback and editing
carry on while the model writes. The system prompt gets a one-line
summary of the project (`brief`) on every step.

## Testing

`provider.rs` tests run against a one-thread HTTP server on localhost
(`provider::fake`) that answers from a closure: success, a refused key, an
unknown model, a wrong path, a closed port, and an endpoint with no model
list; and a streamed chat, fed a byte at a time. `chat.rs` tests run the
loop against a scripted model: streaming into the thread, tools and their
results going back, bad arguments recovered from, a confirmation answered
both ways, a failure and Retry, Stop mid-reply, a runaway model stopped,
and the thread file across a relaunch. `tools.rs` tests run every tool
on `FakeHost` (a project with a history, no engine): edits as single undo
steps with their cards, reads that change nothing, mistakes that change
nothing and say why, patterns written, read, placed and looped, and the
questions destructive tools ask. `assistant/tests.rs` puts it together: a
scripted model reading the project and building a chain through the real
loop and the real tools, fixing a refused call, and a removal waiting on
the player. The browser stand-in in `ui/src/lib/engine.ts` has a fake provider
at `https://droids.example/v1` so `npm run dev` and the screenshots work;
`?assistant` starts with it connected, and `?chat` opens the panel with
a conversation in it.
