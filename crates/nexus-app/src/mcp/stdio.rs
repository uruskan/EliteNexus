//! `EliteNexus --mcp`: the Model Context Protocol on stdin and stdout, for an AI agent on this computer that starts it
//! (Claude Code, Claude Desktop, any MCP client). It opens no window, takes no lock, writes no file and prints nothing
//! but MCP messages, one a line: it lists the tools offered to agents and passes each call to the running Elite Nexus
//! over [`super::endpoint`].
//!
//! Both eras of the protocol: revision 2026-07-28, which is stateless (`server/discover`, and a `_meta` with the
//! protocol version on every request), and the revisions before it (2024-11-05 to 2025-11-25), which begin with
//! `initialize`. An `initialize` selects the older semantics for the rest of the process, as 2026-07-28 describes for a
//! server that serves both.

use super::{WireCall, WireReply};
use crate::ai_tools::{Caller, TOOLS};
use crossbeam_channel::{RecvTimeoutError, Sender};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, Read, Write};
use std::time::{Duration, Instant};

const MODERN: &str = "2026-07-28";
/// The older revisions, newest first: an `initialize` asking for another gets the newest.
const LEGACY: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
const CAPABILITIES_KEY: &str = "io.modelcontextprotocol/clientCapabilities";
const SERVER_INFO_KEY: &str = "io.modelcontextprotocol/serverInfo";
/// How long an agent may keep the tool list: it changes only with Elite Nexus itself.
const TTL_MS: u64 = 3_600_000;
/// Calls waiting their turn at most (an agent may send several at once; Elite Nexus answers one at a time).
const WAITING: usize = 16;
/// A call the app hasn't answered by then is given up: its own deadline, and a margin.
const GIVE_UP: Duration = Duration::from_secs(super::DEADLINE.as_secs() + 30);

const INSTRUCTIONS: &str = "Elite Nexus is a companion app for the game Elite Dangerous. These tools read the commander's game state from their journal (ships, cargo, materials, ranks, engineers, where they have been), Elite Nexus' local galaxy and market database, the game data it ships with, and Spansh. They only read: they change nothing in the game or in Elite Nexus. They answer while Elite Nexus runs with Settings › AI agents switched on, one call at a time, and each answer says where its data comes from and what it can't show.";

const NOT_RUNNING: &str = "Elite Nexus isn't answering: it isn't running, or Settings › AI agents is off. Start Elite Nexus and switch AI agents on, then ask again.";

/// Said when what answers under Elite Nexus' name belongs to another account: nothing is sent to it.
const NOT_YOURS: &str = "Something that isn't this user's Elite Nexus answers under its name on this computer, so nothing was sent to it. Start Elite Nexus and switch Settings › AI agents on, then ask again.";

/// What the main loop is told.
pub enum Event {
    /// A line from stdin.
    Line(String),
    /// stdin closed: the agent is done.
    Eof,
    /// The app's answer to call number `seq`.
    Done { seq: u64, reply: Result<Value, String> },
}

/// Runs until stdin closes; returns the exit code.
pub fn run() -> i32 {
    let (tx, rx) = crossbeam_channel::unbounded::<Event>();
    let reader = tx.clone();
    let started = std::thread::Builder::new().name("mcp-stdin".into()).spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if reader.send(Event::Line(line)).is_err() {
                return;
            }
        }
        let _ = reader.send(Event::Eof);
    });
    if started.is_err() {
        return 1;
    }
    let mut out = std::io::stdout().lock();
    // No endpoint (Windows wouldn't say whose this is): every call is told Elite Nexus isn't answering.
    let mut s = Session::new(super::endpoint().unwrap_or_default(), tx);
    loop {
        let ev = match s.deadline() {
            Some(at) => match rx.recv_deadline(at) {
                Ok(ev) => ev,
                Err(RecvTimeoutError::Timeout) => {
                    s.give_up(&mut out);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(ev) => ev,
                Err(_) => break,
            },
        };
        // Exits as soon as the agent closes stdin, as the protocol asks: a call still running is dropped.
        if matches!(ev, Event::Eof) {
            break;
        }
        s.handle(ev, &mut out);
    }
    0
}

