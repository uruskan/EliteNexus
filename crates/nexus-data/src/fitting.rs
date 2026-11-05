//! Ship hulls and modules with their figures, for planning a loadout (Fleet › Plan a loadout), and what a loadout
//! comes to: mass, jump range, speed, power, shields, armour and cost.
//!
//! Data: EDCD coriolis-data (`data/coriolis`, gathered by `examples/convert_coriolis.rs`), tied to the game's own
//! symbols through FDevIDs ids, with the game's slot names from its `ship-slots.json`. The sums follow the game: an
//! unengineered Kestrel Mk II comes to the unladen mass (323.1 t) and jump range (19.64 ly) its journal Loadout
//! reports. Engineering read from a journal Loadout uses the game's own engineered values.

use crate::outfitting::{self, SlotKind};
use rustc_hash::FxHashMap;
use serde_json::Value;
use std::sync::LazyLock;

mod blueprints;
mod combat;
pub use blueprints::{Engineered, effect_damage, effect_has_figures, has_figures, planned, planned_goal};
pub use combat::{Against, Capacitor, Heat, Pips, PowerGroup, Setup, Weapon, against, weapon};

const SHIPS_JSON: &str = include_str!("../../../data/coriolis/ships.json");
const SLOTS_JSON: &str = include_str!("../../../data/coriolis/ship-slots.json");
const MODULES_JSON: &str = include_str!("../../../data/coriolis/modules.json");

/// The cargo hatch's power draw, MW (every hull has one).
const HATCH_POWER: f64 = 0.6;

// ---------------------------------------------------------------------------------------------
// Data

/// A module's figures, as coriolis-data has them.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleData {
    /// Lower-case journal symbol.
    pub symbol: String,
    /// coriolis-data's group: "fsd", "pp", "t", "sg", "sb", "hr", "pl", "mc", …
    pub grp: String,
    pub class: u8,
    pub rating: char,
    /// The module's own name in coriolis-data ("Bi-Weave", "MkII Agile Boost Thrusters"), if it has one.
    pub name: Option<String>,
    pub description: Option<String>,
    /// Weapon mount: 'F', 'G' or 'T'.
    pub mount: Option<char>,
    /// List price, Cr.
    pub cost: f64,
    pub mass: f64,
    /// Power draw, MW.
    pub power: f64,
    /// A utility that draws power with the hardpoints retracted (shield boosters, scanners).
    pub passive: bool,
    /// The hulls it fits (journal symbols); empty: any.
    pub hulls: Vec<String>,
    /// It goes only in a slot made for it (the Mk II cargo racks' cargo slots, the Type-11's limpet slot).
    pub special_slot: bool,
    /// Sold already engineered (Guardian weapons, pre-engineered drives, Merc gear).
    pub pre_engineered: bool,
    /// Every figure by coriolis-data's name: `optmass`, `maxfuel`, `fuelmul`, `shieldboost`, …
    pub figures: FxHashMap<String, f64>,
    /// The damage types a weapon deals.
    pub damage: Option<DamageDist>,
    coriolis_id: String,
}

impl ModuleData {
    /// A figure, 0 when the module has none.
    pub fn get(&self, key: &str) -> f64 {
        self.base(key).unwrap_or(0.0)
    }

    /// A figure of the module's own, None when it has none: mass, power and price too, and a rate of fire from the
    /// interval between shots.
    pub fn base(&self, key: &str) -> Option<f64> {
        match key {
            "mass" => Some(self.mass),
            "power" => Some(self.power),
            "cost" => Some(self.cost),
            "rof" => self.figures.get("rof").copied().or_else(|| self.figures.get("fireint").filter(|i| **i > 0.0).map(|i| 1.0 / i)),
            _ => self.figures.get(key).copied(),
        }
    }

    /// What players call it: "5A Frame Shift Drive (SCO)", "3C Gimballed Multi-cannon".
    pub fn display_name(&self) -> String {
        outfitting::display_name(&self.symbol)
    }
}

/// One of a hull's bulkheads.
#[derive(Debug, Clone, PartialEq)]
pub struct Bulkhead {
    /// Lower-case journal symbol (`anaconda_armour_grade3`).
    pub symbol: String,
    pub name: String,
    pub cost: f64,
    pub mass: f64,
    /// Armour as a share of the hull's base armour, added to it (2.5: +250 %).
    pub hullboost: f64,
    pub res: Resistances,
}

/// Resistances as shares (0.4 = 40 %).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Resistances {
    pub kinetic: f64,
    pub thermal: f64,
    pub explosive: f64,
    pub caustic: f64,
}

/// How a weapon's damage splits between the types, as shares adding up to 1.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DamageDist {
    pub kinetic: f64,
    pub thermal: f64,
    pub explosive: f64,
    /// Absolute: no resistance stops it.
    pub absolute: f64,
    pub caustic: f64,
}

impl DamageDist {
    /// From coriolis-data's `{"K": 0.5, "T": 0.5}`.
    pub fn from_json(v: &Value) -> DamageDist {
        let share = |k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(0.0);
        DamageDist { kinetic: share("K"), thermal: share("T"), explosive: share("E"), absolute: share("A"), caustic: share("C") }
    }

    /// The main type's name, for a weapon list: "Thermal", "Kinetic", "Thermal and kinetic".
    pub fn describe(&self) -> String {
        let mut parts: Vec<(f64, &str)> = vec![(self.kinetic, "kinetic"), (self.thermal, "thermal"), (self.explosive, "explosive"), (self.absolute, "absolute"), (self.caustic, "caustic")];
        parts.retain(|(s, _)| *s > 0.001);
        parts.sort_by(|a, b| b.0.total_cmp(&a.0));
        let names: Vec<&str> = parts.iter().map(|(_, n)| *n).collect();
        let text = match names.as_slice() {
            [] => return String::new(),
            [one] => (*one).to_string(),
            [a, b] => format!("{a} and {b}"),
            [first @ .., last] => format!("{}, and {last}", first.join(", ")),
        };
        let mut c = text.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }
}

/// A slot of a hull.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotDef {
    /// The game's name for it (`Slot03_Size6`, `LargeHardpoint1`, `FrameShiftDrive`).
    pub name: String,
    pub kind: SlotKind,
    /// Its class: 0 for utility mounts, 1–4 small to huge for hardpoints, 1–8 for internals.
    pub size: u8,
    /// The groups it takes when it takes only some (military slots, the Type-11's limpet slot); empty: any of its
    /// kind.
    pub only: Vec<String>,
    /// What a new hull comes with there.
    pub default: Option<String>,
}

/// A hull, with its slots.
#[derive(Debug, Clone, PartialEq)]
pub struct Hull {
    /// Lower-case journal symbol.
    pub symbol: String,
    pub name: String,
    pub manufacturer: String,
    /// Landing pad: 'S', 'M' or 'L'.
    pub pad: char,
    /// The hull's price with no modules, Cr.
    pub hull_cost: f64,
    /// What a shipyard asks for it: the hull with its standard modules, Cr.
    pub retail_cost: f64,
    pub hull_mass: f64,
    /// It takes these modules: fighter hangars, luxury cabins, Mk II vessel and vehicle hangars.
    pub fighter_hangars: bool,
    pub luxury_cabins: bool,
    pub vessel_hangars_mk2: bool,
    pub vehicle_hangars_mk2: bool,
    /// Top speed and boost at four pips to engines, m/s, before the thrusters' mass curve.
    pub speed: f64,
    pub boost: f64,
    /// Engine capacitor a boost takes, MJ, and the least time between boosts, s.
    pub boost_energy: f64,
    pub boost_cooldown: f64,
    /// Thrust with no pips to engines, as a share of four pips'.
    pub minthrust: f64,
    /// Turn rates at four pips to engines, before the thrusters' mass curve, degrees a second, and the share of them
    /// each pip short of four takes away.
    pub pitch: f64,
    pub yaw: f64,
    pub roll: f64,
    pub turn_per_pip: f64,
    pub base_shield: f64,
    pub base_armour: f64,
    pub hardness: f64,
    pub heat_capacity: f64,
    pub masslock: f64,
    pub crew: u8,
    /// The reserve tank, t: life support's fuel, not the FSD's.
    pub reserve_fuel: f64,
    pub bulkheads: Vec<Bulkhead>,
    pub slots: Vec<SlotDef>,
}

impl Hull {
    pub fn slot(&self, name: &str) -> Option<&SlotDef> {
        self.slots.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// The modules that fit `slot`: its kind of module, its size or smaller (a core slot: the group it takes), as the
    /// slot's, the hull's and the module's own restrictions allow. Biggest first, then rating A to E.
    pub fn fits(&self, slot: &SlotDef) -> Vec<&'static ModuleData> {
        let mut out: Vec<&'static ModuleData> = table()
            .list
            .iter()
            // Pre-engineered and Merc variants share their module's symbol: the plain one is the one to pick.
            .filter(|m| module(&m.symbol).is_some_and(|r| std::ptr::eq(r, *m)))
            // What a new hull is given (`…_free`) isn't sold: its plain twin is.
            .filter(|m| !m.symbol.ends_with("_free"))
            .filter(|m| self.takes(m))
            .filter(|m| fits_slot(m, slot))
            .collect();
        out.sort_by(|a, b| b.class.cmp(&a.class).then(a.rating.cmp(&b.rating)).then(a.symbol.cmp(&b.symbol)));
        out
    }

    /// The hull takes the module at all: one made for other hulls doesn't fit, fighter hangars and luxury cabins only
    /// hulls built for them, and thrusters or a shield generator only when the hull alone isn't over their maximum
    /// mass (the game won't fit them).
    pub fn takes(&self, m: &ModuleData) -> bool {
        let hull_ok = match m.grp.as_str() {
            "fh" => self.fighter_hangars,
            "pcq" => self.luxury_cabins,
            "fhmkii" => self.vessel_hangars_mk2,
            "lpvmkii" => self.vehicle_hangars_mk2,
            "t" | "sg" | "bsg" | "psg" => m.figures.get("maxmass").is_none_or(|max| *max >= self.hull_mass),
            _ => true,
        };
        hull_ok && (m.hulls.is_empty() || m.hulls.contains(&self.symbol))
    }
}

/// Modules a ship takes one of (each list is one kind): a second of them can't be fitted.
pub const ONE_PER_SHIP: [&[&str]; 6] = [&["gfsb"], &["fs"], &["rf"], &["ss"], &["fh", "fhmkii"], &["pv", "lpv", "lpvmkii"]];

/// The kind of module a ship takes only one of that `m` is, if it's one.
pub fn one_per_ship(m: &ModuleData) -> Option<&'static [&'static str]> {
    ONE_PER_SHIP.iter().copied().find(|kind| kind.contains(&m.grp.as_str()))
}

/// Groups that never go in an ordinary optional slot: core modules (fuel tanks do: coriolis-data files the optional
/// ones under the core tank's group), the planetary approach suite and coriolis-data's placeholders.
const NOT_OPTIONAL: [&str; 8] = ["pp", "t", "fsd", "ls", "pd", "s", "pas", "mm"];

/// A utility mount's module: class 0 hardpoint symbols (`hpt_…_tiny`, `…_size0_…`).
fn is_utility(m: &ModuleData) -> bool {
    m.symbol.starts_with("hpt_") && m.class == 0
}

fn fits_slot(m: &ModuleData, slot: &SlotDef) -> bool {
    let weapon_mount = matches!(slot.kind, SlotKind::Hardpoint | SlotKind::Utility);
    // coriolis-data's placeholders for a module it doesn't know (`int_missing_hyperdrive`) aren't modules to fit.
    if m.symbol.starts_with("hpt_") != weapon_mount || m.class > slot.size || m.grp == "mh" || m.grp == "mm" || m.symbol.contains("missing") {
        return false;
    }
    // Life support, sensors and supercruise overcharge drives only of the slot's own size.
    let exact = matches!(m.grp.as_str(), "ls" | "s") || (m.grp == "fsd" && m.symbol.contains("overcharge"));
    if slot.kind == SlotKind::Core && exact && m.class != slot.size {
        return false;
    }
    // A slot that takes only some groups (core slots, military slots, the Type-11's mining, limpet and hangar slots).
    if !slot.only.is_empty() {
        return slot.only.contains(&m.grp);
    }
    if m.special_slot {
        return false;
    }
    match slot.kind {
        SlotKind::Hardpoint => !is_utility(m),
        SlotKind::Utility => is_utility(m),
        SlotKind::Optional | SlotKind::Military => !NOT_OPTIONAL.contains(&m.grp.as_str()),
        _ => false,
    }
}

struct Table {
    list: Vec<ModuleData>,
    by_symbol: FxHashMap<String, usize>,
    hulls: Vec<Hull>,
}

static TABLE: LazyLock<Table> = LazyLock::new(load);

fn table() -> &'static Table {
    &TABLE
}

/// Every module coriolis-data lists, pre-engineered and Merc variants included.
pub fn modules() -> &'static [ModuleData] {
    &table().list
}

/// A module by journal symbol (any case). The plain module wins over pre-engineered or Merc variants sharing its
/// symbol.
pub fn module(symbol: &str) -> Option<&'static ModuleData> {
    let t = table();
    t.by_symbol.get(&symbol.trim().to_ascii_lowercase()).map(|&i| &t.list[i])
}

/// Every hull, by name.
pub fn hulls() -> &'static [Hull] {
    &table().hulls
}

/// A hull by journal symbol (any case).
pub fn hull(symbol: &str) -> Option<&'static Hull> {
    let s = symbol.trim().to_ascii_lowercase();
    table().hulls.iter().find(|h| h.symbol == s)
}

