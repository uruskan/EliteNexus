//! Inara sync (`POST https://inara.cz/inapi/v1/`), see `docs/research/inara.md`.
//!
//! The journal → Inara mapping follows EDMC's inara plugin, minus its known bugs (rank stage
//! arrays, `PowerplayLeave` with 0, `isTaxiDropShip`, empty `RenameSuitLoadout`). "set" events
//! are coalesced in the queue; at most one POST per 35 s per key keeps us far from the
//! "2 requests a minute" limit whose violation locks a user out for an hour.

use crate::context::UploadContext;
use crate::json::{Obj, fnv1a64, get_array, get_bool, get_f64, get_i64, get_nonempty, get_str, get_u64, strip_symbol};
use crate::session::Session;
use crate::transport::HttpResponse;
use nexus_journal::{Companion, CompanionKind, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashSet};

pub const API_URL: &str = "https://inara.cz/inapi/v1/";

/// One Inara API event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InaraEvent {
    #[serde(rename = "eventName")]
    pub name: String,
    #[serde(rename = "eventTimestamp")]
    pub timestamp: String,
    #[serde(rename = "eventData")]
    pub data: Value,
}

impl InaraEvent {
    fn new(name: &str, ts: &str, data: Value) -> Self {
        InaraEvent { name: name.to_string(), timestamp: ts.to_string(), data }
    }

    /// Unsent events with the same key are superseded by this one.
    pub fn coalesce_key(&self) -> Option<String> {
        let id = |k: &str| self.data.get(k).map(Value::to_string).unwrap_or_default();
        Some(match self.name.as_str() {
            "setCommanderShip" => format!("ship:{}", id("shipGameID")),
            "setCommanderShipLoadout" => format!("loadout:{}", id("shipGameID")),
            "setCommanderStorageModules" => "storage".into(),
            "setCommanderInventoryCargo" => "cargo".into(),
            "setCommanderInventoryMaterials" => "materials".into(),
            "resetCommanderInventory" => "locker-reset".into(),
            "setCommanderInventory" => "locker".into(),
            "setCommunityGoal" => format!("cg:{}", id("communitygoalGameID")),
            "setCommanderCommunityGoalProgress" => format!("cgp:{}", id("communitygoalGameID")),
            "setCommanderCredits" => "credits".into(),
            "setCommanderGameStatistics" => "statistics".into(),
            "setCommanderTravelLocation" => "location".into(),
            "setCommanderSuitLoadout" => format!("suit:{}", id("loadoutGameID")),
            _ => return None,
        })
    }
}

/// The "set" events that describe the commander as they are now: ship and loadout, stored modules, cargo, materials,
/// the ship locker, credits, statistics, location, suit loadouts, ranks, reputation and community goals. A profile
/// sync (`Uplink::sync_inara_profile`: at start, when sync is switched on, when it resumes after a pause) sends only
/// these. The other "set" events record something done, such as a mission handed in, failed or abandoned, or a ship
/// transfer: they go out live, never in a sync, which would send what happened before sync was on or while it was
/// paused.
pub const STATE_EVENTS: [&str; 17] = [
    "setCommanderShip",
    "setCommanderShipLoadout",
    "setCommanderStorageModules",
    "setCommanderInventoryCargo",
    "setCommanderInventoryMaterials",
    "setCommanderInventory",
    "setCommanderCredits",
    "setCommanderGameStatistics",
    "setCommanderTravelLocation",
    "setCommanderSuitLoadout",
    "setCommanderRankPilot",
    "setCommanderRankEngineer",
    "setCommanderRankPower",
    "setCommanderReputationMajorFaction",
    "setCommanderReputationMinorFaction",
    "setCommunityGoal",
    "setCommanderCommunityGoalProgress",
];

/// An event that describes the commander's present state (`STATE_EVENTS`), which a profile sync may send.
pub fn is_state(name: &str) -> bool {
    STATE_EVENTS.contains(&name)
}

/// One queued Inara event. The API key is never written to disk, only its fingerprint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InaraItem {
    pub cmdr: String,
    pub fid: String,
    pub key: u64,
    pub event: InaraEvent,
    pub event_time: Timestamp,
    pub queued_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Taxi {
    Shuttle,
    Dropship,
}

#[derive(Debug, Clone, Default)]
struct Ship {
    kind: String,
    id: u64,
    name: Option<String>,
    ident: Option<String>,
}

/// Journal → Inara state machine.
#[derive(Debug, Default)]
pub struct InaraMapper {
    session_start_pending: bool,
    ranks: BTreeMap<String, (Option<i64>, Option<f64>)>,
    reputation: Vec<(String, f64)>,
    engineers: BTreeMap<String, Value>,
    ship: Option<Ship>,
    loadout: Option<Value>,
    ship_extra: Obj,
    last_loadout: Option<u64>,
    credits: Option<(i64, i64)>,
    power: Option<(String, i64)>,
    suppress_dock_at: Option<u64>,
    undocked_since_travel: bool,
    taxi: Option<Taxi>,
    cargo_sent: Option<u64>,
    materials: Option<BTreeMap<String, i64>>,
    materials_sent: Option<u64>,
    storage_sent: Option<u64>,
    fleet_sent: Option<u64>,
    locker_sent: Option<u64>,
    suits_sent: BTreeMap<u64, u64>,
    friends_sent: HashSet<String>,
    expect: Option<(CompanionKind, Timestamp)>,
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

fn hash_of(v: &Value) -> u64 {
    fnv1a64(v.to_string().as_bytes())
}

/// Copies `src[k]` into `dst[as_key]` when present (and not an empty string).
fn put(dst: &mut Map<String, Value>, as_key: &str, v: Option<Value>) {
    if let Some(v) = v
        && !v.is_null()
        && v.as_str() != Some("")
    {
        dst.insert(as_key.to_string(), v);
    }
}

fn val(ev: &Obj, k: &str) -> Option<Value> {
    ev.get(k).cloned()
}

/// Human-readable form of a possibly-localised journal string (`X_Localised` if present).
fn readable(ev: &Obj, k: &str) -> Option<String> {
    get_nonempty(ev, &format!("{k}_Localised")).or_else(|| get_nonempty(ev, k)).map(str::to_string)
}

fn current_system(session: &Session, ctx: &UploadContext) -> Option<String> {
    session.location(ctx).map(|l| l.system_name).or_else(|| Some(ctx.system_name.clone()).filter(|s| !s.is_empty()))
}

impl InaraMapper {
    pub fn new() -> Self {
        Self::default()
    }

    fn ship_fields(&self, d: &mut Map<String, Value>) {
        if let Some(s) = &self.ship {
            d.insert("shipType".into(), json!(s.kind));
            d.insert("shipGameID".into(), json!(s.id));
        }
    }

    fn taxi_fields(&self, ev: &Obj, d: &mut Map<String, Value>) {
        if get_bool(ev, "Taxi") == Some(true) {
            match self.taxi {
                Some(Taxi::Dropship) => {
                    d.insert("isTaxiDropship".into(), json!(true));
                }
                _ => {
                    d.insert("isTaxiShuttle".into(), json!(true));
                }
            }
        } else {
            self.ship_fields(d);
        }
    }

    fn current_ship_event(&self, ts: &str) -> Option<InaraEvent> {
        let s = self.ship.as_ref()?;
        let mut d = Map::new();
        d.insert("shipType".into(), json!(s.kind));
        d.insert("shipGameID".into(), json!(s.id));
        put(&mut d, "shipName", s.name.clone().map(Value::from));
        put(&mut d, "shipIdent", s.ident.clone().map(Value::from));
        d.insert("isCurrentShip".into(), json!(true));
        for (k, v) in &self.ship_extra {
            d.insert(k.clone(), v.clone());
        }
        Some(InaraEvent::new("setCommanderShip", ts, Value::Object(d)))
    }

    fn minor_factions(ev: &Obj, ts: &str, out: &mut Vec<InaraEvent>) {
        let reps: Vec<Value> = get_array(ev, "Factions")
            .into_iter()
            .flatten()
            .filter_map(|f| {
                let f = f.as_object()?;
                let name = get_nonempty(f, "Name")?;
                let rep = get_f64(f, "MyReputation")?;
                Some(json!({"minorfactionName": name, "minorfactionReputation": rep / 100.0}))
            })
            .collect();
        if !reps.is_empty() {
            out.push(InaraEvent::new("setCommanderReputationMinorFaction", ts, Value::Array(reps)));
        }
    }

    fn materials_event(&mut self, ts: &str, out: &mut Vec<InaraEvent>) {
        let Some(m) = &self.materials else { return };
        let list: Vec<Value> =
            m.iter().filter(|(_, c)| **c > 0).map(|(n, c)| json!({"itemName": n, "itemCount": c})).collect();
        let v = Value::Array(list);
        let h = hash_of(&v);
        if self.materials_sent != Some(h) {
            self.materials_sent = Some(h);
            out.push(InaraEvent::new("setCommanderInventoryMaterials", ts, v));
        }
    }

