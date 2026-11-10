//! The conversation: a thread of items the player sees, the loop that
//! asks the model, runs the tools it calls and asks again until it has
//! answered, and the file the thread is kept in between launches.
//!
//! The loop is written against two traits, [`Model`] and [`Toolbox`], so
//! tests drive it with a scripted model and no network or engine.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{oneshot, Notify};

use super::provider::{CallRequest, Connection, Problem, ProblemKind, Step};

/// The most model calls one message may lead to before the loop gives up.
pub const MAX_STEPS: usize = 12;
/// The most items of the thread sent with a request; older ones are left
/// out, from a message of the player's on.
pub const CONTEXT_ITEMS: usize = 80;
/// The thread file's format version.
const THREAD_VERSION: u32 = 1;

/// One entry of the thread. Mirrors `ChatItem` in the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Item {
    /// What the player wrote.
    User { text: String },
    /// What the model wrote.
    Reply {
        text: String,
        /// Stop was pressed before it finished.
        #[serde(default)]
        stopped: bool,
    },
    /// A tool the model called, and what came of it.
    Call {
        id: String,
        name: String,
        /// The arguments as the model wrote them.
        arguments: String,
        /// One line for the card, such as "Added Overdrive to Guitar 1".
        summary: String,
        /// What went back to the model, as JSON.
        result: String,
        ok: bool,
        /// The project revision the edit left behind, when it changed the
        /// project: Undo on the card applies while it is still current.
        #[serde(default)]
        undo: Option<u64>,
        /// The card's Undo was used.
        #[serde(default)]
        undone: bool,
    },
    /// A request that failed. Shown, never sent to the model.
    Error { problem: Problem },
}

/// What the loop tells the UI as it goes. Mirrors `ChatEvent` in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Event {
    /// More of the reply being written.
    Text { text: String },
    /// An item was added to the thread; any text so far is in it.
    Item { item: Item },
    /// A tool wants the player's yes before it runs.
    Confirm { id: String, question: String },
    /// The model has answered, failed or been stopped.
    Done,
}

/// A boxed future, for the traits' async methods.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Something that answers a conversation: a provider, or a script in tests.
pub trait Model: Sync {
    /// One answer to `messages`, with `on_text` given its text as it comes.
    fn complete<'a>(
        &'a self,
        messages: &'a [Value],
        tools: &'a [Value],
        on_text: &'a mut (dyn FnMut(&str) + Send),
    ) -> BoxFuture<'a, Result<Step, Problem>>;
}

/// A provider through an OpenAI-compatible API.
pub struct Remote {
    pub connection: Connection,
    pub client: reqwest::Client,
}

impl Model for Remote {
    fn complete<'a>(
        &'a self,
        messages: &'a [Value],
        tools: &'a [Value],
        on_text: &'a mut (dyn FnMut(&str) + Send),
    ) -> BoxFuture<'a, Result<Step, Problem>> {
        Box::pin(self.connection.chat(&self.client, messages, tools, on_text))
    }
}

/// What came of running a tool.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// One line for the card.
    pub summary: String,
    /// What goes back to the model.
    pub result: Value,
    pub ok: bool,
    /// The project revision after an edit, for the card's Undo.
    pub undo: Option<u64>,
}

impl Outcome {
    /// A failure, told to the model so it can try something else.
    pub fn error(summary: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            result: json!({ "error": message.into() }),
            ok: false,
            undo: None,
        }
    }
}

/// The tools the model may call, and what it is told about the project.
pub trait Toolbox: Send {
    /// The tools, as OpenAI function definitions.
    fn specs(&self) -> Vec<Value>;
    /// The question to ask the player first, for a tool that destroys
    /// something; `None` runs it straight away.
    fn confirm(&self, name: &str, arguments: &Value) -> Option<String>;
    /// Runs a tool.
    fn run(&mut self, name: &str, arguments: &Value) -> Outcome;
    /// The system prompt, with what the model should know of the project.
    fn system(&self) -> String;
}

