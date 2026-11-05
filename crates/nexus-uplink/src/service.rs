//! The running uplink: a processing thread that turns live events into queued uploads, and one
//! async sender per service on the app's tokio runtime.
//!
//! ```text
//! core thread ──on_event/on_companion (enqueue only)──► uplink thread ──► persistent queues
//!                                                          │ (EDDN builder, EDSM, Inara mapper, EDAstro filter)
//! tokio: EDDN sender (1 request at a time, ≥ 0.5 s apart, 400/403/413/426 never retried, ≥ 60 s backoff)
//!        EDSM sender (batches ≤ 100, ≥ 10 s apart, discard list, msgnum handling, rate-limit headers)
//!        Inara sender (batches ≤ 50, ≥ 35 s apart, 1 h pause on "too much requests")
//!        EDAstro sender (accepted events only, batches ≤ 50, ≥ 10 s apart)
//! ```

use crate::config::{UplinkConfig, fingerprint};
use crate::context::UploadContext;
use crate::eddn::{self, EddnBuilder, EddnMessage, EddnOptions, Schema};
use crate::edastro::{self, EdastroItem};
use crate::edsm::{self, EdsmItem, EdsmOutcome};
use crate::inara::{self, InaraEvent, InaraItem, InaraMapper, InaraOutcome};
use crate::json::{Obj, form_urlencode, gzip, parse_object};
use crate::queue::DiskQueue;
use crate::session::Session;
use crate::status::{Counters, ServiceStatus, UplinkStatus};
use crate::transport::{FailingTransport, HttpRequest, ReqwestTransport, Transport};
use crossbeam_channel::{Receiver, Sender};
use nexus_journal::{Companion, CompanionKind, LiveEvent, RawEvent, Timestamp};
use parking_lot::{Mutex, RwLock};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::Instant;

/// Events older than this (vs. the PC clock) are never uploaded: they are not live.
const MAX_EVENT_AGE_SECS: i64 = 3600;
/// Market/outfitting/shipyard data goes stale fast; other EDDN data is useless after a day.
const EDDN_STATION_TTL_SECS: i64 = 3600;
const EDDN_TTL_SECS: i64 = 24 * 3600;
const EDSM_TTL_SECS: i64 = 7 * 24 * 3600;
const INARA_TTL_SECS: i64 = 7 * 24 * 3600;
const EDASTRO_TTL_SECS: i64 = 24 * 3600;
/// The old gateway's 1 MiB compressed limit (larger bodies get 413).
const EDDN_MAX_BODY: usize = 1 << 20;
const EDDN_MAX_ATTEMPTS: u32 = 5;
const EDSM_MAX_BATCH: usize = 100;
const INARA_MAX_BATCH: usize = 50;
const EDASTRO_MAX_BATCH: usize = 50;
const EDASTRO_MAX_ATTEMPTS: u32 = 5;
const LIST_MAX_AGE_SECS: i64 = 6 * 3600;
/// Backoff steps, in "backoff units" (seconds in production).
const EDDN_BACKOFF: [u64; 5] = [60, 120, 300, 600, 900];
const EDSM_BACKOFF: [u64; 6] = [30, 60, 120, 300, 600, 900];
const INARA_BACKOFF: [u64; 5] = [70, 140, 300, 600, 900];
const EDASTRO_BACKOFF: [u64; 5] = [60, 120, 300, 600, 900];

/// Pacing of the senders (tests run with a fast copy).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    /// Pause after every EDDN request: one at a time, ≤ 2 per second.
    pub eddn_spacing: Duration,
    /// Minimum time between two EDSM POSTs.
    pub edsm_min_gap: Duration,
    /// An EDSM batch goes out after this many quiet seconds (or at once for urgent events).
    pub edsm_quiet_secs: i64,
    /// …and at the latest this long after its first event.
    pub edsm_max_wait_secs: i64,
    /// Minimum time between two Inara POSTs (Inara allows ~2 a minute).
    pub inara_min_gap: Duration,
    /// Events are collected this long before an Inara POST.
    pub inara_gather_secs: i64,
    /// Pause after "too much requests" (Inara locks the key for an hour).
    pub inara_lockout: Duration,
    pub edastro_min_gap: Duration,
    pub edastro_gather_secs: i64,
    /// Wait after a failed list download (EDSM discard list, EDAstro accepted events).
    pub list_retry: Duration,
    /// Length of one backoff step unit.
    pub backoff_unit: Duration,
}

pub(crate) const PRODUCTION: Timing = Timing {
    eddn_spacing: Duration::from_millis(500),
    edsm_min_gap: Duration::from_secs(10),
    edsm_quiet_secs: 15,
    edsm_max_wait_secs: 60,
    inara_min_gap: Duration::from_secs(35),
    inara_gather_secs: 10,
    inara_lockout: Duration::from_secs(3660),
    edastro_min_gap: Duration::from_secs(10),
    edastro_gather_secs: 10,
    list_retry: Duration::from_secs(600),
    backoff_unit: Duration::from_secs(1),
};

/// Test pacing: same logic, millisecond waits; batches only go out when flushed or urgent.
#[cfg(test)]
pub(crate) const FAST: Timing = Timing {
    eddn_spacing: Duration::from_millis(2),
    edsm_min_gap: Duration::from_millis(40),
    edsm_quiet_secs: 3600,
    edsm_max_wait_secs: 3600,
    inara_min_gap: Duration::from_millis(40),
    inara_gather_secs: 3600,
    inara_lockout: Duration::from_secs(3660),
    edastro_min_gap: Duration::from_millis(20),
    edastro_gather_secs: 3600,
    list_retry: Duration::from_secs(600),
    backoff_unit: Duration::from_millis(1),
};

fn timing() -> &'static Timing {
    #[cfg(test)]
    {
        &FAST
    }
    #[cfg(not(test))]
    {
        &PRODUCTION
    }
}

/// One queued EDDN message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EddnItem {
    pub schema: Schema,
    /// The exact (uncompressed) body.
    pub json: String,
    pub event_time: Timestamp,
    pub queued_at: Timestamp,
    pub expires: Timestamp,
    pub attempts: u32,
}

enum Input {
    Event(RawEvent, Box<UploadContext>),
    Companion(Companion, Box<UploadContext>),
    /// The current session for an Inara profile update (see [`Uplink::sync_inara_profile`]).
    InaraSnapshot(Vec<RawEvent>, Box<UploadContext>),
    Config(Box<UplinkConfig>),
    Flush,
    Stop,
}

/// Queue + wake-up + counters of one service.
struct Service<T> {
    queue: Mutex<DiskQueue<T>>,
    notify: Notify,
    counters: Counters,
    flush: AtomicBool,
}

impl<T: Serialize + DeserializeOwned> Service<T> {
    fn new(dir: Option<&Path>, file: &str, max_items: usize) -> Self {
        let queue = match dir {
            Some(d) => DiskQueue::open(d.join(file), max_items),
            None => DiskQueue::memory(max_items),
        };
        Service { queue: Mutex::new(queue), notify: Notify::new(), counters: Counters::default(), flush: AtomicBool::new(false) }
    }

    fn push(&self, item: T) {
        let evicted = self.queue.lock().push(item);
        if evicted > 0 {
            self.counters.failed(evicted as u64, "queue full: oldest entries dropped");
        }
    }

    fn wake(&self) {
        self.notify.notify_one();
    }
}

