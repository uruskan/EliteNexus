//! Engineering knowledge: ship blueprints (G1–G5 recipes + engineers), experimental effects,
//! engineers (ship, on-foot, Colonia), synthesis, technology-broker unlocks and Odyssey suit/weapon
//! upgrades and modifications.
//!
//! The data is converted at development time by
//! `cargo run -p nexus-data --example convert_engineering` into
//! `data/engineering/nexus/engineering.json`, which is compiled in and parsed lazily on first use.
//! Sources: EDEngineer (MIT, © 2016 Max) for ship blueprints/experimentals, EDDiscovery
//! `EliteDangerousCore` (Apache-2.0, © EDDiscovery development team) for journal symbols, engineers,
//! synthesis, tech brokers and Odyssey costs, EDCD/FDevIDs for engineer ids — see
//! `data/engineering/nexus/SOURCE.txt`.
//!
//! All ingredient symbols are lower-case journal names that resolve through
//! [`crate::materials`], [`crate::commodities`] or [`crate::materials::micro_resource`].

use std::sync::LazyLock;

use rustc_hash::FxHashMap;
use serde::Deserialize;

use crate::commodities::name_key;
use crate::materials::{MaterialKind, MicroResourceKind};

const DATA_JSON: &str = include_str!("../../../data/engineering/nexus/engineering.json");

// ---------------------------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------------------------

/// `count` × `symbol` (a material, commodity or micro-resource journal symbol).
/// Stored in the JSON as `["symbol", count]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
pub struct Ingredient {
    pub symbol: String,
    pub count: u32,
}

/// What an [`Ingredient`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IngredientKind {
    Material(MaterialKind),
    Commodity,
    MicroResource(MicroResourceKind),
    Unknown,
}

impl Ingredient {
    pub fn kind(&self) -> IngredientKind {
        if let Some(m) = crate::materials::material(&self.symbol) {
            IngredientKind::Material(m.kind)
        } else if let Some(r) = crate::materials::micro_resource(&self.symbol) {
            IngredientKind::MicroResource(r.kind)
        } else if crate::commodities::commodity(&self.symbol).is_some() {
            IngredientKind::Commodity
        } else {
            IngredientKind::Unknown
        }
    }

    /// English display name of the ingredient.
    pub fn name(&self) -> String {
        if let Some(m) = crate::materials::material(&self.symbol) {
            m.name.clone()
        } else if let Some(r) = crate::materials::micro_resource(&self.symbol) {
            r.name.clone()
        } else {
            crate::commodities::display_name(&self.symbol)
        }
    }
}

/// One effect line as shown in planners, e.g. `Optimal Mass +55%` (good). JSON: `["Optimal Mass", "+55%", true]`.
/// Blueprint values are EDEngineer's per-grade figures (indicative maxima, not roll ranges).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Effect {
    pub property: String,
    pub value: String,
    /// Whether the change is beneficial.
    pub good: bool,
}

/// An engineer and the highest grade they offer for a blueprint. JSON: `["Felicity Farseer", 5]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EngineerGrade {
    pub engineer: String,
    pub max_grade: u8,
}

/// A ship-module blueprint for one module type.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Blueprint {
    /// Canonical module type, one of [`MODULE_TYPES`], e.g. `"Frame Shift Drive"`.
    pub module: String,
    /// Journal `BlueprintName` (`Loadout…Engineering`, `EngineerCraft`), e.g. `"FSD_LongRange"`.
    /// Compare case-insensitively.
    pub symbol: String,
    /// Display name, e.g. `"Increased FSD Range"`.
    pub name: String,
    /// One recipe per grade; index 0 is grade 1. Each roll at grade N consumes one recipe.
    pub grades: Vec<Vec<Ingredient>>,
    /// Effect lines per grade (index 0 is grade 1).
    pub effects: Vec<Vec<Effect>>,
    /// Engineers offering this blueprint and their highest grade, best first.
    pub engineers: Vec<EngineerGrade>,
    /// Merc blueprints: Merc Coins per roll for each grade (index 0 is grade 1). Empty otherwise.
    pub merc_coins: Vec<u32>,
    /// The MercGear module a Merc blueprint is for, e.g. `"5E Extended Cargo Rack"`; `None` when it
    /// fits ordinary modules.
    pub merc_module: Option<String>,
}

impl Blueprint {
    /// Highest grade that exists for this blueprint (usually 5).
    pub fn max_grade(&self) -> u8 {
        self.grades.len() as u8
    }

    /// Lowest grade that can be rolled: 2 for blueprints on MercGear modules, which are bought at grade 1.
    pub fn min_grade(&self) -> u8 {
        self.grades.iter().position(|g| !g.is_empty()).map_or(1, |i| i as u8 + 1)
    }

    /// Bought from the engineer with Merc Coins, which each roll costs as well as materials.
    pub fn is_merc(&self) -> bool {
        !self.merc_coins.is_empty()
    }

    /// Merc Coins for one roll at `grade` (1-based); 0 for ordinary blueprints.
    pub fn merc_coins_for(&self, grade: u8) -> u32 {
        usize::from(grade).checked_sub(1).and_then(|i| self.merc_coins.get(i)).copied().unwrap_or(0)
    }

    /// The recipe for one roll at `grade` (1-based).
    pub fn grade(&self, grade: u8) -> Option<&[Ingredient]> {
        self.grades.get(usize::from(grade).checked_sub(1)?).map(Vec::as_slice)
    }

    /// Engineers who can craft this blueprint at `grade`.
    pub fn engineers_for_grade(&self, grade: u8) -> impl Iterator<Item = &EngineerGrade> {
        self.engineers.iter().filter(move |e| e.max_grade >= grade)
    }

    /// Whether `symbol` names this blueprint (case-insensitive).
    pub fn is(&self, symbol: &str) -> bool {
        self.symbol.eq_ignore_ascii_case(symbol.trim())
    }
}

/// An experimental (special) effect.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Experimental {
    /// Journal `ExperimentalEffect`, e.g. `"special_fsd_heavy"`.
    pub symbol: String,
    /// Other journal spellings of the same effect (e.g. rail-gun `"_cooled"` variants).
    pub aliases: Vec<String>,
    /// Display name, e.g. `"Mass Manager"`.
    pub name: String,
    /// Module types it applies to (canonical names; seeker racks are `"Seeker Missile Rack"`).
    pub modules: Vec<String>,
    pub cost: Vec<Ingredient>,
    pub effects: Vec<Effect>,
}

