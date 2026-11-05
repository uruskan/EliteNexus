//! Ship modules (outfitting) keyed by the lower-case journal `Item` symbol, plus slot names.
//!
//! Source: EDCD/FDevIDs `outfitting.csv` (compiled in, parsed lazily). 2026 "MercGear" rows reuse
//! the symbol of the regular module they are based on, so [`module`] returns the regular module
//! and the MercGear variant is reachable through [`module_by_id`] / [`variants`].

use std::sync::LazyLock;

use rustc_hash::FxHashMap;

const OUTFITTING_CSV: &str = include_str!("../../../data/fdevids/outfitting.csv");

/// FDevIDs outfitting category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleCategory {
    /// Core internals and armour.
    Standard,
    /// Optional internals.
    Internal,
    Hardpoint,
    Utility,
    /// 2026 pre-engineered modules bought with MercCoins.
    MercGear,
}

/// Weapon mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mount {
    Fixed,
    Gimballed,
    Turreted,
}

impl Mount {
    pub fn label(self) -> &'static str {
        match self {
            Mount::Fixed => "Fixed",
            Mount::Gimballed => "Gimballed",
            Mount::Turreted => "Turreted",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fixed" => Some(Mount::Fixed),
            "gimballed" | "gimbal" | "gimbaled" => Some(Mount::Gimballed),
            "turreted" | "turret" => Some(Mount::Turreted),
            _ => None,
        }
    }
}

/// Missile guidance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Guidance {
    Dumbfire,
    Seeker,
}

/// One outfitting module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// FDevIDs / Outfitting.json id.
    pub id: u64,
    /// Lower-case journal `Item` symbol, e.g. `"int_hyperdrive_overcharge_size5_class5"`.
    pub symbol: String,
    /// Base display name, e.g. `"Frame Shift Drive (SCO)"`, `"Multi-cannon"`, `"Lightweight Alloy"`.
    pub name: String,
    /// The name as FDevIDs spells it (`"Multi-Cannon"`), which is what Spansh and other tools use.
    pub fdev_name: String,
    pub category: ModuleCategory,
    pub mount: Option<Mount>,
    pub guidance: Option<Guidance>,
    /// Lower-case ship symbol for ship-specific modules (armour), e.g. `"krait_mkii"`.
    pub ship: Option<String>,
    /// Class: 0 for utilities, 1–4 (small…huge) for hardpoints, 1–8 for internals.
    pub size: u8,
    /// The class FDevIDs gives, typos included (Spansh's): `size` takes the symbol's word for it.
    pub fdev_class: u8,
    /// Rating letter `'A'`…`'I'`.
    pub rating: char,
    /// Store entitlement SKU, if any (powerplay modules, store items, Horizons/Odyssey gates).
    pub entitlement: Option<String>,
}

impl Module {
    /// What players expect to read, e.g. `"5A Frame Shift Drive (SCO)"`, `"3C Fixed Multi-cannon"`,
    /// `"Lightweight Alloy (Krait Mk II)"`, `"0A Shield Booster"`, `"Advanced Docking Computer"`.
    pub fn display_name(&self) -> String {
        if let Some(ship) = &self.ship {
            return format!("{} ({})", self.name, crate::ships::ship_name(ship));
        }
        if self.is_hardpoint() {
            return match self.mount {
                Some(m) => format!("{}{} {} {}", self.size, self.rating, m.label(), self.name),
                None => format!("{}{} {}", self.size, self.rating, self.name),
            };
        }
        if self.symbol.contains("_size") {
            return format!("{}{} {}", self.size, self.rating, self.name);
        }
        self.name.clone()
    }

    /// A weapon/tool hardpoint (small…huge), as opposed to a utility mount.
    pub fn is_hardpoint(&self) -> bool {
        self.symbol.starts_with("hpt_") && hardpoint_size_of(&self.symbol).is_some()
    }

    /// `"Small"`, `"Medium"`, `"Large"` or `"Huge"` for hardpoints.
    pub fn hardpoint_size_name(&self) -> Option<&'static str> {
        if self.is_hardpoint() { hardpoint_size_name(self.size) } else { None }
    }
}

