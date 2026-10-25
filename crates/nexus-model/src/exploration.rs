//! Systems and bodies the commander has scanned, unsold cartographic data, and "interesting"
//! body detection.

use nexus_data::exploration as xv;
use nexus_journal::events as ev;
use nexus_journal::Timestamp;
use rustc_hash::FxHashMap;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Star,
    Planet,
    BeltCluster,
    Ring,
    Barycentre,
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub struct RingInfo {
    pub name: String,
    pub class: String,
    pub mass_mt: f64,
    pub inner_rad: f64,
    pub outer_rad: f64,
    /// Hotspots / signals found by DSS-mapping this ring.
    pub signals: Vec<(String, u32)>,
}

#[derive(Debug, Clone, Default)]
pub struct OrganicSpecies {
    pub genus: String,
    pub species: String,
    pub variant: Option<String>,
    /// Codex symbol of the species (`$Codex_Ent_Stratum_07_Name;`).
    pub species_symbol: String,
    /// 0 = only seen/predicted, 1..3 = samples taken.
    pub samples: u8,
    pub analysed: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BodyValue {
    /// Value if only FSS-scanned.
    pub fss: i64,
    /// Value if (efficiently) mapped.
    pub mapped: i64,
    /// What it is worth right now given what we've done.
    pub current: i64,
}

#[derive(Debug, Clone)]
pub struct Body {
    pub id: u32,
    pub name: String,
    pub kind: BodyKind,
    pub parents: Vec<(String, u32)>,
    pub distance_ls: f64,
    pub star_type: Option<String>,
    pub star_subclass: Option<u32>,
    pub luminosity: Option<String>,
    pub stellar_mass: Option<f64>,
    pub age_my: Option<f64>,
    pub absolute_magnitude: Option<f64>,
    pub planet_class: Option<String>,
    pub terraform_state: Option<String>,
    pub atmosphere: Option<String>,
    pub atmosphere_type: Option<String>,
    pub atmosphere_composition: Vec<(String, f64)>,
    pub volcanism: Option<String>,
    pub mass_em: Option<f64>,
    pub radius_m: Option<f64>,
    /// Surface gravity in m/s² (as in the journal); divide by 9.80665 for g.
    pub gravity_ms2: Option<f64>,
    pub temperature_k: Option<f64>,
    /// Surface pressure in pascals (as in the journal).
    pub pressure_pa: Option<f64>,
    pub landable: bool,
    pub materials: Vec<(String, f64)>,
    pub composition: Option<(f64, f64, f64)>,
    pub rings: Vec<RingInfo>,
    pub reserve_level: Option<String>,
    pub tidal_lock: bool,
    pub rotation_period_s: Option<f64>,
    pub orbital_period_s: Option<f64>,
    pub semi_major_axis_m: Option<f64>,
    pub eccentricity: Option<f64>,
    pub inclination: Option<f64>,
    pub axial_tilt: Option<f64>,
    pub was_discovered: Option<bool>,
    pub was_mapped: Option<bool>,
    pub was_footfalled: Option<bool>,
    pub scan_type: String,
    pub scanned: Timestamp,
    /// We have a detailed scan (not just a signal or name).
    pub has_scan: bool,
    pub mapped: bool,
    pub probes_used: Option<u32>,
    pub efficiency_target: Option<u32>,
    pub signals: Vec<(String, u32)>,
    pub bio_signals: u32,
    pub geo_signals: u32,
    /// Planetary mining locations (Rhino surface mining), from the FSS or DSS.
    pub mining_locations: u32,
    pub genuses: Vec<String>,
    pub organics: Vec<OrganicSpecies>,
    pub footfall: bool,
    pub value: BodyValue,
    /// Likely species (bodies with biological signals), best first.
    pub predictions: Vec<nexus_data::bio_predict::Prediction>,
}

impl Body {
    fn placeholder(id: u32, name: &str) -> Self {
        Body {
            id,
            name: name.to_string(),
            kind: BodyKind::Unknown,
            parents: Vec::new(),
            distance_ls: 0.0,
            star_type: None,
            star_subclass: None,
            luminosity: None,
            stellar_mass: None,
            age_my: None,
            absolute_magnitude: None,
            planet_class: None,
            terraform_state: None,
            atmosphere: None,
            atmosphere_type: None,
            atmosphere_composition: Vec::new(),
            volcanism: None,
            mass_em: None,
            radius_m: None,
            gravity_ms2: None,
            temperature_k: None,
            pressure_pa: None,
            landable: false,
            materials: Vec::new(),
            composition: None,
            rings: Vec::new(),
            reserve_level: None,
            tidal_lock: false,
            rotation_period_s: None,
            orbital_period_s: None,
            semi_major_axis_m: None,
            eccentricity: None,
            inclination: None,
            axial_tilt: None,
            was_discovered: None,
            was_mapped: None,
            was_footfalled: None,
            scan_type: String::new(),
            scanned: Timestamp::ZERO,
            has_scan: false,
            mapped: false,
            probes_used: None,
            efficiency_target: None,
            signals: Vec::new(),
            bio_signals: 0,
            geo_signals: 0,
            mining_locations: 0,
            genuses: Vec::new(),
            organics: Vec::new(),
            footfall: false,
            value: BodyValue::default(),
            predictions: Vec::new(),
        }
    }

