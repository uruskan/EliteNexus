//! Where to buy a build: the modules (and the hull) a build needs, what the commander already has
//! in storage, and the nearest stations that sell all of it, from Spansh's outfitting search.
//!
//! Spansh filters on a module's name, class and rating, so a search asks for every name the module
//! goes by (MercGear rows share their base module's symbol) and then checks each station it
//! returns against the exact symbols: a weapon's mount, whose bulkheads they are. When no station
//! sells everything, the nearby station with most of it is the first stop and the station nearest
//! to it with all the rest the second; failing that, the nearest seller of each missing item.

use nexus_data::outfitting::{self, SlotKind};
use nexus_data::{ships, sku};
use nexus_model::fleet::{Fleet, Ship};
use nexus_spansh::{Coords, ModuleFilter, OutfittingQuery, Reference, Spansh, StationRecord};
use serde_json::Value;
use std::time::Duration;

/// A ship build: the hull and what's fitted, read from a journal `Loadout` or a SLEF export.
#[derive(Debug, Clone, PartialEq)]
pub struct Build {
    /// Lower-case hull symbol (`anaconda`).
    pub ship: String,
    /// The ship's name in the build, if it has one.
    pub name: Option<String>,
    pub modules: Vec<Fitted>,
    /// The `Loadout` object, to open the build in EDSY or Coriolis.
    pub loadout: String,
}

/// One fitted module.
#[derive(Debug, Clone, PartialEq)]
pub struct Fitted {
    /// Journal slot (`Slot01_Size7`, `HugeHardpoint1`).
    pub slot: String,
    /// Lower-case module symbol.
    pub item: String,
    /// Its engineering, for the list ("G5 Overcharged").
    pub engineering: Option<String>,
}

const HOW_TO_EXPORT: &str = "EDSY: OPS › Export… › SLEF. Coriolis: name the build, then the Export button › SLEF.";

/// Reads a build from pasted text: a SLEF export (EDSY, Coriolis, EDOMH, EDDiscovery), a journal
/// `Loadout` event, or an EDSY or Coriolis import link (the kind EDMC and Elite Nexus open).
pub fn parse_build(text: &str) -> Result<Build, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Paste a build first.".into());
    }
    let not_a_build = || format!("That isn't a build Elite Nexus can read. Paste a SLEF export ({HOW_TO_EXPORT}), a journal Loadout event or an EDSY or Coriolis import link.");
    let json = if text.starts_with('{') || text.starts_with('[') {
        text.to_string()
    } else if let Some(json) = crate::export::decode_link(text) {
        json
    } else if ["edsy.org", "coriolis.io", "orbis.zone"].iter().any(|site| text.contains(site)) {
        return Err(format!("That's a share link, which only the site itself can read. Open it there and export the build as SLEF instead ({HOW_TO_EXPORT})."));
    } else {
        return Err(not_a_build());
    };
    let value: Value = serde_json::from_str(&json).map_err(|_| not_a_build())?;
    match loadout_of(&value) {
        Some(loadout) => from_loadout(loadout),
        None if is_coriolis_json(&value) => Err("That's Coriolis's own JSON, which has no game symbols: in Coriolis's Export dialog, choose SLEF instead.".into()),
        None => Err(not_a_build()),
    }
}

/// The `Loadout` object in a SLEF array or item, or the event itself. Coriolis's SLEF has no
/// `event` field, so it's recognised by its `Ship` and `Modules`.
fn loadout_of(v: &Value) -> Option<&Value> {
    match v {
        Value::Array(items) => items.iter().find_map(loadout_of),
        Value::Object(o) => match o.get("data") {
            Some(data) => loadout_of(data),
            None => (o.get("Ship").is_some_and(Value::is_string) && o.get("Modules").is_some_and(Value::is_array)).then_some(v),
        },
        _ => None,
    }
}

/// Coriolis's "JSON" export (its `ship-loadout` schema): classes and ratings, no game symbols.
fn is_coriolis_json(v: &Value) -> bool {
    match v {
        Value::Array(items) => items.iter().any(is_coriolis_json),
        Value::Object(o) => o.get("$schema").and_then(Value::as_str).is_some_and(|s| s.contains("coriolis")) || o.get("components").is_some_and(Value::is_object),
        _ => false,
    }
}

fn from_loadout(l: &Value) -> Result<Build, String> {
    let ship = l.get("Ship").and_then(Value::as_str).map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).ok_or("The build doesn't say which ship it's for.")?;
    let name = l.get("ShipName").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let mut modules = Vec::new();
    for m in l.get("Modules").and_then(Value::as_array).into_iter().flatten() {
        let (Some(slot), Some(item)) = (m.get("Slot").and_then(Value::as_str), m.get("Item").and_then(Value::as_str)) else { continue };
        // Paint, the cockpit, the cargo hatch and the built-in scanners aren't bought.
        if matches!(outfitting::slot_kind(slot), SlotKind::Cosmetic | SlotKind::Other) {
            continue;
        }
        let item = item.trim().to_ascii_lowercase();
        if item.is_empty() {
            continue;
        }
        modules.push(Fitted { slot: slot.trim().to_string(), engineering: engineering(&item, m.get("Engineering")), item });
    }
    if modules.is_empty() {
        return Err("That build has no modules in it.".into());
    }
    Ok(Build { ship, name, modules, loadout: l.to_string() })
}