/// `1..=4` → `"Small"`, `"Medium"`, `"Large"`, `"Huge"`.
pub fn hardpoint_size_name(size: u8) -> Option<&'static str> {
    match size {
        1 => Some("Small"),
        2 => Some("Medium"),
        3 => Some("Large"),
        4 => Some("Huge"),
        _ => None,
    }
}

fn hardpoint_size_of(symbol: &str) -> Option<u8> {
    symbol.split('_').find_map(|t| match t {
        "small" => Some(1),
        "medium" => Some(2),
        "large" => Some(3),
        "huge" => Some(4),
        _ => None,
    })
}

/// `..._size5_...` → 5.
fn size_token(symbol: &str) -> Option<u8> {
    symbol.split('_').find_map(|t| t.strip_prefix("size").and_then(|n| n.parse().ok()))
}

fn mount_token(symbol: &str) -> Option<Mount> {
    symbol.split('_').find_map(|t| match t {
        "fixed" => Some(Mount::Fixed),
        "gimbal" => Some(Mount::Gimballed),
        "turret" => Some(Mount::Turreted),
        _ => None,
    })
}

/// Name tidy-ups so the UI reads like the game/community tools.
fn normalise_name(name: &str) -> String {
    name.trim().replace("Multi-Cannon", "Multi-cannon").replace("MkII ", "Mk II ")
}

struct Table {
    list: Vec<Module>,
    by_symbol: FxHashMap<String, usize>,
    by_id: FxHashMap<u64, usize>,
}

static TABLE: LazyLock<Table> = LazyLock::new(load);

fn load() -> Table {
    let mut list = Vec::with_capacity(1300);
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(OUTFITTING_CSV.as_bytes());
    for rec in rdr.records().flatten() {
        let field = |i: usize| rec.get(i).unwrap_or("");
        let symbol = field(1).to_ascii_lowercase();
        if symbol.is_empty() {
            continue;
        }
        let category = match field(2).to_ascii_lowercase().as_str() {
            "standard" => ModuleCategory::Standard,
            "internal" => ModuleCategory::Internal,
            "hardpoint" => ModuleCategory::Hardpoint,
            "utility" => ModuleCategory::Utility,
            "mercgear" => ModuleCategory::MercGear,
            _ => continue,
        };
        let is_hpt = symbol.starts_with("hpt_");
        let mount = if is_hpt { Mount::parse(field(4)).or_else(|| mount_token(&symbol)) } else { None };
        let guidance = match field(5).to_ascii_lowercase().as_str() {
            "dumbfire" => Some(Guidance::Dumbfire),
            "seeker" => Some(Guidance::Seeker),
            _ => None,
        };
        let ship = symbol.split_once("_armour_").map(|(s, _)| s.to_string());
        let csv_class: u8 = field(7).parse().unwrap_or(0);
        // The symbol is authoritative where it encodes the size (FDevIDs has a few class typos).
        let size =
            if is_hpt { hardpoint_size_of(&symbol).or_else(|| size_token(&symbol)) } else { size_token(&symbol) }
                .unwrap_or(csv_class);
        list.push(Module {
            id: field(0).parse().unwrap_or(0),
            name: normalise_name(field(3)),
            fdev_name: field(3).to_string(),
            category,
            mount,
            guidance,
            ship,
            size,
            fdev_class: csv_class,
            rating: field(8).chars().next().filter(char::is_ascii_alphabetic).unwrap_or('?'),
            entitlement: Some(field(9)).filter(|s| !s.is_empty() && *s != "?").map(str::to_string),
            symbol,
        });
    }
    let mut by_symbol: FxHashMap<String, usize> = FxHashMap::default();
    let mut by_id = FxHashMap::default();
    for (i, m) in list.iter().enumerate() {
        by_id.insert(m.id, i);
        match by_symbol.get(&m.symbol) {
            // Prefer the regular module over a MercGear row sharing its symbol.
            Some(&j) if list[j].category != ModuleCategory::MercGear || m.category == ModuleCategory::MercGear => {}
            _ => {
                by_symbol.insert(m.symbol.clone(), i);
            }
        }
    }
    Table { list, by_symbol, by_id }
}

/// Lower-case, strip `$…_name;`.
fn normalise_symbol(raw: &str) -> String {
    crate::commodities::symbol(raw)
}

