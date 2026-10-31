//! The assistant's tools about the commander themselves: their fleet, their progress. Each takes one short read of
//! the model and returns compact JSON.

use super::{NexusTools, arg_bool, r1, strip_nulls};
use nexus_data::outfitting::{display_name, slot_display};
use nexus_data::ranks::RankKind;
use nexus_journal::Timestamp;
use nexus_model::GameState;
use nexus_model::fleet::{Ship, ShipModule};
use serde_json::{Value, json};

fn pct(x: f64) -> i64 {
    (x * 100.0).round() as i64
}

/// `Some(text)` unless it's empty.
fn text(s: &str) -> Option<&str> {
    Some(s.trim()).filter(|s| !s.is_empty())
}

fn engineered(m: &ShipModule) -> Option<Value> {
    let e = m.engineering.as_ref()?;
    let mut v = json!({
        "slot": slot_display(&m.slot), "module": display_name(&m.item),
        "blueprint": e.blueprint, "grade": e.level, "quality_pct": pct(e.quality),
        "engineer": e.engineer, "experimental": e.experimental,
    });
    strip_nulls(&mut v);
    Some(v)
}

fn module(m: &ShipModule) -> Value {
    let mut v = json!({
        "slot": slot_display(&m.slot), "module": display_name(&m.item), "on": m.on, "health_pct": pct(m.health),
        "value_cr": m.value, "engineered": m.engineering.is_some(),
    });
    strip_nulls(&mut v);
    v
}

/// Where a ship is. The current ship's own `location` is stale while it is flown, so that one comes from the
/// commander's position.
fn whereabouts(gs: &GameState, s: &Ship, now: Timestamp) -> Value {
    let mut v = if gs.fleet.current_ship_id == Some(s.id) {
        let loc = &gs.location;
        json!({ "system": text(&loc.system), "station": loc.station.as_ref().filter(|_| loc.docked).map(|st| &st.name), "situation": loc.situation() })
    } else {
        let l = &s.location;
        json!({
            "system": l.system, "station": l.station, "in_transit": l.in_transit.then_some(true),
            "arrives": l.arrives.filter(|_| l.in_transit).map(Timestamp::to_iso),
            "arrives_in_min": l.arrives.filter(|_| l.in_transit).map(|a| ((a.0 - now.0).max(0) as f64 / 60.0).round() as i64),
        })
    };
    strip_nulls(&mut v);
    v
}

fn ship(gs: &GameState, s: &Ship, all_modules: bool, now: Timestamp) -> Value {
    // A ship known only from StoredShips has no loadout: its jump range, cargo and hull would be zeros and a full
    // hull that nobody reported, so they are left out rather than shown.
    let loadout = !s.loadout_ts.is_zero();
    let mut v = json!({
        "id": s.id, "type": s.type_name, "name": text(&s.name), "ident": text(&s.ident),
        "current": gs.fleet.current_ship_id == Some(s.id),
        "where": whereabouts(gs, s, now),
        "value_cr": s.total_value(),
        "rebuy_cr": loadout.then_some(s.rebuy),
        "max_jump_range_ly": loadout.then(|| r1(s.max_jump_range)),
        "cargo_capacity_t": loadout.then_some(s.cargo_capacity),
        "hull_pct": loadout.then(|| pct(s.hull_health)),
        "hot": s.hot.then_some(true),
        "modules_as_of": loadout.then(|| s.loadout_ts.to_iso()),
        "engineered": s.modules.iter().filter_map(engineered).collect::<Vec<_>>(),
    });
    if all_modules {
        v["modules"] = s.modules.iter().map(module).collect();
    }
    strip_nulls(&mut v);
    v
}

/// How far an engineer is on the road to Unlocked, for sorting.
fn engineer_order(status: &str) -> u8 {
    match status {
        "Unlocked" => 0,
        "Invited" => 1,
        "Acquainted" => 2,
        "Known" => 3,
        _ => 4,
    }
}

impl NexusTools {
    pub(super) fn fleet(&self, a: &Value) -> Result<Value, String> {
        let all_modules = arg_bool(a, "all_modules").unwrap_or(false);
        let now = Timestamp::now();
        let model = self.model.read();
        let gs: &GameState = model.active().ok_or("No commander loaded yet.")?;
        // The ship being flown first, then the most valuable.
        let mut ships: Vec<&Ship> = gs.fleet.ships.values().collect();
        ships.sort_by_key(|s| (gs.fleet.current_ship_id != Some(s.id), std::cmp::Reverse(s.total_value())));
        Ok(json!({
            "commander": gs.cmdr.name,
            "ships": ships.into_iter().map(|s| ship(gs, s, all_modules, now)).collect::<Vec<_>>(),
            "total_value_cr": gs.fleet.total_value(),
        }))
    }

    pub(super) fn progress(&self) -> Result<Value, String> {
        let model = self.model.read();
        let gs: &GameState = model.active().ok_or("No commander loaded yet.")?;
        let c = &gs.cmdr;
        let mut engineers: Vec<_> = c.engineers.values().collect();
        engineers.sort_by(|a, b| (engineer_order(&a.progress), &a.name).cmp(&(engineer_order(&b.progress), &b.name)));
        let mut game = json!({
            "running": !gs.game_closed, "at_main_menu": gs.at_main_menu,
            "mode": text(&c.game_mode), "version": text(&c.game_version),
            "last_login": (!c.last_login.is_zero()).then(|| c.last_login.to_iso()),
        });
        strip_nulls(&mut game);
        Ok(json!({
            "commander": c.name,
            "ranks": RankKind::ALL.iter().map(|k| json!({
                "rank": format!("{k:?}"), "name": c.rank_name(*k), "rank_number": c.ranks.get(*k), "progress_pct": c.progress.get(*k),
            })).collect::<Vec<_>>(),
            "engineers": engineers.into_iter().map(|e| {
                let mut v = json!({ "name": e.name, "status": e.progress, "rank": e.rank, "rank_progress_pct": e.rank_progress });
                strip_nulls(&mut v);
                v
            }).collect::<Vec<_>>(),
            "superpower_reputation": {
                "empire": r1(c.reputation.empire), "federation": r1(c.reputation.federation),
                "alliance": r1(c.reputation.alliance), "independent": r1(c.reputation.independent),
            },
            "powerplay": c.powerplay.as_ref().map(|p| json!({
                "power": p.power, "rank": p.rank, "merits": p.merits, "session_merits": p.session_merits,
                "pledged_days": (p.time_pledged_secs.max(0) as f64 / 86_400.0).round() as i64,
            })),
            "squadron": c.squadron.as_ref().map(|s| json!({ "name": s.name, "rank": s.rank_name.clone().or_else(|| s.rank.map(|r| r.to_string())) })),
            "game": game,
        }))
    }
}
