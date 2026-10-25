//! Typed journal events.
//!
//! Every struct is `#[serde(default)]`: the journal format evolves (and differs between Live and
//! Legacy clients), so a missing field must never make an event unreadable. Unknown fields are
//! ignored. Events that nothing in Elite Nexus consumes stay as [`Event::Other`] and are never
//! JSON-parsed at all, which keeps bulk imports fast.

#![allow(non_snake_case)]

use serde::Deserialize;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------------------------
// Shared building blocks
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NamedFaction {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "FactionState")]
    pub faction_state: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StateTrend {
    #[serde(rename = "State")]
    pub state: String,
    #[serde(rename = "Trend")]
    pub trend: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SystemFactionInfo {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "FactionState")]
    pub faction_state: String,
    #[serde(rename = "Government")]
    pub government: String,
    #[serde(rename = "Influence")]
    pub influence: f64,
    #[serde(rename = "Allegiance")]
    pub allegiance: String,
    #[serde(rename = "Happiness")]
    pub happiness: String,
    #[serde(rename = "Happiness_Localised")]
    pub happiness_localised: Option<String>,
    #[serde(rename = "MyReputation")]
    pub my_reputation: Option<f64>,
    #[serde(rename = "PendingStates")]
    pub pending_states: Vec<StateTrend>,
    #[serde(rename = "RecoveringStates")]
    pub recovering_states: Vec<StateTrend>,
    #[serde(rename = "ActiveStates")]
    pub active_states: Vec<StateTrend>,
    #[serde(rename = "SquadronFaction")]
    pub squadron_faction: Option<bool>,
    #[serde(rename = "HappiestSystem")]
    pub happiest_system: Option<bool>,
    #[serde(rename = "HomeSystem")]
    pub home_system: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ConflictSide {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Stake")]
    pub stake: String,
    #[serde(rename = "WonDays")]
    pub won_days: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Conflict {
    #[serde(rename = "WarType")]
    pub war_type: String,
    #[serde(rename = "Status")]
    pub status: String,
    #[serde(rename = "Faction1")]
    pub faction1: ConflictSide,
    #[serde(rename = "Faction2")]
    pub faction2: ConflictSide,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StationEconomyShare {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "Proportion")]
    pub proportion: f64,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
pub struct LandingPads {
    #[serde(rename = "Small")]
    pub small: u32,
    #[serde(rename = "Medium")]
    pub medium: u32,
    #[serde(rename = "Large")]
    pub large: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PowerConflictProgress {
    #[serde(rename = "Power")]
    pub power: String,
    #[serde(rename = "ConflictProgress")]
    pub conflict_progress: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NameCount {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "Count")]
    pub count: i64,
}

// ---------------------------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Fileheader {
    pub part: u32,
    pub language: String,
    #[serde(rename = "Odyssey")]
    pub odyssey: Option<bool>,
    pub gameversion: String,
    pub build: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Commander {
    #[serde(rename = "FID")]
    pub fid: String,
    #[serde(rename = "Name")]
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LoadGame {
    #[serde(rename = "FID")]
    pub fid: String,
    #[serde(rename = "Commander")]
    pub commander: String,
    /// Absent on old clients: `None` means "not stated" (EDDN must then omit the flag).
    #[serde(rename = "Horizons")]
    pub horizons: Option<bool>,
    #[serde(rename = "Odyssey")]
    pub odyssey: Option<bool>,
    #[serde(rename = "Ship")]
    pub ship: Option<String>,
    #[serde(rename = "Ship_Localised")]
    pub ship_localised: Option<String>,
    #[serde(rename = "ShipID")]
    pub ship_id: Option<u64>,
    #[serde(rename = "StartLanded")]
    pub start_landed: Option<bool>,
    #[serde(rename = "StartDead")]
    pub start_dead: Option<bool>,
    #[serde(rename = "ShipName")]
    pub ship_name: Option<String>,
    #[serde(rename = "ShipIdent")]
    pub ship_ident: Option<String>,
    #[serde(rename = "FuelLevel")]
    pub fuel_level: Option<f64>,
    #[serde(rename = "FuelCapacity")]
    pub fuel_capacity: Option<f64>,
    #[serde(rename = "GameMode")]
    pub game_mode: Option<String>,
    #[serde(rename = "Group")]
    pub group: Option<String>,
    #[serde(rename = "Credits")]
    pub credits: i64,
    #[serde(rename = "Loan")]
    pub loan: i64,
    pub language: String,
    pub gameversion: String,
    pub build: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Continued {
    #[serde(rename = "Part")]
    pub part: u32,
}

/// `Music`: the soundtrack changed. `MainMenu` plays at the main menu.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Music {
    #[serde(rename = "MusicTrack")]
    pub music_track: String,
}

// ---------------------------------------------------------------------------------------------
// Ranks, reputation, engineers, powerplay, squadron
// ---------------------------------------------------------------------------------------------

/// Used for both `Rank` (rank index) and `Progress` (percent towards next rank), and `Promotion`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RankSet {
    #[serde(rename = "Combat")]
    pub combat: Option<u32>,
    #[serde(rename = "Trade")]
    pub trade: Option<u32>,
    #[serde(rename = "Explore")]
    pub explore: Option<u32>,
    #[serde(rename = "Soldier")]
    pub soldier: Option<u32>,
    #[serde(rename = "Exobiologist")]
    pub exobiologist: Option<u32>,
    #[serde(rename = "Empire")]
    pub empire: Option<u32>,
    #[serde(rename = "Federation")]
    pub federation: Option<u32>,
    #[serde(rename = "CQC")]
    pub cqc: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Reputation {
    #[serde(rename = "Empire")]
    pub empire: Option<f64>,
    #[serde(rename = "Federation")]
    pub federation: Option<f64>,
    #[serde(rename = "Independent")]
    pub independent: Option<f64>,
    #[serde(rename = "Alliance")]
    pub alliance: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EngineerEntry {
    #[serde(rename = "Engineer")]
    pub engineer: String,
    #[serde(rename = "EngineerID")]
    pub engineer_id: u32,
    #[serde(rename = "Progress")]
    pub progress: String,
    #[serde(rename = "Rank")]
    pub rank: Option<u32>,
    #[serde(rename = "RankProgress")]
    pub rank_progress: Option<u32>,
}

/// Startup snapshot (`Engineers` list) or a single-engineer update (top-level fields).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EngineerProgress {
    #[serde(rename = "Engineers")]
    pub engineers: Option<Vec<EngineerEntry>>,
    #[serde(rename = "Engineer")]
    pub engineer: Option<String>,
    #[serde(rename = "EngineerID")]
    pub engineer_id: Option<u32>,
    #[serde(rename = "Progress")]
    pub progress: Option<String>,
    #[serde(rename = "Rank")]
    pub rank: Option<u32>,
    #[serde(rename = "RankProgress")]
    pub rank_progress: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Powerplay {
    #[serde(rename = "Power")]
    pub power: String,
    #[serde(rename = "Rank")]
    pub rank: u32,
    #[serde(rename = "Merits")]
    pub merits: i64,
    #[serde(rename = "Votes")]
    pub votes: Option<i64>,
    #[serde(rename = "TimePledged")]
    pub time_pledged: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PowerplayChange {
    #[serde(rename = "Power")]
    pub power: Option<String>,
    #[serde(rename = "FromPower")]
    pub from_power: Option<String>,
    #[serde(rename = "ToPower")]
    pub to_power: Option<String>,
    #[serde(rename = "Rank")]
    pub rank: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PowerplayMerits {
    #[serde(rename = "Power")]
    pub power: String,
    #[serde(rename = "MeritsGained")]
    pub merits_gained: i64,
    #[serde(rename = "TotalMerits")]
    pub total_merits: i64,
}

/// `AppliedToSquadron`, `InvitedToSquadron`, `JoinedSquadron`, `LeftSquadron`, `SquadronCreated`,
/// `SquadronPromotion`, `SquadronDemotion`, `KickedFromSquadron`, `DisbandedSquadron`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SquadronEvent {
    #[serde(rename = "SquadronID")]
    pub squadron_id: Option<u64>,
    #[serde(rename = "SquadronName")]
    pub squadron_name: String,
    #[serde(rename = "OldRank")]
    pub old_rank: Option<u32>,
    #[serde(rename = "NewRank")]
    pub new_rank: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SquadronStartup {
    #[serde(rename = "SquadronID")]
    pub squadron_id: Option<u64>,
    #[serde(rename = "SquadronName")]
    pub squadron_name: String,
    #[serde(rename = "CurrentRank")]
    pub current_rank: u32,
    #[serde(rename = "CurrentRankName")]
    pub current_rank_name: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Travel
// ---------------------------------------------------------------------------------------------

/// `Location`, `FSDJump` and `CarrierJump` all describe arriving in (or being in) a system and
/// share almost every field, so they deserialize into this one union struct.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SystemArrival {
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "StarPos")]
    pub star_pos: Option<[f64; 3]>,
    #[serde(rename = "SystemAllegiance")]
    pub system_allegiance: String,
    #[serde(rename = "SystemEconomy")]
    pub system_economy: String,
    #[serde(rename = "SystemEconomy_Localised")]
    pub system_economy_localised: Option<String>,
    #[serde(rename = "SystemSecondEconomy")]
    pub system_second_economy: String,
    #[serde(rename = "SystemSecondEconomy_Localised")]
    pub system_second_economy_localised: Option<String>,
    #[serde(rename = "SystemGovernment")]
    pub system_government: String,
    #[serde(rename = "SystemGovernment_Localised")]
    pub system_government_localised: Option<String>,
    #[serde(rename = "SystemSecurity")]
    pub system_security: String,
    #[serde(rename = "SystemSecurity_Localised")]
    pub system_security_localised: Option<String>,
    #[serde(rename = "Population")]
    pub population: u64,
    #[serde(rename = "Body")]
    pub body: Option<String>,
    #[serde(rename = "BodyID")]
    pub body_id: Option<u32>,
    #[serde(rename = "BodyType")]
    pub body_type: Option<String>,
    #[serde(rename = "Factions")]
    pub factions: Vec<SystemFactionInfo>,
    #[serde(rename = "SystemFaction")]
    pub system_faction: Option<NamedFaction>,
    #[serde(rename = "Conflicts")]
    pub conflicts: Vec<Conflict>,
    #[serde(rename = "Powers")]
    pub powers: Vec<String>,
    #[serde(rename = "ControllingPower")]
    pub controlling_power: Option<String>,
    #[serde(rename = "PowerplayState")]
    pub powerplay_state: Option<String>,
    #[serde(rename = "PowerplayStateControlProgress")]
    pub powerplay_state_control_progress: Option<f64>,
    #[serde(rename = "PowerplayStateReinforcement")]
    pub powerplay_state_reinforcement: Option<f64>,
    #[serde(rename = "PowerplayStateUndermining")]
    pub powerplay_state_undermining: Option<f64>,
    #[serde(rename = "PowerplayConflictProgress")]
    pub powerplay_conflict_progress: Vec<PowerConflictProgress>,
    #[serde(rename = "Taxi")]
    pub taxi: Option<bool>,
    #[serde(rename = "Multicrew")]
    pub multicrew: Option<bool>,
    // FSDJump only
    #[serde(rename = "JumpDist")]
    pub jump_dist: Option<f64>,
    #[serde(rename = "FuelUsed")]
    pub fuel_used: Option<f64>,
    #[serde(rename = "FuelLevel")]
    pub fuel_level: Option<f64>,
    #[serde(rename = "BoostUsed")]
    pub boost_used: Option<f64>,
    // Location / CarrierJump
    #[serde(rename = "Docked")]
    pub docked: Option<bool>,
    #[serde(rename = "OnFoot")]
    pub on_foot: Option<bool>,
    #[serde(rename = "InSRV")]
    pub in_srv: Option<bool>,
    #[serde(rename = "DistFromStarLS")]
    pub dist_from_star_ls: Option<f64>,
    #[serde(rename = "StationName")]
    pub station_name: Option<String>,
    #[serde(rename = "StationType")]
    pub station_type: Option<String>,
    #[serde(rename = "MarketID")]
    pub market_id: Option<u64>,
    #[serde(rename = "StationFaction")]
    pub station_faction: Option<NamedFaction>,
    #[serde(rename = "StationGovernment")]
    pub station_government: Option<String>,
    #[serde(rename = "StationAllegiance")]
    pub station_allegiance: Option<String>,
    #[serde(rename = "StationServices")]
    pub station_services: Vec<String>,
    #[serde(rename = "StationEconomy")]
    pub station_economy: Option<String>,
    #[serde(rename = "StationEconomies")]
    pub station_economies: Vec<StationEconomyShare>,
    #[serde(rename = "Latitude")]
    pub latitude: Option<f64>,
    #[serde(rename = "Longitude")]
    pub longitude: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StartJump {
    #[serde(rename = "JumpType")]
    pub jump_type: String,
    #[serde(rename = "Taxi")]
    pub taxi: Option<bool>,
    #[serde(rename = "StarSystem")]
    pub star_system: Option<String>,
    #[serde(rename = "SystemAddress")]
    pub system_address: Option<u64>,
    #[serde(rename = "StarClass")]
    pub star_class: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FSDTarget {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "StarClass")]
    pub star_class: String,
    #[serde(rename = "RemainingJumpsInRoute")]
    pub remaining_jumps_in_route: Option<u32>,
}

/// `SupercruiseEntry` / `SupercruiseExit`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Supercruise {
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "Body")]
    pub body: Option<String>,
    #[serde(rename = "BodyID")]
    pub body_id: Option<u32>,
    #[serde(rename = "BodyType")]
    pub body_type: Option<String>,
    #[serde(rename = "Taxi")]
    pub taxi: Option<bool>,
    #[serde(rename = "Multicrew")]
    pub multicrew: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SupercruiseDestinationDrop {
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "Type_Localised")]
    pub kind_localised: Option<String>,
    #[serde(rename = "Threat")]
    pub threat: Option<u32>,
    #[serde(rename = "MarketID")]
    pub market_id: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Docked {
    #[serde(rename = "StationName")]
    pub station_name: String,
    #[serde(rename = "StationType")]
    pub station_type: String,
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "StationFaction")]
    pub station_faction: Option<NamedFaction>,
    #[serde(rename = "StationGovernment")]
    pub station_government: Option<String>,
    #[serde(rename = "StationGovernment_Localised")]
    pub station_government_localised: Option<String>,
    #[serde(rename = "StationAllegiance")]
    pub station_allegiance: Option<String>,
    #[serde(rename = "StationServices")]
    pub station_services: Vec<String>,
    #[serde(rename = "StationEconomy")]
    pub station_economy: Option<String>,
    #[serde(rename = "StationEconomy_Localised")]
    pub station_economy_localised: Option<String>,
    #[serde(rename = "StationEconomies")]
    pub station_economies: Vec<StationEconomyShare>,
    #[serde(rename = "DistFromStarLS")]
    pub dist_from_star_ls: Option<f64>,
    #[serde(rename = "LandingPads")]
    pub landing_pads: Option<LandingPads>,
    #[serde(rename = "StationState")]
    pub station_state: Option<String>,
    #[serde(rename = "Wanted")]
    pub wanted: Option<bool>,
    #[serde(rename = "ActiveFine")]
    pub active_fine: Option<bool>,
    #[serde(rename = "CockpitBreach")]
    pub cockpit_breach: Option<bool>,
    #[serde(rename = "Taxi")]
    pub taxi: Option<bool>,
    #[serde(rename = "Multicrew")]
    pub multicrew: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Undocked {
    #[serde(rename = "StationName")]
    pub station_name: String,
    #[serde(rename = "StationType")]
    pub station_type: String,
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "Taxi")]
    pub taxi: Option<bool>,
    #[serde(rename = "Multicrew")]
    pub multicrew: Option<bool>,
}

