//! Read connections and the query implementations (run on the caller's thread).

use crate::commodities::CommodityCache;
use crate::error::Result;
use crate::index::{StationFilter, StationIndex};
use crate::model::*;
use crate::services::{MaterialTrader, Services, TechBroker};
use crate::station::Pad;
use crate::util::{dist, now_unix};
use parking_lot::{Mutex, RwLock};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::ops::Deref;
use std::path::PathBuf;

/// Upper bound for "search outward" radii (the galaxy is ~100 000 ly across).
pub(crate) const MAX_RADIUS: f64 = 100_000.0;

// ---------------------------------------------------------------------------------------------
// Pool
// ---------------------------------------------------------------------------------------------

pub(crate) struct ReadPool {
    path: PathBuf,
    idle: Mutex<Vec<Connection>>,
    max_idle: usize,
}

pub(crate) struct PooledConn<'a> {
    pool: &'a ReadPool,
    conn: Option<Connection>,
}

impl Deref for PooledConn<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        // `conn` is only taken in `drop`.
        self.conn
            .as_ref()
            .expect("pooled connection present until drop")
    }
}

impl Drop for PooledConn<'_> {
    fn drop(&mut self) {
        if let Some(c) = self.conn.take() {
            let mut idle = self.pool.idle.lock();
            if idle.len() < self.pool.max_idle {
                idle.push(c);
            }
        }
    }
}

impl ReadPool {
    pub fn new(path: PathBuf, max_idle: usize) -> ReadPool {
        ReadPool {
            path,
            idle: Mutex::new(Vec::new()),
            max_idle: max_idle.max(1),
        }
    }