fn num(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(0.0)
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// coriolis-data's id for a module: a string ("4o") or a number (17).
fn id_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// The journal's symbol for a coriolis-data hull (`cid` is its id there): by its FDevIDs id, else by its name
/// (coriolis-data has the Corsair's and the Caspian Explorer's ids one off), else its id when that's the symbol.
fn journal_symbol(cid: &str, v: &Value) -> Option<String> {
    let by_id = v.get("edID").and_then(Value::as_u64).and_then(crate::ships::ship_by_id);
    let name = v.pointer("/properties/name").and_then(Value::as_str).unwrap_or_default();
    let by_name = || crate::ships::all().iter().find(|s| s.name.eq_ignore_ascii_case(name.trim()));
    by_id.or_else(by_name).or_else(|| crate::ships::ship(cid)).map(|s| s.symbol.clone())
}

fn load() -> Table {
    let raw: Vec<Value> = serde_json::from_str(MODULES_JSON).unwrap_or_default();
    let ships: serde_json::Map<String, Value> = serde_json::from_str(SHIPS_JSON).unwrap_or_default();
    // A module's hull restriction names hulls by coriolis-data's own ids ("kestrel", "explorer_nx").
    let journal_of = |cid: &str| -> Option<String> { journal_symbol(cid, ships.get(cid)?) };
    let mut list = Vec::with_capacity(raw.len());
    for m in &raw {
        let Some(symbol) = text(m, "symbol").map(|s| s.to_ascii_lowercase()) else { continue };
        let mut figures = FxHashMap::default();
        for (k, v) in m.as_object().into_iter().flatten() {
            if let Some(x) = v.as_f64().filter(|x| x.is_finite()) {
                figures.insert(k.clone(), x);
            }
        }
        let hulls: Vec<String> = match m.get("ship") {
            Some(Value::String(s)) => journal_of(s).into_iter().collect(),
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).filter_map(journal_of).collect(),
            _ => Vec::new(),
        };
        list.push(ModuleData {
            grp: text(m, "grp").unwrap_or_default(),
            class: num(m, "class") as u8,
            rating: text(m, "rating").and_then(|r| r.chars().next()).unwrap_or('?'),
            name: text(m, "name"),
            description: text(m, "ukDiscript"),
            mount: text(m, "mount").and_then(|r| r.chars().next()),
            cost: num(m, "cost"),
            mass: num(m, "mass"),
            power: num(m, "power"),
            passive: m.get("passive").is_some_and(|p| p.as_bool() == Some(true) || p.as_f64().is_some_and(|x| x != 0.0)),
            hulls,
            // The Mk II cargo racks say so ("restriction"); the Type-11's Mk II mining limpet controller goes only in
            // its limpet slot.
            special_slot: m.get("restriction").is_some() || symbol == "int_multidronecontrol_miningv2_size5_class5",
            pre_engineered: m.get("preEngineered").is_some() || m.get("mercModule").is_some_and(|v| v.as_bool() != Some(false)),
            damage: m.get("damagedist").filter(|d| d.is_object()).map(DamageDist::from_json),
            figures,
            coriolis_id: m.get("id").and_then(id_text).unwrap_or_default(),
            symbol,
        });
    }
    let mut by_symbol: FxHashMap<String, usize> = FxHashMap::default();
    for (i, m) in list.iter().enumerate() {
        match by_symbol.get(&m.symbol) {
            Some(&j) if !list[j].pre_engineered || m.pre_engineered => {}
            _ => {
                by_symbol.insert(m.symbol.clone(), i);
            }
        }
    }
    let slots: serde_json::Map<String, Value> = serde_json::from_str(SLOTS_JSON).unwrap_or_default();
    let mut hulls: Vec<Hull> = ships.iter().filter_map(|(cid, v)| hull_from(&journal_symbol(cid, v)?, cid, v, &slots, &list)).collect();
    hulls.sort_by(|a, b| a.name.cmp(&b.name));
    Table { list, by_symbol, hulls }
}

fn hull_from(symbol: &str, cid: &str, v: &Value, slot_table: &serde_json::Map<String, Value>, modules: &[ModuleData]) -> Option<Hull> {
    let symbol = symbol.to_string();
    let p = v.get("properties")?;
    // ship-slots.json goes by the journal's symbol, the Corsair's by coriolis-data's id.
    let names = slot_table.get(&symbol).or_else(|| slot_table.get(cid)).and_then(|s| s.get("slots"));
    let pad = match num(p, "class") as u8 {
        3 => 'L',
        2 => 'M',
        _ => 'S',
    };
    const GRADES: [&str; 5] = ["grade1", "grade2", "grade3", "mirrored", "reactive"];
    let bulkheads = v
        .get("bulkheads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, b)| Bulkhead {
            symbol: text(b, "symbol")
                .or_else(|| b.get("edID").and_then(Value::as_u64).and_then(outfitting::module_by_id).map(|m| m.symbol.clone()))
                .unwrap_or_else(|| format!("{symbol}_armour_{}", GRADES.get(i).copied().unwrap_or("grade1")))
                .to_ascii_lowercase(),
            name: text(b, "name").unwrap_or_default(),
            cost: num(b, "cost"),
            mass: num(b, "mass"),
            hullboost: num(b, "hullboost"),
            res: Resistances { kinetic: num(b, "kinres"), thermal: num(b, "thermres"), explosive: num(b, "explres"), caustic: num(b, "causres") },
        })
        .collect();
    let mut slots = Vec::new();
    // Core: coriolis-data's seven core slots in the game's order, with their defaults ("5A" or a module id).
    const CORE: [(&str, &str); 7] = [("PowerPlant", "pp"), ("MainEngines", "t"), ("FrameShiftDrive", "fsd"), ("LifeSupport", "ls"), ("PowerDistributor", "pd"), ("Radar", "s"), ("FuelTank", "ft")];
    let sizes = v.pointer("/slots/standard").and_then(Value::as_array).cloned().unwrap_or_default();
    let defaults = v.pointer("/defaults/standard").and_then(Value::as_array).cloned().unwrap_or_default();
    for (i, (name, grp)) in CORE.iter().enumerate() {
        let size = sizes.get(i).and_then(slot_class).unwrap_or(0);
        let default = defaults.get(i).and_then(id_text).and_then(|d| {
            let mut cr = d.chars();
            let (class, rating) = (cr.next().and_then(|c| c.to_digit(10)), cr.next());
            modules
                .iter()
                .find(|m| m.grp == *grp && m.coriolis_id == d)
                .or_else(|| modules.iter().find(|m| m.grp == *grp && !m.pre_engineered && Some(u32::from(m.class)) == class && Some(m.rating) == rating && d.len() == 2))
                .map(|m| m.symbol.clone())
        });
        slots.push(SlotDef { name: (*name).to_string(), kind: SlotKind::Core, size, only: vec![(*grp).to_string()], default });
    }
    // Hardpoints, utility mounts and optional internals: the game's names from ship-slots.json, in coriolis-data's
    // order (its sizes, restrictions and defaults line up with them).
    for group in ["hardpoints", "internal"] {
        let sizes = v.pointer(&format!("/slots/{group}")).and_then(Value::as_array).cloned().unwrap_or_default();
        let defaults = v.pointer(&format!("/defaults/{group}")).and_then(Value::as_array).cloned().unwrap_or_default();
        let named = names.and_then(|n| n.get(group)).and_then(Value::as_array).cloned().unwrap_or_default();
        for (i, s) in sizes.iter().enumerate() {
            let Some(name) = named.get(i).and_then(|n| text(n, "name")) else { continue };
            let kind = outfitting::slot_kind(&name);
            if !matches!(kind, SlotKind::Hardpoint | SlotKind::Utility | SlotKind::Optional | SlotKind::Military) {
                // The planetary approach suite and the like: built in, nothing to plan.
                continue;
            }
            let only: Vec<String> = s.get("eligible").and_then(Value::as_object).map(|e| e.keys().cloned().collect()).unwrap_or_default();
            let hardpoint = group == "hardpoints";
            let mut slot = SlotDef { name, kind, size: slot_class(s).unwrap_or(0), only, default: None };
            // A stock module only where it fits (coriolis-data's Corsair has one default too few, so the rest shift).
            slot.default = defaults
                .get(i)
                .and_then(id_text)
                .filter(|d| d != "0")
                .and_then(|id| modules.iter().find(|m| m.coriolis_id == id && m.symbol.starts_with("hpt_") == hardpoint))
                .filter(|m| fits_slot(m, &slot))
                .map(|m| m.symbol.clone());
            slots.push(slot);
        }
    }
    Some(Hull {
        name: crate::ships::ship_name(&symbol),
        manufacturer: text(p, "manufacturer").unwrap_or_default(),
        pad,
        hull_cost: num(p, "hullCost"),
        // Beside the properties, not in them.
        retail_cost: num(v, "retailCost"),
        hull_mass: num(p, "hullMass"),
        fighter_hangars: p.get("fighterHangars").and_then(Value::as_bool).unwrap_or(false),
        luxury_cabins: p.get("luxuryCabins").and_then(Value::as_bool).unwrap_or(false),
        vessel_hangars_mk2: p.get("vesselHangarsMkII").and_then(Value::as_bool).unwrap_or(false),
        vehicle_hangars_mk2: p.get("vehicleHangarsMkII").and_then(Value::as_bool).unwrap_or(false),
        speed: num(p, "speed"),
        boost: num(p, "boost"),
        boost_energy: num(p, "boostEnergy"),
        boost_cooldown: num(p, "boostInt"),
        minthrust: num(p, "minthrust") / 100.0,
        pitch: num(p, "pitch"),
        yaw: num(p, "yaw"),
        roll: num(p, "roll"),
        turn_per_pip: num(p, "pipSpeed"),
        base_shield: num(p, "baseShieldStrength"),
        base_armour: num(p, "baseArmour"),
        hardness: num(p, "hardness"),
        heat_capacity: num(p, "heatCapacity"),
        masslock: num(p, "masslock"),
        crew: num(p, "crew") as u8,
        reserve_fuel: num(p, "reserveFuelCapacity"),
        bulkheads,
        slots,
        symbol,
    })
}

/// A slot's class: a number, or `{ "class": n, … }`.
fn slot_class(v: &Value) -> Option<u8> {
    v.as_u64().or_else(|| v.get("class").and_then(Value::as_u64)).map(|c| c as u8)
}

// ---------------------------------------------------------------------------------------------
// A plan

/// A planned loadout: a hull, its bulkheads and what's in each slot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    /// Lower-case journal symbol of the hull.
    pub hull: String,
    /// The bulkheads' symbol; empty: the hull's first (lightweight).
    pub bulkheads: String,
    /// The bulkheads' engineered figures (`mass`, `hullboost`, resistances), as the game reported them.
    pub bulkheads_engineered: FxHashMap<String, f64>,
    /// The bulkheads' engineering as the game wrote it, to write back out.
    pub bulkheads_engineering: Option<Value>,
    /// Engineering planned for the bulkheads.
    pub bulkheads_goal: Option<Goal>,
    pub fitted: Vec<Fitted>,
    /// The cargo hatch is on, and its power priority (0–4; the game shows 1–5).
    pub hatch: bool,
    pub hatch_priority: u8,
}

/// A module in a slot.
#[derive(Debug, Clone, PartialEq)]
pub struct Fitted {
    pub slot: String,
    /// Lower-case journal symbol.
    pub item: String,
    pub on: bool,
    /// Power priority, 0–4 (the game shows 1–5).
    pub priority: u8,
    /// Engineered figures by coriolis-data's name (`optmass`, `mass`, `power`), as the game reported them.
    pub engineered: FxHashMap<String, f64>,
    /// The journal's blueprint symbol and grade, and how far that grade is rolled (0–1).
    pub blueprint: Option<(String, u8)>,
    pub quality: Option<f64>,
    /// The experimental effect's symbol (`special_fsd_heavy`).
    pub experimental: Option<String>,
    /// The engineering as the game wrote it (blueprint, grade, modifiers), to write back out: EDSY and Coriolis show
    /// it from that.
    pub engineering: Option<Value>,
    /// Engineering planned for it.
    pub goal: Option<Goal>,
}

/// Engineering planned for a module: a blueprint to a grade rolled some or all of the way, and an experimental effect.
#[derive(Debug, Clone, PartialEq)]
pub struct Goal {
    /// The journal's blueprint symbol (`FSD_LongRange`).
    pub blueprint: String,
    pub grade: u8,
    /// How far through the grade the rolls get: 0 its start, 1 its end (a full roll).
    pub roll: f64,
    /// The experimental effect's symbol.
    pub experimental: Option<String>,
    /// Figures typed in exactly (to match a module the game rolled), by coriolis-data's name: they replace what the
    /// roll works out.
    pub exact: Vec<(String, f64)>,
}

impl Goal {
    /// A blueprint to `grade`, rolled all the way, with no experimental effect.
    pub fn new(blueprint: &str, grade: u8) -> Goal {
        Goal { blueprint: blueprint.to_string(), grade, roll: 1.0, experimental: None, exact: Vec::new() }
    }
}