/// A toolbox with no tools: the chat on its own, for tests.
#[cfg(test)]
pub struct NoTools(pub String);

#[cfg(test)]
impl Toolbox for NoTools {
    fn specs(&self) -> Vec<Value> {
        Vec::new()
    }
    fn confirm(&self, _: &str, _: &Value) -> Option<String> {
        None
    }
    fn run(&mut self, name: &str, _: &Value) -> Outcome {
        Outcome::error(
            format!("No tool {name}"),
            format!("there is no tool called {name}"),
        )
    }
    fn system(&self) -> String {
        self.0.clone()
    }
}

/// The thread, and where it is kept.
#[derive(Debug, Default)]
pub struct Thread {
    pub items: Vec<Item>,
    path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct ThreadFile {
    version: u32,
    items: Vec<Item>,
}

impl Thread {
    /// The thread kept at `path`, or an empty one if there is none or it
    /// does not read: a lost chat is not worth failing a launch over.
    pub fn load(path: Option<PathBuf>) -> Self {
        let items = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str::<ThreadFile>(&text).ok())
            .filter(|f| f.version <= THREAD_VERSION)
            .map(|f| f.items)
            .unwrap_or_default();
        Self { items, path }
    }

    /// Adds an item and writes the file.
    pub fn push(&mut self, item: Item) {
        self.items.push(item);
        self.save();
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.items.clear();
        self.save();
    }

    /// Writes the file. A thread that cannot be saved still works.
    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        let file = ThreadFile {
            version: THREAD_VERSION,
            items: self.items.clone(),
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&file) {
            let _ = write_atomically(path, &text);
        }
    }

    /// Takes off what followed the player's last message, ready to ask
    /// again: failures and the last reply, but never a tool call that
    /// ran, since its edit is in the project. False when there is no
    /// message to answer.
    pub fn rewind_for_retry(&mut self) -> bool {
        while let Some(last) = self.items.last() {
            match last {
                Item::Error { .. } | Item::Reply { .. } => {
                    self.items.pop();
                }
                _ => break,
            }
        }
        self.save();
        matches!(
            self.items.last(),
            Some(Item::User { .. } | Item::Call { .. })
        )
    }
}

fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

/// The thread as chat messages: the system prompt, then the player's
/// messages, the model's replies with the tools they called, and each
/// tool's result. Failures are left out, and so is everything before the
/// last `CONTEXT_ITEMS`, cut at a message of the player's.
pub fn messages(system: &str, items: &[Item]) -> Vec<Value> {
    let mut start = items.len().saturating_sub(CONTEXT_ITEMS);
    while start > 0 && !matches!(items.get(start), Some(Item::User { .. })) {
        start -= 1;
    }
    let items = &items[start..];
    let mut out = vec![json!({ "role": "system", "content": system })];
    let mut i = 0;
    while i < items.len() {
        match &items[i] {
            Item::User { text } => {
                out.push(json!({ "role": "user", "content": text }));
                i += 1;
            }
            Item::Error { .. } => i += 1,
            Item::Reply { .. } | Item::Call { .. } => {
                // A reply and the calls straight after it are one message.
                let mut content = Value::Null;
                if let Item::Reply { text, .. } = &items[i] {
                    content = Value::String(text.clone());
                    i += 1;
                }
                let mut calls = Vec::new();
                let mut results = Vec::new();
                while let Some(Item::Call {
                    id,
                    name,
                    arguments,
                    result,
                    ..
                }) = items.get(i)
                {
                    calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": { "name": name, "arguments": arguments },
                    }));
                    results.push(json!({ "role": "tool", "tool_call_id": id, "content": result }));
                    i += 1;
                }
                let mut message = json!({ "role": "assistant", "content": content });
                if !calls.is_empty() {
                    message["tool_calls"] = Value::Array(calls);
                }
                out.push(message);
                out.extend(results);
            }
        }
    }
    out
}

/// Stop, for a conversation in flight.
#[derive(Default)]
pub struct Cancel {
    flag: AtomicBool,
    notify: Notify,
}

impl Cancel {
    /// Asks the conversation to stop.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }

