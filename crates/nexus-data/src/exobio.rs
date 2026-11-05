//! Exobiology knowledge base: genera (clonal colony ranges, codex ids), species (Vista Genomics base values,
//! prediction rule sets, colour variants) and the galactic region names used by region-specific species.
//!
//! Backed by `data/exobiology/species.json` and `genera.json` (per-entry sources and certainty are in the files,
//! the method in `docs/research/exploration.md` §2). Both files are embedded at compile time and parsed once, on
//! first use. The prediction engine that evaluates the rules lives in [`crate::bio_predict`].
//!
//! Values were hand-verified against the commander's own `SellOrganicData` events where possible
//! (Stratum Paleas 1,362,000; Tussock Cultro 1,766,600; Aleoida Laminiae 3,385,200; Fungoida Setisis 1,670,100;
//! Concha Aureolas 7,774,700; Stratum Tectonicas 19,010,800 …). First-logged bonus observed in-game: +4× (5× total).

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// First-logged bonus multiplier on top of the base value (bonus = 4 × value, 5 × in total).
pub const FIRST_LOGGED_BONUS_FACTOR: i64 = 4;

/// Journal `SurfaceGravity` (m/s²) per g: every gravity limit in the dataset is in these g, slightly under the
/// standard 9.80665.
pub const JOURNAL_G_MS2: f64 = 9.797759;

/// Journal `SurfacePressure` (Pa) per atmosphere: every pressure limit in the dataset is in these atmospheres,
/// slightly under the standard 101,325 Pa.
pub const JOURNAL_ATM_PA: f64 = 101_231.656_25;

const SPECIES_JSON: &str = include_str!("../../../data/exobiology/species.json");
const GENERA_JSON: &str = include_str!("../../../data/exobiology/genera.json");

// ------------------------------------------------------------------------------------------------
// Public data model
// ------------------------------------------------------------------------------------------------

/// Game era a genus/species belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Era {
    /// Odyssey exobiology (thin-atmosphere bodies, one species per genus per body).
    Odyssey,
    /// Horizons-era surface organics (Brain Trees, Anemones, Sinuous Tubers, Bark Mounds, …).
    Horizons,
}

/// Overall certainty of a dataset entry (minimum of value, conditions and colony-range certainty).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Certainty {
    Low,
    Medium,
    High,
}

/// What decides a species' colour variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColourBasis {
    /// Class of the star the body orbits (keys `O B A F G K M L T Y TTS Ae W D N`).
    Star,
    /// A rare element present in `Scan.Materials` (keys are element names, e.g. `Antimony`).
    Element,
    /// No variants (Horizons-era organics, Radicoida, Thargoid samples).
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Genus {
    /// English genus name ("Stratum", "Brain Tree", "Sinuous Tubers").
    pub name: &'static str,
    /// Minimum distance between samples of the same species, metres.
    pub colony_range_m: u32,
    /// Genus codex symbol as written in `SAASignalsFound.Genuses[].Genus`.
    pub codex: Option<&'static str>,
    /// Common prefix of the species codex ids (Odyssey genera only).
    pub codex_prefix: Option<&'static str>,
    pub era: Era,
    /// Brain Trees and Sinuous Tubers can host several species on one body; Odyssey genera cannot.
    pub multiple_species_per_body: bool,
}

/// A colour variant: `key` is a star-class code or an element name, `colour` the in-game colour name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub key: &'static str,
    pub colour: &'static str,
    /// Variant codex symbol, e.g. `$Codex_Ent_Aleoids_01_A_Name;`.
    pub codex: &'static str,
    /// A codex entry known to exist in the game (false = a colour only the colour map predicts).
    pub in_codex: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Species {
    pub genus: &'static str,
    /// Full English name ("Stratum Tectonicas", "Roseum Brain Tree").
    pub name: &'static str,
    /// Current Vista Genomics base value (Update 14.01+).
    pub value: i64,
    /// Species codex symbol (`ScanOrganic.Species`, `SellOrganicData.BioData[].Species`).
    pub codex: &'static str,
    pub colony_range_m: u32,
    pub era: Era,
    pub certainty: Certainty,
    /// Thargoid xeno sample (site-only, never predicted from body data).
    pub thargoid: bool,
    /// Base value before Update 14 (2022-11-29), for re-pricing old journals.
    pub value_pre_update14: Option<i64>,
    /// Rule sets: the conditions the species has been found in (OR of rule sets, each an AND of constraints).
    /// Constraints a rule set leaves open are filled from the merged envelope (the dataset's "constrained side
    /// taken" convention), so a rule set never accepts what every source rejects.
    pub rulesets: Vec<RuleSet>,
    /// Union envelope of the rule sets and the cross-check criteria (`conditions` in the dataset) plus the
    /// system-level requirements every rule set shares (region, nebula, Guardian bubble, …). Wider than any one rule
    /// set; meant for filtering and display.
    pub envelope: RuleSet,
    /// A second, wider set of criteria for the species (the dataset's `srvsurvey_crosscheck`), flattened over its
    /// criteria tree: atmospheres, body classes, limits, volcanism and regions. Only the fields it actually constrains
    /// are set. The predictor uses it to widen a rule set field by field: a value outside the rule set but inside
    /// these criteria is a softer miss.
    pub crosscheck: Option<RuleSet>,
    pub colour_basis: ColourBasis,
    pub variants: Vec<Variant>,
    pub aliases: Vec<&'static str>,
}

