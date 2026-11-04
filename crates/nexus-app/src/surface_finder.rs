//! Where to surface-mine: bodies with planetary mining locations near the commander or along a route, from Spansh's
//! body search (the FSS and DSS scans commanders share through EDDN), and what the commander's own Rhino runs gave on
//! each ground.
//!
//! A body's mining-location count is known only when someone sharing data scanned it since the Rhino update (4.4.1,
//! September 2026), so a body without one may still have locations. No journal or feed says what a location holds:
//! the ground ([`Ground`]) and the commander's own runs ([`own_grounds`]) are leads, not promises.

use crate::pages::navigation::{PlannedRoute, Waypoint};
use crate::tours;
use nexus_data::grounds::{Class, Ground, Volcanism};
use nexus_model::GameState;
use nexus_model::location::distance;
use nexus_spansh::{BodyRecord, Coords, Reference, SearchRequest, SortDir, Spansh, filter, sort};
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// The name Spansh's body search gives a body's planetary mining locations (`$PlanetaryMiningLocation_Name;` in
/// the journal).
pub const MINING_SIGNAL: &str = "Planetary Mining Location";

/// Pages of 500 bodies read at most along a route, nearest the line first: 2,000 bodies keep the load on Spansh's
/// server light.
pub const MAX_PAGES: u32 = 4;

/// Spansh's sub-type for a class.
fn spansh_class(c: Class) -> &'static str {
    match c {
        Class::MetalRich => "Metal-rich body",
        Class::HighMetal => "High metal content world",
        Class::Rocky => "Rocky body",
        Class::RockyIce => "Rocky Ice world",
        Class::Icy => "Icy body",
    }
}

/// Spansh's `volcanism_type` values for a volcanism, plain, major and minor. Of those, its field values held 21 on
/// 2026-11-04 (ammonia, methane and nitrogen magma as minor only); none for silicate magma.
fn spansh_volcanism(v: Volcanism) -> Option<[String; 3]> {
    let name = match v {
        Volcanism::MetallicMagma => "Metallic Magma",
        Volcanism::RockyMagma => "Rocky Magma",
        Volcanism::SilicateVapourGeysers => "Silicate Vapour Geysers",
        Volcanism::WaterMagma => "Water Magma",
        Volcanism::WaterGeysers => "Water Geysers",
        Volcanism::CarbonDioxideGeysers => "Carbon Dioxide Geysers",
        Volcanism::AmmoniaMagma => "Ammonia Magma",
        Volcanism::MethaneMagma => "Methane Magma",
        Volcanism::NitrogenMagma => "Nitrogen Magma",
        Volcanism::Inactive | Volcanism::SilicateMagma | Volcanism::Other => return None,
    };
    Some([name.to_string(), format!("Major {name}"), format!("Minor {name}")])
}

/// Spansh's filter for a volcanism. A body without volcanism usually has no `volcanism_type` at all, so "no volcanism"
/// asks for none of the volcanic values rather than for "No volcanism" (52 rocky bodies with mining locations within
/// 20 ly of Sol against 7). None for silicate magma, which no body Spansh knows has.
pub fn volcanism_filter(v: Volcanism) -> Option<Value> {
    match v {
        Volcanism::Inactive => Some(filter::none_of(Volcanism::NAMED.into_iter().filter_map(spansh_volcanism).flatten())),
        _ => spansh_volcanism(v).map(filter::any_of),
    }
}

/// The volcanisms a search can ask for.
pub fn searchable_volcanism() -> impl Iterator<Item = Volcanism> {
    Volcanism::NAMED.into_iter().filter(|v| volcanism_filter(*v).is_some())
}

