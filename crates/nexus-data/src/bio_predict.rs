//! Exobiology species prediction: which genera, species and colours a body probably hosts, with
//! likelihoods and human-readable reasons.
//!
//! # Model
//!
//! 1. **Species fit.** Every species is scored against each of its rule sets ([`crate::exobio::Species::rulesets`]:
//!    the conditions it has been found in) and keeps its best one. Limits are *soft*: they are observed extremes,
//!    so a value just outside one costs `exp(-distance / tolerance)` instead of rejecting the species; categorical
//!    mismatches (atmosphere type, unrelated body class) cost `exp(-8)`. A field the rule set rejects but the
//!    species' wider cross-check criteria ([`crate::exobio::Species::crosscheck`]) explicitly accept costs only
//!    `exp(-V_WIDENED)` (≈ 0.3, or ≈ 0.09 when those flattened criteria mix several atmospheres). Widening only
//!    applies to rule sets for the body's own atmosphere (each rule set is written for its own atmosphere, so
//!    borrowing another atmosphere's limits would be meaningless); the atmosphere itself is widened only when no rule
//!    set covers it (e.g. Electricae Radialem on methane).
//!    Constraints that cannot be checked (region, nebula, other stars …) multiply the score by a prior instead.
//! 2. **Colour availability.** Star-coloured species need a known variant for one of the body's parent stars and
//!    element-coloured species one of their elements in `Scan.Materials` (the cross-check criteria are built per
//!    variant); otherwise the score is penalised. The same pass yields the predicted colour.
//! 3. **Genus odds.** Per genus, a noisy-OR of its species scores times a genus commonness prior; with the body's
//!    biological signal count `N` these weights become inclusion probabilities that sum to `N` (each capped at
//!    [`PRESENCE_CAP`]). Genera revealed by a DSS scan get probability 1, and only they are considered.
//! 4. **Species odds.** Within a genus the scores are normalised (Odyssey: exactly one species per genus per
//!    body), and `likelihood = P(genus) × P(species | genus)`. Species whose rules fit equally well share the
//!    genus equally (e.g. Stratum Paleas / Laminamus on ammonia worlds in the Orion-Cygnus arm): the data cannot
//!    separate them.
//!
//! Checked against the commander's journals with `cargo run -p nexus-data --example bio_eval`.

use crate::exobio::{self, ColourBasis, RegionRule, RuleSet, Species, StarReq, VolcanismRule};
use serde_json::Value;

pub use crate::exobio::{JOURNAL_ATM_PA, JOURNAL_G_MS2};

// ------------------------------------------------------------------------------------------------
// Public types
// ------------------------------------------------------------------------------------------------

/// A star relevant to a body: a parent star, or another star of its system.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StarFacts {
    /// Journal `StarType` (`"K"`, `"DA"`, `"AeBe"`, `"M_RedGiant"`, …).
    pub star_type: String,
    /// Journal `Luminosity` (`"Va"`, `"IV"`, …).
    pub luminosity: Option<String>,
    /// Journal `Subclass` (0–9).
    pub subclass: Option<u32>,
}

/// What is known about a landable body. Build it with [`BodyFacts::from_journal_scan`] and fill the
/// system-level fields; a missing value only makes the prediction less specific.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodyFacts {
    /// Journal `PlanetClass` (`"Rocky body"`, `"High metal content body"`, `"Icy body"`, …).
    pub planet_class: String,
    /// Journal `AtmosphereType` (`"CarbonDioxide"`, `"NeonRich"`; `None`/`"None"` = airless).
    pub atmosphere_type: Option<String>,
    /// Journal `Atmosphere` text ("thin carbon dioxide atmosphere"); used when `atmosphere_type` is missing.
    pub atmosphere: Option<String>,
    /// Journal `AtmosphereComposition` as (gas, percent).
    pub atmosphere_composition: Vec<(String, f64)>,
    /// `SurfaceGravity / JOURNAL_G_MS2`.
    pub gravity_g: Option<f64>,
    /// `SurfaceTemperature`, K.
    pub temperature_k: Option<f64>,
    /// `SurfacePressure / JOURNAL_ATM_PA`.
    pub pressure_atm: Option<f64>,
    /// Journal `Volcanism` text (`None`/empty = no volcanism).
    pub volcanism: Option<String>,
    pub landable: bool,
    /// Journal `Materials` as (element, percent).
    pub materials: Vec<(String, f64)>,
    /// Stars that can decide colour, nearest first: the body's parent stars, then the system's primary star
    /// (see [`resolve_parent_stars`]).
    pub parent_stars: Vec<StarFacts>,
    pub distance_from_arrival_ls: f64,
    pub system_name: String,
    /// Galactic coordinates (`StarPos`), for Guardian bubbles and Sinuous-tuber zones.
    pub star_pos: Option<[f64; 3]>,
    /// Galactic region: name ("Inner Orion Spur"), `$Codex_RegionName_18;` or id.
    pub region: Option<String>,
    /// Biological signals from `FSSBodySignals` / `SAASignalsFound` (bounds the number of genera).
    pub bio_signal_count: Option<u32>,
    /// Other stars of the system ("some star in the system" rules: Anemones, Amphora Plant, Crystalline Shards).
    pub system_stars: Vec<StarFacts>,
    /// `PlanetClass` of the system's scanned bodies (Amphora Plant, Brain Trees, Crystalline Shards); empty = unknown.
    pub system_body_classes: Vec<String>,
    /// `OrbitalPeriod`, s (some Sinuous-tuber rules).
    pub orbital_period_s: Option<f64>,
    /// Within ~150 ly of a nebula (Electricae Radialem, Bark Mounds); `None` = unknown (no nebula catalogue here).
    pub near_nebula: Option<bool>,
}

/// One candidate species for a body.
#[derive(Debug, Clone, PartialEq)]
pub struct Prediction {
    pub genus: String,
    pub species: String,
    /// Vista Genomics base value.
    pub value: i64,
    pub colony_range_m: u32,
    /// Probability that this species is on the body (genus odds × species share), 0..1.
    pub likelihood: f32,
    /// Predicted colour ("Lime"), when it can be determined.
    pub variant: Option<String>,
    pub reasons: Vec<String>,
}

/// One element of a journal `Scan.Parents` array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentRef {
    Star(u32),
    Planet(u32),
    /// `"Null"`: a barycentre.
    Barycentre(u32),
    Ring(u32),
}

/// A star of the body's system, for [`resolve_parent_stars`].
#[derive(Debug, Clone, PartialEq)]
pub struct SystemStar {
    pub body_id: u32,
    /// First element of the star's own `Parents` (what it orbits).
    pub parent: Option<ParentRef>,
    pub distance_ls: f64,
    pub facts: StarFacts,
}

// ------------------------------------------------------------------------------------------------
// Journal helpers
// ------------------------------------------------------------------------------------------------

impl StarFacts {
    pub fn new(star_type: impl Into<String>, luminosity: Option<&str>) -> Self {
        Self { star_type: star_type.into(), luminosity: luminosity.map(str::to_string), subclass: None }
    }

    /// From a journal `Scan` event of a star (`None` for planets).
    pub fn from_journal_scan(scan: &Value) -> Option<Self> {
        Some(Self {
            star_type: scan.get("StarType")?.as_str()?.to_string(),
            luminosity: scan.get("Luminosity").and_then(Value::as_str).map(str::to_string),
            subclass: scan.get("Subclass").and_then(Value::as_u64).and_then(|s| u32::try_from(s).ok()),
        })
    }

    /// Colour key used by star-coloured species (`O B A F G K M L T Y TTS Ae W D N`), if this class has one.
    pub fn colour_key(&self) -> Option<&'static str> {
        star_colour_key(&self.star_type)
    }
}

impl BodyFacts {
    /// Planet-level facts from a journal `Scan` event (`None` for stars and belt clusters). System-level fields
    /// (`parent_stars`, `star_pos`, `region`, `bio_signal_count`, `system_*`, `near_nebula`) are left to the caller.
    pub fn from_journal_scan(scan: &Value) -> Option<Self> {
        let planet_class = scan.get("PlanetClass")?.as_str()?.to_string();
        let text = |k: &str| {
            scan.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
        };
        let num = |k: &str| scan.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
        let pairs = |k: &str| -> Vec<(String, f64)> {
            scan.get(k)
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|m| Some((m.get("Name")?.as_str()?.to_string(), m.get("Percent")?.as_f64()?)))
                        .collect()
                })
                .unwrap_or_default()
        };
        Some(Self {
            planet_class,
            atmosphere_type: text("AtmosphereType"),
            atmosphere: text("Atmosphere"),
            atmosphere_composition: pairs("AtmosphereComposition"),
            gravity_g: num("SurfaceGravity").map(|g| g / JOURNAL_G_MS2),
            temperature_k: num("SurfaceTemperature"),
            pressure_atm: num("SurfacePressure").map(|p| p / JOURNAL_ATM_PA),
            volcanism: text("Volcanism"),
            landable: scan.get("Landable").and_then(Value::as_bool).unwrap_or(false),
            materials: pairs("Materials"),
            distance_from_arrival_ls: num("DistanceFromArrivalLS").unwrap_or(0.0),
            system_name: text("StarSystem").unwrap_or_default(),
            orbital_period_s: num("OrbitalPeriod"),
            ..Self::default()
        })
    }
}

impl ParentRef {
    pub fn id(self) -> u32 {
        match self {
            Self::Star(i) | Self::Planet(i) | Self::Barycentre(i) | Self::Ring(i) => i,
        }
    }

    /// Parses a journal `Scan.Parents` array (`[{"Planet":3},{"Null":1},{"Star":0}]`), nearest first.
    pub fn parse_journal(parents: &Value) -> Vec<ParentRef> {
        let Some(items) = parents.as_array() else { return Vec::new() };
        items
            .iter()
            .filter_map(|p| {
                let (kind, id) = p.as_object()?.iter().next()?;
                let id = u32::try_from(id.as_u64()?).ok()?;
                Some(match kind.as_str() {
                    "Star" => Self::Star(id),
                    "Planet" => Self::Planet(id),
                    "Ring" => Self::Ring(id),
                    _ => Self::Barycentre(id),
                })
            })
            .collect()
    }
}

