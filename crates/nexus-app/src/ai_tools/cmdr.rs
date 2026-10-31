//! The assistant's tools about the commander themselves: their fleet, their progress, their materials, on-foot gear
//! and credits. Each takes one short read of the model and returns compact JSON.

use super::{NexusTools, arg_bool, arg_f64, arg_str, r1, strip_nulls};
use nexus_data::materials::{self, Material};
use nexus_data::outfitting::{display_name, slot_display};
use nexus_data::ranks::RankKind;
use nexus_journal::Timestamp;
use nexus_model::GameState;
use nexus_model::fleet::{Ship, ShipModule};
use nexus_model::inventory::{Locker, MaterialCategory, SuitLoadoutInfo};
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

/// How much of a ship `ship` writes out.
#[derive(Clone, Copy)]
struct Detail {
    /// Every module of the ship, not only the engineered ones.
    modules: bool,
    /// Only how many modules are engineered, not which.
    counts_only: bool,
}

fn ship(gs: &GameState, s: &Ship, detail: Detail, now: Timestamp) -> Value {
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
    });
    if detail.counts_only {
        v["engineered_count"] = s.engineered_modules().into();
    } else {
        v["engineered"] = s.modules.iter().filter_map(engineered).collect();
    }
    if detail.modules {
        v["modules"] = s.modules.iter().map(module).collect();
    }
    strip_nulls(&mut v);
    v
}

/// Ships matching what the commander asked for: an id, or a name, ident or type (as written, ignoring case); when
/// none matches exactly, a name, ident or type that contains it.
fn find_ships<'a>(gs: &'a GameState, query: &str) -> Vec<&'a Ship> {
    let q = query.trim().to_lowercase();
    let is = |s: &Ship, f: fn(&str, &str) -> bool| f(&s.name.to_lowercase(), &q) || f(&s.ident.to_lowercase(), &q) || f(&s.type_name.to_lowercase(), &q);
    let exact: Vec<&Ship> = gs.fleet.ships.values().filter(|s| s.id.to_string() == q || is(s, |a, b| a == b)).collect();
    if !exact.is_empty() {
        return exact;
    }
    gs.fleet.ships.values().filter(|s| is(s, |a, b| !a.is_empty() && a.contains(b))).collect()
}

/// Room left for the reply once the model's tool-result limit is taken off, so it is never cut mid-JSON.
const BUDGET: usize = nexus_ai::agent::MAX_RESULT_CHARS - 1_000;

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

/// One material the commander holds.
struct Held {
    name: String,
    count: i64,
    /// What the game's catalogue says about it (None for a material it doesn't list).
    info: Option<&'static Material>,
}

/// How a material is written, from the most to the least detailed.
fn material_row(h: &Held, form: u8) -> Value {
    let grade = h.info.map(|m| m.grade);
    let cap = h.info.map(|m| i64::from(m.cap()));
    let full = cap.map(|c| h.count >= c);
    match form {
        0 => {
            let mut v = json!({ "name": h.name, "count": h.count, "grade": grade, "cap": cap, "full": full });
            strip_nulls(&mut v);
            v
        }
        1 => {
            let mut v = json!({ "name": h.name, "count": h.count, "grade": grade, "full": full.filter(|f| *f) });
            strip_nulls(&mut v);
            v
        }
        _ => json!([h.name, h.count, grade]),
    }
}

/// `{ "distinct": n, "total": n }` for a bucket of the locker.
fn locker_counts(m: &std::collections::BTreeMap<String, nexus_model::inventory::LockerItem>) -> Value {
    json!({ "distinct": m.len(), "total": m.values().map(|i| i.count).sum::<i64>() })
}