    fn adjust_material(&mut self, name: &str, delta: i64) -> bool {
        let Some(m) = self.materials.as_mut() else { return false };
        let key = lower(strip_symbol(name));
        if delta < 0 && !m.contains_key(&key) {
            return false; // not a ship material (commodity, microresource, …)
        }
        let e = m.entry(key).or_insert(0);
        *e = (*e + delta).max(0);
        true
    }

    fn cargo_event(&mut self, inventory: &[Value], ts: &str, out: &mut Vec<InaraEvent>) {
        let mut totals: BTreeMap<String, i64> = BTreeMap::new();
        for it in inventory {
            let Some(it) = it.as_object() else { continue };
            let (Some(name), Some(count)) = (get_str(it, "Name"), get_i64(it, "Count")) else { continue };
            *totals.entry(lower(strip_symbol(name))).or_default() += count;
        }
        let v = Value::Array(totals.iter().map(|(n, c)| json!({"itemName": n, "itemCount": c})).collect());
        let h = hash_of(&v);
        if self.cargo_sent != Some(h) {
            self.cargo_sent = Some(h);
            out.push(InaraEvent::new("setCommanderInventoryCargo", ts, v));
        }
    }

    fn locker_event(&mut self, ev: &Obj, ts: &str, out: &mut Vec<InaraEvent>) {
        let mut items = Vec::new();
        for (key, item_type) in [("Items", "Items"), ("Components", "Components"), ("Consumables", "Consumables"), ("Data", "Data")] {
            for it in get_array(ev, key).into_iter().flatten() {
                let Some(it) = it.as_object() else { continue };
                let (Some(name), Some(count)) = (get_nonempty(it, "Name"), get_i64(it, "Count")) else { continue };
                items.push(json!({"itemName": name, "itemCount": count, "itemType": item_type}));
            }
        }
        let v = Value::Array(items);
        let h = hash_of(&v);
        if self.locker_sent == Some(h) {
            return;
        }
        self.locker_sent = Some(h);
        let reset = json!([{"itemType": "Items"}, {"itemType": "Components"}, {"itemType": "Data"}, {"itemType": "Consumables"}]);
        out.push(InaraEvent::new("resetCommanderInventory", ts, reset));
        out.push(InaraEvent::new("setCommanderInventory", ts, v));
    }

    fn suit_loadout(ev: &Obj) -> Option<Value> {
        let mut d = Map::new();
        d.insert("loadoutGameID".into(), json!(get_u64(ev, "LoadoutID")?));
        put(&mut d, "loadoutName", val(ev, "LoadoutName"));
        put(&mut d, "suitGameID", val(ev, "SuitID"));
        put(&mut d, "suitType", val(ev, "SuitName"));
        put(&mut d, "suitMods", val(ev, "SuitMods"));
        if let Some(mods) = get_array(ev, "Modules") {
            let slots: Vec<Value> = mods
                .iter()
                .filter_map(Value::as_object)
                .map(|m| {
                    let mut s = Map::new();
                    put(&mut s, "slotName", val(m, "SlotName"));
                    put(&mut s, "itemName", val(m, "ModuleName"));
                    put(&mut s, "itemClass", val(m, "Class"));
                    put(&mut s, "itemGameID", val(m, "SuitModuleID"));
                    let eng: Vec<Value> = get_array(m, "WeaponMods")
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(|b| json!({"blueprintName": b}))
                        .collect();
                    if !eng.is_empty() {
                        s.insert("engineering".into(), Value::Array(eng));
                    }
                    Value::Object(s)
                })
                .collect();
            d.insert("suitLoadout".into(), Value::Array(slots));
        }
        Some(Value::Object(d))
    }

    fn loadout_modules(ev: &Obj) -> Value {
        let modules: Vec<Value> = get_array(ev, "Modules")
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .map(|m| {
                let mut d = Map::new();
                put(&mut d, "slotName", val(m, "Slot"));
                put(&mut d, "itemName", val(m, "Item"));
                put(&mut d, "itemHealth", val(m, "Health"));
                put(&mut d, "isOn", val(m, "On"));
                put(&mut d, "itemPriority", val(m, "Priority"));
                put(&mut d, "itemAmmoClip", val(m, "AmmoInClip"));
                put(&mut d, "itemAmmoHopper", val(m, "AmmoInHopper"));
                put(&mut d, "itemValue", val(m, "Value"));
                put(&mut d, "isHot", val(m, "Hot"));
                if let Some(e) = m.get("Engineering").and_then(Value::as_object) {
                    let mut eng = Map::new();
                    put(&mut eng, "blueprintName", val(e, "BlueprintName"));
                    put(&mut eng, "blueprintLevel", val(e, "Level"));
                    put(&mut eng, "blueprintQuality", val(e, "Quality"));
                    put(&mut eng, "experimentalEffect", val(e, "ExperimentalEffect"));
                    let mods: Vec<Value> = get_array(e, "Modifiers")
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_object)
                        .filter_map(|x| {
                            let label = get_nonempty(x, "Label")?;
                            if let Some(s) = get_str(x, "ValueStr") {
                                return Some(json!({"name": label, "value": s}));
                            }
                            let mut md = Map::new();
                            md.insert("name".into(), json!(label));
                            put(&mut md, "value", val(x, "Value"));
                            put(&mut md, "originalValue", val(x, "OriginalValue"));
                            if let Some(l) = get_i64(x, "LessIsGood") {
                                md.insert("lessIsGood".into(), json!(l != 0));
                            }
                            Some(Value::Object(md))
                        })
                        .collect();
                    if !mods.is_empty() {
                        eng.insert("modifiers".into(), Value::Array(mods));
                    }
                    d.insert("engineering".into(), Value::Object(eng));
                }
                Value::Object(d)
            })
            .collect();
        Value::Array(modules)
    }