/// Where to look.
#[derive(Debug, Clone, PartialEq)]
pub enum Around {
    /// Around a point (the commander's system), nearest first.
    Here([f64; 3]),
    /// Along the straight line between two systems, nearest the line first.
    Route { from: String, to: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub around: Around,
    /// From the point, or from the line, ly.
    pub radius_ly: f64,
    /// One class, or any.
    pub class: Option<Class>,
    /// One volcanism, or any.
    pub volcanism: Option<Volcanism>,
    pub min_locations: u32,
    /// Furthest from the arrival star, ls (0: any).
    pub max_ls: u32,
}

/// One page of the body search, nearest first.
pub fn request(q: &Query, page: u32) -> SearchRequest {
    let reference = match &q.around {
        Around::Here(p) => Reference::Coords(Coords::new(p[0], p[1], p[2])),
        Around::Route { from, to } => Reference::Route { source: from.clone(), destination: to.clone() },
    };
    let mut req = SearchRequest::new()
        .near(reference)
        .within(q.radius_ly)
        .filter("is_landable", filter::eq(true))
        .filter("signals", json!([filter::named_count(MINING_SIGNAL, q.min_locations.max(1), 999)]))
        .sort(sort::field("distance", SortDir::Asc))
        .size(nexus_spansh::search::MAX_PAGE_SIZE)
        .page(page);
    if let Some(c) = q.class {
        req = req.filter("subtype", filter::any_of([spansh_class(c)]));
    }
    if let Some(v) = q.volcanism.and_then(volcanism_filter) {
        req = req.filter("volcanism_type", v);
    }
    if q.max_ls > 0 {
        req = req.filter("distance_to_arrival", filter::between(0.0, f64::from(q.max_ls)));
    }
    req
}

/// A body worth landing on.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub system: String,
    /// Spansh's id64: the journal's SystemAddress.
    pub system_address: Option<u64>,
    pub system_pos: Option<[f64; 3]>,
    pub body: String,
    pub body_id: Option<u32>,
    pub ground: Option<Ground>,
    /// Spansh's sub-type and volcanism, as it spells them.
    pub class: String,
    pub volcanism: Option<String>,
    pub atmosphere: Option<String>,
    pub locations: u32,
    /// From the commander, or from the route's line, ly.
    pub distance_ly: f64,
    pub distance_ls: Option<f64>,
    pub gravity_g: Option<f64>,
    /// When the body's signals (and so its count) were last reported (unix seconds).
    pub reported: Option<i64>,
}

impl Candidate {
    /// "B 2" for "Synuefe AB-C d1-2 B 2".
    pub fn short_body(&self) -> &str {
        self.body.strip_prefix(&self.system).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&self.body)
    }
}

pub fn candidate(b: BodyRecord) -> Candidate {
    let class = b.subtype.clone().unwrap_or_default();
    let system_pos = match (b.system_x, b.system_y, b.system_z) {
        (Some(x), Some(y), Some(z)) => Some([x, y, z]),
        _ => None,
    };
    Candidate {
        ground: Ground::of(&class, b.volcanism_type.as_deref().unwrap_or("")),
        locations: b.signal(MINING_SIGNAL),
        reported: b.signals_updated_at.as_deref().and_then(nexus_spansh::parse_timestamp),
        system_address: b.system_id64,
        system_pos,
        body_id: b.body_id,
        distance_ly: b.distance.unwrap_or(0.0),
        distance_ls: b.distance_to_arrival,
        gravity_g: b.gravity,
        volcanism: b.volcanism_type,
        atmosphere: b.atmosphere,
        system: b.system_name,
        body: b.name,
        class,
    }
}

/// What a search found.
#[derive(Debug, Clone, Default)]
pub struct Found {
    /// Nearest first: to the commander, or to the route's line.
    pub list: Vec<Candidate>,
    /// Bodies that match within the radius (Spansh stops counting at 10,000).
    pub matched: u64,
    /// The furthest body read, ly.
    pub reach_ly: f64,
    /// A route's two ends, as Spansh found them.
    pub ends: Option<([f64; 3], [f64; 3])>,
}

/// The 500 bodies nearest the commander: one request.
pub async fn near(spansh: &Spansh, q: &Query) -> Result<Found, String> {
    let res = spansh.search_bodies(&request(q, 0)).await.map_err(|e| e.to_string())?;
    Ok(Found {
        matched: res.count,
        reach_ly: res.results.last().and_then(|b| b.distance).unwrap_or(0.0),
        list: res.results.into_iter().map(candidate).collect(),
        ends: None,
    })
}

/// Bodies along the route, nearest the line first, a page at a time until each of `stretches` equal stretches of the
/// route has one, or [`MAX_PAGES`] pages are read.
pub async fn along(spansh: &Spansh, q: &Query, stretches: usize) -> Result<Found, String> {
    let mut found = Found::default();
    for page in 0..MAX_PAGES {
        let res = spansh.search_bodies(&request(q, page)).await.map_err(|e| route_error(&e, &q.around))?;
        if page == 0 {
            found.matched = res.count;
            found.ends = match res.reference.as_slice() {
                [a, b] => Some(([a.x, a.y, a.z], [b.x, b.y, b.z])),
                _ => None,
            };
        }
        let full = res.results.len() == nexus_spansh::search::MAX_PAGE_SIZE as usize;
        if let Some(d) = res.results.last().and_then(|b| b.distance) {
            found.reach_ly = d;
        }
        found.list.extend(res.results.into_iter().map(candidate));
        let Some(ends) = found.ends else {
            return Err("Spansh didn't say where the route starts and ends.".into());
        };
        if !full || covered(&found.list, ends, stretches) {
            break;
        }
    }
    Ok(found)
}

