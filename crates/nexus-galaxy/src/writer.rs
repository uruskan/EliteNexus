//! The single writer thread: receives [`Ingest`] items over a channel and commits them in batches
//! (one transaction at most every `commit_interval`, 250 ms by default).

use crate::GalaxyOptions;
use crate::bodies;
use crate::commodities::{CommodityCache, CommodityMeta, fallback_name, normalize_symbol};
use crate::error::Result;
use crate::index::StationIndex;
use crate::model::*;
use crate::names::clean_station_name;
use crate::services::{MaterialTrader, Services, TechBroker};
use crate::station::{Pad, StationKind};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError};
use parking_lot::{Mutex, RwLock};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use rustc_hash::FxHashMap;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Upper bound of ingest items per transaction (keeps a single commit short).
const MAX_BATCH_ITEMS: usize = 20_000;
const PRUNE_EVERY: Duration = Duration::from_secs(3600);

pub(crate) enum WriterMsg {
    Ingest(Vec<Ingest>),
    Import(ImportBatch),
    /// Reply once everything sent before it is committed.
    Flush(Sender<()>),
    /// Commit, then `PRAGMA wal_checkpoint(TRUNCATE)`.
    Checkpoint(Sender<()>),
}

impl WriterMsg {
    fn weight(&self) -> usize {
        match self {
            WriterMsg::Ingest(v) => v.len(),
            WriterMsg::Import(b) => b.ops.len(),
            WriterMsg::Flush(_) | WriterMsg::Checkpoint(_) => 0,
        }
    }

    /// Messages that end the gathering phase immediately.
    fn urgent(&self) -> bool {
        !matches!(self, WriterMsg::Ingest(_))
    }
}

/// A chunk of a dump import.
pub(crate) struct ImportBatch {
    pub ops: Vec<ImportOp>,
    pub hints: Vec<CommodityHint>,
    pub done: Sender<ImportAck>,
}

#[derive(Debug)]
pub(crate) enum ImportOp {
    Ingest(Ingest),
    FactionPresence(FactionPresence),
}

/// Commodity metadata seen in a dump (fills in commodities missing from the seed list).
#[derive(Debug, Clone)]
pub(crate) struct CommodityHint {
    pub symbol: String,
    pub name: String,
    pub category: String,
    pub fdev_id: Option<i64>,
}

/// One row of the Spansh factions dump.
#[derive(Debug, Clone)]
pub(crate) struct FactionPresence {
    pub system_id64: u64,
    pub system_name: String,
    pub pos: Option<[f64; 3]>,
    pub faction: String,
    pub allegiance: Option<String>,
    pub government: Option<String>,
    pub controlling: bool,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ImportAck {
    pub stale_markets: u64,
    pub errors: u64,
    pub committed: bool,
}

/// State shared between the writer thread and the [`crate::GalaxyDb`] handles.
#[derive(Default)]
pub(crate) struct Shared {
    pub subscribers: Mutex<Vec<Sender<MarketUpdate>>>,
    pub commodities: RwLock<CommodityCache>,
    pub stations: RwLock<StationIndex>,
    pub ingested: AtomicU64,
    pub market_snapshots: AtomicU64,
    pub stale_markets: AtomicU64,
    pub commits: AtomicU64,
    pub write_errors: AtomicU64,
    /// Rows in `price_history`, kept up to date by the writer: counting the table can take seconds once it holds
    /// a month of live prices. Only meaningful once `history_counted` (the writer counts once when it starts,
    /// before its first prune).
    pub history_rows: AtomicI64,
    pub history_counted: AtomicBool,
}

impl Shared {
    pub fn load_commodities(&self, conn: &Connection) -> Result<()> {
        let mut cache = CommodityCache::default();
        let mut stmt =
            conn.prepare("SELECT id, symbol, name, category, is_rare FROM commodities")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                CommodityMeta {
                    symbol: r.get(1)?,
                    name: r.get(2)?,
                    category: r.get(3)?,
                    rare: r.get::<_, i64>(4)? != 0,
                },
            ))
        })?;
        for row in rows {
            let (id, meta) = row?;
            cache.insert(id, meta);
        }
        *self.commodities.write() = cache;
        Ok(())
    }
}

pub(crate) struct Writer {
    conn: Connection,
    rx: Receiver<WriterMsg>,
    ctx: Ctx,
}

/// Everything the apply functions need besides the transaction.
struct Ctx {
    shared: Arc<Shared>,
    opts: GalaxyOptions,
    last_prune: Option<Instant>,
    /// Stations written in the current transaction (refreshed in the index after commit).
    dirty: RefCell<Vec<i64>>,
    /// Price history rows written in the current transaction (added to the shared count after commit).
    history_added: Cell<i64>,
}

#[derive(Default)]
struct BatchOutcome {
    updates: Vec<MarketUpdate>,
    stale: u64,
    errors: u64,
}

impl Writer {
    pub fn new(
        conn: Connection,
        rx: Receiver<WriterMsg>,
        shared: Arc<Shared>,
        opts: GalaxyOptions,
    ) -> Writer {
        Writer {
            conn,
            rx,
            ctx: Ctx {
                shared,
                opts,
                last_prune: None,
                dirty: RefCell::new(Vec::new()),
                history_added: Cell::new(0),
            },
        }
    }

