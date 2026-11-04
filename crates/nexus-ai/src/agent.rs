//! The question → tools → answer loop.

use crate::chat::{Reply, ToolSpec, Turn, Usage};
use crate::provider::{AiError, ProviderConfig, ProviderKind};
use crate::{anthropic, openai};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::runtime::Handle;

/// Model rounds per question (each round may call several tools).
pub const MAX_ROUNDS: usize = 8;
/// Tool results are cut to this many characters before they go to the model.
pub const MAX_RESULT_CHARS: usize = 12_000;

/// Something that answers a conversation (a model service, or a script in tests).
pub trait Backend {
    fn complete(&self, system: &str, tools: &[ToolSpec], turns: &[Turn]) -> Result<Reply, AiError>;
}

/// Runs the tools the model asks for (the app's data: markets, journal, routes, …).
pub trait ToolHost {
    fn tools(&self) -> Vec<ToolSpec>;
    fn call(&self, name: &str, args: &Value) -> Result<Value, String>;
}

/// A model service reached over HTTP.
pub struct Remote<'a> {
    pub http: &'a reqwest::Client,
    pub rt: &'a Handle,
    pub cfg: &'a ProviderConfig,
    pub model: String,
    /// Setting it drops the request in flight (pass the flag given to [`ask`]).
    pub stop: Option<&'a AtomicBool>,
}

impl Backend for Remote<'_> {
    fn complete(&self, system: &str, tools: &[ToolSpec], turns: &[Turn]) -> Result<Reply, AiError> {
        match self.cfg.kind {
            ProviderKind::Claude => anthropic::complete(self, system, tools, turns),
            _ => openai::complete(self, system, tools, turns),
        }
    }
}

/// What the agent is doing (for a status line).
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Thinking,
    Tool(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub text: String,
    /// Tools used, in order, without repeats.
    pub tools_used: Vec<String>,
    pub usage: Usage,
}

/// Answers `question` in the context of `conv`, calling tools as the model requests. On error the
/// question is removed again, so the conversation stays valid for the next one. Setting `stop` ends it
/// before the next model request or tool call with [`AiError::Stopped`], the question removed as well.
pub fn ask(backend: &dyn Backend, system: &str, host: &dyn ToolHost, conv: &mut Vec<Turn>, question: String, stop: &AtomicBool, mut step: impl FnMut(Step)) -> Result<Answer, AiError> {
    let tools = host.tools();
    let start = conv.len();
    conv.push(Turn::User(question));
    let mut usage = Usage::default();
    let mut used: Vec<String> = Vec::new();
    for _ in 0..MAX_ROUNDS {
        if stop.load(Ordering::Relaxed) {
            conv.truncate(start);
            return Err(AiError::Stopped);
        }
        step(Step::Thinking);
        let reply = match backend.complete(system, &tools, conv) {
            Ok(r) => r,
            Err(e) => {
                conv.truncate(start);
                return Err(e);
            }
        };
        usage += reply.usage;
        let calls = reply.calls.clone();
        conv.push(Turn::Assistant { text: reply.text.clone(), calls: calls.clone(), raw: reply.raw });
        if calls.is_empty() {
            let mut text = reply.text.trim().to_string();
            if text.is_empty() {
                text = "I don't have an answer for that.".into();
            }
            if reply.truncated {
                text.push_str("\n\n(The answer was cut short.)");
            }
            return Ok(Answer { text, tools_used: used, usage });
        }
        for c in calls {
            if stop.load(Ordering::Relaxed) {
                conv.truncate(start);
                return Err(AiError::Stopped);
            }
            step(Step::Tool(c.name.clone()));
            if !used.contains(&c.name) {
                used.push(c.name.clone());
            }
            let (content, is_error) = match host.call(&c.name, &c.args) {
                Ok(v) => (clip(&v.to_string()), false),
                Err(e) => (e, true),
            };
            conv.push(Turn::ToolResult { call_id: c.id, name: c.name, content, is_error });
        }
    }
    conv.truncate(start);
    Ok(Answer { text: "That needed too many steps. Try asking something more specific.".into(), tools_used: used, usage })
}

fn clip(s: &str) -> String {
    if s.len() <= MAX_RESULT_CHARS {
        return s.to_string();
    }
    let mut end = MAX_RESULT_CHARS;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (truncated)", &s[..end])
}

