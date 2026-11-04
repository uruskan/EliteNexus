//! AI agents over MCP (the Model Context Protocol). An agent on this computer (Claude Code, Claude Desktop, any MCP
//! client) starts `EliteNexus --mcp`, which speaks MCP on its stdin and stdout ([`stdio`]) and passes each tool call to
//! the running app over a named pipe (Windows) or a Unix socket: no network port, nothing another computer can reach.
//!
//! The app answers ([`McpService`]) only while Settings › AI agents is on, which it is not until the commander switches
//! it on. It answers with the read-only tools of [`crate::ai_tools::TOOLS`] that are offered to agents, one call at a
//! time, each within [`DEADLINE`], at most [`CALLS_PER_MINUTE`] a minute ([`ONLINE_CALLS_PER_MINUTE`] of them from
//! online services), with a Spansh client of its own so agents never hold up the app's own searches.

pub mod stdio;
#[cfg(windows)]
mod win;

use crate::ai_tools::{Caller, NexusTools};
use nexus_ai::ToolHost;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

/// A call not answered in this time gets an error instead.
pub const DEADLINE: Duration = Duration::from_secs(120);
/// Calls answered a minute at most, and of those from online services (Spansh, EDGalaxyData).
pub const CALLS_PER_MINUTE: usize = 30;
pub const ONLINE_CALLS_PER_MINUTE: usize = 10;
/// The longest request read from a connection.
const MAX_REQUEST: u64 = 256 * 1024;
/// Calls waiting their turn at most; more are refused as busy.
const QUEUE: usize = 4;

/// The name of whoever is logged in, as a file-name-safe tag ("user" when the system doesn't say).
#[cfg(not(windows))]
fn user_tag() -> String {
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
    let tag: String = user.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).take(40).collect();
    if tag.is_empty() { "user".into() } else { tag.to_ascii_lowercase() }
}

/// Where the app listens and `--mcp` connects: a named pipe named by this user's SID, which other computers can't open
/// and no other account shares ([`win`]).
#[cfg(windows)]
pub fn endpoint() -> Result<String, String> {
    let user = win::user_sid().ok_or("Windows didn't say which account this is.")?;
    Ok(format!(r"\\.\pipe\elite-nexus-mcp-{}", user.text))
}

/// Where the app listens and `--mcp` connects: a socket in this user's private runtime folder (`$XDG_RUNTIME_DIR` on
/// Linux, the per-user temporary folder on a Mac), readable by this user only.
#[cfg(not(windows))]
pub fn endpoint() -> Result<String, String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(std::env::temp_dir);
    Ok(dir.join(format!("elite-nexus-mcp-{}.sock", user_tag())).to_string_lossy().into_owned())
}

/// A pipe or socket of a test's own, which no Elite Nexus answers on.
#[cfg(test)]
pub(crate) fn test_endpoint(tag: &str) -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() % 1_000_000_000);
    if cfg!(windows) {
        format!(r"\\.\pipe\elite-nexus-mcp-test-{tag}-{}-{n}", std::process::id())
    } else {
        std::env::temp_dir().join(format!("enx-{tag}-{}-{n}.sock", std::process::id())).to_string_lossy().into_owned()
    }
}

/// One tool call, as `--mcp` sends it to the app: a line of JSON.
#[derive(Debug, Serialize, Deserialize)]
pub struct WireCall {
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

/// The app's answer: a line of JSON, `{"ok": …}` or `{"error": "…"}`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireReply {
    Ok(Value),
    Error(String),
}

