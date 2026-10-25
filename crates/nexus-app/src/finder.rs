//! Market lookups used by other pages: suppliers for colonisation depots, buyers for the cargo
//! hold. Plain functions over the galaxy database; pages run them in background jobs. And, from the
//! journal alone, whether a pinned fleet carrier is in this system (the Market page and the overlay).

use nexus_galaxy::{CommodityHit, CommodityQuery, GalaxyDb, Sort, TradeMode};
use rustc_hash::FxHashMap;

/// One thing to buy or sell: (symbol, display name, tonnes).
pub type Line = (String, String, i64);

pub struct Supplier {
    pub name: String,
    /// Nearest stations with enough stock for a full hold (or for what is left), nearest first.
    pub hits: Vec<CommodityHit>,
}

/// Nearest sellers for each commodity a construction site still needs.
pub fn suppliers(db: &GalaxyDb, origin: [f64; 3], needs: &[Line], min_pad: Option<char>, hold: i64, max_age_hours: f64) -> Result<Vec<Supplier>, String> {
    needs
        .iter()
        .map(|(symbol, name, needed)| {
            let q = CommodityQuery {
                commodity: symbol.clone(),
                mode: TradeMode::BuyFromStation,
                origin: Some(origin),
                min_pad,
                max_age_hours: Some(max_age_hours),
                min_quantity: (*needed).min(hold.max(1)),
                sort: Sort::Nearest,
                limit: 3,
                ..Default::default()
            };
            let hits = db.search_commodity(&q).map_err(|e| e.to_string())?;
            Ok(Supplier { name: name.clone(), hits })
        })
        .collect()
}

/// A station that buys some or all of the hold.
pub struct SellStop {
    pub hit: CommodityHit,
    /// Credits for everything it buys (limited by its demand).
    pub total: i64,
    /// (name, tonnes it takes, price).
    pub lines: Vec<(String, i64, i64)>,
}

pub struct SellPlan {
    /// Best single stations, most credits first.
    pub stops: Vec<SellStop>,
    /// Best buyers per commodity.
    pub per_item: Vec<(String, i64, Vec<CommodityHit>)>,
}

/// Where to sell the hold: the best buyers of each commodity within `radius_ly`, and the stations
/// that pay the most for everything together.
pub fn sell_plan(db: &GalaxyDb, origin: [f64; 3], hold: &[Line], min_pad: Option<char>, radius_ly: f64, max_age_hours: f64) -> Result<SellPlan, String> {
    let mut per_item = Vec::with_capacity(hold.len());
    let mut stops: FxHashMap<u64, SellStop> = FxHashMap::default();
    for (symbol, name, count) in hold {
        let q = CommodityQuery {
            commodity: symbol.clone(),
            mode: TradeMode::SellToStation,
            origin: Some(origin),
            max_distance_ly: Some(radius_ly),
            min_pad,
            max_age_hours: Some(max_age_hours),
            min_quantity: 1,
            sort: Sort::BestPrice,
            limit: 60,
            ..Default::default()
        };
        let hits = db.search_commodity(&q).map_err(|e| e.to_string())?;
        for h in &hits {
            let tons = (*count).min(h.quantity.max(0));
            if tons <= 0 {
                continue;
            }
            let stop = stops.entry(h.market_id).or_insert_with(|| SellStop { hit: h.clone(), total: 0, lines: Vec::new() });
            stop.total += tons * h.price;
            stop.lines.push((name.clone(), tons, h.price));
        }
        per_item.push((name.clone(), *count, hits.into_iter().take(3).collect()));
    }
    let mut stops: Vec<SellStop> = stops.into_values().collect();
    stops.sort_by_key(|s| std::cmp::Reverse(s.total));
    stops.truncate(15);
    Ok(SellPlan { stops, per_item })
}

/// The ship's landing pad requirement ('S', 'M' or 'L').
pub fn ship_pad(gs: &nexus_model::GameState) -> Option<char> {
    gs.fleet.current().map(|s| nexus_data::ships::pad_size(&s.ship_type))
}