impl Species {
    pub fn genus(&self) -> Option<&'static Genus> {
        genus(self.genus)
    }

    /// Colour variant for a star-class code (`"K"`) or element name (`"Antimony"`), case-insensitive.
    pub fn variant(&self, key: &str) -> Option<&Variant> {
        self.variants.iter().find(|v| v.key.eq_ignore_ascii_case(key))
    }
}

/// One rule set. `None` / empty = unconstrained. Units: g = `SurfaceGravity / JOURNAL_G_MS2`,
/// atm = `SurfacePressure / JOURNAL_ATM_PA`, K, light seconds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleSet {
    /// Allowed `Scan.AtmosphereType` values (`"None"` = airless).
    pub atmospheres: Option<Vec<String>>,
    /// Allowed `Scan.PlanetClass` values.
    pub body_types: Option<Vec<String>>,
    pub min_gravity_g: Option<f64>,
    pub max_gravity_g: Option<f64>,
    pub min_temp_k: Option<f64>,
    pub max_temp_k: Option<f64>,
    pub min_pressure_atm: Option<f64>,
    /// Exclusive: a body at `max` or above is out.
    pub max_pressure_atm: Option<f64>,
    pub volcanism: VolcanismRule,
    pub regions: Option<RegionRule>,
    /// Minimum share (%) of a gas in `Scan.AtmosphereComposition` (Recepta: SulphurDioxide ≥ 1.05 %).
    pub min_atmosphere_pct: Vec<(String, f64)>,
    /// Some star anywhere in the system must match.
    pub star_in_system: Option<Vec<StarReq>>,
    /// The primary star or one of the body's parent stars must match.
    pub parent_star: Option<Vec<StarReq>>,
    /// The system must contain one of these planet classes.
    pub system_body_types: Option<Vec<String>>,
    /// Nebula proximity: `"all"` (large or planetary nebula) or `"large"`.
    pub nebula: Option<String>,
    /// Must be inside a Guardian-ruins bubble.
    pub guardian: bool,
    /// Must lie in the distance band of one of these Sinuous-tuber zones.
    pub tuber_zones: Option<Vec<String>>,
    /// Exact system name (Radicoida: `HIP 87621`).
    pub system: Option<String>,
    pub max_orbital_period_s: Option<f64>,
    pub min_distance_ls: Option<f64>,
}

/// Volcanism constraint, evaluated on the lower-cased `Scan.Volcanism` text (empty = no volcanism).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum VolcanismRule {
    #[default]
    Any,
    /// No volcanism.
    None,
    /// Any volcanism.
    Required,
    /// Text contains one of `contains` or equals one of `equals`; `or_none` also accepts no volcanism.
    Match { contains: Vec<String>, equals: Vec<String>, or_none: bool },
}

/// Galactic region constraint (region ids 1–42, see [`region_name`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegionRule {
    pub inside: Option<Vec<u8>>,
    pub outside: Option<Vec<u8>>,
}

/// Star requirement: class (`"B"`, `"AeBe"`, `"N"`, `"D"` …) and optional luminosity class (`"IV"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarReq {
    pub class: String,
    pub luminosity: Option<String>,
}

// ------------------------------------------------------------------------------------------------
// Lookups
// ------------------------------------------------------------------------------------------------

struct Kb {
    genera: Vec<Genus>,
    species: Vec<Species>,
}

static KB: LazyLock<Kb> = LazyLock::new(load);

/// Every genus in the dataset (incl. Horizons-era organics and the Thargoid sample group).
pub fn genera() -> &'static [Genus] {
    &KB.genera
}

/// Every species in the dataset (incl. Horizons-era organics and Thargoid samples).
pub fn all_species() -> &'static [Species] {
    &KB.species
}

/// Species of one genus (by genus name).
pub fn species_in_genus(genus_name: &str) -> impl Iterator<Item = &'static Species> {
    let g = genus(genus_name).map(|g| g.name);
    all_species().iter().filter(move |s| Some(s.genus) == g)
}

/// Genus by English name ("Stratum", "Brain Trees", "bark mound") or codex symbol
/// (`$Codex_Ent_Stratum_Genus_Name;`, the `$Codex_Ent_Clepeus_Genus_Name;;` misspelling, `$Codex_Ent_Brancae_Name;`).
pub fn genus(name: &str) -> Option<&'static Genus> {
    let n = name.trim();
    if n.starts_with('$') {
        return genera()
            .iter()
            .find(|g| g.codex.is_some_and(|c| same_symbol(c, n)))
            .or_else(|| GENUS_SYMBOL_QUIRKS.iter().find(|(q, _)| same_symbol(q, n)).and_then(|(_, g)| genus(g)));
    }
    fn singular(s: &str) -> &str {
        s.trim_end_matches(['s', 'S'])
    }
    genera()
        .iter()
        .find(|g| g.name.eq_ignore_ascii_case(n))
        .or_else(|| genera().iter().find(|g| singular(g.name).eq_ignore_ascii_case(singular(n))))
}