    pub fn gravity_g(&self) -> Option<f64> {
        self.gravity_ms2.map(|g| g / 9.806_65)
    }

    pub fn pressure_atm(&self) -> Option<f64> {
        self.pressure_pa.map(|p| p / 101_325.0)
    }

    pub fn is_terraformable(&self) -> bool {
        self.terraform_state.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("Terraformable") || t.eq_ignore_ascii_case("Terraforming"))
    }

    pub fn has_atmosphere(&self) -> bool {
        self.atmosphere_type.as_deref().is_some_and(|a| !a.is_empty() && a != "None")
            || self.atmosphere.as_deref().is_some_and(|a| !a.is_empty() && !a.eq_ignore_ascii_case("No atmosphere"))
    }

    pub fn first_discovery(&self) -> bool {
        self.was_discovered == Some(false)
    }

    /// Name without the system prefix ("Synuefe BR-L c24-23 2 a" -> "2 a").
    pub fn short_name<'a>(&'a self, system: &str) -> &'a str {
        let n = self.name.strip_prefix(system).map(str::trim_start).unwrap_or(&self.name);
        if n.is_empty() { "Main star" } else { n }
    }

    pub fn is_mappable(&self) -> bool {
        self.kind == BodyKind::Planet
    }

    /// Nothing biological left to do here: no biological signals, or one species analysed per signal
    /// (each signal is a genus, and only one species per genus grows on a body).
    pub fn bio_done(&self) -> bool {
        self.bio_signals == 0 || self.organics.iter().filter(|o| o.analysed).count() >= self.bio_signals as usize
    }

    pub fn recompute_value(&mut self, odyssey: bool) {
        self.value = match self.kind {
            BodyKind::Star => {
                let v = xv::star_value(
                    self.star_type.as_deref().unwrap_or(""),
                    self.stellar_mass.unwrap_or(1.0),
                    self.first_discovery(),
                );
                BodyValue { fss: v, mapped: v, current: v }
            }
            BodyKind::Planet => {
                let k = xv::planet_k(self.planet_class.as_deref().unwrap_or(""), self.is_terraformable());
                let mass = self.mass_em.unwrap_or(0.0);
                let base = xv::ValueOptions {
                    first_discoverer: self.first_discovery(),
                    first_mapped: self.was_mapped == Some(false),
                    odyssey,
                    ..Default::default()
                };
                let fss = xv::planet_value(k, mass, base);
                let efficient = match (self.probes_used, self.efficiency_target) {
                    (Some(p), Some(t)) => p <= t,
                    _ => true,
                };
                let mapped = xv::planet_value(k, mass, xv::ValueOptions { mapped: true, efficient, ..base });
                BodyValue { fss, mapped, current: if self.mapped { mapped } else { fss } }
            }
            _ => BodyValue::default(),
        };
    }

    fn apply_scan(&mut self, s: &ev::Scan, ts: Timestamp, odyssey: bool) {
        self.name = s.body_name.clone();
        self.kind = if s.is_star() {
            BodyKind::Star
        } else if s.is_planet() {
            BodyKind::Planet
        } else if s.body_name.contains("Belt Cluster") {
            BodyKind::BeltCluster
        } else {
            BodyKind::Ring
        };
        self.parents = s.parents.iter().flat_map(|m| m.iter().map(|(k, v)| (k.clone(), *v))).collect();
        self.distance_ls = s.distance_from_arrival_ls;
        self.star_type = s.star_type.clone();
        self.star_subclass = s.subclass;
        self.luminosity = s.luminosity.clone();
        self.stellar_mass = s.stellar_mass;
        self.age_my = s.age_my;
        self.absolute_magnitude = s.absolute_magnitude;
        self.planet_class = s.planet_class.clone();
        self.terraform_state = s.terraform_state.clone().filter(|t| !t.is_empty());
        self.atmosphere = s.atmosphere.clone().filter(|t| !t.is_empty());
        self.atmosphere_type = s.atmosphere_type.clone().filter(|t| !t.is_empty());
        self.atmosphere_composition = s.atmosphere_composition.iter().map(|a| (a.name.clone(), a.percent)).collect();
        self.volcanism = s.volcanism.clone().filter(|t| !t.is_empty());
        self.mass_em = s.mass_em;
        self.radius_m = s.radius;
        self.gravity_ms2 = s.surface_gravity;
        self.temperature_k = s.surface_temperature;
        self.pressure_pa = s.surface_pressure;
        self.landable = s.landable.unwrap_or(false);
        self.materials = s.materials.iter().map(|m| (m.name.to_ascii_lowercase(), m.percent)).collect();
        self.composition = s.composition.as_ref().map(|c| (c.ice, c.rock, c.metal));
        // keep ring signals found earlier
        let old_rings = std::mem::take(&mut self.rings);
        self.rings = s
            .rings
            .iter()
            .map(|r| RingInfo {
                name: r.name.clone(),
                class: r.ring_class.clone(),
                mass_mt: r.mass_mt,
                inner_rad: r.inner_rad,
                outer_rad: r.outer_rad,
                signals: old_rings.iter().find(|o| o.name == r.name).map(|o| o.signals.clone()).unwrap_or_default(),
            })
            .collect();
        self.reserve_level = s.reserve_level.clone();
        self.tidal_lock = s.tidal_lock.unwrap_or(false);
        self.rotation_period_s = s.rotation_period;
        self.orbital_period_s = s.orbital_period;
        self.semi_major_axis_m = s.semi_major_axis;
        self.eccentricity = s.eccentricity;
        self.inclination = s.orbital_inclination;
        self.axial_tilt = s.axial_tilt;
        // Keep what the FIRST scan said: later rescans reflect our own discovery/landing.
        if self.was_discovered.is_none() {
            self.was_discovered = s.was_discovered;
        }
        if self.was_mapped.is_none() {
            self.was_mapped = s.was_mapped;
        }
        if self.was_footfalled.is_none() {
            self.was_footfalled = s.was_footfalled;
        }
        self.scan_type = s.scan_type.clone();
        self.scanned = ts;
        self.has_scan = true;
        self.recompute_value(odyssey);
    }
}

