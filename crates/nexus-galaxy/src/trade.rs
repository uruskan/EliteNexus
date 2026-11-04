//! Single-hop trade routes near an origin.
//!
//! 1. Candidate stations come from the in-memory station index: within `radius_ly` of the origin,
//!    pad / carrier / planetary / arrival-distance filters, market newer than `max_age_hours`.
//! 2. Their market rows are loaded (in parallel over several read connections) and split per
//!    commodity into sellers (stock, buy price) and buyers (demand, sell price).
//! 3. Per commodity, sellers are visited cheapest first (at most [`MAX_SELLERS`]) and buyers best
//!    price first; the scan of a seller stops at the first buyer that is no longer profitable, after
//!    [`ROUTES_PER_SELLER`] valid buyers within the hop distance, or after [`MAX_BUYER_SCAN`] buyers.
//!    That bounds the work to `commodities x MAX_SELLERS x MAX_BUYER_SCAN` distance checks in the
//!    worst case; in practice a seller stops after a handful.
//! 4. Candidates are ranked by total profit, one route per (from, to) station pair.
//!
//! With `from_market`, that station is every commodity's only seller, so each of its goods may go to
//! as many buyers as the result holds: one seller per commodity is cheap to scan.

use crate::error::Result;
use crate::index::StationFilter;
use crate::model::*;
use crate::query::{MAX_RADIUS, ReadPool, station};
use crate::station::Pad;
use crate::util::{dist, dist2, now_unix};
use crate::writer::Shared;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

const MAX_SELLERS: usize = 120;
const MAX_BUYER_SCAN: usize = 600;
const ROUTES_PER_SELLER: usize = 3;
/// Stations per parallel loading task.
const LOAD_CHUNK: usize = 192;

#[derive(Clone, Copy)]
struct Offer {
    station: u32,
    price: i64,
    qty: i64,
}

#[derive(Default)]
struct Book {
    sellers: Vec<Offer>,
    buyers: Vec<Offer>,
}

struct Candidate {
    from: u32,
    to: u32,
    cid: i64,
    buy: i64,
    sell: i64,
    supply: i64,
    demand: i64,
    units: i64,
    total: i64,
    hop: f64,
}