/// A locker or backpack: `{ name: count }` for each kind, or only how many there are of each when the lists would
/// not fit.
fn locker(l: &Locker, counts_only: bool) -> Value {
    let mut v = if counts_only {
        json!({ "counts": { "items": locker_counts(&l.items), "components": locker_counts(&l.components), "consumables": locker_counts(&l.consumables), "data": locker_counts(&l.data) } })
    } else {
        let list = |m: &std::collections::BTreeMap<String, nexus_model::inventory::LockerItem>| {
            let mut out = serde_json::Map::new();
            for i in m.values().filter(|i| i.count > 0) {
                let n = out.entry(i.name.clone()).or_insert(json!(0));
                *n = json!(n.as_i64().unwrap_or(0) + i.count);
            }
            Value::Object(out)
        };
        json!({ "items": list(&l.items), "components": list(&l.components), "consumables": list(&l.consumables), "data": list(&l.data) })
    };
    v["as_of"] = (!l.updated.is_zero()).then(|| l.updated.to_iso()).into();
    strip_nulls(&mut v);
    v
}

/// A suit loadout: `detail` 0 = everything, 1 = no weapon modifications, 2 = only which suit and weapons.
fn loadout(l: &SuitLoadoutInfo, current: bool, detail: u8) -> Value {
    let mut v = json!({ "name": text(&l.name), "suit": l.suit_name, "current": current });
    if detail >= 2 {
        v["weapons"] = l.weapons.iter().map(|w| w.1.clone()).collect();
    } else {
        if !l.suit_mods.is_empty() {
            v["suit_mods"] = json!(l.suit_mods);
        }
        v["weapons"] = l
            .weapons
            .iter()
            .map(|(slot, weapon, class, mods)| {
                let mut w = json!({ "slot": slot, "weapon": weapon, "grade": class });
                if detail == 0 && !mods.is_empty() {
                    w["mods"] = json!(mods);
                }
                w
            })
            .collect();
    }
    strip_nulls(&mut v);
    v
}

/// `s` cut to `max` characters, with an ellipsis when something was cut.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