/// A tools/call waiting its turn.
struct Call {
    id: Value,
    name: &'static str,
    args: Value,
    modern: bool,
}

/// The call being answered.
struct Busy {
    id: Value,
    seq: u64,
    modern: bool,
    since: Instant,
}

/// One agent's conversation with this process.
pub struct Session {
    endpoint: String,
    events: Sender<Event>,
    /// The revision an `initialize` agreed on: the older semantics from then on.
    legacy: Option<&'static str>,
    busy: Option<Busy>,
    waiting: VecDeque<Call>,
    seq: u64,
}

impl Session {
    /// Answers calls through `endpoint`; the app's answers come back on `events`.
    pub fn new(endpoint: String, events: Sender<Event>) -> Self {
        Session { endpoint, events, legacy: None, busy: None, waiting: VecDeque::new(), seq: 0 }
    }

    /// When the call being answered is given up.
    fn deadline(&self) -> Option<Instant> {
        self.busy.as_ref().map(|b| b.since + GIVE_UP)
    }

    pub fn handle(&mut self, ev: Event, out: &mut impl Write) {
        match ev {
            Event::Line(line) => self.on_line(&line, out),
            Event::Done { seq, reply } => self.on_done(seq, reply, out),
            Event::Eof => {}
        }
    }

    fn give_up(&mut self, out: &mut impl Write) {
        if let Some(b) = self.busy.take() {
            send(out, &response(b.id, call_result(Err(format!("Elite Nexus didn't answer within {} s.", GIVE_UP.as_secs())), b.modern)));
        }
        self.next();
    }

