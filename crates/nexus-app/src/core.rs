//! The core service: loads journal history, then tails the journal live, folding everything into
//! the shared [`Model`]. Runs on its own thread; the UI only ever takes short read locks.

use crossbeam_channel::{Receiver, Sender};
use nexus_journal::JournalIndex;
use nexus_journal::{
    list_journal_files, load_history, Companion, CompanionKind, EventCtx, LiveEvent, WatchMsg, Watcher, WatcherConfig,
};
use nexus_model::{Alert, Alerts, Model};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Starting,
    Loading { done: usize, total: usize },
    Live,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct CoreStatus {
    pub journal_dir: Option<PathBuf>,
    pub phase: Phase,
    pub files: usize,
    pub bytes: u64,
    pub load_ms: f64,
    pub current_file: Option<String>,
    pub live_events: u64,
    pub last_live: Option<Instant>,
    pub errors: Vec<String>,
}

impl Default for CoreStatus {
    fn default() -> Self {
        CoreStatus {
            journal_dir: None,
            phase: Phase::Starting,
            files: 0,
            bytes: 0,
            load_ms: 0.0,
            current_file: None,
            live_events: 0,
            last_live: None,
            errors: Vec::new(),
        }
    }
}

/// Commander/location context captured right after a live event was applied (under the model
/// write lock, so uploaders never see a newer state than the event they are sending).
#[derive(Debug, Clone, Default)]
pub struct EventContext {
    pub cmdr: String,
    pub fid: String,
    pub game_version: String,
    pub game_build: String,
    pub horizons: Option<bool>,
    pub odyssey: Option<bool>,
    pub system_name: String,
    pub system_address: u64,
    pub star_pos: Option<[f64; 3]>,
    pub market_id: Option<u64>,
    pub station_name: Option<String>,
    pub ship_id: Option<u64>,
}

impl EventContext {
    pub fn capture(m: &Model) -> EventContext {
        let Some(gs) = m.active() else { return EventContext::default() };
        let c = &gs.cmdr;
        let st = gs.location.station.as_ref().filter(|_| gs.location.docked);
        EventContext {
            cmdr: c.name.clone(),
            fid: c.fid.clone(),
            game_version: c.game_version.clone(),
            game_build: c.game_build.clone(),
            // EDDN: these flags may only come from LoadGame (absent key = omitted).
            horizons: c.load_game_flags.0,
            odyssey: c.load_game_flags.1,
            system_name: gs.location.system.clone(),
            system_address: gs.location.system_address,
            star_pos: gs.location.star_pos,
            market_id: st.map(|s| s.market_id),
            // The journal's own spelling: uploads send what the game wrote.
            station_name: st.map(|s| s.journal_name.clone()),
            ship_id: gs.fleet.current_ship_id,
        }
    }
}

/// Something that happened live, forwarded to other services (uploaders, market, …).
#[derive(Debug, Clone)]
pub enum LiveMsg {
    Event(LiveEvent, EventContext),
    Companion(Companion, EventContext),
}

/// Receives (event name, raw line) for every historical journal event during the initial load,
/// then ("Market", Market.json) for the current market file (seeds the galaxy database with the
/// commander's own stations, systems and hotspots).
pub type HistorySink = Box<dyn FnMut(&str, &str) + Send>;

/// Rhino launches with the mining location recorded for them, by the launch's journal time.
pub type Drops = std::collections::BTreeMap<nexus_journal::Timestamp, nexus_model::mining::SiteKey>;

pub struct Core {
    pub model: Arc<RwLock<Model>>,
    /// Every journal event (24 bytes each; JSON read from disk on demand).
    pub index: Arc<RwLock<JournalIndex>>,
    pub status: Arc<RwLock<CoreStatus>>,
    pub alerts: Receiver<Alert>,
    subscribers: Arc<RwLock<Vec<Sender<LiveMsg>>>>,
    stop: Arc<AtomicBool>,
}

