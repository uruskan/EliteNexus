//! The assistant's tools: thin wrappers over the app's own data (game model, galaxy database,
//! journal, Spansh). Each returns compact JSON for the model.
//!
//! [`TOOLS`] lists every tool once: its name, description and arguments, the words Ask shows while it runs and under
//! an answer, and who it's offered to (Ask's model, AI agents over MCP). [`fit`] keeps a reply within the length the
//! caller allows ([`NexusTools::budget`]).

use crate::finder;
use crate::pages::navigation::{PlannedRoute, from_spansh};
use nexus_ai::{ToolHost, ToolSpec};
use nexus_data::bio_predict::{self as bp, BodyFacts, ParentRef, Prediction, StarFacts, SystemStar};
use nexus_data::names::compact_credits;
use nexus_galaxy::{
    BodyInfo, BodyUpdate, CommodityQuery, ConflictQuery, ConflictStatus, DealQuery, GalaxyDb, HotspotQuery, Ingest, MaterialTrader, Seen, Services, Sort,
    Source, StationQuery, TechBroker, TradeMode, TradeQuery,
};
use nexus_journal::JournalIndex;
use nexus_model::exploration::{Body as OwnBody, SystemScan};
use nexus_model::{GameState, Model};
use nexus_spansh::{
    ExobiologyRouteRequest, GalaxyRouteRequest, NeutronRouteRequest, RichesRouteRequest, RouteWaypoints, ScanSystem, ShipProfile, Spansh, dump,
};
use parking_lot::{Mutex, RwLock};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

mod bodies;
mod buy;
mod cmdr;
mod engineering;

pub struct NexusTools {
    pub model: Arc<RwLock<Model>>,
    pub db: Option<GalaxyDb>,
    pub index: Arc<RwLock<JournalIndex>>,
    pub spansh: Spansh,
    pub rt: Handle,
    /// Routes the assistant planned, picked up by the Navigation page.
    pub routes: Arc<Mutex<Option<PlannedRoute>>>,
    /// Who calls: decides which tools are offered.
    pub caller: Caller,
    /// The longest reply the caller takes, in characters of JSON (usually [`Caller::budget`]): tools that could say
    /// more give up detail to fit, and say what they left out.
    pub budget: usize,
}

/// Who calls the tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    /// The built-in assistant, the Ask page.
    Ask,
    /// An AI agent on this computer, over MCP (`crate::mcp`).
    Mcp,
}

impl Caller {
    /// The longest reply this caller takes. Ask passes the model the first 12,000 characters of a tool result
    /// (`nexus_ai::agent::MAX_RESULT_CHARS`) and cuts the rest mid-JSON; an agent takes more (Claude Code warns above
    /// 10,000 tokens of tool output), so it gets more of a long answer while staying under that.
    pub fn budget(self) -> usize {
        match self {
            Caller::Ask => nexus_ai::agent::MAX_RESULT_CHARS - 500,
            Caller::Mcp => 30_000,
        }
    }
}

/// The length of `v` as the compact JSON a caller is sent, without building the text.
pub(crate) fn json_len(v: &Value) -> usize {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0 += b.len();
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut n = Count(0);
    let _ = serde_json::to_writer(&mut n, v);
    n.0
}

/// Keeps a reply within `budget` characters of JSON. `make(level)` builds the reply at a level of detail, 0 the most
/// detailed and each level no longer than the one before; `levels` says how many there are (at least 1). Returns the
/// most detailed reply that fits, with its level, or the last level when none does. Only a few levels are built:
/// the first, the last, and a binary search between them.
pub(crate) fn fit(budget: usize, levels: usize, mut make: impl FnMut(usize) -> Value) -> (Value, usize) {
    let first = make(0);
    if levels <= 1 || json_len(&first) <= budget {
        return (first, 0);
    }
    let last_level = levels - 1;
    let last = make(last_level);
    if json_len(&last) > budget {
        return (last, last_level);
    }
    // `lo` doesn't fit, `hi` does.
    let (mut lo, mut hi, mut best) = (0, last_level, last);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        let v = make(mid);
        if json_len(&v) <= budget {
            (hi, best) = (mid, v);
        } else {
            lo = mid;
        }
    }
    (best, hi)
}

/// The commander facts the tools need, read under one short lock.
struct Me {
    pos: Option<[f64; 3]>,
    system: String,
    pad: Option<char>,
    cargo_capacity: i64,
    credits: i64,
    jump_range: f64,
    loadout: Option<String>,
}

fn arg_str(a: &Value, k: &str) -> Option<String> {
    a.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn arg_f64(a: &Value, k: &str) -> Option<f64> {
    a.get(k).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).filter(|v| v.is_finite())
}

fn arg_bool(a: &Value, k: &str) -> Option<bool> {
    a.get(k).and_then(Value::as_bool)
}