/// A server-provided list of event names (EDSM discard list, EDAstro accepted events).
struct EventList {
    set: Arc<HashSet<String>>,
    fetched: Option<Timestamp>,
    last_attempt: Option<Instant>,
}

impl EventList {
    fn new(defaults: &[&str], cache: Option<(Timestamp, Vec<String>)>) -> Self {
        match cache {
            Some((fetched, events)) if !events.is_empty() => {
                EventList { set: Arc::new(events.into_iter().collect()), fetched: Some(fetched), last_attempt: None }
            }
            _ => EventList { set: Arc::new(defaults.iter().map(|s| s.to_string()).collect()), fetched: None, last_attempt: None },
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ListCache {
    fetched: Timestamp,
    events: Vec<String>,
}

struct Shared {
    config: RwLock<UplinkConfig>,
    /// Bumped when the configuration changes: services stopped by a rejection resume.
    epoch: AtomicU64,
    shutdown: AtomicBool,
    transport: Arc<dyn Transport>,
    dir: Option<PathBuf>,
    eddn: Service<EddnItem>,
    edsm: Service<EdsmItem>,
    inara: Service<InaraItem>,
    edastro: Service<EdastroItem>,
    edsm_discard: RwLock<EventList>,
    edastro_accepting: RwLock<EventList>,
    /// Ask Inara who the key belongs to (inside the next batch).
    inara_check: AtomicBool,
    /// ... or on its own if nothing else is waiting (after a key was entered or changed).
    inara_check_now: AtomicBool,
    /// What Inara answered to the last check.
    inara_account: Mutex<Option<crate::status::Account>>,
}

impl Shared {
    fn stopping(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }

    fn wake_all(&self) {
        self.eddn.wake();
        self.edsm.wake();
        self.inara.wake();
        self.edastro.wake();
    }

    fn list_cache_path(&self, name: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(name))
    }
}

fn read_list_cache(path: Option<PathBuf>) -> Option<(Timestamp, Vec<String>)> {
    let text = std::fs::read_to_string(path?).ok()?;
    let c: ListCache = serde_json::from_str(&text).ok()?;
    Some((c.fetched, c.events))
}

/// Which in-game commander each EDSM/Inara credential set uploads for, so that playing an alt
/// never writes the alt's journal into the main account. Persisted in `bindings.json`; keys are
/// credential fingerprints (never the secrets).
#[derive(Debug, Default, Serialize, Deserialize)]
struct Bindings {
    map: std::collections::BTreeMap<String, String>,
}

impl Bindings {
    fn load(dir: Option<&Path>) -> Self {
        dir.and_then(|d| std::fs::read_to_string(d.join("bindings.json")).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save(&self, dir: Option<&Path>) {
        if let Some(d) = dir
            && let Ok(t) = serde_json::to_string_pretty(self)
            && let Err(e) = std::fs::write(d.join("bindings.json"), t)
        {
            tracing::warn!(%e, "uplink: cannot save commander bindings");
        }
    }

    /// May `cmdr` upload with these credentials? The commander whose name equals `preferred`
    /// (the EDSM account name) always may; otherwise the first commander seen is bound.
    fn allow(&mut self, key: String, cmdr: &str, preferred: Option<&str>, dir: Option<&Path>) -> Result<(), String> {
        let preferred_match = preferred.is_some_and(|p| p.trim().eq_ignore_ascii_case(cmdr.trim()));
        match self.map.get(&key) {
            Some(bound) if bound.eq_ignore_ascii_case(cmdr) => Ok(()),
            Some(bound) if !preferred_match => Err(bound.clone()),
            _ => {
                self.map.insert(key, cmdr.to_string());
                self.save(dir);
                Ok(())
            }
        }
    }
}

/// Inara pacing state that must survive restarts (≤ 2 requests a minute, hour-long lock-out).
#[derive(Debug, Default, Serialize, Deserialize)]
struct InaraPacing {
    last_post: Option<Timestamp>,
    locked_until: Option<Timestamp>,
}

impl InaraPacing {
    fn path(shared: &Shared) -> Option<PathBuf> {
        shared.dir.as_ref().map(|d| d.join("inara-pacing.json"))
    }

    fn load(shared: &Shared) -> Self {
        Self::path(shared)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save(&self, shared: &Shared) {
        if let Some(p) = Self::path(shared)
            && let Ok(t) = serde_json::to_string(self)
        {
            let _ = std::fs::write(p, t);
        }
    }
}

/// Everything Elite Nexus uploads. Cheap to call from the core thread: `on_event` and
/// `on_companion` only enqueue; parsing, building and HTTP happen elsewhere.
pub struct Uplink {
    tx: Sender<Input>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Uplink {
    /// Starts the uplink with the real HTTP transport. Queues live in `data_dir\uplink\`;
    /// `rt` must have the time and I/O drivers enabled.
    pub fn new(cfg: UplinkConfig, data_dir: &Path, rt: &tokio::runtime::Handle) -> Uplink {
        let transport: Arc<dyn Transport> = match ReqwestTransport::new(&cfg.user_agent()) {
            Ok(t) => Arc::new(t),
            Err(e) => {
                tracing::error!(%e, "uplink: no HTTP client, uploads will fail");
                Arc::new(FailingTransport(e.0))
            }
        };
        Self::with_transport(cfg, data_dir, rt, transport)
    }

    /// Starts the uplink with a custom transport (tests capture requests instead of sending).
    pub fn with_transport(cfg: UplinkConfig, data_dir: &Path, rt: &tokio::runtime::Handle, transport: Arc<dyn Transport>) -> Uplink {
        let dir = data_dir.join("uplink");
        let dir = match std::fs::create_dir_all(&dir) {
            Ok(()) => Some(dir),
            Err(e) => {
                tracing::warn!(dir = %dir.display(), %e, "uplink: cannot create data directory, queues are memory-only");
                None
            }
        };
        let d = dir.as_deref();
        let edsm_cache = read_list_cache(d.map(|d| d.join("edsm-discard.json")));
        let edastro_cache = read_list_cache(d.map(|d| d.join("edastro-accepting.json")));
        let shared = Arc::new(Shared {
            config: RwLock::new(cfg.clone()),
            epoch: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            transport,
            eddn: Service::new(d, "eddn.jsonl", 5000),
            edsm: Service::new(d, "edsm.jsonl", 20_000),
            inara: Service::new(d, "inara.jsonl", 5000),
            edastro: Service::new(d, "edastro.jsonl", 2000),
            edsm_discard: RwLock::new(EventList::new(&edsm::DEFAULT_DISCARD, edsm_cache)),
            edastro_accepting: RwLock::new(EventList::new(&edastro::DEFAULT_ACCEPTING, edastro_cache)),
            dir,
            // Confirm the key once per run (once resumed, if Inara starts paused).
            inara_check: AtomicBool::new(cfg.inara_credentials().is_some()),
            inara_check_now: AtomicBool::new(false),
            inara_account: Mutex::new(None),
        });
        // Consent may have been withdrawn while the app was closed.
        clear_disabled_queues(&shared, &cfg);

        let (tx, rx) = crossbeam_channel::unbounded();
        let proc_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("uplink".into())
            .spawn(move || Processor::new(proc_shared, cfg).run(rx))
            .map_err(|e| tracing::error!(%e, "uplink: cannot start the processing thread"))
            .ok();
        rt.spawn(run_eddn(shared.clone()));
        rt.spawn(run_edsm(shared.clone()));
        rt.spawn(run_inara(shared.clone()));
        rt.spawn(run_edastro(shared.clone()));
        Uplink { tx, shared, thread }
    }

    /// Hands over one live journal event (call after the model applied it).
    pub fn on_event(&self, ev: &LiveEvent, ctx: &UploadContext) {
        let _ = self.tx.send(Input::Event(ev.raw.clone(), Box::new(ctx.clone())));
    }

    /// Hands over a companion file the watcher read (Status.json, Market.json, …).
    pub fn on_companion(&self, c: &Companion, ctx: &UploadContext) {
        let _ = self.tx.send(Input::Companion(c.clone(), Box::new(ctx.clone())));
    }

    /// Replaces the configuration. Disabling or pausing a service discards its queue (consent
    /// withdrawn, or what happens while paused isn't kept); any change resumes services that were
    /// stopped by a rejection.
    pub fn set_config(&self, cfg: UplinkConfig) {
        clear_disabled_queues(&self.shared, &cfg);
        // A new or changed Inara key: find out again who it belongs to.
        let inara_key = |c: &UplinkConfig| c.inara_credentials().map(|i| fingerprint(&[&i.api_key]));
        let new_key = inara_key(&cfg);
        if new_key != inara_key(&self.shared.config.read()) {
            *self.shared.inara_account.lock() = None;
            self.shared.inara_check.store(new_key.is_some(), Ordering::Relaxed);
            self.shared.inara_check_now.store(new_key.is_some(), Ordering::Relaxed);
        }
        *self.shared.config.write() = cfg.clone();
        self.shared.epoch.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(Input::Config(Box::new(cfg)));
        self.shared.wake_all();
    }

    pub fn config(&self) -> UplinkConfig {
        self.shared.config.read().clone()
    }

    /// Per-service counters.
    pub fn status(&self) -> UplinkStatus {
        let s = &self.shared;
        let cfg = s.config.read().clone();
        UplinkStatus {
            eddn: s.eddn.counters.snapshot(cfg.eddn_enabled(), s.eddn.queue.lock().len()),
            edsm: s.edsm.counters.snapshot(cfg.edsm_enabled(), s.edsm.queue.lock().len()),
            inara: ServiceStatus {
                account: s.inara_account.lock().clone(),
                checking: cfg.inara_enabled() && s.inara_check.load(Ordering::Relaxed),
                ..s.inara.counters.snapshot(cfg.inara_enabled(), s.inara.queue.lock().len())
            },
            edastro: s.edastro.counters.snapshot(cfg.edastro_enabled(), s.edastro.queue.lock().len()),
        }
    }

    /// Asks Inara again who the key belongs to (answer in [`Uplink::status`]).
    pub fn check_inara(&self) {
        if self.shared.config.read().inara_enabled() {
            self.shared.inara_check.store(true, Ordering::Relaxed);
            self.shared.inara_check_now.store(true, Ordering::Relaxed);
            self.shared.inara.wake();
        }
    }

    /// Brings the Inara profile up to date from the current game session: journal lines from the
    /// session's start (`Fileheader`/`LoadGame`) to now. Only "set" events (current state: credits,
    /// ranks, reputation, ship, loadout, materials, cargo, location, …) are sent, so repeating it
    /// is harmless; nothing goes to EDDN, EDSM or EDAstro.
    pub fn sync_inara_profile(&self, session: Vec<RawEvent>, ctx: &UploadContext) {
        let _ = self.tx.send(Input::InaraSnapshot(session, Box::new(ctx.clone())));
    }

    /// Closes pending batches and sends everything queued as soon as each service's rate
    /// limit allows (e.g. when the game shuts down or the app exits).
    pub fn flush(&self) {
        let _ = self.tx.send(Input::Flush);
    }
}

impl Drop for Uplink {
    fn drop(&mut self) {
        let _ = self.tx.send(Input::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.shared.shutdown.store(true, Ordering::Relaxed);
        self.shared.wake_all();
    }
}

fn clear_disabled_queues(shared: &Shared, cfg: &UplinkConfig) {
    if !cfg.eddn_enabled() {
        shared.eddn.queue.lock().clear();
    }
    if !cfg.edsm_enabled() {
        shared.edsm.queue.lock().clear();
    }
    if !cfg.inara_enabled() {
        shared.inara.queue.lock().clear();
    }
    if !cfg.edastro_enabled() {
        shared.edastro.queue.lock().clear();
    }
}

// ---------------------------------------------------------------------------------------------
// Processing thread
// ---------------------------------------------------------------------------------------------

/// An EDSM stub event waiting for its companion file.
struct PendingStub {
    kind: CompanionKind,
    ts: Timestamp,
    item: EdsmItem,
    ctx: UploadContext,
}

struct Processor {
    shared: Arc<Shared>,
    cfg: UplinkConfig,
    session: Session,
    eddn: EddnBuilder,
    inara: InaraMapper,
    stub: Option<PendingStub>,
    /// The most recent context from the app (used when flushing).
    last_ctx: UploadContext,
    bindings: Bindings,
    /// EDDN station data (market, outfitting, shipyard) held until the commander undocks, so the
    /// pause button still keeps a market private that was opened a moment ago.
    held: Vec<EddnMessage>,
}

fn eddn_options(cfg: &UplinkConfig) -> EddnOptions {
    EddnOptions {
        software_name: cfg.software().to_string(),
        software_version: cfg.version().to_string(),
        force_test: cfg.eddn_force_test(),
        outfitting_v3: false,
    }
}

fn is_live(ts: Timestamp, now: Timestamp) -> bool {
    now.0 - ts.0 <= MAX_EVENT_AGE_SECS && ts.0 - now.0 <= 300
}

impl Processor {
    fn new(shared: Arc<Shared>, cfg: UplinkConfig) -> Self {
        Processor {
            eddn: EddnBuilder::new(eddn_options(&cfg)),
            bindings: Bindings::load(shared.dir.as_deref()),
            shared,
            cfg,
            session: Session::new(),
            inara: InaraMapper::new(),
            stub: None,
            last_ctx: UploadContext::default(),
            held: Vec::new(),
        }
    }

    /// Commander binding check for EDSM/Inara (see [`Bindings`]).
    fn bound(&mut self, service: &str, fp: u64, cmdr: &str, preferred: Option<&str>, counters: &Counters) -> bool {
        match self.bindings.allow(format!("{service}:{fp:016x}"), cmdr, preferred, self.shared.dir.as_deref()) {
            Ok(()) => true,
            Err(owner) => {
                counters.error(format!("not uploading CMDR {cmdr}: these credentials belong to CMDR {owner}"));
                false
            }
        }
    }

    fn run(mut self, rx: Receiver<Input>) {
        for input in rx {
            if matches!(input, Input::Stop) {
                break;
            }
            // Never let one malformed input kill the uplink.
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.handle(input)));
            if res.is_err() {
                tracing::error!("uplink: internal error while processing an event (skipped)");
            }
        }
        self.release_stub();
    }

    fn handle(&mut self, input: Input) {
        match input {
            Input::Event(raw, ctx) => {
                self.on_event(&raw, &ctx);
                self.last_ctx = *ctx;
            }
            Input::Companion(c, ctx) => {
                self.on_companion(&c, &ctx);
                self.last_ctx = *ctx;
            }
            Input::Config(cfg) => {
                self.eddn.set_options(eddn_options(&cfg));
                self.cfg = *cfg;
                if !self.cfg.eddn_enabled() {
                    self.held.clear();
                }
                // An EDSM event waiting for its companion file goes with the queue.
                if !self.cfg.edsm_enabled() {
                    self.stub = None;
                }
                // Anything queued under the old configuration for a now-disabled service goes.
                clear_disabled_queues(&self.shared, &self.cfg);
            }
            Input::InaraSnapshot(events, ctx) => self.inara_snapshot(&events, &ctx),
            Input::Flush => {
                self.release_stub();
                let now = Timestamp::now();
                self.release_held(now);
                let msgs = self.eddn.flush(&self.session, &self.last_ctx);
                self.queue_eddn(msgs, now);
                for f in [&self.shared.eddn.flush, &self.shared.edsm.flush, &self.shared.inara.flush, &self.shared.edastro.flush] {
                    f.store(true, Ordering::Relaxed);
                }
                self.shared.wake_all();
            }
            Input::Stop => {}
        }
    }

    /// Common gate for EDSM/Inara/EDAstro: live, not beta, Live galaxy, not crew/CQC/Operation.
    fn journal_sync_allowed(&self, ctx: &UploadContext, ev: Option<&Obj>, live: bool) -> bool {
        live && self.session.blocked(ev).is_none() && !self.session.is_beta(ctx) && self.session.is_live_galaxy(ctx)
    }

    fn on_event(&mut self, raw: &RawEvent, ctx: &UploadContext) {
        // The companion of a pending stub would have been the very next input.
        self.release_stub();
        let Some(ev) = parse_object(&raw.line) else {
            tracing::debug!(event = %raw.name, "uplink: skipping a journal line that is not valid JSON");
            return;
        };
        let name: &str = &raw.name;
        let now = Timestamp::now();
        let live = is_live(raw.ts, now);

        // The docking that produced the held station data is over (or never happened here).
        if matches!(name, "Undocked" | "Docked" | "Location" | "FSDJump" | "Died" | "Shutdown") {
            self.release_held(now);
        }
        let msgs = self.eddn.on_event(&mut self.session, ctx, name, &ev, self.cfg.eddn_enabled() && live);
        self.queue_eddn(msgs, now);

        let allowed = self.journal_sync_allowed(ctx, Some(&ev), live);
        let cmdr = self.session.cmdr(ctx).map(str::to_string);
        let (game_version, game_build) = {
            let (v, b) = self.session.game_version(ctx);
            (v.to_string(), b.to_string())
        };

        // Inara: the mapper tracks state from every event.
        let inara_events = self.inara.on_event(name, &ev, &self.session, ctx);
        if allowed && !inara_events.is_empty() {
            self.queue_inara(inara_events, ctx, now);
        }

        // EDSM
        let shared = self.shared.clone();
        if allowed
            && let (Some(creds), Some(cmdr)) = (self.cfg.edsm_sending().cloned(), cmdr.as_ref())
            && !edsm::is_discarded(&shared.edsm_discard.read().set, name)
            && self.bound("edsm", fingerprint(&[&creds.commander_name, &creds.api_key]), cmdr, Some(&creds.commander_name), &shared.edsm.counters)
        {
            let item = EdsmItem {
                cmdr: cmdr.clone(),
                game_version: game_version.clone(),
                game_build: game_build.clone(),
                cred: fingerprint(&[&creds.commander_name, &creds.api_key]),
                event: name.to_string(),
                event_time: raw.ts,
                queued_at: now,
                urgent: edsm::URGENT_EVENTS.contains(&name),
                json: Value::Object(edsm::with_transient_fields(&ev, ctx)).to_string(),
            };
            match edsm::stub_companion(name, &ev) {
                Some(kind) => self.stub = Some(PendingStub { kind, ts: raw.ts, item, ctx: ctx.clone() }),
                None => {
                    self.shared.edsm.push(item);
                    self.shared.edsm.wake();
                }
            }
        }

        // EDAstro
        if allowed
            && self.cfg.edastro_enabled()
            && let Some(cmdr) = cmdr
            && self.shared.edastro_accepting.read().set.contains(name)
            && let Some(event) = edastro::filter_event(name, &ev)
        {
            let system = self.session.location(ctx).map(|l| l.system_name).unwrap_or_else(|| ctx.system_name.clone());
            self.shared.edastro.push(EdastroItem {
                cmdr,
                system,
                odyssey: self.session.expansions(ctx).1,
                event,
                event_time: raw.ts,
                queued_at: now,
            });
            self.shared.edastro.wake();
        }
    }

    fn on_companion(&mut self, c: &Companion, ctx: &UploadContext) {
        let now = Timestamp::now();
        if let Some(stub) = self.stub.take() {
            let mut item = stub.item;
            if c.kind() == stub.kind
                && (c.timestamp().0 - stub.ts.0).abs() <= 2
                && let Some(file) = companion_text(c).and_then(parse_object)
            {
                // EDMC sends the companion file in place of the stub event.
                item.json = Value::Object(edsm::with_transient_fields(&file, &stub.ctx)).to_string();
            }
            self.shared.edsm.push(item);
            self.shared.edsm.wake();
        }

        let msgs = self.eddn.on_companion(&mut self.session, ctx, c, self.cfg.eddn_enabled());
        self.queue_eddn(msgs, now);

        let inara_events = self.inara.on_companion(c);
        if !inara_events.is_empty() && self.journal_sync_allowed(ctx, None, true) {
            self.queue_inara(inara_events, ctx, now);
        }
    }

    fn release_stub(&mut self) {
        if let Some(stub) = self.stub.take() {
            self.shared.edsm.push(stub.item);
            self.shared.edsm.wake();
        }
    }

    fn queue_eddn(&mut self, msgs: Vec<EddnMessage>, now: Timestamp) {
        self.queue_eddn_inner(msgs, now, true);
    }

    fn queue_eddn_inner(&mut self, msgs: Vec<EddnMessage>, now: Timestamp, hold: bool) {
        if msgs.is_empty() {
            return;
        }
        for m in msgs {
            if hold && m.schema.is_station_data() {
                self.held.push(m);
                continue;
            }
            let ttl = if m.schema.is_station_data() { EDDN_STATION_TTL_SECS } else { EDDN_TTL_SECS };
            let expires = Timestamp(m.event_time.0 + ttl);
            if expires <= now {
                continue;
            }
            self.shared.eddn.push(EddnItem { schema: m.schema, json: m.to_json(), event_time: m.event_time, queued_at: now, expires, attempts: 0 });
        }
        self.shared.eddn.wake();
    }

    /// Queues the held station data (it was dropped instead if EDDN was paused or turned off).
    fn release_held(&mut self, now: Timestamp) {
        let held = std::mem::take(&mut self.held);
        self.queue_eddn_inner(held, now, false);
    }

    /// Replays the current session through a fresh mapper and queues its "set" events: the
    /// commander's present state, sent at once (the "add" events of past activity are left out).
    fn inara_snapshot(&mut self, events: &[RawEvent], ctx: &UploadContext) {
        if self.cfg.inara_sending().is_none() {
            return;
        }
        let mut session = Session::new();
        let mut mapper = InaraMapper::new();
        let mut out: Vec<InaraEvent> = Vec::new();
        for raw in events {
            let Some(ev) = parse_object(&raw.line) else { continue };
            session.apply(&raw.name, &ev);
            out.extend(mapper.on_event(&raw.name, &ev, &session, ctx).into_iter().filter(|e| e.name.starts_with("set")));
        }
        // The same rules as live syncing: Live galaxy only, no beta, not as someone's crew.
        if out.is_empty() || session.is_beta(ctx) || !session.is_live_galaxy(ctx) || session.blocked(None).is_some() {
            return;
        }
        self.queue_inara(out, ctx, Timestamp::now());
        self.shared.inara.flush.store(true, Ordering::Relaxed);
        self.shared.inara.wake();
    }

    fn queue_inara(&mut self, events: Vec<InaraEvent>, ctx: &UploadContext, now: Timestamp) {
        let (Some(creds), Some(cmdr)) = (self.cfg.inara_sending().cloned(), self.session.cmdr(ctx).map(str::to_string)) else {
            return;
        };
        let key = fingerprint(&[&creds.api_key]);
        let shared = self.shared.clone();
        if !self.bound("inara", key, &cmdr, None, &shared.inara.counters) {
            return;
        }
        let cmdr = cmdr.as_str();
        let fid = self.session.fid(ctx).unwrap_or_default().to_string();
        {
            let mut q = self.shared.inara.queue.lock();
            for e in events {
                if let Some(k) = e.coalesce_key() {
                    q.retain(|it| !(it.cmdr == cmdr && it.key == key && it.event.coalesce_key().as_deref() == Some(k.as_str())));
                }
                let event_time = Timestamp::parse(&e.timestamp).unwrap_or(now);
                let evicted = q.push(InaraItem { cmdr: cmdr.to_string(), fid: fid.clone(), key, event: e, event_time, queued_at: now });
                if evicted > 0 {
                    self.shared.inara.counters.failed(evicted as u64, "queue full: oldest entries dropped");
                }
            }
        }
        self.shared.inara.wake();
    }
}

fn companion_text(c: &Companion) -> Option<&str> {
    match c {
        Companion::Cargo(_, raw)
        | Companion::ShipLocker(_, raw)
        | Companion::Backpack(_, raw)
        | Companion::FCMaterials(_, raw)
        | Companion::Market(_, raw)
        | Companion::Outfitting(_, raw)
        | Companion::Shipyard(_, raw)
        | Companion::NavRoute(_, raw)
        | Companion::ModulesInfo(_, raw) => Some(raw),
        Companion::Status(_) => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Sender helpers
// ---------------------------------------------------------------------------------------------

/// Waits for a wake-up or the timeout, whichever comes first.
async fn idle(notify: &Notify, dur: Duration) {
    let _ = tokio::time::timeout(dur.max(Duration::from_millis(50)), notify.notified()).await;
}

/// Exponential backoff with up to +10 % jitter: `steps[level]` units (the last step repeats).
fn backoff(steps: &[u64], level: u32) -> Duration {
    let base = steps.get(level as usize).or(steps.last()).copied().unwrap_or(60);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let jitter = u64::from(nanos) % (base / 10 + 1);
    timing().backoff_unit * u32::try_from(base + jitter).unwrap_or(u32::MAX)
}

/// State shared by every sender loop: pauses, stops and backoff.
struct Gate {
    pause_until: Option<Instant>,
    stopped_at_epoch: Option<u64>,
    level: u32,
}

impl Gate {
    fn new() -> Self {
        Gate { pause_until: None, stopped_at_epoch: None, level: 0 }
    }

    /// Returns how long to wait before doing anything (None = go ahead).
    fn wait(&mut self, epoch: u64, counters: &Counters) -> Option<Duration> {
        if let Some(e) = self.stopped_at_epoch {
            if e == epoch {
                return Some(Duration::from_secs(3600));
            }
            self.stopped_at_epoch = None;
            self.pause_until = None;
            self.level = 0;
            counters.set_paused(None);
        }
        if let Some(t) = self.pause_until {
            let now = Instant::now();
            if t > now {
                return Some(t - now);
            }
            self.pause_until = None;
            counters.set_paused(None);
        }
        None
    }

    fn stop(&mut self, epoch: u64, counters: &Counters, reason: String) {
        tracing::warn!(%reason, "uplink: service stopped until the settings change");
        counters.error(reason.clone());
        counters.set_paused(Some(format!("{reason} (paused until the settings change)")));
        self.stopped_at_epoch = Some(epoch);
    }

    fn pause(&mut self, d: Duration, counters: &Counters, reason: String) {
        tracing::info!(%reason, secs = d.as_secs(), "uplink: pausing");
        counters.error(reason.clone());
        counters.set_paused(Some(format!("{reason}; retrying in {} s", d.as_secs())));
        self.pause_until = Some(Instant::now() + d);
    }

    fn retry(&mut self, steps: &[u64], counters: &Counters, reason: String) {
        let d = backoff(steps, self.level);
        self.level = self.level.saturating_add(1);
        self.pause(d, counters, reason);
    }

    fn success(&mut self) {
        self.level = 0;
    }
}

fn short(body: &str) -> String {
    let b = body.trim();
    if b.len() > 300 { format!("{}…", &b[..b.char_indices().nth(300).map(|(i, _)| i).unwrap_or(b.len())]) } else { b.to_string() }
}

/// Refreshes a server-provided event list (once, then every 6 h; 10 min after a failure).
async fn refresh_list(shared: &Shared, list: &RwLock<EventList>, url: &str, cache_file: &str) {
    {
        let l = list.read();
        let fresh = l.fetched.is_some_and(|f| Timestamp::now().0 - f.0 < LIST_MAX_AGE_SECS);
        let recently_tried = l.last_attempt.is_some_and(|t| t.elapsed() < timing().list_retry);
        if fresh || recently_tried {
            return;
        }
    }
    list.write().last_attempt = Some(Instant::now());
    let res = shared.transport.send(HttpRequest::get(url, Duration::from_secs(20))).await;
    let events: Option<Vec<String>> = match res {
        Ok(r) if r.is_success() => serde_json::from_str::<Vec<String>>(&r.body).ok().filter(|v| !v.is_empty()),
        Ok(r) => {
            tracing::debug!(url, status = r.status, "uplink: list fetch failed");
            None
        }
        Err(e) => {
            tracing::debug!(url, %e, "uplink: list fetch failed");
            None
        }
    };
    if let Some(events) = events {
        let now = Timestamp::now();
        if let Some(path) = shared.list_cache_path(cache_file)
            && let Ok(text) = serde_json::to_string(&ListCache { fetched: now, events: events.clone() })
        {
            let _ = std::fs::write(path, text);
        }
        let mut l = list.write();
        l.set = Arc::new(events.into_iter().collect());
        l.fetched = Some(now);
    }
}

// ---------------------------------------------------------------------------------------------
// EDDN sender
// ---------------------------------------------------------------------------------------------

async fn run_eddn(shared: Arc<Shared>) {
    let svc = &shared.eddn;
    let mut gate = Gate::new();
    let mut forbidden = 0u32;
    while !shared.stopping() {
        let enabled = shared.config.read().eddn_enabled();
        let epoch = shared.epoch.load(Ordering::Relaxed);
        if !enabled {
            idle(&svc.notify, Duration::from_secs(300)).await;
            continue;
        }
        if let Some(d) = gate.wait(epoch, &svc.counters) {
            idle(&svc.notify, d).await;
            continue;
        }
        let now = Timestamp::now();
        let next = {
            let mut q = svc.queue.lock();
            let expired = q.retain(|it| it.expires > now);
            if !expired.is_empty() {
                svc.counters.failed(expired.len() as u64, "expired before it could be sent");
            }
            q.front().map(|e| (e.id, e.item.clone()))
        };
        let Some((id, item)) = next else {
            svc.flush.store(false, Ordering::Relaxed);
            idle(&svc.notify, Duration::from_secs(300)).await;
            continue;
        };
        let body = match gzip(item.json.as_bytes()) {
            Ok(b) if b.len() < EDDN_MAX_BODY => b,
            _ => {
                svc.queue.lock().remove(id);
                svc.counters.failed(1, format!("{} message too large to send", item.schema.path()));
                continue;
            }
        };
        let req = HttpRequest::post(eddn::UPLOAD_URL, body, Duration::from_secs(10))
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "gzip");
        match shared.transport.send(req).await {
            Ok(resp) if resp.is_success() => {
                svc.queue.lock().remove(id);
                svc.counters.sent(1);
                gate.success();
                forbidden = 0;
            }
            Ok(resp) if matches!(resp.status, 400 | 403 | 413 | 426) => {
                // Never retried (400/403/426), 413 dropped: a bad message must not block the queue.
                svc.queue.lock().remove(id);
                let reason = format!("EDDN rejected a {} message: HTTP {} {}", item.schema.path(), resp.status, short(&resp.body));
                tracing::warn!(%reason);
                svc.counters.failed(1, reason);
                if resp.status == 403 {
                    forbidden += 1;
                    if forbidden >= 3 {
                        // Probably a block of this software: stop instead of hammering the gateway.
                        forbidden = 0;
                        gate.stop(epoch, &svc.counters, "EDDN refuses uploads from Elite Nexus (HTTP 403)".into());
                    }
                }
            }
            other => {
                let reason = match other {
                    Ok(resp) => format!("EDDN: HTTP {} {}", resp.status, short(&resp.body)),
                    Err(e) => format!("EDDN: {e}"),
                };
                let attempts = item.attempts + 1;
                if attempts >= EDDN_MAX_ATTEMPTS {
                    svc.queue.lock().remove(id);
                    svc.counters.failed(1, format!("{reason} (gave up after {attempts} attempts)"));
                } else {
                    svc.queue.lock().update(id, |it| it.attempts = attempts);
                }
                gate.retry(&EDDN_BACKOFF, &svc.counters, reason);
            }
        }
        // One request at a time, at most ~2 per second.
        tokio::time::sleep(timing().eddn_spacing).await;
    }
}

// ---------------------------------------------------------------------------------------------
// EDSM sender
// ---------------------------------------------------------------------------------------------

async fn run_edsm(shared: Arc<Shared>) {
    let svc = &shared.edsm;
    let mut gate = Gate::new();
    let mut last_post: Option<Instant> = None;
    while !shared.stopping() {
        let (creds, software, version) = {
            let c = shared.config.read();
            (c.edsm_sending().cloned(), c.software().to_string(), c.version().to_string())
        };
        let epoch = shared.epoch.load(Ordering::Relaxed);
        let Some(creds) = creds else {
            idle(&svc.notify, Duration::from_secs(300)).await;
            continue;
        };
        if let Some(d) = gate.wait(epoch, &svc.counters) {
            idle(&svc.notify, d).await;
            continue;
        }
        if svc.queue.lock().is_empty() {
            svc.flush.store(false, Ordering::Relaxed);
            idle(&svc.notify, Duration::from_secs(300)).await;
            continue;
        }
        refresh_list(&shared, &shared.edsm_discard, edsm::DISCARD_URL, "edsm-discard.json").await;
        let discard = shared.edsm_discard.read().set.clone();
        let fp = fingerprint(&[&creds.commander_name, &creds.api_key]);
        let now = Timestamp::now();
        let batch: Vec<(u64, EdsmItem)> = {
            let mut q = svc.queue.lock();
            let removed = q.retain(|it| it.cred == fp && !edsm::is_discarded(&discard, &it.event) && now.0 - it.queued_at.0 < EDSM_TTL_SECS);
            let expired = removed.iter().filter(|it| it.cred == fp && now.0 - it.queued_at.0 >= EDSM_TTL_SECS).count();
            if expired > 0 {
                svc.counters.failed(expired as u64, "expired before it could be sent");
            }
            let mut batch = Vec::new();
            if let Some(first) = q.front() {
                let key = first.item.batch_key();
                let key = (key.0.to_string(), key.1.to_string(), key.2.to_string(), key.3);
                for e in q.iter() {
                    let k = e.item.batch_key();
                    if (k.0, k.1, k.2, k.3) != (key.0.as_str(), key.1.as_str(), key.2.as_str(), key.3) || batch.len() >= EDSM_MAX_BATCH {
                        break;
                    }
                    batch.push((e.id, e.item.clone()));
                }
            }
            batch
        };
        let Some((_, first)) = batch.first() else { continue };
        let quiet_for = now.0 - batch.iter().map(|(_, i)| i.queued_at.0).max().unwrap_or(now.0);
        let ready = svc.flush.load(Ordering::Relaxed)
            || batch.iter().any(|(_, i)| i.urgent)
            || batch.len() >= EDSM_MAX_BATCH
            || quiet_for >= timing().edsm_quiet_secs
            || now.0 - first.queued_at.0 >= timing().edsm_max_wait_secs;
        if !ready {
            idle(&svc.notify, Duration::from_secs((timing().edsm_quiet_secs - quiet_for).clamp(1, 60) as u64)).await;
            continue;
        }
        if let Some(t) = last_post
            && t.elapsed() < timing().edsm_min_gap
        {
            tokio::time::sleep(timing().edsm_min_gap - t.elapsed()).await;
            continue;
        }
        let message = format!("[{}]", batch.iter().map(|(_, i)| i.json.as_str()).collect::<Vec<_>>().join(","));
        let body = form_urlencode(&[
            ("commanderName", creds.commander_name.trim()),
            ("apiKey", creds.api_key.trim()),
            ("fromSoftware", &software),
            ("fromSoftwareVersion", &version),
            ("fromGameVersion", &first.game_version),
            ("fromGameBuild", &first.game_build),
            ("message", &message),
        ]);
        let req = HttpRequest::post(edsm::JOURNAL_URL, body.into_bytes(), Duration::from_secs(30))
            .header("Content-Type", "application/x-www-form-urlencoded; charset=UTF-8");
        last_post = Some(Instant::now());
        let ids: Vec<u64> = batch.iter().map(|(id, _)| *id).collect();
        match shared.transport.send(req).await {
            Err(e) => gate.retry(&EDSM_BACKOFF, &svc.counters, format!("EDSM: {e}")),
            Ok(resp) => {
                let now = Timestamp::now();
                let limit = edsm::rate_limit_wait(&resp, now);
                match edsm::classify_response(&resp, now) {
                    EdsmOutcome::Done(per_event) => {
                        remove_all(&svc.queue, &ids);
                        gate.success();
                        let (mut ok, mut bad) = (0u64, 0u64);
                        let mut last = None;
                        if per_event.is_empty() {
                            ok = ids.len() as u64;
                        }
                        for (code, msg) in per_event {
                            match code {
                                // 1xx stored; 4xx "ignorable" (unknown item, duplicate visit);
                                // 5xx kept by EDSM for re-processing.
                                100..=199 | 400..=599 => ok += 1,
                                // Discarded event (the live list changed): not an error.
                                304 => {}
                                _ => {
                                    bad += 1;
                                    last = Some(format!("EDSM {code}: {msg}"));
                                }
                            }
                        }
                        if ok > 0 {
                            svc.counters.sent(ok);
                        }
                        if let Some(l) = last {
                            svc.counters.failed(bad, l);
                        }
                        if let Some(w) = limit {
                            gate.pause(Duration::from_secs(w), &svc.counters, "EDSM rate limit reached".into());
                        }
                    }
                    EdsmOutcome::BadCredentials(msg) | EdsmOutcome::SoftwareRejected(msg) => gate.stop(epoch, &svc.counters, msg),
                    EdsmOutcome::BadBatch(msg) => {
                        remove_all(&svc.queue, &ids);
                        svc.counters.failed(ids.len() as u64, msg);
                    }
                    EdsmOutcome::RateLimited(secs) => gate.pause(Duration::from_secs(secs), &svc.counters, "EDSM rate limit reached".into()),
                    EdsmOutcome::Retry(msg) => gate.retry(&EDSM_BACKOFF, &svc.counters, msg),
                }
            }
        }
    }
}

fn remove_all<T: Serialize + DeserializeOwned>(q: &Mutex<DiskQueue<T>>, ids: &[u64]) {
    let set: HashSet<u64> = ids.iter().copied().collect();
    let mut q = q.lock();
    let keep: Vec<u64> = q.iter().map(|e| e.id).filter(|id| set.contains(id)).collect();
    for id in keep {
        q.remove(id);
    }
}

// ---------------------------------------------------------------------------------------------
// Inara sender
// ---------------------------------------------------------------------------------------------

async fn run_inara(shared: Arc<Shared>) {
    let svc = &shared.inara;
    let mut gate = Gate::new();
    // Pacing survives restarts: an app restart must not exceed Inara's limit or end a lock-out.
    let mut pacing = InaraPacing::load(&shared);
    let wall_now = Timestamp::now();
    let mut last_post: Option<Instant> = pacing.last_post.and_then(|t| {
        let ago = u64::try_from(wall_now.0 - t.0).ok()?;
        Instant::now().checked_sub(Duration::from_secs(ago))
    });
    if let Some(until) = pacing.locked_until
        && let Ok(left) = u64::try_from(until.0 - wall_now.0)
        && left > 0
    {
        gate.pause(Duration::from_secs(left), &svc.counters, "Inara rate limit (from a previous run)".into());
    }
    while !shared.stopping() {
        let (creds, software, version) = {
            let c = shared.config.read();
            (c.inara_sending().cloned(), c.software().to_string(), c.version().to_string())
        };
        let epoch = shared.epoch.load(Ordering::Relaxed);
        let Some(creds) = creds else {
            idle(&svc.notify, Duration::from_secs(300)).await;
            continue;
        };
        if let Some(d) = gate.wait(epoch, &svc.counters) {
            idle(&svc.notify, d).await;
            continue;
        }
        // "Who is this key?": normally asked inside the next batch (no extra request); on its own
        // only right after the key was entered or changed and nothing else is waiting.
        if shared.inara_check_now.load(Ordering::Relaxed) && svc.queue.lock().is_empty() {
            if let Some(t) = last_post
                && t.elapsed() < timing().inara_min_gap
            {
                // Still due after the wait ("Check again" right after another request).
                tokio::time::sleep(timing().inara_min_gap - t.elapsed()).await;
                continue;
            }
            shared.inara_check_now.store(false, Ordering::Relaxed);
            let header = json!({
                "appName": software, "appVersion": version, "isBeingDeveloped": creds.is_being_developed, "APIkey": creds.api_key.trim(),
            });
            let event = json!({ "eventName": "getCommanderProfile", "eventTimestamp": Timestamp::now().to_iso(), "eventData": {} });
            let body = json!({ "header": header, "events": [event] }).to_string();
            let req = HttpRequest::post(inara::API_URL, body.into_bytes(), Duration::from_secs(20)).header("Content-Type", "application/json");
            last_post = Some(Instant::now());
            pacing.last_post = Some(Timestamp::now());
            pacing.save(&shared);
            match shared.transport.send(req).await {
                Err(e) => gate.retry(&INARA_BACKOFF, &svc.counters, format!("Inara: {e}")),
                Ok(resp) => match inara::profile_response(&resp) {
                    Ok(account) => {
                        shared.inara_check.store(false, Ordering::Relaxed);
                        *shared.inara_account.lock() = Some(account);
                        gate.success();
                    }
                    Err(InaraOutcome::RateLimited(text)) => {
                        let lock = timing().inara_lockout;
                        pacing.locked_until = Some(Timestamp(Timestamp::now().0 + lock.as_secs() as i64));
                        pacing.save(&shared);
                        gate.pause(lock, &svc.counters, format!("Inara rate limit: {text}"));
                    }
                    Err(InaraOutcome::Rejected(text)) => {
                        shared.inara_check.store(false, Ordering::Relaxed);
                        gate.stop(epoch, &svc.counters, text);
                    }
                    // The key works, but Inara returned no profile for it.
                    Err(InaraOutcome::Done(statuses)) => {
                        shared.inara_check.store(false, Ordering::Relaxed);
                        let text = statuses.first().and_then(|(_, t)| t.clone()).unwrap_or_else(|| "no commander profile".into());
                        svc.counters.error(format!("Inara profile: {text}"));
                    }
                    Err(InaraOutcome::Retry(text)) => gate.retry(&INARA_BACKOFF, &svc.counters, format!("Inara: {text}")),
                },
            }
            continue;
        }
        let key = fingerprint(&[&creds.api_key]);
        let now = Timestamp::now();
        let batch: Vec<(u64, InaraItem)> = {
            let mut q = svc.queue.lock();
            let removed = q.retain(|it| it.key == key && now.0 - it.queued_at.0 < INARA_TTL_SECS && now.0 - it.event_time.0 < 29 * 86400);
            let expired = removed.iter().filter(|it| it.key == key).count();
            if expired > 0 {
                svc.counters.failed(expired as u64, "expired before it could be sent");
            }
            let mut batch: Vec<(u64, InaraItem)> = Vec::new();
            if let Some(first) = q.front() {
                let (cmdr, fid) = (first.item.cmdr.clone(), first.item.fid.clone());
                for e in q.iter() {
                    if e.item.cmdr != cmdr || e.item.fid != fid || batch.len() >= INARA_MAX_BATCH {
                        break;
                    }
                    batch.push((e.id, e.item.clone()));
                }
            }
            batch
        };
        let Some((_, first)) = batch.first() else {
            svc.flush.store(false, Ordering::Relaxed);
            idle(&svc.notify, Duration::from_secs(300)).await;
            continue;
        };
        let age = now.0 - first.queued_at.0;
        if !svc.flush.load(Ordering::Relaxed) && age < timing().inara_gather_secs {
            idle(&svc.notify, Duration::from_secs((timing().inara_gather_secs - age).clamp(1, 60) as u64)).await;
            continue;
        }
        if let Some(t) = last_post
            && t.elapsed() < timing().inara_min_gap
        {
            // Never more than ~2 requests a minute: Inara locks the key for an hour otherwise.
            tokio::time::sleep(timing().inara_min_gap - t.elapsed()).await;
            continue;
        }
        let mut header = serde_json::Map::new();
        header.insert("appName".into(), json!(software));
        header.insert("appVersion".into(), json!(version));
        header.insert("isBeingDeveloped".into(), json!(creds.is_being_developed));
        header.insert("APIkey".into(), json!(creds.api_key.trim()));
        header.insert("commanderName".into(), json!(first.cmdr));
        if !first.fid.is_empty() {
            header.insert("commanderFrontierID".into(), json!(first.fid));
        }
        // A pending "who is this key?" rides along as the first event.
        let check = shared.inara_check.load(Ordering::Relaxed);
        let mut events: Vec<Value> = Vec::with_capacity(batch.len() + 1);
        if check {
            events.push(json!({ "eventName": "getCommanderProfile", "eventTimestamp": Timestamp::now().to_iso(), "eventData": {} }));
        }
        events.extend(batch.iter().map(|(_, i)| serde_json::to_value(&i.event).unwrap_or(Value::Null)));
        let body = json!({"header": header, "events": events}).to_string();
        let req = HttpRequest::post(inara::API_URL, body.into_bytes(), Duration::from_secs(20)).header("Content-Type", "application/json");
        last_post = Some(Instant::now());
        pacing.last_post = Some(Timestamp::now());
        pacing.save(&shared);
        let ids: Vec<u64> = batch.iter().map(|(id, _)| *id).collect();
        match shared.transport.send(req).await {
            Err(e) => gate.retry(&INARA_BACKOFF, &svc.counters, format!("Inara: {e}")),
            Ok(resp) => match inara::classify_response(&resp) {
                InaraOutcome::Done(mut statuses) => {
                    if check {
                        shared.inara_check.store(false, Ordering::Relaxed);
                        shared.inara_check_now.store(false, Ordering::Relaxed);
                        match inara::profile_response(&resp) {
                            Ok(account) => *shared.inara_account.lock() = Some(account),
                            Err(InaraOutcome::Done(st)) => {
                                let text = st.first().and_then(|(_, t)| t.clone()).unwrap_or_else(|| "no commander profile".into());
                                svc.counters.error(format!("Inara profile: {text}"));
                            }
                            Err(_) => {}
                        }
                        if !statuses.is_empty() {
                            statuses.remove(0);
                        }
                    }
                    remove_all(&svc.queue, &ids);
                    gate.success();
                    let mut ok = 0u64;
                    let mut bad = 0u64;
                    let mut last = None;
                    for (i, (status, text)) in statuses.iter().enumerate() {
                        if (200..300).contains(status) {
                            ok += 1;
                        } else {
                            bad += 1;
                            let name = batch.get(i).map(|(_, it)| it.event.name.as_str()).unwrap_or("?");
                            last = Some(format!("Inara {status} for {name}: {}", text.clone().unwrap_or_default()));
                        }
                    }
                    if statuses.is_empty() {
                        ok = ids.len() as u64;
                    }
                    if ok > 0 {
                        svc.counters.sent(ok);
                    }
                    if let Some(l) = last {
                        tracing::info!(error = %l, "uplink: Inara rejected an event");
                        svc.counters.failed(bad, l);
                    }
                }
                InaraOutcome::RateLimited(text) => {
                    let lock = timing().inara_lockout;
                    pacing.locked_until = Some(Timestamp(Timestamp::now().0 + lock.as_secs() as i64));
                    pacing.save(&shared);
                    gate.pause(lock, &svc.counters, format!("Inara rate limit: {text}"));
                }
                InaraOutcome::Rejected(text) => gate.stop(epoch, &svc.counters, text),
                InaraOutcome::Retry(text) => gate.retry(&INARA_BACKOFF, &svc.counters, format!("Inara: {text}")),
            },
        }
    }
}

// ---------------------------------------------------------------------------------------------
// EDAstro sender
// ---------------------------------------------------------------------------------------------

async fn run_edastro(shared: Arc<Shared>) {
    let svc = &shared.edastro;
    let mut gate = Gate::new();
    let mut last_post: Option<Instant> = None;
    let mut attempts = 0u32;
    while !shared.stopping() {
        let (enabled, software, version) = {
            let c = shared.config.read();
            (c.edastro_enabled(), c.software().to_string(), c.version().to_string())
        };
        let epoch = shared.epoch.load(Ordering::Relaxed);
        if !enabled {
            idle(&svc.notify, Duration::from_secs(300)).await;
            continue;
        }
        if let Some(d) = gate.wait(epoch, &svc.counters) {
            idle(&svc.notify, d).await;
            continue;
        }
        if svc.queue.lock().is_empty() {
            svc.flush.store(false, Ordering::Relaxed);
            idle(&svc.notify, Duration::from_secs(300)).await;
            continue;
        }
        refresh_list(&shared, &shared.edastro_accepting, edastro::ACCEPTING_URL, "edastro-accepting.json").await;
        let accepting = shared.edastro_accepting.read().set.clone();
        let now = Timestamp::now();
        let batch: Vec<(u64, EdastroItem)> = {
            let mut q = svc.queue.lock();
            q.retain(|it| {
                now.0 - it.queued_at.0 < EDASTRO_TTL_SECS
                    && it.event.get("event").and_then(Value::as_str).is_some_and(|e| accepting.contains(e))
            });
            let mut batch: Vec<(u64, EdastroItem)> = Vec::new();
            if let Some(first) = q.front() {
                let key = (first.item.cmdr.clone(), first.item.system.clone(), first.item.odyssey);
                for e in q.iter() {
                    if (e.item.cmdr.clone(), e.item.system.clone(), e.item.odyssey) != key || batch.len() >= EDASTRO_MAX_BATCH {
                        break;
                    }
                    batch.push((e.id, e.item.clone()));
                }
            }
            batch
        };
        let Some((_, first)) = batch.first() else {
            svc.flush.store(false, Ordering::Relaxed);
            idle(&svc.notify, Duration::from_secs(300)).await;
            continue;
        };
        let age = now.0 - first.queued_at.0;
        if !svc.flush.load(Ordering::Relaxed) && age < timing().edastro_gather_secs {
            idle(&svc.notify, Duration::from_secs((timing().edastro_gather_secs - age).clamp(1, 60) as u64)).await;
            continue;
        }
        if let Some(t) = last_post
            && t.elapsed() < timing().edastro_min_gap
        {
            tokio::time::sleep(timing().edastro_min_gap - t.elapsed()).await;
            continue;
        }
        let mut header = serde_json::Map::new();
        header.insert("appName".into(), json!(software));
        header.insert("appVersion".into(), json!(version));
        if let Some(o) = first.odyssey {
            header.insert("odyssey".into(), json!(o));
        }
        if !first.system.is_empty() {
            header.insert("system".into(), json!(first.system));
        }
        let mut body = vec![Value::Object(header)];
        body.extend(batch.iter().map(|(_, i)| i.event.clone()));
        let req = HttpRequest::post(edastro::JOURNAL_URL, Value::Array(body).to_string().into_bytes(), Duration::from_secs(20))
            .header("Content-Type", "application/json");
        last_post = Some(Instant::now());
        let ids: Vec<u64> = batch.iter().map(|(id, _)| *id).collect();
        let res = shared.transport.send(req).await.map_err(|e| e.0).and_then(|r| edastro::classify_response(&r));
        match res {
            Ok(()) => {
                remove_all(&svc.queue, &ids);
                svc.counters.sent(ids.len() as u64);
                gate.success();
                attempts = 0;
            }
            Err(e) => {
                attempts += 1;
                if attempts >= EDASTRO_MAX_ATTEMPTS {
                    remove_all(&svc.queue, &ids);
                    svc.counters.failed(ids.len() as u64, format!("EDAstro: {e} (gave up)"));
                    attempts = 0;
                }
                gate.retry(&EDASTRO_BACKOFF, &svc.counters, format!("EDAstro: {e}"));
            }
        }
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
