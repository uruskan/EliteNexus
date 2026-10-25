//! Where the commander is: system, body, station, and what they are sitting in.

use nexus_data::names::{clean_station_name, is_colonisation_ship, localised_or};
use nexus_journal::companion::{flags, flags2, Status, StatusDestination};
use nexus_journal::events as ev;
use nexus_journal::Timestamp;

#[derive(Debug, Clone, Default)]
pub struct FactionSummary {
    pub name: String,
    pub state: String,
    pub government: String,
    pub allegiance: String,
    pub influence: f64,
    pub happiness: String,
    pub my_reputation: Option<f64>,
    pub active_states: Vec<String>,
    pub pending_states: Vec<String>,
    pub recovering_states: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SystemInfo {
    pub allegiance: String,
    pub economy: String,
    pub second_economy: String,
    pub government: String,
    pub security: String,
    pub population: u64,
    pub controlling_faction: Option<String>,
    pub controlling_faction_state: Option<String>,
    pub factions: Vec<FactionSummary>,
    pub powers: Vec<String>,
    pub controlling_power: Option<String>,
    pub powerplay_state: Option<String>,
    pub conflicts: Vec<ev::Conflict>,
    pub updated: Timestamp,
}

impl SystemInfo {
    pub fn from_arrival(a: &ev::SystemArrival, ts: Timestamp) -> Self {
        SystemInfo {
            allegiance: a.system_allegiance.clone(),
            economy: localised_or(&a.system_economy, a.system_economy_localised.as_deref()),
            second_economy: localised_or(&a.system_second_economy, a.system_second_economy_localised.as_deref()),
            government: localised_or(&a.system_government, a.system_government_localised.as_deref()),
            security: localised_or(&a.system_security, a.system_security_localised.as_deref()),
            population: a.population,
            controlling_faction: a.system_faction.as_ref().map(|f| f.name.clone()),
            controlling_faction_state: a.system_faction.as_ref().and_then(|f| f.faction_state.clone()),
            factions: a
                .factions
                .iter()
                .map(|f| FactionSummary {
                    name: f.name.clone(),
                    state: f.faction_state.clone(),
                    government: f.government.clone(),
                    allegiance: f.allegiance.clone(),
                    influence: f.influence,
                    happiness: localised_or(&f.happiness, f.happiness_localised.as_deref()),
                    my_reputation: f.my_reputation,
                    active_states: f.active_states.iter().map(|s| s.state.clone()).collect(),
                    pending_states: f.pending_states.iter().map(|s| s.state.clone()).collect(),
                    recovering_states: f.recovering_states.iter().map(|s| s.state.clone()).collect(),
                })
                .collect(),
            powers: a.powers.clone(),
            controlling_power: a.controlling_power.clone(),
            powerplay_state: a.powerplay_state.clone(),
            conflicts: a.conflicts.clone(),
            updated: ts,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StationInfo {
    /// Display name: `$EXT_PANEL_ColonisationShip; Hind's Pride` is kept as `Hind's Pride`.
    pub name: String,
    /// The name exactly as the journal wrote it, which uploads send.
    pub journal_name: String,
    pub market_id: u64,
    pub station_type: String,
    pub faction: Option<String>,
    pub government: Option<String>,
    pub allegiance: Option<String>,
    pub economy: Option<String>,
    pub services: Vec<String>,
    pub economies: Vec<(String, f64)>,
    pub dist_ls: Option<f64>,
    pub pads: Option<ev::LandingPads>,
    pub system: String,
    pub system_address: u64,
}

impl StationInfo {
    pub fn is_fleet_carrier(&self) -> bool {
        self.station_type == "FleetCarrier"
    }
    /// A System Colonisation Ship (the game says so only through the name's symbol).
    pub fn is_colonisation_ship(&self) -> bool {
        is_colonisation_ship(&self.journal_name)
    }
    pub fn has_service(&self, s: &str) -> bool {
        self.services.iter().any(|x| x.eq_ignore_ascii_case(s))
    }
}

#[derive(Debug, Clone, Default)]
pub struct FsdTarget {
    pub name: String,
    pub system_address: u64,
    pub star_class: String,
    pub remaining_jumps: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Location {
    pub system: String,
    pub system_address: u64,
    pub star_pos: Option<[f64; 3]>,
    pub info: SystemInfo,
    pub body: Option<String>,
    pub body_id: Option<u32>,
    pub body_type: Option<String>,
    pub station: Option<StationInfo>,
    pub settlement: Option<String>,
    /// Where the settlement approached last is (`ApproachSettlement`), on the body the commander is at.
    pub settlement_pos: Option<(f64, f64)>,
    pub docked: bool,
    pub landed: bool,
    pub on_foot: bool,
    pub in_srv: bool,
    /// The SRV launched last (`LaunchSRV`'s `SRVType`: `testbuggy`, `mev_rhino`, `lander01`, …), until it docks.
    pub srv_type: Option<String>,
    pub in_fighter: bool,
    pub in_taxi: bool,
    pub multicrew: bool,
    pub supercruise: bool,
    pub hyperspace: bool,
    pub lat_lon: Option<(f64, f64)>,
    pub fsd_target: Option<FsdTarget>,
    pub destination: Option<StatusDestination>,
    pub last_arrival: Timestamp,
}

impl Location {
    /// Location / FSDJump / CarrierJump.
    pub fn on_arrival(&mut self, a: &ev::SystemArrival, ts: Timestamp, kind: ArrivalKind) {
        let new_system = a.system_address != self.system_address;
        self.system = a.star_system.clone();
        self.system_address = a.system_address;
        if a.star_pos.is_some() {
            self.star_pos = a.star_pos;
        }
        self.info = SystemInfo::from_arrival(a, ts);
        self.body = a.body.clone();
        self.body_id = a.body_id;
        self.body_type = a.body_type.clone();
        self.hyperspace = false;
        self.last_arrival = ts;
        if let Some(t) = a.taxi {
            self.in_taxi = t;
        }
        if let Some(m) = a.multicrew {
            self.multicrew = m;
        }
        match kind {
            ArrivalKind::Jump => {
                self.docked = false;
                self.landed = false;
                self.station = None;
                self.settlement = None;
                self.settlement_pos = None;
                self.supercruise = true;
                if self.fsd_target.as_ref().is_some_and(|t| t.system_address == a.system_address) {
                    self.fsd_target = None;
                }
            }
            ArrivalKind::Location | ArrivalKind::CarrierJump => {
                self.docked = a.docked.unwrap_or(false);
                self.on_foot = a.on_foot.unwrap_or(self.on_foot && !new_system);
                self.in_srv = a.in_srv.unwrap_or(false);
                self.supercruise = false;
                if self.docked {
                    self.station = Some(StationInfo {
                        name: clean_station_name(a.station_name.as_deref().unwrap_or_default()),
                        journal_name: a.station_name.clone().unwrap_or_default(),
                        market_id: a.market_id.unwrap_or_default(),
                        station_type: a.station_type.clone().unwrap_or_default(),
                        faction: a.station_faction.as_ref().map(|f| f.name.clone()),
                        government: a.station_government.clone(),
                        allegiance: a.station_allegiance.clone(),
                        economy: a.station_economy.clone(),
                        services: a.station_services.clone(),
                        economies: a
                            .station_economies
                            .iter()
                            .map(|e| (localised_or(&e.name, e.name_localised.as_deref()), e.proportion))
                            .collect(),
                        dist_ls: a.dist_from_star_ls,
                        pads: None,
                        system: a.star_system.clone(),
                        system_address: a.system_address,
                    });
                } else if new_system {
                    self.station = None;
                }
                if let (Some(lat), Some(lon)) = (a.latitude, a.longitude) {
                    self.lat_lon = Some((lat, lon));
                }
            }
        }
    }

    pub fn on_docked(&mut self, d: &ev::Docked) {
        self.docked = true;
        self.landed = false;
        self.supercruise = false;
        self.system = if d.star_system.is_empty() { self.system.clone() } else { d.star_system.clone() };
        if d.system_address != 0 {
            self.system_address = d.system_address;
        }
        self.station = Some(StationInfo {
            name: clean_station_name(&d.station_name),
            journal_name: d.station_name.clone(),
            market_id: d.market_id,
            station_type: d.station_type.clone(),
            faction: d.station_faction.as_ref().map(|f| f.name.clone()),
            government: d.station_government_localised.clone().or_else(|| d.station_government.clone()),
            allegiance: d.station_allegiance.clone(),
            economy: d.station_economy_localised.clone().or_else(|| d.station_economy.clone()),
            services: d.station_services.clone(),
            economies: d
                .station_economies
                .iter()
                .map(|e| (localised_or(&e.name, e.name_localised.as_deref()), e.proportion))
                .collect(),
            dist_ls: d.dist_from_star_ls,
            pads: d.landing_pads,
            system: self.system.clone(),
            system_address: self.system_address,
        });
    }

    pub fn on_undocked(&mut self) {
        self.docked = false;
    }

    pub fn on_supercruise_entry(&mut self) {
        self.supercruise = true;
        self.docked = false;
        self.landed = false;
        self.body = None;
        self.body_id = None;
        self.body_type = None;
        self.settlement = None;
        self.settlement_pos = None;
    }

    pub fn on_supercruise_exit(&mut self, s: &ev::Supercruise) {
        self.supercruise = false;
        if s.body.is_some() {
            self.body = s.body.clone();
            self.body_id = s.body_id;
            self.body_type = s.body_type.clone();
        }
    }

    /// Refines flags from Status.json (the most authoritative live source).
    pub fn on_status(&mut self, st: &Status) {
        if !st.is_in_game() {
            return;
        }
        self.docked = st.has(flags::DOCKED);
        self.landed = st.has(flags::LANDED);
        self.supercruise = st.has(flags::SUPERCRUISE);
        self.in_srv = st.has(flags::IN_SRV);
        self.in_fighter = st.has(flags::IN_FIGHTER);
        self.on_foot = st.has2(flags2::ON_FOOT);
        self.in_taxi = st.has2(flags2::IN_TAXI);
        self.multicrew = st.has2(flags2::IN_MULTICREW);
        self.lat_lon = st.lat_lon();
        self.destination = st.destination.clone();
        if st.has(flags::FSD_JUMP) {
            self.hyperspace = true;
        }
    }

    pub fn vehicle_label(&self) -> &'static str {
        if self.on_foot {
            "On foot"
        } else if self.in_srv {
            "SRV"
        } else if self.in_fighter {
            "Fighter"
        } else if self.in_taxi {
            "Taxi"
        } else if self.multicrew {
            "Multicrew"
        } else {
            "Ship"
        }
    }

    /// One-line description of the current situation for headers and speech.
    pub fn situation(&self) -> String {
        if self.hyperspace {
            return "In hyperspace".into();
        }
        if let Some(st) = &self.station
            && self.docked {
                return format!("Docked at {}", st.name);
            }
        if self.landed {
            return format!("Landed on {}", self.body.as_deref().unwrap_or("a body"));
        }
        if self.supercruise {
            return "Supercruise".into();
        }
        match &self.body {
            Some(b) => format!("Near {b}"),
            None => "Normal space".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrivalKind {
    Location,
    Jump,
    CarrierJump,
}

/// Straight-line distance in light years.
pub fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Great-circle distance in metres between two surface points (degrees) on a body of `radius_m`.
pub fn surface_distance(a: (f64, f64), b: (f64, f64), radius_m: f64) -> f64 {
    let (lat1, lon1) = (a.0.to_radians(), a.1.to_radians());
    let (lat2, lon2) = (b.0.to_radians(), b.1.to_radians());
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * radius_m * h.sqrt().min(1.0).asin()
}

/// Compass bearing in degrees (0 = north, 90 = east) to set off from `a` towards `b` (both in degrees), as the
/// game's heading counts it.
pub fn surface_bearing(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lat2) = (a.0.to_radians(), b.0.to_radians());
    let dlon = (b.1 - a.1).to_radians();
    let y = dlon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine() {
        // A quarter of the way around a 1000 km body along the equator.
        let d = surface_distance((0.0, 0.0), (0.0, 90.0), 1_000_000.0);
        assert!((d - std::f64::consts::FRAC_PI_2 * 1_000_000.0).abs() < 1.0);
        assert!(surface_distance((10.0, 20.0), (10.0, 20.0), 5e6) < 1e-6);
    }

    #[test]
    fn bearings() {
        let b = |to: (f64, f64)| surface_bearing((0.0, 0.0), to).round();
        assert_eq!(b((1.0, 0.0)), 0.0, "north");
        assert_eq!(b((0.0, 1.0)), 90.0, "east");
        assert_eq!(b((-1.0, 0.0)), 180.0, "south");
        assert_eq!(b((0.0, -1.0)), 270.0, "west");
    }

    #[test]
    fn ly_distance() {
        assert_eq!(distance([0.0, 0.0, 0.0], [3.0, 4.0, 0.0]), 5.0);
    }
}