fn hours_ago(unix: i64) -> f64 {
    let h = (nexus_journal::Timestamp::now().0 - unix).max(0) as f64 / 3600.0;
    (h * 10.0).round() / 10.0
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

const NEAR: &str = "System to search around; default: the commander's current system.";

impl NexusTools {
    fn db(&self) -> Result<&GalaxyDb, String> {
        self.db.as_ref().ok_or_else(|| "The market database is unavailable.".to_string())
    }

    fn me(&self) -> Result<Me, String> {
        let model = self.model.read();
        let gs = model.active().ok_or("No commander loaded yet.")?;
        let ship = gs.fleet.current();
        Ok(Me {
            pos: gs.location.star_pos,
            system: gs.location.system.clone(),
            pad: finder::ship_pad(gs),
            cargo_capacity: ship.map(|s| i64::from(s.cargo_capacity)).filter(|c| *c > 0).unwrap_or(0),
            credits: gs.cmdr.credits,
            jump_range: ship.map(|s| s.max_jump_range).unwrap_or(0.0),
            loadout: ship.and_then(|s| s.loadout_json.clone()),
        })
    }

    /// Position and name of `near_system`, or the commander's, for a tool answering from local data: the local database,
    /// then for Ask only, Spansh. An agent's call stays local, as the tool's `online: false` says, so it counts as one.
    fn origin(&self, args: &Value, me: &Me) -> Result<([f64; 3], String), String> {
        self.locate(args, me, self.caller == Caller::Ask)
    }

    /// [`Self::origin`] for a tool that asks Spansh anyway (`online: true`): any caller may look the system up there.
    fn origin_online(&self, args: &Value, me: &Me) -> Result<([f64; 3], String), String> {
        self.locate(args, me, true)
    }

    fn locate(&self, args: &Value, me: &Me, spansh: bool) -> Result<([f64; 3], String), String> {
        if let Some(name) = arg_str(args, "near_system").filter(|n| !n.eq_ignore_ascii_case(&me.system)) {
            if let Some(db) = &self.db
                && let Ok(Some(s)) = db.system_by_name(&name)
                && let Some(p) = s.pos
            {
                return Ok((p, s.name));
            }
            if !spansh {
                return Err(format!("\"{name}\" isn't in Elite Nexus' galaxy database, which this tool answers from: give a system near it that has markets, or leave near_system out to search around the commander."));
            }
            let found = match self.rt.block_on(async { tokio::time::timeout(SPANSH_TIMEOUT, self.spansh.system_by_name(&name)).await }) {
                Ok(r) => r.map_err(|e| format!("Spansh: {e}"))?,
                Err(_) => return Err("Spansh: no answer in time".into()),
            };
            return found.map(|s| ([s.x, s.y, s.z], s.name)).ok_or_else(|| format!("Unknown system \"{name}\"."));
        }
        me.pos.map(|p| (p, me.system.clone())).ok_or_else(|| "The commander's position is not known yet.".to_string())
    }

    fn commander(&self) -> Result<Value, String> {
        let model = self.model.read();
        let gs: &GameState = model.active().ok_or("No commander loaded yet.")?;
        let loc = &gs.location;
        let ship = gs.fleet.current();
        let cargo: Vec<Value> = gs.inventory.cargo.items.iter().map(|c| json!({ "name": c.name, "tonnes": c.count, "stolen": c.stolen, "mission": c.mission_id.is_some() })).collect();
        // Their own carrier and their squadron's, while they still have them.
        let now = nexus_journal::Timestamp::now();
        let carrier_json = |c: &nexus_model::carrier::Carrier| {
            c.active(now).then(|| {
                json!({
                    "name": c.name, "callsign": c.callsign, "system": c.system,
                    "tritium_t": c.fuel, "cargo_t": c.space.cargo, "capacity_t": c.space.total_capacity,
                    "decommission_scrap_time": c.decommission.map(|d| d.scrap_time.to_iso()),
                })
            })
        };
        let carrier = carrier_json(&gs.carrier);
        let squadron_carrier = carrier_json(&gs.squadron_carrier);
        Ok(json!({
            "commander": gs.cmdr.name,
            "credits": gs.cmdr.credits,
            "credits_text": compact_credits(gs.cmdr.credits),
            "location": {
                "system": loc.system, "coords": loc.star_pos, "body": loc.body,
                "station": loc.station.as_ref().filter(|_| loc.docked).map(|s| &s.name),
                "situation": loc.situation(),
                "population": loc.info.population, "economy": loc.info.economy, "security": loc.info.security,
                "controlling_faction": loc.info.controlling_faction,
            },
            "ship": ship.map(|s| json!({
                "type": s.type_name, "name": s.name, "landing_pad": finder::ship_pad(gs).map(String::from),
                "cargo_capacity_t": s.cargo_capacity, "cargo_used_t": gs.inventory.cargo.total(),
                "max_jump_range_ly": r1(s.max_jump_range), "fuel_t": r1(gs.fleet.fuel_main), "fuel_capacity_t": s.fuel_main_capacity,
                "rebuy": s.rebuy,
            })),
            "cargo": cargo,
            "fleet_carrier": carrier,
            "squadron_carrier": squadron_carrier,
            "ranks": nexus_data::ranks::RankKind::ALL.iter().map(|k| json!({ "rank": format!("{k:?}"), "name": gs.cmdr.rank_name(*k) })).collect::<Vec<_>>(),
            "unsold_exploration_data_cr": gs.exploration.unsold_value(),
            "unsold_exobiology_data_cr": gs.exobio.unsold_value(),
        }))
    }

    fn find_commodity(&self, a: &Value) -> Result<Value, String> {
        let me = self.me()?;
        let (origin, near) = self.origin(a, &me)?;
        let commodity = arg_str(a, "commodity").ok_or("commodity is required")?;
        let sell = arg_str(a, "mode").is_some_and(|m| m.eq_ignore_ascii_case("sell"));
        let q = CommodityQuery {
            commodity: commodity.clone(),
            mode: if sell { TradeMode::SellToStation } else { TradeMode::BuyFromStation },
            origin: Some(origin),
            max_distance_ly: Some(arg_f64(a, "max_distance_ly").unwrap_or(100.0)),
            min_pad: me.pad,
            max_age_hours: Some(arg_f64(a, "max_age_hours").unwrap_or(72.0)),
            min_quantity: arg_f64(a, "min_quantity").map(|v| v as i64).unwrap_or(1),
            include_carriers: arg_bool(a, "include_carriers").unwrap_or(true),
            include_planetary: true,
            limit: arg_f64(a, "limit").map(|v| v as usize).unwrap_or(8).clamp(1, 25),
            sort: if arg_str(a, "sort").is_some_and(|s| s.eq_ignore_ascii_case("distance")) { Sort::Nearest } else { Sort::BestPrice },
        };
        let hits = self.db()?.search_commodity(&q).map_err(|e| e.to_string())?;
        Ok(json!({
            "commodity": commodity, "near": near, "mode": if sell { "stations buying (you sell)" } else { "stations selling (you buy)" },
            "results": hits.iter().map(|h| json!({
                "station": h.station, "system": h.system, "distance_ly": h.distance_ly.map(r1), "price": h.price,
                "quantity": h.quantity, "pad": h.max_pad.map(String::from), "fleet_carrier": h.is_carrier,
                "arrival_distance_ls": h.dist_ls.map(|d| d.round()), "age_hours": hours_ago(h.updated_unix),
            })).collect::<Vec<_>>(),
        }))
    }

    fn untouched_biology(&self, a: &Value) -> Result<Value, String> {
        let me = self.me()?;
        let (origin, near) = self.origin_online(a, &me)?;
        let species = match arg_str(a, "species") {
            Some(n) => nexus_data::exobio::species(&n).ok_or_else(|| format!("Unknown species \"{n}\"."))?,
            None => nexus_data::exobio::species("Stratum Tectonicas").ok_or("Species data missing.")?,
        };
        let include_mapped = arg_bool(a, "include_mapped").unwrap_or(true);
        let limit = arg_f64(a, "limit").map(|v| v as usize).unwrap_or(10).clamp(1, 25);
        let q = crate::untouched::Query { species, origin, radius_ly: arg_f64(a, "max_distance_ly").unwrap_or(500.0).clamp(20.0, 5000.0), limit: limit * 4 };
        let db = self.db.clone();
        let footfalled = move |name: &str| {
            db.as_ref().and_then(|d| d.body_by_name(name).ok().flatten()).is_some_and(|b| b.was_footfalled.is_some_and(|s| s.value))
        };
        // Up to four pages and a search for populated systems: one limit for all of them, so a slow Spansh can't hold an
        // agent's worker long after it was told the call didn't finish.
        let (mut list, st) = self.spansh_search(Duration::from_secs(90), crate::untouched::find(&self.spansh, &q, footfalled))?;
        let mapped_never_sampled = list.iter().filter(|c| c.evidence == crate::untouched::Evidence::GenusConfirmed).count();
        list.retain(|c| include_mapped || c.evidence == crate::untouched::Evidence::NoTrace);
        list.truncate(limit);
        Ok(json!({
            "near": near,
            "species": species.name,
            "value_cr": species.value,
            "value_if_first_logged_cr": bp::payout(species.value, true),
            "radius_ly": q.radius_ly,
            "how_to_read": "no_trace = no commander who shares data (EDMC, EDDiscovery, …) has DSS-mapped or sampled the planet; mapped_never_sampled = someone DSS-mapped it (the genus is confirmed there) but no samples are on record. A sample only leaves a record when the tool uploads it and it added a codex entry for that commander, so someone may have sampled without a trace: both are leads, not guarantees, and no_trace is the better bet. Populated systems and planets your data says were footfalled are already left out.",
            "searched": { "matching_planets": st.matched, "examined_nearest": st.examined, "examined_out_to_ly": r1(st.reach_ly), "already_sampled": st.sampled, "mapped_without_this_genus": st.mapped_without, "mapped_never_sampled": mapped_never_sampled, "populated_system": st.populated, "footfalled": st.footfalled, "conditions_rule_it_out": st.ruled_out },
            "candidates": list.iter().map(|c| json!({
                "system": c.system, "body": c.body, "distance_ly": r1(c.distance_ly), "distance_ls": c.distance_ls.map(|d| d.round()),
                "evidence": match c.evidence { crate::untouched::Evidence::NoTrace => "no_trace", crate::untouched::Evidence::GenusConfirmed => "mapped_never_sampled" },
                "chance_pct": (c.chance * 100.0).round(), "atmosphere": c.atmosphere, "temperature_k": c.temperature_k.map(f64::round),
                "gravity_g": c.gravity_g.map(|g| (g * 100.0).round() / 100.0), "bio_signals": c.bio_signals,
                "last_data_days": c.last_data.map(|t| (hours_ago(t) / 24.0).round()),
            })).collect::<Vec<_>>(),
        }))
    }

    fn price_deals(&self, a: &Value) -> Result<Value, String> {
        let me = self.me()?;
        let (origin, near) = self.origin(a, &me)?;
        let buy = arg_str(a, "mode").is_some_and(|m| m.eq_ignore_ascii_case("buy"));
        let cargo = me.cargo_capacity.max(1);
        let q = DealQuery {
            mode: if buy { TradeMode::BuyFromStation } else { TradeMode::SellToStation },
            origin: Some(origin),
            // 0 = anywhere: the whole-database abnormal-price scan.
            max_distance_ly: Some(arg_f64(a, "max_distance_ly").unwrap_or(80.0)).filter(|d| *d > 0.0),
            min_ratio: arg_f64(a, "min_ratio").unwrap_or(1.3),
            min_pad: me.pad,
            include_carriers: arg_bool(a, "include_carriers").unwrap_or(true),
            max_age_hours: Some(arg_f64(a, "max_age_hours").unwrap_or(48.0).clamp(1.0, 720.0)),
            cargo_capacity: cargo,
            limit: arg_f64(a, "limit").map(|v| v as usize).unwrap_or(10).clamp(1, 25),
        };
        let deals = self.db()?.price_deals(&q).map_err(|e| e.to_string())?;
        Ok(json!({
            "near": near, "cargo_capacity_t": cargo,
            "mode": if buy { "stations selling far below the galactic average" } else { "stations paying far above the galactic average" },
            "deals": deals.iter().map(|d| json!({
                "commodity": d.commodity_name, "station": d.station, "system": d.system, "distance_ly": d.distance_ly.map(r1),
                "price": d.price, "galactic_average": d.average, "percent_better": ((d.ratio - 1.0) * 100.0).round(),
                "quantity": d.quantity, "credits_better_than_average_per_hold": d.extra_value, "credits_for_one_hold": d.trade_value,
                "pad": d.max_pad.map(String::from), "fleet_carrier": d.is_carrier, "age_hours": hours_ago(d.updated_unix),
            })).collect::<Vec<_>>(),
        }))
    }

    fn sell_my_cargo(&self, a: &Value) -> Result<Value, String> {
        let me = self.me()?;
        let (origin, near) = self.origin(a, &me)?;
        let hold = {
            let model = self.model.read();
            finder::sellable_hold(model.active().ok_or("No commander loaded yet.")?)
        };
        if hold.is_empty() {
            return Ok(json!({ "message": "The hold has no sellable cargo (mission cargo, stolen goods and limpets are left out)." }));
        }
        let plan = finder::sell_plan(self.db()?, origin, &hold, me.pad, arg_f64(a, "max_distance_ly").unwrap_or(60.0), 72.0)?;
        Ok(json!({
            "near": near,
            "hold": hold.iter().map(|(_, n, t)| json!({ "name": n, "tonnes": t })).collect::<Vec<_>>(),
            "best_single_stops": plan.stops.iter().take(5).map(|s| json!({
                "station": s.hit.station, "system": s.hit.system, "distance_ly": s.hit.distance_ly.map(r1), "total_credits": s.total,
                "buys": s.lines.iter().map(|(n, t, p)| json!({ "name": n, "tonnes": t, "price": p })).collect::<Vec<_>>(),
                "fleet_carrier": s.hit.is_carrier, "age_hours": hours_ago(s.hit.updated_unix),
            })).collect::<Vec<_>>(),
            "best_buyer_per_item": plan.per_item.iter().map(|(n, t, hits)| json!({
                "name": n, "tonnes": t,
                "best": hits.first().map(|h| json!({ "station": h.station, "system": h.system, "price": h.price, "demand": h.quantity, "distance_ly": h.distance_ly.map(r1) })),
            })).collect::<Vec<_>>(),
        }))
    }

    fn trade_routes(&self, a: &Value) -> Result<Value, String> {
        let me = self.me()?;
        let (origin, near) = self.origin(a, &me)?;
        let q = TradeQuery {
            origin,
            radius_ly: arg_f64(a, "radius_ly").unwrap_or(40.0),
            max_hop_ly: arg_f64(a, "max_hop_ly").unwrap_or(25.0),
            min_pad: me.pad,
            cargo_capacity: me.cargo_capacity.max(1),
            capital: Some(me.credits.max(0)),
            max_age_hours: Some(48.0),
            include_carriers: false,
            include_planetary: true,
            limit: arg_f64(a, "limit").map(|v| v as usize).unwrap_or(6).clamp(1, 20),
            ..Default::default()
        };
        let routes = self.db()?.trade_routes(&q).map_err(|e| e.to_string())?;
        Ok(json!({
            "near": near, "cargo_capacity_t": q.cargo_capacity,
            "routes": routes.iter().map(|r| json!({
                "commodity": r.commodity_name, "profit_per_ton": r.profit_per_ton, "tonnes": r.units, "profit_per_trip": r.total_profit,
                "buy_at": { "station": r.from.station, "system": r.from.system, "price": r.buy_price, "distance_ly": r1(r.from.distance_from_origin_ly), "age_hours": hours_ago(r.from.market_updated_unix) },
                "sell_at": { "station": r.to.station, "system": r.to.system, "price": r.sell_price, "distance_ly": r1(r.to.distance_from_origin_ly), "age_hours": hours_ago(r.to.market_updated_unix) },
                "hop_ly": r1(r.hop_ly),
            })).collect::<Vec<_>>(),
        }))
    }

    fn nearest_service(&self, a: &Value) -> Result<Value, String> {
        let me = self.me()?;
        let (origin, near) = self.origin(a, &me)?;
        let service = arg_str(a, "service").ok_or("service is required")?;
        let mut q = StationQuery { origin, min_pad: me.pad, limit: arg_f64(a, "limit").map(|v| v as usize).unwrap_or(5).clamp(1, 15), ..Default::default() };
        match service.as_str() {
            "raw_material_trader" => q.material_trader = Some(MaterialTrader::Raw),
            "manufactured_material_trader" => q.material_trader = Some(MaterialTrader::Manufactured),
            "encoded_material_trader" => q.material_trader = Some(MaterialTrader::Encoded),
            "human_tech_broker" => q.tech_broker = Some(TechBroker::Human),
            "guardian_tech_broker" => q.tech_broker = Some(TechBroker::Guardian),
            "interstellar_factors" => q.services = Services::INTERSTELLAR_FACTORS,
            "universal_cartographics" => q.services = Services::UNIVERSAL_CARTOGRAPHICS,
            "vista_genomics" => q.services = Services::VISTA_GENOMICS,
            "shipyard" => q.services = Services::SHIPYARD,
            "outfitting" => q.services = Services::OUTFITTING,
            "black_market" => q.services = Services::BLACK_MARKET,
            "refuel" => q.services = Services::REFUEL,
            "repair" => q.services = Services::REPAIR,
            other => return Err(format!("Unknown service \"{other}\".")),
        }
        let hits = self.db()?.stations_near(&q).map_err(|e| e.to_string())?;
        Ok(json!({
            "service": service, "near": near,
            "stations": hits.iter().map(|h| json!({
                "station": h.station.name, "system": h.station.system, "distance_ly": r1(h.distance_ly),
                "arrival_distance_ls": h.station.dist_ls.map(|d| d.round()), "pad": h.station.max_pad.map(String::from),
                "type": h.station.station_type,
            })).collect::<Vec<_>>(),
            "note": "Material trader and technology broker types are inferred from the station's economy.",
        }))
    }

    fn mining_sites(&self, a: &Value) -> Result<Value, String> {
        let model = self.model.read();
        let gs: &GameState = model.active().ok_or("No commander loaded yet.")?;
        let want = arg_str(a, "commodity").map(|c| c.to_lowercase());
        let limit = arg_f64(a, "limit").map(|v| v as usize).unwrap_or(10).clamp(1, 50);
        let mut sites: Vec<_> =
            gs.mining.sites.values().filter(|s| want.as_ref().is_none_or(|w| s.refined.keys().any(|k| k.to_lowercase().contains(w.as_str())))).collect();
        sites.sort_by_key(|s| std::cmp::Reverse(s.last));
        Ok(json!({
            "sites": sites.iter().take(limit).map(|s| json!({
                "system": s.system, "body": s.body, "mining_location": s.key.location,
                "refined_t": s.refined, "visits": s.visits, "last_visit": s.last.to_iso(),
            })).collect::<Vec<_>>(),
            "note": "mining_location is the number the game's navigation panel shows (Planetary Mining Location N): the location the game named nearest to where the ship landed or lifted off, or for a Rhino dropped from a hovering ship the one targeted then (recorded only while Elite Nexus ran); null when none is known. To go back: jump to the system, target the body, then pick that location in the navigation panel; the Mining page's radar shows the deposits. Rigs and deposit sizes aren't in the journal.",
        }))
    }

    fn mining_hotspots(&self, a: &Value) -> Result<Value, String> {
        let me = self.me()?;
        let (origin, near) = self.origin(a, &me)?;
        let commodity = arg_str(a, "commodity").ok_or("commodity is required")?;
        let radius = arg_f64(a, "max_distance_ly").unwrap_or(100.0);
        let db = self.db()?;
        let spots = db
            .hotspots_near(&HotspotQuery { origin, signal: commodity.clone(), max_distance_ly: Some(radius), min_count: 1, limit: arg_f64(a, "limit").map(|v| v as usize).unwrap_or(6).clamp(1, 15) })
            .map_err(|e| e.to_string())?;
        let sells = db
            .search_commodity(&CommodityQuery {
                commodity: commodity.clone(),
                mode: TradeMode::SellToStation,
                origin: Some(origin),
                max_distance_ly: Some(radius * 2.0),
                min_pad: me.pad,
                max_age_hours: Some(72.0),
                limit: 3,
                ..Default::default()
            })
            .map_err(|e| e.to_string())?;
        Ok(json!({
            "commodity": commodity, "near": near,
            "hotspots": spots.iter().map(|h| json!({ "ring": h.body_name, "system": h.system, "hotspots": h.count, "distance_ly": r1(h.distance_ly), "ring_type": h.ring_type })).collect::<Vec<_>>(),
            "best_sell_prices": sells.iter().map(|h| json!({ "station": h.station, "system": h.system, "price": h.price, "demand": h.quantity, "distance_ly": h.distance_ly.map(r1), "age_hours": hours_ago(h.updated_unix) })).collect::<Vec<_>>(),
            "note": if spots.is_empty() { "No hotspots known nearby yet: they come from ring scans shared over EDDN and the Spansh import." } else { "" },
        }))
    }

    fn conflicts_near(&self, a: &Value) -> Result<Value, String> {
        let me = self.me()?;
        let (origin, near) = self.origin(a, &me)?;
        let radius = arg_f64(a, "max_distance_ly").unwrap_or(100.0).clamp(1.0, 100_000.0);
        let days = arg_f64(a, "max_age_days").unwrap_or(3.0).clamp(0.1, 30.0);
        let q = ConflictQuery {
            origin,
            max_distance_ly: Some(radius),
            since_unix: nexus_journal::Timestamp::now().0 - (days * 86_400.0) as i64,
            elections: arg_bool(a, "elections").unwrap_or(false),
            limit: arg_f64(a, "limit").map(|v| v as usize).unwrap_or(8).clamp(1, 25),
        };
        let scan = self.db()?.conflicts_near(&q).map_err(|e| e.to_string())?;
        Ok(json!({
            "near": near, "within_ly": radius, "reported_within_days": days,
            "populated_systems_reported": scan.systems_reported,
            "found_in_range": scan.total,
            "conflicts": scan.hits.iter().map(|h| json!({
                "system": h.system, "distance_ly": r1(h.distance_ly), "kind": h.kind.label(),
                "status": match h.status { ConflictStatus::Active => "active", ConflictStatus::Pending => "pending", ConflictStatus::Ended => "ended" },
                "sides": h.sides.iter().map(|s| json!({ "faction": s.faction, "days_won": s.won_days, "stands_to_lose": s.stake, "allegiance": s.allegiance })).collect::<Vec<_>>(),
                "days_won_known": h.detailed,
                "government": h.government, "security": h.security,
                "report_age_hours": hours_ago(h.updated_unix),
            })).collect::<Vec<_>>(),
            "note": "From EDDN reports Elite Nexus received while it ran and the commander's journal, so a war that began after a system's last report is missing. \
                     Wars and civil wars have conflict zones (fight there after picking a side); elections have none. Without days_won_known only the \
                     faction states were reported. The Combat page lists these with a route button and explains how to fight legally.",
        }))
    }

    fn system_info(&self, a: &Value) -> Result<Value, String> {
        let name = arg_str(a, "system").ok_or("system is required")?;
        let db = self.db()?;
        let Some(s) = db.system_by_name(&name).map_err(|e| e.to_string())? else {
            return Err(format!("\"{name}\" is not in the local database yet."));
        };
        let stations = db.stations_in_system(s.id64).map_err(|e| e.to_string())?;
        let factions = db.system_factions(s.id64).map_err(|e| e.to_string())?;
        let me = self.me().ok();
        let dist = me.and_then(|m| m.pos).zip(s.pos).map(|(a, b)| r1(((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()));
        Ok(json!({
            "system": s.name, "distance_ly": dist, "population": s.population, "allegiance": s.allegiance, "government": s.government,
            "economy": s.economy, "second_economy": s.second_economy, "security": s.security,
            "controlling_faction": s.controlling_faction, "faction_state": s.controlling_faction_state,
            "power": s.controlling_power, "powerplay_state": s.powerplay_state,
            "stations": stations.iter().take(25).map(|st| json!({
                "name": st.name, "type": st.station_type, "pad": st.max_pad.map(String::from), "arrival_distance_ls": st.dist_ls.map(|d| d.round()),
                "economy": st.economy, "faction": st.faction, "fleet_carrier": st.is_carrier,
            })).collect::<Vec<_>>(),
            "factions": factions.iter().map(|f| json!({ "name": f.name, "influence": f.influence.map(|i| (i * 1000.0).round() / 10.0), "state": f.state })).collect::<Vec<_>>(),
        }))
    }

    fn plan_route(&self, a: &Value) -> Result<Value, String> {
        let me = self.me()?;
        let to = arg_str(a, "to").ok_or("to is required")?;
        let from = arg_str(a, "from").unwrap_or_else(|| me.system.clone());
        let neutron = arg_str(a, "kind").is_some_and(|k| k.eq_ignore_ascii_case("neutron"));
        let planned = if neutron || me.loadout.is_none() {
            if me.jump_range <= 0.0 {
                return Err("The ship's jump range is not known yet (fly it once).".into());
            }
            let range = me.jump_range.clamp(*crate::tours::limits::RANGE_LY.start(), *crate::tours::limits::RANGE_LY.end());
            let req = NeutronRouteRequest::new(from.clone(), to.clone(), range).efficiency(60);
            let p = self.rt.block_on(self.spansh.neutron_route(&req)).map_err(|e| format!("Spansh: {e}"))?;
            let w: Vec<_> = p.route.waypoints().iter().map(from_spansh).collect();
            let summary = format!("{} waypoints · {} jumps · {} neutron boosts", w.len(), p.route.total_jumps(), p.route.neutron_count());
            PlannedRoute::new("Neutron", summary, p.web_url, w)
        } else {
            let profile = ShipProfile::from_loadout_json(me.loadout.as_deref().unwrap_or_default()).map_err(|e| format!("Ship profile: {e:?}"))?;
            let cargo = {
                let model = self.model.read();
                model.active().map(|g| g.inventory.cargo.total().max(0) as u32).unwrap_or(0)
            };
            let req = GalaxyRouteRequest::new(from.as_str(), to.as_str(), &profile).cargo(cargo);
            let p = self.rt.block_on(self.spansh.galaxy_route(&req)).map_err(|e| format!("Spansh: {e}"))?;
            let w: Vec<_> = p.route.waypoints().iter().map(from_spansh).collect();
            let summary = format!("{} jumps · {:.0} ly · {} refuels · {} neutron boosts", p.route.jump_count(), p.route.distance(), p.route.refuel_stops(), p.route.neutron_boosts());
            PlannedRoute::new("Exact", summary, p.web_url, w)
        };
        let out = json!({
            "from": from, "to": to, "planner": planned.kind, "summary": planned.summary, "spansh_url": planned.web_url,
            "first_waypoints": planned.waypoints.iter().take(12).map(|w| json!({ "system": w.system, "note": w.note })).collect::<Vec<_>>(),
            "waypoint_count": planned.waypoints.len(),
            "note": "The route is now shown on the Navigation page and the next system is copied to the clipboard after each jump.",
        });
        *self.routes.lock() = Some(planned);
        Ok(out)
    }

    fn money_routes(&self, a: &Value) -> Result<Value, String> {
        let me = self.me()?;
        let from = arg_str(a, "near_system").unwrap_or_else(|| me.system.clone());
        let range = (if me.jump_range > 0.0 { me.jump_range } else { 20.0 }).clamp(*crate::tours::limits::RANGE_LY.start(), *crate::tours::limits::RANGE_LY.end());
        let radius = (arg_f64(a, "radius_ly").unwrap_or(30.0) as u32).clamp(*crate::tours::limits::RADIUS_LY.start(), *crate::tours::limits::RADIUS_LY.end());
        let count = arg_f64(a, "max_systems").unwrap_or(15.0).clamp(1.0, 50.0) as u32;
        let kind = arg_str(a, "kind").unwrap_or_else(|| "exobiology".into()).to_lowercase();
        let exo = kind.contains("exo") || kind.contains("bio");
        let (label, tour) = if exo {
            let req = ExobiologyRouteRequest::new(from.clone(), range).radius(radius).max_results(count).min_value(1_000_000);
            ("Exobiology", self.rt.block_on(crate::tours::known_species(&self.spansh, req)))
        } else {
            let base = match kind.as_str() {
                "earth_likes" | "earth-likes" | "elw" => RichesRouteRequest::earth_likes(from.clone(), range),
                "ammonia_worlds" | "ammonia" => RichesRouteRequest::ammonia_worlds(from.clone(), range),
                _ => RichesRouteRequest::road_to_riches(from.clone(), range),
            };
            ("Road to riches", self.rt.block_on(crate::tours::riches(&self.spansh, base.radius(radius).max_results(count).min_value(300_000).use_mapping_value(true))))
        };
        let tour = tour.map_err(|e| format!("Spansh: {e}"))?;
        if tour.is_empty() {
            return Err(format!("Spansh found nothing worth scanning within {} ly of {from}, even after widening the search (it skips populated systems).", tour.radius));
        }
        let planned = tour.planned(label);
        let w = &planned.waypoints;
        let total: u64 = w.iter().filter_map(|x| x.value).sum();
        // waypoints() maps the route's systems one to one
        let stops: Vec<Value> = w
            .iter()
            .zip(&tour.route.systems)
            .filter(|(_, s)| !s.bodies.is_empty())
            .take(20)
            .map(|(x, s)| {
                let mut stop = json!({ "system": x.system, "value_cr": x.value, "bodies": x.note });
                let footfall = self.route_footfall(s);
                if !footfall.is_empty() {
                    stop["footfall"] = Value::Array(footfall);
                }
                stop
            })
            .collect();
        let mut note = if exo {
            "These planets are on Spansh because commanders already logged these species there, so the species are certain but the first-logged bonus has most likely been claimed: values are base values. For bonus hunting use untouched_biology. The route is now on the Navigation page.".to_string()
        } else {
            "Values are Spansh's estimates for scanning (and mapping) the bodies listed. The route is now on the Navigation page.".to_string()
        };
        if let Some(asked) = tour.widened_from {
            note.push_str(&format!(" Nothing turned up within {asked} ly (Spansh skips populated systems), so the search was widened to {} ly.", tour.radius));
        }
        if stops.iter().any(|s| s.get("footfall").is_some()) {
            note.push_str(" Footfall: from scans commanders shared over EDDN and your own journal, as of the age given; it can be out of date, and bodies without it are unknown.");
        }
        let out = json!({
            "kind": label, "from": from, "radius_ly": tour.radius, "systems": stops.len(), "total_value_cr": total, "total_value_text": compact_credits(total as i64), "spansh_url": tour.web_url,
            "stops": stops,
            "note": note,
        });
        *self.routes.lock() = Some(planned);
        Ok(out)
    }

    fn exobiology_here(&self) -> Result<Value, String> {
        let model = self.model.read();
        let gs = model.active().ok_or("No commander loaded yet.")?;
        let Some(sys) = gs.exploration.systems.get(&gs.location.system_address) else {
            return Ok(json!({ "message": "No scans of the current system yet: FSS-scan it first." }));
        };
        // The most valuable bodies first, each with its likely species, worked out once.
        let mut bodies: Vec<(i64, Value, Vec<Value>)> = sys
            .bodies
            .values()
            .filter(|b| b.bio_signals > 0)
            .map(|b| {
                let value = b.predicted_value(b.was_footfalled == Some(false));
                let body = json!({
                    "body": b.name, "biological_signals": b.bio_signals, "distance_ls": b.distance_ls.round(),
                    "genera_from_dss": b.genuses,
                    "sampled": b.organics.iter().map(|o| json!({ "species": o.species, "samples": o.samples, "analysed": o.analysed })).collect::<Vec<_>>(),
                    "expected_value_cr": value,
                });
                let species = b.likely_species().iter().take(8).map(|p| json!({ "species": p.species, "value_cr": p.value, "likelihood_pct": (p.likelihood * 100.0).round(), "colony_range_m": p.colony_range_m })).collect();
                (value, body, species)
            })
            .collect();
        bodies.sort_by_key(|(value, _, _)| std::cmp::Reverse(*value));
        let unsold = gs.exobio.unsold_value();
        // What gives way when the reply is too long for the caller: the less likely species of each body, then the
        // least valuable bodies, one by one, down to the first.
        const SPECIES: [usize; 3] = [8, 3, 1];
        let (out, _) = fit(self.budget, SPECIES.len() + bodies.len().saturating_sub(1), |level| {
            let species = SPECIES[level.min(SPECIES.len() - 1)];
            let shown = bodies.len() - level.saturating_sub(SPECIES.len() - 1).min(bodies.len().saturating_sub(1));
            let listed: Vec<Value> = bodies[..shown]
                .iter()
                .map(|(_, body, likely)| {
                    let mut b = body.clone();
                    b["likely_species"] = json!(likely.iter().take(species).collect::<Vec<_>>());
                    b
                })
                .collect();
            let mut out = json!({ "system": sys.name, "bio_bodies": listed, "unsold_exobiology_data_cr": unsold });
            let mut notes = Vec::new();
            if species < SPECIES[0] {
                notes.push(format!("Only the {species} likeliest species of each body, to fit."));
            }
            if shown < bodies.len() {
                notes.push(format!("The {} least valuable bodies left out to fit.", bodies.len() - shown));
            }
            if !notes.is_empty() {
                out["notes"] = json!(notes);
            }
            out
        });
        Ok(out)
    }

    /// Journal lines holding a text, newest first, in a period if one is given. It returns at most `limit` of them, and
    /// only as many as fit the caller's reply ([`NexusTools::budget`]), says when older matches were left out, and
    /// `skip` pages past them, so any stretch of the history can be read.
    fn journal_search(&self, a: &Value) -> Result<Value, String> {
        use nexus_journal::Timestamp;
        let needle = arg_str(a, "query").ok_or("query is required")?.to_lowercase();
        let limit = arg_f64(a, "limit").map(|v| v as usize).unwrap_or(15).clamp(1, 40);
        let skip = arg_f64(a, "skip").map(|v| v.max(0.0) as usize).unwrap_or(0);
        // The period: a date is the commander's local day (`to` includes it); a journal time is taken as it is.
        let bound = |key: &str, end: bool| -> Result<Option<i64>, String> {
            let Some(s) = arg_str(a, key) else { return Ok(None) };
            if let Some(t) = Timestamp::parse(&s) {
                return Ok(Some(t.0 + i64::from(end)));
            }
            let (start, next) = Timestamp::local_day(&s).ok_or_else(|| format!("{key} \"{s}\" is not a date (YYYY-MM-DD)"))?;
            Ok(Some(if end { next.0 } else { start.0 }))
        };
        let (from, to) = (bound("from", false)?, bound("to", true)?);
        let dated = from.is_some() || to.is_some();
        let in_period = |ts: i64| from.is_none_or(|f| ts >= f) && to.is_none_or(|t| ts < t);
        // Only the files with events in the period: the index knows every event's time.
        let (files, first) = {
            let idx = self.index.read();
            let mut wanted = vec![!dated; idx.files.len()];
            if dated {
                for e in idx.entries.iter().filter(|e| in_period(e.ts)) {
                    if let Some(w) = wanted.get_mut(e.file as usize) {
                        *w = true;
                    }
                }
            }
            let files: Vec<_> = idx.files.iter().zip(wanted).filter(|(_, w)| *w).map(|(p, _)| p.clone()).collect();
            (files, idx.entries.first().map(|e| e.ts))
        };
        // Room for the rest of the answer within the caller's length.
        let budget = self.budget.saturating_sub(1_000);
        let (mut hits, mut size, mut skipped, mut more) = (Vec::new(), 0, 0, false);
        'files: for path in files.iter().rev() {
            let Ok(text) = std::fs::read_to_string(path) else { continue };
            for line in text.lines().rev() {
                if !line.to_lowercase().contains(&needle) {
                    continue;
                }
                let v: Value = serde_json::from_str(line).unwrap_or(Value::Null);
                let ts = v["timestamp"].as_str().and_then(Timestamp::parse);
                if dated {
                    let Some(ts) = ts else { continue };
                    if from.is_some_and(|f| ts.0 < f) {
                        break 'files; // everything further on is older still
                    }
                    if !in_period(ts.0) {
                        continue;
                    }
                }
                if skipped < skip {
                    skipped += 1;
                    continue;
                }
                let mut snippet: String = line.chars().take(400).collect();
                if line.len() > snippet.len() {
                    snippet.push('…');
                }
                let hit = json!({ "time": ts.map(|t| t.to_local_string("%Y-%m-%d %H:%M")), "event": v["event"], "line": snippet });
                let len = hit.to_string().len() + 1;
                if hits.len() >= limit || size + len > budget {
                    more = true;
                    break 'files;
                }
                size += len;
                hits.push(hit);
            }
        }
        let shown = hits.len();
        let mut out = json!({ "query": needle, "matches_newest_first": hits });
        if let Some(t) = first {
            out["journal_starts"] = json!(Timestamp(t).to_local_string("%Y-%m-%d"));
        }
        if more {
            out["more_matches"] = json!(format!("Older matches were left out: call again with skip {} (same query and period) for the next ones.", skip + shown));
        }
        Ok(out)
    }

    /// Planets and moons of a system: physics and signals (Spansh, local), discovery and footfall
    /// (galaxy database: own journal + EDDN; archived EDDN scans online; Spansh's age), likely species
    /// and value (prediction engine).
    fn system_bodies(&self, a: &Value) -> Result<Value, String> {
        let query = arg_str(a, "system").ok_or("system is required")?;
        let mut filter = arg_str(a, "body");
        let (my_pos, own_systems, codex_region) = {
            let model = self.model.read();
            match model.active() {
                Some(gs) => (
                    gs.location.star_pos,
                    gs.exploration.systems.values().map(|s| (s.address, s.name.clone(), s.star_pos)).collect::<Vec<_>>(),
                    gs.exploration.region_of(&query),
                ),
                None => (None, Vec::new(), None),
            }
        };
        let mut place = self.locate_system(&query, &own_systems, &mut filter)?;
        let mut notes = vec![FOOTFALL_NOTE.to_string()];
        let spansh = match self.spansh_system(place.id64) {
            Ok(s) => Some(s),
            Err(e) => {
                notes.push(format!("Spansh is unavailable ({e}): physics and signals only from local data."));
                None
            }
        };
        if let Some(s) = &spansh {
            if place.name.is_empty() {
                place.name.clone_from(&s.name);
            }
            place.pos = place.pos.or(Some(s.pos));
            place.population = s.population.or(place.population);
        }
        if place.name.is_empty() {
            place.name.clone_from(&query);
        }
        let region = place.region.clone().or(codex_region);
        let rows: Vec<BodyInfo> = match &self.db {
            Some(db) => db.bodies_in_system(place.id64).map_err(|e| e.to_string())?,
            None => Vec::new(),
        };
        let own: Option<SystemScan> = {
            let model = self.model.read();
            model.active().and_then(|gs| gs.exploration.systems.get(&place.id64).cloned())
        };
        let populated = place.population.unwrap_or(0) > 0;

        // One entry per body id, from whichever sources know it.
        let mut ids: Vec<u32> = rows.iter().map(|b| b.body_id).collect();
        ids.extend(spansh.iter().flat_map(|s| s.bodies.iter().map(|b| b.body_id)));
        ids.extend(own.iter().flat_map(|s| s.bodies.keys().copied()));
        ids.sort_unstable();
        ids.dedup();
        let planets: Vec<Planet> = ids
            .into_iter()
            .map(|id| Planet {
                id,
                db: rows.iter().find(|b| b.body_id == id),
                sp: spansh.as_ref().and_then(|s| s.bodies.iter().find(|b| b.body_id == id)),
                own: own.as_ref().and_then(|s| s.bodies.get(&id)).filter(|b| b.kind == nexus_model::exploration::BodyKind::Planet || b.bio_signals > 0),
            })
            .filter(|p| p.db.is_some() || p.sp.is_some() || p.own.is_some())
            .filter(Planet::relevant)
            .collect();
        let total = planets.len();
        let shown: Vec<&Planet> = match &filter {
            Some(f) => planets.iter().filter(|p| body_matches(&p.name(), &place.name, f)).collect(),
            None => planets.iter().collect(),
        };
        let ctx = PlanetCtx { system: &place.name, pos: place.pos, region: region.as_deref(), populated, own: own.as_ref() };
        let preds: Vec<Vec<Prediction>> = shown.iter().map(|p| predictions(p, &ctx)).collect();
        // Footfall, best source first: populated system, our journal / EDDN (galaxy database), archived
        // EDDN messages online (on demand), Spansh having only pre-Odyssey data, else unknown.
        let mut footfall: Vec<Footfall> = shown.iter().map(|p| Footfall::local(p, populated)).collect();
        let looked_up = self.lookup_footfall(place.id64, &shown, &preds, &mut footfall);
        for (p, f) in shown.iter().zip(footfall.iter_mut()) {
            if matches!(f, Footfall::Unknown) {
                *f = Footfall::pre_odyssey(p);
            }
        }
        let mut entries: Vec<(bool, i64, f64, Value)> =
            shown.iter().zip(&footfall).zip(&preds).map(|((p, f), pr)| planet_json(p, f, pr, &ctx)).collect();
        // Biology first (most valuable first), then the rest by distance from the arrival star.
        entries.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.total_cmp(&b.2)));
        let listed = entries.len();
        entries.truncate(MAX_BODIES);
        let bodies: Vec<Value> = entries.into_iter().map(|e| e.3).collect();
        if looked_up > 0 {
            notes.push(format!("Footfall of {looked_up} biology bodies looked up in archived EDDN scans (EDGalaxyData); cached for a day."));
        }
        if populated {
            notes.push("Populated system: first footfall is not possible here and Vista Genomics pays no first-logged bonus.".into());
        }
        if let Some(f) = &filter
            && listed == 0
        {
            let names: Vec<String> = planets.iter().take(30).map(|p| short_name(&p.name(), &place.name).to_string()).collect();
            notes.push(format!("No planet or moon matching \"{f}\" is known here. Known: {}.", names.join(", ")));
        } else if total == 0 {
            notes.push("No landable planets, biology or Earth-like/water/ammonia/terraformable worlds known in this system yet.".into());
        }
        let dist = my_pos.zip(place.pos).map(|(a, b)| r1(((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()));
        // The least interesting bodies give way when the reply is too long for the caller.
        let (mut out, _) = fit(self.budget, bodies.len().max(1), |cut| {
            let kept = bodies.len().saturating_sub(cut);
            let mut notes = notes.clone();
            if listed > kept {
                notes.push(format!("{} more bodies not shown; ask about one by name.", listed - kept));
            }
            json!({
                "system": place.name, "distance_ly": dist, "population": place.population, "region": region,
                "relevant_bodies": total, "bodies": &bodies[..kept], "notes": notes,
            })
        });
        strip_nulls(&mut out);
        Ok(out)
    }

    /// The system to describe: local database, the commander's own scans, Spansh, then bodies the
    /// database knows by name (a body name given as the system narrows the answer to that body).
    fn locate_system(&self, query: &str, own: &[(u64, String, Option<[f64; 3]>)], filter: &mut Option<String>) -> Result<Place, String> {
        if let Some(db) = &self.db
            && let Ok(Some(s)) = db.system_by_name(query)
        {
            return Ok(Place { id64: s.id64, name: s.name, pos: s.pos, population: s.population, region: None });
        }
        if let Some((id64, name, pos)) = own.iter().find(|(_, n, _)| n.eq_ignore_ascii_case(query)) {
            return Ok(Place { id64: *id64, name: name.clone(), pos: *pos, population: None, region: None });
        }
        let found = self.rt.block_on(async { tokio::time::timeout(SPANSH_TIMEOUT, self.spansh.system_by_name(query)).await });
        let unknown = matches!(found, Ok(Ok(None)));
        if let Ok(Ok(Some(r))) = found
            && r.id64 > 0
        {
            return Ok(Place { id64: r.id64, name: r.name, pos: Some([r.x, r.y, r.z]), population: r.population, region: r.region });
        }
        if let Some(db) = &self.db {
            if let Ok(Some(b)) = db.body_by_name(query) {
                filter.get_or_insert_with(|| query.to_string());
                return Ok(match db.system_by_id(b.system_id64) {
                    Ok(Some(s)) => Place { id64: s.id64, name: s.name, pos: s.pos, population: s.population, region: None },
                    _ => Place { id64: b.system_id64, name: String::new(), pos: None, population: None, region: None },
                });
            }
            if let Ok(found) = db.find_bodies(&format!("{} ", query.trim()), 1)
                && let Some(b) = found.first()
            {
                return Ok(Place { id64: b.system_id64, name: query.trim().to_string(), pos: None, population: None, region: None });
            }
        }
        Err(if unknown {
            format!("Unknown system \"{query}\" (not in the local database, your scans or Spansh).")
        } else {
            format!("\"{query}\" is not in the local database or your scans, and Spansh can't be reached to look it up.")
        })
    }

    /// Spansh's dump of a system in planet form, cached for a few minutes.
    fn spansh_system(&self, id64: u64) -> Result<Arc<SpanshSystem>, String> {
        {
            let mut cache = SPANSH_SYSTEMS.lock();
            cache.retain(|(_, t, _)| t.elapsed() < SPANSH_CACHE_TTL);
            if let Some((_, _, s)) = cache.iter().find(|(id, _, _)| *id == id64) {
                return Ok(s.clone());
            }
        }
        let dumped = self
            .rt
            .block_on(async { tokio::time::timeout(SPANSH_TIMEOUT, self.spansh.dump(id64)).await })
            .map_err(|_| "no answer in time".to_string())?
            .map_err(|e| e.to_string())?;
        let sys = Arc::new(SpanshSystem::from_dump(&dumped));
        let mut cache = SPANSH_SYSTEMS.lock();
        cache.push((id64, Instant::now(), sys.clone()));
        if cache.len() > 8 {
            cache.remove(0);
        }
        Ok(sys)
    }

    /// Footfall of a route stop's listed bodies, where the galaxy database knows it.
    fn route_footfall(&self, s: &ScanSystem) -> Vec<Value> {
        let (Some(db), Some(id64)) = (&self.db, s.id64) else { return Vec::new() };
        let Ok(rows) = db.bodies_in_system(id64) else { return Vec::new() };
        s.bodies
            .iter()
            .filter_map(|b| {
                // Spansh body id64 = system id64 + body id << 55
                let body_id = b.id64.map(|id| (id >> 55) as u32);
                let row = rows.iter().find(|r| Some(r.body_id) == body_id || r.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&b.name)))?;
                let seen = row.was_footfalled?;
                let mut f = Footfall::Known { seen, source: row.footfall_source.unwrap_or_default() }.json();
                f["body"] = json!(short_name(&b.name, &s.name));
                Some(f)
            })
            .collect()
    }

    /// Footfall step 4: archived EDDN scans (EDGalaxyData) for the most valuable shown biology bodies
    /// whose footfall is still unknown, a few per call, one at a time. Every answer is cached in the
    /// galaxy database (also "nothing found"), so a body is not asked about again within a day;
    /// failures leave it unknown. Returns the number of bodies looked up.
    fn lookup_footfall(&self, system_id64: u64, shown: &[&Planet], preds: &[Vec<Prediction>], footfall: &mut [Footfall]) -> usize {
        // Without the database there is no cache, and the service would be asked again every time.
        if !ONLINE_FOOTFALL_LOOKUP.load(Ordering::Relaxed) || self.db.is_none() {
            return 0;
        }
        let Some(http) = EDGD_HTTP.as_ref() else { return 0 };
        let now = nexus_journal::Timestamp::now().0;
        let mut order: Vec<usize> = (0..shown.len())
            .filter(|&i| matches!(footfall[i], Footfall::Unknown) && shown[i].bio() > 0 && shown[i].landable() != Some(false))
            .filter(|&i| shown[i].db.and_then(|d| d.checked_online_unix).is_none_or(|t| now - t >= LOOKUP_AGAIN_AFTER_S))
            .collect();
        order.sort_by_key(|&i| std::cmp::Reverse(preds[i].iter().map(|p| bp::expected_value(p, false)).sum::<i64>()));
        order.truncate(LOOKUPS_PER_CALL);
        let started = Instant::now();
        let mut done = 0;
        for i in order {
            if started.elapsed() > LOOKUP_BUDGET {
                break;
            }
            let p = shown[i];
            // One request at a time across Ask and AI agents too.
            let one = EDGD_ONE_AT_A_TIME.lock();
            let asked = self.rt.block_on(archive_footfall(http, system_id64, p.id));
            drop(one);
            let found = match asked {
                Ok(found) => found,
                Err(e) => {
                    // unreachable or failing: don't keep knocking
                    tracing::debug!("EDGalaxyData footfall lookup failed: {e}");
                    break;
                }
            };
            done += 1;
            let mut update = found.unwrap_or_else(|| BodyUpdate { system_id64, body_id: p.id, source: Source::EddnArchive, ..BodyUpdate::default() });
            update.checked_online = Some(now);
            if update.name.is_none() {
                update.name = p.known_name();
            }
            if let Some(value) = update.was_footfalled {
                footfall[i] = Footfall::Known { seen: Seen { value, at_unix: update.timestamp }, source: Source::EddnArchive };
            }
            if let Some(db) = &self.db {
                db.ingest(Ingest::Body(update));
            }
        }
        done
    }
}

// ---------------------------------------------------------------------------------------------
// Planets: footfall, signals, likely biology
// ---------------------------------------------------------------------------------------------

/// `SurfaceGravity` per g and `SurfacePressure` per atmosphere as the galaxy database and Spansh
/// use them; the exobiology rules use slightly different ones (`nexus_data::exobio::JOURNAL_G_MS2`, `JOURNAL_ATM_PA`).
pub(crate) const STANDARD_G: f64 = 9.80665;
pub(crate) const STANDARD_ATM: f64 = 101_325.0;
/// Bodies listed by `system_bodies` at most (fewer when they don't fit the caller's reply).
const MAX_BODIES: usize = 20;
const SPANSH_TIMEOUT: Duration = Duration::from_secs(25);
const SPANSH_CACHE_TTL: Duration = Duration::from_secs(600);

const FOOTFALL_NOTE: &str = "Footfall (has anyone set foot on the body) is in no public database. It comes from the game's WasFootfalled flag in scans since game 4.2.1 (Sept 2025): the commander's own journal (authoritative), scans other commanders shared over EDDN, and archived EDDN scans looked up on demand (EDGalaxyData). Not every program that shares scans keeps the flag, so \"unknown\" is common. It is the state when that scan was made: give its age, and remember that whoever scanned a body often landed right after. \"probably unclaimed\" is a rule of thumb: Spansh has no data on the body since before Odyssey (May 2021). First footfall is impossible in populated systems.";