/// `DockingRequested` / `DockingGranted` / `DockingDenied` / `DockingCancelled` / `DockingTimeout`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Docking {
    #[serde(rename = "StationName")]
    pub station_name: String,
    #[serde(rename = "StationType")]
    pub station_type: String,
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "LandingPad")]
    pub landing_pad: Option<u32>,
    #[serde(rename = "Reason")]
    pub reason: Option<String>,
    #[serde(rename = "LandingPads")]
    pub landing_pads: Option<LandingPads>,
}

/// `ApproachBody` / `LeaveBody`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BodyApproach {
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "Body")]
    pub body: String,
    #[serde(rename = "BodyID")]
    pub body_id: u32,
}

/// `Touchdown` / `Liftoff`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Landing {
    #[serde(rename = "PlayerControlled")]
    pub player_controlled: Option<bool>,
    #[serde(rename = "Taxi")]
    pub taxi: Option<bool>,
    #[serde(rename = "Multicrew")]
    pub multicrew: Option<bool>,
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "Body")]
    pub body: String,
    #[serde(rename = "BodyID")]
    pub body_id: u32,
    #[serde(rename = "OnStation")]
    pub on_station: Option<bool>,
    #[serde(rename = "OnPlanet")]
    pub on_planet: Option<bool>,
    #[serde(rename = "Latitude")]
    pub latitude: Option<f64>,
    #[serde(rename = "Longitude")]
    pub longitude: Option<f64>,
    #[serde(rename = "NearestDestination")]
    pub nearest_destination: Option<String>,
    #[serde(rename = "NearestDestination_Localised")]
    pub nearest_destination_localised: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ApproachSettlement {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "MarketID")]
    pub market_id: Option<u64>,
    #[serde(rename = "StationFaction")]
    pub station_faction: Option<NamedFaction>,
    #[serde(rename = "StationGovernment")]
    pub station_government: Option<String>,
    #[serde(rename = "StationAllegiance")]
    pub station_allegiance: Option<String>,
    #[serde(rename = "StationServices")]
    pub station_services: Vec<String>,
    #[serde(rename = "StationEconomy")]
    pub station_economy: Option<String>,
    #[serde(rename = "StationEconomies")]
    pub station_economies: Vec<StationEconomyShare>,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "BodyID")]
    pub body_id: u32,
    #[serde(rename = "BodyName")]
    pub body_name: String,
    #[serde(rename = "Latitude")]
    pub latitude: Option<f64>,
    #[serde(rename = "Longitude")]
    pub longitude: Option<f64>,
}

/// `Embark` / `Disembark`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EmbarkDisembark {
    #[serde(rename = "SRV")]
    pub srv: bool,
    #[serde(rename = "Taxi")]
    pub taxi: bool,
    #[serde(rename = "Multicrew")]
    pub multicrew: bool,
    #[serde(rename = "ID")]
    pub id: Option<u64>,
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "Body")]
    pub body: Option<String>,
    #[serde(rename = "BodyID")]
    pub body_id: Option<u32>,
    #[serde(rename = "OnStation")]
    pub on_station: bool,
    #[serde(rename = "OnPlanet")]
    pub on_planet: bool,
    #[serde(rename = "StationName")]
    pub station_name: Option<String>,
    #[serde(rename = "StationType")]
    pub station_type: Option<String>,
    #[serde(rename = "MarketID")]
    pub market_id: Option<u64>,
}

/// `LaunchSRV`, `DockSRV`, `LaunchFighter`, `DockFighter`, `SRVDestroyed`, `FighterDestroyed`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct VehicleEvent {
    #[serde(rename = "Loadout")]
    pub loadout: Option<String>,
    #[serde(rename = "ID")]
    pub id: Option<u64>,
    #[serde(rename = "PlayerControlled")]
    pub player_controlled: Option<bool>,
    #[serde(rename = "SRVType")]
    pub srv_type: Option<String>,
    #[serde(rename = "SRVType_Localised")]
    pub srv_type_localised: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FuelScoop {
    #[serde(rename = "Scooped")]
    pub scooped: f64,
    #[serde(rename = "Total")]
    pub total: f64,
}

