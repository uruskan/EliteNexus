//! Scenario tests: real-format journal lines through the full model.

use nexus_journal::{raw::scan_line, Event, EventCtx};
use nexus_model::{Alerts, Model};

/// Applies journal lines (live = true so alerts fire) and returns the collected alerts.
fn feed(model: &mut Model, lines: &[&str]) -> Vec<nexus_model::Alert> {
    let mut alerts = Alerts::live();
    for line in lines {
        let (ts, name) = scan_line(line).expect("valid journal line");
        let ev = Event::parse(name, line).expect("typed parse");
        let ctx = EventCtx { ts, name, line, live: true };
        model.apply(&ctx, &ev, &mut alerts);
    }
    alerts.items
}

const LOGIN: &[&str] = &[
    r#"{ "timestamp":"2026-10-01T10:00:00Z", "event":"Fileheader", "part":1, "language":"English/UK", "Odyssey":true, "gameversion":"4.4.1.1", "build":"r332841/r0 " }"#,
    r#"{ "timestamp":"2026-10-01T10:00:01Z", "event":"Commander", "FID":"F1", "Name":"Test" }"#,
    r#"{ "timestamp":"2026-10-01T10:00:02Z", "event":"LoadGame", "FID":"F1", "Commander":"Test", "Horizons":true, "Odyssey":true, "Ship":"explorer_nx", "ShipID":22, "ShipName":"", "ShipIdent":"", "FuelLevel":64.0, "FuelCapacity":64.0, "GameMode":"Solo", "Credits":1000000, "Loan":0, "language":"English/UK", "gameversion":"4.4.1.1", "build":"r332841/r0 " }"#,
    r#"{ "timestamp":"2026-10-01T10:00:03Z", "event":"Location", "Docked":false, "StarSystem":"Alpha", "SystemAddress":100, "StarPos":[0.0,0.0,0.0], "Population":0, "Body":"Alpha", "BodyID":0, "BodyType":"Star" }"#,
];

#[test]
fn login_sets_commander_and_location() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let gs = m.active().expect("commander");
    assert_eq!(gs.cmdr.name, "Test");
    assert_eq!(gs.cmdr.game_build, "r332841/r0 ", "game build must be kept byte-exact (EDDN)");
    assert_eq!(gs.location.system, "Alpha");
    assert_eq!(gs.fleet.current_ship_id, Some(22));
}

#[test]
fn unsold_data_and_sale_skipping() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let alerts = feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"Scan", "ScanType":"AutoScan", "BodyName":"Alpha", "BodyID":0, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":0.0, "StarType":"K", "StellarMass":0.7, "WasDiscovered":true, "WasMapped":false }"#,
            r#"{ "timestamp":"2026-10-01T10:02:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 1", "BodyID":1, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":400.0, "PlanetClass":"Earthlike body", "MassEM":1.0, "TerraformState":"", "Landable":false, "WasDiscovered":false, "WasMapped":false }"#,
            r#"{ "timestamp":"2026-10-01T10:05:00Z", "event":"FSDJump", "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":0, "JumpDist":10.0, "FuelUsed":1.0, "FuelLevel":63.0 }"#,
            r#"{ "timestamp":"2026-10-01T10:06:00Z", "event":"Scan", "ScanType":"AutoScan", "BodyName":"Beta", "BodyID":0, "StarSystem":"Beta", "SystemAddress":200, "DistanceFromArrivalLS":0.0, "StarType":"M", "StellarMass":0.3, "WasDiscovered":true }"#,
        ],
    );
    assert!(alerts.iter().any(|a| a.title.contains("Earth-like")), "ELW must raise an exploration alert");
    let gs = m.active().unwrap();
    assert_eq!(gs.exploration.unsold().count(), 2);
    assert!(gs.exploration.unsold_value() > 500_000, "first-discovered ELW is worth a lot");

    // Sell only Alpha: Beta was scanned before the sale but the game didn't buy it -> skipped.
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T11:00:00Z", "event":"FSDJump", "StarSystem":"Gamma", "SystemAddress":300, "StarPos":[20.0,0.0,0.0], "Population":5000, "JumpDist":10.0 }"#,
            r#"{ "timestamp":"2026-10-01T11:10:00Z", "event":"MultiSellExplorationData", "Discovered":[ { "SystemName":"Alpha", "NumBodies":2 } ], "BaseValue":300000, "Bonus":400000, "TotalEarnings":700000 }"#,
        ],
    );
    let gs = m.active().unwrap();
    assert_eq!(gs.exploration.unsold().count(), 0);
    assert_eq!(gs.exploration.skipped().count(), 1);
    assert_eq!(gs.exploration.sold_total, 700_000);

    // Sold data stays in the history, with what became of it.
    use nexus_model::exploration::DataStatus;
    let ex = &gs.exploration;
    assert_eq!(ex.history().count(), 2);
    let sale = ex.last_sale().expect("sale recorded");
    assert_eq!((sale.systems, sale.earnings), (1, 700_000));
    assert_eq!(ex.systems[&100].status(), DataStatus::Sold(sale.ts));
    assert_eq!(ex.systems[&200].status(), DataStatus::NotSellable);
}