/// Recently fetched Spansh dumps (the model often asks about the same system twice).
static SPANSH_SYSTEMS: Mutex<Vec<(u64, Instant, Arc<SpanshSystem>)>> = Mutex::new(Vec::new());

/// Footfall lookups in EDGalaxyData's EDDN index (archived EDDN messages per body), made on demand by
/// `system_bodies` for biology bodies whose footfall is unknown locally. It is a Patreon-funded
/// service without published limits, so: a few bodies per call, one request at a time, a day's
/// cache. Store `false` to switch the lookups off (e.g. from a setting); off in unit tests.
pub(crate) static ONLINE_FOOTFALL_LOOKUP: AtomicBool = AtomicBool::new(!cfg!(test));
/// Held during each EDGalaxyData request: Ask and an AI agent never ask it at the same time.
static EDGD_ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
const EDGD_URL: &str = "https://edgalaxydata.space/eddn-lookup-v2";
/// `WasFootfalled` first appears in EDDN scans with game 4.2.1.0, on 2025-09-29.
const FOOTFALL_KEY_SINCE: &str = "2025-09-29T00:00:00Z";
const LOOKUPS_PER_CALL: usize = 6;
/// Archived scans fetched per body at most (the newest that can carry the flag).
const EXTRACTS_PER_BODY: usize = 2;
const LOOKUP_AGAIN_AFTER_S: i64 = 86_400;
const LOOKUP_BUDGET: Duration = Duration::from_secs(20);
/// Uploaders that strip `WasFootfalled` (lower-case parts of `softwareName`).
const STRIPS_FOOTFALL: [&str; 4] = ["eddiscovery", "eddlite", "materials helper", "edomh"];
/// Odyssey's launch, 2021-05-19 (unix seconds): on-foot play, and so footfall, began then.
const ODYSSEY_LAUNCH: i64 = 1_621_382_400;

/// HTTP client for EDGalaxyData (TLS 1.3 only: rustls handles it, Windows 10's Schannel can't).
static EDGD_HTTP: LazyLock<Option<reqwest::Client>> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("EliteNexus/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(8))
        .build()
        .inspect_err(|e| tracing::warn!("EDGalaxyData client: {e}"))
        .ok()
});

/// The system `system_bodies` describes.
struct Place {
    id64: u64,
    name: String,
    pos: Option<[f64; 3]>,
    population: Option<u64>,
    region: Option<String>,
}

/// The planets and moons of a Spansh system dump, in journal terms.
#[derive(Debug, Clone)]
struct SpanshSystem {
    name: String,
    population: Option<u64>,
    pos: [f64; 3],
    bodies: Vec<SpanshBody>,
}

#[derive(Debug, Clone)]
struct SpanshBody {
    body_id: u32,
    name: String,
    /// Journal `PlanetClass`.
    class: Option<String>,
    landable: bool,
    distance_ls: Option<f64>,
    /// Standard g.
    gravity_g: Option<f64>,
    temperature_k: Option<f64>,
    /// Spansh's text ("Thin Carbon dioxide").
    atmosphere: Option<String>,
    /// Journal style ("minor rocky magma volcanism").
    volcanism: Option<String>,
    terraform: Option<String>,
    bio: u32,
    geo: u32,
    genera: Vec<String>,
    /// Newest data Spansh has on the body (unix seconds).
    updated_unix: Option<i64>,
    /// Prediction input (without the region, which comes from elsewhere).
    facts: BodyFacts,
}

impl SpanshSystem {
    fn from_dump(d: &dump::System) -> SpanshSystem {
        let stars: Vec<SystemStar> = d
            .bodies
            .iter()
            .filter(|b| b.is_star())
            .filter_map(|b| {
                Some(SystemStar {
                    body_id: b.body_id?,
                    parent: b.parents.first().map(parent_ref),
                    distance_ls: b.distance_to_arrival.unwrap_or(0.0),
                    facts: StarFacts {
                        star_type: journal_star_type(b.sub_type.as_deref()?)?,
                        luminosity: b.luminosity.clone(),
                        subclass: b.spectral_class.as_deref().and_then(|c| c.chars().last()?.to_digit(10)),
                    },
                })
            })
            .collect();
        let planets: Vec<&dump::Body> = d.bodies.iter().filter(|b| b.body_type.as_deref() == Some("Planet")).collect();
        let classes: Vec<String> = planets.iter().filter_map(|b| b.sub_type.as_deref().map(journal_planet_class)).collect();
        let pos = [d.coords.x, d.coords.y, d.coords.z];
        let bodies = planets
            .iter()
            .filter_map(|b| {
                let body_id = b.body_id.or_else(|| u32::try_from(b.id64 >> 55).ok().filter(|_| b.id64 > 0))?;
                let class = b.sub_type.as_deref().map(journal_planet_class);
                let volcanism = b
                    .volcanism_type
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("No volcanism"))
                    .map(|v| format!("{} volcanism", v.to_ascii_lowercase()));
                let (bio, geo) = (b.signal_count("Biological"), b.signal_count("Geological"));
                let parents: Vec<ParentRef> = b.parents.iter().map(parent_ref).collect();
                let facts = BodyFacts {
                    planet_class: class.clone().unwrap_or_default(),
                    atmosphere: b.atmosphere_type.clone(),
                    atmosphere_composition: b.atmosphere_composition.iter().map(|(g, p)| (g.replace(' ', ""), *p)).collect(),
                    gravity_g: b.gravity.map(|g| g * STANDARD_G / bp::JOURNAL_G_MS2),
                    temperature_k: b.surface_temperature,
                    pressure_atm: b.surface_pressure.map(|p| p * STANDARD_ATM / bp::JOURNAL_ATM_PA),
                    volcanism: volcanism.clone(),
                    landable: b.is_landable == Some(true),
                    materials: b.materials.iter().map(|(m, p)| (m.to_ascii_lowercase(), *p)).collect(),
                    parent_stars: bp::resolve_parent_stars(&parents, &stars),
                    distance_from_arrival_ls: b.distance_to_arrival.unwrap_or(0.0),
                    system_name: d.name.clone(),
                    star_pos: Some(pos),
                    bio_signal_count: (bio > 0).then_some(bio),
                    system_stars: stars.iter().map(|s| s.facts.clone()).collect(),
                    system_body_classes: classes.clone(),
                    orbital_period_s: b.orbital_period.map(|days| days * 86_400.0),
                    ..BodyFacts::default()
                };
                Some(SpanshBody {
                    body_id,
                    name: b.name.clone(),
                    class,
                    landable: b.is_landable == Some(true),
                    distance_ls: b.distance_to_arrival,
                    gravity_g: b.gravity,
                    temperature_k: b.surface_temperature,
                    atmosphere: b.atmosphere_type.clone().filter(|t| !t.eq_ignore_ascii_case("No atmosphere")),
                    volcanism,
                    terraform: b.terraforming_state.clone().filter(|t| nexus_galaxy::is_terraformable(Some(t))),
                    bio,
                    geo,
                    genera: b.signals.as_ref().map(|s| s.genuses.clone()).unwrap_or_default(),
                    updated_unix: [b.update_time.as_deref(), b.signals.as_ref().and_then(|s| s.update_time.as_deref())]
                        .into_iter()
                        .flatten()
                        .chain(b.timestamps.values().map(String::as_str))
                        .filter_map(nexus_spansh::parse_timestamp)
                        .max(),
                    facts,
                })
            })
            .collect();
        SpanshSystem { name: d.name.clone(), population: d.population, pos, bodies }
    }
}

fn parent_ref(p: &dump::ParentRef) -> ParentRef {
    match p.kind.as_str() {
        "Star" => ParentRef::Star(p.body_id),
        "Planet" => ParentRef::Planet(p.body_id),
        "Ring" => ParentRef::Ring(p.body_id),
        _ => ParentRef::Barycentre(p.body_id),
    }
}

/// Spansh (EDSM-style) planet sub-type -> journal `PlanetClass`.
pub(crate) fn journal_planet_class(sub: &str) -> String {
    let s = sub.trim();
    let class = match s {
        "Metal-rich body" => "Metal rich body",
        "High metal content world" => "High metal content body",
        "Rocky Ice world" | "Rocky ice world" => "Rocky ice body",
        "Earth-like world" => "Earthlike body",
        "Gas giant with water-based life" => "Gas giant with water based life",
        "Gas giant with ammonia-based life" => "Gas giant with ammonia based life",
        "Helium-rich gas giant" => "Helium rich gas giant",
        _ => {
            if let Some(n) = s.strip_prefix("Class ").and_then(|r| r.strip_suffix(" gas giant")) {
                return format!("Sudarsky class {n} gas giant");
            }
            s
        }
    };
    class.to_string()
}

/// Spansh star sub-type ("K (Yellow-Orange) Star", "White Dwarf (DA) Star") -> journal `StarType`.
fn journal_star_type(sub: &str) -> Option<String> {
    let s = sub.trim();
    let l = s.to_ascii_lowercase();
    let fixed = match l.as_str() {
        "t tauri star" => Some("TTS"),
        "herbig ae/be star" => Some("AeBe"),
        "neutron star" => Some("N"),
        "black hole" => Some("H"),
        "supermassive black hole" => Some("SupermassiveBlackHole"),
        "c star" => Some("C"),
        "cn star" => Some("CN"),
        "cj star" => Some("CJ"),
        "ms-type star" => Some("MS"),
        "s-type star" => Some("S"),
        _ => None,
    };
    if let Some(t) = fixed {
        return Some(t.to_string());
    }
    if l.starts_with("white dwarf") {
        return Some(s.split(['(', ')']).nth(1).map_or("D", str::trim).to_string());
    }
    if let Some(rest) = s.strip_prefix("Wolf-Rayet") {
        return Some(format!("W{}", rest.trim().trim_end_matches("Star").trim()));
    }
    let class = s.split_whitespace().next()?;
    if !matches!(class, "O" | "B" | "A" | "F" | "G" | "K" | "M" | "L" | "T" | "Y") {
        return None;
    }
    let giant = match class {
        "A" | "B" if l.contains("super giant") => Some(format!("{class}_BlueWhiteSuperGiant")),
        "F" | "G" if l.contains("super giant") => Some(format!("{class}_WhiteSuperGiant")),
        "M" if l.contains("super giant") => Some("M_RedSuperGiant".into()),
        "M" if l.contains("giant") => Some("M_RedGiant".into()),
        "K" if l.contains("giant") => Some("K_OrangeGiant".into()),
        _ => None,
    };
    Some(giant.unwrap_or_else(|| class.to_string()))
}

/// One planet or moon as known from the galaxy database (footfall, discovery, signals: EDDN and the
/// commander's journal), Spansh (physics, signals, parent stars) and the commander's own scans.
struct Planet<'a> {
    id: u32,
    db: Option<&'a BodyInfo>,
    sp: Option<&'a SpanshBody>,
    own: Option<&'a OwnBody>,
}

impl Planet<'_> {
    fn known_name(&self) -> Option<String> {
        self.db
            .and_then(|d| d.name.clone())
            .or_else(|| self.sp.map(|s| s.name.clone()))
            .or_else(|| self.own.map(|o| o.name.clone()))
            .filter(|n| !n.is_empty())
    }

    fn name(&self) -> String {
        self.known_name().unwrap_or_else(|| format!("body {}", self.id))
    }

    fn own_scan(&self) -> Option<&OwnBody> {
        self.own.filter(|o| o.has_scan)
    }

    fn class(&self) -> Option<String> {
        self.db
            .and_then(|d| d.planet_class.clone())
            .or_else(|| self.own_scan().and_then(|o| o.planet_class.clone()))
            .or_else(|| self.sp.and_then(|s| s.class.clone()))
    }

    fn landable(&self) -> Option<bool> {
        self.db.and_then(|d| d.landable).or_else(|| self.own_scan().map(|o| o.landable)).or_else(|| self.sp.map(|s| s.landable))
    }

    fn terraform(&self) -> Option<String> {
        self.db
            .and_then(|d| d.terraform_state.clone())
            .or_else(|| self.own_scan().and_then(|o| o.terraform_state.clone()))
            .or_else(|| self.sp.and_then(|s| s.terraform.clone()))
            .filter(|t| nexus_galaxy::is_terraformable(Some(t)))
    }

    /// Signals found by anyone.
    fn bio(&self) -> u32 {
        let db = self.db.and_then(|d| d.bio_signals).unwrap_or(0);
        db.max(self.own.map_or(0, |o| o.bio_signals)).max(self.sp.map_or(0, |s| s.bio))
    }

    fn geo(&self) -> u32 {
        let db = self.db.and_then(|d| d.geo_signals).unwrap_or(0);
        db.max(self.own.map_or(0, |o| o.geo_signals)).max(self.sp.map_or(0, |s| s.geo))
    }

    /// Genera revealed by a DSS scan (codex symbols or English names).
    fn genera(&self) -> Vec<String> {
        [
            self.db.map(|d| d.genera.clone()),
            self.own.map(|o| o.genuses.clone()),
            self.sp.map(|s| s.genera.clone()),
        ]
        .into_iter()
        .flatten()
        .find(|g| !g.is_empty())
        .unwrap_or_default()
    }

    fn notable(&self) -> bool {
        self.class().is_some_and(|c| nexus_galaxy::is_notable_class(&c)) || self.terraform().is_some()
    }

    fn relevant(&self) -> bool {
        self.landable() == Some(true) || self.bio() > 0 || self.geo() > 0 || self.notable()
    }
}

/// Footfall of one body, from the best source that knows it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Footfall {
    /// First footfall is impossible in populated systems (the game's flag stays false there).
    Populated,
    /// The game's `WasFootfalled` as observed: our journal or EDDN (galaxy database), an archive lookup.
    Known { seen: Seen, source: Source },
    /// The commander disembarked there (own scans; no time recorded).
    Landed,
    /// The commander took the first footfall: their first scan said nobody had landed, then they did
    /// (unix seconds when that was first seen; 0 = unknown).
    Yours { at: i64 },
    /// Spansh's newest data on the body predates Odyssey (unix seconds of it).
    PreOdyssey { last: i64 },
    Unknown,
}

impl Footfall {
    /// Steps 1-3: populated system; the galaxy database (our journal, authoritative, and EDDN);
    /// else the commander's own scans.
    fn local(p: &Planet, populated: bool) -> Footfall {
        if populated {
            return Footfall::Populated;
        }
        // The commander's first scan of the body (the model keeps the first one) and their landing.
        let first_scan_unclaimed = p.own.is_some_and(|o| o.was_footfalled == Some(false));
        let you_landed = p.own.is_some_and(|o| o.footfall);
        if let Some(d) = p.db
            && let Some(seen) = d.was_footfalled
        {
            if seen.value && first_scan_unclaimed && you_landed {
                return Footfall::Yours { at: seen.at_unix };
            }
            return Footfall::Known { seen, source: d.footfall_source.unwrap_or_default() };
        }
        match p.own {
            Some(_) if you_landed && first_scan_unclaimed => Footfall::Yours { at: 0 },
            Some(_) if you_landed => Footfall::Landed,
            Some(o) => o.was_footfalled.map_or(Footfall::Unknown, |value| Footfall::Known { seen: Seen { value, at_unix: o.scanned.0 }, source: Source::Journal }),
            None => Footfall::Unknown,
        }
    }

    /// Step 5, a rule of thumb: nobody has sent Spansh anything about the body since Odyssey
    /// brought on-foot play, so probably nobody has walked on it.
    fn pre_odyssey(p: &Planet) -> Footfall {
        let seen_since = p.db.is_some_and(|d| d.updated_unix >= ODYSSEY_LAUNCH) || p.own.is_some();
        match p.sp.and_then(|s| s.updated_unix) {
            Some(last) if last < ODYSSEY_LAUNCH && !seen_since => Footfall::PreOdyssey { last },
            _ => Footfall::Unknown,
        }
    }

    /// Whether somebody else had set foot there before the commander could (the first-logged bonus
    /// heuristic).
    fn footfalled(self) -> Option<bool> {
        match self {
            Footfall::Known { seen, .. } => Some(seen.value),
            Footfall::Landed => Some(true),
            Footfall::Yours { .. } | Footfall::PreOdyssey { .. } => Some(false),
            Footfall::Populated | Footfall::Unknown => None,
        }
    }

    /// `{"status", "as_of", "source"}`.
    fn json(self) -> Value {
        match self {
            Footfall::Populated => json!({ "status": "not possible (populated)" }),
            Footfall::Known { seen, source } => json!({
                "status": if seen.value { "footfalled" } else { "not footfalled" },
                "as_of": ago(seen.at_unix),
                "source": source_label(source),
            }),
            Footfall::Landed => json!({ "status": "footfalled", "source": "your journal (you landed there)" }),
            Footfall::Yours { at } => {
                let mut v = json!({ "status": "footfalled", "source": "your journal: you took the first footfall" });
                if at > 0 {
                    v["as_of"] = json!(ago(at));
                }
                v
            }
            Footfall::PreOdyssey { last } => json!({
                "status": "probably unclaimed",
                "as_of": ago(last),
                "source": "Spansh: no data on this body since before Odyssey (May 2021)",
            }),
            Footfall::Unknown => json!({ "status": "unknown" }),
        }
    }
}

fn source_label(source: Source) -> &'static str {
    match source {
        Source::Journal => "your journal",
        Source::Eddn => "EDDN",
        Source::EddnArchive => "EDDN archive (EDGalaxyData)",
        Source::Spansh => "Spansh",
    }
}

/// Footfall of one body in archived EDDN messages (EDGalaxyData's EDDN index, messages since the flag
/// exists): the newest `ScanOrganic` (somebody was on foot there with a genetic sampler) or the newest
/// `Scan` that kept `WasFootfalled`, as an observation to merge. `Ok(None)`: nothing usable;
/// `Err`: the service didn't answer.
async fn archive_footfall(http: &reqwest::Client, system_id64: u64, body_id: u32) -> Result<Option<BodyUpdate>, String> {
    let url = format!("{EDGD_URL}/bodies/{system_id64}/{body_id}?minDate={FOOTFALL_KEY_SINCE}&limitMatches=5");
    let list = get_json(http, &url).await?;
    let mut matches: Vec<(i64, &Value)> = list
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|b| b.get("Matches").and_then(Value::as_array).into_iter().flatten())
        .filter_map(|m| Some((nexus_spansh::parse_timestamp(m.get("Timestamp")?.as_str()?)?, m)))
        .collect();
    matches.sort_by_key(|m| std::cmp::Reverse(m.0));
    let mut fetched = 0;
    for (ts, m) in matches {
        let text = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or_default();
        let event = text("EventType");
        if event.eq_ignore_ascii_case("ScanOrganic") {
            return Ok(Some(BodyUpdate { system_id64, body_id, was_footfalled: Some(true), timestamp: ts, source: Source::EddnArchive, ..BodyUpdate::default() }));
        }
        let software = text("SoftwareName").to_ascii_lowercase();
        if !event.eq_ignore_ascii_case("Scan") || STRIPS_FOOTFALL.iter().any(|s| software.contains(s)) || !carries_footfall(text("GameVersion")) {
            continue;
        }
        let extract = text("Extract");
        if !extract.starts_with(EDGD_URL) {
            continue;
        }
        if fetched == EXTRACTS_PER_BODY {
            break;
        }
        fetched += 1;
        let envelope = get_json(http, extract).await?;
        let scan = Ingest::from_eddn(&envelope).into_iter().find_map(|i| match i {
            Ingest::Body(b) if b.system_id64 == system_id64 && b.body_id == body_id && b.was_footfalled.is_some() => Some(b),
            _ => None,
        });
        if let Some(mut b) = scan {
            b.source = Source::EddnArchive;
            return Ok(Some(b));
        }
    }
    Ok(None)
}

async fn get_json(http: &reqwest::Client, url: &str) -> Result<Value, String> {
    let resp = http.get(url).send().await.map_err(|e| e.to_string())?;
    resp.error_for_status().map_err(|e| e.to_string())?.json::<Value>().await.map_err(|e| e.to_string())
}

/// A journal game version that writes `WasFootfalled` (4.2.1 or newer); unparsable ones get a try.
fn carries_footfall(game_version: &str) -> bool {
    let v: Vec<u32> = game_version.split('.').map_while(|p| p.trim().parse().ok()).take(3).collect();
    v.len() < 2 || v >= vec![4, 2, 1]
}

/// What `planet_json` needs to know about the system.
struct PlanetCtx<'a> {
    system: &'a str,
    pos: Option<[f64; 3]>,
    region: Option<&'a str>,
    populated: bool,
    own: Option<&'a SystemScan>,
}

/// Likely species of a bio body: the commander's own predictions when they scanned it, else the
/// prediction engine fed from Spansh (parent stars, materials) or the galaxy database.
fn predictions(p: &Planet, ctx: &PlanetCtx) -> Vec<Prediction> {
    let bio = p.bio();
    if bio == 0 {
        return Vec::new();
    }
    if let Some(o) = p.own
        && !o.predictions.is_empty()
    {
        return o.predictions.clone();
    }
    let facts = if let Some(s) = p.sp {
        let mut f = s.facts.clone();
        // the journal's exact atmosphere type beats parsing Spansh's text
        if let Some(t) = p.db.and_then(|d| d.atmosphere_type.clone()) {
            f.atmosphere_type = Some(t);
        }
        Some(f)
    } else if let Some(d) = p.db.filter(|d| d.planet_class.is_some()) {
        Some(BodyFacts {
            planet_class: d.planet_class.clone().unwrap_or_default(),
            atmosphere_type: d.atmosphere_type.clone(),
            atmosphere: d.atmosphere.clone(),
            gravity_g: d.gravity_g.map(|g| g * STANDARD_G / bp::JOURNAL_G_MS2),
            temperature_k: d.temperature_k,
            pressure_atm: d.pressure_atm.map(|x| x * STANDARD_ATM / bp::JOURNAL_ATM_PA),
            volcanism: d.volcanism.clone(),
            distance_from_arrival_ls: d.distance_ls.unwrap_or(0.0),
            system_name: ctx.system.to_string(),
            star_pos: ctx.pos,
            ..BodyFacts::default()
        })
    } else {
        p.own.zip(ctx.own).and_then(|(o, sys)| nexus_model::exploration::body_facts(o, sys, None))
    };
    let Some(mut facts) = facts else { return Vec::new() };
    facts.landable = true; // biology needs a surface
    facts.bio_signal_count = Some(bio);
    if facts.region.is_none() {
        facts.region = ctx.region.map(str::to_string);
    }
    bp::predict(&facts, &p.genera())
}

/// (has biology, expected value, distance, JSON) of one body.
fn planet_json(p: &Planet, footfall: &Footfall, preds: &[Prediction], ctx: &PlanetCtx) -> (bool, i64, f64, Value) {
    let bio = p.bio();
    let bonus = bp::first_logged_bonus_likely(footfall.footfalled(), ctx.populated, None);
    let expected: i64 = preds.iter().map(|x| bp::expected_value(x, bonus)).sum();
    // footfall unknown: what the ×5 bonus would make of it, should nobody have landed yet
    let if_first: Option<i64> = (matches!(footfall, Footfall::Unknown) && !bonus && expected > 0).then(|| preds.iter().map(|x| bp::expected_value(x, true)).sum());
    let mut seen_genera: Vec<&str> = Vec::new();
    let likely: Vec<Value> = preds
        .iter()
        .filter(|x| {
            let first = !seen_genera.contains(&x.genus.as_str());
            seen_genera.push(&x.genus);
            first
        })
        .take(4)
        .map(|x| json!({ "species": x.species, "chance_pct": (x.likelihood * 100.0).round(), "value_cr": x.value }))
        .collect();
    let genera: Vec<String> = p
        .genera()
        .iter()
        .map(|g| nexus_data::exobio::genus(g).map_or_else(|| nexus_data::names::humanize_symbol(g), |x| x.name.to_string()))
        .collect();
    let distance = p.sp.and_then(|s| s.distance_ls).or_else(|| p.db.and_then(|d| d.distance_ls)).or_else(|| p.own_scan().map(|o| o.distance_ls));
    let gravity = p.db.and_then(|d| d.gravity_g).or_else(|| p.own_scan().and_then(|o| o.gravity_ms2).map(|g| g / STANDARD_G)).or_else(|| p.sp.and_then(|s| s.gravity_g));
    let temperature = p.db.and_then(|d| d.temperature_k).or_else(|| p.own_scan().and_then(|o| o.temperature_k)).or_else(|| p.sp.and_then(|s| s.temperature_k));
    let atmosphere = p
        .db
        .and_then(|d| d.atmosphere.clone())
        .or_else(|| p.own_scan().and_then(|o| o.atmosphere.clone()))
        .or_else(|| p.sp.and_then(|s| s.atmosphere.clone()));
    let volcanism = p.db.and_then(|d| d.volcanism.clone()).or_else(|| p.own_scan().and_then(|o| o.volcanism.clone())).or_else(|| p.sp.and_then(|s| s.volcanism.clone()));
    let discovered = p.db.and_then(|d| d.was_discovered.map(|s| s.value)).or_else(|| p.own.and_then(|o| o.was_discovered));
    let mapped = p.db.and_then(|d| d.was_mapped.map(|s| s.value)).or_else(|| p.own.and_then(|o| o.was_mapped));
    let mut v = json!({
        "name": short_name(&p.name(), ctx.system),
        "type": p.class(),
        "distance_ls": distance.map(f64::round),
        "landable": p.landable(),
        "gravity_g": gravity.map(|g| (g * 100.0).round() / 100.0),
        "temperature_k": temperature.map(f64::round),
        "atmosphere": atmosphere,
        "volcanism": volcanism,
        "terraformable": p.terraform().map(|_| true),
        "bio_signals": (bio > 0).then_some(bio),
        "geo_signals": Some(p.geo()).filter(|g| *g > 0),
        "genera": (!genera.is_empty()).then_some(genera),
        "footfall": footfall.json(),
        "discovered": discovered,
        "mapped": mapped,
        "likely_species": (!likely.is_empty()).then_some(likely),
        "expected_value_cr": (expected > 0).then_some(expected),
        "first_logged_bonus_likely": (bio > 0).then_some(bonus),
        "expected_value_if_first_logged_cr": if_first,
    });
    strip_nulls(&mut v);
    (bio > 0, expected, distance.unwrap_or(f64::MAX), v)
}

/// "5 hours ago", "3 days ago", "on 2020-11-03".
fn ago(unix: i64) -> String {
    let s = (nexus_journal::Timestamp::now().0 - unix).max(0);
    match s {
        0..3600 => "less than an hour ago".to_string(),
        3600..172_800 => match s / 3600 {
            1 => "1 hour ago".to_string(),
            h => format!("{h} hours ago"),
        },
        172_800..5_184_000 => format!("{} days ago", s / 86_400),
        _ => format!("on {}", nexus_journal::Timestamp(unix).to_iso().get(..10).unwrap_or_default()),
    }
}

/// "Swoilz MH-W c17-3 A 4" -> "A 4" (named bodies keep their name).
fn short_name<'a>(name: &'a str, system: &str) -> &'a str {
    match name.get(..system.len()) {
        Some(head) if !system.is_empty() && head.eq_ignore_ascii_case(system) && name[system.len()..].starts_with(' ') => name[system.len()..].trim(),
        _ => name,
    }
}

/// The body filter of `system_bodies`: the full name, or the part after the system name ("A 3").
fn body_matches(name: &str, system: &str, filter: &str) -> bool {
    let f = filter.trim();
    name.eq_ignore_ascii_case(f) || short_name(name, system).eq_ignore_ascii_case(f) || name.to_ascii_lowercase().ends_with(&format!(" {}", f.to_ascii_lowercase()))
}

fn strip_nulls(v: &mut Value) {
    if let Value::Object(m) = v {
        m.retain(|_, x| !x.is_null());
    }
}