    fn on_line(&mut self, line: &str, out: &mut impl Write) {
        // A client written in .NET can start stdin with a UTF-8 byte-order mark (its console's encoding writes one).
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            return;
        }
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            return send(out, &error(Value::Null, -32700, "Parse error", None));
        };
        let Value::Object(m) = msg else {
            return send(out, &error(Value::Null, -32600, "Invalid request: one JSON-RPC message a line, no batches", None));
        };
        // A response: this server never asks anything.
        let Some(method) = m.get("method").and_then(Value::as_str) else { return };
        let params = m.get("params").cloned().unwrap_or_else(|| json!({}));
        match m.get("id") {
            None => self.on_notification(method, &params),
            Some(id) if id.is_string() || id.is_i64() || id.is_u64() => self.on_request(id.clone(), method, &params, out),
            Some(_) => send(out, &error(Value::Null, -32600, "Invalid request: an id is a string or an integer", None)),
        }
    }

    fn on_notification(&mut self, method: &str, params: &Value) {
        // notifications/initialized, and the rest: nothing to do.
        if method != "notifications/cancelled" {
            return;
        }
        let Some(id) = params.get("requestId") else { return };
        if self.busy.as_ref().is_some_and(|b| &b.id == id) {
            // Its answer is dropped when it comes: nothing more is said about a cancelled call.
            self.busy = None;
            self.next();
        } else {
            self.waiting.retain(|c| &c.id != id);
        }
    }

    fn on_request(&mut self, id: Value, method: &str, params: &Value, out: &mut impl Write) {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
                let version = LEGACY.iter().copied().find(|v| *v == asked).unwrap_or(LEGACY[0]);
                self.legacy = Some(version);
                send(out, &response(id, json!({ "protocolVersion": version, "capabilities": { "tools": { "listChanged": false } }, "serverInfo": server_info(), "instructions": INSTRUCTIONS })));
            }
            // The older revisions' keep-alive; 2026-07-28 has none.
            "ping" => send(out, &response(id, json!({}))),
            "server/discover" => match meta_version(params) {
                Some(v) if v != MODERN => send(out, &unsupported(id, v)),
                _ => {
                    let mut r = json!({ "resultType": "complete", "supportedVersions": [MODERN], "capabilities": { "tools": {} }, "instructions": INSTRUCTIONS, "ttlMs": TTL_MS, "cacheScope": "public" });
                    r["_meta"] = info_meta();
                    send(out, &response(id, r));
                }
            },
            "tools/list" | "tools/call" => match self.era(&id, params) {
                Err(e) => send(out, &e),
                Ok(modern) if method == "tools/list" => send(out, &tool_list(id, params, modern)),
                Ok(modern) => self.call(id, params, modern, out),
            },
            _ => send(out, &error(id, -32601, &format!("Method not found: {method}"), None)),
        }
    }

    /// The era a tools request is served in: 2026-07-28 when its `_meta` says so, the older one after an `initialize`.
    /// Anything else gets the error 2026-07-28 gives.
    fn era(&self, id: &Value, params: &Value) -> Result<bool, Value> {
        match (meta_version(params), self.legacy) {
            (Some(v), _) if v == MODERN => {
                if params.get("_meta").and_then(|m| m.get(CAPABILITIES_KEY)).is_some_and(Value::is_object) {
                    Ok(true)
                } else {
                    Err(error(id.clone(), -32602, &format!("Invalid params: _meta needs {CAPABILITIES_KEY}"), None))
                }
            }
            (Some(v), None) => Err(unsupported(id.clone(), v)),
            (_, Some(_)) => Ok(false),
            (None, None) => Err(error(id.clone(), -32602, &format!("Invalid params: _meta needs {VERSION_KEY} ({MODERN}); clients of earlier revisions begin with initialize"), None)),
        }
    }

    fn call(&mut self, id: Value, params: &Value, modern: bool, out: &mut impl Write) {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return send(out, &error(id, -32602, "Invalid params: name is required", None));
        };
        let Some(tool) = crate::ai_tools::tool(name).filter(|t| t.offered_to(Caller::Mcp)) else {
            return send(out, &error(id, -32602, &format!("Unknown tool: {name}"), None));
        };
        let args = match params.get("arguments") {
            None | Some(Value::Null) => json!({}),
            Some(a @ Value::Object(_)) => a.clone(),
            Some(_) => return send(out, &error(id, -32602, "Invalid params: arguments is an object", None)),
        };
        if self.waiting.len() >= WAITING {
            return send(out, &response(id, call_result(Err(format!("{WAITING} calls are already waiting: Elite Nexus answers one at a time.")), modern)));
        }
        self.waiting.push_back(Call { id, name: tool.name, args, modern });
        self.next();
    }

    /// Starts the next waiting call, unless one is being answered.
    fn next(&mut self) {
        if self.busy.is_some() {
            return;
        }
        let Some(c) = self.waiting.pop_front() else { return };
        self.seq += 1;
        let seq = self.seq;
        self.busy = Some(Busy { id: c.id, seq, modern: c.modern, since: Instant::now() });
        let (endpoint, events) = (self.endpoint.clone(), self.events.clone());
        let spawned = std::thread::Builder::new().name("mcp-call".into()).spawn(move || {
            let reply = ask_app(&endpoint, c.name, &c.args);
            let _ = events.send(Event::Done { seq, reply });
        });
        if spawned.is_err() {
            let _ = self.events.send(Event::Done { seq, reply: Err("Couldn't start the call.".into()) });
        }
    }

    fn on_done(&mut self, seq: u64, reply: Result<Value, String>, out: &mut impl Write) {
        // A cancelled or given-up call's answer: nothing more is said about it.
        if self.busy.as_ref().is_none_or(|b| b.seq != seq) {
            return;
        }
        if let Some(b) = self.busy.take() {
            send(out, &response(b.id, call_result(reply, b.modern)));
        }
        self.next();
    }
}

fn meta_version(params: &Value) -> Option<&str> {
    params.get("_meta").and_then(|m| m.get(VERSION_KEY)).and_then(Value::as_str)
}

fn server_info() -> Value {
    json!({ "name": "elite-nexus", "title": "Elite Nexus", "version": env!("CARGO_PKG_VERSION") })
}

/// `{"io.modelcontextprotocol/serverInfo": …}`, which 2026-07-28 asks to go on every result.
fn info_meta() -> Value {
    let mut m = serde_json::Map::new();
    m.insert(SERVER_INFO_KEY.into(), server_info());
    Value::Object(m)
}