/// What Settings › AI agents shows.
#[derive(Debug, Clone, Default)]
pub struct Status {
    /// Started this run (screenshot runs never start it).
    pub started: bool,
    /// Waiting for agents' calls.
    pub listening: bool,
    /// Why it couldn't start, or stopped.
    pub error: Option<String>,
    /// Calls answered this run, refused for going over the limits, and the last one answered.
    pub answered: u64,
    pub refused: u64,
    pub last: Option<(&'static str, Instant)>,
}

/// Calls a minute: all of them, and those from online services.
#[derive(Debug, Default)]
struct Limiter {
    calls: VecDeque<Instant>,
    online: VecDeque<Instant>,
}

impl Limiter {
    fn admit(&mut self, online: bool, now: Instant) -> Result<(), String> {
        let minute = Duration::from_secs(60);
        for q in [&mut self.calls, &mut self.online] {
            while q.front().is_some_and(|t| now.duration_since(*t) >= minute) {
                q.pop_front();
            }
        }
        let wait = |q: &VecDeque<Instant>| q.front().map_or(1, |t| minute.saturating_sub(now.duration_since(*t)).as_secs().max(1));
        if self.calls.len() >= CALLS_PER_MINUTE {
            return Err(format!("Elite Nexus answers AI agents at most {CALLS_PER_MINUTE} calls a minute: try again in {} s.", wait(&self.calls)));
        }
        if online && self.online.len() >= ONLINE_CALLS_PER_MINUTE {
            return Err(format!(
                "Elite Nexus answers AI agents at most {ONLINE_CALLS_PER_MINUTE} calls a minute from online services (Spansh, EDGalaxyData), to keep the load on them light: try again in {} s.",
                wait(&self.online)
            ));
        }
        self.calls.push_back(now);
        if online {
            self.online.push_back(now);
        }
        Ok(())
    }
}

/// A call for the worker thread.
struct Call {
    name: String,
    args: Value,
    reply: tokio::sync::oneshot::Sender<WireReply>,
}

/// The agents' side of the app: a listener on [`endpoint`] and a worker thread that runs the tools one call at a time.
#[derive(Default)]
pub struct McpService {
    running: Option<Running>,
    status: Arc<Mutex<Status>>,
}

struct Running {
    listener: tokio::task::JoinHandle<()>,
    calls: crossbeam_channel::Sender<Call>,
    /// New tools for the worker (the journal was read again): a channel of its own, so the window never waits behind
    /// queued calls to hand them over.
    tools: crossbeam_channel::Sender<NexusTools>,
    /// Set when switched off: calls still queued are answered that it's off, and none runs.
    off: Arc<AtomicBool>,
    endpoint: String,
}

impl McpService {
    /// Starts or stops answering agents, as Settings › AI agents says. `tools` is built only to start.
    pub fn apply(&mut self, on: bool, tools: impl FnOnce() -> NexusTools, rt: &tokio::runtime::Handle) {
        match (on, self.running.is_some()) {
            (true, false) => match endpoint() {
                Ok(at) => self.start_at(at, tools(), rt),
                Err(e) => {
                    let mut s = self.status.lock();
                    s.started = true;
                    s.error = Some(e);
                }
            },
            (false, true) => self.stop(),
            _ => {}
        }
    }