    /// Maps one live journal event. Call for every event (state is tracked even when nothing
    /// is produced); `session` must already reflect the event.
    pub fn on_event(&mut self, name: &str, ev: &Obj, session: &Session, ctx: &UploadContext) -> Vec<InaraEvent> {
        let mut out = Vec::new();
        let Some(ts) = get_str(ev, "timestamp").map(str::to_string) else { return out };
        let ts = ts.as_str();
        let system = || current_system(session, ctx);
        match name {
            "LoadGame" => {
                self.session_start_pending = true;
                self.cargo_sent = None;
                self.materials_sent = None;
                self.locker_sent = None;
                self.storage_sent = None;
                self.fleet_sent = None;
                self.friends_sent.clear();
                self.suppress_dock_at = None;
                self.undocked_since_travel = false;
                self.taxi = None;
                if let (Some(c), Some(l)) = (get_i64(ev, "Credits"), get_i64(ev, "Loan")) {
                    self.credits = Some((c, l));
                }
                if let (Some(kind), Some(id)) = (get_nonempty(ev, "Ship"), get_u64(ev, "ShipID"))
                    && !lower(kind).contains("suit")
                {
                    self.ship = Some(Ship {
                        kind: lower(kind),
                        id,
                        name: get_nonempty(ev, "ShipName").map(str::to_string),
                        ident: get_nonempty(ev, "ShipIdent").map(str::to_string),
                    });
                }
            }
            "Rank" | "Progress" => {
                for (k, v) in ev {
                    if matches!(k.as_str(), "timestamp" | "event") {
                        continue;
                    }
                    let Some(n) = v.as_i64() else { continue };
                    let e = self.ranks.entry(lower(k)).or_default();
                    if name == "Rank" {
                        e.0 = Some(n);
                    } else {
                        e.1 = Some(n as f64);
                    }
                }
                if name == "Progress" {
                    let list: Vec<Value> = self
                        .ranks
                        .iter()
                        .filter_map(|(k, (r, p))| {
                            let mut d = Map::new();
                            d.insert("rankName".into(), json!(k));
                            put(&mut d, "rankValue", r.map(Value::from));
                            put(&mut d, "rankProgress", p.map(|p| json!((p / 100.0).clamp(0.0, 1.0))));
                            (d.len() > 1).then_some(Value::Object(d))
                        })
                        .collect();
                    if !list.is_empty() {
                        out.push(InaraEvent::new("setCommanderRankPilot", ts, Value::Array(list)));
                    }
                }
            }
            "Promotion" => {
                let mut list = Vec::new();
                for (k, v) in ev {
                    let Some(n) = v.as_i64().filter(|_| !matches!(k.as_str(), "timestamp" | "event")) else { continue };
                    self.ranks.insert(lower(k), (Some(n), Some(0.0)));
                    list.push(json!({"rankName": lower(k), "rankValue": n, "rankProgress": 0}));
                }
                if !list.is_empty() {
                    out.push(InaraEvent::new("setCommanderRankPilot", ts, Value::Array(list)));
                }
            }
            "Reputation" => {
                self.reputation = ["Empire", "Federation", "Independent", "Alliance"]
                    .iter()
                    .filter_map(|k| get_f64(ev, k).map(|v| (lower(k), v / 100.0)))
                    .collect();
            }
            "EngineerProgress" => {
                let engineer_data = |e: &Obj| -> Option<Value> {
                    let n = get_nonempty(e, "Engineer")?;
                    let stage = get_str(e, "Progress").filter(|s| matches!(*s, "Invited" | "Acquainted" | "Unlocked" | "Barred"))?;
                    let mut d = Map::new();
                    d.insert("engineerName".into(), json!(n));
                    d.insert("rankStage".into(), json!(stage));
                    if stage == "Unlocked" {
                        put(&mut d, "rankValue", val(e, "Rank"));
                    }
                    Some(Value::Object(d))
                };
                if let Some(list) = get_array(ev, "Engineers") {
                    for e in list.iter().filter_map(Value::as_object) {
                        if let (Some(n), Some(d)) = (get_nonempty(e, "Engineer"), engineer_data(e)) {
                            self.engineers.insert(n.to_string(), d);
                        }
                    }
                } else if let (Some(n), Some(d)) = (get_nonempty(ev, "Engineer"), engineer_data(ev)) {
                    self.engineers.insert(n.to_string(), d.clone());
                    out.push(InaraEvent::new("setCommanderRankEngineer", ts, d));
                }
            }
            "Cargo" => {
                if get_str(ev, "Vessel") == Some("Ship") {
                    if self.session_start_pending {
                        self.session_start_pending = false;
                        if !self.reputation.is_empty() {
                            let list: Vec<Value> = self
                                .reputation
                                .iter()
                                .map(|(k, v)| json!({"majorfactionName": k, "majorfactionReputation": v}))
                                .collect();
                            out.push(InaraEvent::new("setCommanderReputationMajorFaction", ts, Value::Array(list)));
                        }
                        if !self.engineers.is_empty() {
                            let list: Vec<Value> = self.engineers.values().cloned().collect();
                            out.push(InaraEvent::new("setCommanderRankEngineer", ts, Value::Array(list)));
                        }
                        if let Some(e) = self.current_ship_event(ts) {
                            out.push(e);
                            if let (Some(s), Some(l)) = (&self.ship, &self.loadout) {
                                out.push(InaraEvent::new(
                                    "setCommanderShipLoadout",
                                    ts,
                                    json!({"shipType": s.kind, "shipGameID": s.id, "shipLoadout": l}),
                                ));
                            }
                        }
                    }
                    match get_array(ev, "Inventory") {
                        Some(inv) => self.cargo_event(inv, ts, &mut out),
                        None => self.expect = Some((CompanionKind::Cargo, Timestamp::parse(ts).unwrap_or_default())),
                    }
                }
            }
            "Materials" => {
                let mut m = BTreeMap::new();
                for k in ["Raw", "Manufactured", "Encoded"] {
                    for it in get_array(ev, k).into_iter().flatten().filter_map(Value::as_object) {
                        if let (Some(n), Some(c)) = (get_str(it, "Name"), get_i64(it, "Count")) {
                            m.insert(lower(n), c);
                        }
                    }
                }
                self.materials = Some(m);
                self.materials_event(ts, &mut out);
            }
            "MaterialCollected" | "MaterialDiscarded" => {
                if let (Some(n), Some(c)) = (get_str(ev, "Name"), get_i64(ev, "Count")) {
                    let sign = if name == "MaterialCollected" { 1 } else { -1 };
                    self.adjust_material(n, sign * c);
                    self.materials_event(ts, &mut out);
                }
            }
            "MaterialTrade" => {
                let paid = ev.get("Paid").and_then(Value::as_object);
                let recv = ev.get("Received").and_then(Value::as_object);
                if let Some((n, q)) = paid.and_then(|p| Some((get_str(p, "Material")?, get_i64(p, "Quantity")?))) {
                    self.adjust_material(n, -q);
                }
                if let Some((n, q)) = recv.and_then(|p| Some((get_str(p, "Material")?, get_i64(p, "Quantity")?))) {
                    self.adjust_material(n, q);
                }
                self.materials_event(ts, &mut out);
            }
            "EngineerCraft" | "Synthesis" | "TechnologyBroker" => {
                let key = match name {
                    "EngineerCraft" => "Ingredients",
                    _ => "Materials",
                };
                for it in get_array(ev, key).into_iter().flatten().filter_map(Value::as_object) {
                    if let (Some(n), Some(c)) = (get_str(it, "Name"), get_i64(it, "Count")) {
                        self.adjust_material(n, -c);
                    }
                }
                self.materials_event(ts, &mut out);
            }
            "ScientificResearch" | "EngineerContribution" => {
                let (n, c) = if name == "ScientificResearch" {
                    (get_str(ev, "Name"), get_i64(ev, "Count"))
                } else if get_str(ev, "Type") == Some("Materials") {
                    (get_str(ev, "Material"), get_i64(ev, "Quantity"))
                } else {
                    (None, None)
                };
                if let (Some(n), Some(c)) = (n, c) {
                    self.adjust_material(n, -c);
                    self.materials_event(ts, &mut out);
                }
            }
            "Statistics" => {
                if let Some((credits, loan)) = self.credits {
                    let mut d = Map::new();
                    d.insert("commanderCredits".into(), json!(credits));
                    d.insert("commanderLoan".into(), json!(loan));
                    put(&mut d, "commanderAssets", ev.get("Bank_Account").and_then(|b| b.get("Current_Wealth")).cloned());
                    out.push(InaraEvent::new("setCommanderCredits", ts, Value::Object(d)));
                }
                let mut stats = ev.clone();
                stats.remove("timestamp");
                stats.remove("event");
                out.push(InaraEvent::new("setCommanderGameStatistics", ts, Value::Object(stats)));
            }
            "Docked" => {
                let market = get_u64(ev, "MarketID");
                let suppressed = self.suppress_dock_at.is_some() && self.suppress_dock_at == market;
                let redock = self.undocked_since_travel;
                self.suppress_dock_at = None;
                self.undocked_since_travel = false;
                if !suppressed && !redock {
                    let mut d = Map::new();
                    put(&mut d, "starsystemName", val(ev, "StarSystem"));
                    put(&mut d, "stationName", val(ev, "StationName"));
                    put(&mut d, "marketID", val(ev, "MarketID"));
                    self.taxi_fields(ev, &mut d);
                    if d.contains_key("starsystemName") && d.contains_key("stationName") {
                        out.push(InaraEvent::new("addCommanderTravelDock", ts, Value::Object(d)));
                    }
                }
                if get_bool(ev, "Taxi") != Some(true) {
                    self.taxi = None;
                }
            }
            "Undocked" => {
                self.undocked_since_travel = true;
                self.suppress_dock_at = None;
            }
            "SupercruiseEntry" => self.undocked_since_travel = false,
            "SupercruiseExit" => {
                let mut d = Map::new();
                put(&mut d, "starsystemName", val(ev, "StarSystem"));
                if get_str(ev, "BodyType") == Some("Planet") {
                    put(&mut d, "starsystemBodyName", val(ev, "Body"));
                }
                if d.contains_key("starsystemName") {
                    out.push(InaraEvent::new("setCommanderTravelLocation", ts, Value::Object(d)));
                }
            }
            "ApproachSettlement" => {
                if let Some(sys) = system() {
                    let mut d = Map::new();
                    d.insert("starsystemName".into(), json!(sys));
                    put(&mut d, "stationName", val(ev, "Name").filter(|_| get_str(ev, "Name").is_some_and(|n| !n.starts_with('$'))));
                    put(&mut d, "marketID", val(ev, "MarketID"));
                    put(&mut d, "starsystemBodyName", val(ev, "BodyName"));
                    if let (Some(lat), Some(lon)) = (get_f64(ev, "Latitude"), get_f64(ev, "Longitude")) {
                        d.insert("starsystemBodyCoords".into(), json!([lat, lon]));
                    }
                    out.push(InaraEvent::new("setCommanderTravelLocation", ts, Value::Object(d)));
                }
            }
            "FSDJump" => {
                self.undocked_since_travel = false;
                self.suppress_dock_at = None;
                let mut d = Map::new();
                put(&mut d, "starsystemName", val(ev, "StarSystem"));
                put(&mut d, "starsystemCoords", val(ev, "StarPos"));
                put(&mut d, "jumpDistance", val(ev, "JumpDist"));
                self.taxi_fields(ev, &mut d);
                if d.contains_key("starsystemName") {
                    out.push(InaraEvent::new("addCommanderTravelFSDJump", ts, Value::Object(d)));
                }
                Self::minor_factions(ev, ts, &mut out);
            }
            "CarrierJump" => {
                self.undocked_since_travel = false;
                let mut d = Map::new();
                put(&mut d, "starsystemName", val(ev, "StarSystem"));
                put(&mut d, "starsystemCoords", val(ev, "StarPos"));
                if get_bool(ev, "Docked") == Some(true) {
                    put(&mut d, "stationName", val(ev, "StationName"));
                    put(&mut d, "marketID", val(ev, "MarketID"));
                    self.suppress_dock_at = get_u64(ev, "MarketID");
                }
                self.ship_fields(&mut d);
                if d.contains_key("starsystemName") {
                    out.push(InaraEvent::new("addCommanderTravelCarrierJump", ts, Value::Object(d)));
                }
                Self::minor_factions(ev, ts, &mut out);
            }
            "Location" => {
                let mut d = Map::new();
                put(&mut d, "starsystemName", val(ev, "StarSystem"));
                put(&mut d, "starsystemCoords", val(ev, "StarPos"));
                let docked = get_bool(ev, "Docked") == Some(true);
                if docked {
                    put(&mut d, "stationName", val(ev, "StationName"));
                    put(&mut d, "marketID", val(ev, "MarketID"));
                    // A session that starts docked must not log a dock (EDMC).
                    self.suppress_dock_at = get_u64(ev, "MarketID");
                }
                if get_str(ev, "BodyType") == Some("Planet") {
                    put(&mut d, "starsystemBodyName", val(ev, "Body"));
                }
                if let (Some(lat), Some(lon)) = (get_f64(ev, "Latitude"), get_f64(ev, "Longitude")) {
                    d.insert("starsystemBodyCoords".into(), json!([lat, lon]));
                }
                if d.contains_key("starsystemName") {
                    out.push(InaraEvent::new("setCommanderTravelLocation", ts, Value::Object(d)));
                }
                Self::minor_factions(ev, ts, &mut out);
            }
            "BookTaxi" => self.taxi = Some(Taxi::Shuttle),
            "BookDropship" => self.taxi = Some(Taxi::Dropship),
            "CancelTaxi" | "CancelDropship" => self.taxi = None,
            "DropshipDeploy" => {
                let mut d = Map::new();
                put(&mut d, "starsystemName", val(ev, "StarSystem"));
                put(&mut d, "starsystemBodyName", val(ev, "Body"));
                d.insert("isTaxiDropship".into(), json!(true));
                if d.contains_key("starsystemName") && d.contains_key("starsystemBodyName") {
                    out.push(InaraEvent::new("addCommanderTravelLand", ts, Value::Object(d)));
                }
                self.taxi = None;
            }
            "Touchdown" => {
                if get_bool(ev, "PlayerControlled") == Some(true) && get_bool(ev, "OnPlanet") != Some(false) {
                    let mut d = Map::new();
                    put(&mut d, "starsystemName", val(ev, "StarSystem"));
                    put(&mut d, "starsystemBodyName", val(ev, "Body"));
                    if let (Some(lat), Some(lon)) = (get_f64(ev, "Latitude"), get_f64(ev, "Longitude")) {
                        d.insert("starsystemBodyCoords".into(), json!([lat, lon]));
                    }
                    if get_bool(ev, "Taxi") != Some(true) {
                        self.ship_fields(&mut d);
                    }
                    d.insert("isTaxiShuttle".into(), json!(false));
                    d.insert("isTaxiDropship".into(), json!(false));
                    if d.contains_key("starsystemName") && d.contains_key("starsystemBodyName") {
                        out.push(InaraEvent::new("addCommanderTravelLand", ts, Value::Object(d)));
                    }
                }
            }
            "ShipyardNew" => {
                if let (Some(kind), Some(id)) = (get_nonempty(ev, "ShipType"), get_u64(ev, "NewShipID")) {
                    out.push(InaraEvent::new("addCommanderShip", ts, json!({"shipType": lower(kind), "shipGameID": id})));
                    self.ship = Some(Ship { kind: lower(kind), id, name: None, ident: None });
                    self.ship_extra.clear();
                    self.loadout = None;
                    self.suppress_dock_at = get_u64(ev, "MarketID").or(ctx.market_id);
                }
            }
            "ShipyardBuy" | "ShipyardSwap" | "ShipyardSell" | "SellShipOnRebuy" => {
                if let (Some(kind), Some(id)) = (get_nonempty(ev, "StoreOldShip"), get_u64(ev, "StoreShipID")) {
                    let mut d = Map::new();
                    d.insert("shipType".into(), json!(lower(kind)));
                    d.insert("shipGameID".into(), json!(id));
                    put(&mut d, "starsystemName", system().map(Value::from));
                    put(&mut d, "stationName", ctx.station_name.clone().map(Value::from));
                    put(&mut d, "marketID", val(ev, "MarketID").or(ctx.market_id.map(Value::from)));
                    out.push(InaraEvent::new("setCommanderShip", ts, Value::Object(d)));
                }
                let sold = match name {
                    "ShipyardBuy" | "ShipyardSwap" => get_nonempty(ev, "SellOldShip").zip(get_u64(ev, "SellShipID")),
                    "ShipyardSell" => get_nonempty(ev, "ShipType").zip(get_u64(ev, "SellShipID")),
                    _ => get_nonempty(ev, "ShipType").zip(get_u64(ev, "SellShipId").or_else(|| get_u64(ev, "SellShipID"))),
                };
                if let Some((kind, id)) = sold {
                    out.push(InaraEvent::new("delCommanderShip", ts, json!({"shipType": lower(kind), "shipGameID": id})));
                }
                if name == "ShipyardSwap"
                    && let (Some(kind), Some(id)) = (get_nonempty(ev, "ShipType"), get_u64(ev, "ShipID"))
                {
                    self.ship = Some(Ship { kind: lower(kind), id, name: None, ident: None });
                    self.ship_extra.clear();
                    self.loadout = None;
                    self.suppress_dock_at = get_u64(ev, "MarketID").or(ctx.market_id);
                }
            }
            "SetUserShipName" => {
                if let (Some(kind), Some(id)) = (get_nonempty(ev, "Ship"), get_u64(ev, "ShipID")) {
                    let s = Ship {
                        kind: lower(kind),
                        id,
                        name: get_nonempty(ev, "UserShipName").map(str::to_string),
                        ident: get_nonempty(ev, "UserShipId").map(str::to_string),
                    };
                    let mut d = Map::new();
                    d.insert("shipType".into(), json!(s.kind));
                    d.insert("shipGameID".into(), json!(s.id));
                    put(&mut d, "shipName", s.name.clone().map(Value::from));
                    put(&mut d, "shipIdent", s.ident.clone().map(Value::from));
                    d.insert("isCurrentShip".into(), json!(true));
                    out.push(InaraEvent::new("setCommanderShip", ts, Value::Object(d)));
                    self.ship = Some(s);
                }
            }
            "ShipyardTransfer" => {
                if let (Some(kind), Some(id), Some(sys), Some(station)) =
                    (get_nonempty(ev, "ShipType"), get_u64(ev, "ShipID"), system(), ctx.station_name.clone())
                {
                    let mut d = Map::new();
                    d.insert("shipType".into(), json!(lower(kind)));
                    d.insert("shipGameID".into(), json!(id));
                    d.insert("starsystemName".into(), json!(sys));
                    d.insert("stationName".into(), json!(station));
                    put(&mut d, "marketID", val(ev, "MarketID"));
                    put(&mut d, "transferTime", val(ev, "TransferTime"));
                    out.push(InaraEvent::new("setCommanderShipTransfer", ts, Value::Object(d)));
                }
            }
            "StoredShips" => {
                let mut content = ev.clone();
                content.remove("timestamp");
                let h = hash_of(&Value::Object(content));
                if self.fleet_sent != Some(h) {
                    self.fleet_sent = Some(h);
                    for s in get_array(ev, "ShipsHere").into_iter().flatten().filter_map(Value::as_object) {
                        let (Some(kind), Some(id)) = (get_nonempty(s, "ShipType"), get_u64(s, "ShipID")) else { continue };
                        let mut d = Map::new();
                        d.insert("shipType".into(), json!(lower(kind)));
                        d.insert("shipGameID".into(), json!(id));
                        put(&mut d, "shipName", val(s, "Name"));
                        put(&mut d, "isHot", val(s, "Hot"));
                        put(&mut d, "starsystemName", val(ev, "StarSystem"));
                        put(&mut d, "stationName", val(ev, "StationName"));
                        put(&mut d, "marketID", val(ev, "MarketID"));
                        out.push(InaraEvent::new("setCommanderShip", ts, Value::Object(d)));
                    }
                    for s in get_array(ev, "ShipsRemote").into_iter().flatten().filter_map(Value::as_object) {
                        let (Some(kind), Some(id)) = (get_nonempty(s, "ShipType"), get_u64(s, "ShipID")) else { continue };
                        let mut d = Map::new();
                        d.insert("shipType".into(), json!(lower(kind)));
                        d.insert("shipGameID".into(), json!(id));
                        put(&mut d, "shipName", val(s, "Name"));
                        put(&mut d, "isHot", val(s, "Hot"));
                        if get_bool(s, "InTransit") != Some(true) {
                            put(&mut d, "starsystemName", val(s, "StarSystem"));
                            put(&mut d, "marketID", val(s, "ShipMarketID"));
                        }
                        out.push(InaraEvent::new("setCommanderShip", ts, Value::Object(d)));
                    }
                }
            }
            "Loadout" => {
                if let (Some(kind), Some(id)) = (get_nonempty(ev, "Ship"), get_u64(ev, "ShipID")) {
                    self.ship = Some(Ship {
                        kind: lower(kind),
                        id,
                        name: get_nonempty(ev, "ShipName").map(str::to_string),
                        ident: get_nonempty(ev, "ShipIdent").map(str::to_string),
                    });
                    self.ship_extra.clear();
                    put(&mut self.ship_extra, "shipMaxJumpRange", val(ev, "MaxJumpRange"));
                    put(&mut self.ship_extra, "shipCargoCapacity", val(ev, "CargoCapacity"));
                    put(&mut self.ship_extra, "shipHullValue", val(ev, "HullValue"));
                    put(&mut self.ship_extra, "shipModulesValue", val(ev, "ModulesValue"));
                    put(&mut self.ship_extra, "shipRebuyCost", val(ev, "Rebuy"));
                    let modules = Self::loadout_modules(ev);
                    let h = fnv1a64(format!("{id}:{modules}").as_bytes());
                    self.loadout = Some(modules.clone());
                    if self.last_loadout != Some(h) {
                        self.last_loadout = Some(h);
                        out.push(InaraEvent::new(
                            "setCommanderShipLoadout",
                            ts,
                            json!({"shipType": lower(kind), "shipGameID": id, "shipLoadout": modules}),
                        ));
                    }
                    if let Some(e) = self.current_ship_event(ts) {
                        out.push(e);
                    }
                }
            }
            "StoredModules" => {
                let mut items: Vec<(i64, Value)> = Vec::new();
                for it in get_array(ev, "Items").into_iter().flatten().filter_map(Value::as_object) {
                    let Some(n) = get_nonempty(it, "Name") else { continue };
                    let mut d = Map::new();
                    d.insert("itemName".into(), json!(lower(strip_symbol(n))));
                    put(&mut d, "itemValue", val(it, "BuyPrice"));
                    put(&mut d, "isHot", val(it, "Hot"));
                    if get_bool(it, "InTransit") != Some(true) {
                        put(&mut d, "starsystemName", val(it, "StarSystem"));
                        put(&mut d, "marketID", val(it, "MarketID"));
                    }
                    if let Some(bp) = get_nonempty(it, "EngineerModifications") {
                        let mut e = Map::new();
                        e.insert("blueprintName".into(), json!(bp));
                        put(&mut e, "blueprintLevel", val(it, "Level"));
                        put(&mut e, "blueprintQuality", val(it, "Quality"));
                        d.insert("engineering".into(), Value::Object(e));
                    }
                    items.push((get_i64(it, "StorageSlot").unwrap_or(i64::MAX), Value::Object(d)));
                }
                items.sort_by_key(|(slot, _)| *slot);
                let v = Value::Array(items.into_iter().map(|(_, v)| v).collect());
                let h = hash_of(&v);
                if self.storage_sent != Some(h) {
                    self.storage_sent = Some(h);
                    out.push(InaraEvent::new("setCommanderStorageModules", ts, v));
                }
            }
            "MissionAccepted" => {
                if let (Some(mname), Some(id), Some(sys)) = (get_nonempty(ev, "Name"), get_u64(ev, "MissionID"), system()) {
                    let mut d = Map::new();
                    d.insert("missionName".into(), json!(mname));
                    d.insert("missionGameID".into(), json!(id));
                    d.insert("starsystemNameOrigin".into(), json!(sys));
                    put(&mut d, "stationNameOrigin", ctx.station_name.clone().map(Value::from));
                    put(&mut d, "minorfactionNameOrigin", val(ev, "Faction"));
                    put(&mut d, "influenceGain", val(ev, "Influence"));
                    put(&mut d, "reputationGain", val(ev, "Reputation"));
                    put(&mut d, "missionExpiry", val(ev, "Expiry"));
                    put(&mut d, "starsystemNameTarget", val(ev, "DestinationSystem"));
                    put(&mut d, "stationNameTarget", val(ev, "DestinationStation"));
                    put(&mut d, "minorfactionNameTarget", val(ev, "TargetFaction"));
                    put(&mut d, "commodityName", get_str(ev, "Commodity").map(|c| json!(lower(strip_symbol(c)))));
                    put(&mut d, "commodityCount", val(ev, "Count"));
                    put(&mut d, "targetName", readable(ev, "Target").map(Value::from));
                    put(&mut d, "targetType", readable(ev, "TargetType").map(Value::from));
                    put(&mut d, "killCount", val(ev, "KillCount"));
                    put(&mut d, "passengerType", val(ev, "PassengerType"));
                    put(&mut d, "passengerCount", val(ev, "PassengerCount"));
                    put(&mut d, "passengerIsVIP", val(ev, "PassengerVIPs"));
                    put(&mut d, "passengerIsWanted", val(ev, "PassengerWanted"));
                    out.push(InaraEvent::new("addCommanderMission", ts, Value::Object(d)));
                }
            }
            "MissionAbandoned" | "MissionFailed" => {
                if let Some(id) = get_u64(ev, "MissionID") {
                    let n = if name == "MissionAbandoned" { "setCommanderMissionAbandoned" } else { "setCommanderMissionFailed" };
                    out.push(InaraEvent::new(n, ts, json!({"missionGameID": id})));
                }
            }
            "MissionCompleted" => {
                if let Some(id) = get_u64(ev, "MissionID") {
                    let permits: Vec<String> =
                        get_array(ev, "PermitsAwarded").into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
                    for p in &permits {
                        out.push(InaraEvent::new("addCommanderPermit", ts, json!({"starsystemName": p})));
                    }
                    let mut d = Map::new();
                    d.insert("missionGameID".into(), json!(id));
                    put(&mut d, "donationCredits", val(ev, "Donated"));
                    put(&mut d, "rewardCredits", val(ev, "Reward"));
                    if !permits.is_empty() {
                        d.insert("rewardPermits".into(), Value::Array(permits.iter().map(|p| json!({"starsystemName": p})).collect()));
                    }
                    let items = |key: &str, ship_materials_only: bool| -> Vec<Value> {
                        get_array(ev, key)
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_object)
                            .filter(|it| {
                                !ship_materials_only
                                    || get_str(it, "Category").is_some_and(|c| {
                                        let c = strip_symbol(c).trim_start_matches("MICRORESOURCE_CATEGORY_").to_ascii_lowercase();
                                        matches!(c.as_str(), "raw" | "manufactured" | "encoded")
                                    })
                            })
                            .filter_map(|it| Some(json!({"itemName": lower(strip_symbol(get_str(it, "Name")?)), "itemCount": get_i64(it, "Count")?})))
                            .collect()
                    };
                    let commodities = items("CommodityReward", false);
                    let materials = items("MaterialsReward", true);
                    if !commodities.is_empty() {
                        d.insert("rewardCommodities".into(), Value::Array(commodities));
                    }
                    if !materials.is_empty() {
                        for m in &materials {
                            if let (Some(n), Some(c)) = (m["itemName"].as_str(), m["itemCount"].as_i64()) {
                                self.adjust_material(n, c);
                            }
                        }
                        d.insert("rewardMaterials".into(), Value::Array(materials));
                    }
                    let effects: Vec<Value> = get_array(ev, "FactionEffects")
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_object)
                        .filter_map(|f| {
                            let fname = get_nonempty(f, "Faction")?;
                            let mut e = Map::new();
                            e.insert("minorfactionName".into(), json!(fname));
                            let influence = get_array(f, "Influence")
                                .into_iter()
                                .flatten()
                                .filter_map(|i| i.get("Influence").and_then(Value::as_str))
                                .max_by_key(|s| s.len());
                            put(&mut e, "influenceGain", influence.map(Value::from));
                            put(&mut e, "reputationGain", val(f, "Reputation"));
                            Some(Value::Object(e))
                        })
                        .collect();
                    if !effects.is_empty() {
                        d.insert("minorfactionEffects".into(), Value::Array(effects));
                    }
                    out.push(InaraEvent::new("setCommanderMissionCompleted", ts, Value::Object(d)));
                    self.materials_event(ts, &mut out);
                }
            }
            "Died" => {
                if let Some(sys) = system() {
                    let mut d = Map::new();
                    d.insert("starsystemName".into(), json!(sys));
                    let wing: Vec<String> = get_array(ev, "Killers")
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_object)
                        .filter_map(|k| readable(k, "Name"))
                        .collect();
                    if !wing.is_empty() {
                        d.insert("wingOpponentNames".into(), json!(wing));
                    } else if let Some(k) = readable(ev, "KillerName").or_else(|| readable(ev, "KillerShip")) {
                        d.insert("opponentName".into(), json!(k));
                    }
                    if d.len() > 1 {
                        out.push(InaraEvent::new("addCommanderCombatDeath", ts, Value::Object(d)));
                    }
                }
            }
            "Interdicted" | "Interdiction" | "EscapeInterdiction" => {
                let who = if name == "Interdiction" { "Interdicted" } else { "Interdictor" };
                let opponent = readable(ev, who)
                    .or_else(|| get_nonempty(ev, "Faction").map(str::to_string))
                    .or_else(|| get_nonempty(ev, "Power").map(str::to_string))
                    .or_else(|| (get_bool(ev, "IsThargoid") == Some(true)).then(|| "Thargoid".to_string()));
                if let (Some(sys), Some(opponent)) = (system(), opponent) {
                    let mut d = Map::new();
                    d.insert("starsystemName".into(), json!(sys));
                    d.insert("opponentName".into(), json!(opponent));
                    d.insert("isPlayer".into(), json!(get_bool(ev, "IsPlayer").unwrap_or(false)));
                    let event = match name {
                        "Interdicted" => {
                            put(&mut d, "isSubmit", val(ev, "Submitted"));
                            "addCommanderCombatInterdicted"
                        }
                        "Interdiction" => {
                            put(&mut d, "isSuccess", val(ev, "Success"));
                            "addCommanderCombatInterdiction"
                        }
                        _ => "addCommanderCombatInterdictionEscape",
                    };
                    out.push(InaraEvent::new(event, ts, Value::Object(d)));
                }
            }
            "PVPKill" => {
                if let (Some(sys), Some(victim)) = (system(), get_nonempty(ev, "Victim")) {
                    out.push(InaraEvent::new("addCommanderCombatKill", ts, json!({"starsystemName": sys, "opponentName": victim})));
                }
            }
            "ShipLocker" => {
                if ev.contains_key("Items") {
                    self.locker_event(ev, ts, &mut out);
                } else {
                    self.expect = Some((CompanionKind::ShipLocker, Timestamp::parse(ts).unwrap_or_default()));
                }
            }
            "SuitLoadout" | "CreateSuitLoadout" => {
                if let Some(d) = Self::suit_loadout(ev) {
                    let id = get_u64(ev, "LoadoutID").unwrap_or_default();
                    let h = hash_of(&d);
                    if self.suits_sent.get(&id) != Some(&h) {
                        self.suits_sent.insert(id, h);
                        out.push(InaraEvent::new("setCommanderSuitLoadout", ts, d));
                    }
                }
            }
            "DeleteSuitLoadout" => {
                if let Some(id) = get_u64(ev, "LoadoutID") {
                    self.suits_sent.remove(&id);
                    out.push(InaraEvent::new("delCommanderSuitLoadout", ts, json!({"loadoutGameID": id})));
                }
            }
            "RenameSuitLoadout" | "LoadoutEquipModule" => {
                if let Some(id) = get_u64(ev, "LoadoutID") {
                    let mut d = Map::new();
                    d.insert("loadoutGameID".into(), json!(id));
                    put(&mut d, "loadoutName", val(ev, "LoadoutName"));
                    put(&mut d, "suitType", val(ev, "SuitName"));
                    put(&mut d, "suitGameID", val(ev, "SuitID"));
                    if name == "LoadoutEquipModule" {
                        let mut slot = Map::new();
                        put(&mut slot, "slotName", val(ev, "SlotName"));
                        put(&mut slot, "itemName", val(ev, "ModuleName"));
                        put(&mut slot, "itemGameID", val(ev, "SuitModuleID"));
                        put(&mut slot, "itemClass", val(ev, "Class"));
                        let eng: Vec<Value> = get_array(ev, "WeaponMods")
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(|b| json!({"blueprintName": b}))
                            .collect();
                        if !eng.is_empty() {
                            slot.insert("engineering".into(), Value::Array(eng));
                        }
                        d.insert("suitLoadout".into(), json!([slot]));
                    }
                    self.suits_sent.remove(&id);
                    out.push(InaraEvent::new("updateCommanderSuitLoadout", ts, Value::Object(d)));
                }
            }
            "CommunityGoal" => {
                for g in get_array(ev, "CurrentGoals").into_iter().flatten().filter_map(Value::as_object) {
                    let Some(id) = get_u64(g, "CGID") else { continue };
                    let mut d = Map::new();
                    d.insert("communitygoalGameID".into(), json!(id));
                    put(&mut d, "communitygoalName", val(g, "Title"));
                    put(&mut d, "starsystemName", val(g, "SystemName"));
                    put(&mut d, "stationName", val(g, "MarketName"));
                    put(&mut d, "goalExpiry", val(g, "Expiry"));
                    put(&mut d, "isCompleted", val(g, "IsComplete"));
                    put(&mut d, "contributorsNum", val(g, "NumContributors"));
                    put(&mut d, "contributionsTotal", val(g, "CurrentTotal"));
                    let last_int = |s: &str| s.split_whitespace().last().and_then(|w| w.parse::<i64>().ok());
                    put(&mut d, "tierReached", get_str(g, "TierReached").and_then(last_int).map(Value::from));
                    put(&mut d, "topRankSize", val(g, "TopRankSize"));
                    if let Some(top) = g.get("TopTier").and_then(Value::as_object) {
                        put(&mut d, "tierMax", get_str(top, "Name").and_then(last_int).map(Value::from));
                        put(&mut d, "completionBonus", val(top, "Bonus"));
                    }
                    let required = ["communitygoalName", "starsystemName", "stationName", "goalExpiry"];
                    if required.iter().all(|k| d.contains_key(*k)) {
                        out.push(InaraEvent::new("setCommunityGoal", ts, Value::Object(d)));
                    }
                    let mut p = Map::new();
                    p.insert("communitygoalGameID".into(), json!(id));
                    put(&mut p, "contribution", val(g, "PlayerContribution"));
                    put(&mut p, "percentileBand", val(g, "PlayerPercentileBand"));
                    put(&mut p, "percentileBandReward", val(g, "Bonus"));
                    put(&mut p, "isTopRank", val(g, "PlayerInTopRank"));
                    if p.contains_key("contribution") && p.contains_key("percentileBand") && p.contains_key("percentileBandReward") {
                        out.push(InaraEvent::new("setCommanderCommunityGoalProgress", ts, Value::Object(p)));
                    }
                }
            }
            "Friends" => {
                if let (Some(status), Some(friend)) = (get_str(ev, "Status"), get_nonempty(ev, "Name")) {
                    let n = match status {
                        "Added" | "Online" => Some("addCommanderFriend"),
                        "Declined" | "Lost" => Some("delCommanderFriend"),
                        _ => None,
                    };
                    if let Some(n) = n
                        && self.friends_sent.insert(format!("{n}:{friend}"))
                    {
                        out.push(InaraEvent::new(n, ts, json!({"commanderName": friend, "gamePlatform": "pc"})));
                    }
                }
            }
            "Powerplay" => {
                if let (Some(p), Some(r)) = (get_nonempty(ev, "Power"), get_i64(ev, "Rank")) {
                    self.power = Some((p.to_string(), r));
                    let mut d = Map::new();
                    d.insert("powerName".into(), json!(p));
                    d.insert("rankValue".into(), json!(r));
                    put(&mut d, "meritsValue", val(ev, "Merits"));
                    out.push(InaraEvent::new("setCommanderRankPower", ts, Value::Object(d)));
                }
            }
            "PowerplayJoin" => {
                if let Some(p) = get_nonempty(ev, "Power") {
                    self.power = Some((p.to_string(), 1));
                    out.push(InaraEvent::new("setCommanderRankPower", ts, json!({"powerName": p, "rankValue": 1})));
                }
            }
            "PowerplayLeave" => {
                if let Some(p) = get_nonempty(ev, "Power") {
                    self.power = None;
                    // The documented way to leave a power is -1 (EDMC's 0 is a bug).
                    out.push(InaraEvent::new("setCommanderRankPower", ts, json!({"powerName": p, "rankValue": -1})));
                }
            }
            "PowerplayDefect" => {
                if let Some(p) = get_nonempty(ev, "ToPower") {
                    self.power = Some((p.to_string(), 1));
                    out.push(InaraEvent::new("setCommanderRankPower", ts, json!({"powerName": p, "rankValue": 1})));
                }
            }
            "PowerplayRank" => {
                if let (Some(p), Some(r)) = (get_nonempty(ev, "Power"), get_i64(ev, "Rank")) {
                    self.power = Some((p.to_string(), r));
                    out.push(InaraEvent::new("setCommanderRankPower", ts, json!({"powerName": p, "rankValue": r})));
                }
            }
            "PowerplayMerits" => {
                if let (Some(p), Some(total)) = (get_nonempty(ev, "Power"), get_i64(ev, "TotalMerits"))
                    && let Some((_, rank)) = self.power.as_ref().filter(|(pp, _)| pp == p)
                {
                    out.push(InaraEvent::new("setCommanderRankPower", ts, json!({"powerName": p, "rankValue": rank, "meritsValue": total})));
                }
            }
            _ => {}
        }
        out
    }

    /// Companion files announced by a live event (`Cargo.json`, `ShipLocker.json`).
    pub fn on_companion(&mut self, c: &Companion) -> Vec<InaraEvent> {
        let mut out = Vec::new();
        let Some((kind, ts)) = self.expect else { return out };
        if c.kind() != kind || (c.timestamp().0 - ts.0).abs() > 2 {
            return out;
        }
        self.expect = None;
        let raw = match c {
            Companion::Cargo(_, raw) | Companion::ShipLocker(_, raw) => raw,
            _ => return out,
        };
        let Some(file) = crate::json::parse_object(raw) else { return out };
        let Some(t) = get_str(&file, "timestamp").map(str::to_string) else { return out };
        match kind {
            CompanionKind::Cargo if get_str(&file, "Vessel") == Some("Ship") => {
                if let Some(inv) = get_array(&file, "Inventory") {
                    let inv = inv.clone();
                    self.cargo_event(&inv, &t, &mut out);
                }
            }
            CompanionKind::ShipLocker => self.locker_event(&file, &t, &mut out),
            _ => {}
        }
        out
    }
}

