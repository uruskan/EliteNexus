//! Public data types: what goes into the database ([`Ingest`]) and what comes out of queries.

use crate::services::{MaterialTrader, Services, TechBroker};
use serde::{Deserialize, Serialize};

/// Where a piece of data came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub enum Source {
    /// The live EDDN feed.
    #[default]
    Eddn,
    /// The commander's own journal.
    Journal,
    Spansh,
    /// An older EDDN message looked up on demand in an archive (EDGalaxyData's EDDN index).
    EddnArchive,
}

impl Source {
    /// Stable code stored in the database.
    pub(crate) fn code(self) -> i64 {
        match self {
            Source::Eddn => 0,
            Source::Journal => 1,
            Source::Spansh => 2,
            Source::EddnArchive => 3,
        }
    }

    pub(crate) fn from_code(code: i64) -> Source {
        match code {
            1 => Source::Journal,
            2 => Source::Spansh,
            3 => Source::EddnArchive,
            _ => Source::Eddn,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Ingest
// ---------------------------------------------------------------------------------------------

/// One unit of data for the writer thread. Build them by hand, or with [`Ingest::from_eddn`] /
/// [`Ingest::from_journal_event`], or hand raw JSON over with [`Ingest::Raw`] and let the writer
/// convert it.
#[derive(Debug, Clone, PartialEq)]
pub enum Ingest {
    /// A complete market snapshot; replaces everything stored for the station's market.
    Market(MarketSnapshot),
    /// System position and (optionally) state, factions and powers.
    System(SystemUpdate),
    /// Station details (type, pads, services, economies, faction, distance).
    Station(StationUpdate),
    /// Ring hotspots from `SAASignalsFound` on a ring.
    RingSignals(RingSignalsUpdate),
    /// What one observation (scan, signal count, being on foot there) says about a planet or moon.
    Body(BodyUpdate),
    /// Raw JSON converted on the writer thread (via [`Ingest::from_eddn`] / [`Ingest::from_journal_event`]).
    Raw(RawIngest),
}

/// Raw JSON for [`Ingest::Raw`].
#[derive(Debug, Clone, PartialEq)]
pub enum RawIngest {
    /// A complete EDDN envelope (`{"$schemaRef", "header", "message"}`).
    Eddn(serde_json::Value),
    /// A journal event (or companion file such as Market.json) and its event name.
    Journal {
        event: String,
        json: serde_json::Value,
    },
}

/// Full market of one station (EDDN `commodity/3`, our own `Market.json`, or a Spansh dump).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarketSnapshot {
    pub market_id: u64,
    pub system_name: String,
    pub station_name: String,
    /// Raw station type in journal or Spansh spelling (`Coriolis`, `FleetCarrier`, `Planetary Port`).
    pub station_type: Option<String>,
    pub carrier_docking_access: Option<String>,
    /// Unix seconds (UTC) of the snapshot.
    pub timestamp: i64,
    pub items: Vec<MarketItem>,
    /// Prohibited commodity symbols, when the source provides them.
    pub prohibited: Option<Vec<String>>,
    /// Station economies (display name, proportion), when the source provides them.
    pub economies: Option<Vec<(String, f64)>>,
    pub source: Source,
}

/// One commodity row of a [`MarketSnapshot`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarketItem {
    /// Lower-case symbol (`gold`); see [`crate::normalize_symbol`].
    pub symbol: String,
    /// Price the station sells at (what you pay); 0 = not sold.
    pub buy: i64,
    /// Price the station buys at (what you get); 0 = not bought.
    pub sell: i64,
    /// Galactic mean price (0 when unknown).
    pub mean: i64,
    pub stock: i64,
    /// 0..=3, -1 for EDDN `""` (temporarily available), `None` when unknown.
    pub stock_bracket: Option<i8>,
    pub demand: i64,
    pub demand_bracket: Option<i8>,
}

/// System position and optionally its full state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemUpdate {
    pub id64: u64,
    pub name: String,
    pub pos: Option<[f64; 3]>,
    /// Unix seconds of the information.
    pub timestamp: i64,
    /// `None` = position/name only (e.g. from `Docked`), existing state is left alone.
    pub details: Option<SystemDetails>,
}