/// Stars that can decide a star-coloured species' variant, nearest first: the stars of the body's `Parents`
/// chain (for a barycentre, the stars orbiting it directly, brightest class first: the brightest parent star is
/// taken to decide), followed by the system's primary star (the rule sets' star requirements accept it too).
pub fn resolve_parent_stars(parents: &[ParentRef], stars: &[SystemStar]) -> Vec<StarFacts> {
    fn push<'a>(picked: &mut Vec<&'a SystemStar>, star: &'a SystemStar) {
        if !picked.iter().any(|p| p.body_id == star.body_id) {
            picked.push(star);
        }
    }
    let mut picked: Vec<&SystemStar> = Vec::new();
    for parent in parents {
        match *parent {
            ParentRef::Star(id) => {
                if let Some(s) = stars.iter().find(|s| s.body_id == id) {
                    push(&mut picked, s);
                }
            }
            ParentRef::Barycentre(id) => {
                let mut orbiting: Vec<&SystemStar> =
                    stars.iter().filter(|s| s.parent == Some(ParentRef::Barycentre(id))).collect();
                orbiting.sort_by_key(|s| brightness_rank(&s.facts.star_type));
                for s in orbiting {
                    push(&mut picked, s);
                }
            }
            ParentRef::Planet(_) | ParentRef::Ring(_) => {}
        }
    }
    let primary = stars
        .iter()
        .find(|s| s.body_id == 0)
        .or_else(|| stars.iter().filter(|s| s.distance_ls <= 0.0).min_by_key(|s| s.body_id));
    if let Some(p) = primary {
        push(&mut picked, p);
    }
    picked.into_iter().map(|s| s.facts.clone()).collect()
}

// ------------------------------------------------------------------------------------------------
// Bonus and value helpers
// ------------------------------------------------------------------------------------------------

/// Whether Vista Genomics will probably pay the first-logged bonus (×5 in total) for a species on a body.
///
/// Likely when the system is unpopulated, nobody had landed on the body yet (`Scan.WasFootfalled == false`, from
/// the scan made before your own landing), and the species is not logged there (`ScanOrganic.WasLogged`, the latest
/// value seen for that species on that body; `None` before any scan).
///
/// `WasLogged = false` proves nothing on its own: across one commander's sales (2026-10) it was false for 67 species
/// and the bonus came for 2, never in a populated system (0 of 46). `WasLogged = true` always meant no bonus (it can
/// flip while you sample if another commander sells first). The 2 paid ones were on a body someone had set foot on,
/// where a third species on the same body paid none: the bonus is per species, so a footfalled body can still pay
/// for a species nobody sold there, but nothing in the journal tells which one.
///
/// MEDIUM certainty: only the sale (`SellOrganicData.Bonus`) confirms it.
pub fn first_logged_bonus_likely(body_was_footfalled: Option<bool>, system_populated: bool, was_logged: Option<bool>) -> bool {
    was_logged != Some(true) && !system_populated && body_was_footfalled == Some(false)
}

/// Vista Genomics payout for one species: base value, ×5 with the first-logged bonus.
pub fn payout(base_value: i64, first_logged_bonus: bool) -> i64 {
    if first_logged_bonus { base_value * (1 + exobio::FIRST_LOGGED_BONUS_FACTOR) } else { base_value }
}

/// Likelihood-weighted payout of a prediction (summed over a body's predictions: the expected body value).
pub fn expected_value(prediction: &Prediction, bonus: bool) -> i64 {
    let likelihood = if prediction.likelihood.is_finite() { prediction.likelihood.clamp(0.0, 1.0) } else { 0.0 };
    (payout(prediction.value, bonus) as f64 * f64::from(likelihood)).round() as i64
}

// ------------------------------------------------------------------------------------------------
// Tunables (see module docs). Violation weights are in nats: a constraint costs `exp(-v)`.
// ------------------------------------------------------------------------------------------------

/// Maximum presence probability of a genus not confirmed by DSS (rules can be wrong).
pub const PRESENCE_CAP: f64 = 0.97;
/// Species scoring below this are not candidates (unless their genus was revealed by DSS).
const MIN_SCORE: f64 = 0.02;
/// Species with less than this share of their genus are dropped (the genus' best species is always kept).
const MIN_SHARE: f64 = 0.03;
/// Predictions below this likelihood are dropped (except the best species of a DSS-confirmed genus).
const MIN_LIKELIHOOD: f64 = 0.01;

/// Cost of a field the rule set rejects but the cross-check criteria explicitly accept (exp(-1.2) ≈ 0.3).
const V_WIDENED: f64 = 1.2;
/// Same, when the flattened cross-check criteria span several atmospheres: their limits then mix different leaves
/// of the criteria tree (e.g. Osseus Discus' "no volcanism" comes from its water-atmosphere leaf), so they are
/// weaker evidence for any one rule set (exp(-2.4) ≈ 0.09).
const V_WIDENED_MIXED: f64 = 2.4;
const V_HARD: f64 = 8.0;
/// X vs X-rich atmosphere that neither the rule sets nor the cross-check criteria allow for the species
/// (exp(-3) ≈ 0.05).
const V_RICH: f64 = 3.0;
/// Rocky vs HMC (or icy vs rocky-ice) that neither source allows for the species (exp(-4.5) ≈ 0.01): the sources
/// disagree on these splits, which widening already covers, so a class neither allows is near-impossible.
const V_RELATED_BODY: f64 = 4.5;
const V_VOLCANISM: f64 = 3.0;
const V_REGION: f64 = 5.0;
const V_STAR: f64 = 3.0;
const V_SYSTEM_BODY: f64 = 2.5;
const V_NEBULA: f64 = 4.0;
const V_ZONE: f64 = 6.0;
const V_SYSTEM: f64 = 20.0;
const V_ORBIT: f64 = 3.0;
const V_COLOUR: f64 = 2.0;
const V_AIRLESS: f64 = 1.0;
const V_NUMERIC_CAP: f64 = 8.0;

/// Priors for constraints that cannot be checked. LOW certainty, chosen so that e.g. three arm-specific Tussock
/// species stay tied when the region is unknown while region-free species are not penalised.
const U_REGION_IN: f64 = 0.5;
const U_REGION_OUT: f64 = 0.8;
const U_NEBULA: f64 = 0.3;
const U_ZONE: f64 = 0.25;
const U_STAR: f64 = 0.5;
const U_SYSTEM_BODY: f64 = 0.5;
const U_COMPOSITION: f64 = 0.7;
const U_ORBIT: f64 = 0.7;

/// Temperature tolerance around a limit (K). Limits are observed extremes, so their uncertainty scales with the
/// width of the observed range (expected gap ≈ width / (samples + 1)): narrow generator bands are sharp (the Tussock
/// CO2 species tile 145–197 K in 3–10 K bands), broad ranges less so. The 0.5 K base covers the integer rounding of
/// the published limits (a body at 170.5 K sits between Tussock Ignis ≤ 170 and Serrati ≥ 171 and must be one of
/// them). One-sided limits scale with the limit.
fn tol_temp(limit: f64, width: Option<f64>) -> f64 {
    match width {
        Some(w) => 0.5 + 0.02 * w,
        None => 0.75 + 0.005 * limit.abs(),
    }
}

/// Gravity tolerance around a limit (g), same idea (the two sources' usual caps, 0.276 and 0.2817 g, differ by
/// ~2 %).
fn tol_gravity(limit: f64, width: Option<f64>) -> f64 {
    match width {
        Some(w) => 0.003 + 0.03 * w,
        None => 0.005 + 0.02 * limit.abs(),
    }
}

/// Pressure tolerance, relative (ln units: 0.1 ≈ 10 %).
const TOL_PRESSURE_LN: f64 = 0.1;
/// Trace-gas tolerance, percentage points (Recepta's SO2 ≥ 1.05 % must reject the common 0.99 % CO2 mix).
const TOL_COMPOSITION_PCT: f64 = 0.02;
/// Distance-from-arrival tolerance, relative.
const TOL_DISTANCE_REL: f64 = 0.05;
/// Pressure treated as airless, atm.
const AIRLESS_ATM: f64 = 1e-6;

/// Genus commonness prior, used only to rank genera before DSS when a body has more candidate genera than
/// biological signals. LOW certainty: Bacterium is the most widespread genus, Stratum and Fonticulua are very common
/// and Horizons-era organics are rare, but there are no published counts.
fn genus_prior(genus: &str) -> f64 {
    match genus {
        "Bacterium" | "Radicoida" => 1.0,
        "Stratum" | "Fonticulua" => 0.8,
        "Tussock" | "Osseus" | "Fungoida" | "Frutexa" | "Concha" | "Tubus" | "Aleoida" | "Cactoida" => 0.5,
        "Clypeus" | "Recepta" | "Fumerola" | "Electricae" => 0.35,
        _ => 0.15,
    }
}

// ------------------------------------------------------------------------------------------------
// Prediction
// ------------------------------------------------------------------------------------------------

