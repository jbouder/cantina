//! Requests to an OpenAI-compatible API: OpenAI, Anthropic's compatible
//! endpoint, OpenRouter, or a local Ollama or LM Studio.
//!
//! Every request goes out from here, the shell, so the API key never
//! reaches the web view. What goes wrong is a [`Problem`] in plain words,
//! sorted into the few kinds a player can act on: a bad URL, a bad key, a
//! model the endpoint does not have, a rate limit, no network.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// What kind of thing went wrong, for the UI to pick its wording and for
/// the chat to say what to do next. Mirrors `ProblemKind` in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProblemKind {
    /// The assistant is turned off; nothing was sent.
    Off,
    /// The URL is malformed, nothing answers there, or it is not an
    /// OpenAI-compatible API.
    BadUrl,
    /// The endpoint refused the key, or wants one and has none.
    BadKey,
    /// The endpoint has no model by that name, or none is picked.
    UnknownModel,
    /// Too many requests, or the account is out of credit.
    RateLimited,
    /// No connection, or it timed out.
    Offline,
    /// The provider answered with an error of its own.
    Server,
    /// Anything else.
    Other,
}

/// A failed request, in plain words. Mirrors `AssistantProblem` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub kind: ProblemKind,
    pub message: String,
}

impl Problem {
    /// A problem of a kind, in these words.
    pub fn new(kind: ProblemKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// The assistant is off, so nothing was sent.
    pub fn off() -> Self {
        Self::new(
            ProblemKind::Off,
            "The assistant is off. Turn it on in Settings.",
        )
    }

    /// Something outside the request went wrong, such as the Keychain.
    pub fn other(message: impl Into<String>) -> Self {
        Self::new(ProblemKind::Other, message)
    }

    /// A transport error from reqwest, in plain words.
    fn from_reqwest(e: &reqwest::Error, base: &str) -> Self {
        if e.is_timeout() {
            Self::new(
                ProblemKind::Offline,
                format!("{base} took too long to answer. Check your connection and try again."),
            )
        } else if e.is_connect() {
            Self::new(
                ProblemKind::BadUrl,
                format!(
                    "Could not connect to {base}. Check the URL, your connection, and for a \
                     local model that its server is running."
                ),
            )
        } else if e.is_decode() || e.is_body() {
            Self::new(
                ProblemKind::BadUrl,
                format!("{base} did not answer like an OpenAI-compatible API."),
            )
        } else {
            Self::new(ProblemKind::Other, format!("The request failed: {e}"))
        }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Which request an HTTP error came from, since a 404 means a bad URL
/// from `/models` and usually an unknown model from `/chat/completions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Listing the models.
    Models,
    /// A chat completion.
    Chat,
}

/// Turns an HTTP error status and its body into a [`Problem`].
pub fn classify(status: u16, body: &str, stage: Stage, model: &str) -> Problem {
    let said = error_message(body);
    let detail = said
        .as_deref()
        .map(|m| format!(" The provider said: {m}"))
        .unwrap_or_default();
    let mentions_model = said
        .as_deref()
        .is_some_and(|m| m.to_lowercase().contains("model"));
    match status {
        401 | 403 => Problem::new(
            ProblemKind::BadKey,
            format!("The endpoint refused the API key. Check it, or paste a new one.{detail}"),
        ),
        404 if stage == Stage::Chat && mentions_model => Problem::new(
            ProblemKind::UnknownModel,
            format!("The endpoint has no model called {model:?}. Pick another.{detail}"),
        ),
        400 | 422 if mentions_model => Problem::new(
            ProblemKind::UnknownModel,
            format!("The endpoint will not run the model {model:?}. Pick another.{detail}"),
        ),
        404 | 405 => Problem::new(
            ProblemKind::BadUrl,
            format!(
                "Nothing answered at that URL. It should be the API's base, usually ending in \
                 /v1.{detail}"
            ),
        ),
        402 | 429 => Problem::new(
            ProblemKind::RateLimited,
            format!("The provider is limiting requests, or the account is out of credit. Wait a moment and try again.{detail}"),
        ),
        500..=599 => Problem::new(
            ProblemKind::Server,
            format!("The provider had a problem of its own (HTTP {status}). Try again shortly.{detail}"),
        ),
        _ => Problem::new(
            ProblemKind::Other,
            format!("The request failed with HTTP {status}.{detail}"),
        ),
    }
}

/// The `error.message` (or `error`, or `message`) of an error body.
fn error_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let message = value
        .pointer("/error/message")
        .or_else(|| value.get("error").filter(|e| e.is_string()))
        .or_else(|| value.get("message"))?
        .as_str()?
        .trim();
    (!message.is_empty()).then(|| message.chars().take(300).collect())
}

/// An endpoint, a model and a key: everything a request needs.
#[derive(Clone)]
pub struct Connection {
    /// The API's base URL, without a trailing slash.
    pub base: String,
    pub model: String,
    key: Option<String>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key, so a stray debug print cannot leak it.
        f.debug_struct("Connection")
            .field("base", &self.base)
            .field("model", &self.model)
            .field("key", &self.key.as_ref().map(|_| "…"))
            .finish()
    }
}