#[derive(Debug, Clone)]
pub struct FssSignal {
    pub name: String,
    pub kind: Option<String>,
    pub uss_type: Option<String>,
    pub is_station: bool,
    pub threat: Option<u32>,
    pub faction: Option<String>,
    /// When it was last logged (fleet carriers come and go between visits).
    pub ts: Timestamp,
}

#[derive(Debug, Clone)]
pub struct CodexRecord {
    pub ts: Timestamp,
    pub name: String,
    pub category: String,
    pub sub_category: String,
    pub region: String,
    pub system: String,
    pub body_id: Option<u32>,
    pub new_entry: bool,
    pub voucher: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct SystemScan {
    pub address: u64,
    pub name: String,
    pub star_pos: Option<[f64; 3]>,
    pub first_visit: Timestamp,
    pub last_visit: Timestamp,
    pub fss_body_count: Option<u32>,
    pub fss_non_body_count: Option<u32>,
    pub fss_progress: f64,
    pub all_bodies_found: bool,
    pub nav_beacon: bool,
    pub bodies: BTreeMap<u32, Body>,
    pub signals: Vec<FssSignal>,
    pub sold: bool,
    /// When the data was last sold (Universal Cartographics).
    pub sold_at: Option<Timestamp>,
    /// The data was lost when the ship was destroyed.
    pub lost: bool,
    /// Scanned before a sale but not included in it. The game sells everything it will buy, so
    /// this data is very likely not sellable (shown separately, excluded from unsold totals).
    pub skipped_in_sale: bool,
}

/// Where a system's cartographic data stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataStatus {
    Unsold,
    Sold(Timestamp),
    /// Left out of a sale: the game won't buy it.
    NotSellable,
    /// Lost with the ship.
    Lost,
}

/// One sale of cartographic data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataSale {
    pub ts: Timestamp,
    pub systems: usize,
    pub earnings: i64,
}

impl SystemScan {
    fn new(address: u64, name: &str, ts: Timestamp) -> Self {
        SystemScan {
            address,
            name: name.to_string(),
            star_pos: None,
            first_visit: ts,
            last_visit: ts,
            fss_body_count: None,
            fss_non_body_count: None,
            fss_progress: 0.0,
            all_bodies_found: false,
            nav_beacon: false,
            bodies: BTreeMap::new(),
            signals: Vec::new(),
            sold: false,
            sold_at: None,
            lost: false,
            skipped_in_sale: false,
        }
    }