    /// True once stopped.
    pub fn is_canceled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Resolves when stopped.
    pub async fn canceled(&self) {
        while !self.is_canceled() {
            self.notify.notified().await;
        }
    }
}

/// How the loop asks the player to confirm: given a call's id, a
/// receiver for the answer.
pub type Ask<'a> = &'a (dyn Fn(&str) -> oneshot::Receiver<bool> + Sync);

/// Answers the thread's last message: asks the model, runs the tools it
/// calls (asking first for destructive ones), and asks again, until it
/// answers without a tool, fails, is stopped or reaches [`MAX_STEPS`].
/// Every item is added to `thread` and sent to `sink` as it happens.
pub async fn converse(
    model: &dyn Model,
    tools: &mut dyn Toolbox,
    thread: &Mutex<Thread>,
    sink: &(dyn Fn(Event) + Sync),
    cancel: &Cancel,
    ask: Ask<'_>,
) {
    let push = |item: Item| {
        lock(thread).push(item.clone());
        sink(Event::Item { item });
    };
    let specs = tools.specs();
    for _ in 0..MAX_STEPS {
        let request = messages(&tools.system(), &lock(thread).items);
        let written = Mutex::new(String::new());
        let mut on_text = |text: &str| {
            lock(&written).push_str(text);
            sink(Event::Text {
                text: text.to_string(),
            });
        };
        let step = tokio::select! {
            step = model.complete(&request, &specs, &mut on_text) => Some(step),
            _ = cancel.canceled() => None,
        };
        let step = match step {
            None => {
                let text = lock(&written).clone();
                if !text.is_empty() {
                    push(Item::Reply {
                        text,
                        stopped: true,
                    });
                }
                break;
            }
            Some(Err(problem)) => {
                push(Item::Error { problem });
                break;
            }
            Some(Ok(step)) => step,
        };
        if !step.text.trim().is_empty() {
            push(Item::Reply {
                text: step.text.clone(),
                stopped: false,
            });
        }
        if step.calls.is_empty() {
            break;
        }
        for call in &step.calls {
            let item = run_call(tools, call, sink, cancel, ask).await;
            push(item);
            if cancel.is_canceled() {
                break;
            }
        }
        if cancel.is_canceled() {
            break;
        }
    }
    if !cancel.is_canceled() && ended_on_a_call(&lock(thread).items) {
        push(Item::Error {
            problem: Problem::new(
                ProblemKind::Other,
                format!("Stopped after {MAX_STEPS} steps without an answer. Press Retry to let it carry on."),
            ),
        });
    }
    sink(Event::Done);
}

/// True when the thread ends on a tool call the model never answered,
/// which only happens when the loop ran out of steps.
fn ended_on_a_call(items: &[Item]) -> bool {
    matches!(items.last(), Some(Item::Call { .. }))
}