/// Spansh refuses a route search with "Invalid request" (HTTP 400) when it doesn't know one of the systems.
fn route_error(e: &nexus_spansh::Error, around: &Around) -> String {
    match (e.status(), around) {
        (Some(400), Around::Route { from, to }) => {
            format!("Spansh refused the search, as it does when it doesn't know a system: check that {to} is spelled as in the galaxy map. Spansh may not know {from} yet either.")
        }
        _ => e.to_string(),
    }
}

/// How far along the line from `a` to `b` the point `p` lies: 0 at `a`, 1 at `b` (clamped).
pub fn progress(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
    if len2 <= f64::EPSILON {
        return 0.0;
    }
    let ap = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
    ((ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2]) / len2).clamp(0.0, 1.0)
}

/// Which of `n` equal stretches progress `t` falls in.
fn stretch(t: f64, n: usize) -> usize {
    ((t * n as f64) as usize).min(n.saturating_sub(1))
}

/// Every stretch has a body.
fn covered(list: &[Candidate], (a, b): ([f64; 3], [f64; 3]), n: usize) -> bool {
    let mut seen = vec![false; n.max(1)];
    for p in list.iter().filter_map(|c| c.system_pos) {
        seen[stretch(progress(a, b, p), n.max(1))] = true;
    }
    seen.iter().all(|s| *s)
}

/// A system to stop at on the way, with its bodies worth landing on.
#[derive(Debug, Clone)]
pub struct RouteStop {
    pub system: String,
    pub pos: [f64; 3],
    /// How far along the route from its start, ly.
    pub along_ly: f64,
    /// From the straight line, ly.
    pub off_line_ly: f64,
    /// Most mining locations first.
    pub bodies: Vec<Candidate>,
}

impl RouteStop {
    pub fn most_locations(&self) -> u32 {
        self.bodies.first().map_or(0, |c| c.locations)
    }
}

/// A system is better with a body of more mining locations, then with more bodies that have them, then closer to the
/// line.
fn better(a: &RouteStop, b: &RouteStop) -> Ordering {
    a.most_locations().cmp(&b.most_locations()).then(a.bodies.len().cmp(&b.bodies.len())).then(b.off_line_ly.total_cmp(&a.off_line_ly))
}

/// The best system in each of `stops` equal stretches of the route, in route order. Stretches where the search found
/// nothing are left out.
pub fn route_stops(found: &Found, stops: usize) -> Vec<RouteStop> {
    let Some((a, b)) = found.ends else { return Vec::new() };
    let n = stops.max(1);
    let length = distance(a, b);
    let mut systems: BTreeMap<&str, RouteStop> = BTreeMap::new();
    for c in &found.list {
        let Some(p) = c.system_pos else { continue };
        let s = systems.entry(c.system.as_str()).or_insert_with(|| RouteStop {
            system: c.system.clone(),
            pos: p,
            along_ly: progress(a, b, p) * length,
            off_line_ly: c.distance_ly,
            bodies: Vec::new(),
        });
        s.bodies.push(c.clone());
    }
    let mut best: Vec<Option<RouteStop>> = vec![None; n];
    for mut s in systems.into_values() {
        s.bodies.sort_by(|x, y| y.locations.cmp(&x.locations).then_with(|| x.body.cmp(&y.body)));
        let i = stretch(if length > 0.0 { s.along_ly / length } else { 0.0 }, n);
        if best[i].as_ref().is_none_or(|cur| better(&s, cur) == Ordering::Greater) {
            best[i] = Some(s);
        }
    }
    best.into_iter().flatten().collect()
}

/// "B 2 · 14 locations, B 3 · 9": a stop's best bodies.
fn bodies_note<'a>(bodies: impl Iterator<Item = &'a Candidate>) -> String {
    bodies.take(3).map(|c| format!("{} · {} location{}", c.short_body(), c.locations, if c.locations == 1 { "" } else { "s" })).collect::<Vec<_>>().join(", ")
}