impl Experimental {
    pub fn applies_to(&self, module_type: &str) -> bool {
        self.modules.iter().any(|m| m.eq_ignore_ascii_case(module_type))
    }
}

/// Ship engineer or on-foot (Odyssey) engineer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
pub enum EngineerKind {
    #[default]
    Ship,
    OnFoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
pub enum Region {
    #[default]
    Bubble,
    Colonia,
}

/// A module type and the highest grade an engineer offers for it. JSON: `["Frame Shift Drive", 5]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModuleGrade {
    pub module: String,
    pub max_grade: u8,
}

/// One engineer.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Engineer {
    /// Journal `EngineerID` (FDevIDs `engineers.csv`): 3000xx ship, 4000xx on-foot.
    pub id: u32,
    pub name: String,
    pub kind: EngineerKind,
    pub region: Region,
    pub system: String,
    pub system_address: u64,
    pub market_id: u64,
    /// Workshop (base) name.
    pub base: String,
    /// Body designation within the system, e.g. `"6 A"`.
    pub body: String,
    /// System position (ly, Sol-centred).
    pub pos: [f64; 3],
    /// The system needs a permit.
    pub permit: bool,
    /// How the engineer is discovered.
    pub discovery: String,
    /// Requirement to receive an invitation.
    pub meeting: String,
    /// Unlock task (ship engineers).
    pub unlock: String,
    /// Structured unlock cost where it is goods/materials (empty for credits/bonds).
    pub unlock_cost: Vec<Ingredient>,
    /// How reputation is gained.
    pub reputation: String,
    /// Referral task that reveals the next engineer(s).
    pub referral: String,
    pub referral_cost: Vec<Ingredient>,
    pub referred_by: Vec<String>,
    pub refers_to: Vec<String>,
    /// Ship engineers: module types offered and the max grade, best first.
    pub modules: Vec<ModuleGrade>,
    /// On-foot engineers: modification symbols offered (see [`odyssey_mods`]).
    pub mods: Vec<String>,
}

impl Engineer {
    /// Highest grade offered for a module type or item symbol (ship engineers).
    pub fn max_grade_for(&self, module_symbol_or_type: &str) -> Option<u8> {
        let t = blueprint_module(module_type(module_symbol_or_type)?);
        self.modules.iter().find(|m| m.module == t).map(|m| m.max_grade)
    }

    /// Distance in light years from a position.
    pub fn distance_to(&self, pos: [f64; 3]) -> f64 {
        let d: f64 = self.pos.iter().zip(pos).map(|(a, b)| (a - b) * (a - b)).sum();
        d.sqrt()
    }
}

/// One synthesis recipe (journal `Synthesis.Name`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SynthesisRecipe {
    /// Family, e.g. `"FSD Injection"`, `"Small Calibre Munitions"`.
    pub family: String,
    /// Journal `Synthesis.Name`, e.g. `"FSD Basic"`.
    pub name: String,
    /// 1 = Basic, 2 = Standard, 3 = Premium.
    pub grade: u8,
    /// Effect text, e.g. `"+25% Jump Range"` (may be empty).
    pub effect: String,
    pub cost: Vec<Ingredient>,
}

impl SynthesisRecipe {
    pub fn grade_name(&self) -> &'static str {
        match self.grade {
            1 => "Basic",
            2 => "Standard",
            3 => "Premium",
            _ => "",
        }
    }
}

/// A technology-broker unlock.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct TechBrokerUnlock {
    /// `"Human"` or `"Guardian"`.
    pub broker: String,
    pub name: String,
    /// Outfitting symbol of the unlocked module where known.
    pub module: Option<String>,
    pub cost: Vec<Ingredient>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
pub enum OdysseyKind {
    #[default]
    Suit,
    Weapon,
}

/// Grade upgrade of a suit or hand weapon family at Pioneer Supplies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct OdysseyUpgrade {
    pub kind: OdysseyKind,
    /// Manufacturer family: suits `"Remlok"` (Maverick), `"Supratech"` (Artemis), `"Manticore"`
    /// (Dominator); weapons `"Kinematic Armaments"`, `"Manticore"`, `"Takada"`.
    pub family: String,
    /// Target grade (2..=5).
    pub grade: u8,
    pub cost: Vec<Ingredient>,
}

/// An on-foot engineer modification.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct OdysseyMod {
    pub kind: OdysseyKind,
    /// Journal symbol in `SuitMods`/`WeaponMods`, e.g. `"suit_increasedo2capacity"`.
    pub symbol: String,
    pub name: String,
    /// Weapon family the recipe is for (`None` = all suits/weapons).
    pub variant: Option<String>,
    pub credits: u64,
    pub cost: Vec<Ingredient>,
    pub engineers: Vec<String>,
}

/// An upgradeable Odyssey suit or hand weapon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OdysseyItem {
    /// Journal symbol without the `_classN` suffix, e.g. `"utilitysuit"`, `"wpn_m_sniper_plasma_charged"`.
    pub symbol: &'static str,
    pub name: &'static str,
    pub kind: OdysseyKind,
    pub family: &'static str,
    /// Grade-1 price at Pioneer Supplies.
    pub base_price: u64,
}

impl OdysseyItem {
    /// Credits to upgrade to `grade` (2..=5): base price × 4 / 15 / 30 / 50. Matches every complete
    /// row of the wiki tables; the journal `UpgradeSuit`/`UpgradeWeapon.Cost` is authoritative.
    pub fn upgrade_credits(&self, grade: u8) -> Option<u64> {
        let mult = match grade {
            2 => 4,
            3 => 15,
            4 => 30,
            5 => 50,
            _ => return None,
        };
        Some(self.base_price * mult)
    }
}