/// The figures planned engineering changes on a module (or bulkheads) `item`, as (coriolis-data's name, what it comes
/// to, exact values included, and the module's own), by name; empty when this data has no figures for the goal. The
/// ends of a curve, which move with its middle, aren't listed.
pub fn goal_figures(item: &str, goal: &Goal) -> Vec<(String, f64, f64)> {
    let item = item.trim().to_ascii_lowercase();
    let bulkhead = hulls().iter().flat_map(|h| h.bulkheads.iter()).find(|b| b.symbol == item);
    let base = |k: &str| match (bulkhead, module(&item)) {
        (Some(b), _) => bulkhead_figure(b, k),
        (None, Some(m)) => m.base(k),
        (None, None) => None,
    };
    let Some(e) = blueprints::planned_goal(&item, &base, goal) else { return Vec::new() };
    let mut out: Vec<(String, f64, f64)> = e.stats.iter().filter(|(k, _)| !blueprints::is_curve_end(k)).filter_map(|(k, v)| Some((k.clone(), *v, base(k)?))).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Rolls planned per grade, as the Engineering page plans them by default.
pub const ROLLS_PER_GRADE: u32 = 3;

/// The materials a plan's engineering goals take, by journal symbol: each blueprint from the grade above the one the
/// module has of it (else grade 1) to its goal, [`ROLLS_PER_GRADE`] rolls a grade, and each experimental effect not on
/// it yet. Engineering already done isn't counted again.
pub fn goal_materials(plan: &Plan) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    let mut add = |symbol: &str, n: u32| match out.iter_mut().find(|(s, _)| s == symbol) {
        Some((_, c)) => *c = c.saturating_add(n),
        None => out.push((symbol.to_string(), n)),
    };
    // Each module with a goal, and the bulkheads: what's there now, and what's planned.
    let bulkheads = plan.bulkheads_goal.as_ref().map(|g| (plan.bulkheads.as_str(), g, plan.bulkheads_done(), plan.bulkheads_experimental()));
    let modules = plan.fitted.iter().filter_map(|f| f.goal.as_ref().map(|g| (f.item.as_str(), g, f.blueprint.clone(), f.experimental.as_deref())));
    for (item, goal, done, experimental) in bulkheads.into_iter().chain(modules) {
        if let Some(bp) = crate::engineering::blueprint_for(item, &goal.blueprint) {
            let from = match &done {
                Some((done, grade)) if bp.is(done) => grade.saturating_add(1),
                _ => 1,
            };
            if from <= goal.grade {
                for (s, n) in crate::trader::blueprint_shopping(bp, from, goal.grade, ROLLS_PER_GRADE) {
                    add(&s, n);
                }
            }
        }
        // An effect the module has already, under any of its journal spellings, isn't gathered again.
        let has = |x: &str| experimental.is_some_and(|y| same_experimental(x, y));
        if let Some(x) = goal.experimental.as_deref().filter(|x| !has(x))
            && let Some(e) = crate::engineering::experimental(x)
        {
            for i in &e.cost {
                add(&i.symbol, i.count);
            }
        }
    }
    out
}

/// Two experimental effects are the same one: by the Engineering page's data (which knows each one's other journal
/// spellings, such as a rail gun's `_cooled`), else by symbol in any case.
fn same_experimental(a: &str, b: &str) -> bool {
    match (crate::engineering::experimental(a), crate::engineering::experimental(b)) {
        (Some(x), Some(y)) => std::ptr::eq(x, y),
        _ => a.trim().eq_ignore_ascii_case(b.trim()),
    }
}

impl Fitted {
    pub fn new(slot: &str, item: &str) -> Fitted {
        Fitted { slot: slot.to_string(), item: item.trim().to_ascii_lowercase(), on: true, priority: 0, engineered: FxHashMap::default(), blueprint: None, quality: None, experimental: None, engineering: None, goal: None }
    }

    /// A figure of this module as fitted: engineered, or its own.
    pub fn get(&self, data: &ModuleData, key: &str) -> f64 {
        self.engineered.get(key).copied().unwrap_or_else(|| match key {
            "mass" => data.mass,
            "power" => data.power,
            "cost" => data.cost,
            _ => data.get(key),
        })
    }
}

impl Plan {
    /// A new hull as the shipyard sells it, with its standard modules.
    pub fn stock(hull: &Hull) -> Plan {
        let fitted = hull.slots.iter().filter_map(|s| s.default.as_ref().map(|d| Fitted::new(&s.name, d))).collect();
        Plan { hull: hull.symbol.clone(), bulkheads: hull.bulkheads.first().map(|b| b.symbol.clone()).unwrap_or_default(), fitted, hatch: true, hatch_priority: 2, ..Plan::default() }
    }

    /// From a journal `Loadout` or a SLEF export's build, with the game's engineered values.
    pub fn from_loadout(l: &Value) -> Option<Plan> {
        let hull = l.get("Ship")?.as_str()?.trim().to_ascii_lowercase();
        let mut plan = Plan { hull, hatch: true, hatch_priority: 2, ..Plan::default() };
        for m in l.get("Modules").and_then(Value::as_array).into_iter().flatten() {
            let (Some(slot), Some(item)) = (m.get("Slot").and_then(Value::as_str), m.get("Item").and_then(Value::as_str)) else { continue };
            let on = m.get("On").and_then(Value::as_bool).unwrap_or(true);
            let item = item.trim().to_ascii_lowercase();
            if slot.eq_ignore_ascii_case("Armour") {
                let base = hull_ref(&plan.hull).and_then(|h| h.bulkheads.iter().find(|b| b.symbol == item));
                match (m.get("Engineering"), base) {
                    // Engineering planned in Elite Nexus comes back as planned.
                    (Some(e), Some(b)) if goal_of(&b.symbol, &|k| bulkhead_figure(b, k), e).is_some() => {
                        plan.bulkheads_goal = goal_of(&b.symbol, &|k| bulkhead_figure(b, k), e);
                    }
                    (Some(e), b) => {
                        if let Some(b) = b {
                            plan.bulkheads_engineered = engineered(&|k| bulkhead_figure(b, k), e);
                        }
                        plan.bulkheads_engineering = Some(e.clone());
                    }
                    (None, _) => {}
                }
                plan.bulkheads = item;
                continue;
            }
            if slot.eq_ignore_ascii_case("CargoHatch") {
                plan.hatch = on;
                plan.hatch_priority = m.get("Priority").and_then(Value::as_u64).unwrap_or(2).min(4) as u8;
                continue;
            }
            if matches!(outfitting::slot_kind(slot), SlotKind::Cosmetic | SlotKind::Other) {
                continue;
            }
            let mut f = Fitted::new(slot, &item);
            f.on = on;
            f.priority = m.get("Priority").and_then(Value::as_u64).unwrap_or(0).min(4) as u8;
            let planned = m.get("Engineering").zip(module(&f.item)).and_then(|(e, data)| goal_of(&f.item, &|k| data.base(k), e));
            if planned.is_some() {
                // Engineering planned in Elite Nexus comes back as planned.
                f.goal = planned;
            } else if let Some(e) = m.get("Engineering") {
                f.blueprint = e.get("BlueprintName").and_then(Value::as_str).map(|b| (b.to_string(), e.get("Level").and_then(Value::as_u64).unwrap_or(0).min(5) as u8));
                f.quality = e.get("Quality").and_then(Value::as_f64).filter(|q| q.is_finite()).map(|q| q.clamp(0.0, 1.0));
                f.experimental = e.get("ExperimentalEffect").and_then(Value::as_str).map(str::to_string);
                if let Some(data) = module(&f.item) {
                    f.engineered = engineered(&|k| data.base(k), e);
                }
                f.engineering = Some(e.clone());
            }
            plan.fitted.push(f);
        }
        plan.in_slot_order();
        Some(plan)
    }

    pub fn module_in(&self, slot: &str) -> Option<&Fitted> {
        self.fitted.iter().find(|f| f.slot.eq_ignore_ascii_case(slot))
    }

    /// The blueprint and grade the game reported on the bulkheads.
    pub fn bulkheads_done(&self) -> Option<(String, u8)> {
        let e = self.bulkheads_engineering.as_ref()?;
        Some((e.get("BlueprintName")?.as_str()?.to_string(), e.get("Level").and_then(Value::as_u64).unwrap_or(0).min(5) as u8))
    }

    /// The experimental effect the game reported on the bulkheads.
    pub fn bulkheads_experimental(&self) -> Option<&str> {
        self.bulkheads_engineering.as_ref()?.get("ExperimentalEffect")?.as_str()
    }

    /// Puts `item` in `slot`, plain (none: empties it).
    pub fn set(&mut self, slot: &str, item: Option<&str>) {
        match item {
            Some(item) => self.put(Fitted::new(slot, item)),
            None => self.fitted.retain(|f| !f.slot.eq_ignore_ascii_case(slot)),
        }
    }

    /// Puts a module in its slot as it is, engineering and all (one picked back as it was), in place of what's there.
    pub fn put(&mut self, f: Fitted) {
        match self.fitted.iter_mut().find(|x| x.slot.eq_ignore_ascii_case(&f.slot)) {
            Some(x) => *x = f,
            None => {
                self.fitted.push(f);
                self.in_slot_order();
            }
        }
    }

    /// The modules in the hull's slot order, so two plans with the same modules are equal however they were made.
    fn in_slot_order(&mut self) {
        let Some(hull) = hull_ref(&self.hull) else { return };
        let at = |slot: &str| hull.slots.iter().position(|s| s.name.eq_ignore_ascii_case(slot)).unwrap_or(usize::MAX);
        self.fitted.sort_by_key(|f| at(&f.slot));
    }

    /// The build as a journal `Loadout` event, for Where to buy and saved builds, with the engineering the game
    /// reported (engineering planned here isn't in it: the game hasn't rolled it).
    pub fn to_loadout(&self) -> Value {
        self.loadout(false)
    }

    /// The build as a journal `Loadout` event with the engineering planned here written as the game writes engineering
    /// (blueprint, grade, roll, experimental effect and each figure it changes), so EDSY and Coriolis show the plan.
    /// Planned engineering this data has no figures for keeps what the game reported.
    pub fn to_loadout_planned(&self) -> Value {
        self.loadout(true)
    }

    fn loadout(&self, with_plans: bool) -> Value {
        let entry = |slot: &str, item: &str, on: bool, priority: u8, engineering: Option<Value>| {
            let mut m = serde_json::json!({ "Slot": slot, "Item": item, "On": on, "Priority": priority });
            if let (Some(e), Some(o)) = (engineering, m.as_object_mut()) {
                o.insert("Engineering".into(), e);
            }
            m
        };
        let mut modules = Vec::new();
        if !self.bulkheads.is_empty() {
            let base = hull_ref(&self.hull).and_then(|h| h.bulkheads.iter().find(|b| b.symbol == self.bulkheads));
            let goal = match (with_plans, &self.bulkheads_goal, base) {
                (true, Some(g), Some(b)) => planned_engineering("bh", &b.symbol, &|k| bulkhead_figure(b, k), g),
                _ => None,
            };
            modules.push(entry("Armour", &self.bulkheads, true, 1, goal.or_else(|| self.bulkheads_engineering.clone())));
        }
        modules.push(entry("CargoHatch", "modularcargobaydoor", self.hatch, self.hatch_priority, None));
        for f in &self.fitted {
            let goal = match (with_plans, &f.goal, module(&f.item)) {
                (true, Some(g), Some(m)) => planned_engineering(&m.grp, &f.item, &|k| m.base(k), g),
                _ => None,
            };
            modules.push(entry(&f.slot, &f.item, f.on, f.priority, goal.or_else(|| f.engineering.clone())));
        }
        serde_json::json!({ "event": "Loadout", "Ship": self.hull, "Modules": modules })
    }
}

/// The field that marks an `Engineering` written from a plan (the game never writes it).
const PLANNED_MARK: &str = "EliteNexusPlanned";

/// Engineering written from a plan ([`Plan::to_loadout_planned`]) as the goal it came from: the blueprint, grade, roll
/// and effect, and as exact values the figures that differ from what those work out to (values typed in).
fn goal_of(item: &str, base: &dyn Fn(&str) -> Option<f64>, e: &Value) -> Option<Goal> {
    if e.get(PLANNED_MARK).and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let mut goal = Goal::new(e.get("BlueprintName")?.as_str()?, e.get("Level").and_then(Value::as_u64).unwrap_or(1).clamp(1, 5) as u8);
    goal.roll = e.get("Quality").and_then(Value::as_f64).filter(|q| q.is_finite()).map_or(1.0, |q| q.clamp(0.0, 1.0));
    goal.experimental = e.get("ExperimentalEffect").and_then(Value::as_str).map(str::to_string);
    let rolled = planned_goal(item, base, &goal).map(|p| p.stats).unwrap_or_default();
    let mut exact: Vec<(String, f64)> = engineered(base, e)
        .into_iter()
        .filter(|(k, v)| !blueprints::is_curve_end(k) && rolled.get(k).is_none_or(|r| (r - v).abs() > 1e-6 * r.abs().max(1.0)))
        .collect();
    exact.sort_by(|a, b| a.0.cmp(&b.0));
    goal.exact = exact;
    Some(goal)
}

/// Planned engineering as the game writes a module's `Engineering`: the blueprint, its grade, the roll as its quality,
/// the experimental effect, and each figure it changes with the game's label, value and original value. None when this
/// data has no figures for the blueprint at that grade.
fn planned_engineering(grp: &str, item: &str, base: &dyn Fn(&str) -> Option<f64>, g: &Goal) -> Option<Value> {
    let e = planned_goal(item, base, g)?;
    let mut keys: Vec<&String> = e.stats.keys().collect();
    keys.sort();
    let modifiers: Vec<Value> = keys
        .into_iter()
        .filter_map(|k| {
            let (label, scale, less_is_good) = journal_label(grp, k)?;
            let (value, original) = (*e.stats.get(k)?, base(k)?);
            ((value - original).abs() > 1e-9 && value.is_finite()).then(|| serde_json::json!({ "Label": label, "Value": value * scale, "OriginalValue": original * scale, "LessIsGood": u8::from(less_is_good) }))
        })
        .collect();
    // Marked as planned, so a build pasted back reads it as planned again rather than as the game's own.
    let mut out = serde_json::json!({ "BlueprintName": g.blueprint, "Level": g.grade, "Quality": g.roll.clamp(0.0, 1.0), "Modifiers": modifiers, PLANNED_MARK: true });
    if let (Some(x), Some(o)) = (&g.experimental, out.as_object_mut()) {
        o.insert("ExperimentalEffect".into(), Value::from(x.as_str()));
        if let Some(name) = crate::engineering::experimental(x) {
            o.insert("ExperimentalEffect_Localised".into(), Value::from(name.name.as_str()));
        }
    }
    Some(out)
}