/// `s` cut to at most `max` bytes, on a character boundary.
fn clip_at(s: &str, max: usize) -> &str {
    let mut end = max.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// A knowledge section cut to about `cap` characters around what was asked: the lines holding most of the question's
/// words, in their order, "…" where lines were left out; a line longer than what's left shows the stretch around its
/// first match. Whole when it fits. Returns the text and whether it was cut.
fn excerpt(text: &str, query: &str, cap: usize) -> (String, bool) {
    if text.len() <= cap {
        return (text.to_string(), false);
    }
    let words: Vec<String> = query.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 3).map(str::to_lowercase).collect();
    let lines: Vec<&str> = text.lines().collect();
    let found = |l: &str| {
        let low = l.to_lowercase();
        words.iter().filter_map(|w| low.find(w.as_str())).collect::<Vec<usize>>()
    };
    let mut ranked: Vec<(usize, usize)> = lines.iter().enumerate().map(|(i, l)| (found(l).len(), i)).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut kept: Vec<Option<String>> = vec![None; lines.len()];
    let mut left = cap;
    for (hits, i) in ranked {
        let line = lines[i];
        if line.len() < left {
            left -= line.len() + 1;
            kept[i] = Some(line.to_string());
        } else if hits > 0 && left >= 400 {
            // The stretch of a long line (a row of the user guide's tables) around what was asked.
            let at = found(line).into_iter().min().unwrap_or(0);
            let start = clip_at(line, at.saturating_sub(left / 4)).len();
            let piece = clip_at(&line[start..], left - 2);
            kept[i] = Some(format!("{}{piece}{}", if start > 0 { "…" } else { "" }, if start + piece.len() < line.len() { "…" } else { "" }));
            left = 0;
        }
    }
    let mut out = String::new();
    let mut gap = false;
    for k in kept {
        match k {
            Some(l) => {
                if gap && !out.is_empty() {
                    out.push_str("…\n");
                }
                out.push_str(&l);
                out.push('\n');
                gap = false;
            }
            None => gap = true,
        }
    }
    if gap {
        out.push('…');
    }
    (out, true)
}

/// The last word on length: a reply still over the caller's budget is cut, and says so. No tool should need it.
fn within(v: Value, budget: usize) -> Value {
    let len = json_len(&v);
    if len <= budget {
        return v;
    }
    let text = v.to_string();
    let note = format!("This answer was {len} characters, more than the {budget} this caller takes: here is its beginning, cut off. Ask for less to get all of it.");
    // Quotes in the text are escaped once more inside the string: the longest beginning that still fits.
    fit(budget, 20, |level| json!({ "partial_json": clip_at(&text, budget * (20 - level) / 20), "note": note })).0
}

impl NexusTools {
    fn knowledge(&self, a: &Value) -> Result<Value, String> {
        let q = arg_str(a, "query").ok_or("query is required")?;
        let hits = nexus_ai::knowledge::search(&q, 4);
        // Each section gets an equal share of the caller's length; a longer one is cut to what was asked about.
        let share = self.budget.saturating_sub(1_000) / hits.len().max(1);
        let mut cut = false;
        let sections: Vec<Value> = hits
            .into_iter()
            .map(|(title, text)| {
                let (text, c) = excerpt(&text, &q, share);
                cut |= c;
                json!({ "title": title, "text": text })
            })
            .collect();
        let mut out = json!({ "sections": sections });
        if cut {
            out["note"] = json!("Long sections are cut to the lines about what was asked (… marks what was left out): ask a narrower question for more of one.");
        }
        Ok(out)
    }
}

impl ToolHost for NexusTools {
    fn tools(&self) -> Vec<ToolSpec> {
        specs(self.caller)
    }

    fn call(&self, name: &str, args: &Value) -> Result<Value, String> {
        match tool(name) {
            Some(t) if t.offered_to(self.caller) => (t.run)(self, args).map(|v| within(v, self.budget)),
            _ => Err(format!("Unknown tool \"{name}\".")),
        }
    }
}

/// One tool, everything about it in one place.
pub struct Tool {
    pub name: &'static str,
    /// What the commander reads: Ask's list of what it can see and do, and an agent's display name for it.
    pub title: &'static str,
    /// What the model reads to decide when to call it.
    pub description: &'static str,
    /// Its arguments, as a JSON schema.
    pub params: fn() -> Value,
    /// Ask's status line while it runs.
    pub working: &'static str,
    /// What Ask's "used" line under an answer calls it.
    pub noun: &'static str,
    /// Only reads: it changes nothing in Elite Nexus or the game (caching what it looked up aside).
    pub read_only: bool,
    /// Offered to Ask's model. Every offered tool's description and schema go with every question, so only the tools
    /// that earn those tokens.
    pub ask: bool,
    /// Offered to AI agents over MCP. Read-only tools only.
    pub mcp: bool,
    /// Answers from an online service (Spansh, EDGalaxyData) rather than the commander's own data and the local
    /// database: for agents, MCP's openWorldHint, and the tighter limit on calls a minute.
    pub online: bool,
    run: fn(&NexusTools, &Value) -> Result<Value, String>,
}

impl Tool {
    pub fn offered_to(&self, caller: Caller) -> bool {
        match caller {
            Caller::Ask => self.ask,
            Caller::Mcp => self.mcp && self.read_only,
        }
    }

    pub fn spec(&self) -> ToolSpec {
        ToolSpec { name: self.name, description: self.description, parameters: (self.params)() }
    }
}

/// The tool called `name`.
pub fn tool(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// Ask's status line while a tool runs.
pub fn working(name: &str) -> &'static str {
    tool(name).map_or("Working…", |t| t.working)
}

/// What Ask's "used" line calls a tool.
pub fn noun(name: &str) -> &'static str {
    tool(name).map_or("tools", |t| t.noun)
}

/// The tools a caller is offered, in the order of [`TOOLS`]. Kept stable: providers cache the list.
pub fn specs(caller: Caller) -> Vec<ToolSpec> {
    TOOLS.iter().filter(|t| t.offered_to(caller)).map(Tool::spec).collect()
}

/// Every tool, in the order the model sees them.
pub static TOOLS: &[Tool] = &[
    Tool {
        name: "commander",
        title: "Your ship, location and credits",
        description: "The commander's current state: location, docked station, ship (type, landing pad, cargo capacity, jump range, fuel, rebuy), cargo hold, credits, fleet carrier and squadron carrier, ranks, unsold exploration and exobiology data.",
        params: || schema(json!({}), &[]),
        working: "Checking your ship…",
        noun: "your ship",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: |t, _| t.commander(),
    },
    Tool {
        name: "find_commodity",
        title: "Where to buy or sell a commodity",
        description: "Where to buy or sell one commodity near a system, from the live market database (EDDN, refreshed in real time). Results respect the ship's landing pad size.",
        params: || {
            schema(
                json!({
                    "commodity": { "type": "string", "description": "Commodity name, e.g. Tritium, Gold, Low Temperature Diamonds." },
                    "mode": { "type": "string", "enum": ["buy", "sell"], "description": "buy = stations selling it (you buy); sell = stations buying it (you sell)." },
                    "near_system": { "type": "string", "description": NEAR },
                    "max_distance_ly": { "type": "number", "description": "Search radius, default 100." },
                    "min_quantity": { "type": "integer", "description": "Minimum stock (buy) or demand (sell)." },
                    "sort": { "type": "string", "enum": ["price", "distance"] },
                    "include_carriers": { "type": "boolean" },
                    "limit": { "type": "integer" }
                }),
                &["commodity"],
            )
        },
        working: "Searching markets…",
        noun: "markets",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::find_commodity,
    },
    Tool {
        name: "price_deals",
        title: "Price deals near you",
        description: "Best deals and abnormal prices right now: stations and fleet carriers paying far above the galactic average for any commodity (mode sell) or selling far below it (mode buy), ranked by what one full hold of the commander's ship earns. The fastest way to find unusually profitable trades. For abnormal prices anywhere (most are fleet-carrier orders, e.g. a carrier paying 100× the normal price), use max_distance_ly 0 and min_ratio 2 or more.",
        params: || {
            schema(
                json!({
                    "mode": { "type": "string", "enum": ["sell", "buy"] },
                    "near_system": { "type": "string", "description": NEAR },
                    "max_distance_ly": { "type": "number", "description": "Default 80. 0 = anywhere in the galaxy." },
                    "min_ratio": { "type": "number", "description": "How much better than average, e.g. 1.3 = 30 % (default); 2 = double; 10 = ten times." },
                    "max_age_hours": { "type": "number", "description": "Only prices reported in the last N hours. Default 48." },
                    "include_carriers": { "type": "boolean" },
                    "limit": { "type": "integer" }
                }),
                &[],
            )
        },
        working: "Looking for deals…",
        noun: "deals",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::price_deals,
    },
    Tool {
        name: "untouched_biology",
        title: "Untouched biology",
        description: "Find planets where a valuable exobiology species should grow (default Stratum Tectonicas, 19 M Cr, 95 M with the first-logged bonus) and that no data-sharing commander has DSS-mapped or sampled, so the ×5 first-logged bonus is likely still available. Searches Spansh around a system, leaves out populated systems and planets known to be footfalled, ranks by expected bonus value and distance.",
        params: || {
            schema(
                json!({
                    "species": { "type": "string", "description": "e.g. Stratum Tectonicas (default), Concha Biconcavis, Tussock Stigmasis, Fonticulua Segmentatus, Fonticulua Fluctus." },
                    "near_system": { "type": "string", "description": NEAR },
                    "max_distance_ly": { "type": "number", "description": "Default 500." },
                    "include_mapped": { "type": "boolean", "description": "Also list planets someone DSS-mapped (genus confirmed) but nobody sampled. Default true." },
                    "limit": { "type": "integer" }
                }),
                &[],
            )
        },
        working: "Looking for untouched planets…",
        noun: "untouched planets",
        read_only: true,
        ask: true,
        mcp: true,
        online: true,
        run: NexusTools::untouched_biology,
    },
    Tool {
        name: "sell_my_cargo",
        title: "Where to sell your cargo",
        description: "Where to sell everything currently in the hold: the best single stations (limited by their demand) and the best buyer for each item.",
        params: || schema(json!({ "max_distance_ly": { "type": "number", "description": "Default 60." }, "near_system": { "type": "string", "description": NEAR } }), &[]),
        working: "Planning your cargo sale…",
        noun: "your cargo",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::sell_my_cargo,
    },
    Tool {
        name: "trade_routes",
        title: "Trade routes",
        description: "Best buy-here-sell-there trade routes near a system, sized to the ship's cargo capacity and the commander's credits.",
        params: || {
            schema(
                json!({
                    "near_system": { "type": "string", "description": NEAR },
                    "radius_ly": { "type": "number", "description": "Both stations within this distance, default 40." },
                    "max_hop_ly": { "type": "number", "description": "Maximum distance between the two stations, default 25." },
                    "limit": { "type": "integer" }
                }),
                &[],
            )
        },
        working: "Finding trade routes…",
        noun: "trade routes",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::trade_routes,
    },
    Tool {
        name: "nearest_service",
        title: "The nearest station with a service",
        description: "Nearest stations with a service, for the commander's landing pad size.",
        params: || {
            schema(
                json!({
                    "service": { "type": "string", "enum": ["raw_material_trader", "manufactured_material_trader", "encoded_material_trader", "human_tech_broker", "guardian_tech_broker", "interstellar_factors", "universal_cartographics", "vista_genomics", "shipyard", "outfitting", "black_market", "refuel", "repair"] },
                    "near_system": { "type": "string", "description": NEAR },
                    "limit": { "type": "integer" }
                }),
                &["service"],
            )
        },
        working: "Finding stations…",
        noun: "stations",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::nearest_service,
    },
    Tool {
        name: "mining_hotspots",
        title: "Ring hotspots",
        description: "Known planetary-ring hotspots for a mineable commodity near a system, plus the best nearby places to sell it.",
        params: || {
            schema(
                json!({
                    "commodity": { "type": "string", "description": "e.g. Platinum, Painite, Low Temperature Diamonds, Void Opal, Tritium." },
                    "near_system": { "type": "string", "description": NEAR },
                    "max_distance_ly": { "type": "number", "description": "Default 100." },
                    "limit": { "type": "integer" }
                }),
                &["commodity"],
            )
        },
        working: "Finding hotspots…",
        noun: "hotspots",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::mining_hotspots,
    },
    Tool {
        name: "mining_sites",
        title: "Your Rhino mining sites",
        description: "Planetary mining locations the commander worked with the Rhino SRV (surface mining), newest first: system, body, the location's number as the game's navigation panel shows it, tonnes refined per commodity (by-products included), visits and the last visit. Use for \"where did I mine …\" or \"where was that site\" questions.",
        params: || {
            schema(
                json!({
                    "commodity": { "type": "string", "description": "Only locations that gave this, e.g. Iridium." },
                    "limit": { "type": "integer", "description": "Default 10." }
                }),
                &[],
            )
        },
        working: "Reading your mining sites…",
        noun: "your mining sites",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::mining_sites,
    },
    Tool {
        name: "system_info",
        title: "A system's facts",
        description: "Facts about a star system from the local database: distance from the commander, population, economy, security, factions and their states, powerplay, stations.",
        params: || schema(json!({ "system": { "type": "string" } }), &["system"]),
        working: "Looking up the system…",
        noun: "system data",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::system_info,
    },
    Tool {
        name: "system_bodies",
        title: "A system's planets, footfall and biology",
        description: "The planets and moons of a star system worth landing on or scanning: type, distance, gravity, atmosphere, biological and geological signals, genera, the most likely species with their values and whether the ×5 first-logged bonus looks likely, plus discovery and footfall status: whether anyone has set foot on a body. No public site tracks footfall; it comes from the game's flag in the commander's own journal, in scans other commanders shared over EDDN, and in archived EDDN scans looked up online for biology bodies (EDGalaxyData), each with its age; \"probably unclaimed\" means Spansh has no data on the body since before Odyssey (a rule of thumb). Use it for any question about a planet, its biology or first footfall.",
        params: || {
            schema(
                json!({
                    "system": { "type": "string", "description": "Star system name." },
                    "body": { "type": "string", "description": "Only this body: its full name, or the part after the system name such as \"A 3\" or \"2 a\"." }
                }),
                &["system"],
            )
        },
        working: "Checking the planets…",
        noun: "planets",
        read_only: true,
        ask: true,
        mcp: true,
        online: true,
        run: NexusTools::system_bodies,
    },
    Tool {
        name: "plan_route",
        title: "Plan a route",
        description: "Plans a route with Spansh: the exact plotter uses the commander's real ship (fuel, scooping, neutron boosts); the neutron plotter only needs the jump range. The route is put on the Navigation page and the next system is copied to the clipboard after every jump.",
        params: || {
            schema(
                json!({
                    "to": { "type": "string", "description": "Destination system." },
                    "from": { "type": "string", "description": "Start system; default: current." },
                    "kind": { "type": "string", "enum": ["exact", "neutron"] }
                }),
                &["to"],
            )
        },
        working: "Plotting a route with Spansh…",
        noun: "Spansh",
        // It replaces the route on the Navigation page.
        read_only: false,
        ask: true,
        mcp: false,
        online: true,
        run: NexusTools::plan_route,
    },
    Tool {
        name: "money_routes",
        title: "Routes that make money",
        description: "Plans a money-making exploration trip with Spansh near a system: exobiology (planets where commanders already logged valuable species: certain, but base value only; for the first-logged bonus use untouched_biology), or road-to-riches bodies (earth-likes, water/ammonia worlds, terraformables). Spansh skips populated systems, so from the bubble the search widens itself until it finds something. Returns stops with values, the total and, where known, whether anyone has set foot on the listed bodies; the route goes to the Navigation page.",
        params: || {
            schema(
                json!({
                    "kind": { "type": "string", "enum": ["exobiology", "road_to_riches", "earth_likes", "ammonia_worlds"] },
                    "near_system": { "type": "string", "description": NEAR },
                    "radius_ly": { "type": "number", "description": "Search radius, 1 to 1,000 ly. Default 30; widened automatically when nothing is found." },
                    "max_systems": { "type": "integer", "description": "Default 15." }
                }),
                &[],
            )
        },
        working: "Planning a money route with Spansh…",
        noun: "Spansh",
        // It replaces the route on the Navigation page.
        read_only: false,
        ask: true,
        mcp: false,
        online: true,
        run: NexusTools::money_routes,
    },
    Tool {
        name: "exobiology_here",
        title: "Exobiology where you are",
        description: "Bodies with biological signals in the current system: genera found, species already sampled, likely species with their values (predictions), and unsold exobiology data.",
        params: || schema(json!({}), &[]),
        working: "Checking the biology here…",
        noun: "biology",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: |t, _| t.exobiology_here(),
    },
    Tool {
        name: "journal_search",
        title: "Search your journal",
        description: "Searches the commander's own game journal (every journal file in their journal folder; journal_starts is the first day it holds) for lines with a text, newest first, each with its local time. One call returns up to limit matches, fewer when the lines are long; more_matches says when older ones were left out and how to get them. For a period, such as a week of travel, pass from and to. Event names work as text: FSDJump (jumps), Docked, MarketSell, MissionCompleted. Use for questions about past trips, sales, missions, ships, places visited.",
        params: || {
            schema(
                json!({
                    "query": { "type": "string", "description": "Text to find, e.g. a system, station, ship, commodity or event name." },
                    "from": { "type": "string", "description": "First day to search, YYYY-MM-DD (the commander's local date)." },
                    "to": { "type": "string", "description": "Last day to search, YYYY-MM-DD, included." },
                    "skip": { "type": "integer", "description": "Leave out this many of the newest matches: the next page." },
                    "limit": { "type": "integer", "description": "Default 15, at most 40." }
                }),
                &["query"],
            )
        },
        working: "Searching your journal…",
        noun: "your journal",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::journal_search,
    },
    Tool {
        name: "knowledge",
        title: "Game mechanics and this guide",
        description: "Searches the built-in knowledge base: game mechanics, money-making strategies, exobiology, mining, engineering, carriers, colonisation, useful websites, and how to use Elite Nexus.",
        params: || schema(json!({ "query": { "type": "string" } }), &["query"]),
        working: "Reading up…",
        noun: "knowledge base",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::knowledge,
    },
    Tool {
        name: "fleet",
        title: "Your fleet",
        description: "Every ship the commander owns: where it is (or that it's in transit and when it arrives), jump range, cargo, value, rebuy and its engineered modules (blueprint, grade, experimental effect). ship= picks one ship (name, ident or type); all_modules=true adds every module of that ship, or of the current ship. Modules are those of the last loadout the game reported for that ship.",
        params: || {
            schema(
                json!({
                    "ship": { "type": "string", "description": "Only this ship: its name, ident, type (e.g. Python) or id. Default: every ship." },
                    "all_modules": { "type": "boolean", "description": "Also list every module, not only the engineered ones: of the ship picked with ship, or of the current ship." }
                }),
                &[],
            )
        },
        working: "Reading your fleet…",
        noun: "your fleet",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::fleet,
    },
    Tool {
        name: "progress",
        title: "Your ranks and engineers",
        description: "The commander's progress: every rank with its progress to the next, engineers (unlocked with their rank, or invited/known), superpower reputation, Powerplay, squadron, and whether the game is running or at the main menu.",
        params: || schema(json!({}), &[]),
        working: "Checking your ranks and engineers…",
        noun: "your ranks and engineers",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: |t, _| t.progress(),
    },
    Tool {
        name: "materials",
        title: "Your materials",
        description: "Engineering materials the commander holds (raw, manufactured, encoded), each with its grade, storage cap and whether it's full. Filter by category or by part of a name.",
        params: || {
            schema(
                json!({
                    "category": { "type": "string", "enum": ["raw", "manufactured", "encoded"], "description": "Only this category. Default: all three." },
                    "name": { "type": "string", "description": "Only materials whose name contains this (e.g. polonium, wake echoes)." }
                }),
                &[],
            )
        },
        working: "Counting your materials…",
        noun: "your materials",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::materials,
    },
    Tool {
        name: "on_foot",
        title: "Your on-foot gear",
        description: "On-foot equipment: suit loadouts (suit, modifications, weapons and their modifications), the one in use, and what's in the ship locker and backpack (items, components, consumables, data).",
        params: || schema(json!({}), &[]),
        working: "Checking your on-foot gear…",
        noun: "your on-foot gear",
        read_only: true,
        // Rarely asked: not worth its tokens in every Ask request.
        ask: false,
        mcp: true,
        online: false,
        run: |t, _| t.on_foot(),
    },
    Tool {
        name: "credit_history",
        title: "Your credit history",
        description: "Credits earned and spent since the game was last loaded (or in the last since_hours), newest first, with totals per category. Built from the journal: a few game actions record no amount, so the list may not add up to the balance exactly.",
        params: || {
            schema(
                json!({
                    "limit": { "type": "integer", "description": "How many transactions to list, newest first. Default 25, at most 100." },
                    "since_hours": { "type": "number", "description": "Look back this many hours instead of since the game was last loaded." }
                }),
                &[],
            )
        },
        working: "Reading your credit history…",
        noun: "your credit history",
        read_only: true,
        // Rarely asked: not worth its tokens in every Ask request.
        ask: false,
        mcp: true,
        online: false,
        run: NexusTools::credit_history,
    },
    Tool {
        name: "system_factions",
        title: "The factions you met",
        description: "Factions in a system as the commander last saw it (default: the current system): influence, state, active, pending and recovering states, happiness and the commander's own reputation with each. Only for systems in the commander's journal; system_info has EDDN faction data for any system, without reputation.",
        params: || schema(json!({ "system": { "type": "string", "description": "System name. Default: the commander's current system." } }), &[]),
        working: "Reading the factions you saw there…",
        noun: "your journal",
        read_only: true,
        // Rarely asked: not worth its tokens in every Ask request.
        ask: false,
        mcp: true,
        online: false,
        run: NexusTools::system_factions,
    },
    Tool {
        name: "find_material_body",
        title: "Where to find a raw material",
        description: "Landable bodies near a system that hold a raw material (e.g. Polonium), from Spansh: the richest within a distance (default 100 ly), or the nearest. Spansh knows a body's materials only from scans commanders shared.",
        params: || {
            schema(
                json!({
                    "material": { "type": "string", "description": "A raw material, e.g. Polonium, Tellurium, Niobium, Arsenic." },
                    "near_system": { "type": "string", "description": NEAR },
                    "max_distance_ly": { "type": "number", "description": "Search radius, 1 to 1,000 ly. Default 100." },
                    "sort": { "type": "string", "enum": ["richest", "nearest"], "description": "Default richest." },
                    "max_gravity_g": { "type": "number", "description": "Highest surface gravity in g. Default 1.0." },
                    "max_distance_ls": { "type": "number", "description": "Farthest from the arrival star, in ls. Default: any." }
                }),
                &["material"],
            )
        },
        working: "Asking Spansh where to find it…",
        noun: "Spansh",
        read_only: true,
        ask: true,
        mcp: true,
        online: true,
        run: NexusTools::find_material_body,
    },
    Tool {
        name: "engineering",
        title: "Engineering",
        description: "Ship engineering, from the Engineering page's data. module: its blueprints, experimental effects and engineers. module and blueprint: what each grade takes against what the commander holds, what's short, material-trader swaps that cover it, and the engineers offering that grade (unlocked or not, how far, how to unlock). engineer: where they are, what they offer, how to unlock them.",
        params: || {
            schema(
                json!({
                    "module": { "type": "string", "description": "e.g. Frame Shift Drive, Thrusters, Shield Booster." },
                    "blueprint": { "type": "string", "description": "e.g. Increased FSD Range, Dirty Drive Tuning." },
                    "grade": { "type": "integer", "description": "Grade to reach. Default: the highest." },
                    "from_grade": { "type": "integer", "description": "Default 1, or the grade the current ship's module has of that blueprint." },
                    "rolls_per_grade": { "type": "integer", "description": "1 to 10. Default 3." },
                    "experimental": { "type": "string", "description": "e.g. Mass Manager." },
                    "engineer": { "type": "string", "description": "An engineer's name." }
                }),
                &[],
            )
        },
        working: "Looking up the blueprints…",
        noun: "the engineering data",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::engineering,
    },
    Tool {
        name: "where_to_mine",
        title: "Where to surface-mine",
        description: "Rhino surface mining: landable bodies with planetary mining locations near a system, or the best system in each stretch of the way to to_system, from the scans commanders shared that Spansh holds; the odds of each material on each body's ground (how often it was listed at the locations commanders read in the game); and what the commander's own runs gave on each ground (class and volcanism). Nothing records what a given location holds.",
        params: || {
            schema(
                json!({
                    "near_system": { "type": "string", "description": NEAR },
                    "to_system": { "type": "string", "description": "Search along the way from near_system (default: the commander's) to here instead." },
                    "class": { "type": "string", "enum": ["metal-rich", "high metal content", "rocky", "rocky ice", "icy"] },
                    "volcanism": { "type": "string", "description": "e.g. rocky magma, metallic magma, water geysers, or none." },
                    "material": { "type": "string", "description": "A material the Rhino mines (Monazite, Platinum, Iridium, Low Temperature Diamonds…): without a class, only the classes where it turned up are searched, and each body gets its odds." },
                    "min_locations": { "type": "integer", "description": "Default 1." },
                    "max_distance_ly": { "type": "number", "description": "1 to 1,000. Default 100; along a route, from the line, 25." },
                    "max_distance_ls": { "type": "number", "description": "From the arrival star. Default: any." },
                    "sort": { "type": "string", "enum": ["nearest", "most_locations"], "description": "most_locations: with a material, the most locations likely to list it." },
                    "stops": { "type": "integer", "description": "Along a route: how many stretches. Default 10." }
                }),
                &[],
            )
        },
        working: "Asking Spansh where to mine…",
        noun: "Spansh",
        read_only: true,
        ask: true,
        mcp: true,
        online: true,
        run: NexusTools::where_to_mine,
    },
    Tool {
        name: "where_to_buy",
        title: "Where to buy a build",
        description: "The nearest stations that sell a ship build (pasted, or one of the commander's ships: by default the current one), or the stops that cover it between them, with cost and data age, from Spansh's outfitting lists. What the commander has isn't searched for: a hull of that type they own, the modules fitted on it (a build for a hull they own is a refit of it) and modules in storage, so one of their own ships' builds has nothing to buy. items= narrows a build to some of its modules or its hull; items alone (no ship or build) searches for those, also to buy them again.",
        params: || {
            schema(
                json!({
                    "ship": { "type": "string", "description": "One of the commander's ships: name, ident, type or id." },
                    "build": { "type": "string", "description": "Or a build: a SLEF export (EDSY, Coriolis), a journal Loadout or an import link." },
                    "items": { "type": "array", "items": { "type": "string" }, "description": "Names as outfitting shows them, e.g. 6A Frame Shift Drive, 2A Gimballed Multi-cannon, or a hull, e.g. Mandalay." },
                    "near_system": { "type": "string", "description": NEAR },
                    "carriers": { "type": "boolean", "description": "Fleet carriers too. Default true." },
                    "max_age_days": { "type": "integer", "description": "Leave out stations whose list is older. Default: any age." }
                }),
                &[],
            )
        },
        working: "Asking Spansh where to buy it…",
        noun: "Spansh",
        read_only: true,
        ask: true,
        mcp: true,
        online: true,
        run: NexusTools::where_to_buy,
    },
    Tool {
        name: "conflicts_near",
        title: "Wars near you",
        // Kept short: every Ask question carries it (ask_specs_stay_small). The reply explains itself.
        description: "Nearest wars and civil wars (conflict zones) from the local database: factions, days won, stakes, report age.",
        params: || {
            schema(
                json!({
                    "near_system": { "type": "string", "description": "Default: current system." },
                    "max_distance_ly": { "type": "number", "description": "Default 100." },
                    "elections": { "type": "boolean" }
                }),
                &[],
            )
        },
        working: "Looking for wars near you…",
        noun: "wars nearby",
        read_only: true,
        ask: true,
        mcp: true,
        online: false,
        run: NexusTools::conflicts_near,
    },
];

