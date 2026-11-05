//! The assistant's tools about the commander themselves: their fleet, their progress, their materials, on-foot gear,
//! credits and the factions of the systems they have visited. Each takes one short read of the model (or the journal
//! index) and returns compact JSON; nothing is read from disk under a lock.

use super::{NexusTools, arg_bool, arg_f64, arg_str, fit, r1, strip_nulls};
use nexus_data::materials::{self, Material};
use nexus_data::outfitting::{SlotKind, display_name, slot_display, slot_kind};
use nexus_data::ranks::RankKind;
use nexus_journal::events::SystemArrival;
use nexus_journal::{IndexEntry, Timestamp};
use nexus_model::GameState;
use nexus_model::fleet::{Ship, ShipLocation, ShipModule, ShipPlace};
use nexus_model::history::LedgerCategory;
use nexus_model::inventory::{Locker, MaterialCategory, SuitLoadoutInfo};
use nexus_model::location::{FactionSummary, SystemInfo};
use serde_json::{Value, json};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

fn pct(x: f64) -> i64 {
    (x * 100.0).round() as i64
}

/// `Some(text)` unless it's empty.
fn text(s: &str) -> Option<&str> {
    Some(s.trim()).filter(|s| !s.is_empty())
}

/// A blueprint by name ("Increased FSD Range"), not by the journal's symbol (`FSD_LongRange`), as the Engineering page
/// names it.
fn blueprint_name(item: &str, symbol: &str) -> String {
    match nexus_data::engineering::blueprint_for(item, symbol) {
        Some(b) => b.name.clone(),
        None => nexus_data::names::split_camel(&symbol.replace('_', " ")),
    }
}

fn engineered(m: &ShipModule) -> Option<Value> {
    let e = m.engineering.as_ref()?;
    let mut v = json!({
        "slot": slot_display(&m.slot), "module": display_name(&m.item),
        "blueprint": blueprint_name(&m.item, &e.blueprint), "grade": e.level, "quality_pct": pct(e.quality),
        "engineer": e.engineer, "experimental": e.experimental,
    });
    strip_nulls(&mut v);
    Some(v)
}

/// The modules that make the ship what it is: paint, decals, the cockpit, the cargo hatch and the built-in scanners
/// aren't listed.
fn fitted(s: &Ship) -> impl Iterator<Item = &ShipModule> {
    s.modules.iter().filter(|m| !matches!(slot_kind(&m.slot), SlotKind::Cosmetic | SlotKind::Other))
}

fn module(m: &ShipModule) -> Value {
    let mut v = json!({
        "slot": slot_display(&m.slot), "module": display_name(&m.item), "on": m.on, "health_pct": pct(m.health),
        "value_cr": m.value, "engineered": m.engineering.is_some(),
    });
    strip_nulls(&mut v);
    v
}

