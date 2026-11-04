//! The assistant's Spansh body tools: which landable bodies hold a raw material, and where to surface-mine with the
//! Rhino (Mining › Where to mine). Each splits "ask Spansh" (`NexusTools`) from "shape the records into JSON" (plain
//! functions), so the requests and the shaping are tested offline.

use super::{NexusTools, SPANSH_TIMEOUT, arg_f64, arg_str, fit, r1, strip_nulls};
use crate::surface_finder::{self as sf, Around, Candidate, Found, GroundYield, Query, RouteStop};
use nexus_data::commodities::name_key;
use nexus_data::grounds::{Class, Ground, Volcanism};
use nexus_data::materials::{self, Material, MaterialKind};
use nexus_journal::Timestamp;
use nexus_spansh::{BodyRecord, Coords, SearchRequest, SortDir, filter, sort};
use serde_json::{Value, json};
use std::future::Future;
use std::time::Duration;

/// Bodies listed.
const BODY_HITS: usize = 10;
/// Spansh stops counting the bodies that match at this many.
const COUNT_CAP: u64 = 10_000;

fn add_note(out: &mut Value, text: &str) {
    let note = out["note"].as_str().map(|n| format!("{n} {text}")).unwrap_or_else(|| text.to_string());
    out["note"] = json!(note);
}

/// How the bodies are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    Richest,
    Nearest,
}

/// What `find_material_body` asks Spansh, and how many bodies matched.
struct BodySearch<'a> {
    near: &'a str,
    material: &'a Material,
    radius_ly: f64,
    max_gravity_g: f64,
    max_ls: Option<f64>,
    order: Order,
    /// Bodies within the radius that hold it (Spansh stops counting at [`COUNT_CAP`]).
    matched: u64,
}

/// Spansh's body search for the material: landable bodies within the radius that hold it, light enough and near enough
/// their star, richest or nearest first. Spansh filters on the material's share itself, so one request answers.
fn request(origin: [f64; 3], s: &BodySearch) -> SearchRequest {
    let mut req = SearchRequest::new()
        .near(Coords { x: origin[0], y: origin[1], z: origin[2] })
        .within(s.radius_ly)
        .filter("is_landable", filter::eq(true))
        .filter("gravity", filter::between(0.0, s.max_gravity_g))
        .filter("materials", json!([{ "name": s.material.name, "share": [0.0001, 100], "comparison": "<=>" }]))
        .size(BODY_HITS as u32);
    if let Some(m) = s.max_ls {
        req = req.filter("distance_to_arrival", filter::between(0.0, m));
    }
    match s.order {
        Order::Richest => req.sort(sort::named("materials", &s.material.name, SortDir::Desc)),
        Order::Nearest => req.sort_by("distance", SortDir::Asc),
    }
}

/// The reply of `find_material_body`: the bodies Spansh listed, in its order, and how many hold the material.
fn shape_bodies(s: &BodySearch, records: &[BodyRecord]) -> Value {
    let rows: Vec<Value> = records
        .iter()
        .filter_map(|b| {
            let share = b.materials.iter().find(|m| m.name.eq_ignore_ascii_case(&s.material.name)).map(|m| m.share)?;
            let geo = b.signal("Geological");
            let mut v = json!({
                "body": b.name, "system": b.system_name, "share_pct": (share * 100.0).round() / 100.0,
                "distance_ly": b.distance.map(r1), "distance_ls": b.distance_to_arrival.map(f64::round),
                "gravity_g": b.gravity.map(|g| (g * 100.0).round() / 100.0), "geological_signals": (geo > 0).then_some(geo),
            });
            strip_nulls(&mut v);
            Some(v)
        })
        .collect();
    let holding = if s.matched >= COUNT_CAP { json!(format!("{COUNT_CAP} or more")) } else { json!(s.matched) };
    let order = match s.order {
        Order::Richest => "richest first",
        Order::Nearest => "nearest first",
    };
    let mut out = json!({
        "near": s.near, "material": s.material.name, "rarity_grade": s.material.grade,
        "within_ly": s.radius_ly, "max_gravity_g": s.max_gravity_g, "max_distance_ls": s.max_ls,
        "bodies_holding_it": holding, "order": order, "bodies": rows,
        "note": "share_pct is the material's share of the body's surface materials, from the scans commanders shared through EDDN that Spansh holds: a body nobody scanned isn't listed.",
    });
    strip_nulls(&mut out);
    if s.matched == 0 {
        add_note(&mut out, &format!("No landable body within {} ly is known to hold it: try a larger max_distance_ly or max_gravity_g.", s.radius_ly));
    } else if s.matched > rows.len() as u64 {
        add_note(&mut out, &format!("The {} {order} of them are listed.", rows.len()));
    }
    out
}

