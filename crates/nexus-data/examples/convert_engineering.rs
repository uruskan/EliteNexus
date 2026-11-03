//! Development-time converter for `nexus_data::engineering`.
//!
//! Inputs (all under `data/`):
//! * `engineering/edengineer/blueprints.json` — EDEngineer, MIT, © 2016 Max: ship blueprint grades,
//!   engineers per grade, experimental effects and their costs, effect texts, ship-engineer unlock
//!   costs, synthesis effect texts.
//! * `engineering/eddiscovery-src/*.cs` — EDDiscovery `EliteDangerousCore`, Apache-2.0,
//!   © EDDiscovery development team: journal symbols (`BlueprintName` / `ExperimentalEffect`),
//!   engineers (locations, requirements), synthesis, technology broker, Odyssey upgrades/mods,
//!   ingredient short codes (`MCMRType.cs`).
//! * `fdevids/engineers.csv` — EDCD/FDevIDs: journal `EngineerID`, system address, market id.
//! * [`MERC`] below: the Merc blueprints, bought with Merc Coins.
//!
//! Output: `engineering/nexus/engineering.json` (one record per line) which the library embeds.
//!
//! Run:  `cargo run -p nexus-data --example convert_engineering`
//! The validation report (unresolved names, EDEngineer ↔ EDDiscovery disagreements, corrections
//! applied) is printed to stdout.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use nexus_data::commodities::{self, name_key};
use nexus_data::materials;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Report {
    sections: BTreeMap<&'static str, Vec<String>>,
}

impl Report {
    fn add(&mut self, section: &'static str, msg: impl Into<String>) {
        self.sections.entry(section).or_default().push(msg.into());
    }
    fn print(&self) {
        for (s, msgs) in &self.sections {
            println!("\n== {s} ({}) ==", msgs.len());
            for m in msgs {
                println!("  {m}");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Minimal C# tokenizer / call extractor
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Str(String),
    Ident(String),
    Num(String),
    Punct(char),
}

fn tokenize(src: &str) -> Vec<Tok> {
    let b: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c.is_whitespace() || c == '\u{feff}' {
            i += 1;
        } else if c == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else if c == '@' && next == Some('"') {
            // Verbatim string: "" is an escaped quote.
            i += 2;
            let mut s = String::new();
            while i < b.len() {
                if b[i] == '"' {
                    if b.get(i + 1) == Some(&'"') {
                        s.push('"');
                        i += 2;
                        continue;
                    }
                    break;
                }
                s.push(b[i]);
                i += 1;
            }
            i += 1;
            out.push(Tok::Str(s));
        } else if c == '"' {
            i += 1;
            let mut s = String::new();
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' && i + 1 < b.len() {
                    i += 1;
                    s.push(match b[i] {
                        'n' => '\n',
                        't' => '\t',
                        x => x,
                    });
                } else {
                    s.push(b[i]);
                }
                i += 1;
            }
            i += 1;
            out.push(Tok::Str(s));
        } else if c == '\'' {
            // char literal
            i += 1;
            while i < b.len() && b[i] != '\'' {
                if b[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_' || b[i] == '.') {
                i += 1;
            }
            out.push(Tok::Ident(b[start..i].iter().collect()));
        } else if c.is_ascii_digit() || (c == '-' && next.is_some_and(|n| n.is_ascii_digit())) {
            let start = i;
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '.') {
                i += 1;
            }
            out.push(Tok::Num(b[start..i].iter().collect()));
        } else {
            out.push(Tok::Punct(c));
            i += 1;
        }
    }
    out
}

type Arg = Vec<Tok>;
/// Ingredient list: (symbol, count).
type Ings = Vec<(String, u32)>;
/// One EDD blueprint grade: recipe and the engineers offering it.
type EddGrade = (Ings, BTreeSet<String>);
/// Odyssey upgrade row: (kind, family, grade, cost).
type OUpgrade = (&'static str, String, u8, Ings);

/// Every `new <name>(…)` (or `<name>(…)` when `require_new` is false) with its top-level arguments.
fn calls(toks: &[Tok], name: &str, require_new: bool) -> Vec<Vec<Arg>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < toks.len() {
        let is_call = matches!(&toks[i], Tok::Ident(n) if n == name)
            && toks[i + 1] == Tok::Punct('(')
            && (!require_new || (i > 0 && matches!(&toks[i - 1], Tok::Ident(n) if n == "new")));
        if !is_call {
            i += 1;
            continue;
        }
        let mut depth = 0i32;
        let mut nest = 0i32;
        let mut args = Vec::new();
        let mut cur: Arg = Vec::new();
        let mut j = i + 1;
        while j < toks.len() {
            match &toks[j] {
                Tok::Punct('(') => {
                    depth += 1;
                    if depth > 1 {
                        cur.push(toks[j].clone());
                    }
                }
                Tok::Punct(')') => {
                    depth -= 1;
                    if depth == 0 {
                        if !cur.is_empty() {
                            args.push(std::mem::take(&mut cur));
                        }
                        break;
                    }
                    cur.push(toks[j].clone());
                }
                Tok::Punct(c @ ('{' | '[')) => {
                    nest += 1;
                    cur.push(Tok::Punct(*c));
                }
                Tok::Punct(c @ ('}' | ']')) => {
                    nest -= 1;
                    cur.push(Tok::Punct(*c));
                }
                Tok::Punct(',') if depth == 1 && nest == 0 => args.push(std::mem::take(&mut cur)),
                t => cur.push(t.clone()),
            }
            j += 1;
        }
        out.push(args);
        i = j + 1;
    }
    out
}

fn a_str(a: &Arg) -> Option<&str> {
    match a.as_slice() {
        [Tok::Str(x)] => Some(x),
        _ => None,
    }
}

fn a_ident(a: &Arg) -> Option<&str> {
    match a.as_slice() {
        [Tok::Ident(x)] => Some(x),
        _ => None,
    }
}

fn a_num(a: &Arg) -> Option<f64> {
    match a.as_slice() {
        [Tok::Num(x)] => x.trim_end_matches(['m', 'f', 'd', 'M', 'F', 'D']).parse().ok(),
        _ => None,
    }
}

fn a_bool(a: &Arg) -> Option<bool> {
    match a_ident(a)? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------------------------

/// Exact-symbol check against the library tables.
fn known_symbol(sym: &str) -> bool {
    materials::material(sym).is_some_and(|m| m.symbol == sym)
        || materials::micro_resource(sym).is_some_and(|m| m.symbol == sym)
        || commodities::commodity(sym).is_some_and(|c| c.symbol == sym)
}

#[derive(Clone, Copy, PartialEq)]
enum Pref {
    Material,
    Commodity,
    Micro,
}

/// English name → journal symbol through the library tables, trying the preferred table first.
fn symbol_by_name(name: &str, pref: Pref) -> Option<String> {
    let mat = || materials::material(name).map(|m| m.symbol.clone());
    let com = || commodities::commodity(name).map(|c| c.symbol.clone());
    let mic = || materials::micro_resource(name).map(|m| m.symbol.clone());
    match pref {
        Pref::Material => mat().or_else(mic).or_else(com),
        Pref::Commodity => com().or_else(mat).or_else(mic),
        Pref::Micro => mic().or_else(mat).or_else(com),
    }
}

/// EDD short code table (`MCMRType.cs`): lower-case code → journal symbol.
struct Codes {
    by_code: HashMap<String, String>,
    /// English name key → symbol, for names that the FDevIDs tables spell differently.
    by_name: HashMap<String, String>,
}

fn load_codes(src: &str, rep: &mut Report) -> Codes {
    let toks = tokenize(src);
    let mut by_code = HashMap::new();
    let mut by_name = HashMap::new();
    for args in calls(&toks, "Add", false) {
        let Some(cat) = args.first().and_then(a_ident).and_then(|c| c.strip_prefix("CatType.")) else { continue };
        let Some(k) = args.iter().position(|a| a_ident(a).is_some_and(|i| i.starts_with("MCMR."))) else { continue };
        let fdname = a_ident(&args[k]).unwrap_or_default().trim_start_matches("MCMR.").to_ascii_lowercase();
        let strings: Vec<&str> = args[k + 1..].iter().filter_map(a_str).collect();
        let Some(&english) = strings.first() else { continue };
        let pref = match cat {
            "Commodity" => Pref::Commodity,
            "Raw" | "Manufactured" | "Encoded" => Pref::Material,
            _ => Pref::Micro,
        };
        let symbol = if known_symbol(&fdname) {
            fdname.clone()
        } else if let Some(s) = symbol_by_name(english, pref) {
            rep.add("EDD symbol fixed via English name", format!("{fdname} ({english}) -> {s}"));
            s
        } else {
            fdname.clone()
        };
        by_name.entry(name_key(english)).or_insert_with(|| symbol.clone());
        if let Some(short) = strings.get(1).filter(|s| !s.is_empty()) {
            by_code.insert(short.to_ascii_lowercase(), symbol);
        }
    }
    Codes { by_code, by_name }
}

/// `"1Fe,1CCo, 1HDC"` → `[("iron",1), …]`, duplicates summed.
fn parse_codes(list: &str, codes: &Codes, ctx: &str, rep: &mut Report) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    for item in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let digits: String = item.chars().take_while(char::is_ascii_digit).collect();
        let code = &item[digits.len()..];
        let Ok(n) = digits.parse::<u32>() else {
            rep.add("EDD ingredient parse errors", format!("{ctx}: {item}"));
            continue;
        };
        let Some(sym) = codes.by_code.get(&code.to_ascii_lowercase()) else {
            rep.add("EDD unknown short codes", format!("{ctx}: {code}"));
            continue;
        };
        if !known_symbol(sym) {
            rep.add("Symbols not in FDevIDs tables (kept)", format!("{ctx}: {sym}"));
        }
        match out.iter_mut().find(|(s, _)| s == sym) {
            Some((_, c)) => {
                rep.add("EDD duplicate ingredient (summed)", format!("{ctx}: {sym}"));
                *c += n;
            }
            None => out.push((sym.clone(), n)),
        }
    }
    out
}

