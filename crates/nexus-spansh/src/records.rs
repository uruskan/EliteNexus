//! Snake-case records returned by the search and lookup endpoints:
//! `/api/{systems,bodies,stations}/search`, `/api/system|body|station/{id}`, `/api/search`,
//! `/api/nearest`, autocomplete and the field catalogue. (The camelCase dump records live in
//! [`crate::dump`].)

use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::de;
use crate::util::{Coords, parse_timestamp};

/// A named system with coordinates (autocomplete, `/api/nearest`, search reference points).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemPoint {
    #[serde(deserialize_with = "de::opt_u64")]
    pub id64: Option<u64>,
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    #[serde(deserialize_with = "de::f64_or_zero")]
    pub x: f64,
    #[serde(deserialize_with = "de::f64_or_zero")]
    pub y: f64,
    #[serde(deserialize_with = "de::f64_or_zero")]
    pub z: f64,
    /// From the query point / reference, ly (when the endpoint reports it).
    #[serde(deserialize_with = "de::opt_f64")]
    pub distance: Option<f64>,
}

impl SystemPoint {
    pub fn coords(&self) -> Coords {
        Coords::new(self.x, self.y, self.z)
    }
}

/// `{name, share}` — economies (share %), atmosphere/solid composition, materials.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NameShare {
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    #[serde(deserialize_with = "de::f64_or_zero")]
    pub share: f64,
}

/// `{name, count}` — body and ring signals (`Biological`, `Painite`, …).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NameCount {
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    #[serde(deserialize_with = "de::u32_or_zero")]
    pub count: u32,
}

/// Surface landmark: exobiology species (`type` = genus, `subtype` = species), surface stations,
/// tourist beacons, ….
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Landmark {
    #[serde(rename = "type", deserialize_with = "de::string")]
    pub kind: String,
    #[serde(deserialize_with = "de::string")]
    pub subtype: String,
    #[serde(deserialize_with = "de::opt_string")]
    pub variant: Option<String>,
    #[serde(deserialize_with = "de::opt_u32")]
    pub count: Option<u32>,
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub value: u64,
}

// =============================================================================================
// stations

