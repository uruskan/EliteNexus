//! The local galaxy/market database and the live EDDN feed that keeps it current.
//!
//! Fed from three sides: the EDDN relay (every commander's market, station and ring data, when
//! "Receive the live market feed" is on), the commander's own journal history at start-up, and
//! live journal events / Market.json while playing. Queries run in background jobs (pages/market.rs).

use crate::core::{Core, HistorySink, LiveMsg};
use crate::jobs::Job;
use crate::settings::PriceWatch;
use crossbeam_channel::{Receiver, Sender};
use nexus_eddn::{EddnHandle, EddnStatsSnapshot, EddnSubscriber};
use nexus_galaxy::{DbStats, GalaxyDb, ImportProgress, ImportSummary, MarketUpdate, StationMarket};
use nexus_journal::{Companion, Timestamp};
use nexus_model::{Alert, AlertKind, Priority};
use parking_lot::{Mutex, RwLock};
use rustc_hash::FxHashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the database counters are counted again while a page shows them.
const STATS_EVERY: Duration = Duration::from_secs(10);

/// Daily Spansh dump of every system with stations: services, pads, markets (about 4.5 GB gzip).
pub const SPANSH_STATIONS_URL: &str = "https://downloads.spansh.co.uk/galaxy_stations.json.gz";

/// Journal events the galaxy database learns from (plus Market.json as "Market"). Scan /
/// FSSBodySignals / SAASignalsFound / Disembark feed the planets (footfall, biology).
const RELEVANT: [&str; 10] =
    ["FSDJump", "Location", "CarrierJump", "Docked", "SAASignalsFound", "ApproachSettlement", "Market", "Scan", "FSSBodySignals", "Disembark"];

/// Whether a journal line is worth parsing for the galaxy database. `Scan` is frequent and mostly
/// stars, belt clusters and planets nobody lands on: those are skipped before they are copied.
pub(crate) fn relevant(event: &str, line: &str) -> bool {
    RELEVANT.contains(&event) && nexus_galaxy::journal_line_useful(event, line)
}

pub struct MarketService {
    pub db: Option<GalaxyDb>,
    pub db_error: Option<String>,
    eddn: Option<EddnHandle>,
    /// The last database counters, counted on a worker thread: the counts scan big tables, which froze the window
    /// for a second or more while the live feed was filling them.
    stats: Option<DbStats>,
    stats_job: Option<Job<Option<DbStats>>>,
    stats_at: Option<Instant>,
    import: Option<ImportJob>,
    watch: Arc<RwLock<WatchState>>,
    /// Price alerts raised by the watcher thread (drained by the app with the core's alerts).
    pub alerts: Receiver<Alert>,
    ctx: egui::Context,
}

impl MarketService {
    pub fn open(data_dir: &Path, ctx: &egui::Context) -> MarketService {
        let watch = Arc::new(RwLock::new(WatchState::default()));
        let (tx, alerts) = crossbeam_channel::unbounded();
        let (db, db_error) = match GalaxyDb::open(data_dir.join("galaxy.db")) {
            Ok(db) => {
                start_watcher(db.clone(), watch.clone(), tx, ctx.clone());
                (Some(db), None)
            }
            Err(e) => {
                tracing::error!("galaxy database: {e}");
                (None, Some(format!("Cannot open the market database: {e}")))
            }
        };
        MarketService { db, db_error, eddn: None, stats: None, stats_job: None, stats_at: None, import: None, watch, alerts, ctx: ctx.clone() }
    }

    /// Replaces the price-alert rules (call when they change).
    pub fn set_watches(&self, watches: &[PriceWatch]) {
        let rules = watches
            .iter()
            .map(|w| (w.clone(), self.db.as_ref().and_then(|db| db.commodity_symbol(&w.commodity))))
            .collect();
        self.watch.write().rules = rules;
    }

    /// The commander's position, for the rules' distance limits.
    pub fn set_origin(&self, origin: Option<[f64; 3]>) {
        if self.watch.read().origin != origin {
            self.watch.write().origin = origin;
        }
    }

    /// Whether a rule's commodity name is known (for the editor).
    pub fn resolves(&self, commodity: &str) -> bool {
        self.db.as_ref().is_none_or(|db| db.commodity_symbol(commodity).is_some())
    }