/// EDEngineer spellings that differ from FDevIDs/EDD names.
const NAME_ALIASES: &[(&str, &str)] = &[("Abnormal Compact Emission Data", "compactemissionsdata")];

/// EDEngineer English ingredient name → symbol.
fn resolve_name(name: &str, codes: &Codes, pref: Pref, ctx: &str, rep: &mut Report) -> Option<String> {
    let s = NAME_ALIASES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, s)| (*s).to_string())
        .or_else(|| symbol_by_name(name, pref))
        .or_else(|| codes.by_name.get(&name_key(name)).cloned())
        // e.g. "Xihe Companions" is the symbol `xihecompanions` ("Xihe Biomorphic Companions").
        .or_else(|| Some(name_key(name)).filter(|k| known_symbol(k)));
    match &s {
        Some(sym) if !known_symbol(sym) => {
            rep.add("Symbols not in FDevIDs tables (kept)", format!("{ctx}: {name} -> {sym}"))
        }
        None => rep.add("EDEngineer names not resolved (dropped)", format!("{ctx}: {name}")),
        _ => {}
    }
    s
}

// ---------------------------------------------------------------------------------------------
// Module types
// ---------------------------------------------------------------------------------------------

const WEAPONS: &[&str] = &[
    "Beam Laser",
    "Burst Laser",
    "Pulse Laser",
    "Cannon",
    "Multi-cannon",
    "Fragment Cannon",
    "Plasma Accelerator",
    "Rail Gun",
    "Missile Rack",
    "Seeker Missile Rack",
    "Mine Launcher",
    "Torpedo Pylon",
];

/// EDEngineer `Type` → canonical module type.
fn edengineer_type(t: &str) -> Option<&'static str> {
    Some(match t {
        "Surface Scanner" => "Detailed Surface Scanner",
        "Wake Scanner" => "Frame Shift Wake Scanner",
        other => nexus_data::engineering::MODULE_TYPES.iter().find(|m| **m == other)?,
    })
}

/// EDD `ModuleTypes.X` (or the bare name in special-effect lists) → canonical module types.
fn edd_types(t: &str, fdname: &str) -> Vec<&'static str> {
    let t = t.trim().trim_start_matches("ItemData.ShipModule.ModuleTypes.");
    let one = |s: &'static str| vec![s];
    match t {
        "Armour" => one("Armour"),
        "AutoFieldMaintenanceUnit" => one("Auto Field-Maintenance Unit"),
        "BeamLaser" => one("Beam Laser"),
        "BurstLaser" => one("Burst Laser"),
        "PulseLaser" => one("Pulse Laser"),
        "Cannon" => one("Cannon"),
        "CargoScanner" => one("Manifest Scanner"),
        "ChaffLauncher" => one("Chaff Launcher"),
        "CollectorLimpetController" => one("Collector Limpet Controller"),
        "ElectronicCountermeasure" => one("Electronic Countermeasure"),
        "FragmentCannon" => one("Fragment Cannon"),
        "FrameShiftDrive" => one("Frame Shift Drive"),
        "FrameShiftDriveInterdictor" => one("Frame Shift Drive Interdictor"),
        "FuelScoop" => one("Fuel Scoop"),
        "FuelTransferLimpetController" => one("Fuel Transfer Limpet Controller"),
        "HatchBreakerLimpetController" => one("Hatch Breaker Limpet Controller"),
        "HeatSinkLauncher" => one("Heat Sink Launcher"),
        "HullReinforcementPackage" => one("Hull Reinforcement Package"),
        "KillWarrantScanner" => one("Kill Warrant Scanner"),
        // EDD files the mine-launcher recipes and three mine experimentals under LifeSupport.
        "LifeSupport" if fdname.starts_with("weapon_") || fdname.starts_with("special_") => one("Mine Launcher"),
        "LifeSupport" => one("Life Support"),
        "MineLauncher" => one("Mine Launcher"),
        "MissileRack" => one("Missile Rack"),
        "SeekerMissileRack" => one("Seeker Missile Rack"),
        "Multi_Cannon" => one("Multi-cannon"),
        "PlasmaAccelerator" => one("Plasma Accelerator"),
        "PointDefence" => one("Point Defence"),
        "PowerDistributor" => one("Power Distributor"),
        "PowerPlant" => one("Power Plant"),
        "ProspectorLimpetController" => one("Prospector Limpet Controller"),
        "RailGun" => one("Rail Gun"),
        "Refinery" => one("Refinery"),
        "Sensor" => one("Sensors"),
        "ShieldBooster" => one("Shield Booster"),
        "ShieldCellBank" => one("Shield Cell Bank"),
        "ShieldGenerator" => one("Shield Generator"),
        "SurfaceScanner" => one("Detailed Surface Scanner"),
        "Thrusters" => one("Thrusters"),
        "TorpedoPylon" => one("Torpedo Pylon"),
        "WakeScanner" => one("Frame Shift Wake Scanner"),
        "Weapon" => WEAPONS.to_vec(),
        _ => Vec::new(),
    }
}

/// Journal `BlueprintName` for an EDEngineer (module type, blueprint name).
fn blueprint_symbol(module: &str, name: &str) -> Option<&'static str> {
    const SCANNERS: &[&str] = &["Kill Warrant Scanner", "Manifest Scanner", "Frame Shift Wake Scanner"];
    let is_weapon = WEAPONS.contains(&module);
    Some(match (module, name) {
        ("Armour", "Blast Resistant") => "Armour_Explosive",
        ("Armour", "Heavy Duty") => "Armour_HeavyDuty",
        ("Armour", "Kinetic Resistant") => "Armour_Kinetic",
        ("Armour", "Lightweight") => "Armour_Advanced",
        ("Armour", "Thermal Resistant") => "Armour_Thermic",
        ("Hull Reinforcement Package", "Blast Resistant Hull Reinforcement") => "HullReinforcement_Explosive",
        ("Hull Reinforcement Package", "Heavy Duty Hull Reinforcement") => "HullReinforcement_HeavyDuty",
        ("Hull Reinforcement Package", "Kinetic Resistant Hull Reinforcement") => "HullReinforcement_Kinetic",
        ("Hull Reinforcement Package", "Lightweight Hull Reinforcement") => "HullReinforcement_Advanced",
        ("Hull Reinforcement Package", "Thermal Resistant Hull Reinforcement") => "HullReinforcement_Thermic",
        ("Shield Booster", "Blast Resistant") => "ShieldBooster_Explosive",
        ("Shield Booster", "Heavy Duty") => "ShieldBooster_HeavyDuty",
        ("Shield Booster", "Kinetic Resistant") => "ShieldBooster_Kinetic",
        ("Shield Booster", "Resistance Augmented") => "ShieldBooster_Resistive",
        ("Shield Booster", "Thermal Resistant") => "ShieldBooster_Thermic",
        ("Shield Generator", "Enhanced, Low Power Shields") => "ShieldGenerator_Optimised",
        ("Shield Generator", "Kinetic Resistant Shields") => "ShieldGenerator_Kinetic",
        ("Shield Generator", "Reinforced Shields") => "ShieldGenerator_Reinforced",
        ("Shield Generator", "Thermal Resistant Shields") => "ShieldGenerator_Thermic",
        ("Shield Cell Bank", "Rapid Charge") => "ShieldCellBank_Rapid",
        ("Shield Cell Bank", "Specialised") => "ShieldCellBank_Specialised",
        ("Frame Shift Drive", "Faster FSD Boot Sequence") => "FSD_FastBoot",
        ("Frame Shift Drive", "Increased FSD Range") => "FSD_LongRange",
        ("Frame Shift Drive", "Shielded FSD") => "FSD_Shielded",
        ("Frame Shift Drive Interdictor", "Expanded FSD Interdictor Capture Arc") => "FSDinterdictor_Expanded",
        ("Frame Shift Drive Interdictor", "Long Range FSD Interdictor") => "FSDinterdictor_LongRange",
        ("Thrusters", "Clean Drive Tuning") => "Engine_Tuned",
        ("Thrusters", "Dirty Drive Tuning") => "Engine_Dirty",
        ("Thrusters", "Drive Strengthening") => "Engine_Reinforced",
        ("Power Plant", "Armoured") => "PowerPlant_Armoured",
        ("Power Plant", "Low Emissions") => "PowerPlant_Stealth",
        ("Power Plant", "Overcharged") => "PowerPlant_Boosted",
        ("Power Distributor", "Charge Enhanced") => "PowerDistributor_HighFrequency",
        ("Power Distributor", "Engine Focused") => "PowerDistributor_PriorityEngines",
        ("Power Distributor", "High Charge Capacity") => "PowerDistributor_HighCapacity",
        ("Power Distributor", "Shielded") => "PowerDistributor_Shielded",
        ("Power Distributor", "System Focused") => "PowerDistributor_PrioritySystems",
        ("Power Distributor", "Weapon Focused") => "PowerDistributor_PriorityWeapons",
        ("Sensors", "Light Weight Scanner") => "Sensor_LightWeight",
        ("Sensors", "Long Range Scanner") => "Sensor_LongRange",
        ("Sensors", "Wide Angle Scanner") => "Sensor_WideAngle",
        ("Detailed Surface Scanner", "Expanded Probe Scanning Radius") => "Sensor_Expanded",
        (m, "Fast Scanner") if SCANNERS.contains(&m) => "Sensor_FastScan",
        (m, "Long Range Scanner") if SCANNERS.contains(&m) => "Sensor_LongRange",
        (m, "Wide Angle Scanner") if SCANNERS.contains(&m) => "Sensor_WideAngle",
        ("Chaff Launcher", "Ammo Capacity") => "Misc_ChaffCapacity",
        ("Heat Sink Launcher", "Ammo Capacity") => "Misc_HeatSinkCapacity",
        ("Point Defence", "Ammo Capacity") => "Misc_PointDefenseCapacity",
        (_, "Double Shot") if is_weapon => "Weapon_DoubleShot",
        (_, "Efficient Weapon") if is_weapon => "Weapon_Efficient",
        (_, "Focused Weapon") if is_weapon => "Weapon_Focused",
        (_, "High Capacity Magazine") if is_weapon => "Weapon_HighCapacity",
        (_, "Lightweight Mount") if is_weapon => "Weapon_LightWeight",
        (_, "Long Range Weapon") if is_weapon => "Weapon_LongRange",
        (_, "Overcharged Weapon") if is_weapon => "Weapon_Overcharged",
        (_, "Rapid Fire Modification") if is_weapon => "Weapon_RapidFire",
        (_, "Short Range Blaster") if is_weapon => "Weapon_ShortRange",
        (_, "Sturdy Mount") if is_weapon => "Weapon_Sturdy",
        (_, "Lightweight") => "Misc_LightWeight",
        (_, "Reinforced") => "Misc_Reinforced",
        (_, "Shielded") => "Misc_Shielded",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------
// Engineers
// ---------------------------------------------------------------------------------------------

struct FdevEngineer {
    id: u32,
    name: String,
    system_address: u64,
    market_id: u64,
}

fn engineer_keys(name: &str) -> Vec<String> {
    let mut no_nick = String::new();
    let mut in_quote = false;
    for c in name.chars() {
        match c {
            '\'' | '"' => in_quote = !in_quote,
            _ if !in_quote => no_nick.push(c),
            _ => {}
        }
    }
    let mut keys = vec![name_key(name), name_key(&no_nick)];
    for t in ["The ", "Colonel ", "Professor "] {
        if let Some(r) = name.strip_prefix(t) {
            keys.push(name_key(r));
        }
    }
    keys
}

fn canon_engineer(name: &str, fdev: &[FdevEngineer]) -> Option<String> {
    let k = name_key(name);
    fdev.iter().find(|e| engineer_keys(&e.name).contains(&k)).map(|e| e.name.clone())
}

/// On-foot engineer facts that changed after EDD's data (engineering.md §4.2, wiki-checked).
struct OnFoot {
    name: &'static str,
    discovery: &'static str,
    meeting: &'static str,
    referral: &'static str,
    /// (English name, count)
    referral_cost: &'static [(&'static str, u32)],
}