impl Connection {
    /// Checks and tidies the URL: `http` or `https`, no trailing slash,
    /// and a pasted `/chat/completions` or `/models` taken off.
    pub fn new(endpoint: &str, model: &str, key: Option<String>) -> Result<Self, Problem> {
        let mut base = endpoint.trim().trim_end_matches('/').to_string();
        for suffix in ["/chat/completions", "/models"] {
            if let Some(stripped) = base.strip_suffix(suffix) {
                base = stripped.to_string();
            }
        }
        if base.is_empty() {
            return Err(Problem::new(
                ProblemKind::BadUrl,
                "Enter the endpoint's URL, such as https://api.openai.com/v1.",
            ));
        }
        let rest = base
            .strip_prefix("https://")
            .or_else(|| base.strip_prefix("http://"));
        if rest.is_none_or(|r| r.is_empty() || r.contains(char::is_whitespace)) {
            return Err(Problem::new(
                ProblemKind::BadUrl,
                format!("{base:?} is not a web address. It should start with https:// (or http:// for a local model)."),
            ));
        }
        Ok(Self {
            base,
            model: model.trim().to_string(),
            key: key.filter(|k| !k.trim().is_empty()),
        })
    }

    /// A request to `path` under the base, with the key.
    fn request(
        &self,
        client: &reqwest::Client,
        method: reqwest::Method,
        path: &str,
    ) -> reqwest::RequestBuilder {
        let mut request = client.request(method, format!("{}{path}", self.base));
        if let Some(key) = &self.key {
            request = request.bearer_auth(key);
            // Anthropic's own endpoints take the key in their header too.
            if self.base.contains("api.anthropic.com") {
                request = request
                    .header("x-api-key", key)
                    .header("anthropic-version", "2023-06-01");
            }
        }
        request
    }

    /// The ids of the models the endpoint offers, sorted.
    pub async fn models(&self, client: &reqwest::Client) -> Result<Vec<String>, Problem> {
        let response = self
            .request(client, reqwest::Method::GET, "/models")
            .send()
            .await
            .map_err(|e| Problem::from_reqwest(&e, &self.base))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|e| Problem::from_reqwest(&e, &self.base))?;
        if !(200..300).contains(&status) {
            return Err(classify(status, &body, Stage::Models, &self.model));
        }
        parse_models(&body).ok_or_else(|| {
            Problem::new(
                ProblemKind::BadUrl,
                format!(
                    "{} answered, but not with a list of models. Is it an OpenAI-compatible API?",
                    self.base
                ),
            )
        })
    }

    /// Checks the URL, the key and the model: the model list, and when
    /// the endpoint has no list, a one-word chat.
    pub async fn test(&self, client: &reqwest::Client) -> Result<(), Problem> {
        if self.model.is_empty() {
            return Err(Problem::new(
                ProblemKind::UnknownModel,
                "Pick a model to talk to.",
            ));
        }
        match self.models(client).await {
            Ok(models) if models.is_empty() || models.contains(&self.model) => Ok(()),
            Ok(_) => Err(Problem::new(
                ProblemKind::UnknownModel,
                format!(
                    "The endpoint has no model called {:?}. Pick one from the list.",
                    self.model
                ),
            )),
            // Some endpoints have no model list; ask the model itself.
            Err(p) if p.kind == ProblemKind::BadUrl => {
                self.ping(client).await.map_err(
                    |q| {
                        if q.kind == ProblemKind::BadUrl {
                            p
                        } else {
                            q
                        }
                    },
                )
            }
            Err(p) => Err(p),
        }
    }

    /// A one-token chat completion, to see that the model answers.
    async fn ping(&self, client: &reqwest::Client) -> Result<(), Problem> {
        let body = json!({
            "model": self.model,
            "messages": [{"role": "user", "content": "Say OK."}],
            "max_tokens": 1,
        });
        let response = self
            .request(client, reqwest::Method::POST, "/chat/completions")
            .json(&body)
            .send()
            .await
            .map_err(|e| Problem::from_reqwest(&e, &self.base))?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        Err(classify(status, &body, Stage::Chat, &self.model))
    }
}