#[test]
fn exobiology_sampling_and_bonus() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 2", "BodyID":2, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":10.0, "PlanetClass":"Rocky body", "MassEM":0.1, "Landable":true, "WasDiscovered":false, "WasMapped":false, "WasFootfalled":false }"#],
    );
    let organic = |kind: &str, ts: &str| {
        format!(
            r#"{{ "timestamp":"{ts}", "event":"ScanOrganic", "ScanType":"{kind}", "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum", "Species":"$Codex_Ent_Stratum_07_Name;", "Species_Localised":"Stratum Tectonicas", "Variant":"$Codex_Ent_Stratum_07_K_Name;", "Variant_Localised":"Stratum Tectonicas - Lime", "WasLogged":false, "SystemAddress":100, "Body":2 }}"#
        )
    };
    let l1 = organic("Log", "2026-10-01T10:10:00Z");
    let l2 = organic("Sample", "2026-10-01T10:12:00Z");
    let l3 = organic("Analyse", "2026-10-01T10:14:00Z");
    let alerts = feed(&mut m, &[&l1]);
    assert!(alerts.iter().any(|a| a.detail.contains("500 m")), "Stratum colony range is 500 m");
    feed(&mut m, &[&l2]);
    let alerts = feed(&mut m, &[&l3]);
    assert!(alerts.iter().any(|a| a.title.contains("Analysis complete")));
    let gs = m.active().unwrap();
    assert!(gs.exobio.current.is_none());
    assert_eq!(gs.exobio.unsold.len(), 1);
    let rec = &gs.exobio.unsold[0];
    assert_eq!(rec.value, 19_010_800);
    assert!(rec.bonus_expected, "unfootfalled body in an unpopulated system");
    assert_eq!(rec.expected_payout(), 19_010_800 * 5);

    // A partly sampled organism is abandoned when the commander jumps away.
    let l4 = organic("Log", "2026-10-01T10:20:00Z");
    feed(&mut m, &[&l4]);
    assert!(m.active().unwrap().exobio.current.is_some());
    feed(
        &mut m,
        &[r#"{ "timestamp":"2026-10-01T10:30:00Z", "event":"FSDJump", "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":0, "JumpDist":10.0, "FuelUsed":1.0, "FuelLevel":60.0 }"#],
    );
    assert!(m.active().unwrap().exobio.current.is_none(), "stale sample cleared after leaving the system");
}

#[test]
fn carrier_transfer_is_not_spending() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[r#"{ "timestamp":"2026-10-01T10:30:00Z", "event":"CarrierBankTransfer", "CarrierID":3716323840, "Deposit":500000, "PlayerBalance":500000, "CarrierBalance":500000 }"#],
    );
    let gs = m.active().unwrap();
    assert_eq!(gs.session.expenses, 0, "a deposit to your own carrier is a transfer");
    assert_eq!(gs.cmdr.credits, 500_000);
}

/// The carrier jump timer through real journal lines: request, cancel, request again, arrive
/// while docked aboard, cool down, ready.
#[test]
fn carrier_jump_countdown_then_cooldown() {
    use nexus_journal::Timestamp;
    use nexus_model::carrier::JumpTimer;
    let t = |s: &str| Timestamp::parse(s).expect("time");
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:00:05Z", "event":"CarrierLocation", "CarrierType":"FleetCarrier", "CarrierID":3716323840, "StarSystem":"Alpha", "SystemAddress":100, "BodyID":0 }"#,
            r#"{ "timestamp":"2026-10-01T10:10:00Z", "event":"CarrierJumpRequest", "CarrierType":"FleetCarrier", "CarrierID":3716323840, "SystemName":"Beta", "Body":"Beta A 1", "SystemAddress":200, "BodyID":3, "DepartureTime":"2026-10-01T10:25:10Z" }"#,
        ],
    );
    let c = &m.active().unwrap().carrier;
    let timer = c.jump_timer(t("2026-10-01T10:15:10Z")).expect("countdown");
    assert!(matches!(&timer, JumpTimer::Countdown { to, .. } if to == "Beta"));
    assert_eq!(timer.left(t("2026-10-01T10:15:10Z")), Some(600));

    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T10:12:00Z", "event":"CarrierJumpCancelled", "CarrierType":"FleetCarrier", "CarrierID":3716323840 }"#]);
    assert_eq!(m.active().unwrap().carrier.jump_timer(t("2026-10-01T10:12:01Z")), None, "cancelled");

    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"CarrierJumpRequest", "CarrierType":"FleetCarrier", "CarrierID":3716323840, "SystemName":"Beta", "SystemAddress":200, "BodyID":0, "DepartureTime":"2026-10-01T10:35:00Z" }"#,
            r#"{ "timestamp":"2026-10-01T10:35:40Z", "event":"CarrierJump", "Docked":true, "StationName":"KFK-L3V", "StationType":"FleetCarrier", "MarketID":3716323840, "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":0, "Body":"Beta", "BodyID":0, "BodyType":"Star" }"#,
        ],
    );
    let gs = m.active().unwrap();
    assert_eq!(gs.carrier.system.as_deref(), Some("Beta"));
    let timer = gs.carrier.jump_timer(t("2026-10-01T10:36:40Z")).expect("cooldown");
    assert_eq!(timer, JumpTimer::Cooldown { at: "Beta".into(), body: Some("Beta".into()), ready: t("2026-10-01T10:40:40Z"), estimated: false });
    assert_eq!(gs.carrier.jump_timer(t("2026-10-01T10:40:40Z")), None, "ready to jump again");
}

const OWN_STATS: &str = r#"{ "timestamp":"2026-10-01T10:05:00Z", "event":"CarrierStats", "CarrierID":3716323840, "CarrierType":"FleetCarrier", "Callsign":"KFK-L3V", "Name":"TCG BARBAROS", "DockingAccess":"all", "AllowNotorious":false, "FuelLevel":500, "JumpRangeCurr":500.000000, "JumpRangeMax":500.000000, "PendingDecommission":false, "SpaceUsage":{ "TotalCapacity":25000, "Crew":6170, "Cargo":1200, "CargoSpaceReserved":0, "ShipPacks":0, "ModulePacks":0, "FreeSpace":17630 }, "Finance":{ "CarrierBalance":1000000000, "ReserveBalance":0, "AvailableBalance":1000000000 }, "Crew":[ { "CrewRole":"Captain", "Activated":true, "Enabled":true, "CrewName":"Raphael Mendez" } ], "ShipPacks":[ ], "ModulePacks":[ ] }"#;
const SQUADRON_STATS: &str = r#"{ "timestamp":"2026-10-01T10:06:00Z", "event":"CarrierStats", "CarrierID":3720000000, "CarrierType":"SquadronCarrier", "Callsign":"SQD-001", "Name":"VANGUARD HOME", "DockingAccess":"squadron", "AllowNotorious":false, "FuelLevel":1800, "JumpRangeCurr":500.000000, "JumpRangeMax":500.000000, "PendingDecommission":false, "SpaceUsage":{ "TotalCapacity":60000, "Crew":6170, "Cargo":20000, "CargoSpaceReserved":0, "ShipPacks":0, "ModulePacks":0, "FreeSpace":33830 }, "Finance":{ "CarrierBalance":5000000000, "ReserveBalance":0, "AvailableBalance":5000000000 }, "Crew":[ ], "ShipPacks":[ ], "ModulePacks":[ ] }"#;