    pub fn status(&self) -> DataStatus {
        match (self.sold, self.lost, self.sold_at) {
            (true, true, _) => DataStatus::Lost,
            (true, false, Some(ts)) => DataStatus::Sold(ts),
            _ if self.skipped_in_sale => DataStatus::NotSellable,
            _ => DataStatus::Unsold,
        }
    }

    /// Estimated value of what we've scanned here (current state), plus full-scan/map bonuses.
    pub fn value(&self) -> i64 {
        let mut v: i64 = self.bodies.values().map(|b| b.value.current).sum();
        if self.all_bodies_found {
            v += xv::FULL_SCAN_BONUS_PER_BODY * self.fss_body_count.unwrap_or(0) as i64;
        }
        let mappable = self.bodies.values().filter(|b| b.is_mappable()).count();
        if mappable > 0 && self.all_bodies_found && self.bodies.values().filter(|b| b.is_mappable()).all(|b| b.mapped) {
            v += xv::FULL_MAP_BONUS_PER_BODY * mappable as i64;
        }
        v
    }

    /// Value if every planet worth mapping got mapped.
    pub fn potential_value(&self) -> i64 {
        self.bodies.values().map(|b| b.value.mapped.max(b.value.current)).sum()
    }

    pub fn scanned_bodies(&self) -> usize {
        self.bodies.values().filter(|b| b.has_scan && matches!(b.kind, BodyKind::Star | BodyKind::Planet)).count()
    }

    pub fn bio_signal_total(&self) -> u32 {
        self.bodies.values().map(|b| b.bio_signals).sum()
    }

    /// Nothing left worth doing here: the FSS found every body, every planet worth at least `high_value`
    /// credits once mapped has been mapped, and every body with biological signals has all of them analysed.
    /// Planets below the threshold don't need mapping. `high_value` is the Settings "High-value body" threshold.
    pub fn fully_explored(&self, high_value: i64) -> bool {
        self.all_bodies_found
            && self.bodies.values().all(|b| b.mapped || !b.is_mappable() || b.value.mapped < high_value)
            && self.bodies.values().all(Body::bio_done)
    }

    pub fn body_mut(&mut self, id: u32, name: &str) -> &mut Body {
        self.bodies.entry(id).or_insert_with(|| Body::placeholder(id, name))
    }
}

/// Why a body is worth a look (Observatory-style checks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Interest {
    EarthLike,
    WaterWorld,
    AmmoniaWorld,
    Terraformable,
    GasGiantLife,
    HighValue,
    FirstDiscovery,
    FirstFootfallOpportunity,
    LandableAtmosphere,
    BioSignals,
    GeoSignals,
    GuardianOrThargoid,
    CloseRings,
    WideRings,
    FastRotation,
    FastOrbit,
    HighEccentricity,
    TinyBody,
    LargeLandable,
    ShepherdMoon,
    NestedMoon,
    Jumponium,
    HotLandable,
}

impl Interest {
    pub fn label(self) -> &'static str {
        match self {
            Interest::EarthLike => "Earth-like world",
            Interest::WaterWorld => "Water world",
            Interest::AmmoniaWorld => "Ammonia world",
            Interest::Terraformable => "Terraformable",
            Interest::GasGiantLife => "Gas giant with life",
            Interest::HighValue => "High value",
            Interest::FirstDiscovery => "First discovery",
            Interest::FirstFootfallOpportunity => "First footfall possible",
            Interest::LandableAtmosphere => "Landable with atmosphere",
            Interest::BioSignals => "Biological signals",
            Interest::GeoSignals => "Geological signals",
            Interest::GuardianOrThargoid => "Guardian/Thargoid signals",
            Interest::CloseRings => "Rings very close to surface",
            Interest::WideRings => "Very wide rings",
            Interest::FastRotation => "Fast rotation",
            Interest::FastOrbit => "Fast orbit",
            Interest::HighEccentricity => "Highly eccentric orbit",
            Interest::TinyBody => "Tiny body",
            Interest::LargeLandable => "Large landable",
            Interest::ShepherdMoon => "Shepherd moon (orbits inside rings)",
            Interest::NestedMoon => "Moon of a moon",
            Interest::Jumponium => "FSD boost materials (jumponium)",
            Interest::HotLandable => "Very hot landable",
        }
    }
}

/// Bodies above this current-value (credits) count as "high value".
pub const HIGH_VALUE_THRESHOLD: i64 = 1_000_000;