/// State of a system (from `FSDJump` / `Location` / `CarrierJump` or a dump).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemDetails {
    pub population: Option<u64>,
    pub allegiance: Option<String>,
    pub government: Option<String>,
    pub economy: Option<String>,
    pub second_economy: Option<String>,
    pub security: Option<String>,
    pub controlling_faction: Option<String>,
    pub controlling_faction_state: Option<String>,
    pub controlling_power: Option<String>,
    pub powerplay_state: Option<String>,
    pub powers: Vec<String>,
    /// `Some` replaces the stored faction list of the system; `None` leaves it alone.
    pub factions: Option<Vec<FactionUpdate>>,
    /// The game's own list of the system's wars, civil wars and elections, when the report carries it whole (empty:
    /// none). It goes with `factions`: a report that replaces the factions with `None` here (a Spansh dump) forgets
    /// the stored conflicts, which are then worked out from the faction states ([`ConflictHit::detailed`]).
    pub conflicts: Option<Vec<ConflictUpdate>>,
}

/// One war, civil war or election in a system (`Conflicts[]` of `FSDJump`, `Location` and `CarrierJump`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConflictUpdate {
    pub kind: ConflictKind,
    pub status: ConflictStatus,
    pub sides: [ConflictSideUpdate; 2],
}

/// One side of a conflict as the game reports it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConflictSideUpdate {
    pub faction: String,
    /// The asset it stands to lose (a station or settlement); `None` when nothing is at stake.
    pub stake: Option<String>,
    pub won_days: u32,
}

/// What kind of conflict: wars and civil wars are fought in conflict zones, elections aren't.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum ConflictKind {
    #[default]
    War,
    CivilWar,
    Election,
}

impl ConflictKind {
    /// The journal's `WarType` (`war`, `civilwar`, `election`) or a faction state (`War`, `Civil War`, `CivilWar`,
    /// `Election`), in any case.
    pub fn parse(raw: &str) -> Option<ConflictKind> {
        let k: String = raw.chars().filter(|c| !c.is_whitespace() && *c != '_').collect::<String>().to_ascii_lowercase();
        match k.as_str() {
            "war" => Some(ConflictKind::War),
            "civilwar" => Some(ConflictKind::CivilWar),
            "election" => Some(ConflictKind::Election),
            _ => None,
        }
    }

    /// How it's stored and how the journal spells it.
    pub fn token(self) -> &'static str {
        match self {
            ConflictKind::War => "war",
            ConflictKind::CivilWar => "civilwar",
            ConflictKind::Election => "election",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConflictKind::War => "War",
            ConflictKind::CivilWar => "Civil war",
            ConflictKind::Election => "Election",
        }
    }

    /// Fought with ships and on foot in conflict zones (wars and civil wars); an election is won without fighting.
    pub fn is_fight(self) -> bool {
        self != ConflictKind::Election
    }
}

/// Where a conflict stands (`Conflicts[].Status`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum ConflictStatus {
    /// Being fought: each day's winner is counted.
    #[default]
    Active,
    /// Declared and about to start.
    Pending,
    /// Over: the game keeps listing it, with no status (`""`), while both factions recover from it (seen on EDDN,
    /// 2026-11-05: every such conflict's factions had it among their recovering states).
    Ended,
}

impl ConflictStatus {
    /// `active`, `pending`; anything else (the game writes `""`) is [`ConflictStatus::Ended`].
    pub fn parse(raw: &str) -> ConflictStatus {
        match raw.trim().to_ascii_lowercase().as_str() {
            "active" => ConflictStatus::Active,
            "pending" => ConflictStatus::Pending,
            _ => ConflictStatus::Ended,
        }
    }

    pub fn token(self) -> &'static str {
        match self {
            ConflictStatus::Active => "active",
            ConflictStatus::Pending => "pending",
            ConflictStatus::Ended => "",
        }
    }
}

/// One faction present in a system.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FactionUpdate {
    pub name: String,
    /// 0..=1.
    pub influence: Option<f64>,
    pub state: Option<String>,
    pub government: Option<String>,
    pub allegiance: Option<String>,
    pub happiness: Option<String>,
    pub active_states: Vec<String>,
    pub pending_states: Vec<String>,
    pub recovering_states: Vec<String>,
}

/// Landing pad counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct LandingPads {
    pub small: u32,
    pub medium: u32,
    pub large: u32,
}