/// The tools offered to agents, read-only every one; the online ones say so (openWorldHint).
fn tool_list(id: Value, params: &Value, modern: bool) -> Value {
    if params.get("cursor").is_some_and(|c| !c.is_null()) {
        return error(id, -32602, "Invalid cursor", None);
    }
    let tools: Vec<Value> = TOOLS
        .iter()
        .filter(|t| t.offered_to(Caller::Mcp))
        .map(|t| json!({ "name": t.name, "title": t.title, "description": t.description, "inputSchema": (t.params)(), "annotations": { "readOnlyHint": true, "openWorldHint": t.online } }))
        .collect();
    let mut r = json!({ "tools": tools });
    if modern {
        r["resultType"] = json!("complete");
        r["ttlMs"] = json!(TTL_MS);
        r["cacheScope"] = json!("public");
        r["_meta"] = info_meta();
    }
    response(id, r)
}

/// A tool's answer as MCP's result: its JSON as text, or what went wrong with `isError`.
fn call_result(reply: Result<Value, String>, modern: bool) -> Value {
    let (text, is_error) = match reply {
        Ok(v) => (v.to_string(), false),
        Err(e) => (e, true),
    };
    let mut r = json!({ "content": [{ "type": "text", "text": text }], "isError": is_error });
    if modern {
        r["resultType"] = json!("complete");
        r["_meta"] = info_meta();
    }
    r
}

fn response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut e = json!({ "code": code, "message": message });
    if let Some(d) = data {
        e["data"] = d;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": e })
}

/// 2026-07-28's UnsupportedProtocolVersionError.
fn unsupported(id: Value, asked: &str) -> Value {
    error(id, -32022, "Unsupported protocol version", Some(json!({ "supported": [MODERN], "requested": asked })))
}

/// Writes one message as one line: serde_json puts no newline inside it.
fn send(out: &mut impl Write, msg: &Value) {
    if let Ok(mut text) = serde_json::to_string(msg) {
        text.push('\n');
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    }
}

/// One call to the running app: a line of JSON there, a line back.
fn ask_app(endpoint: &str, name: &str, args: &Value) -> Result<Value, String> {
    let mut conn = connect(endpoint).map_err(|e| if e.kind() == std::io::ErrorKind::PermissionDenied { NOT_YOURS.to_string() } else { NOT_RUNNING.to_string() })?;
    let mut call = serde_json::to_string(&WireCall { tool: name.to_string(), arguments: args.clone() }).map_err(|e| e.to_string())?;
    call.push('\n');
    conn.write_all(call.as_bytes()).and_then(|()| conn.flush()).map_err(|e| format!("Lost the connection to Elite Nexus: {e}"))?;
    let mut reply = String::new();
    std::io::BufReader::new(&mut conn).read_line(&mut reply).map_err(|e| format!("Lost the connection to Elite Nexus: {e}"))?;
    if reply.trim().is_empty() {
        return Err("Elite Nexus closed the connection without an answer: was Settings › AI agents switched off?".into());
    }
    match serde_json::from_str::<WireReply>(&reply) {
        Ok(WireReply::Ok(v)) => Ok(v),
        Ok(WireReply::Error(e)) => Err(e),
        Err(_) => Err("Elite Nexus sent an answer that couldn't be read.".into()),
    }
}

/// Opens the app's pipe, waiting a moment when every instance of it is busy. At identification level, so whoever
/// answers can't act as the user; and only a pipe the user owns is used ([`super::win`]).
#[cfg(windows)]
fn connect(endpoint: &str) -> std::io::Result<impl Read + Write> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    // ERROR_PIPE_BUSY: the app answers another connection right now.
    const PIPE_BUSY: i32 = 231;
    let until = Instant::now() + Duration::from_secs(5);
    let pipe = loop {
        match std::fs::OpenOptions::new().read(true).write(true).security_qos_flags(super::win::SECURITY_IDENTIFICATION).open(endpoint) {
            Err(e) if e.raw_os_error() == Some(PIPE_BUSY) && Instant::now() < until => std::thread::sleep(Duration::from_millis(50)),
            r => break r?,
        }
    };
    let user = super::win::user_sid().ok_or_else(|| std::io::Error::other("Windows didn't say which account this is"))?;
    if !super::win::owned_by(pipe.as_raw_handle(), &user) {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "the pipe isn't the user's"));
    }
    Ok(pipe)
}