const fn on_foot(
    name: &'static str,
    discovery: &'static str,
    meeting: &'static str,
    referral: &'static str,
    referral_cost: &'static [(&'static str, u32)],
) -> OnFoot {
    OnFoot { name, discovery, meeting, referral, referral_cost }
}

const ON_FOOT: &[OnFoot] = &[
    on_foot(
        "Domino Green",
        "Public knowledge.",
        "Travel 100 light years in Apex Interstellar Transport shuttles.",
        "Provide 5 units of Push to learn about Kit Fowler.",
        &[("Push", 5)],
    ),
    on_foot(
        "Hero Ferrari",
        "Public knowledge.",
        "Complete 10 low-threat conflict zones.",
        "Provide 5 units of Settlement Defence Plans to learn about Wellington Beck.",
        &[("Settlement Defence Plans", 5)],
    ),
    on_foot(
        "Jude Navarro",
        "Public knowledge.",
        "Complete 10 Restore or Reactivation on-foot missions.",
        "Provide 5 units of Genetic Repair Meds to learn about Terra Velasquez.",
        &[("Genetic Repair Meds", 5)],
    ),
    on_foot(
        "Kit Fowler",
        "Fulfil Domino Green's referral requirement.",
        "Sell 5 Opinion Polls to bartenders.",
        "Provide 5 units of Surveillance Equipment to learn about Yarden Bond.",
        &[("Surveillance Equipment", 5)],
    ),
    on_foot(
        "Oden Geiger",
        "Fulfil Terra Velasquez's referral requirement.",
        "Sell 20 units of Biological Sample, Employee Genetic Data and/or Genetic Research to bartenders.",
        "",
        &[],
    ),
    on_foot(
        "Terra Velasquez",
        "Fulfil Jude Navarro's referral requirement.",
        "Complete 6 Covert Heist and/or Covert Theft on-foot missions.",
        "Provide 15 units of Financial Projections to learn about Oden Geiger.",
        &[("Financial Projections", 15)],
    ),
    on_foot(
        "Uma Laszlo",
        "Fulfil Wellington Beck's referral requirement.",
        "Reach Unfriendly reputation or lower with Sirius Corporation.",
        "",
        &[],
    ),
    on_foot(
        "Wellington Beck",
        "Fulfil Hero Ferrari's referral requirement.",
        "Sell 15 units of Classic Entertainment, Multimedia Entertainment and/or Cat Media to bartenders.",
        "Provide 5 units of InSight Entertainment Suites to learn about Uma Laszlo.",
        &[("InSight Entertainment Suite", 5)],
    ),
    on_foot(
        "Yarden Bond",
        "Fulfil Kit Fowler's referral requirement.",
        "Sell 5 Smear Campaign Plans to bartenders.",
        "",
        &[],
    ),
    on_foot(
        "Baltanos",
        "Public knowledge.",
        "Reach Friendly or better with the Colonia Council to receive an invitation mission.",
        "Provide 10 units of Faction Associates to learn about Yi Shen.",
        &[("Faction Associates", 10)],
    ),
    on_foot(
        "Eleanor Bresa",
        "Public knowledge.",
        "Dock and disembark at 5 settlements in the Colonia system.",
        "Provide 10 units of Digital Designs to learn about Yi Shen.",
        &[("Digital Designs", 10)],
    ),
    on_foot(
        "Rosa Dayette",
        "Public knowledge.",
        "Sell 10 Culinary Recipes and/or Cocktail Recipes to bartenders in Colonia.",
        "Provide 10 units of Manufacturing Instructions to learn about Yi Shen.",
        &[("Manufacturing Instructions", 10)],
    ),
    on_foot(
        "Yi Shen",
        "Fulfil the referral requirements of Baltanos, Eleanor Bresa and Rosa Dayette.",
        "Fulfil the referral requirements of Baltanos, Eleanor Bresa and Rosa Dayette.",
        "",
        &[],
    ),
];

/// On-foot modification names as shown in game (EDD names are terse or mislabelled).
fn mod_name(symbol: &str, edd_name: &str) -> String {
    match symbol {
        "weapon_accuracy" => "Improved Hip Fire Accuracy".into(),
        "weapon_headshotdamage" => "Headshot Damage".into(),
        "weapon_backpackreloading" => "Stowed Reloading".into(),
        "weapon_range" => "Greater Range".into(),
        _ => edd_name.split(':').next().unwrap_or(edd_name).trim().to_string(),
    }
}

/// A Merc blueprint: bought from the engineer, and each roll costs Merc Coins as well as materials.
/// Those for a MercGear module start at grade 2 (the module is bought at grade 1) and fit only it.
struct Merc {
    module: &'static str,
    /// Journal `BlueprintName`, matched case-insensitively.
    symbol: &'static str,
    name: &'static str,
    /// The MercGear module it is for; `None` = any module of the type.
    merc_module: Option<&'static str>,
    first_grade: u8,
    /// One roll per grade from `first_grade` up: (ingredients as short codes, Merc Coins).
    grades: &'static [(&'static str, u32)],
    /// (engineer, highest grade)
    engineers: &'static [(&'static str, u8)],
}

const fn merc(
    module: &'static str,
    symbol: &'static str,
    name: &'static str,
    merc_module: Option<&'static str>,
    first_grade: u8,
    grades: &'static [(&'static str, u32)],
    engineers: &'static [(&'static str, u8)],
) -> Merc {
    Merc { module, symbol, name, merc_module, first_grade, grades, engineers }
}

const MERC_PLASMA: &[(&str, u32)] = &[
    ("1P,1HCW", 10),
    ("1As,1HCW,1HDP", 15),
    ("1As,1HDP,1PA", 20),
    ("1HDP,1PA,1RFC,1CCom", 25),
    ("1Tc,1ThA,1MSC,1EFW", 30),
];
const MERC_EXTENDED: &[(&str, u32)] = &[("1MS,1V,1Mn", 10), ("1MS,1V,1Nb", 20), ("1ME,1HDC,1Sn,1Nb", 40)];
const MERC_SCREAMING: &[(&str, u32)] =
    &[("1C,1MS,1ME", 10), ("1C,1ME,1Sn", 15), ("1V,1CIF,1MC,1SFP", 25), ("1HDC,1MC,1CCom,1EFW", 50)];