/// Candidate species for a body, most likely first (ties: higher value first).
///
/// `known_genera` are the genera revealed by a DSS scan (`SAASignalsFound.Genuses[].Genus` symbols or English
/// names): when non-empty, only those genera are predicted and each gets at least one candidate (the nearest
/// species when no rule fits). Otherwise `facts.bio_signal_count` bounds the expected number of genera; when it
/// is unknown, likelihoods are conditional on the body having life.
pub fn predict(facts: &BodyFacts, known_genera: &[String]) -> Vec<Prediction> {
    if !facts.landable || (known_genera.is_empty() && facts.bio_signal_count == Some(0)) {
        return Vec::new();
    }
    let ctx = Ctx::new(facts);

    let mut known: Vec<&'static str> = Vec::new();
    let mut unknown_genera: Vec<&str> = Vec::new();
    for g in known_genera {
        match exobio::genus(g) {
            Some(genus) if !known.contains(&genus.name) => known.push(genus.name),
            Some(_) => {}
            None => unknown_genera.push(g.as_str()),
        }
    }
    let restricted = !known_genera.is_empty();

    let mut groups: Vec<GenusGroup> = Vec::new();
    for genus in exobio::genera() {
        let is_known = known.contains(&genus.name);
        if restricted && !is_known {
            continue;
        }
        let mut scored: Vec<Scored> =
            exobio::species_in_genus(genus.name).filter(|s| !s.thargoid).map(|s| score_species(s, &ctx)).collect();
        scored.sort_by(|a, b| b.score.total_cmp(&a.score));
        let Some(best) = scored.first().map(|s| s.score) else { continue };
        let weak = best < MIN_SCORE;
        let candidates: Vec<Scored> = if !weak {
            scored.into_iter().filter(|s| s.score >= MIN_SCORE).collect()
        } else if is_known {
            // DSS says the genus is here but no rule fits: keep the nearest species (within 20× of the best).
            let floor = best / 20.0;
            scored.into_iter().take_while(|s| s.score >= floor).take(3).collect()
        } else {
            continue;
        };
        let strength = 1.0 - candidates.iter().map(|s| 1.0 - s.score.min(0.999)).product::<f64>();
        groups.push(GenusGroup {
            name: genus.name,
            weight: genus_prior(genus.name) * strength,
            presence: 0.0,
            known: is_known,
            weak,
            multiple: genus.multiple_species_per_body,
            species: candidates,
        });
    }

    // DSS genera without any predictable species (e.g. the Thargoid sample group) still get a placeholder.
    for name in &known {
        if !groups.iter().any(|g| g.name == *name) {
            unknown_genera.push(name);
        }
    }

    if restricted {
        for g in &mut groups {
            g.presence = 1.0;
        }
    } else {
        let n = match facts.bio_signal_count {
            Some(n) => f64::from(n),
            None => (groups.len() as f64 * 0.5).clamp(1.0, 3.0),
        };
        let weights: Vec<f64> = groups.iter().map(|g| g.weight).collect();
        for (g, p) in groups.iter_mut().zip(inclusion_probabilities(&weights, n, PRESENCE_CAP)) {
            g.presence = p;
        }
    }
    let n_candidates = groups.iter().filter(|g| g.presence >= MIN_LIKELIHOOD).count();

    let mut out: Vec<Prediction> = Vec::new();
    for g in &groups {
        let total: f64 = g.species.iter().map(|s| s.score).sum();
        let genus_reason = if g.known {
            "genus confirmed by DSS".to_string()
        } else {
            match facts.bio_signal_count {
                Some(n) => format!(
                    "genus {:.0}% ({n} bio signal{}, {n_candidates} candidate genera)",
                    g.presence * 100.0,
                    if n == 1 { "" } else { "s" }
                ),
                None => format!("genus {:.0}% if the body has life (signal count unknown)", g.presence * 100.0),
            }
        };
        for (i, s) in g.species.iter().enumerate() {
            let share = if total > 0.0 { s.score / total } else { 1.0 / g.species.len() as f64 };
            let likelihood = g.presence * share;
            let keep_anyway = i == 0 && g.known;
            if !keep_anyway && (share < MIN_SHARE || likelihood < MIN_LIKELIHOOD) {
                continue;
            }
            let (explained, colour) = s.explain(&ctx, g.species.len(), g.weak);
            let mut reasons = vec![genus_reason.clone()];
            reasons.extend(explained);
            if g.multiple && g.species.len() > 1 {
                // Brain Trees / Sinuous Tubers: several species can share a body, so shares are lower bounds.
                reasons.push(format!("several {} species can share one body", g.name));
            }
            out.push(Prediction {
                genus: g.name.to_string(),
                species: s.species.name.to_string(),
                value: s.species.value,
                colony_range_m: s.species.colony_range_m,
                likelihood: likelihood as f32,
                variant: colour.single().map(str::to_string),
                reasons,
            });
        }
    }
    for g in unknown_genera {
        out.push(Prediction {
            genus: exobio::genus(g).map_or(g, |genus| genus.name).to_string(),
            species: format!("{g} (unknown species)"),
            value: 0,
            colony_range_m: exobio::colony_range_m(g).unwrap_or(0),
            likelihood: 1.0,
            variant: None,
            reasons: vec!["genus confirmed by DSS but no predictable species in the exobiology dataset".to_string()],
        });
    }
    out.sort_by(|a, b| {
        b.likelihood.total_cmp(&a.likelihood).then(b.value.cmp(&a.value)).then_with(|| a.species.cmp(&b.species))
    });
    out
}

/// Probabilities `p_i = min(cap, λ·w_i)` with `Σ p_i = min(n, cap·k)` ("water filling"): how likely each of `k`
/// candidates is to be among the `n` present when candidates are drawn in proportion to their weights.
fn inclusion_probabilities(weights: &[f64], n: f64, cap: f64) -> Vec<f64> {
    let mut p = vec![0.0; weights.len()];
    let mut free: Vec<usize> = (0..weights.len()).filter(|&i| weights[i] > 0.0).collect();
    let mut remaining = n.min(cap * free.len() as f64);
    while remaining > 1e-12 && !free.is_empty() {
        let wsum: f64 = free.iter().map(|&i| weights[i]).sum();
        let lambda = remaining / wsum;
        let capped: Vec<usize> = free.iter().copied().filter(|&i| lambda * weights[i] >= cap).collect();
        if capped.is_empty() {
            for &i in &free {
                p[i] = lambda * weights[i];
            }
            break;
        }
        for &i in &capped {
            p[i] = cap;
            remaining -= cap;
        }
        free.retain(|i| !capped.contains(i));
    }
    p
}

struct GenusGroup<'s> {
    name: &'static str,
    weight: f64,
    presence: f64,
    known: bool,
    /// DSS-confirmed but no rule fits: candidates are only the nearest misses.
    weak: bool,
    /// Several species of the genus can share a body (Horizons-era Brain Trees, Sinuous Tubers).
    multiple: bool,
    species: Vec<Scored<'s>>,
}

// ------------------------------------------------------------------------------------------------
// Species scoring
// ------------------------------------------------------------------------------------------------

/// Normalised body facts shared by all rule evaluations.
struct Ctx<'a> {
    facts: &'a BodyFacts,
    /// `AtmosphereType`, `"None"` when airless.
    atmosphere: String,
    /// Lower-case `Volcanism`, empty when none.
    volcanism: String,
    region: Option<u8>,
    /// Lower-case element names present on the body.
    materials: Vec<String>,
    /// Inside a Guardian bubble (`None` without `star_pos`).
    guardian: Option<bool>,
    /// Sinuous-tuber zones whose band contains the system (`None` without `star_pos`).
    tuber_zones: Option<Vec<&'static str>>,
}

impl<'a> Ctx<'a> {
    fn new(facts: &'a BodyFacts) -> Self {
        let volcanism = facts.volcanism.as_deref().unwrap_or("").trim().to_ascii_lowercase();
        let volcanism = if volcanism == "no volcanism" || volcanism == "none" { String::new() } else { volcanism };
        Self {
            facts,
            atmosphere: normalise_atmosphere(facts),
            volcanism,
            region: facts.region.as_deref().and_then(exobio::region_id),
            materials: facts.materials.iter().filter(|(_, p)| *p > 0.0).map(|(m, _)| m.to_ascii_lowercase()).collect(),
            guardian: facts.star_pos.map(|p| GUARDIAN_BUBBLES.iter().any(|b| dist(p, b.pos) <= b.radius_ly)),
            tuber_zones: facts.star_pos.map(|p| {
                TUBER_ZONES.iter().filter(|z| (z.min_ly..=z.max_ly).contains(&dist(p, z.pos))).map(|z| z.name).collect()
            }),
        }
    }

    fn airless(&self) -> bool {
        self.atmosphere == "None"
    }
}

/// `AtmosphereType`, falling back to the `Atmosphere` text ("thin carbon dioxide-rich atmosphere" →
/// "CarbonDioxideRich"); `"None"` when airless.
fn normalise_atmosphere(facts: &BodyFacts) -> String {
    if let Some(t) = facts.atmosphere_type.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        return if t.eq_ignore_ascii_case("none") || t.eq_ignore_ascii_case("noatmosphere") {
            "None".to_string()
        } else {
            t.to_string()
        };
    }
    let Some(text) = facts.atmosphere.as_deref() else { return "None".to_string() };
    let mut t = text.trim().to_ascii_lowercase();
    for prefix in ["hot ", "thick ", "thin "] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.to_string();
        }
    }
    let t = t.trim_end_matches(" atmosphere").trim();
    if t.is_empty() || t == "no" || t == "none" {
        return "None".to_string();
    }
    let (base, rich) = match t.strip_suffix("-rich").or_else(|| t.strip_suffix(" rich")) {
        Some(b) => (b, true),
        None => (t, false),
    };
    let mut out: String = base
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect();
    if rich {
        out.push_str("Rich");
    }
    out
}

/// Result of evaluating one rule set. Messages are only built when `explain` is set: scoring every rule set of
/// every species with messages would dominate the cost of `predict`.
#[derive(Debug, Clone)]
struct Fit {
    v: f64,
    u: f64,
    /// Cost of a field widened by the cross-check criteria.
    widen_cost: f64,
    explain: bool,
    misses: Vec<String>,
    unknowns: Vec<String>,
}

impl Fit {
    fn new(widen_cost: f64, explain: bool) -> Self {
        Self { v: 0.0, u: 1.0, widen_cost, explain, misses: Vec::new(), unknowns: Vec::new() }
    }

    fn score(&self) -> f64 {
        self.u * (-self.v).exp()
    }

    fn miss(&mut self, v: f64, why: impl FnOnce() -> String) {
        // Guard against NaN/negative inputs: a violation never raises the score.
        let v = if v.is_finite() { v.max(0.0) } else { V_NUMERIC_CAP };
        self.v += v;
        if self.explain {
            self.misses.push(why());
        }
    }

