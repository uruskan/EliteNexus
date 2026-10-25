//! What the commander is doing right now, read from Status.json and the journal: one answer for everything that
//! follows the game, such as the overlay or switching pages with it. Pure, like the rest of the model.
//!
//! Status.json is the live source: the game rewrites it whenever the cockpit changes. Without it (a journal read
//! back from history, or a game that wrote none yet) the journal's own events decide. A journal that isn't being
//! followed live means the game isn't running either; only the app knows that, so it says so itself.

use crate::location::Location;
use crate::state::GameState;
use nexus_journal::companion::{flags, flags2, GuiFocus, Status};

/// The vehicles that set Status.json's InSRV flag. `LaunchSRV`'s `SRVType` tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vehicle {
    /// The SRV Scarab (`testbuggy`).
    Scarab,
    /// The SRV Scorpion (`combat_multicrew_srv_01`).
    Scorpion,
    /// The Rhino mining vehicle (`mev_rhino`).
    Rhino,
    /// The Nomad lander (`lander01`).
    Nomad,
    /// Another type, or one launched before Elite Nexus was reading the journal.
    Other,
}

impl Vehicle {
    /// From `LaunchSRV`'s `SRVType`.
    pub fn from_srv_type(srv_type: Option<&str>) -> Vehicle {
        match srv_type.map(str::to_ascii_lowercase).as_deref() {
            Some("testbuggy") => Vehicle::Scarab,
            Some("combat_multicrew_srv_01") => Vehicle::Scorpion,
            Some("mev_rhino") => Vehicle::Rhino,
            Some("lander01") => Vehicle::Nomad,
            _ => Vehicle::Other,
        }
    }
}

/// What the commander is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// At the main menu (Status.json's flags are 0, or the main menu's music plays), or the game is closed.
    NotInGame,
    /// Docked, or on foot in a station, its hangar or its social space.
    Docked,
    /// In supercruise away from any body.
    Supercruise,
    /// Charging a hyperspace jump (not a supercruise one).
    ChargingJump,
    /// In the hyperspace tunnel, from `StartJump` until the arrival.
    Hyperspace,
    /// Near a body: in orbital cruise or flying low over it (Status.json gives a latitude and longitude).
    NearBody,
    /// Gliding down to a body after dropping out of orbital cruise.
    Glide,
    /// The ship has landed on a body.
    Landed,
    /// Driving an SRV, the Rhino or the Nomad.
    Srv(Vehicle),
    /// On foot on a planet.
    OnFootPlanet,
    /// Anywhere else in normal space.
    Space,
}

/// The body the commander is at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRef {
    pub name: String,
    /// Its id in the system, when the journal named the same body.
    pub id: Option<u32>,
    pub system_address: u64,
}

/// What the commander is doing, where the game's attention is, and whether it's dangerous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub activity: Activity,
    /// Which screen of the game has the commander's attention (Status.json's GuiFocus; None without it).
    pub focus: GuiFocus,
    /// In danger or being interdicted.
    pub danger: bool,
    /// The body the commander is near, landed on, driving or walking on.
    pub body: Option<BodyRef>,
}

impl Default for Context {
    fn default() -> Self {
        Context { activity: Activity::NotInGame, focus: GuiFocus::None, danger: false, body: None }
    }
}

impl Context {
    /// The commander's context from the model.
    pub fn of(gs: &GameState) -> Context {
        if gs.game_closed || gs.at_main_menu {
            return Context::default();
        }
        let loc = &gs.location;
        let vehicle = Vehicle::from_srv_type(loc.srv_type.as_deref());
        let (activity, focus, danger) = match gs.status.as_deref() {
            Some(st) => (from_status(st, loc.hyperspace, vehicle), st.gui_focus(), st.has(flags::IS_IN_DANGER) || st.has(flags::BEING_INTERDICTED)),
            None => (from_journal(loc, vehicle), GuiFocus::None, false),
        };
        if activity == Activity::NotInGame {
            return Context::default();
        }
        let at_body = matches!(activity, Activity::NearBody | Activity::Glide | Activity::Landed | Activity::Srv(_) | Activity::OnFootPlanet);
        let body = at_body
            .then(|| gs.status.as_ref().and_then(|s| s.body_name.clone()).filter(|n| !n.is_empty()).or_else(|| loc.body.clone()))
            .flatten()
            .map(|name| BodyRef { id: loc.body_id.filter(|_| loc.body.as_deref() == Some(name.as_str())), name, system_address: loc.system_address });
        Context { activity, focus, danger, body }
    }