pub fn interesting(body: &Body, system: &SystemScan) -> Vec<Interest> {
    let mut out = Vec::new();
    if !body.has_scan {
        if body.bio_signals > 0 {
            out.push(Interest::BioSignals);
        }
        return out;
    }
    let class = body.planet_class.as_deref().unwrap_or("");
    match class {
        "Earthlike body" => out.push(Interest::EarthLike),
        "Water world" => out.push(Interest::WaterWorld),
        "Ammonia world" => out.push(Interest::AmmoniaWorld),
        "Gas giant with water based life" | "Gas giant with ammonia based life" | "Water giant with life" => {
            out.push(Interest::GasGiantLife)
        }
        _ => {}
    }
    if body.is_terraformable() {
        out.push(Interest::Terraformable);
    }
    if body.value.mapped >= HIGH_VALUE_THRESHOLD || body.value.current >= HIGH_VALUE_THRESHOLD {
        out.push(Interest::HighValue);
    }
    if body.first_discovery() && body.kind != BodyKind::Ring && body.kind != BodyKind::BeltCluster {
        out.push(Interest::FirstDiscovery);
    }
    if body.landable && body.was_footfalled == Some(false) {
        out.push(Interest::FirstFootfallOpportunity);
    }
    if body.landable && body.has_atmosphere() {
        out.push(Interest::LandableAtmosphere);
    }
    if body.bio_signals > 0 {
        out.push(Interest::BioSignals);
    }
    if body.geo_signals > 0 {
        out.push(Interest::GeoSignals);
    }
    if body.signals.iter().any(|(k, _)| {
        let k = k.to_ascii_lowercase();
        k.contains("guardian") || k.contains("thargoid")
    }) {
        out.push(Interest::GuardianOrThargoid);
    }
    // Ring checks only make sense for planets (stars' asteroid belts are always "wide").
    if let (BodyKind::Planet, Some(r), Some(first)) = (body.kind, body.radius_m, body.rings.first()) {
        if first.inner_rad > 0.0 && first.inner_rad < r * 1.1 {
            out.push(Interest::CloseRings);
        }
        if body.rings.iter().any(|ring| ring.outer_rad - ring.inner_rad > r * 5.0) {
            out.push(Interest::WideRings);
        }
    }
    if body.kind == BodyKind::Planet && body.rotation_period_s.is_some_and(|p| p.abs() < 3_600.0 * 2.0) && !body.tidal_lock {
        out.push(Interest::FastRotation);
    }
    if body.orbital_period_s.is_some_and(|p| p.abs() < 3_600.0 * 8.0) && body.orbital_period_s.is_some_and(|p| p > 0.0) {
        out.push(Interest::FastOrbit);
    }
    if body.eccentricity.is_some_and(|e| e > 0.9) {
        out.push(Interest::HighEccentricity);
    }
    if body.kind == BodyKind::Planet && body.radius_m.is_some_and(|r| r < 300_000.0) {
        out.push(Interest::TinyBody);
    }
    if body.landable && body.radius_m.is_some_and(|r| r > 15_000_000.0) {
        out.push(Interest::LargeLandable);
    }
    if body.landable && body.temperature_k.is_some_and(|t| t > 1_800.0) {
        out.push(Interest::HotLandable);
    }
    // Moon-of-a-moon: the nearest parent is a planet that itself has a planet parent.
    if let Some((kind, pid)) = body.parents.first()
        && kind == "Planet"
            && let Some(parent) = system.bodies.get(pid) {
                if parent.parents.first().is_some_and(|(k, _)| k == "Planet") {
                    out.push(Interest::NestedMoon);
                }
                // Shepherd moon: orbit inside the parent's ring system.
                if let (Some(a), Some(outer)) = (body.semi_major_axis_m, parent.rings.iter().map(|r| r.outer_rad).reduce(f64::max))
                    && a < outer {
                        out.push(Interest::ShepherdMoon);
                    }
            }
    if body.landable {
        const JUMPONIUM: [&str; 6] = ["arsenic", "germanium", "niobium", "polonium", "vanadium", "yttrium"];
        let have = JUMPONIUM.iter().filter(|j| body.materials.iter().any(|(m, _)| m == *j)).count();
        if have >= 4 {
            out.push(Interest::Jumponium);
        }
    }
    out
}

/// What the exploration module holds.
#[derive(Debug, Clone, Default)]
pub struct Exploration {
    pub systems: FxHashMap<u64, SystemScan>,
    pub current: Option<u64>,
    pub codex: Vec<CodexRecord>,
    pub sold_total: i64,
    /// Every sale of cartographic data, oldest first.
    pub sales: Vec<DataSale>,
    pub odyssey: bool,
}

impl Exploration {
    pub fn system_mut(&mut self, address: u64, name: &str, ts: Timestamp) -> &mut SystemScan {
        let s = self.systems.entry(address).or_insert_with(|| SystemScan::new(address, name, ts));
        if s.name.is_empty() && !name.is_empty() {
            s.name = name.to_string();
        }
        s
    }