const MERC_LOCKDOWN: &[(&str, u32)] =
    &[("1MS,1Ni,1V", 10), ("1MS,1ME,1W", 15), ("1MCF,1W,1Mo,1ThA", 25), ("1MC,1Mo,1Tc,1MSC", 45)];

const LASER_ENGINEERS: &[(&str, u8)] = &[("Broo Tarquin", 5), ("Mel Brandon", 5), ("The Dweller", 3)];
const MINING_ENGINEERS: &[(&str, u8)] = &[("Juri Ishmaak", 5), ("Liz Ryder", 3)];
const SEEKER_ENGINEERS: &[(&str, u8)] = &[("Juri Ishmaak", 5), ("Petra Olmanova", 5), ("Liz Ryder", 3)];
const DISTRIBUTOR_ENGINEERS: &[(&str, u8)] =
    &[("The Dweller", 5), ("Etienne Dorn", 5), ("Hera Tani", 3), ("Marco Qwent", 3)];
const CARGO_ENGINEERS: &[(&str, u8)] = &[("Bill Turner", 4), ("Ram Tah", 4), ("Tiana Fortune", 4)];
const FRAGMENT_ENGINEERS: &[(&str, u8)] =
    &[("Marsha Hicks", 5), ("Zacariah Nemo", 5), ("Tod 'The Blaster' McQuinn", 3)];

const MERC: &[Merc] = &[
    merc(
        "Abrasion Blaster",
        "abrasionblaster_farreaching",
        "Far-reaching",
        Some("1D Far-Reaching Abrasion Blaster"),
        2,
        &[("1S,1HDP,1MCF", 5), ("1Cr,1MCF,1HE", 5), ("1MCF,1CPo,1Se,1HV", 10), ("1CIF,1ThA,1BiC,1PHR", 20)],
        MINING_ENGINEERS,
    ),
    merc(
        "Beam Laser",
        "beamlaser_overloaded",
        "Overloaded",
        Some("4A Overloaded Beam Laser"),
        2,
        &[("1Cr,1CCo,1Ni", 10), ("1Cr,1CCo,1EA", 15), ("1EA,1HDC,1PCa,1FoC", 25), ("1Zr,1MSC,1EFW,1FoC", 50)],
        LASER_ENGINEERS,
    ),
    merc("Beam Laser", "beamlaser_thermalplasmaconversion", "Plasma conversion", None, 1, MERC_PLASMA, LASER_ENGINEERS),
    merc(
        "Burst Laser",
        "burstlaser_regenerative",
        "Regenerative",
        Some("2F Regenerative Burst Laser"),
        2,
        &[("1Ni,1MCF,1CCo", 10), ("1Ni,1CCo,1EA", 10), ("1CCe,1EA,1PCa,1RFC", 20), ("1EFW,1CPo,1PCa,1SS", 40)],
        LASER_ENGINEERS,
    ),
    merc("Burst Laser", "burstlaser_thermalplasmaconversion", "Plasma conversion", None, 1, MERC_PLASMA, LASER_ENGINEERS),
    merc(
        "Cannon",
        "cannon_forceimpact",
        "Force impact",
        Some("4B Force Impact Cannon"),
        2,
        &[("1V,1CCo,1MS", 10), ("1Nb,1CCo,1MS", 15), ("1Nb,1CCe,1HDC,1PCa", 20), ("1EFW,1CPo,1PCa,1FPC", 45)],
        &[("Marsha Hicks", 5), ("The Sarge", 5), ("Tod 'The Blaster' McQuinn", 2)],
    ),
    merc(
        "Cargo Rack",
        "cargoracks5c1_extended",
        "Extended (5E)",
        Some("5E Extended Cargo Rack"),
        2,
        MERC_EXTENDED,
        CARGO_ENGINEERS,
    ),
    merc(
        "Cargo Rack",
        "cargoracks6c1_extended",
        "Extended (6E)",
        Some("6E Extended Cargo Rack"),
        2,
        MERC_EXTENDED,
        CARGO_ENGINEERS,
    ),
    merc(
        "Detailed Surface Scanner",
        "detailedsurfacescanner_longrange",
        "Long range",
        Some("Long Range Detailed Surface Scanner"),
        2,
        &[("1MS,1Ge,1MCF", 5), ("1MS,1Ge,1PA", 5), ("1ME,1Nb,1PA,1PLA", 10), ("1ME,1MC,1Sn,1PRA", 20)],
        &[("Etienne Dorn", 5)],
    ),
    merc(
        "Enzyme Missile Rack",
        "enzymemissilerack_highyield",
        "High-yield",
        Some("2B High-Yield Enzyme Missile Rack"),
        2,
        &[("1MS,1V,1CCo", 10), ("1MS,1V,1EA", 15), ("1ME,1HDC,1Sn,1PCa", 20), ("1MC,1FPC,1EFW,1MSC", 45)],
        MINING_ENGINEERS,
    ),
    merc(
        "Fragment Cannon",
        "fragmentcannonsmall_doublescreaming",
        "Double screaming (1E)",
        Some("1E Double Screaming Fragment Cannon"),
        2,
        MERC_SCREAMING,
        FRAGMENT_ENGINEERS,
    ),
    merc(
        "Fragment Cannon",
        "fragmentcannonlarge_doublescreaming",
        "Double screaming (3C)",
        Some("3C Double Screaming Fragment Cannon"),
        2,
        MERC_SCREAMING,
        FRAGMENT_ENGINEERS,
    ),
    merc(
        "Fuel Scoop",
        "fuelscoop_efficiency",
        "Scoop rate enhanced",
        None,
        1,
        &[("1P,1GR", 10), ("1Cr,1GR,1CP", 15), ("1Cr,1CP,1CCe", 20), ("2CP,1CHD,1PLA", 25), ("1Te,1CM,1PHR,1PRA", 30)],
        &[("Marsha Hicks", 5), ("Lori Jameson", 4), ("Bill Turner", 3)],
    ),
    merc(
        "Mining Laser",
        "mininglaser_longrange",
        "Long range",
        Some("1D Long Range Mining Laser"),
        2,
        &[("1S,1HDP,1MCF", 5), ("1Cr,1MCF,1HE", 5), ("1MCF,1CPo,1Se,1HV", 10), ("1CIF,1ThA,1BiC,1PHR", 25)],
        MINING_ENGINEERS,
    ),
    merc(
        "Missile Rack",
        "seekermissilerack_drag",
        "Drag",
        Some("2B Drag Seeker Missile Rack"),
        2,
        &[("1MS,1V,1HDP", 10), ("1MS,1ME,1PAll", 10), ("1MCF,1MC,1HDC,1ThA", 20), ("1MC,1FPC,1Tc,1MSC", 40)],
        SEEKER_ENGINEERS,
    ),
    merc(
        "Missile Rack",
        "seekermissileracklarge_exposingmissiles",
        "Exposing",
        Some("3A Exposing Missiles"),
        2,
        &[("1V,1HDP,1MS", 10), ("1V,1SLF,1PAll", 15), ("1MCF,1PAll,1ThA", 25), ("1Tc,1CCom,1GR,1ThA", 45)],
        SEEKER_ENGINEERS,
    ),
    merc(
        "Missile Rack",
        "seekermissilerack_lightweightthermal",
        "Lightweight thermal",
        Some("2B Lightweight Thermal Seeker Missile Rack"),
        2,
        &[("1MS,1V,1Mn", 10), ("1MS,1ME,1CCe", 10), ("1MCF,1PA,1PLA,1ThA", 20), ("1MC,1PLA,1PRA,1MSC", 40)],
        SEEKER_ENGINEERS,
    ),
    merc(
        "Missile Rack",
        "seekermissilerackmedium_lockdown",
        "Lockdown (2B)",
        Some("2B Lockdown Seeker Missile Rack"),
        2,
        MERC_LOCKDOWN,
        SEEKER_ENGINEERS,
    ),
    merc(
        "Missile Rack",
        "seekermissileracklarge_lockdown",
        "Lockdown (3A)",
        Some("3A Lockdown Seeker Missile Rack"),
        2,
        MERC_LOCKDOWN,
        SEEKER_ENGINEERS,
    ),
    merc(
        "Module Reinforcement Package",
        "modulereinforcement_heavyduty",
        "Heavy duty",
        Some("5D Heavy Duty Module Reinforcement Package"),
        2,
        &[("1C,1Fe,1SHE", 5), ("1Fe,1SHE,1HDC", 5), ("1SHE,1HDC,1FPC,1W", 10), ("1CoS,1Ru,1PHR,1FCC", 25)],
        &[("Selene Jean", 5), ("Petra Olmanova", 5)],
    ),
    merc(
        "Multi-cannon",
        "multicannon_rapid",
        "Rapid phase",
        Some("2E Rapid Phase Multi-cannon"),
        2,
        &[("1MS,1HDP,1CCo", 10), ("1SLF,1ME,1PAll", 15), ("1MCF,1MC,1CCe,1ThA", 20), ("1PAll,1CCom,1CCe,1Tc", 45)],
        &[("Tod 'The Blaster' McQuinn", 5), ("Marsha Hicks", 5), ("Zacariah Nemo", 3)],
    ),
    merc(
        "Power Distributor",
        "powerdistributor_balanced",
        "Balanced",
        Some("5A Balanced Power Distributor"),
        2,
        &[("1S,1SLF,1CP", 5), ("1SLF,1CP,1HDC", 10), ("1CP,1HDC,1Se,1CM", 15), ("1FPC,1CM,1MSC,1EFC", 25)],
        DISTRIBUTOR_ENGINEERS,
    ),
    merc(
        "Power Distributor",
        "powerdistributors3c2_supportfocused",
        "Support focused (3D)",
        Some("3D Support Focused Power Distributor"),
        2,
        &[("1S,1SLF,1CCo", 5), ("1ABSD,1Cr,1EA", 5), ("1USA,1MCF,1Se,1PCa", 10), ("1CSD,1Cd,1FPC,1MSC", 15)],
        DISTRIBUTOR_ENGINEERS,
    ),
    merc(
        "Power Distributor",
        "powerdistributors3c5_supportfocused",
        "Support focused (3A)",
        Some("3A Support Focused Power Distributor"),
        2,
        &[("1S,1SLF,1CCo", 5), ("1ABSD,1Cr,1EA", 5), ("1USA,1MCF,1Se,1PCa", 10), ("1CSD,1Cd,1FPC,1MSC", 20)],
        DISTRIBUTOR_ENGINEERS,
    ),
    merc(
        "Power Distributor",
        "powerdistributors4c2_supportfocused",
        "Support focused (4D)",
        Some("4D Support Focused Power Distributor"),
        2,
        &[("1S,1SLF,1CCo", 5), ("1ABSD,1Cr,1EA", 5), ("1USA,1MCF,1Se,1PCa", 10), ("1CSD,1Cd,1FPC,1MSC", 20)],
        DISTRIBUTOR_ENGINEERS,
    ),
    merc(
        "Power Distributor",
        "powerdistributors4c5_supportfocused",
        "Support focused (4A)",
        Some("4A Support Focused Power Distributor"),
        2,
        &[("1S,1SLF,1CCo", 5), ("1ABSD,1Cr,1EA", 5), ("1USA,1MCF,1Se,1PCa", 10), ("1CSD,1Cd,1FPC,1MSC", 25)],
        DISTRIBUTOR_ENGINEERS,
    ),
    merc(
        "Power Distributor",
        "powerdistributors6c5_supportfocused",
        "Support focused (6A)",
        Some("6A Support Focused Power Distributor"),
        2,
        &[("1S,1SLF,1CCo", 5), ("1ABSD,1Cr,1EA", 10), ("1USA,1MCF,1Se,1PCa", 15), ("1CSD,1Cd,1FPC,1MSC", 30)],
        DISTRIBUTOR_ENGINEERS,
    ),
    merc("Pulse Laser", "pulselaser_thermalplasmaconversion", "Plasma conversion", None, 1, MERC_PLASMA, LASER_ENGINEERS),
    merc(
        "Rail Gun",
        "railgun_longshot",
        "Enduring feedback",
        Some("2B Enduring Feedback Rail Gun"),
        2,
        &[("1MS,1S,1MCF", 10), ("1S,1MCF,1Nb", 15), ("1MCF,1Nb,1CPo,1Sn", 25), ("1MC,1ThA,1MSC,1BiC", 50)],
        &[("Tod 'The Blaster' McQuinn", 5), ("Etienne Dorn", 5), ("The Sarge", 3)],
    ),
];

