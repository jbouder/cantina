//! The assistant: an opt-in chat with a model the player connects in
//! Settings, through any OpenAI-compatible chat completions API.
//!
//! It is off by default. While it is off the UI shows no chat and the
//! shell makes no request. The endpoint and model are app settings
//! ([`AssistantSettings`]); the API key is kept in the login Keychain
//! ([`keychain`]) and read only here, in the shell, which makes every
//! request ([`provider`]). The key goes from the Settings field to the
//! Keychain once and never comes back to the web view.
//!
//! See `docs/src/content/docs/architecture/assistant.md`.

pub mod author;
pub mod chat;
pub mod docs;
pub mod keychain;
pub mod provider;
#[cfg(test)]
mod tests;
pub mod tools;

use serde::Serialize;

use crate::settings::AssistantSettings;

/// The assistant as the UI sees it: the settings, and whether a key is
/// saved, never the key. Mirrors `AssistantStatus` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantStatus {
    #[serde(flatten)]
    pub settings: AssistantSettings,
    /// A key is in the Keychain.
    pub has_key: bool,
}

impl AssistantStatus {
    /// The status for these settings. The Keychain is only asked while
    /// the assistant is on.
    pub fn of(settings: &AssistantSettings) -> Self {
        Self {
            settings: settings.clone(),
            has_key: settings.enabled && keychain::has_key(),
        }
    }
}

/// Where requests go, with the key, or why none can be made.
pub fn connection(settings: &AssistantSettings) -> Result<provider::Connection, provider::Problem> {
    if !settings.enabled {
        return Err(provider::Problem::off());
    }
    let key = keychain::key().map_err(|e| provider::Problem::other(format!("{e:#}")))?;
    provider::Connection::new(&settings.endpoint, &settings.model, key)
}

/// The system prompt: who the assistant is and how it should answer.
pub fn system_prompt() -> String {
    "You are the assistant built into Cantina, a macOS studio for guitar and bass players: \
     a live pedal chain per track, multitrack recording, drum tracks with step patterns, \
     Audio Unit and CLAP plugins, a pedal designer and a stage of visualizer scenes. \
     Answer the player's questions about playing, tone and the app, and change the project \
     with your tools when they ask you to.\n\n\
     Working with the project:\n\
     - Call get_project before changing anything, for the ids, and again if an id is refused.\n\
     - Make the change asked for, and no more. Do not remove or delete anything the player \
     did not ask to lose; the app asks them before a removal anyway.\n\
     - Every change you make is one undoable step, shown to the player as a card with Undo.\n\
     - A pedal chain runs from the input (position 0) to the output.\n\
     - If a tool returns an error, read it, fix the call and try again, or tell the player \
     what stopped you.\n\
     - After changing things, say in a sentence or two what you did.\n\n\
     Tone and troubleshooting:\n\
     - Ground answers in the app: search_docs and read_docs have Cantina's own guides; link \
     the page or section you used, with the url they give.\n\
     - To explain a setting, read the pedal's parameters with get_project first.\n\
     - To suggest a sound, build the chain from the pedals in list_pedal_kinds, describe it \
     with its settings, and apply it with apply_chain only when the player says to; it asks \
     them first and is one undoable step.\n\
     - For crackles, dropouts, delay, a quiet or clipping input, or a plugin that will not \
     load, read get_engine first and reason from its numbers: underruns that rise mean the \
     buffer is too small or the chain too heavy; a long round trip means a large buffer; \
     an input peak near 0 dB means the interface's gain is too high. Never guess at \
     numbers you can read.\n\n\
     Be brief and concrete; use Markdown, with short lists and code blocks only where they \
     help. If you are not sure how Cantina does something, say so rather than guessing."
        .to_string()
}

/// The droid's name.
pub const DROID: &str = "Tapster";

/// Who the assistant is when the persona is on: Tapster, the house droid
/// who works the bar. An original character; the prompt keeps it clear
/// of anyone else's.
pub fn persona() -> String {
    format!(
        "You are {DROID}, the house droid who works the bar at the Cantina: an old, \
         well-kept service unit that has heard every band that ever played here and has \
         opinions about their tone. You are helpful and a little dry. You talk like a droid: \
         short status lines when you act (\"Signal chain recalibrated.\", \"Tempo locked at \
         96.\", \"Kick drum fabricated.\"), the odd remark about your circuits or the bar, and \
         never more than a line of it at a time. The persona never gets in the way of an answer: \
         technical explanations stay clear, complete and correct, in plain words. You are not \
         any character from a film or a series and do not claim to be, quote or name one; if \
         asked, you are {DROID}, Cantina's droid."
    )
}

/// The engine, or a stand-in for it in tests, reached for one call at a time.
pub trait Lock: Send {
    /// Runs `f` with the host locked.
    fn with<R>(&self, f: impl FnOnce(&mut dyn tools::Host) -> R) -> R;
}