/// A squadron carrier next to the commander's own: each keeps its own stats, place and jumps.
#[test]
fn squadron_carrier_alongside_a_personal_one() {
    use nexus_journal::Timestamp;
    use nexus_model::carrier::{CarrierKind, JumpTimer};
    let t = |s: &str| Timestamp::parse(s).expect("time");
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:00:04Z", "event":"SquadronStartup", "SquadronName":"VANGUARDS OF TEST", "SquadronID":42, "CurrentRank":3, "CurrentRankName":"Officer" }"#,
            r#"{ "timestamp":"2026-10-01T10:00:05Z", "event":"CarrierLocation", "CarrierType":"FleetCarrier", "CarrierID":3716323840, "StarSystem":"Alpha", "SystemAddress":100, "BodyID":0 }"#,
            r#"{ "timestamp":"2026-10-01T10:00:05Z", "event":"CarrierLocation", "CarrierType":"SquadronCarrier", "CarrierID":3720000000, "StarSystem":"Gamma", "SystemAddress":300, "BodyID":1 }"#,
            OWN_STATS,
            SQUADRON_STATS,
            r#"{ "timestamp":"2026-10-01T10:10:00Z", "event":"CarrierJumpRequest", "CarrierType":"SquadronCarrier", "CarrierID":3720000000, "SystemName":"Delta", "SystemAddress":400, "BodyID":0, "DepartureTime":"2026-10-01T10:25:00Z" }"#,
            r#"{ "timestamp":"2026-10-01T10:11:00Z", "event":"CarrierDepositFuel", "CarrierID":3720000000, "Amount":200, "Total":2000 }"#,
        ],
    );
    let gs = m.active().unwrap();
    let (own, sq) = (&gs.carrier, &gs.squadron_carrier);
    assert_eq!((own.callsign.as_str(), own.fuel, own.system.as_deref()), ("KFK-L3V", 500, Some("Alpha")));
    assert_eq!((sq.callsign.as_str(), sq.fuel, sq.system.as_deref()), ("SQD-001", 2000, Some("Gamma")), "an untyped event goes by the carrier's id");
    assert_eq!(sq.space.total_capacity, 60_000);
    assert!(own.pending_jump.is_none(), "the squadron carrier's jump isn't the personal one's");
    let now = t("2026-10-01T10:20:00Z");
    assert!(matches!(sq.jump_timer(now), Some(JumpTimer::Countdown { to, .. }) if to == "Delta"));
    let kinds: Vec<CarrierKind> = gs.carriers(now).into_iter().map(|(k, _)| k).collect();
    assert_eq!(kinds, [CarrierKind::Personal, CarrierKind::Squadron]);

    // Docked aboard the squadron carrier when it jumps: only it moves.
    feed(
        &mut m,
        &[r#"{ "timestamp":"2026-10-01T10:25:40Z", "event":"CarrierJump", "Docked":true, "StationName":"SQD-001", "StationType":"SquadronCarrier", "MarketID":3720000000, "StarSystem":"Delta", "SystemAddress":400, "StarPos":[30.0,0.0,0.0], "Population":0, "Body":"Delta", "BodyID":0, "BodyType":"Star" }"#],
    );
    let gs = m.active().unwrap();
    assert_eq!(gs.squadron_carrier.system.as_deref(), Some("Delta"));
    assert_eq!(gs.carrier.system.as_deref(), Some("Alpha"));
    assert!(matches!(gs.squadron_carrier.jump_timer(t("2026-10-01T10:26:00Z")), Some(JumpTimer::Cooldown { estimated: false, .. })));

    // Credits given to the squadron's carrier are spent; on the commander's own they stay theirs.
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:30:00Z", "event":"CarrierBankTransfer", "CarrierType":"SquadronCarrier", "CarrierID":3720000000, "Deposit":100000, "PlayerBalance":900000, "CarrierBalance":5000100000 }"#,
            r#"{ "timestamp":"2026-10-01T10:31:00Z", "event":"CarrierBankTransfer", "CarrierType":"FleetCarrier", "CarrierID":3716323840, "Deposit":200000, "PlayerBalance":700000, "CarrierBalance":1000200000 }"#,
        ],
    );
    let gs = m.active().unwrap();
    assert_eq!(gs.session.expenses, -100_000);
    assert_eq!((gs.squadron_carrier.finance.carrier_balance, gs.carrier.finance.carrier_balance), (5_000_100_000, 1_000_200_000));

    // Leaving the squadron leaves its carrier behind.
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T11:00:00Z", "event":"LeftSquadron", "SquadronName":"VANGUARDS OF TEST", "SquadronID":42 }"#]);
    let gs = m.active().unwrap();
    assert!(!gs.squadron_carrier.owned());
    assert!(gs.carrier.owned());
}

/// The Reddit report: a commander scrapped their own carrier and now uses their squadron's. The
/// page kept showing the old one as "decommissioning" and never the squadron carrier.
#[test]
fn decommissioned_carrier_gives_way_to_the_squadron_carrier() {
    use nexus_journal::Timestamp;
    use nexus_model::carrier::CarrierKind;
    let t = |s: &str| Timestamp::parse(s).expect("time");
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[
            OWN_STATS,
            r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"CarrierDecommission", "CarrierID":3716323840, "ScrapRefund":4850000000, "ScrapTime":1791454800 }"#,
            &OWN_STATS.replace("10:05:00", "10:21:00").replace(r#""PendingDecommission":false"#, r#""PendingDecommission":true"#),
            SQUADRON_STATS,
        ],
    );
    let gs = m.active().unwrap();
    let before = t("2026-10-08T10:19:59Z");
    let after = t("2026-10-08T10:20:00Z");
    assert!(gs.carrier.pending_decommission);
    assert_eq!(gs.carrier.decommission.map(|d| (d.scrap_time, d.refund)), Some((after, 4_850_000_000)));
    assert_eq!(gs.carriers(before).len(), 2, "still there until the scrap time");
    assert!(gs.carrier.decommissioned(after));
    let left: Vec<CarrierKind> = gs.carriers(after).into_iter().map(|(k, _)| k).collect();
    assert_eq!(left, [CarrierKind::Squadron], "once scrapped, only the squadron carrier is left");
    assert_eq!(gs.squadron_carrier.name, "VANGUARD HOME", "a known personal carrier doesn't hide the squadron's");
    assert!(gs.feed.iter().any(|f| f.event == "CarrierDecommission"));
}