/// "G5 Overcharged, Thermal Conduit" from a module's `Engineering` object.
fn engineering(item: &str, e: Option<&Value>) -> Option<String> {
    let e = e?;
    let bp = e.get("BlueprintName").and_then(Value::as_str)?;
    let name = nexus_data::engineering::blueprint_for(item, bp).map(|b| b.name.clone()).unwrap_or_else(|| nexus_data::names::split_camel(&bp.replace('_', " ")));
    let experimental = e
        .get("ExperimentalEffect_Localised")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| e.get("ExperimentalEffect").and_then(Value::as_str).and_then(nexus_data::engineering::experimental).map(|x| x.name.clone()));
    let grade = e.get("Level").and_then(Value::as_u64).filter(|g| *g > 0).map(|g| format!("G{g} ")).unwrap_or_default();
    Some(match experimental {
        Some(x) => format!("{grade}{name}, {x}"),
        None => format!("{grade}{name}"),
    })
}

// ---------------------------------------------------------------------------------------------
// The shopping list

/// Where a row sits in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Hull,
    Core,
    Optional,
    Hardpoints,
    Utility,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::Hull => "Hull",
            Group::Core => "Core internals",
            Group::Optional => "Optional internals",
            Group::Hardpoints => "Hardpoints",
            Group::Utility => "Utility mounts",
        }
    }

    fn of(slot: &str) -> Group {
        match outfitting::slot_kind(slot) {
            SlotKind::Core => Group::Core,
            SlotKind::Hardpoint => Group::Hardpoints,
            SlotKind::Utility => Group::Utility,
            _ => Group::Optional,
        }
    }
}

/// One thing to buy: a module (as many as the build fits) or the hull.
#[derive(Debug, Clone, PartialEq)]
pub struct Need {
    pub group: Group,
    /// Lower-case symbol of the module or the hull.
    pub symbol: String,
    pub name: String,
    pub count: u32,
    /// Engineering on the build's copies: stations sell the plain module, engineers do the rest.
    pub engineering: Vec<String>,
    /// Ticked: the search looks for it.
    pub buy: bool,
    /// What the commander already has, or that it comes with the hull.
    pub note: Option<String>,
    pub unlock: Option<sku::Unlock>,
}

/// What `build` needs bought: the hull, then each module once with how many the build fits.
/// Modules in the commander's storage, the bulkheads every new hull comes with and a hull they
/// already own start unticked, with a note saying why.
pub fn needs(build: &Build, fleet: &Fleet) -> Vec<Need> {
    let ship_name = ships::ship_name(&build.ship);
    let owned = fleet.ships.values().find(|s| s.ship_type.eq_ignore_ascii_case(&build.ship));
    let mut out = vec![Need {
        group: Group::Hull,
        symbol: build.ship.clone(),
        name: ship_name.clone(),
        count: 1,
        engineering: Vec::new(),
        buy: owned.is_none(),
        note: owned.map(|s| format!("You own one: {}{}", s.display_name(), where_is(s, fleet))),
        unlock: ships::ship(&build.ship).and_then(|h| h.entitlement.as_deref()).and_then(unlock),
    }];
    for f in &build.modules {
        if let Some(n) = out.iter_mut().find(|n| n.group != Group::Hull && n.symbol == f.item) {
            n.count += 1;
            n.engineering.extend(f.engineering.clone());
            continue;
        }
        out.push(Need {
            group: Group::of(&f.slot),
            symbol: f.item.clone(),
            name: outfitting::display_name(&f.item),
            count: 1,
            engineering: f.engineering.iter().cloned().collect(),
            buy: true,
            note: None,
            unlock: outfitting::module(&f.item).and_then(|m| m.entitlement.as_deref()).and_then(unlock),
        });
    }
    for n in out.iter_mut().filter(|n| n.group != Group::Hull) {
        if comes_with_hull(&n.symbol) {
            n.buy = false;
            n.note = Some(format!("Comes with every new {ship_name}"));
            continue;
        }
        let stored: Vec<_> = fleet.stored_modules.iter().filter(|m| m.name == n.symbol).collect();
        if stored.is_empty() {
            continue;
        }
        let mut places: Vec<&str> = stored.iter().filter_map(|m| m.system.as_deref()).collect();
        places.sort_unstable();
        places.dedup();
        let at = if places.is_empty() { String::new() } else { format!(" in {}", places.join(", ")) };
        let have = stored.len() as u32;
        if have >= n.count {
            n.buy = false;
            n.note = Some(if n.count == 1 { format!("In your storage{at}") } else { format!("{have} in your storage{at}") });
        } else {
            n.note = Some(format!("{have} of {} in your storage{at}", n.count));
        }
    }
    out.sort_by_key(|n| n.group);
    out
}

/// The unlock a SKU needs: tech brokers and Powerplay. FDevIDs keeps "early access" on hulls that
/// sell for credits by now (the Kestrel Mk II in November 2026), so ARX-only is read from Spansh's
/// prices instead ([`Offer::sells`] has no price for it).
fn unlock(sku: &str) -> Option<sku::Unlock> {
    sku::unlock(sku).filter(|u| *u != sku::Unlock::ArxEarlyAccess)
}