/// Keeps only the last `questions` questions (with their answers and tool calls).
pub fn trim_history(conv: &mut Vec<Turn>, questions: usize) {
    let users: Vec<usize> = conv.iter().enumerate().filter(|(_, t)| matches!(t, Turn::User(_))).map(|(i, _)| i).collect();
    if users.len() > questions {
        let cut = users[users.len() - questions];
        conv.drain(..cut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ToolCall;
    use serde_json::json;
    use std::cell::RefCell;

    /// Replies from a script, recording what it was sent.
    struct Script {
        replies: RefCell<Vec<Result<Reply, AiError>>>,
        seen: RefCell<Vec<usize>>,
    }

    impl Backend for Script {
        fn complete(&self, _system: &str, _tools: &[ToolSpec], turns: &[Turn]) -> Result<Reply, AiError> {
            self.seen.borrow_mut().push(turns.len());
            self.replies.borrow_mut().remove(0)
        }
    }

    fn reply(text: &str, calls: Vec<ToolCall>) -> Result<Reply, AiError> {
        Ok(Reply { text: text.into(), calls, raw: json!({ "role": "assistant" }), usage: Usage { input_tokens: 100, output_tokens: 10, cached_tokens: 0 }, truncated: false })
    }

    struct Host;
    impl ToolHost for Host {
        fn tools(&self) -> Vec<ToolSpec> {
            vec![ToolSpec { name: "commander", description: "", parameters: json!({}) }]
        }
        fn call(&self, name: &str, args: &Value) -> Result<Value, String> {
            match name {
                "commander" => Ok(json!({ "system": "Sol", "asked": args })),
                other => Err(format!("unknown tool {other}")),
            }
        }
    }

    #[test]
    fn calls_tools_then_answers() {
        let script = Script {
            replies: RefCell::new(vec![
                reply("", vec![ToolCall { id: "1".into(), name: "commander".into(), args: json!({ "detail": true }) }, ToolCall { id: "2".into(), name: "nope".into(), args: json!({}) }]),
                reply("You are in Sol.", vec![]),
            ]),
            seen: RefCell::new(vec![]),
        };
        let mut conv = Vec::new();
        let mut steps = Vec::new();
        let a = ask(&script, "sys", &Host, &mut conv, "Where am I?".into(), &AtomicBool::new(false), |s| steps.push(s)).unwrap();
        assert_eq!(a.text, "You are in Sol.");
        assert_eq!(a.tools_used, vec!["commander", "nope"]);
        assert_eq!(a.usage.input_tokens, 200);
        assert_eq!(*script.seen.borrow(), vec![1, 4], "second request carries the call and both results");
        match &conv[2] {
            Turn::ToolResult { content, is_error, .. } => assert!(content.contains("Sol") && !is_error),
            t => panic!("{t:?}"),
        }
        assert!(matches!(&conv[3], Turn::ToolResult { is_error: true, .. }), "unknown tool reported as an error, not a crash");
        assert_eq!(steps, vec![Step::Thinking, Step::Tool("commander".into()), Step::Tool("nope".into()), Step::Thinking]);
    }

    #[test]
    fn errors_roll_back_and_history_trims() {
        let script = Script { replies: RefCell::new(vec![Err(AiError::Http("offline".into()))]), seen: RefCell::new(vec![]) };
        let mut conv = vec![Turn::User("old".into())];
        assert!(ask(&script, "s", &Host, &mut conv, "new".into(), &AtomicBool::new(false), |_| {}).is_err());
        assert_eq!(conv.len(), 1, "failed question removed");

        let mut conv = vec![
            Turn::User("q1".into()),
            Turn::Assistant { text: "a1".into(), calls: vec![], raw: json!({}) },
            Turn::User("q2".into()),
            Turn::Assistant { text: "a2".into(), calls: vec![], raw: json!({}) },
            Turn::User("q3".into()),
        ];
        trim_history(&mut conv, 2);
        assert!(matches!(&conv[0], Turn::User(q) if q == "q2"));
        assert!(clip(&"é".repeat(MAX_RESULT_CHARS)).chars().count() < MAX_RESULT_CHARS);
    }

    #[test]
    fn stop_ends_the_question_between_steps() {
        /// A tool during which the commander presses Stop.
        struct Stopper<'a>(&'a AtomicBool);
        impl ToolHost for Stopper<'_> {
            fn tools(&self) -> Vec<ToolSpec> {
                Host.tools()
            }
            fn call(&self, _name: &str, _args: &Value) -> Result<Value, String> {
                self.0.store(true, Ordering::Relaxed);
                Ok(json!("done"))
            }
        }
        let calls = vec![ToolCall { id: "1".into(), name: "commander".into(), args: json!({}) }, ToolCall { id: "2".into(), name: "commander".into(), args: json!({}) }];
        let script = Script { replies: RefCell::new(vec![reply("", calls), reply("never sent", vec![])]), seen: RefCell::new(vec![]) };
        let stop = AtomicBool::new(false);
        let mut conv = vec![Turn::User("old".into())];
        let mut steps = Vec::new();
        assert_eq!(ask(&script, "s", &Stopper(&stop), &mut conv, "new".into(), &stop, |s| steps.push(s)), Err(AiError::Stopped));
        assert_eq!(conv.len(), 1, "the stopped question is removed");
        assert_eq!(*script.seen.borrow(), vec![2], "no model request after the stop");
        assert_eq!(steps, vec![Step::Thinking, Step::Tool("commander".into())], "the second tool never ran");
    }
}