/// All FDevIDs outfitting rows (including MercGear), in FDevIDs order.
pub fn all() -> &'static [Module] {
    &TABLE.list
}

/// Looks a module up by journal `Item` symbol (any case, `$…_name;` tolerated).
pub fn module(symbol: &str) -> Option<&'static Module> {
    let t = &*TABLE;
    t.by_symbol.get(&normalise_symbol(symbol)).map(|&i| &t.list[i])
}

/// Looks a module up by its FDevIDs / Outfitting.json id (MercGear rows included).
pub fn module_by_id(id: u64) -> Option<&'static Module> {
    let t = &*TABLE;
    t.by_id.get(&id).map(|&i| &t.list[i])
}

/// Every row sharing a symbol (the regular module plus any MercGear variants).
pub fn variants(symbol: &str) -> Vec<&'static Module> {
    let sym = normalise_symbol(symbol);
    all().iter().filter(|m| m.symbol == sym).collect()
}

/// Every FDevIDs name a symbol goes by, the regular module's first. Spansh lists a station's module
/// under one of them: for the symbols MercGear shares, mostly the MercGear name (most stations
/// selling a 5A power distributor list it as "Balanced Power Distributor").
pub fn fdev_names(symbol: &str) -> Vec<&'static str> {
    let mut rows = variants(symbol);
    rows.sort_by_key(|m| m.category == ModuleCategory::MercGear);
    let mut names: Vec<&'static str> = Vec::new();
    for m in rows {
        if !names.contains(&m.fdev_name.as_str()) {
            names.push(&m.fdev_name);
        }
    }
    names
}

/// Display name for a journal module symbol, e.g. `"5A Frame Shift Drive (SCO)"`,
/// `"3C Fixed Multi-cannon"`, `"Lightweight Alloy (Krait Mk II)"`. Cosmetics and unknown symbols
/// get a readable fallback.
pub fn display_name(symbol: &str) -> String {
    let sym = normalise_symbol(symbol);
    if let Some(m) = module(&sym) {
        return m.display_name();
    }
    if let Some(name) = cosmetic_name(&sym) {
        return name.to_string();
    }
    // Variant suffixes the CSV does not carry, e.g. `explorer_nx_armour_grade1_default`.
    let mut base = sym.as_str();
    while let Some((head, _)) = base.rsplit_once('_') {
        if !head.contains('_') {
            break;
        }
        if let Some(m) = module(head) {
            return m.display_name();
        }
        base = head;
    }
    prettify_module(&sym)
}

fn cosmetic_name(sym: &str) -> Option<&'static str> {
    let starts = |p: &str| sym.starts_with(p);
    Some(if sym.ends_with("_cockpit") {
        "Cockpit"
    } else if starts("modularcargobaydoor") {
        "Cargo Hatch"
    } else if starts("paintjob_") {
        "Paint Job"
    } else if starts("decal_") {
        "Decal"
    } else if starts("nameplate_") {
        "Nameplate"
    } else if starts("voicepack_") {
        "COVAS Voice"
    } else if starts("weaponcustomisation_") {
        "Weapon Colour"
    } else if starts("enginecustomisation_") {
        "Engine Colour"
    } else if starts("bobble_") {
        "Bobblehead"
    } else if starts("string_lights") {
        "String Lights"
    } else if sym.contains("_shipkit") {
        "Ship Kit"
    } else if sym == "int_codexscanner" {
        "Codex Scanner"
    } else if sym == "int_datalinkscanner" {
        "Data Link Scanner"
    } else {
        return None;
    })
}