/// Bodies listed near the commander at most (fewer when they don't fit the caller's reply).
const MINE_HITS: usize = 15;
/// Bodies named for each stop along a route.
const STOP_BODIES: usize = 3;
/// Stops along a route unless asked otherwise, as on the Mining page.
const STOPS: usize = 10;
/// A route search reads up to four pages: it gets longer than one call's timeout.
const ROUTE_TIMEOUT: Duration = Duration::from_secs(60);

const NO_PROMISE: &str = "No journal or feed says what a mining location holds: the game records what the Rhino's refinery made, not what was in the deposit. A ground (class and volcanism) and the commander's own runs on it are leads, not promises.";
const COUNTS: &str = "Mining-location counts come from the FSS and DSS scans commanders shared through EDDN, as Spansh holds them. Only scans since the Rhino update (September 2026) carry them, so a body without a count may still have locations.";

/// The class asked for: "rocky", "Rocky ice", "metal-rich", "high metal"…; "any" or nothing for any class.
fn class_arg(a: &Value) -> Result<Option<Class>, String> {
    let Some(asked) = arg_str(a, "class").filter(|c| !matches!(c.to_ascii_lowercase().as_str(), "any" | "all")) else { return Ok(None) };
    let k = name_key(&asked);
    Class::of(&asked)
        .or_else(|| Class::ALL.into_iter().find(|c| !k.is_empty() && name_key(c.label()).starts_with(&k)))
        .map(Some)
        .ok_or_else(|| format!("class must be one of {}, not \"{asked}\".", Class::ALL.map(|c| c.label().to_lowercase()).join(", ")))
}

/// The volcanism asked for: "rocky magma", "water geysers"…, "none" for bodies without; "any" or nothing for any.
fn volcanism_arg(a: &Value) -> Result<Option<Volcanism>, String> {
    let Some(asked) = arg_str(a, "volcanism") else { return Ok(None) };
    match asked.to_ascii_lowercase().as_str() {
        "any" | "all" => return Ok(None),
        "none" | "no" | "inactive" | "no volcanism" => return Ok(Some(Volcanism::Inactive)),
        _ => {}
    }
    let v = Volcanism::of(&asked);
    if v == Volcanism::Other || sf::volcanism_filter(v).is_none() {
        return Err(format!("volcanism must be none or one of {}, not \"{asked}\".", sf::searchable_volcanism().filter(|v| *v != Volcanism::Inactive).map(Volcanism::label).collect::<Vec<_>>().join(", ")));
    }
    Ok(Some(v))
}

/// "rocky bodies with rocky magma and 5 or more mining locations", as the Mining page says it.
fn what(q: &Query) -> String {
    let noun = q.class.map_or_else(|| "bodies".to_string(), |c| c.noun(true));
    let volcanism = match q.volcanism {
        None => String::new(),
        Some(Volcanism::Inactive) => " without volcanism".into(),
        Some(v) => format!(" with {}", v.label()),
    };
    let locations = if q.min_locations <= 1 { "mining locations".to_string() } else { format!("{} or more mining locations", q.min_locations) };
    format!("landable {noun}{volcanism} that have {locations}")
}

fn ground(g: Option<Ground>) -> Value {
    g.map_or(Value::Null, |g| json!(g.label()))
}