impl Connection {
    /// A chat completion, streamed: `on_text` gets each piece of the
    /// reply as it arrives, and the whole step, with any tool calls,
    /// comes back at the end. An endpoint that ignores `stream` and
    /// answers in one piece works too.
    pub async fn chat(
        &self,
        client: &reqwest::Client,
        messages: &[Value],
        tools: &[Value],
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<Step, Problem> {
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
        }
        let mut response = self
            .request(client, reqwest::Method::POST, "/chat/completions")
            .json(&body)
            .send()
            .await
            .map_err(|e| Problem::from_reqwest(&e, &self.base))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let body = response.text().await.unwrap_or_default();
            return Err(classify(status, &body, Stage::Chat, &self.model));
        }
        let streamed = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("event-stream"));
        let mut stream = StreamState::default();
        if streamed {
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|e| Problem::from_reqwest(&e, &self.base))?
            {
                let text = stream.feed(&chunk);
                if !text.is_empty() {
                    on_text(&text);
                }
                if stream.done {
                    break;
                }
            }
        } else {
            let body = response
                .text()
                .await
                .map_err(|e| Problem::from_reqwest(&e, &self.base))?;
            let text = stream.whole(&body);
            if !text.is_empty() {
                on_text(&text);
            }
        }
        stream.finish()
    }
}

/// A tool call the model asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct CallRequest {
    /// The id the result goes back under.
    pub id: String,
    pub name: String,
    /// The arguments as the model wrote them: JSON, if it got them right.
    pub arguments: String,
}

/// One answer from the model: its text and the tools it wants run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub text: String,
    pub calls: Vec<CallRequest>,
}

/// A streamed answer being put back together from its server-sent
/// events: `data: {json}` lines, each a piece of text or of a tool call.
#[derive(Debug, Default)]
pub struct StreamState {
    /// Bytes after the last newline, which may end mid-character.
    pending: Vec<u8>,
    text: String,
    calls: Vec<CallRequest>,
    /// The stream said `[DONE]`.
    pub done: bool,
    error: Option<String>,
}