    pub fn in_game(&self) -> bool {
        self.activity != Activity::NotInGame
    }

    /// On a body's surface: landed, driving or on foot.
    pub fn on_surface(&self) -> bool {
        matches!(self.activity, Activity::Landed | Activity::Srv(_) | Activity::OnFootPlanet)
    }

    /// A cockpit panel or the station's menus are open (GuiFocus 1 to 5).
    pub fn in_panel(&self) -> bool {
        matches!(self.focus, GuiFocus::InternalPanel | GuiFocus::ExternalPanel | GuiFocus::CommsPanel | GuiFocus::RolePanel | GuiFocus::StationServices)
    }
}

/// The activity Status.json's flags describe. `hyperspace` is the journal's say (`StartJump` until the arrival),
/// which a Status.json written just before the jump doesn't have yet.
pub fn from_status(st: &Status, hyperspace: bool, vehicle: Vehicle) -> Activity {
    if !st.is_in_game() {
        return Activity::NotInGame;
    }
    if st.has(flags::DOCKED) || st.has2(flags2::ON_FOOT_IN_STATION | flags2::ON_FOOT_IN_HANGAR | flags2::ON_FOOT_SOCIAL_SPACE) {
        return Activity::Docked;
    }
    if hyperspace || st.has(flags::FSD_JUMP) {
        return Activity::Hyperspace;
    }
    if st.has2(flags2::FSD_HYPERDRIVE_CHARGING) {
        return Activity::ChargingJump;
    }
    if st.has2(flags2::ON_FOOT_ON_PLANET) || (st.on_foot() && st.has(flags::HAS_LAT_LONG)) {
        return Activity::OnFootPlanet;
    }
    if st.on_foot() {
        // On foot, neither on a planet nor in a station's spaces: aboard a carrier or a ship's interior.
        return Activity::Docked;
    }
    if st.has(flags::IN_SRV) {
        return Activity::Srv(vehicle);
    }
    if st.has(flags::LANDED) {
        return Activity::Landed;
    }
    if st.has2(flags2::GLIDE_MODE) {
        return Activity::Glide;
    }
    match (st.has(flags::SUPERCRUISE), st.has(flags::HAS_LAT_LONG)) {
        (_, true) => Activity::NearBody,
        (true, false) => Activity::Supercruise,
        (false, false) => Activity::Space,
    }
}