/// Station details. `None` fields leave the stored value alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StationUpdate {
    pub market_id: u64,
    pub name: String,
    pub system_id64: Option<u64>,
    pub system_name: Option<String>,
    /// Raw station type in journal or Spansh spelling.
    pub station_type: Option<String>,
    pub dist_ls: Option<f64>,
    pub landing_pads: Option<LandingPads>,
    pub services: Option<Services>,
    pub primary_economy: Option<String>,
    /// Display name + proportion, highest first.
    pub economies: Option<Vec<(String, f64)>>,
    pub faction: Option<String>,
    pub faction_state: Option<String>,
    pub government: Option<String>,
    pub allegiance: Option<String>,
    pub carrier_docking_access: Option<String>,
    /// Hint for sources that know the station is on a planet but not its type (ApproachSettlement).
    pub planetary: Option<bool>,
    pub timestamp: i64,
}

/// Ring hotspots of one ring (replaces what is stored for that ring).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RingSignalsUpdate {
    pub system_id64: u64,
    pub system_name: Option<String>,
    /// e.g. `HIP 21991 A 1 A Ring`.
    pub body_name: String,
    /// `Icy`, `Metallic`, `Metal Rich`, `Rocky` when known.
    pub ring_type: Option<String>,
    /// (signal, count): commodity symbols such as `painite`, `lowtemperaturediamond`, `opal`.
    pub signals: Vec<(String, i64)>,
    pub timestamp: i64,
}

/// One observation of a planet or moon, from any source: a `Scan`, a signal count
/// (`FSSBodySignals`, `SAASignalsFound`), evidence that someone was on foot there (`Disembark`,
/// `ScanOrganic`), or the result of an online lookup. `None` fields say nothing and leave the stored
/// value alone, so any source (live EDDN, the commander's journal, an EDDN archive lookup, later e.g.
/// a dump importer) is just one more input to the same merge:
///
/// * physical facts (`scan`, a complete scan): the newest scan replaces them; an older one adds
///   nothing;
/// * `was_*` flags are monotonic: once `true`, they stay `true`. The stored time is when `true` was
///   first seen, or while still `false` the newest observation that said `false` ("not footfalled as
///   of …"). For footfall the commander's own journal is authoritative: its `false` overrides an
///   older `true` from anywhere else (the game never un-footfalls a body, so that `true` was wrong),
///   and such a `true` arriving late is ignored;
/// * signal counts come from the newest signals observation; genera are never replaced by an empty
///   list.
///
/// What may create a row: landable bodies, bodies with biological or geological signals, Earth-like /
/// water / ammonia worlds, terraformables, on-foot evidence and online lookups. From EDDN (every
/// commander's scans, ~200 a minute) only bodies with biological signals, Earth-like / water /
/// ammonia worlds, terraformables and on-foot evidence are kept, unless
/// [`crate::GalaxyOptions::all_eddn_bodies`] is set; rows that exist are always updated.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodyUpdate {
    pub system_id64: u64,
    /// Journal `BodyID` (unique within the system).
    pub body_id: u32,
    /// Full name (`Synuefe BR-L c24-23 2 a`), when the source gives it.
    pub name: Option<String>,
    /// Physical facts from a `Scan`.
    pub scan: Option<BodyScan>,
    pub was_discovered: Option<bool>,
    pub was_mapped: Option<bool>,
    /// Somebody has set foot on the body (first footfall taken).
    pub was_footfalled: Option<bool>,
    pub signals: Option<BodySignals>,
    /// Unix seconds of the observation (0 for a lookup that found nothing).
    pub timestamp: i64,
    pub source: Source,
    /// Unix seconds of the online lookup this came from (recorded even when it found nothing, so
    /// the lookup isn't repeated soon).
    pub checked_online: Option<i64>,
}

/// Physical facts of a planet or moon, in journal spelling.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodyScan {
    /// Journal `PlanetClass` (`High metal content body`, `Earthlike body`, `Icy body`).
    pub planet_class: String,
    /// Journal `Atmosphere` text (`thin carbon dioxide atmosphere`); `None` when airless.
    pub atmosphere: Option<String>,
    /// Journal `AtmosphereType` (`CarbonDioxide`); `None` when airless.
    pub atmosphere_type: Option<String>,
    /// Surface gravity in g (`SurfaceGravity` / 9.80665).
    pub gravity_g: Option<f64>,
    pub temperature_k: Option<f64>,
    /// Surface pressure in atmospheres (`SurfacePressure` / 101325).
    pub pressure_atm: Option<f64>,
    /// Journal `Volcanism` text (`minor silicate vapour geysers volcanism`); `None` when none.
    pub volcanism: Option<String>,
    /// `Terraformable`, `Terraforming`, `Terraformed`; `None` when not terraformable.
    pub terraform_state: Option<String>,
    pub distance_ls: Option<f64>,
    pub landable: bool,
}