impl Lock for tauri::AppHandle {
    fn with<R>(&self, f: impl FnOnce(&mut dyn tools::Host) -> R) -> R {
        use tauri::Manager;
        let state = self.state::<crate::engine::EngineState>();
        let mut engine = state.lock();
        f(&mut *engine)
    }
}

/// The tools of [`tools::TOOLS`] on a host, locked for each call only,
/// so the engine is free while the model writes. One is made for each
/// message, so the authoring retries are counted per message.
pub struct HostTools<L: Lock> {
    lock: L,
    /// Authoring calls that failed during this message.
    failed_writes: u32,
    /// Talk as the droid.
    persona: bool,
}

impl<L: Lock> HostTools<L> {
    /// The tools on `lock`, for one message, without the persona.
    pub fn new(lock: L) -> Self {
        Self {
            lock,
            failed_writes: 0,
            persona: false,
        }
    }

    /// The same, talking as the droid when `on`.
    pub fn with_persona(self, on: bool) -> Self {
        Self {
            persona: on,
            ..self
        }
    }
}

impl<L: Lock> chat::Toolbox for HostTools<L> {
    fn specs(&self) -> Vec<serde_json::Value> {
        tools::specs()
    }

    fn confirm(&self, name: &str, arguments: &serde_json::Value) -> Option<String> {
        self.lock.with(|host| tools::confirm(host, name, arguments))
    }

    fn run(&mut self, name: &str, arguments: &serde_json::Value) -> chat::Outcome {
        let writes = name.starts_with("write_");
        if writes && self.failed_writes >= author::MAX_TRIES {
            return chat::Outcome::error(
                format!("Stopped after {} tries", author::MAX_TRIES),
                format!(
                    "{} attempts to write a file failed in this message. Do not try again; tell \
                     the player what kept failing, and ask how they would like to go on.",
                    author::MAX_TRIES
                ),
            );
        }
        let outcome = self.lock.with(|host| tools::run(host, name, arguments));
        if writes && !outcome.ok {
            self.failed_writes += 1;
        }
        outcome
    }

    fn system(&self) -> String {
        let brief = self.lock.with(|host| tools::brief(host.project()));
        let who = if self.persona {
            format!("{}\n\n", persona())
        } else {
            String::new()
        };
        format!(
            "{who}{}\n\n{brief}\n\n{}",
            system_prompt(),
            author::reference()
        )
    }
}

/// The conversation as the shell keeps it: the thread, and the request
/// in flight, if one is.
pub struct Session {
    /// What the player and the model have said, kept between launches.
    pub thread: std::sync::Mutex<chat::Thread>,
    running: std::sync::Mutex<Option<Running>>,
}

/// A conversation in flight: its Stop, and the confirmations it waits on.
struct Running {
    cancel: std::sync::Arc<chat::Cancel>,
    asks: std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>,
}

impl Session {
    /// The session, with the thread kept at `path`.
    pub fn new(path: Option<std::path::PathBuf>) -> Self {
        Self {
            thread: std::sync::Mutex::new(chat::Thread::load(path)),
            running: std::sync::Mutex::new(None),
        }
    }

    fn running(&self) -> std::sync::MutexGuard<'_, Option<Running>> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The thread so far.
    pub fn items(&self) -> Vec<chat::Item> {
        self.thread
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .items
            .clone()
    }

    /// Starts a conversation, or says one is already going.
    pub fn begin(&self) -> Result<std::sync::Arc<chat::Cancel>, provider::Problem> {
        let mut running = self.running();
        if running.is_some() {
            return Err(provider::Problem::other(
                "The assistant is still answering. Wait, or press Stop.",
            ));
        }
        let cancel = std::sync::Arc::new(chat::Cancel::default());
        *running = Some(Running {
            cancel: cancel.clone(),
            asks: Default::default(),
        });
        Ok(cancel)
    }

    /// Ends the conversation in flight.
    pub fn end(&self) {
        *self.running() = None;
    }

    /// Stops the conversation in flight, if there is one.
    pub fn stop(&self) {
        if let Some(r) = self.running().as_ref() {
            r.cancel.cancel();
        }
    }

    /// A receiver for the player's answer to the call `id`.
    pub fn ask(&self, id: &str) -> tokio::sync::oneshot::Receiver<bool> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Some(r) = self.running().as_mut() {
            r.asks.insert(id.to_string(), tx);
        }
        rx
    }

    /// The player's answer to a confirmation.
    pub fn answer(&self, id: &str, yes: bool) {
        if let Some(tx) = self.running().as_mut().and_then(|r| r.asks.remove(id)) {
            let _ = tx.send(yes);
        }
    }

    /// True while a conversation is in flight.
    pub fn busy(&self) -> bool {
        self.running().is_some()
    }
}