/// Upgradeable Odyssey items (journal.md §8.3, prices engineering.md §5.1).
pub const ODYSSEY_ITEMS: &[OdysseyItem] = &[
    OdysseyItem {
        symbol: "utilitysuit",
        name: "Maverick Suit",
        kind: OdysseyKind::Suit,
        family: "Remlok",
        base_price: 150_000,
    },
    OdysseyItem {
        symbol: "explorationsuit",
        name: "Artemis Suit",
        kind: OdysseyKind::Suit,
        family: "Supratech",
        base_price: 150_000,
    },
    OdysseyItem {
        symbol: "tacticalsuit",
        name: "Dominator Suit",
        kind: OdysseyKind::Suit,
        family: "Manticore",
        base_price: 150_000,
    },
    OdysseyItem {
        symbol: "wpn_m_assaultrifle_kinetic_fauto",
        name: "Karma AR-50",
        kind: OdysseyKind::Weapon,
        family: "Kinematic Armaments",
        base_price: 125_000,
    },
    OdysseyItem {
        symbol: "wpn_m_submachinegun_kinetic_fauto",
        name: "Karma C-44",
        kind: OdysseyKind::Weapon,
        family: "Kinematic Armaments",
        base_price: 75_000,
    },
    OdysseyItem {
        symbol: "wpn_m_launcher_rocket_sauto",
        name: "Karma L-6",
        kind: OdysseyKind::Weapon,
        family: "Kinematic Armaments",
        base_price: 175_000,
    },
    OdysseyItem {
        symbol: "wpn_s_pistol_kinetic_sauto",
        name: "Karma P-15",
        kind: OdysseyKind::Weapon,
        family: "Kinematic Armaments",
        base_price: 50_000,
    },
    OdysseyItem {
        symbol: "wpn_m_sniper_plasma_charged",
        name: "Manticore Executioner",
        kind: OdysseyKind::Weapon,
        family: "Manticore",
        base_price: 175_000,
    },
    OdysseyItem {
        symbol: "wpn_m_shotgun_plasma_doublebarrel",
        name: "Manticore Intimidator",
        kind: OdysseyKind::Weapon,
        family: "Manticore",
        base_price: 100_000,
    },
    OdysseyItem {
        symbol: "wpn_m_assaultrifle_plasma_fauto",
        name: "Manticore Oppressor",
        kind: OdysseyKind::Weapon,
        family: "Manticore",
        base_price: 125_000,
    },
    OdysseyItem {
        symbol: "wpn_s_pistol_plasma_charged",
        name: "Manticore Tormentor",
        kind: OdysseyKind::Weapon,
        family: "Manticore",
        base_price: 50_000,
    },
    OdysseyItem {
        symbol: "wpn_m_assaultrifle_laser_fauto",
        name: "TK Aphelion",
        kind: OdysseyKind::Weapon,
        family: "Takada",
        base_price: 125_000,
    },
    OdysseyItem {
        symbol: "wpn_m_submachinegun_laser_fauto",
        name: "TK Eclipse",
        kind: OdysseyKind::Weapon,
        family: "Takada",
        base_price: 75_000,
    },
    OdysseyItem {
        symbol: "wpn_s_pistol_laser_sauto",
        name: "TK Zenith",
        kind: OdysseyKind::Weapon,
        family: "Takada",
        base_price: 50_000,
    },
];

// ---------------------------------------------------------------------------------------------
// Module types
// ---------------------------------------------------------------------------------------------

/// Canonical engineerable module types. Blueprints for seeker racks are stored under
/// `"Missile Rack"`; `"Seeker Missile Rack"` exists to scope experimentals. Abrasion blasters, cargo
/// racks, enzyme missile racks, mining lasers and module reinforcements only have Merc blueprints.
pub const MODULE_TYPES: &[&str] = &[
    "Abrasion Blaster",
    "Armour",
    "Auto Field-Maintenance Unit",
    "Beam Laser",
    "Burst Laser",
    "Cannon",
    "Cargo Rack",
    "Chaff Launcher",
    "Collector Limpet Controller",
    "Detailed Surface Scanner",
    "Electronic Countermeasure",
    "Enzyme Missile Rack",
    "Fragment Cannon",
    "Frame Shift Drive",
    "Frame Shift Drive Interdictor",
    "Frame Shift Wake Scanner",
    "Fuel Scoop",
    "Fuel Transfer Limpet Controller",
    "Hatch Breaker Limpet Controller",
    "Heat Sink Launcher",
    "Hull Reinforcement Package",
    "Kill Warrant Scanner",
    "Life Support",
    "Manifest Scanner",
    "Mine Launcher",
    "Mining Laser",
    "Missile Rack",
    "Module Reinforcement Package",
    "Multi-cannon",
    "Plasma Accelerator",
    "Point Defence",
    "Power Distributor",
    "Power Plant",
    "Prospector Limpet Controller",
    "Pulse Laser",
    "Rail Gun",
    "Refinery",
    "Seeker Missile Rack",
    "Sensors",
    "Shield Booster",
    "Shield Cell Bank",
    "Shield Generator",
    "Thrusters",
    "Torpedo Pylon",
];

/// Journal item-symbol prefixes → module type (longest/most specific first where prefixes nest).
const ITEM_PREFIXES: &[(&str, &str)] = &[
    ("int_hyperdrive", "Frame Shift Drive"),
    ("int_engine", "Thrusters"),
    ("int_powerplant", "Power Plant"),
    ("int_powerdistributor", "Power Distributor"),
    ("int_lifesupport", "Life Support"),
    ("int_sensors", "Sensors"),
    ("int_shieldgenerator", "Shield Generator"),
    ("int_shieldcellbank", "Shield Cell Bank"),
    ("int_hullreinforcement", "Hull Reinforcement Package"),
    ("int_fuelscoop", "Fuel Scoop"),
    ("int_refinery", "Refinery"),
    ("int_repairer", "Auto Field-Maintenance Unit"),
    ("int_fsdinterdictor", "Frame Shift Drive Interdictor"),
    ("int_detailedsurfacescanner", "Detailed Surface Scanner"),
    ("int_dronecontrol_collection", "Collector Limpet Controller"),
    ("int_dronecontrol_fueltransfer", "Fuel Transfer Limpet Controller"),
    ("int_dronecontrol_resourcesiphon", "Hatch Breaker Limpet Controller"),
    ("int_dronecontrol_prospector", "Prospector Limpet Controller"),
    ("int_cargorack", "Cargo Rack"),
    ("int_modulereinforcement", "Module Reinforcement Package"),
    ("hpt_shieldbooster", "Shield Booster"),
    ("hpt_chafflauncher", "Chaff Launcher"),
    ("hpt_electroniccountermeasure", "Electronic Countermeasure"),
    ("hpt_heatsinklauncher", "Heat Sink Launcher"),
    ("hpt_plasmapointdefence", "Point Defence"),
    ("hpt_cargoscanner", "Manifest Scanner"),
    ("hpt_cloudscanner", "Frame Shift Wake Scanner"),
    ("hpt_crimescanner", "Kill Warrant Scanner"),
    ("hpt_beamlaser", "Beam Laser"),
    ("hpt_pulselaserburst", "Burst Laser"),
    ("hpt_pulselaser", "Pulse Laser"),
    ("hpt_cannon", "Cannon"),
    ("hpt_slugshot", "Fragment Cannon"),
    ("hpt_multicannon", "Multi-cannon"),
    ("hpt_plasmaaccelerator", "Plasma Accelerator"),
    ("hpt_railgun", "Rail Gun"),
    ("hpt_basicmissilerack", "Seeker Missile Rack"),
    ("hpt_dumbfiremissilerack", "Missile Rack"),
    ("hpt_minelauncher", "Mine Launcher"),
    ("hpt_advancedtorppylon", "Torpedo Pylon"),
    ("hpt_causticmissile", "Enzyme Missile Rack"),
    ("hpt_mining_abrblstr", "Abrasion Blaster"),
    ("hpt_mininglaser", "Mining Laser"),
];

