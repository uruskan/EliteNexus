//! Follow my flight (Settings › Exploration): the page that fits what the commander is doing, the way Elite Dangerous
//! Exploration Buddy switches its tabs. Pure: `Follower::step` reads the model and names the page to open; the app
//! decides whether it may (the setting, the page shown, a journal followed live).

use crate::pages::Page;
use nexus_journal::companion::{GuiFocus, Status};
use nexus_model::context::{Activity, Context};
use nexus_model::GameState;
use std::sync::Arc;

/// What Follow my flight follows: the commander's context (`nexus_model::context`), reduced to the pages here the
/// way Elite Dangerous Exploration Buddy reduces it to its tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flight {
    /// Nothing to follow: not in the game, docked, driving, on foot inside.
    Idle,
    /// The galaxy map, a charging hyperspace jump, hyperspace: Navigation.
    Route,
    /// Flying the system, orbital cruise included: Exploration.
    System,
    /// Under the "Planet view below" altitude over a body, landed, or on foot outside: Exobiology when the body has
    /// life still to analyse, Exploration otherwise.
    Planet,
}

impl Flight {
    /// `altitude_m` is Status.json's altitude, when it gives one; `planet_m` is Settings' "Planet view below".
    fn of(cx: &Context, altitude_m: Option<f64>, planet_m: f64) -> Flight {
        // Plotting the next trip, wherever the map is opened: docked or in an SRV too.
        if cx.focus == GuiFocus::GalaxyMap {
            return Flight::Route;
        }
        match cx.activity {
            Activity::NotInGame | Activity::Docked | Activity::Srv(_) => Flight::Idle,
            Activity::ChargingJump | Activity::Hyperspace => Flight::Route,
            Activity::Landed | Activity::OnFootPlanet => Flight::Planet,
            Activity::NearBody | Activity::Glide => if altitude_m.is_some_and(|a| a < planet_m) { Flight::Planet } else { Flight::System },
            Activity::Supercruise | Activity::Space => Flight::System,
        }
    }
}

/// What Follow my flight compares from one Status.json update or journal event to the next.
struct Snapshot {
    /// The commander it was taken for (`cmdr.fid`).
    fid: String,
    /// The Status.json the model held when this was taken, compared by pointer (`Arc::ptr_eq`): the model makes a
    /// new `Arc` for every update, while the timestamp inside has one-second resolution and could miss one.
    status: Option<Arc<Status>>,
    /// `GameState::events_applied` when this was taken.
    events: u64,
    flight: Flight,
    /// The current system is fully explored (Settings "High-value body" threshold).
    done: bool,
}

/// What the commander was doing at the last Status.json update or journal event.
#[derive(Default)]
pub struct Follower {
    /// `None` until the first look at a commander.
    last: Option<Snapshot>,
}