    pub fn current_system(&self) -> Option<&SystemScan> {
        self.current.and_then(|a| self.systems.get(&a))
    }

    pub fn enter_system(&mut self, address: u64, name: &str, pos: Option<[f64; 3]>, ts: Timestamp) {
        let s = self.system_mut(address, name, ts);
        s.last_visit = ts;
        if pos.is_some() {
            s.star_pos = pos;
        }
        self.current = Some(address);
    }

    /// Returns (system address, body id) of the scanned body.
    pub fn on_scan(&mut self, s: &ev::Scan, ts: Timestamp) -> (u64, u32) {
        let odyssey = self.odyssey;
        let sys = self.system_mut(s.system_address, &s.star_system, ts);
        // A system we sold data for and come back to: new scans are new unsold data.
        let body = sys.body_mut(s.body_id, &s.body_name);
        let was_scanned = body.has_scan && body.scan_type != "NavBeaconDetail";
        body.apply_scan(s, ts, odyssey);
        if !was_scanned && s.scan_type != "NavBeaconDetail" {
            sys.sold = false;
            sys.lost = false;
            sys.skipped_in_sale = false;
        }
        let is_star = s.is_star();
        let has_bio = sys.bodies.get(&s.body_id).is_some_and(|b| b.bio_signals > 0);
        if is_star {
            // Parent-star facts changed for every bio body of the system.
            self.update_predictions(s.system_address, None);
        } else if has_bio {
            self.update_predictions(s.system_address, Some(s.body_id));
        }
        // Rings of this body may already carry signals from SAASignalsFound on the ring itself.
        (s.system_address, s.body_id)
    }

    pub fn on_fss_discovery(&mut self, f: &ev::FSSDiscoveryScan, ts: Timestamp) {
        let sys = self.system_mut(f.system_address, &f.system_name, ts);
        sys.fss_body_count = Some(f.body_count);
        sys.fss_non_body_count = Some(f.non_body_count);
        sys.fss_progress = f.progress.clamp(0.0, 1.0);
    }

    pub fn on_all_bodies_found(&mut self, f: &ev::FSSAllBodiesFound, ts: Timestamp) {
        let sys = self.system_mut(f.system_address, &f.system_name, ts);
        sys.all_bodies_found = true;
        sys.fss_progress = 1.0;
        if sys.fss_body_count.is_none() {
            sys.fss_body_count = Some(f.count);
        }
    }

    pub fn on_body_signals(&mut self, b: &ev::BodySignals, from_dss: bool, ts: Timestamp) {
        let sys = self.system_mut(b.system_address, "", ts);
        // Ring hotspots are reported against the ring's own name/ID.
        if b.body_name.ends_with(" Ring") {
            let ring_name = b.body_name.clone();
            let sigs: Vec<(String, u32)> =
                b.signals.iter().map(|s| (s.kind_localised.clone().unwrap_or_else(|| s.kind.clone()), s.count)).collect();
            for body in sys.bodies.values_mut() {
                if let Some(r) = body.rings.iter_mut().find(|r| r.name == ring_name) {
                    r.signals = sigs.clone();
                }
            }
            // Keep a ring record too so we know about rings whose parent we haven't scanned.
            let ring = sys.body_mut(b.body_id, &b.body_name);
            ring.kind = BodyKind::Ring;
            ring.signals = sigs;
            return;
        }
        let body = sys.body_mut(b.body_id, &b.body_name);
        for s in &b.signals {
            let label = s.kind_localised.clone().unwrap_or_else(|| nexus_data::names::humanize_symbol(&s.kind));
            if let Some(e) = body.signals.iter_mut().find(|(k, _)| *k == label) {
                e.1 = s.count;
            } else {
                body.signals.push((label, s.count));
            }
            let k = s.kind.to_ascii_lowercase();
            if k.contains("biological") {
                body.bio_signals = s.count;
            } else if k.contains("geological") {
                body.geo_signals = s.count;
            } else if k.contains("planetarymininglocation") {
                body.mining_locations = s.count;
            }
        }
        let (addr, id) = (b.system_address, b.body_id);
        if from_dss {
            for g in &b.genuses {
                let name = g.genus_localised.clone().unwrap_or_else(|| nexus_data::names::humanize_symbol(&g.genus));
                if !body.genuses.contains(&name) {
                    body.genuses.push(name);
                }
            }
        }
        self.update_predictions(addr, Some(id));
    }