impl Core {
    /// `drops`: Rhino launches recorded on earlier runs ([`nexus_model::mining::Mining::drops`]), given to the model
    /// before it reads the journals.
    pub fn start(journal_dir: Option<PathBuf>, repaint: impl Fn() + Send + 'static, history_sink: Option<HistorySink>, drops: Drops) -> Core {
        let model = Arc::new(RwLock::new(Model::default()));
        let index = Arc::new(RwLock::new(JournalIndex::default()));
        let status = Arc::new(RwLock::new(CoreStatus::default()));
        let subscribers: Arc<RwLock<Vec<Sender<LiveMsg>>>> = Arc::new(RwLock::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (alert_tx, alert_rx) = crossbeam_channel::unbounded();
        {
            let (model, index, status, subscribers, stop) = (model.clone(), index.clone(), status.clone(), subscribers.clone(), stop.clone());
            std::thread::Builder::new()
                .name("nexus-core".into())
                .spawn(move || run(journal_dir, model, index, status, subscribers, alert_tx, stop, repaint, history_sink, drops))
                .expect("spawn core thread");
        }
        Core { model, index, status, alerts: alert_rx, subscribers, stop }
    }

    /// Receive every live journal event / companion update (for uploaders etc.).
    pub fn subscribe(&self) -> Receiver<LiveMsg> {
        let (tx, rx) = crossbeam_channel::unbounded();
        self.subscribers.write().push(tx);
        rx
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Runs one event through the model. A panic there (a bug, or journal data the model doesn't expect) skips that
/// event instead of ending the thread that follows the journal; the panic hook has written it to crash.log.
fn guarded(apply: impl FnOnce()) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(apply)).is_ok()
}

#[allow(clippy::too_many_arguments)]
fn run(
    journal_dir: Option<PathBuf>,
    model: Arc<RwLock<Model>>,
    index: Arc<RwLock<JournalIndex>>,
    status: Arc<RwLock<CoreStatus>>,
    subscribers: Arc<RwLock<Vec<Sender<LiveMsg>>>>,
    alert_tx: Sender<Alert>,
    stop: Arc<AtomicBool>,
    repaint: impl Fn(),
    mut history_sink: Option<HistorySink>,
    drops: Drops,
) {
    let Some(dir) = journal_dir.or_else(nexus_journal::default_journal_dir) else {
        tracing::warn!("no journal folder found");
        status.write().phase = Phase::Error("Elite Nexus couldn't find Elite Dangerous's journal folder on this computer.".into());
        repaint();
        return;
    };
    tracing::info!(dir = %dir.display(), "journal folder");
    // Bindings and HUD colours read the game's Options folder, which on Linux and macOS sits in the same Wine prefix.
    nexus_journal::set_journal_dir_in_use(Some(&dir));
    status.write().journal_dir = Some(dir.clone());
    let files = match list_journal_files(&dir) {
        Ok(f) => f,
        Err(e) => {
            status.write().phase = Phase::Error(if e.kind() == std::io::ErrorKind::NotFound {
                format!("Elite Nexus looked for the journals in {}, but that folder doesn't exist on this computer.", dir.display())
            } else {
                format!("Elite Nexus can't read the journal folder {} ({e}).", dir.display())
            });
            repaint();
            return;
        }
    };

    // ---- 1. history ---------------------------------------------------------------------
    let started = Instant::now();
    let total = files.len();
    {
        let mut st = status.write();
        st.files = total;
        st.bytes = files.iter().map(|f| f.size).sum();
        st.phase = Phase::Loading { done: 0, total };
    }
    repaint();
    let mut local = Model::with_mining_drops(drops);
    let mut idx = JournalIndex::default();
    let mut quiet = Alerts::disabled();
    let mut resume: Option<(PathBuf, u64)> = None;
    let mut done = 0usize;
    let mut skipped = 0usize;
    let mut last_progress = Instant::now();
    load_history(&files, 32, |f, fe| {
        let file_id = idx.add_file(f.path.clone());
        for e in &fe.events {
            let ctx = EventCtx { ts: e.ts, name: fe.name(e), line: fe.line(e), live: false };
            if !guarded(|| local.apply(&ctx, &e.event, &mut quiet)) {
                skipped += 1;
            }
            idx.push(e.ts, file_id, u64::from(e.offset), e.len as usize, fe.name(e));
            if let Some(sink) = history_sink.as_mut() {
                sink(fe.name(e), fe.line(e));
            }
        }
        resume = Some((f.path.clone(), fe.consumed));
        done += 1;
        if last_progress.elapsed() > Duration::from_millis(100) {
            status.write().phase = Phase::Loading { done, total };
            repaint();
            last_progress = Instant::now();
        }
    });
    // Current companion files (Status, Cargo, Market, …) describe the present.
    for kind in CompanionKind::ALL {
        if let Ok(text) = std::fs::read_to_string(dir.join(kind.file_name()))
            && let Some(c) = Companion::parse(kind, &text) {
                if !guarded(|| local.apply_companion(&c, false, &mut quiet)) {
                    skipped += 1;
                }
                if let (Companion::Market(_, raw), Some(sink)) = (&c, history_sink.as_mut()) {
                    sink("Market", raw);
                }
            }
    }
    // Done with history: let the sink's consumer finish and exit.
    drop(history_sink);
    *model.write() = local;
    let mut live_file_id = resume.as_ref().map(|(p, _)| idx.add_file(p.clone())).unwrap_or(0);
    *index.write() = idx;
    {
        let mut st = status.write();
        st.load_ms = started.elapsed().as_secs_f64() * 1e3;
        st.phase = Phase::Live;
        if skipped > 0 {
            st.errors.push(format!("{skipped} journal entries couldn't be handled and were skipped (crash.log in the data folder has the details)."));
        }
        st.current_file = resume.as_ref().and_then(|(p, _)| p.file_name()).map(|n| n.to_string_lossy().into_owned());
    }
    tracing::info!(files = total, ms = status.read().load_ms, "journal history loaded");
    repaint();

    // ---- 2. live ------------------------------------------------------------------------
    let (tx, rx) = crossbeam_channel::unbounded();
    let cfg = WatcherConfig { dir: dir.clone(), poll: Duration::from_millis(100), resume, emit_initial_companions: false };
    let _watcher = match Watcher::spawn(cfg, tx) {
        Ok(w) => w,
        Err(e) => {
            status.write().phase = Phase::Error(format!("Cannot start journal watcher: {e}"));
            repaint();
            return;
        }
    };

    while !stop.load(Ordering::Relaxed) {
        let first = match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(m) => m,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        };
        // Batch everything that is already queued under one write lock.
        let mut batch = vec![first];
        while let Ok(m) = rx.try_recv() {
            batch.push(m);
            if batch.len() >= 512 {
                break;
            }
        }
        let mut alerts = Alerts::live();
        let mut forward = Vec::new();
        let mut failed: Vec<String> = Vec::new();
        {
            let mut m = model.write();
            for msg in batch {
                match msg {
                    WatchMsg::Event(le) => {
                        index.write().push(le.raw.ts, live_file_id, le.raw.source.offset, le.raw.line.len(), &le.raw.name);
                        let ctx = EventCtx { ts: le.raw.ts, name: &le.raw.name, line: &le.raw.line, live: true };
                        if !guarded(|| m.apply(&ctx, &le.event, &mut alerts)) {
                            failed.push(format!("A {} event couldn't be handled and was skipped (crash.log in the data folder has the details).", le.raw.name));
                        }
                        let ectx = EventContext::capture(&m);
                        forward.push(LiveMsg::Event(le, ectx));
                    }
                    WatchMsg::Companion(c) => {
                        if !guarded(|| m.apply_companion(&c, true, &mut alerts)) {
                            failed.push(format!("{} couldn't be handled and was skipped (crash.log in the data folder has the details).", c.kind().file_name()));
                        }
                        let ectx = EventContext::capture(&m);
                        forward.push(LiveMsg::Companion(c, ectx));
                    }
                    WatchMsg::FileOpened(f) => {
                        live_file_id = index.write().add_file(f.path.clone());
                        status.write().current_file = Some(f.file_name.clone());
                    }
                    WatchMsg::Error(e) => {
                        tracing::warn!("{e}");
                        let mut st = status.write();
                        st.errors.push(e);
                        if st.errors.len() > 50 {
                            st.errors.remove(0);
                        }
                    }
                }
            }
        }
        {
            let mut st = status.write();
            st.live_events += forward.len() as u64;
            st.last_live = Some(Instant::now());
            st.errors.extend(failed);
            let excess = st.errors.len().saturating_sub(50);
            st.errors.drain(..excess);
        }
        for a in alerts.items {
            let _ = alert_tx.send(a);
        }
        if !forward.is_empty() {
            let mut subs = subscribers.write();
            subs.retain(|s| forward.iter().all(|msg| s.send(msg.clone()).is_ok()));
        }
        repaint();
    }
}