#[cfg(not(windows))]
fn connect(endpoint: &str) -> std::io::Result<impl Read + Write> {
    std::os::unix::net::UnixStream::connect(endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session whose app isn't there, and what it writes.
    struct Harness {
        s: Session,
        events: crossbeam_channel::Receiver<Event>,
        out: Vec<u8>,
    }

    impl Harness {
        fn new(endpoint: String) -> Self {
            let (tx, rx) = crossbeam_channel::unbounded();
            Harness { s: Session::new(endpoint, tx), events: rx, out: Vec::new() }
        }

        fn nowhere() -> Self {
            Harness::new(crate::mcp::test_endpoint("nobody-home"))
        }

        /// Sends a message and returns the lines written since.
        fn send(&mut self, msg: Value) -> Vec<Value> {
            self.s.handle(Event::Line(msg.to_string()), &mut self.out);
            self.take()
        }

        /// Waits for the app's answer to the call being made, and returns what was written.
        fn answer(&mut self) -> Vec<Value> {
            let ev = self.events.recv_timeout(Duration::from_secs(30)).expect("an answer");
            self.s.handle(ev, &mut self.out);
            self.take()
        }

        fn take(&mut self) -> Vec<Value> {
            let text = String::from_utf8(std::mem::take(&mut self.out)).unwrap();
            assert!(text.is_empty() || text.ends_with('\n'), "every message ends its line");
            text.lines().map(|l| serde_json::from_str(l).expect("one JSON message a line")).collect()
        }
    }

    fn modern(method: &str, id: i64, params: Value) -> Value {
        let mut p = params;
        p["_meta"] = json!({ VERSION_KEY: MODERN, CAPABILITIES_KEY: {}, "io.modelcontextprotocol/clientInfo": { "name": "test", "version": "1" } });
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": p })
    }

    #[test]
    fn an_older_client_begins_with_initialize() {
        let mut h = Harness::nowhere();
        let init = h.send(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } } }));
        let r = &init[0]["result"];
        assert_eq!((&r["protocolVersion"], &r["serverInfo"]["name"], &r["capabilities"]["tools"]["listChanged"]), (&json!("2025-06-18"), &json!("elite-nexus"), &json!(false)), "{r}");
        assert!(h.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_empty(), "notifications get no answer");
        let list = h.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));
        let tools = list[0]["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 20 && list[0]["result"].get("resultType").is_none(), "older shape: {list:?}");
        assert!(tools.iter().all(|t| t["annotations"]["readOnlyHint"] == true && t["inputSchema"]["type"] == "object"));
        assert!(!tools.iter().any(|t| t["name"] == "plan_route"), "tools that change something aren't offered");
        assert!(tools.iter().any(|t| t["name"] == "system_bodies" && t["annotations"]["openWorldHint"] == true));
        assert!(tools.iter().any(|t| t["name"] == "fleet" && t["annotations"]["openWorldHint"] == false));
        assert_eq!(h.send(json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" }))[0]["result"], json!({}));
        // A version it doesn't know gets the newest it does.
        let mut other = Harness::nowhere();
        let r = other.send(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2099-01-01" } }));
        assert_eq!(r[0]["result"]["protocolVersion"], LEGACY[0]);
    }

    #[test]
    fn a_2026_07_28_client_discovers_and_sends_meta() {
        let mut h = Harness::nowhere();
        let d = &h.send(modern("server/discover", 1, json!({})))[0]["result"];
        assert_eq!((&d["resultType"], &d["supportedVersions"], &d["cacheScope"]), (&json!("complete"), &json!([MODERN]), &json!("public")), "{d}");
        assert!(d["ttlMs"].is_u64() && d["capabilities"]["tools"].is_object() && d["_meta"][SERVER_INFO_KEY]["name"] == "elite-nexus", "{d}");
        let list = &h.send(modern("tools/list", 2, json!({})))[0]["result"];
        assert_eq!((&list["resultType"], &list["cacheScope"]), (&json!("complete"), &json!("public")), "{list}");
        assert!(list["ttlMs"].is_u64() && list["_meta"][SERVER_INFO_KEY].is_object());
        // Without the version, or with one it doesn't serve, the errors 2026-07-28 names.
        let bare = h.send(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {} }));
        assert_eq!(bare[0]["error"]["code"], -32602, "{bare:?}");
        let old = h.send(json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": { "_meta": { VERSION_KEY: "2025-06-18", CAPABILITIES_KEY: {} } } }));
        assert_eq!((&old[0]["error"]["code"], &old[0]["error"]["data"]["supported"], &old[0]["error"]["data"]["requested"]), (&json!(-32022), &json!([MODERN]), &json!("2025-06-18")));
        let no_caps = h.send(json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/list", "params": { "_meta": { VERSION_KEY: MODERN } } }));
        assert_eq!(no_caps[0]["error"]["code"], -32602);
        assert_eq!(h.send(modern("resources/list", 6, json!({})))[0]["error"]["code"], -32601);
        assert_eq!(h.send(modern("tools/list", 7, json!({ "cursor": "x" })))[0]["error"]["message"], "Invalid cursor");
    }

    #[test]
    fn calls_say_when_elite_nexus_isnt_answering() {
        let mut h = Harness::nowhere();
        assert!(h.send(modern("tools/call", 1, json!({ "name": "commander", "arguments": {} }))).is_empty(), "answered when the app has replied");
        let r = &h.answer()[0];
        assert_eq!((&r["id"], &r["result"]["isError"], &r["result"]["resultType"]), (&json!(1), &json!(true), &json!("complete")), "{r}");
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("Settings › AI agents"), "{r}");
        assert_eq!(h.send(modern("tools/call", 2, json!({ "name": "plan_route", "arguments": { "to": "Sol" } })))[0]["error"]["code"], -32602, "not offered to agents");
        assert_eq!(h.send(modern("tools/call", 3, json!({ "name": "fleet", "arguments": [] })))[0]["error"]["code"], -32602);
    }

    #[test]
    fn a_cancelled_call_gets_no_answer_and_the_next_one_does() {
        let mut h = Harness::nowhere();
        h.send(modern("tools/call", 1, json!({ "name": "commander" })));
        h.send(modern("tools/call", 2, json!({ "name": "fleet" })));
        assert!(h.send(json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 1, "reason": "test" } })).is_empty());
        // The first's answer comes and goes unsaid; then the second's.
        let mut said = h.answer();
        if said.is_empty() {
            said = h.answer();
        }
        assert_eq!(said.len(), 1, "{said:?}");
        assert_eq!(said[0]["id"], 2);
    }

    #[test]
    fn what_isnt_a_request_is_answered_as_json_rpc_says() {
        let mut h = Harness::nowhere();
        h.s.handle(Event::Line("{not json".into()), &mut h.out);
        assert_eq!(h.take()[0]["error"]["code"], -32700);
        assert_eq!(h.send(json!([{ "jsonrpc": "2.0", "id": 1, "method": "ping" }]))[0]["error"]["code"], -32600, "no batches");
        assert!(h.send(json!({ "jsonrpc": "2.0", "method": "notifications/whatever" })).is_empty());
        assert!(h.send(json!({ "jsonrpc": "2.0", "id": 9, "result": {} })).is_empty(), "a response to nothing asked");
        h.s.handle(Event::Line("   ".into()), &mut h.out);
        assert!(h.take().is_empty());
        // A byte-order mark before the first message, as a .NET client's stdin writes one.
        let init = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-11-25" } });
        h.s.handle(Event::Line(format!("\u{feff}{init}")), &mut h.out);
        assert_eq!(h.take()[0]["result"]["protocolVersion"], "2025-11-25");
    }
}