/// The game's label for a figure of a module of group `grp`, what to multiply the figure by to write it the game's
/// way (resistances and boosts in percent), and whether less is better. The reverse of [`engineered`]; None for a
/// figure the game doesn't list (a curve's ends, which move with its middle).
fn journal_label(grp: &str, key: &str) -> Option<(&'static str, f64, bool)> {
    let shield_generator = matches!(grp, "sg" | "bsg" | "psg");
    Some(match key {
        "mass" => ("Mass", 1.0, true),
        "integrity" => ("Integrity", 1.0, false),
        "power" => ("PowerDraw", 1.0, true),
        "boot" => ("BootTime", 1.0, true),
        "optmass" if grp == "fsd" => ("FSDOptimalMass", 1.0, false),
        "optmass" if grp == "t" => ("EngineOptimalMass", 1.0, false),
        "optmass" if shield_generator => ("ShieldGenOptimalMass", 1.0, false),
        "optmul" if grp == "t" => ("EngineOptPerformance", 100.0, false),
        "optmul" if shield_generator => ("ShieldGenStrength", 100.0, false),
        "maxfuel" => ("MaxFuelPerJump", 1.0, false),
        "pgen" => ("PowerCapacity", 1.0, false),
        "eff" => ("HeatEfficiency", 1.0, true),
        "engcap" => ("EnginesCapacity", 1.0, false),
        "engrate" => ("EnginesRecharge", 1.0, false),
        "wepcap" => ("WeaponsCapacity", 1.0, false),
        "weprate" => ("WeaponsRecharge", 1.0, false),
        "syscap" => ("SystemsCapacity", 1.0, false),
        "sysrate" => ("SystemsRecharge", 1.0, false),
        "kinres" => ("KineticResistance", 100.0, false),
        "thermres" => ("ThermicResistance", 100.0, false),
        "explres" => ("ExplosiveResistance", 100.0, false),
        "causres" => ("CausticResistance", 100.0, false),
        "shieldboost" => ("DefenceModifierShieldMultiplier", 100.0, false),
        "shieldaddition" => ("DefenceModifierShieldAddition", 1.0, false),
        "hullboost" => ("DefenceModifierHealthMultiplier", 100.0, false),
        "hullreinforcement" => ("DefenceModifierHealthAddition", 1.0, false),
        "regen" => ("RegenRate", 1.0, false),
        "brokenregen" => ("BrokenRegenRate", 1.0, false),
        "distdraw" if shield_generator => ("EnergyPerRegen", 1.0, true),
        "distdraw" => ("DistributorDraw", 1.0, true),
        "thermload" if grp == "scb" => ("ShieldBankHeat", 1.0, true),
        "thermload" => ("ThermalLoad", 1.0, true),
        "damage" => ("Damage", 1.0, false),
        "rof" => ("RateOfFire", 1.0, false),
        "clip" => ("AmmoClipSize", 1.0, false),
        "ammo" => ("AmmoMaximum", 1.0, false),
        "reload" => ("ReloadTime", 1.0, true),
        "range" if grp != "s" => ("MaximumRange", 1.0, false),
        "falloff" => ("DamageFalloffRange", 1.0, false),
        "piercing" => ("ArmourPenetration", 1.0, false),
        "shotspeed" => ("ShotSpeed", 1.0, false),
        "jitter" => ("Jitter", 1.0, true),
        "burst" => ("BurstSize", 1.0, false),
        "burstrof" => ("BurstRateOfFire", 1.0, false),
        "fuel" => ("FuelCapacity", 1.0, false),
        "cargo" => ("CargoCapacity", 1.0, false),
        "shieldreinforcement" => ("ShieldBankReinforcement", 1.0, false),
        "duration" => ("ShieldBankDuration", 1.0, false),
        "spinup" => ("ShieldBankSpinUp", 1.0, true),
        _ => return None,
    })
}

fn hull_ref(symbol: &str) -> Option<&'static Hull> {
    hull(symbol)
}

fn bulkhead_figure(b: &Bulkhead, key: &str) -> Option<f64> {
    Some(match key {
        "mass" => b.mass,
        "hullboost" => b.hullboost,
        "kinres" => b.res.kinetic,
        "thermres" => b.res.thermal,
        "explres" => b.res.explosive,
        "causres" => b.res.caustic,
        _ => return None,
    })
}

/// The game's engineered values for the figures the sums use, by coriolis-data's names. `base` gives a figure's
/// unengineered value (None: the module has no such figure). A mass curve's masses or multipliers move together,
/// as the game moves the whole curve.
fn engineered(base: &dyn Fn(&str) -> Option<f64>, e: &Value) -> FxHashMap<String, f64> {
    let mut out = FxHashMap::default();
    for md in e.get("Modifiers").and_then(Value::as_array).into_iter().flatten() {
        let Some(raw) = md.get("Label").and_then(Value::as_str) else { continue };
        // The game's labels, in any case, with or without the "OutfittingFieldType_" and "PerSecond" some sources add.
        let lower = raw.trim().to_ascii_lowercase();
        let lower = lower.trim_start_matches("outfittingfieldtype_").replace("persecond", "");
        let label = match lower.as_str() {
            "mass" => "Mass",
            "powerdraw" => "PowerDraw",
            "powercapacity" => "PowerCapacity",
            "enginescapacity" => "EnginesCapacity",
            "integrity" => "Integrity",
            "fsdoptimalmass" => "FSDOptimalMass",
            "maxfuelperjump" | "fsdmaxfuelperjump" => "MaxFuelPerJump",
            "engineoptimalmass" => "EngineOptimalMass",
            "shieldgenoptimalmass" => "ShieldGenOptimalMass",
            "engineoptperformance" => "EngineOptPerformance",
            "shieldgenstrength" => "ShieldGenStrength",
            "kineticresistance" => "KineticResistance",
            "thermicresistance" => "ThermicResistance",
            "explosiveresistance" => "ExplosiveResistance",
            "causticresistance" => "CausticResistance",
            "defencemodifiershieldmultiplier" => "DefenceModifierShieldMultiplier",
            "defencemodifiershieldaddition" => "DefenceModifierShieldAddition",
            "defencemodifierhealthaddition" => "DefenceModifierHealthAddition",
            "defencemodifierhealthmultiplier" => "DefenceModifierHealthMultiplier",
            "shieldbankreinforcement" => "ShieldBankReinforcement",
            "shieldbankduration" => "ShieldBankDuration",
            "shieldbankheat" => "ShieldBankHeat",
            "fuelcapacity" => "FuelCapacity",
            "cargocapacity" => "CargoCapacity",
            "damage" => "Damage",
            "rateoffire" => "RateOfFire",
            "distributordraw" => "DistributorDraw",
            "thermalload" => "ThermalLoad",
            "ammoclipsize" | "weapon_clip_size_override" => "AmmoClipSize",
            "ammomaximum" => "AmmoMaximum",
            "reloadtime" => "ReloadTime",
            "maximumrange" => "MaximumRange",
            "falloffrange" | "damagefalloffrange" => "FalloffRange",
            "armourpenetration" => "ArmourPenetration",
            "shotspeed" => "ShotSpeed",
            "jitter" => "Jitter",
            "burstsize" | "weapon_burst_size" => "BurstSize",
            "burstrateoffire" | "weapon_burst_rof" => "BurstRateOfFire",
            "regenrate" => "RegenRate",
            "brokenregenrate" => "BrokenRegenRate",
            "energyperregen" => "EnergyPerRegen",
            "weaponscapacity" => "WeaponsCapacity",
            "weaponsrecharge" => "WeaponsRecharge",
            "enginesrecharge" => "EnginesRecharge",
            "systemscapacity" => "SystemsCapacity",
            "systemsrecharge" => "SystemsRecharge",
            "heatefficiency" => "HeatEfficiency",
            "moduledefenceabsorption" => "ModuleDefenceAbsorption",
            _ => continue,
        };
        let Some(value) = md.get("Value").and_then(Value::as_f64).filter(|v| v.is_finite()) else { continue };
        let original = md.get("OriginalValue").and_then(Value::as_f64).filter(|v| v.is_finite() && *v != 0.0);
        // A curve's figures, each by the share the game moved the one it reports.
        let scaled = |keys: &[&'static str]| -> Vec<(&'static str, f64)> {
            let Some(o) = original else { return Vec::new() };
            keys.iter().filter_map(|k| base(k).map(|b| (*k, b * value / o))).collect()
        };
        let (key, v) = match label {
            "Mass" => ("mass", value),
            "PowerDraw" => ("power", value),
            "PowerCapacity" => ("pgen", value),
            "EnginesCapacity" => ("engcap", value),
            "Integrity" => ("integrity", value),
            "FSDOptimalMass" => ("optmass", value),
            "MaxFuelPerJump" => ("maxfuel", value),
            "EngineOptimalMass" | "ShieldGenOptimalMass" | "EngineOptPerformance" | "ShieldGenStrength" => {
                let keys: &[&'static str] = match label {
                    "EngineOptPerformance" => &["minmul", "optmul", "maxmul", "minmulspeed", "optmulspeed", "maxmulspeed"],
                    "ShieldGenStrength" => &["minmul", "optmul", "maxmul"],
                    _ => &["minmass", "optmass", "maxmass"],
                };
                for (k, x) in scaled(keys) {
                    out.insert(k.to_string(), x);
                }
                continue;
            }
            "KineticResistance" => ("kinres", value / 100.0),
            "ThermicResistance" => ("thermres", value / 100.0),
            "ExplosiveResistance" => ("explres", value / 100.0),
            "CausticResistance" => ("causres", value / 100.0),
            "DefenceModifierShieldMultiplier" => ("shieldboost", value / 100.0),
            "DefenceModifierShieldAddition" => ("shieldaddition", value),
            "DefenceModifierHealthAddition" => ("hullreinforcement", value),
            "DefenceModifierHealthMultiplier" => ("hullboost", value / 100.0),
            "ShieldBankReinforcement" => ("shieldreinforcement", value),
            "ShieldBankDuration" => ("duration", value),
            "ShieldBankHeat" => ("thermload", value),
            "FuelCapacity" => ("fuel", value),
            "CargoCapacity" => ("cargo", value),
            // Weapons, in the units the data has them: damage a shot, shots a second, metres, seconds.
            "Damage" => ("damage", value),
            "RateOfFire" => ("rof", value),
            "DistributorDraw" => ("distdraw", value),
            "ThermalLoad" => ("thermload", value),
            "AmmoClipSize" => ("clip", value),
            "AmmoMaximum" => ("ammo", value),
            "ReloadTime" => ("reload", value),
            "MaximumRange" => ("range", value),
            "FalloffRange" => ("falloff", value),
            "ArmourPenetration" => ("piercing", value),
            "ShotSpeed" => ("shotspeed", value),
            "Jitter" => ("jitter", value),
            "BurstSize" => ("burst", value),
            "BurstRateOfFire" => ("burstrof", value),
            // Shield generators and the power distributor.
            "RegenRate" => ("regen", value),
            "BrokenRegenRate" => ("brokenregen", value),
            "EnergyPerRegen" => ("distdraw", value),
            "WeaponsCapacity" => ("wepcap", value),
            "WeaponsRecharge" => ("weprate", value),
            "EnginesRecharge" => ("engrate", value),
            "SystemsCapacity" => ("syscap", value),
            "SystemsRecharge" => ("sysrate", value),
            // Units the data may hold differently: by the share the game moved them.
            "HeatEfficiency" | "ModuleDefenceAbsorption" => {
                let key = if label == "HeatEfficiency" { "eff" } else { "protection" };
                for (k, x) in scaled(&[key]) {
                    out.insert(k.to_string(), x);
                }
                continue;
            }
            _ => continue,
        };
        out.insert(key.to_string(), v);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// What a plan comes to

/// What a plan comes to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Figures {
    /// What the figures that depend on it were worked out for: pips, cargo and fuel carried.
    pub setup: Setup,
    /// The hull alone, and with modules but no fuel or cargo: the game's "unladen mass", t.
    pub hull_mass: f64,
    pub unladen_mass: f64,
    /// Main tank and any internal tanks, t.
    pub fuel: f64,
    pub cargo: f64,
    pub passengers: f64,
    /// No cargo and fuel for one jump: the game's "max jump range", ly.
    pub max_range: f64,
    /// With a full tank and an empty hold.
    pub full_tank_range: f64,
    /// With a full tank and a full hold.
    pub laden_range: f64,
    /// With the cargo and fuel of `setup`.
    pub range_carrying: f64,
    /// Jumping on one full tank until it runs dry, empty hold.
    pub total_range: f64,
    /// The most fuel the frame shift drive burns in one jump, t.
    pub max_fuel_per_jump: f64,
    /// What a Guardian FSD booster adds to every jump, ly (part of the ranges above).
    pub booster: f64,
    /// What a neutron star's cone multiplies the drive's range by (6 for the FSD Mk II, else 4), and a white dwarf's
    /// (None: not known for this drive).
    pub neutron_boost: f64,
    pub white_dwarf_boost: Option<f64>,
    /// Four pips to engines, full tank, empty hold, m/s. 0: the thrusters can't move this mass.
    pub speed: f64,
    /// Top speed by pips to engines, 0 to 4, with the cargo and fuel of `setup`.
    pub speed_by_pips: [f64; 5],
    /// Four pips to engines, full tank, empty hold; and with `setup`'s cargo and fuel. 0 when the power distributor's
    /// engines capacitor holds less than a boost takes.
    pub boost: f64,
    pub boost_carrying: f64,
    /// What a boost takes from the engines capacitor, and what the power distributor's holds, MJ.
    pub boost_energy: f64,
    pub engines_capacity: f64,
    /// Seconds between boosts at `setup`'s engine pips: what the engines capacitor takes to refill one, but never
    /// less than the hull allows; None: it doesn't refill.
    pub boost_interval: Option<f64>,
    /// Turn rates at `setup`'s engine pips, cargo and fuel, degrees a second.
    pub pitch: f64,
    pub yaw: f64,
    pub roll: f64,
    /// MW.
    pub power_generated: f64,
    /// Drawn with the hardpoints retracted, and deployed.
    pub power_retracted: f64,
    pub power_deployed: f64,
    /// Power priority groups 1 to 5: what each and those before it draw.
    pub power_groups: [PowerGroup; 5],
    pub shield: Option<Shield>,
    /// Hull points.
    pub armour: f64,
    pub hull_res: Resistances,
    pub hardness: f64,
    /// The share of damage to modules that module reinforcement packages take, and what they can take in all.
    pub module_protection: f64,
    pub module_armour: f64,
    pub heat: Heat,
    pub weapons: Vec<Weapon>,
    /// The power distributor's weapons capacitor at `setup`'s weapon pips, every weapon firing.
    pub wep: Capacitor,
    pub crew: u8,
    pub masslock: f64,
    /// The hull with its bulkheads, list price, Cr.
    pub hull_cost: f64,
    pub modules_cost: f64,
    /// Restocking every module's ammunition from empty, list price, Cr.
    pub ammo_cost: f64,
    /// Modules this data has no figures for (newer than it): not counted.
    pub unknown: Vec<String>,
}

impl Figures {
    pub fn total_cost(&self) -> f64 {
        self.hull_cost + self.modules_cost
    }

    /// The insurance excess: 5 % of the ship's value.
    pub fn rebuy(&self) -> f64 {
        (self.total_cost() * 0.05).ceil()
    }
}

/// Shields, MJ.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Shield {
    /// The generator, boosters and Guardian shield reinforcement together.
    pub strength: f64,
    pub generator: f64,
    pub boosters: f64,
    pub addition: f64,
    /// What every shield cell bank's charges add, all fired.
    pub cells: f64,
    /// The generator's resistances with its boosters'.
    pub res: Resistances,
    /// The share of damage `setup`'s systems pips stop, on top of the resistances.
    pub sys_res: f64,
    /// MJ a second while up, and while broken.
    pub regen: f64,
    pub broken_regen: f64,
    /// Seconds from half to full; from broken back to half, the 16 seconds before they start included. None: never,
    /// at these systems pips.
    pub recharge: Option<f64>,
    pub recover: Option<f64>,
}