    pub fn get(&self) -> Result<PooledConn<'_>> {
        let conn = match self.idle.lock().pop() {
            Some(c) => c,
            None => crate::schema::open_reader(&self.path)?,
        };
        Ok(PooledConn {
            pool: self,
            conn: Some(conn),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------------------------

pub(crate) const STATION_COLS: &str = "s.market_id, s.name, s.type, s.system_name, s.system_id64, s.x, s.y, s.z, \
     s.dist_ls, s.max_pad, s.is_carrier, s.is_planetary, s.is_odyssey_settlement, s.services, s.mat_trader, \
     s.tech_broker, s.economy, s.second_economy, s.faction, s.faction_state, s.government, s.allegiance, \
     s.carrier_access, s.updated, s.market_updated, s.n_commodities";
pub(crate) const STATION_NCOLS: usize = 26;

pub(crate) fn opt_pos(x: Option<f64>, y: Option<f64>, z: Option<f64>) -> Option<[f64; 3]> {
    Some([x?, y?, z?])
}

pub(crate) fn station_info(r: &Row, o: usize) -> rusqlite::Result<StationInfo> {
    Ok(StationInfo {
        market_id: r.get::<_, i64>(o)? as u64,
        name: r.get(o + 1)?,
        station_type: r.get(o + 2)?,
        system: r.get(o + 3)?,
        system_id64: r.get::<_, Option<i64>>(o + 4)?.map(|v| v as u64),
        pos: opt_pos(r.get(o + 5)?, r.get(o + 6)?, r.get(o + 7)?),
        dist_ls: r.get(o + 8)?,
        max_pad: Pad::from_code(r.get(o + 9)?).map(Pad::as_char),
        is_carrier: r.get::<_, i64>(o + 10)? != 0,
        is_planetary: r.get::<_, i64>(o + 11)? != 0,
        is_odyssey_settlement: r.get::<_, i64>(o + 12)? != 0,
        services: Services(r.get::<_, i64>(o + 13)? as u64),
        material_trader: MaterialTrader::from_code(r.get(o + 14)?),
        tech_broker: TechBroker::from_code(r.get(o + 15)?),
        economy: r.get(o + 16)?,
        second_economy: r.get(o + 17)?,
        faction: r.get(o + 18)?,
        faction_state: r.get(o + 19)?,
        government: r.get(o + 20)?,
        allegiance: r.get(o + 21)?,
        carrier_docking_access: r.get(o + 22)?,
        updated_unix: r.get(o + 23)?,
        market_updated_unix: r.get(o + 24)?,
        commodity_count: r.get::<_, i64>(o + 25)?.clamp(0, i64::from(u32::MAX)) as u32,
    })
}

const SYSTEM_COLS: &str = "sy.id64, sy.name, sy.x, sy.y, sy.z, sy.population, sy.allegiance, sy.government, \
     sy.economy, sy.second_economy, sy.security, sy.controlling_faction, sy.controlling_faction_state, \
     sy.controlling_power, sy.powerplay_state, sy.powers, sy.updated";

fn json_strings(s: Option<String>) -> Vec<String> {
    s.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn system_info(r: &Row) -> rusqlite::Result<SystemInfo> {
    Ok(SystemInfo {
        id64: r.get::<_, i64>(0)? as u64,
        name: r.get(1)?,
        pos: opt_pos(r.get(2)?, r.get(3)?, r.get(4)?),
        population: r.get::<_, Option<i64>>(5)?.map(|p| p.max(0) as u64),
        allegiance: r.get(6)?,
        government: r.get(7)?,
        economy: r.get(8)?,
        second_economy: r.get(9)?,
        security: r.get(10)?,
        controlling_faction: r.get(11)?,
        controlling_faction_state: r.get(12)?,
        controlling_power: r.get(13)?,
        powerplay_state: r.get(14)?,
        powers: json_strings(r.get(15)?),
        updated_unix: r.get(16)?,
    })
}

/// A fleet carrier's docking access in the journal's spelling (`all`, `none`, `friends`, `squadron`,
/// `squadronfriends`), however its source wrote it (Spansh: `Squadron Friends`); `None` for a station, or a carrier
/// whose access was never reported.
pub(crate) fn carrier_access(is_carrier: bool, raw: Option<String>) -> Option<String> {
    let a = raw?.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    (is_carrier && !a.is_empty()).then_some(a)
}

/// SQL bounding box parameters for a sphere.
fn bbox(o: [f64; 3], r: f64) -> [f64; 6] {
    [o[0] - r, o[0] + r, o[1] - r, o[1] + r, o[2] - r, o[2] + r]
}

/// Pad filter code: 0 = no filter ('S' or none: every station has a small pad).
fn pad_code(c: Option<char>) -> i64 {
    match c.and_then(Pad::from_char) {
        Some(Pad::Medium) => 2,
        Some(Pad::Large) => 3,
        _ => 0,
    }
}

fn age_cutoff(hours: Option<f64>) -> i64 {
    match hours {
        Some(h) if h.is_finite() && h > 0.0 => now_unix() - (h * 3600.0) as i64,
        _ => i64::MIN,
    }
}

// ---------------------------------------------------------------------------------------------
// Commodity search
// ---------------------------------------------------------------------------------------------

/// Which market rows a commodity search reads.
#[derive(Clone, Copy)]
pub(crate) enum Goods<'a> {
    /// One commodity, by id: its range of the covering partial indexes.
    One(i64),
    /// Rare goods as one: those of one category, or (`None`) all of them, each rare good's range of the same indexes.
    Rare(Option<&'a str>),
}

impl Goods<'_> {
    /// The first SQL parameter: the commodity id, or the category (NULL: every category).
    fn key(self) -> rusqlite::types::Value {
        match self {
            Goods::One(cid) => rusqlite::types::Value::Integer(cid),
            Goods::Rare(category) => category.map_or(rusqlite::types::Value::Null, |c| rusqlite::types::Value::Text(c.to_string())),
        }
    }

    /// The query for the rows of markets selling these goods (with stock) or buying them (with demand).
    fn rows(self, selling: bool) -> &'static str {
        match (self, selling) {
            (Goods::One(_), true) => SELLER_ROWS,
            (Goods::One(_), false) => BUYER_ROWS,
            (Goods::Rare(_), true) => RARE_SELLER_ROWS,
            (Goods::Rare(_), false) => RARE_BUYER_ROWS,
        }
    }

    /// A row's commodity id (a rare goods row's is its query's column 4).
    fn cid(self, r: &Row) -> rusqlite::Result<i64> {
        match self {
            Goods::One(cid) => Ok(cid),
            Goods::Rare(_) => r.get(4),
        }
    }

    /// A rare goods row's commodity (its query's columns 5 and 6), its average still 0: that is looked up for the
    /// rows kept (`RowAverages`). `None` for one commodity's rows.
    fn commodity(self, r: &Row) -> rusqlite::Result<Option<RowCommodity>> {
        match self {
            Goods::One(_) => Ok(None),
            Goods::Rare(_) => Ok(Some(RowCommodity { symbol: r.get(5)?, name: r.get(6)?, mean: 0 })),
        }
    }
}

/// Candidate row of a commodity search (before the station details are fetched).
struct Cand {
    market_id: i64,
    cid: i64,
    price: i64,
    qty: i64,
    updated: i64,
    dist: Option<f64>,
    commodity: Option<RowCommodity>,
}

/// A commodity search's query: the rows of markets selling (or buying), best price first.
fn search_sql(goods: Goods, selling: bool) -> String {
    format!("{} ORDER BY {}", goods.rows(selling), if selling { "buy ASC" } else { "sell DESC" })
}

/// Commodity search: scans the commodity's range of the covering partial index (`market_sellers` /
/// `market_buyers`, contiguous pages, best price first) and filters it against the in-memory station
/// index; only the final hits are looked up in `stations`.
pub(crate) fn search_commodity(
    conn: &Connection,
    index: &RwLock<StationIndex>,
    goods: Goods,
    q: &CommodityQuery,
) -> Result<Vec<CommodityHit>> {
    let limit = q.limit.clamp(1, 100_000);
    let filter = StationFilter {
        min_pad: pad_code(q.min_pad) as u8,
        carriers: q.include_carriers,
        planetary: q.include_planetary,
        max_dist_ls: -1.0,
    };
    let max_d = q.max_distance_ly.filter(|d| d.is_finite() && *d >= 0.0);
    let sql = search_sql(goods, q.mode == TradeMode::BuyFromStation);
    // Rows arrive best price first: for that order we can stop once `limit` rows passed the filters
    // (and the price changes, so that equal prices are all collected for the distance tie-break).
    let early_stop = q.sort == Sort::BestPrice;
    let mut cands: Vec<Cand> = Vec::new();
    {
        let mut stmt = conn.prepare_cached(&sql)?;
        let mut rows = stmt.query(params![
            goods.key(),
            q.min_quantity.max(1),
            age_cutoff(q.max_age_hours)
        ])?;
        let idx = index.read();
        let mut last_price = None;
        while let Some(r) = rows.next()? {
            let price: i64 = r.get(1)?;
            if early_stop && cands.len() >= limit && last_price != Some(price) {
                break;
            }
            let market_id: i64 = r.get(0)?;
            let Some(st) = idx.get(market_id) else {
                continue;
            };
            if !filter.passes(st) {
                continue;
            }
            let d = match (q.origin, st.pos) {
                (Some(o), Some(p)) => Some(dist(o, p)),
                _ => None,
            };
            if let Some(m) = max_d
                && !d.is_some_and(|d| d <= m)
            {
                continue;
            }
            last_price = Some(price);
            cands.push(Cand {
                market_id,
                cid: goods.cid(r)?,
                price,
                qty: r.get(2)?,
                updated: r.get(3)?,
                dist: d,
                commodity: goods.commodity(r)?,
            });
        }
    }
    let price = |a: &Cand, b: &Cand| match q.mode {
        TradeMode::BuyFromStation => a.price.cmp(&b.price),
        TradeMode::SellToStation => b.price.cmp(&a.price),
    };
    let d = |c: &Cand| c.dist.unwrap_or(f64::INFINITY);
    let cmp = |a: &Cand, b: &Cand| match q.sort {
        Sort::BestPrice => price(a, b)
            .then(d(a).total_cmp(&d(b)))
            .then(b.updated.cmp(&a.updated)),
        Sort::Nearest => d(a).total_cmp(&d(b)).then(price(a, b)),
        Sort::Freshest => b.updated.cmp(&a.updated).then(price(a, b)),
    };
    if cands.len() > limit {
        cands.select_nth_unstable_by(limit, cmp);
        cands.truncate(limit);
    }
    cands.sort_by(cmp);

    let mut averages = RowAverages::new(conn);
    let mut stmt = conn.prepare_cached(
        "SELECT name, type, system_name, system_id64, x, y, z, dist_ls, max_pad, is_carrier, is_planetary, carrier_access \
         FROM stations WHERE market_id = ?1",
    )?;
    let mut out = Vec::with_capacity(cands.len());
    for c in cands {
        let commodity = match c.commodity {
            Some(rc) => Some(RowCommodity { mean: averages.at(c.market_id, c.cid)?, ..rc }),
            None => None,
        };
        let hit = stmt
            .query_row([c.market_id], |r| {
                let is_carrier = r.get::<_, i64>(9)? != 0;
                Ok(CommodityHit {
                    system: r.get(2)?,
                    system_id64: r.get::<_, Option<i64>>(3)?.map(|v| v as u64),
                    pos: opt_pos(r.get(4)?, r.get(5)?, r.get(6)?),
                    distance_ly: c.dist,
                    station: r.get(0)?,
                    market_id: c.market_id as u64,
                    station_type: r.get(1)?,
                    dist_ls: r.get(7)?,
                    max_pad: Pad::from_code(r.get(8)?).map(Pad::as_char),
                    price: c.price,
                    quantity: c.qty,
                    updated_unix: c.updated,
                    is_carrier,
                    is_planetary: r.get::<_, i64>(10)? != 0,
                    carrier_access: carrier_access(is_carrier, r.get(11)?),
                    commodity,
                })
            })
            .optional()?;
        out.extend(hit);
    }
    Ok(out)
}

/// Rows of one commodity with stock (`market_sellers`) or demand (`market_buyers`): the WHERE
/// clauses repeat the partial-index conditions word for word, so SQLite reads only the index.
const SELLER_ROWS: &str = "SELECT market_id, buy, stock, updated FROM market \
     WHERE commodity_id = ?1 AND buy > 0 AND stock > 0 AND stock >= ?2 AND updated >= ?3";
const BUYER_ROWS: &str = "SELECT market_id, sell, demand, updated FROM market \
     WHERE commodity_id = ?1 AND sell > 0 AND demand > 0 AND demand >= ?2 AND updated >= ?3";
/// The same for rare goods (one category's, or every category's with ?1 NULL), with each row's commodity. The rare
/// goods come first (CROSS JOIN keeps that order), so SQLite reads each one's range of the same index, never the
/// whole index or market table; every column read is in the index (`rare_goods_read_their_own_rows` checks the plans).
const RARE_SELLER_ROWS: &str = "SELECT m.market_id, m.buy, m.stock, m.updated, m.commodity_id, c.symbol, c.name \
     FROM commodities AS c CROSS JOIN market AS m ON m.commodity_id = c.id \
     WHERE c.is_rare = 1 AND (?1 IS NULL OR c.category = ?1) AND m.buy > 0 AND m.stock > 0 AND m.stock >= ?2 AND m.updated >= ?3";
const RARE_BUYER_ROWS: &str = "SELECT m.market_id, m.sell, m.demand, m.updated, m.commodity_id, c.symbol, c.name \
     FROM commodities AS c CROSS JOIN market AS m ON m.commodity_id = c.id \
     WHERE c.is_rare = 1 AND (?1 IS NULL OR c.category = ?1) AND m.sell > 0 AND m.demand > 0 AND m.demand >= ?2 AND m.updated >= ?3";
/// The rare goods of one category, or (?1 NULL) all of them.
const RARE_IDS: &str = "SELECT id FROM commodities WHERE is_rare = 1 AND (?1 IS NULL OR category = ?1)";

/// The galactic averages of rare goods rows, looked up for the rows kept: the one the row's market reports or, where
/// it reports none (fleet carriers), the one most markets report for that good; 0 when no market reports one.
struct RowAverages<'c> {
    conn: &'c Connection,
    goods: rustc_hash::FxHashMap<i64, i64>,
}

impl<'c> RowAverages<'c> {
    fn new(conn: &'c Connection) -> Self {
        RowAverages { conn, goods: Default::default() }
    }