/// " (your current ship)", " at Jameson Memorial (Shinrarta Dezhra)".
fn where_is(s: &Ship, fleet: &Fleet) -> String {
    if fleet.current_ship_id == Some(s.id) {
        return " (your current ship)".into();
    }
    match (&s.location.station, &s.location.system) {
        (Some(st), Some(sys)) => format!(" at {st} ({sys})"),
        (None, Some(sys)) => format!(" in {sys}"),
        _ => String::new(),
    }
}

/// The bulkheads every new hull comes with: `…_armour_grade1`, or `…_armour_grade1_default` on
/// hulls (Caspian Explorer, Anaconda Mk II) whose plain grade 1 is a bought ablative alloy.
fn comes_with_hull(symbol: &str) -> bool {
    symbol.ends_with("_armour_grade1_default") || (symbol.ends_with("_armour_grade1") && outfitting::module(&format!("{symbol}_default")).is_none())
}

// ---------------------------------------------------------------------------------------------
// Searching

/// One thing the search looks for: a ticked [`Need`].
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    pub symbol: String,
    pub name: String,
    pub count: u32,
    pub hull: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Search {
    pub wanted: Vec<Wanted>,
    pub near: Reference,
    /// The landing pad the ship needs: 'S', 'M' or 'L'. 'S', so any pad, for a hull this build doesn't know.
    pub pad: char,
    pub carriers: bool,
    /// Leave out stations whose outfitting data is older than this many days.
    pub max_age_days: Option<u32>,
}

impl Search {
    /// Looks for the ticked rows of `needs`, for a ship of hull `ship`.
    pub fn new(needs: &[Need], ship: &str, near: Reference, carriers: bool, max_age_days: Option<u32>) -> Search {
        let wanted = needs.iter().filter(|n| n.buy).map(|n| Wanted { symbol: n.symbol.clone(), name: n.name.clone(), count: n.count, hull: n.group == Group::Hull }).collect();
        Search { wanted, near, pad: ships::pad_size(ship), carriers, max_age_days }
    }
}

/// A station, and which of the wanted items it sells at what price.
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub station: String,
    pub system: String,
    pub market_id: u64,
    pub station_type: Option<String>,
    pub carrier: bool,
    pub planetary: bool,
    /// From the search's reference (a second stop: from the first), ly.
    pub distance_ly: f64,
    pub arrival_ls: Option<f64>,
    pub pos: Option<[f64; 3]>,
    /// `UnderRepairs`, `Damaged`, …: its services may be down.
    pub state: Option<String>,
    /// When Spansh last had its outfitting data (and shipyard data, if it sells the hull): the
    /// older of the two, unix seconds.
    pub updated: Option<i64>,
    /// The wanted items it sells (indexes into `Search::wanted`), with their price if known.
    pub sells: Vec<(usize, Option<u64>)>,
}

impl Offer {
    pub fn sells(&self, i: usize) -> bool {
        self.sells.iter().any(|(j, _)| *j == i)
    }

    /// What `items` cost here (each × its count), and how many of them have no price on Spansh.
    pub fn cost(&self, items: &[usize], wanted: &[Wanted]) -> (u64, usize) {
        let (mut total, mut unpriced) = (0u64, 0usize);
        for &i in items {
            match self.sells.iter().find(|(j, _)| *j == i) {
                Some((_, Some(price))) => total += price * u64::from(wanted.get(i).map_or(1, |w| w.count)),
                Some((_, None)) => unpriced += 1,
                None => {}
            }
        }
        (total, unpriced)
    }
}

fn offer(st: &StationRecord, wanted: &[Wanted]) -> Offer {
    let mut sells = Vec::new();
    let mut hull = false;
    for (i, w) in wanted.iter().enumerate() {
        let price = if w.hull { st.ship(&w.symbol).map(|s| s.price) } else { st.module(&w.symbol).map(|m| m.price) };
        if let Some(p) = price {
            sells.push((i, p));
            hull |= w.hull;
        }
    }
    let outfitting = st.outfitting_updated_unix();
    let updated = if hull { [outfitting, st.shipyard_updated_unix()].into_iter().flatten().min() } else { outfitting };
    Offer {
        station: st.name.clone(),
        system: st.system_name.clone(),
        market_id: st.market_id,
        station_type: st.station_type.clone(),
        carrier: st.is_fleet_carrier(),
        planetary: st.is_planetary.unwrap_or(false),
        distance_ly: st.distance.unwrap_or(0.0),
        arrival_ls: st.distance_to_arrival,
        pos: st.system_coords().map(|c| [c.x, c.y, c.z]),
        state: st.state.as_deref().and_then(state_label).map(str::to_string),
        updated,
        sells,
    }
}

/// Spansh's station states that can take services down. `Construction` (a colony station still
/// being built up) keeps its outfitting, so it isn't flagged.
fn state_label(state: &str) -> Option<&'static str> {
    match state.trim() {
        "Damaged" | "DamagedHuman" => Some("Damaged"),
        "UnderAttack" => Some("Under attack"),
        "UnderRepairs" => Some("Under repairs"),
        _ => None,
    }
}