impl Follower {
    /// Reads what the commander is doing (`Context::of`, from Status.json while the game runs) and, when it changed,
    /// names the page that fits: Navigation in the galaxy map, while a hyperspace jump charges and in hyperspace;
    /// Exploration while flying the system; Exobiology under the "Planet view below" altitude (`planet_m`), landed or
    /// on foot outside, when the body has life still to analyse (Exploration otherwise); Navigation too once the
    /// system is done (`high_value`: Settings' "High-value body"). The first look at a commander only takes the
    /// snapshot, after a switch to another commander too. Works only when a Status.json update or a journal event
    /// arrived: on every other frame it compares a counter, a pointer and the commander's id, and allocates nothing.
    pub fn step(&mut self, gs: &GameState, planet_m: f64, high_value: i64) -> Option<Page> {
        let same_status = |p: &Snapshot| match (&p.status, &gs.status) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if self.last.as_ref().is_some_and(|p| p.events == gs.events_applied && same_status(p) && p.fid == gs.cmdr.fid) {
            return None;
        }
        let cx = Context::of(gs);
        let flight = Flight::of(&cx, gs.status.as_ref().and_then(|s| s.altitude), planet_m);
        let system = gs.exploration.current_system();
        let done = system.is_some_and(|s| s.fully_explored(high_value));
        let prev = self.last.replace(Snapshot { fid: gs.cmdr.fid.clone(), status: gs.status.clone(), events: gs.events_applied, flight, done });
        // The first look at a commander only takes the snapshot: another commander's says nothing about this one (an
        // alt loaded in a finished system didn't just finish it).
        let prev = prev.filter(|p| p.fid == gs.cmdr.fid)?;
        if flight != prev.flight {
            match flight {
                Flight::Route => Some(Page::Navigation),
                Flight::System => Some(Page::Exploration),
                Flight::Planet => {
                    // The body by its id when the journal named it, else by Status.json's name.
                    let body = cx.body.as_ref().zip(system).and_then(|(b, s)| b.id.and_then(|id| s.bodies.get(&id)).or_else(|| s.bodies.values().find(|x| x.name == b.name)));
                    Some(if body.is_some_and(|b| b.bio_signals > 0 && !b.bio_done()) { Page::Exobiology } else { Page::Exploration })
                }
                Flight::Idle => None,
            }
        } else if done && !prev.done {
            Some(Page::Navigation)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_journal::companion::{flags, flags2};
    use nexus_journal::{raw::scan_line, Companion, CompanionKind, Event, EventCtx};
    use nexus_model::context::Vehicle;
    use nexus_model::{Alerts, Model};

    const KM150: f64 = 150_000.0;
    const HIGH_VALUE: i64 = 1_000_000;
    const SHIP: u32 = flags::IN_MAIN_SHIP | flags::SHIELDS_UP;
    /// A real Status.json's flags, docked: gear down, shields up, mass locked, in the main ship.
    const DOCKED: u32 = 16_842_765;

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

    /// Status.json's fields `altitude_m` above `body`.
    fn near(body: &str, altitude_m: u32) -> String {
        format!(r#", "Latitude":10.0, "Longitude":20.0, "Heading":90, "Altitude":{altitude_m}, "BodyName":"{body}", "PlanetRadius":1000000.0"#)
    }

    fn organic(kind: &str, ts: &str) -> String {
        format!(
            r#"{{ "timestamp":"{ts}", "event":"ScanOrganic", "ScanType":"{kind}", "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum", "Species":"$Codex_Ent_Stratum_07_Name;", "Species_Localised":"Stratum Tectonicas", "Variant":"$Codex_Ent_Stratum_07_K_Name;", "Variant_Localised":"Stratum Tectonicas - Lime", "WasLogged":false, "SystemAddress":200, "Body":1 }}"#
        )
    }

    fn cx(activity: Activity, focus: GuiFocus) -> Context {
        Context { activity, focus, danger: false, body: None }
    }

    /// A trip, step by step: the page each change opens, and nothing in between.
    #[test]
    fn follows_a_trip() {
        let mut m = Model::default();
        feed(&mut m, LOGIN);
        let mut f = Follower::default();
        let mut step = |m: &Model| f.step(m.active().expect("commander"), KM150, HIGH_VALUE);
        status(&mut m, SHIP | flags::SUPERCRUISE, 0, "");
        assert_eq!(step(&m), None, "the first look only takes the snapshot");
        assert_eq!(step(&m), None, "nothing new");
        status(&mut m, SHIP | flags::SUPERCRUISE | flags::FSD_CHARGING, flags2::FSD_HYPERDRIVE_CHARGING, "");
        assert_eq!(step(&m), Some(Page::Navigation), "a hyperspace jump charges");
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:11:00Z", "event":"StartJump", "JumpType":"Hyperspace", "StarSystem":"Beta", "SystemAddress":200, "StarClass":"K" }"#]);
        assert_eq!(step(&m), None, "hyperspace is still the route");
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:11:30Z", "event":"FSDJump", "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":0, "JumpDist":10.0, "FuelUsed":1.0, "FuelLevel":63.0 }"#]);
        status(&mut m, SHIP | flags::SUPERCRUISE, 0, "");
        assert_eq!(step(&m), Some(Page::Exploration), "arrived");
        // A rocky planet with one genus and an icy one without life; nothing worth mapping.
        feed(
            &mut m,
            &[
                r#"{ "timestamp":"2026-10-25T10:12:00Z", "event":"Scan", "ScanType":"AutoScan", "BodyName":"Beta", "BodyID":0, "StarSystem":"Beta", "SystemAddress":200, "DistanceFromArrivalLS":0.0, "StarType":"K", "StellarMass":0.7, "WasDiscovered":true, "WasMapped":false }"#,
                r#"{ "timestamp":"2026-10-25T10:13:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Beta 1", "BodyID":1, "StarSystem":"Beta", "SystemAddress":200, "DistanceFromArrivalLS":10.0, "PlanetClass":"Rocky body", "MassEM":0.1, "Landable":true, "WasDiscovered":false, "WasMapped":false, "WasFootfalled":false }"#,
                r#"{ "timestamp":"2026-10-25T10:13:10Z", "event":"FSSBodySignals", "BodyName":"Beta 1", "BodyID":1, "SystemAddress":200, "Signals":[ { "Type":"$SAA_SignalType_Biological;", "Type_Localised":"Biological", "Count":1 } ] }"#,
                r#"{ "timestamp":"2026-10-25T10:14:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Beta 2", "BodyID":2, "StarSystem":"Beta", "SystemAddress":200, "DistanceFromArrivalLS":900.0, "PlanetClass":"Icy body", "MassEM":0.01, "Landable":true, "WasDiscovered":true, "WasMapped":false }"#,
                r#"{ "timestamp":"2026-10-25T10:14:10Z", "event":"FSSAllBodiesFound", "SystemName":"Beta", "SystemAddress":200, "Count":3 }"#,
            ],
        );
        assert_eq!(step(&m), None, "scanning changes nothing");
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:15:00Z", "event":"ApproachBody", "StarSystem":"Beta", "SystemAddress":200, "Body":"Beta 1", "BodyID":1 }"#]);
        status(&mut m, SHIP | flags::SUPERCRUISE | flags::HAS_LAT_LONG, 0, &near("Beta 1", 400_000));
        assert_eq!(step(&m), None, "orbital cruise is still the system");
        status(&mut m, SHIP | flags::SUPERCRUISE | flags::HAS_LAT_LONG, 0, &near("Beta 1", 100_000));
        assert_eq!(step(&m), Some(Page::Exobiology), "under Planet view below, over biology left to analyse");
        status(&mut m, SHIP | flags::HAS_LAT_LONG, flags2::GLIDE_MODE, &near("Beta 1", 20_000));
        assert_eq!(step(&m), None, "gliding");
        status(&mut m, SHIP | flags::HAS_LAT_LONG | flags::LANDED | flags::LANDING_GEAR_DOWN, 0, &near("Beta 1", 0));
        assert_eq!(step(&m), None, "landed");
        status(&mut m, flags::HAS_LAT_LONG, flags2::ON_FOOT | flags2::ON_FOOT_ON_PLANET | flags2::ON_FOOT_EXTERIOR, &near("Beta 1", 0));
        assert_eq!(step(&m), None, "on foot");
        feed(&mut m, &[&organic("Log", "2026-10-25T10:20:00Z"), &organic("Sample", "2026-10-25T10:22:00Z")]);
        assert_eq!(step(&m), None, "two samples out of three");
        feed(&mut m, &[&organic("Analyse", "2026-10-25T10:24:00Z")]);
        assert_eq!(step(&m), Some(Page::Navigation), "the last genus analysed: the system is done");
        status(&mut m, SHIP | flags::SUPERCRUISE | flags::HAS_LAT_LONG, 0, &near("Beta 1", 400_000));
        assert_eq!(step(&m), Some(Page::Exploration), "back above Planet view below");
        feed(&mut m, &[r#"{ "timestamp":"2026-10-25T10:30:00Z", "event":"ApproachBody", "StarSystem":"Beta", "SystemAddress":200, "Body":"Beta 2", "BodyID":2 }"#]);
        status(&mut m, SHIP | flags::SUPERCRUISE | flags::HAS_LAT_LONG, 0, &near("Beta 2", 100_000));
        assert_eq!(step(&m), Some(Page::Exploration), "a planet without biology");
        status(&mut m, DOCKED, 0, r#", "GuiFocus":0"#);
        assert_eq!(step(&m), None, "docked");
        status(&mut m, DOCKED, 0, r#", "GuiFocus":6"#);
        assert_eq!(step(&m), Some(Page::Navigation), "the galaxy map, docked too");
        status(&mut m, DOCKED, 0, r#", "GuiFocus":0"#);
        assert_eq!(step(&m), None, "closing it");
    }

    /// Loading an alt docked in a finished system isn't the system just finished: the switch only takes a snapshot.
    #[test]
    fn another_commander_starts_afresh() {
        let alt = [
            r#"{ "timestamp":"2026-10-24T09:00:00Z", "event":"Commander", "FID":"F2", "Name":"Alt" }"#,
            r#"{ "timestamp":"2026-10-24T09:00:01Z", "event":"LoadGame", "FID":"F2", "Commander":"Alt", "Horizons":true, "Odyssey":true, "Ship":"explorer_nx", "ShipID":3, "ShipName":"", "ShipIdent":"", "FuelLevel":64.0, "FuelCapacity":64.0, "GameMode":"Solo", "Credits":1000, "Loan":0 }"#,
            r#"{ "timestamp":"2026-10-24T09:00:02Z", "event":"Location", "Docked":true, "StationName":"Gamma Port", "StationType":"Coriolis", "MarketID":3001, "StarSystem":"Gamma", "SystemAddress":300, "StarPos":[20.0,0.0,0.0], "Population":1000, "Body":"Gamma", "BodyID":0, "BodyType":"Star" }"#,
        ];
        let mut m = Model::default();
        // The alt's history: Gamma, a lone star the FSS has found.
        feed(&mut m, &alt);
        feed(
            &mut m,
            &[
                r#"{ "timestamp":"2026-10-24T09:01:00Z", "event":"Scan", "ScanType":"AutoScan", "BodyName":"Gamma", "BodyID":0, "StarSystem":"Gamma", "SystemAddress":300, "DistanceFromArrivalLS":0.0, "StarType":"G", "StellarMass":1.0, "WasDiscovered":true, "WasMapped":false }"#,
                r#"{ "timestamp":"2026-10-24T09:01:10Z", "event":"FSSAllBodiesFound", "SystemName":"Gamma", "SystemAddress":300, "Count":1 }"#,
            ],
        );
        // The main commander, docked in Alpha, which isn't done.
        feed(&mut m, LOGIN);
        status(&mut m, DOCKED, 0, r#", "GuiFocus":0"#);
        let mut f = Follower::default();
        let mut step = |m: &Model| f.step(m.active().expect("commander"), KM150, HIGH_VALUE);
        assert_eq!(step(&m), None, "the first look");
        feed(&mut m, &alt);
        assert!(m.active().unwrap().exploration.current_system().is_some_and(|s| s.fully_explored(HIGH_VALUE)), "the alt is in a finished system");
        assert_eq!(step(&m), None, "another commander");
        status(&mut m, SHIP | flags::SUPERCRUISE, 0, "");
        assert_eq!(step(&m), Some(Page::Exploration), "the alt is followed from there");
    }

    /// The pages Follow my flight moves between, from the commander's context, EDEB's way.
    #[test]
    fn flight_from_context() {
        let of = |a: Activity, alt: Option<f64>| Flight::of(&cx(a, GuiFocus::None), alt, KM150);
        assert_eq!(of(Activity::Supercruise, None), Flight::System);
        assert_eq!(of(Activity::Space, None), Flight::System);
        assert_eq!(of(Activity::ChargingJump, None), Flight::Route);
        assert_eq!(of(Activity::Hyperspace, None), Flight::Route);
        assert_eq!(of(Activity::NearBody, Some(400_000.0)), Flight::System, "orbital cruise");
        assert_eq!(of(Activity::NearBody, Some(149_999.0)), Flight::Planet);
        assert_eq!(of(Activity::NearBody, None), Flight::System, "no altitude isn't under it");
        assert_eq!(of(Activity::Glide, Some(20_000.0)), Flight::Planet);
        assert_eq!(of(Activity::Landed, Some(0.0)), Flight::Planet);
        assert_eq!(of(Activity::OnFootPlanet, Some(0.0)), Flight::Planet);
        assert_eq!(of(Activity::Srv(Vehicle::Scarab), Some(0.0)), Flight::Idle, "driving changes nothing");
        assert_eq!(of(Activity::Docked, None), Flight::Idle);
        assert_eq!(of(Activity::NotInGame, None), Flight::Idle);
        assert_eq!(Flight::of(&cx(Activity::NearBody, GuiFocus::GalaxyMap), Some(1_000.0), KM150), Flight::Route, "the galaxy map wins");
        assert_eq!(Flight::of(&cx(Activity::Docked, GuiFocus::GalaxyMap), None, KM150), Flight::Route, "docked too");
        assert_eq!(Flight::of(&cx(Activity::Srv(Vehicle::Scarab), GuiFocus::GalaxyMap), Some(0.0), KM150), Flight::Route, "and in an SRV");
        assert_eq!(Flight::of(&cx(Activity::NearBody, GuiFocus::SystemMap), Some(1_000.0), KM150), Flight::Planet, "the system map doesn't");
        assert_eq!(Flight::of(&cx(Activity::NearBody, GuiFocus::None), Some(100_000.0), 50_000.0), Flight::System, "the altitude is the setting's");
    }
}