/// One market row of a station record.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarketEntry {
    #[serde(deserialize_with = "de::string")]
    pub commodity: String,
    #[serde(deserialize_with = "de::opt_string")]
    pub category: Option<String>,
    /// What we pay, cr (0 = not sold here).
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub buy_price: u64,
    /// What we get, cr.
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub sell_price: u64,
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub supply: u64,
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub demand: u64,
    #[serde(deserialize_with = "de::bool_or_false")]
    pub is_rare: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutfittingEntry {
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    #[serde(deserialize_with = "de::opt_string")]
    pub category: Option<String>,
    #[serde(deserialize_with = "de::opt_u32")]
    pub class: Option<u32>,
    #[serde(deserialize_with = "de::opt_string")]
    pub rating: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub ed_symbol: Option<String>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub price: Option<u64>,
    /// Ship-specific modules (armour, …).
    #[serde(deserialize_with = "de::opt_string")]
    pub ship: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShipyardEntry {
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    #[serde(deserialize_with = "de::opt_u64")]
    pub price: Option<u64>,
    #[serde(deserialize_with = "de::opt_string")]
    pub symbol: Option<String>,
}

/// Station / carrier / settlement as returned by `/api/stations/search` and `/api/station/{id}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StationRecord {
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub market_id: u64,
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    /// `Coriolis Starport`, `Drake-Class Carrier`, `Planetary Outpost`, ….
    #[serde(rename = "type", deserialize_with = "de::opt_string")]
    pub station_type: Option<String>,
    /// `UnderRepairs`, `Construction`, ….
    #[serde(deserialize_with = "de::opt_string")]
    pub state: Option<String>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub is_planetary: Option<bool>,
    /// From the search reference, ly.
    #[serde(deserialize_with = "de::opt_f64")]
    pub distance: Option<f64>,
    /// ls.
    #[serde(deserialize_with = "de::opt_f64")]
    pub distance_to_arrival: Option<f64>,
    #[serde(deserialize_with = "de::opt_string")]
    pub updated_at: Option<String>,

    #[serde(deserialize_with = "de::opt_string")]
    pub allegiance: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub government: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub controlling_minor_faction: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub controlling_minor_faction_state: Option<String>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub controlling_minor_faction_influence: Option<f64>,
    #[serde(deserialize_with = "de::opt_string")]
    pub primary_economy: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub secondary_economy: Option<String>,
    #[serde(deserialize_with = "de::nullable")]
    pub economies: Vec<NameShare>,

    #[serde(deserialize_with = "de::opt_bool")]
    pub has_large_pad: Option<bool>,
    #[serde(deserialize_with = "de::opt_u32")]
    pub large_pads: Option<u32>,
    #[serde(deserialize_with = "de::opt_u32")]
    pub medium_pads: Option<u32>,
    #[serde(deserialize_with = "de::opt_u32")]
    pub small_pads: Option<u32>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub has_market: Option<bool>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub has_outfitting: Option<bool>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub has_shipyard: Option<bool>,
    /// `Dock`, `Market`, `Material Trader`, `Interstellar Factors Contact`, ….
    #[serde(deserialize_with = "de::name_list")]
    pub services: Vec<String>,
    /// `None`, `Raw`, `Manufactured`, `Encoded`.
    #[serde(deserialize_with = "de::opt_string")]
    pub material_trader: Option<String>,
    /// `None`, `Guardian`, `Human`.
    #[serde(deserialize_with = "de::opt_string")]
    pub technology_broker: Option<String>,

    #[serde(deserialize_with = "de::nullable")]
    pub market: Vec<MarketEntry>,
    #[serde(deserialize_with = "de::opt_string")]
    pub market_updated_at: Option<String>,
    #[serde(deserialize_with = "de::name_list")]
    pub export_commodities: Vec<String>,
    #[serde(deserialize_with = "de::name_list")]
    pub import_commodities: Vec<String>,
    #[serde(deserialize_with = "de::name_list")]
    pub prohibited_commodities: Vec<String>,
    #[serde(deserialize_with = "de::nullable")]
    pub modules: Vec<OutfittingEntry>,
    #[serde(deserialize_with = "de::opt_string")]
    pub outfitting_updated_at: Option<String>,
    #[serde(deserialize_with = "de::nullable")]
    pub ships: Vec<ShipyardEntry>,
    #[serde(deserialize_with = "de::opt_string")]
    pub shipyard_updated_at: Option<String>,

    #[serde(deserialize_with = "de::opt_string")]
    pub carrier_name: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub carrier_docking_access: Option<String>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub is_dssa_carrier: Option<bool>,
    /// Name given by the colonising architect.
    #[serde(deserialize_with = "de::opt_string")]
    pub real_name: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub body_name: Option<String>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub body_id64: Option<u64>,

    #[serde(deserialize_with = "de::string")]
    pub system_name: String,
    #[serde(deserialize_with = "de::opt_u64")]
    pub system_id64: Option<u64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub system_x: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub system_y: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub system_z: Option<f64>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub system_population: Option<u64>,
    #[serde(deserialize_with = "de::opt_string")]
    pub system_primary_economy: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub system_secondary_economy: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub system_controlling_power: Option<String>,
    #[serde(deserialize_with = "de::name_list")]
    pub system_power: Vec<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub system_power_state: Option<String>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub system_is_colonised: Option<bool>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub system_is_being_colonised: Option<bool>,
}

impl StationRecord {
    /// Market row for a commodity display name (case-insensitive).
    pub fn commodity(&self, name: &str) -> Option<&MarketEntry> {
        self.market
            .iter()
            .find(|m| m.commodity.eq_ignore_ascii_case(name))
    }

    pub fn has_service(&self, service: &str) -> bool {
        self.services
            .iter()
            .any(|s| s.eq_ignore_ascii_case(service))
    }

    /// Drake-class (player) or squadron carrier.
    pub fn is_fleet_carrier(&self) -> bool {
        self.station_type
            .as_deref()
            .is_some_and(|t| t.contains("Carrier"))
            || self.carrier_name.is_some()
    }

    pub fn system_coords(&self) -> Option<Coords> {
        Coords::from_parts(self.system_x, self.system_y, self.system_z)
    }

    /// Market data timestamp as unix seconds.
    pub fn market_updated_unix(&self) -> Option<i64> {
        self.market_updated_at.as_deref().and_then(parse_timestamp)
    }