/// Has a pad the ship fits on. Stations Spansh has no pad counts for are kept.
fn has_pad(st: &StationRecord, pad: char) -> bool {
    if [st.small_pads, st.medium_pads, st.large_pads].iter().all(Option::is_none) {
        return pad != 'L' || st.has_large_pad != Some(false);
    }
    let (m, l) = (st.medium_pads.unwrap_or(0), st.large_pads.unwrap_or(0));
    match pad {
        'L' => l > 0,
        'M' => m + l > 0,
        _ => true,
    }
}

/// A fleet carrier that lets only its owner's friends or squadron dock.
fn closed_carrier(st: &StationRecord) -> bool {
    st.is_fleet_carrier() && st.carrier_docking_access.as_deref().map(str::trim).is_some_and(|a| !a.is_empty() && !a.eq_ignore_ascii_case("all"))
}

/// How the search went, for saying what was left out and why.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    /// Stations Spansh matched by name, class and rating (its count stops around 10,000).
    pub matched: u64,
    /// How many of them were read, nearest first.
    pub read: usize,
    /// Matched by name, but sold another mount or another hull's bulkheads.
    pub not_exact: usize,
    /// Fleet carriers that let only their owner's friends or squadron dock.
    pub closed: usize,
    /// No pad the ship fits on.
    pub no_pad: usize,
}

/// What the search found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    /// Stations that sell everything, nearest first.
    pub one_stop: Vec<Offer>,
    /// None does: the nearby station with most of it…
    pub first: Option<Offer>,
    /// …and the station nearest to it with all the rest (its distance is from `first`).
    pub second: Option<Offer>,
    /// No such second stop: for each item `first` lacks, the nearest station that sells it.
    pub elsewhere: Vec<(usize, Option<Offer>)>,
    /// Items `first` lacks that weren't looked up one by one (more than [`SINGLES`]).
    pub not_looked_up: usize,
    pub stats: Stats,
}

/// Stations listed when several sell everything.
pub const LIST: usize = 10;
/// Stations per page: each comes with its whole market and outfitting lists, tens of KB.
const PAGE: u32 = 20;
/// Pages read per question at most, nearest first.
pub const PAGES: u32 = 3;
/// Nearest stations with outfitting read to choose a first stop when none sells everything.
const NEAREST: u32 = 30;
/// Missing items looked up one by one at most.
pub const SINGLES: usize = 8;

/// Finds the nearest stations that sell everything in `q`; when none does, the best two stops, or
/// the nearest seller of each item the best first stop lacks.
pub async fn find(spansh: &Spansh, q: &Search) -> Result<Outcome, String> {
    if q.wanted.is_empty() {
        return Err("Nothing to buy: tick what you need.".into());
    }
    let all: Vec<usize> = (0..q.wanted.len()).collect();
    let mut out = Outcome::default();
    out.one_stop = sellers(spansh, q, &q.near, &all, LIST, PAGES, &mut out.stats).await?;
    if !out.one_stop.is_empty() || all.len() == 1 {
        return Ok(out);
    }
    let Some(first) = first_stop(nearest_with_outfitting(spansh, q).await?) else { return Ok(out) };
    let missing: Vec<usize> = all.iter().copied().filter(|&i| !first.sells(i)).collect();
    let from = first.pos.map(|[x, y, z]| Reference::Coords(Coords::new(x, y, z))).unwrap_or_else(|| Reference::System(first.system.clone()));
    let mut scratch = Stats::default();
    out.second = sellers(spansh, q, &from, &missing, 1, PAGES, &mut scratch).await?.into_iter().next();
    if out.second.is_none() {
        // One page each: these only say where the nearest seller of each piece is.
        for &i in missing.iter().take(SINGLES) {
            let nearest = sellers(spansh, q, &q.near, &[i], 1, 1, &mut scratch).await?.into_iter().next();
            out.elsewhere.push((i, nearest));
        }
        out.not_looked_up = missing.len().saturating_sub(SINGLES);
    }
    out.first = Some(first);
    Ok(out)
}

/// The station that sells the most of it, the nearest of those.
fn first_stop(offers: Vec<Offer>) -> Option<Offer> {
    offers.into_iter().filter(|o| !o.sells.is_empty()).min_by(|a, b| b.sells.len().cmp(&a.sells.len()).then(a.distance_ly.total_cmp(&b.distance_ly)))
}

/// Up to `want` stations near `near` that sell every one of `items`, nearest first: Spansh's
/// matches by name, checked against the exact symbols, a page at a time (`pages` at most).
async fn sellers(spansh: &Spansh, q: &Search, near: &Reference, items: &[usize], want: usize, pages: u32, stats: &mut Stats) -> Result<Vec<Offer>, String> {
    let (modules, hulls) = filters(items.iter().filter_map(|&i| q.wanted.get(i)));
    let mut found = Vec::new();
    for page in 0..pages {
        let res = spansh.outfitting_search(&query(q, near, modules.clone(), hulls.clone(), PAGE, page)).await.map_err(|e| format!("Spansh: {e}"))?;
        if page == 0 {
            stats.matched = res.count;
        }
        for st in &res.results {
            stats.read += 1;
            if !has_pad(st, q.pad) {
                stats.no_pad += 1;
            } else if closed_carrier(st) {
                stats.closed += 1;
            } else {
                let o = offer(st, &q.wanted);
                if items.iter().all(|&i| o.sells(i)) {
                    found.push(o);
                } else {
                    stats.not_exact += 1;
                }
            }
        }
        if found.len() >= want || !res.has_more() || res.results.is_empty() {
            break;
        }
    }
    found.truncate(want);
    Ok(found)
}