fn date(unix: i64) -> String {
    Timestamp(unix).to_iso().chars().take(10).collect()
}

/// A body near the commander. `worked`: the mining locations their own Rhino worked on it.
fn mine_body(c: &Candidate, worked: usize) -> Value {
    let mut v = json!({
        "body": c.body, "system": c.system, "ground": ground(c.ground), "mining_locations": c.locations,
        "distance_ly": r1(c.distance_ly), "distance_ls": c.distance_ls.map(f64::round), "gravity_g": c.gravity_g.map(|g| (g * 100.0).round() / 100.0),
        "counted_on": c.reported.map(date), "you_worked": (worked > 0).then_some(worked),
    });
    strip_nulls(&mut v);
    v
}

/// What the commander's own Rhino runs gave on each ground, the commodities that gave most first.
fn yields(own: &[GroundYield], commodities: usize) -> Vec<Value> {
    own.iter()
        .map(|y| {
            json!({
                "ground": y.ground.map_or_else(|| "unknown: the body was never scanned".to_string(), Ground::label),
                "locations_worked": y.locations, "bodies": y.bodies,
                "refined_t": y.by_tonnes().into_iter().take(commodities).map(|(c, t, n)| json!({ "commodity": c, "tonnes": t, "locations": n })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn matched(n: u64) -> Value {
    if n >= COUNT_CAP { json!(format!("{COUNT_CAP} or more")) } else { json!(n) }
}

/// The reply of `where_to_mine` near a system. `list` is nearest first, or the most locations first when `most`;
/// `worked(c)` the commander's own locations on a body.
#[allow(clippy::too_many_arguments)]
fn shape_near(near: &str, q: &Query, found: &Found, list: &[&Candidate], most: bool, worked: &dyn Fn(&Candidate) -> usize, own: &[GroundYield], budget: usize) -> Value {
    let shown = list.len().min(MINE_HITS);
    // What gives way when the reply is too long for the caller: the commodities of the commander's grounds, then the
    // bodies furthest down the list, one by one, down to the first.
    let (out, _) = fit(budget, 2 + shown.saturating_sub(1), |level| {
        let n = shown - level.saturating_sub(1).min(shown.saturating_sub(1));
        let mut out = json!({
            "near": near, "within_ly": q.radius_ly, "looking_for": what(q), "max_distance_ls": (q.max_ls > 0).then_some(q.max_ls),
            "bodies_matching": matched(found.matched),
            "bodies": list[..n].iter().map(|c| mine_body(c, worked(c))).collect::<Vec<_>>(),
        });
        if !own.is_empty() {
            out["your_grounds"] = json!(yields(own, if level == 0 { 6 } else { 0 }));
        }
        let mut notes = vec![COUNTS.to_string(), NO_PROMISE.to_string()];
        if most {
            notes.push(format!("The {n} with the most mining locations among the {} nearest are listed.", found.list.len()));
        } else if (n as u64) < found.matched {
            notes.push(format!("The {n} nearest are listed."));
        }
        if found.matched == 0 {
            notes.push(format!("Nothing within {} ly: search further, or ask for fewer locations or any class or volcanism.", q.radius_ly));
        }
        strip_nulls(&mut out);
        out["notes"] = json!(notes);
        out
    });
    out
}

/// The reply of `where_to_mine` along a route: the best system in each stretch.
#[allow(clippy::too_many_arguments)]
fn shape_route(from: &str, to: &str, q: &Query, found: &Found, stops: &[RouteStop], asked: usize, own: &[GroundYield], budget: usize) -> Value {
    let length = found.ends.map_or(0.0, |(a, b)| nexus_model::location::distance(a, b));
    // What gives way when the reply is too long for the caller: all but the best body of each stop, the commodities of
    // the commander's grounds, then the stops furthest along, one by one, down to the first.
    let (out, _) = fit(budget, 3 + stops.len().saturating_sub(1), |level| {
        let n = stops.len() - level.saturating_sub(2).min(stops.len().saturating_sub(1));
        let per_stop = if level == 0 { STOP_BODIES } else { 1 };
        let rows: Vec<Value> = stops[..n]
            .iter()
            .map(|s| {
                json!({
                    "system": s.system, "along_ly": s.along_ly.round(), "off_line_ly": r1(s.off_line_ly),
                    "bodies": s.bodies.iter().take(per_stop).map(|c| {
                        let mut v = json!({ "body": c.short_body(), "ground": ground(c.ground), "mining_locations": c.locations, "distance_ls": c.distance_ls.map(f64::round) });
                        strip_nulls(&mut v);
                        v
                    }).collect::<Vec<_>>(),
                    "bodies_with_locations": s.bodies.len(),
                })
            })
            .collect();
        let mut out = json!({
            "from": from, "to": to, "length_ly": length.round(), "off_line_ly": q.radius_ly, "looking_for": what(q),
            "bodies_matching": matched(found.matched), "bodies_read": found.list.len(), "stops": rows,
        });
        if !own.is_empty() {
            out["your_grounds"] = json!(yields(own, if level <= 1 { 6 } else { 0 }));
        }
        let mut notes = vec![
            format!("The best system (most mining locations on one body) in each of {asked} stretches of {} ly, nearest the straight line first; Spansh reads at most 2,000 bodies.", (length / asked.max(1) as f64).round()),
            COUNTS.to_string(),
            NO_PROMISE.to_string(),
        ];
        if stops.len() < asked {
            notes.push(format!("{} stretches had none.", asked - stops.len()));
        }
        if n < stops.len() {
            notes.push(format!("The last {} stops left out to fit: ask for fewer stops.", stops.len() - n));
        }
        out["notes"] = json!(notes);
        out
    });
    out
}

impl NexusTools {
    /// One Spansh call, under the usual timeout.
    fn spansh_call<T>(&self, call: impl Future<Output = nexus_spansh::Result<T>>) -> Result<T, String> {
        match self.rt.block_on(async { tokio::time::timeout(SPANSH_TIMEOUT, call).await }) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(format!("Spansh: {e}")),
            Err(_) => Err("Spansh: no answer in time".to_string()),
        }
    }

    /// A search of several Spansh calls, under `limit` for all of them.
    pub(super) fn spansh_search<T>(&self, limit: Duration, call: impl Future<Output = Result<T, String>>) -> Result<T, String> {
        match self.rt.block_on(async { tokio::time::timeout(limit, call).await }) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) if e.starts_with("Spansh") => Err(e),
            Ok(Err(e)) => Err(format!("Spansh: {e}")),
            Err(_) => Err(format!("Spansh: no answer within {} s", limit.as_secs())),
        }
    }

    pub(super) fn where_to_mine(&self, a: &Value) -> Result<Value, String> {
        let class = class_arg(a)?;
        let volcanism = volcanism_arg(a)?;
        let min_locations = arg_f64(a, "min_locations").map_or(1, |n| n.round().clamp(1.0, 999.0) as u32);
        let max_ls = arg_f64(a, "max_distance_ls").map_or(0, |d| d.round().clamp(0.0, 1.0e9) as u32);
        let radius = |default: f64| arg_f64(a, "max_distance_ly").unwrap_or(default).clamp(1.0, f64::from(*crate::tours::limits::RADIUS_LY.end()));
        let me = self.me()?;
        let own_runs = || {
            let model = self.model.read();
            model.active().map(sf::own_grounds).unwrap_or_default()
        };
        if let Some(to) = arg_str(a, "to_system") {
            let from = arg_str(a, "near_system").unwrap_or_else(|| me.system.clone());
            if from.trim().is_empty() {
                return Err("The commander's system isn't known yet: give near_system as the start.".into());
            }
            let asked = arg_f64(a, "stops").map_or(STOPS, |n| n.round().clamp(1.0, f64::from(*crate::tours::limits::SYSTEMS.end())) as usize);
            let q = Query { around: Around::Route { from: from.clone(), to: to.clone() }, radius_ly: radius(25.0), class, volcanism, min_locations, max_ls };
            let found = self.spansh_search(ROUTE_TIMEOUT, sf::along(&self.spansh, &q, asked))?;
            let stops = sf::route_stops(&found, asked);
            return Ok(shape_route(&from, &to, &q, &found, &stops, asked, &own_runs(), self.budget));
        }
        let (origin, near) = self.origin_online(a, &me)?;
        let q = Query { around: Around::Here(origin), radius_ly: radius(100.0), class, volcanism, min_locations, max_ls };
        let found = self.spansh_search(SPANSH_TIMEOUT, sf::near(&self.spansh, &q))?;
        let mut list: Vec<&Candidate> = found.list.iter().collect();
        let most = arg_str(a, "sort").is_some_and(|s| s.eq_ignore_ascii_case("most_locations"));
        if most {
            list.sort_by(|x, y| y.locations.cmp(&x.locations).then(x.distance_ly.total_cmp(&y.distance_ly)));
        }
        let model = self.model.read();
        let gs = model.active();
        let own = gs.map(sf::own_grounds).unwrap_or_default();
        let worked = |c: &Candidate| match (gs, c.system_address, c.body_id) {
            (Some(gs), Some(s), Some(b)) => sf::worked(gs, s, b),
            _ => 0,
        };
        Ok(shape_near(&near, &q, &found, &list, most, &worked, &own, self.budget))
    }

    pub(super) fn find_material_body(&self, a: &Value) -> Result<Value, String> {
        let asked = arg_str(a, "material").ok_or("material is required")?;
        let material = materials::material(&asked).ok_or_else(|| format!("Unknown material \"{asked}\". Give a raw material, e.g. Polonium, Tellurium, Niobium or Arsenic."))?;
        if material.kind != MaterialKind::Raw {
            return Err(format!(
                "{} is a {} material, not a raw one: this tool finds raw materials, the ones lying on planetary surfaces (e.g. Polonium).",
                material.name,
                material.kind.label().to_lowercase()
            ));
        }
        let max_gravity_g = arg_f64(a, "max_gravity_g").unwrap_or(1.0);
        if max_gravity_g <= 0.0 {
            return Err("max_gravity_g must be above 0.".to_string());
        }
        let max_ls = arg_f64(a, "max_distance_ls").filter(|d| *d > 0.0);
        let radius_ly = arg_f64(a, "max_distance_ly").unwrap_or(100.0).clamp(1.0, f64::from(*crate::tours::limits::RADIUS_LY.end()));
        let order = if arg_str(a, "sort").is_some_and(|s| s.eq_ignore_ascii_case("nearest")) { Order::Nearest } else { Order::Richest };
        let me = self.me()?;
        let (origin, near) = self.origin_online(a, &me)?;
        let mut s = BodySearch { near: &near, material, radius_ly, max_gravity_g, max_ls, order, matched: 0 };
        let found = self.spansh_call(self.spansh.search_bodies(&request(origin, &s)))?;
        s.matched = found.count;
        Ok(shape_bodies(&s, &found.results))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_tools::json_len;

    fn body(name: &str, gravity: f64, ls: f64, shares: &[(&str, f64)]) -> BodyRecord {
        BodyRecord {
            name: name.into(),
            system_name: "Deciat".into(),
            gravity: Some(gravity),
            distance: Some(3.0),
            distance_to_arrival: Some(ls),
            materials: shares.iter().map(|(n, s)| nexus_spansh::records::NameShare { name: (*n).into(), share: *s }).collect(),
            ..BodyRecord::default()
        }
    }

    fn search(material: &'static Material, order: Order, matched: u64) -> BodySearch<'static> {
        BodySearch { near: "Deciat", material, radius_ly: 100.0, max_gravity_g: 1.0, max_ls: Some(50_000.0), order, matched }
    }

    /// Spansh filters on the material itself: one request, not pages of every landable body nearby.
    #[test]
    fn spansh_is_asked_for_the_material() {
        let polonium = materials::material("Polonium").unwrap();
        let j = request([1.0, 2.0, 3.0], &search(polonium, Order::Richest, 0)).to_json();
        let f = &j["filters"];
        assert_eq!(f["materials"], json!([{ "name": "Polonium", "share": [0.0001, 100], "comparison": "<=>" }]));
        assert_eq!(f["is_landable"]["value"], true);
        assert_eq!(f["gravity"]["value"], json!([0, 1]));
        assert_eq!(f["distance_to_arrival"]["value"], json!([0, 50_000]));
        assert_eq!(f["distance"]["max"], "100");
        assert_eq!(j["sort"], json!([{ "materials": [{ "name": "Polonium", "direction": "desc" }] }]));
        assert_eq!(j["size"], 10);
        let nearest = request([0.0; 3], &search(polonium, Order::Nearest, 0)).to_json();
        assert_eq!(nearest["sort"], json!([{ "distance": { "direction": "asc" } }]));
    }

    #[test]
    fn the_reply_lists_spanshs_bodies_and_counts_them() {
        let polonium = materials::material("Polonium").unwrap();
        let records = vec![body("Deciat 2", 0.3, 100.0, &[("Polonium", 1.12), ("Iron", 20.0)]), body("Deciat 1", 0.2, 90.0, &[("polonium", 0.9)])];
        let v = shape_bodies(&search(polonium, Order::Richest, 4_442), &records);
        assert_eq!((&v["bodies"][0]["body"], &v["bodies"][0]["share_pct"]), (&json!("Deciat 2"), &json!(1.12)));
        assert_eq!(v["bodies"][1]["share_pct"], 0.9, "the name is matched whatever its case");
        assert_eq!((&v["bodies_holding_it"], &v["order"], &v["within_ly"]), (&json!(4_442), &json!("richest first"), &json!(100.0)));
        assert!(v["note"].as_str().unwrap().contains("The 2 richest first of them are listed"), "{v}");
        let many = shape_bodies(&search(polonium, Order::Nearest, COUNT_CAP), &records);
        assert_eq!(many["bodies_holding_it"], "10000 or more");
        let none = shape_bodies(&search(polonium, Order::Richest, 0), &[]);
        assert!(none["note"].as_str().unwrap().contains("No landable body within 100 ly"), "{none}");
        assert!(none.to_string().len() < nexus_ai::agent::MAX_RESULT_CHARS);
    }

    #[test]
    fn grounds_are_read_as_commanders_write_them() {
        let class = |s: &str| class_arg(&json!({ "class": s }));
        assert_eq!((class("rocky ice"), class("Metal-rich"), class("high metal"), class("any")), (Ok(Some(Class::RockyIce)), Ok(Some(Class::MetalRich)), Ok(Some(Class::HighMetal)), Ok(None)));
        assert!(class("gas giant").unwrap_err().contains("rocky ice"), "the error lists the classes");
        let volcanism = |s: &str| volcanism_arg(&json!({ "volcanism": s }));
        assert_eq!((volcanism("rocky magma"), volcanism("Major Metallic Magma"), volcanism("none")), (Ok(Some(Volcanism::RockyMagma)), Ok(Some(Volcanism::MetallicMagma)), Ok(Some(Volcanism::Inactive))));
        assert!(volcanism("silicate magma").is_err(), "no body Spansh knows has it: not searchable");
        assert!(volcanism("lava").unwrap_err().contains("water geysers"));
    }

    fn mine(system: &str, body: &str, locations: u32, ly: f64, x: f64) -> Candidate {
        Candidate {
            system: system.into(),
            system_address: Some(7),
            system_pos: Some([x, 0.0, 0.0]),
            body: format!("{system} {body}"),
            body_id: Some(3),
            ground: Ground::of("Rocky body", "minor rocky magma volcanism"),
            class: "Rocky body".into(),
            volcanism: Some("Minor Rocky Magma".into()),
            atmosphere: None,
            locations,
            distance_ly: ly,
            distance_ls: Some(1234.4),
            gravity_g: Some(0.234),
            reported: Some(1_790_000_000),
        }
    }

    fn query(around: Around) -> Query {
        Query { around, radius_ly: 100.0, class: Some(Class::Rocky), volcanism: Some(Volcanism::RockyMagma), min_locations: 5, max_ls: 0 }
    }

    #[test]
    fn near_a_system_the_reply_lists_bodies_and_the_commanders_grounds() {
        let list: Vec<Candidate> = (0..20).map(|i| mine(&format!("Sys {i}"), "A 1", 5 + i, f64::from(i) + 0.5, 0.0)).collect();
        let found = Found { matched: 20, reach_ly: 19.5, list, ends: None };
        let own = vec![GroundYield { ground: Ground::of("Rocky body", "rocky magma"), locations: 4, bodies: 2, tonnes: [("Iridium".to_string(), 40)].into(), gave: [("Iridium".to_string(), 3)].into() }];
        let refs: Vec<&Candidate> = found.list.iter().collect();
        let worked = |c: &Candidate| usize::from(c.system == "Sys 0");
        let v = shape_near("Alpha", &query(Around::Here([0.0; 3])), &found, &refs, false, &worked, &own, 11_500);
        let bodies = v["bodies"].as_array().unwrap();
        assert_eq!(bodies.len(), MINE_HITS, "{v}");
        assert_eq!((&bodies[0]["body"], &bodies[0]["ground"], &bodies[0]["mining_locations"], &bodies[0]["you_worked"]), (&json!("Sys 0 A 1"), &json!("Rocky · rocky magma"), &json!(5), &json!(1)), "{v}");
        assert_eq!((&bodies[0]["distance_ls"], &bodies[0]["gravity_g"], &bodies[0]["counted_on"]), (&json!(1234.0), &json!(0.23), &json!("2026-09-21")), "{v}");
        assert!(bodies[1].get("you_worked").is_none());
        assert_eq!(v["looking_for"], "landable rocky bodies with rocky magma that have 5 or more mining locations");
        assert_eq!(v["your_grounds"][0]["refined_t"][0], json!({ "commodity": "Iridium", "tonnes": 40, "locations": 3 }));
        assert!(v["notes"].as_array().unwrap().iter().any(|n| n == "The 15 nearest are listed."), "{v}");
        // A short budget lists fewer bodies, nearest first.
        let short = shape_near("Alpha", &query(Around::Here([0.0; 3])), &found, &refs, false, &worked, &own, 2_500);
        assert!(json_len(&short) <= 2_500 && short["bodies"][0]["body"] == "Sys 0 A 1", "{short}");
        assert!(short["bodies"].as_array().unwrap().len() < MINE_HITS);
    }

    #[test]
    fn along_a_route_the_reply_names_the_best_system_in_each_stretch() {
        // A 1,000 ly route along x: bodies near its start and its end, none in the middle.
        let list = vec![mine("Start", "B 1", 9, 2.0, 50.0), mine("Start", "B 2", 3, 2.0, 50.0), mine("End", "C 1", 12, 4.0, 950.0)];
        let found = Found { matched: 3, reach_ly: 4.0, list, ends: Some(([0.0; 3], [1000.0, 0.0, 0.0])) };
        let stops = sf::route_stops(&found, 4);
        let v = shape_route("Here", "There", &query(Around::Route { from: "Here".into(), to: "There".into() }), &found, &stops, 4, &[], 11_500);
        let rows = v["stops"].as_array().unwrap();
        assert_eq!((rows.len(), &rows[0]["system"], &rows[1]["system"], &v["length_ly"]), (2, &json!("Start"), &json!("End"), &json!(1000.0)), "{v}");
        assert_eq!((&rows[0]["bodies"][0]["body"], &rows[0]["bodies_with_locations"]), (&json!("B 1"), &json!(2)), "most locations first: {v}");
        assert!(v["notes"].as_array().unwrap().iter().any(|n| n == "2 stretches had none."), "{v}");
        assert!(v.get("your_grounds").is_none(), "no runs of their own: nothing to say");
    }
}