/// `RefuelAll` / `RefuelPartial`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Refuel {
    #[serde(rename = "Cost")]
    pub cost: i64,
    #[serde(rename = "Amount")]
    pub amount: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ReservoirReplenished {
    #[serde(rename = "FuelMain")]
    pub fuel_main: f64,
    #[serde(rename = "FuelReservoir")]
    pub fuel_reservoir: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct JetConeBoost {
    #[serde(rename = "BoostValue")]
    pub boost_value: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Killer {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Ship")]
    pub ship: Option<String>,
    #[serde(rename = "Rank")]
    pub rank: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Died {
    #[serde(rename = "KillerName")]
    pub killer_name: Option<String>,
    #[serde(rename = "KillerName_Localised")]
    pub killer_name_localised: Option<String>,
    #[serde(rename = "KillerShip")]
    pub killer_ship: Option<String>,
    #[serde(rename = "KillerRank")]
    pub killer_rank: Option<String>,
    #[serde(rename = "Killers")]
    pub killers: Vec<Killer>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Resurrect {
    #[serde(rename = "Option")]
    pub option: String,
    #[serde(rename = "Cost")]
    pub cost: i64,
    #[serde(rename = "Bankrupt")]
    pub bankrupt: bool,
}

/// `Interdicted` / `Interdiction` / `EscapeInterdiction`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct InterdictionEvent {
    #[serde(rename = "Submitted")]
    pub submitted: Option<bool>,
    #[serde(rename = "Success")]
    pub success: Option<bool>,
    #[serde(rename = "Interdictor")]
    pub interdictor: Option<String>,
    #[serde(rename = "Interdicted")]
    pub interdicted: Option<String>,
    #[serde(rename = "IsPlayer")]
    pub is_player: bool,
    #[serde(rename = "CombatRank")]
    pub combat_rank: Option<u32>,
    #[serde(rename = "Faction")]
    pub faction: Option<String>,
    #[serde(rename = "Power")]
    pub power: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct HullDamage {
    #[serde(rename = "Health")]
    pub health: f64,
    #[serde(rename = "PlayerPilot")]
    pub player_pilot: Option<bool>,
    #[serde(rename = "Fighter")]
    pub fighter: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BookTaxi {
    #[serde(rename = "Cost")]
    pub cost: i64,
    #[serde(rename = "DestinationSystem")]
    pub destination_system: Option<String>,
    #[serde(rename = "DestinationLocation")]
    pub destination_location: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Ship, fleet & outfitting
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FuelCapacity {
    #[serde(rename = "Main")]
    pub main: f64,
    #[serde(rename = "Reserve")]
    pub reserve: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EngineeringModifier {
    #[serde(rename = "Label")]
    pub label: String,
    #[serde(rename = "Value")]
    pub value: Option<f64>,
    #[serde(rename = "OriginalValue")]
    pub original_value: Option<f64>,
    #[serde(rename = "LessIsGood")]
    pub less_is_good: Option<u8>,
    #[serde(rename = "ValueStr")]
    pub value_str: Option<String>,
    #[serde(rename = "ValueStr_Localised")]
    pub value_str_localised: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ModuleEngineering {
    #[serde(rename = "Engineer")]
    pub engineer: Option<String>,
    #[serde(rename = "EngineerID")]
    pub engineer_id: Option<u32>,
    #[serde(rename = "BlueprintID")]
    pub blueprint_id: Option<u64>,
    #[serde(rename = "BlueprintName")]
    pub blueprint_name: String,
    #[serde(rename = "Level")]
    pub level: u32,
    #[serde(rename = "Quality")]
    pub quality: f64,
    #[serde(rename = "ExperimentalEffect")]
    pub experimental_effect: Option<String>,
    #[serde(rename = "ExperimentalEffect_Localised")]
    pub experimental_effect_localised: Option<String>,
    #[serde(rename = "Modifiers")]
    pub modifiers: Vec<EngineeringModifier>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LoadoutModule {
    #[serde(rename = "Slot")]
    pub slot: String,
    #[serde(rename = "Item")]
    pub item: String,
    #[serde(rename = "On")]
    pub on: bool,
    #[serde(rename = "Priority")]
    pub priority: u32,
    #[serde(rename = "Health")]
    pub health: f64,
    #[serde(rename = "Value")]
    pub value: Option<i64>,
    #[serde(rename = "AmmoInClip")]
    pub ammo_in_clip: Option<u32>,
    #[serde(rename = "AmmoInHopper")]
    pub ammo_in_hopper: Option<u32>,
    #[serde(rename = "Engineering")]
    pub engineering: Option<ModuleEngineering>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Loadout {
    #[serde(rename = "Ship")]
    pub ship: String,
    #[serde(rename = "ShipID")]
    pub ship_id: u64,
    #[serde(rename = "ShipName")]
    pub ship_name: String,
    #[serde(rename = "ShipIdent")]
    pub ship_ident: String,
    #[serde(rename = "HullValue")]
    pub hull_value: Option<i64>,
    #[serde(rename = "ModulesValue")]
    pub modules_value: Option<i64>,
    #[serde(rename = "HullHealth")]
    pub hull_health: f64,
    #[serde(rename = "UnladenMass")]
    pub unladen_mass: f64,
    #[serde(rename = "CargoCapacity")]
    pub cargo_capacity: u32,
    #[serde(rename = "MaxJumpRange")]
    pub max_jump_range: f64,
    #[serde(rename = "FuelCapacity")]
    pub fuel_capacity: FuelCapacity,
    #[serde(rename = "Rebuy")]
    pub rebuy: i64,
    #[serde(rename = "Hot")]
    pub hot: Option<bool>,
    #[serde(rename = "Modules")]
    pub modules: Vec<LoadoutModule>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ShipyardBuy {
    #[serde(rename = "ShipType")]
    pub ship_type: String,
    #[serde(rename = "ShipType_Localised")]
    pub ship_type_localised: Option<String>,
    #[serde(rename = "ShipPrice")]
    pub ship_price: i64,
    #[serde(rename = "StoreOldShip")]
    pub store_old_ship: Option<String>,
    #[serde(rename = "StoreShipID")]
    pub store_ship_id: Option<u64>,
    #[serde(rename = "SellOldShip")]
    pub sell_old_ship: Option<String>,
    #[serde(rename = "SellShipID")]
    pub sell_ship_id: Option<u64>,
    #[serde(rename = "SellPrice")]
    pub sell_price: Option<i64>,
    #[serde(rename = "MarketID")]
    pub market_id: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ShipyardNew {
    #[serde(rename = "ShipType")]
    pub ship_type: String,
    #[serde(rename = "ShipType_Localised")]
    pub ship_type_localised: Option<String>,
    #[serde(rename = "NewShipID")]
    pub new_ship_id: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ShipyardSell {
    #[serde(rename = "ShipType")]
    pub ship_type: String,
    #[serde(rename = "SellShipID")]
    pub sell_ship_id: u64,
    #[serde(rename = "ShipPrice")]
    pub ship_price: i64,
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "System")]
    pub system: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ShipyardSwap {
    #[serde(rename = "ShipType")]
    pub ship_type: String,
    #[serde(rename = "ShipType_Localised")]
    pub ship_type_localised: Option<String>,
    #[serde(rename = "ShipID")]
    pub ship_id: u64,
    #[serde(rename = "StoreOldShip")]
    pub store_old_ship: Option<String>,
    #[serde(rename = "StoreShipID")]
    pub store_ship_id: Option<u64>,
    #[serde(rename = "SellOldShip")]
    pub sell_old_ship: Option<String>,
    #[serde(rename = "SellShipID")]
    pub sell_ship_id: Option<u64>,
    #[serde(rename = "MarketID")]
    pub market_id: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ShipyardTransfer {
    #[serde(rename = "ShipType")]
    pub ship_type: String,
    #[serde(rename = "ShipType_Localised")]
    pub ship_type_localised: Option<String>,
    #[serde(rename = "ShipID")]
    pub ship_id: u64,
    #[serde(rename = "System")]
    pub system: String,
    #[serde(rename = "ShipMarketID")]
    pub ship_market_id: u64,
    #[serde(rename = "Distance")]
    pub distance: f64,
    #[serde(rename = "TransferPrice")]
    pub transfer_price: i64,
    #[serde(rename = "TransferTime")]
    pub transfer_time: i64,
    #[serde(rename = "MarketID")]
    pub market_id: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StoredShip {
    #[serde(rename = "ShipID")]
    pub ship_id: u64,
    #[serde(rename = "ShipType")]
    pub ship_type: String,
    #[serde(rename = "ShipType_Localised")]
    pub ship_type_localised: Option<String>,
    #[serde(rename = "Name")]
    pub name: Option<String>,
    #[serde(rename = "Value")]
    pub value: i64,
    #[serde(rename = "Hot")]
    pub hot: bool,
    // Remote only
    #[serde(rename = "StarSystem")]
    pub star_system: Option<String>,
    #[serde(rename = "ShipMarketID")]
    pub ship_market_id: Option<u64>,
    #[serde(rename = "TransferPrice")]
    pub transfer_price: Option<i64>,
    #[serde(rename = "TransferTime")]
    pub transfer_time: Option<i64>,
    #[serde(rename = "InTransit")]
    pub in_transit: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StoredShips {
    #[serde(rename = "StationName")]
    pub station_name: String,
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "ShipsHere")]
    pub ships_here: Vec<StoredShip>,
    #[serde(rename = "ShipsRemote")]
    pub ships_remote: Vec<StoredShip>,
}

/// `ShipyardRedeem` (claiming a store-bought ship) / `ShipRedeemed` (it arrives with a new ShipID).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ShipRedeemed {
    #[serde(rename = "ShipType")]
    pub ship_type: String,
    #[serde(rename = "ShipType_Localised")]
    pub ship_type_localised: Option<String>,
    #[serde(rename = "NewShipID")]
    pub new_ship_id: Option<u64>,
    #[serde(rename = "BundleID")]
    pub bundle_id: Option<u64>,
    #[serde(rename = "MarketID")]
    pub market_id: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SetUserShipName {
    #[serde(rename = "Ship")]
    pub ship: String,
    #[serde(rename = "ShipID")]
    pub ship_id: u64,
    #[serde(rename = "UserShipName")]
    pub user_ship_name: String,
    #[serde(rename = "UserShipId")]
    pub user_ship_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StoredModule {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "StorageSlot")]
    pub storage_slot: u32,
    #[serde(rename = "StarSystem")]
    pub star_system: Option<String>,
    #[serde(rename = "MarketID")]
    pub market_id: Option<u64>,
    #[serde(rename = "TransferCost")]
    pub transfer_cost: Option<i64>,
    #[serde(rename = "TransferTime")]
    pub transfer_time: Option<i64>,
    #[serde(rename = "BuyPrice")]
    pub buy_price: Option<i64>,
    #[serde(rename = "Hot")]
    pub hot: bool,
    #[serde(rename = "InTransit")]
    pub in_transit: Option<bool>,
    #[serde(rename = "EngineerModifications")]
    pub engineer_modifications: Option<String>,
    #[serde(rename = "Level")]
    pub level: Option<u32>,
    #[serde(rename = "Quality")]
    pub quality: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StoredModules {
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "StationName")]
    pub station_name: String,
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "Items")]
    pub items: Vec<StoredModule>,
}

/// `ModuleBuy`, `ModuleSell`, `ModuleStore`, `ModuleRetrieve`, `ModuleSwap`, `ModuleSellRemote`, `ModuleBuyAndStore`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ModuleChange {
    #[serde(rename = "Slot")]
    pub slot: Option<String>,
    #[serde(rename = "BuyItem")]
    pub buy_item: Option<String>,
    #[serde(rename = "BuyPrice")]
    pub buy_price: Option<i64>,
    #[serde(rename = "SellItem")]
    pub sell_item: Option<String>,
    #[serde(rename = "SellPrice")]
    pub sell_price: Option<i64>,
    #[serde(rename = "StoredItem")]
    pub stored_item: Option<String>,
    #[serde(rename = "RetrievedItem")]
    pub retrieved_item: Option<String>,
    #[serde(rename = "Cost")]
    pub cost: Option<i64>,
    #[serde(rename = "Ship")]
    pub ship: Option<String>,
    #[serde(rename = "ShipID")]
    pub ship_id: Option<u64>,
    #[serde(rename = "MarketID")]
    pub market_id: Option<u64>,
}

/// `Repair`, `RepairAll`, `BuyAmmo`, `BuyDrones`, `SellDrones`, `RestockVehicle`, `PayFines`,
/// `PayBounties`, `PayLegacyFines` — anything that costs or pays credits with a `Cost`/`Amount`/`TotalCost`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CreditFlow {
    #[serde(rename = "Cost")]
    pub cost: Option<i64>,
    #[serde(rename = "TotalCost")]
    pub total_cost: Option<i64>,
    #[serde(rename = "Amount")]
    pub amount: Option<i64>,
    #[serde(rename = "SellPrice")]
    pub sell_price: Option<i64>,
    #[serde(rename = "TotalSale")]
    pub total_sale: Option<i64>,
    #[serde(rename = "Count")]
    pub count: Option<i64>,
    #[serde(rename = "Type")]
    pub kind: Option<String>,
    #[serde(rename = "Faction")]
    pub faction: Option<String>,
    #[serde(rename = "AllFines")]
    pub all_fines: Option<bool>,
}

// ---------------------------------------------------------------------------------------------
// Exploration
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NamePercent {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Percent")]
    pub percent: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BodyComposition {
    #[serde(rename = "Ice")]
    pub ice: f64,
    #[serde(rename = "Rock")]
    pub rock: f64,
    #[serde(rename = "Metal")]
    pub metal: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Ring {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "RingClass")]
    pub ring_class: String,
    #[serde(rename = "MassMT")]
    pub mass_mt: f64,
    #[serde(rename = "InnerRad")]
    pub inner_rad: f64,
    #[serde(rename = "OuterRad")]
    pub outer_rad: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Scan {
    #[serde(rename = "ScanType")]
    pub scan_type: String,
    #[serde(rename = "BodyName")]
    pub body_name: String,
    #[serde(rename = "BodyID")]
    pub body_id: u32,
    /// e.g. `[{"Null":5},{"Star":0}]` — nearest parent first.
    #[serde(rename = "Parents")]
    pub parents: Vec<BTreeMap<String, u32>>,
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "DistanceFromArrivalLS")]
    pub distance_from_arrival_ls: f64,
    // stars
    #[serde(rename = "StarType")]
    pub star_type: Option<String>,
    #[serde(rename = "Subclass")]
    pub subclass: Option<u32>,
    #[serde(rename = "StellarMass")]
    pub stellar_mass: Option<f64>,
    #[serde(rename = "AbsoluteMagnitude")]
    pub absolute_magnitude: Option<f64>,
    #[serde(rename = "Age_MY")]
    pub age_my: Option<f64>,
    #[serde(rename = "Luminosity")]
    pub luminosity: Option<String>,
    // planets
    #[serde(rename = "TidalLock")]
    pub tidal_lock: Option<bool>,
    #[serde(rename = "TerraformState")]
    pub terraform_state: Option<String>,
    #[serde(rename = "PlanetClass")]
    pub planet_class: Option<String>,
    #[serde(rename = "Atmosphere")]
    pub atmosphere: Option<String>,
    #[serde(rename = "AtmosphereType")]
    pub atmosphere_type: Option<String>,
    #[serde(rename = "AtmosphereComposition")]
    pub atmosphere_composition: Vec<NamePercent>,
    #[serde(rename = "Volcanism")]
    pub volcanism: Option<String>,
    #[serde(rename = "MassEM")]
    pub mass_em: Option<f64>,
    #[serde(rename = "SurfaceGravity")]
    pub surface_gravity: Option<f64>,
    #[serde(rename = "SurfacePressure")]
    pub surface_pressure: Option<f64>,
    #[serde(rename = "Landable")]
    pub landable: Option<bool>,
    #[serde(rename = "Materials")]
    pub materials: Vec<NamePercent>,
    #[serde(rename = "Composition")]
    pub composition: Option<BodyComposition>,
    // both
    #[serde(rename = "Radius")]
    pub radius: Option<f64>,
    #[serde(rename = "SurfaceTemperature")]
    pub surface_temperature: Option<f64>,
    #[serde(rename = "SemiMajorAxis")]
    pub semi_major_axis: Option<f64>,
    #[serde(rename = "Eccentricity")]
    pub eccentricity: Option<f64>,
    #[serde(rename = "OrbitalInclination")]
    pub orbital_inclination: Option<f64>,
    #[serde(rename = "Periapsis")]
    pub periapsis: Option<f64>,
    #[serde(rename = "OrbitalPeriod")]
    pub orbital_period: Option<f64>,
    #[serde(rename = "AscendingNode")]
    pub ascending_node: Option<f64>,
    #[serde(rename = "MeanAnomaly")]
    pub mean_anomaly: Option<f64>,
    #[serde(rename = "RotationPeriod")]
    pub rotation_period: Option<f64>,
    #[serde(rename = "AxialTilt")]
    pub axial_tilt: Option<f64>,
    #[serde(rename = "Rings")]
    pub rings: Vec<Ring>,
    #[serde(rename = "ReserveLevel")]
    pub reserve_level: Option<String>,
    #[serde(rename = "WasDiscovered")]
    pub was_discovered: Option<bool>,
    #[serde(rename = "WasMapped")]
    pub was_mapped: Option<bool>,
    #[serde(rename = "WasFootfalled")]
    pub was_footfalled: Option<bool>,
}

impl Scan {
    pub fn is_star(&self) -> bool {
        self.star_type.is_some()
    }
    pub fn is_planet(&self) -> bool {
        self.planet_class.is_some()
    }
    /// Belt clusters / rings appear as Scan events with neither star nor planet data.
    pub fn is_belt_or_ring(&self) -> bool {
        !self.is_star() && !self.is_planet()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FSSDiscoveryScan {
    #[serde(rename = "Progress")]
    pub progress: f64,
    #[serde(rename = "BodyCount")]
    pub body_count: u32,
    #[serde(rename = "NonBodyCount")]
    pub non_body_count: u32,
    #[serde(rename = "SystemName")]
    pub system_name: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FSSAllBodiesFound {
    #[serde(rename = "SystemName")]
    pub system_name: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "Count")]
    pub count: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BodySignal {
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "Type_Localised")]
    pub kind_localised: Option<String>,
    #[serde(rename = "Count")]
    pub count: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct GenusEntry {
    #[serde(rename = "Genus")]
    pub genus: String,
    #[serde(rename = "Genus_Localised")]
    pub genus_localised: Option<String>,
}

/// `FSSBodySignals` / `SAASignalsFound`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BodySignals {
    #[serde(rename = "BodyName")]
    pub body_name: String,
    #[serde(rename = "BodyID")]
    pub body_id: u32,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "Signals")]
    pub signals: Vec<BodySignal>,
    #[serde(rename = "Genuses")]
    pub genuses: Vec<GenusEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SAAScanComplete {
    #[serde(rename = "BodyName")]
    pub body_name: String,
    #[serde(rename = "BodyID")]
    pub body_id: u32,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "ProbesUsed")]
    pub probes_used: u32,
    #[serde(rename = "EfficiencyTarget")]
    pub efficiency_target: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FSSSignalDiscovered {
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "SignalName")]
    pub signal_name: String,
    #[serde(rename = "SignalName_Localised")]
    pub signal_name_localised: Option<String>,
    #[serde(rename = "SignalType")]
    pub signal_type: Option<String>,
    #[serde(rename = "IsStation")]
    pub is_station: Option<bool>,
    #[serde(rename = "USSType")]
    pub uss_type: Option<String>,
    #[serde(rename = "USSType_Localised")]
    pub uss_type_localised: Option<String>,
    #[serde(rename = "SpawningState")]
    pub spawning_state: Option<String>,
    #[serde(rename = "SpawningFaction")]
    pub spawning_faction: Option<String>,
    #[serde(rename = "ThreatLevel")]
    pub threat_level: Option<u32>,
    #[serde(rename = "TimeRemaining")]
    pub time_remaining: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ScanBaryCentre {
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "BodyID")]
    pub body_id: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NavBeaconScan {
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "NumBodies")]
    pub num_bodies: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CodexEntry {
    #[serde(rename = "EntryID")]
    pub entry_id: u64,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "SubCategory")]
    pub sub_category: String,
    #[serde(rename = "SubCategory_Localised")]
    pub sub_category_localised: Option<String>,
    #[serde(rename = "Category")]
    pub category: String,
    #[serde(rename = "Category_Localised")]
    pub category_localised: Option<String>,
    #[serde(rename = "Region")]
    pub region: String,
    #[serde(rename = "Region_Localised")]
    pub region_localised: Option<String>,
    #[serde(rename = "System")]
    pub system: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "BodyID")]
    pub body_id: Option<u32>,
    #[serde(rename = "Latitude")]
    pub latitude: Option<f64>,
    #[serde(rename = "Longitude")]
    pub longitude: Option<f64>,
    #[serde(rename = "IsNewEntry")]
    pub is_new_entry: Option<bool>,
    #[serde(rename = "VoucherAmount")]
    pub voucher_amount: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ScanOrganic {
    /// `Log` (1st sample), `Sample` (2nd), `Analyse` (3rd, complete).
    #[serde(rename = "ScanType")]
    pub scan_type: String,
    #[serde(rename = "Genus")]
    pub genus: String,
    #[serde(rename = "Genus_Localised")]
    pub genus_localised: Option<String>,
    #[serde(rename = "Species")]
    pub species: String,
    #[serde(rename = "Species_Localised")]
    pub species_localised: Option<String>,
    #[serde(rename = "Variant")]
    pub variant: Option<String>,
    #[serde(rename = "Variant_Localised")]
    pub variant_localised: Option<String>,
    #[serde(rename = "WasLogged")]
    pub was_logged: Option<bool>,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "Body")]
    pub body: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BioDataSale {
    #[serde(rename = "Genus")]
    pub genus: String,
    #[serde(rename = "Genus_Localised")]
    pub genus_localised: Option<String>,
    #[serde(rename = "Species")]
    pub species: String,
    #[serde(rename = "Species_Localised")]
    pub species_localised: Option<String>,
    #[serde(rename = "Variant")]
    pub variant: Option<String>,
    #[serde(rename = "Variant_Localised")]
    pub variant_localised: Option<String>,
    #[serde(rename = "Value")]
    pub value: i64,
    #[serde(rename = "Bonus")]
    pub bonus: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SellOrganicData {
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "BioData")]
    pub bio_data: Vec<BioDataSale>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SoldSystem {
    #[serde(rename = "SystemName")]
    pub system_name: String,
    #[serde(rename = "NumBodies")]
    pub num_bodies: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MultiSellExplorationData {
    #[serde(rename = "Discovered")]
    pub discovered: Vec<SoldSystem>,
    #[serde(rename = "BaseValue")]
    pub base_value: i64,
    #[serde(rename = "Bonus")]
    pub bonus: i64,
    #[serde(rename = "TotalEarnings")]
    pub total_earnings: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SellExplorationData {
    #[serde(rename = "Systems")]
    pub systems: Vec<String>,
    #[serde(rename = "Discovered")]
    pub discovered: Vec<String>,
    #[serde(rename = "BaseValue")]
    pub base_value: i64,
    #[serde(rename = "Bonus")]
    pub bonus: i64,
    #[serde(rename = "TotalEarnings")]
    pub total_earnings: i64,
}

// ---------------------------------------------------------------------------------------------
// Trade & cargo
// ---------------------------------------------------------------------------------------------

/// The `Market` journal event; full data is in Market.json.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MarketEvent {
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "StationName")]
    pub station_name: String,
    #[serde(rename = "StationType")]
    pub station_type: Option<String>,
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "CarrierDockingAccess")]
    pub carrier_docking_access: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MarketBuy {
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "Type_Localised")]
    pub kind_localised: Option<String>,
    #[serde(rename = "Count")]
    pub count: i64,
    #[serde(rename = "BuyPrice")]
    pub buy_price: i64,
    #[serde(rename = "TotalCost")]
    pub total_cost: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MarketSell {
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "Type_Localised")]
    pub kind_localised: Option<String>,
    #[serde(rename = "Count")]
    pub count: i64,
    #[serde(rename = "SellPrice")]
    pub sell_price: i64,
    #[serde(rename = "TotalSale")]
    pub total_sale: i64,
    #[serde(rename = "AvgPricePaid")]
    pub avg_price_paid: i64,
    #[serde(rename = "IllegalGoods")]
    pub illegal_goods: Option<bool>,
    #[serde(rename = "StolenGoods")]
    pub stolen_goods: Option<bool>,
    #[serde(rename = "BlackMarket")]
    pub black_market: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CargoItem {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "Count")]
    pub count: i64,
    #[serde(rename = "Stolen")]
    pub stolen: i64,
    #[serde(rename = "MissionID")]
    pub mission_id: Option<u64>,
}