/// A decommission called off: the carrier stays.
#[test]
fn cancelled_decommission() {
    use nexus_journal::Timestamp;
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[
            OWN_STATS,
            r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"CarrierDecommission", "CarrierType":"FleetCarrier", "CarrierID":3716323840, "ScrapRefund":4850000000, "ScrapTime":1791454800 }"#,
            r#"{ "timestamp":"2026-10-02T09:00:00Z", "event":"CarrierCancelDecommission", "CarrierType":"FleetCarrier", "CarrierID":3716323840 }"#,
        ],
    );
    let c = &m.active().unwrap().carrier;
    assert!(!c.pending_decommission && c.decommission.is_none());
    assert!(c.active(Timestamp::parse("2026-10-09T00:00:00Z").unwrap()));
}

#[test]
fn stored_ships_in_transit_show_destination() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[r#"{ "timestamp":"2026-10-01T10:40:00Z", "event":"StoredShips", "StationName":"KFK-L3V", "MarketID":3716323840, "StarSystem":"Ross 775", "ShipsHere":[ ], "ShipsRemote":[ { "ShipID":5, "ShipType":"DiamondBackXL", "Name":"", "InTransit":true, "Value":1789130, "Hot":false } ] }"#],
    );
    let gs = m.active().unwrap();
    let s = &gs.fleet.ships[&5];
    assert!(s.location.in_transit);
    assert_eq!(s.location.station.as_deref(), Some("KFK-L3V"));
    assert_eq!(s.location.system.as_deref(), Some("Ross 775"));
    assert!(gs.fleet.ships.contains_key(&22), "current ship is kept");
}

#[test]
fn massacre_stack_end_to_end() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let accept = |id: u64, faction: &str, kills: u32, ts: &str| {
        format!(
            r#"{{ "timestamp":"{ts}", "event":"MissionAccepted", "Faction":"{faction}", "Name":"Mission_MassacreWing", "LocalisedName":"Kill pirates", "MissionID":{id}, "Reward":10000000, "TargetFaction":"Pirates", "KillCount":{kills}, "Expiry":"2026-10-03T10:00:00Z", "Wing":true }}"#
        )
    };
    let (a, b) = (accept(1, "A", 2, "2026-10-01T10:00:10Z"), accept(2, "B", 1, "2026-10-01T10:00:11Z"));
    feed(&mut m, &[&a, &b]);
    let bounty = r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"Bounty", "Rewards":[ { "Faction":"X", "Reward":1000 } ], "Target":"anaconda", "TotalReward":1000, "VictimFaction":"Pirates" }"#;
    let alerts = feed(&mut m, &[bounty]);
    assert_eq!(alerts.iter().filter(|a| a.title.contains("Massacre mission complete")).count(), 1, "B's single kill done");
    let stacks = m.active().unwrap().missions.massacre_stacks();
    assert_eq!(stacks[0].kills_remaining, 1);
    feed(&mut m, &[bounty]);
    assert_eq!(m.active().unwrap().missions.massacre_stacks()[0].kills_remaining, 0);
}

/// A construction depot snapshot, as the game writes it every few seconds while you're docked there.
fn depot_line(ts: &str, market_id: u64, progress: f64, provided_steel: i64) -> String {
    format!(
        r#"{{ "timestamp":"{ts}", "event":"ColonisationConstructionDepot", "MarketID":{market_id}, "ConstructionProgress":{progress}, "ConstructionComplete":false, "ConstructionFailed":false, "ResourcesRequired":[ {{ "Name":"$Aluminium_name;", "Name_Localised":"Aluminium", "RequiredAmount":510, "ProvidedAmount":0, "Payment":3239 }}, {{ "Name":"$Steel_name;", "Name_Localised":"Steel", "RequiredAmount":21325, "ProvidedAmount":{provided_steel}, "Payment":5057 }} ] }}"#
    )
}

const HINDS_PRIDE: u64 = 3_956_789_012;