/// Generic fallback: `int_newthing_size4_class2` → `"4D Newthing"`, `hpt_foo_fixed_large` → `"3? Fixed Foo"`.
fn prettify_module(sym: &str) -> String {
    let body = sym.strip_prefix("int_").or_else(|| sym.strip_prefix("hpt_")).unwrap_or(sym);
    let mut size: Option<u8> = None;
    let mut class: Option<u8> = None;
    let mut mount: Option<Mount> = None;
    let mut words: Vec<String> = Vec::new();
    for t in body.split('_').filter(|t| !t.is_empty()) {
        if let Some(n) = t.strip_prefix("size").and_then(|n| n.parse().ok()) {
            size = Some(n);
        } else if let Some(n) = t.strip_prefix("class").and_then(|n| n.parse().ok()) {
            class = Some(n);
        } else if let Some(m) = mount_token(t) {
            mount = Some(m);
        } else if let Some(n) = hardpoint_size_of(t) {
            size = Some(n);
        } else if t != "tiny" {
            words.push(crate::names::split_camel(t));
        }
    }
    let rating = class.and_then(|c| match c {
        1 => Some('E'),
        2 => Some('D'),
        3 => Some('C'),
        4 => Some('B'),
        5 => Some('A'),
        _ => None,
    });
    let mut out = String::new();
    if let Some(s) = size {
        out.push_str(&s.to_string());
        out.push(rating.unwrap_or('?'));
        out.push(' ');
    }
    if let Some(m) = mount {
        out.push_str(m.label());
        out.push(' ');
    }
    out.push_str(&words.join(" "));
    out.trim().to_string()
}

// ---------------------------------------------------------------------------------------------
// Slots
// ---------------------------------------------------------------------------------------------

/// Broad kind of a `Loadout.Modules[].Slot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotKind {
    Core,
    Optional,
    Military,
    Hardpoint,
    Utility,
    Cosmetic,
    Other,
}

const CORE_SLOTS: &[(&str, &str)] = &[
    ("armour", "Bulkheads"),
    ("powerplant", "Power Plant"),
    ("mainengines", "Thrusters"),
    ("frameshiftdrive", "Frame Shift Drive"),
    ("lifesupport", "Life Support"),
    ("powerdistributor", "Power Distributor"),
    ("radar", "Sensors"),
    ("fueltank", "Fuel Tank"),
];

const OTHER_SLOTS: &[(&str, &str, SlotKind)] = &[
    ("planetaryapproachsuite", "Planetary Approach Suite", SlotKind::Other),
    ("cargohatch", "Cargo Hatch", SlotKind::Other),
    ("datalinkscanner", "Data Link Scanner", SlotKind::Other),
    ("codexscanner", "Codex Scanner", SlotKind::Other),
    ("discoveryscanner", "Discovery Scanner", SlotKind::Other),
    ("colonisationsuite", "Colonisation Suite", SlotKind::Other),
    ("shipcockpit", "Cockpit", SlotKind::Cosmetic),
    ("paintjob", "Paint Job", SlotKind::Cosmetic),
    ("vesselvoice", "COVAS Voice", SlotKind::Cosmetic),
    ("weaponcolour", "Weapon Colour", SlotKind::Cosmetic),
    ("enginecolour", "Engine Colour", SlotKind::Cosmetic),
    ("stringlights", "String Lights", SlotKind::Cosmetic),
    ("shipkitspoiler", "Ship Kit (Spoiler)", SlotKind::Cosmetic),
    ("shipkitwings", "Ship Kit (Wings)", SlotKind::Cosmetic),
    ("shipkittail", "Ship Kit (Tail)", SlotKind::Cosmetic),
    ("shipkitbumper", "Ship Kit (Bumper)", SlotKind::Cosmetic),
];

/// Numbered slot families: (lower-case prefix, label, kind, zero-based?). Newer hulls have slots
/// that take only one kind of module (the Type-11 Prospector's mining hardpoints, limpet and hangar
/// slots), named for it as EDSY and Coriolis name them.
const NUMBERED_SLOTS: &[(&str, &str, SlotKind, bool)] = &[
    ("tinyhardpoint", "Utility", SlotKind::Utility, false),
    ("smallhardpoint", "Small hardpoint", SlotKind::Hardpoint, false),
    ("mediumhardpoint", "Medium hardpoint", SlotKind::Hardpoint, false),
    ("largehardpoint", "Large hardpoint", SlotKind::Hardpoint, false),
    ("hugehardpoint", "Huge hardpoint", SlotKind::Hardpoint, false),
    ("smallmininghardpoint", "Small mining hardpoint", SlotKind::Hardpoint, false),
    ("mediummininghardpoint", "Medium mining hardpoint", SlotKind::Hardpoint, false),
    ("largemininghardpoint", "Large mining hardpoint", SlotKind::Hardpoint, false),
    ("hugemininghardpoint", "Huge mining hardpoint", SlotKind::Hardpoint, false),
    ("military", "Military", SlotKind::Military, false),
    ("passenger", "Passenger", SlotKind::Optional, false),
    ("cargo", "Cargo slot", SlotKind::Optional, false),
    ("limpetcontroller", "Limpet slot", SlotKind::Optional, false),
    ("fighterbay", "Hangar slot", SlotKind::Optional, false),
    ("decal", "Decal", SlotKind::Cosmetic, false),
    ("bobble", "Bobblehead", SlotKind::Cosmetic, false),
    ("shipname", "Nameplate", SlotKind::Cosmetic, true),
    ("shipid", "Ship ID", SlotKind::Cosmetic, true),
];