    /// The outfitting entry for a module symbol, any case (`Hpt_MultiCannon_Gimbal_Huge`). Its
    /// name may be a MercGear one: those share their base module's symbol.
    pub fn module(&self, symbol: &str) -> Option<&OutfittingEntry> {
        self.modules.iter().find(|m| {
            m.ed_symbol
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(symbol))
        })
    }

    /// The shipyard entry for a ship symbol, any case (`Anaconda`, `krait_mkii`).
    pub fn ship(&self, symbol: &str) -> Option<&ShipyardEntry> {
        self.ships.iter().find(|s| {
            s.symbol
                .as_deref()
                .is_some_and(|x| x.eq_ignore_ascii_case(symbol))
        })
    }

    /// Outfitting data timestamp as unix seconds.
    pub fn outfitting_updated_unix(&self) -> Option<i64> {
        self.outfitting_updated_at.as_deref().and_then(parse_timestamp)
    }

    /// Shipyard data timestamp as unix seconds.
    pub fn shipyard_updated_unix(&self) -> Option<i64> {
        self.shipyard_updated_at.as_deref().and_then(parse_timestamp)
    }
}

// =============================================================================================
// bodies

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BodyParent {
    #[serde(deserialize_with = "de::opt_u64")]
    pub id64: Option<u64>,
    /// `Star`, `Planet`, `Null` (barycentre).
    #[serde(rename = "type", deserialize_with = "de::opt_string")]
    pub kind: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub subtype: Option<String>,
}

/// A planetary ring or an asteroid belt.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RingRecord {
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    /// `Icy`, `Metal Rich`, `Metallic`, `Rocky`.
    #[serde(rename = "type", deserialize_with = "de::opt_string")]
    pub ring_type: Option<String>,
    /// Mt.
    #[serde(deserialize_with = "de::opt_f64")]
    pub mass: Option<f64>,
    /// m.
    #[serde(deserialize_with = "de::opt_f64")]
    pub inner_radius: Option<f64>,
    /// m.
    #[serde(deserialize_with = "de::opt_f64")]
    pub outer_radius: Option<f64>,
    #[serde(deserialize_with = "de::opt_u32")]
    pub signal_count: Option<u32>,
    /// Hotspots by commodity display name (`Low Temperature Diamonds`, `Painite`, …).
    #[serde(deserialize_with = "de::nullable")]
    pub signals: Vec<NameCount>,
    #[serde(deserialize_with = "de::opt_string")]
    pub signals_updated_at: Option<String>,
}

impl RingRecord {
    /// Hotspots of `commodity` in this ring (case-insensitive).
    pub fn hotspots(&self, commodity: &str) -> u32 {
        self.signals
            .iter()
            .filter(|s| s.name.eq_ignore_ascii_case(commodity))
            .map(|s| s.count)
            .sum()
    }
}