impl Shield {
    /// What the shields take of one damage type before they fall, with the systems pips: MJ / the damage that gets
    /// through.
    pub fn against(&self, res: f64) -> f64 {
        let through = (1.0 - res) * (1.0 - self.sys_res);
        if through > 0.0 { self.strength / through } else { f64::INFINITY }
    }
}

/// A fitted module and what its figures come to: planned engineering where it has a goal this data has figures for,
/// else the engineering the game reported, else the module's own.
pub struct Live<'a> {
    pub f: &'a Fitted,
    pub m: &'static ModuleData,
    planned: Option<Engineered>,
}

impl<'a> Live<'a> {
    pub fn new(f: &'a Fitted, m: &'static ModuleData) -> Live<'a> {
        let planned = f.goal.as_ref().and_then(|g| planned_goal(&f.item, &|k| m.base(k), g));
        Live { f, m, planned }
    }

    /// The fitted module's live figures, if this data has the module.
    pub fn of(f: &'a Fitted) -> Option<Live<'a>> {
        module(&f.item).map(|m| Live::new(f, m))
    }

    /// A figure, 0 when the module has none.
    pub fn get(&self, key: &str) -> f64 {
        self.opt(key).unwrap_or(0.0)
    }

    pub fn opt(&self, key: &str) -> Option<f64> {
        let changed = match &self.planned {
            Some(p) => p.stats.get(key),
            None => self.f.engineered.get(key),
        };
        changed.copied().or_else(|| self.m.base(key))
    }

    /// The figures come from engineering planned here, not the game's.
    pub fn is_planned(&self) -> bool {
        self.planned.is_some()
    }

    /// The experimental effect the figures include.
    pub fn experimental(&self) -> Option<&str> {
        if self.planned.is_some() { self.f.goal.as_ref().and_then(|g| g.experimental.as_deref()) } else { self.f.experimental.as_deref() }
    }

    /// The damage types it deals.
    pub fn damage(&self) -> DamageDist {
        let changed = match &self.planned {
            Some(p) => p.damage,
            None => self.f.experimental.as_deref().and_then(effect_damage),
        };
        changed.or(self.m.damage).unwrap_or_default()
    }
}

/// The mass curve thrusters and shield generators share: a multiplier for `mass` between the module's minimum and
/// maximum mass, as the game computes it.
fn mass_curve(min_mass: f64, opt_mass: f64, max_mass: f64, min_mul: f64, opt_mul: f64, max_mul: f64, mass: f64) -> f64 {
    if max_mass <= min_mass || max_mul <= min_mul || opt_mul <= min_mul {
        return opt_mul;
    }
    // A mass that isn't a number counts as the lightest (`clamp` would pass the NaN on).
    let x = (max_mass - mass) / (max_mass - min_mass);
    let xnorm = if x.is_nan() { 1.0 } else { x.clamp(0.0, 1.0) };
    let exponent = ((opt_mul - min_mul) / (max_mul - min_mul)).ln() / ((max_mass - opt_mass) / (max_mass - min_mass)).min(1.0).ln();
    min_mul + xnorm.powf(exponent) * (max_mul - min_mul)
}

/// Shield resistance with boosters: past 30 % below the generator's own damage, each further share counts half.
fn shield_res(generator: f64, boosters_damage: f64) -> f64 {
    let gen_dmg = 1.0 - generator;
    let combined = gen_dmg * boosters_damage;
    let max = gen_dmg * 0.7;
    let dmg = if combined < max { max / 2.0 + max / 2.0 * (combined / max) } else { combined };
    1.0 - dmg
}

/// Hull resistance: the bulkheads', then each hull reinforcement package from the weakest up, with diminishing
/// returns past 30 %, capped at 75 %.
fn hull_res(base: f64, mut packages: Vec<f64>) -> f64 {
    packages.sort_by(f64::total_cmp);
    let mut r = base;
    for p in packages {
        let p = if p > 0.3 { p * 2.0 - 0.3 } else { p };
        let lower = r.max(0.3);
        let stacked = 1.0 - (1.0 - r) * (1.0 - p);
        let capped = lower + (stacked - lower) / (1.0 - lower) * (0.65 - lower);
        r = if capped >= 0.3 { capped } else { stacked };
    }
    r.min(0.75)
}

fn is_hardpoint_slot(slot: &str) -> bool {
    matches!(outfitting::slot_kind(slot), SlotKind::Hardpoint | SlotKind::Utility)
}

/// What `plan` comes to with two pips everywhere, a full tank and an empty hold; `None` for a hull this data doesn't
/// have.
pub fn figures(plan: &Plan) -> Option<Figures> {
    figures_with(plan, &Setup::default())
}