impl BodyScan {
    /// Earth-like, water or ammonia world, or terraformable: worth keeping even when not landable.
    pub fn is_notable(&self) -> bool {
        is_notable_class(&self.planet_class) || is_terraformable(self.terraform_state.as_deref())
    }
}

/// Earth-like, water or ammonia world (journal `PlanetClass`).
pub fn is_notable_class(planet_class: &str) -> bool {
    ["Earthlike body", "Water world", "Ammonia world"]
        .iter()
        .any(|c| c.eq_ignore_ascii_case(planet_class.trim()))
}

/// `Terraformable`, `Terraforming` or `Terraformed` (not `""` / `Not terraformable`).
pub fn is_terraformable(state: Option<&str>) -> bool {
    state.is_some_and(|s| {
        let s = s.trim().to_ascii_lowercase();
        s.starts_with("terraform") || s.starts_with("candidate")
    })
}

/// Signals of a planet or moon (`FSSBodySignals` after an FSS scan, `SAASignalsFound` after DSS).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodySignals {
    pub bio: u32,
    pub geo: u32,
    /// Genus codex symbols (`$Codex_Ent_Bacterial_Genus_Name;`) revealed by a DSS scan; empty when
    /// unknown.
    pub genera: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// Commodity search
// ---------------------------------------------------------------------------------------------

/// Direction of a commodity search, from the player's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TradeMode {
    /// Stations that sell the commodity (you buy): lowest price first, needs stock.
    #[default]
    BuyFromStation,
    /// Stations that buy the commodity (you sell): highest price first, needs demand.
    SellToStation,
}

/// Result ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Sort {
    #[default]
    BestPrice,
    Nearest,
    Freshest,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommodityQuery {
    /// Symbol (`gold`), journal name (`$gold_name;`) or display name (`Low Temperature Diamonds`).
    pub commodity: String,
    pub mode: TradeMode,
    pub origin: Option<[f64; 3]>,
    pub max_distance_ly: Option<f64>,
    /// Minimum landing pad: `'S'`, `'M'` or `'L'`.
    pub min_pad: Option<char>,
    pub max_age_hours: Option<f64>,
    /// Minimum stock (buy mode) or demand (sell mode); values below 1 are treated as 1.
    pub min_quantity: i64,
    pub include_carriers: bool,
    pub include_planetary: bool,
    pub limit: usize,
    pub sort: Sort,
}

impl Default for CommodityQuery {
    fn default() -> Self {
        CommodityQuery {
            commodity: String::new(),
            mode: TradeMode::BuyFromStation,
            origin: None,
            max_distance_ly: None,
            min_pad: None,
            max_age_hours: None,
            min_quantity: 1,
            include_carriers: true,
            include_planetary: true,
            limit: 50,
            sort: Sort::BestPrice,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommodityHit {
    pub system: String,
    pub system_id64: Option<u64>,
    pub pos: Option<[f64; 3]>,
    /// From the query origin (None without origin or unknown system position).
    pub distance_ly: Option<f64>,
    pub station: String,
    pub market_id: u64,
    pub station_type: Option<String>,
    pub dist_ls: Option<f64>,
    pub max_pad: Option<char>,
    /// Buy price (buy mode) or sell price (sell mode).
    pub price: i64,
    /// Stock (buy mode) or demand (sell mode).
    pub quantity: i64,
    pub updated_unix: i64,
    pub is_carrier: bool,
    pub is_planetary: bool,
    /// A fleet carrier's docking access as last reported (`all`, `none`, `friends`, `squadron`, `squadronfriends`);
    /// `None` for a station, or a carrier whose access was never reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carrier_access: Option<String>,
    /// Rare goods searches: which commodity this row is. `None` in a search for one commodity (the row is that one).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commodity: Option<RowCommodity>,
}

/// The commodity of a row in a search over several (rare goods).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RowCommodity {
    pub symbol: String,
    pub name: String,
    /// Its galactic average: the one this market reports or, where it reports none (fleet carriers), the one most
    /// markets report for this good; 0 when no market in the database reports one.
    pub mean: i64,
}

/// What the database knows about one commodity across the whole galaxy (no distance or station
/// filters): for the head of a commodity page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommoditySummary {
    pub symbol: String,
    pub name: String,
    pub category: String,
    pub is_rare: bool,
    /// The in-game galactic average: the one most stations report (`None` when none does).
    pub average: Option<i64>,
    /// Markets with stock to sell (you buy there), within the age limit.
    pub sellers: u32,
    pub carrier_sellers: u32,
    /// Markets with demand (you sell there), within the age limit.
    pub buyers: u32,
    pub carrier_buyers: u32,
    /// Cheapest price anyone sells for / best price anyone pays (carriers included).
    pub lowest_buy: Option<i64>,
    pub highest_sell: Option<i64>,
    /// Newest price of this commodity anywhere (unix seconds).
    pub newest_unix: Option<i64>,
}

/// One market's full row for a commodity (both directions), for a "latest prices" list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommodityQuote {
    pub market_id: u64,
    pub station: String,
    pub station_type: Option<String>,
    pub system: String,
    pub distance_ly: Option<f64>,
    pub dist_ls: Option<f64>,
    pub max_pad: Option<char>,
    pub is_carrier: bool,
    pub is_planetary: bool,
    /// A fleet carrier's docking access as last reported (see [`CommodityHit::carrier_access`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carrier_access: Option<String>,
    /// 0 when the station does not sell it.
    pub buy: i64,
    /// 0 when the station does not buy it.
    pub sell: i64,
    /// The galactic average the station reports (0 for fleet carriers).
    pub mean: i64,
    pub stock: i64,
    pub demand: i64,
    pub updated_unix: i64,
    /// Rare goods searches: which commodity this row is. `None` in a search for one commodity (the row is that one).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commodity: Option<RowCommodity>,
}