impl StreamState {
    /// Takes the next bytes and returns the text they add.
    pub fn feed(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut added = String::new();
        while let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            if let Some(data) = line.trim().strip_prefix("data:") {
                let data = data.trim();
                if data == "[DONE]" {
                    self.done = true;
                } else if let Ok(event) = serde_json::from_str::<Value>(data) {
                    self.event(&event, "delta", &mut added);
                }
            }
        }
        added
    }

    /// Takes an answer that came in one piece and returns its text.
    pub fn whole(&mut self, body: &str) -> String {
        let mut added = String::new();
        match serde_json::from_str::<Value>(body) {
            Ok(answer) => self.event(&answer, "message", &mut added),
            Err(_) => self.error = Some("The answer was not JSON.".into()),
        }
        self.done = true;
        added
    }

    fn event(&mut self, event: &Value, field: &str, added: &mut String) {
        if let Some(error) = event.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
                .unwrap_or("unknown error");
            self.error = Some(message.to_string());
            return;
        }
        let Some(choice) = event.pointer("/choices/0") else {
            return;
        };
        let Some(part) = choice.get(field) else {
            return;
        };
        if let Some(text) = part.get("content").and_then(Value::as_str) {
            self.text.push_str(text);
            added.push_str(text);
        }
        for (n, call) in part
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let index = call
                .get("index")
                .and_then(Value::as_u64)
                .map_or(n, |i| i as usize);
            while self.calls.len() <= index {
                self.calls.push(CallRequest {
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                });
            }
            let slot = &mut self.calls[index];
            if let Some(id) = call.get("id").and_then(Value::as_str) {
                slot.id = id.to_string();
            }
            if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                // Sent once in the first piece, though some servers repeat it.
                if !slot.name.ends_with(name) {
                    slot.name.push_str(name);
                }
            }
            if let Some(args) = call.pointer("/function/arguments") {
                match args {
                    Value::String(piece) => slot.arguments.push_str(piece),
                    other => slot.arguments = other.to_string(),
                }
            }
        }
    }

    /// The step, once the stream has ended.
    pub fn finish(self) -> Result<Step, Problem> {
        if let Some(message) = self.error {
            return Err(Problem::new(
                ProblemKind::Server,
                format!("The provider stopped with an error: {message}"),
            ));
        }
        let calls = self
            .calls
            .into_iter()
            .enumerate()
            .filter(|(_, c)| !c.name.is_empty())
            .map(|(n, c)| CallRequest {
                id: if c.id.is_empty() {
                    format!("call_{n}")
                } else {
                    c.id
                },
                ..c
            })
            .collect();
        Ok(Step {
            text: self.text,
            calls,
        })
    }
}