    pub fn on_saa_complete(&mut self, s: &ev::SAAScanComplete, ts: Timestamp) {
        let odyssey = self.odyssey;
        let sys = self.system_mut(s.system_address, "", ts);
        let body = sys.body_mut(s.body_id, &s.body_name);
        body.mapped = true;
        body.probes_used = Some(s.probes_used);
        body.efficiency_target = Some(s.efficiency_target);
        body.recompute_value(odyssey);
        sys.sold = false;
        sys.lost = false;
    }

    pub fn on_fss_signal(&mut self, f: &ev::FSSSignalDiscovered, system_name: &str, ts: Timestamp) {
        let sys = self.system_mut(f.system_address, system_name, ts);
        let name = f.signal_name_localised.clone().unwrap_or_else(|| f.signal_name.clone());
        if let Some(s) = sys.signals.iter_mut().find(|s| s.name == name) {
            // Seen again (another visit): fleet carriers come and go, so "when" matters.
            s.ts = s.ts.max(ts);
            return;
        }
        sys.signals.push(FssSignal {
            name,
            kind: f.signal_type.clone(),
            uss_type: f.uss_type_localised.clone().or_else(|| f.uss_type.clone()),
            is_station: f.is_station.unwrap_or(false),
            threat: f.threat_level,
            faction: f.spawning_faction.clone(),
            ts,
        });
    }

    pub fn on_nav_beacon(&mut self, n: &ev::NavBeaconScan, system_name: &str, ts: Timestamp) {
        let sys = self.system_mut(n.system_address, system_name, ts);
        sys.nav_beacon = true;
        if sys.fss_body_count.is_none() {
            sys.fss_body_count = Some(n.num_bodies);
        }
    }

    pub fn on_codex(&mut self, c: &ev::CodexEntry, ts: Timestamp) {
        let known_region = self.region_of(&c.system).is_some();
        self.codex.push(CodexRecord {
            ts,
            name: c.name_localised.clone().unwrap_or_else(|| c.name.clone()),
            category: c.category_localised.clone().unwrap_or_else(|| c.category.clone()),
            sub_category: c.sub_category_localised.clone().unwrap_or_else(|| c.sub_category.clone()),
            region: c.region_localised.clone().unwrap_or_else(|| c.region.clone()),
            system: c.system.clone(),
            body_id: c.body_id,
            new_entry: c.is_new_entry.unwrap_or(false),
            voucher: c.voucher_amount,
        });
        if !known_region && c.body_id.is_some() {
            self.update_predictions(c.system_address, None);
        }
    }

    pub fn on_footfall(&mut self, system_address: u64, body_id: u32) {
        if let Some(b) = self.systems.get_mut(&system_address).and_then(|s| s.bodies.get_mut(&body_id)) {
            b.footfall = true;
        }
    }

    /// Selling marks the listed systems as sold; returns the number matched. Systems scanned
    /// before the sale that the game did not buy are flagged `skipped_in_sale` (verified on real
    /// journals: data the game refuses stays unlisted across every later sale).
    pub fn on_sell(&mut self, systems: &[String], earnings: i64, ts: Timestamp) -> usize {
        self.sold_total += earnings;
        self.sales.push(DataSale { ts, systems: systems.len(), earnings });
        let mut matched = 0;
        for s in self.systems.values_mut() {
            if s.sold {
                continue;
            }
            if systems.iter().any(|n| n.eq_ignore_ascii_case(&s.name)) {
                s.sold = true;
                s.sold_at = Some(ts);
                s.skipped_in_sale = false;
                matched += 1;
            } else if s.last_visit <= ts && s.scanned_bodies() > 0 && self.current != Some(s.address) {
                s.skipped_in_sale = true;
            }
        }
        matched
    }

    /// Unsold cartographic data is lost on death.
    pub fn on_death(&mut self) {
        for s in self.systems.values_mut() {
            if !s.sold {
                s.sold = true;
                s.lost = s.scanned_bodies() > 0;
            }
        }
    }

    /// Every system with scanned bodies, whatever became of the data.
    pub fn history(&self) -> impl Iterator<Item = &SystemScan> {
        self.systems.values().filter(|s| s.scanned_bodies() > 0)
    }

    pub fn last_sale(&self) -> Option<&DataSale> {
        self.sales.last()
    }

    pub fn unsold(&self) -> impl Iterator<Item = &SystemScan> {
        self.systems.values().filter(|s| !s.sold && !s.skipped_in_sale && s.scanned_bodies() > 0)
    }

    /// Data the game left out of a previous sale (probably not sellable).
    pub fn skipped(&self) -> impl Iterator<Item = &SystemScan> {
        self.systems.values().filter(|s| !s.sold && s.skipped_in_sale)
    }

    pub fn unsold_value(&self) -> i64 {
        self.unsold().map(SystemScan::value).sum()
    }