/// Short/legacy names (as name keys) → module type.
const TYPE_ALIASES: &[(&str, &str)] = &[
    ("surfacescanner", "Detailed Surface Scanner"),
    ("dss", "Detailed Surface Scanner"),
    ("wakescanner", "Frame Shift Wake Scanner"),
    ("fsdwakescanner", "Frame Shift Wake Scanner"),
    ("cloudscanner", "Frame Shift Wake Scanner"),
    ("cargoscanner", "Manifest Scanner"),
    ("killwarrant", "Kill Warrant Scanner"),
    ("kws", "Kill Warrant Scanner"),
    ("crimescanner", "Kill Warrant Scanner"),
    ("afm", "Auto Field-Maintenance Unit"),
    ("afmu", "Auto Field-Maintenance Unit"),
    ("repairer", "Auto Field-Maintenance Unit"),
    ("autofieldmaintenance", "Auto Field-Maintenance Unit"),
    ("hrp", "Hull Reinforcement Package"),
    ("hullreinforcement", "Hull Reinforcement Package"),
    ("fsd", "Frame Shift Drive"),
    ("hyperdrive", "Frame Shift Drive"),
    ("fsdinterdictor", "Frame Shift Drive Interdictor"),
    ("interdictor", "Frame Shift Drive Interdictor"),
    ("engine", "Thrusters"),
    ("engines", "Thrusters"),
    ("sensor", "Sensors"),
    ("radar", "Sensors"),
    ("shield", "Shield Generator"),
    ("shieldgen", "Shield Generator"),
    ("shieldcell", "Shield Cell Bank"),
    ("scb", "Shield Cell Bank"),
    ("ecm", "Electronic Countermeasure"),
    ("chaff", "Chaff Launcher"),
    ("heatsink", "Heat Sink Launcher"),
    ("pointdefense", "Point Defence"),
    ("multicannons", "Multi-cannon"),
    ("railgun", "Rail Gun"),
    ("railguns", "Rail Gun"),
    ("fragmentcannons", "Fragment Cannon"),
    ("seekermissilerack", "Seeker Missile Rack"),
    ("dumbfiremissilerack", "Missile Rack"),
    ("torpedo", "Torpedo Pylon"),
    ("collectionlimpet", "Collector Limpet Controller"),
    ("collectorlimpet", "Collector Limpet Controller"),
    ("fueltransferlimpet", "Fuel Transfer Limpet Controller"),
    ("hatchbreakerlimpet", "Hatch Breaker Limpet Controller"),
    ("prospectinglimpet", "Prospector Limpet Controller"),
    ("prospectorlimpet", "Prospector Limpet Controller"),
    ("refineries", "Refinery"),
];

/// Resolves a module type name (`"Frame Shift Drive"`, `"Multi-cannon"`, `"Surface Scanner"`) or a
/// journal item symbol (`"int_hyperdrive_overcharge_size5_class5"`, `"krait_mkii_armour_grade1"`)
/// to the canonical engineering module type. `None` for modules that cannot be engineered.
pub fn module_type(module_symbol_or_type: &str) -> Option<&'static str> {
    static TYPE_KEYS: LazyLock<FxHashMap<String, &'static str>> = LazyLock::new(|| {
        let mut m: FxHashMap<String, &'static str> = MODULE_TYPES.iter().map(|t| (name_key(t), *t)).collect();
        for (alias, t) in TYPE_ALIASES {
            m.entry((*alias).to_string()).or_insert(t);
        }
        m
    });
    let raw = module_symbol_or_type.trim();
    if let Some(t) = TYPE_KEYS.get(&name_key(raw)) {
        return Some(t);
    }
    let sym = crate::commodities::symbol(raw);
    if sym.contains("_armour_") {
        return Some("Armour");
    }
    ITEM_PREFIXES.iter().find(|(p, _)| sym.starts_with(p)).map(|(_, t)| *t)
}

/// All canonical module types.
pub fn module_types() -> &'static [&'static str] {
    MODULE_TYPES
}

/// The module type blueprints are filed under (seeker racks share the missile-rack blueprints).
fn blueprint_module(t: &str) -> &str {
    if t == "Seeker Missile Rack" { "Missile Rack" } else { t }
}

// ---------------------------------------------------------------------------------------------
// Data + indexes
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Data {
    blueprints: Vec<Blueprint>,
    experimentals: Vec<Experimental>,
    engineers: Vec<Engineer>,
    synthesis: Vec<SynthesisRecipe>,
    tech_broker: Vec<TechBrokerUnlock>,
    odyssey_upgrades: Vec<OdysseyUpgrade>,
    odyssey_mods: Vec<OdysseyMod>,
}

struct Db {
    data: Data,
    bp_by_symbol: FxHashMap<String, Vec<usize>>,
    exp_by_symbol: FxHashMap<String, usize>,
    eng_by_id: FxHashMap<u32, usize>,
    eng_by_key: FxHashMap<String, usize>,
}