/// Someone else's colonisation ship: the forum report of 0.5.0 (a phantom "$EXT_PANEL_ColonisationShip; Hind's Pride"
/// at 0 %, shown as an active construction forever).
#[test]
fn someone_elses_colonisation_ship_is_not_yours() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let snapshot = depot_line("2026-10-01T10:12:00Z", HINDS_PRIDE, 0.0, 0);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:05:00Z", "event":"FSDJump", "StarSystem":"Hind", "SystemAddress":500, "StarPos":[5.0,0.0,0.0], "Population":0, "JumpDist":5.0, "FuelUsed":1.0, "FuelLevel":63.0 }"#,
            r#"{ "timestamp":"2026-10-01T10:11:00Z", "event":"Docked", "StationName":"$EXT_PANEL_ColonisationShip; Hind's Pride", "StationName_Localised":"System Colonisation Ship", "StationType":"SpaceConstructionDepot", "Taxi":false, "Multicrew":false, "StarSystem":"Hind", "SystemAddress":500, "MarketID":3956789012, "StationFaction":{ "Name":"Brewer Corporation" }, "StationGovernment":"$government_Corporate;", "StationServices":[ "dock", "autodock", "commodities", "contacts", "colonisationcontribution" ], "StationEconomy":"$economy_Colony;", "DistFromStarLS":12.3, "LandingPads":{ "Small":4, "Medium":4, "Large":8 } }"#,
            &snapshot,
        ],
    );
    let gs = m.active().unwrap();
    // The symbol is stripped wherever the name is shown: the situation line, the activity feed, the docking history.
    let here = gs.location.station.as_ref().expect("docked");
    assert_eq!(here.name, "Hind's Pride");
    assert!(here.is_colonisation_ship());
    assert_eq!(here.journal_name, "$EXT_PANEL_ColonisationShip; Hind's Pride", "uploads keep the journal's own name");
    assert!(gs.feed.iter().any(|f| f.text == "Docked at Hind's Pride (Hind)"));
    assert_eq!(gs.history.docks.last().map(|d| d.station.as_str()), Some("Hind's Pride"));

    let col = &gs.colonisation;
    let d = &col.depots[&HINDS_PRIDE];
    assert_eq!((d.station.as_deref(), d.system.as_str(), d.system_address), (Some("Hind's Pride"), "Hind", 500));
    assert!(d.colonisation_ship);
    assert_eq!(d.remaining_tons(), 21_835);
    use nexus_model::colonisation::Involvement;
    assert_eq!(col.involvement(d), Involvement::Visited, "no claim, no delivery: someone else's project");
    let soon = nexus_journal::Timestamp::parse("2026-10-01T12:00:00Z").unwrap();
    assert_eq!(col.active_depots(soon).count(), 0, "the dashboard doesn't show it as yours");

    // Leave and dock elsewhere. A snapshot of a depot you aren't docked at never gets this station's name or system;
    // one of a depot already known updates its figures and keeps where it is.
    let unknown = depot_line("2026-10-01T11:01:00Z", 3_956_000_001, 0.5, 100);
    let known = depot_line("2026-10-01T11:02:00Z", HINDS_PRIDE, 0.25, 5_000);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:40:00Z", "event":"Undocked", "StationName":"$EXT_PANEL_ColonisationShip; Hind's Pride", "StationType":"SpaceConstructionDepot", "MarketID":3956789012, "Taxi":false, "Multicrew":false }"#,
            r#"{ "timestamp":"2026-10-01T10:50:00Z", "event":"FSDJump", "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":5000, "JumpDist":5.0, "FuelUsed":1.0, "FuelLevel":62.0 }"#,
            r#"{ "timestamp":"2026-10-01T11:00:00Z", "event":"Docked", "StationName":"Beta Hub", "StationType":"Coriolis", "StarSystem":"Beta", "SystemAddress":200, "MarketID":128000001, "StationServices":[ "dock", "commodities" ], "DistFromStarLS":300.0 }"#,
            &unknown,
            &known,
        ],
    );
    let gs = m.active().unwrap();
    assert!(gs.feed.iter().any(|f| f.text == "Undocked from Hind's Pride"));
    let col = &gs.colonisation;
    assert_eq!(col.depots.len(), 1, "an unplaced snapshot isn't pinned on the station you're docked at");
    let d = &col.depots[&HINDS_PRIDE];
    assert_eq!((d.station.as_deref(), d.system.as_str()), (Some("Hind's Pride"), "Hind"));
    assert_eq!(d.progress, 0.25);
    assert_eq!(d.remaining_tons(), 16_835);

    // Four weeks without a visit: out of date (a colonisation ship has four weeks to be finished).
    let later = nexus_journal::Timestamp::parse("2026-10-29T11:02:00Z").unwrap();
    assert!(d.is_stale(later));
    assert!(!d.is_stale(soon));
}

#[test]
fn claimed_and_delivered_constructions_are_yours() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let ship = depot_line("2026-10-01T10:21:00Z", 3_956_000_100, 0.1, 2_000);
    let site = depot_line("2026-10-01T11:01:00Z", 3_956_000_200, 0.6, 12_000);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"ColonisationSystemClaim", "StarSystem":"Gamma", "SystemAddress":300 }"#,
            r#"{ "timestamp":"2026-10-01T10:10:00Z", "event":"FSDJump", "StarSystem":"Gamma", "SystemAddress":300, "StarPos":[20.0,0.0,0.0], "Population":0, "JumpDist":10.0 }"#,
            r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"Docked", "StationName":"$EXT_PANEL_ColonisationShip; Gamma Rising", "StationType":"SpaceConstructionDepot", "StarSystem":"Gamma", "SystemAddress":300, "MarketID":3956000100 }"#,
            &ship,
            r#"{ "timestamp":"2026-10-01T10:30:00Z", "event":"Undocked", "StationName":"$EXT_PANEL_ColonisationShip; Gamma Rising", "MarketID":3956000100 }"#,
            r#"{ "timestamp":"2026-10-01T10:50:00Z", "event":"FSDJump", "StarSystem":"Delta", "SystemAddress":400, "StarPos":[30.0,0.0,0.0], "Population":0, "JumpDist":10.0 }"#,
            r#"{ "timestamp":"2026-10-01T11:00:00Z", "event":"Docked", "StationName":"Orbital Construction Site: Delta Works", "StationType":"SpaceConstructionDepot", "StarSystem":"Delta", "SystemAddress":400, "MarketID":3956000200 }"#,
            &site,
            r#"{ "timestamp":"2026-10-01T11:02:00Z", "event":"ColonisationContribution", "MarketID":3956000200, "Contributions":[ { "Name":"$Steel_name;", "Name_Localised":"Steel", "Amount":200 } ] }"#,
        ],
    );
    use nexus_model::colonisation::Involvement;
    let col = &m.active().unwrap().colonisation;
    let ship = &col.depots[&3_956_000_100];
    assert_eq!(ship.station.as_deref(), Some("Gamma Rising"));
    assert_eq!(col.involvement(ship), Involvement::Claimed);
    let site = &col.depots[&3_956_000_200];
    assert!(!site.colonisation_ship);
    assert_eq!(site.delivered_tons(), 200);
    assert_eq!(col.involvement(site), Involvement::Contributed, "someone else's system, but you delivered");
    let soon = nexus_journal::Timestamp::parse("2026-10-02T00:00:00Z").unwrap();
    let active: Vec<u64> = col.active_depots(soon).map(|d| d.market_id).collect();
    assert_eq!(active, [3_956_000_200, 3_956_000_100], "most recently updated first");
    let later = nexus_journal::Timestamp::parse("2026-11-01T00:00:00Z").unwrap();
    assert_eq!(col.active_depots(later).count(), 0, "out of date after four weeks");

    // Releasing the claim makes the colonisation ship someone else's business again.
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T12:00:00Z", "event":"ColonisationSystemClaimRelease", "StarSystem":"Gamma", "SystemAddress":300 }"#]);
    let col = &m.active().unwrap().colonisation;
    assert_eq!(col.involvement(&col.depots[&3_956_000_100]), Involvement::Visited);
}