    /// The tools to answer from after the journal was read again (a new model). Never waits.
    pub fn attach(&self, tools: NexusTools) {
        if let Some(r) = &self.running {
            let _ = r.tools.send(tools);
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().clone()
    }

    /// Starts answering on `endpoint` (tests use one of their own).
    pub(crate) fn start_at(&mut self, endpoint: String, tools: NexusTools, rt: &tokio::runtime::Handle) {
        {
            let mut s = self.status.lock();
            s.started = true;
            s.listening = false;
            s.error = None;
        }
        let (calls, inbox) = crossbeam_channel::bounded::<Call>(QUEUE);
        let (new_tools, swaps) = crossbeam_channel::unbounded::<NexusTools>();
        let off = Arc::new(AtomicBool::new(false));
        let (status, stop) = (self.status.clone(), off.clone());
        let spawned = std::thread::Builder::new().name("mcp-tools".into()).spawn(move || work(tools, inbox, swaps, stop, status));
        if let Err(e) = spawned {
            self.status.lock().error = Some(format!("Couldn't start: {e}"));
            return;
        }
        let (status, to_worker, at) = (self.status.clone(), calls.clone(), endpoint.clone());
        let listener = rt.spawn(async move {
            if let Err(e) = listen(&at, to_worker, status.clone()).await {
                tracing::warn!("MCP: stopped listening on {at}: {e}");
                let mut s = status.lock();
                s.listening = false;
                s.error = Some(if e.kind() == std::io::ErrorKind::AddrInUse || e.kind() == std::io::ErrorKind::PermissionDenied {
                    "Another program already answers AI agents under Elite Nexus' name: is a second Elite Nexus running?".into()
                } else {
                    format!("Couldn't listen for AI agents: {e}")
                });
            }
        });
        tracing::info!("MCP: answering AI agents on {endpoint}");
        self.running = Some(Running { listener, calls, tools: new_tools, off, endpoint });
    }

    fn stop(&mut self) {
        let Some(r) = self.running.take() else { return };
        // Nothing more runs, not even what is queued; the worker ends once the last connection lets go of the channel.
        r.off.store(true, Ordering::Relaxed);
        r.listener.abort();
        drop(r.calls);
        #[cfg(not(windows))]
        let _ = std::fs::remove_file(&r.endpoint);
        #[cfg(windows)]
        let _ = r.endpoint;
        let mut s = self.status.lock();
        s.started = false;
        s.listening = false;
        s.error = None;
        tracing::info!("MCP: no longer answering AI agents");
    }
}

impl Drop for McpService {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The worker: one call at a time, within the limits, from the tools offered to agents; none once switched off.
fn work(mut tools: NexusTools, inbox: crossbeam_channel::Receiver<Call>, swaps: crossbeam_channel::Receiver<NexusTools>, off: Arc<AtomicBool>, status: Arc<Mutex<Status>>) {
    let mut limiter = Limiter::default();
    for Call { name, args, reply } in inbox {
        // The newest tools handed over (a new model), whatever came before.
        while let Ok(t) = swaps.try_recv() {
            tools = t;
        }
        // The caller gave up waiting (its deadline): not worth running.
        if reply.is_closed() {
            continue;
        }
        if off.load(Ordering::Relaxed) {
            let _ = reply.send(WireReply::Error("Elite Nexus stopped answering AI agents (Settings › AI agents was switched off).".into()));
            continue;
        }
        let answer = match crate::ai_tools::tool(&name).filter(|t| t.offered_to(Caller::Mcp)) {
            None => WireReply::Error(format!("Unknown tool \"{name}\".")),
            Some(t) => match limiter.admit(t.online, Instant::now()) {
                Err(e) => {
                    status.lock().refused += 1;
                    WireReply::Error(e)
                }
                Ok(()) => {
                    // A tool that panics answers with an error; the worker keeps answering.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tools.call(t.name, &args)));
                    let mut s = status.lock();
                    s.answered += 1;
                    s.last = Some((t.name, Instant::now()));
                    match result {
                        Ok(Ok(v)) => WireReply::Ok(v),
                        Ok(Err(e)) => WireReply::Error(e),
                        Err(_) => WireReply::Error(format!("{} failed inside Elite Nexus (written to nexus.log).", t.name)),
                    }
                }
            },
        };
        let _ = reply.send(answer);
    }
}

/// Waits for agents' connections and answers each in a task of its own.
#[cfg(windows)]
async fn listen(name: &str, jobs: crossbeam_channel::Sender<Call>, status: Arc<Mutex<Status>>) -> std::io::Result<()> {
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    // Every instance with the user as owner and only the user let in.
    let user = win::user_sid().ok_or_else(|| std::io::Error::other("Windows didn't say which account this is"))?;
    let owner_only = win::OwnerOnly::new(&user).ok_or_else(|| std::io::Error::other("the pipe's security descriptor couldn't be made"))?;
    let create = |first: bool| -> std::io::Result<NamedPipeServer> {
        let mut attributes = owner_only.attributes();
        // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES whose descriptor lives as long as `owner_only`.
        unsafe { ServerOptions::new().first_pipe_instance(first).reject_remote_clients(true).create_with_security_attributes_raw(name, (&raw mut attributes).cast()) }
    };
    // The first instance must be ours: another program holding the name can't stand in for Elite Nexus.
    let mut server = create(true)?;
    status.lock().listening = true;
    loop {
        let connected = server.connect().await;
        // A new instance for the next agent, whatever became of this one.
        let next = create(false)?;
        let conn = std::mem::replace(&mut server, next);
        match connected {
            Ok(()) => drop(tokio::spawn(serve(conn, jobs.clone()))),
            // An agent that came and went before it was answered: the next one is waited for all the same.
            Err(e) => tracing::debug!("MCP: a connection failed: {e}"),
        }
    }
}

/// Waits for agents' connections and answers each in a task of its own.
#[cfg(not(windows))]
async fn listen(path: &str, jobs: crossbeam_channel::Sender<Call>, status: Arc<Mutex<Status>>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // A socket left behind by a run that ended abruptly answers nobody: it goes. One that answers is another app's.
    if std::path::Path::new(path).exists() {
        if tokio::net::UnixStream::connect(path).await.is_ok() {
            return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "in use"));
        }
        std::fs::remove_file(path)?;
    }
    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    status.lock().listening = true;
    loop {
        match listener.accept().await {
            Ok((conn, _)) => drop(tokio::spawn(serve(conn, jobs.clone()))),
            // Out of file handles, say: wait a moment rather than spin, and go on listening.
            Err(e) => {
                tracing::debug!("MCP: a connection failed: {e}");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

/// One connection: a call in, its answer out.
async fn serve<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(conn: S, jobs: crossbeam_channel::Sender<Call>) {
    let (read, mut write) = tokio::io::split(conn);
    let mut line = String::new();
    let got = tokio::time::timeout(Duration::from_secs(10), BufReader::new(read.take(MAX_REQUEST)).read_line(&mut line)).await;
    if !matches!(got, Ok(Ok(n)) if n > 0) {
        return;
    }
    let reply = match serde_json::from_str::<WireCall>(&line) {
        Ok(call) => answer(call, &jobs).await,
        Err(e) => WireReply::Error(format!("Unreadable call: {e}")),
    };
    let mut text = serde_json::to_string(&reply).unwrap_or_else(|_| r#"{"error":"The answer couldn't be written."}"#.to_string());
    text.push('\n');
    let _ = write.write_all(text.as_bytes()).await;
    let _ = write.shutdown().await;
}

/// Hands a call to the worker and waits for it, up to [`DEADLINE`].
async fn answer(call: WireCall, jobs: &crossbeam_channel::Sender<Call>) -> WireReply {
    let (reply, answered) = tokio::sync::oneshot::channel();
    let args = if call.arguments.is_null() { Value::Object(Default::default()) } else { call.arguments };
    if jobs.try_send(Call { name: call.tool, args, reply }).is_err() {
        return WireReply::Error("Elite Nexus is busy with other agents' calls: try again in a moment.".into());
    }
    match tokio::time::timeout(DEADLINE, answered).await {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => WireReply::Error("Elite Nexus stopped answering AI agents (Settings › AI agents was switched off).".into()),
        Err(_) => WireReply::Error(format!("Elite Nexus didn't finish within {} s.", DEADLINE.as_secs())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limits_count_a_minute() {
        let mut l = Limiter::default();
        let t0 = Instant::now();
        for i in 0..ONLINE_CALLS_PER_MINUTE {
            assert!(l.admit(true, t0 + Duration::from_secs(i as u64)).is_ok());
        }
        let online = l.admit(true, t0 + Duration::from_secs(20)).unwrap_err();
        assert!(online.contains("online services") && online.contains("try again in 40 s"), "{online}");
        // Local calls go on until the minute's whole limit.
        for _ in ONLINE_CALLS_PER_MINUTE..CALLS_PER_MINUTE {
            assert!(l.admit(false, t0 + Duration::from_secs(20)).is_ok());
        }
        assert!(l.admit(false, t0 + Duration::from_secs(21)).unwrap_err().contains(&format!("at most {CALLS_PER_MINUTE} calls a minute")));
        // A minute after the first call, it's room again.
        assert!(l.admit(true, t0 + Duration::from_secs(61)).is_ok());
    }

    /// Switched off, a call still in the queue isn't run: it's answered that agents were switched off.
    #[test]
    fn switched_off_nothing_more_runs() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = NexusTools {
            model: Arc::new(parking_lot::RwLock::new(nexus_model::Model::default())),
            db: None,
            index: Default::default(),
            spansh: nexus_spansh::Spansh::new(),
            rt: rt.handle().clone(),
            routes: Default::default(),
            caller: Caller::Mcp,
            budget: Caller::Mcp.budget(),
        };
        let (calls, inbox) = crossbeam_channel::bounded(QUEUE);
        let (new_tools, swaps) = crossbeam_channel::unbounded();
        let status = Arc::new(Mutex::new(Status::default()));
        let (reply, answered) = tokio::sync::oneshot::channel();
        calls.send(Call { name: "commander".into(), args: serde_json::json!({}), reply }).unwrap();
        drop((calls, new_tools));
        work(tools, inbox, swaps, Arc::new(AtomicBool::new(true)), status.clone());
        match answered.blocking_recv() {
            Ok(WireReply::Error(e)) => assert!(e.contains("switched off"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(status.lock().answered, 0, "nothing ran");
    }

    #[test]
    fn the_endpoint_is_this_users_and_local() {
        let e = endpoint().unwrap();
        assert!(e.contains("elite-nexus-mcp"), "{e}");
        if cfg!(windows) {
            assert!(e.starts_with(r"\\.\pipe\elite-nexus-mcp-S-1-"), "a local named pipe named by the user's SID: {e}");
        }
    }
}
