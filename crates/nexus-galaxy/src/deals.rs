//! "Deals": stations paying far above the galactic average for a commodity, or selling it far
//! below the average, ranked by what the difference is worth for one hold.
//!
//! 1. Candidate stations come from the in-memory station index (radius, pad, carriers, market age).
//! 2. Their market rows are loaded in parallel over several read connections.
//! 3. The average is the row's `mean` (the in-game galactic average). Fleet carriers report 0, so
//!    their rows use the average most other stations report for that commodity; when no other
//!    station in range has one, the commodity's average is looked up across the whole database.

use crate::error::Result;
use crate::index::StationFilter;
use crate::model::*;
use crate::query::{MAX_RADIUS, ReadPool, station};
use crate::station::Pad;
use crate::util::{dist, now_unix};
use crate::writer::Shared;
use rayon::prelude::*;
use rusqlite::Connection;
use rustc_hash::FxHashMap;

/// Stations per parallel loading task.
const LOAD_CHUNK: usize = 192;

struct Row {
    market: i64,
    dist: Option<f64>,
    cid: i64,
    buy: i64,
    sell: i64,
    mean: i64,
    stock: i64,
    demand: i64,
    updated: i64,
}

struct Cand {
    row: usize,
    price: i64,
    average: i64,
    ratio: f64,
    quantity: i64,
    extra: i64,
    trade: i64,
}

/// Partial indexes over exactly the rows an abnormal-price scan reads: prices already 1.5× off
/// their galactic average, and every unpriced order (fleet carriers report no average, and most
/// abnormal prices are carrier orders). SQLite keeps them current on every write, so a whole-galaxy
/// scan touches only these rows. Created by `schema::open_writer`.
pub(crate) const ANOMALY_INDEXES: &str = "
CREATE INDEX IF NOT EXISTS market_sell_high ON market(updated) WHERE mean > 0 AND sell > 0 AND demand > 0 AND sell * 2 >= mean * 3;
CREATE INDEX IF NOT EXISTS market_buy_low ON market(updated) WHERE mean > 0 AND buy > 0 AND stock > 0 AND buy * 3 <= mean * 2;
CREATE INDEX IF NOT EXISTS market_unpriced_buyers ON market(updated) WHERE mean <= 0 AND sell > 0 AND demand > 0;
CREATE INDEX IF NOT EXISTS market_unpriced_sellers ON market(updated) WHERE mean <= 0 AND buy > 0 AND stock > 0;
";

/// The multiple from which the anomaly indexes hold every qualifying row.
const INDEXED_RATIO: f64 = 1.5;