    /// For [`Core::start`]: the commander's own history seeds stations, systems and hotspots.
    /// Parsing happens on a separate thread so the history load is not slowed down.
    pub fn history_sink(&self) -> Option<HistorySink> {
        let db = self.db.clone()?;
        let tx = ingest_thread(db, "galaxy-history");
        Some(Box::new(move |name: &str, line: &str| {
            if relevant(name, line) {
                let _ = tx.send((name.to_string(), line.to_string()));
            }
        }))
    }

    /// Follows live journal events and Market.json updates of a (re)started core.
    pub fn attach(&self, core: &Core) {
        let Some(db) = self.db.clone() else { return };
        let rx: Receiver<LiveMsg> = core.subscribe();
        let spawned = std::thread::Builder::new().name("galaxy-live".into()).spawn(move || {
            // Ends when the core stops (its senders drop).
            for msg in rx {
                let (event, text) = match &msg {
                    LiveMsg::Event(e, _) if relevant(&e.raw.name, &e.raw.line) => (&*e.raw.name, &*e.raw.line),
                    LiveMsg::Companion(Companion::Market(_, raw), _) => ("Market", &**raw),
                    _ => continue,
                };
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
                    db.ingest_journal(event, &v);
                }
            }
        });
        if let Err(e) = spawned {
            tracing::error!("galaxy: cannot start the live feed thread: {e}");
        }
    }

    /// Starts or stops the EDDN subscriber.
    pub fn set_listen(&mut self, on: bool, rt: &tokio::runtime::Handle) {
        match (on, self.eddn.is_some(), &self.db) {
            (true, false, Some(db)) => self.eddn = Some(EddnSubscriber::spawn(rt, db.clone())),
            (false, true, _) => self.eddn = None, // dropping the handle stops it
            _ => {}
        }
    }

    /// (connected, messages per minute) while the feed is on; cheap enough for every frame.
    pub fn feed_state(&self) -> Option<(bool, u64)> {
        let s = self.eddn.as_ref()?.stats();
        Some((s.is_connected(), s.messages_per_minute()))
    }

    /// (connected, "N msgs/min · …") while the feed is on. "Updates stored" are those that changed the database:
    /// most scanned bodies aren't kept, and a market report older than the stored one isn't either.
    pub fn feed_summary(&self) -> Option<(bool, String)> {
        let s = self.eddn.as_ref()?.stats().snapshot();
        let mut line = format!(
            "{} msgs/min · {} received · {} updates stored",
            s.messages_per_minute,
            nexus_data::names::group_digits(s.total_messages as i64),
            nexus_data::names::group_digits(s.stored as i64)
        );
        if !s.connected
            && let Some(e) = &s.last_error
        {
            line = format!("{line} · {e}");
        }
        Some((s.connected, line))
    }

    /// The last database counters; asking counts them again on a worker thread every [`STATS_EVERY`] at most.
    pub fn db_stats(&mut self) -> Option<&DbStats> {
        if let Some(job) = self.stats_job.as_mut() {
            if let Some(s) = job.poll() {
                self.stats_job = None;
                self.stats = s.or(self.stats.take());
            } else if job.elapsed() > Duration::from_secs(120) {
                // Never answered (the thread died): count again.
                self.stats_job = None;
            }
        }
        if self.stats_job.is_none()
            && self.stats_at.is_none_or(|t| t.elapsed() > STATS_EVERY)
            && let Some(db) = self.db.clone()
        {
            self.stats_at = Some(Instant::now());
            self.stats_job = Some(Job::spawn(&self.ctx, move || db.stats().ok()));
        }
        self.stats.as_ref()
    }

    pub fn feed_snapshot(&self) -> Option<EddnStatsSnapshot> {
        self.eddn.as_ref().map(|h| h.stats().snapshot())
    }

    /// The live feed's newest message in words ("Docking granted at Ray Gateway"), while the feed is on.
    pub fn latest_headline(&self) -> Option<String> {
        self.eddn.as_ref()?.stats().headlines(1).pop().map(|h| h.text)
    }

    pub fn db_summary(&mut self) -> Option<String> {
        let s = self.db_stats()?;
        Some(format!(
            "{} stations · {} markets · {} prices · {} systems · {:.1} MB",
            nexus_data::names::group_digits(s.stations as i64),
            nexus_data::names::group_digits(s.stations_with_market as i64),
            nexus_data::names::group_digits(s.market_rows as i64),
            nexus_data::names::group_digits(s.systems as i64),
            s.db_bytes as f64 / 1_048_576.0
        ))
    }
}