// ---------------------------------------------------------------------------------------------
// Stations / systems
// ---------------------------------------------------------------------------------------------

/// Everything stored about a station (without its market rows).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StationInfo {
    pub market_id: u64,
    pub name: String,
    pub station_type: Option<String>,
    pub system: String,
    pub system_id64: Option<u64>,
    pub pos: Option<[f64; 3]>,
    pub dist_ls: Option<f64>,
    pub max_pad: Option<char>,
    pub is_carrier: bool,
    pub is_planetary: bool,
    pub is_odyssey_settlement: bool,
    pub services: Services,
    pub material_trader: Option<MaterialTrader>,
    pub tech_broker: Option<TechBroker>,
    pub economy: Option<String>,
    pub second_economy: Option<String>,
    pub faction: Option<String>,
    pub faction_state: Option<String>,
    pub government: Option<String>,
    pub allegiance: Option<String>,
    pub carrier_docking_access: Option<String>,
    /// Station details last updated (unix seconds; 0 = never).
    pub updated_unix: i64,
    /// Market last updated (unix seconds; 0 = no market data).
    pub market_updated_unix: i64,
    pub commodity_count: u32,
}

/// One market row with names.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MarketRow {
    pub symbol: String,
    pub name: String,
    pub category: String,
    pub buy: i64,
    pub sell: i64,
    pub mean: i64,
    pub stock: i64,
    pub stock_bracket: Option<i8>,
    pub demand: i64,
    pub demand_bracket: Option<i8>,
    pub updated_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StationMarket {
    pub station: StationInfo,
    pub system: Option<SystemInfo>,
    pub prohibited: Vec<String>,
    /// Sorted by category, then name.
    pub items: Vec<MarketRow>,
}

/// Everything stored about a system.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemInfo {
    pub id64: u64,
    pub name: String,
    pub pos: Option<[f64; 3]>,
    pub population: Option<u64>,
    pub allegiance: Option<String>,
    pub government: Option<String>,
    pub economy: Option<String>,
    pub second_economy: Option<String>,
    pub security: Option<String>,
    pub controlling_faction: Option<String>,
    pub controlling_faction_state: Option<String>,
    pub controlling_power: Option<String>,
    pub powerplay_state: Option<String>,
    pub powers: Vec<String>,
    /// State last updated (unix seconds; 0 = only the position is known).
    pub updated_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemHit {
    pub system: SystemInfo,
    pub distance_ly: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FactionInfo {
    pub name: String,
    pub influence: Option<f64>,
    pub state: Option<String>,
    pub government: Option<String>,
    pub allegiance: Option<String>,
    pub happiness: Option<String>,
    pub active_states: Vec<String>,
    pub pending_states: Vec<String>,
    pub recovering_states: Vec<String>,
    pub updated_unix: i64,
}

/// Wars, civil wars and elections around a point, nearest first; ended ones are left out.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictQuery {
    pub origin: [f64; 3],
    /// `None` = the whole galaxy.
    pub max_distance_ly: Option<f64>,
    /// Leave out systems last reported before this (unix seconds).
    pub since_unix: i64,
    /// Elections too (they have no conflict zones).
    pub elections: bool,
    pub limit: usize,
}