pub(crate) fn price_deals(pool: &ReadPool, shared: &Shared, q: &DealQuery) -> Result<Vec<Deal>> {
    if q.min_ratio >= INDEXED_RATIO {
        return abnormal_prices(pool, shared, q);
    }
    let cutoff = match q.max_age_hours {
        Some(h) if h.is_finite() && h > 0.0 => now_unix() - (h * 3600.0) as i64,
        _ => 1,
    };
    let filter = StationFilter {
        min_pad: match q.min_pad.and_then(Pad::from_char) {
            Some(Pad::Medium) => 2,
            Some(Pad::Large) => 3,
            _ => 0,
        },
        carriers: q.include_carriers,
        planetary: true,
        max_dist_ls: -1.0,
    };
    let radius = q
        .max_distance_ly
        .filter(|d| d.is_finite() && *d >= 0.0)
        .map(|d| d.min(MAX_RADIUS));

    // 1. candidate stations
    let stations: Vec<(i64, Option<f64>)> = {
        let idx = shared.stations.read();
        idx.iter()
            .filter_map(|(id, s)| {
                if s.n_commodities == 0 || s.market_updated < cutoff || !filter.passes(s) {
                    return None;
                }
                let d = match (q.origin, s.pos) {
                    (Some(o), Some(p)) => Some(dist(o, p)),
                    _ => None,
                };
                if let Some(r) = radius
                    && !d.is_some_and(|d| d <= r)
                {
                    return None;
                }
                Some((id, d))
            })
            .collect()
    };

    // 2. market rows
    let parts: Vec<Result<Vec<Row>>> = stations
        .par_chunks(LOAD_CHUNK)
        .map(|chunk| {
            let conn = pool.get()?;
            let mut stmt = conn.prepare_cached(
                "SELECT commodity_id, buy, sell, mean, stock, demand, updated FROM market WHERE market_id = ?1",
            )?;
            let mut out = Vec::with_capacity(chunk.len() * 64);
            for &(market, dist) in chunk {
                let mut rows = stmt.query([market])?;
                while let Some(r) = rows.next()? {
                    out.push(Row {
                        market,
                        dist,
                        cid: r.get(0)?,
                        buy: r.get(1)?,
                        sell: r.get(2)?,
                        mean: r.get(3)?,
                        stock: r.get(4)?,
                        demand: r.get(5)?,
                        updated: r.get(6)?,
                    });
                }
            }
            Ok(out)
        })
        .collect();
    let mut rows = Vec::new();
    for p in parts {
        rows.extend(p?);
    }

    // 3. reference averages (for carrier rows that report 0)
    let mut counts: FxHashMap<i64, FxHashMap<i64, u32>> = FxHashMap::default();
    for r in rows.iter().filter(|r| r.mean > 0) {
        *counts.entry(r.cid).or_default().entry(r.mean).or_default() += 1;
    }
    let mut reference: FxHashMap<i64, i64> = counts
        .into_iter()
        .filter_map(|(cid, m)| m.into_iter().max_by_key(|&(v, n)| (n, v)).map(|(v, _)| (cid, v)))
        .collect();
    let conn = pool.get()?;
    let missing: Vec<i64> = {
        let mut m: Vec<i64> = rows.iter().filter(|r| r.mean <= 0 && !reference.contains_key(&r.cid)).map(|r| r.cid).collect();
        m.sort_unstable();
        m.dedup();
        m
    };
    for cid in missing {
        if let Some(avg) = global_average(&conn, cid)? {
            reference.insert(cid, avg);
        }
    }

    // 4. deals
    let cap = q.cargo_capacity.max(1);
    let min_ratio = if q.min_ratio.is_finite() { q.min_ratio.max(1.0) } else { 1.0 };
    let mut cands: Vec<Cand> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let average = if r.mean > 0 { r.mean } else { reference.get(&r.cid).copied().unwrap_or(0) };
        if average <= 0 {
            continue;
        }
        match q.mode {
            TradeMode::SellToStation => {
                if r.sell <= 0 || r.demand <= 0 || (r.sell as f64) < average as f64 * min_ratio || r.sell <= average {
                    continue;
                }
                let t = r.demand.min(cap);
                cands.push(Cand {
                    row: i,
                    price: r.sell,
                    average,
                    ratio: r.sell as f64 / average as f64,
                    quantity: r.demand,
                    extra: (r.sell - average) * t,
                    trade: r.sell * t,
                });
            }
            TradeMode::BuyFromStation => {
                if r.buy <= 0 || r.stock <= 0 || (r.buy as f64) * min_ratio > average as f64 || r.buy >= average {
                    continue;
                }
                let t = r.stock.min(cap);
                cands.push(Cand {
                    row: i,
                    price: r.buy,
                    average,
                    ratio: average as f64 / r.buy as f64,
                    quantity: r.stock,
                    extra: (average - r.buy) * t,
                    trade: r.buy * t,
                });
            }
        }
    }
    cands.sort_by_key(|c| std::cmp::Reverse(c.extra));
    cands.truncate(q.limit.clamp(1, 500));

    // 5. names and station details for the winners
    finish(&conn, shared, &rows, cands)
}

