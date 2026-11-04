//! The assistant's Where to buy tool: the nearest stations that sell a build (one of the commander's ships, a build
//! from EDSY or Coriolis, or modules and hulls named one by one), as Fleet › Where to buy finds them through Spansh's
//! outfitting search (`crate::shopping`).

use super::{NexusTools, arg_bool, arg_f64, arg_str, fit, r1, strip_nulls};
use crate::shopping::{self, Group, Need, Offer, Outcome, Search};
use nexus_data::commodities::name_key;
use nexus_data::outfitting::{self, Module, ModuleCategory};
use nexus_data::{ships, sku};
use nexus_journal::Timestamp;
use nexus_spansh::{Coords, Reference};
use serde_json::{Value, json};
use std::time::Duration;

/// A search reads up to three pages of stations, then the nearest stations for a first stop, a second stop and up to
/// eight single lookups: longer than one call's timeout.
const BUY_TIMEOUT: Duration = Duration::from_secs(90);

const SOURCE: &str = "Stations, prices and data dates come from Spansh, which players' tools keep up to date through EDDN: a station's list is only as new as the last visit by someone who shares data. Fleet carriers move, and their owners change what they stock. Each station was checked for the exact module, a weapon's mount and a hull's own bulkheads included.";

/// One thing asked for by name.
#[derive(Debug, Clone, Copy)]
enum Item {
    Module(&'static Module),
    Hull(&'static ships::Ship),
}

/// "fsd" is how commanders write a frame shift drive.
fn spelled_out(asked: &str) -> String {
    asked.split_whitespace().map(|w| if w.eq_ignore_ascii_case("fsd") { "Frame Shift Drive" } else { w }).collect::<Vec<_>>().join(" ")
}

/// A module or hull asked for: its name as the game shows it ("6A Frame Shift Drive", "2A Gimballed Multi-cannon",
/// "Mandalay"), its journal symbol, or words only one module's name has.
fn find_item(asked: &str) -> Result<Item, String> {
    let asked = spelled_out(asked);
    let k = name_key(&asked);
    if let Some(s) = ships::all().iter().find(|s| name_key(&s.name) == k || name_key(&s.fdev_name) == k || s.symbol.eq_ignore_ascii_case(asked.trim())) {
        return Ok(Item::Hull(s));
    }
    if let Some(m) = outfitting::module(&asked) {
        return Ok(Item::Module(m));
    }
    let named = |exact: bool| {
        let words: Vec<String> = asked.split_whitespace().map(name_key).filter(|w| !w.is_empty()).collect();
        let mut hits: Vec<&'static Module> = Vec::new();
        for m in outfitting::all() {
            let name = name_key(&m.display_name());
            let hit = if exact { name == k } else { !words.is_empty() && words.iter().all(|w| name.contains(w.as_str())) };
            // MercGear rows share their module's symbol: the module is what's sold.
            if hit && !hits.iter().any(|h| h.symbol == m.symbol) {
                hits.push(outfitting::module(&m.symbol).unwrap_or(m));
            }
        }
        hits
    };
    let exact = named(true);
    let hits = if exact.is_empty() { named(false) } else { exact };
    match hits.as_slice() {
        [m] => Ok(Item::Module(m)),
        [] => Err(format!("No module or ship is called \"{asked}\": name it as the game's outfitting shows it, e.g. 6A Frame Shift Drive, 2A Gimballed Multi-cannon, Mandalay.")),
        many => {
            let names: Vec<String> = many.iter().take(12).map(|m| m.display_name()).collect();
            let more = if many.len() > 12 { format!(" and {} more", many.len() - 12) } else { String::new() };
            Err(format!("\"{asked}\" could be {}{more}: say which.", names.join(", ")))
        }
    }
}

/// An unlock a module or hull needs first: tech brokers and Powerplay. (ARX early access is read from Spansh's prices.)
fn unlock(sku: Option<&str>) -> Option<sku::Unlock> {
    sku.and_then(sku::unlock).filter(|u| *u != sku::Unlock::ArxEarlyAccess)
}

/// What buying `item` alone needs.
fn need(item: Item) -> Need {
    match item {
        Item::Hull(s) => Need { group: Group::Hull, symbol: s.symbol.clone(), name: s.name.clone(), count: 1, engineering: Vec::new(), buy: true, note: None, unlock: unlock(s.entitlement.as_deref()) },
        Item::Module(m) => Need {
            group: match m.category {
                ModuleCategory::Standard => Group::Core,
                ModuleCategory::Hardpoint => Group::Hardpoints,
                ModuleCategory::Utility => Group::Utility,
                _ => Group::Optional,
            },
            symbol: m.symbol.clone(),
            name: m.display_name(),
            count: 1,
            engineering: Vec::new(),
            buy: true,
            note: None,
            unlock: unlock(m.entitlement.as_deref()),
        },
    }
}

fn symbol(item: Item) -> &'static str {
    match item {
        Item::Hull(s) => &s.symbol,
        Item::Module(m) => &m.symbol,
    }
}

fn days_ago(unix: i64) -> i64 {
    ((Timestamp::now().0 - unix).max(0) as f64 / 86_400.0).floor() as i64
}

/// A station: where, how far, how old its data is and what the items bought there cost.
fn station(o: &Offer, items: &[usize], q: &Search, from_first: bool) -> Value {
    let (cost, unpriced) = o.cost(items, &q.wanted);
    let mut v = json!({
        "station": o.station, "system": o.system,
        "arrival_ls": o.arrival_ls.map(f64::round), "type": o.station_type,
        "fleet_carrier": o.carrier.then_some(true), "planetary": o.planetary.then_some(true), "state": o.state,
        "data_age_days": o.updated.map(days_ago), "cost_cr": (cost > 0).then_some(cost), "items_without_price": (unpriced > 0).then_some(unpriced),
    });
    v[if from_first { "distance_from_first_stop_ly" } else { "distance_ly" }] = json!(r1(o.distance_ly));
    if items.len() < q.wanted.len() {
        v["sells"] = items.iter().filter_map(|&i| q.wanted.get(i)).map(|w| json!(w.name)).collect();
    }
    strip_nulls(&mut v);
    v
}

/// "Spansh matched 412 stations by module name; the nearest 60 were checked one by one: 3 sold a different mount…".
fn checked(out: &Outcome, q: &Search) -> String {
    let s = &out.stats;
    let mut parts = Vec::new();
    if s.not_exact > 0 {
        parts.push(format!("{} sold a different mount or another hull's bulkheads", s.not_exact));
    }
    if s.closed > 0 {
        parts.push(format!("{} fleet carriers let only friends or their squadron dock", s.closed));
    }
    if s.no_pad > 0 {
        parts.push(format!("{} have no pad the ship fits ({})", s.no_pad, pad(q.pad)));
    }
    let matched = if s.matched >= 10_000 { "over 10,000".to_string() } else { s.matched.to_string() };
    let mut line = format!("Spansh matched {matched} stations by module name; the nearest {} were checked one by one", s.read);
    if !parts.is_empty() {
        line.push_str(&format!(": {}", parts.join(", ")));
    }
    line.push('.');
    line
}

fn pad(p: char) -> &'static str {
    match p {
        'L' => "large",
        'M' => "medium or large",
        _ => "any",
    }
}

