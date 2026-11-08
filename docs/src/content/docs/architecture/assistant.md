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

## Testing

`provider.rs` tests run against a one-thread HTTP server on localhost
(`provider::fake`) that answers from a closure: success, a refused key, an
unknown model, a wrong path, a closed port, and an endpoint with no model
list. The browser stand-in in `ui/src/lib/engine.ts` has a fake provider
at `https://droids.example/v1` so `npm run dev` and the screenshots work;
`?assistant` starts with it connected.