/// `Cargo` event (and Cargo.json). `Inventory` is only present in the file / at startup.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Cargo {
    #[serde(rename = "Vessel")]
    pub vessel: String,
    #[serde(rename = "Count")]
    pub count: i64,
    #[serde(rename = "Inventory")]
    pub inventory: Option<Vec<CargoItem>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CargoTransferItem {
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "Count")]
    pub count: i64,
    #[serde(rename = "Direction")]
    pub direction: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CargoTransfer {
    #[serde(rename = "Transfers")]
    pub transfers: Vec<CargoTransferItem>,
}

/// `CollectCargo` / `EjectCargo`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CargoMove {
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "Type_Localised")]
    pub kind_localised: Option<String>,
    #[serde(rename = "Count")]
    pub count: Option<i64>,
    #[serde(rename = "Stolen")]
    pub stolen: Option<bool>,
    #[serde(rename = "Abandoned")]
    pub abandoned: Option<bool>,
    #[serde(rename = "MissionID")]
    pub mission_id: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CargoDepot {
    #[serde(rename = "MissionID")]
    pub mission_id: u64,
    #[serde(rename = "UpdateType")]
    pub update_type: String,
    #[serde(rename = "CargoType")]
    pub cargo_type: Option<String>,
    #[serde(rename = "Count")]
    pub count: Option<i64>,
    #[serde(rename = "ItemsCollected")]
    pub items_collected: i64,
    #[serde(rename = "ItemsDelivered")]
    pub items_delivered: i64,
    #[serde(rename = "TotalItemsToDeliver")]
    pub total_items_to_deliver: i64,
    #[serde(rename = "Progress")]
    pub progress: f64,
}