static DB: LazyLock<Db> = LazyLock::new(|| {
    let data: Data = serde_json::from_str(DATA_JSON).unwrap_or_default();
    let mut bp_by_symbol: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (i, b) in data.blueprints.iter().enumerate() {
        bp_by_symbol.entry(b.symbol.to_ascii_lowercase()).or_default().push(i);
    }
    let mut exp_by_symbol = FxHashMap::default();
    for (i, e) in data.experimentals.iter().enumerate() {
        for s in std::iter::once(&e.symbol).chain(&e.aliases) {
            exp_by_symbol.entry(s.to_ascii_lowercase()).or_insert(i);
        }
    }
    let mut eng_by_id = FxHashMap::default();
    let mut eng_by_key = FxHashMap::default();
    for (i, e) in data.engineers.iter().enumerate() {
        if e.id != 0 {
            eng_by_id.insert(e.id, i);
        }
        for k in engineer_keys(&e.name) {
            eng_by_key.entry(k).or_insert(i);
        }
    }
    Db { data, bp_by_symbol, exp_by_symbol, eng_by_id, eng_by_key }
});

/// Lookup keys for an engineer name: full name, name without a quoted nickname
/// (`Tod 'The Blaster' McQuinn` → `Tod McQuinn`), and without a title (`Colonel`, `Professor`, `The`).
fn engineer_keys(name: &str) -> Vec<String> {
    let mut keys = vec![name_key(name)];
    let mut no_nick = String::new();
    let mut in_quote = false;
    for c in name.chars() {
        match c {
            '\'' | '"' => in_quote = !in_quote,
            _ if !in_quote => no_nick.push(c),
            _ => {}
        }
    }
    keys.push(name_key(&no_nick));
    for title in ["The ", "Colonel ", "Professor "] {
        if let Some(rest) = name.strip_prefix(title) {
            keys.push(name_key(rest));
        }
    }
    keys.retain(|k| !k.is_empty());
    keys.dedup();
    keys
}

// ---------------------------------------------------------------------------------------------
// Blueprints
// ---------------------------------------------------------------------------------------------

/// All ship blueprints (one entry per module type × blueprint).
pub fn blueprints() -> &'static [Blueprint] {
    &DB.data.blueprints
}

/// Looks a blueprint up by journal `BlueprintName` (case-insensitive). Generic names such as
/// `"Weapon_Overcharged"` or `"Misc_Shielded"` exist for several module types with identical
/// recipes; the first is returned — use [`blueprint_for`] when the module is known. Module-specific
/// legacy spellings (`"Sensor_Sensor_LightWeight"`, `"HeatSinkLauncher_LightWeight"`, `"AFM_Shielded"`)
/// are resolved by their final word and module hint.
pub fn blueprint(symbol: &str) -> Option<&'static Blueprint> {
    let db = &*DB;
    let lower = symbol.trim().to_ascii_lowercase();
    if let Some(&i) = db.bp_by_symbol.get(&lower).and_then(|v| v.first()) {
        return Some(&db.data.blueprints[i]);
    }
    let tokens: Vec<&str> = lower.split('_').filter(|t| !t.is_empty()).collect();
    let (&suffix, rest) = tokens.split_last()?;
    let hint = rest.iter().rev().find_map(|t| module_type(t)).map(blueprint_module);
    blueprints()
        .iter()
        .find(|b| suffix_of(&b.symbol).eq_ignore_ascii_case(suffix) && hint.is_none_or(|h| b.module == h))
}

/// The blueprint `symbol` for a specific module (item symbol or module type), tolerant of legacy
/// spellings (matched on the final word within the module).
pub fn blueprint_for(module_symbol_or_type: &str, symbol: &str) -> Option<&'static Blueprint> {
    let t = blueprint_module(module_type(module_symbol_or_type)?);
    let sym = symbol.trim();
    let in_module = || blueprints().iter().filter(move |b| b.module == t);
    in_module().find(|b| b.is(sym)).or_else(|| {
        let suffix = sym.rsplit('_').next()?;
        in_module().find(|b| suffix_of(&b.symbol).eq_ignore_ascii_case(suffix))
    })
}

fn suffix_of(symbol: &str) -> &str {
    symbol.rsplit('_').next().unwrap_or(symbol)
}

/// All blueprints applicable to a module (journal item symbol or module type name).
pub fn blueprints_for_module(module_symbol_or_type: &str) -> Vec<&'static Blueprint> {
    let Some(t) = module_type(module_symbol_or_type) else { return Vec::new() };
    let t = blueprint_module(t);
    blueprints().iter().filter(|b| b.module == t).collect()
}

// ---------------------------------------------------------------------------------------------
// Experimentals
// ---------------------------------------------------------------------------------------------

/// All experimental effects.
pub fn experimentals() -> &'static [Experimental] {
    &DB.data.experimentals
}

/// Looks an experimental up by journal `ExperimentalEffect` symbol (or alias), case-insensitively.
pub fn experimental(symbol: &str) -> Option<&'static Experimental> {
    let db = &*DB;
    db.exp_by_symbol.get(&symbol.trim().to_ascii_lowercase()).map(|&i| &db.data.experimentals[i])
}

/// Experimental effects applicable to a module (journal item symbol or module type name).
/// A seeker rack symbol (`hpt_basicmissilerack_*`) gets the seeker-only effects, dumbfire racks the
/// dumbfire-only ones.
pub fn experimentals_for(module_symbol_or_type: &str) -> Vec<&'static Experimental> {
    let Some(t) = module_type(module_symbol_or_type) else { return Vec::new() };
    experimentals().iter().filter(|e| e.applies_to(t)).collect()
}

// ---------------------------------------------------------------------------------------------
// Engineers
// ---------------------------------------------------------------------------------------------

/// All engineers (ship first, then on-foot).
pub fn engineers() -> &'static [Engineer] {
    &DB.data.engineers
}

/// Looks an engineer up by journal `EngineerID`.
pub fn engineer_by_id(id: u32) -> Option<&'static Engineer> {
    let db = &*DB;
    db.eng_by_id.get(&id).map(|&i| &db.data.engineers[i])
}

/// Looks an engineer up by name, case-insensitively and tolerant of nicknames and titles
/// (`"Tod McQuinn"`, `"Bris Dekker"`, `"Palin"`).
pub fn engineer_by_name(name: &str) -> Option<&'static Engineer> {
    let db = &*DB;
    db.eng_by_key.get(&name_key(name)).map(|&i| &db.data.engineers[i])
}