/// Body as returned by `/api/bodies/search` and `/api/body/{id64}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BodyRecord {
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub id64: u64,
    #[serde(deserialize_with = "de::opt_u32")]
    pub body_id: Option<u32>,
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    /// `Star`, `Planet`, `Barycentre`.
    #[serde(rename = "type", deserialize_with = "de::opt_string")]
    pub body_type: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub subtype: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub updated_at: Option<String>,
    /// From the search reference, ly.
    #[serde(deserialize_with = "de::opt_f64")]
    pub distance: Option<f64>,
    /// ls.
    #[serde(deserialize_with = "de::opt_f64")]
    pub distance_to_arrival: Option<f64>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub is_main_star: Option<bool>,

    // stars
    #[serde(deserialize_with = "de::opt_string")]
    pub spectral_class: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub luminosity_class: Option<String>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub solar_masses: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub solar_radius: Option<f64>,
    /// Myr.
    #[serde(deserialize_with = "de::opt_f64")]
    pub age: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub absolute_magnitude: Option<f64>,

    // planets
    /// g.
    #[serde(deserialize_with = "de::opt_f64")]
    pub gravity: Option<f64>,
    /// km.
    #[serde(deserialize_with = "de::opt_f64")]
    pub radius: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub earth_masses: Option<f64>,
    /// K.
    #[serde(deserialize_with = "de::opt_f64")]
    pub surface_temperature: Option<f64>,
    /// atm.
    #[serde(deserialize_with = "de::opt_f64")]
    pub surface_pressure: Option<f64>,
    #[serde(deserialize_with = "de::opt_string")]
    pub atmosphere: Option<String>,
    #[serde(deserialize_with = "de::nullable")]
    pub atmosphere_composition: Vec<NameShare>,
    #[serde(deserialize_with = "de::nullable")]
    pub solid_composition: Vec<NameShare>,
    #[serde(deserialize_with = "de::nullable")]
    pub materials: Vec<NameShare>,
    #[serde(deserialize_with = "de::opt_string")]
    pub volcanism_type: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub terraforming_state: Option<String>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub is_landable: Option<bool>,
    /// Ring/belt reserve: `Pristine`, `Major`, `Common`, `Low`, `Depleted`.
    #[serde(deserialize_with = "de::opt_string")]
    pub reserve_level: Option<String>,

    // orbit
    /// days.
    #[serde(deserialize_with = "de::opt_f64")]
    pub orbital_period: Option<f64>,
    /// AU.
    #[serde(deserialize_with = "de::opt_f64")]
    pub semi_major_axis: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub orbital_eccentricity: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub orbital_inclination: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub arg_of_periapsis: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub ascending_node: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub mean_anomaly: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub axis_tilt: Option<f64>,
    /// days.
    #[serde(deserialize_with = "de::opt_f64")]
    pub rotational_period: Option<f64>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub is_rotational_period_tidally_locked: Option<bool>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub orbital_synchronicity: Option<f64>,
    #[serde(deserialize_with = "de::nullable")]
    pub parents: Vec<BodyParent>,

    // value & signals
    #[serde(deserialize_with = "de::opt_u64")]
    pub estimated_scan_value: Option<u64>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub estimated_mapping_value: Option<u64>,
    /// `Human`, `Biological`, `Geological`, ….
    #[serde(deserialize_with = "de::nullable")]
    pub signals: Vec<NameCount>,
    #[serde(deserialize_with = "de::opt_u32")]
    pub signal_count: Option<u32>,
    #[serde(deserialize_with = "de::opt_string")]
    pub signals_updated_at: Option<String>,
    /// Exobiology genera reported by DSS.
    #[serde(deserialize_with = "de::name_list")]
    pub genuses: Vec<String>,
    #[serde(deserialize_with = "de::nullable")]
    pub rings: Vec<RingRecord>,
    #[serde(deserialize_with = "de::nullable")]
    pub belts: Vec<RingRecord>,
    #[serde(deserialize_with = "de::nullable")]
    pub landmarks: Vec<Landmark>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub landmark_value: Option<u64>,

    // system
    #[serde(deserialize_with = "de::string")]
    pub system_name: String,
    #[serde(deserialize_with = "de::opt_u64")]
    pub system_id64: Option<u64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub system_x: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub system_y: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub system_z: Option<f64>,
    #[serde(deserialize_with = "de::opt_string")]
    pub system_region: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub system_controlling_power: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub system_power_state: Option<String>,
}

impl BodyRecord {
    /// Most hotspots of `commodity` in any single ring (what the `ring_signals` filter matches).
    pub fn hotspots(&self, commodity: &str) -> u32 {
        self.rings
            .iter()
            .map(|r| r.hotspots(commodity))
            .max()
            .unwrap_or(0)
    }

    /// The ring with the most `commodity` hotspots.
    pub fn best_ring(&self, commodity: &str) -> Option<&RingRecord> {
        self.rings
            .iter()
            .filter(|r| r.hotspots(commodity) > 0)
            .max_by_key(|r| r.hotspots(commodity))
    }

    /// Count of a body signal (`Biological`, `Geological`, …), case-insensitive.
    pub fn signal(&self, name: &str) -> u32 {
        self.signals
            .iter()
            .filter(|s| s.name.eq_ignore_ascii_case(name))
            .map(|s| s.count)
            .sum()
    }

    pub fn system_coords(&self) -> Option<Coords> {
        Coords::from_parts(self.system_x, self.system_y, self.system_z)
    }
}