/// NavRoute.json as the game writes it when a route is plotted: the system it was plotted from comes first.
fn nav_route(systems: &[(&str, u64, f64)]) -> nexus_journal::Companion {
    let hops: Vec<String> = systems
        .iter()
        .map(|(name, addr, x)| format!(r#"{{ "StarSystem":"{name}", "SystemAddress":{addr}, "StarPos":[{x:.1},0.0,0.0], "StarClass":"M" }}"#))
        .collect();
    let text = format!(r#"{{ "timestamp":"2026-10-01T10:01:00Z", "event":"NavRoute", "Route":[ {} ] }}"#, hops.join(", "));
    nexus_journal::Companion::parse(nexus_journal::CompanionKind::NavRoute, &text).expect("NavRoute.json")
}

fn jump(ts: &str, name: &str, addr: u64, x: f64) -> String {
    format!(r#"{{ "timestamp":"{ts}", "event":"FSDJump", "StarSystem":"{name}", "SystemAddress":{addr}, "StarPos":[{x:.1},0.0,0.0], "Population":0, "JumpDist":10.0, "FuelUsed":1.0, "FuelLevel":60.0 }}"#)
}

fn route_names(m: &Model) -> Vec<String> {
    m.active().expect("commander").nav_route.iter().map(|h| h.star_system.clone()).collect()
}

/// What the Navigation page, the Dashboard and "copy the next system" show as still ahead.
fn ahead(m: &Model) -> Vec<String> {
    m.active().expect("commander").route_remaining().iter().map(|h| h.star_system.clone()).collect()
}

/// Frontier Forums report: after the first jump of a plotted route, the system it was plotted from came back as
/// the next stop (and the remaining distance grew), because only the system just reached was taken off the route.
#[test]
fn plotted_route_drops_the_systems_behind_you() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    m.apply_companion(&nav_route(&[("Alpha", 100, 0.0), ("Beta", 200, 10.0), ("Gamma", 300, 20.0), ("Delta", 400, 30.0)]), true, &mut Alerts::live());
    assert_eq!(ahead(&m), ["Beta", "Gamma", "Delta"], "3 jumps from where it was plotted");
    feed(&mut m, &[jump("2026-10-01T10:05:00Z", "Beta", 200, 10.0).as_str()]);
    assert_eq!(route_names(&m), ["Gamma", "Delta"]);
    assert_eq!(ahead(&m), ["Gamma", "Delta"]);
    feed(&mut m, &[jump("2026-10-01T10:06:00Z", "Gamma", 300, 20.0).as_str()]);
    assert_eq!(route_names(&m), ["Delta"]);
    assert_eq!(ahead(&m), ["Delta"]);
    feed(&mut m, &[jump("2026-10-01T10:07:00Z", "Delta", 400, 30.0).as_str()]);
    assert!(ahead(&m).is_empty());
}

/// NavRoute.json read again in the middle of a route (as after a restart): what's behind is skipped.
#[test]
fn route_read_again_mid_route() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(&mut m, &[jump("2026-10-01T10:05:00Z", "Beta", 200, 10.0).as_str()]);
    m.apply_companion(&nav_route(&[("Alpha", 100, 0.0), ("Beta", 200, 10.0), ("Gamma", 300, 20.0), ("Delta", 400, 30.0)]), true, &mut Alerts::live());
    assert_eq!(ahead(&m), ["Gamma", "Delta"]);
}

/// A jump off the plotted route leaves the route as it was, without the system it was plotted from.
#[test]
fn jump_off_the_route_keeps_it() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    m.apply_companion(&nav_route(&[("Alpha", 100, 0.0), ("Beta", 200, 10.0), ("Gamma", 300, 20.0)]), true, &mut Alerts::live());
    feed(&mut m, &[jump("2026-10-01T10:05:00Z", "Elsewhere", 900, 5.0).as_str()]);
    assert_eq!(route_names(&m), ["Beta", "Gamma"]);
    assert_eq!(ahead(&m), ["Beta", "Gamma"]);
}

/// Touchdown or Liftoff on Alpha 1, by planetary mining location `index`.
fn landing(event: &str, ts: &str, index: u32, lat: f64, lon: f64, player: bool) -> String {
    format!(
        r#"{{ "timestamp":"{ts}", "event":"{event}", "PlayerControlled":{player}, "Taxi":false, "Multicrew":false, "StarSystem":"Alpha", "SystemAddress":100, "Body":"Alpha 1", "BodyID":1, "OnStation":false, "OnPlanet":true, "Latitude":{lat:.6}, "Longitude":{lon:.6}, "NearestDestination":"$SAA_Unknown_Signal:#type=$PlanetaryMiningLocation_Name;:#index={index};" }}"#
    )
}

fn refined(ts: &str, symbol: &str, name: &str) -> String {
    format!(r#"{{ "timestamp":"{ts}", "event":"MiningRefined", "Type":"${symbol}_name;", "Type_Localised":"{name}" }}"#)
}

/// Status.json in the Rhino on Alpha 1 (a 1,000 km body), as the game rewrites it while driving.
fn rhino_at(m: &mut Model, lat: f64, lon: f64) {
    let text = format!(
        r#"{{ "timestamp":"2026-10-01T10:05:00Z", "event":"Status", "Flags":69206016, "Flags2":0, "Latitude":{lat:.6}, "Longitude":{lon:.6}, "Heading":90, "Altitude":0, "BodyName":"Alpha 1", "PlanetRadius":1000000.0 }}"#
    );
    let status = nexus_journal::Companion::parse(nexus_journal::CompanionKind::Status, &text).expect("Status.json");
    m.apply_companion(&status, true, &mut Alerts::live());
}

const RHINO_OUT: &str = r#"{ "timestamp":"2026-10-01T10:04:00Z", "event":"LaunchSRV", "SRVType":"mev_rhino", "SRVType_Localised":"Rhino MEV", "Loadout":"default", "ID":53, "PlayerControlled":true }"#;
const RHINO_IN: &str = r#"{ "timestamp":"2026-10-01T10:30:00Z", "event":"DockSRV", "SRVType":"mev_rhino", "SRVType_Localised":"Rhino MEV", "ID":53 }"#;

/// Chequers2's request (discussion 5): remember the body and the numbered mining location the Rhino worked, with
/// what came out, so the commander can go back. The number comes from the journal (Touchdown/Liftoff), the
/// deposits' positions only from a live Status.json.
#[test]
fn surface_mining_remembers_the_location() {
    use nexus_model::mining::SiteKey;
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let touchdown = landing("Touchdown", "2026-10-01T10:03:00Z", 4, 10.0, 20.0, true);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"FSSBodySignals", "BodyName":"Alpha 1", "BodyID":1, "SystemAddress":100, "Signals":[ { "Type":"$PlanetaryMiningLocation_Name;", "Type_Localised":"Planetary Mining Location", "Count":13 } ] }"#,
            r#"{ "timestamp":"2026-10-01T10:02:00Z", "event":"ApproachBody", "StarSystem":"Alpha", "SystemAddress":100, "Body":"Alpha 1", "BodyID":1 }"#,
            touchdown.as_str(),
            RHINO_OUT,
        ],
    );
    // Two tonnes of Iridium and a by-product at one deposit, then another deposit 2 km away.
    rhino_at(&mut m, 10.01, 20.0);
    feed(&mut m, &[refined("2026-10-01T10:10:00Z", "iridium", "Iridium").as_str(), refined("2026-10-01T10:11:00Z", "iridium", "Iridium").as_str()]);
    rhino_at(&mut m, 10.0103, 20.0);
    feed(&mut m, &[refined("2026-10-01T10:12:00Z", "platinum", "Platinum").as_str()]);
    rhino_at(&mut m, 10.13, 20.0);
    feed(&mut m, &[refined("2026-10-01T10:20:00Z", "iridium", "Iridium").as_str()]);
    let liftoff = landing("Liftoff", "2026-10-01T10:31:00Z", 4, 10.0, 20.0, true);
    feed(&mut m, &[RHINO_IN, liftoff.as_str()]);

    let gs = m.active().unwrap();
    let key = SiteKey { system_address: 100, body_id: 1, location: Some(4) };
    let site = &gs.mining.sites[&key];
    assert_eq!((site.system.as_str(), site.body.as_str(), site.short_body()), ("Alpha", "Alpha 1", "1"));
    assert_eq!(site.by_tonnes(), [("Iridium", 3), ("Platinum", 1)]);
    assert_eq!((site.visits, site.landings.as_slice()), (1, [(10.0, 20.0)].as_slice()));
    assert_eq!(site.spots.len(), 2, "one deposit per place, within 200 m");
    assert_eq!(site.spots[0].refined.len(), 2, "the by-product counts at the deposit it came from");
    assert_eq!(gs.mining.spots_rev, 3, "two deposits and one new commodity: saved three times, not per tonne");
    assert_eq!(gs.mining.session.as_ref().unwrap().sites, [key]);
    assert_eq!(gs.exploration.systems[&100].bodies[&1].mining_locations, 13);

    // The ship's refinery (not in the Rhino) is ring mining: no surface site.
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T10:40:00Z", "event":"SupercruiseEntry", "StarSystem":"Alpha", "SystemAddress":100 }"#]);
    feed(&mut m, &[refined("2026-10-01T10:50:00Z", "painite", "Painite").as_str()]);
    let gs = m.active().unwrap();
    assert!(gs.mining.here.is_none(), "supercruise forgets the location");
    assert_eq!(gs.mining.sites.len(), 1);

    // Days later: landing by location 4 again says so (quietly), and the run counts as a second visit.
    feed(&mut m, &[jump("2026-10-02T09:00:00Z", "Beta", 200, 10.0).as_str(), jump("2026-10-05T09:00:00Z", "Alpha", 100, 0.0).as_str()]);
    let alerts = feed(&mut m, &[landing("Touchdown", "2026-10-05T09:10:00Z", 4, 10.2, 20.1, true).as_str()]);
    let back = alerts.iter().find(|a| a.title == "Back at mining location 4").expect("reminder");
    assert_eq!(back.priority, nexus_model::Priority::Low, "a banner, not spoken unless the voice threshold is Low");
    assert!(back.detail.contains("3 t Iridium, 1 t Platinum"), "{}", back.detail);
    feed(&mut m, &[RHINO_OUT, refined("2026-10-05T09:20:00Z", "iridium", "Iridium").as_str()]);
    let site = &m.active().unwrap().mining.sites[&key];
    assert_eq!((site.visits, site.landings.len()), (2, 2));
    assert_eq!(site.spots.len(), 2, "refined at the second deposit again: no new one");

    // A landing by a location never worked raises nothing; the ship landing itself (recalled) neither.
    let alerts = feed(&mut m, &[landing("Touchdown", "2026-10-05T10:00:00Z", 7, 11.0, 21.0, true).as_str()]);
    assert!(alerts.is_empty());
    let alerts = feed(&mut m, &[landing("Touchdown", "2026-10-05T10:05:00Z", 4, 10.2, 20.1, false).as_str()]);
    assert!(alerts.is_empty(), "the ship landing itself (recalled), in the session that worked it");
}