    /// The average for a commodity at one market.
    fn at(&mut self, market_id: i64, cid: i64) -> Result<i64> {
        let own: Option<i64> = self
            .conn
            .prepare_cached("SELECT mean FROM market WHERE market_id = ?1 AND commodity_id = ?2")?
            .query_row([market_id, cid], |r| r.get(0))
            .optional()?;
        self.or_good(cid, own.unwrap_or(0))
    }

    /// `own`, the average a market reported with its prices, or the good's when that is 0.
    fn or_good(&mut self, cid: i64, own: i64) -> Result<i64> {
        if own > 0 {
            return Ok(own);
        }
        if let Some(&avg) = self.goods.get(&cid) {
            return Ok(avg);
        }
        let avg = crate::deals::global_average(self.conn, cid)?.unwrap_or(0);
        self.goods.insert(cid, avg);
        Ok(avg)
    }
}

/// Rare goods' galactic average, taken as one commodity: the mean of each rare good's own (the one most markets
/// report for it), every good counted once.
fn rare_average(conn: &Connection, category: Option<&str>) -> Result<Option<i64>> {
    let ids: Vec<i64> = conn.prepare_cached(RARE_IDS)?.query_map([category], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let (mut sum, mut n) = (0_i64, 0_i64);
    for cid in ids {
        if let Some(avg) = crate::deals::global_average(conn, cid)? {
            sum += avg;
            n += 1;
        }
    }
    Ok((n > 0).then(|| (sum as f64 / n as f64).round() as i64))
}

/// Galaxy-wide counts and extremes for one commodity, or rare goods taken as one (only the age limit
/// applies). Rare goods have no symbol or name of their own: those stay empty.
pub(crate) fn commodity_summary(
    conn: &Connection,
    index: &RwLock<StationIndex>,
    cache: &CommodityCache,
    goods: Goods,
    max_age_hours: Option<f64>,
) -> Result<Option<CommoditySummary>> {
    let (symbol, name, category, is_rare, average) = match goods {
        Goods::One(cid) => {
            let Some(meta) = cache.meta(cid) else {
                return Ok(None);
            };
            (meta.symbol.clone(), meta.name.clone(), meta.category.clone(), meta.rare, crate::deals::global_average(conn, cid)?)
        }
        Goods::Rare(category) => (String::new(), String::new(), category.unwrap_or_default().to_string(), true, rare_average(conn, category)?),
    };
    let mut s = CommoditySummary {
        symbol,
        name,
        category,
        is_rare,
        average,
        sellers: 0,
        carrier_sellers: 0,
        buyers: 0,
        carrier_buyers: 0,
        lowest_buy: None,
        highest_sell: None,
        newest_unix: None,
    };
    let cutoff = age_cutoff(max_age_hours);
    let idx = index.read();
    // Markets, not rows: a carrier with buy orders for ten rare goods is one market buying them.
    let mut counted = rustc_hash::FxHashSet::default();
    for selling in [true, false] {
        let mut stmt = conn.prepare_cached(goods.rows(selling))?;
        let mut rows = stmt.query(params![goods.key(), 1, cutoff])?;
        while let Some(r) = rows.next()? {
            let market_id: i64 = r.get(0)?;
            let Some(st) = idx.get(market_id) else {
                continue;
            };
            let price: i64 = r.get(1)?;
            let updated: i64 = r.get(3)?;
            s.newest_unix = Some(s.newest_unix.map_or(updated, |n| n.max(updated)));
            let new = u32::from(counted.insert((selling, market_id)));
            let carrier = new * u32::from(st.is_carrier());
            if selling {
                s.sellers += new;
                s.carrier_sellers += carrier;
                s.lowest_buy = Some(s.lowest_buy.map_or(price, |p| p.min(price)));
            } else {
                s.buyers += new;
                s.carrier_buyers += carrier;
                s.highest_sell = Some(s.highest_sell.map_or(price, |p| p.max(price)));
            }
        }
    }
    Ok(Some(s))
}

/// A "latest prices" candidate: market, last update, distance, commodity id, and rare goods' commodity.
type LatestCand = (i64, i64, Option<f64>, i64, Option<RowCommodity>);

/// Every market trading one commodity, or rare goods (stock or demand of at least `min_quantity`),
/// newest update first. `mode` and `sort` are ignored; the other filters apply as in a commodity search.
pub(crate) fn latest_prices(
    conn: &Connection,
    index: &RwLock<StationIndex>,
    goods: Goods,
    q: &CommodityQuery,
) -> Result<Vec<CommodityQuote>> {
    let limit = q.limit.clamp(1, 100_000);
    let filter = StationFilter {
        min_pad: pad_code(q.min_pad) as u8,
        carriers: q.include_carriers,
        planetary: q.include_planetary,
        max_dist_ls: -1.0,
    };
    let max_d = q.max_distance_ly.filter(|d| d.is_finite() && *d >= 0.0);
    // One entry per market and commodity, even when the market both sells and buys it.
    let mut cands: Vec<LatestCand> = Vec::new();
    {
        let idx = index.read();
        let mut seen = rustc_hash::FxHashSet::default();
        for selling in [true, false] {
            let mut stmt = conn.prepare_cached(goods.rows(selling))?;
            let mut rows = stmt.query(params![
                goods.key(),
                q.min_quantity.max(1),
                age_cutoff(q.max_age_hours)
            ])?;
            while let Some(r) = rows.next()? {
                let market_id: i64 = r.get(0)?;
                let cid = goods.cid(r)?;
                let Some(st) = idx.get(market_id) else {
                    continue;
                };
                if !filter.passes(st) || seen.contains(&(market_id, cid)) {
                    continue;
                }
                let d = match (q.origin, st.pos) {
                    (Some(o), Some(p)) => Some(dist(o, p)),
                    _ => None,
                };
                if let Some(m) = max_d
                    && !d.is_some_and(|d| d <= m)
                {
                    continue;
                }
                seen.insert((market_id, cid));
                cands.push((market_id, r.get(3)?, d, cid, goods.commodity(r)?));
            }
        }
    }
    let newest = |a: &LatestCand, b: &LatestCand| {
        b.1.cmp(&a.1)
            .then(a.2.unwrap_or(f64::INFINITY).total_cmp(&b.2.unwrap_or(f64::INFINITY)))
    };
    if cands.len() > limit {
        cands.select_nth_unstable_by(limit, newest);
        cands.truncate(limit);
    }
    cands.sort_by(newest);

    let mut stmt = conn.prepare_cached(
        "SELECT m.buy, m.sell, m.mean, m.stock, m.demand, m.updated, \
                s.name, s.type, s.system_name, s.dist_ls, s.max_pad, s.is_carrier, s.is_planetary, s.carrier_access \
         FROM market AS m JOIN stations AS s ON s.market_id = m.market_id \
         WHERE m.market_id = ?1 AND m.commodity_id = ?2",
    )?;
    let mut averages = RowAverages::new(conn);
    let mut out = Vec::with_capacity(cands.len());
    for (market_id, _, d, cid, commodity) in cands {
        let mut quote = stmt
            .query_row(params![market_id, cid], |r| {
                let is_carrier = r.get::<_, i64>(11)? != 0;
                Ok(CommodityQuote {
                    market_id: market_id as u64,
                    buy: r.get(0)?,
                    sell: r.get(1)?,
                    mean: r.get(2)?,
                    stock: r.get(3)?,
                    demand: r.get(4)?,
                    updated_unix: r.get(5)?,
                    station: r.get(6)?,
                    station_type: r.get(7)?,
                    system: r.get(8)?,
                    distance_ly: d,
                    dist_ls: r.get(9)?,
                    max_pad: Pad::from_code(r.get(10)?).map(Pad::as_char),
                    is_carrier,
                    is_planetary: r.get::<_, i64>(12)? != 0,
                    carrier_access: carrier_access(is_carrier, r.get(13)?),
                    commodity,
                })
            })
            .optional()?;
        if let Some(q) = &mut quote
            && let Some(c) = &mut q.commodity
        {
            c.mean = averages.or_good(cid, q.mean)?;
        }
        out.extend(quote);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Stations
// ---------------------------------------------------------------------------------------------

pub(crate) fn station(conn: &Connection, market_id: u64) -> Result<Option<StationInfo>> {
    let sql = format!("SELECT {STATION_COLS} FROM stations AS s WHERE s.market_id = ?1");
    Ok(conn
        .prepare_cached(&sql)?
        .query_row([market_id as i64], |r| station_info(r, 0))
        .optional()?)
}

pub(crate) fn station_market(
    conn: &Connection,
    cache: &CommodityCache,
    market_id: u64,
) -> Result<Option<StationMarket>> {
    let sql =
        format!("SELECT {STATION_COLS}, s.prohibited FROM stations AS s WHERE s.market_id = ?1");
    let Some((station, prohibited)) = conn
        .prepare_cached(&sql)?
        .query_row([market_id as i64], |r| {
            Ok((
                station_info(r, 0)?,
                r.get::<_, Option<String>>(STATION_NCOLS)?,
            ))
        })
        .optional()?
    else {
        return Ok(None);
    };
    let system = match station.system_id64 {
        Some(id) => system_by_id(conn, id)?,
        None => None,
    };
    let mut stmt = conn.prepare_cached(
        "SELECT commodity_id, buy, sell, mean, stock, stock_bracket, demand, demand_bracket, updated \
         FROM market WHERE market_id = ?1",
    )?;
    let rows = stmt.query_map([market_id as i64], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            MarketRow {
                symbol: String::new(),
                name: String::new(),
                category: String::new(),
                buy: r.get(1)?,
                sell: r.get(2)?,
                mean: r.get(3)?,
                stock: r.get(4)?,
                stock_bracket: r.get::<_, Option<i64>>(5)?.map(|b| b.clamp(-1, 3) as i8),
                demand: r.get(6)?,
                demand_bracket: r.get::<_, Option<i64>>(7)?.map(|b| b.clamp(-1, 3) as i8),
                updated_unix: r.get(8)?,
            },
        ))
    })?;
    let mut items = Vec::new();
    for row in rows {
        let (cid, mut item) = row?;
        if let Some(m) = cache.meta(cid) {
            item.symbol.clone_from(&m.symbol);
            item.name.clone_from(&m.name);
            item.category.clone_from(&m.category);
        } else {
            item.symbol = format!("#{cid}");
            item.name.clone_from(&item.symbol);
        }
        items.push(item);
    }
    items.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(Some(StationMarket {
        station,
        system,
        prohibited: json_strings(prohibited),
        items,
    }))
}

pub(crate) fn stations_in_system(conn: &Connection, id64: u64) -> Result<Vec<StationInfo>> {
    let sql = format!(
        "SELECT {STATION_COLS} FROM stations AS s WHERE s.system_id64 = ?1 ORDER BY s.dist_ls"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([id64 as i64], |r| station_info(r, 0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn like_prefix(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 1);
    for c in s.trim().chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

pub(crate) fn find_stations(
    conn: &Connection,
    prefix: &str,
    limit: usize,
) -> Result<Vec<StationInfo>> {
    if prefix.trim().is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {STATION_COLS} FROM stations AS s WHERE s.name LIKE ?1 ESCAPE '\\' \
         ORDER BY s.market_updated DESC LIMIT ?2"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(
        params![like_prefix(prefix), limit.clamp(1, 1000) as i64],
        |r| station_info(r, 0),
    )?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn recent_markets(conn: &Connection, limit: usize) -> Result<Vec<StationInfo>> {
    let sql = format!(
        "SELECT {STATION_COLS} FROM stations AS s WHERE s.market_updated > 0 ORDER BY s.market_updated DESC LIMIT ?1"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([limit.clamp(1, 10_000) as i64], |r| station_info(r, 0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Nearest stations: a scan of the in-memory station index, then details for the hits only.
pub(crate) fn stations_near(
    conn: &Connection,
    index: &RwLock<StationIndex>,
    q: &StationQuery,
) -> Result<Vec<StationHit>> {
    let limit = q.limit.clamp(1, 100_000);
    let mut services = q.services;
    if q.material_trader.is_some() {
        services |= Services::MATERIAL_TRADER;
    }
    if q.tech_broker.is_some() {
        services |= Services::TECH_BROKER;
    }
    let mask = services.bits();
    let mt = q.material_trader.map_or(0, MaterialTrader::code) as u8;
    let tb = q.tech_broker.map_or(0, TechBroker::code) as u8;
    let filter = StationFilter {
        min_pad: pad_code(q.min_pad) as u8,
        carriers: q.include_carriers,
        planetary: q.include_planetary,
        max_dist_ls: q.max_dist_ls.filter(|d| d.is_finite()).unwrap_or(-1.0),
    };
    let max_d = q
        .max_distance_ly
        .filter(|d| d.is_finite() && *d >= 0.0)
        .unwrap_or(f64::INFINITY);
    let mut cands: Vec<(f64, f32, i64)> = {
        let idx = index.read();
        idx.iter()
            .filter_map(|(id, s)| {
                if s.services & mask != mask
                    || (mt != 0 && s.mat_trader != mt)
                    || (tb != 0 && s.tech_broker != tb)
                {
                    return None;
                }
                if !filter.passes(s) {
                    return None;
                }
                let d = dist(q.origin, s.pos?);
                (d <= max_d).then_some((
                    d,
                    if s.dist_ls.is_nan() {
                        f32::MAX
                    } else {
                        s.dist_ls
                    },
                    id,
                ))
            })
            .collect()
    };
    let cmp = |a: &(f64, f32, i64), b: &(f64, f32, i64)| {
        a.0.total_cmp(&b.0)
            .then(a.1.total_cmp(&b.1))
            .then(a.2.cmp(&b.2))
    };
    if cands.len() > limit {
        cands.select_nth_unstable_by(limit, cmp);
        cands.truncate(limit);
    }
    cands.sort_by(cmp);
    let mut out = Vec::with_capacity(cands.len());
    for (d, _, id) in cands {
        if let Some(st) = station(conn, id as u64)? {
            out.push(StationHit {
                station: st,
                distance_ly: d,
            });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------------------------

pub(crate) fn system_by_id(conn: &Connection, id64: u64) -> Result<Option<SystemInfo>> {
    let sql = format!("SELECT {SYSTEM_COLS} FROM systems AS sy WHERE sy.id64 = ?1");
    Ok(conn
        .prepare_cached(&sql)?
        .query_row([id64 as i64], system_info)
        .optional()?)
}

pub(crate) fn system_by_name(conn: &Connection, name: &str) -> Result<Option<SystemInfo>> {
    let sql = format!(
        "SELECT {SYSTEM_COLS} FROM systems AS sy WHERE sy.name_lower = ?1 ORDER BY (sy.x IS NULL), sy.updated DESC LIMIT 1"
    );
    Ok(conn
        .prepare_cached(&sql)?
        .query_row([name.trim().to_lowercase()], system_info)
        .optional()?)
}

/// Systems whose name starts with `prefix` (case-insensitive), for autocompletion.
pub(crate) fn find_systems(
    conn: &Connection,
    prefix: &str,
    limit: usize,
) -> Result<Vec<SystemInfo>> {
    let lo = prefix.trim().to_lowercase();
    if lo.is_empty() {
        return Ok(Vec::new());
    }
    let hi = format!("{lo}\u{10FFFF}");
    let sql = format!(
        "SELECT {SYSTEM_COLS} FROM systems AS sy WHERE sy.name_lower >= ?1 AND sy.name_lower < ?2 \
         ORDER BY sy.name_lower LIMIT ?3"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![lo, hi, limit.clamp(1, 1000) as i64], system_info)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn systems_near(
    conn: &Connection,
    pos: [f64; 3],
    radius_ly: f64,
    limit: usize,
) -> Result<Vec<SystemHit>> {
    let r = radius_ly.clamp(0.0, MAX_RADIUS);
    let b = bbox(pos, r);
    let sql = format!(
        "SELECT {SYSTEM_COLS} FROM systems_rtree AS r CROSS JOIN systems AS sy ON sy.id64 = r.id \
         WHERE r.max_x >= ?1 AND r.min_x <= ?2 AND r.max_y >= ?3 AND r.min_y <= ?4 AND r.max_z >= ?5 AND r.min_z <= ?6"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![b[0], b[1], b[2], b[3], b[4], b[5]], system_info)?;
    let mut out = Vec::new();
    for s in rows {
        let s = s?;
        if let Some(p) = s.pos {
            let d = dist(pos, p);
            if d <= r {
                out.push(SystemHit {
                    system: s,
                    distance_ly: d,
                });
            }
        }
    }
    out.sort_by(|a, b| a.distance_ly.total_cmp(&b.distance_ly));
    out.truncate(limit.max(1));
    Ok(out)
}

pub(crate) fn system_factions(conn: &Connection, id64: u64) -> Result<Vec<FactionInfo>> {
    let mut stmt = conn.prepare_cached(
        "SELECT faction, influence, state, government, allegiance, happiness, active_states, pending_states, \
         recovering_states, updated FROM system_factions WHERE system_id64 = ?1 ORDER BY influence DESC, faction",
    )?;
    let rows = stmt.query_map([id64 as i64], |r| {
        Ok(FactionInfo {
            name: r.get(0)?,
            influence: r.get(1)?,
            state: r.get(2)?,
            government: r.get(3)?,
            allegiance: r.get(4)?,
            happiness: r.get(5)?,
            active_states: json_strings(r.get(6)?),
            pending_states: json_strings(r.get(7)?),
            recovering_states: json_strings(r.get(8)?),
            updated_unix: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// `signal` is the resolved commodity symbol.
pub(crate) fn hotspots_near(
    conn: &Connection,
    signal: &str,
    q: &HotspotQuery,
) -> Result<Vec<HotspotHit>> {
    let r = q
        .max_distance_ly
        .unwrap_or(MAX_RADIUS)
        .clamp(0.0, MAX_RADIUS);
    let b = bbox(q.origin, r);
    let mut stmt = conn.prepare_cached(
        "SELECT sy.name, rs.system_id64, sy.x, sy.y, sy.z, rs.body_name, rs.ring_type, rs.signal, rs.count, rs.updated \
         FROM ring_signals AS rs CROSS JOIN systems AS sy ON sy.id64 = rs.system_id64 \
         WHERE rs.signal = ?1 AND rs.count >= ?2 AND sy.x BETWEEN ?3 AND ?4 AND sy.y BETWEEN ?5 AND ?6 AND sy.z BETWEEN ?7 AND ?8",
    )?;
    let rows = stmt.query_map(
        params![
            signal,
            q.min_count.max(1),
            b[0],
            b[1],
            b[2],
            b[3],
            b[4],
            b[5]
        ],
        |row| {
            let pos = [row.get(2)?, row.get(3)?, row.get(4)?];
            Ok(HotspotHit {
                system: row.get(0)?,
                system_id64: row.get::<_, i64>(1)? as u64,
                pos,
                distance_ly: dist(q.origin, pos),
                body_name: row.get(5)?,
                ring_type: row.get(6)?,
                signal: row.get(7)?,
                count: row.get(8)?,
                updated_unix: row.get(9)?,
            })
        },
    )?;
    let mut out: Vec<HotspotHit> = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    out.retain(|h| h.distance_ly <= r);
    out.sort_by(|a, b| {
        a.distance_ly
            .total_cmp(&b.distance_ly)
            .then(b.count.cmp(&a.count))
    });
    out.truncate(q.limit.max(1));
    Ok(out)
}

pub(crate) fn price_history(
    conn: &Connection,
    market_id: u64,
    cid: i64,
) -> Result<Vec<PricePoint>> {
    let mut stmt = conn.prepare_cached(
        "SELECT ts, buy, sell FROM price_history WHERE market_id = ?1 AND commodity_id = ?2 ORDER BY ts",
    )?;
    let rows = stmt.query_map(params![market_id as i64, cid], |r| {
        Ok(PricePoint {
            ts_unix: r.get(0)?,
            buy: r.get(1)?,
            sell: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// How long a market's prices for a commodity have stood (see [`Stood`]); `None` when the market doesn't list it.
/// `kept_from`: the oldest time the history still holds every change from (older ones are pruned).
pub(crate) fn price_since(conn: &Connection, market_id: u64, cid: i64, kept_from: i64) -> Result<Option<PriceSince>> {
    let row = conn
        .prepare_cached(
            "SELECT m.buy, m.sell, s.history_since FROM market AS m JOIN stations AS s ON s.market_id = m.market_id \
             WHERE m.market_id = ?1 AND m.commodity_id = ?2",
        )?
        .query_row(params![market_id as i64, cid], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?)))
        .optional()?;
    let Some((buy, sell, tracked)) = row else {
        return Ok(None);
    };
    let points = price_history(conn, market_id, cid)?;
    let side = |now: i64, of: fn(&PricePoint) -> i64| {
        (now > 0).then(|| {
            let pts: Vec<(i64, i64)> = points.iter().map(|p| (p.ts_unix, of(p))).collect();
            stood(&pts, now, tracked, kept_from)
        })
    };
    Ok(Some(PriceSince { buy: side(buy, |p| p.buy), sell: side(sell, |p| p.sell) }))
}

/// How long `price` has stood, from one side's history points (time, price; oldest first). `tracked`: the report
/// since which the history has every change at this market; `kept_from`: changes before this may have been pruned.
pub(crate) fn stood(points: &[(i64, i64)], price: i64, tracked: Option<i64>, kept_from: i64) -> Stood {
    let Some(tracked) = tracked else {
        return Stood::Unknown;
    };
    // Points from before an unfollowed stretch (an import replaced the prices) say nothing about since.
    let pts: Vec<(i64, i64)> = points.iter().copied().filter(|p| p.0 >= tracked).collect();
    // The newest points at this price, back to the change that brought it.
    let first = pts.iter().rposition(|p| p.1 != price).map_or(0, |i| i + 1);
    match (first.checked_sub(1).map(|i| pts[i]), pts.get(first)) {
        // A change on record: from the report before it to the first at this price.
        (Some(before), Some(at)) => Stood::Since { since: at.0, before: before.0, previous: before.1 },
        // The newest point isn't this price: a change went unrecorded (too small to keep, say).
        (Some(_), None) => Stood::Unknown,
        // No change on record since the history began here. Changes older than what's kept are gone: with nothing
        // left, the price has stood since then at least; with points left, the first may be the one that brought it
        // (its report before pruned away), so only from that one.
        (None, at) => Stood::AtLeast {
            since: match at {
                Some(at) if tracked < kept_from => at.0,
                _ => tracked.max(kept_from),
            },
        },
    }
}

/// Row counts. Station / market numbers come from the in-memory index; the rest are `COUNT(*)`s, except the price
/// history (the writer keeps that count, see `GalaxyDb::stats`).
pub(crate) fn stats(conn: &Connection, index: &RwLock<StationIndex>) -> Result<DbStats> {
    let mut s = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM systems), (SELECT COUNT(*) FROM commodities), \
                (SELECT COUNT(*) FROM system_factions), (SELECT COUNT(*) FROM ring_signals)",
        [],
        |r| {
            Ok(DbStats {
                systems: r.get::<_, i64>(0)? as u64,
                commodities: r.get::<_, i64>(1)? as u64,
                faction_rows: r.get::<_, i64>(2)? as u64,
                ring_signals: r.get::<_, i64>(3)? as u64,
                ..DbStats::default()
            })
        },
    )?;
    let idx = index.read();
    let mut newest = 0;
    for (_, st) in idx.iter() {
        s.stations += 1;
        if st.n_commodities > 0 {
            s.stations_with_market += 1;
            s.market_rows += u64::from(st.n_commodities);
        }
        newest = newest.max(st.market_updated);
    }
    s.newest_market_update = (newest > 0).then_some(newest);
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    /// What a price's history says about how long it stood, case by case (times in days; 30 days kept).
    #[test]
    fn how_long_a_price_stood() {
        let now = 100 * DAY;
        let kept = now - 30 * DAY;
        let pts = |v: &[(i64, i64)]| v.iter().map(|&(d, p)| (d * DAY, p)).collect::<Vec<_>>();
        let tracked = Some(80 * DAY);
        // Changed from 50 to 55 between day 82 and day 88; unchanged since.
        assert_eq!(stood(&pts(&[(82, 50), (88, 55)]), 55, tracked, kept), Stood::Since { since: 88 * DAY, before: 82 * DAY, previous: 50 });
        // A later change of the other side adds points at this price: still since day 88.
        assert_eq!(stood(&pts(&[(82, 50), (88, 55), (90, 55), (95, 55)]), 55, tracked, kept), Stood::Since { since: 88 * DAY, before: 82 * DAY, previous: 50 });
        // New on the market: the report before didn't list it.
        assert_eq!(stood(&pts(&[(85, 0), (86, 60)]), 60, tracked, kept), Stood::Since { since: 86 * DAY, before: 85 * DAY, previous: 0 });
        // No change since the history of this market began.
        assert_eq!(stood(&[], 55, tracked, kept), Stood::AtLeast { since: 80 * DAY });
        assert_eq!(stood(&pts(&[(84, 55), (90, 55)]), 55, tracked, kept), Stood::AtLeast { since: 80 * DAY }, "only the other side changed");
        // Followed for longer than the history keeps: at least since what's kept...
        assert_eq!(stood(&[], 55, Some(10 * DAY), kept), Stood::AtLeast { since: kept });
        // ...or, with a point left whose report before was pruned, since that point only.
        assert_eq!(stood(&pts(&[(75, 55)]), 55, Some(10 * DAY), kept), Stood::AtLeast { since: 75 * DAY });
        // Points from before the prices were last replaced without comparing don't count.
        assert_eq!(stood(&pts(&[(70, 40), (72, 55)]), 55, Some(80 * DAY), kept), Stood::AtLeast { since: 80 * DAY });
        // The newest point isn't the price now (a change too small to keep): no answer.
        assert_eq!(stood(&pts(&[(82, 50), (88, 55)]), 56, tracked, kept), Stood::Unknown);
        // Not followed.
        assert_eq!(stood(&pts(&[(82, 50)]), 55, None, kept), Stood::Unknown);
    }

    /// Commodity pages, Rare Goods' included, read each good's range of the market indexes and only the index:
    /// never the whole market table or a whole index (a page load runs 6 to 8 of these queries). Plans of the empty
    /// schema: CROSS JOIN fixes the rare goods' join order whatever statistics a real database has.
    #[test]
    fn rare_goods_read_their_own_rows() {
        let conn = crate::schema::memory();
        let plan = |sql: &str| -> String {
            let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).expect("explain");
            let nulls = vec![rusqlite::types::Value::Null; stmt.parameter_count()];
            let lines: Vec<String> = stmt.query_map(rusqlite::params_from_iter(nulls), |r| r.get(3)).expect("plan").collect::<rusqlite::Result<_>>().expect("lines");
            lines.join("; ")
        };
        for goods in [Goods::One(1), Goods::Rare(None)] {
            for (selling, index) in [(true, "market_sellers"), (false, "market_buyers")] {
                for sql in [goods.rows(selling).to_string(), search_sql(goods, selling)] {
                    let p = plan(&sql);
                    assert!(p.contains(&format!("USING COVERING INDEX {index} (commodity_id=?")), "{sql}: {p}");
                    assert!(!p.contains("SCAN m"), "{sql}: {p}");
                }
            }
        }
        // Rare goods' averages, each good's own.
        for sql in crate::deals::AVERAGE_SAMPLES {
            let p = plan(sql);
            assert!(p.contains("USING INDEX market_") && p.contains("(commodity_id=?") && !p.contains("SCAN m"), "{sql}: {p}");
        }
    }
}