/// What an Inara response means for the batch.
#[derive(Debug, Clone, PartialEq)]
pub enum InaraOutcome {
    /// Processed; per-event `(eventStatus, eventStatusText)`.
    Done(Vec<(i64, Option<String>)>),
    /// "Too much requests, slow down, cowboy": the key is locked for about an hour.
    RateLimited(String),
    /// Header-level rejection (bad key, app not whitelisted, …): stop until the config changes.
    Rejected(String),
    /// Transport/server trouble or an empty response: keep the batch and back off.
    Retry(String),
}

/// Inara's answer to `getCommanderProfile`: who the key belongs to. A header-level problem (bad
/// key, app not accepted, rate limit) comes back as that outcome; an answer without a profile as
/// `Done` with the event's status.
pub fn profile_response(resp: &HttpResponse) -> Result<crate::status::Account, InaraOutcome> {
    match classify_response(resp) {
        InaraOutcome::Done(_) => {}
        other => return Err(other),
    }
    let body: Value = serde_json::from_str(&resp.body).unwrap_or(Value::Null);
    let ev = body.pointer("/events/0");
    let status = ev.and_then(|e| e.get("eventStatus")).and_then(Value::as_i64).unwrap_or(0);
    let text = ev.and_then(|e| e.get("eventStatusText")).and_then(Value::as_str).map(str::to_string);
    let data = ev.and_then(|e| e.get("eventData"));
    let name = data.and_then(|d| d.get("commanderName").or_else(|| d.get("userName"))).and_then(Value::as_str).filter(|s| !s.is_empty());
    match name {
        Some(name) if (200..300).contains(&status) => Ok(crate::status::Account {
            name: name.to_string(),
            url: data.and_then(|d| d.get("inaraURL")).and_then(Value::as_str).map(str::to_string),
        }),
        _ => Err(InaraOutcome::Done(vec![(status, text)])),
    }
}