/// Logging out in the Rhino and back in keeps the location the landing named; logging in on another body drops it.
#[test]
fn surface_mining_location_survives_a_relog_on_the_same_body() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let relog = |body: &str, id: u32| {
        format!(r#"{{ "timestamp":"2026-10-01T11:00:00Z", "event":"Location", "Docked":false, "InSRV":true, "StarSystem":"Alpha", "SystemAddress":100, "StarPos":[0.0,0.0,0.0], "Population":0, "Body":"{body}", "BodyID":{id}, "BodyType":"Planet", "Latitude":10.0, "Longitude":20.0 }}"#)
    };
    feed(&mut m, &[landing("Touchdown", "2026-10-01T10:03:00Z", 4, 10.0, 20.0, true).as_str(), RHINO_OUT, relog("Alpha 1", 1).as_str()]);
    assert_eq!(m.active().unwrap().mining.here.as_ref().and_then(|h| h.key.location), Some(4));
    feed(&mut m, &[relog("Alpha 2", 2).as_str()]);
    assert!(m.active().unwrap().mining.here.is_none());
}

/// Replayed journals (start-up) rebuild the locations and tonnes but never invent positions.
#[test]
fn surface_mining_from_history_has_no_positions() {
    let mut m = Model::default();
    let touchdown = landing("Touchdown", "2026-10-01T10:03:00Z", 4, 10.0, 20.0, true);
    let iridium = refined("2026-10-01T10:10:00Z", "iridium", "Iridium");
    let mut quiet = Alerts::disabled();
    let lines: Vec<&str> = LOGIN.iter().copied().chain([touchdown.as_str(), RHINO_OUT, iridium.as_str()]).collect();
    for line in lines {
        let (ts, name) = scan_line(line).expect("valid journal line");
        let ev = Event::parse(name, line).expect("typed parse");
        m.apply(&EventCtx { ts, name, line, live: false }, &ev, &mut quiet);
    }
    let gs = m.active().unwrap();
    let site = gs.mining.sites.values().next().expect("site from the journal");
    assert_eq!((site.key.location, site.total()), (Some(4), 1));
    assert!(site.spots.is_empty());
    assert_eq!(gs.mining.spots_rev, 0);
}