    /// A field the rule set rejects (`v_rule`); `v_cross` is the violation against the cross-check criteria when
    /// they explicitly constrain that field (None = they don't check it, so no evidence either way).
    fn field(&mut self, v_rule: f64, v_cross: Option<f64>, why: impl FnOnce() -> String) {
        if v_rule <= 0.0 {
            return;
        }
        match v_cross {
            Some(vc) if self.widen_cost + vc < v_rule => {
                // Outside the rule set but inside the wider criteria: a softer miss, and the reason says why.
                self.miss(self.widen_cost + vc, || format!("{} (a wider set of limits allows it)", why()));
            }
            _ => self.miss(v_rule, why),
        }
    }

    fn unknown(&mut self, u: f64, why: impl FnOnce() -> String) {
        self.u *= u;
        if self.explain {
            self.unknowns.push(why());
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Colour {
    /// Species without variants.
    NotApplicable,
    /// No parent-star / material data.
    Unknown,
    /// (colour, why); further entries are alternatives (farther stars).
    Known(Vec<(&'static str, String)>),
    /// Several of the species' elements are present.
    Ambiguous(Vec<(&'static str, String)>),
    /// None of the available stars / elements has a variant.
    Unavailable(String),
}

impl Colour {
    fn single(&self) -> Option<&'static str> {
        match self {
            Self::Known(options) => options.first().map(|(c, _)| *c),
            _ => None,
        }
    }
}

struct Scored<'s> {
    species: &'s Species,
    score: f64,
    /// Index of the best rule set.
    rule: usize,
    /// Cross-check criteria used to widen that rule set, and the widening cost.
    cross: Option<&'s RuleSet>,
    widen_cost: f64,
}

impl Scored<'_> {
    /// Human-readable reasons, and the colour. Built only for the predictions returned (re-evaluates the chosen
    /// rule set with messages).
    fn explain(&self, ctx: &Ctx, genus_candidates: usize, weak: bool) -> (Vec<String>, Colour) {
        let sp = self.species;
        let mut out = Vec::new();
        let mut fit = match sp.rulesets.get(self.rule) {
            Some(rs) => eval_rule(rs, self.cross, self.widen_cost, ctx, true),
            None => Fit::new(self.widen_cost, true),
        };
        let colour = colour_for(sp, ctx);
        if let Colour::Unavailable(why) = &colour {
            fit.misses.push(why.clone());
        }
        if let Some(rs) = sp.rulesets.get(self.rule) {
            // What the rule checks: the conditions the species has been found in. A fit is no find: the likelihood
            // says how sure the prediction is.
            let n = sp.rulesets.len();
            let verb = if fit.misses.is_empty() { "fits" } else { "closest to" };
            out.push(format!("{verb} the conditions it has been found in (rule {} of {n}): {}", self.rule + 1, summarise(rs)));
        }
        if !fit.misses.is_empty() {
            out.push(format!("{}: {}", if weak { "no rule fits" } else { "near miss" }, fit.misses.join("; ")));
        }
        if !fit.unknowns.is_empty() {
            out.push(format!("unverified: {}", fit.unknowns.join("; ")));
        }
        match &colour {
            Colour::Known(options) => {
                let mut s = format!("colour {} ({})", options[0].0, options[0].1);
                if options.len() > 1 {
                    let alt: Vec<String> = options[1..].iter().map(|(c, w)| format!("{c} ({w})")).collect();
                    s.push_str(&format!(", or {}", alt.join(", ")));
                }
                out.push(s);
            }
            Colour::Ambiguous(options) => {
                let alt: Vec<String> = options.iter().map(|(c, w)| format!("{c} ({w})")).collect();
                out.push(format!("colour {}", alt.join(" or ")));
            }
            Colour::Unknown => out.push(match sp.colour_basis {
                ColourBasis::Star => "colour unknown (parent star not scanned)".to_string(),
                _ => "colour unknown (materials not known)".to_string(),
            }),
            Colour::Unavailable(_) | Colour::NotApplicable => {}
        }
        if genus_candidates > 1 {
            out.push(format!("1 of {genus_candidates} candidate {} species", sp.genus));
        }
        if sp.certainty == exobio::Certainty::Low {
            out.push("low-certainty data".to_string());
        }
        (out, colour)
    }
}

fn score_species<'s>(sp: &'s Species, ctx: &Ctx) -> Scored<'s> {
    let atmosphere_ok = |rs: &RuleSet| rs.atmospheres.as_ref().is_none_or(|l| v_atmosphere(l, ctx) == 0.0);
    let covered = sp.rulesets.iter().any(atmosphere_ok);
    let widen_cost = match sp.crosscheck.as_ref().and_then(|s| s.atmospheres.as_ref()) {
        Some(list) if list.len() > 1 => V_WIDENED_MIXED,
        _ => V_WIDENED,
    };
    let (rule, cross, mut fit) = sp
        .rulesets
        .iter()
        .enumerate()
        .map(|(i, rs)| {
            // The cross-check criteria may widen the rule set for the body's atmosphere, or any rule set when none
            // covers it.
            let cross = sp.crosscheck.as_ref().filter(|_| !covered || atmosphere_ok(rs));
            (i, cross, eval_rule(rs, cross, widen_cost, ctx, false))
        })
        .max_by(|a, b| a.2.score().total_cmp(&b.2.score()).then(b.0.cmp(&a.0)))
        .unwrap_or((0, None, Fit { v: V_HARD, ..Fit::new(widen_cost, false) }));
    if colour_available(sp, ctx) == Some(false) {
        fit.miss(V_COLOUR, String::new);
    }
    Scored { species: sp, score: fit.score(), rule, cross, widen_cost }
}

/// Cheap colour check used while scoring: `Some(false)` when none of the body's parent stars / materials has a
/// known variant of the species, `None` when unknown or not applicable.
fn colour_available(sp: &Species, ctx: &Ctx) -> Option<bool> {
    match sp.colour_basis {
        ColourBasis::None => None,
        _ if sp.variants.is_empty() => None,
        ColourBasis::Star => {
            let mut keyed = false;
            for key in ctx.facts.parent_stars.iter().filter_map(StarFacts::colour_key) {
                if sp.variant(key).is_some() {
                    return Some(true);
                }
                keyed = true;
            }
            keyed.then_some(false)
        }
        ColourBasis::Element if ctx.materials.is_empty() => None,
        ColourBasis::Element => {
            Some(sp.variants.iter().any(|v| ctx.materials.iter().any(|m| m.eq_ignore_ascii_case(v.key))))
        }
    }
}

fn colour_for(sp: &Species, ctx: &Ctx) -> Colour {
    match sp.colour_basis {
        ColourBasis::None => Colour::NotApplicable,
        _ if sp.variants.is_empty() => Colour::NotApplicable,
        ColourBasis::Star => {
            let mut keyed = Vec::new();
            let mut options: Vec<(&'static str, String)> = Vec::new();
            for star in &ctx.facts.parent_stars {
                let Some(key) = star.colour_key() else { continue };
                keyed.push(key);
                if let Some(v) = sp.variant(key)
                    && !options.iter().any(|(c, _)| *c == v.colour)
                {
                    options.push((v.colour, format!("{} star", star.star_type)));
                }
            }
            if keyed.is_empty() {
                Colour::Unknown
            } else if options.is_empty() {
                Colour::Unavailable(format!("no known {} variant for {} stars", sp.name, keyed.join("/")))
            } else {
                Colour::Known(options)
            }
        }
        ColourBasis::Element => {
            if ctx.materials.is_empty() {
                return Colour::Unknown;
            }
            let options: Vec<(&'static str, String)> = sp
                .variants
                .iter()
                .filter(|v| ctx.materials.iter().any(|m| m.eq_ignore_ascii_case(v.key)))
                .map(|v| (v.colour, v.key.to_ascii_lowercase()))
                .collect();
            match options.len() {
                0 => Colour::Unavailable(format!(
                    "none of {} in materials",
                    sp.variants.iter().map(|v| v.key).collect::<Vec<_>>().join("/")
                )),
                1 => Colour::Known(options),
                _ => Colour::Ambiguous(options),
            }
        }
    }
}

fn eval_rule(rs: &RuleSet, cross: Option<&RuleSet>, widen_cost: f64, ctx: &Ctx, explain: bool) -> Fit {
    let f = ctx.facts;
    let mut fit = Fit::new(widen_cost, explain);

    // Body-level fields: the rule set, widened where the cross-check criteria explicitly accept the value.
    if let Some(list) = &rs.atmospheres {
        let v = v_atmosphere(list, ctx);
        let vs = cross.and_then(|s| s.atmospheres.as_ref()).map(|l| v_atmosphere(l, ctx));
        fit.field(v, vs, || format!("{} atmosphere, needs {}", ctx.atmosphere, list.join("/")));
    }
    if let Some(list) = &rs.body_types {
        let v = v_body(list, &f.planet_class);
        let vs = cross.and_then(|s| s.body_types.as_ref()).map(|l| v_body(l, &f.planet_class));
        let needs: Vec<&str> = list.iter().map(|b| body_abbrev(b)).collect();
        fit.field(v, vs, || format!("{}, needs {}", body_abbrev(&f.planet_class), needs.join("/")));
    }
    if let Some(g) = f.gravity_g {
        range_field(&mut fit, g, (rs.min_gravity_g, rs.max_gravity_g), cross.map(|s| (s.min_gravity_g, s.max_gravity_g)), tol_gravity, "g", 3);
    }
    if let Some(t) = f.temperature_k {
        range_field(&mut fit, t, (rs.min_temp_k, rs.max_temp_k), cross.map(|s| (s.min_temp_k, s.max_temp_k)), tol_temp, "K", 1);
    }
    if let Some(p) = f.pressure_atm {
        let v = v_pressure(p, rs.min_pressure_atm, rs.max_pressure_atm);
        if v > 0.0 {
            let below = rs.min_pressure_atm.is_some_and(|m| p < m);
            let vs = cross
                .filter(|s| if below { s.min_pressure_atm.is_some() } else { s.max_pressure_atm.is_some() })
                .map(|s| v_pressure(p, s.min_pressure_atm, s.max_pressure_atm));
            let limits = match (rs.min_pressure_atm, rs.max_pressure_atm) {
                (Some(lo), Some(hi)) => format!("{lo:.4}–{hi:.4}"),
                (Some(lo), None) => format!("≥ {lo:.4}"),
                (None, Some(hi)) if hi <= AIRLESS_ATM => "airless".to_string(),
                (None, Some(hi)) => format!("< {hi:.4}"),
                (None, None) => String::new(),
            };
            fit.field(v, vs, || format!("{p:.4} atm, needs {limits} atm"));
        }
    }
    if !volcanism_ok(&rs.volcanism, &ctx.volcanism) {
        let vs = cross
            .filter(|s| s.volcanism != VolcanismRule::Any)
            .map(|s| if volcanism_ok(&s.volcanism, &ctx.volcanism) { 0.0 } else { V_VOLCANISM });
        let has = if ctx.volcanism.is_empty() { "no volcanism" } else { ctx.volcanism.as_str() };
        fit.field(V_VOLCANISM, vs, || format!("{has}, needs {}", describe_volcanism(&rs.volcanism)));
    }
    if let Some(rule) = &rs.regions {
        match ctx.region {
            Some(id) if !region_ok(rule, id) => {
                let vs = cross.and_then(|s| s.regions.as_ref()).map(|r| if region_ok(r, id) { 0.0 } else { V_REGION });
                fit.field(V_REGION, vs, || format!("not found in {}", exobio::region_name(id).unwrap_or("this region")));
            }
            Some(_) => {}
            None => match (&rule.inside, &rule.outside) {
                (Some(list), _) => fit.unknown(
                    U_REGION_IN,
                    || format!("region unknown (only {} regions, e.g. {})", list.len(), region_examples(list)),
                ),
                (None, Some(list)) => {
                    fit.unknown(U_REGION_OUT, || format!("region unknown (absent from {})", region_examples(list)))
                }
                (None, None) => {}
            },
        }
    }

    // System-level and special requirements (rule sets only: the cross-check criteria have none).
    for (gas, min) in &rs.min_atmosphere_pct {
        if f.atmosphere_composition.is_empty() {
            fit.unknown(U_COMPOSITION, || format!("atmosphere composition unknown (needs {gas} ≥ {min}%)"));
        } else {
            let pct = f
                .atmosphere_composition
                .iter()
                .find(|(g, _)| g.eq_ignore_ascii_case(gas))
                .map_or(0.0, |(_, p)| *p);
            if pct < *min {
                fit.miss(((min - pct) / TOL_COMPOSITION_PCT).min(V_NUMERIC_CAP), || format!("{gas} {pct:.2}% < {min}%"));
            }
        }
    }
    if let Some(reqs) = &rs.star_in_system {
        let stars: Vec<&StarFacts> = f.parent_stars.iter().chain(&f.system_stars).collect();
        if stars.is_empty() {
            fit.unknown(U_STAR, || format!("system stars unknown (needs {})", describe_stars(reqs)));
        } else if !stars.iter().any(|s| reqs.iter().any(|r| star_matches(r, s))) {
            fit.miss(V_STAR, || format!("no {} star in system", describe_stars(reqs)));
        }
    }
    if let Some(reqs) = &rs.parent_star {
        if f.parent_stars.is_empty() {
            fit.unknown(U_STAR, || format!("parent stars unknown (needs {})", describe_stars(reqs)));
        } else if !f.parent_stars.iter().any(|s| reqs.iter().any(|r| star_matches(r, s))) {
            fit.miss(V_STAR, || format!("no {} parent/primary star", describe_stars(reqs)));
        }
    }
    if let Some(classes) = &rs.system_body_types {
        if f.system_body_classes.is_empty() {
            fit.unknown(U_SYSTEM_BODY, || format!("other bodies unknown (needs {})", classes.join("/")));
        } else if !f.system_body_classes.iter().any(|b| classes.iter().any(|c| c.eq_ignore_ascii_case(b))) {
            fit.miss(V_SYSTEM_BODY, || format!("no {} in system", classes.join("/")));
        }
    }
    if let Some(kind) = &rs.nebula {
        match f.near_nebula {
            Some(true) => {}
            Some(false) => fit.miss(V_NEBULA, || "not near a nebula".to_string()),
            None => fit.unknown(U_NEBULA, || {
                let which = if kind == "large" { "a large nebula" } else { "a nebula" };
                format!("nebula proximity unknown (needs {which} within ~150 ly)")
            }),
        }
    }
    if rs.guardian {
        match ctx.guardian {
            Some(true) => {}
            Some(false) => fit.miss(V_ZONE, || "outside the Guardian bubbles".to_string()),
            None => fit.unknown(U_ZONE, || "position unknown (needs a Guardian bubble)".to_string()),
        }
    }
    if let Some(zones) = &rs.tuber_zones {
        match &ctx.tuber_zones {
            Some(here) if zones.iter().any(|z| here.contains(&z.as_str())) => {}
            Some(_) => fit.miss(V_ZONE, || "outside the Sinuous-tuber zones".to_string()),
            None => fit.unknown(U_ZONE, || "position unknown (needs a Sinuous-tuber zone)".to_string()),
        }
    }
    if let Some(system) = &rs.system
        && !f.system_name.eq_ignore_ascii_case(system)
    {
        fit.miss(V_SYSTEM, || format!("only in {system}"));
    }
    if let Some(max) = rs.max_orbital_period_s {
        match f.orbital_period_s {
            Some(p) if p.abs() <= max => {}
            Some(p) => fit.miss(V_ORBIT, || format!("orbit {:.1} d > {:.1} d", p.abs() / 86_400.0, max / 86_400.0)),
            None => fit.unknown(U_ORBIT, || "orbital period unknown".to_string()),
        }
    }
    if let Some(min) = rs.min_distance_ls {
        let d = f.distance_from_arrival_ls;
        if d < min {
            let v = ((min - d) / (TOL_DISTANCE_REL * min.max(1.0))).min(V_NUMERIC_CAP);
            fit.miss(v, || format!("{d:.0} ls from arrival < {min:.0} ls"));
        }
    }
    fit
}

fn v_atmosphere(list: &[String], ctx: &Ctx) -> f64 {
    if list.iter().any(|a| a.eq_ignore_ascii_case(&ctx.atmosphere)) {
        return 0.0;
    }
    let base = strip_rich(&ctx.atmosphere);
    let sibling = !ctx.airless() && list.iter().any(|a| strip_rich(a).eq_ignore_ascii_case(base));
    if sibling { V_RICH } else { V_HARD }
}

fn v_body(list: &[String], class: &str) -> f64 {
    if list.iter().any(|b| b.eq_ignore_ascii_case(class)) {
        0.0
    } else if list.iter().any(|b| related_body(b, class)) {
        V_RELATED_BODY
    } else {
        V_HARD
    }
}

fn v_range(x: f64, min: Option<f64>, max: Option<f64>, tol: Tolerance) -> f64 {
    let width = min.zip(max).map(|(lo, hi)| (hi - lo).max(0.0));
    let below = min.filter(|m| x < *m).map_or(0.0, |m| (m - x) / tol(m, width));
    let above = max.filter(|m| x > *m).map_or(0.0, |m| (x - m) / tol(m, width));
    (below + above).min(V_NUMERIC_CAP)
}

/// Tolerance around a numeric limit, given the rule's range width when both limits exist.
type Tolerance = fn(f64, Option<f64>) -> f64;

/// Numeric field with cross-check widening; the cross-check criteria count as evidence only on the side they
/// constrain.
fn range_field(
    fit: &mut Fit,
    x: f64,
    (min, max): (Option<f64>, Option<f64>),
    cross: Option<(Option<f64>, Option<f64>)>,
    tol: Tolerance,
    unit: &str,
    decimals: usize,
) {
    let v = v_range(x, min, max, tol);
    if v <= 0.0 {
        return;
    }
    let below = min.is_some_and(|m| x < m);
    let vs = cross.and_then(|(cmin, cmax)| {
        let explicit = if below { cmin.is_some() } else { cmax.is_some() };
        explicit.then(|| v_range(x, cmin, cmax, tol))
    });
    fit.field(v, vs, || {
        if below {
            format!("{x:.decimals$} {unit} < min {:.decimals$}", min.unwrap_or_default())
        } else {
            format!("{x:.decimals$} {unit} > max {:.decimals$}", max.unwrap_or_default())
        }
    });
}

/// Pressure limits: `max` is exclusive; a `max` of 0 (the cross-check criteria's Horizons-era organics) means airless.
fn v_pressure(p: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let mut v = 0.0;
    if let Some(min) = min.filter(|m| p < *m) {
        v += ((min.max(AIRLESS_ATM) / p.max(AIRLESS_ATM)).ln() / TOL_PRESSURE_LN).clamp(0.0, V_NUMERIC_CAP);
    }
    if let Some(max) = max {
        if max <= AIRLESS_ATM {
            if p > AIRLESS_ATM {
                v += V_AIRLESS;
            }
        } else if p >= max {
            v += ((p / max).ln() / TOL_PRESSURE_LN + 0.25).clamp(0.0, V_NUMERIC_CAP);
        }
    }
    v
}

fn region_ok(rule: &RegionRule, id: u8) -> bool {
    rule.inside.as_ref().is_none_or(|l| l.contains(&id)) && rule.outside.as_ref().is_none_or(|l| !l.contains(&id))
}

fn volcanism_ok(rule: &VolcanismRule, v: &str) -> bool {
    match rule {
        VolcanismRule::Any => true,
        VolcanismRule::None => v.is_empty(),
        VolcanismRule::Required => !v.is_empty(),
        VolcanismRule::Match { contains, equals, or_none } => {
            if v.is_empty() {
                *or_none
            } else {
                contains.iter().any(|c| v.contains(c.as_str())) || equals.iter().any(|e| v == e)
            }
        }
    }
}

fn describe_volcanism(rule: &VolcanismRule) -> String {
    match rule {
        VolcanismRule::Any => "any".to_string(),
        VolcanismRule::None => "none".to_string(),
        VolcanismRule::Required => "any volcanism".to_string(),
        VolcanismRule::Match { contains, equals, or_none } => {
            let mut parts: Vec<String> = contains.iter().chain(equals).cloned().collect();
            if *or_none {
                parts.push("none".to_string());
            }
            parts.join("/")
        }
    }
}

fn strip_rich(atmosphere: &str) -> &str {
    atmosphere.strip_suffix("Rich").unwrap_or(atmosphere)
}

/// Body classes the rule sets and the cross-check criteria disagree on for the same species.
fn related_body(a: &str, b: &str) -> bool {
    const PAIRS: &[(&str, &str)] = &[
        ("Rocky body", "High metal content body"),
        ("Icy body", "Rocky ice body"),
        ("Rocky body", "Rocky ice body"),
        ("High metal content body", "Metal rich body"),
    ];
    PAIRS.iter().any(|(x, y)| {
        (a.eq_ignore_ascii_case(x) && b.eq_ignore_ascii_case(y)) || (a.eq_ignore_ascii_case(y) && b.eq_ignore_ascii_case(x))
    })
}

fn body_abbrev(class: &str) -> &str {
    match class {
        "Rocky body" => "rocky",
        "High metal content body" => "HMC",
        "Icy body" => "icy",
        "Rocky ice body" => "rocky-ice",
        "Metal rich body" => "metal-rich",
        other => other,
    }
}

fn summarise(rs: &RuleSet) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(a) = &rs.atmospheres {
        parts.push(a.join("/"));
    }
    if let Some(b) = &rs.body_types {
        parts.push(b.iter().map(|b| body_abbrev(b)).collect::<Vec<_>>().join("/"));
    }
    match (rs.min_gravity_g, rs.max_gravity_g) {
        (Some(lo), Some(hi)) => parts.push(format!("{lo:.3}–{hi:.3} g")),
        (None, Some(hi)) => parts.push(format!("≤{hi:.3} g")),
        (Some(lo), None) => parts.push(format!("≥{lo:.3} g")),
        (None, None) => {}
    }
    match (rs.min_temp_k, rs.max_temp_k) {
        (Some(lo), Some(hi)) => parts.push(format!("{lo:.0}–{hi:.0} K")),
        (None, Some(hi)) => parts.push(format!("≤{hi:.0} K")),
        (Some(lo), None) => parts.push(format!("≥{lo:.0} K")),
        (None, None) => {}
    }
    match &rs.volcanism {
        VolcanismRule::Any => {}
        VolcanismRule::None => parts.push("no volcanism".to_string()),
        VolcanismRule::Required => parts.push("volcanism required".to_string()),
        rule @ VolcanismRule::Match { .. } => parts.push(format!("volcanism {}", describe_volcanism(rule))),
    }
    parts.join(" · ")
}

fn region_examples(ids: &[u8]) -> String {
    let names: Vec<&str> = ids.iter().filter_map(|&i| exobio::region_name(i)).take(2).collect();
    let more = ids.len().saturating_sub(names.len());
    if more > 0 { format!("{} +{more}", names.join(", ")) } else { names.join(", ") }
}

fn describe_stars(reqs: &[StarReq]) -> String {
    reqs.iter()
        .map(|r| match &r.luminosity {
            Some(l) => format!("{} {l}", r.class),
            None => r.class.clone(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Star matching: giants/supergiants count as their class, `D`/`C`/`W` are prefixes, the rest exact;
/// a luminosity requirement `IV` matches `IV`, `IVa`, `IVab` ….
fn star_matches(req: &StarReq, star: &StarFacts) -> bool {
    let t = star.star_type.as_str();
    let class_ok = match req.class.as_str() {
        "A" => t == "A" || t == "A_BlueWhiteSuperGiant",
        "B" => t == "B" || t == "B_BlueWhiteSuperGiant",
        "F" => t == "F" || t == "F_WhiteSuperGiant",
        "G" => t == "G" || t == "G_WhiteSuperGiant",
        "K" => t == "K" || t == "K_OrangeGiant",
        "M" => t == "M" || t == "M_RedGiant" || t == "M_RedSuperGiant",
        c @ ("D" | "C" | "W") => t.starts_with(c),
        c => t == c,
    };
    class_ok
        && req.luminosity.as_deref().is_none_or(|need| {
            star.luminosity
                .as_deref()
                .is_some_and(|l| l.trim_end_matches(|c: char| c.is_ascii_lowercase() || c == '0') == need)
        })
}

fn star_colour_key(star_type: &str) -> Option<&'static str> {
    Some(match star_type {
        "O" => "O",
        "B" | "B_BlueWhiteSuperGiant" => "B",
        "A" | "A_BlueWhiteSuperGiant" => "A",
        "F" | "F_WhiteSuperGiant" => "F",
        "G" | "G_WhiteSuperGiant" => "G",
        "K" | "K_OrangeGiant" => "K",
        "M" | "M_RedGiant" | "M_RedSuperGiant" => "M",
        "L" => "L",
        "T" => "T",
        "Y" => "Y",
        "TTS" => "TTS",
        "AeBe" => "Ae",
        "N" => "N",
        t if t.starts_with('W') => "W",
        t if t.starts_with('D') => "D",
        _ => return None,
    })
}

/// Rough visual-brightness order of star classes (lower = brighter), to pick the dominant star of a barycentre.
fn brightness_rank(star_type: &str) -> u8 {
    match star_type {
        "TTS" => 5,
        "AeBe" => 2,
        "H" | "SupermassiveBlackHole" | "X" => 12,
        _ => match star_type.as_bytes().first() {
            Some(b'O') => 0,
            Some(b'B' | b'W') => 1,
            Some(b'A') => 2,
            Some(b'F') => 3,
            Some(b'G') => 4,
            Some(b'K') => 5,
            Some(b'M' | b'C' | b'S') => 6,
            Some(b'L') => 7,
            Some(b'T') => 8,
            Some(b'Y') => 9,
            Some(b'D') => 10,
            Some(b'N') => 11,
            _ => 12,
        },
    }
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

// ------------------------------------------------------------------------------------------------
// Reference positions (see docs/research/exploration.md §2.6)
// ------------------------------------------------------------------------------------------------

struct Bubble {
    pos: [f64; 3],
    radius_ly: f64,
}

/// Guardian-ruins bubbles required by Brain Trees: the system at the centre of each, and its radius.
const GUARDIAN_BUBBLES: &[Bubble] = &[
    Bubble { pos: [-840.65625, -561.15625, 13361.8125], radius_ly: 750.0 },  // Hen 2-333
    Bubble { pos: [1099.21875, -146.6875, -133.59375], radius_ly: 750.0 },   // Gamma Velorum
    Bubble { pos: [-5493.09375, -589.28125, 10424.4375], radius_ly: 100.0 }, // Skaudai AA-A h71
    Bubble { pos: [1220.40625, -694.625, 12312.8125], radius_ly: 100.0 },    // Blaa Hypai AA-A h68
    Bubble { pos: [4949.9375, 164.0, 20640.125], radius_ly: 100.0 },         // Eorl Auwsy AA-A h72
    Bubble { pos: [-9294.875, -458.40625, 7905.71875], radius_ly: 100.0 },   // Prai Hypoo AA-A h60
    Bubble { pos: [8579.96875, -138.96875, 2701.375], radius_ly: 100.0 },    // Eta Carina Nebula
    Bubble { pos: [14574.15625, -259.625, 3511.90625], radius_ly: 100.0 },   // NGC 3199
];

struct TuberZone {
    name: &'static str,
    pos: [f64; 3],
    min_ly: f64,
    max_ly: f64,
}

/// Sinuous-tuber zones: distance bands around zone centres.
const TUBER_ZONES: &[TuberZone] = &[
    TuberZone { name: "Arcadian Stream", pos: [8885.0, -20.0, 20535.0], min_ly: 200.0, max_ly: 600.0 },
    TuberZone { name: "Empyrean Straits", pos: [4325.0, 400.0, 21185.0], min_ly: 200.0, max_ly: 400.0 },
    TuberZone { name: "Galactic Center", pos: [44.5, 492.7, 25916.0], min_ly: 500.0, max_ly: 1000.0 },
    TuberZone { name: "Hawking A", pos: [5788.0, 150.0, 6335.0], min_ly: 150.0, max_ly: 600.0 },
    TuberZone { name: "Hawking B", pos: [9990.0, -40.0, 8335.0], min_ly: 200.0, max_ly: 600.0 },
    TuberZone { name: "Inner Orion Spur", pos: [-3485.0, 39.0, 7320.0], min_ly: 200.0, max_ly: 600.0 },
    TuberZone { name: "Inner O-P Conflux", pos: [-13245.0, -80.0, 30285.0], min_ly: 350.0, max_ly: 750.0 },
    TuberZone { name: "Inner S-C Arm A", pos: [-1600.0, -37.0, 10720.0], min_ly: 200.0, max_ly: 600.0 },
    TuberZone { name: "Inner S-C Arm B 1", pos: [-6645.0, 0.0, 12590.0], min_ly: 150.0, max_ly: 300.0 },
    TuberZone { name: "Inner S-C Arm B 2", pos: [-6645.0, 0.0, 12590.0], min_ly: 200.0, max_ly: 600.0 },
    TuberZone { name: "Inner S-C Arm C", pos: [-9355.0, -50.0, 17175.0], min_ly: 200.0, max_ly: 600.0 },
    TuberZone { name: "Inner S-C Arm D", pos: [-12000.0, 232.0, 22670.0], min_ly: 300.0, max_ly: 400.0 },
    TuberZone { name: "Izanami", pos: [-4610.0, 370.0, 37225.0], min_ly: 200.0, max_ly: 750.0 },
    TuberZone { name: "Norma Arm A", pos: [3722.6, 200.0, 16441.0], min_ly: 500.0, max_ly: 1000.0 },
    TuberZone { name: "Norma Arm B", pos: [3740.0, 175.0, 16460.0], min_ly: 200.0, max_ly: 500.0 },
    TuberZone { name: "Norma Expanse A", pos: [4245.0, -42.0, 12071.0], min_ly: 200.0, max_ly: 600.0 },
    TuberZone { name: "Norma Expanse B", pos: [5580.0, 40.0, 11727.0], min_ly: 150.0, max_ly: 250.0 },
    TuberZone { name: "Odin A", pos: [-7945.0, 230.0, 28025.0], min_ly: 750.0, max_ly: 1000.0 },
    TuberZone { name: "Odin B", pos: [-5329.0, -68.0, 18647.0], min_ly: 200.0, max_ly: 600.0 },
    TuberZone { name: "Ryker A", pos: [1715.0, 766.0, 34070.0], min_ly: 250.0, max_ly: 750.0 },
    TuberZone { name: "Ryker B", pos: [-1445.0, 345.0, 30345.0], min_ly: 750.0, max_ly: 1500.0 },
    TuberZone { name: "Trojan Belt", pos: [18600.0, 65.0, 31750.0], min_ly: 250.0, max_ly: 500.0 },
];

/// Centre of a Sinuous-tuber zone and its distance band (ly), for tests and tools.
pub fn tuber_zone(name: &str) -> Option<([f64; 3], f64, f64)> {
    TUBER_ZONES.iter().find(|z| z.name.eq_ignore_ascii_case(name)).map(|z| (z.pos, z.min_ly, z.max_ly))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rocky_ammonia(region: Option<&str>) -> BodyFacts {
        BodyFacts {
            planet_class: "Rocky body".into(),
            atmosphere_type: Some("Ammonia".into()),
            atmosphere_composition: vec![("Ammonia".into(), 100.0)],
            gravity_g: Some(0.0574),
            temperature_k: Some(167.06),
            pressure_atm: Some(0.00343),
            landable: true,
            materials: vec![("iron".into(), 19.6), ("cadmium".into(), 1.5), ("ruthenium".into(), 1.2), ("tin".into(), 1.2)],
            parent_stars: vec![StarFacts::new("K", Some("Vab"))],
            distance_from_arrival_ls: 777.0,
            system_name: "Swoiwns EH-J d10-73".into(),
            star_pos: Some([-224.78125, -133.75, 1168.9375]),
            region: region.map(str::to_string),
            bio_signal_count: Some(8),
            ..BodyFacts::default()
        }
    }

    fn hmc_co2() -> BodyFacts {
        BodyFacts {
            planet_class: "High metal content body".into(),
            atmosphere_type: Some("CarbonDioxide".into()),
            atmosphere_composition: vec![("CarbonDioxide".into(), 99.009911), ("SulphurDioxide".into(), 0.990099)],
            gravity_g: Some(0.0894),
            temperature_k: Some(165.16),
            pressure_atm: Some(0.01094),
            landable: true,
            materials: vec![("iron".into(), 23.5), ("niobium".into(), 1.6), ("ruthenium".into(), 1.45), ("tungsten".into(), 1.3)],
            parent_stars: vec![StarFacts::new("K", Some("Va")), StarFacts::new("M", Some("VI"))],
            system_name: "Swoilz MH-W c17-3".into(),
            region: Some("Inner Orion Spur".into()),
            bio_signal_count: Some(4),
            ..BodyFacts::default()
        }
    }

    fn top<'a>(preds: &'a [Prediction], genus: &str) -> Option<&'a Prediction> {
        preds.iter().find(|p| p.genus == genus)
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn real_hmc_co2_body_after_dss() {
        let genera = names(&["$Codex_Ent_Bacterial_Genus_Name;", "$Codex_Ent_Stratum_Genus_Name;", "Tubus", "Tussock"]);
        let preds = predict(&hmc_co2(), &genera);
        let stratum = top(&preds, "Stratum").expect("stratum");
        assert_eq!(stratum.species, "Stratum Tectonicas");
        assert_eq!(stratum.variant.as_deref(), Some("Lime"));
        let tussock = top(&preds, "Tussock").expect("tussock");
        assert_eq!(tussock.species, "Tussock Ignis");
        assert_eq!(tussock.variant.as_deref(), Some("Green"));
        assert_eq!(top(&preds, "Bacterium").map(|p| p.species.as_str()), Some("Bacterium Aurasus"));
        assert!(preds.iter().all(|p| ["Bacterium", "Stratum", "Tubus", "Tussock"].contains(&p.genus.as_str())));
        let total: f32 = preds.iter().map(|p| p.likelihood).sum();
        assert!((total - 4.0).abs() < 0.05, "one species per known genus: {total}");
        assert!(stratum.reasons.iter().any(|r| r.contains("the conditions it has been found in (rule ")), "{:?}", stratum.reasons);
        // Reasons say what was checked, never which tool's rules they are.
        for p in &preds {
            assert!(p.reasons.iter().all(|r| !r.contains("BioScan") && !r.contains("SrvSurvey")), "{:?}", p.reasons);
        }
    }

    #[test]
    fn region_decides_arm_specific_species() {
        let preds = predict(&rocky_ammonia(Some("Inner Orion Spur")), &names(&["Tussock"]));
        assert_eq!(preds[0].species, "Tussock Cultro");
        assert!(preds[0].likelihood > 0.9, "{preds:?}");
        assert_eq!(preds[0].variant.as_deref(), Some("Green"));

        let blind = predict(&rocky_ammonia(None), &names(&["Tussock"]));
        let arm: Vec<&Prediction> = blind
            .iter()
            .filter(|p| ["Tussock Cultro", "Tussock Catena", "Tussock Divisa"].contains(&p.species.as_str()))
            .collect();
        assert_eq!(arm.len(), 3, "{blind:?}");
        assert!(arm.iter().all(|p| (p.likelihood - arm[0].likelihood).abs() < 1e-4));
        assert!(arm[0].reasons.iter().any(|r| r.contains("region unknown")));
    }

    #[test]
    fn signal_count_bounds_genus_odds() {
        let mut f = hmc_co2();
        for n in [1u32, 2, 4] {
            f.bio_signal_count = Some(n);
            let preds = predict(&f, &[]);
            let total: f32 = preds.iter().map(|p| p.likelihood).sum();
            assert!(total <= n as f32 + 0.01, "n={n}: {total}");
            assert!(total >= n as f32 * 0.85, "n={n}: {total}");
            assert!(preds.iter().all(|p| p.likelihood.is_finite() && p.likelihood <= PRESENCE_CAP as f32 + 1e-4));
        }
        f.bio_signal_count = Some(0);
        assert!(predict(&f, &[]).is_empty());
        f.bio_signal_count = None;
        assert!(!predict(&f, &[]).is_empty());
        f.landable = false;
        assert!(predict(&f, &[]).is_empty());
    }

    #[test]
    fn known_genus_always_gets_a_candidate() {
        // No Fumerola rule fits a cold ammonia world without volcanism, but DSS says it is there.
        let preds = predict(&rocky_ammonia(Some("Inner Orion Spur")), &names(&["Fumerola", "Not A Genus"]));
        assert!(preds.iter().any(|p| p.genus == "Fumerola"), "{preds:?}");
        assert!(preds.iter().any(|p| p.genus == "Not A Genus" && p.value == 0));
        let thargoid = predict(&rocky_ammonia(None), &names(&["Thargoid"]));
        assert_eq!(thargoid.len(), 1, "{thargoid:?}");
        assert_eq!(thargoid[0].genus, "Thargoid");
        let f = preds.iter().find(|p| p.genus == "Fumerola").expect("fumerola");
        assert!(f.reasons.iter().any(|r| r.contains("near miss") || r.contains("no rule fits")), "{:?}", f.reasons);
    }

    #[test]
    fn electricae_needs_bright_or_remnant_parent_star() {
        let mut f = BodyFacts {
            planet_class: "Icy body".into(),
            atmosphere_type: Some("Argon".into()),
            atmosphere_composition: vec![("Argon".into(), 100.0)],
            gravity_g: Some(0.19),
            temperature_k: Some(90.0),
            pressure_atm: Some(0.002),
            landable: true,
            materials: vec![("sulphur".into(), 24.0), ("antimony".into(), 0.7)],
            parent_stars: vec![StarFacts::new("A", Some("V"))],
            bio_signal_count: Some(1),
            ..BodyFacts::default()
        };
        let preds = predict(&f, &names(&["Electricae"]));
        assert_eq!(preds[0].species, "Electricae Pluma");
        assert_eq!(preds[0].variant.as_deref(), Some("Cobalt"));
        f.parent_stars = vec![StarFacts::new("M", Some("Va"))];
        f.near_nebula = Some(true);
        let preds = predict(&f, &names(&["Electricae"]));
        assert_eq!(preds[0].species, "Electricae Radialem");
    }

    #[test]
    fn recepta_needs_sulphur_dioxide() {
        let mut f = hmc_co2();
        f.temperature_k = Some(170.0);
        let preds = predict(&f, &names(&["Recepta"]));
        assert!(preds.iter().all(|p| p.reasons.iter().any(|r| r.contains("SulphurDioxide"))), "{preds:?}");
        f.atmosphere_composition = vec![("CarbonDioxide".into(), 78.0), ("SulphurDioxide".into(), 22.0)];
        let preds = predict(&f, &names(&["Recepta"]));
        assert!(preds[0].reasons.iter().any(|r| r.starts_with("fits the conditions it has been found in")), "{preds:?}");
    }

    #[test]
    fn widened_miss_says_why() {
        // Outside the rule set but inside the cross-check criteria: the softer cost, and a reason that says so.
        let mut fit = Fit::new(V_WIDENED, true);
        fit.field(5.0, Some(0.0), || "0.285 g > max 0.276".to_string());
        assert_eq!(fit.misses, vec!["0.285 g > max 0.276 (a wider set of limits allows it)".to_string()]);
        assert!((fit.v - V_WIDENED).abs() < 1e-9);
        // Outside both: the rule set's own cost, and the plain reason.
        let mut fit = Fit::new(V_WIDENED, true);
        fit.field(5.0, Some(V_HARD), || "0.9 g > max 0.276".to_string());
        assert_eq!((fit.misses.as_slice(), fit.v), (&["0.9 g > max 0.276".to_string()][..], 5.0));
    }

    #[test]
    fn horizons_organics_are_finite_and_zone_bound() {
        let (centre, lo, hi) = tuber_zone("Arcadian Stream").expect("zone");
        let f = BodyFacts {
            planet_class: "High metal content body".into(),
            atmosphere_type: None,
            gravity_g: Some(1.0),
            temperature_k: Some(350.0),
            pressure_atm: Some(0.0),
            volcanism: Some("major silicate vapour geysers volcanism".into()),
            landable: true,
            parent_stars: vec![StarFacts::new("K", Some("V"))],
            star_pos: Some([centre[0] + (lo + hi) / 2.0, centre[1], centre[2]]),
            bio_signal_count: Some(1),
            ..BodyFacts::default()
        };
        let preds = predict(&f, &names(&["$Codex_Ent_Tube_Name;"]));
        assert!(!preds.is_empty());
        assert!(preds.iter().all(|p| p.likelihood.is_finite()));
        assert!(["Blatteum Sinuous Tubers", "Violaceum Sinuous Tubers"].contains(&preds[0].species.as_str()), "{preds:?}");
        let anywhere = predict(&f, &[]);
        assert!(anywhere.iter().all(|p| p.likelihood.is_finite()));
    }

    #[test]
    fn bonus_rule_and_values() {
        assert!(first_logged_bonus_likely(Some(false), false, None));
        assert!(!first_logged_bonus_likely(Some(true), false, None));
        assert!(!first_logged_bonus_likely(Some(false), true, None));
        assert!(!first_logged_bonus_likely(None, false, None));
        assert!(first_logged_bonus_likely(Some(false), false, Some(false)));
        // Sales (2026-10-24): WasLogged false paid nothing in populated systems (HR 8061 B 4, not footfalled) and
        // nothing for 19 of 21 species on footfalled bodies; true never paid.
        assert!(!first_logged_bonus_likely(Some(false), true, Some(false)));
        assert!(!first_logged_bonus_likely(Some(true), false, Some(false)));
        assert!(!first_logged_bonus_likely(None, false, Some(false)));
        assert!(!first_logged_bonus_likely(Some(false), false, Some(true)));
        assert_eq!(payout(1_362_000, true), 6_810_000);
        assert_eq!(payout(1_362_000, false), 1_362_000);
        let p = Prediction {
            genus: "Stratum".into(),
            species: "Stratum Paleas".into(),
            value: 1_362_000,
            colony_range_m: 500,
            likelihood: 0.5,
            variant: None,
            reasons: Vec::new(),
        };
        assert_eq!(expected_value(&p, false), 681_000);
        assert_eq!(expected_value(&p, true), 3_405_000);
    }

    #[test]
    fn journal_helpers() {
        let scan: Value = serde_json::from_str(
            r#"{"event":"Scan","BodyName":"X 1","BodyID":5,"Parents":[{"Null":3},{"Star":0}],"StarSystem":"X",
            "PlanetClass":"High metal content body","Atmosphere":"thin carbon dioxide atmosphere",
            "AtmosphereType":"CarbonDioxide","AtmosphereComposition":[{"Name":"CarbonDioxide","Percent":99.0}],
            "Volcanism":"","SurfaceGravity":0.876,"SurfaceTemperature":165.1,"SurfacePressure":1107.6,
            "Landable":true,"Materials":[{"Name":"iron","Percent":23.4}],"DistanceFromArrivalLS":844.8}"#,
        )
        .expect("json");
        let f = BodyFacts::from_journal_scan(&scan).expect("planet");
        assert!((f.gravity_g.unwrap_or_default() - 0.0894).abs() < 1e-3);
        assert!((f.pressure_atm.unwrap_or_default() - 0.01094).abs() < 1e-4);
        assert_eq!(f.volcanism, None);
        assert!(f.landable);
        let parents = ParentRef::parse_journal(&scan["Parents"]);
        assert_eq!(parents, vec![ParentRef::Barycentre(3), ParentRef::Star(0)]);
        let star = |id, parent, t: &str| SystemStar {
            body_id: id,
            parent,
            distance_ls: if id == 0 { 0.0 } else { 100.0 },
            facts: StarFacts::new(t, Some("V")),
        };
        let stars = vec![
            star(0, None, "G"),
            star(1, Some(ParentRef::Barycentre(3)), "M"),
            star(2, Some(ParentRef::Barycentre(3)), "K"),
        ];
        let resolved: Vec<String> =
            resolve_parent_stars(&parents, &stars).into_iter().map(|s| s.star_type).collect();
        assert_eq!(resolved, vec!["K", "M", "G"]);
        let mut text_only = f.clone();
        text_only.atmosphere_type = None;
        text_only.atmosphere = Some("hot thin carbon dioxide-rich atmosphere".into());
        assert_eq!(normalise_atmosphere(&text_only), "CarbonDioxideRich");
    }

    #[test]
    fn every_rule_set_is_satisfiable() {
        // A body at the centre of each rule set must yield that species as a candidate of its genus.
        for sp in exobio::all_species().iter().filter(|s| !s.thargoid) {
            for (i, rs) in sp.rulesets.iter().enumerate() {
                let f = synthetic_body(sp, rs);
                let ctx = Ctx::new(&f);
                let fit = eval_rule(rs, sp.crosscheck.as_ref(), V_WIDENED, &ctx, true);
                assert!(fit.misses.is_empty(), "{} rule {}: {:?}", sp.name, i + 1, fit.misses);
                let preds = predict(&f, &[sp.genus.to_string()]);
                assert!(preds.iter().any(|p| p.species == sp.name), "{} rule {} not predicted: {preds:?}", sp.name, i + 1);
            }
        }
    }

    /// A body at the centre of a rule set (shared with the bio_eval example's synthetic check in spirit).
    fn synthetic_body(sp: &Species, rs: &RuleSet) -> BodyFacts {
        let mid = |lo: Option<f64>, hi: Option<f64>, default: f64| match (lo, hi) {
            (Some(a), Some(b)) => (a + b) / 2.0,
            (Some(a), None) => a * 1.1 + 1.0,
            (None, Some(b)) => b * 0.9,
            (None, None) => default,
        };
        let atmosphere = rs.atmospheres.as_ref().and_then(|a| a.first().cloned()).unwrap_or_else(|| "None".into());
        let airless = atmosphere == "None";
        let pressure = match (rs.min_pressure_atm, rs.max_pressure_atm) {
            (_, Some(b)) if b <= AIRLESS_ATM => 0.0,
            (Some(a), Some(b)) => (a * b).sqrt(),
            (Some(a), None) => a * 1.2,
            (None, Some(b)) => b * 0.7,
            (None, None) if airless => 0.0,
            (None, None) => 0.01,
        };
        let volcanism = match &rs.volcanism {
            VolcanismRule::Any | VolcanismRule::None => None,
            VolcanismRule::Required => Some("minor rocky magma volcanism".to_string()),
            VolcanismRule::Match { contains, equals, .. } => {
                equals.first().cloned().or_else(|| contains.first().map(|c| format!("{c} volcanism")))
            }
        };
        let region = rs.regions.as_ref().and_then(|r| match (&r.inside, &r.outside) {
            (Some(list), _) => list.last().copied(),
            (None, Some(out)) => (1..=42u8).rev().find(|id| !out.contains(id)),
            (None, None) => None,
        });
        let star_from = |req: &StarReq| StarFacts::new(req.class.clone(), req.luminosity.as_deref());
        let mut parent_stars: Vec<StarFacts> = rs.parent_star.iter().flatten().take(1).map(star_from).collect();
        if sp.colour_basis == ColourBasis::Star
            && !parent_stars.iter().any(|s| s.colour_key().is_some_and(|k| sp.variant(k).is_some()))
            && let Some(v) = sp.variants.iter().find(|v| v.in_codex)
        {
            let class = if v.key == "Ae" { "AeBe".to_string() } else { v.key.to_string() };
            parent_stars.insert(0, StarFacts::new(class, Some("V")));
        }
        let mut materials: Vec<(String, f64)> = vec![("iron".into(), 20.0)];
        if sp.colour_basis == ColourBasis::Element
            && let Some(v) = sp.variants.first()
        {
            materials.push((v.key.to_ascii_lowercase(), 1.0));
        }
        let mut composition: Vec<(String, f64)> = Vec::new();
        if !airless {
            composition.push((strip_rich(&atmosphere).to_string(), 98.0));
        }
        for (gas, min) in &rs.min_atmosphere_pct {
            composition.push((gas.clone(), min + 1.0));
        }
        let star_pos = if rs.guardian {
            Some(GUARDIAN_BUBBLES[1].pos)
        } else if let Some((c, lo, hi)) = rs.tuber_zones.as_ref().and_then(|z| z.first()).and_then(|z| tuber_zone(z)) {
            Some([c[0] + (lo + hi) / 2.0, c[1], c[2]])
        } else {
            Some([0.0, 0.0, 0.0])
        };
        BodyFacts {
            planet_class: rs.body_types.as_ref().and_then(|b| b.first().cloned()).unwrap_or_else(|| "Rocky body".into()),
            atmosphere_type: Some(atmosphere),
            atmosphere_composition: composition,
            gravity_g: Some(mid(rs.min_gravity_g, rs.max_gravity_g, 0.1)),
            temperature_k: Some(mid(rs.min_temp_k, rs.max_temp_k, 170.0)),
            pressure_atm: Some(pressure),
            volcanism,
            landable: true,
            materials,
            parent_stars,
            distance_from_arrival_ls: rs.min_distance_ls.map_or(500.0, |d| d * 1.1),
            system_name: rs.system.clone().unwrap_or_else(|| "Synthetic".into()),
            star_pos,
            region: region.map(|r| r.to_string()),
            bio_signal_count: Some(1),
            system_stars: rs.star_in_system.iter().flatten().take(1).map(star_from).collect(),
            system_body_classes: rs.system_body_types.clone().unwrap_or_else(|| vec!["Rocky body".into()]),
            orbital_period_s: Some(rs.max_orbital_period_s.map_or(1.0e6, |p| p * 0.5)),
            near_nebula: rs.nebula.as_ref().map(|_| true),
            ..BodyFacts::default()
        }
    }
}