/// Species by English name ("Stratum Tectonicas", alias "Radicoida Unicus"), localised variant name
/// ("Stratum Tectonicas - Lime"), species codex symbol or variant codex symbol.
pub fn species(name: &str) -> Option<&'static Species> {
    let n = name.trim();
    if n.starts_with('$') {
        return all_species()
            .iter()
            .find(|s| same_symbol(s.codex, n))
            .or_else(|| variant_by_codex(n).map(|(s, _)| s));
    }
    let by_name = |q: &str| {
        all_species()
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(q) || s.aliases.iter().any(|a| a.eq_ignore_ascii_case(q)))
    };
    by_name(n).or_else(|| n.split_once(" - ").and_then(|(base, _)| by_name(base.trim())))
}

/// Species and colour variant for a variant codex symbol (`$Codex_Ent_Tubus_01_A_Name;`, `CodexEntry.Name`).
pub fn variant_by_codex(symbol: &str) -> Option<(&'static Species, &'static Variant)> {
    all_species()
        .iter()
        .find_map(|s| s.variants.iter().find(|v| same_symbol(v.codex, symbol)).map(|v| (s, v)))
}

/// Colony range for a genus or species (name or codex symbol; falls back to the genus part of a species name).
pub fn colony_range_m(genus_or_species: &str) -> Option<u32> {
    genus(genus_or_species)
        .map(|g| g.colony_range_m)
        .or_else(|| species(genus_or_species).map(|s| s.colony_range_m))
        .or_else(|| genus_or_species.split_whitespace().next().and_then(genus).map(|g| g.colony_range_m))
}

/// Journal genus symbols that do not follow the dataset's codex ids.
const GENUS_SYMBOL_QUIRKS: &[(&str, &str)] = &[
    ("$Codex_Ent_Clepeus_Genus_Name;", "Clypeus"),
    ("$Codex_Ent_Stratum_04_Name;", "Stratum"),
];

/// Codex symbols compare case-insensitively, ignoring the `;;` seen on some journal symbols.
fn same_symbol(a: &str, b: &str) -> bool {
    a.trim().trim_end_matches(';').eq_ignore_ascii_case(b.trim().trim_end_matches(';'))
}

// ------------------------------------------------------------------------------------------------
// Galactic regions
// ------------------------------------------------------------------------------------------------

/// Galactic region names by id (index 0 unused). Ids match `CodexEntry.Region` (`$Codex_RegionName_18;`) and
/// klightspeed's EliteDangerousRegionMap.
pub const REGION_NAMES: [&str; 43] = [
    "",
    "Galactic Centre",
    "Empyrean Straits",
    "Ryker's Hope",
    "Odin's Hold",
    "Norma Arm",
    "Arcadian Stream",
    "Izanami",
    "Inner Orion-Perseus Conflux",
    "Inner Scutum-Centaurus Arm",
    "Norma Expanse",
    "Trojan Belt",
    "The Veils",
    "Newton's Vault",
    "The Conduit",
    "Outer Orion-Perseus Conflux",
    "Orion-Cygnus Arm",
    "Temple",
    "Inner Orion Spur",
    "Hawking's Gap",
    "Dryman's Point",
    "Sagittarius-Carina Arm",
    "Mare Somnia",
    "Acheron",
    "Formorian Frontier",
    "Hieronymus Delta",
    "Outer Scutum-Centaurus Arm",
    "Outer Arm",
    "Aquila's Halo",
    "Errant Marches",
    "Perseus Arm",
    "Formidine Rift",
    "Vulcan Gate",
    "Elysian Shore",
    "Sanguineous Rim",
    "Outer Orion Spur",
    "Achilles's Altar",
    "Xibalba",
    "Lyra's Song",
    "Tenebrae",
    "The Abyss",
    "Kepler's Crest",
    "The Void",
];

pub fn region_name(id: u8) -> Option<&'static str> {
    REGION_NAMES.get(usize::from(id)).copied().filter(|n| !n.is_empty())
}

/// Region id from a region name ("Inner Orion Spur", case/punctuation-insensitive), a codex symbol
/// (`$Codex_RegionName_18;`) or a plain number.
pub fn region_id(region: &str) -> Option<u8> {
    let r = region.trim();
    let digits = r.strip_prefix("$Codex_RegionName_").map_or(r, |s| s.trim_end_matches(';'));
    if let Ok(id) = digits.parse::<u8>() {
        return region_name(id).map(|_| id);
    }
    if same_alnum(r, "Galactic Center") {
        return Some(1);
    }
    (1..=42u8).find(|&id| same_alnum(REGION_NAMES[usize::from(id)], r))
}

/// Equal ignoring case and everything but ASCII letters and digits ("Ryker's Hope" == "rykers hope").
fn same_alnum(a: &str, b: &str) -> bool {
    fn key(s: &str) -> impl Iterator<Item = char> + '_ {
        s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase())
    }
    key(a).eq(key(b))
}