// ---------------------------------------------------------------------------------------------
// Mining
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProspectedMaterial {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "Proportion")]
    pub proportion: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProspectedAsteroid {
    #[serde(rename = "Materials")]
    pub materials: Vec<ProspectedMaterial>,
    #[serde(rename = "MotherlodeMaterial")]
    pub motherlode_material: Option<String>,
    #[serde(rename = "MotherlodeMaterial_Localised")]
    pub motherlode_material_localised: Option<String>,
    #[serde(rename = "Content")]
    pub content: String,
    #[serde(rename = "Content_Localised")]
    pub content_localised: Option<String>,
    #[serde(rename = "Remaining")]
    pub remaining: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MiningRefined {
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "Type_Localised")]
    pub kind_localised: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LaunchDrone {
    #[serde(rename = "Type")]
    pub kind: String,
}

// ---------------------------------------------------------------------------------------------
// Materials & engineering
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Materials {
    #[serde(rename = "Raw")]
    pub raw: Vec<NameCount>,
    #[serde(rename = "Manufactured")]
    pub manufactured: Vec<NameCount>,
    #[serde(rename = "Encoded")]
    pub encoded: Vec<NameCount>,
}

/// `MaterialCollected` / `MaterialDiscarded` / `MaterialDiscovered`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MaterialChange {
    #[serde(rename = "Category")]
    pub category: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "Count")]
    pub count: i64,
    #[serde(rename = "DiscoveryNumber")]
    pub discovery_number: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TradedMaterial {
    #[serde(rename = "Material")]
    pub material: String,
    #[serde(rename = "Material_Localised")]
    pub material_localised: Option<String>,
    #[serde(rename = "Category")]
    pub category: String,
    #[serde(rename = "Quantity")]
    pub quantity: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MaterialTrade {
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "TraderType")]
    pub trader_type: String,
    #[serde(rename = "Paid")]
    pub paid: TradedMaterial,
    #[serde(rename = "Received")]
    pub received: TradedMaterial,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EngineerCraft {
    #[serde(rename = "Slot")]
    pub slot: Option<String>,
    #[serde(rename = "Module")]
    pub module: Option<String>,
    #[serde(rename = "Ingredients")]
    pub ingredients: Vec<NameCount>,
    #[serde(rename = "Engineer")]
    pub engineer: Option<String>,
    #[serde(rename = "EngineerID")]
    pub engineer_id: Option<u32>,
    #[serde(rename = "BlueprintID")]
    pub blueprint_id: Option<u64>,
    #[serde(rename = "BlueprintName")]
    pub blueprint_name: String,
    #[serde(rename = "Level")]
    pub level: u32,
    #[serde(rename = "Quality")]
    pub quality: f64,
    #[serde(rename = "ApplyExperimentalEffect")]
    pub apply_experimental_effect: Option<String>,
    #[serde(rename = "ExperimentalEffect")]
    pub experimental_effect: Option<String>,
    #[serde(rename = "Modifiers")]
    pub modifiers: Vec<EngineeringModifier>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EngineerContribution {
    #[serde(rename = "Engineer")]
    pub engineer: String,
    #[serde(rename = "EngineerID")]
    pub engineer_id: u32,
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "Commodity")]
    pub commodity: Option<String>,
    #[serde(rename = "Material")]
    pub material: Option<String>,
    #[serde(rename = "Faction")]
    pub faction: Option<String>,
    #[serde(rename = "Quantity")]
    pub quantity: i64,
    #[serde(rename = "TotalQuantity")]
    pub total_quantity: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Synthesis {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Materials")]
    pub materials: Vec<NameCount>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BrokerMaterial {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "Count")]
    pub count: i64,
    #[serde(rename = "Category")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TechnologyBroker {
    #[serde(rename = "BrokerType")]
    pub broker_type: String,
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "ItemsUnlocked")]
    pub items_unlocked: Vec<NameCount>,
    #[serde(rename = "Commodities")]
    pub commodities: Vec<BrokerMaterial>,
    #[serde(rename = "Materials")]
    pub materials: Vec<BrokerMaterial>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ScientificResearch {
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Category")]
    pub category: String,
    #[serde(rename = "Count")]
    pub count: i64,
}

// ---------------------------------------------------------------------------------------------
// Odyssey: micro-resources, suits, weapons
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MicroResource {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "OwnerID")]
    pub owner_id: Option<u64>,
    #[serde(rename = "MissionID")]
    pub mission_id: Option<u64>,
    #[serde(rename = "Count")]
    pub count: i64,
    /// `Item` / `Component` / `Consumable` / `Data` (BackpackChange) or category (trades).
    #[serde(rename = "Type")]
    pub kind: Option<String>,
    #[serde(rename = "Category")]
    pub category: Option<String>,
}

/// `ShipLocker` / `Backpack` (event + .json file). The journal events at startup may be empty
/// (`{}`) with the real content only in the json file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MicroResourceInventory {
    #[serde(rename = "Items")]
    pub items: Option<Vec<MicroResource>>,
    #[serde(rename = "Components")]
    pub components: Option<Vec<MicroResource>>,
    #[serde(rename = "Consumables")]
    pub consumables: Option<Vec<MicroResource>>,
    #[serde(rename = "Data")]
    pub data: Option<Vec<MicroResource>>,
}

impl MicroResourceInventory {
    pub fn has_content(&self) -> bool {
        self.items.is_some() || self.components.is_some() || self.consumables.is_some() || self.data.is_some()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BackpackChange {
    #[serde(rename = "Added")]
    pub added: Vec<MicroResource>,
    #[serde(rename = "Removed")]
    pub removed: Vec<MicroResource>,
}

/// `BuyMicroResources` / `SellMicroResources` / `TradeMicroResources` / `TransferMicroResources`
/// / `CollectItems` / `DropItems` / `UseConsumable`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MicroResourceFlow {
    #[serde(rename = "MicroResources")]
    pub micro_resources: Vec<MicroResource>,
    #[serde(rename = "Offered")]
    pub offered: Vec<MicroResource>,
    #[serde(rename = "Transfers")]
    pub transfers: Vec<MicroResourceTransfer>,
    #[serde(rename = "Received")]
    pub received: Option<String>,
    #[serde(rename = "Name")]
    pub name: Option<String>,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "Type")]
    pub kind: Option<String>,
    #[serde(rename = "Category")]
    pub category: Option<String>,
    #[serde(rename = "Count")]
    pub count: Option<i64>,
    #[serde(rename = "TotalCount")]
    pub total_count: Option<i64>,
    #[serde(rename = "Price")]
    pub price: Option<i64>,
    #[serde(rename = "MarketID")]
    pub market_id: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MicroResourceTransfer {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Category")]
    pub category: String,
    #[serde(rename = "LockerOldCount")]
    pub locker_old_count: i64,
    #[serde(rename = "LockerNewCount")]
    pub locker_new_count: i64,
    #[serde(rename = "Direction")]
    pub direction: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SuitModule {
    #[serde(rename = "SlotName")]
    pub slot_name: String,
    #[serde(rename = "SuitModuleID")]
    pub suit_module_id: u64,
    #[serde(rename = "ModuleName")]
    pub module_name: String,
    #[serde(rename = "ModuleName_Localised")]
    pub module_name_localised: Option<String>,
    #[serde(rename = "Class")]
    pub class: u32,
    #[serde(rename = "WeaponMods")]
    pub weapon_mods: Vec<String>,
}

/// `SuitLoadout` / `SwitchSuitLoadout` / `CreateSuitLoadout`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SuitLoadout {
    #[serde(rename = "SuitID")]
    pub suit_id: u64,
    #[serde(rename = "SuitName")]
    pub suit_name: String,
    #[serde(rename = "SuitName_Localised")]
    pub suit_name_localised: Option<String>,
    #[serde(rename = "SuitMods")]
    pub suit_mods: Vec<String>,
    #[serde(rename = "LoadoutID")]
    pub loadout_id: u64,
    #[serde(rename = "LoadoutName")]
    pub loadout_name: String,
    #[serde(rename = "Modules")]
    pub modules: Vec<SuitModule>,
}

