//! Requests to an OpenAI-compatible API: OpenAI, Anthropic's compatible
//! endpoint, OpenRouter, or a local Ollama or LM Studio.
//!
//! Every request goes out from here, the shell, so the API key never
//! reaches the web view. What goes wrong is a [`Problem`] in plain words,
//! sorted into the few kinds a player can act on: a bad URL, a bad key, a
//! model the endpoint does not have, a rate limit, no network.

use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

/// What kind of thing went wrong, for the UI to pick its wording and for
/// the chat to say what to do next. Mirrors `ProblemKind` in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub kind: ProblemKind,
    pub message: String,
}

impl Problem {
    fn new(kind: ProblemKind, message: impl Into<String>) -> Self {
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
    // The updater installs the same provider; whichever runs first wins.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("Cantina/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
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
            let authorised = seen
                .headers
                .iter()
                .any(|(k, v)| k == "authorization" && v == "Bearer good");
            match (seen.path.as_str(), authorised) {
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
