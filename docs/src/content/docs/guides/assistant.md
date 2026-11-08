---
title: "The assistant"
description: "Connect an AI provider to Cantina's optional chat assistant: hosted APIs and local models, where your key is kept, and exactly what is sent off your Mac and when."
---

Cantina has an optional chat assistant. It is **off until you turn it on**,
and while it is off nothing is shown and nothing leaves your Mac. It works
with any service that speaks the OpenAI chat completions API, hosted or
running on your own machine. You bring the model and the key; Cantina has
no account or server of its own.

## Connect a provider

Open **Settings** (the gear at the top right) and scroll to **Assistant**.

![The Assistant section of Settings, connected to a provider](../../../assets/guides/assistant-settings.png)

1. Press **Assistant off** so it reads **Assistant on**.
2. Pick a **Provider**, or choose **Other** and type the **Endpoint URL**
   yourself. It is the API's base address, usually ending in `/v1`.
3. Paste your **API key** and press **Save key**. A local model needs none.
4. Pick a **Model** from the list the endpoint offers. If the endpoint has
   no list, type the model's id.
5. Press **Test connection**.

Test connection says plainly what is wrong, if anything: a URL where
nothing answers, a key the provider refuses, a model it does not have, a
rate limit, or no network.

### A hosted API

| Provider | Endpoint URL | Key |
| --- | --- | --- |
| OpenAI | `https://api.openai.com/v1` | from platform.openai.com |
| Anthropic | `https://api.anthropic.com/v1` | from console.anthropic.com |
| OpenRouter | `https://openrouter.ai/api/v1` | from openrouter.ai |

A hosted provider bills your account for what the assistant sends and
receives, at its usual rates.

### A local model

A model running on your Mac keeps everything on your Mac.

- **Ollama**: install it, run `ollama pull llama3.2` (or any model that
  supports tool calls), and use `http://localhost:11434/v1` with no key.
- **LM Studio**: load a model, start its local server, and use
  `http://localhost:1234/v1` with no key.

Smaller local models answer more slowly and make more mistakes with
Cantina's tools than the large hosted ones.

## Where your key is kept

Your API key goes into the macOS **Keychain**, under
`com.jbouder.cantina.assistant`. It is never written to a project, to
Cantina's settings file or to a log, and once saved it is never shown
again: Settings only says that one is saved. Press **Remove key** to
delete it from the Keychain. You can also see or delete it in the
Keychain Access app.

The requests themselves are made by Cantina's native side, not by its
window, so the key never travels back into the interface.

## What is sent, and when

Nothing is sent while the assistant is off. With it on, Cantina contacts
the endpoint only:

- to list its models, when you open Settings or change the endpoint or key;
- when you press **Test connection**.

Each of these sends your key and, for the test, the model's name and a
two-word prompt. No audio, no project and no file is ever sent.

## Turn it off

Press **Assistant on** again in Settings. Nothing is shown or sent from
then on. Your endpoint and model are remembered for next time; remove the
key too if you want it gone.