/// `BuySuit` / `SellSuit` / `BuyWeapon` / `SellWeapon`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct GearPurchase {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "Price")]
    pub price: i64,
    #[serde(rename = "SuitID")]
    pub suit_id: Option<u64>,
    #[serde(rename = "SuitModuleID")]
    pub suit_module_id: Option<u64>,
    #[serde(rename = "Class")]
    pub class: Option<u32>,
    #[serde(rename = "SuitMods")]
    pub suit_mods: Vec<String>,
    #[serde(rename = "WeaponMods")]
    pub weapon_mods: Vec<String>,
}

/// `UpgradeSuit` / `UpgradeWeapon`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpgradeGear {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "SuitID")]
    pub suit_id: Option<u64>,
    #[serde(rename = "SuitModuleID")]
    pub suit_module_id: Option<u64>,
    #[serde(rename = "Class")]
    pub class: u32,
    #[serde(rename = "Cost")]
    pub cost: i64,
    #[serde(rename = "Resources")]
    pub resources: Vec<NameCount>,
}

// ---------------------------------------------------------------------------------------------
// Missions, combat, crime
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MissionSummary {
    #[serde(rename = "MissionID")]
    pub mission_id: u64,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "PassengerMission")]
    pub passenger_mission: bool,
    /// Seconds remaining until expiry.
    #[serde(rename = "Expires")]
    pub expires: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Missions {
    #[serde(rename = "Active")]
    pub active: Vec<MissionSummary>,
    #[serde(rename = "Failed")]
    pub failed: Vec<MissionSummary>,
    #[serde(rename = "Complete")]
    pub complete: Vec<MissionSummary>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MissionAccepted {
    #[serde(rename = "Faction")]
    pub faction: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "LocalisedName")]
    pub localised_name: Option<String>,
    #[serde(rename = "MissionID")]
    pub mission_id: u64,
    #[serde(rename = "Influence")]
    pub influence: Option<String>,
    #[serde(rename = "Reputation")]
    pub reputation: Option<String>,
    #[serde(rename = "Reward")]
    pub reward: Option<i64>,
    #[serde(rename = "Wing")]
    pub wing: Option<bool>,
    #[serde(rename = "Expiry")]
    pub expiry: Option<String>,
    #[serde(rename = "TargetFaction")]
    pub target_faction: Option<String>,
    #[serde(rename = "KillCount")]
    pub kill_count: Option<i64>,
    #[serde(rename = "TargetType")]
    pub target_type: Option<String>,
    #[serde(rename = "TargetType_Localised")]
    pub target_type_localised: Option<String>,
    #[serde(rename = "Target")]
    pub target: Option<String>,
    #[serde(rename = "Target_Localised")]
    pub target_localised: Option<String>,
    #[serde(rename = "DestinationSystem")]
    pub destination_system: Option<String>,
    #[serde(rename = "DestinationStation")]
    pub destination_station: Option<String>,
    #[serde(rename = "DestinationSettlement")]
    pub destination_settlement: Option<String>,
    #[serde(rename = "Commodity")]
    pub commodity: Option<String>,
    #[serde(rename = "Commodity_Localised")]
    pub commodity_localised: Option<String>,
    #[serde(rename = "Count")]
    pub count: Option<i64>,
    #[serde(rename = "PassengerCount")]
    pub passenger_count: Option<i64>,
    #[serde(rename = "PassengerVIPs")]
    pub passenger_vips: Option<bool>,
    #[serde(rename = "PassengerWanted")]
    pub passenger_wanted: Option<bool>,
    #[serde(rename = "PassengerType")]
    pub passenger_type: Option<String>,
    #[serde(rename = "Donation")]
    pub donation: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct InfluenceEffect {
    #[serde(rename = "SystemAddress")]
    pub system_address: Option<u64>,
    #[serde(rename = "Trend")]
    pub trend: Option<String>,
    #[serde(rename = "Influence")]
    pub influence: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FactionEffect {
    #[serde(rename = "Faction")]
    pub faction: String,
    #[serde(rename = "Influence")]
    pub influence: Vec<InfluenceEffect>,
    #[serde(rename = "ReputationTrend")]
    pub reputation_trend: Option<String>,
    #[serde(rename = "Reputation")]
    pub reputation: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MissionCompleted {
    #[serde(rename = "Faction")]
    pub faction: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "LocalisedName")]
    pub localised_name: Option<String>,
    #[serde(rename = "MissionID")]
    pub mission_id: u64,
    #[serde(rename = "Reward")]
    pub reward: Option<i64>,
    #[serde(rename = "Donation")]
    pub donation: Option<String>,
    #[serde(rename = "Donated")]
    pub donated: Option<i64>,
    #[serde(rename = "TargetFaction")]
    pub target_faction: Option<String>,
    #[serde(rename = "Commodity")]
    pub commodity: Option<String>,
    #[serde(rename = "Count")]
    pub count: Option<i64>,
    #[serde(rename = "FactionEffects")]
    pub faction_effects: Vec<FactionEffect>,
}