/// EDD writes a couple of system names in capitals ("DERISO") → "Deriso".
fn tidy_system(s: &str) -> String {
    if s.len() > 3 && s.chars().all(|c| !c.is_ascii_lowercase()) && s.chars().any(|c| c.is_ascii_uppercase()) {
        let mut chars = s.chars();
        chars.next().map(|h| h.to_string() + &chars.as_str().to_ascii_lowercase()).unwrap_or_default()
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------------------------

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("data")
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

fn ing_json(list: &[(String, u32)]) -> Value {
    Value::Array(list.iter().map(|(s, n)| json!([s, n])).collect())
}

fn sorted(mut v: Vec<(String, u32)>) -> Vec<(String, u32)> {
    v.sort();
    v
}

fn main() {
    let data = data_dir();
    let edd_dir = data.join("engineering").join("eddiscovery-src");
    let mut rep = Report::default();

    // --- EDD sources -------------------------------------------------------------------------
    let codes = load_codes(&read(&edd_dir.join("MCMRType.cs")), &mut rep);
    let recipe_toks = tokenize(&read(&edd_dir.join("RecipesEngineering.cs")));
    let edd_recipes = calls(&recipe_toks, "EngineeringRecipe", true);

    // --- FDevIDs engineers ---------------------------------------------------------------------
    let mut fdev: Vec<FdevEngineer> = Vec::new();
    let eng_csv = read(&data.join("fdevids").join("engineers.csv"));
    for line in eng_csv.lines().skip(1) {
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        if f.len() >= 4 {
            fdev.push(FdevEngineer {
                id: f[0].parse().unwrap_or(0),
                system_address: f[1].parse().unwrap_or(0),
                market_id: f[2].parse().unwrap_or(0),
                name: f[3].to_string(),
            });
        }
    }

    // --- EDEngineer --------------------------------------------------------------------------
    let ed: Vec<Value> = serde_json::from_str(
        read(&data.join("engineering").join("edengineer").join("blueprints.json")).trim_start_matches('\u{feff}'),
    )
    .expect("blueprints.json");
    let eng_list = |r: &Value| -> Vec<String> {
        r["Engineers"]
            .as_array()
            .map(|a| a.iter().filter_map(|e| e.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    let effects_of = |r: &Value| -> Value {
        Value::Array(
            r["Effects"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|e| {
                            json!([
                                e["Property"].as_str().unwrap_or(""),
                                e["Effect"].as_str().unwrap_or(""),
                                e["IsGood"].as_bool().unwrap_or(true)
                            ])
                        })
                        .collect()
                })
                .unwrap_or_default(),
        )
    };

    // ===== Ship blueprints ========================================================================
    struct Bp {
        module: &'static str,
        name: String,
        symbol: &'static str,
        grades: BTreeMap<u8, Vec<(String, u32)>>,
        effects: BTreeMap<u8, Value>,
        engineers: BTreeMap<String, u8>,
        /// Merc Coins per roll from grade 1 (empty for ordinary blueprints).
        merc_coins: Vec<u32>,
        merc_module: Option<&'static str>,
    }
    let mut bps: Vec<Bp> = Vec::new();
    for r in &ed {
        let t = r["Type"].as_str().unwrap_or("");
        let engs = eng_list(r);
        let Some(grade) = r["Grade"].as_u64().map(|g| g as u8) else { continue };
        if engs.iter().any(|e| e.starts_with('@')) || matches!(t, "Unlock" | "Guardian" | "Human" | "Weapon" | "Suit") {
            continue;
        }
        let Some(module) = edengineer_type(t) else {
            rep.add("EDEngineer types skipped", format!("{t} / {}", r["Name"]));
            continue;
        };
        let name = r["Name"].as_str().unwrap_or("").to_string();
        let Some(symbol) = blueprint_symbol(module, &name) else {
            rep.add("Blueprints without journal symbol (skipped)", format!("{module} / {name}"));
            continue;
        };
        let ctx = format!("{module}/{name} G{grade}");
        let ings: Vec<(String, u32)> = r["Ingredients"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|i| {
                        let n = i["Name"].as_str()?;
                        let sym = resolve_name(n, &codes, Pref::Material, &ctx, &mut rep)?;
                        Some((sym, i["Size"].as_u64().unwrap_or(1) as u32))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let idx = match bps.iter().position(|b| b.module == module && b.name == name) {
            Some(i) => i,
            None => {
                bps.push(Bp {
                    module,
                    name: name.clone(),
                    symbol,
                    grades: BTreeMap::new(),
                    effects: BTreeMap::new(),
                    engineers: BTreeMap::new(),
                    merc_coins: Vec::new(),
                    merc_module: None,
                });
                bps.len() - 1
            }
        };
        let bp = &mut bps[idx];
        if bp.grades.insert(grade, ings).is_some() {
            rep.add("Duplicate blueprint grade rows", ctx.clone());
        }
        bp.effects.insert(grade, effects_of(r));
        for e in engs {
            let Some(e) = canon_engineer(&e, &fdev) else {
                rep.add("Unknown engineer names", format!("{ctx}: {e}"));
                continue;
            };
            let g = bp.engineers.entry(e).or_insert(0);
            *g = (*g).max(grade);
        }
    }
    // Corrections from engineering.md §6.3 / A5 (wiki): Juri Ishmaak offers the three scanners up to
    // G3 (missing in EDEngineer); Lori Jameson's Life Support tops out at G4 (EDEngineer says G5).
    for bp in &mut bps {
        if matches!(bp.module, "Kill Warrant Scanner" | "Manifest Scanner" | "Frame Shift Wake Scanner") {
            let max = bp.grades.keys().max().copied().unwrap_or(1).min(3);
            if bp.engineers.insert("Juri Ishmaak".into(), max).is_none() {
                rep.add("Corrections applied", format!("{} {}: + Juri Ishmaak G{max}", bp.module, bp.symbol));
            }
        }
        if bp.module == "Life Support"
            && let Some(g) = bp.engineers.get_mut("Lori Jameson")
            && *g > 4
        {
            rep.add("Corrections applied", format!("{} {}: Lori Jameson G{g} -> G4", bp.module, bp.symbol));
            *g = 4;
        }
    }
    for bp in &bps {
        let grades: Vec<u8> = bp.grades.keys().copied().collect();
        let expect: Vec<u8> = (1..=grades.len() as u8).collect();
        if grades != expect {
            rep.add("Blueprints with non-contiguous grades", format!("{} {}: {grades:?}", bp.module, bp.name));
        }
    }
    // Cross-check with EDD's blueprint rows (journal symbols and recipes); add blueprints only EDD has.
    {
        let mut edd_bp: BTreeMap<(&'static str, String, u8), EddGrade> = BTreeMap::new();
        for args in &edd_recipes {
            if args.len() < 6 {
                continue;
            }
            let (Some(name), Some(fd), Some(ings), Some(mt), Some(lvl), Some(engs)) = (
                a_str(&args[0]),
                a_str(&args[1]),
                a_str(&args[2]),
                a_ident(&args[3]),
                a_num(&args[4]),
                a_str(&args[5]),
            ) else {
                continue;
            };
            for m in edd_types(mt, fd) {
                let ctx = format!("EDD {m}/{name} G{lvl}");
                let set: BTreeSet<String> =
                    engs.split(',').filter(|s| !s.is_empty()).filter_map(|e| canon_engineer(e, &fdev)).collect();
                edd_bp.insert(
                    (m, fd.to_ascii_lowercase(), lvl as u8),
                    (sorted(parse_codes(ings, &codes, &ctx, &mut rep)), set),
                );
            }
        }
        let mut matched = BTreeSet::new();
        for bp in &bps {
            for (g, ings) in &bp.grades {
                let key = (bp.module, bp.symbol.to_ascii_lowercase(), *g);
                match edd_bp.get(&key) {
                    None => rep.add("Blueprint grades not in EDD", format!("{} {} G{g}", bp.module, bp.symbol)),
                    Some((edd_ings, edd_engs)) => {
                        matched.insert(key.clone());
                        if sorted(ings.clone()) != *edd_ings {
                            rep.add(
                                "Recipe differences EDEngineer vs EDD (EDEngineer kept)",
                                format!("{} {} G{g}: {ings:?} vs {edd_ings:?}", bp.module, bp.symbol),
                            );
                        }
                        let ours: BTreeSet<String> =
                            bp.engineers.iter().filter(|(_, mg)| **mg >= *g).map(|(e, _)| e.clone()).collect();
                        if ours != *edd_engs {
                            let only_ours: Vec<_> = ours.difference(edd_engs).collect();
                            let only_edd: Vec<_> = edd_engs.difference(&ours).collect();
                            rep.add(
                                "Engineer differences vs EDD (ours kept)",
                                format!("{} {} G{g}: +ours {only_ours:?} +edd {only_edd:?}", bp.module, bp.symbol),
                            );
                        }
                    }
                }
            }
        }
        // EDD-only (module, blueprint) pairs, e.g. Pulse Laser Short Range Blaster (offered in game,
        // missing from EDEngineer). Rows without engineers (EDD's made-up DSS "sensor_fastscan") are
        // ignored, as are symbols whose journal capitalisation is unknown.
        let mut extra: BTreeMap<(&'static str, String), BTreeMap<u8, EddGrade>> = BTreeMap::new();
        for (key, val) in edd_bp.iter().filter(|(k, _)| !matched.contains(*k)) {
            extra.entry((key.0, key.1.clone())).or_default().insert(key.2, val.clone());
        }
        for ((module, fd), grades) in extra {
            let template = bps.iter().find(|b| b.symbol.eq_ignore_ascii_case(&fd));
            let contiguous = grades.keys().copied().eq(1..=grades.len() as u8);
            match template {
                Some(t) if contiguous && grades.values().all(|(_, e)| !e.is_empty()) => {
                    let mut engineers: BTreeMap<String, u8> = BTreeMap::new();
                    for (g, (_, engs)) in &grades {
                        for e in engs {
                            let m = engineers.entry(e.clone()).or_insert(0);
                            *m = (*m).max(*g);
                        }
                    }
                    rep.add(
                        "Blueprints added from EDD (missing in EDEngineer)",
                        format!("{module} {} ({} grades)", t.symbol, grades.len()),
                    );
                    let (symbol, name) = (t.symbol, t.name.clone());
                    bps.push(Bp {
                        module,
                        name,
                        symbol,
                        grades: grades.iter().map(|(g, (i, _))| (*g, i.clone())).collect(),
                        effects: BTreeMap::new(),
                        engineers,
                        merc_coins: Vec::new(),
                        merc_module: None,
                    });
                }
                _ => rep.add(
                    "EDD blueprint rows not in EDEngineer (ignored)",
                    format!("{module} {fd} G{:?}", grades.keys().collect::<Vec<_>>()),
                ),
            }
        }
    }
    // Merc blueprints; grades below the first one stay as empty recipes, so index 0 is still grade 1.
    for m in MERC {
        let mut grades: BTreeMap<u8, Vec<(String, u32)>> = (1..m.first_grade).map(|g| (g, Vec::new())).collect();
        let mut merc_coins = vec![0; usize::from(m.first_grade - 1)];
        for (g, (ings, coins)) in (m.first_grade..).zip(m.grades) {
            grades.insert(g, parse_codes(ings, &codes, &format!("Merc {} G{g}", m.symbol), &mut rep));
            merc_coins.push(*coins);
        }
        let mut engineers = BTreeMap::new();
        for (e, g) in m.engineers {
            match canon_engineer(e, &fdev) {
                Some(e) => {
                    engineers.insert(e, *g);
                }
                None => rep.add("Unknown engineer names", format!("Merc {}: {e}", m.symbol)),
            }
        }
        bps.push(Bp {
            module: m.module,
            name: m.name.to_string(),
            symbol: m.symbol,
            grades,
            effects: BTreeMap::new(),
            engineers,
            merc_coins,
            merc_module: m.merc_module,
        });
    }
    bps.sort_by(|a, b| (a.module, &a.symbol).cmp(&(b.module, &b.symbol)));

    // ===== Experimentals ==========================================================================
    struct EddSpecial {
        name: String,
        fdname: String,
        modules: Vec<&'static str>,
        cost: Vec<(String, u32)>,
    }
    let mut specials: Vec<EddSpecial> = Vec::new();
    for args in &edd_recipes {
        if args.len() != 4 {
            continue;
        }
        let (Some(name), Some(fd), Some(ings)) = (a_str(&args[0]), a_str(&args[1]), a_str(&args[3])) else { continue };
        if !fd.starts_with("special_") {
            continue;
        }
        let modules: Vec<&'static str> = match (a_ident(&args[2]), a_str(&args[2])) {
            (Some(i), _) => edd_types(i, fd),
            (_, Some(list)) => list.split(',').flat_map(|t| edd_types(t, fd)).collect(),
            _ => Vec::new(),
        };
        let cost = parse_codes(ings, &codes, &format!("EDD special {name}"), &mut rep);
        specials.push(EddSpecial { name: name.to_string(), fdname: fd.to_string(), modules, cost });
    }
    let norm = |s: &str| -> String {
        let k = name_key(s.split('(').next().unwrap_or(s));
        match k.as_str() {
            "iondisruptor" => "iondisruption".into(),
            _ => k.trim_end_matches('s').to_string(),
        }
    };
    struct Exp {
        symbol: String,
        aliases: Vec<String>,
        name: String,
        modules: Vec<&'static str>,
        cost: Vec<(String, u32)>,
        effects: Value,
    }
    let mut exps: Vec<Exp> = Vec::new();
    for r in &ed {
        let t = r["Type"].as_str().unwrap_or("");
        if !r["Grade"].is_null() || matches!(t, "Unlock" | "Guardian" | "Human" | "Weapon" | "Suit") {
            continue;
        }
        let engs = eng_list(r);
        let raw_name = r["Name"].as_str().unwrap_or("");
        if engs.iter().any(|e| e.starts_with('@')) {
            rep.add("EDEngineer special rows skipped", format!("{t} / {raw_name} {engs:?}"));
            continue;
        }
        let Some(module) = edengineer_type(t) else { continue };
        let applicable: Vec<&'static str> = if module == "Missile Rack" {
            if raw_name.contains("Seeker only") {
                vec!["Seeker Missile Rack"]
            } else if raw_name.contains("Dumbfire only") {
                vec!["Missile Rack"]
            } else {
                vec!["Missile Rack", "Seeker Missile Rack"]
            }
        } else {
            vec![module]
        };
        let base_name = raw_name.split(" (").next().unwrap_or(raw_name).trim().to_string();
        let mut cands: Vec<&EddSpecial> = specials
            .iter()
            .filter(|s| norm(&s.name) == norm(&base_name) && s.modules.iter().any(|m| applicable.contains(m)))
            .collect();
        if cands.is_empty() {
            rep.add("Experimentals without EDD symbol (skipped)", format!("{module} / {raw_name}"));
            continue;
        }
        let ctx = format!("{module}/{base_name}");
        let cost: Vec<(String, u32)> = r["Ingredients"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|i| {
                        Some((
                            resolve_name(i["Name"].as_str()?, &codes, Pref::Material, &ctx, &mut rep)?,
                            i["Size"].as_u64().unwrap_or(1) as u32,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let primary = cands.remove(0);
        let aliases: Vec<String> = cands.iter().map(|c| c.fdname.clone()).collect();
        if sorted(cost.clone()) != sorted(primary.cost.clone()) {
            rep.add(
                "Experimental cost differences EDEngineer vs EDD (EDEngineer kept)",
                format!("{} ({module}): {cost:?} vs {:?}", primary.fdname, primary.cost),
            );
        }
        match exps.iter_mut().find(|e| e.symbol == primary.fdname) {
            Some(e) => {
                for m in applicable {
                    if !e.modules.contains(&m) {
                        e.modules.push(m);
                    }
                }
                if sorted(e.cost.clone()) != sorted(cost.clone()) {
                    // Dazzle Shell: EDEngineer's Fragment Cannon row lacks the Mechanical Components
                    // its Plasma Accelerator row has (EDD's copy lists Mechanical Scrap twice) — keep
                    // the more complete recipe.
                    rep.add(
                        "Experimental cost differs between module types (fuller kept)",
                        format!("{} {module}: {:?} vs {cost:?}", e.symbol, e.cost),
                    );
                    if cost.len() > e.cost.len() {
                        e.cost = cost.clone();
                    }
                }
                for a in aliases {
                    if !e.aliases.contains(&a) {
                        e.aliases.push(a);
                    }
                }
            }
            None => exps.push(Exp {
                symbol: primary.fdname.clone(),
                aliases,
                name: base_name,
                modules: applicable,
                cost,
                effects: effects_of(r),
            }),
        }
    }
    for s in &specials {
        if !exps.iter().any(|e| e.symbol == s.fdname || e.aliases.contains(&s.fdname)) {
            rep.add(
                "EDD experimentals not matched to EDEngineer (ignored)",
                format!("{} {} {:?}", s.fdname, s.name, s.modules),
            );
        }
    }
    for e in &mut exps {
        e.modules.sort_unstable();
    }
    exps.sort_by(|a, b| (&a.name, &a.symbol).cmp(&(&b.name, &b.symbol)));

    // ===== Engineers =============================================================================
    struct EngInfo {
        name: String,
        system: String,
        base: String,
        pos: [f64; 3],
        body: String,
        discovery: String,
        meeting: String,
        unlock: String,
        reputation: String,
        permit: bool,
        odyssey: bool,
    }
    let mut infos: Vec<EngInfo> = Vec::new();
    for args in calls(&tokenize(&read(&edd_dir.join("Engineers.cs"))), "EngineeringInfo", true) {
        if args.len() < 11 {
            continue;
        }
        let s = |i: usize| a_str(&args[i]).unwrap_or("").to_string();
        let n = |i: usize| a_num(&args[i]).unwrap_or(0.0);
        infos.push(EngInfo {
            name: s(0),
            system: s(1),
            base: s(2),
            pos: [n(3), n(4), n(5)],
            body: s(6),
            discovery: s(7),
            meeting: s(8),
            unlock: s(9),
            reputation: s(10),
            permit: args.get(11).and_then(a_bool).unwrap_or(false),
            odyssey: args.get(12).and_then(a_bool).unwrap_or(false),
        });
    }
    // Ship-engineer unlock costs from EDEngineer ("Unlock" rows).
    let mut unlock_costs: HashMap<String, Vec<(String, u32)>> = HashMap::new();
    for r in ed.iter().filter(|r| r["Type"] == "Unlock") {
        let Some(name) = r["Name"].as_str().and_then(|n| canon_engineer(n, &fdev)) else { continue };
        let ctx = format!("unlock {name}");
        let list: Vec<(String, u32)> = r["Ingredients"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|i| {
                        Some((
                            resolve_name(i["Name"].as_str()?, &codes, Pref::Commodity, &ctx, &mut rep)?,
                            i["Size"].as_u64().unwrap_or(1) as u32,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        unlock_costs.insert(name, list);
    }
    // Ship engineer specialities from the final blueprint data. Merc blueprints list their own
    // engineers and leave the module grades alone (Juri Ishmaak takes Merc seekers to G5, missile racks to G3).
    let mut specialities: BTreeMap<String, BTreeMap<&'static str, u8>> = BTreeMap::new();
    for bp in bps.iter().filter(|b| b.merc_coins.is_empty()) {
        for (e, g) in &bp.engineers {
            let m = specialities.entry(e.clone()).or_default().entry(bp.module).or_insert(0);
            *m = (*m).max(*g);
        }
    }

    // On-foot mods (needed for the engineers' lists too).
    struct OMod {
        kind: &'static str,
        symbol: String,
        name: String,
        variant: Option<String>,
        credits: u64,
        cost: Vec<(String, u32)>,
        engineers: Vec<String>,
    }
    let mut omods: Vec<OMod> = Vec::new();
    let mut oups: Vec<OUpgrade> = Vec::new();
    for args in &edd_recipes {
        let kind = match args.first().and_then(a_ident) {
            Some("ItemData.ShipModule.ModuleTypes.Suit") => "Suit",
            Some("ItemData.ShipModule.ModuleTypes.Weapon") => "Weapon",
            _ => continue,
        };
        if args.len() == 4 {
            let (Some(family), Some(lvl), Some(ings)) = (a_str(&args[1]), a_num(&args[2]), a_str(&args[3])) else {
                continue;
            };
            let cost = parse_codes(ings, &codes, &format!("{kind} upgrade {family} G{lvl}"), &mut rep);
            oups.push((kind, family.to_string(), lvl as u8, cost));
        } else if args.len() == 7 {
            let (Some(fd), Some(manu), Some(name), Some(credits), Some(ings), Some(engs)) =
                (a_str(&args[1]), a_str(&args[2]), a_str(&args[3]), a_num(&args[4]), a_str(&args[5]), a_str(&args[6]))
            else {
                continue;
            };
            let cost = parse_codes(ings, &codes, &format!("{kind} mod {fd} {manu}"), &mut rep);
            let engineers: Vec<String> = engs
                .split(',')
                .filter_map(|e| {
                    let c = canon_engineer(e.trim(), &fdev);
                    if c.is_none() {
                        rep.add("Unknown engineer names", format!("mod {fd}: {e}"));
                    }
                    c
                })
                .collect();
            omods.push(OMod {
                kind,
                symbol: fd.to_string(),
                name: mod_name(fd, name),
                variant: (manu != "All").then(|| manu.to_string()),
                credits: credits as u64,
                cost,
                engineers,
            });
        }
    }

    let colonia = [-9530.5, -910.28125, 19808.125];
    let mut engineers_json: Vec<Value> = Vec::new();
    let mut referred_by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for info in &infos {
        let Some(name) = canon_engineer(&info.name, &fdev) else {
            rep.add("Unknown engineer names", format!("Engineers.cs: {}", info.name));
            continue;
        };
        let mut by: Vec<String> = Vec::new();
        if info.odyssey {
            if let Some(disc) = ON_FOOT.iter().find(|o| o.name == name).map(|o| o.discovery) {
                for f in &fdev {
                    let short = f.name.as_str();
                    if disc.contains(&format!("{short}'s"))
                        || (disc.contains("requirements of") && disc.contains(short))
                    {
                        by.push(f.name.clone());
                    }
                }
            }
        } else if let Some(rest) = info.discovery.strip_prefix("From ") {
            let who = rest.split(" (").next().unwrap_or(rest).trim_end_matches('.');
            match canon_engineer(who, &fdev) {
                Some(w) => by.push(w),
                None => rep.add("Unknown engineer names", format!("discovery of {name}: {who}")),
            }
        }
        referred_by.insert(name, by);
    }
    let mut refers_to: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (e, by) in &referred_by {
        for b in by {
            refers_to.entry(b.clone()).or_default().push(e.clone());
        }
    }
    let mut ordered: Vec<&EngInfo> = infos.iter().collect();
    ordered.sort_by_key(|i| {
        (
            i.odyssey,
            canon_engineer(&i.name, &fdev).and_then(|n| fdev.iter().find(|f| f.name == n).map(|f| f.id)).unwrap_or(0),
        )
    });
    for info in ordered {
        let Some(name) = canon_engineer(&info.name, &fdev) else { continue };
        let f = fdev.iter().find(|f| f.name == name);
        let d2: f64 = info.pos.iter().zip(colonia).map(|(a, b)| (a - b) * (a - b)).sum();
        let region = if d2.sqrt() < 1000.0 { "Colonia" } else { "Bubble" };
        let to = refers_to.get(&name).cloned().unwrap_or_default();
        let by = referred_by.get(&name).cloned().unwrap_or_default();
        let (discovery, meeting, unlock, referral, referral_cost) = if info.odyssey {
            match ON_FOOT.iter().find(|o| o.name == name) {
                Some(o) => {
                    let rc: Vec<(String, u32)> = o
                        .referral_cost
                        .iter()
                        .filter_map(|(n, c)| {
                            Some((resolve_name(n, &codes, Pref::Micro, &format!("referral {name}"), &mut rep)?, *c))
                        })
                        .collect();
                    (o.discovery.to_string(), o.meeting.to_string(), String::new(), o.referral.to_string(), rc)
                }
                None => {
                    rep.add("On-foot engineers without overrides", name.clone());
                    (info.discovery.clone(), info.meeting.clone(), String::new(), info.unlock.clone(), Vec::new())
                }
            }
        } else {
            let referral = if to.is_empty() {
                String::new()
            } else {
                format!("Reach grade 3 plus a third of grade 4 to learn about {}.", to.join(" and "))
            };
            (info.discovery.clone(), info.meeting.clone(), info.unlock.clone(), referral, Vec::new())
        };
        let modules: Vec<Value> = {
            let mut v: Vec<(&&str, &u8)> = specialities.get(&name).map(|m| m.iter().collect()).unwrap_or_default();
            v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            v.into_iter().map(|(m, g)| json!([m, g])).collect()
        };
        let mods: Vec<String> = {
            let mut v: Vec<String> = Vec::new();
            for m in omods.iter().filter(|m| m.engineers.contains(&name)) {
                if !v.contains(&m.symbol) {
                    v.push(m.symbol.clone());
                }
            }
            v
        };
        if !info.odyssey && modules.is_empty() {
            rep.add("Ship engineers without blueprints", name.clone());
        }
        engineers_json.push(json!({
            "id": f.map_or(0, |f| f.id),
            "name": name,
            "kind": if info.odyssey { "OnFoot" } else { "Ship" },
            "region": region,
            "system": tidy_system(&info.system),
            "system_address": f.map_or(0, |f| f.system_address),
            "market_id": f.map_or(0, |f| f.market_id),
            "base": info.base,
            "body": info.body,
            "pos": info.pos,
            "permit": info.permit,
            "discovery": discovery,
            "meeting": meeting,
            "unlock": unlock,
            "unlock_cost": ing_json(unlock_costs.get(&name).map(Vec::as_slice).unwrap_or_default()),
            "reputation": info.reputation,
            "referral": referral,
            "referral_cost": ing_json(&referral_cost),
            "referred_by": by,
            "refers_to": to,
            "modules": modules,
            "mods": mods,
        }));
    }
    for f in &fdev {
        if !engineers_json.iter().any(|e| e["id"] == f.id) {
            rep.add("FDevIDs engineers without EDD info", f.name.clone());
        }
    }

    // ===== Synthesis =============================================================================
    let synth_toks = tokenize(&read(&edd_dir.join("RecipesSynthesis.cs")));
    let mut synth_effects: HashMap<(String, u8), String> = HashMap::new();
    for r in ed.iter().filter(|r| eng_list(r).iter().any(|e| e == "@Synthesis")) {
        if let (Some(t), Some(g)) = (r["Type"].as_str(), r["Grade"].as_u64()) {
            let eff = r["Name"].as_str().unwrap_or("");
            synth_effects.insert((t.to_string(), g as u8), if eff == "None" { String::new() } else { eff.to_string() });
        }
    }
    let mut synth_json: Vec<Value> = Vec::new();
    for args in calls(&synth_toks, "SynthesisRecipe", true) {
        if args.len() != 4 {
            continue;
        }
        let (Some(family), Some(level), Some(ings)) = (a_str(&args[0]), a_ident(&args[2]), a_str(&args[3])) else {
            continue;
        };
        let grade = match level.rsplit('.').next() {
            Some("Basic") => 1,
            Some("Standard") => 2,
            Some("Premium") => 3,
            _ => continue,
        };
        let family = match family {
            "FSD" => "FSD Injection",
            f => f,
        };
        let ed_family = match family {
            "Chaff" => "Chaff Launcher",
            "Heat Sinks" => "Heat Sink Launcher",
            "SRV Ammo" => "SRV Ammo Restock",
            f => f,
        };
        let level_name = ["", "Basic", "Standard", "Premium"][grade as usize];
        let name = a_str(&args[1]).map(str::to_string).unwrap_or_else(|| format!("{family} {level_name}"));
        let cost = parse_codes(ings, &codes, &format!("synthesis {name}"), &mut rep);
        let effect = synth_effects.get(&(ed_family.to_string(), grade)).cloned().unwrap_or_default();
        synth_json
            .push(json!({ "family": family, "name": name, "grade": grade, "effect": effect, "cost": ing_json(&cost) }));
    }
    synth_json
        .sort_by(|a, b| (a["family"].as_str(), a["grade"].as_u64()).cmp(&(b["family"].as_str(), b["grade"].as_u64())));

    // ===== Technology broker =====================================================================
    let mut tb_json: Vec<Value> = Vec::new();
    for args in &edd_recipes {
        if args.len() != 5 {
            continue;
        }
        let (Some(name), Some(fd), Some(kind), Some(_mt), Some(ings)) =
            (a_str(&args[0]), a_str(&args[1]), a_str(&args[2]), a_ident(&args[3]), a_str(&args[4]))
        else {
            continue;
        };
        let broker = match kind {
            "Human" => "Human",
            "Guardian" | "Guardian Weapons" => "Guardian",
            _ => continue,
        };
        let module =
            (fd.starts_with("int_") || fd.starts_with("hpt_") || fd.starts_with("gdn_")).then(|| fd.to_string());
        if let Some(m) = &module
            && nexus_data::outfitting::module(m).is_none()
            && !m.starts_with("gdn_")
        {
            rep.add("Tech-broker modules not in FDevIDs", m.clone());
        }
        let cost = parse_codes(ings, &codes, &format!("tech broker {name}"), &mut rep);
        tb_json.push(json!({ "broker": broker, "name": name, "module": module, "cost": ing_json(&cost) }));
    }

    // ===== Output ================================================================================
    let bp_json: Vec<Value> = bps
        .iter()
        .map(|b| {
            let mut engs: Vec<(&String, &u8)> = b.engineers.iter().collect();
            engs.sort_by(|x, y| y.1.cmp(x.1).then(x.0.cmp(y.0)));
            let mut v = json!({
                "module": b.module,
                "symbol": b.symbol,
                "name": b.name,
                "grades": b.grades.values().map(|g| ing_json(g)).collect::<Vec<_>>(),
                "effects": b.effects.values().cloned().collect::<Vec<_>>(),
                "engineers": engs.into_iter().map(|(e, g)| json!([e, g])).collect::<Vec<_>>(),
            });
            if !b.merc_coins.is_empty() {
                v["merc_coins"] = json!(b.merc_coins);
                v["merc_module"] = json!(b.merc_module);
            }
            v
        })
        .collect();
    let exp_json: Vec<Value> = exps
        .iter()
        .map(|e| json!({ "symbol": e.symbol, "aliases": e.aliases, "name": e.name, "modules": e.modules, "cost": ing_json(&e.cost), "effects": e.effects }))
        .collect();
    let up_json: Vec<Value> =
        oups.iter().map(|(k, f, g, c)| json!({ "kind": k, "family": f, "grade": g, "cost": ing_json(c) })).collect();
    let mod_json: Vec<Value> = omods
        .iter()
        .map(|m| json!({ "kind": m.kind, "symbol": m.symbol, "name": m.name, "variant": m.variant, "credits": m.credits, "cost": ing_json(&m.cost), "engineers": m.engineers }))
        .collect();

    let sections: [(&str, &[Value]); 7] = [
        ("blueprints", &bp_json),
        ("experimentals", &exp_json),
        ("engineers", &engineers_json),
        ("synthesis", &synth_json),
        ("tech_broker", &tb_json),
        ("odyssey_upgrades", &up_json),
        ("odyssey_mods", &mod_json),
    ];
    let mut out = String::from("{\n\"generated_by\": \"cargo run -p nexus-data --example convert_engineering\",\n");
    out.push_str("\"sources\": \"EDEngineer (MIT, (c) 2016 Max); EDDiscovery EliteDangerousCore (Apache-2.0, (c) EDDiscovery development team); EDCD/FDevIDs engineers.csv. See SOURCE.txt.\"");
    for (name, items) in sections {
        let _ = write!(out, ",\n\"{name}\": [\n");
        let lines: Vec<String> = items.iter().map(|v| serde_json::to_string(v).expect("json")).collect();
        out.push_str(&lines.join(",\n"));
        out.push_str("\n]");
    }
    out.push_str("\n}\n");
    let out_path = data.join("engineering").join("nexus").join("engineering.json");
    fs::create_dir_all(out_path.parent().expect("parent")).expect("mkdir");
    fs::write(&out_path, &out).expect("write engineering.json");

    rep.print();
    println!(
        "\nWrote {} ({} bytes): {} blueprints ({} module types), {} experimentals, {} engineers, {} synthesis recipes, {} tech-broker unlocks, {} Odyssey upgrades, {} Odyssey mods",
        out_path.display(),
        out.len(),
        bp_json.len(),
        bps.iter().map(|b| b.module).collect::<BTreeSet<_>>().len(),
        exp_json.len(),
        engineers_json.len(),
        synth_json.len(),
        tb_json.len(),
        up_json.len(),
        mod_json.len()
    );
    println!("\nShip engineer specialities (for comparison with engineering.md §4.3):");
    for e in &engineers_json {
        if e["kind"] == "Ship" {
            let mods: Vec<String> = e["modules"]
                .as_array()
                .map(|a| a.iter().map(|m| format!("{} {}", m[0].as_str().unwrap_or(""), m[1])).collect())
                .unwrap_or_default();
            println!(
                "  {} [{}]: {}",
                e["name"].as_str().unwrap_or(""),
                e["region"].as_str().unwrap_or(""),
                mods.join(", ")
            );
        }
    }
}