/// Runs one call: parses its arguments, asks first if the tool needs it,
/// and runs it. A failure becomes the result, for the model to read.
async fn run_call(
    tools: &mut dyn Toolbox,
    call: &CallRequest,
    sink: &(dyn Fn(Event) + Sync),
    cancel: &Cancel,
    ask: Ask<'_>,
) -> Item {
    let arguments = if call.arguments.trim().is_empty() {
        Ok(json!({}))
    } else {
        serde_json::from_str::<Value>(&call.arguments)
    };
    let outcome = match arguments {
        Err(e) => Outcome::error(
            format!("Could not read the arguments for {}", call.name),
            format!(
                "the arguments are not valid JSON ({e}); call {} again with a JSON object",
                call.name
            ),
        ),
        Ok(arguments) => match tools.confirm(&call.name, &arguments) {
            None => tools.run(&call.name, &arguments),
            Some(question) => {
                sink(Event::Confirm {
                    id: call.id.clone(),
                    question: question.clone(),
                });
                let answer = ask(&call.id);
                let yes = tokio::select! {
                    answer = answer => answer.unwrap_or(false),
                    _ = cancel.canceled() => false,
                };
                if yes {
                    tools.run(&call.name, &arguments)
                } else {
                    Outcome {
                        summary: format!("Not done: {question}"),
                        result: json!({ "declined": "The player said no. Do not do it; ask what they would like instead." }),
                        ok: false,
                        undo: None,
                    }
                }
            }
        },
    };
    Item::Call {
        id: call.id.clone(),
        name: call.name.clone(),
        arguments: call.arguments.clone(),
        summary: outcome.summary,
        result: outcome.result.to_string(),
        ok: outcome.ok,
        undo: outcome.undo,
        undone: false,
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
pub(crate) mod script {
    //! A model that answers from a script, recording what it was asked.

    use super::*;

    /// Plays its steps in order, each text in two pieces; past the end
    /// it says "Done.". A step of `Err` fails that request.
    pub struct Scripted {
        pub steps: Mutex<std::collections::VecDeque<Result<Step, Problem>>>,
        pub seen: Mutex<Vec<Vec<Value>>>,
        /// Never answers, so Stop can be tested.
        pub hang: bool,
    }

    impl Scripted {
        pub fn new(steps: Vec<Result<Step, Problem>>) -> Self {
            Self {
                steps: Mutex::new(steps.into()),
                seen: Mutex::new(Vec::new()),
                hang: false,
            }
        }
    }

    /// A step that calls one tool.
    pub fn call(id: &str, name: &str, arguments: Value) -> Result<Step, Problem> {
        Ok(Step {
            text: String::new(),
            calls: vec![CallRequest {
                id: id.into(),
                name: name.into(),
                arguments: arguments.to_string(),
            }],
        })
    }

    /// A step that only talks.
    pub fn say(text: &str) -> Result<Step, Problem> {
        Ok(Step {
            text: text.into(),
            calls: Vec::new(),
        })
    }

    impl Model for Scripted {
        fn complete<'a>(
            &'a self,
            messages: &'a [Value],
            _tools: &'a [Value],
            on_text: &'a mut (dyn FnMut(&str) + Send),
        ) -> BoxFuture<'a, Result<Step, Problem>> {
            Box::pin(async move {
                lock(&self.seen).push(messages.to_vec());
                if self.hang {
                    on_text("Thinking about");
                    std::future::pending::<()>().await;
                }
                let step = lock(&self.steps)
                    .pop_front()
                    .unwrap_or_else(|| say("Done."));
                if let Ok(step) = &step {
                    let half = step.text.len() / 2;
                    let half = (0..=half)
                        .rev()
                        .find(|i| step.text.is_char_boundary(*i))
                        .unwrap_or(0);
                    on_text(&step.text[..half]);
                    on_text(&step.text[half..]);
                }
                step
            })
        }
    }

    /// Runs a conversation to its end and returns the events.
    pub fn converse_with(
        model: &dyn Model,
        tools: &mut dyn Toolbox,
        thread: &Mutex<Thread>,
        answer: bool,
    ) -> Vec<Event> {
        let events = Mutex::new(Vec::new());
        let sink = |e: Event| lock(&events).push(e);
        let cancel = Cancel::default();
        let ask = move |_: &str| {
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(answer);
            rx
        };
        tauri::async_runtime::block_on(converse(model, tools, thread, &sink, &cancel, &ask));
        events.into_inner().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::script::*;
    use super::*;

    /// Two tools: `count` adds one, `wipe` asks first.
    struct Counter(u32);

    impl Toolbox for Counter {
        fn specs(&self) -> Vec<Value> {
            vec![json!({"type": "function", "function": {"name": "count"}})]
        }
        fn confirm(&self, name: &str, _: &Value) -> Option<String> {
            (name == "wipe").then(|| "Wipe the count?".to_string())
        }
        fn run(&mut self, name: &str, args: &Value) -> Outcome {
            match name {
                "count" => {
                    self.0 += args["by"].as_u64().unwrap_or(1) as u32;
                    Outcome {
                        summary: format!("Counted to {}", self.0),
                        result: json!({ "count": self.0 }),
                        ok: true,
                        undo: Some(u64::from(self.0)),
                    }
                }
                "wipe" => {
                    self.0 = 0;
                    Outcome {
                        summary: "Wiped".into(),
                        result: json!({}),
                        ok: true,
                        undo: None,
                    }
                }
                _ => Outcome::error("No such tool", "no such tool"),
            }
        }
        fn system(&self) -> String {
            format!("count is {}", self.0)
        }
    }

    fn thread_with(text: &str) -> Mutex<Thread> {
        let mut thread = Thread::default();
        thread.push(Item::User { text: text.into() });
        Mutex::new(thread)
    }

    #[test]
    fn a_reply_streams_and_lands_in_the_thread() {
        let model = Scripted::new(vec![say("Hello there.")]);
        let thread = thread_with("hi");
        let events = converse_with(&model, &mut NoTools("sys".into()), &thread, true);
        let texts: String = events
            .iter()
            .filter_map(|e| match e {
                Event::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, "Hello there.");
        assert!(matches!(events.last(), Some(Event::Done)));
        let items = &thread.lock().unwrap().items;
        assert_eq!(
            items[1],
            Item::Reply {
                text: "Hello there.".into(),
                stopped: false
            }
        );
        let seen = model.seen.lock().unwrap();
        assert_eq!(seen[0][0], json!({"role": "system", "content": "sys"}));
        assert_eq!(seen[0][1], json!({"role": "user", "content": "hi"}));
    }

    #[test]
    fn tools_run_and_their_results_go_back_to_the_model() {
        let model = Scripted::new(vec![
            call("a", "count", json!({"by": 2})),
            call("b", "nothing", json!({})),
            say("Counted to two."),
        ]);
        let thread = thread_with("count by two");
        let mut tools = Counter(0);
        converse_with(&model, &mut tools, &thread, true);
        assert_eq!(tools.0, 2);
        let items = thread.lock().unwrap().items.clone();
        assert!(
            matches!(&items[1], Item::Call { summary, ok: true, undo: Some(2), .. } if summary == "Counted to 2")
        );
        assert!(matches!(&items[2], Item::Call { ok: false, .. }));
        assert!(matches!(&items[3], Item::Reply { text, .. } if text == "Counted to two."));
        // The third request carries both calls and both results, the
        // failure as an error the model can read.
        let seen = model.seen.lock().unwrap();
        let last = &seen[2];
        assert_eq!(last[2]["tool_calls"][0]["function"]["name"], "count");
        assert_eq!(last[3]["role"], "tool");
        assert_eq!(last[3]["tool_call_id"], "a");
        assert!(last[3]["content"].as_str().unwrap().contains("\"count\":2"));
        // Two steps of calls with no words between read as one message.
        assert_eq!(last[2]["tool_calls"][1]["function"]["name"], "nothing");
        assert!(last[4]["content"].as_str().unwrap().contains("error"));
        // The system prompt is asked for again each step.
        assert_eq!(last[0]["content"], "count is 2");
    }

    #[test]
    fn bad_arguments_go_back_as_an_error_the_model_can_fix() {
        let model = Scripted::new(vec![
            Ok(Step {
                text: String::new(),
                calls: vec![CallRequest {
                    id: "x".into(),
                    name: "count".into(),
                    arguments: "{by: two".into(),
                }],
            }),
            call("y", "count", json!({"by": 2})),
            say("Fixed it."),
        ]);
        let thread = thread_with("count");
        let mut tools = Counter(0);
        converse_with(&model, &mut tools, &thread, true);
        assert_eq!(tools.0, 2);
        let items = thread.lock().unwrap().items.clone();
        assert!(
            matches!(&items[1], Item::Call { ok: false, result, .. } if result.contains("not valid JSON"))
        );
    }

    #[test]
    fn a_destructive_tool_waits_for_a_yes() {
        for answer in [true, false] {
            let model = Scripted::new(vec![call("w", "wipe", json!({})), say("ok")]);
            let thread = thread_with("wipe it");
            let mut tools = Counter(5);
            let events = converse_with(&model, &mut tools, &thread, answer);
            assert!(events.iter().any(
                |e| matches!(e, Event::Confirm { question, .. } if question == "Wipe the count?")
            ));
            assert_eq!(tools.0, if answer { 0 } else { 5 });
            let items = thread.lock().unwrap().items.clone();
            assert!(matches!(&items[1], Item::Call { ok, .. } if *ok == answer));
        }
    }

    #[test]
    fn a_failure_shows_in_the_thread_and_retry_asks_again() {
        let model = Scripted::new(vec![
            Err(Problem::new(ProblemKind::RateLimited, "slow down")),
            say("Second time lucky."),
        ]);
        let thread = thread_with("hi");
        converse_with(&model, &mut NoTools(String::new()), &thread, true);
        assert!(matches!(
            &thread.lock().unwrap().items[1],
            Item::Error { problem } if problem.kind == ProblemKind::RateLimited
        ));
        // The failure is never sent to the model.
        assert!(thread.lock().unwrap().rewind_for_retry());
        assert_eq!(thread.lock().unwrap().items.len(), 1);
        converse_with(&model, &mut NoTools(String::new()), &thread, true);
        assert!(
            matches!(&thread.lock().unwrap().items[1], Item::Reply { text, .. } if text == "Second time lucky.")
        );
    }

    #[test]
    fn stop_keeps_what_was_written_and_ends_the_turn() {
        let mut model = Scripted::new(vec![]);
        model.hang = true;
        let thread = thread_with("hi");
        let events = Mutex::new(Vec::new());
        let sink = |e: Event| lock(&events).push(e);
        let cancel = Cancel::default();
        let ask = |_: &str| oneshot::channel().1;
        let mut tools = NoTools(String::new());
        tauri::async_runtime::block_on(async {
            let run = converse(&model, &mut tools, &thread, &sink, &cancel, &ask);
            let stop = async {
                tokio::task::yield_now().await;
                cancel.cancel();
            };
            tokio::join!(run, stop);
        });
        assert_eq!(
            thread.lock().unwrap().items[1],
            Item::Reply {
                text: "Thinking about".into(),
                stopped: true
            }
        );
        assert!(matches!(lock(&events).last(), Some(Event::Done)));
    }

    #[test]
    fn a_model_that_never_stops_calling_tools_is_stopped() {
        let model = Scripted::new(
            (0..MAX_STEPS + 2)
                .map(|i| call(&i.to_string(), "count", json!({})))
                .collect(),
        );
        let thread = thread_with("go");
        let mut tools = Counter(0);
        converse_with(&model, &mut tools, &thread, true);
        assert_eq!(tools.0 as usize, MAX_STEPS);
        assert!(matches!(
            thread.lock().unwrap().items.last(),
            Some(Item::Error { .. })
        ));
    }

    #[test]
    fn the_thread_survives_a_relaunch_and_a_bad_file_is_an_empty_thread() {
        let dir = std::env::temp_dir().join(format!("cantina-thread-{}", std::process::id()));
        let path = dir.join("assistant-thread.json");
        let mut thread = Thread::load(Some(path.clone()));
        assert!(thread.items.is_empty());
        thread.push(Item::User { text: "hi".into() });
        thread.push(Item::Reply {
            text: "hello".into(),
            stopped: false,
        });
        assert_eq!(Thread::load(Some(path.clone())).items, thread.items);
        thread.clear();
        assert!(Thread::load(Some(path.clone())).items.is_empty());
        std::fs::write(&path, "not json").unwrap();
        assert!(Thread::load(Some(path)).items.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_items_are_left_out_from_a_message_of_the_players() {
        let mut items = Vec::new();
        for n in 0..CONTEXT_ITEMS {
            items.push(Item::User {
                text: n.to_string(),
            });
            items.push(Item::Reply {
                text: n.to_string(),
                stopped: false,
            });
        }
        let out = messages("s", &items);
        assert!(out.len() <= CONTEXT_ITEMS + 1);
        assert_eq!(out[1]["role"], "user");
    }
}