// =============================================================================================
// systems

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FactionPresence {
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    #[serde(deserialize_with = "de::opt_string")]
    pub allegiance: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub government: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub state: Option<String>,
    /// 0..1.
    #[serde(deserialize_with = "de::opt_f64")]
    pub influence: Option<f64>,
    #[serde(deserialize_with = "de::name_list")]
    pub active_states: Vec<String>,
    #[serde(deserialize_with = "de::name_list")]
    pub pending_states: Vec<String>,
    #[serde(deserialize_with = "de::name_list")]
    pub recovering_states: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SynthesisRecipe {
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    /// `Basic`, `Standard`, `Premium`.
    #[serde(deserialize_with = "de::string")]
    pub level: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerProgress {
    #[serde(alias = "power", deserialize_with = "de::string")]
    pub name: String,
    #[serde(deserialize_with = "de::opt_f64")]
    pub progress: Option<f64>,
}

/// Body summary inside a system record.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemBody {
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub id64: u64,
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    #[serde(rename = "type", deserialize_with = "de::opt_string")]
    pub body_type: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub subtype: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub terraforming_state: Option<String>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub is_main_star: Option<bool>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub distance_to_arrival: Option<f64>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub estimated_scan_value: Option<u64>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub estimated_mapping_value: Option<u64>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub landmark_value: Option<u64>,
    #[serde(deserialize_with = "de::nullable")]
    pub landmarks: Vec<Landmark>,
}

/// Station summary inside a system record.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemStation {
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub market_id: u64,
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    #[serde(rename = "type", deserialize_with = "de::opt_string")]
    pub station_type: Option<String>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub body_id64: Option<u64>,
    #[serde(deserialize_with = "de::opt_string")]
    pub body_name: Option<String>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub distance_to_arrival: Option<f64>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub has_large_pad: Option<bool>,
    #[serde(deserialize_with = "de::opt_u32")]
    pub large_pads: Option<u32>,
    #[serde(deserialize_with = "de::opt_u32")]
    pub medium_pads: Option<u32>,
    #[serde(deserialize_with = "de::opt_u32")]
    pub small_pads: Option<u32>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub has_market: Option<bool>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub has_shipyard: Option<bool>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub has_outfitting: Option<bool>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub is_dssa_carrier: Option<bool>,
    #[serde(deserialize_with = "de::name_list")]
    pub services: Vec<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub controlling_minor_faction: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub controlling_minor_faction_state: Option<String>,
}

/// System as returned by `/api/system/{id64}`, `/api/systems/search` and `/api/search`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemRecord {
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub id64: u64,
    #[serde(deserialize_with = "de::string")]
    pub name: String,
    #[serde(deserialize_with = "de::f64_or_zero")]
    pub x: f64,
    #[serde(deserialize_with = "de::f64_or_zero")]
    pub y: f64,
    #[serde(deserialize_with = "de::f64_or_zero")]
    pub z: f64,
    /// From the search reference, ly.
    #[serde(deserialize_with = "de::opt_f64")]
    pub distance: Option<f64>,
    #[serde(deserialize_with = "de::opt_string")]
    pub region: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub updated_at: Option<String>,

    #[serde(deserialize_with = "de::opt_u64")]
    pub population: Option<u64>,
    #[serde(deserialize_with = "de::opt_string")]
    pub allegiance: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub government: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub security: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub state: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub primary_economy: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub secondary_economy: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub controlling_minor_faction: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub controlling_minor_faction_state: Option<String>,
    #[serde(deserialize_with = "de::nullable")]
    pub minor_faction_presences: Vec<FactionPresence>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub nearest_populated_distance: Option<f64>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub nearest_populated_id64: Option<u64>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub is_colonised: Option<bool>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub is_being_colonised: Option<bool>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub needs_permit: Option<bool>,
    #[serde(deserialize_with = "de::opt_bool")]
    pub known_permit: Option<bool>,

    #[serde(deserialize_with = "de::opt_string")]
    pub controlling_power: Option<String>,
    #[serde(deserialize_with = "de::name_list")]
    pub power: Vec<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub power_state: Option<String>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub power_state_control_progress: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub power_state_reinforcement: Option<f64>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub power_state_undermining: Option<f64>,
    #[serde(deserialize_with = "de::nullable")]
    pub power_conflict_progress: Vec<PowerProgress>,

    #[serde(deserialize_with = "de::opt_string")]
    pub thargoid_war_state: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub thargoid_war_success_state: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub thargoid_war_failure_state: Option<String>,
    #[serde(deserialize_with = "de::opt_f64")]
    pub thargoid_war_progress: Option<f64>,

    #[serde(deserialize_with = "de::opt_u32")]
    pub body_count: Option<u32>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub estimated_scan_value: Option<u64>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub estimated_mapping_value: Option<u64>,
    #[serde(deserialize_with = "de::opt_u64")]
    pub landmark_value: Option<u64>,
    #[serde(deserialize_with = "de::nullable")]
    pub bodies: Vec<SystemBody>,
    #[serde(deserialize_with = "de::nullable")]
    pub stations: Vec<SystemStation>,
    #[serde(deserialize_with = "de::nullable")]
    pub synthesis_recipes: Vec<SynthesisRecipe>,
}

impl SystemRecord {
    pub fn coords(&self) -> Coords {
        Coords::new(self.x, self.y, self.z)
    }