pub(crate) fn trade_routes(
    pool: &ReadPool,
    shared: &Shared,
    q: &TradeQuery,
) -> Result<Vec<TradeRoute>> {
    let radius = q.radius_ly.clamp(0.0, MAX_RADIUS);
    let hop2 = q.max_hop_ly.max(0.0).powi(2);
    let cutoff = match q.max_age_hours {
        Some(h) if h.is_finite() && h > 0.0 => now_unix() - (h * 3600.0) as i64,
        _ => 1, // any market data at all
    };
    let filter = StationFilter {
        min_pad: match q.min_pad.and_then(Pad::from_char) {
            Some(Pad::Medium) => 2,
            Some(Pad::Large) => 3,
            _ => 0,
        },
        carriers: q.include_carriers,
        planetary: q.include_planetary,
        max_dist_ls: q.max_dist_ls.filter(|d| d.is_finite()).unwrap_or(-1.0),
    };

    // 1. candidate stations (the station routes start from passes the filters: the commander is there)
    let from_id = q.from_market.map(|m| m as i64);
    let stations: Vec<(i64, [f64; 3])> = {
        let idx = shared.stations.read();
        idx.iter()
            .filter_map(|(id, s)| {
                if s.n_commodities == 0
                    || s.market_updated < cutoff
                    || (Some(id) != from_id && !filter.passes(s))
                {
                    return None;
                }
                let p = s.pos?;
                (dist(q.origin, p) <= radius).then_some((id, p))
            })
            .collect()
    };
    if stations.len() < 2 {
        return Ok(Vec::new());
    }
    let from = match from_id {
        Some(id) => match stations.iter().position(|s| s.0 == id) {
            Some(i) => Some(i as u32),
            // Its market is unknown, or older than the age limit.
            None => return Ok(Vec::new()),
        },
        None => None,
    };
    let per_seller = if from.is_some() {
        q.limit.clamp(1, 1000)
    } else {
        ROUTES_PER_SELLER
    };

    // 2. market rows -> per-commodity books
    let ids: Vec<(u32, i64)> = stations
        .iter()
        .enumerate()
        .map(|(i, s)| (i as u32, s.0))
        .collect();
    let parts: Vec<Result<Vec<(i64, Offer, bool)>>> = ids
        .par_chunks(LOAD_CHUNK)
        .map(|chunk| {
            let conn = pool.get()?;
            let mut stmt = conn.prepare_cached(
                "SELECT commodity_id, buy, sell, stock, demand FROM market WHERE market_id = ?1",
            )?;
            let mut out = Vec::with_capacity(chunk.len() * 64);
            for &(idx, mid) in chunk {
                let mut rows = stmt.query([mid])?;
                while let Some(r) = rows.next()? {
                    let cid: i64 = r.get(0)?;
                    let (buy, sell, stock, demand): (i64, i64, i64, i64) =
                        (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
                    if buy > 0 && stock > 0 {
                        out.push((
                            cid,
                            Offer {
                                station: idx,
                                price: buy,
                                qty: stock,
                            },
                            true,
                        ));
                    }
                    if sell > 0 && demand > 0 {
                        out.push((
                            cid,
                            Offer {
                                station: idx,
                                price: sell,
                                qty: demand,
                            },
                            false,
                        ));
                    }
                }
            }
            Ok(out)
        })
        .collect();
    let mut books: FxHashMap<i64, Book> = FxHashMap::default();
    for part in parts {
        for (cid, offer, is_seller) in part? {
            let b = books.entry(cid).or_default();
            if is_seller {
                b.sellers.push(offer);
            } else {
                b.buyers.push(offer);
            }
        }
    }

    // 3. per-commodity bounded scan
    let cargo = q.cargo_capacity.max(1);
    let capital = q.capital.filter(|c| *c > 0);
    let min_profit = q.min_profit_per_ton.max(1);
    let mut cands: Vec<Candidate> = books
        .par_iter_mut()
        .flat_map_iter(|(&cid, book)| {
            let mut found = Vec::new();
            if let Some(f) = from {
                book.sellers.retain(|o| o.station == f);
            }
            if book.sellers.is_empty() || book.buyers.is_empty() {
                return found.into_iter();
            }
            book.sellers.sort_unstable_by_key(|o| o.price);
            book.buyers
                .sort_unstable_by_key(|o| std::cmp::Reverse(o.price));
            if book.buyers[0].price - book.sellers[0].price < min_profit {
                return found.into_iter();
            }
            for s in book.sellers.iter().take(MAX_SELLERS) {
                let mut n = 0;
                for b in book.buyers.iter().take(MAX_BUYER_SCAN) {
                    let profit = b.price - s.price;
                    if profit < min_profit {
                        break;
                    }
                    if b.station == s.station {
                        continue;
                    }
                    let d2 = dist2(
                        stations[s.station as usize].1,
                        stations[b.station as usize].1,
                    );
                    if d2 > hop2 {
                        continue;
                    }
                    let mut units = cargo.min(s.qty).min(b.qty);
                    if let Some(c) = capital {
                        units = units.min(c / s.price.max(1));
                    }
                    if units <= 0 {
                        continue;
                    }
                    found.push(Candidate {
                        from: s.station,
                        to: b.station,
                        cid,
                        buy: s.price,
                        sell: b.price,
                        supply: s.qty,
                        demand: b.qty,
                        units,
                        total: units.saturating_mul(profit),
                        hop: d2.sqrt(),
                    });
                    n += 1;
                    if n >= per_seller {
                        break;
                    }
                }
            }
            found.into_iter()
        })
        .collect();

    // 4. rank, one route per station pair
    cands.sort_unstable_by(|a, b| {
        b.total
            .cmp(&a.total)
            .then((b.sell - b.buy).cmp(&(a.sell - a.buy)))
            .then(a.hop.total_cmp(&b.hop))
    });
    let limit = q.limit.clamp(1, 1000);
    let mut seen = FxHashSet::default();
    let mut picked = Vec::with_capacity(limit);
    for c in cands {
        if seen.insert((c.from, c.to)) {
            picked.push(c);
            if picked.len() >= limit {
                break;
            }
        }
    }

    // details of the stations that made it
    let conn = pool.get()?;
    let mut details: FxHashMap<u32, TradeStation> = FxHashMap::default();
    for c in &picked {
        for i in [c.from, c.to] {
            if details.contains_key(&i) {
                continue;
            }
            let (mid, pos) = stations[i as usize];
            if let Some(st) = station(&conn, mid as u64)? {
                details.insert(i, trade_station(st, pos, q.origin));
            }
        }
    }
    let names = shared.commodities.read();
    let mut out = Vec::with_capacity(picked.len());
    for c in picked {
        let (Some(from), Some(to)) = (details.get(&c.from), details.get(&c.to)) else {
            continue;
        };
        let (sym, name) = match names.meta(c.cid) {
            Some(m) => (m.symbol.clone(), m.name.clone()),
            None => (format!("#{}", c.cid), format!("#{}", c.cid)),
        };
        out.push(TradeRoute {
            from: from.clone(),
            to: to.clone(),
            commodity: sym,
            commodity_name: name,
            buy_price: c.buy,
            sell_price: c.sell,
            profit_per_ton: c.sell - c.buy,
            supply: c.supply,
            demand: c.demand,
            units: c.units,
            total_profit: c.total,
            hop_ly: c.hop,
        });
    }
    Ok(out)
}

fn trade_station(st: StationInfo, pos: [f64; 3], origin: [f64; 3]) -> TradeStation {
    TradeStation {
        market_id: st.market_id,
        station: st.name,
        system: st.system,
        system_id64: st.system_id64,
        pos,
        distance_from_origin_ly: dist(origin, pos),
        station_type: st.station_type,
        dist_ls: st.dist_ls,
        max_pad: st.max_pad,
        is_carrier: st.is_carrier,
        is_planetary: st.is_planetary,
        market_updated_unix: st.market_updated_unix,
    }
}