// ------------------------------------------------------------------------------------------------
// Loading
// ------------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawGeneraFile {
    genera: BTreeMap<String, RawGenus>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawGenus {
    genus_codex: Option<String>,
    codex_prefix: Option<String>,
    colony_range_m: u32,
    era: String,
    multiple_species_possible_per_body: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSpecies {
    genus: String,
    full_name: String,
    codex_name_pattern: String,
    era: String,
    value: i64,
    value_pre_update14: Option<i64>,
    colony_range_m: u32,
    conditions: RawRule,
    rulesets: Vec<RawRule>,
    srvsurvey_crosscheck: Option<RawSrvSurvey>,
    color_variants: RawColours,
    certainty: String,
    category: Option<String>,
    aliases: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawColours {
    determined_by: String,
    variants: Vec<RawVariant>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawVariant {
    key: String,
    color: String,
    codex: String,
    in_game_codex_ref: bool,
}

/// Union of the fields used by `rulesets[]` and `conditions` (merged envelope).
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawRule {
    atmospheres: Option<Vec<String>>,
    body_types: Option<Vec<String>>,
    min_gravity_g: Option<f64>,
    max_gravity_g: Option<f64>,
    min_temp_k: Option<f64>,
    max_temp_k: Option<f64>,
    min_pressure_atm: Option<f64>,
    max_pressure_atm: Option<f64>,
    volcanism: Option<RawVolcanism>,
    region_ids: Option<RawRegion>,
    min_atmosphere_component_pct: Option<BTreeMap<String, f64>>,
    star_anywhere_in_system: Option<Vec<RawStar>>,
    parent_or_primary_star: Option<Vec<RawStar>>,
    system_has_body_type: Option<Vec<String>>,
    nebula: Option<String>,
    near_guardian_ruins: Option<bool>,
    tuber_zones: Option<Vec<String>>,
    system: Option<String>,
    max_orbital_period_s: Option<f64>,
    min_distance_from_arrival_ls: Option<f64>,
    /// Envelope only: `"A"`, `"B IV"` … (merged from both star rule kinds).
    star_classes: Option<Vec<String>>,
}

/// `srvsurvey_crosscheck`: the second criteria tree, flattened ([`Species::crosscheck`]).
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSrvSurvey {
    atmospheres: Option<Vec<String>>,
    body_types: Option<Vec<String>>,
    min_gravity_g: Option<f64>,
    max_gravity_g: Option<f64>,
    min_temp_k: Option<f64>,
    max_temp_k: Option<f64>,
    min_pressure_atm: Option<f64>,
    max_pressure_atm: Option<f64>,
    /// `"any"` or exact volcanism names without the " volcanism" suffix ("Minor Rocky Magma", "None").
    volcanism: Option<RawVolcanism>,
    regions_in: Option<Vec<u8>>,
    regions_not_in: Option<Vec<u8>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawVolcanism {
    Word(String),
    List(Vec<String>),
    Match {
        #[serde(default)]
        contains_any: Vec<String>,
        #[serde(default)]
        equals_any: Vec<String>,
    },
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawRegion {
    #[serde(rename = "in")]
    inside: Option<Vec<u8>>,
    not_in: Option<Vec<u8>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawStar {
    Class(String),
    ClassLuminosity(String, String),
}

fn leak(s: String) -> &'static str {
    // The knowledge base lives for the whole process (LazyLock), so interning its strings is free.
    Box::leak(s.into_boxed_str())
}

fn era(s: &str) -> Era {
    if s.eq_ignore_ascii_case("horizons") { Era::Horizons } else { Era::Odyssey }
}

fn certainty(s: &str) -> Certainty {
    match s.to_ascii_lowercase().as_str() {
        "high" => Certainty::High,
        "medium" => Certainty::Medium,
        _ => Certainty::Low,
    }
}

fn load() -> Kb {
    let genera_file: RawGeneraFile = parse_embedded("genera.json", GENERA_JSON);
    let raw_species: Vec<RawSpecies> = parse_embedded("species.json", SPECIES_JSON);

    let genera = genera_file
        .genera
        .into_iter()
        .map(|(name, g)| Genus {
            name: leak(name),
            colony_range_m: g.colony_range_m,
            codex: g.genus_codex.map(leak),
            codex_prefix: g.codex_prefix.map(leak),
            era: era(&g.era),
            multiple_species_per_body: g.multiple_species_possible_per_body.unwrap_or(false),
        })
        .collect();
    let species = raw_species.into_iter().map(convert_species).collect();
    Kb { genera, species }
}

fn parse_embedded<T: serde::de::DeserializeOwned + Default>(file: &str, text: &str) -> T {
    match serde_json::from_str(text) {
        Ok(v) => v,
        // Embedded at compile time and covered by unit tests; never fails at run time in a tested build.
        Err(e) if cfg!(debug_assertions) => panic!("embedded data/exobiology/{file} is invalid: {e}"),
        Err(_) => T::default(),
    }
}

fn convert_species(r: RawSpecies) -> Species {
    let envelope_base = convert_rule(r.conditions);
    let mut rulesets: Vec<RuleSet> = r.rulesets.into_iter().map(convert_rule).collect();
    let mut envelope = envelope_base.clone();
    add_shared_requirements(&mut envelope, &rulesets);
    fill_open_constraints(&mut rulesets, &envelope_base);
    let colour_basis = match r.color_variants.determined_by.as_str() {
        "star" => ColourBasis::Star,
        "element" => ColourBasis::Element,
        _ => ColourBasis::None,
    };
    Species {
        genus: leak(r.genus),
        name: leak(r.full_name),
        value: r.value,
        codex: leak(r.codex_name_pattern),
        colony_range_m: r.colony_range_m,
        era: era(&r.era),
        certainty: certainty(&r.certainty),
        thargoid: r.category.is_some_and(|c| c.contains("thargoid")),
        value_pre_update14: r.value_pre_update14,
        rulesets,
        envelope,
        crosscheck: r.srvsurvey_crosscheck.map(convert_crosscheck),
        colour_basis,
        variants: r
            .color_variants
            .variants
            .into_iter()
            .map(|v| Variant { key: leak(v.key), colour: leak(v.color), codex: leak(v.codex), in_codex: v.in_game_codex_ref })
            .collect(),
        aliases: r.aliases.into_iter().map(leak).collect(),
    }
}

fn convert_rule(r: RawRule) -> RuleSet {
    let volcanism = match r.volcanism {
        None => VolcanismRule::Any,
        Some(RawVolcanism::Word(w)) => match w.to_ascii_lowercase().as_str() {
            "none" => VolcanismRule::None,
            "required" => VolcanismRule::Required,
            _ => VolcanismRule::Any,
        },
        Some(RawVolcanism::List(items)) => {
            let mut contains = Vec::new();
            let mut equals = Vec::new();
            let mut or_none = false;
            for item in items {
                let item = item.to_ascii_lowercase();
                if item == "none" {
                    or_none = true;
                } else if let Some(exact) = item.strip_prefix('=') {
                    equals.push(exact.to_string());
                } else {
                    contains.push(item);
                }
            }
            VolcanismRule::Match { contains, equals, or_none }
        }
        Some(RawVolcanism::Match { contains_any, equals_any }) => VolcanismRule::Match {
            contains: contains_any.into_iter().map(|s| s.to_ascii_lowercase()).collect(),
            equals: equals_any.into_iter().map(|s| s.to_ascii_lowercase()).collect(),
            or_none: false,
        },
    };
    let stars = |v: Option<Vec<RawStar>>| {
        v.map(|list| {
            list.into_iter()
                .map(|s| match s {
                    RawStar::Class(c) => StarReq { class: c, luminosity: None },
                    RawStar::ClassLuminosity(c, l) => StarReq { class: c, luminosity: Some(l) },
                })
                .collect::<Vec<_>>()
        })
    };
    // Envelope star classes ("B IV") apply to any star in the system (the looser of the two kinds of star rule).
    let envelope_stars = r.star_classes.map(|list| {
        list.into_iter()
            .map(|s| match s.split_once(' ') {
                Some((c, l)) => StarReq { class: c.to_string(), luminosity: Some(l.to_string()) },
                None => StarReq { class: s, luminosity: None },
            })
            .collect::<Vec<_>>()
    });
    RuleSet {
        atmospheres: r.atmospheres,
        body_types: r.body_types,
        min_gravity_g: r.min_gravity_g,
        max_gravity_g: r.max_gravity_g,
        min_temp_k: r.min_temp_k,
        max_temp_k: r.max_temp_k,
        min_pressure_atm: r.min_pressure_atm,
        max_pressure_atm: r.max_pressure_atm,
        volcanism,
        regions: r
            .region_ids
            .map(|g| RegionRule { inside: g.inside, outside: g.not_in })
            .filter(|g| g.inside.is_some() || g.outside.is_some()),
        min_atmosphere_pct: r.min_atmosphere_component_pct.unwrap_or_default().into_iter().collect(),
        star_in_system: stars(r.star_anywhere_in_system).or(envelope_stars),
        parent_star: stars(r.parent_or_primary_star),
        system_body_types: r.system_has_body_type,
        nebula: r.nebula,
        guardian: r.near_guardian_ruins.unwrap_or(false),
        tuber_zones: r.tuber_zones,
        system: r.system,
        max_orbital_period_s: r.max_orbital_period_s,
        min_distance_ls: r.min_distance_from_arrival_ls,
    }
}

fn convert_crosscheck(r: RawSrvSurvey) -> RuleSet {
    // The cross-check's volcanism names are exact journal values without the " volcanism" suffix ("any" = unchecked).
    let volcanism = match r.volcanism {
        Some(RawVolcanism::List(names)) if !names.iter().any(|n| n.eq_ignore_ascii_case("any")) => {
            let or_none = names.iter().any(|n| n.eq_ignore_ascii_case("none"));
            let equals = names
                .iter()
                .filter(|n| !n.eq_ignore_ascii_case("none"))
                .map(|n| format!("{} volcanism", n.to_ascii_lowercase().replace("methanemagma", "methane magma")))
                .collect();
            VolcanismRule::Match { contains: Vec::new(), equals, or_none }
        }
        _ => VolcanismRule::Any,
    };
    let regions = match (r.regions_in, r.regions_not_in) {
        (None, None) => None,
        (inside, outside) => Some(RegionRule { inside, outside }),
    };
    RuleSet {
        atmospheres: r.atmospheres,
        body_types: r.body_types,
        min_gravity_g: r.min_gravity_g,
        max_gravity_g: r.max_gravity_g,
        min_temp_k: r.min_temp_k,
        max_temp_k: r.max_temp_k,
        min_pressure_atm: r.min_pressure_atm,
        max_pressure_atm: r.max_pressure_atm,
        volcanism,
        regions,
        ..RuleSet::default()
    }
}

/// The rule sets leave some constraints open that the merged envelope (its cross-check side) constrains, e.g.
/// atmosphere, body class and gravity for Horizons-era organics. Fill those from the envelope ("constrained side
/// taken").
///
/// Atmosphere and body-class lists are filled per rule set (the envelope only drops classes nobody has seen, e.g.
/// metal-rich bodies for Bacterium Tela). Numeric limits are filled only when *no* rule set of the species
/// constrains them: where some rule sets carry a limit and others don't (Clypeus/Concha Renibus temperature and
/// pressure, Stratum water-atmosphere rules), the omission looks deliberate and is kept.
fn fill_open_constraints(rulesets: &mut [RuleSet], env: &RuleSet) {
    fn fill<T: Clone>(slot: &mut Option<T>, from: &Option<T>, allowed: bool) {
        if allowed && slot.is_none() {
            slot.clone_from(from);
        }
    }
    let none_has = |f: fn(&RuleSet) -> bool| !rulesets.iter().any(f);
    let fill_min_g = none_has(|r| r.min_gravity_g.is_some());
    let fill_max_g = none_has(|r| r.max_gravity_g.is_some());
    let fill_min_t = none_has(|r| r.min_temp_k.is_some());
    let fill_max_t = none_has(|r| r.max_temp_k.is_some());
    let fill_min_p = none_has(|r| r.min_pressure_atm.is_some());
    let fill_max_p = none_has(|r| r.max_pressure_atm.is_some());
    let fill_regions = none_has(|r| r.regions.is_some());
    let fill_volcanism = none_has(|r| r.volcanism != VolcanismRule::Any);
    for rs in rulesets.iter_mut() {
        fill(&mut rs.atmospheres, &env.atmospheres, true);
        fill(&mut rs.body_types, &env.body_types, true);
        fill(&mut rs.min_gravity_g, &env.min_gravity_g, fill_min_g);
        fill(&mut rs.max_gravity_g, &env.max_gravity_g, fill_max_g);
        fill(&mut rs.min_temp_k, &env.min_temp_k, fill_min_t);
        fill(&mut rs.max_temp_k, &env.max_temp_k, fill_max_t);
        fill(&mut rs.min_pressure_atm, &env.min_pressure_atm, fill_min_p);
        fill(&mut rs.max_pressure_atm, &env.max_pressure_atm, fill_max_p);
        fill(&mut rs.regions, &env.regions, fill_regions);
        if fill_volcanism && rs.volcanism == VolcanismRule::Any {
            rs.volcanism = env.volcanism.clone();
        }
    }
}

/// The dataset's `conditions` envelope does not carry system-level requirements (they are prose in `special`).
/// Add those every rule set shares, so the wider envelope cannot bypass them.
fn add_shared_requirements(env: &mut RuleSet, rulesets: &[RuleSet]) {
    if rulesets.is_empty() {
        return;
    }
    fn union<T: Clone + PartialEq>(lists: impl Iterator<Item = Option<Vec<T>>>) -> Option<Vec<T>> {
        let mut out: Vec<T> = Vec::new();
        for list in lists {
            for item in list? {
                if !out.contains(&item) {
                    out.push(item);
                }
            }
        }
        Some(out)
    }
    if env.star_in_system.is_none() {
        env.star_in_system = union(rulesets.iter().map(|r| r.star_in_system.clone()));
    }
    env.parent_star = union(rulesets.iter().map(|r| r.parent_star.clone()));
    env.system_body_types = union(rulesets.iter().map(|r| r.system_body_types.clone()));
    env.tuber_zones = union(rulesets.iter().map(|r| r.tuber_zones.clone()));
    if rulesets.iter().all(|r| r.nebula.is_some()) {
        // "all" (large or planetary nebula) is the looser requirement.
        let any_all = rulesets.iter().any(|r| r.nebula.as_deref() == Some("all"));
        env.nebula = Some(if any_all { "all" } else { "large" }.to_string());
    }
    env.guardian = rulesets.iter().all(|r| r.guardian);
    if rulesets.iter().all(|r| r.system.is_some()) {
        env.system.clone_from(&rulesets[0].system);
    }
    env.min_distance_ls = rulesets.iter().map(|r| r.min_distance_ls).try_fold(f64::INFINITY, |acc, d| d.map(|d| acc.min(d)));
    env.max_orbital_period_s =
        rulesets.iter().map(|r| r.max_orbital_period_s).try_fold(0.0_f64, |acc, d| d.map(|d| acc.max(d)));
    if env.min_atmosphere_pct.is_empty() && rulesets.iter().all(|r| !r.min_atmosphere_pct.is_empty()) {
        // Every rule set requires the same trace gas (Recepta: SO2); keep the most lenient threshold per gas.
        let mut gases: Vec<(String, f64)> = rulesets[0].min_atmosphere_pct.clone();
        for r in &rulesets[1..] {
            gases.retain_mut(|(gas, pct)| match r.min_atmosphere_pct.iter().find(|(g, _)| g == gas) {
                Some((_, p)) => {
                    *pct = pct.min(*p);
                    true
                }
                None => false,
            });
        }
        env.min_atmosphere_pct = gases;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hand-maintained value table this module used before the JSON dataset (Odyssey genera). Kept as an
    /// independent cross-check of `species.json`.
    const HAND_VERIFIED: &[(&str, i64)] = &[
        ("Aleoida Arcus", 7_252_500),
        ("Aleoida Coronamus", 6_284_600),
        ("Aleoida Gravis", 12_934_900),
        ("Aleoida Laminiae", 3_385_200),
        ("Aleoida Spica", 3_385_200),
        ("Bacterium Acies", 1_000_000),
        ("Bacterium Alcyoneum", 1_658_500),
        ("Bacterium Aurasus", 1_000_000),
        ("Bacterium Bullaris", 1_152_500),
        ("Bacterium Cerbrus", 1_689_800),
        ("Bacterium Informem", 8_418_000),
        ("Bacterium Nebulus", 5_289_900),
        ("Bacterium Omentum", 4_638_900),
        ("Bacterium Scopulum", 4_934_500),
        ("Bacterium Tela", 1_949_000),
        ("Bacterium Verrata", 3_897_000),
        ("Bacterium Vesicula", 1_000_000),
        ("Bacterium Volu", 7_774_700),
        ("Cactoida Cortexum", 3_667_600),
        ("Cactoida Lapis", 2_483_600),
        ("Cactoida Peperatis", 2_483_600),
        ("Cactoida Pullulanta", 3_667_600),
        ("Cactoida Vermis", 16_202_800),
        ("Clypeus Lacrimam", 8_418_000),
        ("Clypeus Margaritus", 11_873_200),
        ("Clypeus Speculumi", 16_202_800),
        ("Concha Aureolas", 7_774_700),
        ("Concha Biconcavis", 19_010_800),
        ("Concha Labiata", 2_352_400),
        ("Concha Renibus", 4_572_400),
        ("Electricae Pluma", 6_284_600),
        ("Electricae Radialem", 6_284_600),
        ("Fonticulua Campestris", 1_000_000),
        ("Fonticulua Digitos", 1_804_100),
        ("Fonticulua Fluctus", 20_000_000),
        ("Fonticulua Lapida", 3_111_000),
        ("Fonticulua Segmentatus", 19_010_800),
        ("Fonticulua Upupam", 5_727_600),
        ("Frutexa Acus", 7_774_700),
        ("Frutexa Collum", 1_639_800),
        ("Frutexa Fera", 1_632_500),
        ("Frutexa Flabellum", 1_808_900),
        ("Frutexa Flammasis", 10_326_000),
        ("Frutexa Metallicum", 1_632_500),
        ("Frutexa Sponsae", 5_988_000),
        ("Fumerola Aquatis", 6_284_600),
        ("Fumerola Carbosis", 6_284_600),
        ("Fumerola Extremus", 16_202_800),
        ("Fumerola Nitris", 7_500_900),
        ("Fungoida Bullarum", 3_703_200),
        ("Fungoida Gelata", 3_330_300),
        ("Fungoida Setisis", 1_670_100),
        ("Fungoida Stabitis", 2_680_300),
        ("Osseus Cornibus", 1_483_000),
        ("Osseus Discus", 12_934_900),
        ("Osseus Fractus", 4_027_800),
        ("Osseus Pellebantus", 9_739_000),
        ("Osseus Pumice", 3_156_300),
        ("Osseus Spiralis", 2_404_700),
        ("Recepta Conditivus", 14_313_700),
        ("Recepta Deltahedronix", 16_202_800),
        ("Recepta Umbrux", 12_934_900),
        ("Stratum Araneamus", 2_448_900),
        ("Stratum Cucumisis", 16_202_800),
        ("Stratum Excutitus", 2_448_900),
        ("Stratum Frigus", 2_637_500),
        ("Stratum Laminamus", 2_788_300),
        ("Stratum Limaxus", 1_362_000),
        ("Stratum Paleas", 1_362_000),
        ("Stratum Tectonicas", 19_010_800),
        ("Tubus Cavas", 11_873_200),
        ("Tubus Compagibus", 7_774_700),
        ("Tubus Conifer", 2_415_500),
        ("Tubus Rosarium", 2_637_500),
        ("Tubus Sororibus", 5_727_600),
        ("Tussock Albata", 3_252_500),
        ("Tussock Capillum", 7_025_800),
        ("Tussock Caputus", 3_472_400),
        ("Tussock Catena", 1_766_600),
        ("Tussock Cultro", 1_766_600),
        ("Tussock Divisa", 1_766_600),
        ("Tussock Ignis", 1_849_000),
        ("Tussock Pennata", 5_853_800),
        ("Tussock Pennatis", 1_000_000),
        ("Tussock Propagito", 1_000_000),
        ("Tussock Serrati", 4_447_100),
        ("Tussock Stigmasis", 19_010_800),
        ("Tussock Triticum", 7_774_700),
        // The hand table had 3_277_700 (transposed digits); every value source in the dataset gives 3,227,700.
        ("Tussock Ventusa", 3_227_700),
        ("Tussock Virgam", 14_313_700),
    ];

    const HAND_RANGES: &[(&str, u32)] = &[
        ("Aleoida", 150),
        ("Amphora Plant", 100),
        ("Anemone", 100),
        ("Bacterium", 500),
        ("Bark Mound", 100),
        ("Brain Tree", 100),
        ("Cactoida", 300),
        ("Clypeus", 150),
        ("Concha", 150),
        ("Crystalline Shards", 100),
        ("Electricae", 1000),
        ("Fonticulua", 500),
        ("Frutexa", 150),
        ("Fumerola", 100),
        ("Fungoida", 300),
        ("Osseus", 800),
        ("Recepta", 150),
        ("Sinuous Tubers", 100),
        ("Stratum", 500),
        ("Tubus", 800),
        ("Tussock", 200),
    ];

    #[test]
    fn dataset_loads() {
        assert!(genera().len() >= 22, "genera: {}", genera().len());
        assert!(all_species().len() >= 126, "species: {}", all_species().len());
        for s in all_species() {
            assert!(genus(s.genus).is_some(), "{} has unknown genus {}", s.name, s.genus);
            assert!(s.value > 0, "{} has no value", s.name);
            if !s.thargoid {
                assert!(!s.rulesets.is_empty(), "{} has no rule sets", s.name);
            }
        }
    }

    #[test]
    fn hand_verified_values_match_dataset() {
        for &(name, value) in HAND_VERIFIED {
            let s = species(name).unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!(s.value, value, "{name}");
        }
        let odyssey_sellable = all_species().iter().filter(|s| s.era == Era::Odyssey && !s.thargoid && s.genus != "Radicoida");
        assert_eq!(odyssey_sellable.count(), HAND_VERIFIED.len());
        for &(g, range) in HAND_RANGES {
            assert_eq!(colony_range_m(g), Some(range), "{g}");
        }
    }

    #[test]
    fn lookups() {
        assert_eq!(species("stratum tectonicas").map(|s| s.value), Some(19_010_800));
        assert_eq!(colony_range_m("Bacterium"), Some(500));
        assert_eq!(colony_range_m("Osseus Discus"), Some(800));
        assert_eq!(colony_range_m("Brain Trees"), Some(100));
        assert_eq!(colony_range_m("Bark Mounds"), Some(100));
        assert_eq!(colony_range_m("$Codex_Ent_Tubus_Genus_Name;"), Some(800));
        assert_eq!(colony_range_m("$Codex_Ent_Stratum_07_Name;"), Some(500));
        assert_eq!(genus("$Codex_Ent_Clepeus_Genus_Name;;").map(|g| g.name), Some("Clypeus"));
        assert_eq!(genus("$Codex_Ent_Brancae_Name;").map(|g| g.name), Some("Brain Tree"));
        assert_eq!(species("$Codex_Ent_Stratum_07_Name;").map(|s| s.name), Some("Stratum Tectonicas"));
        assert_eq!(species("Stratum Tectonicas - Lime").map(|s| s.name), Some("Stratum Tectonicas"));
        assert_eq!(species("Radicoida Unicus").map(|s| s.name), Some("Radicoida Unica"));
        let (s, v) = variant_by_codex("$Codex_Ent_Tussocks_04_K_Name;").expect("variant");
        assert_eq!((s.name, v.colour), ("Tussock Cultro", "Green"));
        assert_eq!(species_in_genus("Tussock").count(), 15);
        assert!(species("Not A Species").is_none());
    }

    #[test]
    fn regions() {
        assert_eq!(region_id("Inner Orion Spur"), Some(18));
        assert_eq!(region_id("$Codex_RegionName_18;"), Some(18));
        assert_eq!(region_id("rykers hope"), Some(3));
        assert_eq!(region_id("Galactic Center"), Some(1));
        assert_eq!(region_id("42"), Some(42));
        assert_eq!(region_id("43"), None);
        assert_eq!(region_name(35), Some("Outer Orion Spur"));
    }

    #[test]
    fn rules_are_merged_with_envelope() {
        // The Brain Tree rule set only has temperature/volcanism/region/guardian; atmosphere and body class
        // come from the cross-check side of the envelope.
        let s = species("Roseum Brain Tree").expect("species");
        let rs = &s.rulesets[0];
        assert_eq!(rs.atmospheres.as_deref(), Some(&["None".to_string()][..]));
        assert!(rs.guardian && s.envelope.guardian);
        let radialem = species("Electricae Radialem").expect("species");
        assert_eq!(radialem.envelope.nebula.as_deref(), Some("all"));
        let pluma = species("Electricae Pluma").expect("species");
        assert!(pluma.envelope.parent_star.as_ref().is_some_and(|p| p.iter().any(|s| s.class == "AeBe")));
        let recepta = species("Recepta Umbrux").expect("species");
        assert_eq!(recepta.envelope.min_atmosphere_pct, vec![("SulphurDioxide".to_string(), 1.05)]);
    }
}