/// What [`GalaxyDb::conflicts_near`](crate::GalaxyDb::conflicts_near) found.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ConflictScan {
    /// Nearest first; at a system, active before pending.
    pub hits: Vec<ConflictHit>,
    /// How many were found before the query's `limit` cut the list.
    pub total: usize,
    /// Populated systems in range with a report since `since_unix`: what the list is based on.
    pub systems_reported: usize,
}

/// A conflict in a system near the query's origin.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConflictHit {
    pub system: String,
    pub system_id64: u64,
    pub pos: [f64; 3],
    pub distance_ly: f64,
    pub kind: ConflictKind,
    pub status: ConflictStatus,
    /// Two sides when `detailed`; from faction states alone, every faction in that state (usually two).
    pub sides: Vec<ConflictSide>,
    /// From the game's own list of the system's conflicts (who fights whom, stakes, days won). False when worked
    /// out from faction states alone (a Spansh dump, or a report sent without the list): sides only.
    pub detailed: bool,
    /// When the system was last reported (unix seconds).
    pub updated_unix: i64,
    pub government: Option<String>,
    pub security: Option<String>,
    pub allegiance: Option<String>,
    pub population: Option<u64>,
    pub controlling_faction: Option<String>,
}

/// One side of a [`ConflictHit`].
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ConflictSide {
    pub faction: String,
    /// The asset it stands to lose (a station or settlement).
    pub stake: Option<String>,
    /// `None` when not reported (`detailed` is false).
    pub won_days: Option<u32>,
    /// 0..=1.
    pub influence: Option<f64>,
    pub allegiance: Option<String>,
    pub government: Option<String>,
}

/// Nearest-station search with service filters.
#[derive(Debug, Clone, PartialEq)]
pub struct StationQuery {
    pub origin: [f64; 3],
    /// `None` = search outward until `limit` stations are found.
    pub max_distance_ly: Option<f64>,
    /// Every service in this mask must be present.
    pub services: Services,
    /// Require a material trader of this type (implies [`Services::MATERIAL_TRADER`]).
    pub material_trader: Option<MaterialTrader>,
    /// Require a technology broker of this type (implies [`Services::TECH_BROKER`]).
    pub tech_broker: Option<TechBroker>,
    pub min_pad: Option<char>,
    /// Maximum distance from the arrival star in light seconds.
    pub max_dist_ls: Option<f64>,
    pub include_carriers: bool,
    pub include_planetary: bool,
    pub limit: usize,
}

