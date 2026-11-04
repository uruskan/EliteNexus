//! The assistant's tools: thin wrappers over the app's own data (game model, galaxy database,
//! journal, Spansh). Each returns compact JSON for the model.

use crate::finder;
use crate::pages::navigation::{PlannedRoute, from_spansh};
use nexus_ai::{ToolHost, ToolSpec};
use nexus_data::bio_predict::{self as bp, BodyFacts, ParentRef, Prediction, StarFacts, SystemStar};
use nexus_data::names::compact_credits;
use nexus_galaxy::{
    BodyInfo, BodyUpdate, CommodityQuery, DealQuery, GalaxyDb, HotspotQuery, Ingest, MaterialTrader, Seen, Services, Sort, Source, StationQuery,
    TechBroker, TradeMode, TradeQuery,
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

pub struct NexusTools {
    pub model: Arc<RwLock<Model>>,
    pub db: Option<GalaxyDb>,
    pub index: Arc<RwLock<JournalIndex>>,
    pub spansh: Spansh,
    pub rt: Handle,
    /// Routes the assistant planned, picked up by the Navigation page.
    pub routes: Arc<Mutex<Option<PlannedRoute>>>,
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

    /// Position and name of `near_system` (local database, then Spansh), or the commander's.
    fn origin(&self, args: &Value, me: &Me) -> Result<([f64; 3], String), String> {
        if let Some(name) = arg_str(args, "near_system").filter(|n| !n.eq_ignore_ascii_case(&me.system)) {
            if let Some(db) = &self.db
                && let Ok(Some(s)) = db.system_by_name(&name)
                && let Some(p) = s.pos
            {
                return Ok((p, s.name));
            }
            let found = self.rt.block_on(self.spansh.system_by_name(&name)).map_err(|e| format!("Spansh: {e}"))?;
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
        let (origin, near) = self.origin(a, &me)?;
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
        let (mut list, st) = self.rt.block_on(crate::untouched::find(&self.spansh, &q, footfalled))?;
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
        let bodies: Vec<Value> = sys
            .bodies
            .values()
            .filter(|b| b.bio_signals > 0)
            .map(|b| {
                json!({
                    "body": b.name, "biological_signals": b.bio_signals, "distance_ls": b.distance_ls.round(),
                    "genera_from_dss": b.genuses,
                    "sampled": b.organics.iter().map(|o| json!({ "species": o.species, "samples": o.samples, "analysed": o.analysed })).collect::<Vec<_>>(),
                    "likely_species": b.likely_species().iter().take(8).map(|p| json!({ "species": p.species, "value_cr": p.value, "likelihood_pct": (p.likelihood * 100.0).round(), "colony_range_m": p.colony_range_m })).collect::<Vec<_>>(),
                    "expected_value_cr": b.predicted_value(b.was_footfalled == Some(false)),
                })
            })
            .collect();
        Ok(json!({ "system": sys.name, "bio_bodies": bodies, "unsold_exobiology_data_cr": gs.exobio.unsold_value() }))
    }

    /// Journal lines holding a text, newest first, in a period if one is given. It returns at most `limit` of them, and
    /// only as many as fit what the model is passed (`MAX_RESULT_CHARS`), says when older matches were left out, and
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
        // Room for the rest of the answer under the cut the agent makes.
        let budget = nexus_ai::agent::MAX_RESULT_CHARS - 1_000;
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
        if listed > entries.len() {
            notes.push(format!("{} more bodies not shown; ask about one by name.", listed - entries.len()));
        }
        let dist = my_pos.zip(place.pos).map(|(a, b)| r1(((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()));
        let mut out = json!({
            "system": place.name, "distance_ly": dist, "population": place.population, "region": region,
            "relevant_bodies": total, "bodies": entries.into_iter().map(|e| e.3).collect::<Vec<_>>(), "notes": notes,
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
            let found = match self.rt.block_on(archive_footfall(http, system_id64, p.id)) {
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
/// use them; the exobiology rules use BioScan's slightly different constants.
pub(crate) const STANDARD_G: f64 = 9.80665;
pub(crate) const STANDARD_ATM: f64 = 101_325.0;
/// Bodies listed by `system_bodies` (the agent cuts tool results at 12 000 characters).
const MAX_BODIES: usize = 20;
const SPANSH_TIMEOUT: Duration = Duration::from_secs(25);
const SPANSH_CACHE_TTL: Duration = Duration::from_secs(600);

const FOOTFALL_NOTE: &str = "Footfall (has anyone set foot on the body) is in no public database (Spansh, EDSM, Inara, EDAstro, Canonn). It comes from the game's WasFootfalled flag in scans since game 4.2.1 (Sept 2025): the commander's own journal (authoritative), scans other commanders shared over EDDN, and archived EDDN scans looked up on demand (EDGalaxyData). Only some tools upload the flag (EDMC, EDDI, GameGlass do; EDDiscovery and EDO Materials Helper don't), so \"unknown\" is common. It is the state when that scan was made: give its age, and remember that whoever scanned a body often landed right after. \"probably unclaimed\" is a rule of thumb: Spansh has no data on the body since before Odyssey (May 2021). First footfall is impossible in populated systems.";

/// Recently fetched Spansh dumps (the model often asks about the same system twice).
static SPANSH_SYSTEMS: Mutex<Vec<(u64, Instant, Arc<SpanshSystem>)>> = Mutex::new(Vec::new());

/// Footfall lookups in EDGalaxyData's EDDN index (archived EDDN messages per body), made on demand by
/// `system_bodies` for biology bodies whose footfall is unknown locally. It is a Patreon-funded
/// service without published limits, so: a few bodies per call, one request at a time, a day's
/// cache. Store `false` to switch the lookups off (e.g. from a setting); off in unit tests.
pub(crate) static ONLINE_FOOTFALL_LOOKUP: AtomicBool = AtomicBool::new(!cfg!(test));
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

    /// Step 5, a community rule of thumb: nobody has sent Spansh anything about the body since Odyssey
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

impl ToolHost for NexusTools {
    fn tools(&self) -> Vec<ToolSpec> {
        specs()
    }

    fn call(&self, name: &str, args: &Value) -> Result<Value, String> {
        match name {
            "commander" => self.commander(),
            "find_commodity" => self.find_commodity(args),
            "price_deals" => self.price_deals(args),
            "untouched_biology" => self.untouched_biology(args),
            "sell_my_cargo" => self.sell_my_cargo(args),
            "trade_routes" => self.trade_routes(args),
            "nearest_service" => self.nearest_service(args),
            "mining_hotspots" => self.mining_hotspots(args),
            "mining_sites" => self.mining_sites(args),
            "system_info" => self.system_info(args),
            "system_bodies" => self.system_bodies(args),
            "plan_route" => self.plan_route(args),
            "money_routes" => self.money_routes(args),
            "exobiology_here" => self.exobiology_here(),
            "journal_search" => self.journal_search(args),
            "knowledge" => {
                let q = arg_str(args, "query").ok_or("query is required")?;
                let hits = nexus_ai::knowledge::search(&q, 4);
                Ok(json!({ "sections": hits.into_iter().map(|(t, x)| json!({ "title": t, "text": x })).collect::<Vec<_>>() }))
            }
            other => Err(format!("Unknown tool \"{other}\".")),
        }
    }
}

/// The tool list sent to the model. Kept stable (providers cache it).
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "commander",
            description: "The commander's current state: location, docked station, ship (type, landing pad, cargo capacity, jump range, fuel, rebuy), cargo hold, credits, fleet carrier and squadron carrier, ranks, unsold exploration and exobiology data.",
            parameters: schema(json!({}), &[]),
        },
        ToolSpec {
            name: "find_commodity",
            description: "Where to buy or sell one commodity near a system, from the live market database (EDDN, refreshed in real time). Results respect the ship's landing pad size.",
            parameters: schema(
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
            ),
        },
        ToolSpec {
            name: "price_deals",
            description: "Best deals and abnormal prices right now: stations and fleet carriers paying far above the galactic average for any commodity (mode sell) or selling far below it (mode buy), ranked by what one full hold of the commander's ship earns. The fastest way to find unusually profitable trades. For abnormal prices anywhere (most are fleet-carrier orders, e.g. a carrier paying 100× the normal price), use max_distance_ly 0 and min_ratio 2 or more.",
            parameters: schema(
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
            ),
        },
        ToolSpec {
            name: "untouched_biology",
            description: "Find planets where a valuable exobiology species should grow (default Stratum Tectonicas, 19 M Cr, 95 M with the first-logged bonus) and that no data-sharing commander has DSS-mapped or sampled, so the ×5 first-logged bonus is likely still available. Searches Spansh around a system, leaves out populated systems and planets known to be footfalled, ranks by expected bonus value and distance.",
            parameters: schema(
                json!({
                    "species": { "type": "string", "description": "e.g. Stratum Tectonicas (default), Concha Biconcavis, Tussock Stigmasis, Fonticulua Segmentatus, Fonticulua Fluctus." },
                    "near_system": { "type": "string", "description": NEAR },
                    "max_distance_ly": { "type": "number", "description": "Default 500." },
                    "include_mapped": { "type": "boolean", "description": "Also list planets someone DSS-mapped (genus confirmed) but nobody sampled. Default true." },
                    "limit": { "type": "integer" }
                }),
                &[],
            ),
        },
        ToolSpec {
            name: "sell_my_cargo",
            description: "Where to sell everything currently in the hold: the best single stations (limited by their demand) and the best buyer for each item.",
            parameters: schema(json!({ "max_distance_ly": { "type": "number", "description": "Default 60." }, "near_system": { "type": "string", "description": NEAR } }), &[]),
        },
        ToolSpec {
            name: "trade_routes",
            description: "Best buy-here-sell-there trade routes near a system, sized to the ship's cargo capacity and the commander's credits.",
            parameters: schema(
                json!({
                    "near_system": { "type": "string", "description": NEAR },
                    "radius_ly": { "type": "number", "description": "Both stations within this distance, default 40." },
                    "max_hop_ly": { "type": "number", "description": "Maximum distance between the two stations, default 25." },
                    "limit": { "type": "integer" }
                }),
                &[],
            ),
        },
        ToolSpec {
            name: "nearest_service",
            description: "Nearest stations with a service, for the commander's landing pad size.",
            parameters: schema(
                json!({
                    "service": { "type": "string", "enum": ["raw_material_trader", "manufactured_material_trader", "encoded_material_trader", "human_tech_broker", "guardian_tech_broker", "interstellar_factors", "universal_cartographics", "vista_genomics", "shipyard", "outfitting", "black_market", "refuel", "repair"] },
                    "near_system": { "type": "string", "description": NEAR },
                    "limit": { "type": "integer" }
                }),
                &["service"],
            ),
        },
        ToolSpec {
            name: "mining_hotspots",
            description: "Known planetary-ring hotspots for a mineable commodity near a system, plus the best nearby places to sell it.",
            parameters: schema(
                json!({
                    "commodity": { "type": "string", "description": "e.g. Platinum, Painite, Low Temperature Diamonds, Void Opal, Tritium." },
                    "near_system": { "type": "string", "description": NEAR },
                    "max_distance_ly": { "type": "number", "description": "Default 100." },
                    "limit": { "type": "integer" }
                }),
                &["commodity"],
            ),
        },
        ToolSpec {
            name: "mining_sites",
            description: "Planetary mining locations the commander worked with the Rhino SRV (surface mining), newest first: system, body, the location's number as the game's navigation panel shows it, tonnes refined per commodity (by-products included), visits and the last visit. Use for \"where did I mine …\" or \"where was that site\" questions.",
            parameters: schema(
                json!({
                    "commodity": { "type": "string", "description": "Only locations that gave this, e.g. Iridium." },
                    "limit": { "type": "integer", "description": "Default 10." }
                }),
                &[],
            ),
        },
        ToolSpec {
            name: "system_info",
            description: "Facts about a star system from the local database: distance from the commander, population, economy, security, factions and their states, powerplay, stations.",
            parameters: schema(json!({ "system": { "type": "string" } }), &["system"]),
        },
        ToolSpec {
            name: "system_bodies",
            description: "The planets and moons of a star system worth landing on or scanning: type, distance, gravity, atmosphere, biological and geological signals, genera, the most likely species with their values and whether the ×5 first-logged bonus looks likely, plus discovery and footfall status: whether anyone has set foot on a body. No public site tracks footfall; it comes from the game's flag in the commander's own journal, in scans other commanders shared over EDDN, and in archived EDDN scans looked up online for biology bodies (EDGalaxyData), each with its age; \"probably unclaimed\" means Spansh has no data on the body since before Odyssey (a rule of thumb). Use it for any question about a planet, its biology or first footfall.",
            parameters: schema(
                json!({
                    "system": { "type": "string", "description": "Star system name." },
                    "body": { "type": "string", "description": "Only this body: its full name, or the part after the system name such as \"A 3\" or \"2 a\"." }
                }),
                &["system"],
            ),
        },
        ToolSpec {
            name: "plan_route",
            description: "Plans a route with Spansh: the exact plotter uses the commander's real ship (fuel, scooping, neutron boosts); the neutron plotter only needs the jump range. The route is put on the Navigation page and the next system is copied to the clipboard after every jump.",
            parameters: schema(
                json!({
                    "to": { "type": "string", "description": "Destination system." },
                    "from": { "type": "string", "description": "Start system; default: current." },
                    "kind": { "type": "string", "enum": ["exact", "neutron"] }
                }),
                &["to"],
            ),
        },
        ToolSpec {
            name: "money_routes",
            description: "Plans a money-making exploration trip with Spansh near a system: exobiology (planets where commanders already logged valuable species: certain, but base value only; for the first-logged bonus use untouched_biology), or road-to-riches bodies (earth-likes, water/ammonia worlds, terraformables). Spansh skips populated systems, so from the bubble the search widens itself until it finds something. Returns stops with values, the total and, where known, whether anyone has set foot on the listed bodies; the route goes to the Navigation page.",
            parameters: schema(
                json!({
                    "kind": { "type": "string", "enum": ["exobiology", "road_to_riches", "earth_likes", "ammonia_worlds"] },
                    "near_system": { "type": "string", "description": NEAR },
                    "radius_ly": { "type": "number", "description": "Search radius, 1 to 1,000 ly. Default 30; widened automatically when nothing is found." },
                    "max_systems": { "type": "integer", "description": "Default 15." }
                }),
                &[],
            ),
        },
        ToolSpec {
            name: "exobiology_here",
            description: "Bodies with biological signals in the current system: genera found, species already sampled, likely species with their values (predictions), and unsold exobiology data.",
            parameters: schema(json!({}), &[]),
        },
        ToolSpec {
            name: "journal_search",
            description: "Searches the commander's own game journal (every journal file in their journal folder; journal_starts is the first day it holds) for lines with a text, newest first, each with its local time. One call returns up to limit matches, fewer when the lines are long; more_matches says when older ones were left out and how to get them. For a period, such as a week of travel, pass from and to. Event names work as text: FSDJump (jumps), Docked, MarketSell, MissionCompleted. Use for questions about past trips, sales, missions, ships, places visited.",
            parameters: schema(
                json!({
                    "query": { "type": "string", "description": "Text to find, e.g. a system, station, ship, commodity or event name." },
                    "from": { "type": "string", "description": "First day to search, YYYY-MM-DD (the commander's local date)." },
                    "to": { "type": "string", "description": "Last day to search, YYYY-MM-DD, included." },
                    "skip": { "type": "integer", "description": "Leave out this many of the newest matches: the next page." },
                    "limit": { "type": "integer", "description": "Default 15, at most 40." }
                }),
                &["query"],
            ),
        },
        ToolSpec {
            name: "knowledge",
            description: "Searches the built-in knowledge base: game mechanics, money-making strategies, exobiology, mining, engineering, carriers, colonisation, useful websites, and how to use Elite Nexus.",
            parameters: schema(json!({ "query": { "type": "string" } }), &["query"]),
        },
    ]
}

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
            format!(r#"{{ "timestamp":"{now}", "event":"Loadout", "Ship":"anaconda", "ShipID":1, "ShipName":"", "ShipIdent":"", "HullValue":100, "ModulesValue":100, "HullHealth":1.0, "UnladenMass":400.0, "CargoCapacity":400, "MaxJumpRange":30.5, "FuelCapacity":{{ "Main":32.0, "Reserve":1.07 }}, "Rebuy":5000000, "Modules":[] }}"#),
            format!(r#"{{ "timestamp":"{now}", "event":"Cargo", "Vessel":"Ship", "Count":100, "Inventory":[ {{ "Name":"gold", "Name_Localised":"Gold", "Count":100, "Stolen":0 }} ] }}"#),
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
        assert_eq!(specs().len(), 16);

        // The full loop with a scripted model: one tool call, then an answer.
        struct Script(parking_lot::Mutex<Vec<nexus_ai::Reply>>);
        impl nexus_ai::Backend for Script {
            fn complete(&self, _s: &str, tools: &[ToolSpec], turns: &[nexus_ai::Turn]) -> Result<nexus_ai::Reply, nexus_ai::AiError> {
                assert_eq!(tools.len(), 16);
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