/// What `plan` comes to with `setup`'s pips, cargo and fuel; `None` for a hull this data doesn't have.
pub fn figures_with(plan: &Plan, setup: &Setup) -> Option<Figures> {
    let hull = hull(&plan.hull)?;
    let bulkheads = hull.bulkheads.iter().find(|b| b.symbol == plan.bulkheads).or_else(|| hull.bulkheads.first());
    // Planned engineering on the bulkheads replaces what the game reported for them.
    let bulkheads_planned = match (bulkheads, &plan.bulkheads_goal) {
        (Some(b), Some(g)) => planned_goal(&b.symbol, &|k| bulkhead_figure(b, k), g),
        _ => None,
    };
    let bh = |key: &str, base: f64| match &bulkheads_planned {
        Some(p) => p.stats.get(key).copied().unwrap_or(base),
        None => plan.bulkheads_engineered.get(key).copied().unwrap_or(base),
    };
    let lives: Vec<Live> = plan.fitted.iter().filter_map(Live::of).collect();
    let in_slot = |slot: &str| lives.iter().find(|l| l.f.slot.eq_ignore_ascii_case(slot));
    let on = || lives.iter().filter(|l| l.f.on);
    let sum = |key: &str| lives.iter().map(|l| l.get(key)).sum::<f64>();
    let pips = setup.pips;

    let mut out = Figures {
        setup: *setup,
        hull_mass: hull.hull_mass,
        hardness: hull.hardness,
        crew: hull.crew,
        masslock: hull.masslock,
        unknown: plan.fitted.iter().filter(|f| module(&f.item).is_none()).map(|f| f.item.clone()).collect(),
        ..Figures::default()
    };
    out.unladen_mass = hull.hull_mass + bulkheads.map_or(0.0, |b| bh("mass", b.mass)) + sum("mass");
    out.fuel = sum("fuel");
    out.cargo = lives.iter().map(|l| l.get("cargo").round()).sum();
    out.passengers = sum("passengers");
    let fuel = setup.fuel.map_or(out.fuel, |f| f.clamp(0.0, out.fuel));
    let cargo = setup.cargo.clamp(0.0, out.cargo);

    // Jump range: one jump's fuel moves optmass × (fuel / fuelmul)^(1 / fuelpower) / mass light years; a Guardian FSD
    // booster (a ship takes one) adds its own distance. The game's own "max jump range" leaves the reserve tank out.
    if let Some(l) = in_slot("FrameShiftDrive") {
        let (opt, maxfuel, mul, power) = (l.get("optmass"), l.get("maxfuel"), l.get("fuelmul"), l.get("fuelpower"));
        let booster = on().filter(|l| l.m.grp == "gfsb").map(|l| l.get("jumpboost")).fold(0.0, f64::max);
        out.max_fuel_per_jump = maxfuel;
        out.booster = booster;
        // The Caspian's FSD Mk II supercharges further; how far a white dwarf takes it isn't known.
        let mk2 = l.f.item.contains("overchargebooster_mkii");
        (out.neutron_boost, out.white_dwarf_boost) = if mk2 { (6.0, None) } else { (4.0, Some(1.5)) };
        if opt > 0.0 && mul > 0.0 && power > 0.0 && maxfuel > 0.0 {
            let range = |mass: f64, fuel: f64| if fuel > 0.0 { (fuel.min(maxfuel) / mul).powf(1.0 / power) * opt / mass + booster } else { 0.0 };
            let one = maxfuel.min(out.fuel);
            out.max_range = range(out.unladen_mass + one, one);
            out.full_tank_range = range(out.unladen_mass + out.fuel, one);
            out.laden_range = range(out.unladen_mass + out.fuel + out.cargo, one);
            out.range_carrying = range(out.unladen_mass + fuel + cargo, maxfuel.min(fuel));
            // Jump by jump until the tank is dry; a build with odd figures (a drive that burns next to nothing, a
            // tank of billions of tonnes) stops at a thousand jumps.
            let (mut left, mut mass) = (out.fuel, out.unladen_mass + out.fuel);
            for _ in 0..1000 {
                if left <= 1e-9 {
                    break;
                }
                let burn = left.min(maxfuel);
                out.total_range += range(mass, burn);
                mass -= burn;
                left -= burn;
            }
        }
    }

    // Speed: the hull's speed through the thrusters' mass curve, with the fuel, the reserve and the cargo carried; fewer
    // pips to engines bring it down to the hull's minimum thrust. Turning follows the same pips. A boost takes more from
    // the engines capacitor than some power distributors hold, and it refills at the engine pips' rate.
    out.boost_energy = hull.boost_energy;
    let pd = in_slot("PowerDistributor");
    out.engines_capacity = pd.map_or(0.0, |l| l.get("engcap"));
    let pip = |eng: f64| hull.minthrust + (1.0 - hull.minthrust) * eng.clamp(0.0, 4.0) / 4.0;
    let can_boost = out.boost_energy <= 0.0 || out.engines_capacity <= 0.0 || out.engines_capacity >= out.boost_energy;
    if let Some(l) = in_slot("MainEngines") {
        let pick = |a: &str, b: &str| if l.m.figures.contains_key(a) { l.get(a) } else { l.get(b) };
        // The thrusters' multiplier at a mass; None past their maximum.
        let curve = |mass: f64, min: f64, opt: f64, max: f64| (mass <= l.get("maxmass")).then(|| mass_curve(l.get("minmass"), l.get("optmass"), l.get("maxmass"), min, opt, max, mass));
        let speed_at = |mass: f64| curve(mass, pick("minmulspeed", "minmul"), pick("optmulspeed", "optmul"), pick("maxmulspeed", "maxmul"));
        // The usual figures: a full tank and the reserve, an empty hold.
        if let Some(mul) = speed_at(out.unladen_mass + out.fuel + hull.reserve_fuel) {
            out.speed = mul * hull.speed;
            out.boost = if can_boost { mul * hull.boost } else { 0.0 };
        }
        // With what's carried.
        let mass = out.unladen_mass + fuel + cargo + hull.reserve_fuel;
        if let Some(mul) = speed_at(mass) {
            for (i, s) in out.speed_by_pips.iter_mut().enumerate() {
                *s = mul * hull.speed * pip(i as f64);
            }
            out.boost_carrying = if can_boost { mul * hull.boost } else { 0.0 };
        }
        if let Some(turn) = curve(mass, pick("minmulrotation", "minmul"), pick("optmulrotation", "optmul"), pick("maxmulrotation", "maxmul")) {
            let turning = turn * (1.0 - hull.turn_per_pip * (4.0 - pips.eng.clamp(0.0, 4.0))).max(0.0);
            (out.pitch, out.yaw, out.roll) = (hull.pitch * turning, hull.yaw * turning, hull.roll * turning);
        }
    }
    let eng_rate = pd.map_or(0.0, |l| l.get("engrate")) * Pips::recharge_share(pips.eng);
    out.boost_interval = (out.boost > 0.0 && hull.boost_energy > 0.0 && eng_rate > 0.0).then(|| (hull.boost_energy / eng_rate).max(hull.boost_cooldown));

    // Power: the power plant's output against what's switched on; hardpoints draw only when deployed, except the
    // utilities that run all the time. Each priority group counts what it and the groups before it draw.
    out.power_generated = in_slot("PowerPlant").map_or(0.0, |l| l.get("pgen"));
    let hatch = if plan.hatch { HATCH_POWER } else { 0.0 };
    let retracted = |l: &&Live| !is_hardpoint_slot(&l.f.slot) || l.m.passive;
    out.power_retracted = hatch + on().filter(retracted).map(|l| l.get("power")).sum::<f64>();
    out.power_deployed = hatch + on().map(|l| l.get("power")).sum::<f64>();
    for (g, group) in out.power_groups.iter_mut().enumerate() {
        let upto = |p: u8| usize::from(p.min(4)) <= g;
        let hatch = if plan.hatch && upto(plan.hatch_priority) { HATCH_POWER } else { 0.0 };
        group.retracted = hatch + on().filter(|l| upto(l.f.priority)).filter(retracted).map(|l| l.get("power")).sum::<f64>();
        group.deployed = hatch + on().filter(|l| upto(l.f.priority)).map(|l| l.get("power")).sum::<f64>();
    }

    // Shields: the generator's strength for the hull's own mass, boosters adding shares of it, Guardian shield
    // reinforcement packages their own. They regenerate from the systems capacitor, which the systems pips refill.
    if let Some(l) = on().find(|l| matches!(l.m.grp.as_str(), "sg" | "bsg" | "psg")) {
        let generator = hull.base_shield * mass_curve(l.get("minmass"), l.get("optmass"), l.get("maxmass"), l.get("minmul"), l.get("optmul"), l.get("maxmul"), hull.hull_mass);
        let boosters: Vec<&Live> = on().filter(|l| l.m.grp == "sb").collect();
        let boost: f64 = boosters.iter().map(|l| l.get("shieldboost")).sum();
        let addition: f64 = on().filter(|l| l.m.grp == "gsrp").map(|l| l.get("shieldaddition")).sum();
        let damage = |key: &str| boosters.iter().map(|l| 1.0 - l.get(key)).product::<f64>();
        let cells = on().filter(|l| l.m.grp == "scb").map(|l| l.get("shieldreinforcement") * l.get("duration") * (l.get("clip").max(1.0) + l.get("ammo"))).sum();
        let strength = generator * (1.0 + boost) + addition;
        let (syscap, sysrate) = pd.map_or((0.0, 0.0), |p| (p.get("syscap"), p.get("sysrate") * Pips::recharge_share(pips.sys)));
        let (regen, broken, draw) = (l.get("regen"), l.get("brokenregen"), l.get("distdraw"));
        out.shield = Some(Shield {
            strength,
            generator,
            boosters: generator * boost,
            addition,
            cells,
            res: Resistances {
                kinetic: shield_res(l.get("kinres"), damage("kinres")),
                thermal: shield_res(l.get("thermres"), damage("thermres")),
                explosive: shield_res(l.get("explres"), damage("explres")),
                caustic: 0.0,
            },
            sys_res: pips.sys_resistance(),
            regen,
            broken_regen: broken,
            recharge: combat::regen_time(strength / 2.0, regen, draw, syscap, sysrate),
            recover: combat::regen_time(strength / 2.0, broken, draw, syscap, sysrate).map(|t| t + 16.0),
        });
    }

    // Armour: the hull's base armour with its bulkheads' boost, plus hull reinforcement packages.
    let packages: Vec<&Live> = on().filter(|l| matches!(l.m.grp.as_str(), "hr" | "ghrp" | "mahr")).collect();
    let base = hull.base_armour;
    out.armour = base * (1.0 + bulkheads.map_or(0.0, |b| bh("hullboost", b.hullboost))) + packages.iter().map(|l| l.get("hullreinforcement") + base * l.get("hullboost")).sum::<f64>();
    let res = |key: &str, from: f64| hull_res(from, packages.iter().map(|l| l.get(key)).collect());
    let b = bulkheads.map(|b| b.res).unwrap_or_default();
    out.hull_res = Resistances {
        kinetic: res("kinres", bh("kinres", b.kinetic)),
        thermal: res("thermres", bh("thermres", b.thermal)),
        explosive: res("explres", bh("explres", b.explosive)),
        caustic: res("causres", bh("causres", b.caustic)),
    };
    let reinforcements: Vec<&Live> = on().filter(|l| matches!(l.m.grp.as_str(), "mrp" | "gmrp")).collect();
    out.module_protection = 1.0 - reinforcements.iter().map(|l| 1.0 - l.get("protection")).product::<f64>();
    out.module_armour = reinforcements.iter().map(|l| l.get("integrity")).sum();

    // Weapons and the weapons capacitor they draw on.
    out.weapons = on().filter(|l| is_hardpoint_slot(&l.f.slot)).filter_map(|l| combat::weapon(l)).collect();
    let (wepcap, weprate) = pd.map_or((0.0, 0.0), |p| (p.get("wepcap"), p.get("weprate") * Pips::recharge_share(pips.wep)));
    // Drained at what the weapons draw while firing: a capacitor empties within seconds, before a clip runs out and a
    // reload pauses the draw.
    out.wep = combat::capacitor(wepcap, weprate, out.weapons.iter().map(|w| w.eps).sum());

    // Heat: what the ship holds, what the power plant makes from what's drawn, and what firing and shield cells add.
    let eff = in_slot("PowerPlant").map_or(0.0, |l| l.get("eff"));
    out.heat = Heat {
        capacity: hull.heat_capacity,
        plant_retracted: out.power_retracted * eff,
        plant_deployed: out.power_deployed * eff,
        weapons: out.weapons.iter().map(|w| w.hps).sum(),
        cell: on().filter(|l| l.m.grp == "scb").map(|l| l.get("thermload")).fold(0.0, f64::max),
        sinks: on().filter(|l| l.m.grp == "hs").map(|l| (l.get("clip") + l.get("ammo")).max(0.0) as u32).sum(),
    };

    // A station's discount takes its share off the hull and every module (and so off the rebuy), not the ammunition.
    let keep = 1.0 - if setup.discount.is_finite() { setup.discount.clamp(0.0, 1.0) } else { 0.0 };
    out.hull_cost = (hull.hull_cost + bulkheads.map_or(0.0, |b| b.cost)) * keep;
    out.modules_cost = lives.iter().map(|l| l.m.cost).sum::<f64>() * keep;
    out.ammo_cost = lives.iter().map(|l| l.get("ammocost") * (l.get("clip") + l.get("ammo"))).sum();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unengineered Kestrel Mk II, as a commander's journal Loadout had it on 2026-11-04, with the game's own
    /// figures for it: UnladenMass 323.100006, MaxJumpRange 19.642370, FuelCapacity 16 (+0.61 reserve), CargoCapacity 4.
    const KESTREL: &str = r#"{ "event":"Loadout", "Ship":"smallcombat01_nx", "Modules":[
        { "Slot":"LargeHardpoint1", "Item":"hpt_pulselaser_gimbal_large", "On":true, "Priority":0 },
        { "Slot":"LargeHardpoint2", "Item":"hpt_multicannon_gimbal_large", "On":true, "Priority":0 },
        { "Slot":"LargeHardpoint3", "Item":"hpt_multicannon_gimbal_large", "On":true, "Priority":0 },
        { "Slot":"SmallHardpoint1", "Item":"hpt_pulselaser_gimbal_small", "On":true, "Priority":0 },
        { "Slot":"SmallHardpoint2", "Item":"hpt_pulselaser_gimbal_small", "On":true, "Priority":0 },
        { "Slot":"TinyHardpoint1", "Item":"hpt_shieldbooster_size0_class5", "On":true, "Priority":0 },
        { "Slot":"TinyHardpoint2", "Item":"hpt_shieldbooster_size0_class5", "On":true, "Priority":0 },
        { "Slot":"TinyHardpoint3", "Item":"hpt_chafflauncher_tiny", "On":true, "Priority":0 },
        { "Slot":"TinyHardpoint4", "Item":"hpt_crimescanner_size0_class3", "On":true, "Priority":0 },
        { "Slot":"Armour", "Item":"smallcombat01_nx_armour_grade1", "On":true, "Priority":1 },
        { "Slot":"PowerPlant", "Item":"int_powerplant_size5_class5", "On":true, "Priority":1 },
        { "Slot":"MainEngines", "Item":"int_mkiiagileboost_engine_size5_class5", "On":true, "Priority":0 },
        { "Slot":"FrameShiftDrive", "Item":"int_hyperdrive_overcharge_size4_class5", "On":true, "Priority":0 },
        { "Slot":"LifeSupport", "Item":"int_lifesupport_size1_class2", "On":true, "Priority":0 },
        { "Slot":"PowerDistributor", "Item":"int_powerdistributor_size5_class5", "On":true, "Priority":0 },
        { "Slot":"Radar", "Item":"int_sensors_size2_class2", "On":true, "Priority":0 },
        { "Slot":"FuelTank", "Item":"int_fueltank_size4_class3", "On":true, "Priority":1 },
        { "Slot":"Slot01_Size5", "Item":"int_shieldgenerator_size4_class5", "On":true, "Priority":0 },
        { "Slot":"Slot02_Size4", "Item":"int_hullreinforcement_size4_class2", "On":true, "Priority":1 },
        { "Slot":"Slot03_Size3", "Item":"int_modulereinforcement_size3_class2", "On":true, "Priority":1 },
        { "Slot":"Slot04_Size2", "Item":"int_modulereinforcement_size2_class2", "On":true, "Priority":1 },
        { "Slot":"Slot05_Size2", "Item":"int_dronecontrol_collection_size1_class5", "On":true, "Priority":0 },
        { "Slot":"Slot06_Size2", "Item":"int_cargorack_size2_class1", "On":true, "Priority":1 },
        { "Slot":"Slot07_Size1", "Item":"int_supercruiseassist", "On":true, "Priority":0 },
        { "Slot":"Military01", "Item":"int_hullreinforcement_size4_class2", "On":true, "Priority":1 },
        { "Slot":"PlanetaryApproachSuite", "Item":"int_planetapproachsuite_advanced", "On":true, "Priority":1 },
        { "Slot":"VesselVoice", "Item":"voicepack_verity", "On":true, "Priority":1 },
        { "Slot":"ShipCockpit", "Item":"smallcombat01_nx_cockpit", "On":true, "Priority":1 },
        { "Slot":"CargoHatch", "Item":"modularcargobaydoor", "On":true, "Priority":2 } ] }"#;

    #[test]
    fn the_sums_come_to_the_games_own_figures() {
        let plan = Plan::from_loadout(&serde_json::from_str(KESTREL).expect("json")).expect("plan");
        assert_eq!(plan.fitted.len(), 24, "cosmetics, the hatch, bulkheads and the approach suite aren't slots to plan");
        let f = figures(&plan).expect("figures");
        assert!(f.unknown.is_empty(), "{:?}", f.unknown);
        assert!((f.unladen_mass - 323.1).abs() < 0.01, "unladen mass {}", f.unladen_mass);
        assert!((f.max_range - 19.64237).abs() < 0.0005, "max jump range {}", f.max_range);
        assert_eq!((f.fuel, f.cargo), (16.0, 4.0));
        // Ranges shrink with load; the whole tank goes further than one jump.
        assert!(f.max_range > f.full_tank_range && f.full_tank_range > f.laden_range && f.total_range > 4.0 * f.max_range, "{f:?}");
        // Figures there's no game value to check against are at least in their ranges.
        assert!(f.speed > 200.0 && f.boost > f.speed, "{f:?}");
        assert!(f.power_generated > 0.0 && f.power_deployed > f.power_retracted, "{f:?}");
        let s = f.shield.expect("a shield generator");
        assert!(s.strength > 100.0 && (0.0..0.75).contains(&s.res.kinetic), "{s:?}");
        assert!(f.armour > hull("smallcombat01_nx").expect("hull").base_armour, "{f:?}");
        // Lightweight alloy takes 20 % more kinetic damage: a resistance can be below 0, never above 75 %.
        assert!((-1.0..=0.75).contains(&f.hull_res.kinetic) && f.hull_res.kinetic > -0.2, "{f:?}");
        assert!(f.module_protection > 0.0 && f.rebuy() > 0.0);
        // A station's 15 % off takes 15 % off the hull and modules and the rebuy, and nothing else.
        let off = figures_with(&plan, &Setup { discount: 0.15, ..Setup::default() }).expect("figures");
        assert!((off.total_cost() - f.total_cost() * 0.85).abs() < 1.0 && (off.rebuy() - (off.total_cost() * 0.05).ceil()).abs() < 1e-9, "{} {}", off.total_cost(), f.total_cost());
        assert_eq!((off.max_range, off.ammo_cost), (f.max_range, f.ammo_cost));
        // The plan goes back out as a Loadout with the same modules.
        let back = Plan::from_loadout(&plan.to_loadout()).expect("again");
        assert_eq!((back.hull.as_str(), back.fitted.len(), back.bulkheads.as_str()), ("smallcombat01_nx", 24, "smallcombat01_nx_armour_grade1"));
    }

    #[test]
    fn engineering_uses_the_games_values() {
        let plain = r#"{"Ship":"smallcombat01_nx","Modules":[{"Slot":"FrameShiftDrive","Item":"int_hyperdrive_overcharge_size4_class5"},{"Slot":"FuelTank","Item":"int_fueltank_size4_class3"}]}"#;
        let tuned = r#"{"Ship":"smallcombat01_nx","Modules":[{"Slot":"FrameShiftDrive","Item":"int_hyperdrive_overcharge_size4_class5","Engineering":{"BlueprintName":"FSD_LongRange","Level":5,"Modifiers":[
            {"Label":"Mass","Value":13.0,"OriginalValue":10.0,"LessIsGood":1},{"Label":"FSDOptimalMass","Value":1000.0,"OriginalValue":585.0,"LessIsGood":0}]}},
            {"Slot":"FuelTank","Item":"int_fueltank_size4_class3"}]}"#;
        let figures_of = |text: &str| figures(&Plan::from_loadout(&serde_json::from_str(text).expect("json")).expect("plan")).expect("figures");
        let (a, b) = (figures_of(plain), figures_of(tuned));
        let fsd = module("int_hyperdrive_overcharge_size4_class5").expect("fsd");
        assert!((b.unladen_mass - a.unladen_mass - (13.0 - fsd.mass)).abs() < 1e-9);
        // Range scales with optimal mass over total mass.
        let expected = a.max_range * (1000.0 / fsd.get("optmass")) * (a.unladen_mass + 3.2) / (b.unladen_mass + 3.2);
        assert!((b.max_range - expected).abs() < 1e-6, "{} vs {expected}", b.max_range);
        let plan = Plan::from_loadout(&serde_json::from_str(tuned).expect("json")).expect("plan");
        assert_eq!(plan.fitted[0].blueprint, Some(("FSD_LongRange".to_string(), 5)));
        // The engineering goes back out with the build, so EDSY and Coriolis show it engineered.
        assert_eq!(Plan::from_loadout(&plan.to_loadout()).expect("again"), plan);
        // Another drive, then the engineered one picked back: the plan is the ship again, engineering and all, and so
        // is a slot emptied and filled again.
        let mut p = plan.clone();
        p.set("FrameShiftDrive", Some("int_hyperdrive_size4_class5"));
        assert!(p.module_in("FrameShiftDrive").is_some_and(|f| f.engineering.is_none() && f.engineered.is_empty()));
        p.put(plan.module_in("FrameShiftDrive").expect("fsd").clone());
        p.set("FuelTank", None);
        p.put(plan.module_in("FuelTank").expect("tank").clone());
        assert_eq!(p, plan);
    }

    /// Play-test finding 14: planned engineering changes the figures, as EDSY and Coriolis show it.
    #[test]
    fn planned_engineering_changes_the_figures() {
        let ship = Plan::from_loadout(&serde_json::from_str(KESTREL).expect("json")).expect("plan");
        let before = figures(&ship).expect("figures");
        let mut plan = ship.clone();
        let fsd = plan.fitted.iter_mut().find(|f| f.slot == "FrameShiftDrive").expect("fsd");
        fsd.goal = Some(Goal::new("FSD_LongRange", 5));
        let after = figures(&plan).expect("figures");
        // The 4A SCO drive: optimal mass 585 t +55 %, its mass 10 t +30 %.
        let drive = module("int_hyperdrive_overcharge_size4_class5").expect("drive");
        let (opt, mass) = (drive.get("optmass") * 1.55, drive.mass * 1.3);
        let unladen = before.unladen_mass - drive.mass + mass;
        let fuel = drive.get("maxfuel").min(after.fuel);
        let expected = (fuel / drive.get("fuelmul")).powf(1.0 / drive.get("fuelpower")) * opt / (unladen + fuel);
        assert!((after.unladen_mass - unladen).abs() < 1e-9 && (after.max_range - expected).abs() < 1e-9, "{} vs {expected}", after.max_range);
        assert!(after.max_range > before.max_range * 1.4, "{} vs {}", after.max_range, before.max_range);
        // Half a roll lands between none and a full one; Mass Manager goes further.
        let roll = |r: f64, x: Option<&str>| {
            let mut p = plan.clone();
            if let Some(g) = p.fitted.iter_mut().find(|f| f.slot == "FrameShiftDrive").and_then(|f| f.goal.as_mut()) {
                g.roll = r;
                g.experimental = x.map(str::to_string);
            }
            figures(&p).expect("figures").max_range
        };
        assert!(roll(0.5, None) < after.max_range && roll(0.0, None) < roll(0.5, None));
        assert!(roll(1.0, Some("special_fsd_heavy")) > after.max_range);
        // Heavy Duty bulkheads add armour.
        let mut armoured = ship.clone();
        armoured.bulkheads_goal = Some(Goal::new("Armour_HeavyDuty", 5));
        assert!(figures(&armoured).expect("figures").armour > before.armour);
        // Thermal Resistant shields stop more thermal damage.
        let mut shields = ship.clone();
        let sg = shields.fitted.iter_mut().find(|f| f.item.starts_with("int_shieldgenerator")).expect("shields");
        sg.goal = Some(Goal::new("ShieldGenerator_Thermic", 5));
        let (a, b) = (before.shield.expect("shields"), figures(&shields).expect("figures").shield.expect("shields"));
        assert!(b.res.thermal > a.res.thermal, "{} vs {}", b.res.thermal, a.res.thermal);
    }

    #[test]
    fn planned_engineering_goes_out_as_the_game_writes_it() {
        // An FSD with Mass Manager, reinforced shields, dirty thrusters and heavy duty bulkheads, planned.
        let mut plan = Plan::from_loadout(&serde_json::from_str(KESTREL).expect("json")).expect("plan");
        for f in plan.fitted.iter_mut() {
            f.goal = match f.item.as_str() {
                i if i.starts_with("int_hyperdrive") => Some(Goal { roll: 0.5, experimental: Some("special_fsd_heavy".into()), ..Goal::new("FSD_LongRange", 5) }),
                i if i.starts_with("int_shieldgenerator") => Some(Goal::new("ShieldGenerator_Reinforced", 4)),
                i if i.starts_with("int_engine") => Some(Goal::new("Engine_Dirty", 3)),
                _ => None,
            };
        }
        plan.bulkheads_goal = Some(Goal::new("Armour_HeavyDuty", 5));
        let out = plan.to_loadout_planned();
        // As the game's journal has it: the blueprint, grade, roll, effect, and the figures it changes.
        let fsd = out["Modules"].as_array().and_then(|m| m.iter().find(|m| m["Slot"] == "FrameShiftDrive")).expect("fsd");
        let e = &fsd["Engineering"];
        assert_eq!((e["BlueprintName"].as_str(), e["Level"].as_u64(), e["Quality"].as_f64(), e["ExperimentalEffect"].as_str()), (Some("FSD_LongRange"), Some(5), Some(0.5), Some("special_fsd_heavy")));
        let labels: Vec<&str> = e["Modifiers"].as_array().expect("modifiers").iter().filter_map(|m| m["Label"].as_str()).collect();
        assert!(labels.contains(&"FSDOptimalMass") && labels.contains(&"Mass"), "{labels:?}");
        // Read as the game's own engineering (another app's, with no mark), it comes to what the plan does: the game's
        // way of writing it loses nothing the sums use.
        let mut unmarked = out.clone();
        for m in unmarked["Modules"].as_array_mut().into_iter().flatten() {
            if let Some(e) = m.get_mut("Engineering").and_then(Value::as_object_mut) {
                e.remove(PLANNED_MARK);
            }
        }
        let back = Plan::from_loadout(&unmarked).expect("read back");
        assert!(back.module_in("FrameShiftDrive").is_some_and(|f| f.goal.is_none() && f.blueprint.is_some()), "the game's engineering");
        let (planned, read) = (figures(&plan).expect("figures"), figures(&back).expect("figures"));
        assert!((planned.max_range - read.max_range).abs() < 1e-6, "{} vs {}", planned.max_range, read.max_range);
        assert!((planned.speed - read.speed).abs() < 1e-6 && (planned.armour - read.armour).abs() < 1e-6, "{planned:?} vs {read:?}");
        let (s, r) = (planned.shield.expect("shields"), read.shield.expect("shields"));
        assert!((s.strength - r.strength).abs() < 1e-6 && (s.res.kinetic - r.res.kinetic).abs() < 1e-9, "{s:?} vs {r:?}");
        assert!((planned.hull_res.kinetic - read.hull_res.kinetic).abs() < 1e-9);
        // Marked as planned, it comes back as the plan it was: the same goals.
        let again = Plan::from_loadout(&out).expect("read back");
        assert_eq!(again.module_in("FrameShiftDrive").and_then(|f| f.goal.clone()), plan.module_in("FrameShiftDrive").and_then(|f| f.goal.clone()));
        assert_eq!(again.bulkheads_goal, plan.bulkheads_goal);
        // The plain Loadout keeps only what the game reported.
        assert!(plan.to_loadout()["Modules"].as_array().expect("modules").iter().all(|m| m.get("Engineering").is_none()));
        // A figure typed in exactly goes out as typed, the figures list it, and it comes back typed.
        let fsd = plan.fitted.iter_mut().find(|f| f.slot == "FrameShiftDrive").expect("fsd");
        if let Some(g) = fsd.goal.as_mut() {
            g.exact = vec![("optmass".into(), 900.0)];
        }
        let item = fsd.item.clone();
        let listed = goal_figures(&item, fsd.goal.as_ref().expect("goal"));
        assert!(listed.iter().any(|(k, v, base)| k == "optmass" && *v == 900.0 && *base > 0.0), "{listed:?}");
        let typed = Plan::from_loadout(&plan.to_loadout_planned()).expect("read back");
        assert_eq!(typed.module_in("FrameShiftDrive").and_then(|f| f.goal.as_ref()).map(|g| g.exact.clone()), Some(vec![("optmass".to_string(), 900.0)]));
    }

    #[test]
    fn fights_flight_and_power_add_up() {
        let plan = Plan::from_loadout(&serde_json::from_str(KESTREL).expect("json")).expect("plan");
        let f = figures(&plan).expect("figures");
        // Weapons: a gimballed large pulse laser fires every 0.31 s, a gimballed large multi-cannon every 0.15 s with a
        // 90-round clip and a 5-second reload.
        let laser = f.weapons.iter().find(|w| w.item == "hpt_pulselaser_gimbal_large").expect("laser");
        assert!((laser.dps - 4.58 / 0.31).abs() < 1e-9 && laser.sustained_dps == laser.dps && laser.time_to_empty.is_none(), "{laser:?}");
        let mc = f.weapons.iter().find(|w| w.item == "hpt_multicannon_gimbal_large").expect("multi-cannon");
        let per_clip = 89.0 * 0.15 + 5.0;
        assert!((mc.sustained_dps - mc.dps * (90.0 / per_clip) / (1.0 / 0.15)).abs() < 1e-9, "{mc:?}");
        assert!(mc.dist.kinetic == 1.0 && laser.dist.thermal == 1.0);
        assert_eq!(f.weapons.len(), 5, "utilities aren't weapons");
        assert!(f.wep.capacity > 0.0 && f.wep.recharge > 0.0);
        // Speed by pips: four pips is the top speed, none the hull's minimum thrust.
        let hull = hull("smallcombat01_nx").expect("hull");
        assert!((f.speed_by_pips[4] - f.speed).abs() < 1e-9 && (f.speed_by_pips[0] - f.speed * hull.minthrust).abs() < 1e-9);
        assert!(f.boost_interval.is_some_and(|t| t > 0.0) && f.pitch > 0.0 && f.yaw > 0.0 && f.roll > 0.0, "{f:?}");
        // Power groups add up to everything that's drawn.
        assert!(f.power_groups.windows(2).all(|w| w[0].deployed <= w[1].deployed));
        assert!((f.power_groups[4].deployed - f.power_deployed).abs() < 1e-9 && (f.power_groups[4].retracted - f.power_retracted).abs() < 1e-9);
        // Shields come back, faster with more systems pips.
        let s = f.shield.expect("shields");
        assert!(s.recharge.is_some() && s.recover.is_some_and(|t| t > 16.0), "{s:?}");
        let four = figures_with(&plan, &Setup { pips: Pips { sys: 4.0, eng: 1.0, wep: 1.0 }, ..Setup::default() }).expect("figures");
        let s4 = four.shield.expect("shields");
        assert!(s4.sys_res > s.sys_res && s4.against(s4.res.thermal) > s.against(s.res.thermal));
        // Heat: the power plant's is what's drawn × its efficiency.
        let plant = module("int_powerplant_size5_class5").expect("plant");
        assert!((f.heat.plant_deployed - f.power_deployed * plant.get("eff")).abs() < 1e-9 && f.heat.capacity > 0.0 && f.heat.weapons > 0.0);
        // Cargo carried shortens the jump and slows the ship; the usual top speed and boost stay at an empty hold.
        let laden = figures_with(&plan, &Setup { cargo: 4.0, ..Setup::default() }).expect("figures");
        assert!(laden.range_carrying < f.range_carrying && laden.speed_by_pips[4] <= f.speed_by_pips[4] && laden.boost_carrying <= f.boost);
        assert!(laden.speed == f.speed && laden.boost == f.boost);
        assert!((f.range_carrying - f.full_tank_range).abs() < 1e-9);
        assert!(f.ammo_cost > 0.0, "the multi-cannons and chaff restock");
        // Never more boosts than the hull allows, however fast the capacitor refills.
        let fast = figures_with(&plan, &Setup { pips: Pips { sys: 1.0, eng: 4.0, wep: 1.0 }, ..Setup::default() }).expect("figures");
        assert!(fast.boost_interval.is_some_and(|t| t >= hull.boost_cooldown && t <= f.boost_interval.unwrap_or(f64::MAX)), "{:?} {:?}", fast.boost_interval, f.boost_interval);
        // Point defence shoots down missiles, not ships: it isn't a weapon here.
        let mut pd = plan.clone();
        pd.set("TinyHardpoint4", Some("hpt_plasmapointdefence_turret_tiny"));
        assert_eq!(figures(&pd).expect("figures").weapons.len(), 5);
    }

    /// The game writes some labels more than one way: either spelling reads.
    #[test]
    fn journal_labels_in_either_spelling() {
        let modifiers = |label: &str, value: f64, original: f64| serde_json::json!({ "Modifiers": [{ "Label": label, "Value": value, "OriginalValue": original }] });
        let base = |k: &str| match k {
            "falloff" => Some(500.0),
            "maxfuel" => Some(5.0),
            _ => None,
        };
        assert_eq!(engineered(&base, &modifiers("DamageFalloffRange", 3000.0, 500.0)).get("falloff"), Some(&3000.0));
        assert_eq!(engineered(&base, &modifiers("FalloffRange", 3000.0, 500.0)).get("falloff"), Some(&3000.0));
        assert_eq!(engineered(&base, &modifiers("FSDMaxFuelPerJump", 6.0, 5.0)).get("maxfuel"), Some(&6.0));
        assert_eq!(engineered(&base, &modifiers("OutfittingFieldType_MaxFuelPerJump", 6.0, 5.0)).get("maxfuel"), Some(&6.0));
        assert_eq!(engineered(&base, &modifiers("weapon_burst_size", 4.0, 3.0)).get("burst"), Some(&4.0));
        assert!(engineered(&base, &modifiers("SomethingNew", 1.0, 2.0)).is_empty());
    }

    #[test]
    fn boosts_and_boosters_as_the_game_allows() {
        let anaconda = hull("anaconda").expect("Anaconda");
        let mut plan = Plan::stock(anaconda);
        let stock = figures(&plan).expect("figures");
        assert!(stock.boost > stock.speed, "{stock:?}");
        // A power distributor whose engines capacitor holds less than a boost takes: no boost.
        let small = module("int_powerdistributor_size1_class1").expect("1E distributor");
        assert!(small.get("engcap") < anaconda.boost_energy);
        plan.set("PowerDistributor", Some(&small.symbol));
        let weak = figures(&plan).expect("figures");
        assert!(weak.boost == 0.0 && weak.speed > 0.0, "{weak:?}");
        // A second Guardian FSD booster adds only its mass.
        let mut one = Plan::stock(anaconda);
        let optional: Vec<&SlotDef> = anaconda.slots.iter().filter(|s| s.kind == SlotKind::Optional && s.size >= 5).collect();
        one.set(&optional[0].name, Some("int_guardianfsdbooster_size5"));
        let mut two = one.clone();
        two.set(&optional[1].name, Some("int_guardianfsdbooster_size5"));
        let (a, b) = (figures(&one).expect("one"), figures(&two).expect("two"));
        assert!(b.max_range < a.max_range && a.max_range > stock.max_range, "{} {} {}", stock.max_range, a.max_range, b.max_range);
        assert!(one_per_ship(module("int_guardianfsdbooster_size5").expect("booster")).is_some());
        assert!(one_per_ship(module("int_fuelscoop_size5_class5").expect("scoop")).is_some());
        assert!(one_per_ship(module("int_shieldgenerator_size5_class5").expect("shields")).is_none());
        // A tank of billions of tonnes and a drive that burns next to nothing still come to an answer.
        let mut odd = Plan::stock(anaconda);
        if let Some(f) = odd.fitted.iter_mut().find(|f| f.slot == "FuelTank") {
            f.engineered.insert("fuel".into(), 1e17);
        }
        assert!(figures(&odd).expect("odd").total_range.is_finite());
    }

    #[test]
    fn an_experimental_already_on_a_module_is_not_gathered_again() {
        let mut plan = Plan::from_loadout(&serde_json::from_str(KESTREL).expect("json")).expect("plan");
        let f = plan.fitted.iter_mut().find(|f| f.slot == "FrameShiftDrive").expect("fsd");
        f.experimental = Some("SPECIAL_FSD_HEAVY".into());
        f.blueprint = Some(("FSD_LongRange".into(), 5));
        f.goal = Some(Goal { experimental: Some("special_fsd_heavy".into()), ..Goal::new("FSD_LongRange", 5) });
        assert!(goal_materials(&plan).is_empty(), "{:?}", goal_materials(&plan));
    }

    /// Play-test finding 14/15: "Total Materials Needed For This Build".
    #[test]
    fn engineering_goals_add_up_their_materials() {
        let mut plan = Plan::from_loadout(&serde_json::from_str(KESTREL).expect("json")).expect("plan");
        let item = "int_hyperdrive_overcharge_size4_class5";
        let bp = crate::engineering::blueprint_for(item, "FSD_LongRange").expect("Increased FSD Range for an SCO drive");
        let x = crate::engineering::experimental("special_fsd_heavy").expect("Mass Manager");
        let rolls = |from: u8| (from..=5).flat_map(|g| bp.grade(g).unwrap_or_default().iter().map(|i| i.count * ROLLS_PER_GRADE)).sum::<u32>();
        let total = |p: &Plan| goal_materials(p).iter().map(|(_, n)| *n).sum::<u32>();
        assert_eq!(total(&plan), 0, "no goals, nothing to gather");
        let f = plan.fitted.iter_mut().find(|f| f.slot == "FrameShiftDrive").expect("fsd");
        f.goal = Some(Goal { experimental: Some("special_fsd_heavy".into()), ..Goal::new("FSD_LongRange", 5) });
        let experimental: u32 = x.cost.iter().map(|i| i.count).sum();
        assert_eq!(total(&plan), rolls(1) + experimental, "grades 1 to 5, three rolls each, and the experimental");
        // What the module has done already isn't gathered again.
        let f = plan.fitted.iter_mut().find(|f| f.slot == "FrameShiftDrive").expect("fsd");
        f.blueprint = Some(("FSD_LongRange".into(), 3));
        f.experimental = Some("special_fsd_heavy".into());
        assert_eq!(total(&plan), rolls(4));
    }

    #[test]
    fn every_hull_has_slots_named_as_the_game_names_them() {
        // Every hull a shipyard sells has figures.
        let missing: Vec<&str> = crate::ships::all().iter().filter(|s| hull(&s.symbol).is_none()).map(|s| s.name.as_str()).collect();
        assert!(missing.is_empty(), "no figures for {missing:?}");
        for h in hulls() {
            assert_eq!(h.slots.iter().filter(|s| s.kind == SlotKind::Core).count(), 7, "{}", h.name);
            assert!(h.slots.iter().any(|s| s.kind == SlotKind::Optional), "{}", h.name);
            assert!(h.bulkheads.len() >= 5, "{}", h.name);
            assert!(h.hull_mass > 0.0 && h.speed > 0.0 && h.base_armour > 0.0, "{}", h.name);
            // A new hull's standard modules fit where they are.
            for s in &h.slots {
                if let Some(d) = &s.default {
                    let m = module(d).unwrap_or_else(|| panic!("{}: default {d} has no figures", h.name));
                    assert!(fits_slot(m, s), "{}: {d} doesn't fit {}", h.name, s.name);
                }
            }
            let stock = figures(&Plan::stock(h)).expect("stock figures");
            assert!(stock.max_range > 0.0 && stock.speed > 0.0, "{}: {stock:?}", h.name);
        }
        // The Type-11's slots for one kind of module, by the game's names.
        let t11 = hull("lakonminer").expect("Type-11");
        let limpets = t11.slot("LimpetController01").expect("limpet slot");
        assert!(!limpets.only.is_empty() && t11.fits(limpets).iter().all(|m| limpets.only.contains(&m.grp)));
        assert!(t11.slot("LargeMiningHardpoint1").is_some());
        // The Anaconda's optional slots skip numbers, as the game's do.
        let anaconda = hull("anaconda").expect("Anaconda");
        assert!(anaconda.slot("Slot13_Size2").is_some() && anaconda.slot("Slot11_Size2").is_none());
    }

    #[test]
    fn modules_fit_where_the_game_lets_them() {
        let anaconda = hull("anaconda").expect("Anaconda");
        let names = |slot: &str| anaconda.fits(anaconda.slot(slot).expect(slot)).iter().map(|m| m.symbol.clone()).collect::<Vec<_>>();
        let fsds = names("FrameShiftDrive");
        assert!(fsds.contains(&"int_hyperdrive_size6_class5".to_string()) && fsds.contains(&"int_hyperdrive_size2_class1".to_string()));
        assert!(!fsds.iter().any(|s| s.contains("size7")), "nothing bigger than the slot");
        assert!(fsds.iter().all(|s| s.starts_with("int_hyperdrive")), "{fsds:?}");
        // Optional slots take fuel tanks (filed under the core tank's group), not the approach suite or core modules.
        let optional = names("Slot02_Size6");
        assert!(optional.contains(&"int_fueltank_size6_class3".to_string()) && optional.contains(&"int_shieldgenerator_size6_class5".to_string()), "{optional:?}");
        assert!(!optional.iter().any(|s| s.contains("planetapproachsuite") || s.contains("hyperdrive") || s.contains("powerplant")), "{optional:?}");
        let military = names("Military01");
        assert!(military.iter().all(|s| !s.contains("shieldgenerator") && !s.contains("cargorack")), "{military:?}");
        assert!(military.contains(&"int_hullreinforcement_size5_class2".to_string()));
        let utility = names("TinyHardpoint1");
        assert!(utility.contains(&"hpt_shieldbooster_size0_class5".to_string()) && !utility.iter().any(|s| s.contains("pulselaser")));
        let huge = names("HugeHardpoint1");
        assert!(huge.contains(&"hpt_multicannon_gimbal_huge".to_string()) && !huge.contains(&"hpt_shieldbooster_size0_class5".to_string()));
        // Hull-only modules: the Kestrel's Mk II thrusters fit no Anaconda.
        assert!(!names("MainEngines").contains(&"int_mkiiagileboost_engine_size5_class5".to_string()));
        // A symbol is offered once, the plain module of it.
        let pds = names("PowerDistributor");
        let mut unique = pds.clone();
        unique.dedup();
        assert_eq!(pds.len(), unique.len());
        assert!(!module("int_powerdistributor_size5_class5").expect("pd").pre_engineered);
        // Life support, sensors and supercruise overcharge drives only of the slot's size; other drives smaller too.
        let ls = anaconda.slot("LifeSupport").expect("life support");
        assert!(anaconda.fits(ls).iter().all(|m| m.class == ls.size));
        let fsd = anaconda.slot("FrameShiftDrive").expect("fsd");
        assert!(anaconda.fits(fsd).iter().all(|m| !m.symbol.contains("overcharge") || m.class == fsd.size), "{fsds:?}");
        assert!(fsds.iter().any(|s| s.contains("overcharge")) && fsds.contains(&"int_hyperdrive_size5_class5".to_string()));
        // The hull decides too: no fighter hangars, luxury cabins, Mk II hangars or Mk II cargo racks in a Python's
        // optional slots, which have no slot made for them.
        let python = hull("python").expect("Python");
        let six = python.slots.iter().find(|s| s.kind == SlotKind::Optional && s.size == 6).expect("a size-6 slot");
        let groups: Vec<&str> = python.fits(six).iter().map(|m| m.grp.as_str()).collect();
        assert!(!groups.iter().any(|g| matches!(*g, "fh" | "fhmkii" | "pcq" | "crl" | "lpvmkii")), "{groups:?}");
        assert!(groups.contains(&"cr") && groups.contains(&"sg"), "{groups:?}");
        let biggest = anaconda.slots.iter().filter(|s| s.kind == SlotKind::Optional).max_by_key(|s| s.size).expect("an optional slot");
        assert!(anaconda.fits(biggest).iter().any(|m| m.symbol.starts_with("int_fighterbay")), "the Anaconda takes fighters");
        // No shield generator or thrusters the hull alone is too heavy for.
        let t9 = hull("type9").expect("Type-9");
        for s in t9.slots.iter().filter(|s| matches!(s.kind, SlotKind::Optional | SlotKind::Core)) {
            for m in t9.fits(s).iter().filter(|m| matches!(m.grp.as_str(), "sg" | "bsg" | "psg" | "t")) {
                assert!(m.get("maxmass") >= t9.hull_mass, "{} on a Type-9", m.symbol);
            }
        }
        // What a new hull is given isn't sold, and the Type-11's Mk II mining limpet controller goes only in its
        // limpet slot.
        for h in hulls() {
            for s in &h.slots {
                assert!(h.fits(s).iter().all(|m| !m.symbol.ends_with("_free")), "{} {}", h.name, s.name);
            }
        }
        let t11 = hull("lakonminer").expect("Type-11");
        let mk2 = "int_multidronecontrol_miningv2_size5_class5";
        for s in t11.slots.iter().filter(|s| s.kind == SlotKind::Optional && s.only.is_empty()) {
            assert!(!t11.fits(s).iter().any(|m| m.symbol == mk2), "{}", s.name);
        }
    }
}