impl NexusTools {
    pub(super) fn materials(&self, a: &Value) -> Result<Value, String> {
        let category = match arg_str(a, "category") {
            None => None,
            Some(c) => Some(match c.to_ascii_lowercase().as_str() {
                "raw" => MaterialCategory::Raw,
                "manufactured" => MaterialCategory::Manufactured,
                "encoded" => MaterialCategory::Encoded,
                _ => return Err(format!("category must be raw, manufactured or encoded, not \"{c}\".")),
            }),
        };
        let query = arg_str(a, "name").map(|n| n.to_lowercase());
        let (lists, as_of) = {
            let model = self.model.read();
            let gs: &GameState = model.active().ok_or("No commander loaded yet.")?;
            let mats = &gs.inventory.materials;
            let lists: Vec<(&'static str, Vec<Held>)> = [("raw", MaterialCategory::Raw), ("manufactured", MaterialCategory::Manufactured), ("encoded", MaterialCategory::Encoded)]
                .into_iter()
                .map(|(key, cat)| {
                    let held = if category.is_none_or(|c| c == cat) {
                        mats.map(cat)
                            .iter()
                            .filter(|(_, n)| **n > 0)
                            .map(|(sym, n)| Held { name: mats.names.get(sym).cloned().unwrap_or_else(|| materials::display_name(sym)), count: *n, info: materials::material(sym) })
                            .filter(|h| query.as_ref().is_none_or(|q| h.name.to_lowercase().contains(q) || h.info.is_some_and(|m| m.symbol.contains(q.as_str()))))
                            .collect()
                    } else {
                        Vec::new()
                    };
                    (key, held)
                })
                .collect();
            (lists, (!mats.updated.is_zero()).then(|| mats.updated.to_iso()))
        };
        // The reply has to fit what the model is given of a tool result: the storage cap goes first, then the objects
        // become rows, and in the end only how many there are.
        let mut notes: Vec<&str> = Vec::new();
        for form in 0..=3u8 {
            let mut out = json!({ "as_of": as_of });
            strip_nulls(&mut out);
            for (key, held) in &lists {
                out[*key] = if form >= 3 { json!({ "distinct": held.len(), "total": held.iter().map(|h| h.count).sum::<i64>() }) } else { held.iter().map(|h| material_row(h, form)).collect() };
            }
            match form {
                1 => notes.push("Too many to give each one's storage cap: it is 300, 250, 200, 150 or 100 for grade 1 to 5, and a material is full at its cap."),
                2 => notes.push("Written as [name, count, grade] rows to fit."),
                3 => notes.push("Too many to list: only how many of each category. Ask with category= or name= to see them."),
                _ => {}
            }
            if lists.iter().all(|(_, h)| h.is_empty()) && as_of.is_none() {
                out["notes"] = json!(["No materials reported yet: the game lists them when it starts."]);
                return Ok(out);
            }
            if !notes.is_empty() {
                out["notes"] = json!(notes);
            }
            if form == 3 || out.to_string().len() <= BUDGET {
                return Ok(out);
            }
        }
        Err("Too many materials to list.".into())
    }

    pub(super) fn on_foot(&self) -> Result<Value, String> {
        let model = self.model.read();
        let gs: &GameState = model.active().ok_or("No commander loaded yet.")?;
        let inv = &gs.inventory;
        let mut loadouts: Vec<&SuitLoadoutInfo> = inv.suit_loadouts.values().collect();
        loadouts.sort_by_key(|l| (inv.current_suit_loadout != Some(l.loadout_id), l.loadout_id));
        // What gives way when there is too much: the ship locker's lists, then the backpack's, then the weapon
        // modifications, then everything about a loadout but its suit and weapons.
        let mut notes: Vec<&str> = Vec::new();
        for level in 0..=4u8 {
            let mut out = json!({
                "loadouts": loadouts.iter().map(|l| loadout(l, inv.current_suit_loadout == Some(l.loadout_id), level.saturating_sub(2))).collect::<Vec<_>>(),
                "locker": locker(&inv.ship_locker, level >= 1),
                "backpack": locker(&inv.backpack, level >= 2),
            });
            match level {
                1 => notes.push("Ship locker too big to list: how many of each kind (items, components, consumables, data) instead."),
                2 => notes.push("Backpack too big to list: how many of each kind instead."),
                3 => notes.push("Weapon modifications left out to fit."),
                4 => notes.push("Suit modifications and weapon details left out to fit."),
                _ => {}
            }
            if !notes.is_empty() {
                out["notes"] = json!(notes);
            }
            if level == 4 || out.to_string().len() <= BUDGET {
                return Ok(out);
            }
        }
        Err("Too much to list.".into())
    }

    pub(super) fn credit_history(&self, a: &Value) -> Result<Value, String> {
        let limit = arg_f64(a, "limit").map_or(25, |l| l.clamp(1.0, 100.0) as usize);
        let now = Timestamp::now();
        let model = self.model.read();
        let gs: &GameState = model.active().ok_or("No commander loaded yet.")?;
        let since = match arg_f64(a, "since_hours").filter(|h| *h > 0.0) {
            Some(h) => Timestamp(now.0 - (h.min(1.0e6) * 3600.0) as i64),
            None => gs.cmdr.last_login,
        };
        let mut entries: Vec<&nexus_model::history::LedgerEntry> = gs.history.ledger.iter().rev().filter(|e| e.ts >= since).collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.ts));
        let total = entries.len();
        let totals: Vec<Value> = gs.history.totals_since(since).into_iter().map(|(c, income, expense)| json!({ "category": c.label(), "income": income, "expense": expense })).collect();
        entries.truncate(limit);
        let rows: Vec<(Timestamp, &'static str, i64, String)> = entries.iter().map(|e| (e.ts, e.category.label(), e.amount, e.note.clone())).collect();
        let (balance, commander) = (gs.cmdr.credits, gs.cmdr.name.clone());
        drop(model);
        // The reply has to fit what the model is given of a tool result: long notes are cut, then dropped, then the
        // oldest transactions go.
        let mut notes: Vec<&str> = Vec::new();
        let mut note_max = Some(100usize);
        let mut shown = rows.len();
        loop {
            let tx: Vec<Value> = rows[..shown]
                .iter()
                .map(|(ts, category, amount, note)| {
                    let mut v = json!({ "time": ts.to_iso(), "category": category, "amount": amount });
                    if let Some(max) = note_max.filter(|m| *m > 0) {
                        v["note"] = json!(clip(note, max));
                    }
                    v
                })
                .collect();
            let mut out = json!({
                "commander": commander, "balance": balance, "since": (!since.is_zero()).then(|| since.to_iso()),
                "transactions_total": total, "transactions": tx, "totals": totals,
            });
            strip_nulls(&mut out);
            if !notes.is_empty() {
                out["notes"] = json!(notes);
            }
            if out.to_string().len() <= BUDGET || shown <= 1 {
                return Ok(out);
            }
            match note_max {
                Some(100) => {
                    note_max = Some(40);
                    notes.push("Transaction notes shortened to fit.");
                }
                Some(40) => {
                    note_max = Some(0);
                    notes.pop();
                    notes.push("Transaction notes left out to fit.");
                }
                _ => {
                    shown -= 1;
                    if !notes.iter().any(|n| n.starts_with("Oldest")) {
                        notes.push("Oldest transactions left out to fit: ask with a smaller limit or since_hours.");
                    }
                }
            }
        }
    }