/// A client for a chat: no limit on the whole answer, which streams for
/// as long as the model writes, but one on each wait for more of it.
pub fn chat_client() -> reqwest::Client {
    install_crypto();
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(120))
        .user_agent(concat!("Cantina/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

/// The model ids in a `/models` answer: `{"data": [{"id": …}]}`, or the
/// bare list some local servers send.
pub fn parse_models(body: &str) -> Option<Vec<String>> {
    let value: Value = serde_json::from_str(body).ok()?;
    let list = value
        .get("data")
        .or_else(|| value.get("models"))
        .unwrap_or(&value)
        .as_array()?;
    let mut ids: Vec<String> = list
        .iter()
        .filter_map(|m| {
            m.get("id")
                .or_else(|| m.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    ids.sort();
    ids.dedup();
    Some(ids)
}

/// A client for short requests: listing models and testing.
pub fn client() -> reqwest::Client {
    install_crypto();
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("Cantina/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

/// rustls's crypto. The updater installs the same provider; whichever
/// runs first wins.
fn install_crypto() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A one-thread HTTP server on localhost that answers from a closure,
    //! for testing requests without a provider.

    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread;

    /// A request as the server saw it.
    #[derive(Debug, Clone)]
    pub struct Seen {
        pub method: String,
        pub path: String,
        pub headers: Vec<(String, String)>,
        pub body: String,
    }

    /// Serves until the test ends; returns the base URL. `answer` gets
    /// each request and returns the status, content type and body.
    pub fn serve(answer: impl Fn(&Seen) -> (u16, &'static str, String) + Send + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let mut headers = Vec::new();
                let mut length = 0;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = h.trim_end().split_once(':') {
                        let (k, v) = (k.trim().to_lowercase(), v.trim().to_string());
                        if k == "content-length" {
                            length = v.parse().unwrap_or(0);
                        }
                        headers.push((k, v));
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                let seen = Seen {
                    method,
                    path,
                    headers,
                    body: String::from_utf8_lossy(&body).into_owned(),
                };
                let (status, kind, text) = answer(&seen);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        format!("http://127.0.0.1:{port}/v1")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run<F: std::future::Future>(f: F) -> F::Output {
        tauri::async_runtime::block_on(f)
    }

    #[test]
    fn urls_are_tidied_and_bad_ones_refused() {
        let c = Connection::new(" https://api.openai.com/v1/ ", "gpt", None).unwrap();
        assert_eq!(c.base, "https://api.openai.com/v1");
        let c = Connection::new("http://localhost:11434/v1/chat/completions", "m", None).unwrap();
        assert_eq!(c.base, "http://localhost:11434/v1");
        for bad in [
            "",
            "api.openai.com/v1",
            "ftp://x",
            "https://",
            "https://a b",
        ] {
            let p = Connection::new(bad, "m", None).unwrap_err();
            assert_eq!(p.kind, ProblemKind::BadUrl, "{bad:?}");
        }
        // An empty key is no key.
        let c = Connection::new("https://x/v1", "m", Some("  ".into())).unwrap();
        assert!(c.key.is_none());
        assert!(!format!(
            "{:?}",
            Connection::new("https://x", "m", Some("sk-secret".into())).unwrap()
        )
        .contains("secret"));
    }

    #[test]
    fn errors_are_sorted_into_what_a_player_can_fix() {
        let openai =
            r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#;
        let p = classify(401, openai, Stage::Models, "gpt");
        assert_eq!(p.kind, ProblemKind::BadKey);
        assert!(p.message.contains("Incorrect API key provided"));
        let missing = r#"{"error":{"message":"The model `gpt-9` does not exist"}}"#;
        assert_eq!(
            classify(404, missing, Stage::Chat, "gpt-9").kind,
            ProblemKind::UnknownModel
        );
        assert_eq!(
            classify(404, "Not Found", Stage::Models, "m").kind,
            ProblemKind::BadUrl
        );
        assert_eq!(
            classify(404, "", Stage::Chat, "m").kind,
            ProblemKind::BadUrl
        );
        assert_eq!(
            classify(400, r#"{"error":"model 'x' not found"}"#, Stage::Chat, "x").kind,
            ProblemKind::UnknownModel
        );
        assert_eq!(
            classify(429, "", Stage::Chat, "m").kind,
            ProblemKind::RateLimited
        );
        assert_eq!(
            classify(503, "", Stage::Chat, "m").kind,
            ProblemKind::Server
        );
        assert_eq!(classify(418, "", Stage::Chat, "m").kind, ProblemKind::Other);
    }

    #[test]
    fn model_lists_parse_in_either_shape() {
        let openai = r#"{"object":"list","data":[{"id":"gpt-b"},{"id":"gpt-a"}]}"#;
        assert_eq!(parse_models(openai).unwrap(), ["gpt-a", "gpt-b"]);
        let bare = r#"[{"id":"local"}]"#;
        assert_eq!(parse_models(bare).unwrap(), ["local"]);
        assert!(parse_models("<html>").is_none());
        assert!(parse_models(r#"{"ok":true}"#).is_none());
    }

    #[test]
    fn test_connection_reports_success_a_bad_key_a_bad_url_and_an_unknown_model() {
        let base = fake::serve(|seen| {
            let authorized = seen
                .headers
                .iter()
                .any(|(k, v)| k == "authorization" && v == "Bearer good");
            match (seen.path.as_str(), authorized) {
                (_, false) => (
                    401,
                    "application/json",
                    r#"{"error":{"message":"bad key"}}"#.into(),
                ),
                ("/v1/models", true) => (
                    200,
                    "application/json",
                    r#"{"data":[{"id":"small"},{"id":"large"}]}"#.into(),
                ),
                _ => (404, "text/plain", "no".into()),
            }
        });
        let client = client();
        let ok = Connection::new(&base, "small", Some("good".into())).unwrap();
        assert_eq!(run(ok.test(&client)), Ok(()));
        assert_eq!(run(ok.models(&client)).unwrap(), ["large", "small"]);

        let bad_key = Connection::new(&base, "small", Some("nope".into())).unwrap();
        assert_eq!(
            run(bad_key.test(&client)).unwrap_err().kind,
            ProblemKind::BadKey
        );

        let unknown = Connection::new(&base, "huge", Some("good".into())).unwrap();
        assert_eq!(
            run(unknown.test(&client)).unwrap_err().kind,
            ProblemKind::UnknownModel
        );

        let none = Connection::new(&base, "", Some("good".into())).unwrap();
        assert_eq!(
            run(none.test(&client)).unwrap_err().kind,
            ProblemKind::UnknownModel
        );

        let wrong_path =
            Connection::new(&format!("{base}/nothing"), "small", Some("good".into())).unwrap();
        assert_eq!(
            run(wrong_path.test(&client)).unwrap_err().kind,
            ProblemKind::BadUrl
        );

        // A port nothing listens on.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let nobody =
            Connection::new(&format!("http://127.0.0.1:{port}/v1"), "small", None).unwrap();
        assert_eq!(
            run(nobody.test(&client)).unwrap_err().kind,
            ProblemKind::BadUrl
        );
    }

    #[test]
    fn a_stream_is_put_back_together_across_split_lines_and_characters() {
        let events = concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"Signal \"}}]}\n\n",
            ": a comment\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"chain é\"}}]}\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"set_tempo\",\"arguments\":\"\"}}]}}]}\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"bpm\\\":\"}}]}}]}\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\" 90}\"}},{\"index\":1,\"function\":{\"name\":\"list_tracks\"}}]}}]}\n",
            "data: [DONE]\n",
        );
        // Fed a byte at a time, so lines and the é split anywhere.
        let mut state = StreamState::default();
        let mut text = String::new();
        for byte in events.as_bytes() {
            text.push_str(&state.feed(std::slice::from_ref(byte)));
        }
        assert!(state.done);
        assert_eq!(text, "Signal chain é");
        let step = state.finish().unwrap();
        assert_eq!(step.text, "Signal chain é");
        assert_eq!(
            step.calls,
            [
                CallRequest {
                    id: "c1".into(),
                    name: "set_tempo".into(),
                    arguments: "{\"bpm\": 90}".into()
                },
                CallRequest {
                    id: "call_1".into(),
                    name: "list_tracks".into(),
                    arguments: String::new()
                },
            ]
        );
        let mut failed = StreamState::default();
        failed.feed(b"data: {\"error\":{\"message\":\"overloaded\"}}\n");
        assert_eq!(failed.finish().unwrap_err().kind, ProblemKind::Server);
    }

    #[test]
    fn a_chat_streams_from_the_endpoint_and_one_in_a_piece_works_too() {
        let base = fake::serve(|seen| {
            if seen.body.contains("\"whole\"") {
                (
                    200,
                    "application/json",
                    r#"{"choices":[{"message":{"role":"assistant","content":"All of it."}}]}"#
                        .into(),
                )
            } else if seen.body.contains("\"stream\":true") && seen.body.contains("\"tools\"") {
                (
                    200,
                    "text/event-stream",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" there\"}}]}\n\ndata: [DONE]\n\n".into(),
                )
            } else {
                (
                    400,
                    "application/json",
                    r#"{"error":{"message":"no"}}"#.into(),
                )
            }
        });
        let client = chat_client();
        let messages = [json!({"role": "user", "content": "hello"})];
        let tools = [
            json!({"type": "function", "function": {"name": "t", "parameters": {"type": "object"}}}),
        ];
        let streamed = Connection::new(&base, "m", None).unwrap();
        let mut pieces = Vec::new();
        let step = run(streamed.chat(&client, &messages, &tools, &mut |t| {
            pieces.push(t.to_string())
        }))
        .unwrap();
        assert_eq!(pieces.concat(), "Hi there");
        assert_eq!(step.text, "Hi there");
        let whole = Connection::new(&base, "whole", None).unwrap();
        let step = run(whole.chat(&client, &messages, &[], &mut |_| {})).unwrap();
        assert_eq!(step.text, "All of it.");
    }

    #[test]
    fn an_endpoint_without_a_model_list_is_tested_with_a_chat() {
        let base = fake::serve(|seen| match seen.path.as_str() {
            "/v1/chat/completions" if seen.method == "POST" && seen.body.contains("\"known\"") => (
                200,
                "application/json",
                r#"{"choices":[{"message":{"role":"assistant","content":"OK"}}]}"#.into(),
            ),
            "/v1/chat/completions" => (
                404,
                "application/json",
                r#"{"error":{"message":"model not found"}}"#.into(),
            ),
            _ => (404, "text/plain", "".into()),
        });
        let client = client();
        let known = Connection::new(&base, "known", None).unwrap();
        assert_eq!(run(known.test(&client)), Ok(()));
        let unknown = Connection::new(&base, "other", None).unwrap();
        assert_eq!(
            run(unknown.test(&client)).unwrap_err().kind,
            ProblemKind::UnknownModel
        );
    }
}