impl Default for StationQuery {
    fn default() -> Self {
        StationQuery {
            origin: [0.0; 3],
            max_distance_ly: None,
            services: Services::NONE,
            material_trader: None,
            tech_broker: None,
            min_pad: None,
            max_dist_ls: None,
            include_carriers: false,
            include_planetary: true,
            limit: 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StationHit {
    pub station: StationInfo,
    pub distance_ly: f64,
}

/// Ring hotspot search.
#[derive(Debug, Clone, PartialEq)]
pub struct HotspotQuery {
    pub origin: [f64; 3],
    /// Commodity symbol or name (`painite`, `Low Temperature Diamonds`, `Void Opal`).
    pub signal: String,
    pub max_distance_ly: Option<f64>,
    pub min_count: i64,
    pub limit: usize,
}

impl Default for HotspotQuery {
    fn default() -> Self {
        HotspotQuery {
            origin: [0.0; 3],
            signal: String::new(),
            max_distance_ly: None,
            min_count: 1,
            limit: 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HotspotHit {
    pub system: String,
    pub system_id64: u64,
    pub pos: [f64; 3],
    pub distance_ly: f64,
    pub body_name: String,
    pub ring_type: Option<String>,
    pub signal: String,
    pub count: i64,
    pub updated_unix: i64,
}

// ---------------------------------------------------------------------------------------------
// Bodies
// ---------------------------------------------------------------------------------------------

/// A `Was*` flag of a body and when it was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Seen {
    pub value: bool,
    /// Unix seconds. `value == true`: the first observation that reported it (it may have happened
    /// earlier). `value == false`: the newest observation that reported it, i.e. "still false as of".
    pub at_unix: i64,
}

/// A planet or moon as stored (see [`BodyUpdate`] for what is kept and how observations merge).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BodyInfo {
    pub system_id64: u64,
    pub body_id: u32,
    pub name: Option<String>,
    /// Journal `PlanetClass`.
    pub planet_class: Option<String>,
    pub atmosphere: Option<String>,
    pub atmosphere_type: Option<String>,
    pub gravity_g: Option<f64>,
    pub temperature_k: Option<f64>,
    pub pressure_atm: Option<f64>,
    pub volcanism: Option<String>,
    pub terraform_state: Option<String>,
    pub distance_ls: Option<f64>,
    pub landable: Option<bool>,
    /// Newest scan that supplied the physical facts (unix seconds).
    pub scan_unix: Option<i64>,
    pub was_discovered: Option<Seen>,
    pub was_mapped: Option<Seen>,
    pub was_footfalled: Option<Seen>,
    /// Source of the observation behind `was_footfalled`.
    pub footfall_source: Option<Source>,
    pub bio_signals: Option<u32>,
    pub geo_signals: Option<u32>,
    /// Genus codex symbols revealed by a DSS scan.
    pub genera: Vec<String>,
    /// Newest signals observation (unix seconds).
    pub signals_unix: Option<i64>,
    /// Newest observation of any kind (unix seconds).
    pub updated_unix: i64,
    /// Source of the newest observation.
    pub source: Source,
    /// Last online footfall lookup (unix seconds), whatever it found.
    pub checked_online_unix: Option<i64>,
}

impl BodyInfo {
    /// Earth-like, water or ammonia world, or terraformable.
    pub fn is_notable(&self) -> bool {
        self.planet_class.as_deref().is_some_and(is_notable_class)
            || is_terraformable(self.terraform_state.as_deref())
    }
}

// ---------------------------------------------------------------------------------------------
// Trade routes
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct TradeQuery {
    pub origin: [f64; 3],
    /// Both ends of a route must be within this distance of `origin`.
    pub radius_ly: f64,
    /// Maximum distance between the buying and the selling station.
    pub max_hop_ly: f64,
    pub min_pad: Option<char>,
    pub cargo_capacity: i64,
    /// Credits available for the purchase (`None` = unlimited).
    pub capital: Option<i64>,
    pub max_age_hours: Option<f64>,
    pub include_carriers: bool,
    pub include_planetary: bool,
    /// Maximum station distance from the arrival star (both ends), light seconds.
    pub max_dist_ls: Option<f64>,
    pub min_profit_per_ton: i64,
    pub limit: usize,
    /// Only routes that buy at this station (a market ID), such as the one the commander is docked at. The pad,
    /// carrier, planetary and distance-from-star filters don't apply to it (the commander is there); its prices
    /// must still be newer than `max_age_hours`.
    pub from_market: Option<u64>,
}

impl Default for TradeQuery {
    fn default() -> Self {
        TradeQuery {
            origin: [0.0; 3],
            radius_ly: 30.0,
            max_hop_ly: 20.0,
            min_pad: None,
            cargo_capacity: 100,
            capital: None,
            max_age_hours: Some(48.0),
            include_carriers: false,
            include_planetary: true,
            max_dist_ls: None,
            min_profit_per_ton: 1,
            limit: 25,
            from_market: None,
        }
    }
}

/// One end of a trade route.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TradeStation {
    pub market_id: u64,
    pub station: String,
    pub system: String,
    pub system_id64: Option<u64>,
    pub pos: [f64; 3],
    pub distance_from_origin_ly: f64,
    pub station_type: Option<String>,
    pub dist_ls: Option<f64>,
    pub max_pad: Option<char>,
    pub is_carrier: bool,
    pub is_planetary: bool,
    pub market_updated_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TradeRoute {
    pub from: TradeStation,
    pub to: TradeStation,
    pub commodity: String,
    pub commodity_name: String,
    pub buy_price: i64,
    pub sell_price: i64,
    pub profit_per_ton: i64,
    /// Stock at the buying station.
    pub supply: i64,
    /// Demand at the selling station.
    pub demand: i64,
    /// min(cargo capacity, capital / buy price, supply, demand).
    pub units: i64,
    pub total_profit: i64,
    pub hop_ly: f64,
}

/// "Deals": stations paying far above the galactic average for a commodity (sell) or selling it
/// far below the average (buy).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DealQuery {
    pub mode: TradeMode,
    pub origin: Option<[f64; 3]>,
    /// `None` = anywhere (a full scan: slower).
    pub max_distance_ly: Option<f64>,
    /// Price / average (sell) or average / price (buy) at least this, e.g. 1.25 = 25 % better.
    pub min_ratio: f64,
    pub min_pad: Option<char>,
    pub include_carriers: bool,
    pub max_age_hours: Option<f64>,
    /// Tonnes one trip can move: caps a deal's value.
    pub cargo_capacity: i64,
    pub limit: usize,
}

impl Default for DealQuery {
    fn default() -> Self {
        DealQuery {
            mode: TradeMode::SellToStation,
            origin: None,
            max_distance_ly: None,
            min_ratio: 1.25,
            min_pad: None,
            include_carriers: true,
            max_age_hours: Some(48.0),
            cargo_capacity: 100,
            limit: 25,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Deal {
    pub commodity: String,
    pub commodity_name: String,
    pub price: i64,
    /// The in-game galactic average price.
    pub average: i64,
    /// price / average (sell) or average / price (buy).
    pub ratio: f64,
    /// Demand (sell) or stock (buy).
    pub quantity: i64,
    /// Credits better than average for min(quantity, cargo) tonnes.
    pub extra_value: i64,
    /// Credits paid (sell) or spent (buy) for min(quantity, cargo) tonnes.
    pub trade_value: i64,
    pub station: String,
    pub system: String,
    pub market_id: u64,
    pub distance_ly: Option<f64>,
    pub max_pad: Option<char>,
    pub is_carrier: bool,
    /// A fleet carrier's docking access as last reported (see [`CommodityHit::carrier_access`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carrier_access: Option<String>,
    pub updated_unix: i64,
}

// ---------------------------------------------------------------------------------------------
// Misc
// ---------------------------------------------------------------------------------------------

/// Pushed to [`crate::GalaxyDb::subscribe_updates`] receivers after each committed live market snapshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MarketUpdate {
    pub market_id: u64,
    pub station: String,
    pub system: String,
    pub system_id64: Option<u64>,
    pub pos: Option<[f64; 3]>,
    pub commodities: usize,
    pub updated_unix: i64,
    pub is_carrier: bool,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommodityInfo {
    pub symbol: String,
    pub name: String,
    pub category: String,
    pub is_rare: bool,
}

/// One point of a market's price history: a report just before or at a change. `buy` and `sell` 0 on the report
/// before a commodity first appeared there (the market didn't list it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PricePoint {
    pub ts_unix: i64,
    pub buy: i64,
    pub sell: i64,
}

/// How long a market's prices for one commodity have stood, as far as the price history can tell (see
/// [`crate::GalaxyDb::price_since`]): what it sells for (you buy) and what it pays (you sell), each `None` when it
/// doesn't trade that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PriceSince {
    pub buy: Option<Stood>,
    pub sell: Option<Stood>,
}

/// How long one price has stood (unix seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Stood {
    /// It changed to this price after `before`, the last report at the `previous` price (0: the market didn't
    /// list the commodity then), and by `since`, the first report at this one.
    Since { since: i64, before: i64, previous: i64 },
    /// No change on record: it has stood at least since `since`, as far back as the history of this market reaches
    /// (it keeps changes for 30 days, and only from the first report it compared).
    AtLeast { since: i64 },
    /// The history doesn't follow this market's prices yet (it does from its next report).
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DbStats {
    pub systems: u64,
    pub stations: u64,
    pub stations_with_market: u64,
    pub market_rows: u64,
    pub commodities: u64,
    pub faction_rows: u64,
    pub ring_signals: u64,
    /// Kept by the writer; `None` until it has counted the table once after opening.
    pub price_history_rows: Option<u64>,
    pub newest_market_update: Option<i64>,
    /// Main file + WAL.
    pub db_bytes: u64,
    /// Items waiting for the writer thread.
    pub pending_writes: u64,
    /// Items accepted since open (after `Raw` expansion).
    pub ingested: u64,
    pub market_snapshots: u64,
    /// Market snapshots ignored because newer data was already stored.
    pub stale_markets: u64,
    pub commits: u64,
}