    pub(super) fn fleet(&self, a: &Value) -> Result<Value, String> {
        let all_modules = arg_bool(a, "all_modules").unwrap_or(false);
        let query = a.get("ship").and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string()))).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let now = Timestamp::now();
        let model = self.model.read();
        let gs: &GameState = model.active().ok_or("No commander loaded yet.")?;
        // The ship being flown first, then the most valuable.
        let mut ships: Vec<&Ship> = match &query {
            Some(q) => find_ships(gs, q),
            None => gs.fleet.ships.values().collect(),
        };
        if let (true, Some(q)) = (ships.is_empty(), &query) {
            let mut all: Vec<&Ship> = gs.fleet.ships.values().collect();
            all.sort_by_key(|s| s.id);
            let list: Vec<String> = all.iter().take(30).map(|s| format!("{} ({}{})", text(&s.name).unwrap_or("unnamed"), s.type_name, text(&s.ident).map(|i| format!(", {i}")).unwrap_or_default())).collect();
            let more = all.len().saturating_sub(30);
            return Err(if list.is_empty() {
                format!("No ship matches \"{q}\": no ships are known yet.")
            } else {
                format!("No ship matches \"{q}\". The commander's ships: {}{}.", list.join("; "), if more > 0 { format!("; and {more} more") } else { String::new() })
            });
        }
        ships.sort_by_key(|s| (gs.fleet.current_ship_id != Some(s.id), std::cmp::Reverse(s.total_value())));
        // The reply has to fit what the model is given of a tool result. A big fleet gives up detail in this order:
        // the module lists, then which modules are engineered, then the ships of least value.
        let (mut with_modules, mut counts_only, mut omitted) = (all_modules, false, 0usize);
        let mut notes: Vec<&str> = Vec::new();
        loop {
            let listed: Vec<Value> = ships
                .iter()
                .map(|s| {
                    // With no ship asked for, only the ship being flown gets its whole module list.
                    let modules = with_modules && (query.is_some() || gs.fleet.current_ship_id == Some(s.id));
                    ship(gs, s, Detail { modules, counts_only }, now)
                })
                .collect();
            let mut out = json!({ "commander": gs.cmdr.name, "ships": listed, "total_value_cr": gs.fleet.total_value() });
            if omitted > 0 {
                out["ships_omitted"] = omitted.into();
            }
            if !notes.is_empty() {
                out["notes"] = json!(notes);
            }
            let fits = out.to_string().len() <= BUDGET;
            if fits || (ships.len() <= 1 && !with_modules && counts_only) {
                return Ok(out);
            }
            if with_modules {
                with_modules = false;
                notes.push("Too much to list every module: ask for one ship with ship= to see its modules.");
            } else if !counts_only {
                counts_only = true;
                notes.push("Too many engineered modules to list: engineered_count says how many each ship has; ask for one ship with ship= to see which.");
            } else {
                ships.pop();
                omitted += 1;
                if !notes.iter().any(|n| n.starts_with("Ships left out")) {
                    notes.push("Ships left out (the least valuable) to fit: ask for one with ship=.");
                }
            }
        }
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