/// Sellable cargo (not mission cargo, not stolen, no limpets).
pub fn sellable_hold(gs: &nexus_model::GameState) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    for c in gs.inventory.cargo.items.iter().filter(|c| c.mission_id.is_none() && c.symbol != "drones") {
        let n = c.count - c.stolen;
        if n <= 0 {
            continue;
        }
        match lines.iter_mut().find(|l| l.0 == c.symbol) {
            Some(l) => l.2 += n,
            None => lines.push((c.symbol.clone(), c.name.clone(), n)),
        }
    }
    lines
}

/// Fleet carriers the game has listed in the current system since you arrived: (signal name, callsign).
/// The game logs a carrier's signal when it shows up in your navigation panel: on arrival, or
/// when you drop out of supercruise near it.
pub fn carriers_here(gs: &nexus_model::GameState) -> Vec<(String, String)> {
    let Some(sys) = gs.exploration.systems.get(&gs.location.system_address) else { return Vec::new() };
    sys.signals
        .iter()
        .filter(|s| s.kind.as_deref() == Some("FleetCarrier") && s.ts >= gs.location.last_arrival)
        .map(|s| (s.name.clone(), callsign(&s.name)))
        .collect()
}

/// A carrier signal's callsign: its last word (`WEDDY BLIRTNAUT H1X-L0L` → `H1X-L0L`).
pub fn callsign(signal: &str) -> String {
    signal.split_whitespace().last().unwrap_or_default().to_ascii_uppercase()
}

/// Whether a carrier is in the current system, from what the game listed since you arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Here,
    /// The game listed this many carriers here, and not this one.
    NotHere(usize),
    /// No carrier listed yet this visit.
    Unknown,
}

/// Whether the carrier with the callsign `station` is among those `listed` (see [`carriers_here`]).
pub fn presence(listed: &[(String, String)], station: &str) -> Presence {
    let cs = station.trim().to_ascii_uppercase();
    if listed.iter().any(|(_, c)| *c == cs) {
        Presence::Here
    } else if listed.is_empty() {
        Presence::Unknown
    } else {
        Presence::NotHere(listed.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TempDb, good, station};
    use serde_json::json;

    #[test]
    fn sell_plan_and_suppliers() {
        let t = TempDb::new("finder");
        let db = t.db();
        // Alpha pays most for gold but only wants 80 t; Beta takes everything.
        station(db, ("Alpha", 1001, 0.0), "Alpha Port", 5001, json!([good("gold", 0, 0, 60_000, 80), good("steel", 300, 100, 0, 0)]));
        station(db, ("Beta", 1002, 10.0), "Beta Hub", 5002, json!([good("gold", 0, 0, 55_000, 1000), good("silver", 0, 0, 5_000, 1000), good("steel", 320, 10_000, 0, 0)]));
        db.flush().unwrap();

        let hold = vec![("gold".to_string(), "Gold".to_string(), 100), ("silver".to_string(), "Silver".to_string(), 50)];
        let plan = sell_plan(db, [0.0; 3], &hold, None, 50.0, 24.0).unwrap();
        assert_eq!(plan.stops.len(), 2);
        assert_eq!(plan.stops[0].hit.station, "Beta Hub");
        assert_eq!(plan.stops[0].total, 100 * 55_000 + 50 * 5_000);
        assert_eq!(plan.stops[1].total, 80 * 60_000, "limited by Alpha's demand");
        assert_eq!(plan.per_item[0].2[0].station, "Alpha Port", "best gold price");

        // 500 t of steel with a 200 t hold: Alpha's 100 t is not enough, Beta is the nearest real supplier.
        let s = suppliers(db, [0.0; 3], &[("steel".to_string(), "Steel".to_string(), 500)], None, 200, 24.0).unwrap();
        assert_eq!(s[0].hits[0].station, "Beta Hub");
    }
}