/// The reply: what was searched for, and the stations that sell it all, or the stops that cover it between them.
fn shape_buy(what: &str, from: &str, needs: &[Need], q: &Search, out: &Outcome, budget: usize) -> Value {
    let n = q.wanted.len();
    let all: Vec<usize> = (0..n).collect();
    let to_buy: Vec<Value> = needs
        .iter()
        .filter(|x| x.buy)
        .map(|x| {
            let mut v = json!({ "item": x.name, "count": (x.count > 1).then_some(x.count), "unlock_first": x.unlock.map(|u| u.explain()), "note": x.note });
            if !x.engineering.is_empty() {
                v["engineering_on_the_build"] = json!(x.engineering);
            }
            strip_nulls(&mut v);
            v
        })
        .collect();
    let skipped: Vec<Value> = needs.iter().filter(|x| !x.buy).map(|x| json!({ "item": x.name, "why": x.note })).collect();
    let mut notes: Vec<String> = vec![checked(out, q), SOURCE.into()];
    if needs.iter().any(|x| x.buy && !x.engineering.is_empty()) {
        notes.push("Stations sell modules plain: the engineering on the build is done at engineers afterwards.".into());
    }
    // A hull with no credit price at any listed station may still be an ARX-only early-access ship.
    let hull = q.wanted.iter().position(|w| w.hull);
    let listed: Vec<&Offer> = out.one_stop.iter().chain(&out.first).chain(&out.second).chain(out.elsewhere.iter().filter_map(|(_, o)| o.as_ref())).collect();
    if let Some(h) = hull {
        let prices: Vec<Option<u64>> = listed.iter().filter_map(|o| o.sells.iter().find(|(i, _)| *i == h).map(|(_, p)| *p)).collect();
        if !prices.is_empty() && prices.iter().all(Option::is_none) {
            notes.push(format!("Spansh lists no credit price for the {} at these stations: it may still be sold for ARX only (early access), so the costs leave it out.", q.wanted[h].name));
        }
    }
    // What gives way when the reply is too long for the caller: what isn't searched for, then the stations furthest
    // down the list, one by one, down to the first.
    let stations = out.one_stop.len().max(1);
    let (reply, _) = fit(budget, 2 + stations - 1, |level| {
        let mut v = json!({ "searched_for": what, "near": from, "landing_pad": pad(q.pad), "to_buy": to_buy });
        if level == 0 && !skipped.is_empty() {
            v["not_searched"] = json!(skipped);
        } else if !skipped.is_empty() {
            v["not_searched_count"] = json!(skipped.len());
        }
        let mut notes = notes.clone();
        if !out.one_stop.is_empty() {
            let shown = out.one_stop.len() - level.saturating_sub(1).min(out.one_stop.len() - 1);
            v["sell_everything"] = out.one_stop[..shown].iter().map(|o| station(o, &all, q, false)).collect();
            if shown < out.one_stop.len() {
                notes.push(format!("The {} further stations left out to fit.", out.one_stop.len() - shown));
            }
        } else if let Some(first) = &out.first {
            let have: Vec<usize> = all.iter().copied().filter(|&i| first.sells(i)).collect();
            let rest: Vec<usize> = all.iter().copied().filter(|&i| !first.sells(i)).collect();
            v["no_station_sells_everything"] = json!(true);
            v["first_stop"] = station(first, &have, q, false);
            if let Some(second) = &out.second {
                v["second_stop"] = station(second, &rest, q, true);
            } else {
                let mut nowhere = Vec::new();
                let each: Vec<Value> = out
                    .elsewhere
                    .iter()
                    .filter_map(|(i, o)| match o {
                        Some(o) => Some(station(o, &[*i], q, false)),
                        None => {
                            nowhere.push(q.wanted[*i].name.clone());
                            None
                        }
                    })
                    .collect();
                v["nearest_seller_of_each_other_item"] = json!(each);
                if !nowhere.is_empty() {
                    v["sold_nowhere_near"] = json!(nowhere);
                }
                if out.not_looked_up > 0 {
                    notes.push(format!("{} more items weren't looked up one by one (at most {} are).", out.not_looked_up, shopping::SINGLES));
                }
            }
        } else {
            v["sell_everything"] = json!([]);
            notes.push(format!("Spansh knows no station near {from} that sells {}{}. Try without the data-age limit, with fleet carriers, or from another system.", if n == 1 { q.wanted[0].name.clone() } else { "any of these".into() }, match q.pad { 'L' => " with a large pad", 'M' => " with a medium or large pad", _ => "" }));
        }
        if level > 0 && !skipped.is_empty() {
            notes.push("What isn't searched for (in storage, or come with the hull) left out to fit.".into());
        }
        v["notes"] = json!(notes);
        v
    });
    reply
}