/// A system is done once every body is found, the planets worth mapping are mapped and every biological signal is analysed.
#[test]
fn fully_explored_needs_scan_map_and_bio() {
    const HIGH_VALUE: i64 = 1_000_000;
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let done = |m: &Model| m.active().unwrap().exploration.systems[&100].fully_explored(HIGH_VALUE);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"Scan", "ScanType":"AutoScan", "BodyName":"Alpha", "BodyID":0, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":0.0, "StarType":"K", "StellarMass":0.7, "WasDiscovered":true, "WasMapped":false }"#,
            r#"{ "timestamp":"2026-10-01T10:02:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 1", "BodyID":1, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":400.0, "PlanetClass":"Earthlike body", "MassEM":1.0, "TerraformState":"", "Landable":false, "WasDiscovered":false, "WasMapped":false }"#,
            r#"{ "timestamp":"2026-10-01T10:03:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 2", "BodyID":2, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":10.0, "PlanetClass":"Rocky body", "MassEM":0.1, "Landable":true, "WasDiscovered":false, "WasMapped":false, "WasFootfalled":false }"#,
            r#"{ "timestamp":"2026-10-01T10:03:10Z", "event":"FSSBodySignals", "BodyName":"Alpha 2", "BodyID":2, "SystemAddress":100, "Signals":[ { "Type":"$SAA_SignalType_Biological;", "Type_Localised":"Biological", "Count":1 } ] }"#,
        ],
    );
    {
        let sys = &m.active().unwrap().exploration.systems[&100];
        assert!(sys.bodies[&1].value.mapped >= HIGH_VALUE, "the ELW is worth mapping");
        assert!(sys.bodies[&2].value.mapped < HIGH_VALUE, "the rocky bio body isn't");
        assert!(!sys.bodies[&2].bio_done());
    }
    assert!(!done(&m), "the FSS hasn't found every body yet");
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T10:04:00Z", "event":"FSSAllBodiesFound", "SystemName":"Alpha", "SystemAddress":100, "Count":3 }"#]);
    assert!(!done(&m), "the ELW isn't mapped");
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T10:10:00Z", "event":"SAAScanComplete", "BodyName":"Alpha 1", "SystemAddress":100, "BodyID":1, "ProbesUsed":5, "EfficiencyTarget":7 }"#]);
    assert!(!done(&m), "biology left on Alpha 2");
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"SAASignalsFound", "BodyName":"Alpha 2", "SystemAddress":100, "BodyID":2, "Signals":[ { "Type":"$SAA_SignalType_Biological;", "Type_Localised":"Biological", "Count":1 } ], "Genuses":[ { "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum" } ] }"#]);
    let organic = |kind: &str, ts: &str| {
        format!(
            r#"{{ "timestamp":"{ts}", "event":"ScanOrganic", "ScanType":"{kind}", "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum", "Species":"$Codex_Ent_Stratum_07_Name;", "Species_Localised":"Stratum Tectonicas", "Variant":"$Codex_Ent_Stratum_07_K_Name;", "Variant_Localised":"Stratum Tectonicas - Lime", "WasLogged":false, "SystemAddress":100, "Body":2 }}"#
        )
    };
    feed(&mut m, &[&organic("Log", "2026-10-01T10:30:00Z"), &organic("Sample", "2026-10-01T10:32:00Z")]);
    assert!(!done(&m), "two samples out of three");
    feed(&mut m, &[&organic("Analyse", "2026-10-01T10:34:00Z")]);
    assert!(m.active().unwrap().exploration.systems[&100].bodies[&2].bio_done());
    assert!(done(&m), "found, mapped and analysed");

    // A star and a worthless icy body: nothing to map, nothing alive, so done as soon as every body is found.
    feed(
        &mut m,
        &[
            jump("2026-10-01T11:00:00Z", "Beta", 200, 10.0).as_str(),
            r#"{ "timestamp":"2026-10-01T11:01:00Z", "event":"Scan", "ScanType":"AutoScan", "BodyName":"Beta", "BodyID":0, "StarSystem":"Beta", "SystemAddress":200, "DistanceFromArrivalLS":0.0, "StarType":"M", "StellarMass":0.3, "WasDiscovered":true, "WasMapped":false }"#,
            r#"{ "timestamp":"2026-10-01T11:02:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Beta 1", "BodyID":1, "StarSystem":"Beta", "SystemAddress":200, "DistanceFromArrivalLS":900.0, "PlanetClass":"Icy body", "MassEM":0.01, "Landable":true, "WasDiscovered":true, "WasMapped":false }"#,
        ],
    );
    let beta = |m: &Model| m.active().unwrap().exploration.systems[&200].fully_explored(HIGH_VALUE);
    assert!(m.active().unwrap().exploration.systems[&200].bodies[&1].value.mapped < HIGH_VALUE);
    assert!(!beta(&m));
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T11:03:00Z", "event":"FSSAllBodiesFound", "SystemName":"Beta", "SystemAddress":200, "Count":2 }"#]);
    assert!(beta(&m), "the icy body isn't worth mapping");
}