/// One line about the commander for each question (location, ship, credits, date).
pub fn context_line(gs: &GameState) -> String {
    let loc = &gs.location;
    let place = match loc.station.as_ref().filter(|_| loc.docked) {
        Some(s) => format!("docked at {} in {}", s.name, loc.system),
        None => format!("in {} ({})", loc.system, loc.situation()),
    };
    let ship = gs
        .fleet
        .current()
        .map(|s| {
            format!(
                "flying a {} ({} pad, {} t cargo, {:.1} ly jump)",
                s.type_name,
                finder::ship_pad(gs).unwrap_or('?'),
                s.cargo_capacity,
                s.max_jump_range
            )
        })
        .unwrap_or_default();
    format!(
        "CMDR {} {place}, {ship}, {} Cr; today is {}",
        gs.cmdr.name,
        compact_credits(gs.cmdr.credits),
        nexus_journal::Timestamp::now().to_local_string("%Y-%m-%d")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TempDb, good, station};
    use nexus_journal::{Event, EventCtx, raw::scan_line};
    use nexus_model::Alerts;

    /// A commander in Alpha flying an Anaconda with 100 t of gold aboard.
    fn commander() -> Model {
        let now = nexus_journal::Timestamp::now().to_iso();
        let lines = [
            format!(r#"{{ "timestamp":"{now}", "event":"Fileheader", "part":1, "language":"English/UK", "Odyssey":true, "gameversion":"4.4.1.1", "build":"r1 " }}"#),
            format!(r#"{{ "timestamp":"{now}", "event":"Commander", "FID":"F1", "Name":"Tester" }}"#),
            format!(r#"{{ "timestamp":"{now}", "event":"LoadGame", "FID":"F1", "Commander":"Tester", "Horizons":true, "Odyssey":true, "Ship":"Anaconda", "ShipID":1, "ShipName":"", "ShipIdent":"", "FuelLevel":32.0, "FuelCapacity":32.0, "GameMode":"Solo", "Credits":250000000, "Loan":0 }}"#),
            format!(r#"{{ "timestamp":"{now}", "event":"Location", "Docked":false, "StarSystem":"Alpha", "SystemAddress":1001, "StarPos":[0.0,0.0,0.0], "Population":1000, "Body":"Alpha", "BodyID":0, "BodyType":"Star" }}"#),
            format!(r#"{{ "timestamp":"{now}", "event":"Loadout", "Ship":"anaconda", "ShipID":1, "ShipName":"", "ShipIdent":"", "HullValue":100, "ModulesValue":100, "HullHealth":1.0, "UnladenMass":400.0, "CargoCapacity":400, "MaxJumpRange":30.5, "FuelCapacity":{{ "Main":32.0, "Reserve":1.07 }}, "Rebuy":5000000, "Modules":[ {{ "Slot":"FrameShiftDrive", "Item":"int_hyperdrive_size6_class5", "On":true, "Priority":0, "Health":1.0, "Value":3000000, "Engineering":{{ "Engineer":"Felicity Farseer", "EngineerID":300100, "BlueprintID":128673694, "BlueprintName":"FSD_LongRange", "Level":5, "Quality":1.0, "ExperimentalEffect":"special_fsd_heavy", "ExperimentalEffect_Localised":"Mass Manager", "Modifiers":[] }} }}, {{ "Slot":"PaintJob", "Item":"paintjob_anaconda_tactical_blue", "On":true, "Priority":1, "Health":1.0 }}, {{ "Slot":"ShipCockpit", "Item":"anaconda_cockpit", "On":true, "Priority":1, "Health":1.0 }} ] }}"#),
            format!(r#"{{ "timestamp":"{now}", "event":"Cargo", "Vessel":"Ship", "Count":100, "Inventory":[ {{ "Name":"gold", "Name_Localised":"Gold", "Count":100, "Stolen":0 }} ] }}"#),
            // A second ship in storage, and the progress the Rank, Progress and EngineerProgress events carry.
            format!(r#"{{ "timestamp":"{now}", "event":"StoredShips", "StationName":"Alpha Port", "MarketID":5001, "StarSystem":"Alpha", "ShipsHere":[ {{ "ShipID":2, "ShipType":"Python", "Name":"Mule", "Value":55000000, "Hot":false }} ], "ShipsRemote":[ ] }}"#),
            format!(r#"{{ "timestamp":"{now}", "event":"Rank", "Combat":3, "Trade":4, "Explore":6, "Soldier":0, "Exobiologist":2, "Empire":7, "Federation":1, "CQC":0 }}"#),
            format!(r#"{{ "timestamp":"{now}", "event":"Progress", "Combat":30, "Trade":46, "Explore":78, "Soldier":0, "Exobiologist":35, "Empire":37, "Federation":100, "CQC":0 }}"#),
            format!(r#"{{ "timestamp":"{now}", "event":"EngineerProgress", "Engineers":[ {{ "Engineer":"Felicity Farseer", "EngineerID":300100, "Progress":"Unlocked", "RankProgress":0, "Rank":5 }}, {{ "Engineer":"Eleanor Bresa", "EngineerID":400011, "Progress":"Known" }} ] }}"#),
        ];
        let mut m = Model::default();
        let mut quiet = Alerts::disabled();
        for line in &lines {
            let (ts, name) = scan_line(line).expect("line");
            let ev = Event::parse(name, line).expect("event");
            m.apply(&EventCtx { ts, name, line, live: false }, &ev, &mut quiet);
        }
        m
    }

    #[test]
    fn tools_answer_from_the_commanders_data() {
        let t = TempDb::new("ai-tools");
        let db = t.db();
        station(db, ("Alpha", 1001, 0.0), "Alpha Port", 5001, serde_json::json!([good("gold", 0, 0, 60_000, 80)]));
        station(db, ("Beta", 1002, 10.0), "Beta Hub", 5002, serde_json::json!([good("gold", 0, 0, 55_000, 1000), good("tritium", 40_000, 500, 0, 0)]));
        db.flush().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = NexusTools {
            model: Arc::new(RwLock::new(commander())),
            db: Some(db.clone()),
            index: Arc::new(RwLock::new(JournalIndex::default())),
            spansh: Spansh::new(),
            rt: rt.handle().clone(),
            routes: Arc::new(Mutex::new(None)),
            caller: Caller::Ask,
            budget: Caller::Ask.budget(),
        };

        let me = tools.call("commander", &json!({})).unwrap();
        assert_eq!(me["ship"]["landing_pad"], "L");
        assert_eq!(me["ship"]["cargo_capacity_t"], 400);
        assert_eq!(me["cargo"][0]["tonnes"], 100);

        let buy = tools.call("find_commodity", &json!({ "commodity": "Tritium", "mode": "buy" })).unwrap();
        assert_eq!(buy["results"][0]["station"], "Beta Hub");
        assert_eq!(buy["results"][0]["distance_ly"], 10.0);

        let sell = tools.call("sell_my_cargo", &json!({})).unwrap();
        assert_eq!(sell["best_single_stops"][0]["station"], "Beta Hub", "takes all 100 t: {sell}");
        assert_eq!(sell["best_single_stops"][0]["total_credits"], 100 * 55_000);

        let k = tools.call("knowledge", &json!({ "query": "neutron star jump range boost" })).unwrap();
        assert!(k["sections"][0]["title"].as_str().unwrap().contains("Travel"));

        let s = tools.call("system_info", &json!({ "system": "beta" })).unwrap();
        assert_eq!(s["stations"][0]["name"], "Beta Hub");
        assert_eq!(s["distance_ly"], 10.0);

        assert!(tools.call("journal_search", &json!({ "query": "gold" })).unwrap()["matches_newest_first"].as_array().unwrap().is_empty());
        assert!(tools.call("nope", &json!({})).is_err());
        assert!(tools.call("find_commodity", &json!({})).is_err(), "missing required argument is an error, not a panic");
        assert!(specs(Caller::Ask).len() >= 16);

        // The full loop with a scripted model: one tool call, then an answer.
        struct Script(parking_lot::Mutex<Vec<nexus_ai::Reply>>);
        impl nexus_ai::Backend for Script {
            fn complete(&self, _s: &str, tools: &[ToolSpec], turns: &[nexus_ai::Turn]) -> Result<nexus_ai::Reply, nexus_ai::AiError> {
                assert_eq!(tools.len(), specs(Caller::Ask).len());
                if turns.len() > 1 {
                    assert!(matches!(&turns[2], nexus_ai::Turn::ToolResult { content, is_error: false, .. } if content.contains("Beta Hub")));
                }
                Ok(self.0.lock().remove(0))
            }
        }
        let reply = |text: &str, calls: Vec<nexus_ai::ToolCall>| nexus_ai::Reply { text: text.into(), calls, raw: json!({ "role": "assistant" }), usage: Default::default(), truncated: false };
        let script = Script(parking_lot::Mutex::new(vec![
            reply("", vec![nexus_ai::ToolCall { id: "1".into(), name: "sell_my_cargo".into(), args: json!({}) }]),
            reply("Sell your 100 t of gold at **Beta Hub** for 5.5M Cr.", vec![]),
        ]));
        let mut conv = Vec::new();
        let ans = nexus_ai::ask(&script, nexus_ai::SYSTEM_PROMPT, &tools, &mut conv, "Where should I sell my cargo?".into(), &AtomicBool::new(false), |_| {}).unwrap();
        assert!(ans.text.contains("Beta Hub"));
        assert_eq!(ans.tools_used, vec!["sell_my_cargo"]);
    }

    /// "Where's the nearest war?": wars near the commander from the galaxy database, with the score and the stakes.
    #[test]
    fn conflicts_near_lists_the_wars_around_the_commander() {
        let t = TempDb::new("ai-conflicts");
        let db = t.db();
        let ts = nexus_journal::Timestamp::now().to_iso();
        let war = |name: &str, id: u64, x: f64, kind: &str| {
            json!({ "$schemaRef": "https://eddn.edcd.io/schemas/journal/1", "header": { "uploaderID": "t", "softwareName": "t", "softwareVersion": "1" },
                "message": { "timestamp": ts, "event": "FSDJump", "StarSystem": name, "SystemAddress": id, "StarPos": [x, 0.0, 0.0], "Population": 5000,
                    "SystemGovernment": "$government_Anarchy;",
                    "Factions": [{ "Name": "Red", "FactionState": "War", "Influence": 0.4, "Allegiance": "Independent", "ActiveStates": [{ "State": "War" }] },
                                 { "Name": "Blue", "FactionState": "War", "Influence": 0.4, "Allegiance": "Federation", "ActiveStates": [{ "State": "War" }] }],
                    "Conflicts": [{ "WarType": kind, "Status": "active", "Faction1": { "Name": "Red", "Stake": "Red Port", "WonDays": 2 },
                                    "Faction2": { "Name": "Blue", "Stake": "", "WonDays": 1 } }] } })
        };
        db.ingest_eddn(&war("Beta", 1002, 10.0, "war"));
        db.ingest_eddn(&war("Gamma", 1003, 60.0, "election"));
        db.ingest_eddn(&war("Far", 1004, 300.0, "civilwar"));
        db.flush().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = NexusTools { db: Some(db.clone()), ..model_tools(commander(), &rt) };

        let near = tools.call("conflicts_near", &json!({})).unwrap();
        let conflicts = near["conflicts"].as_array().unwrap();
        assert_eq!(conflicts.len(), 1, "the election isn't a fight and Far is out of range: {near}");
        let beta = &conflicts[0];
        assert_eq!((&beta["system"], &beta["kind"], &beta["status"], &beta["distance_ly"]), (&json!("Beta"), &json!("War"), &json!("active"), &json!(10.0)));
        assert_eq!((&beta["sides"][0]["days_won"], &beta["sides"][0]["stands_to_lose"], &beta["sides"][1]["stands_to_lose"]), (&json!(2), &json!("Red Port"), &Value::Null));
        assert_eq!((&beta["government"], &beta["days_won_known"]), (&json!("Anarchy"), &json!(true)));

        let wider = tools.call("conflicts_near", &json!({ "max_distance_ly": 500, "elections": true })).unwrap();
        let systems: Vec<&str> = wider["conflicts"].as_array().unwrap().iter().map(|c| c["system"].as_str().unwrap()).collect();
        assert_eq!(systems, ["Beta", "Gamma", "Far"]);
        let around = tools.call("conflicts_near", &json!({ "near_system": "Far", "max_distance_ly": 20 })).unwrap();
        assert_eq!((&around["near"], &around["conflicts"][0]["distance_ly"]), (&json!("Far"), &json!(0.0)));
    }

    /// One entry per tool: unique names, Ask's words for it, a schema the providers take, the writers kept from agents,
    /// and every entry answered by `call`.
    #[test]
    fn the_registry_describes_every_tool_once() {
        let mut names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len(), "tool names are unique");
        let mut titles: Vec<&str> = TOOLS.iter().map(|t| t.title).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), TOOLS.len(), "each tool has a title of its own");
        for t in TOOLS {
            assert!(!t.title.is_empty() && !t.description.is_empty() && !t.working.is_empty() && !t.noun.is_empty(), "{}", t.name);
            let p = (t.params)();
            assert_eq!(p["type"], "object", "{}", t.name);
            for r in p["required"].as_array().into_iter().flatten() {
                assert!(p["properties"].get(r.as_str().unwrap_or_default()).is_some(), "{}: {r} is required but not described", t.name);
            }
            assert!(!t.mcp || t.read_only, "{} changes something: not for agents", t.name);
            assert!(t.ask || t.mcp, "{} is offered to nobody", t.name);
        }
        assert_eq!((working("nope"), noun("nope")), ("Working…", "tools"));
        assert_eq!((working("sell_my_cargo"), noun("sell_my_cargo")), ("Planning your cargo sale…", "your cargo"));
        // Routes reach the Navigation page: Ask plans them, agents don't.
        let offered = |c: Caller| specs(c).iter().map(|s| s.name).collect::<Vec<_>>();
        let (ask, mcp) = (offered(Caller::Ask), offered(Caller::Mcp));
        assert!(ask.contains(&"plan_route") && ask.contains(&"money_routes"));
        assert!(!mcp.contains(&"plan_route") && !mcp.contains(&"money_routes") && mcp.contains(&"system_bodies"));
        // Every tool is answered: with no commander loaded they fail early, offline, but never as an unknown tool. A
        // tool the caller isn't offered is unknown to it.
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let mut tools = NexusTools {
            model: Arc::new(RwLock::new(Model::default())),
            db: None,
            index: Arc::new(RwLock::new(JournalIndex::default())),
            spansh: Spansh::builder().base_url("http://127.0.0.1:9").connect_timeout(Duration::from_millis(200)).build().unwrap(),
            rt: rt.handle().clone(),
            routes: Arc::new(Mutex::new(None)),
            caller: Caller::Ask,
            budget: Caller::Ask.budget(),
        };
        for t in TOOLS.iter().filter(|t| t.ask) {
            if let Err(e) = tools.call(t.name, &json!({})) {
                assert!(!e.contains("Unknown tool"), "{}: {e}", t.name);
            }
        }
        tools.caller = Caller::Mcp;
        assert!(tools.call("plan_route", &json!({ "to": "Sol" })).unwrap_err().contains("Unknown tool"));
        assert_eq!(tools.tools().len(), mcp.len());
    }

    /// Every spec Ask's model is offered goes with every question, and again with each round of tool calls: they're
    /// kept in check (about four characters make a token).
    #[test]
    fn ask_specs_stay_small() {
        let size = |t: &Tool| json_len(&json!({ "name": t.name, "description": t.description, "parameters": (t.params)() }));
        for t in TOOLS {
            println!("{:>5} {}{}", size(t), t.name, if t.ask { "" } else { " (agents only)" });
        }
        let ask: usize = TOOLS.iter().filter(|t| t.ask).map(size).sum();
        println!("{ask} characters offered to Ask");
        // Raising this is a decision: a tool offered to Ask has to earn its tokens. Tools few commanders ask about are
        // offered to agents only (on_foot, credit_history, system_factions). engineering, which commanders asked Ask for
        // (GitHub issue #4), took it from 13,500; where_to_mine and where_to_buy, the Mining and Fleet pages' searches Ask
        // couldn't run before, to 17,000; conflicts_near, the Combat page's nearest wars (play-test finding 19, its spec
        // cut to under 400 characters), to 17,500.
        assert!(ask <= 17_500, "{ask} characters of tool specs go with every Ask question");
    }

    /// `fit` finds the most detail that fits in a few tries.
    #[test]
    fn fit_keeps_the_most_detail_that_fits() {
        // Level n keeps 10 - n strings of 8 letters: 11 characters each with quotes and comma, so k of them are 11k + 1.
        let built = std::cell::Cell::new(0);
        let reply = |level: usize| {
            built.set(built.get() + 1);
            json!(vec!["x".repeat(8); 10 - level.min(10)])
        };
        assert_eq!(fit(1_000, 10, reply).1, 0, "the whole reply fits");
        built.set(0);
        let (v, level) = fit(45, 10, reply);
        assert_eq!((level, json_len(&v)), (6, 45), "four strings fit, five don't");
        assert!(built.get() <= 6, "{} replies built for 10 levels", built.get());
        assert_eq!(fit(5, 10, reply).1, 9, "nothing fits: the last level");
        assert_eq!(fit(5, 1, reply).1, 0, "one level: that one");
        assert_eq!(json_len(&json!({ "a": "é" })), r#"{"a":"é"}"#.len());
    }

    /// A long section keeps the lines about what was asked, and the stretch of a long line around its match.
    #[test]
    fn long_knowledge_sections_keep_what_was_asked() {
        let rows: Vec<String> = (0..200).map(|i| format!("| Page {i} | something about page {i} and its many settings |")).collect();
        let text = format!("{}\n| Fleet | Where to buy a build: the nearest stations |\n{}", rows[..100].join("\n"), rows[100..].join("\n"));
        let (cut, was_cut) = excerpt(&text, "where to buy", 1_000);
        assert!(was_cut && cut.len() <= 1_100, "{} characters", cut.len());
        assert!(cut.contains("| Fleet | Where to buy a build"), "{cut}");
        assert!(cut.contains('…'), "says where lines were left out");
        let long = format!("{} the AI agents switch {}", "x".repeat(3_000), "y".repeat(3_000));
        let (stretch, _) = excerpt(&long, "agents", 1_000);
        assert!(stretch.contains("the AI agents switch") && stretch.len() <= 1_100, "{stretch}");
        assert_eq!(excerpt("short", "anything", 1_000), ("short".to_string(), false));
    }

    /// The user guide's Pages section alone is longer than an agent takes: knowledge answers within the caller's budget,
    /// and the last guard cuts anything still over it.
    #[test]
    fn knowledge_and_every_reply_fit_the_callers_budget() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = agent_tools(commander(), &rt);
        for budget in [Caller::Mcp.budget(), Caller::Ask.budget()] {
            let tools = NexusTools { budget, ..agent_tools(commander(), &rt) };
            let k = tools.call("knowledge", &json!({ "query": "Settings AI agents copy command Claude Code" })).unwrap();
            assert!(json_len(&k) <= budget, "{} over {budget}", json_len(&k));
            assert!(k["sections"].as_array().unwrap().iter().any(|s| s["text"].as_str().unwrap().contains("AI agents")), "{k}");
        }
        drop(tools);
        let big = json!({ "rows": vec!["\"quoted\" text"; 5_000] });
        let cut = within(big, 2_000);
        assert!(json_len(&cut) <= 2_000 && cut["partial_json"].as_str().unwrap().starts_with("{\"rows\""), "{cut}");
        assert!(cut["note"].as_str().unwrap().contains("more than the 2000 this caller takes"));
        assert_eq!(within(json!({ "a": 1 }), 2_000), json!({ "a": 1 }), "a reply that fits is left alone");
    }

    /// "Which systems did I pass through from 22 to 29 October?" (GitHub issue #4): any period of the journal, page by
    /// page, never more than the model is passed.
    #[test]
    fn journal_search_reads_any_period_and_says_what_it_left_out() {
        let dir = std::env::temp_dir().join(format!("nexus-journal-search-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Ten days of 30 jumps (12:00 to 12:29 UTC), a journal file a day, indexed as the app does; the last day's
        // lines are long.
        let mut index = JournalIndex::default();
        for day in 20..30 {
            let path = dir.join(format!("Journal.2026-10-{day}T000000.01.log"));
            let file = index.add_file(path.clone());
            let note = if day == 29 { "x".repeat(400) } else { String::new() };
            let mut text = String::new();
            for j in 0..30 {
                let ts = nexus_journal::Timestamp::parse(&format!("2026-10-{day}T12:{j:02}:00Z")).unwrap();
                let line = format!(r#"{{ "timestamp":"{}", "event":"FSDJump", "StarSystem":"Sys {day}-{j}", "Note":"{note}" }}"#, ts.to_iso());
                index.push(ts, file, text.len() as u64, line.len(), "FSDJump");
                text.push_str(&line);
                text.push('\n');
            }
            std::fs::write(&path, text).unwrap();
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = NexusTools {
            model: Arc::new(RwLock::new(Model::default())),
            db: None,
            index: Arc::new(RwLock::new(index)),
            spansh: Spansh::new(),
            rt: rt.handle().clone(),
            routes: Arc::new(Mutex::new(None)),
            caller: Caller::Ask,
            budget: Caller::Ask.budget(),
        };
        let search = |args: Value| tools.call("journal_search", &args).unwrap();
        let systems = |v: &Value| -> Vec<String> {
            let hits = v["matches_newest_first"].as_array().unwrap();
            hits.iter().map(|m| m["line"].as_str().unwrap().split("\"StarSystem\":\"").nth(1).and_then(|s| s.split('"').next()).unwrap().to_string()).collect()
        };

        // Newest first, 15 by default, saying that older ones were left out.
        let v = search(json!({ "query": "fsdjump" }));
        assert_eq!(systems(&v).len(), 15);
        assert_eq!(systems(&v)[0], "Sys 29-29");
        assert!(v["more_matches"].as_str().unwrap().contains("skip 15"), "{v}");
        assert!(v["journal_starts"].is_string());

        // A period in journal time, paged with skip: the 22nd's 30 jumps, then nothing more.
        let page = |skip: usize| search(json!({ "query": "FSDJump", "from": "2026-10-22T00:00:00Z", "to": "2026-10-22T23:59:59Z", "limit": 10, "skip": skip }));
        let pages: Vec<Value> = [0, 10, 20].into_iter().map(page).collect();
        assert_eq!(systems(&pages[0])[0], "Sys 22-29");
        assert!(pages[1]["more_matches"].as_str().unwrap().contains("skip 20"));
        assert_eq!(systems(&pages[2]).last().map(String::as_str), Some("Sys 22-0"));
        assert!(pages[2].get("more_matches").is_none(), "the last page: {}", pages[2]);

        // A date is the commander's local day: one day's 30 jumps, whatever the time zone.
        let v = search(json!({ "query": "FSDJump", "from": "2026-10-24", "to": "2026-10-24", "limit": 40 }));
        let times: Vec<&str> = v["matches_newest_first"].as_array().unwrap().iter().filter_map(|m| m["time"].as_str()).collect();
        assert_eq!(times.len(), 30, "{v}");
        assert!(times.iter().all(|t| t.starts_with("2026-10-24")), "{times:?}");

        // Long lines: fewer than the limit, so the agent never cuts the answer, which says more are left.
        let v = search(json!({ "query": "FSDJump", "from": "2026-10-29T00:00:00Z", "limit": 40 }));
        assert!(systems(&v).len() < 30 && v["more_matches"].is_string(), "{v}");
        assert!(v.to_string().len() < nexus_ai::agent::MAX_RESULT_CHARS);

        assert!(tools.call("journal_search", &json!({ "query": "FSDJump", "from": "22/10" })).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Tools over a model alone: no database, no network (Spansh is never reached by the tools these tests call).
    fn model_tools(m: Model, rt: &tokio::runtime::Runtime) -> NexusTools {
        NexusTools {
            model: Arc::new(RwLock::new(m)),
            db: None,
            index: Arc::new(RwLock::new(JournalIndex::default())),
            spansh: Spansh::new(),
            rt: rt.handle().clone(),
            routes: Arc::new(Mutex::new(None)),
            caller: Caller::Ask,
            budget: Caller::Ask.budget(),
        }
    }

    /// Tools as an AI agent over MCP calls them (with Ask's length, so the size checks hold): for the tools only agents are
    /// offered.
    fn agent_tools(m: Model, rt: &tokio::runtime::Runtime) -> NexusTools {
        NexusTools { caller: Caller::Mcp, ..model_tools(m, rt) }
    }

    #[test]
    fn fleet_lists_every_ship_with_engineering() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = model_tools(commander(), &rt);
        let f = tools.call("fleet", &json!({})).unwrap();
        let ships = f["ships"].as_array().unwrap();
        assert_eq!(ships.len(), 2, "{f}");
        let here = ships.iter().find(|s| s["current"] == true).expect("the current ship");
        assert_eq!(here["type"], "Anaconda");
        assert_eq!(here["where"]["system"], "Alpha");
        assert_eq!(here["max_jump_range_ly"], 30.5);
        assert_eq!(here["rebuy_cr"], 5_000_000);
        let engineered = here["engineered"].as_array().unwrap();
        assert_eq!(engineered.len(), 1);
        assert_eq!(engineered[0]["blueprint"], "Increased FSD Range", "by name, not the journal's symbol");
        assert_eq!(engineered[0]["grade"], 5);
        assert_eq!(engineered[0]["engineer"], "Felicity Farseer");
        assert_eq!(engineered[0]["experimental"], "Mass Manager");
        assert!(here.get("modules").is_none(), "modules only on request: {here}");
        let stored = ships.iter().find(|s| s["current"] != true).expect("the stored ship");
        assert_eq!(stored["name"], "Mule");
        assert_eq!(stored["where"]["station"], "Alpha Port");
        assert_eq!(stored["where"]["system"], "Alpha");
        assert_eq!(stored["value_cr"], 55_000_000);
        assert!(stored.get("engineered").is_none() && stored["loadout_known"] == false, "never seen is not \"no engineering\": {stored}");
        assert_eq!(f["total_value_cr"], 55_000_000 + 200);

        let all = tools.call("fleet", &json!({ "all_modules": true })).unwrap();
        let here = all["ships"].as_array().unwrap().iter().find(|s| s["current"] == true).unwrap();
        let modules = here["modules"].as_array().unwrap();
        assert_eq!(modules.len(), 1, "the paint job and the cockpit aren't modules to list: {modules:?}");
        assert_eq!(modules[0]["slot"], "Frame Shift Drive");
        assert_eq!(modules[0]["engineered"], true);
        assert!(modules[0]["module"].as_str().is_some_and(|m| !m.is_empty()));

        // ship= picks one ship, by name or type, and all_modules then lists the modules of that ship only.
        let mule = tools.call("fleet", &json!({ "ship": "mule", "all_modules": true })).unwrap();
        assert_eq!(mule["ships"].as_array().unwrap().len(), 1);
        assert_eq!(mule["ships"][0]["name"], "Mule");
        assert!(mule["ships"][0].get("modules").is_none() && mule["notes"][0].as_str().unwrap().contains("loadout_known"), "no loadout known for it: {mule}");
        let anaconda = tools.call("fleet", &json!({ "ship": "ANACONDA", "all_modules": true })).unwrap();
        assert_eq!(anaconda["ships"].as_array().unwrap().len(), 1);
        assert_eq!(anaconda["ships"][0]["modules"].as_array().map(Vec::len), Some(1));
        let one_id = tools.call("fleet", &json!({ "ship": 2 })).unwrap();
        assert_eq!(one_id["ships"][0]["name"], "Mule");
        let err = tools.call("fleet", &json!({ "ship": "nope" })).unwrap_err();
        assert!(err.contains("Mule (Python)") && err.contains("Anaconda"), "lists the commander's ships: {err}");
    }

    /// The commander's fleet grown to `n` ships of 8 engineered modules each (idents `ID-00`...), the first one flown.
    fn crowded_fleet(n: u64) -> Model {
        use nexus_model::fleet::{ModuleEngineering, Ship, ShipModule};
        let mut m = commander();
        let gs = m.active_mut().expect("commander");
        gs.fleet.ships.clear();
        let slots = ["HugeHardpoint1", "LargeHardpoint1", "LargeHardpoint2", "FrameShiftDrive", "PowerPlant", "MainEngines", "PowerDistributor", "Slot01_Size6"];
        for id in 1..=n {
            let mut s = Ship::new(id, "Python");
            s.name = format!("Pythonesque number {id}");
            s.ident = format!("ID-{:02}", id - 1);
            s.hull_value = 50_000_000 + id as i64;
            s.rebuy = 2_000_000;
            s.cargo_capacity = 192;
            s.max_jump_range = 21.7;
            s.loadout_ts = nexus_journal::Timestamp::now();
            s.modules = slots
                .iter()
                .map(|slot| ShipModule {
                    slot: slot.to_string(),
                    item: "int_hyperdrive_size5_class5".into(),
                    on: true,
                    health: 1.0,
                    engineering: Some(ModuleEngineering {
                        engineer: Some("Tod 'The Blaster' McQuinn".into()),
                        blueprint: "Weapon_LongRange".into(),
                        level: 5,
                        quality: 0.87,
                        experimental: Some("Scramble Spectrum".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .collect();
            gs.fleet.ships.insert(id, s);
        }
        gs.fleet.current_ship_id = Some(1);
        m
    }

    #[test]
    fn fleet_stays_within_the_assistants_result_size() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let limit = nexus_ai::agent::MAX_RESULT_CHARS;
        let tools = model_tools(crowded_fleet(15), &rt);

        let f = tools.call("fleet", &json!({})).unwrap();
        assert!(f.to_string().len() < limit, "{} chars", f.to_string().len());
        assert_eq!(f["ships"].as_array().unwrap().len(), 15, "every ship is still listed: {}", f["notes"]);
        assert!(f["ships"][0]["current"] == true && f["ships"][0]["engineered_count"] == 8, "engineered_count replaces the list: {}", f["ships"][0]);
        assert!(f["notes"][0].as_str().unwrap().contains("ship="), "says how to get the detail: {}", f["notes"]);

        let all = tools.call("fleet", &json!({ "all_modules": true })).unwrap();
        assert!(all.to_string().len() < limit, "{} chars", all.to_string().len());
        assert_eq!(all["ships"].as_array().unwrap().len(), 15);

        // One ship asked for by its ident: exactly that one, in full.
        let one = tools.call("fleet", &json!({ "ship": "ID-07", "all_modules": true })).unwrap();
        assert!(one.to_string().len() < limit);
        let ships = one["ships"].as_array().unwrap();
        assert_eq!(ships.len(), 1);
        assert_eq!(ships[0]["ident"], "ID-07");
        assert_eq!(ships[0]["engineered"].as_array().unwrap().len(), 8);
        assert_eq!(ships[0]["modules"].as_array().unwrap().len(), 8);
        assert!(one.get("notes").is_none());

        assert!(tools.call("fleet", &json!({ "ship": "nope" })).is_err());
    }

    /// The ship being flown stays where the commander stepped out of it while an Apex taxi takes them elsewhere
    /// (review note 8 on PR #14).
    #[test]
    fn fleet_says_where_you_left_your_ship() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let (t1, t2, t3) = (at(10), at(20), at(30));
        let m = with_lines(
            commander(),
            &[
                format!(r#"{{ "timestamp":"{t1}", "event":"Disembark", "SRV":false, "Taxi":false, "Multicrew":false, "StarSystem":"Alpha", "SystemAddress":1001, "Body":"Alpha Port", "BodyID":3, "OnStation":true, "OnPlanet":false, "StationName":"Alpha Port", "StationType":"Coriolis", "MarketID":5001 }}"#),
                format!(r#"{{ "timestamp":"{t2}", "event":"Embark", "SRV":false, "Taxi":true, "Multicrew":false, "StarSystem":"Alpha", "SystemAddress":1001, "Body":"Alpha Port", "BodyID":3, "OnStation":true, "OnPlanet":false, "StationName":"Alpha Port", "StationType":"Coriolis", "MarketID":5001 }}"#),
                format!(r#"{{ "timestamp":"{t3}", "event":"FSDJump", "Taxi":true, "StarSystem":"Beta", "SystemAddress":1002, "StarPos":[10.0,0.0,0.0], "Population":0, "JumpDist":10.0, "FuelUsed":0.0, "FuelLevel":0.0 }}"#),
            ],
        );
        let f = model_tools(m, &rt).call("fleet", &json!({ "ship": "anaconda" })).unwrap();
        let here = &f["ships"][0]["where"];
        assert_eq!((&here["system"], &here["station"]), (&json!("Alpha"), &json!("Alpha Port")), "not Beta, where the taxi went: {f}");
        assert!(here["left_there"].is_string() && here.get("with_commander").is_none(), "{here}");
    }

    /// Advice on what was left out never sends the model back to ask for the ship it just asked for.
    #[test]
    fn fleet_advice_fits_what_was_asked() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let mut tools = model_tools(crowded_fleet(15), &rt);
        let notes = |tools: &NexusTools, args: Value| tools.call("fleet", &args).unwrap()["notes"].to_string();
        tools.budget = 2_500;
        assert!(notes(&tools, json!({})).contains("ship="), "no ship asked for: ask for one");
        let several = notes(&tools, json!({ "ship": "python" }));
        assert!(several.contains("by its ident or id") && !several.contains("ship="), "{several}");
        tools.budget = 1_200;
        let one = notes(&tools, json!({ "ship": "ID-07", "all_modules": true }));
        assert!(one.contains("every module") && !one.contains("ask for"), "{one}");
    }

    /// How much a reply says is the caller's: an agent with room for more gets the detail Ask has to leave out.
    #[test]
    fn the_callers_budget_sets_how_much_is_said() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let mut tools = model_tools(crowded_fleet(15), &rt);
        let ask = tools.call("fleet", &json!({})).unwrap();
        assert_eq!(ask["ships"][0]["engineered_count"], 8, "Ask's length: counts instead of lists");
        tools.budget = Caller::Mcp.budget();
        let mcp = tools.call("fleet", &json!({})).unwrap();
        assert_eq!(mcp["ships"][0]["engineered"].as_array().map(Vec::len), Some(8), "{}", mcp["notes"]);
        assert!(mcp.get("notes").is_none() && mcp.to_string().len() <= Caller::Mcp.budget());
        // And the other way: a caller that takes little gets fewer ships, the one being flown first…
        tools.budget = 2_000;
        let small = tools.call("fleet", &json!({})).unwrap();
        let kept = small["ships"].as_array().map_or(0, Vec::len);
        assert!((1..15).contains(&kept) && small["ships_omitted"] == json!(15 - kept) && json_len(&small) <= 2_000, "{small}");
        assert_eq!(small["ships"][0]["current"], true, "{small}");
        // …and one that takes less than even one ship gets the beginning, cut, and is told so: never an error.
        tools.budget = 500;
        let tiny = tools.call("fleet", &json!({})).unwrap();
        assert!(json_len(&tiny) <= 500 && tiny["note"].as_str().unwrap().contains("this caller takes"), "{tiny}");
    }

    #[test]
    fn fleet_drops_the_least_valuable_ships_when_even_the_short_form_is_too_long() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = model_tools(crowded_fleet(120), &rt);
        let f = tools.call("fleet", &json!({})).unwrap();
        assert!(f.to_string().len() < nexus_ai::agent::MAX_RESULT_CHARS);
        let shown = f["ships"].as_array().unwrap().len();
        assert!(shown > 20 && shown < 120, "{shown} ships");
        assert_eq!(f["ships_omitted"], 120 - shown);
        assert_eq!(f["ships"][0]["current"], true, "the ship being flown stays");
        assert!(f["notes"].as_array().unwrap().iter().any(|n| n.as_str().is_some_and(|n| n.starts_with("Ships left out"))));
    }

    #[test]
    fn fleet_says_when_a_ship_is_in_transit() {
        let mut m = commander();
        let arrives = nexus_journal::Timestamp(nexus_journal::Timestamp::now().0 + 1800);
        let gs = m.active_mut().expect("commander");
        let ship = gs.fleet.ships.get_mut(&2).expect("stored ship");
        ship.location = nexus_model::fleet::ShipLocation { system: Some("Beta".into()), station: Some("Beta Hub".into()), in_transit: true, arrives: Some(arrives), ..Default::default() };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let f = model_tools(m, &rt).call("fleet", &json!({})).unwrap();
        let stored = f["ships"].as_array().unwrap().iter().find(|s| s["current"] != true).unwrap();
        assert_eq!(stored["where"]["in_transit"], true);
        assert_eq!(stored["where"]["station"], "Beta Hub");
        assert_eq!(stored["where"]["arrives"], arrives.to_iso());
        assert_eq!(stored["where"]["arrives_in_min"], 30);
    }

    #[test]
    fn progress_has_ranks_engineers_and_game_state() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let m = commander();
        let (closed, expected_rank) = {
            let gs = m.active().unwrap();
            (gs.game_closed, gs.cmdr.rank_name(nexus_data::ranks::RankKind::Explore))
        };
        let tools = model_tools(m, &rt);
        let p = tools.call("progress", &json!({})).unwrap();
        let ranks = p["ranks"].as_array().unwrap();
        assert_eq!(ranks.len(), 8);
        let explore = ranks.iter().find(|r| r["rank"] == "Explore").unwrap();
        assert_eq!(explore["name"], expected_rank);
        assert_eq!(explore["progress_pct"], 78);
        let engineers = p["engineers"].as_array().unwrap();
        assert_eq!(engineers[0]["name"], "Felicity Farseer", "unlocked engineers come first: {p}");
        assert_eq!(engineers[0]["status"], "Unlocked");
        assert_eq!(engineers[0]["rank"], 5);
        assert_eq!(engineers[1]["name"], "Eleanor Bresa");
        assert_eq!(engineers[1]["status"], "Known");
        assert!(engineers[1].get("rank").is_none());
        assert_eq!(p["game"]["running"], !closed);
        assert!(p["game"]["at_main_menu"].is_boolean());
        assert_eq!(p["game"]["mode"], "Solo");
        assert!(p["powerplay"].is_null() && p["squadron"].is_null(), "not pledged, no squadron: {p}");
    }

    /// `m` with journal lines replayed on top.
    fn with_lines(mut m: Model, lines: &[String]) -> Model {
        let mut quiet = Alerts::disabled();
        for line in lines {
            let (ts, name) = scan_line(line).expect("line");
            let ev = Event::parse(name, line).expect("event");
            m.apply(&EventCtx { ts, name, line, live: false }, &ev, &mut quiet);
        }
        m
    }

    /// An ISO timestamp `secs` after now.
    fn at(secs: i64) -> String {
        nexus_journal::Timestamp(nexus_journal::Timestamp::now().0 + secs).to_iso()
    }

    /// The commander with iron (full) and polonium, a suit loadout in use and a ship locker.
    fn geared_commander() -> Model {
        let t = at(30);
        with_lines(
            commander(),
            &[
                format!(r#"{{ "timestamp":"{t}", "event":"Materials", "Raw":[ {{ "Name":"iron", "Name_Localised":"Iron", "Count":300 }}, {{ "Name":"polonium", "Name_Localised":"Polonium", "Count":12 }} ], "Manufactured":[ {{ "Name":"chemicaldistillery", "Name_Localised":"Chemical Distillery", "Count":4 }} ], "Encoded":[ ] }}"#),
                format!(r#"{{ "timestamp":"{t}", "event":"SuitLoadout", "SuitID":1700216165682989, "SuitName":"tacticalsuit_class1", "SuitName_Localised":"Dominator Suit", "SuitMods":["suit_increasedbatterycapacity"], "LoadoutID":4293000001, "LoadoutName":"Fighter", "Modules":[ {{ "SlotName":"PrimaryWeapon1", "SuitModuleID":1700216180036986, "ModuleName":"wpn_m_assaultrifle_plasma_fauto", "ModuleName_Localised":"Manticore Tormentor", "Class":3, "WeaponMods":["weapon_clipsize"] }}, {{ "SlotName":"SecondaryWeapon", "SuitModuleID":1700216180036987, "ModuleName":"wpn_s_pistol_kinetic_sauto", "ModuleName_Localised":"Karma P-15", "Class":1, "WeaponMods":[] }} ] }}"#),
                format!(r#"{{ "timestamp":"{t}", "event":"ShipLocker", "Items":[ {{ "Name":"healthpack", "Name_Localised":"Medkit", "OwnerID":0, "Count":3 }} ], "Components":[ {{ "Name":"graphene", "Name_Localised":"Graphene", "OwnerID":0, "Count":9 }} ], "Consumables":[ {{ "Name":"energycell", "Name_Localised":"Energy Cell", "OwnerID":0, "Count":5 }} ], "Data":[ {{ "Name":"airqualityreports", "Name_Localised":"Air Quality Reports", "OwnerID":0, "Count":2 }} ] }}"#),
            ],
        )
    }

    #[test]
    fn materials_have_grade_cap_and_full_flag() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = model_tools(geared_commander(), &rt);
        let all = tools.call("materials", &json!({})).unwrap();
        let raw = all["raw"].as_array().unwrap();
        let iron = raw.iter().find(|m| m["name"] == "Iron").expect("iron");
        assert_eq!((&iron["count"], &iron["grade"], &iron["cap"], &iron["full"]), (&json!(300), &json!(1), &json!(300), &json!(true)), "{iron}");
        let polonium = raw.iter().find(|m| m["name"] == "Polonium").expect("polonium");
        assert_eq!((&polonium["count"], &polonium["grade"], &polonium["cap"]), (&json!(12), &json!(4), &json!(150)), "{polonium}");
        assert!(polonium["full"] != true, "{polonium}");
        assert_eq!(all["manufactured"][0]["name"], "Chemical Distillery");
        assert!(all["encoded"].as_array().unwrap().is_empty());

        let one = tools.call("materials", &json!({ "category": "raw", "name": "polo" })).unwrap();
        let raw = one["raw"].as_array().unwrap();
        assert_eq!(raw.len(), 1, "{one}");
        assert_eq!(raw[0]["name"], "Polonium");
        assert!(one["manufactured"].as_array().unwrap().is_empty());

        // A name filter reaches every category; an unknown category is an error, not an empty answer.
        let distill = tools.call("materials", &json!({ "name": "DISTILL" })).unwrap();
        assert_eq!(distill["manufactured"].as_array().unwrap().len(), 1);
        assert!(distill["raw"].as_array().unwrap().is_empty());
        assert!(tools.call("materials", &json!({ "category": "guardian" })).is_err());
    }

    #[test]
    fn materials_stay_within_the_assistants_result_size() {
        use nexus_data::materials::{MaterialKind, all};
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let limit = nexus_ai::agent::MAX_RESULT_CHARS;
        // Every material the game knows, held at its cap: the most the commander can ever ask about.
        let mut m = commander();
        let mats = &mut m.active_mut().expect("commander").inventory.materials;
        for x in all() {
            let map = match x.kind {
                MaterialKind::Raw => &mut mats.raw,
                MaterialKind::Manufactured => &mut mats.manufactured,
                MaterialKind::Encoded => &mut mats.encoded,
            };
            map.insert(x.symbol.clone(), i64::from(x.cap()));
        }
        let held = mats.raw.len() + mats.manufactured.len() + mats.encoded.len();
        assert!(held > 120, "{held} materials held");
        let mut tools = model_tools(m, &rt);
        let v = tools.call("materials", &json!({})).unwrap();
        let len = v.to_string().len();
        assert!(len < limit, "{len} chars for {held} materials");
        let listed: usize = ["raw", "manufactured", "encoded"].iter().map(|k| v[k].as_array().unwrap().len()).sum();
        assert_eq!(listed, held, "every material is still listed: {}", v["notes"]);
        let raw = tools.call("materials", &json!({ "category": "raw" })).unwrap();
        assert!(raw.to_string().len() < limit);
        // A caller with a little less room: the storage cap is dropped (and said once in a note) while the grade and
        // the full flag stay, and every material is still listed.
        tools.budget = len - 1;
        let v = tools.call("materials", &json!({})).unwrap();
        let first = &v["raw"][0];
        assert!(first.get("grade").is_some() && first.get("full").is_some() && first.get("cap").is_none(), "{first}");
        assert!(v["notes"][0].as_str().unwrap().contains("storage cap"), "{}", v["notes"]);
        let listed: usize = ["raw", "manufactured", "encoded"].iter().map(|k| v[k].as_array().unwrap().len()).sum();
        assert_eq!(listed, held);
    }

    /// On a game client in another language the journal names materials in that language: a name asked for in English
    /// still finds them, and the reply gives the English name beside the game's (review note 6 on PR #14).
    #[test]
    fn materials_match_english_names_on_any_client() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let t = at(30);
        let m = with_lines(
            commander(),
            &[format!(
                r#"{{ "timestamp":"{t}", "event":"Materials", "Raw":[ {{ "Name":"polonium", "Name_Localised":"Polonium", "Count":12 }} ], "Manufactured":[ {{ "Name":"chemicaldistillery", "Name_Localised":"Chemische Destillerie", "Count":4 }} ], "Encoded":[ {{ "Name":"disruptedwakeechoes", "Name_Localised":"Atypische gestörte Sogechos", "Count":40 }} ] }}"#
            )],
        );
        let tools = model_tools(m, &rt);
        let found = |name: &str| {
            let v = tools.call("materials", &json!({ "name": name })).unwrap();
            ["raw", "manufactured", "encoded"].iter().flat_map(|k| v[k].as_array().cloned().unwrap_or_default()).collect::<Vec<_>>()
        };
        let distillery = found("chemical distillery");
        assert_eq!(distillery.len(), 1, "the English name finds the German one");
        assert_eq!((&distillery[0]["name"], &distillery[0]["english"]), (&json!("Chemische Destillerie"), &json!("Chemical Distillery")));
        assert_eq!(found("Wake Echoes")[0]["english"], "Atypical Disrupted Wake Echoes");
        assert_eq!(found("destillerie").len(), 1, "and the game's own name still does");
        assert_eq!(found("chemicaldistillery").len(), 1, "so does the journal symbol");
        assert!(found("polonium")[0].get("english").is_none(), "no English name when it's the same");
        assert!(found("iron").is_empty());
    }

    #[test]
    fn on_foot_lists_loadouts_and_locker() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = agent_tools(geared_commander(), &rt);
        let v = tools.call("on_foot", &json!({})).unwrap();
        let loadouts = v["loadouts"].as_array().unwrap();
        assert_eq!(loadouts.len(), 1, "{v}");
        assert_eq!(loadouts[0]["suit"], "Dominator Suit");
        assert_eq!(loadouts[0]["name"], "Fighter");
        assert_eq!(loadouts[0]["current"], true);
        assert_eq!(loadouts[0]["suit_mods"][0], "Improved Battery Capacity", "by name, not the journal's symbol");
        let weapons = loadouts[0]["weapons"].as_array().unwrap();
        assert_eq!(weapons.len(), 2);
        assert_eq!(weapons[0]["weapon"], "Manticore Tormentor");
        assert_eq!(weapons[0]["grade"], 3);
        assert_eq!(weapons[0]["mods"][0], "Magazine Size");
        assert!(weapons[1].get("mods").is_none(), "no mods, no key: {}", weapons[1]);
        assert_eq!(v["locker"]["items"]["Medkit"], 3);
        assert_eq!(v["locker"]["components"]["Graphene"], 9);
        assert_eq!(v["locker"]["consumables"]["Energy Cell"], 5);
        assert_eq!(v["locker"]["data"]["Air Quality Reports"], 2);
        assert!(v["backpack"]["items"].as_object().unwrap().is_empty());
    }

    /// The loadouts are the ones the game still has, under their names now (review note 7 on PR #14).
    #[test]
    fn on_foot_follows_deleted_and_renamed_loadouts() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let t = at(40);
        let m = with_lines(
            geared_commander(),
            &[
                format!(r#"{{ "timestamp":"{t}", "event":"CreateSuitLoadout", "SuitID":18, "SuitName":"explorationsuit_class1", "SuitName_Localised":"Artemis Suit", "SuitMods":[], "LoadoutID":4293000002, "LoadoutName":"Loadout 2", "Modules":[] }}"#),
                format!(r#"{{ "timestamp":"{t}", "event":"RenameSuitLoadout", "SuitID":1700216165682989, "SuitName":"tacticalsuit_class1", "SuitName_Localised":"$TacticalSuit_Class1_Name;", "LoadoutID":4293000001, "LoadoutName":"Assault" }}"#),
                format!(r#"{{ "timestamp":"{t}", "event":"DeleteSuitLoadout", "SuitID":18, "SuitName":"explorationsuit_class1", "SuitName_Localised":"Artemis Suit", "LoadoutID":4293000002, "LoadoutName":"Loadout 2" }}"#),
            ],
        );
        let v = agent_tools(m, &rt).call("on_foot", &json!({})).unwrap();
        let loadouts = v["loadouts"].as_array().unwrap();
        assert_eq!(loadouts.len(), 1, "the deleted one is gone: {v}");
        assert_eq!((&loadouts[0]["name"], &loadouts[0]["suit"]), (&json!("Assault"), &json!("Dominator Suit")));
    }

    #[test]
    fn on_foot_stays_within_the_assistants_result_size() {
        use nexus_model::inventory::{Locker, LockerItem, SuitLoadoutInfo};
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let limit = nexus_ai::agent::MAX_RESULT_CHARS;
        let fill = |l: &mut Locker, n: usize| {
            for (kind, map) in [&mut l.items, &mut l.components, &mut l.consumables, &mut l.data].into_iter().enumerate() {
                for i in 0..n {
                    map.insert(format!("k{kind}i{i}"), LockerItem { name: format!("Ultra Long Micro-resource Name {kind}-{i}"), count: 100 + i as i64 });
                }
            }
        };
        let mut m = commander();
        let inv = &mut m.active_mut().expect("commander").inventory;
        fill(&mut inv.ship_locker, 80);
        // Something used up (count 0) is not counted as held.
        inv.ship_locker.items.insert("used".into(), LockerItem { name: "Used up".into(), count: 0 });
        fill(&mut inv.backpack, 40);
        for id in 1..=10u64 {
            inv.suit_loadouts.insert(
                id,
                SuitLoadoutInfo {
                    loadout_id: id,
                    name: format!("Loadout number {id}"),
                    suit_id: id,
                    suit_name: "Maverick Suit".into(),
                    suit_mods: vec!["suit_increased_air_reserves".into(), "suit_improved_battery_capacity".into(), "suit_added_melee_damage".into()],
                    weapons: (0..3).map(|w| (format!("Weapon{w}"), "Manticore Tormentor".into(), 5, vec!["weapon_extended_magazine".into(), "weapon_scope".into(), "weapon_stability".into()])).collect(),
                },
            );
        }
        inv.current_suit_loadout = Some(3);
        let tools = agent_tools(m, &rt);
        let v = tools.call("on_foot", &json!({})).unwrap();
        let len = v.to_string().len();
        assert!(len < limit, "{len} chars");
        assert_eq!(v["loadouts"].as_array().unwrap().len(), 10, "every loadout is still listed");
        assert_eq!(v["loadouts"][0]["current"], true, "the one in use comes first");
        assert!(v["notes"][0].as_str().unwrap().contains("locker"), "says what was cut: {}", v["notes"]);
        assert_eq!(v["locker"]["counts"]["items"]["distinct"], 80, "counts per kind instead of the list: {}", v["locker"]);
    }

    /// The commander with a sale at +60 s and a refuel at +120 s after logging in, and a refuel an hour before.
    fn paid_commander() -> Model {
        let (old, t1, t2) = (at(-3600), at(60), at(120));
        with_lines(
            commander(),
            &[
                format!(r#"{{ "timestamp":"{old}", "event":"RefuelAll", "Cost":999, "Amount":3.0 }}"#),
                format!(r#"{{ "timestamp":"{t1}", "event":"MarketSell", "MarketID":5001, "Type":"silver", "Type_Localised":"Silver", "Count":20, "SellPrice":5000, "TotalSale":100000, "AvgPricePaid":4000 }}"#),
                format!(r#"{{ "timestamp":"{t2}", "event":"RefuelAll", "Cost":1500, "Amount":10.5 }}"#),
            ],
        )
    }

    #[test]
    fn credit_history_since_login_newest_first() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let m = paid_commander();
        let credits = m.active().unwrap().cmdr.credits;
        let tools = agent_tools(m, &rt);
        let v = tools.call("credit_history", &json!({})).unwrap();
        assert_eq!(v["balance"], credits);
        let tx = v["transactions"].as_array().unwrap();
        assert_eq!(tx.len(), 2, "the refuel before the login is left out: {v}");
        assert_eq!(tx[0]["category"], "Fuel", "newest first");
        assert_eq!(tx[0]["amount"], -1500);
        assert_eq!(tx[1]["category"], "Trade");
        assert_eq!(tx[1]["amount"], 100_000);
        assert!(tx[1]["note"].as_str().unwrap().contains("Silver"));
        let totals = v["totals"].as_array().unwrap();
        let trade = totals.iter().find(|t| t["category"] == "Trade").expect("trade");
        assert_eq!(trade["income"], 100_000);
        let fuel = totals.iter().find(|t| t["category"] == "Fuel").expect("fuel");
        assert_eq!(fuel["expense"], -1500);
        assert_eq!(v["transactions_total"], 2);

        let one = tools.call("credit_history", &json!({ "limit": 1 })).unwrap();
        assert_eq!(one["transactions"].as_array().unwrap().len(), 1);
        assert_eq!(one["transactions"][0]["category"], "Fuel");
        assert_eq!(one["transactions_total"], 2, "says how many there were: {one}");

        // since_hours reaches back past the login.
        let wide = tools.call("credit_history", &json!({ "since_hours": 2 })).unwrap();
        assert_eq!(wide["transactions"].as_array().unwrap().len(), 3);
        assert_eq!(wide["transactions"][2]["amount"], -999);
    }

    #[test]
    fn credit_history_lists_the_later_of_two_same_second_entries_first() {
        use nexus_model::history::LedgerCategory;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let mut m = commander();
        let ts = nexus_journal::Timestamp(nexus_journal::Timestamp::now().0 + 10);
        let h = &mut m.active_mut().expect("commander").history;
        h.ledger(ts, LedgerCategory::Repairs, -10, "applied first");
        h.ledger(ts, LedgerCategory::Ammo, -20, "applied second");
        let v = agent_tools(m, &rt).call("credit_history", &json!({})).unwrap();
        let tx = v["transactions"].as_array().unwrap();
        assert_eq!((tx[0]["amount"].clone(), tx[1]["amount"].clone()), (json!(-20), json!(-10)), "{v}");
    }

    #[test]
    fn credit_history_says_carrier_transfers_are_not_income_or_spending() {
        use nexus_model::history::LedgerCategory;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let ts = nexus_journal::Timestamp(nexus_journal::Timestamp::now().0 + 10);
        let mut m = paid_commander();
        let none = agent_tools(m.clone(), &rt).call("credit_history", &json!({})).unwrap();
        assert!(none.get("notes").is_none(), "no transfer, no note: {none}");
        m.active_mut().expect("commander").history.ledger(ts, LedgerCategory::Transfer, -5_000, "To carrier");
        let v = agent_tools(m, &rt).call("credit_history", &json!({})).unwrap();
        assert!(v["totals"].as_array().unwrap().iter().any(|t| t["category"] == "Carrier transfers"));
        assert_eq!(v["notes"][0], "Carrier transfers move credits between you and your carrier: they are not income or spending.");
    }

    #[test]
    fn credit_history_stays_within_the_assistants_result_size() {
        use nexus_model::history::LedgerCategory;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let limit = nexus_ai::agent::MAX_RESULT_CHARS;
        let mut m = commander();
        let gs = m.active_mut().expect("commander");
        let base = nexus_journal::Timestamp::now().0 + 10;
        let cats = [LedgerCategory::Trade, LedgerCategory::Mining, LedgerCategory::Missions, LedgerCategory::Outfitting, LedgerCategory::Exobiology, LedgerCategory::Bounties];
        for i in 0..300i64 {
            let note = format!("Sold {} Low Temperature Diamonds to a very long named station in a very long named system (profit {})", 700 + i, 12_345_678 + i);
            gs.history.ledger(nexus_journal::Timestamp(base + i), cats[(i % 6) as usize], if i % 3 == 0 { -123_456_789 - i } else { 987_654_321 + i }, note);
        }
        let tools = agent_tools(m, &rt);
        let v = tools.call("credit_history", &json!({ "limit": 100 })).unwrap();
        let len = v.to_string().len();
        assert!(len < limit, "{len} chars");
        let tx = v["transactions"].as_array().unwrap();
        assert!(tx.len() >= 50, "{} transactions", tx.len());
        assert_eq!(tx[0]["amount"], 987_654_321i64 + 299, "newest first");
        assert_eq!(v["transactions_total"], 300);
        assert!(v["notes"][0].as_str().unwrap().contains("note"), "says what was shortened: {}", v["notes"]);
        // limit is clamped to 1..=100.
        let many = tools.call("credit_history", &json!({ "limit": 5000 })).unwrap();
        assert!(many["transactions"].as_array().unwrap().len() <= 100);
        assert!(many.to_string().len() < limit);
    }

    /// A throw-away journal folder, deleted on drop.
    struct JournalDir(std::path::PathBuf);

    impl JournalDir {
        fn new(name: &str) -> JournalDir {
            let dir = std::env::temp_dir().join(format!("nexus-app-test-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp journal dir");
            JournalDir(dir)
        }
    }

    impl Drop for JournalDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An `FSDJump` into Gamma: two factions, the first with a reputation and a pending state.
    fn gamma_jump(ts: &str) -> String {
        format!(
            r#"{{ "timestamp":"{ts}", "event":"FSDJump", "Taxi":false, "StarSystem":"Gamma", "SystemAddress":1003, "StarPos":[10.0,0.0,0.0], "SystemAllegiance":"Federation", "SystemEconomy":"$economy_Industrial;", "SystemEconomy_Localised":"Industrial", "SystemSecondEconomy":"$economy_None;", "SystemGovernment":"$government_Democracy;", "SystemGovernment_Localised":"Democracy", "SystemSecurity":"$SYSTEM_SECURITY_medium;", "SystemSecurity_Localised":"Medium Security", "Population":5000000, "Body":"Gamma A", "BodyID":1, "BodyType":"Star", "JumpDist":10.0, "FuelUsed":1.0, "FuelLevel":20.0, "SystemFaction":{{ "Name":"Gamma Labour", "FactionState":"Boom" }}, "Factions":[ {{ "Name":"Gamma Labour", "FactionState":"Boom", "Government":"Democracy", "Influence":0.52, "Allegiance":"Federation", "Happiness":"$Faction_HappinessBand2;", "Happiness_Localised":"Happy", "MyReputation":42.5, "PendingStates":[ {{ "State":"Expansion", "Trend":0 }} ], "ActiveStates":[ {{ "State":"Boom" }} ] }}, {{ "Name":"Gamma Crew", "FactionState":"None", "Government":"Anarchy", "Influence":0.48, "Allegiance":"Independent", "Happiness":"", "MyReputation":-10.0 }} ] }}"#
        )
    }

    /// A `Location` in Alpha with one faction.
    fn alpha_location(ts: &str) -> String {
        format!(
            r#"{{ "timestamp":"{ts}", "event":"Location", "Docked":false, "StarSystem":"Alpha", "SystemAddress":1001, "StarPos":[0.0,0.0,0.0], "SystemAllegiance":"Federation", "SystemEconomy":"$economy_Agri;", "SystemEconomy_Localised":"Agriculture", "SystemGovernment":"$government_Democracy;", "SystemGovernment_Localised":"Democracy", "SystemSecurity":"$SYSTEM_SECURITY_high;", "SystemSecurity_Localised":"High Security", "Population":1000, "Body":"Alpha", "BodyID":0, "BodyType":"Star", "SystemFaction":{{ "Name":"Alpha Party", "FactionState":"None" }}, "Factions":[ {{ "Name":"Alpha Party", "FactionState":"None", "Government":"Democracy", "Influence":1.0, "Allegiance":"Federation", "Happiness":"$Faction_HappinessBand2;", "Happiness_Localised":"Happy", "MyReputation":5.0 }} ] }}"#
        )
    }

    /// Tools over `lines` written as a journal file in `dir` and indexed the way the app does; the model is the
    /// commander with those lines replayed on top.
    fn journal_tools(dir: &JournalDir, lines: &[String], rt: &tokio::runtime::Runtime) -> NexusTools {
        journal_tools_indexing(dir, lines, lines, rt)
    }

    /// Like [`journal_tools`], but the journal file and its index hold only `indexed` lines of those the model saw.
    fn journal_tools_indexing(dir: &JournalDir, lines: &[String], indexed: &[String], rt: &tokio::runtime::Runtime) -> NexusTools {
        let path = dir.0.join("Journal.2026-10-31T100000.01.log");
        let mut text = String::new();
        let mut index = JournalIndex::default();
        let file = index.add_file(path.clone());
        for line in indexed {
            let (ts, name) = scan_line(line).expect("line");
            index.push(ts, file, text.len() as u64, line.len(), name);
            text.push_str(line);
            text.push('\n');
        }
        std::fs::write(&path, text).expect("write journal");
        let tools = agent_tools(with_lines(commander(), lines), rt);
        *tools.index.write() = index;
        tools
    }

    /// The lines of a login of commander `fid`.
    fn login(ts: &str, fid: &str, name: &str) -> Vec<String> {
        vec![
            format!(r#"{{ "timestamp":"{ts}", "event":"Commander", "FID":"{fid}", "Name":"{name}" }}"#),
            format!(r#"{{ "timestamp":"{ts}", "event":"LoadGame", "FID":"{fid}", "Commander":"{name}", "Horizons":true, "Odyssey":true, "Ship":"Anaconda", "ShipID":1, "ShipName":"", "ShipIdent":"", "FuelLevel":32.0, "FuelCapacity":32.0, "GameMode":"Solo", "Credits":1000, "Loan":0 }}"#),
        ]
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    #[test]
    fn factions_of_current_system_from_model() {
        let (dir, rt) = (JournalDir::new("factions-now"), runtime());
        let tools = journal_tools(&dir, &[gamma_jump(&at(10)), alpha_location(&at(20))], &rt);
        let v = tools.call("system_factions", &json!({})).unwrap();
        assert_eq!(v["system"], "Alpha", "{v}");
        assert_eq!(v["factions"].as_array().unwrap().len(), 1);
        assert_eq!(v["factions"][0]["name"], "Alpha Party");
        assert_eq!(v["factions"][0]["influence_pct"], 100.0);
        assert_eq!(v["factions"][0]["my_reputation"], 5.0);
        assert!(v["source"].as_str().unwrap().contains("journal"), "{v}");
        assert!(v["as_of"].is_string());
        // Naming the current system gives the same answer, whatever the case.
        let named = tools.call("system_factions", &json!({ "system": "ALPHA" })).unwrap();
        assert_eq!(named["factions"], v["factions"]);
    }

    #[test]
    fn factions_of_a_visited_system_from_journal() {
        let (dir, rt) = (JournalDir::new("factions-visited"), runtime());
        let visit = at(10);
        let tools = journal_tools(&dir, &[gamma_jump(&visit), alpha_location(&at(20))], &rt);
        let v = tools.call("system_factions", &json!({ "system": "gamma" })).unwrap();
        assert_eq!(v["system"], "Gamma", "{v}");
        assert_eq!(v["as_of"], visit, "the time of that visit");
        assert_eq!(v["controlling_faction"], "Gamma Labour");
        assert_eq!(v["government"], "Democracy");
        let f = v["factions"].as_array().unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0]["name"], "Gamma Labour");
        assert_eq!(f[0]["influence_pct"], 52.0);
        assert_eq!(f[0]["my_reputation"], 42.5);
        assert_eq!(f[0]["happiness"], "Happy");
        assert_eq!(f[0]["pending_states"], json!(["Expansion"]));
        assert_eq!(f[0]["active_states"], json!(["Boom"]));
        assert_eq!(f[1]["my_reputation"], -10.0);
        assert!(f[1].get("pending_states").is_none(), "empty lists are left out: {}", f[1]);
        assert!(v["source"].as_str().unwrap().contains("journal"));
    }

    /// The journal index holds every commander's journals: the answer is about the commander asking, with the time
    /// they were there, not about whoever arrived in that system last.
    #[test]
    fn factions_are_the_active_commanders_own_visit() {
        let (dir, rt) = (JournalDir::new("factions-alt"), runtime());
        let (mine, alt_visit, back) = (at(10), at(30), at(40));
        let mut lines = vec![gamma_jump(&mine)];
        lines.extend(login(&at(20), "F2", "Alt"));
        lines.push(gamma_jump(&alt_visit).replace("42.5", "99.0"));
        lines.extend(login(&back, "F1", "Tester"));
        lines.push(alpha_location(&at(50)));
        let tools = journal_tools(&dir, &lines, &rt);
        assert_eq!(tools.model.read().active().unwrap().cmdr.name, "Tester");
        let v = tools.call("system_factions", &json!({ "system": "Gamma" })).unwrap();
        assert_eq!(v["as_of"], mine, "{v}");
        assert_eq!(v["factions"][0]["my_reputation"], 42.5, "not the alt's: {v}");
        assert!(!v["notes"].to_string().contains("another commander"), "{}", v["notes"]);
    }

    /// When the entry of that second isn't in the index, the newest earlier arrival is given, and it says so.
    #[test]
    fn factions_say_when_the_exact_arrival_was_not_found() {
        let (dir, rt) = (JournalDir::new("factions-walk"), runtime());
        let (mine, earlier) = (at(10), at(5));
        let tools = journal_tools_indexing(&dir, &[gamma_jump(&mine), alpha_location(&at(20))], &[gamma_jump(&earlier), alpha_location(&at(20))], &rt);
        let v = tools.call("system_factions", &json!({ "system": "Gamma" })).unwrap();
        assert_eq!(v["as_of"], earlier, "{v}");
        assert!(v["notes"].to_string().contains("another commander"), "{}", v["notes"]);
    }

    /// A system the commander never visited is told from their own history: the journal index isn't touched.
    #[test]
    fn unvisited_system_does_not_read_the_journal_index() {
        let (dir, rt) = (JournalDir::new("factions-no-index"), runtime());
        let tools = journal_tools(&dir, &[gamma_jump(&at(10)), alpha_location(&at(20))], &rt);
        let guard = tools.index.write();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            let tools = &tools;
            s.spawn(move || tx.send(tools.call("system_factions", &json!({ "system": "Nowhere" }))).ok());
            let answer = rx.recv_timeout(std::time::Duration::from_secs(5));
            drop(guard);
            let e = answer.expect("system_factions waited for the journal index").unwrap_err();
            assert!(e.contains("Nowhere") && e.contains("system_info"), "{e}");
        });
    }

    #[test]
    fn unvisited_system_says_so() {
        let (dir, rt) = (JournalDir::new("factions-unvisited"), runtime());
        let tools = journal_tools(&dir, &[gamma_jump(&at(10)), alpha_location(&at(20))], &rt);
        let e = tools.call("system_factions", &json!({ "system": "Nowhere" })).unwrap_err();
        assert!(e.contains("Nowhere") && e.contains("system_info"), "{e}");
    }

    /// A system with ten factions, each in three states of every kind, still fits the assistant's result size.
    #[test]
    fn system_factions_stays_within_the_assistants_result_size() {
        let (dir, rt) = (JournalDir::new("factions-big"), runtime());
        let states = |names: [&str; 3]| names.iter().map(|n| format!(r#"{{ "State":"{n}", "Trend":0 }}"#)).collect::<Vec<_>>().join(", ");
        let factions: Vec<String> = (0..10)
            .map(|i| {
                format!(
                    r#"{{ "Name":"The Very Long Named Minor Faction Number {i} Of Gamma", "FactionState":"CivilUnrest", "Government":"Confederacy", "Influence":0.1, "Allegiance":"Independent", "Happiness":"$Faction_HappinessBand2;", "Happiness_Localised":"Discontented", "MyReputation":-12.5, "ActiveStates":[ {} ], "PendingStates":[ {} ], "RecoveringStates":[ {} ] }}"#,
                    states(["CivilUnrest", "Outbreak", "Lockdown"]),
                    states(["Expansion", "Retreat", "Famine"]),
                    states(["Drought", "Blight", "Bust"])
                )
            })
            .collect();
        let jump = gamma_jump(&at(10));
        let start = jump.find(r#""Factions":["#).unwrap();
        let big = format!(r#"{}"Factions":[ {} ] }}"#, &jump[..start], factions.join(", "));
        let tools = journal_tools(&dir, &[big, alpha_location(&at(20))], &rt);
        let v = tools.call("system_factions", &json!({ "system": "Gamma" })).unwrap();
        assert_eq!(v["factions"].as_array().unwrap().len(), 10, "{v}");
        assert!(v.to_string().len() < nexus_ai::agent::MAX_RESULT_CHARS, "{} chars", v.to_string().len());
    }

    /// After a promotion the progress the game reported at login belongs to the old rank: it isn't given with the new
    /// one (review note 9 on PR #14), and a squadron promotion brings its rank's name.
    #[test]
    fn progress_after_a_promotion() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let t = at(60);
        let m = with_lines(
            commander(),
            &[
                format!(r#"{{ "timestamp":"{t}", "event":"SquadronStartup", "SquadronName":"VANGUARDS OF TEST", "SquadronID":42, "CurrentRank":4, "CurrentRankName":"$Squadron_DefaultRankName_Rank4;", "CurrentRankName_Localised":"Rookie" }}"#),
                format!(r#"{{ "timestamp":"{t}", "event":"Promotion", "Explore":7 }}"#),
                format!(r#"{{ "timestamp":"{t}", "event":"SquadronPromotion", "SquadronID":42, "SquadronName":"VANGUARDS OF TEST", "OldRank":4, "NewRank":3, "NewRankName":"$Squadron_DefaultRankName_Rank3;", "NewRankName_Localised":"Agent" }}"#),
            ],
        );
        let p = model_tools(m, &rt).call("progress", &json!({})).unwrap();
        let rank = |k: &str| p["ranks"].as_array().unwrap().iter().find(|r| r["rank"] == k).cloned().unwrap();
        assert_eq!((&rank("Explore")["name"], &rank("Explore")["progress_pct"]), (&json!("Pioneer"), &Value::Null), "{p}");
        assert_eq!(rank("Trade")["progress_pct"], 46, "the others keep theirs");
        assert!(p["notes"][0].as_str().unwrap().contains("promoted"), "{p}");
        assert_eq!(p["squadron"]["rank"], "Agent");
    }

    /// "What do I still need for my FSD?" (GitHub issue #4): the blueprint's materials against what the commander holds,
    /// the trades that cover what's short, and who offers the grade.
    #[test]
    fn engineering_plans_a_blueprint_against_what_you_hold() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let t = at(30);
        let m = with_lines(
            commander(),
            &[format!(r#"{{ "timestamp":"{t}", "event":"Materials", "Raw":[ {{ "Name":"arsenic", "Count":1 }} ], "Manufactured":[ {{ "Name":"chemicaldistillery", "Name_Localised":"Chemical Distillery", "Count":60 }} ], "Encoded":[ ] }}"#)],
        );
        let tools = model_tools(m, &rt);
        let name = |s: &str| nexus_data::materials::material(s).unwrap().name.clone();
        // G5 of Increased FSD Range, one roll: arsenic, chemical manipulators and datamined wake exceptions.
        let p = tools.call("engineering", &json!({ "module": "fsd", "blueprint": "increased range", "grade": "G5", "from_grade": 5, "rolls_per_grade": 1 })).unwrap();
        assert_eq!((&p["blueprint"], &p["from_grade"], &p["to_grade"]), (&json!("Increased FSD Range"), &json!(5), &json!(5)), "{p}");
        let needed = p["needed"].as_array().unwrap();
        let row = |n: &str| needed.iter().find(|r| r["name"] == n).cloned().unwrap_or_else(|| panic!("{n} in {p}"));
        assert_eq!((&row("Arsenic")["have"], row("Arsenic").get("short")), (&json!(1), None));
        assert_eq!((&row(&name("chemicalmanipulators"))["short"], &row(&name("chemicalmanipulators"))["kind"]), (&json!(1), &json!("manufactured")));
        assert!(needed[0]["short"].is_number() && needed[2].get("short").is_none(), "what's short comes first: {p}");
        // Chemical Distillery trades up to Chemical Manipulators; nothing spare trades for the encoded one.
        assert!(p["trades"].as_array().unwrap().iter().any(|t| t["get"] == name("chemicalmanipulators") && t["give"] == "Chemical Distillery" && t["trader"] == "manufactured"), "{p}");
        let still = &p["still_short"][0];
        assert_eq!(still["materials"], json!([name("dataminedwake")]), "{p}");
        assert!(still["why"].as_str().unwrap().contains("not enough spare"), "{p}");
        assert!(p["verdict"].as_str().unwrap().starts_with("Short of 2 of the 3 materials"), "{p}");
        // Felicity Farseer is unlocked: first, with how far she is; the others say how to unlock them.
        let engineers = p["engineers"].as_array().unwrap();
        assert_eq!((&engineers[0]["name"], &engineers[0]["status"], &engineers[0]["up_to_grade"]), (&json!("Felicity Farseer"), &json!("Unlocked"), &json!(5)), "{p}");
        assert!(engineers[0]["distance_ly"].as_f64().unwrap() > 100.0 && engineers[0].get("to_unlock").is_none(), "{p}");
        assert!(engineers[1]["to_unlock"].is_string(), "{p}");
        assert_eq!(p["current_ship"][0]["blueprint"], "Increased FSD Range");
        assert!(json_len(&p) <= Caller::Ask.budget(), "the whole plan fits Ask");
    }

    /// A module's blueprints and engineers, where the current ship's module stands, and an engineer on their own.
    #[test]
    fn engineering_lists_a_modules_blueprints_and_engineers() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = model_tools(commander(), &rt);
        let o = tools.call("engineering", &json!({ "module": "Frame Shift Drive" })).unwrap();
        assert!(o["blueprints"].as_array().unwrap().iter().any(|b| b["name"] == "Increased FSD Range" && b["max_grade"] == 5), "{o}");
        assert!(o["experimentals"].as_array().unwrap().iter().any(|x| x["name"] == "Mass Manager"), "{o}");
        assert_eq!(o["engineers"][0]["name"], "Felicity Farseer", "unlocked first: {o}");
        assert_eq!((&o["current_ship"][0]["grade"], &o["current_ship"][0]["experimental"]), (&json!(5), &json!("Mass Manager")), "{o}");
        // The current FSD has the blueprint at G5, fully rolled: nothing left to plan unless asked from scratch.
        let done = tools.call("engineering", &json!({ "blueprint": "Increased FSD Range" })).unwrap();
        assert_eq!(done["verdict"], "Nothing to roll.", "{done}");
        assert!(done["notes"][0].as_str().unwrap().contains("already has it at G5, fully rolled"), "{done}");
        let fresh = tools.call("engineering", &json!({ "blueprint": "Increased FSD Range", "from_grade": 1 })).unwrap();        assert_eq!((&fresh["from_grade"], &fresh["per_roll"].as_array().unwrap().len()), (&json!(1), &5), "{fresh}");
        // An engineer the commander hasn't heard of: where, and every step to unlock them.
        let ram = tools.call("engineering", &json!({ "engineer": "Ram Tah" })).unwrap();
        assert_eq!(ram["system"], "Meene");
        assert!(ram.get("status").is_none() && ram["to_unlock"].as_str().unwrap().contains("Classified Scan Databanks"), "{ram}");
        assert!(tools.call("engineering", &json!({})).is_err());
        assert!(tools.call("engineering", &json!({ "blueprint": "Overcharged" })).unwrap_err().contains("module="));
    }

    #[test]
    fn engineering_gives_up_detail_to_fit_the_callers_budget() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let mut tools = model_tools(commander(), &rt);
        for args in [json!({ "module": "Multi-cannon" }), json!({ "module": "fsd", "blueprint": "Increased FSD Range", "from_grade": 1, "experimental": "Mass Manager" })] {
            tools.budget = Caller::Ask.budget();
            let full = tools.call("engineering", &args).unwrap();
            assert!(json_len(&full) <= tools.budget, "{args}: the whole reply fits Ask");
            tools.budget = json_len(&full) - 1;
            let cut = tools.call("engineering", &args).unwrap();
            assert!(json_len(&cut) <= tools.budget, "{cut}");
            assert!(cut["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("left out to fit")), "{cut}");
        }
    }

    /// What a build needs bought, before Spansh is asked: a module fitted on the ship or in storage isn't searched for, the
    /// ship flown is owned, and items named with a ship must be on it.
    #[test]
    fn where_to_buy_searches_only_what_you_still_need() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let t = at(30);
        let m = with_lines(
            commander(),
            &[format!(r#"{{ "timestamp":"{t}", "event":"StoredModules", "MarketID":5001, "StationName":"Alpha Port", "StarSystem":"Alpha", "Items":[ {{ "Name":"$int_hyperdrive_size6_class5_name;", "Name_Localised":"Frame Shift Drive", "StorageSlot":1, "StarSystem":"Alpha", "MarketID":5001, "TransferCost":0, "TransferTime":0, "BuyPrice":3000000, "Hot":false }} ] }}"#)],
        );
        let tools = model_tools(m, &rt);
        let v = tools.call("where_to_buy", &json!({ "ship": "Anaconda", "items": ["6A FSD"] })).unwrap();
        assert_eq!(v["to_buy"], json!([]), "{v}");
        let why = |item: &str| v["not_searched"].as_array().unwrap().iter().find(|n| n["item"] == item).map(|n| n["why"].clone());
        assert_eq!(why("6A Frame Shift Drive"), Some(json!("1 fitted on your Anaconda, 1 in your storage in Alpha")), "{v}");
        assert!(why("Anaconda").unwrap().as_str().unwrap().starts_with("You own one"), "{v}");
        let not_fitted = tools.call("where_to_buy", &json!({ "ship": "Anaconda", "items": ["5A Thrusters"] })).unwrap_err();
        assert_eq!(not_fitted, "Not in that build: 5A Thrusters.");
        assert!(tools.call("where_to_buy", &json!({ "ship": "Cutter" })).unwrap_err().contains("No ship of the commander's matches"));
        assert!(tools.call("where_to_buy", &json!({ "build": "not a build" })).is_err());
    }

    /// Waits until a service listens, or says why it doesn't.
    fn listening(service: &crate::mcp::McpService) -> Result<(), String> {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let s = service.status();
            if s.listening {
                return Ok(());
            }
            if let Some(e) = s.error {
                return Err(e);
            }
            if Instant::now() > until {
                return Err("neither listening nor failed after 10 s".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// A second copy (another data folder) can't answer where the first does, and stopping it leaves the first's pipe or
    /// socket alone: agents still reach the first.
    #[test]
    fn a_second_copy_leaves_the_first_answering() {
        use crate::mcp::stdio::{Event, Session};
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let endpoint = crate::mcp::test_endpoint("two");
        let mut first = crate::mcp::McpService::default();
        first.start_at(endpoint.clone(), agent_tools(commander(), &rt), rt.handle());
        listening(&first).unwrap();
        let mut second = crate::mcp::McpService::default();
        second.start_at(endpoint.clone(), agent_tools(commander(), &rt), rt.handle());
        assert!(listening(&second).unwrap_err().contains("Another program already answers"));
        drop(second);
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut session = Session::new(endpoint, tx);
        let mut out = Vec::new();
        let meta = json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {} });
        session.handle(Event::Line(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "commander", "_meta": meta } }).to_string()), &mut out);
        session.handle(rx.recv_timeout(Duration::from_secs(30)).unwrap(), &mut out);
        let reply: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        drop(first);
    }

    /// Switched off while an agent's connection is open, and on again at once: the connection is closed, so the name is
    /// free and the app listens again.
    #[cfg(windows)]
    #[test]
    fn off_and_on_again_with_a_connection_open() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let endpoint = crate::mcp::test_endpoint("again");
        let mut service = crate::mcp::McpService::default();
        service.start_at(endpoint.clone(), agent_tools(commander(), &rt), rt.handle());
        listening(&service).unwrap();
        // An agent that connected and hasn't sent its call yet: its instance would otherwise stay taken for 10 s.
        let held = std::fs::OpenOptions::new().read(true).write(true).open(&endpoint).unwrap();
        service.apply(false, || unreachable!("on: not started"), rt.handle());
        service.start_at(endpoint, agent_tools(commander(), &rt), rt.handle());
        listening(&service).unwrap();
        drop(held);
    }

    /// An agent's call to a tool that answers from local data stays local: a system the galaxy database doesn't know
    /// isn't looked up on Spansh, so the call is the offline one its `online: false` (and openWorldHint) says.
    #[test]
    fn an_agents_local_tool_never_asks_spansh() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let err = agent_tools(commander(), &rt).call("find_commodity", &json!({ "commodity": "Gold", "near_system": "Nowhere Special" })).unwrap_err();
        assert!(err.contains("isn't in Elite Nexus' galaxy database"), "{err}");
    }

    /// An AI agent over MCP, end to end: `--mcp`'s session passes a call over a pipe (a socket off Windows) of the test's
    /// own to the app's listener, which answers from the commander's data; switched off, the agent is told.
    #[test]
    fn an_agent_reads_the_commander_over_mcp() {
        use crate::mcp::stdio::{Event, Session};
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let endpoint = crate::mcp::test_endpoint("e2e");
        let mut service = crate::mcp::McpService::default();
        service.start_at(endpoint.clone(), agent_tools(commander(), &rt), rt.handle());
        let until = Instant::now() + Duration::from_secs(10);
        while !service.status().listening {
            assert!(Instant::now() < until && service.status().error.is_none(), "{:?}", service.status());
            std::thread::sleep(Duration::from_millis(20));
        }
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut session = Session::new(endpoint, tx);
        let meta = json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {} });
        let mut ask = |id: i64, name: &str| -> Value {
            let mut out = Vec::new();
            session.handle(Event::Line(json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": {}, "_meta": meta } }).to_string()), &mut out);
            session.handle(rx.recv_timeout(Duration::from_secs(30)).expect("an answer"), &mut out);
            serde_json::from_slice(&out).expect("one message")
        };
        let reply = ask(1, "commander");
        assert_eq!((&reply["id"], &reply["result"]["isError"]), (&json!(1), &json!(false)), "{reply}");
        let answer: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(answer["commander"], "Tester");
        assert_eq!(service.status().answered, 1);
        assert_eq!(service.status().last.map(|(t, _)| t), Some("commander"));
        // Switched off: the agent hears why.
        service.apply(false, || unreachable!("not started again"), rt.handle());
        let off = ask(2, "commander");
        assert_eq!(off["result"]["isError"], true, "{off}");
        assert!(off["result"]["content"][0]["text"].as_str().unwrap().contains("Settings › AI agents"), "{off}");
    }

    #[test]
    fn new_tools_are_appended_and_old_ones_unchanged() {
        let old = [
            "commander", "find_commodity", "price_deals", "untouched_biology", "sell_my_cargo", "trade_routes", "nearest_service", "mining_hotspots",
            "mining_sites", "system_info", "system_bodies", "plan_route", "money_routes", "exobiology_here", "journal_search", "knowledge",
        ];
        let names: Vec<&str> = specs(Caller::Ask).iter().map(|s| s.name).collect();
        assert_eq!(&names[..old.len()], &old, "the first 16 specs keep their order");
        assert!(names.len() > old.len());
        let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len(), "tool names are unique");
        // With no commander loaded the tools fail early (no database, no network), but every one of them is known.
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = model_tools(Model::default(), &rt);
        for name in names {
            if let Err(e) = tools.call(name, &json!({})) {
                assert!(!e.contains("Unknown tool"), "{name} is in specs() but not handled by call()");
            }
        }
    }

    /// "Where did I mine Iridium?": the Rhino's mining locations, from the journal.
    #[test]
    fn mining_sites_answers_where_the_rhino_worked() {
        let mut m = commander();
        let lines = [
            r#"{ "timestamp":"2026-10-20T10:03:00Z", "event":"Touchdown", "PlayerControlled":true, "StarSystem":"Alpha", "SystemAddress":1001, "Body":"Alpha 1", "BodyID":1, "OnStation":false, "OnPlanet":true, "Latitude":-12.5, "Longitude":44.1, "NearestDestination":"$SAA_Unknown_Signal:#type=$PlanetaryMiningLocation_Name;:#index=4;" }"#,
            r#"{ "timestamp":"2026-10-20T10:04:00Z", "event":"LaunchSRV", "SRVType":"mev_rhino", "Loadout":"default", "ID":53, "PlayerControlled":true }"#,
            r#"{ "timestamp":"2026-10-20T10:10:00Z", "event":"MiningRefined", "Type":"$iridium_name;", "Type_Localised":"Iridium" }"#,
        ];
        let mut quiet = Alerts::disabled();
        for line in lines {
            let (ts, name) = scan_line(line).expect("line");
            let ev = Event::parse(name, line).expect("event");
            m.apply(&EventCtx { ts, name, line, live: false }, &ev, &mut quiet);
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let tools = NexusTools {
            model: Arc::new(RwLock::new(m)),
            db: None,
            index: Arc::new(RwLock::new(JournalIndex::default())),
            spansh: Spansh::new(),
            rt: rt.handle().clone(),
            routes: Arc::new(Mutex::new(None)),
            caller: Caller::Ask,
            budget: Caller::Ask.budget(),
        };
        let found = tools.call("mining_sites", &json!({ "commodity": "iridium" })).unwrap();
        assert_eq!(found["sites"][0]["body"], "Alpha 1");
        assert_eq!(found["sites"][0]["mining_location"], 4);
        assert_eq!(found["sites"][0]["refined_t"]["Iridium"], 1);
        assert!(tools.call("mining_sites", &json!({ "commodity": "Painite" })).unwrap()["sites"].as_array().unwrap().is_empty());
    }

    fn eddn(schema: &str, message: Value) -> Value {
        json!({ "$schemaRef": format!("https://eddn.edcd.io/schemas/{schema}"), "header": { "uploaderID": "t", "softwareName": "t", "softwareVersion": "1", "gameversion": "4.4.1.1" }, "message": message })
    }

    /// A landable planet `Scan` (journal / EDDN form).
    fn planet_scan(ts: i64, id: u32, name: &str, class: &str, atmosphere: (&str, &str), footfalled: bool) -> Value {
        json!({ "timestamp": nexus_journal::Timestamp(ts).to_iso(), "event": "Scan", "ScanType": "Detailed", "BodyName": name, "BodyID": id,
            "StarSystem": "Swoilz MH-W c17-3", "SystemAddress": 908552967066u64, "DistanceFromArrivalLS": 1209.4 + f64::from(id),
            "TerraformState": "", "PlanetClass": class, "Atmosphere": atmosphere.0, "AtmosphereType": atmosphere.1, "Volcanism": "",
            "SurfaceGravity": 0.876, "SurfaceTemperature": 165.16, "SurfacePressure": 1107.5, "Landable": true,
            "WasDiscovered": true, "WasMapped": false, "WasFootfalled": footfalled, "StarPos": [-28.8, -112.4, -96.9] })
    }

    /// Footfall from EDDN and the journal, predictions from the database's physics; Spansh offline.
    #[test]
    fn system_bodies_reports_footfall_and_biology() {
        let t = TempDb::new("ai-bodies");
        let db = t.db();
        let now = nexus_journal::Timestamp::now().0;
        let (day, sys) = (86_400, 908552967066u64);
        let iso = |ts: i64| nexus_journal::Timestamp(ts).to_iso();
        // The commander jumped there (unpopulated; our own journal keeps it) and scanned a bare icy moon.
        db.ingest_journal("FSDJump", &json!({ "timestamp": iso(now - 4 * day), "event": "FSDJump", "StarSystem": "Swoilz MH-W c17-3",
            "SystemAddress": sys, "StarPos": [-28.8, -112.4, -96.9], "Population": 0 }));
        db.ingest_journal("Scan", &planet_scan(now - 4 * day, 12, "Swoilz MH-W c17-3 B 1", "Icy body", ("", "None"), false));
        // EDDN: another commander's FSS of a biology world, not footfalled three days ago...
        let fss = |ts: i64, id: u32, name: &str, bio: u32| {
            eddn("fssbodysignals/1", json!({ "timestamp": iso(ts), "event": "FSSBodySignals", "BodyName": name, "BodyID": id, "SystemAddress": sys,
                "StarSystem": "Swoilz MH-W c17-3", "StarPos": [-28.8, -112.4, -96.9], "Signals": [{ "Type": "$SAA_SignalType_Biological;", "Count": bio }] }))
        };
        db.ingest_eddn(&fss(now - 3 * day - 1, 6, "Swoilz MH-W c17-3 A 4", 3));
        db.ingest_eddn(&eddn(
            "journal/1",
            planet_scan(now - 3 * day, 6, "Swoilz MH-W c17-3 A 4", "High metal content body", ("thin carbon dioxide atmosphere", "CarbonDioxide"), false),
        ));
        // ...and a biology moon where somebody sampled organics yesterday.
        db.ingest_eddn(&fss(now - 2 * day, 7, "Swoilz MH-W c17-3 A 5 a", 1));
        db.ingest_eddn(&eddn("scanorganic/1", json!({ "timestamp": iso(now - day), "event": "ScanOrganic", "ScanType": "Sample",
            "Genus": "$Codex_Ent_Bacterial_Genus_Name;", "Species": "$Codex_Ent_Bacterial_04_Name;", "SystemAddress": sys, "BodyID": 7,
            "StarSystem": "Swoilz MH-W c17-3", "StarPos": [-28.8, -112.4, -96.9] })));
        db.flush().unwrap();

        // multi-thread, like the app's: Handle::block_on can't drive IO on a current-thread runtime
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let offline = Spansh::builder().base_url("http://127.0.0.1:9").connect_timeout(Duration::from_millis(500)).build().unwrap();
        let tools = NexusTools {
            model: Arc::new(RwLock::new(commander())),
            db: Some(db.clone()),
            index: Arc::new(RwLock::new(JournalIndex::default())),
            spansh: offline,
            rt: rt.handle().clone(),
            routes: Arc::new(Mutex::new(None)),
            caller: Caller::Ask,
            budget: Caller::Ask.budget(),
        };
        let out = tools.call("system_bodies", &json!({ "system": "swoilz mh-w c17-3" })).unwrap();
        assert_eq!(out["system"], "Swoilz MH-W c17-3");
        assert_eq!(out["population"], 0);
        assert_eq!(out["relevant_bodies"], 3);
        let bodies = out["bodies"].as_array().unwrap();
        let names: Vec<&str> = bodies.iter().map(|b| b["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["A 4", "A 5 a", "B 1"], "biology first, the valuable one first: {out:#}");
        let a4 = &bodies[0];
        assert_eq!(a4["footfall"], json!({ "status": "not footfalled", "as_of": "3 days ago", "source": "EDDN" }));
        assert_eq!(a4["bio_signals"], 3);
        assert_eq!(a4["first_logged_bonus_likely"], true);
        assert!(a4["expected_value_cr"].as_i64().unwrap() > 1_000_000, "{a4}");
        assert!(!a4["likely_species"].as_array().unwrap().is_empty());
        assert_eq!(a4["discovered"], true);
        assert_eq!(a4["mapped"], false);
        assert_eq!(a4["gravity_g"], 0.09);
        let a5a = &bodies[1];
        assert_eq!(a5a["footfall"]["status"], "footfalled");
        assert_eq!(a5a["footfall"]["as_of"], "24 hours ago");
        assert_eq!(a5a["first_logged_bonus_likely"], false);
        assert_eq!(bodies[2]["footfall"]["source"], "your journal");
        assert!(bodies[2].get("bio_signals").is_none());
        let notes = out["notes"].to_string();
        assert!(notes.contains("EDDN") && notes.contains("Spansh is unavailable"), "{notes}");
        assert!(out.to_string().len() < 6000);

        // One body, by the part after the system name or by its full name given as the system.
        let one = tools.call("system_bodies", &json!({ "system": "Swoilz MH-W c17-3", "body": "a 4" })).unwrap();
        assert_eq!(one["bodies"].as_array().unwrap().len(), 1);
        let one = tools.call("system_bodies", &json!({ "system": "Swoilz MH-W c17-3 A 5 a" })).unwrap();
        assert_eq!(one["bodies"][0]["name"], "A 5 a", "{one}");
        let none = tools.call("system_bodies", &json!({ "system": "Swoilz MH-W c17-3", "body": "C 9" })).unwrap();
        assert!(none["notes"].to_string().contains("A 4"), "lists the known bodies: {none}");
        assert!(tools.call("system_bodies", &json!({ "system": "Nowhere At All" })).is_err());
        assert!(tools.call("system_bodies", &json!({})).is_err());

        // money_routes: footfall of a stop's listed bodies (Spansh body id64 = system + body id << 55)
        let stop = ScanSystem {
            name: "Swoilz MH-W c17-3".into(),
            id64: Some(sys),
            bodies: vec![
                nexus_spansh::ScanBody { name: "Swoilz MH-W c17-3 A 4".into(), id64: Some(sys | (6 << 55)), ..Default::default() },
                nexus_spansh::ScanBody { name: "Swoilz MH-W c17-3 C 1".into(), id64: Some(sys | (40 << 55)), ..Default::default() },
            ],
            ..Default::default()
        };
        let f = tools.route_footfall(&stop);
        assert_eq!(f, vec![json!({ "body": "A 4", "status": "not footfalled", "as_of": "3 days ago", "source": "EDDN" })], "unknown bodies are left out");
    }

    #[test]
    fn footfall_order_and_wording() {
        let row = |footfalled: Option<Seen>, source: Source, updated: i64| BodyInfo {
            system_id64: 5,
            body_id: 1,
            was_footfalled: footfalled,
            footfall_source: footfalled.map(|_| source),
            updated_unix: updated,
            ..BodyInfo::default()
        };
        let spansh = |updated: Option<i64>| SpanshBody {
            body_id: 1,
            name: "X 1".into(),
            class: Some("Rocky body".into()),
            landable: true,
            distance_ls: None,
            gravity_g: None,
            temperature_k: None,
            atmosphere: None,
            volcanism: None,
            terraform: None,
            bio: 2,
            geo: 0,
            genera: Vec::new(),
            updated_unix: updated,
            facts: BodyFacts::default(),
        };
        let now = nexus_journal::Timestamp::now().0;
        let ours = row(Some(Seen { value: false, at_unix: now - 7_200 }), Source::Journal, now);
        let p = Planet { id: 1, db: Some(&ours), sp: None, own: None };
        assert_eq!(Footfall::local(&p, true), Footfall::Populated, "populated beats everything");
        let local = Footfall::local(&p, false);
        assert_eq!(local.json(), json!({ "status": "not footfalled", "as_of": "2 hours ago", "source": "your journal" }));
        assert_eq!(local.footfalled(), Some(false));
        // Spansh only knows the body from before Odyssey and nothing local is newer: probably unclaimed.
        let old = spansh(Some(1_558_000_000)); // May 2019
        let p = Planet { id: 1, db: None, sp: Some(&old), own: None };
        assert_eq!(Footfall::local(&p, false), Footfall::Unknown);
        let pre = Footfall::pre_odyssey(&p);
        assert_eq!(pre.json()["status"], "probably unclaimed");
        assert_eq!(pre.json()["as_of"], "on 2019-05-16");
        assert_eq!(pre.footfalled(), Some(false), "counts as unclaimed for the bonus estimate");
        let seen_since = row(None, Source::Eddn, now - 86_400);
        let p = Planet { id: 1, db: Some(&seen_since), sp: Some(&old), own: None };
        assert_eq!(Footfall::pre_odyssey(&p), Footfall::Unknown, "an EDDN message since then: no longer only old data");
        let recent = spansh(Some(now - 86_400));
        assert_eq!(Footfall::pre_odyssey(&Planet { id: 1, db: None, sp: Some(&recent), own: None }), Footfall::Unknown);
        let archived = Footfall::Known { seen: Seen { value: true, at_unix: now - 10 * 86_400 }, source: Source::EddnArchive };
        assert_eq!(archived.json(), json!({ "status": "footfalled", "as_of": "10 days ago", "source": "EDDN archive (EDGalaxyData)" }));
        assert_eq!(Footfall::Unknown.json(), json!({ "status": "unknown" }));
        // which archived scans can carry the flag
        assert!(carries_footfall("4.2.1.0") && carries_footfall("4.3.1.0") && carries_footfall("5.0"));
        assert!(!carries_footfall("4.2.0.0") && !carries_footfall("4.1.0.0") && !carries_footfall("3.8.0.407"));
        assert!(carries_footfall(""), "unknown versions get a try");
        assert!(!ONLINE_FOOTFALL_LOOKUP.load(Ordering::Relaxed), "no network in unit tests");
    }

    #[test]
    fn spansh_names_map_to_journal_names() {
        assert_eq!(journal_planet_class("High metal content world"), "High metal content body");
        assert_eq!(journal_planet_class("Earth-like world"), "Earthlike body");
        assert_eq!(journal_planet_class("Class III gas giant"), "Sudarsky class III gas giant");
        assert_eq!(journal_planet_class("Icy body"), "Icy body");
        assert_eq!(journal_star_type("K (Yellow-Orange) Star").as_deref(), Some("K"));
        assert_eq!(journal_star_type("M (Red giant) Star").as_deref(), Some("M_RedGiant"));
        assert_eq!(journal_star_type("A (Blue-White super giant) Star").as_deref(), Some("A_BlueWhiteSuperGiant"));
        assert_eq!(journal_star_type("White Dwarf (DA) Star").as_deref(), Some("DA"));
        assert_eq!(journal_star_type("Wolf-Rayet NC Star").as_deref(), Some("WNC"));
        assert_eq!(journal_star_type("Herbig Ae/Be Star").as_deref(), Some("AeBe"));
        assert_eq!(journal_star_type("T Tauri Star").as_deref(), Some("TTS"));
        assert_eq!(journal_star_type("Neutron Star").as_deref(), Some("N"));
        assert_eq!(short_name("Swoilz MH-W c17-3 A 4", "Swoilz MH-W c17-3"), "A 4");
        assert_eq!(short_name("Earth", "Sol"), "Earth");
        assert!(body_matches("Swoilz MH-W c17-3 A 4", "Swoilz MH-W c17-3", "A 4"));
        assert!(!body_matches("Swoilz MH-W c17-3 A 4 a", "Swoilz MH-W c17-3", "A 4"));
    }

    #[test]
    fn find_material_body_rejects_non_raw() {
        let rt = runtime();
        let tools = model_tools(commander(), &rt);
        let e = tools.call("find_material_body", &json!({ "material": "Biotech Conductors" })).unwrap_err();
        assert!(e.contains("raw"), "{e}");
        assert!(tools.call("find_material_body", &json!({ "material": "Unobtainium" })).is_err());
        assert!(tools.call("find_material_body", &json!({})).is_err());
    }

    /// A runtime that drives network I/O for `Handle::block_on` (a current-thread one only does inside its own `block_on`).
    fn live_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap()
    }

    /// Live: `cargo test -p nexus-app live_find -- --ignored --nocapture` (needs the internet; reads Spansh only).
    #[test]
    #[ignore]
    fn live_find_material_body() {
        let rt = live_runtime();
        let tools = model_tools(commander(), &rt);
        let v = tools.call("find_material_body", &json!({ "material": "Polonium", "near_system": "Deciat" })).unwrap();
        println!("{v:#}");
        let bodies = v["bodies"].as_array().unwrap();
        assert!(!bodies.is_empty(), "{v}");
        let shares: Vec<f64> = bodies.iter().map(|b| b["share_pct"].as_f64().unwrap()).collect();
        assert!(shares.windows(2).all(|w| w[0] >= w[1]), "richest first: {shares:?}");
        assert!(bodies.iter().all(|b| b["system"].is_string() && b["gravity_g"].as_f64().is_some_and(|g| g <= 1.0)));
        assert!(v["bodies_holding_it"].as_u64().unwrap() > 0);
    }

    /// The whole chain with a built program, as an agent starts it: `EliteNexus --mcp` with piped stdin and stdout,
    /// talking to a listener on this user's real pipe that answers from a test commander. Set `ELITE_NEXUS_EXE` to the
    /// program and run `cargo test -p nexus-app live_agent_chain -- --ignored`, with no Elite Nexus answering agents.
    #[test]
    #[ignore]
    fn live_agent_chain() {
        use std::io::{BufRead, Write};
        let exe = std::env::var_os("ELITE_NEXUS_EXE").expect("ELITE_NEXUS_EXE: the program to start");
        let rt = live_runtime();
        let mut service = crate::mcp::McpService::default();
        service.start_at(crate::mcp::endpoint().unwrap(), agent_tools(commander(), &rt), rt.handle());
        let until = Instant::now() + Duration::from_secs(10);
        while !service.status().listening {
            assert!(Instant::now() < until && service.status().error.is_none(), "{:?}", service.status());
            std::thread::sleep(Duration::from_millis(20));
        }
        let piped = std::process::Stdio::piped;
        let mut child = std::process::Command::new(exe).arg("--mcp").stdin(piped()).stdout(piped()).stderr(piped()).spawn().expect("the program starts");
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        let text = {
            let mut ask = |msg: Value| -> Value {
                writeln!(stdin, "{msg}").unwrap();
                let mut line = String::new();
                stdout.read_line(&mut line).unwrap();
                serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"))
            };
            let init = ask(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } } }));
            assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
            let call = ask(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "commander", "arguments": {} } }));
            let text = call["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string();
            assert!(call["result"]["isError"] == false && text.contains("\"Tester\""), "{call}");
            text
        };
        drop(stdin);
        let status = child.wait().unwrap();
        assert!(status.success(), "exits when stdin closes: {status}");
        let mut err = String::new();
        let _ = std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err);
        assert!(err.is_empty(), "nothing on stderr: {err}");
        println!("{text}");
    }

    /// Live: `cargo test -p nexus-app live_where_to_mine -- --ignored --nocapture` (needs the internet; reads Spansh only).
    #[test]
    #[ignore]
    fn live_where_to_mine() {
        let rt = live_runtime();
        let tools = model_tools(commander(), &rt);
        let near = tools.call("where_to_mine", &json!({ "near_system": "Sol", "class": "rocky", "sort": "most_locations" })).unwrap();
        println!("{near:#}");
        let bodies = near["bodies"].as_array().unwrap();
        assert!(!bodies.is_empty(), "{near}");
        let counts: Vec<u64> = bodies.iter().map(|b| b["mining_locations"].as_u64().unwrap()).collect();
        assert!(counts.windows(2).all(|w| w[0] >= w[1]), "most locations first: {counts:?}");
        assert!(bodies.iter().all(|b| b["ground"].as_str().unwrap().starts_with("Rocky")), "{near}");
        let route = tools.call("where_to_mine", &json!({ "near_system": "Sol", "to_system": "Colonia", "stops": 5 })).unwrap();
        println!("{route:#}");
        assert!(!route["stops"].as_array().unwrap().is_empty() && route["length_ly"].as_f64().unwrap() > 20_000.0, "{route}");
    }

    /// Live: `cargo test -p nexus-app live_where_to_buy -- --ignored --nocapture` (needs the internet; reads Spansh only).
    #[test]
    #[ignore]
    fn live_where_to_buy() {
        let rt = live_runtime();
        let tools = model_tools(commander(), &rt);
        let v = tools.call("where_to_buy", &json!({ "items": ["5A Frame Shift Drive", "5A Thrusters"], "near_system": "Sol" })).unwrap();
        println!("{v:#}");
        let first = &v["sell_everything"][0];
        assert!(first["station"].is_string() && first["cost_cr"].as_u64().unwrap() > 0, "{v}");
        assert_eq!(v["landing_pad"], "large", "the Anaconda being flown needs a large pad");
    }
}

#[cfg(test)]
mod real_data {
    use super::*;
    use nexus_journal::{EventCtx, list_journal_files, load_history};
    use nexus_model::Alerts;

    /// Runs the local-data tools over this PC's real journals (prints the results).
    /// `cargo test -p nexus-app -- --ignored real_journals --nocapture`
    #[test]
    #[ignore]
    fn real_journals() {
        let Some(dir) = nexus_journal::default_journal_dir() else { return };
        let files = list_journal_files(&dir).unwrap_or_default();
        let mut model = Model::default();
        let mut index = JournalIndex::default();
        let mut quiet = Alerts::disabled();
        load_history(&files, 32, |f, fe| {
            index.add_file(f.path.clone());
            for e in &fe.events {
                model.apply(&EventCtx { ts: e.ts, name: fe.name(e), line: fe.line(e), live: false }, &e.event, &mut quiet);
            }
        });
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let t = crate::testutil::TempDb::new("ai-real");
        let context = context_line(model.active().unwrap());
        let tools = NexusTools {
            model: Arc::new(RwLock::new(model)),
            db: Some(t.db().clone()),
            index: Arc::new(RwLock::new(index)),
            spansh: Spansh::new(),
            rt: rt.handle().clone(),
            routes: Arc::new(Mutex::new(None)),
            caller: Caller::Ask,
            budget: Caller::Ask.budget(),
        };
        println!("context: {context}");
        for (name, args) in [
            ("commander", json!({})),
            ("exobiology_here", json!({})),
            ("journal_search", json!({ "query": "Stratum", "limit": 3 })),
            ("sell_my_cargo", json!({})),
            ("price_deals", json!({})),
            ("knowledge", json!({ "query": "first logged bonus" })),
        ] {
            let started = std::time::Instant::now();
            let out = tools.call(name, &args);
            let text = match &out {
                Ok(v) => v.to_string(),
                Err(e) => format!("ERROR {e}"),
            };
            println!("\n== {name} ({:.1} ms, {} chars)\n{}", started.elapsed().as_secs_f64() * 1e3, text.len(), text.chars().take(700).collect::<String>());
            assert!(out.is_ok(), "{name} failed: {text}");
        }
    }

    /// `system_bodies` over this PC's real data: the journal history replayed into a scratch galaxy
    /// database as at start-up, Spansh live (a few requests). Prints the tool output for systems with
    /// biology the commander visited.
    /// `cargo test -p nexus-app -- --ignored real_system_bodies --nocapture`
    #[test]
    #[ignore]
    fn real_system_bodies() {
        let Some(dir) = nexus_journal::default_journal_dir() else { return };
        let files = list_journal_files(&dir).unwrap_or_default();
        let mut model = Model::default();
        let mut quiet = Alerts::disabled();
        let t = crate::testutil::TempDb::new("ai-real-bodies");
        let db = t.db().clone();
        let started = std::time::Instant::now();
        let mut forwarded = 0usize;
        load_history(&files, 32, |_, fe| {
            for e in &fe.events {
                model.apply(&EventCtx { ts: e.ts, name: fe.name(e), line: fe.line(e), live: false }, &e.event, &mut quiet);
                if crate::market::relevant(fe.name(e), fe.line(e))
                    && let Ok(v) = serde_json::from_str::<Value>(fe.line(e))
                {
                    forwarded += 1;
                    db.ingest_journal(fe.name(e), &v);
                }
            }
        });
        db.flush().unwrap();
        println!("history: {} files, {forwarded} galaxy events, replayed in {:.0} ms", files.len(), started.elapsed().as_secs_f64() * 1e3);
        let gs = model.active().unwrap();
        let mut bio_systems: Vec<(String, u32)> = gs
            .exploration
            .systems
            .values()
            .map(|s| (s.name.clone(), s.bodies.values().map(|b| b.bio_signals).sum::<u32>()))
            .filter(|(_, bio)| *bio > 0)
            .collect();
        bio_systems.sort_by_key(|s| std::cmp::Reverse(s.1));
        println!("systems with biology in the journal: {bio_systems:?}");
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let tools = NexusTools {
            model: Arc::new(RwLock::new(model)),
            db: Some(db.clone()),
            index: Arc::new(RwLock::new(JournalIndex::default())),
            spansh: Spansh::new(),
            rt: rt.handle().clone(),
            routes: Arc::new(Mutex::new(None)),
            caller: Caller::Ask,
            budget: Caller::Ask.budget(),
        };
        ONLINE_FOOTFALL_LOOKUP.store(true, Ordering::Relaxed);
        // The archive lookup itself (TLS 1.3 through rustls): Synuefe BR-L c24-23 3 c has an EDMC scan
        // with the flag (game 4.3.1.0, 2026-03-21) and an EDDiscovery one without it.
        let started = std::time::Instant::now();
        let http = EDGD_HTTP.as_ref().expect("client");
        let found = rt.block_on(archive_footfall(http, 6406446944978, 27));
        println!("archive lookup Synuefe BR-L c24-23 3 c ({:.0} ms): {found:?}", started.elapsed().as_secs_f64() * 1e3);
        let found = found.expect("EDGalaxyData answers").expect("a scan with WasFootfalled");
        assert_eq!((found.was_footfalled, found.source), (Some(true), Source::EddnArchive));
        assert_eq!(found.name.as_deref(), Some("Synuefe BR-L c24-23 3 c"));
        let mut calls: Vec<Value> = bio_systems.iter().take(2).map(|(s, _)| json!({ "system": s })).collect();
        if let Some((s, _)) = bio_systems.first() {
            calls.push(json!({ "system": s, "body": "A 4" }));
        }
        // Systems the commander hasn't scanned: stops of a small exobiology route out in the black
        // (footfall from EDDN archive lookups / Spansh's age, not from the journal).
        if let Some((near, _)) = bio_systems.get(1) {
            let args = json!({ "kind": "exobiology", "radius_ly": 40, "max_systems": 4, "near_system": near });
            let route = tools.call("money_routes", &args);
            println!("\n== money_routes {args} {}", route.as_ref().map(|v| v.to_string()).unwrap_or_else(|e| format!("ERROR {e}")));
            for stop in route.iter().flat_map(|r| r["stops"].as_array().cloned().unwrap_or_default()).filter(|s| s["value_cr"].as_u64().unwrap_or(0) > 0).take(2) {
                calls.push(json!({ "system": stop["system"] }));
            }
        }
        let repeat = calls.last().cloned();
        calls.extend(repeat);
        for args in calls {
            let started = std::time::Instant::now();
            let out = tools.call("system_bodies", &args);
            let text = match &out {
                Ok(v) => serde_json::to_string_pretty(v).unwrap_or_default(),
                Err(e) => format!("ERROR {e}"),
            };
            println!("\n== system_bodies {args} ({:.0} ms, {} chars compact)\n{text}", started.elapsed().as_secs_f64() * 1e3, out.as_ref().map(|v| v.to_string().len()).unwrap_or(0));
            assert!(out.is_ok_and(|v| v.to_string().len() < nexus_ai::agent::MAX_RESULT_CHARS), "{text}");
            db.flush().unwrap(); // lookups are cached through the writer
        }
    }
}