/// `Slot03_Size6` → `(3, 6)`.
fn optional_slot(lower: &str) -> Option<(u32, u32)> {
    let (num, size) = lower.strip_prefix("slot")?.split_once("_size")?;
    Some((num.parse().ok()?, size.parse().ok()?))
}

/// Classifies a journal slot name.
pub fn slot_kind(slot: &str) -> SlotKind {
    let lower = slot.trim().to_ascii_lowercase();
    if optional_slot(&lower).is_some() {
        return SlotKind::Optional;
    }
    if CORE_SLOTS.iter().any(|(k, _)| *k == lower) {
        return SlotKind::Core;
    }
    if let Some((_, _, kind)) = OTHER_SLOTS.iter().find(|(k, _, _)| *k == lower) {
        return *kind;
    }
    NUMBERED_SLOTS
        .iter()
        .find(|(p, ..)| lower.strip_prefix(p).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
        .map(|(_, _, k, _)| *k)
        .unwrap_or(SlotKind::Other)
}

/// Human-readable slot name: `"Slot03_Size6"` → `"Optional 3 (size 6)"`, `"TinyHardpoint2"` →
/// `"Utility 2"`, `"MediumHardpoint1"` → `"Medium hardpoint 1"`, `"MainEngines"` → `"Thrusters"`,
/// `"Radar"` → `"Sensors"`, `"Armour"` → `"Bulkheads"`.
pub fn slot_display(slot: &str) -> String {
    let lower = slot.trim().to_ascii_lowercase();
    if let Some((n, size)) = optional_slot(&lower) {
        return format!("Optional {n} (size {size})");
    }
    if let Some((_, label)) = CORE_SLOTS.iter().find(|(k, _)| *k == lower) {
        return (*label).to_string();
    }
    if let Some((_, label, _)) = OTHER_SLOTS.iter().find(|(k, _, _)| *k == lower) {
        return (*label).to_string();
    }
    for (prefix, label, _, zero_based) in NUMBERED_SLOTS {
        if let Some(n) = lower.strip_prefix(prefix).and_then(|n| n.parse::<u32>().ok()) {
            return format!("{label} {}", if *zero_based { n + 1 } else { n });
        }
    }
    // Unknown: split words and trailing digits ("SomethingNew02" → "Something New 2").
    let trimmed = slot.trim();
    let digits_at = trimmed.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (word, num) = trimmed.split_at(digits_at);
    let word = crate::names::split_camel(&word.replace('_', " "));
    match num.parse::<u32>() {
        Ok(n) => format!("{word} {n}"),
        Err(_) => word,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_all_rows() {
        assert_eq!(all().len(), 1243);
        assert_eq!(all().iter().filter(|m| m.category == ModuleCategory::MercGear).count(), 25);
    }

    #[test]
    fn display_names_players_expect() {
        assert_eq!(display_name("int_hyperdrive_overcharge_size5_class5"), "5A Frame Shift Drive (SCO)");
        assert_eq!(display_name("Hpt_MultiCannon_Fixed_Large"), "3C Fixed Multi-cannon");
        assert_eq!(display_name("krait_mkii_armour_grade1"), "Lightweight Alloy (Krait Mk II)");
        assert_eq!(display_name("$int_hyperdrive_size5_class5_name;"), "5A Frame Shift Drive");
        assert_eq!(display_name("hpt_beamlaser_gimbal_medium"), "2D Gimballed Beam Laser");
        assert_eq!(display_name("hpt_shieldbooster_size0_class5"), "0A Shield Booster");
        assert_eq!(display_name("hpt_heatsinklauncher_turret_tiny"), "Heat Sink Launcher");
        assert_eq!(display_name("int_dockingcomputer_advanced"), "Advanced Docking Computer");
        assert_eq!(display_name("int_supercruiseassist"), "Supercruise Assist");
        assert_eq!(display_name("int_shieldgenerator_size6_class3_fast"), "6C Bi-Weave Shield Generator");
        assert_eq!(display_name("int_guardianfsdbooster_size5"), "5H Guardian FSD Booster");
        assert_eq!(display_name("int_detailedsurfacescanner_tiny"), "Detailed Surface Scanner");
        assert_eq!(display_name("hpt_basicmissilerack_fixed_medium"), "2B Fixed Seeker Missile Rack");
    }

    #[test]
    fn display_names_new_2025_2026_modules() {
        // Caspian Explorer, Kestrel Mk II, Lynx Highliner armour; SCO Mk II; Mk II thrusters and racks.
        assert_eq!(display_name("explorer_nx_armour_grade1"), "Mk II Ablative Lightweight Alloys (Caspian Explorer)");
        // The Caspian's default bulkheads are a separate (non-ablative) item.
        assert_eq!(display_name("explorer_nx_armour_grade1_default"), "Lightweight Alloy (Caspian Explorer)");
        // Unknown variant suffixes fall back to the base item.
        assert_eq!(display_name("krait_mkii_armour_grade3_somevariant"), "Military Grade Composite (Krait Mk II)");
        assert_eq!(display_name("smallcombat01_nx_armour_grade3"), "Military Grade Composite (Kestrel Mk II)");
        assert_eq!(display_name("mediumtransport01_armour_grade1"), "Lightweight Alloy (Lynx Highliner)");
        // The Anaconda Mk II (Oct 2026): ablative bulkheads, and the default ones it comes with.
        assert_eq!(display_name("anacondamkii_armour_grade3"), "Mk II Ablative Military Grade Composite (Anaconda Mk II)");
        assert_eq!(display_name("AnacondaMkII_Armour_Grade1_Default"), "Lightweight Alloy (Anaconda Mk II)");
        assert_eq!(display_name("int_largecargorack_size6_class1"), "6D Mk II Cargo Rack");
        assert_eq!(display_name("lakonminer_armour_reactive"), "Reactive Surface Composite (Type-11 Prospector)");
        assert_eq!(
            display_name("int_hyperdrive_overcharge_size8_class5_overchargebooster_mkii"),
            "8A Mk II Supercharge Optimised Frame Shift Drive (SCO)"
        );
        assert_eq!(
            display_name("int_engine_size7_class5_gravityoptimised_mkii"),
            "7A Mk II Gravity Optimised Thrusters"
        );
        assert_eq!(display_name("int_largecargorack_size8_class1"), "8D Mk II Cargo Rack");
        assert_eq!(display_name("hpt_miningtoolv2_fixed_large"), "3C Fixed Mining Volley Repeater");
        // FDevIDs says class 5 for the size-6 Mk II cabin; the symbol wins (Spansh keeps the 5).
        assert_eq!(display_name("int_mkii_passengercabin_size6_class2"), "6C Mk II Business Class Passenger Cabin");
        assert_eq!(module("int_mkii_passengercabin_size6_class2").map(|m| (m.size, m.fdev_class)), Some((6, 5)));
        assert_eq!(display_name("int_mkiilargebuggybay_size2_class3"), "2F Mk II Large Planetary Vehicle Hangar");
    }

    #[test]
    fn mercgear_and_fallbacks() {
        // The regular rail gun wins the symbol; the MercGear row is reachable by id.
        let rg = module("hpt_railgun_fixed_medium").unwrap();
        assert_eq!((rg.id, rg.category), (128049489, ModuleCategory::Hardpoint));
        let merc = module_by_id(129044375).unwrap();
        assert_eq!(
            (merc.category, merc.name.as_str(), merc.mount),
            (ModuleCategory::MercGear, "Enduring Feedback Rail Gun", Some(Mount::Fixed))
        );
        assert_eq!(merc.display_name(), "2B Fixed Enduring Feedback Rail Gun");
        assert_eq!(variants("Hpt_BasicMissileRack_Fixed_Medium").len(), 4);
        // Spansh's spellings: FDevIDs' own, regular module first.
        assert_eq!(fdev_names("int_powerdistributor_size5_class5"), ["Power Distributor", "Balanced Power Distributor"]);
        assert_eq!(fdev_names("Hpt_MultiCannon_Gimbal_Huge"), ["Multi-Cannon"]);
        assert_eq!(module("hpt_multicannon_gimbal_huge").map(|m| (m.name.as_str(), m.fdev_name.as_str())), Some(("Multi-cannon", "Multi-Cannon")));
        assert!(fdev_names("no_such_module").is_empty());
        // Cosmetics and unknowns.
        assert_eq!(display_name("krait_mkii_cockpit"), "Cockpit");
        assert_eq!(display_name("modularcargobaydoorfdl"), "Cargo Hatch");
        assert_eq!(display_name("paintjob_krait_mkii_lrpo_azure"), "Paint Job");
        assert_eq!(display_name("int_newthing_size4_class2"), "4D Newthing");
        let a = module("Krait_MkII_Armour_Grade3").unwrap();
        assert_eq!((a.ship.as_deref(), a.size, a.rating), (Some("krait_mkii"), 1, 'I'));
        let mc = module("hpt_multicannon_gimbal_huge").unwrap();
        assert_eq!((mc.size, mc.mount, mc.hardpoint_size_name()), (4, Some(Mount::Gimballed), Some("Huge")));
        let sm = module("hpt_basicmissilerack_fixed_small").unwrap();
        assert_eq!(sm.guidance, Some(Guidance::Seeker));
    }

    #[test]
    fn slot_names() {
        assert_eq!(slot_display("Slot03_Size6"), "Optional 3 (size 6)");
        assert_eq!(slot_display("Slot00_Size8"), "Optional 0 (size 8)");
        assert_eq!(slot_display("TinyHardpoint2"), "Utility 2");
        assert_eq!(slot_display("MediumHardpoint1"), "Medium hardpoint 1");
        assert_eq!(slot_display("HugeHardpoint1"), "Huge hardpoint 1");
        assert_eq!(slot_display("MainEngines"), "Thrusters");
        assert_eq!(slot_display("Radar"), "Sensors");
        assert_eq!(slot_display("Armour"), "Bulkheads");
        assert_eq!(slot_display("FrameShiftDrive"), "Frame Shift Drive");
        assert_eq!(slot_display("PlanetaryApproachSuite"), "Planetary Approach Suite");
        assert_eq!(slot_display("Military01"), "Military 1");
        assert_eq!(slot_display("Passenger02"), "Passenger 2");
        assert_eq!(slot_display("ShipName0"), "Nameplate 1");
        assert_eq!(slot_display("SomethingNew02"), "Something New 2");
        assert_eq!(slot_kind("Slot01_Size6"), SlotKind::Optional);
        assert_eq!(slot_kind("PowerPlant"), SlotKind::Core);
        assert_eq!(slot_kind("TinyHardpoint1"), SlotKind::Utility);
        assert_eq!(slot_kind("LargeHardpoint3"), SlotKind::Hardpoint);
        assert_eq!(slot_kind("PaintJob"), SlotKind::Cosmetic);
        assert_eq!(slot_kind("Bobble07"), SlotKind::Cosmetic);
        // Slots for one kind of module (Type-11 Prospector), not cosmetics or built-ins to skip.
        assert_eq!(slot_kind("LargeMiningHardpoint1"), SlotKind::Hardpoint);
        assert_eq!(slot_display("MediumMiningHardpoint2"), "Medium mining hardpoint 2");
        assert_eq!((slot_kind("LimpetController01"), slot_display("LimpetController01").as_str()), (SlotKind::Optional, "Limpet slot 1"));
        assert_eq!((slot_kind("FighterBay01"), slot_display("FighterBay01").as_str()), (SlotKind::Optional, "Hangar slot 1"));
        assert_eq!((slot_kind("Cargo02"), slot_display("Cargo02").as_str()), (SlotKind::Optional, "Cargo slot 2"));
        assert_eq!(slot_kind("CargoHatch"), SlotKind::Other);
    }
}