/// A thread that parses (event, json) pairs and hands them to the database writer.
fn ingest_thread(db: GalaxyDb, name: &str) -> Sender<(String, String)> {
    let (tx, rx) = crossbeam_channel::unbounded::<(String, String)>();
    let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
        for (event, line) in rx {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                db.ingest_journal(&event, &v);
            }
        }
    });
    if let Err(e) = spawned {
        tracing::error!("galaxy: cannot start {name}: {e}");
    }
    tx
}

// ---------------------------------------------------------------------------------------------
// Price alerts: every committed live market (EDDN or own journal) is checked against the rules
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct WatchState {
    /// Each rule with its resolved commodity symbol (None = unknown commodity).
    rules: Vec<(PriceWatch, Option<String>)>,
    origin: Option<[f64; 3]>,
}

/// The same rule does not fire again for the same station within this time.
const WATCH_COOLDOWN: Duration = Duration::from_secs(3600);

fn start_watcher(db: GalaxyDb, state: Arc<RwLock<WatchState>>, tx: Sender<Alert>, ctx: egui::Context) {
    let updates = db.subscribe_updates();
    let spawned = std::thread::Builder::new().name("price-alerts".into()).spawn(move || {
        let mut fired: FxHashMap<(usize, u64), Instant> = FxHashMap::default();
        for u in updates {
            let (rules, origin) = {
                let s = state.read();
                if !s.rules.iter().any(|(w, sym)| w.enabled && sym.is_some()) {
                    continue;
                }
                (s.rules.clone(), s.origin)
            };
            let dist = origin.zip(u.pos).map(|(a, b)| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt());
            let mut market: Option<Option<StationMarket>> = None;
            for (i, (w, sym)) in rules.iter().enumerate() {
                let Some(sym) = sym.as_deref().filter(|_| w.enabled) else { continue };
                if !rule_applies(w, &u, dist) {
                    continue;
                }
                // One station lookup per update, only when some rule could match.
                let Some(m) = market.get_or_insert_with(|| db.station_market(u.market_id).ok().flatten()) else { break };
                let Some(alert) = check_rule(w, sym, &u, m, dist) else { continue };
                if fired.get(&(i, u.market_id)).is_some_and(|t| t.elapsed() < WATCH_COOLDOWN) {
                    continue;
                }
                fired.insert((i, u.market_id), Instant::now());
                if tx.send(alert).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
            if fired.len() > 10_000 {
                fired.retain(|_, t| t.elapsed() < WATCH_COOLDOWN);
            }
        }
    });
    if let Err(e) = spawned {
        tracing::error!("galaxy: cannot start the price-alert thread: {e}");
    }
}

/// Cheap checks that need no database lookup.
fn rule_applies(w: &PriceWatch, u: &MarketUpdate, dist: Option<f64>) -> bool {
    (w.carriers || !u.is_carrier) && (w.max_ly <= 0.0 || dist.is_some_and(|d| d <= w.max_ly))
}

fn pad_rank(p: char) -> u8 {
    match p.to_ascii_uppercase() {
        'S' => 1,
        'M' => 2,
        'L' => 3,
        _ => 0,
    }
}

/// The alert a rule raises for a freshly updated market, if it matches.
fn check_rule(w: &PriceWatch, symbol: &str, u: &MarketUpdate, m: &StationMarket, dist: Option<f64>) -> Option<Alert> {
    if let Some(p) = w.min_pad
        && !m.station.max_pad.is_some_and(|have| pad_rank(have) >= pad_rank(p))
    {
        return None;
    }
    let row = m.items.iter().find(|r| r.symbol == symbol)?;
    let min_qty = w.min_qty.max(1);
    let (price, qty) = if w.sell {
        (row.sell >= w.price && row.demand >= min_qty).then_some((row.sell, row.demand))?
    } else {
        (row.buy > 0 && row.buy <= w.price && row.stock >= min_qty).then_some((row.buy, row.stock))?
    };
    let digits = nexus_data::names::group_digits;
    let where_ = match dist {
        Some(d) => format!("{} · {} · {d:.1} ly", u.station, u.system),
        None => format!("{} · {}", u.station, u.system),
    };
    let (verb, qty_word) = if w.sell { ("sells for", "demand") } else { ("can be bought for", "stock") };
    let carrier = if u.is_carrier { " · fleet carrier" } else { "" };
    let spoken_dist = dist.map(|d| format!(", {d:.0} light years away")).unwrap_or_default();
    Some(
        Alert::new(Timestamp::now(), AlertKind::Trade, Priority::High, format!("{} {verb} {} Cr", row.name, digits(price)))
            .detail(format!("{where_} · {qty_word} {}{carrier}", digits(qty)))
            .speak(format!("{} {verb} {} at {}{spoken_dist}.", row.name, spoken_credits(price), u.station)),
    )
}

/// "62 thousand", "1.4 million" (TTS reads "62,000 Cr" poorly).
fn spoken_credits(v: i64) -> String {
    if v >= 1_000_000 {
        format!("{:.1} million credits", v as f64 / 1e6)
    } else if v >= 10_000 {
        format!("{} thousand credits", v / 1000)
    } else {
        format!("{v} credits")
    }
}

// ---------------------------------------------------------------------------------------------
// Baseline import (Spansh dump, streamed: nothing is written to disk but the database)
// ---------------------------------------------------------------------------------------------

pub enum ImportSource {
    Url(String),
    File(PathBuf),
}

#[derive(Default)]
pub struct ImportStatus {
    pub source: String,
    /// Compressed size, when known (progress = bytes_read / total).
    pub total_bytes: Option<u64>,
    pub progress: ImportProgress,
    pub started: Option<Instant>,
    pub finished: Option<Result<ImportSummary, String>>,
}

pub struct ImportJob {
    pub status: Arc<Mutex<ImportStatus>>,
    cancel: Arc<AtomicBool>,
}

impl ImportJob {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn running(&self) -> bool {
        self.status.lock().finished.is_none()
    }
}

impl MarketService {
    pub fn import(&self) -> Option<&ImportJob> {
        self.import.as_ref()
    }

    /// Streams a dump into the database on a background thread (live EDDN data keeps flowing;
    /// markets the database already has newer data for are skipped).
    pub fn start_import(&mut self, src: ImportSource, ctx: &egui::Context, rt: &tokio::runtime::Handle) {
        let Some(db) = self.db.clone() else { return };
        if self.import.as_ref().is_some_and(ImportJob::running) {
            return;
        }
        let label = match &src {
            ImportSource::Url(u) => u.clone(),
            ImportSource::File(p) => p.display().to_string(),
        };
        let status = Arc::new(Mutex::new(ImportStatus { source: label, started: Some(Instant::now()), ..Default::default() }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (st, stop, ctx, rt) = (status.clone(), cancel.clone(), ctx.clone(), rt.clone());
        let spawned = std::thread::Builder::new().name("galaxy-import".into()).spawn(move || {
            let opened: Result<(Box<dyn Read + Send>, Option<u64>), String> = match src {
                ImportSource::Url(url) => http_reader(&url, &rt).map(|(r, n)| (Box::new(r) as Box<dyn Read + Send>, n)),
                ImportSource::File(path) => std::fs::File::open(&path)
                    .map(|f| {
                        let n = f.metadata().ok().map(|m| m.len());
                        (Box::new(f) as Box<dyn Read + Send>, n)
                    })
                    .map_err(|e| format!("{}: {e}", path.display())),
            };
            let result = opened.and_then(|(reader, total)| {
                st.lock().total_bytes = total;
                db.import_spansh_dump(reader, |p| {
                    st.lock().progress = p.clone();
                    ctx.request_repaint();
                    !stop.load(Ordering::Relaxed)
                })
                .map_err(|e| e.to_string())
            });
            if let Err(e) = &result {
                tracing::warn!("galaxy import failed: {e}");
            }
            st.lock().finished = Some(result);
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => self.import = Some(ImportJob { status, cancel }),
            Err(e) => tracing::error!("galaxy: cannot start the import thread: {e}"),
        }
    }
}

/// An HTTP body as a blocking `Read`, downloaded on the shared runtime with back-pressure (at most
/// 64 chunks buffered), so a 4.5 GB dump never sits in memory or on disk.
fn http_reader(url: &str, rt: &tokio::runtime::Handle) -> Result<(BodyReader, Option<u64>), String> {
    let (meta_tx, meta_rx) = tokio::sync::oneshot::channel::<Result<Option<u64>, String>>();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, String>>(64);
    let url = url.to_string();
    rt.spawn(async move {
        let client = reqwest::Client::builder()
            .user_agent(concat!("EliteNexus/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(90))
            .build();
        let sent = match client {
            Ok(c) => c.get(&url).send().await.and_then(|r| r.error_for_status()),
            Err(e) => Err(e),
        };
        let mut resp = match sent {
            Ok(r) => r,
            Err(e) => {
                let _ = meta_tx.send(Err(e.to_string()));
                return;
            }
        };
        let _ = meta_tx.send(Ok(resp.content_length()));
        loop {
            let item = match resp.chunk().await {
                Ok(Some(b)) => Ok(b.to_vec()),
                Ok(None) => return,
                Err(e) => Err(e.to_string()),
            };
            let failed = item.is_err();
            // The reader went away (import finished or cancelled): stop downloading.
            if tx.send(item).await.is_err() || failed {
                return;
            }
        }
    });
    let total = meta_rx.blocking_recv().map_err(|_| "download task stopped".to_string())??;
    Ok((BodyReader { rx, buf: Vec::new(), pos: 0 }, total))
}

struct BodyReader {
    rx: tokio::sync::mpsc::Receiver<Result<Vec<u8>, String>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for BodyReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pos >= self.buf.len() {
            match self.rx.blocking_recv() {
                Some(Ok(b)) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Some(Err(e)) => return Err(std::io::Error::other(e)),
                None => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TempDb, good, station};
    use serde_json::json;

    #[test]
    fn price_alert_fires_for_the_matching_market_only() {
        let t = TempDb::new("alerts");
        let db = t.db();
        let rule = PriceWatch { commodity: "Gold".into(), sell: true, price: 50_000, max_ly: 20.0, min_pad: Some('L'), carriers: false, min_qty: 10, enabled: true };
        let state = Arc::new(RwLock::new(WatchState { rules: vec![(rule, db.commodity_symbol("Gold"))], origin: Some([0.0; 3]) }));
        let (tx, rx) = crossbeam_channel::unbounded();
        start_watcher(db.clone(), state, tx, egui::Context::default());

        station(db, ("Far", 1003, 100.0), "Far Port", 5003, json!([good("gold", 0, 0, 70_000, 80)])); // too far
        station(db, ("Beta", 1002, 10.0), "Beta Hub", 5002, json!([good("gold", 0, 0, 40_000, 80)])); // too cheap
        station(db, ("Gamma", 1004, 5.0), "Gamma Dock", 5004, json!([good("gold", 0, 0, 65_000, 5)])); // demand too low
        station(db, ("Alpha", 1001, 0.0), "Alpha Port", 5001, json!([good("gold", 0, 0, 60_000, 80)])); // match
        db.flush().unwrap();

        let a = rx.recv_timeout(Duration::from_secs(5)).expect("an alert for Alpha Port");
        assert_eq!(a.title, "Gold sells for 60,000 Cr");
        assert!(a.detail.contains("Alpha Port") && a.detail.contains("0.0 ly"), "{}", a.detail);
        assert!(a.speech.as_deref().is_some_and(|s| s.contains("60 thousand credits")));
        // The same market again within the cooldown stays quiet.
        station(db, ("Alpha", 1001, 0.0), "Alpha Port", 5001, json!([good("gold", 0, 0, 61_000, 80)]));
        db.flush().unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(700)).is_err(), "only one alert");
    }

    /// Start-up history replay into the galaxy database with and without the body events, on this
    /// PC's journals plus a synthetic heavy explorer (the commander's planet scans repeated across
    /// many systems). `cargo test -p nexus-app -- --ignored history_replay_cost --nocapture`
    #[test]
    #[ignore]
    fn history_replay_cost() {
        const OLD: [&str; 7] = ["FSDJump", "Location", "CarrierJump", "Docked", "SAASignalsFound", "ApproachSettlement", "Market"];
        let Some(dir) = nexus_journal::default_journal_dir() else { return };
        let files = nexus_journal::list_journal_files(&dir).unwrap_or_default();
        let mut lines: Vec<(String, String)> = Vec::new();
        for f in &files {
            let Ok(text) = std::fs::read_to_string(&f.path) else { continue };
            for line in text.lines() {
                if let Some((_, name)) = nexus_journal::raw::scan_line(line) {
                    lines.push((name.to_string(), line.to_string()));
                }
            }
        }
        // What the core thread pays in the sink (filter + copy), and what the galaxy-history thread and
        // the writer pay (parse + convert + commit), for a filter.
        let replay = |label: &str, input: &[(String, String)], keep: &dyn Fn(&str, &str) -> bool| {
            let t = TempDb::new(&format!("replay-{label}"));
            let db = t.db();
            let started = Instant::now();
            let sent: Vec<(String, String)> = input.iter().filter(|(n, l)| keep(n, l)).map(|(n, l)| (n.clone(), l.clone())).collect();
            let sink = started.elapsed();
            let started = Instant::now();
            let mut items = 0;
            for (name, line) in &sent {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    items += db.ingest_journal(name, &v);
                }
            }
            let parse = started.elapsed();
            db.flush().unwrap();
            let total = started.elapsed();
            let id64s: rustc_hash::FxHashSet<u64> = sent
                .iter()
                .filter_map(|(_, l)| serde_json::from_str::<serde_json::Value>(l).ok()?.get("SystemAddress")?.as_u64())
                .collect();
            let bodies: usize = id64s.iter().map(|id| db.bodies_in_system(*id).map(|b| b.len()).unwrap_or(0)).sum();
            println!(
                "{label:>14}: {} lines -> {} forwarded ({:.1} ms sink), {items} items, parse+convert {:.0} ms, committed after {:.0} ms; {bodies} body rows",
                input.len(),
                sent.len(),
                sink.as_secs_f64() * 1e3,
                parse.as_secs_f64() * 1e3,
                total.as_secs_f64() * 1e3
            );
        };
        replay("before", &lines, &|n, _| OLD.contains(&n));
        replay("with bodies", &lines, &|n, l| relevant(n, l));
        replay("no pre-filter", &lines, &|n, _| RELEVANT.contains(&n));
        // A heavy explorer: every Scan line of this history, repeated in 400 different systems.
        let scans: Vec<&(String, String)> = lines.iter().filter(|(n, _)| n == "Scan").collect();
        let mut heavy: Vec<(String, String)> = Vec::new();
        for k in 0..400u64 {
            for (n, l) in &scans {
                let Ok(mut v) = serde_json::from_str::<serde_json::Value>(l) else { continue };
                let addr = v["SystemAddress"].as_u64().unwrap_or(0) + k * 1_000_003;
                v["SystemAddress"] = serde_json::json!(addr);
                heavy.push((n.clone(), v.to_string()));
            }
        }
        replay("heavy explorer", &heavy, &|n, l| relevant(n, l));
    }

    /// Streams Spansh's small daily systems dump (~4 MB) through the HTTP reader into a scratch
    /// database. Needs the network: `cargo test -p nexus-app -- --ignored streams_spansh`.
    #[test]
    #[ignore]
    fn streams_spansh_dump_over_http() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let dir = std::env::temp_dir().join(format!("nexus-import-test-{}", std::process::id()));
        let db = GalaxyDb::open(dir.join("galaxy.db")).unwrap();
        let (reader, total) = http_reader("https://downloads.spansh.co.uk/systems_1day.json.gz", rt.handle()).unwrap();
        let t = Instant::now();
        let sum = db.import_spansh_dump(reader, |_| true).unwrap();
        println!("{total:?} bytes -> {} systems in {:?}", sum.progress.systems, t.elapsed());
        assert!(total.unwrap_or(0) > 1_000_000);
        assert_eq!(sum.progress.bytes_read, total.unwrap());
        assert!(sum.progress.systems > 10_000, "{sum:?}");
        assert!(sum.stream_error.is_none(), "{sum:?}");
        drop(db);
        let _ = std::fs::remove_dir_all(dir);
    }
}