    pub fn main_star(&self) -> Option<&SystemBody> {
        self.bodies.iter().find(|b| b.is_main_star == Some(true))
    }
}

// =============================================================================================
// smart search, autocomplete, field catalogue

/// `/api/search?q=` answer.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct SmartSearchResponse {
    #[serde(deserialize_with = "de::u64_or_zero")]
    pub count: u64,
    #[serde(deserialize_with = "de::string")]
    pub query: String,
    #[serde(deserialize_with = "de::lenient_vec")]
    pub results: Vec<SmartSearchHit>,
}

/// One hit of the global search.
#[derive(Debug, Clone, PartialEq)]
pub enum SmartSearchHit {
    System(Box<SystemRecord>),
    Body(Box<BodyRecord>),
    Station(Box<StationRecord>),
    /// A record type this crate does not know yet.
    Other {
        kind: String,
        record: Value,
    },
}

impl SmartSearchHit {
    pub fn name(&self) -> &str {
        match self {
            SmartSearchHit::System(s) => &s.name,
            SmartSearchHit::Body(b) => &b.name,
            SmartSearchHit::Station(s) => &s.name,
            SmartSearchHit::Other { record, .. } => record
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        }
    }
}

impl<'de> Deserialize<'de> for SmartSearchHit {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(rename = "type", default, deserialize_with = "de::string")]
            kind: String,
            #[serde(default)]
            record: Value,
        }
        let raw = Raw::deserialize(d)?;
        let typed = match raw.kind.as_str() {
            "system" => serde_json::from_value(raw.record.clone())
                .map(|r| SmartSearchHit::System(Box::new(r))),
            "body" => serde_json::from_value(raw.record.clone())
                .map(|r| SmartSearchHit::Body(Box::new(r))),
            "station" => serde_json::from_value(raw.record.clone())
                .map(|r| SmartSearchHit::Station(Box::new(r))),
            _ => {
                return Ok(SmartSearchHit::Other {
                    kind: raw.kind,
                    record: raw.record,
                });
            }
        };
        Ok(typed.unwrap_or(SmartSearchHit::Other {
            kind: raw.kind,
            record: raw.record,
        }))
    }
}

/// `/api/systems/field_values/system_names?q=` answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemNames {
    /// Matching systems with id64 and coordinates.
    #[serde(deserialize_with = "de::lenient_vec")]
    pub min_max: Vec<SystemPoint>,
    /// Matching names only.
    #[serde(deserialize_with = "de::name_list")]
    pub values: Vec<String>,
}

/// `/api/{kind}/field_values/{field}` answer: enum values for a group field (flat or grouped),
/// or min/max for numeric sliders.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FieldValues {
    pub values: Value,
    pub min_max: Value,
}

impl FieldValues {
    /// Every string value, flattening groups.
    pub fn names(&self) -> Vec<String> {
        fn collect(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::String(s) => out.push(s.clone()),
                Value::Array(items) => items.iter().for_each(|i| collect(i, out)),
                Value::Object(m) => m.values().for_each(|i| collect(i, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        collect(&self.values, &mut out);
        out
    }
}

/// One entry of `/api/{kind}/searchable_fields`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchableField {
    #[serde(deserialize_with = "de::string")]
    pub field: String,
    /// `string`, `keyword`, `numeric`, `group`, `boolean`, `timestamp`, `value_struct`,
    /// `combined`, `special`.
    #[serde(rename = "type", deserialize_with = "de::string")]
    pub kind: String,
    /// `integer` / `real` for numeric fields.
    #[serde(deserialize_with = "de::opt_string")]
    pub subtype: Option<String>,
    #[serde(deserialize_with = "de::bool_or_false")]
    pub multiple: bool,
    #[serde(deserialize_with = "de::bool_or_false")]
    pub is_logarithmic: bool,
    /// Document path when it differs from `field` (e.g. `rings.signals`).
    #[serde(deserialize_with = "de::opt_string")]
    pub path: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub display_field: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub field_values: Option<String>,
    #[serde(deserialize_with = "de::opt_string")]
    pub field_values_url: Option<String>,
    /// `value_struct`: the name subfield.
    pub subfield: Option<Value>,
    /// `combined`: subfield name → `{order, primary, type}`.
    pub subfields: Option<Value>,
    /// `value_struct`: the numeric subfield.
    pub valuefield: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SearchableFields {
    #[serde(deserialize_with = "de::lenient_vec")]
    pub fields: Vec<SearchableField>,
}