/// Engineers offering a module (item symbol or type) with their max grade, best first.
pub fn engineers_for(module_symbol_or_type: &str) -> Vec<(&'static Engineer, u8)> {
    let Some(t) = module_type(module_symbol_or_type) else { return Vec::new() };
    let t = blueprint_module(t);
    let mut v: Vec<(&'static Engineer, u8)> =
        engineers().iter().filter_map(|e| e.modules.iter().find(|m| m.module == t).map(|m| (e, m.max_grade))).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));
    v
}

// ---------------------------------------------------------------------------------------------
// Synthesis, tech broker, Odyssey
// ---------------------------------------------------------------------------------------------

/// All synthesis recipes (Basic/Standard/Premium per family).
pub fn synthesis_recipes() -> &'static [SynthesisRecipe] {
    &DB.data.synthesis
}

/// Looks a synthesis recipe up by journal `Synthesis.Name` (e.g. `"FSD Premium"`), case-insensitively.
/// A few names are shared by two families (`"Ammo Basic"`: SRV and small-calibre ammo); use
/// [`synthesis_recipe_matching`] with the event's `Materials` to tell them apart.
pub fn synthesis_recipe(name: &str) -> Option<&'static SynthesisRecipe> {
    let n = name.trim();
    synthesis_recipes().iter().find(|r| r.name.eq_ignore_ascii_case(n))
}

/// The recipe named `name` whose cost equals `materials` (journal `Synthesis.Materials`, any symbol
/// spelling); falls back to the first recipe with that name.
pub fn synthesis_recipe_matching(name: &str, materials: &[(String, u32)]) -> Option<&'static SynthesisRecipe> {
    let n = name.trim();
    let mut used: Vec<(String, u32)> = materials.iter().map(|(s, c)| (crate::commodities::symbol(s), *c)).collect();
    used.sort();
    let named = || synthesis_recipes().iter().filter(move |r| r.name.eq_ignore_ascii_case(n));
    named()
        .find(|r| {
            let mut cost: Vec<(String, u32)> = r.cost.iter().map(|i| (i.symbol.clone(), i.count)).collect();
            cost.sort();
            cost == used
        })
        .or_else(|| named().next())
}

/// All technology-broker unlocks.
pub fn tech_broker_unlocks() -> &'static [TechBrokerUnlock] {
    &DB.data.tech_broker
}

/// Suit/weapon grade upgrade recipes.
pub fn odyssey_upgrades() -> &'static [OdysseyUpgrade] {
    &DB.data.odyssey_upgrades
}

/// On-foot engineer modifications.
pub fn odyssey_mods() -> &'static [OdysseyMod] {
    &DB.data.odyssey_mods
}

/// Upgradeable suits and weapons.
pub fn odyssey_items() -> &'static [OdysseyItem] {
    ODYSSEY_ITEMS
}

/// Looks a suit/weapon up by journal symbol (`"utilitysuit_class3"`, `"$wpn_m_sniper_plasma_charged_name;"`).
pub fn odyssey_item(symbol: &str) -> Option<&'static OdysseyItem> {
    let s = crate::commodities::symbol(symbol);
    let base = match s.rsplit_once("_class") {
        Some((b, g)) if g.parse::<u8>().is_ok() => b,
        _ => s.as_str(),
    };
    ODYSSEY_ITEMS.iter().find(|i| i.symbol == base)
}

/// The recipe to upgrade an item to `grade` (2..=5).
pub fn odyssey_upgrade(item_symbol: &str, grade: u8) -> Option<&'static OdysseyUpgrade> {
    let item = odyssey_item(item_symbol)?;
    odyssey_upgrades().iter().find(|u| u.kind == item.kind && u.family == item.family && u.grade == grade)
}

/// Modifications available for an item (suit or weapon symbol), weapon-family-specific recipes
/// included.
pub fn odyssey_mods_for(item_symbol: &str) -> Vec<&'static OdysseyMod> {
    let Some(item) = odyssey_item(item_symbol) else { return Vec::new() };
    odyssey_mods()
        .iter()
        .filter(|m| m.kind == item.kind && m.variant.as_deref().is_none_or(|v| v == item.family))
        .collect()
}