/// Where a ship is, as far as the journal tells: the ship being flown is where the commander is, unless they stepped
/// out of it and went on by taxi or in another commander's ship.
fn whereabouts(gs: &GameState, s: &Ship, now: Timestamp) -> Value {
    let place = |l: &ShipLocation| json!({ "system": l.system, "station": l.station, "body": l.body });
    let mut v = match gs.ship_place(s) {
        ShipPlace::WithYou => {
            let loc = &gs.location;
            json!({ "with_commander": true, "system": text(&loc.system), "station": loc.station.as_ref().filter(|_| loc.docked).map(|st| &st.name), "situation": loc.situation() })
        }
        ShipPlace::Left(l) => {
            let mut v = place(&l);
            v["left_there"] = json!("The commander stepped out of it there and is elsewhere now.");
            v
        }
        ShipPlace::Away => json!({ "unknown": "The commander is away from it (an Apex taxi, a dropship or another commander's ship) and their journal doesn't say where they left it." }),
        ShipPlace::Stored(l) => {
            let mut v = place(&l);
            if l.in_transit_at(now) {
                v["in_transit"] = json!(true);
                v["arrives"] = json!(l.arrives.map(Timestamp::to_iso));
                v["arrives_in_min"] = json!(l.arrives.map(|a| ((a.0 - now.0).max(0) as f64 / 60.0).round() as i64));
            }
            v
        }
        ShipPlace::Unknown => json!({ "unknown": "Not in the journal." }),
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
    // A ship known only from StoredShips has no loadout: its jump range, cargo, hull and modules would be zeros, a full
    // hull and an empty list that nobody reported, which read as "no engineering". They are left out rather than shown.
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
    if !loadout {
        v["loadout_known"] = json!(false);
    } else if detail.counts_only {
        v["engineered_count"] = s.engineered_modules().into();
    } else {
        v["engineered"] = s.modules.iter().filter_map(engineered).collect();
    }
    if detail.modules && loadout {
        v["modules"] = fitted(s).map(module).collect();
    }
    strip_nulls(&mut v);
    v
}

/// Said when a listed ship's loadout was never seen.
const LOADOUT_UNKNOWN: &str = "loadout_known false: the game hasn't reported that ship's modules in the journal Elite Nexus read (it does when the ship is flown), so its engineering, modules, jump range and cargo aren't known.";

/// Ships matching what the commander asked for: an id, or a name, ident or type (as written, ignoring case); when
/// none matches exactly, a name, ident or type that contains it.
pub(super) fn find_ships<'a>(gs: &'a GameState, query: &str) -> Vec<&'a Ship> {
    let q = query.trim().to_lowercase();
    let is = |s: &Ship, f: fn(&str, &str) -> bool| f(&s.name.to_lowercase(), &q) || f(&s.ident.to_lowercase(), &q) || f(&s.type_name.to_lowercase(), &q);
    let exact: Vec<&Ship> = gs.fleet.ships.values().filter(|s| s.id.to_string() == q || is(s, |a, b| a == b)).collect();
    if !exact.is_empty() {
        return exact;
    }
    gs.fleet.ships.values().filter(|s| is(s, |a, b| !a.is_empty() && a.contains(b))).collect()
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

/// One material the commander holds.
struct Held {
    /// As the game names it, in the game's language.
    name: String,
    count: i64,
    /// What the game's catalogue says about it (None for a material it doesn't list).
    info: Option<&'static Material>,
}

impl Held {
    /// Its English name, when the game's is another.
    fn english(&self) -> Option<&str> {
        self.info.map(|m| m.name.as_str()).filter(|en| !en.eq_ignore_ascii_case(&self.name))
    }

    /// Whether a name asked for (already a [`name_key`]) is part of this material's name: as the game writes it, in
    /// English, or its journal symbol. A commander on a German client asking for "chemical distillery" or "wake
    /// echoes" finds them too.
    fn named(&self, key: &str) -> bool {
        name_key(&self.name).contains(key) || self.info.is_some_and(|m| name_key(&m.name).contains(key) || m.symbol.contains(key))
    }
}