/// The route for Navigation: the stops in order, then the destination.
pub fn route_plan(stops: &[RouteStop], from: Option<[f64; 3]>, to: &str, to_pos: Option<[f64; 3]>, range_ly: f64) -> PlannedRoute {
    let jumps = |a: Option<[f64; 3]>, b: Option<[f64; 3]>| match (a, b) {
        (Some(a), Some(b)) => (distance(a, b) / range_ly.max(1.0)).ceil().max(1.0) as u32,
        _ => 1,
    };
    let waypoint = |system: &str, pos: Option<[f64; 3]>, jumps: u32, note: String| Waypoint {
        system: system.to_string(),
        pos,
        star_class: None,
        jumps,
        neutron: false,
        refuel: false,
        scoopable: None,
        value: None,
        note,
    };
    let mut here = from;
    let mut waypoints = Vec::with_capacity(stops.len() + 1);
    for s in stops {
        waypoints.push(waypoint(&s.system, Some(s.pos), jumps(here, Some(s.pos)), bodies_note(s.bodies.iter())));
        here = Some(s.pos);
    }
    if !stops.last().is_some_and(|s| s.system.eq_ignore_ascii_case(to)) {
        waypoints.push(waypoint(to, to_pos, jumps(here, to_pos), "Destination".into()));
    }
    let bodies: usize = stops.iter().map(|s| s.bodies.len()).sum();
    let summary = format!("{} stop{} on the way to {to} · {bodies} bodies with mining locations", stops.len(), if stops.len() == 1 { "" } else { "s" });
    PlannedRoute::new("Surface mining", summary, String::new(), waypoints)
}

/// A system picked for a tour, with its bodies.
struct Pick<'a> {
    system: &'a str,
    pos: Option<[f64; 3]>,
    bodies: Vec<&'a Candidate>,
}

/// A tour of the first `systems` systems in `list`, each leg to the nearest system not yet visited.
pub fn near_tour(list: &[&Candidate], systems: usize, origin: Option<[f64; 3]>, range_ly: f64) -> PlannedRoute {
    let mut picked: Vec<Pick> = Vec::new();
    for &c in list {
        if let Some(p) = picked.iter_mut().find(|p| p.system.eq_ignore_ascii_case(&c.system)) {
            p.bodies.push(c);
        } else if picked.len() < systems {
            picked.push(Pick { system: &c.system, pos: c.system_pos, bodies: vec![c] });
        }
    }
    let bodies: usize = picked.iter().map(|p| p.bodies.len()).sum();
    let summary = format!("{} systems · {bodies} bodies with mining locations", picked.len());
    let stops = picked
        .into_iter()
        .map(|mut p| {
            p.bodies.sort_by_key(|c| std::cmp::Reverse(c.locations));
            tours::Stop { system: p.system.to_string(), pos: p.pos, note: bodies_note(p.bodies.into_iter()), value: 0 }
        })
        .collect();
    let mut route = tours::tour_through("Surface mining", summary, origin, range_ly, stops);
    // The stops carry no credit value.
    for w in &mut route.waypoints {
        w.value = None;
    }
    route
}

/// Mining locations the commander's Rhino refined at on this body.
pub fn worked(gs: &GameState, system_address: u64, body_id: u32) -> usize {
    gs.mining.sites.keys().filter(|k| k.system_address == system_address && k.body_id == body_id).count()
}

/// A landable planet in the commander's system, from their own scans.
#[derive(Debug, Clone)]
pub struct LocalBody {
    pub name: String,
    pub ground: Option<Ground>,
    /// From their FSS or DSS scan (0: none reported).
    pub locations: u32,
    pub distance_ls: f64,
    pub gravity_g: Option<f64>,
    pub worked: usize,
}

/// The landable planets of the commander's system that their scans described, most mining locations first.
pub fn this_system(gs: &GameState) -> Vec<LocalBody> {
    let Some(sys) = gs.exploration.systems.get(&gs.location.system_address) else { return Vec::new() };
    let mut out: Vec<LocalBody> = sys
        .bodies
        .values()
        .filter(|b| b.landable)
        .filter_map(|b| {
            let class = b.planet_class.as_deref()?;
            let short = b.name.strip_prefix(&sys.name).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&b.name);
            Some(LocalBody {
                name: short.to_string(),
                ground: Ground::of(class, b.volcanism.as_deref().unwrap_or("")),
                locations: b.mining_locations,
                distance_ls: b.distance_ls,
                gravity_g: b.gravity_g(),
                worked: worked(gs, sys.address, b.id),
            })
        })
        .collect();
    out.sort_by(|a, b| b.locations.cmp(&a.locations).then(a.distance_ls.total_cmp(&b.distance_ls)));
    out
}