    pub fn set_odyssey(&mut self, odyssey: bool) {
        if self.odyssey != odyssey {
            self.odyssey = odyssey;
            for s in self.systems.values_mut() {
                for b in s.bodies.values_mut() {
                    b.recompute_value(odyssey);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Exobiology predictions
// ---------------------------------------------------------------------------------------------

use nexus_data::bio_predict::{self as bp, BodyFacts, ParentRef, StarFacts, SystemStar};

fn parent_ref(kind: &str, id: u32) -> ParentRef {
    match kind {
        "Star" => ParentRef::Star(id),
        "Planet" => ParentRef::Planet(id),
        "Ring" => ParentRef::Ring(id),
        _ => ParentRef::Barycentre(id),
    }
}

fn star_facts(b: &Body) -> Option<StarFacts> {
    Some(StarFacts { star_type: b.star_type.clone()?, luminosity: b.luminosity.clone(), subclass: b.star_subclass })
}

/// Builds the prediction input for a body from what the model knows about it and its system.
pub fn body_facts(b: &Body, sys: &SystemScan, region: Option<String>) -> Option<BodyFacts> {
    let planet_class = b.planet_class.clone()?;
    let stars: Vec<SystemStar> = sys
        .bodies
        .values()
        .filter_map(|s| {
            Some(SystemStar {
                body_id: s.id,
                parent: s.parents.first().map(|(k, id)| parent_ref(k, *id)),
                distance_ls: s.distance_ls,
                facts: star_facts(s)?,
            })
        })
        .collect();
    let parents: Vec<ParentRef> = b.parents.iter().map(|(k, id)| parent_ref(k, *id)).collect();
    Some(BodyFacts {
        planet_class,
        atmosphere_type: b.atmosphere_type.clone(),
        atmosphere: b.atmosphere.clone(),
        atmosphere_composition: b.atmosphere_composition.clone(),
        gravity_g: b.gravity_ms2.map(|g| g / nexus_data::exobio::JOURNAL_G_MS2),
        temperature_k: b.temperature_k,
        pressure_atm: b.pressure_pa.map(|p| p / nexus_data::exobio::JOURNAL_ATM_PA),
        volcanism: b.volcanism.clone(),
        landable: b.landable,
        materials: b.materials.clone(),
        parent_stars: bp::resolve_parent_stars(&parents, &stars),
        distance_from_arrival_ls: b.distance_ls,
        system_name: sys.name.clone(),
        star_pos: sys.star_pos,
        region,
        bio_signal_count: (b.bio_signals > 0).then_some(b.bio_signals),
        system_stars: stars.into_iter().map(|s| s.facts).collect(),
        system_body_classes: sys.bodies.values().filter_map(|x| x.planet_class.clone()).collect(),
        orbital_period_s: b.orbital_period_s,
        ..Default::default()
    })
}

impl Exploration {
    /// Codex region known for a system (from a codex entry logged there), if any.
    pub fn region_of(&self, system: &str) -> Option<String> {
        self.codex.iter().rev().find(|c| c.system.eq_ignore_ascii_case(system)).map(|c| c.region.clone())
    }

    /// (Re)computes species predictions for bio bodies of a system (all of them, or one).
    pub fn update_predictions(&mut self, address: u64, only: Option<u32>) {
        let Some(sys) = self.systems.get(&address) else { return };
        let region = self.region_of(&sys.name);
        let mut results: Vec<(u32, Vec<bp::Prediction>)> = Vec::new();
        for b in sys.bodies.values() {
            if b.bio_signals == 0 || only.is_some_and(|id| id != b.id) {
                continue;
            }
            if let Some(facts) = body_facts(b, sys, region.clone()) {
                results.push((b.id, bp::predict(&facts, &b.genuses)));
            }
        }
        if let Some(sys) = self.systems.get_mut(&address) {
            for (id, preds) in results {
                if let Some(b) = sys.bodies.get_mut(&id) {
                    b.predictions = preds;
                }
            }
        }
    }
}

impl Body {
    /// Expected Vista Genomics value of the body: likelihood-weighted over all predictions.
    pub fn predicted_value(&self, bonus: bool) -> i64 {
        self.predictions.iter().map(|p| bp::expected_value(p, bonus)).sum()
    }

    /// Most likely species per genus (what you'll probably find), best first.
    pub fn likely_species(&self) -> Vec<&bp::Prediction> {
        let mut seen: Vec<&str> = Vec::new();
        let mut out = Vec::new();
        for p in &self.predictions {
            if !seen.contains(&p.genus.as_str()) {
                seen.push(&p.genus);
                out.push(p);
            }
        }
        out
    }
}