/// Whole-database scan for prices at least `min_ratio` (≥ 1.5) off their galactic average, read
/// through the anomaly indexes. Unpriced rows (fleet carriers) are compared with the average the
/// stations report for that commodity.
fn abnormal_prices(pool: &ReadPool, shared: &Shared, q: &DealQuery) -> Result<Vec<Deal>> {
    let cutoff = match q.max_age_hours {
        Some(h) if h.is_finite() && h > 0.0 => now_unix() - (h * 3600.0) as i64,
        _ => 1,
    };
    let filter = StationFilter {
        min_pad: match q.min_pad.and_then(Pad::from_char) {
            Some(Pad::Medium) => 2,
            Some(Pad::Large) => 3,
            _ => 0,
        },
        carriers: q.include_carriers,
        planetary: true,
        max_dist_ls: -1.0,
    };
    let radius = q.max_distance_ly.filter(|d| d.is_finite() && *d >= 0.0);
    // The WHERE clauses repeat the index conditions word for word, so SQLite uses the indexes.
    let (priced, unpriced) = match q.mode {
        TradeMode::SellToStation => (
            "SELECT market_id, commodity_id, buy, sell, mean, stock, demand, updated FROM market              WHERE mean > 0 AND sell > 0 AND demand > 0 AND sell * 2 >= mean * 3 AND updated >= ?1",
            "SELECT market_id, commodity_id, buy, sell, mean, stock, demand, updated FROM market              WHERE mean <= 0 AND sell > 0 AND demand > 0 AND updated >= ?1",
        ),
        TradeMode::BuyFromStation => (
            "SELECT market_id, commodity_id, buy, sell, mean, stock, demand, updated FROM market              WHERE mean > 0 AND buy > 0 AND stock > 0 AND buy * 3 <= mean * 2 AND updated >= ?1",
            "SELECT market_id, commodity_id, buy, sell, mean, stock, demand, updated FROM market              WHERE mean <= 0 AND buy > 0 AND stock > 0 AND updated >= ?1",
        ),
    };
    let conn = pool.get()?;
    let mut rows: Vec<Row> = Vec::new();
    {
        let idx = shared.stations.read();
        for sql in [priced, unpriced] {
            let mut stmt = conn.prepare_cached(sql)?;
            let mut it = stmt.query([cutoff])?;
            while let Some(r) = it.next()? {
                let market: i64 = r.get(0)?;
                // Station filters and distance from the in-memory index (no extra queries).
                let Some(s) = idx.get(market) else { continue };
                if !filter.passes(s) {
                    continue;
                }
                let d = match (q.origin, s.pos) {
                    (Some(o), Some(p)) => Some(dist(o, p)),
                    _ => None,
                };
                if let Some(rad) = radius
                    && !d.is_some_and(|d| d <= rad)
                {
                    continue;
                }
                rows.push(Row {
                    market,
                    dist: d,
                    cid: r.get(1)?,
                    buy: r.get(2)?,
                    sell: r.get(3)?,
                    mean: r.get(4)?,
                    stock: r.get(5)?,
                    demand: r.get(6)?,
                    updated: r.get(7)?,
                });
            }
        }
    }
    // Averages for unpriced rows, one lookup per commodity.
    let mut reference: FxHashMap<i64, Option<i64>> = FxHashMap::default();
    let cap = q.cargo_capacity.max(1);
    let mut cands: Vec<Cand> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let average = if r.mean > 0 {
            r.mean
        } else {
            let avg = match reference.get(&r.cid) {
                Some(a) => *a,
                None => {
                    let a = global_average(&conn, r.cid)?;
                    reference.insert(r.cid, a);
                    a
                }
            };
            match avg {
                Some(a) if a > 0 => a,
                _ => continue,
            }
        };
        match q.mode {
            TradeMode::SellToStation => {
                if (r.sell as f64) < average as f64 * q.min_ratio {
                    continue;
                }
                let t = r.demand.min(cap);
                cands.push(Cand { row: i, price: r.sell, average, ratio: r.sell as f64 / average as f64, quantity: r.demand, extra: (r.sell - average) * t, trade: r.sell * t });
            }
            TradeMode::BuyFromStation => {
                if (r.buy as f64) * q.min_ratio > average as f64 {
                    continue;
                }
                let t = r.stock.min(cap);
                cands.push(Cand { row: i, price: r.buy, average, ratio: average as f64 / r.buy as f64, quantity: r.stock, extra: (average - r.buy) * t, trade: r.buy * t });
            }
        }
    }
    cands.sort_by_key(|c| std::cmp::Reverse(c.extra));
    cands.truncate(q.limit.clamp(1, 500));
    finish(&conn, shared, &rows, cands)
}

/// Names and station details for the winning rows.
fn finish(conn: &Connection, shared: &Shared, rows: &[Row], cands: Vec<Cand>) -> Result<Vec<Deal>> {
    let commodities = shared.commodities.read();
    let mut out = Vec::with_capacity(cands.len());
    for c in cands {
        let r = &rows[c.row];
        let Some(st) = station(conn, r.market as u64)? else { continue };
        let (symbol, name) = commodities
            .meta(r.cid)
            .map(|m| (m.symbol.clone(), m.name.clone()))
            .unwrap_or_else(|| (format!("#{}", r.cid), format!("Commodity {}", r.cid)));
        out.push(Deal {
            commodity: symbol,
            commodity_name: name,
            price: c.price,
            average: c.average,
            ratio: c.ratio,
            quantity: c.quantity,
            extra_value: c.extra,
            trade_value: c.trade,
            station: st.name,
            system: st.system,
            market_id: r.market as u64,
            distance_ly: r.dist,
            max_pad: st.max_pad,
            is_carrier: st.is_carrier,
            carrier_access: crate::query::carrier_access(st.is_carrier, st.carrier_docking_access),
            updated_unix: r.updated,
        });
    }
    Ok(out)
}

/// The averages [`global_average`] samples: the commodity's range of the buyer and seller indexes.
pub(crate) const AVERAGE_SAMPLES: [&str; 2] = [
    "SELECT mean FROM market WHERE commodity_id = ?1 AND sell > 0 AND demand > 0 AND mean > 0 LIMIT 64",
    "SELECT mean FROM market WHERE commodity_id = ?1 AND buy > 0 AND stock > 0 AND mean > 0 LIMIT 64",
];

/// The most common average reported for a commodity anywhere (sampled through the buyer/seller
/// indexes, which cover only rows with demand or stock).
pub(crate) fn global_average(conn: &Connection, cid: i64) -> Result<Option<i64>> {
    let mut counts: FxHashMap<i64, u32> = FxHashMap::default();
    for sql in AVERAGE_SAMPLES {
        let mut stmt = conn.prepare_cached(sql)?;
        let mut rows = stmt.query([cid])?;
        while let Some(r) = rows.next()? {
            *counts.entry(r.get::<_, i64>(0)?).or_default() += 1;
        }
    }
    let best = counts.into_iter().max_by_key(|&(v, n)| (n, v)).map(|(v, _)| v);
    Ok(best)
}