    pub fn run(mut self) {
        let interval = self.ctx.opts.commit_interval;
        let mut last_commit = Instant::now()
            .checked_sub(interval)
            .unwrap_or_else(Instant::now);
        // Counted once, here, before anything changes the table; kept up to date from then on. Items queue meanwhile.
        match self.conn.query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get::<_, i64>(0)) {
            Ok(n) => {
                self.ctx.shared.history_rows.fetch_add(n, Ordering::Relaxed);
                self.ctx.shared.history_counted.store(true, Ordering::Relaxed);
            }
            Err(e) => tracing::warn!(error = %e, "couldn't count the price history"),
        }
        self.ctx.maybe_prune(&mut self.conn);
        loop {
            let Ok(first) = self.rx.recv() else { break };
            let mut weight = first.weight();
            let mut urgent = first.urgent();
            let mut batch = vec![first];
            let mut disconnected = false;
            if !urgent {
                let deadline = (last_commit + interval).max(Instant::now());
                while weight < MAX_BATCH_ITEMS {
                    match self.rx.recv_deadline(deadline) {
                        Ok(m) => {
                            weight += m.weight();
                            urgent = m.urgent();
                            batch.push(m);
                            if urgent {
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }
            }
            self.process(batch);
            last_commit = Instant::now();
            if disconnected {
                break;
            }
        }
        if let Err(e) = self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
            tracing::debug!(error = %e, "final WAL checkpoint failed");
        }
        tracing::debug!("galaxy writer stopped");
    }

    fn process(&mut self, batch: Vec<WriterMsg>) {
        let mut flushers = Vec::new();
        let mut checkpoints = Vec::new();
        let mut acks: Vec<(Sender<ImportAck>, ImportAck)> = Vec::new();
        let mut outcome = BatchOutcome::default();
        let started = Instant::now();
        let committed = match self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
        {
            Ok(tx) => {
                for msg in batch {
                    match msg {
                        WriterMsg::Ingest(items) => {
                            for item in items {
                                for it in item.expand() {
                                    self.ctx.shared.ingested.fetch_add(1, Ordering::Relaxed);
                                    self.ctx.apply_logged(&tx, &it, true, &mut outcome);
                                }
                            }
                        }
                        WriterMsg::Import(b) => {
                            let mut local = BatchOutcome::default();
                            for h in &b.hints {
                                if let Err(e) = self.ctx.apply_hint(&tx, h) {
                                    tracing::warn!(error = %e, symbol = h.symbol, "commodity hint failed");
                                    local.errors += 1;
                                }
                            }
                            for op in &b.ops {
                                match op {
                                    ImportOp::Ingest(it) => {
                                        self.ctx.apply_logged(&tx, it, false, &mut local)
                                    }
                                    ImportOp::FactionPresence(f) => {
                                        if let Err(e) = self
                                            .ctx
                                            .savepoint(&tx, |c| c.apply_faction_presence(&tx, f))
                                        {
                                            tracing::warn!(error = %e, "faction presence failed");
                                            local.errors += 1;
                                        }
                                    }
                                }
                            }
                            acks.push((
                                b.done,
                                ImportAck {
                                    stale_markets: local.stale,
                                    errors: local.errors,
                                    committed: false,
                                },
                            ));
                            outcome.errors += local.errors;
                        }
                        WriterMsg::Flush(s) => flushers.push(s),
                        WriterMsg::Checkpoint(s) => checkpoints.push(s),
                    }
                }
                match tx.commit() {
                    Ok(()) => true,
                    Err(e) => {
                        tracing::error!(error = %e, "galaxy database commit failed");
                        false
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "could not start a galaxy database transaction");
                false
            }
        };
        let shared = &self.ctx.shared;
        let mut dirty = std::mem::take(&mut *self.ctx.dirty.borrow_mut());
        let history_added = self.ctx.history_added.take();
        if committed {
            shared.commits.fetch_add(1, Ordering::Relaxed);
            shared.history_rows.fetch_add(history_added, Ordering::Relaxed);
            shared
                .stale_markets
                .fetch_add(outcome.stale, Ordering::Relaxed);
            shared
                .write_errors
                .fetch_add(outcome.errors, Ordering::Relaxed);
            if !dirty.is_empty() {
                dirty.sort_unstable();
                dirty.dedup();
                if let Err(e) = shared.stations.write().refresh(&self.conn, &dirty) {
                    tracing::error!(error = %e, "station index refresh failed; reloading it");
                    match StationIndex::load(&self.conn) {
                        Ok(idx) => *shared.stations.write() = idx,
                        Err(e) => tracing::error!(error = %e, "station index reload failed"),
                    }
                }
            }
            self.ctx.publish(std::mem::take(&mut outcome.updates));
        } else {
            // Anything cached during the failed transaction (new commodity ids) may be gone.
            if let Err(e) = shared.load_commodities(&self.conn) {
                tracing::error!(error = %e, "could not reload commodities");
            }
        }
        let elapsed = started.elapsed();
        if elapsed > Duration::from_secs(2) {
            tracing::debug!(ms = elapsed.as_millis() as u64, "long galaxy transaction");
        }
        if !checkpoints.is_empty()
            && let Err(e) = self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        {
            tracing::warn!(error = %e, "WAL checkpoint failed");
        }
        for f in flushers.into_iter().chain(checkpoints) {
            let _ = f.send(());
        }
        for (done, mut ack) in acks {
            ack.committed = committed;
            let _ = done.send(ack);
        }
        self.ctx.maybe_prune(&mut self.conn);
    }
}

// ---------------------------------------------------------------------------------------------
// Station rows
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct StationRow {
    market_id: i64,
    system_id64: Option<i64>,
    system_name: String,
    system_name_lower: String,
    name: String,
    kind: Option<String>,
    dist_ls: Option<f64>,
    max_pad: i64,
    is_planetary: bool,
    is_carrier: bool,
    is_settlement: bool,
    services: i64,
    mat_trader: i64,
    tech_broker: i64,
    economy: Option<String>,
    second_economy: Option<String>,
    economies: Option<String>,
    faction: Option<String>,
    faction_state: Option<String>,
    government: Option<String>,
    allegiance: Option<String>,
    carrier_access: Option<String>,
    pos: Option<[f64; 3]>,
    updated: i64,
    market_updated: i64,
    n_commodities: i64,
    prohibited: Option<String>,
    /// The market report since which the price history has every change at this market (see the schema).
    history_since: Option<i64>,
}

const STATION_ROW_COLS: &str = "market_id, system_id64, system_name, system_name_lower, name, type, dist_ls, max_pad, \
     is_planetary, is_carrier, is_odyssey_settlement, services, mat_trader, tech_broker, economy, second_economy, \
     economies, faction, faction_state, government, allegiance, carrier_access, x, y, z, updated, market_updated, \
     n_commodities, prohibited, history_since";

fn opt_pos(x: Option<f64>, y: Option<f64>, z: Option<f64>) -> Option<[f64; 3]> {
    Some([x?, y?, z?])
}

impl StationRow {
    fn new(market_id: i64, name: &str, system_name: &str) -> StationRow {
        StationRow {
            market_id,
            system_id64: None,
            system_name: system_name.to_string(),
            system_name_lower: system_name.to_lowercase(),
            name: name.to_string(),
            kind: None,
            dist_ls: None,
            max_pad: 0,
            is_planetary: false,
            is_carrier: false,
            is_settlement: false,
            services: 0,
            mat_trader: 0,
            tech_broker: 0,
            economy: None,
            second_economy: None,
            economies: None,
            faction: None,
            faction_state: None,
            government: None,
            allegiance: None,
            carrier_access: None,
            pos: None,
            updated: 0,
            market_updated: 0,
            n_commodities: 0,
            prohibited: None,
            history_since: None,
        }
    }

    fn load(tx: &Transaction, market_id: i64) -> Result<Option<StationRow>> {
        let sql = format!("SELECT {STATION_ROW_COLS} FROM stations WHERE market_id = ?1");
        let row = tx
            .prepare_cached(&sql)?
            .query_row([market_id], |r| {
                Ok(StationRow {
                    market_id: r.get(0)?,
                    system_id64: r.get(1)?,
                    system_name: r.get(2)?,
                    system_name_lower: r.get(3)?,
                    name: r.get(4)?,
                    kind: r.get(5)?,
                    dist_ls: r.get(6)?,
                    max_pad: r.get(7)?,
                    is_planetary: r.get::<_, i64>(8)? != 0,
                    is_carrier: r.get::<_, i64>(9)? != 0,
                    is_settlement: r.get::<_, i64>(10)? != 0,
                    services: r.get(11)?,
                    mat_trader: r.get(12)?,
                    tech_broker: r.get(13)?,
                    economy: r.get(14)?,
                    second_economy: r.get(15)?,
                    economies: r.get(16)?,
                    faction: r.get(17)?,
                    faction_state: r.get(18)?,
                    government: r.get(19)?,
                    allegiance: r.get(20)?,
                    carrier_access: r.get(21)?,
                    pos: opt_pos(r.get(22)?, r.get(23)?, r.get(24)?),
                    updated: r.get(25)?,
                    market_updated: r.get(26)?,
                    n_commodities: r.get(27)?,
                    prohibited: r.get(28)?,
                    history_since: r.get(29)?,
                })
            })
            .optional()?;
        Ok(row)
    }

    /// Inserts or updates the row and marks it for the in-memory index refresh.
    fn save(
        &self,
        tx: &Transaction,
        previous: Option<&StationRow>,
        dirty: &mut Vec<i64>,
    ) -> Result<()> {
        let (x, y, z) = match self.pos {
            Some([x, y, z]) => (Some(x), Some(y), Some(z)),
            None => (None, None, None),
        };
        let sql = if previous.is_some() {
            "UPDATE stations SET system_id64 = ?2, system_name = ?3, system_name_lower = ?4, name = ?5, type = ?6, \
             dist_ls = ?7, max_pad = ?8, is_planetary = ?9, is_carrier = ?10, is_odyssey_settlement = ?11, \
             services = ?12, mat_trader = ?13, tech_broker = ?14, economy = ?15, second_economy = ?16, \
             economies = ?17, faction = ?18, faction_state = ?19, government = ?20, allegiance = ?21, \
             carrier_access = ?22, x = ?23, y = ?24, z = ?25, updated = ?26, market_updated = ?27, \
             n_commodities = ?28, prohibited = ?29, history_since = ?30 WHERE market_id = ?1"
                .to_string()
        } else {
            format!(
                "INSERT INTO stations ({STATION_ROW_COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, \
                 ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)"
            )
        };
        tx.prepare_cached(&sql)?.execute(params![
            self.market_id,
            self.system_id64,
            self.system_name,
            self.system_name_lower,
            self.name,
            self.kind,
            self.dist_ls,
            self.max_pad,
            self.is_planetary as i64,
            self.is_carrier as i64,
            self.is_settlement as i64,
            self.services,
            self.mat_trader,
            self.tech_broker,
            self.economy,
            self.second_economy,
            self.economies,
            self.faction,
            self.faction_state,
            self.government,
            self.allegiance,
            self.carrier_access,
            x,
            y,
            z,
            self.updated,
            self.market_updated,
            self.n_commodities,
            self.prohibited,
            self.history_since,
        ])?;
        dirty.push(self.market_id);
        Ok(())
    }

    fn apply_type(&mut self, raw: &str) {
        let Some(kind) = StationKind::parse(raw) else {
            return;
        };
        self.kind = StationKind::display_for(raw);
        if kind != StationKind::Other {
            self.is_carrier = kind.is_carrier();
            self.is_planetary = kind.is_planetary();
            self.is_settlement = kind.is_settlement();
        }
        if self.max_pad == 0
            && let Some(p) = kind.default_max_pad()
        {
            self.max_pad = p.code();
        }
    }

    /// `economies` sorted highest proportion first.
    fn set_economies(&mut self, economies: &[(String, f64)], primary: Option<&str>) {
        if let Some(p) = primary.or_else(|| economies.first().map(|(n, _)| n.as_str())) {
            self.economy = Some(p.to_string());
            self.second_economy = economies
                .iter()
                .map(|(n, _)| n)
                .find(|n| n.as_str() != p)
                .cloned();
        }
        if !economies.is_empty() {
            let map: serde_json::Map<String, serde_json::Value> = economies
                .iter()
                .map(|(n, p)| (n.clone(), serde_json::json!(p)))
                .collect();
            self.economies = Some(serde_json::Value::Object(map).to_string());
        }
    }

    fn recompute_traders(&mut self) {
        let svc = Services(self.services as u64);
        self.mat_trader = if svc.contains(Services::MATERIAL_TRADER) {
            MaterialTrader::infer(self.economy.as_deref(), self.second_economy.as_deref())
                .map_or(0, MaterialTrader::code)
        } else {
            0
        };
        self.tech_broker = if svc.contains(Services::TECH_BROKER) {
            TechBroker::infer(self.economy.as_deref(), self.second_economy.as_deref()).code()
        } else {
            0
        };
    }

    fn set_system(&mut self, id: Option<i64>, name: &str, pos: Option<[f64; 3]>) {
        self.system_id64 = id;
        if !name.is_empty() {
            self.system_name = name.to_string();
            self.system_name_lower = name.to_lowercase();
        }
        self.pos = pos;
    }
}

fn system_rtree_set(tx: &Transaction, id64: i64, pos: [f64; 3]) -> Result<()> {
    tx.prepare_cached(
        "INSERT OR REPLACE INTO systems_rtree (id, min_x, max_x, min_y, max_y, min_z, max_z) \
         VALUES (?1, ?2, ?2, ?3, ?3, ?4, ?4)",
    )?
    .execute(params![id64, pos[0], pos[1], pos[2]])?;
    Ok(())
}

/// (id64, name, position) of a stored system.
type SystemRef = (i64, String, Option<[f64; 3]>);

/// Most plausible system for a name: one with a known position, most recently updated.
fn resolve_system(tx: &Transaction, name_lower: &str) -> Result<Option<SystemRef>> {
    let row = tx
        .prepare_cached(
            "SELECT id64, name, x, y, z FROM systems WHERE name_lower = ?1 ORDER BY (x IS NULL), updated DESC LIMIT 1",
        )?
        .query_row([name_lower], |r| Ok((r.get(0)?, r.get(1)?, opt_pos(r.get(2)?, r.get(3)?, r.get(4)?))))
        .optional()?;
    Ok(row)
}

/// Name and position of a system; creates a position-less placeholder when it is unknown and a name is given.
fn ensure_system(
    tx: &Transaction,
    id64: i64,
    name: Option<&str>,
    dirty: &mut Vec<i64>,
) -> Result<(String, Option<[f64; 3]>)> {
    let row: Option<(String, Option<[f64; 3]>)> = tx
        .prepare_cached("SELECT name, x, y, z FROM systems WHERE id64 = ?1")?
        .query_row([id64], |r| {
            Ok((r.get(0)?, opt_pos(r.get(1)?, r.get(2)?, r.get(3)?)))
        })
        .optional()?;
    if let Some(r) = row {
        return Ok(r);
    }
    let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
        return Ok((String::new(), None));
    };
    let lower = name.to_lowercase();
    tx.prepare_cached(
        "INSERT INTO systems (id64, name, name_lower, updated) VALUES (?1, ?2, ?3, 0)",
    )?
    .execute(params![id64, name, lower])?;
    link_unlinked_stations(tx, id64, &lower, None, dirty)?;
    Ok((name.to_string(), None))
}

/// Stations that were waiting for this system (market data arrived before the system) get linked.
fn link_unlinked_stations(
    tx: &Transaction,
    id64: i64,
    name_lower: &str,
    pos: Option<[f64; 3]>,
    dirty: &mut Vec<i64>,
) -> Result<()> {
    let ids: Vec<i64> = tx
        .prepare_cached(
            "SELECT market_id FROM stations WHERE system_id64 IS NULL AND system_name_lower = ?1",
        )?
        .query_map([name_lower], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if ids.is_empty() {
        return Ok(());
    }
    let (x, y, z) = split_pos(pos);
    tx.prepare_cached(
        "UPDATE stations SET system_id64 = ?1, x = ?2, y = ?3, z = ?4 WHERE system_id64 IS NULL AND system_name_lower = ?5",
    )?
    .execute(params![id64, x, y, z, name_lower])?;
    dirty.extend(ids);
    Ok(())
}

/// Propagates a system's (new) position and name to its stations.
fn refresh_linked_stations(
    tx: &Transaction,
    id64: i64,
    name: &str,
    pos: Option<[f64; 3]>,
    dirty: &mut Vec<i64>,
) -> Result<()> {
    let ids: Vec<i64> = tx
        .prepare_cached("SELECT market_id FROM stations WHERE system_id64 = ?1")?
        .query_map([id64], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if ids.is_empty() {
        return Ok(());
    }
    let (x, y, z) = split_pos(pos);
    tx.prepare_cached(
        "UPDATE stations SET x = ?2, y = ?3, z = ?4, system_name = ?5, system_name_lower = ?6 WHERE system_id64 = ?1",
    )?
    .execute(params![id64, x, y, z, name, name.to_lowercase()])?;
    dirty.extend(ids);
    Ok(())
}

fn split_pos(pos: Option<[f64; 3]>) -> (Option<f64>, Option<f64>, Option<f64>) {
    match pos {
        Some([x, y, z]) => (Some(x), Some(y), Some(z)),
        None => (None, None, None),
    }
}

fn json_list(v: &[String]) -> Option<String> {
    (!v.is_empty()).then(|| serde_json::to_string(v).unwrap_or_default())
}

fn price_changed(old: i64, new: i64, min_rel: f64) -> bool {
    if old == new {
        return false;
    }
    if min_rel <= 0.0 {
        return true;
    }
    let base = old.max(new).max(1) as f64;
    ((new - old).abs() as f64 / base) >= min_rel
}

// ---------------------------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------------------------

impl Ctx {
    /// Applies one item inside a savepoint, so a failing statement can't leave it half-applied.
    fn apply_logged(&self, tx: &Transaction, item: &Ingest, live: bool, out: &mut BatchOutcome) {
        let res = self.savepoint(tx, |ctx| ctx.apply_item(tx, item, live, out));
        if let Err(e) = res {
            out.errors += 1;
            tracing::warn!(error = %e, "galaxy ingest failed");
        }
    }

    fn savepoint(&self, tx: &Transaction, f: impl FnOnce(&Ctx) -> Result<()>) -> Result<()> {
        tx.prepare_cached("SAVEPOINT item")?.execute([])?;
        let history_before = self.history_added.get();
        match f(self) {
            Ok(()) => {
                tx.prepare_cached("RELEASE item")?.execute([])?;
                Ok(())
            }
            Err(e) => {
                tx.execute_batch("ROLLBACK TO item; RELEASE item;")?;
                self.history_added.set(history_before);
                // a commodity created inside the rolled-back part is gone again
                self.shared.load_commodities(tx)?;
                Err(e)
            }
        }
    }

    fn apply_item(
        &self,
        tx: &Transaction,
        item: &Ingest,
        live: bool,
        out: &mut BatchOutcome,
    ) -> Result<()> {
        match item {
            Ingest::Market(m) => match self.apply_market(tx, m, live) {
                Ok(MarketResult::Stored(u)) => {
                    self.shared.market_snapshots.fetch_add(1, Ordering::Relaxed);
                    if live {
                        out.updates.push(u);
                    }
                    Ok(())
                }
                Ok(MarketResult::Stale) => {
                    out.stale += 1;
                    Ok(())
                }
                Ok(MarketResult::Ignored) => Ok(()),
                Err(e) => Err(e),
            },
            Ingest::System(s) => self.apply_system(tx, s),
            Ingest::Station(s) => self.apply_station(tx, s),
            Ingest::RingSignals(r) => self.apply_rings(tx, r),
            Ingest::Body(b) => self.apply_body(tx, b),
            Ingest::Raw(_) => Ok(()), // expanded before we get here
        }
    }

    /// Merges one observation of a planet or moon into its row (created only for what
    /// [`bodies::admits`] keeps); writes only when something changed.
    fn apply_body(&self, tx: &Transaction, u: &BodyUpdate) -> Result<()> {
        if u.system_id64 <= 1 || u.system_id64 > i64::MAX as u64 {
            return Ok(());
        }
        let existing = bodies::load(tx, u.system_id64, u.body_id)?;
        let mut row = match &existing {
            Some(r) => r.clone(),
            None if bodies::admits(u, self.opts.all_eddn_bodies) => BodyInfo {
                system_id64: u.system_id64,
                body_id: u.body_id,
                ..BodyInfo::default()
            },
            None => return Ok(()),
        };
        bodies::merge(&mut row, u);
        if existing.as_ref() != Some(&row) {
            bodies::save(tx, &row)?;
        }
        Ok(())
    }

    fn commodity_id(&self, tx: &Transaction, symbol: &str) -> Result<i64> {
        if let Some(id) = self.shared.commodities.read().id_by_symbol(symbol) {
            return Ok(id);
        }
        self.insert_commodity(tx, symbol, &fallback_name(symbol), "", None)
    }

    fn insert_commodity(
        &self,
        tx: &Transaction,
        symbol: &str,
        name: &str,
        category: &str,
        fdev_id: Option<i64>,
    ) -> Result<i64> {
        tx.prepare_cached("INSERT OR IGNORE INTO commodities (symbol, name, category, fdev_id) VALUES (?1, ?2, ?3, ?4)")?
            .execute(params![symbol, name, category, fdev_id])?;
        let id: i64 = tx
            .prepare_cached("SELECT id FROM commodities WHERE symbol = ?1")?
            .query_row([symbol], |r| r.get(0))?;
        tracing::debug!(symbol, name, id, "new commodity");
        self.shared.commodities.write().insert(
            id,
            CommodityMeta {
                symbol: symbol.to_string(),
                name: name.to_string(),
                category: category.to_string(),
                rare: false,
            },
        );
        Ok(id)
    }

    fn apply_hint(&self, tx: &Transaction, h: &CommodityHint) -> Result<()> {
        let symbol = normalize_symbol(&h.symbol);
        if symbol.is_empty()
            || self
                .shared
                .commodities
                .read()
                .id_by_symbol(&symbol)
                .is_some()
        {
            return Ok(());
        }
        let name = if h.name.trim().is_empty() {
            fallback_name(&symbol)
        } else {
            h.name.trim().to_string()
        };
        self.insert_commodity(tx, &symbol, &name, h.category.trim(), h.fdev_id)?;
        Ok(())
    }

    fn apply_market(
        &self,
        tx: &Transaction,
        m: &MarketSnapshot,
        live: bool,
    ) -> Result<MarketResult> {
        if m.market_id == 0 || m.market_id > i64::MAX as u64 || m.station_name.trim().is_empty() {
            return Ok(MarketResult::Ignored);
        }
        let mid = m.market_id as i64;
        let existing = StationRow::load(tx, mid)?;
        if let Some(e) = &existing
            && e.market_updated > 0
            && e.market_updated >= m.timestamp
        {
            return Ok(MarketResult::Stale);
        }
        let name = clean_station_name(&m.station_name);
        let mut row = existing
            .clone()
            .unwrap_or_else(|| StationRow::new(mid, &name, &m.system_name));
        row.name = name;
        let sys_lower = m.system_name.trim().to_lowercase();
        let same_system = existing
            .as_ref()
            .is_some_and(|e| e.system_id64.is_some() && e.system_name_lower == sys_lower);
        if !same_system && !sys_lower.is_empty() {
            match resolve_system(tx, &sys_lower)? {
                Some((id, name, pos)) => row.set_system(Some(id), &name, pos),
                None => row.set_system(None, m.system_name.trim(), None),
            }
        }
        if let Some(t) = &m.station_type {
            row.apply_type(t);
        }
        if let Some(a) = &m.carrier_docking_access {
            row.carrier_access = Some(a.clone());
        }
        if let Some(p) = &m.prohibited {
            row.prohibited = Some(serde_json::to_string(p).unwrap_or_default());
        }
        if let Some(e) = &m.economies
            && !e.is_empty()
        {
            row.set_economies(e, None);
        }
        row.market_updated = m.timestamp;

        // Previous prices, for the history table.
        let mut old: FxHashMap<i64, (i64, i64, i64)> = FxHashMap::default();
        let history = live
            && self.opts.price_history
            && existing.as_ref().is_some_and(|e| e.n_commodities > 0);
        // Since when the history has every change here: compared with the last report, it goes on from where it
        // began (or, for a market stored before this was kept, from that last report); a market seen for the first
        // time, or replaced without comparing (an import), starts afresh with this report.
        row.history_since = match &existing {
            Some(e) if history => e.history_since.or(Some(e.market_updated)),
            _ if self.opts.price_history => Some(m.timestamp),
            _ => None,
        };
        if history {
            let mut stmt = tx.prepare_cached(
                "SELECT commodity_id, buy, sell, updated FROM market WHERE market_id = ?1",
            )?;
            let rows = stmt.query_map([mid], |r| {
                Ok((r.get::<_, i64>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
            })?;
            for r in rows {
                let (cid, v) = r?;
                old.insert(cid, v);
            }
        }

        tx.prepare_cached("DELETE FROM market WHERE market_id = ?1")?
            .execute([mid])?;
        let mut n = 0i64;
        {
            let mut ins = tx.prepare_cached(
                "INSERT OR REPLACE INTO market (market_id, commodity_id, buy, sell, mean, stock, stock_bracket, demand, \
                 demand_bracket, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            let mut hist_old = tx.prepare_cached(
                "INSERT OR IGNORE INTO price_history (market_id, commodity_id, ts, buy, sell) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            // A new point, or the prices of one already stored for that snapshot (not counted as a new row).
            let mut hist_new = tx.prepare_cached(
                "INSERT OR IGNORE INTO price_history (market_id, commodity_id, ts, buy, sell) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut hist_set = tx.prepare_cached(
                "UPDATE price_history SET buy = ?4, sell = ?5 WHERE market_id = ?1 AND commodity_id = ?2 AND ts = ?3",
            )?;
            let mut seen = rustc_hash::FxHashSet::default();
            for it in &m.items {
                if it.symbol.is_empty() {
                    continue;
                }
                let cid = self.commodity_id(tx, &it.symbol)?;
                if !seen.insert(cid) {
                    continue; // duplicate entry in one snapshot: first one wins
                }
                ins.execute(params![
                    mid,
                    cid,
                    it.buy,
                    it.sell,
                    it.mean,
                    it.stock,
                    it.stock_bracket.map(i64::from),
                    crate::schema::real_demand(it.demand, it.demand_bracket),
                    it.demand_bracket.map(i64::from),
                    m.timestamp
                ])?;
                n += 1;
                // A change: the price before it (at its report) and this one. New on a market reported before (a
                // carrier's new order, say): its last report, without it (0, 0), and this one.
                let before = match old.get(&cid) {
                    Some(&(ob, os, ou)) => {
                        let min = self.opts.price_history_min_change;
                        (price_changed(ob, it.buy, min) || price_changed(os, it.sell, min)).then_some((ou, ob, os))
                    }
                    None => (history && (it.buy > 0 || it.sell > 0)).then(|| (existing.as_ref().map_or(0, |e| e.market_updated), 0, 0)),
                };
                if let Some((ou, ob, os)) = before {
                    let mut rows = hist_old.execute(params![mid, cid, ou, ob, os])?;
                    let point = params![mid, cid, m.timestamp, it.buy, it.sell];
                    match hist_new.execute(point)? {
                        0 => _ = hist_set.execute(point)?,
                        n => rows += n,
                    }
                    self.history_added.set(self.history_added.get() + rows as i64);
                }
            }
        }
        row.n_commodities = n;
        row.recompute_traders();
        row.save(tx, existing.as_ref(), &mut self.dirty.borrow_mut())?;
        Ok(MarketResult::Stored(MarketUpdate {
            market_id: m.market_id,
            station: row.name.clone(),
            system: row.system_name.clone(),
            system_id64: row.system_id64.map(|i| i as u64),
            pos: row.pos,
            commodities: n as usize,
            updated_unix: m.timestamp,
            is_carrier: row.is_carrier,
            source: m.source,
        }))
    }

    fn apply_system(&self, tx: &Transaction, s: &SystemUpdate) -> Result<()> {
        let name = s.name.trim();
        if s.id64 <= 1 || s.id64 > i64::MAX as u64 || name.is_empty() {
            return Ok(());
        }
        let id = s.id64 as i64;
        let lower = name.to_lowercase();
        let existing: Option<(String, Option<[f64; 3]>, i64)> = tx
            .prepare_cached("SELECT name, x, y, z, updated FROM systems WHERE id64 = ?1")?
            .query_row([id], |r| {
                Ok((
                    r.get(0)?,
                    opt_pos(r.get(1)?, r.get(2)?, r.get(3)?),
                    r.get(4)?,
                ))
            })
            .optional()?;
        let details_newer = match &existing {
            None => true,
            Some((_, _, updated)) => s.timestamp >= *updated,
        };
        match &existing {
            None => {
                let (x, y, z) = split_pos(s.pos);
                tx.prepare_cached("INSERT INTO systems (id64, name, name_lower, x, y, z, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)")?
                    .execute(params![id, name, lower, x, y, z])?;
                if let Some(p) = s.pos {
                    system_rtree_set(tx, id, p)?;
                }
                link_unlinked_stations(tx, id, &lower, s.pos, &mut self.dirty.borrow_mut())?;
                refresh_linked_stations(tx, id, name, s.pos, &mut self.dirty.borrow_mut())?;
            }
            Some((old_name, old_pos, _)) => {
                let new_pos = s.pos.or(*old_pos);
                let pos_changed = new_pos != *old_pos;
                if old_name != name || pos_changed {
                    let (x, y, z) = split_pos(new_pos);
                    tx.prepare_cached("UPDATE systems SET name = ?2, name_lower = ?3, x = ?4, y = ?5, z = ?6 WHERE id64 = ?1")?
                        .execute(params![id, name, lower, x, y, z])?;
                    if let Some(p) = new_pos
                        && pos_changed
                    {
                        system_rtree_set(tx, id, p)?;
                    }
                    if old_name.to_lowercase() != lower {
                        link_unlinked_stations(
                            tx,
                            id,
                            &lower,
                            new_pos,
                            &mut self.dirty.borrow_mut(),
                        )?;
                    }
                    refresh_linked_stations(tx, id, name, new_pos, &mut self.dirty.borrow_mut())?;
                }
            }
        }
        if let Some(d) = &s.details
            && details_newer
        {
            tx.prepare_cached(
                "UPDATE systems SET population = ?2, allegiance = ?3, government = ?4, economy = ?5, second_economy = ?6, \
                 security = ?7, controlling_faction = ?8, controlling_faction_state = ?9, controlling_power = ?10, \
                 powerplay_state = ?11, powers = ?12, updated = ?13 WHERE id64 = ?1",
            )?
            .execute(params![
                id,
                d.population.map(|p| p.min(i64::MAX as u64) as i64),
                d.allegiance,
                d.government,
                d.economy,
                d.second_economy,
                d.security,
                d.controlling_faction,
                d.controlling_faction_state,
                d.controlling_power,
                d.powerplay_state,
                json_list(&d.powers),
                s.timestamp
            ])?;
            if let Some(factions) = &d.factions {
                tx.prepare_cached("DELETE FROM system_factions WHERE system_id64 = ?1")?
                    .execute([id])?;
                let mut ins = tx.prepare_cached(
                    "INSERT OR REPLACE INTO system_factions (system_id64, faction, influence, state, government, allegiance, \
                     happiness, active_states, pending_states, recovering_states, updated) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                )?;
                for f in factions {
                    if f.name.trim().is_empty() {
                        continue;
                    }
                    ins.execute(params![
                        id,
                        f.name.trim(),
                        f.influence,
                        f.state,
                        f.government,
                        f.allegiance,
                        f.happiness,
                        json_list(&f.active_states),
                        json_list(&f.pending_states),
                        json_list(&f.recovering_states),
                        s.timestamp
                    ])?;
                }
            }
        }
        Ok(())
    }

    fn apply_station(&self, tx: &Transaction, st: &StationUpdate) -> Result<()> {
        let name = clean_station_name(&st.name);
        let name = name.as_str();
        if st.market_id == 0 || st.market_id > i64::MAX as u64 || name.is_empty() {
            return Ok(());
        }
        let mid = st.market_id as i64;
        let existing = StationRow::load(tx, mid)?;
        if let Some(e) = &existing
            && st.timestamp < e.updated
        {
            return Ok(()); // older than what we have
        }
        let mut row = existing.clone().unwrap_or_else(|| {
            StationRow::new(mid, name, st.system_name.as_deref().unwrap_or_default())
        });
        row.name = name.to_string();
        if let Some(id) = st.system_id64.filter(|i| *i > 1 && *i <= i64::MAX as u64) {
            let (sys_name, pos) = ensure_system(
                tx,
                id as i64,
                st.system_name.as_deref(),
                &mut self.dirty.borrow_mut(),
            )?;
            let name = if sys_name.is_empty() {
                st.system_name.clone().unwrap_or_default()
            } else {
                sys_name
            };
            row.set_system(Some(id as i64), &name, pos);
        } else if let Some(sys) = st
            .system_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let lower = sys.to_lowercase();
            if row.system_id64.is_none() || row.system_name_lower != lower {
                match resolve_system(tx, &lower)? {
                    Some((id, n, pos)) => row.set_system(Some(id), &n, pos),
                    None => row.set_system(None, sys, None),
                }
            }
        }
        if let Some(t) = &st.station_type {
            row.apply_type(t);
        }
        if st.planetary == Some(true) {
            row.is_planetary = true;
        }
        if let Some(d) = st.dist_ls {
            row.dist_ls = Some(d);
        }
        if let Some(p) = st.landing_pads
            && let Some(pad) = Pad::from_counts(p.small, p.medium, p.large)
        {
            row.max_pad = pad.code();
        }
        if let Some(s) = st.services {
            row.services = s.bits() as i64;
        }
        match (&st.economies, &st.primary_economy) {
            (Some(e), p) if !e.is_empty() => row.set_economies(e, p.as_deref()),
            (_, Some(p)) => {
                if row.economy.as_deref() != Some(p.as_str()) {
                    row.second_economy = None;
                }
                row.economy = Some(p.clone());
            }
            _ => {}
        }
        if st.faction.is_some() {
            row.faction.clone_from(&st.faction);
            row.faction_state.clone_from(&st.faction_state);
        }
        if st.government.is_some() {
            row.government.clone_from(&st.government);
        }
        if st.allegiance.is_some() {
            row.allegiance.clone_from(&st.allegiance);
        }
        if st.carrier_docking_access.is_some() {
            row.carrier_access.clone_from(&st.carrier_docking_access);
        }
        row.updated = row.updated.max(st.timestamp);
        row.recompute_traders();
        if existing.as_ref() != Some(&row) {
            row.save(tx, existing.as_ref(), &mut self.dirty.borrow_mut())?;
        }
        Ok(())
    }

    fn resolve_signal(&self, raw: &str) -> String {
        let cache = self.shared.commodities.read();
        cache
            .resolve(raw)
            .and_then(|id| cache.meta(id))
            .map(|m| m.symbol.clone())
            .unwrap_or_else(|| normalize_symbol(raw))
    }

    fn apply_rings(&self, tx: &Transaction, r: &RingSignalsUpdate) -> Result<()> {
        let body = r.body_name.trim();
        if r.system_id64 <= 1 || r.system_id64 > i64::MAX as u64 || body.is_empty() {
            return Ok(());
        }
        let id = r.system_id64 as i64;
        ensure_system(
            tx,
            id,
            r.system_name.as_deref(),
            &mut self.dirty.borrow_mut(),
        )?;
        tx.prepare_cached("DELETE FROM ring_signals WHERE system_id64 = ?1 AND body_name = ?2")?
            .execute(params![id, body])?;
        let mut ins = tx.prepare_cached(
            "INSERT OR REPLACE INTO ring_signals (system_id64, body_name, signal, count, ring_type, updated) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for (sig, count) in &r.signals {
            let sym = self.resolve_signal(sig);
            if sym.is_empty() {
                continue;
            }
            ins.execute(params![id, body, sym, count, r.ring_type, r.timestamp])?;
        }
        Ok(())
    }

    fn apply_faction_presence(&self, tx: &Transaction, f: &FactionPresence) -> Result<()> {
        if f.system_id64 <= 1 || f.system_id64 > i64::MAX as u64 || f.faction.trim().is_empty() {
            return Ok(());
        }
        self.apply_system(
            tx,
            &SystemUpdate {
                id64: f.system_id64,
                name: f.system_name.clone(),
                pos: f.pos,
                timestamp: f.timestamp,
                details: None,
            },
        )?;
        let id = f.system_id64 as i64;
        tx.prepare_cached(
            "INSERT OR IGNORE INTO system_factions (system_id64, faction, government, allegiance, updated) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?
        .execute(params![id, f.faction.trim(), f.government, f.allegiance, f.timestamp])?;
        if f.controlling {
            tx.prepare_cached("UPDATE systems SET controlling_faction = ?2 WHERE id64 = ?1 AND controlling_faction IS NULL")?
                .execute(params![id, f.faction.trim()])?;
        }
        Ok(())
    }

    fn publish(&self, updates: Vec<MarketUpdate>) {
        if updates.is_empty() {
            return;
        }
        let mut subs = self.shared.subscribers.lock();
        subs.retain(|s| {
            for u in &updates {
                match s.try_send(u.clone()) {
                    Ok(()) | Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => return false,
                }
            }
            true
        });
    }

    fn maybe_prune(&mut self, conn: &mut Connection) {
        if self.last_prune.is_some_and(|t| t.elapsed() < PRUNE_EVERY) {
            return;
        }
        self.last_prune = Some(Instant::now());
        let cutoff = crate::util::now_unix() - self.opts.price_history_days.max(1) * 86_400;
        // In batches, each its own transaction: after a long break the first prune removes days of live prices, and
        // one statement would hold the writer (and grow the WAL) until it's done.
        let mut pruned = 0;
        loop {
            match conn.execute(
                "DELETE FROM price_history WHERE (market_id, commodity_id, ts) IN \
                 (SELECT market_id, commodity_id, ts FROM price_history WHERE ts < ?1 LIMIT 5000)",
                [cutoff],
            ) {
                Ok(0) => break,
                Ok(n) => {
                    pruned += n;
                    self.shared.history_rows.fetch_sub(n as i64, Ordering::Relaxed);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "price history pruning failed");
                    break;
                }
            }
        }
        if pruned > 0 {
            tracing::info!(rows = pruned, "pruned old price history");
        }
    }
}

enum MarketResult {
    Stored(MarketUpdate),
    Stale,
    Ignored,
}
