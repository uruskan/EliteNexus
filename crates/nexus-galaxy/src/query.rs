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
    /// Rare goods as one: those of one category, or (`None`) all of them, through `market_goods`.
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

    /// A rare goods row's commodity (its query's columns 5 to 7); `None` for one commodity's rows.
    fn commodity(self, r: &Row) -> rusqlite::Result<Option<RowCommodity>> {
        match self {
            Goods::One(_) => Ok(None),
            Goods::Rare(_) => Ok(Some(RowCommodity { symbol: r.get(5)?, name: r.get(6)?, mean: r.get(7)? })),
        }
    }
}

/// Candidate row of a commodity search (before the station details are fetched).
struct Cand {
    market_id: i64,
    price: i64,
    qty: i64,
    updated: i64,
    dist: Option<f64>,
    commodity: Option<RowCommodity>,
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
    let sql = match (goods, q.mode) {
        (Goods::One(_), TradeMode::BuyFromStation) => {
            "SELECT market_id, buy, stock, updated FROM market \
             WHERE commodity_id = ?1 AND buy > 0 AND stock > 0 AND stock >= ?2 AND updated >= ?3 ORDER BY buy ASC"
        }
        (Goods::One(_), TradeMode::SellToStation) => {
            "SELECT market_id, sell, demand, updated FROM market \
             WHERE commodity_id = ?1 AND sell > 0 AND demand > 0 AND demand >= ?2 AND updated >= ?3 ORDER BY sell DESC"
        }
        (Goods::Rare(_), TradeMode::BuyFromStation) => {
            "SELECT market_id, buy, stock, updated, commodity_id, symbol, name, mean FROM market_goods \
             WHERE is_rare = 1 AND (?1 IS NULL OR category = ?1) AND buy > 0 AND stock > 0 AND stock >= ?2 AND updated >= ?3 ORDER BY buy ASC"
        }
        (Goods::Rare(_), TradeMode::SellToStation) => {
            "SELECT market_id, sell, demand, updated, commodity_id, symbol, name, mean FROM market_goods \
             WHERE is_rare = 1 AND (?1 IS NULL OR category = ?1) AND sell > 0 AND demand > 0 AND demand >= ?2 AND updated >= ?3 ORDER BY sell DESC"
        }
    };
    // Rows arrive best price first: for that order we can stop once `limit` rows passed the filters
    // (and the price changes, so that equal prices are all collected for the distance tie-break).
    let early_stop = q.sort == Sort::BestPrice;
    let mut cands: Vec<Cand> = Vec::new();
    {
        let mut stmt = conn.prepare_cached(sql)?;
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

    let mut stmt = conn.prepare_cached(
        "SELECT name, type, system_name, system_id64, x, y, z, dist_ls, max_pad, is_carrier, is_planetary \
         FROM stations WHERE market_id = ?1",
    )?;
    let mut out = Vec::with_capacity(cands.len());
    for c in cands {
        let hit = stmt
            .query_row([c.market_id], |r| {
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
                    is_carrier: r.get::<_, i64>(9)? != 0,
                    is_planetary: r.get::<_, i64>(10)? != 0,
                    commodity: c.commodity.clone(),
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
/// The same for rare goods (one category's, or every category's with ?1 NULL), with each row's commodity.
const RARE_SELLER_ROWS: &str = "SELECT market_id, buy, stock, updated, commodity_id, symbol, name, mean FROM market_goods \
     WHERE is_rare = 1 AND (?1 IS NULL OR category = ?1) AND buy > 0 AND stock > 0 AND stock >= ?2 AND updated >= ?3";
const RARE_BUYER_ROWS: &str = "SELECT market_id, sell, demand, updated, commodity_id, symbol, name, mean FROM market_goods \
     WHERE is_rare = 1 AND (?1 IS NULL OR category = ?1) AND sell > 0 AND demand > 0 AND demand >= ?2 AND updated >= ?3";

impl Goods<'_> {
    /// The seller and buyer row queries for these goods.
    fn row_queries(self) -> [(&'static str, bool); 2] {
        match self {
            Goods::One(_) => [(SELLER_ROWS, true), (BUYER_ROWS, false)],
            Goods::Rare(_) => [(RARE_SELLER_ROWS, true), (RARE_BUYER_ROWS, false)],
        }
    }
}

/// Rare goods' galactic average, taken as one commodity: the mean of the averages their markets report.
fn rare_average(conn: &Connection, category: Option<&str>) -> Result<Option<i64>> {
    let avg: Option<f64> = conn.query_row(
        "SELECT AVG(mean) FROM market_goods WHERE is_rare = 1 AND (?1 IS NULL OR category = ?1) AND mean > 0",
        [category],
        |r| r.get(0),
    )?;
    Ok(avg.map(|a| a.round() as i64))
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
    for (sql, selling) in goods.row_queries() {
        let mut stmt = conn.prepare_cached(sql)?;
        let mut rows = stmt.query(params![goods.key(), 1, cutoff])?;
        while let Some(r) = rows.next()? {
            let Some(st) = idx.get(r.get(0)?) else {
                continue;
            };
            let price: i64 = r.get(1)?;
            let updated: i64 = r.get(3)?;
            s.newest_unix = Some(s.newest_unix.map_or(updated, |n| n.max(updated)));
            let carrier = u32::from(st.is_carrier());
            if selling {
                s.sellers += 1;
                s.carrier_sellers += carrier;
                s.lowest_buy = Some(s.lowest_buy.map_or(price, |p| p.min(price)));
            } else {
                s.buyers += 1;
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
        for (sql, _) in goods.row_queries() {
            let mut stmt = conn.prepare_cached(sql)?;
            let mut rows = stmt.query(params![
                goods.key(),
                q.min_quantity.max(1),
                age_cutoff(q.max_age_hours)
            ])?;
            while let Some(r) = rows.next()? {
                let market_id: i64 = r.get(0)?;
                let cid = match goods {
                    Goods::One(cid) => cid,
                    Goods::Rare(_) => r.get(4)?,
                };
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
                s.name, s.type, s.system_name, s.dist_ls, s.max_pad, s.is_carrier, s.is_planetary \
         FROM market AS m JOIN stations AS s ON s.market_id = m.market_id \
         WHERE m.market_id = ?1 AND m.commodity_id = ?2",
    )?;
    let mut out = Vec::with_capacity(cands.len());
    for (market_id, _, d, cid, commodity) in cands {
        let quote = stmt
            .query_row(params![market_id, cid], |r| {
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
                    is_carrier: r.get::<_, i64>(11)? != 0,
                    is_planetary: r.get::<_, i64>(12)? != 0,
                    commodity,
                })
            })
            .optional()?;
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