/// Lower-case letters and digits only: "Wake Echoes", "wake echoes" and "wakeechoes" are the same.
fn name_key(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// How a material is written, from the most to the least detailed.
fn material_row(h: &Held, form: u8) -> Value {
    let grade = h.info.map(|m| m.grade);
    let cap = h.info.map(|m| i64::from(m.cap()));
    let full = cap.map(|c| h.count >= c);
    match form {
        0 => {
            let mut v = json!({ "name": h.name, "english": h.english(), "count": h.count, "grade": grade, "cap": cap, "full": full });
            strip_nulls(&mut v);
            v
        }
        1 => {
            let mut v = json!({ "name": h.name, "english": h.english(), "count": h.count, "grade": grade, "full": full.filter(|f| *f) });
            strip_nulls(&mut v);
            v
        }
        _ => json!([h.name, h.count, grade]),
    }
}

/// `{ "distinct": n, "total": n }` for a bucket of the locker.
fn locker_counts(m: &std::collections::BTreeMap<String, nexus_model::inventory::LockerItem>) -> Value {
    json!({ "distinct": m.values().filter(|i| i.count > 0).count(), "total": m.values().map(|i| i.count.max(0)).sum::<i64>() })
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

/// An on-foot modification by name ("Improved Battery Capacity"), not by the journal's symbol
/// (`suit_increasedbatterycapacity`).
fn mod_name(symbol: &str) -> String {
    match nexus_data::engineering::odyssey_mod(symbol) {
        Some(m) => m.name.clone(),
        None => nexus_data::names::split_camel(&symbol.trim_start_matches("suit_").trim_start_matches("weapon_").replace('_', " ")),
    }
}

/// A suit loadout: `detail` 0 = everything, 1 = no weapon modifications, 2 = only which suit and weapons.
fn loadout(l: &SuitLoadoutInfo, current: bool, detail: u8) -> Value {
    let mut v = json!({ "name": text(&l.name), "suit": l.suit_name, "current": current });
    if detail >= 2 {
        v["weapons"] = l.weapons.iter().map(|w| w.1.clone()).collect();
    } else {
        if !l.suit_mods.is_empty() {
            v["suit_mods"] = l.suit_mods.iter().map(|m| mod_name(m)).collect();
        }
        v["weapons"] = l
            .weapons
            .iter()
            .map(|(slot, weapon, class, mods)| {
                let mut w = json!({ "slot": slot, "weapon": weapon, "grade": class });
                if detail == 0 && !mods.is_empty() {
                    w["mods"] = mods.iter().map(|m| mod_name(m)).collect();
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

/// A list for the reply, or nothing when it is empty.
fn list(v: &[String]) -> Option<&[String]> {
    (!v.is_empty()).then_some(v)
}

/// One faction of a system, from the most (0) to the least (2) detailed.
fn faction(f: &FactionSummary, form: u8) -> Value {
    let mut v = json!({
        "name": f.name, "state": text(&f.state), "influence_pct": r1(f.influence * 100.0), "my_reputation": f.my_reputation.map(r1),
    });
    if form < 2 {
        v["active_states"] = json!(list(&f.active_states));
        v["pending_states"] = json!(list(&f.pending_states));
        v["recovering_states"] = json!(list(&f.recovering_states));
    }
    if form < 1 {
        v["government"] = json!(text(&f.government));
        v["allegiance"] = json!(text(&f.allegiance));
        v["happiness"] = json!(text(&f.happiness));
    }
    strip_nulls(&mut v);
    v
}

/// A system's factions and conflicts as one reply of the given form.
fn system_reply(system: &str, info: &SystemInfo, source: &str, form: u8) -> Value {
    let side = |s: &nexus_journal::events::ConflictSide| {
        let mut v = json!({ "faction": s.name, "stake": if form < 1 { text(&s.stake) } else { None }, "days_won": s.won_days });
        strip_nulls(&mut v);
        v
    };
    let mut v = json!({
        "system": system, "source": source, "as_of": (!info.updated.is_zero()).then(|| info.updated.to_iso()),
        "controlling_faction": info.controlling_faction, "controlling_faction_state": info.controlling_faction_state.as_deref().and_then(text),
        "government": text(&info.government), "allegiance": text(&info.allegiance), "economy": text(&info.economy), "security": text(&info.security),
        "population": info.population,
        "factions": info.factions.iter().map(|f| faction(f, form)).collect::<Vec<_>>(),
    });
    if !info.conflicts.is_empty() {
        v["conflicts"] = info.conflicts.iter().map(|c| json!({ "type": text(&c.war_type), "status": text(&c.status), "sides": [side(&c.faction1), side(&c.faction2)] })).collect();
    }
    strip_nulls(&mut v);
    v
}

/// The raw line of an index entry, read from disk. The last file opened stays open in `open`: a walk through the
/// journal newest first reads one file's lines together.
fn read_entry(files: &[PathBuf], e: &IndexEntry, open: &mut Option<(u32, std::fs::File)>) -> Option<String> {
    if open.as_ref().is_none_or(|(file, _)| *file != e.file) {
        *open = Some((e.file, std::fs::File::open(files.get(e.file as usize)?).ok()?));
    }
    let (_, f) = open.as_mut()?;
    f.seek(SeekFrom::Start(u64::from(e.offset))).ok()?;
    let mut buf = vec![0u8; e.len as usize];
    f.read_exact(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
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
        let query = arg_str(a, "name").map(|n| name_key(&n)).filter(|k| !k.is_empty());
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
                            .filter(|h| query.as_deref().is_none_or(|q| h.named(q)))
                            .collect()
                    } else {
                        Vec::new()
                    };
                    (key, held)
                })
                .collect();
            (lists, (!mats.updated.is_zero()).then(|| mats.updated.to_iso()))
        };
        if lists.iter().all(|(_, h)| h.is_empty()) && as_of.is_none() {
            return Ok(json!({ "raw": [], "manufactured": [], "encoded": [], "notes": ["No materials reported yet: the game lists them when it starts."] }));
        }
        // What gives way when the reply is too long for the caller: the storage cap, then the objects become rows, and
        // in the end only how many there are.
        const NOTES: [&str; 3] = [
            "Too many to give each one's storage cap: it is 300, 250, 200, 150 or 100 for grade 1 to 5, and a material is full at its cap.",
            "Written as [name, count, grade] rows to fit.",
            "Too many to list: only how many of each category. Ask with category= or name= to see them.",
        ];
        let (out, _) = fit(self.budget, NOTES.len() + 1, |form| {
            let mut out = json!({ "as_of": as_of });
            strip_nulls(&mut out);
            for (key, held) in &lists {
                out[*key] = if form >= 3 { json!({ "distinct": held.len(), "total": held.iter().map(|h| h.count).sum::<i64>() }) } else { held.iter().map(|h| material_row(h, form as u8)).collect() };
            }
            if form > 0 {
                out["notes"] = json!(NOTES[..form]);
            }
            out
        });
        Ok(out)
    }

    pub(super) fn on_foot(&self) -> Result<Value, String> {
        // Copied under one short read; the shrinking below happens without the lock.
        let (loadouts, current, ship_locker, backpack) = {
            let model = self.model.read();
            let gs: &GameState = model.active().ok_or("No commander loaded yet.")?;
            let inv = &gs.inventory;
            let mut loadouts: Vec<SuitLoadoutInfo> = inv.suit_loadouts.values().cloned().collect();
            loadouts.sort_by_key(|l| (inv.current_suit_loadout != Some(l.loadout_id), l.loadout_id));
            (loadouts, inv.current_suit_loadout, inv.ship_locker.clone(), inv.backpack.clone())
        };
        // What gives way when the reply is too long for the caller: the ship locker's lists, then the backpack's, then
        // the weapon modifications, then everything about a loadout but its suit and weapons.
        const NOTES: [&str; 4] = [
            "Ship locker too big to list: how many of each kind (items, components, consumables, data) instead.",
            "Backpack too big to list: how many of each kind instead.",
            "Weapon modifications left out to fit.",
            "Suit modifications and weapon details left out to fit.",
        ];
        let (out, _) = fit(self.budget, NOTES.len() + 1, |level| {
            let mut out = json!({
                "loadouts": loadouts.iter().map(|l| loadout(l, current == Some(l.loadout_id), level.saturating_sub(2) as u8)).collect::<Vec<_>>(),
                "locker": locker(&ship_locker, level >= 1),
                "backpack": locker(&backpack, level >= 2),
            });
            if level > 0 {
                out["notes"] = json!(NOTES[..level]);
            }
            out
        });
        Ok(out)
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
        let transfers = entries.iter().any(|e| e.category == LedgerCategory::Transfer);
        let totals: Vec<Value> = gs.history.totals_since(since).into_iter().map(|(c, income, expense)| json!({ "category": c.label(), "income": income, "expense": expense })).collect();
        entries.truncate(limit);
        let rows: Vec<(Timestamp, &'static str, i64, String)> = entries.iter().map(|e| (e.ts, e.category.label(), e.amount, e.note.clone())).collect();
        let (balance, commander) = (gs.cmdr.credits, gs.cmdr.name.clone());
        drop(model);
        // What gives way when the reply is too long for the caller: long notes are cut, then dropped, then the oldest
        // transactions go, one by one, down to the newest.
        let (out, _) = fit(self.budget, 3 + rows.len().saturating_sub(1), |level| {
            let note_max = [100, 40, 0][level.min(2)];
            let shown = rows.len() - level.saturating_sub(2).min(rows.len().saturating_sub(1));
            let tx: Vec<Value> = rows[..shown]
                .iter()
                .map(|(ts, category, amount, note)| {
                    let mut v = json!({ "time": ts.to_iso(), "category": category, "amount": amount });
                    if note_max > 0 {
                        v["note"] = json!(clip(note, note_max));
                    }
                    v
                })
                .collect();
            let mut notes: Vec<&str> = Vec::new();
            if transfers {
                notes.push("Carrier transfers move credits between you and your carrier: they are not income or spending.");
            }
            match level {
                0 => {}
                1 => notes.push("Transaction notes shortened to fit."),
                _ => notes.push("Transaction notes left out to fit."),
            }
            if shown < rows.len() {
                notes.push("Oldest transactions left out to fit: ask with a smaller limit or since_hours.");
            }
            let mut out = json!({
                "commander": commander, "balance": balance, "since": (!since.is_zero()).then(|| since.to_iso()),
                "transactions_total": total, "transactions": tx, "totals": totals,
            });
            strip_nulls(&mut out);
            if !notes.is_empty() {
                out["notes"] = json!(notes);
            }
            out
        });
        Ok(out)
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
        // Where to look for what had to be left out: never "ask for one ship" to a question that named one.
        let ask_one = match (&query, ships.len()) {
            (Some(_), 1) => None,
            (Some(_), _) => Some("ask for one of them by its ident or id"),
            (None, _) => Some("ask for one ship with ship="),
        };
        let then = |what: &str| ask_one.map(|a| format!("{what}{a}")).unwrap_or_default();
        let unknown_loadout = ships.iter().any(|s| s.loadout_ts.is_zero());
        // What gives way when the reply is too long for the caller: the module lists, then which modules are
        // engineered (a count each instead), then the ships of least value, one by one, down to the first.
        let steps = if all_modules { 3 } else { 2 };
        let (out, _) = fit(self.budget, steps + ships.len().saturating_sub(1), |level| {
            let step = level + 3 - steps;
            let (with_modules, counts_only) = (step == 0, step >= 2);
            let kept = ships.len() - (step.saturating_sub(2)).min(ships.len().saturating_sub(1));
            let listed: Vec<Value> = ships[..kept]
                .iter()
                .map(|s| {
                    // With no ship asked for, only the ship being flown gets its whole module list.
                    let modules = with_modules && (query.is_some() || gs.fleet.current_ship_id == Some(s.id));
                    ship(gs, s, Detail { modules, counts_only }, now)
                })
                .collect();
            let mut notes: Vec<String> = Vec::new();
            if unknown_loadout {
                notes.push(LOADOUT_UNKNOWN.into());
            }
            if all_modules && !with_modules {
                notes.push(format!("Too much to list every module{}.", then(": ")));
            }
            if counts_only {
                notes.push(format!("Too many engineered modules to list: engineered_count says how many each ship has{}.", then("; ")));
            }
            let mut out = json!({ "commander": gs.cmdr.name, "ships": listed, "total_value_cr": gs.fleet.total_value() });
            if kept < ships.len() {
                out["ships_omitted"] = (ships.len() - kept).into();
                notes.push(format!("Ships left out (the least valuable) to fit{}.", then(": ")));
            }
            if !notes.is_empty() {
                out["notes"] = json!(notes);
            }
            out
        });
        Ok(out)
    }

    pub(super) fn progress(&self) -> Result<Value, String> {
        let model = self.model.read();
        let gs: &GameState = model.active().ok_or("No commander loaded yet.")?;
        let c = &gs.cmdr;
        let mut engineers: Vec<_> = c.engineers.values().collect();
        engineers.sort_by(|a, b| (engineer_order(&a.progress), &a.name).cmp(&(engineer_order(&b.progress), &b.name)));
        let mut game = json!({
            "running": gs.game_running(), "at_main_menu": gs.at_main_menu,
            "mode": text(&c.game_mode), "version": text(&c.game_version),
            "last_login": (!c.last_login.is_zero()).then(|| c.last_login.to_iso()),
        });
        strip_nulls(&mut game);
        let mut out = json!({
            "commander": c.name,
            "ranks": RankKind::ALL.iter().map(|k| json!({
                "rank": format!("{k:?}"), "name": c.rank_name(*k), "rank_number": c.ranks.get(*k), "progress_pct": c.rank_progress(*k),
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
        });
        if !c.promoted.is_empty() {
            out["notes"] = json!(["progress_pct is null for a rank promoted since the game last reported progress: it does at login, so it isn't known until the next one."]);
        }
        Ok(out)
    }

    /// The index entries of arrivals (FSDJump, Location, CarrierJump) up to `last`, newest first: only those of that
    /// very second, or all of them. Copied under one short read of the index, with the journal files' paths.
    fn arrival_entries(&self, last: Timestamp, exact_only: bool) -> (Vec<IndexEntry>, Vec<PathBuf>) {
        let index = self.index.read();
        let ids: Vec<u16> = ["FSDJump", "Location", "CarrierJump"].iter().filter_map(|n| index.id_of(n)).collect();
        let upto = index.entries.partition_point(|e| e.ts <= last.0);
        let arrivals = index.entries[..upto].iter().rev().filter(|e| ids.contains(&e.name));
        let entries = if exact_only { arrivals.take_while(|e| e.ts == last.0).copied().collect() } else { arrivals.copied().collect() };
        (entries, index.files.clone())
    }

    /// The arrival in `system` the commander made at `last`, the time their own history says they were there last.
    /// The journal index covers every commander's journals, so the entries of that second are looked at first; only
    /// if none matches does it walk back from there, which can then land on another commander's arrival (the bool
    /// says so). The lines are read from disk without any lock.
    fn arrival_in(&self, system: &str, last: Timestamp) -> Option<(SystemArrival, Timestamp, bool)> {
        let needle = system.to_ascii_lowercase();
        let find = |(entries, files): (Vec<IndexEntry>, Vec<PathBuf>)| {
            let mut open = None;
            for e in entries {
                let Some(line) = read_entry(&files, &e, &mut open) else { continue };
                if !line.to_ascii_lowercase().contains(&needle) {
                    continue;
                }
                if let Ok(a) = serde_json::from_str::<SystemArrival>(&line)
                    && a.star_system.eq_ignore_ascii_case(system)
                {
                    return Some((a, Timestamp(e.ts)));
                }
            }
            None
        };
        if let Some((a, ts)) = find(self.arrival_entries(last, true)) {
            return Some((a, ts, false));
        }
        find(self.arrival_entries(last, false)).map(|(a, ts)| (a, ts, true))
    }

    pub(super) fn system_factions(&self, a: &Value) -> Result<Value, String> {
        let asked = arg_str(a, "system");
        // The system being flown comes from the model, and so does when the commander was last in any other: from
        // their own history, not the journal index, which also holds their alts' journals.
        let (here, last) = {
            let model = self.model.read();
            let gs: &GameState = model.active().ok_or("No commander loaded yet.")?;
            let loc = &gs.location;
            match &asked {
                Some(s) if !loc.system.eq_ignore_ascii_case(s) => (None, gs.history.visited.values().filter(|v| v.name.eq_ignore_ascii_case(s)).map(|v| v.last).max()),
                _ if text(&loc.system).is_none() => return Err("The commander's current system isn't known yet: name a system with system=.".into()),
                _ => (Some((loc.system.clone(), loc.info.clone())), None),
            }
        };
        let mut notes: Vec<&str> = vec!["my_reputation runs from -100 to 100; influence_pct is the faction's share of the system's influence."];
        let (system, info, source) = match here {
            Some((system, info)) => (system, info, "Commander's journal: what the game reported at the last arrival in this system. Influence and states may have moved since."),
            None => {
                let name = asked.unwrap_or_default();
                let unknown = || {
                    format!("\"{}\" isn't in the commander's journal, so there is nothing to say about its factions or the commander's reputation there. system_info has EDDN faction data for any system, without reputation.", clip(&name, 60))
                };
                let (arrival, ts, walked) = self.arrival_in(&name, last.ok_or_else(unknown)?).ok_or_else(unknown)?;
                if walked {
                    notes.push("The commander's own arrival wasn't found in the journal index: this is the newest earlier arrival there, which may be another commander's, so my_reputation may not be theirs.");
                }
                let info = SystemInfo::from_arrival(&arrival, ts);
                (arrival.star_system, info, "Commander's journal: what the game reported when the commander last arrived in this system. Influence and states may have moved since.")
            }
        };
        if info.factions.is_empty() {
            notes.push("The journal lists no factions for this system (uninhabited, or it was not reported).");
        }
        // What gives way when the reply is too long for the caller: government, allegiance and happiness, then the
        // pending, active and recovering states.
        const CUT: [&str; 2] = ["Government, allegiance and happiness left out to fit.", "Active, pending and recovering states left out to fit."];
        let (out, _) = fit(self.budget, CUT.len() + 1, |form| {
            let mut out = system_reply(&system, &info, source, form as u8);
            out["notes"] = json!(notes.iter().chain(&CUT[..form]).collect::<Vec<_>>());
            out
        });
        Ok(out)
    }
}