/// `MissionFailed` / `MissionAbandoned`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MissionEnded {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "LocalisedName")]
    pub localised_name: Option<String>,
    #[serde(rename = "MissionID")]
    pub mission_id: u64,
    #[serde(rename = "Fine")]
    pub fine: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MissionRedirected {
    #[serde(rename = "MissionID")]
    pub mission_id: u64,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "LocalisedName")]
    pub localised_name: Option<String>,
    #[serde(rename = "NewDestinationStation")]
    pub new_destination_station: Option<String>,
    #[serde(rename = "NewDestinationSystem")]
    pub new_destination_system: Option<String>,
    #[serde(rename = "OldDestinationStation")]
    pub old_destination_station: Option<String>,
    #[serde(rename = "OldDestinationSystem")]
    pub old_destination_system: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FactionReward {
    #[serde(rename = "Faction")]
    pub faction: String,
    #[serde(rename = "Reward")]
    pub reward: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Bounty {
    #[serde(rename = "Rewards")]
    pub rewards: Vec<FactionReward>,
    #[serde(rename = "Target")]
    pub target: Option<String>,
    #[serde(rename = "Target_Localised")]
    pub target_localised: Option<String>,
    #[serde(rename = "TotalReward")]
    pub total_reward: Option<i64>,
    #[serde(rename = "VictimFaction")]
    pub victim_faction: Option<String>,
    #[serde(rename = "SharedWithOthers")]
    pub shared_with_others: Option<u32>,
    #[serde(rename = "PilotName")]
    pub pilot_name: Option<String>,
    // Single-faction variant (e.g. skimmers)
    #[serde(rename = "Faction")]
    pub faction: Option<String>,
    #[serde(rename = "Reward")]
    pub reward: Option<i64>,
}

/// `FactionKillBond` / `CapShipBond`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct KillBond {
    #[serde(rename = "Reward")]
    pub reward: i64,
    #[serde(rename = "AwardingFaction")]
    pub awarding_faction: String,
    #[serde(rename = "VictimFaction")]
    pub victim_faction: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RedeemVoucher {
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "Amount")]
    pub amount: i64,
    #[serde(rename = "Faction")]
    pub faction: Option<String>,
    #[serde(rename = "Factions")]
    pub factions: Vec<FactionReward>,
    #[serde(rename = "BrokerPercentage")]
    pub broker_percentage: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CommitCrime {
    #[serde(rename = "CrimeType")]
    pub crime_type: String,
    #[serde(rename = "Faction")]
    pub faction: String,
    #[serde(rename = "Victim")]
    pub victim: Option<String>,
    #[serde(rename = "Fine")]
    pub fine: Option<i64>,
    #[serde(rename = "Bounty")]
    pub bounty: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ShipTargeted {
    #[serde(rename = "TargetLocked")]
    pub target_locked: bool,
    #[serde(rename = "Ship")]
    pub ship: Option<String>,
    #[serde(rename = "Ship_Localised")]
    pub ship_localised: Option<String>,
    #[serde(rename = "ScanStage")]
    pub scan_stage: Option<u32>,
    #[serde(rename = "PilotName")]
    pub pilot_name: Option<String>,
    #[serde(rename = "PilotName_Localised")]
    pub pilot_name_localised: Option<String>,
    #[serde(rename = "PilotRank")]
    pub pilot_rank: Option<String>,
    #[serde(rename = "ShieldHealth")]
    pub shield_health: Option<f64>,
    #[serde(rename = "HullHealth")]
    pub hull_health: Option<f64>,
    #[serde(rename = "Faction")]
    pub faction: Option<String>,
    #[serde(rename = "LegalStatus")]
    pub legal_status: Option<String>,
    #[serde(rename = "Bounty")]
    pub bounty: Option<i64>,
    #[serde(rename = "Power")]
    pub power: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Fleet carriers
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierSpaceUsage {
    #[serde(rename = "TotalCapacity")]
    pub total_capacity: i64,
    #[serde(rename = "Crew")]
    pub crew: i64,
    #[serde(rename = "Cargo")]
    pub cargo: i64,
    #[serde(rename = "CargoSpaceReserved")]
    pub cargo_space_reserved: i64,
    #[serde(rename = "ShipPacks")]
    pub ship_packs: i64,
    #[serde(rename = "ModulePacks")]
    pub module_packs: i64,
    #[serde(rename = "FreeSpace")]
    pub free_space: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierFinanceInfo {
    #[serde(rename = "CarrierBalance")]
    pub carrier_balance: i64,
    #[serde(rename = "ReserveBalance")]
    pub reserve_balance: i64,
    #[serde(rename = "AvailableBalance")]
    pub available_balance: i64,
    #[serde(rename = "ReservePercent")]
    pub reserve_percent: Option<f64>,
    #[serde(rename = "TaxRate_pioneersupplies")]
    pub tax_rate_pioneersupplies: Option<f64>,
    #[serde(rename = "TaxRate_shipyard")]
    pub tax_rate_shipyard: Option<f64>,
    #[serde(rename = "TaxRate_rearm")]
    pub tax_rate_rearm: Option<f64>,
    #[serde(rename = "TaxRate_outfitting")]
    pub tax_rate_outfitting: Option<f64>,
    #[serde(rename = "TaxRate_refuel")]
    pub tax_rate_refuel: Option<f64>,
    #[serde(rename = "TaxRate_repair")]
    pub tax_rate_repair: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierCrewMember {
    #[serde(rename = "CrewRole")]
    pub crew_role: String,
    #[serde(rename = "Activated")]
    pub activated: bool,
    #[serde(rename = "Enabled")]
    pub enabled: Option<bool>,
    #[serde(rename = "CrewName")]
    pub crew_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierStats {
    #[serde(rename = "CarrierID")]
    pub carrier_id: u64,
    #[serde(rename = "CarrierType")]
    pub carrier_type: Option<String>,
    #[serde(rename = "Callsign")]
    pub callsign: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "DockingAccess")]
    pub docking_access: String,
    #[serde(rename = "AllowNotorious")]
    pub allow_notorious: bool,
    #[serde(rename = "FuelLevel")]
    pub fuel_level: i64,
    #[serde(rename = "JumpRangeCurr")]
    pub jump_range_curr: f64,
    #[serde(rename = "JumpRangeMax")]
    pub jump_range_max: f64,
    #[serde(rename = "PendingDecommission")]
    pub pending_decommission: bool,
    #[serde(rename = "SpaceUsage")]
    pub space_usage: CarrierSpaceUsage,
    #[serde(rename = "Finance")]
    pub finance: CarrierFinanceInfo,
    #[serde(rename = "Crew")]
    pub crew: Vec<CarrierCrewMember>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierJumpRequest {
    #[serde(rename = "CarrierType")]
    pub carrier_type: Option<String>,
    #[serde(rename = "CarrierID")]
    pub carrier_id: u64,
    #[serde(rename = "SystemName")]
    pub system_name: String,
    #[serde(rename = "Body")]
    pub body: Option<String>,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "BodyID")]
    pub body_id: Option<u32>,
    #[serde(rename = "DepartureTime")]
    pub departure_time: Option<String>,
}

/// `CarrierLocation` (startup), and the simple `CarrierJumpCancelled` and `CarrierCancelDecommission`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierLocation {
    #[serde(rename = "CarrierType")]
    pub carrier_type: Option<String>,
    #[serde(rename = "CarrierID")]
    pub carrier_id: u64,
    #[serde(rename = "StarSystem")]
    pub star_system: Option<String>,
    #[serde(rename = "SystemAddress")]
    pub system_address: Option<u64>,
    #[serde(rename = "BodyID")]
    pub body_id: Option<u32>,
}

/// The owner asked to decommission the carrier: it's scrapped at `ScrapTime` (Unix seconds) unless
/// they cancel before (`CarrierCancelDecommission`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierDecommission {
    #[serde(rename = "CarrierID")]
    pub carrier_id: u64,
    #[serde(rename = "CarrierType")]
    pub carrier_type: Option<String>,
    #[serde(rename = "ScrapRefund")]
    pub scrap_refund: i64,
    #[serde(rename = "ScrapTime")]
    pub scrap_time: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierFinance {
    #[serde(rename = "CarrierID")]
    pub carrier_id: u64,
    #[serde(rename = "CarrierType")]
    pub carrier_type: Option<String>,
    #[serde(rename = "TaxRate")]
    pub tax_rate: Option<f64>,
    #[serde(rename = "CarrierBalance")]
    pub carrier_balance: i64,
    #[serde(rename = "ReserveBalance")]
    pub reserve_balance: i64,
    #[serde(rename = "AvailableBalance")]
    pub available_balance: i64,
    #[serde(rename = "ReservePercent")]
    pub reserve_percent: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierBankTransfer {
    #[serde(rename = "CarrierID")]
    pub carrier_id: u64,
    #[serde(rename = "CarrierType")]
    pub carrier_type: Option<String>,
    #[serde(rename = "Deposit")]
    pub deposit: Option<i64>,
    #[serde(rename = "Withdraw")]
    pub withdraw: Option<i64>,
    #[serde(rename = "PlayerBalance")]
    pub player_balance: i64,
    #[serde(rename = "CarrierBalance")]
    pub carrier_balance: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierDepositFuel {
    #[serde(rename = "CarrierID")]
    pub carrier_id: u64,
    #[serde(rename = "CarrierType")]
    pub carrier_type: Option<String>,
    #[serde(rename = "Amount")]
    pub amount: i64,
    #[serde(rename = "Total")]
    pub total: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierTradeOrder {
    #[serde(rename = "CarrierID")]
    pub carrier_id: u64,
    #[serde(rename = "CarrierType")]
    pub carrier_type: Option<String>,
    #[serde(rename = "BlackMarket")]
    pub black_market: bool,
    #[serde(rename = "Commodity")]
    pub commodity: String,
    #[serde(rename = "Commodity_Localised")]
    pub commodity_localised: Option<String>,
    #[serde(rename = "PurchaseOrder")]
    pub purchase_order: Option<i64>,
    #[serde(rename = "SaleOrder")]
    pub sale_order: Option<i64>,
    #[serde(rename = "CancelTrade")]
    pub cancel_trade: Option<bool>,
    #[serde(rename = "Price")]
    pub price: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierNameChange {
    #[serde(rename = "CarrierID")]
    pub carrier_id: u64,
    #[serde(rename = "CarrierType")]
    pub carrier_type: Option<String>,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Callsign")]
    pub callsign: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierCrewServices {
    #[serde(rename = "CarrierID")]
    pub carrier_id: u64,
    #[serde(rename = "CarrierType")]
    pub carrier_type: Option<String>,
    #[serde(rename = "CrewRole")]
    pub crew_role: String,
    #[serde(rename = "Operation")]
    pub operation: String,
    #[serde(rename = "CrewName")]
    pub crew_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CarrierBuy {
    #[serde(rename = "CarrierID")]
    pub carrier_id: u64,
    #[serde(rename = "CarrierType")]
    pub carrier_type: Option<String>,
    #[serde(rename = "BoughtAtMarket")]
    pub bought_at_market: u64,
    #[serde(rename = "Location")]
    pub location: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "Price")]
    pub price: i64,
    #[serde(rename = "Variant")]
    pub variant: String,
    #[serde(rename = "Callsign")]
    pub callsign: String,
}

// ---------------------------------------------------------------------------------------------
// Colonisation (Trailblazers, 2025+)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ConstructionResource {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "RequiredAmount")]
    pub required_amount: i64,
    #[serde(rename = "ProvidedAmount")]
    pub provided_amount: i64,
    #[serde(rename = "Payment")]
    pub payment: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ColonisationConstructionDepot {
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "ConstructionProgress")]
    pub construction_progress: f64,
    #[serde(rename = "ConstructionComplete")]
    pub construction_complete: bool,
    #[serde(rename = "ConstructionFailed")]
    pub construction_failed: bool,
    #[serde(rename = "ResourcesRequired")]
    pub resources_required: Vec<ConstructionResource>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Contribution {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Name_Localised")]
    pub name_localised: Option<String>,
    #[serde(rename = "Amount")]
    pub amount: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ColonisationContribution {
    #[serde(rename = "MarketID")]
    pub market_id: u64,
    #[serde(rename = "Contributions")]
    pub contributions: Vec<Contribution>,
}

/// `ColonisationSystemClaim` / `ColonisationSystemClaimRelease` / `ColonisationBeaconDeployed`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ColonisationSystem {
    #[serde(rename = "StarSystem")]
    pub star_system: String,
    #[serde(rename = "SystemAddress")]
    pub system_address: u64,
    #[serde(rename = "BodyID")]
    pub body_id: Option<u32>,
}

// ---------------------------------------------------------------------------------------------
// Comms
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ReceiveText {
    #[serde(rename = "From")]
    pub from: String,
    #[serde(rename = "From_Localised")]
    pub from_localised: Option<String>,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "Message_Localised")]
    pub message_localised: Option<String>,
    #[serde(rename = "Channel")]
    pub channel: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Friends {
    #[serde(rename = "Status")]
    pub status: String,
    #[serde(rename = "Name")]
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Statistics {
    #[serde(rename = "Bank_Account")]
    pub bank_account: serde_json::Value,
    #[serde(rename = "Combat")]
    pub combat: serde_json::Value,
    #[serde(rename = "Crime")]
    pub crime: serde_json::Value,
    #[serde(rename = "Smuggling")]
    pub smuggling: serde_json::Value,
    #[serde(rename = "Trading")]
    pub trading: serde_json::Value,
    #[serde(rename = "Mining")]
    pub mining: serde_json::Value,
    #[serde(rename = "Exploration")]
    pub exploration: serde_json::Value,
    #[serde(rename = "Passengers")]
    pub passengers: serde_json::Value,
    #[serde(rename = "Search_And_Rescue")]
    pub search_and_rescue: serde_json::Value,
    #[serde(rename = "Crafting")]
    pub crafting: serde_json::Value,
    #[serde(rename = "Crew")]
    pub crew: serde_json::Value,
    #[serde(rename = "Multicrew")]
    pub multicrew: serde_json::Value,
    #[serde(rename = "Material_Trader_Stats")]
    pub material_trader_stats: serde_json::Value,
    #[serde(rename = "Exobiology")]
    pub exobiology: serde_json::Value,
    #[serde(rename = "Thargoid_Encounter_Stats")]
    pub thargoid_encounter_stats: serde_json::Value,
    #[serde(rename = "TG_ENCOUNTERS")]
    pub tg_encounters: serde_json::Value,
    #[serde(rename = "CQC")]
    pub cqc: serde_json::Value,
    #[serde(rename = "FLEETCARRIER")]
    pub fleetcarrier: serde_json::Value,
}

/// Events with no payload that still matter to the model.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Empty {}

// ---------------------------------------------------------------------------------------------
// The event enum
// ---------------------------------------------------------------------------------------------

macro_rules! journal_events {
    ($($variant:ident => $ty:ty),* $(,)?) => {
        /// A parsed journal event. Event names Elite Nexus does not model are [`Event::Other`].
        #[derive(Debug, Clone)]
        pub enum Event {
            $($variant(Box<$ty>),)*
            Other,
        }

        impl Event {
            /// Parses `line` as the event called `name`. Unmodelled names return `Event::Other`
            /// without touching the JSON.
            pub fn parse(name: &str, line: &str) -> serde_json::Result<Event> {
                Ok(match name {
                    $(stringify!($variant) => Event::$variant(Box::new(serde_json::from_str::<$ty>(line)?)),)*
                    _ => Event::Other,
                })
            }

            /// True if `name` is an event this crate parses into a typed variant.
            pub fn is_modelled(name: &str) -> bool {
                matches!(name, $(stringify!($variant))|*)
            }

            /// The journal event name of a typed event (`"Other"` for unmodelled events).
            pub fn name(&self) -> &'static str {
                match self {
                    $(Event::$variant(_) => stringify!($variant),)*
                    Event::Other => "Other",
                }
            }
        }
    };
}

journal_events! {
    // session
    Fileheader => Fileheader,
    Commander => Commander,
    LoadGame => LoadGame,
    NewCommander => Commander,
    Continued => Continued,
    Shutdown => Empty,
    Music => Music,
    // ranks
    Rank => RankSet,
    Progress => RankSet,
    Promotion => RankSet,
    Reputation => Reputation,
    EngineerProgress => EngineerProgress,
    Powerplay => Powerplay,
    PowerplayJoin => PowerplayChange,
    PowerplayLeave => PowerplayChange,
    PowerplayDefect => PowerplayChange,
    PowerplayRank => PowerplayChange,
    PowerplayMerits => PowerplayMerits,
    SquadronStartup => SquadronStartup,
    AppliedToSquadron => SquadronEvent,
    InvitedToSquadron => SquadronEvent,
    JoinedSquadron => SquadronEvent,
    LeftSquadron => SquadronEvent,
    SquadronCreated => SquadronEvent,
    SquadronPromotion => SquadronEvent,
    SquadronDemotion => SquadronEvent,
    KickedFromSquadron => SquadronEvent,
    DisbandedSquadron => SquadronEvent,
    // travel
    Location => SystemArrival,
    FSDJump => SystemArrival,
    CarrierJump => SystemArrival,
    StartJump => StartJump,
    FSDTarget => FSDTarget,
    SupercruiseEntry => Supercruise,
    SupercruiseExit => Supercruise,
    SupercruiseDestinationDrop => SupercruiseDestinationDrop,
    Docked => Docked,
    Undocked => Undocked,
    DockingRequested => Docking,
    DockingGranted => Docking,
    DockingDenied => Docking,
    DockingCancelled => Docking,
    DockingTimeout => Docking,
    ApproachBody => BodyApproach,
    LeaveBody => BodyApproach,
    Touchdown => Landing,
    Liftoff => Landing,
    ApproachSettlement => ApproachSettlement,
    Embark => EmbarkDisembark,
    Disembark => EmbarkDisembark,
    LaunchSRV => VehicleEvent,
    DockSRV => VehicleEvent,
    SRVDestroyed => VehicleEvent,
    LaunchFighter => VehicleEvent,
    DockFighter => VehicleEvent,
    FighterDestroyed => VehicleEvent,
    NavRoute => Empty,
    NavRouteClear => Empty,
    FuelScoop => FuelScoop,
    RefuelAll => Refuel,
    RefuelPartial => Refuel,
    ReservoirReplenished => ReservoirReplenished,
    JetConeBoost => JetConeBoost,
    Died => Died,
    Resurrect => Resurrect,
    Interdicted => InterdictionEvent,
    Interdiction => InterdictionEvent,
    EscapeInterdiction => InterdictionEvent,
    HullDamage => HullDamage,
    BookTaxi => BookTaxi,
    BookDropship => BookTaxi,
    // ship & fleet
    Loadout => Loadout,
    ShipyardBuy => ShipyardBuy,
    ShipyardNew => ShipyardNew,
    ShipyardSell => ShipyardSell,
    ShipyardSwap => ShipyardSwap,
    ShipyardTransfer => ShipyardTransfer,
    ShipyardRedeem => ShipRedeemed,
    ShipRedeemed => ShipRedeemed,
    StoredShips => StoredShips,
    SetUserShipName => SetUserShipName,
    StoredModules => StoredModules,
    ModuleBuy => ModuleChange,
    ModuleSell => ModuleChange,
    ModuleSellRemote => ModuleChange,
    ModuleStore => ModuleChange,
    ModuleRetrieve => ModuleChange,
    ModuleSwap => ModuleChange,
    ModuleBuyAndStore => ModuleChange,
    Repair => CreditFlow,
    RepairAll => CreditFlow,
    BuyAmmo => CreditFlow,
    BuyDrones => CreditFlow,
    SellDrones => CreditFlow,
    RestockVehicle => CreditFlow,
    PayFines => CreditFlow,
    PayBounties => CreditFlow,
    // exploration
    Scan => Scan,
    FSSDiscoveryScan => FSSDiscoveryScan,
    FSSAllBodiesFound => FSSAllBodiesFound,
    FSSBodySignals => BodySignals,
    SAASignalsFound => BodySignals,
    SAAScanComplete => SAAScanComplete,
    FSSSignalDiscovered => FSSSignalDiscovered,
    ScanBaryCentre => ScanBaryCentre,
    NavBeaconScan => NavBeaconScan,
    CodexEntry => CodexEntry,
    ScanOrganic => ScanOrganic,
    SellOrganicData => SellOrganicData,
    SellExplorationData => SellExplorationData,
    MultiSellExplorationData => MultiSellExplorationData,
    // trade & cargo
    Market => MarketEvent,
    MarketBuy => MarketBuy,
    MarketSell => MarketSell,
    Cargo => Cargo,
    CargoTransfer => CargoTransfer,
    CollectCargo => CargoMove,
    EjectCargo => CargoMove,
    CargoDepot => CargoDepot,
    // mining
    ProspectedAsteroid => ProspectedAsteroid,
    MiningRefined => MiningRefined,
    AsteroidCracked => Empty,
    LaunchDrone => LaunchDrone,
    // materials & engineering
    Materials => Materials,
    MaterialCollected => MaterialChange,
    MaterialDiscarded => MaterialChange,
    MaterialDiscovered => MaterialChange,
    MaterialTrade => MaterialTrade,
    EngineerCraft => EngineerCraft,
    EngineerContribution => EngineerContribution,
    Synthesis => Synthesis,
    TechnologyBroker => TechnologyBroker,
    ScientificResearch => ScientificResearch,
    // odyssey
    ShipLocker => MicroResourceInventory,
    Backpack => MicroResourceInventory,
    BackpackChange => BackpackChange,
    BuyMicroResources => MicroResourceFlow,
    SellMicroResources => MicroResourceFlow,
    TradeMicroResources => MicroResourceFlow,
    TransferMicroResources => MicroResourceFlow,
    CollectItems => MicroResourceFlow,
    DropItems => MicroResourceFlow,
    UseConsumable => MicroResourceFlow,
    SuitLoadout => SuitLoadout,
    SwitchSuitLoadout => SuitLoadout,
    CreateSuitLoadout => SuitLoadout,
    UpgradeSuit => UpgradeGear,
    UpgradeWeapon => UpgradeGear,
    BuySuit => GearPurchase,
    SellSuit => GearPurchase,
    BuyWeapon => GearPurchase,
    SellWeapon => GearPurchase,
    // missions, combat
    Missions => Missions,
    MissionAccepted => MissionAccepted,
    MissionCompleted => MissionCompleted,
    MissionFailed => MissionEnded,
    MissionAbandoned => MissionEnded,
    MissionRedirected => MissionRedirected,
    Bounty => Bounty,
    FactionKillBond => KillBond,
    CapShipBond => KillBond,
    RedeemVoucher => RedeemVoucher,
    CommitCrime => CommitCrime,
    ShipTargeted => ShipTargeted,
    // fleet carriers
    CarrierStats => CarrierStats,
    CarrierJumpRequest => CarrierJumpRequest,
    CarrierJumpCancelled => CarrierLocation,
    CarrierLocation => CarrierLocation,
    CarrierFinance => CarrierFinance,
    CarrierBankTransfer => CarrierBankTransfer,
    CarrierDepositFuel => CarrierDepositFuel,
    CarrierTradeOrder => CarrierTradeOrder,
    CarrierNameChange => CarrierNameChange,
    CarrierBuy => CarrierBuy,
    CarrierCrewServices => CarrierCrewServices,
    CarrierDecommission => CarrierDecommission,
    CarrierCancelDecommission => CarrierLocation,
    // colonisation
    ColonisationConstructionDepot => ColonisationConstructionDepot,
    ColonisationContribution => ColonisationContribution,
    ColonisationSystemClaim => ColonisationSystem,
    ColonisationSystemClaimRelease => ColonisationSystem,
    ColonisationBeaconDeployed => ColonisationSystem,
    // comms & stats
    ReceiveText => ReceiveText,
    Friends => Friends,
    Statistics => Statistics,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fsdjump_union() {
        let line = r#"{ "timestamp":"2026-09-05T01:06:02Z", "event":"FSDJump", "Taxi":false, "StarSystem":"Synuefe BR-L c24-23", "SystemAddress":6406446944978, "StarPos":[220.96875,-124.81250,-12.59375], "Population":107996107, "JumpDist":9.851, "FuelUsed":0.484949, "FuelLevel":7.515051, "Factions":[ { "Name":"Sacra Oculus", "FactionState":"None", "Government":"Theocracy", "Influence":0.162978, "Allegiance":"Independent", "Happiness":"$Faction_HappinessBand2;", "MyReputation":0.000000, "RecoveringStates":[ { "State":"Expansion", "Trend":0 } ] } ] }"#;
        let Event::FSDJump(j) = Event::parse("FSDJump", line).unwrap() else { panic!() };
        assert_eq!(j.star_system, "Synuefe BR-L c24-23");
        assert_eq!(j.star_pos.unwrap()[0], 220.96875);
        assert_eq!(j.factions[0].recovering_states[0].state, "Expansion");
        assert_eq!(j.jump_dist, Some(9.851));
    }

    #[test]
    fn unknown_is_other_without_parsing() {
        assert!(matches!(Event::parse("Screenshot", "not even json").unwrap(), Event::Other));
        assert!(Event::is_modelled("Scan"));
        assert!(!Event::is_modelled("Screenshot"));
    }

    #[test]
    fn music_track() {
        let l = r#"{ "timestamp":"2026-10-25T09:00:00Z", "event":"Music", "MusicTrack":"MainMenu" }"#;
        let Event::Music(m) = Event::parse("Music", l).unwrap() else { panic!() };
        assert_eq!(m.music_track, "MainMenu");
    }

    #[test]
    fn scan_organic_and_signals() {
        let l = r#"{ "timestamp":"2026-09-28T03:04:04Z", "event":"SAASignalsFound", "BodyName":"Swoilz MH-W c17-3 A 4", "SystemAddress":908552967066, "BodyID":6, "Signals":[ { "Type":"$SAA_SignalType_Biological;", "Type_Localised":"Biological", "Count":4 } ], "Genuses":[ { "Genus":"$Codex_Ent_Bacterial_Genus_Name;", "Genus_Localised":"Bacterium" } ] }"#;
        let Event::SAASignalsFound(s) = Event::parse("SAASignalsFound", l).unwrap() else { panic!() };
        assert_eq!(s.signals[0].count, 4);
        assert_eq!(s.genuses[0].genus_localised.as_deref(), Some("Bacterium"));
    }
}