/// A modification by journal symbol (the first recipe when several weapon families differ; use
/// [`odyssey_mods_for`] for the family-specific one).
pub fn odyssey_mod(symbol: &str) -> Option<&'static OdysseyMod> {
    let s = symbol.trim();
    odyssey_mods().iter().find(|m| m.symbol.eq_ignore_ascii_case(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ing(b: &Blueprint, grade: u8) -> Vec<(&str, u32)> {
        let mut v: Vec<(&str, u32)> = b.grade(grade).unwrap().iter().map(|i| (i.symbol.as_str(), i.count)).collect();
        v.sort();
        v
    }

    #[test]
    fn data_loads() {
        assert!(blueprints().len() >= 150, "blueprints: {}", blueprints().len());
        assert!(experimentals().len() >= 60, "experimentals: {}", experimentals().len());
        assert_eq!(engineers().len(), 38);
        assert!(synthesis_recipes().len() >= 60);
        assert!(tech_broker_unlocks().len() >= 50);
        assert_eq!(odyssey_upgrades().len(), 24);
        assert!(odyssey_mods().len() >= 30);
    }

    #[test]
    fn fsd_long_range_g5() {
        let b = blueprint("FSD_LongRange").expect("FSD_LongRange");
        assert_eq!(b.module, "Frame Shift Drive");
        assert_eq!(b.name, "Increased FSD Range");
        assert_eq!(b.max_grade(), 5);
        assert_eq!(ing(b, 5), vec![("arsenic", 1), ("chemicalmanipulators", 1), ("dataminedwake", 1)]);
        assert_eq!(ing(b, 1), vec![("disruptedwakeechoes", 1)]);
        assert_eq!(ing(b, 4), vec![("chemicaldistillery", 1), ("hyperspacetrajectories", 1), ("manganese", 1)]);
        let g5: Vec<&str> = b.engineers_for_grade(5).map(|e| e.engineer.as_str()).collect();
        for e in ["Felicity Farseer", "Elvira Martuuk", "Mel Brandon"] {
            assert!(g5.contains(&e), "{e} missing from {g5:?}");
        }
        assert!(!g5.contains(&"Professor Palin"));
        // Case-insensitive, and reachable from the SCO FSD item symbol.
        assert_eq!(blueprint("fsd_longrange").map(|b| b.name.as_str()), Some("Increased FSD Range"));
        let sco = blueprints_for_module("int_hyperdrive_overcharge_size5_class5");
        assert_eq!(sco.len(), 3);
        assert!(sco.iter().any(|b| b.symbol == "FSD_LongRange"));
    }

    #[test]
    fn blueprint_symbols_resolve() {
        assert_eq!(blueprint("Engine_Dirty").map(|b| b.module.as_str()), Some("Thrusters"));
        assert_eq!(blueprint("PowerPlant_Boosted").map(|b| b.module.as_str()), Some("Power Plant"));
        assert_eq!(blueprint("Sensor_LightWeight").map(|b| b.module.as_str()), Some("Sensors"));
        assert_eq!(blueprint("Sensor_Sensor_LightWeight").map(|b| b.symbol.as_str()), Some("Sensor_LightWeight"));
        let hs = blueprint("HeatSinkLauncher_HeatSinkCapacity").unwrap();
        assert_eq!((hs.module.as_str(), hs.symbol.as_str()), ("Heat Sink Launcher", "Misc_HeatSinkCapacity"));
        assert_eq!(blueprint("AFM_Shielded").map(|b| b.module.as_str()), Some("Auto Field-Maintenance Unit"));
        let mc = blueprint_for("hpt_multicannon_gimbal_large", "Weapon_Overcharged").unwrap();
        assert_eq!(mc.module, "Multi-cannon");
        assert!(mc.engineers.iter().any(|e| e.engineer == "Tod 'The Blaster' McQuinn" && e.max_grade == 5));
        assert_eq!(
            blueprint_for("hpt_basicmissilerack_fixed_medium", "Weapon_HighCapacity").map(|b| b.module.as_str()),
            Some("Missile Rack")
        );
        assert!(blueprint("Nonsense_Blueprint").is_none());
        // Every blueprint's ingredients resolve to known materials.
        for b in blueprints() {
            for g in &b.grades {
                for i in g {
                    assert!(matches!(i.kind(), IngredientKind::Material(_)), "{} {} {}", b.module, b.symbol, i.symbol);
                }
            }
        }
    }

    #[test]
    fn engineer_grade_corrections() {
        // engineering.md §6.3: Juri Ishmaak G3 on scanners; Lori Jameson Life Support G4.
        let juri = engineer_by_name("Juri Ishmaak").unwrap();
        assert_eq!(juri.max_grade_for("Kill Warrant Scanner"), Some(3));
        assert_eq!(juri.max_grade_for("hpt_cloudscanner_size0_class5"), Some(3));
        let lori = engineer_by_name("lori jameson").unwrap();
        assert_eq!(lori.max_grade_for("Life Support"), Some(4));
        let dorn = engineer_by_name("Etienne Dorn").unwrap();
        assert_eq!(dorn.max_grade_for("Rail Gun"), Some(5));
        assert_eq!(dorn.region, Region::Colonia);
        let farseer = engineer_by_id(300100).unwrap();
        assert_eq!((farseer.name.as_str(), farseer.system.as_str()), ("Felicity Farseer", "Deciat"));
        assert_eq!(farseer.max_grade_for("Frame Shift Drive"), Some(5));
        assert_eq!(farseer.unlock_cost, vec![Ingredient { symbol: "metaalloys".into(), count: 1 }]);
        assert!(farseer.refers_to.iter().any(|n| n == "Juri Ishmaak"));
        assert_eq!(engineer_by_name("Tod McQuinn").map(|e| e.id), Some(300260));
        assert_eq!(engineer_by_name("Bris Dekker").map(|e| e.id), Some(300140));
        let fsd: Vec<&str> =
            engineers_for("Frame Shift Drive").iter().filter(|(_, g)| *g == 5).map(|(e, _)| e.name.as_str()).collect();
        assert_eq!(fsd, vec!["Elvira Martuuk", "Felicity Farseer", "Mel Brandon"]);
        let yi = engineer_by_id(400013).unwrap();
        assert_eq!((yi.kind, yi.region), (EngineerKind::OnFoot, Region::Colonia));
        assert_eq!(yi.referred_by.len(), 3);
        let domino = engineer_by_name("Domino Green").unwrap();
        assert_eq!(domino.referral_cost, vec![Ingredient { symbol: "push".into(), count: 5 }]);
        assert!(domino.mods.iter().any(|m| m == "suit_improvedradar"));
    }

    #[test]
    fn experimentals_lookup() {
        let mm = experimental("special_fsd_heavy").unwrap();
        assert_eq!(mm.name, "Mass Manager");
        assert_eq!(mm.modules, vec!["Frame Shift Drive".to_string()]);
        assert_eq!(mm.cost.len(), 3);
        let fsd: Vec<&str> = experimentals_for("int_hyperdrive_size5_class5").iter().map(|e| e.name.as_str()).collect();
        assert_eq!(fsd.len(), 5, "{fsd:?}");
        assert!(experimental("special_engine_overloaded").is_some_and(|e| e.name == "Drag Drives"));
        // Seeker vs dumbfire racks.
        let seeker: Vec<&str> =
            experimentals_for("hpt_basicmissilerack_fixed_medium").iter().map(|e| e.symbol.as_str()).collect();
        let dumb: Vec<&str> =
            experimentals_for("hpt_dumbfiremissilerack_fixed_medium").iter().map(|e| e.symbol.as_str()).collect();
        assert!(seeker.contains(&"special_drag_munitions") && !dumb.contains(&"special_drag_munitions"));
        assert!(dumb.contains(&"special_fsd_interrupt") && !seeker.contains(&"special_fsd_interrupt"));
        assert!(seeker.contains(&"special_weapon_damage") && dumb.contains(&"special_weapon_damage"));
    }

    #[test]
    fn synthesis_and_brokers() {
        let fsd: Vec<&SynthesisRecipe> = synthesis_recipes().iter().filter(|r| r.family == "FSD Injection").collect();
        assert_eq!(fsd.len(), 3);
        let premium = synthesis_recipe("FSD Premium").unwrap();
        assert_eq!((premium.grade, premium.grade_name(), premium.effect.as_str()), (3, "Premium", "+100% Jump Range"));
        let mut syms: Vec<&str> = premium.cost.iter().map(|i| i.symbol.as_str()).collect();
        syms.sort();
        assert_eq!(syms, vec!["arsenic", "carbon", "germanium", "niobium", "polonium", "yttrium"]);
        assert_eq!(synthesis_recipe("fsd basic").map(|r| r.effect.as_str()), Some("+25% Jump Range"));
        // "Ammo Basic" is both SRV ammo and small-calibre (multi-cannon) ammo; materials decide.
        let mats = |v: &[(&str, u32)]| v.iter().map(|(s, n)| ((*s).to_string(), *n)).collect::<Vec<_>>();
        let mc = synthesis_recipe_matching("Ammo Basic", &mats(&[("iron", 2), ("sulphur", 2), ("Nickel", 1)])).unwrap();
        assert_eq!(mc.family, "Small Calibre Munitions");
        let srv = synthesis_recipe_matching("Ammo Basic", &mats(&[("phosphorus", 1), ("sulphur", 2)])).unwrap();
        assert_eq!(srv.family, "SRV Ammo");
        assert!(synthesis_recipes().iter().filter(|r| r.family == "Limpets").count() == 1);
        let sco = tech_broker_unlocks()
            .iter()
            .find(|t| t.module.as_deref() == Some("int_hyperdrive_overcharge_size5_class5"))
            .unwrap();
        assert_eq!(sco.broker, "Human");
        assert!(sco.cost.iter().any(|i| i.symbol == "thargoidtitandrivecomponent" && i.count == 1));
    }

    #[test]
    fn odyssey() {
        let up = odyssey_upgrade("utilitysuit_class1", 5).unwrap();
        let mut c: Vec<(&str, u32)> = up.cost.iter().map(|i| (i.symbol.as_str(), i.count)).collect();
        c.sort();
        assert_eq!(
            c,
            vec![
                ("carbonfibreplating", 12),
                ("graphene", 12),
                ("healthmonitor", 5),
                ("manufacturinginstructions", 5),
                ("suitschematic", 5)
            ]
        );
        assert_eq!(odyssey_item("explorationsuit_class2").unwrap().upgrade_credits(3), Some(2_250_000));
        assert_eq!(odyssey_item("wpn_m_launcher_rocket_sauto").unwrap().upgrade_credits(5), Some(8_750_000));
        let o2 = odyssey_mod("suit_increasedo2capacity").unwrap();
        assert_eq!((o2.name.as_str(), o2.credits), ("Increased Air Reserves", 750_000));
        let takada: Vec<&OdysseyMod> = odyssey_mods_for("wpn_s_pistol_laser_sauto")
            .into_iter()
            .filter(|m| m.symbol == "weapon_headshotdamage")
            .collect();
        assert_eq!(takada.len(), 1);
        assert_eq!(takada[0].credits, 750_000);
        assert!(odyssey_mods_for("tacticalsuit_class3").iter().all(|m| m.kind == OdysseyKind::Suit));
    }

    #[test]
    fn module_types_resolve() {
        assert_eq!(module_type("int_hyperdrive_overcharge_size5_class5"), Some("Frame Shift Drive"));
        assert_eq!(module_type("Hpt_PulseLaserBurst_Gimbal_Small"), Some("Burst Laser"));
        assert_eq!(module_type("hpt_pulselaser_fixed_small"), Some("Pulse Laser"));
        assert_eq!(module_type("krait_mkii_armour_grade3"), Some("Armour"));
        assert_eq!(module_type("Surface Scanner"), Some("Detailed Surface Scanner"));
        assert_eq!(module_type("multi-cannon"), Some("Multi-cannon"));
        assert_eq!(module_type("int_guardianfsdbooster_size5"), None);
        assert_eq!(module_type("int_fueltank_size5_class3"), None);
        assert_eq!(blueprints_for_module("int_fueltank_size5_class3").len(), 0);
        // Mining lasers only have the Merc blueprint.
        assert_eq!(module_type("hpt_mininglaser_fixed_small"), Some("Mining Laser"));
        assert_eq!(blueprints_for_module("hpt_mininglaser_fixed_small").len(), 1);
        // Every blueprint/experimental module is canonical.
        for b in blueprints() {
            assert!(MODULE_TYPES.contains(&b.module.as_str()), "{}", b.module);
        }
        for e in experimentals() {
            for m in &e.modules {
                assert!(MODULE_TYPES.contains(&m.as_str()), "{m}");
            }
        }
    }

    #[test]
    fn merc_blueprints() {
        assert_eq!(blueprints().iter().filter(|b| b.is_merc()).count(), 29);
        let rack = blueprint_for("int_cargorack_size5_class1", "CargoRackS5C1_Extended").unwrap();
        assert_eq!((rack.module.as_str(), rack.min_grade(), rack.max_grade()), ("Cargo Rack", 2, 4));
        assert_eq!(rack.merc_module.as_deref(), Some("5E Extended Cargo Rack"));
        assert_eq!((rack.merc_coins_for(1), rack.merc_coins_for(2), rack.merc_coins_for(4)), (0, 10, 40));
        assert!(rack.grade(1).unwrap().is_empty());
        assert_eq!(ing(rack, 2), vec![("manganese", 1), ("mechanicalscrap", 1), ("vanadium", 1)]);
        assert!(rack.engineers_for_grade(4).any(|e| e.engineer == "Ram Tah"));
        // Scoop Rate Enhanced fits any fuel scoop, from grade 1.
        let scoop = blueprint("fuelscoop_efficiency").unwrap();
        assert_eq!((scoop.min_grade(), scoop.merc_module.as_deref(), scoop.merc_coins_for(5)), (1, None, 30));
        // Merc seeker racks are filed with the missile-rack blueprints.
        let seekers = blueprints_for_module("hpt_basicmissilerack_fixed_large");
        assert!(seekers.iter().any(|b| b.is("seekermissileracklarge_exposingmissiles")));
        // Ordinary blueprints cost no coins, and Merc blueprints leave the engineers' module grades alone.
        assert!(!blueprint("FSD_LongRange").unwrap().is_merc());
        assert_eq!(engineer_by_name("Juri Ishmaak").unwrap().max_grade_for("Missile Rack"), Some(3));
    }
}