/// The activity the journal alone describes, when there's no Status.json.
fn from_journal(loc: &Location, vehicle: Vehicle) -> Activity {
    if loc.docked {
        Activity::Docked
    } else if loc.hyperspace {
        Activity::Hyperspace
    } else if loc.on_foot {
        Activity::OnFootPlanet
    } else if loc.in_srv {
        Activity::Srv(vehicle)
    } else if loc.landed {
        Activity::Landed
    } else if loc.supercruise {
        Activity::Supercruise
    } else if loc.body.is_some() && loc.lat_lon.is_some() {
        Activity::NearBody
    } else {
        Activity::Space
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Alerts, Model};
    use nexus_journal::{raw::scan_line, Companion, CompanionKind, Event, EventCtx};

    const LOGIN: &[&str] = &[
        r#"{ "timestamp":"2026-10-25T10:00:01Z", "event":"Commander", "FID":"F1", "Name":"Test" }"#,
        r#"{ "timestamp":"2026-10-25T10:00:02Z", "event":"LoadGame", "FID":"F1", "Commander":"Test", "Horizons":true, "Odyssey":true, "Ship":"explorer_nx", "ShipID":22, "ShipName":"", "ShipIdent":"", "FuelLevel":64.0, "FuelCapacity":64.0, "GameMode":"Solo", "Credits":1000000, "Loan":0 }"#,
        r#"{ "timestamp":"2026-10-25T10:00:03Z", "event":"Location", "Docked":false, "StarSystem":"Alpha", "SystemAddress":100, "StarPos":[0.0,0.0,0.0], "Population":0, "Body":"Alpha", "BodyID":0, "BodyType":"Star" }"#,
    ];

    fn feed(m: &mut Model, lines: &[&str]) {
        let mut alerts = Alerts::live();
        for line in lines {
            let (ts, name) = scan_line(line).expect("journal line");
            let ev = Event::parse(name, line).expect("typed parse");
            m.apply(&EventCtx { ts, name, line, live: true }, &ev, &mut alerts);
        }
    }

    /// Status.json with these flags, as the game writes it; `extra` adds fields (`, "GuiFocus":6`).
    fn status(m: &mut Model, flags: u32, flags2: u32, extra: &str) {
        let text = format!(r#"{{ "timestamp":"2026-10-25T10:05:00Z", "event":"Status", "Flags":{flags}, "Flags2":{flags2}{extra} }}"#);
        let st = Companion::parse(CompanionKind::Status, &text).expect("Status.json");
        m.apply_companion(&st, true, &mut Alerts::live());
    }

    fn context(m: &Model) -> Context {
        Context::of(m.active().expect("commander"))
    }

    fn activity(m: &Model) -> Activity {
        context(m).activity
    }

    fn logged_in() -> Model {
        let mut m = Model::default();
        feed(&mut m, LOGIN);
        m
    }

    const SHIP: u32 = flags::IN_MAIN_SHIP | flags::SHIELDS_UP;
    const NEAR: &str = r#", "Latitude":10.0, "Longitude":20.0, "Heading":90, "Altitude":1500, "BodyName":"Alpha 1", "PlanetRadius":1000000.0"#;

    #[test]
    fn main_menu_and_a_closed_game_are_not_in_game() {
        let mut m = logged_in();
        status(&mut m, 0, 0, "");
        assert_eq!(activity(&m), Activity::NotInGame, "the main menu's Status.json has only Flags 0");
        status(&mut m, SHIP | flags::SUPERCRUISE, 0, "");
        assert_eq!(activity(&m), Activity::Supercruise);
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:06:00Z", "event":"Music", "MusicTrack":"MainMenu" }"#]);
        assert!(!context(&m).in_game(), "the main menu's music, before Status.json says so");
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:07:00Z", "event":"Music", "MusicTrack":"Supercruise" }"#]);
        assert_eq!(activity(&m), Activity::Supercruise);
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:08:00Z", "event":"Music", "MusicTrack":"MainMenu" }"#, LOGIN[1]]);
        assert_eq!(activity(&m), Activity::Supercruise, "loading a game leaves the menu");
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:09:00Z", "event":"Shutdown" }"#]);
        assert_eq!(context(&m), Context::default(), "the game closed");
    }

    #[test]
    fn docked_in_the_ship_or_on_foot_in_the_station() {
        let mut m = logged_in();
        // A real Status.json: docked, gear down, shields up, mass locked, in the main ship.
        status(&mut m, 16_842_765, 0, r#", "GuiFocus":0, "LegalState":"Clean""#);
        assert_eq!(activity(&m), Activity::Docked);
        status(&mut m, 0, flags2::ON_FOOT | flags2::ON_FOOT_IN_STATION | flags2::ON_FOOT_SOCIAL_SPACE, "");
        assert_eq!(activity(&m), Activity::Docked, "walking the concourse");
        status(&mut m, 0, flags2::ON_FOOT | flags2::ON_FOOT_IN_STATION | flags2::ON_FOOT_IN_HANGAR, "");
        assert_eq!(activity(&m), Activity::Docked, "in the hangar");
        assert!(context(&m).body.is_none());
    }

    #[test]
    fn supercruise_approach_glide_and_landing() {
        let mut m = logged_in();
        status(&mut m, SHIP | flags::SUPERCRUISE, 0, "");
        assert_eq!(activity(&m), Activity::Supercruise);
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:10:00Z", "event":"ApproachBody", "StarSystem":"Alpha", "SystemAddress":100, "Body":"Alpha 1", "BodyID":1 }"#]);
        status(&mut m, SHIP | flags::SUPERCRUISE | flags::HAS_LAT_LONG, 0, NEAR);
        let c = context(&m);
        assert_eq!(c.activity, Activity::NearBody, "orbital cruise");
        assert_eq!(c.body, Some(BodyRef { name: "Alpha 1".into(), id: Some(1), system_address: 100 }));
        status(&mut m, SHIP | flags::HAS_LAT_LONG, flags2::GLIDE_MODE, NEAR);
        assert_eq!(activity(&m), Activity::Glide);
        status(&mut m, SHIP | flags::HAS_LAT_LONG, 0, NEAR);
        assert_eq!(activity(&m), Activity::NearBody, "flying low over it");
        status(&mut m, SHIP | flags::HAS_LAT_LONG | flags::LANDED | flags::LANDING_GEAR_DOWN, 0, NEAR);
        let c = context(&m);
        assert_eq!(c.activity, Activity::Landed);
        assert!(c.on_surface());
        status(&mut m, SHIP, 0, "");
        assert_eq!(activity(&m), Activity::Space);
        assert!(context(&m).body.is_none(), "no body away from one");
    }

    #[test]
    fn charging_a_jump_and_hyperspace() {
        let mut m = logged_in();
        status(&mut m, SHIP | flags::SUPERCRUISE | flags::FSD_CHARGING, 0, "");
        assert_eq!(activity(&m), Activity::Supercruise, "a supercruise charge isn't a jump");
        status(&mut m, SHIP | flags::SUPERCRUISE | flags::FSD_CHARGING, flags2::FSD_HYPERDRIVE_CHARGING, "");
        assert_eq!(activity(&m), Activity::ChargingJump);
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:11:00Z", "event":"StartJump", "JumpType":"Hyperspace", "StarSystem":"Beta", "SystemAddress":200, "StarClass":"K" }"#]);
        assert_eq!(activity(&m), Activity::Hyperspace, "the journal knows before Status.json does");
        status(&mut m, SHIP | flags::FSD_JUMP, 0, "");
        assert_eq!(activity(&m), Activity::Hyperspace);
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:11:30Z", "event":"FSDJump", "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":0, "JumpDist":10.0, "FuelUsed":1.0, "FuelLevel":63.0 }"#]);
        status(&mut m, SHIP | flags::SUPERCRUISE, 0, "");
        assert_eq!(activity(&m), Activity::Supercruise, "arrived");
    }

    #[test]
    fn srv_rhino_nomad_and_on_foot() {
        let mut m = logged_in();
        let drive = |m: &mut Model, srv_type: &str| {
            feed(m, &[format!(r#"{{ "timestamp":"2026-10-25T10:12:00Z", "event":"LaunchSRV", "SRVType":"{srv_type}", "Loadout":"default", "ID":53, "PlayerControlled":true }}"#).as_str()]);
            status(m, flags::IN_SRV | flags::HAS_LAT_LONG, 0, NEAR);
        };
        drive(&mut m, "mev_rhino");
        let c = context(&m);
        assert_eq!(c.activity, Activity::Srv(Vehicle::Rhino));
        assert_eq!(c.body.map(|b| b.name), Some("Alpha 1".to_string()));
        drive(&mut m, "lander01");
        assert_eq!(activity(&m), Activity::Srv(Vehicle::Nomad), "the Nomad sets the same InSRV flag");
        drive(&mut m, "testbuggy");
        assert_eq!(activity(&m), Activity::Srv(Vehicle::Scarab));
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:20:00Z", "event":"DockSRV", "SRVType":"testbuggy", "ID":53 }"#]);
        assert_eq!(m.active().unwrap().location.srv_type, None);
        status(&mut m, flags::HAS_LAT_LONG, flags2::ON_FOOT | flags2::ON_FOOT_ON_PLANET | flags2::ON_FOOT_EXTERIOR, NEAR);
        let c = context(&m);
        assert_eq!(c.activity, Activity::OnFootPlanet);
        assert!(c.on_surface());
    }

    #[test]
    fn maps_panels_and_danger() {
        let mut m = logged_in();
        for (focus, want) in [(6, GuiFocus::GalaxyMap), (7, GuiFocus::SystemMap), (8, GuiFocus::Orrery), (9, GuiFocus::Fss), (10, GuiFocus::Saa)] {
            status(&mut m, SHIP | flags::SUPERCRUISE, 0, &format!(r#", "GuiFocus":{focus}"#));
            let c = context(&m);
            assert_eq!(c.focus, want);
            assert!(!c.in_panel());
        }
        for focus in 1..=5 {
            status(&mut m, SHIP | flags::SUPERCRUISE, 0, &format!(r#", "GuiFocus":{focus}"#));
            assert!(context(&m).in_panel(), "GuiFocus {focus} is a cockpit panel or the station's menus");
        }
        status(&mut m, SHIP | flags::SUPERCRUISE, 0, "");
        assert!(!context(&m).danger);
        status(&mut m, SHIP | flags::SUPERCRUISE | flags::IS_IN_DANGER, 0, "");
        assert!(context(&m).danger);
        status(&mut m, SHIP | flags::SUPERCRUISE | flags::BEING_INTERDICTED, 0, "");
        assert!(context(&m).danger, "an interdiction is danger too");
    }

    #[test]
    fn the_journal_alone_without_status_json() {
        let mut m = logged_in();
        assert_eq!(activity(&m), Activity::Space);
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:30:00Z", "event":"Docked", "StationName":"Jameson Memorial", "StationType":"Orbis", "StarSystem":"Alpha", "SystemAddress":100, "MarketID":128666762 }"#]);
        assert_eq!(activity(&m), Activity::Docked);
        feed(
            &mut m,
            &[
                r#"{ "timestamp":"2026-10-25T10:31:00Z", "event":"Undocked", "StationName":"Jameson Memorial", "MarketID":128666762 }"#,
                r#"{ "timestamp":"2026-10-25T10:32:00Z", "event":"SupercruiseEntry", "StarSystem":"Alpha", "SystemAddress":100 }"#,
            ],
        );
        assert_eq!(activity(&m), Activity::Supercruise);
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:40:00Z", "event":"Touchdown", "PlayerControlled":true, "StarSystem":"Alpha", "SystemAddress":100, "Body":"Alpha 1", "BodyID":1, "OnStation":false, "OnPlanet":true, "Latitude":10.0, "Longitude":20.0 }"#]);
        let c = context(&m);
        assert_eq!(c.activity, Activity::Landed);
        assert_eq!(c.body.map(|b| (b.name, b.id)), Some(("Alpha 1".to_string(), Some(1))));
        assert_eq!(c.focus, GuiFocus::None);
    }

    #[test]
    fn vehicles_by_srv_type() {
        assert_eq!(Vehicle::from_srv_type(Some("mev_rhino")), Vehicle::Rhino);
        assert_eq!(Vehicle::from_srv_type(Some("Lander01")), Vehicle::Nomad);
        assert_eq!(Vehicle::from_srv_type(Some("combat_multicrew_srv_01")), Vehicle::Scorpion);
        assert_eq!(Vehicle::from_srv_type(Some("testbuggy")), Vehicle::Scarab);
        assert_eq!(Vehicle::from_srv_type(None), Vehicle::Other);
    }
}