/// What the commander's own Rhino runs gave on one ground.
#[derive(Debug, Clone, Default)]
pub struct GroundYield {
    /// None: bodies with no class on record (never scanned).
    pub ground: Option<Ground>,
    /// Mining locations worked (one the game didn't name counts once on its body).
    pub locations: usize,
    pub bodies: usize,
    /// Tonnes refined by commodity, by-products included.
    pub tonnes: BTreeMap<String, u32>,
    /// How many of those locations gave each commodity.
    pub gave: BTreeMap<String, usize>,
}

impl GroundYield {
    pub fn total(&self) -> u32 {
        self.tonnes.values().sum()
    }

    /// (commodity, tonnes, locations that gave it), most tonnes first.
    pub fn by_tonnes(&self) -> Vec<(&str, u32, usize)> {
        let mut v: Vec<(&str, u32, usize)> = self.tonnes.iter().map(|(c, t)| (c.as_str(), *t, self.gave.get(c).copied().unwrap_or(0))).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        v
    }
}

/// The commander's Surface mining sites by the ground of their body (from their scan of it): most locations first,
/// bodies with no class on record last.
pub fn own_grounds(gs: &GameState) -> Vec<GroundYield> {
    let mut by: BTreeMap<Option<Ground>, GroundYield> = BTreeMap::new();
    let mut bodies: BTreeSet<(Option<Ground>, u64, u32)> = BTreeSet::new();
    for site in gs.mining.sites.values() {
        let body = gs.exploration.systems.get(&site.key.system_address).and_then(|s| s.bodies.get(&site.key.body_id));
        let ground = body.and_then(|b| Ground::of(b.planet_class.as_deref()?, b.volcanism.as_deref().unwrap_or("")));
        let y = by.entry(ground).or_insert_with(|| GroundYield { ground, ..Default::default() });
        y.locations += 1;
        for (c, t) in &site.refined {
            *y.tonnes.entry(c.clone()).or_default() += t;
            *y.gave.entry(c.clone()).or_default() += 1;
        }
        bodies.insert((ground, site.key.system_address, site.key.body_id));
    }
    for (g, _, _) in &bodies {
        if let Some(y) = by.get_mut(g) {
            y.bodies += 1;
        }
    }
    let mut out: Vec<GroundYield> = by.into_values().collect();
    out.sort_by(|a, b| a.ground.is_none().cmp(&b.ground.is_none()).then(b.locations.cmp(&a.locations)).then(a.ground.cmp(&b.ground)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_journal::Timestamp;
    use nexus_model::mining::{SiteKey, SurfaceSite};

    fn query(class: Option<Class>, volcanism: Option<Volcanism>) -> Query {
        Query { around: Around::Here([1.0, 2.0, 3.0]), radius_ly: 100.0, class, volcanism, min_locations: 5, max_ls: 0 }
    }

    /// Spansh's `volcanism_type` values other than "No volcanism", as its field values listed them on 2026-11-04.
    const SPANSH_VOLCANISM: [&str; 21] = [
        "Carbon Dioxide Geysers",
        "Major Carbon Dioxide Geysers",
        "Major Metallic Magma",
        "Major Rocky Magma",
        "Major Silicate Vapour Geysers",
        "Major Water Geysers",
        "Major Water Magma",
        "Metallic Magma",
        "Minor Ammonia Magma",
        "Minor Carbon Dioxide Geysers",
        "Minor Metallic Magma",
        "Minor Methane Magma",
        "Minor Nitrogen Magma",
        "Minor Rocky Magma",
        "Minor Silicate Vapour Geysers",
        "Minor Water Geysers",
        "Minor Water Magma",
        "Rocky Magma",
        "Silicate Vapour Geysers",
        "Water Geysers",
        "Water Magma",
    ];

    #[test]
    fn the_request_asks_for_mining_locations_class_and_volcanism() {
        let j = request(&query(Some(Class::Rocky), Some(Volcanism::RockyMagma)), 1).to_json();
        let f = &j["filters"];
        assert_eq!(f["signals"], json!([{"name": "Planetary Mining Location", "count": [5, 999], "comparison": "<=>"}]));
        assert_eq!(f["is_landable"]["value"], json!(true));
        assert_eq!(f["subtype"]["value"], json!(["Rocky body"]));
        assert_eq!(f["volcanism_type"]["value"], json!(["Rocky Magma", "Major Rocky Magma", "Minor Rocky Magma"]));
        assert!(f["volcanism_type"].get("logic").is_none());
        assert_eq!(f["distance"]["max"], "100");
        assert!(f.get("distance_to_arrival").is_none());
        assert_eq!(j["reference_coords"], json!({"x": 1.0, "y": 2.0, "z": 3.0}));
        assert_eq!((j["page"].clone(), j["size"].clone()), (json!(1), json!(500)));
        // A class alone, or a volcanism alone.
        let icy = request(&query(Some(Class::Icy), None), 0).to_json();
        assert_eq!((icy["filters"]["subtype"]["value"].clone(), icy["filters"].get("volcanism_type")), (json!(["Icy body"]), None));
        let water = request(&query(None, Some(Volcanism::WaterGeysers)), 0).to_json();
        assert!(water["filters"].get("subtype").is_none());
    }

    #[test]
    fn spansh_volcanism_values_read_back_as_the_kind_asked_for() {
        // Every value Spansh lists belongs to a kind a search can ask for, and reads back as that kind.
        let asked: Vec<(Volcanism, Vec<String>)> = searchable_volcanism()
            .filter(|v| *v != Volcanism::Inactive)
            .map(|v| (v, volcanism_filter(v).and_then(|f| f["value"].as_array().cloned()).unwrap_or_default().iter().filter_map(|s| s.as_str().map(str::to_string)).collect()))
            .collect();
        for name in SPANSH_VOLCANISM {
            let (kind, _) = asked.iter().find(|(_, values)| values.iter().any(|v| v == name)).unwrap_or_else(|| panic!("{name} isn't asked for"));
            assert_eq!(Volcanism::of(name), *kind, "{name}");
        }
        assert_eq!(searchable_volcanism().count(), 10, "all named kinds but silicate magma");
        assert!(!searchable_volcanism().any(|v| v == Volcanism::SilicateMagma));
    }

    #[test]
    fn no_volcanism_is_none_of_the_volcanic_values() {
        let j = request(&query(Some(Class::Rocky), Some(Volcanism::Inactive)), 0).to_json();
        let v = &j["filters"]["volcanism_type"];
        assert_eq!(v["logic"], "NOT");
        let excluded: Vec<&str> = v["value"].as_array().map(|a| a.iter().filter_map(|s| s.as_str()).collect()).unwrap_or_default();
        for name in SPANSH_VOLCANISM {
            assert!(excluded.contains(&name), "{name}");
        }
    }

    #[test]
    fn any_ground_and_the_arrival_distance() {
        let q = Query { max_ls: 5000, around: Around::Route { from: "Sol".into(), to: "Colonia".into() }, ..query(None, None) };
        let j = request(&q, 0).to_json();
        assert!(j["filters"].get("subtype").is_none() && j["filters"].get("volcanism_type").is_none());
        assert_eq!(j["filters"]["distance_to_arrival"], json!({"comparison": "<=>", "value": [0, 5000]}));
        assert_eq!(j["reference_route"], json!({"source": "Sol", "destination": "Colonia"}));
    }

    fn body(system: &str, name: &str, x: f64, off_line: f64, locations: u32) -> Candidate {
        candidate(
            serde_json::from_value(json!({
                "id64": 1, "body_id": 3, "name": format!("{system} {name}"), "subtype": "Rocky body", "volcanism_type": "Minor Rocky Magma",
                "signals": [{"name": "Planetary Mining Location", "count": locations}, {"name": "Geological", "count": 2}],
                "signals_updated_at": "2026-10-20T10:00:00Z", "system_name": system, "system_id64": 77,
                "system_x": x, "system_y": 0.0, "system_z": 0.0, "distance": off_line, "distance_to_arrival": 120.0, "gravity": 0.2
            }))
            .expect("body"),
        )
    }

    #[test]
    fn a_spansh_body_becomes_a_candidate() {
        let c = body("Alpha", "B 2", 10.0, 3.5, 14);
        assert_eq!((c.short_body(), c.locations, c.ground), ("B 2", 14, Some(Ground { class: Class::Rocky, volcanism: Volcanism::RockyMagma })));
        assert_eq!((c.system_address, c.body_id, c.system_pos), (Some(77), Some(3), Some([10.0, 0.0, 0.0])));
        assert!(c.reported.is_some());
    }

    #[test]
    fn route_stops_take_the_best_system_of_each_stretch_in_order() {
        let found = Found {
            list: vec![
                body("Late", "1", 900.0, 1.0, 4),
                body("Early", "1", 50.0, 0.5, 6),
                body("Early rich", "A 1", 120.0, 8.0, 20),
                body("Middle", "2", 480.0, 2.0, 3),
                body("Middle", "3", 480.0, 2.0, 3),
                body("Middle lone", "1", 490.0, 1.0, 3),
            ],
            ends: Some(([0.0; 3], [1000.0, 0.0, 0.0])),
            ..Found::default()
        };
        let stops = route_stops(&found, 4);
        let names: Vec<&str> = stops.iter().map(|s| s.system.as_str()).collect();
        // Stretches of 250 ly. First: 20 locations beat 6. Second: Middle's two bodies of 3 beat one, though further
        // from the line. Third: nothing.
        assert_eq!(names, ["Early rich", "Middle", "Late"]);
        assert_eq!(stops[1].bodies.len(), 2);
        assert!((stops[2].along_ly - 900.0).abs() < 1e-9);
        let plan = route_plan(&stops, Some([0.0; 3]), "Colonia", Some([1000.0, 0.0, 0.0]), 50.0);
        let systems: Vec<&str> = plan.waypoints.iter().map(|w| w.system.as_str()).collect();
        assert_eq!(systems, ["Early rich", "Middle", "Late", "Colonia"]);
        assert_eq!(plan.waypoints[0].jumps, 3, "120 ly at 50 ly a jump");
        assert_eq!(plan.waypoints[1].note, "2 · 3 locations, 3 · 3 locations");
        assert!(plan.waypoints.iter().all(|w| w.value.is_none()));
    }

    /// Seen live: a route from a system Spansh doesn't know answers HTTP 400 "Invalid request".
    #[test]
    fn an_unknown_system_gets_a_plain_answer() {
        let route = Around::Route { from: "Alpha".into(), to: "Colonai".into() };
        let unknown = nexus_spansh::Error::Http { status: 400, url: "https://spansh.co.uk/api/bodies/search".into(), message: "Invalid request".into() };
        assert!(route_error(&unknown, &route).contains("check that Colonai is spelled as in the galaxy map"));
        let busy = nexus_spansh::Error::Http { status: 503, url: String::new(), message: "busy".into() };
        assert!(route_error(&busy, &route).contains("HTTP 503"), "other failures say what happened");
    }

    #[test]
    fn a_route_is_covered_when_every_stretch_has_a_body() {
        let ends = ([0.0; 3], [100.0, 0.0, 0.0]);
        let list = vec![body("A", "1", 10.0, 0.0, 1), body("B", "1", 60.0, 0.0, 1)];
        assert!(covered(&list, ends, 2));
        assert!(!covered(&list, ends, 3), "B at 60 ly is in the middle third: the last third is empty");
        assert_eq!(progress(ends.0, ends.1, [150.0, 5.0, 0.0]), 1.0, "past the end counts as the end");
        assert_eq!(progress(ends.0, ends.0, [5.0, 0.0, 0.0]), 0.0, "a route to where you are");
    }

    #[test]
    fn a_tour_near_you_visits_the_nearest_systems() {
        let list = [body("Near", "1", 5.0, 5.0, 3), body("Near", "2", 5.0, 5.0, 9), body("Mid", "1", 20.0, 20.0, 2), body("Far", "1", 60.0, 60.0, 30)];
        let refs: Vec<&Candidate> = list.iter().collect();
        let r = near_tour(&refs, 2, Some([0.0; 3]), 10.0);
        let systems: Vec<&str> = r.waypoints.iter().map(|w| w.system.as_str()).collect();
        assert_eq!(systems, ["Near", "Mid"]);
        assert_eq!(r.waypoints[0].note, "2 · 9 locations, 1 · 3 locations");
        assert!(r.waypoints.iter().all(|w| w.value.is_none()));
    }

    /// Two locations on a rocky magma moon, one on a metal-rich body, one on a body never scanned.
    #[test]
    fn your_grounds_come_from_your_sites_and_scans() {
        let mut gs = GameState::default();
        let sys = gs.exploration.system_mut(100, "Alpha", Timestamp(1));
        let moon = sys.body_mut(1, "Alpha 1 a");
        (moon.planet_class, moon.volcanism, moon.landable) = (Some("Rocky body".into()), Some("minor rocky magma volcanism".into()), true);
        let metal = sys.body_mut(2, "Alpha 2");
        (metal.planet_class, metal.landable) = (Some("Metal rich body".into()), true);
        let mut site = |body_id: u32, location: Option<u32>, refined: &[(&str, u32)]| {
            let key = SiteKey { system_address: 100, body_id, location };
            let mut s = SurfaceSite::new(key, "Alpha", "Alpha 1 a");
            s.refined = refined.iter().map(|(c, t)| (c.to_string(), *t)).collect();
            gs.mining.sites.insert(key, s);
        };
        site(1, Some(4), &[("Bastnasite", 30), ("Monazite", 2)]);
        site(1, Some(7), &[("Bastnasite", 12)]);
        site(2, None, &[("Platinum", 8)]);
        site(9, Some(1), &[("Iridium", 5)]);
        let g = own_grounds(&gs);
        let rocky_magma = Ground { class: Class::Rocky, volcanism: Volcanism::RockyMagma };
        let metal_rich = Ground { class: Class::MetalRich, volcanism: Volcanism::Inactive };
        assert_eq!(g.iter().map(|y| y.ground).collect::<Vec<_>>(), [Some(rocky_magma), Some(metal_rich), None]);
        assert_eq!((g[0].locations, g[0].bodies, g[0].total()), (2, 1, 44));
        assert_eq!(g[0].by_tonnes(), [("Bastnasite", 42, 2), ("Monazite", 2, 1)]);
        assert_eq!(worked(&gs, 100, 1), 2);
    }

    #[test]
    fn this_system_lists_the_landable_planets_you_scanned() {
        let mut gs = GameState::default();
        gs.location.system_address = 100;
        let sys = gs.exploration.system_mut(100, "Alpha", Timestamp(1));
        for (id, name, class, landable, locations, ls) in
            [(1, "Alpha 1", "Rocky body", true, 4, 900.0), (2, "Alpha 2", "Icy body", true, 13, 2000.0), (3, "Alpha 3", "Class I gas giant", false, 0, 50.0), (4, "Alpha 4", "High metal content body", true, 0, 10.0)]
        {
            let b = sys.body_mut(id, name);
            (b.planet_class, b.landable, b.mining_locations, b.distance_ls) = (Some(class.into()), landable, locations, ls);
        }
        let list = this_system(&gs);
        assert_eq!(list.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["2", "1", "4"], "most locations first, the gas giant left out");
        assert_eq!(list[0].ground.map(|g| g.class), Some(Class::Icy));
    }

    /// Live: `cargo test -p nexus-app surface_finder -- --ignored --nocapture` (needs the internet).
    #[test]
    #[ignore]
    fn live_search_near_sol_and_along_a_route() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
        let sp = Spansh::new();
        // The nearest rocky magma body with mining locations is 55 ly from Sol (2026-11-04).
        let rocky_magma = Ground { class: Class::Rocky, volcanism: Volcanism::RockyMagma };
        let near_q = Query { around: Around::Here([0.0, 0.0, 0.0]), radius_ly: 100.0, class: Some(Class::Rocky), volcanism: Some(Volcanism::RockyMagma), min_locations: 1, max_ls: 0 };
        let found = rt.block_on(near(&sp, &near_q)).expect("near");
        println!("near Sol: {} matched, {} read", found.matched, found.list.len());
        assert!(!found.list.is_empty());
        assert!(found.list.iter().all(|c| c.ground == Some(rocky_magma) && c.locations > 0));
        // Rocky bodies without volcanism: Spansh mostly leaves the field out for them, which the filter allows for.
        let quiet_q = Query { volcanism: Some(Volcanism::Inactive), radius_ly: 20.0, ..near_q };
        let found = rt.block_on(near(&sp, &quiet_q)).expect("quiet");
        println!("rocky, no volcanism, within 20 ly of Sol: {}", found.matched);
        assert!(found.list.iter().any(|c| c.body == "Ganymede"));
        assert!(found.list.iter().all(|c| c.ground.is_some_and(|g| g.volcanism == Volcanism::Inactive)));
        let route_q = Query { around: Around::Route { from: "Sol".into(), to: "Colonia".into() }, radius_ly: 25.0, class: None, volcanism: None, min_locations: 10, max_ls: 0 };
        let found = rt.block_on(along(&sp, &route_q, 10)).expect("along");
        let stops = route_stops(&found, 10);
        for s in &stops {
            println!("{:>6.0} ly  {:<36} {:>4.1} ly off  {}", s.along_ly, s.system, s.off_line_ly, bodies_note(s.bodies.iter()));
        }
        assert!(found.ends.is_some() && !stops.is_empty());
    }
}