impl NexusTools {
    pub(super) fn where_to_buy(&self, a: &Value) -> Result<Value, String> {
        let items: Vec<Item> = match a.get("items") {
            Some(Value::Array(v)) => v.iter().filter_map(Value::as_str).filter(|s| !s.trim().is_empty()).map(find_item).collect::<Result<_, _>>()?,
            Some(Value::String(s)) if !s.trim().is_empty() => vec![find_item(s)?],
            _ => Vec::new(),
        };
        let carriers = arg_bool(a, "carriers").unwrap_or(true);
        let max_age = arg_f64(a, "max_age_days").filter(|d| *d >= 1.0).map(|d| d.round().min(3650.0) as u32);
        // The build, what it needs and where to search from, under one short read.
        let (what, hull, needs, near, from) = {
            let model = self.model.read();
            let gs = model.active().ok_or("No commander loaded yet.")?;
            let (what, hull, mut needs) = match (arg_str(a, "build"), arg_str(a, "ship")) {
                (Some(text), _) => {
                    let b = shopping::parse_build(&text)?;
                    let what = match &b.name {
                        Some(name) => format!("{} ({name})", ships::ship_name(&b.ship)),
                        None => ships::ship_name(&b.ship),
                    };
                    (what, b.ship.clone(), shopping::needs(&b, &gs.fleet))
                }
                (None, Some(q)) => {
                    let found = super::cmdr::find_ships(gs, &q);
                    let s = match found.as_slice() {
                        [s] => *s,
                        [] => return Err(format!("No ship of the commander's matches \"{q}\".")),
                        many => return Err(format!("\"{q}\" could be {}: say which by ident or id.", many.iter().map(|s| format!("{} (id {})", s.display_name(), s.id)).collect::<Vec<_>>().join(", "))),
                    };
                    let b = s.loadout_json.as_deref().map(shopping::parse_build).ok_or("No loadout recorded for that ship yet: the game reports it when the ship is flown.")??;
                    (s.display_name(), b.ship.clone(), shopping::needs(&b, &gs.fleet))
                }
                (None, None) if !items.is_empty() => {
                    let hull = items.iter().find_map(|i| if let Item::Hull(s) = i { Some(s.symbol.clone()) } else { None });
                    let flying = gs.fleet.current().map(|s| s.ship_type.to_ascii_lowercase()).unwrap_or_default();
                    let names: Vec<String> = items.iter().map(|i| need(*i).name).collect();
                    (names.join(", "), hull.unwrap_or(flying), items.iter().map(|i| need(*i)).collect())
                }
                (None, None) => {
                    let s = gs.fleet.current().ok_or("No ship is known yet: give ship, build or items.")?;
                    let b = s.loadout_json.as_deref().map(shopping::parse_build).ok_or("No loadout recorded for the current ship yet: the game reports it at the next login or outfitting.")??;
                    (s.display_name(), b.ship.clone(), shopping::needs(&b, &gs.fleet))
                }
            };
            // Items named with a ship or build: only those of it.
            if !items.is_empty() && (a.get("build").is_some() || a.get("ship").is_some()) {
                let missing: Vec<String> = items.iter().filter(|i| !needs.iter().any(|n| n.symbol == symbol(**i))).map(|i| need(*i).name).collect();
                if !missing.is_empty() {
                    return Err(format!("Not in that build: {}.", missing.join(", ")));
                }
                for n in &mut needs {
                    if !items.iter().any(|i| symbol(*i) == n.symbol) {
                        n.buy = false;
                        n.note.get_or_insert_with(|| "Not asked for".into());
                    }
                }
            }
            let typed = arg_str(a, "near_system").filter(|n| !n.eq_ignore_ascii_case(&gs.location.system));
            let (near, from) = match (typed, gs.location.star_pos) {
                (Some(t), _) => (Reference::System(t.clone()), t),
                (None, Some([x, y, z])) => (Reference::Coords(Coords::new(x, y, z)), gs.location.system.clone()),
                (None, None) if !gs.location.system.is_empty() => (Reference::System(gs.location.system.clone()), gs.location.system.clone()),
                (None, None) => return Err("The commander's position isn't known yet: give near_system.".into()),
            };
            (what, hull, needs, near, from)
        };
        let q = Search::new(&needs, &hull, near, carriers, max_age);
        if q.wanted.is_empty() {
            let mut v = json!({
                "searched_for": what, "to_buy": [],
                "not_searched": needs.iter().map(|x| json!({ "item": x.name, "why": x.note })).collect::<Vec<_>>(),
                "notes": ["Nothing to buy: the commander has all of it (in storage or as their own ship), or it comes with the hull."],
            });
            strip_nulls(&mut v);
            return Ok(v);
        }
        let out = self.spansh_search(BUY_TIMEOUT, shopping::find(&self.spansh, &q))?;
        Ok(shape_buy(&what, &from, &needs, &q, &out, self.budget))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shopping::Stats;

    #[test]
    fn items_are_found_by_the_names_the_game_shows() {
        let module = |s: &str| match find_item(s) {
            Ok(Item::Module(m)) => m.symbol.clone(),
            other => panic!("{s}: {other:?}"),
        };
        assert_eq!(module("6A Frame Shift Drive"), "int_hyperdrive_size6_class5");
        assert_eq!(module("6a fsd"), "int_hyperdrive_size6_class5", "FSD spelled out, any case");
        assert_eq!(module("int_hyperdrive_size6_class5"), "int_hyperdrive_size6_class5");
        assert!(matches!(find_item("anaconda"), Ok(Item::Hull(s)) if s.symbol == "anaconda"));
        assert!(find_item("frame shift drive").unwrap_err().contains("could be"), "every size and rating has that name");
        assert!(find_item("toaster").unwrap_err().contains("6A Frame Shift Drive"), "the error says how to name one");
    }

    fn offer(station: &str, ly: f64, sells: &[(usize, Option<u64>)]) -> Offer {
        Offer {
            station: station.into(),
            system: format!("{station} system"),
            market_id: 1,
            station_type: Some("Coriolis Starport".into()),
            carrier: false,
            planetary: false,
            distance_ly: ly,
            arrival_ls: Some(512.4),
            pos: None,
            state: None,
            updated: Some(Timestamp::now().0 - 3 * 86_400),
            sells: sells.to_vec(),
        }
    }

    fn two_items() -> (Vec<Need>, Search) {
        let needs = vec![need(find_item("6A Frame Shift Drive").unwrap()), need(find_item("5A Thrusters").unwrap())];
        let q = Search::new(&needs, "anaconda", Reference::System("Alpha".into()), true, None);
        (needs, q)
    }

    #[test]
    fn the_reply_lists_stations_that_sell_everything_with_their_cost() {
        let (needs, q) = two_items();
        let out = Outcome {
            one_stop: vec![offer("Near Port", 4.2, &[(0, Some(16_000_000)), (1, Some(5_000_000))]), offer("Far Port", 30.0, &[(0, Some(1)), (1, None)])],
            stats: Stats { matched: 412, read: 20, not_exact: 3, ..Stats::default() },
            ..Outcome::default()
        };
        let v = shape_buy("6A Frame Shift Drive, 5A Thrusters", "Alpha", &needs, &q, &out, 11_500);
        let first = &v["sell_everything"][0];
        assert_eq!((&first["station"], &first["distance_ly"], &first["cost_cr"], &first["data_age_days"]), (&json!("Near Port"), &json!(4.2), &json!(21_000_000), &json!(3)), "{v}");
        assert_eq!(v["sell_everything"][1]["items_without_price"], 1);
        assert_eq!(v["landing_pad"], "large");
        assert!(v["notes"][0].as_str().unwrap().starts_with("Spansh matched 412 stations") && v["notes"][0].as_str().unwrap().contains("3 sold a different mount"), "{v}");
        // A short budget keeps the nearest station and says what it left out.
        let short = shape_buy("…", "Alpha", &needs, &q, &out, 900);
        assert_eq!(short["sell_everything"].as_array().unwrap().len(), 1, "{short}");
        assert!(short["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("left out to fit")), "{short}");
    }

    #[test]
    fn without_a_station_for_everything_the_stops_cover_it_between_them() {
        let (needs, q) = two_items();
        let out = Outcome {
            first: Some(offer("First", 5.0, &[(0, Some(16_000_000))])),
            elsewhere: vec![(1, None)],
            stats: Stats { matched: 2, read: 2, ..Stats::default() },
            ..Outcome::default()
        };
        let v = shape_buy("…", "Alpha", &needs, &q, &out, 11_500);
        assert_eq!((&v["no_station_sells_everything"], &v["first_stop"]["sells"]), (&json!(true), &json!(["6A Frame Shift Drive"])), "{v}");
        assert_eq!(v["sold_nowhere_near"], json!(["5A Thrusters"]));
    }
}