pub fn classify_response(resp: &HttpResponse) -> InaraOutcome {
    if !resp.is_success() {
        return InaraOutcome::Retry(format!("HTTP {}", resp.status));
    }
    let Ok(body) = serde_json::from_str::<Value>(&resp.body) else {
        return InaraOutcome::Retry("empty or unreadable response".into());
    };
    let header = body.get("header");
    let status = header.and_then(|h| h.get("eventStatus")).and_then(Value::as_i64).unwrap_or(0);
    let text = header.and_then(|h| h.get("eventStatusText")).and_then(Value::as_str).unwrap_or_default().to_string();
    if text.to_ascii_lowercase().contains("too much requests") {
        return InaraOutcome::RateLimited(text);
    }
    if status / 100 != 2 {
        return if status == 0 {
            InaraOutcome::Retry("response without a header status".into())
        } else {
            InaraOutcome::Rejected(format!("Inara {status}: {text}"))
        };
    }
    let events = body
        .get("events")
        .and_then(Value::as_array)
        .map(|evs| {
            evs.iter()
                .map(|e| {
                    (
                        e.get("eventStatus").and_then(Value::as_i64).unwrap_or(0),
                        e.get("eventStatusText").and_then(Value::as_str).map(str::to_string),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    InaraOutcome::Done(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse_object;

    struct Rig {
        m: InaraMapper,
        s: Session,
        ctx: UploadContext,
    }

    impl Rig {
        fn new() -> Self {
            Rig { m: InaraMapper::new(), s: Session::new(), ctx: UploadContext::default() }
        }
        fn feed(&mut self, line: &str) -> Vec<InaraEvent> {
            let ev = parse_object(line).unwrap();
            let name = get_str(&ev, "event").unwrap().to_string();
            self.s.apply(&name, &ev);
            self.m.on_event(&name, &ev, &self.s, &self.ctx)
        }
    }

    fn names(evs: &[InaraEvent]) -> Vec<&str> {
        evs.iter().map(|e| e.name.as_str()).collect()
    }

    /// A profile sync sends the commander's state, never something done: of the "set" events, the mission and
    /// ship-transfer ones aren't state.
    #[test]
    fn state_events_leave_out_what_was_done() {
        for name in ["setCommanderCredits", "setCommanderShip", "setCommanderShipLoadout", "setCommanderTravelLocation", "setCommanderRankPilot", "setCommanderReputationMinorFaction", "setCommanderInventoryMaterials", "setCommunityGoal"] {
            assert!(is_state(name), "{name}");
        }
        for name in ["setCommanderMissionCompleted", "setCommanderMissionFailed", "setCommanderMissionAbandoned", "setCommanderShipTransfer", "addCommanderMission", "addCommanderTravelFSDJump", "delCommanderShip", "resetCommanderInventory"] {
            assert!(!is_state(name), "{name}");
        }
    }

    #[test]
    fn session_start_dump_on_first_cargo() {
        let mut r = Rig::new();
        r.feed(r#"{ "timestamp":"2026-10-12T06:29:35Z", "event":"Rank", "Combat":1, "Trade":4, "Explore":6, "Soldier":0, "Exobiologist":2, "Empire":7, "Federation":1, "CQC":0 }"#);
        let p = r.feed(r#"{ "timestamp":"2026-10-12T06:29:35Z", "event":"Progress", "Combat":30, "Trade":46, "Explore":78, "Soldier":0, "Exobiologist":35, "Empire":37, "Federation":100, "CQC":0 }"#);
        assert_eq!(names(&p), ["setCommanderRankPilot"]);
        let ranks = p[0].data.as_array().unwrap();
        assert_eq!(ranks.len(), 8);
        assert!(ranks.iter().any(|r| r["rankName"] == "explore" && r["rankValue"] == 6 && r["rankProgress"] == 0.78));
        r.feed(r#"{ "timestamp":"2026-10-12T06:29:35Z", "event":"Reputation", "Empire":70.500000, "Federation":77.584801, "Independent":0.000000, "Alliance":22.007000 }"#);
        r.feed(r#"{ "timestamp":"2026-10-12T06:29:35Z", "event":"EngineerProgress", "Engineers":[ { "Engineer":"Felicity Farseer", "EngineerID":300100, "Progress":"Unlocked", "RankProgress":0, "Rank":5 }, { "Engineer":"Eleanor Bresa", "EngineerID":400011, "Progress":"Known" } ] }"#);
        r.feed(r#"{ "timestamp":"2026-10-12T06:29:40Z", "event":"LoadGame", "FID":"F1", "Commander":"Jameson", "Horizons":true, "Odyssey":true, "Ship":"explorer_nx", "ShipID":22, "ShipName":"", "ShipIdent":"AL-11E", "Credits":3889591046, "Loan":0 }"#);
        let st = r.feed(r#"{ "timestamp":"2026-10-12T06:29:41Z", "event":"Statistics", "Bank_Account":{ "Current_Wealth":11042131150 } }"#);
        assert_eq!(names(&st), ["setCommanderCredits", "setCommanderGameStatistics"]);
        assert_eq!(st[0].data["commanderAssets"], 11_042_131_150i64);
        r.feed(r#"{ "timestamp":"2026-10-12T06:29:45Z", "event":"Loadout", "Ship":"explorer_nx", "ShipID":22, "ShipName":"", "ShipIdent":"AL-11E", "HullValue":194861290, "ModulesValue":131778652, "CargoCapacity":216, "MaxJumpRange":86.896965, "Rebuy":16331996, "Modules":[ { "Slot":"PowerPlant", "Item":"int_powerplant_size5_class5", "On":true, "Priority":1, "Health":1.000000, "Value":4323699, "Engineering":{ "Engineer":"Felicity Farseer", "EngineerID":300100, "BlueprintID":128673765, "BlueprintName":"PowerPlant_Boosted", "Level":1, "Quality":1.000000, "Modifiers":[ { "Label":"PowerCapacity", "Value":24.36, "OriginalValue":21.0, "LessIsGood":0 } ] } } ] }"#);
        let c = r.feed(r#"{ "timestamp":"2026-10-12T06:29:46Z", "event":"Cargo", "Vessel":"Ship", "Count":2, "Inventory":[ { "Name":"metaalloys", "Name_Localised":"Meta-Alloys", "Count":2, "Stolen":0 } ] }"#);
        assert_eq!(
            names(&c),
            ["setCommanderReputationMajorFaction", "setCommanderRankEngineer", "setCommanderShip", "setCommanderShipLoadout", "setCommanderInventoryCargo"]
        );
        assert_eq!(c[1].data, json!([{"engineerName": "Felicity Farseer", "rankStage": "Unlocked", "rankValue": 5}]));
        assert_eq!(c[2].data["shipIdent"], "AL-11E");
        assert!(c[2].data.get("shipName").is_none(), "empty names are not sent");
        assert_eq!(c[3].data["shipLoadout"][0]["engineering"]["modifiers"][0]["lessIsGood"], false);
        assert_eq!(c[4].data, json!([{"itemName": "metaalloys", "itemCount": 2}]));
        // unchanged cargo is not re-sent
        let again = r.feed(r#"{ "timestamp":"2026-10-12T06:30:46Z", "event":"Cargo", "Vessel":"Ship", "Count":2, "Inventory":[ { "Name":"metaalloys", "Count":2, "Stolen":0 } ] }"#);
        assert!(again.is_empty());
    }

    #[test]
    fn travel_events_and_dock_suppression() {
        let mut r = Rig::new();
        r.feed(r#"{ "timestamp":"2026-09-27T23:00:00Z", "event":"LoadGame", "FID":"F1", "Commander":"Jameson", "Horizons":true, "Odyssey":true, "Ship":"krait_mkii", "ShipID":12, "Credits":1, "Loan":0 }"#);
        let loc = r.feed(r#"{ "timestamp":"2026-09-27T23:00:05Z", "event":"Location", "Docked":true, "StationName":"i Sola Prospect", "StationType":"CraterOutpost", "MarketID":3516500480, "StarSystem":"Brestla", "SystemAddress":673101653409, "StarPos":[159.46875,-103.75000,-19.68750], "Body":"Brestla A 1", "BodyID":8, "BodyType":"Planet", "Factions":[ { "Name":"Heilelang Patron's Principles", "MyReputation":45.5 } ] }"#);
        assert_eq!(names(&loc), ["setCommanderTravelLocation", "setCommanderReputationMinorFaction"]);
        assert_eq!(loc[0].data["starsystemBodyName"], "Brestla A 1");
        assert_eq!(loc[1].data[0]["minorfactionReputation"], 0.455);
        // The dock at session start is not logged.
        let d = r.feed(r#"{ "timestamp":"2026-09-27T23:00:10Z", "event":"Docked", "StationName":"i Sola Prospect", "StationType":"CraterOutpost", "StarSystem":"Brestla", "SystemAddress":673101653409, "MarketID":3516500480 }"#);
        assert!(d.is_empty());
        r.feed(r#"{ "timestamp":"2026-09-27T23:10:00Z", "event":"Undocked", "StationName":"i Sola Prospect", "MarketID":3516500480 }"#);
        // Re-docking without travelling is not logged either.
        assert!(r.feed(r#"{ "timestamp":"2026-09-27T23:11:00Z", "event":"Docked", "StationName":"i Sola Prospect", "StationType":"CraterOutpost", "StarSystem":"Brestla", "SystemAddress":673101653409, "MarketID":3516500480 }"#).is_empty());
        r.feed(r#"{ "timestamp":"2026-09-27T23:12:00Z", "event":"Undocked", "StationName":"i Sola Prospect", "MarketID":3516500480 }"#);
        let j = r.feed(r#"{ "timestamp":"2026-09-27T23:20:00Z", "event":"FSDJump", "Taxi":false, "StarSystem":"Synuefe BR-L c24-23", "SystemAddress":6406446944978, "StarPos":[220.96875,-124.81250,-12.59375], "JumpDist":9.851, "FuelUsed":0.48, "FuelLevel":7.5 }"#);
        assert_eq!(names(&j), ["addCommanderTravelFSDJump"]);
        assert_eq!(j[0].data["shipType"], "krait_mkii");
        assert_eq!(j[0].data["jumpDistance"], 9.851);
        let d = r.feed(r#"{ "timestamp":"2026-09-27T23:30:00Z", "event":"Docked", "StationName":"SadiCarnot Reach", "StationType":"Orbis", "Taxi":false, "StarSystem":"Synuefe BR-L c24-23", "SystemAddress":6406446944978, "MarketID":4220345091 }"#);
        assert_eq!(names(&d), ["addCommanderTravelDock"]);
        assert_eq!(d[0].data["shipGameID"], 12);
    }

    #[test]
    fn powerplay_and_combat() {
        let mut r = Rig::new();
        let leave = r.feed(r#"{ "timestamp":"2026-09-27T23:00:00Z", "event":"PowerplayLeave", "Power":"Aisling Duval" }"#);
        assert_eq!(leave[0].data["rankValue"], -1);
        r.ctx.system_name = "Brestla".into();
        let i = r.feed(r#"{ "timestamp":"2026-10-11T05:10:46Z", "event":"Interdicted", "Submitted":true, "Interdictor":"Leebo", "IsPlayer":false, "Faction":"Remlok Industries" }"#);
        assert_eq!(names(&i), ["addCommanderCombatInterdicted"]);
        assert_eq!(i[0].data["opponentName"], "Leebo");
        assert!(r.feed(r#"{ "timestamp":"2026-09-28T05:28:13Z", "event":"Died" }"#).is_empty(), "no opponent: not recorded");
    }

    #[test]
    fn materials_tracking() {
        let mut r = Rig::new();
        assert!(r.feed(r#"{ "timestamp":"2026-10-12T06:30:00Z", "event":"MaterialCollected", "Category":"Raw", "Name":"nickel", "Count":3 }"#).is_empty(), "no base yet");
        let m = r.feed(r#"{ "timestamp":"2026-10-12T06:30:01Z", "event":"Materials", "Raw":[ { "Name":"nickel", "Count":10 } ], "Manufactured":[ { "Name":"chemicaldistillery", "Name_Localised":"Chemical Distillery", "Count":4 } ], "Encoded":[  ] }"#);
        assert_eq!(names(&m), ["setCommanderInventoryMaterials"]);
        let t = r.feed(r#"{ "timestamp":"2026-10-12T06:31:00Z", "event":"MaterialTrade", "MarketID":1, "TraderType":"manufactured", "Paid":{ "Material":"chemicaldistillery", "Category":"Manufactured", "Quantity":3 }, "Received":{ "Material":"chemicalmanipulators", "Category":"Manufactured", "Quantity":1 } }"#);
        assert_eq!(t[0].data, json!([{"itemName": "chemicaldistillery", "itemCount": 1}, {"itemName": "chemicalmanipulators", "itemCount": 1}, {"itemName": "nickel", "itemCount": 10}]));
    }

    #[test]
    fn coalesce_keys() {
        let e = InaraEvent::new("setCommanderShip", "t", json!({"shipGameID": 5}));
        assert_eq!(e.coalesce_key().as_deref(), Some("ship:5"));
        assert_eq!(InaraEvent::new("addCommanderTravelDock", "t", json!({})).coalesce_key(), None);
    }

    #[test]
    fn responses() {
        let ok = HttpResponse::new(200, r#"{"header":{"eventStatus":200},"events":[{"eventStatus":200},{"eventStatus":204,"eventStatusText":"No results found."}]}"#);
        assert_eq!(classify_response(&ok), InaraOutcome::Done(vec![(200, None), (204, Some("No results found.".into()))]));
        let limited = HttpResponse::new(200, r#"{"header":{"eventStatus":400,"eventStatusText":"Too much requests, slow down, cowboy. ;)"}}"#);
        assert!(matches!(classify_response(&limited), InaraOutcome::RateLimited(_)));
        let bad = HttpResponse::new(200, r#"{"header":{"eventStatus":400,"eventStatusText":"Invalid API key."}}"#);
        assert!(matches!(classify_response(&bad), InaraOutcome::Rejected(_)));
        assert!(matches!(classify_response(&HttpResponse::new(200, "")), InaraOutcome::Retry(_)));
        assert!(matches!(classify_response(&HttpResponse::new(503, "maintenance")), InaraOutcome::Retry(_)));
    }
}