/// The nearest stations with outfitting, and what of `q` each sells.
async fn nearest_with_outfitting(spansh: &Spansh, q: &Search) -> Result<Vec<Offer>, String> {
    let res = spansh.outfitting_search(&query(q, &q.near, Vec::new(), Vec::new(), NEAREST, 0)).await.map_err(|e| format!("Spansh: {e}"))?;
    Ok(res.results.iter().filter(|st| has_pad(st, q.pad) && !closed_carrier(st)).map(|st| offer(st, &q.wanted)).collect())
}

fn query(q: &Search, near: &Reference, modules: Vec<ModuleFilter>, hulls: Vec<String>, size: u32, page: u32) -> OutfittingQuery {
    let mut query = OutfittingQuery::new(modules, near.clone()).ships(hulls).large_pad(q.pad == 'L').include_carriers(q.carriers).size(size).page(page);
    if let Some(days) = q.max_age_days {
        query = query.max_age(Duration::from_secs(u64::from(days) * 86_400));
    }
    query
}

/// Spansh filters for `wanted`: each module under every FDevIDs name it goes by, with FDevIDs'
/// class and rating; the hull by its FDevIDs name. Items FDevIDs doesn't know (newer than our
/// copy) get no filter and are left to the exact check.
fn filters<'a>(wanted: impl Iterator<Item = &'a Wanted>) -> (Vec<ModuleFilter>, Vec<String>) {
    let mut modules: Vec<ModuleFilter> = Vec::new();
    let mut hulls = Vec::new();
    for w in wanted {
        if w.hull {
            hulls.extend(ships::ship(&w.symbol).map(|s| s.fdev_name.clone()));
        } else if let Some(m) = outfitting::module(&w.symbol) {
            let names = outfitting::fdev_names(&w.symbol).into_iter().map(str::to_string).collect();
            let f = ModuleFilter::new(names, Some(m.fdev_class), Some(m.rating).filter(char::is_ascii_alphabetic));
            if !modules.contains(&f) {
                modules.push(f);
            }
        }
    }
    (modules, hulls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_model::fleet::StoredModuleInfo;
    use serde_json::json;

    /// An EDSY SLEF export, as its `exportSLEF` writes it: one line, lower-case symbols.
    const EDSY_SLEF: &str = r#"[{"header":{"appName":"EDSY","appVersion":424029901,"appURL":"https://edsy.org/#/L=K00"},"data":{"event":"Loadout","Ship":"anaconda","ShipName":"Bonecrusher","Modules":[
        {"Slot":"Armour","Item":"anaconda_armour_grade3"},
        {"Slot":"PowerPlant","Item":"int_powerplant_size8_class5"},
        {"Slot":"FrameShiftDrive","Item":"int_hyperdrive_size6_class5"},
        {"Slot":"PowerDistributor","Item":"int_powerdistributor_size8_class5"},
        {"Slot":"HugeHardpoint1","Item":"hpt_multicannon_gimbal_huge","Engineering":{"BlueprintName":"Weapon_Overcharged","Level":5,"ExperimentalEffect":"special_auto_loader"}},
        {"Slot":"LargeHardpoint1","Item":"hpt_beamlaser_gimbal_large"},
        {"Slot":"LargeHardpoint2","Item":"hpt_beamlaser_gimbal_large"},
        {"Slot":"TinyHardpoint1","Item":"hpt_shieldbooster_size0_class5"},
        {"Slot":"Slot01_Size7","Item":"int_shieldgenerator_size7_class5"},
        {"Slot":"Military01","Item":"int_hullreinforcement_size5_class2"},
        {"Slot":"PaintJob","Item":"paintjob_anaconda_tactical_blue"},
        {"Slot":"PlanetaryApproachSuite","Item":"int_planetapproachsuite_advanced"},
        {"Slot":"ShipCockpit","Item":"anaconda_cockpit"}]}}]"#;

    fn station(extra: Value) -> StationRecord {
        let mut v = json!({
            "market_id": 1, "name": "Alpha Port", "type": "Coriolis Starport", "system_name": "Alpha", "distance": 12.0,
            "distance_to_arrival": 400.0, "system_x": 10.0, "system_y": 0.0, "system_z": 5.0,
            "small_pads": 10, "medium_pads": 10, "large_pads": 6, "has_large_pad": true,
            "outfitting_updated_at": "2026-11-03T10:00:00Z", "shipyard_updated_at": "2026-11-02T10:00:00Z",
            "modules": [
                {"name": "Frame Shift Drive", "class": 6, "rating": "A", "ed_symbol": "Int_Hyperdrive_Size6_Class5", "price": 16_179_531},
                {"name": "Multi-Cannon", "class": 4, "rating": "A", "ed_symbol": "Hpt_MultiCannon_Fixed_Huge", "price": 6_377_600},
                // MercGear shares the symbol; Spansh lists the plain 5A distributor under its name.
                {"name": "Balanced Power Distributor", "category": "mercgear", "class": 5, "rating": "A", "ed_symbol": "Int_PowerDistributor_Size5_Class5"}
            ],
            "ships": [{"name": "Anaconda", "symbol": "Anaconda", "price": 146_969_450}]
        });
        if let (Some(base), Some(add)) = (v.as_object_mut(), extra.as_object()) {
            for (k, x) in add {
                base.insert(k.clone(), x.clone());
            }
        }
        serde_json::from_value(v).expect("station")
    }

    fn wanted(symbol: &str, count: u32, hull: bool) -> Wanted {
        Wanted { symbol: symbol.into(), name: symbol.into(), count, hull }
    }

    #[test]
    fn reads_an_edsy_slef_export() {
        let b = parse_build(EDSY_SLEF).expect("build");
        assert_eq!((b.ship.as_str(), b.name.as_deref()), ("anaconda", Some("Bonecrusher")));
        // Paint, the approach suite and the cockpit aren't bought.
        assert_eq!(b.modules.len(), 10);
        let mc = b.modules.iter().find(|m| m.slot == "HugeHardpoint1").expect("multi-cannon");
        // Blueprint names as the Engineering page has them.
        assert_eq!(mc.engineering.as_deref(), Some("G5 Overcharged Weapon, Auto Loader"));
        // The EDSY and Coriolis buttons open it again.
        crate::export::tests::both_sites_import(&b.loadout);
    }

    #[test]
    fn reads_coriolis_slef_journal_loadouts_and_import_links() {
        // Coriolis's SLEF has no `event`; older Coriolis keeps the symbols' case.
        let coriolis = r#"[{"header":{"appName":"Coriolis"},"data":{"Ship":"Krait_MkII","Modules":[{"Slot":"FrameShiftDrive","Item":"Int_Hyperdrive_Size5_Class5"}]}}]"#;
        let b = parse_build(coriolis).expect("coriolis");
        assert_eq!((b.ship.as_str(), b.modules[0].item.as_str()), ("krait_mkii", "int_hyperdrive_size5_class5"));
        // Without an event, EDSY couldn't import it from the build's EDSY button (finding 15, 2026-11-04).
        assert_eq!(crate::export::tests::both_sites_import(&b.loadout)["Ship"], "Krait_MkII");
        let journal = r#"{ "timestamp":"2026-10-01T12:00:00Z", "event":"Loadout", "Ship":"python", "ShipID":3, "Modules":[ { "Slot":"MainEngines", "Item":"int_engine_size6_class5", "On":true, "Priority":0 } ] }"#;
        assert_eq!(parse_build(journal).expect("journal").modules[0].item, "int_engine_size6_class5");
        let link = crate::export::coriolis_url(journal).expect("link");
        let from_link = parse_build(&link).expect("link");
        assert_eq!(from_link.ship, "python");
        crate::export::tests::both_sites_import(&from_link.loadout);
    }

    /// Coriolis's SLEF export as its `Serializer` writes it: the game's slot names, the cargo hatch,
    /// and the slots a Type-11 Prospector keeps for mining tools, limpet controllers and hangars.
    #[test]
    fn named_slots_are_bought_too() {
        let t11 = r#"[{"header":{"appName":"Coriolis","appVersion":"4.0","appURL":"https://coriolis.io/outfit/type_11_prospector"},"data":{"Ship":"LakonMiner","Modules":[
            {"Slot":"Armour","Item":"lakonminer_armour_grade1","On":true,"Priority":1},
            {"Slot":"CargoHatch","Item":"modularcargobaydoor","On":true,"Priority":2},
            {"Slot":"PowerPlant","Item":"int_powerplant_size6_class5","On":true,"Priority":1},
            {"Slot":"LargeMiningHardpoint1","Item":"hpt_miningtoolv2_fixed_large","On":true,"Priority":1},
            {"Slot":"MediumMiningHardpoint1","Item":"hpt_mininglaser_turret_medium","On":true,"Priority":1},
            {"Slot":"MediumHardpoint1","Item":"hpt_pulselaser_gimbal_medium","On":true,"Priority":1},
            {"Slot":"TinyHardpoint1","Item":"hpt_shieldbooster_size0_class5","On":true,"Priority":1},
            {"Slot":"Slot01_Size6","Item":"int_shieldgenerator_size6_class5","On":true,"Priority":1},
            {"Slot":"LimpetController01","Item":"int_dronecontrol_prospector_size5_class5","On":true,"Priority":1},
            {"Slot":"FighterBay01","Item":"int_buggybay_size4_class2","On":true,"Priority":1}]}}]"#;
        let b = parse_build(t11).expect("build");
        let slots: Vec<&str> = b.modules.iter().map(|m| m.slot.as_str()).collect();
        // The bulkheads every hull comes with stay on the list (unticked later); only the hatch is left out.
        assert_eq!(slots, ["Armour", "PowerPlant", "LargeMiningHardpoint1", "MediumMiningHardpoint1", "MediumHardpoint1", "TinyHardpoint1", "Slot01_Size6", "LimpetController01", "FighterBay01"]);
        let list = needs(&b, &Fleet::default());
        let group = |sym: &str| list.iter().find(|n| n.symbol == sym).map(|n| n.group);
        assert_eq!(group("hpt_miningtoolv2_fixed_large"), Some(Group::Hardpoints));
        assert_eq!(group("hpt_mininglaser_turret_medium"), Some(Group::Hardpoints));
        assert_eq!(group("int_dronecontrol_prospector_size5_class5"), Some(Group::Optional));
        assert_eq!(group("int_buggybay_size4_class2"), Some(Group::Optional));
        crate::export::tests::both_sites_import(&b.loadout);
    }

    #[test]
    fn says_why_a_paste_cant_be_read() {
        assert!(parse_build("").is_err());
        assert!(parse_build("https://coriolis.io/outfit/anaconda?code=A0p0tdFfld").unwrap_err().contains("share link"));
        assert!(parse_build("https://edsy.org/#/L=Aw18ZlAB").unwrap_err().contains("share link"));
        let coriolis_json = r#"{"$schema":"https://coriolis.io/schemas/ship-loadout/5.json#","name":"PvE","ship":"Anaconda","components":{"standard":{}}}"#;
        assert!(parse_build(coriolis_json).unwrap_err().contains("choose SLEF"));
        assert!(parse_build(r#"{"event":"Loadout","Ship":"anaconda","Modules":[]}"#).unwrap_err().contains("no modules"));
        assert!(parse_build("hello").unwrap_err().contains("SLEF"));
    }

    #[test]
    fn the_list_leaves_out_what_the_commander_has() {
        let b = parse_build(EDSY_SLEF).expect("build");
        let mut fleet = Fleet::default();
        fleet.stored_modules.push(StoredModuleInfo { name: "int_hyperdrive_size6_class5".into(), system: Some("Shinrarta Dezhra".into()), ..Default::default() });
        fleet.stored_modules.push(StoredModuleInfo { name: "hpt_beamlaser_gimbal_large".into(), system: Some("Sol".into()), ..Default::default() });
        let list = needs(&b, &fleet);
        assert_eq!(list[0].group, Group::Hull);
        assert!(list[0].buy, "no Anaconda in the fleet");
        let row = |sym: &str| list.iter().find(|n| n.symbol == sym).expect(sym);
        let fsd = row("int_hyperdrive_size6_class5");
        assert_eq!((fsd.buy, fsd.note.as_deref()), (false, Some("In your storage in Shinrarta Dezhra")));
        // Two beam lasers fitted, one in storage: still buy.
        let beams = row("hpt_beamlaser_gimbal_large");
        assert_eq!((beams.count, beams.buy, beams.note.as_deref()), (2, true, Some("1 of 2 in your storage in Sol")));
        assert_eq!(row("hpt_multicannon_gimbal_huge").engineering, ["G5 Overcharged Weapon, Auto Loader"]);
        // Groups in game order: hull, core, optional, hardpoints, utility.
        let groups: Vec<Group> = list.iter().map(|n| n.group).collect();
        assert!(groups.windows(2).all(|w| w[0] <= w[1]), "{groups:?}");
        // A hull they own already starts unticked.
        fleet.ships.insert(7, nexus_model::fleet::Ship::new(7, "anaconda"));
        assert!(!needs(&b, &fleet)[0].buy);
    }

    #[test]
    fn default_bulkheads_and_unlocks() {
        assert!(comes_with_hull("anaconda_armour_grade1"));
        assert!(!comes_with_hull("anaconda_armour_grade3"));
        // The Anaconda Mk II comes with plain alloy; its grade 1 is a bought ablative alloy.
        assert!(comes_with_hull("anacondamkii_armour_grade1_default"));
        assert!(!comes_with_hull("anacondamkii_armour_grade1"));
        let b = parse_build(r#"{"Ship":"anacondamkii","Modules":[{"Slot":"Armour","Item":"anacondamkii_armour_grade1_default"},{"Slot":"Slot01_Size7","Item":"int_shieldgenerator_size7_class5_strong"}]}"#).expect("build");
        let list = needs(&b, &Fleet::default());
        // ARX-only comes from the stations' prices, not FDevIDs (it lags behind the game).
        assert_eq!(list[0].unlock, None);
        assert_eq!((list[1].buy, list[1].note.as_deref()), (false, Some("Comes with every new Anaconda Mk II")));
        assert_eq!(list[2].unlock, Some(sku::Unlock::Powerplay), "prismatic shields");
    }

    #[test]
    fn stations_are_checked_against_the_exact_symbols() {
        let w = vec![
            wanted("int_hyperdrive_size6_class5", 1, false),
            wanted("hpt_multicannon_gimbal_huge", 1, false),
            wanted("int_powerdistributor_size5_class5", 1, false),
            wanted("anaconda", 1, true),
        ];
        let o = offer(&station(json!({})), &w);
        // The fixed multi-cannon doesn't count for a gimballed one; the MercGear-named row does.
        assert!(o.sells(0) && !o.sells(1) && o.sells(2) && o.sells(3));
        assert_eq!(o.cost(&[0, 2, 3], &w), (16_179_531 + 146_969_450, 1));
        // Selling the hull, its data is as old as the older of outfitting and shipyard.
        assert_eq!(o.updated, nexus_spansh::parse_timestamp("2026-11-02T10:00:00Z"));
        assert_eq!((o.pos, o.state.as_deref()), (Some([10.0, 0.0, 5.0]), None));
        // Damage is flagged; a colony station still under construction keeps its outfitting.
        assert_eq!(offer(&station(json!({"state": "UnderRepairs"})), &w).state.as_deref(), Some("Under repairs"));
        assert_eq!(offer(&station(json!({"state": "Construction"})), &w).state, None);
    }

    #[test]
    fn pads_and_closed_carriers() {
        let outpost = station(json!({"large_pads": 0, "has_large_pad": false}));
        assert!(!has_pad(&outpost, 'L') && has_pad(&outpost, 'M') && has_pad(&outpost, 'S'));
        let unknown = station(json!({"small_pads": null, "medium_pads": null, "large_pads": null, "has_large_pad": null}));
        assert!(has_pad(&unknown, 'L'), "no pad data: kept");
        let open = station(json!({"type": "Drake-Class Carrier", "carrier_docking_access": "All"}));
        let friends = station(json!({"type": "Drake-Class Carrier", "carrier_docking_access": "Squadron Friends"}));
        assert!(!closed_carrier(&open) && closed_carrier(&friends) && !closed_carrier(&station(json!({}))));
    }

    #[test]
    fn filters_use_every_name_and_fdevids_class() {
        let w = [wanted("hpt_multicannon_gimbal_huge", 1, false), wanted("hpt_multicannon_fixed_huge", 1, false), wanted("int_powerdistributor_size5_class5", 2, false), wanted("krait_mkii", 1, true), wanted("int_brandnewthing_size3_class5", 1, false)];
        let (modules, hulls) = filters(w.iter());
        // Fixed and gimballed share one filter; the unknown module gets none.
        assert_eq!(
            modules,
            [
                ModuleFilter::new(vec!["Multi-Cannon".into()], Some(4), Some('A')),
                ModuleFilter::new(vec!["Power Distributor".into(), "Balanced Power Distributor".into()], Some(5), Some('A'))
            ]
        );
        assert_eq!(hulls, ["Krait MkII"]);
        let (cabin, _) = filters([wanted("int_mkii_passengercabin_size6_class2", 1, false)].iter());
        assert_eq!(cabin[0].class, Some(5), "FDevIDs' class, as Spansh has it");
    }

    #[test]
    fn the_first_stop_has_the_most_then_is_nearest() {
        let mk = |d: f64, n: usize| Offer { distance_ly: d, sells: (0..n).map(|i| (i, None)).collect(), ..offer(&station(json!({})), &[]) };
        let best = first_stop(vec![mk(5.0, 2), mk(30.0, 4), mk(12.0, 4), mk(1.0, 0)]).expect("first");
        assert_eq!((best.distance_ly, best.sells.len()), (12.0, 4));
        assert!(first_stop(vec![mk(1.0, 0)]).is_none());
    }

    /// Live: `cargo test -p nexus-app shopping -- --ignored --nocapture` (needs the internet).
    #[test]
    #[ignore]
    fn live_anaconda_build_near_sol() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
        let b = parse_build(EDSY_SLEF).expect("build");
        let list = needs(&b, &Fleet::default());
        let q = Search::new(&list, &b.ship, Reference::System("Sol".into()), true, None);
        let started = std::time::Instant::now();
        let out = rt.block_on(find(&Spansh::new(), &q)).expect("search");
        println!("{:?} in {:.1} s", out.stats, started.elapsed().as_secs_f64());
        for o in &out.one_stop {
            println!("{:>6.1} ly  {:<32} {:<28} {:?}", o.distance_ly, o.station, o.system, o.cost(&(0..q.wanted.len()).collect::<Vec<_>>(), &q.wanted));
        }
        assert!(!out.one_stop.is_empty(), "big stations near Sol sell a plain Anaconda build");
        assert!(out.one_stop.windows(2).all(|w| w[0].distance_ly <= w[1].distance_ly));
    }

    /// Live: an Anaconda Mk II (ARX early access in November 2026, so no credit price) with its
    /// own ablative bulkheads. `cargo test -p nexus-app shopping -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_anaconda_mk_ii_near_sol() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
        let b = parse_build(r#"{"Ship":"anacondamkii","Modules":[{"Slot":"Armour","Item":"anacondamkii_armour_grade3"},{"Slot":"FrameShiftDrive","Item":"int_hyperdrive_overcharge_size6_class5"},{"Slot":"Slot01_Size7","Item":"int_shieldgenerator_size7_class5"},{"Slot":"HugeHardpoint1","Item":"hpt_multicannon_gimbal_huge"},{"Slot":"TinyHardpoint1","Item":"hpt_shieldbooster_size0_class5"}]}"#).expect("build");
        let q = Search::new(&needs(&b, &Fleet::default()), &b.ship, Reference::System("Sol".into()), true, None);
        assert_eq!(q.pad, 'L');
        let out = rt.block_on(find(&Spansh::new(), &q)).expect("search");
        println!("{:?}", out.stats);
        let hull = q.wanted.iter().position(|w| w.hull).expect("hull wanted");
        let offers: Vec<&Offer> = out.one_stop.iter().chain(out.first.iter()).chain(out.second.iter()).collect();
        for o in &offers {
            println!("{:>6.1} ly  {:<32} {:<28} sells {} of {}, hull price {:?}", o.distance_ly, o.station, o.system, o.sells.len(), q.wanted.len(), o.sells.iter().find(|(i, _)| *i == hull).map(|(_, p)| p));
        }
        assert!(!offers.is_empty());
    }
}
