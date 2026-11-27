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

/// Universal Cartographics buys data by the body: sell a system, come back and map one of its planets, and only that
/// mapping is unsold data. Its other bodies stay sold, a probed ring adds nothing (the model gives rings no value), and
/// a death loses only what was new.
#[test]
fn data_mapped_after_a_sale_is_all_that_is_unsold() {
    use nexus_journal::Timestamp;
    use nexus_model::exploration::DataStatus;
    let t = |s: &str| Timestamp::parse(s).expect("time");
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"Scan", "ScanType":"AutoScan", "BodyName":"Alpha", "BodyID":0, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":0.0, "StarType":"K", "StellarMass":0.7, "WasDiscovered":false, "WasMapped":false }"#,
            r#"{ "timestamp":"2026-10-01T10:02:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 1", "BodyID":1, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":400.0, "PlanetClass":"Earthlike body", "MassEM":1.0, "TerraformState":"", "Landable":false, "WasDiscovered":false, "WasMapped":false }"#,
            r#"{ "timestamp":"2026-10-01T10:03:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 2", "BodyID":2, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":10.0, "PlanetClass":"Rocky body", "MassEM":0.1, "Landable":true, "Rings":[ { "Name":"Alpha 2 A Ring", "RingClass":"eRingClass_Icy", "MassMT":1.0e9, "InnerRad":5.0e6, "OuterRad":9.0e6 } ], "WasDiscovered":false, "WasMapped":false, "WasFootfalled":false }"#,
            r#"{ "timestamp":"2026-10-01T10:04:00Z", "event":"FSSAllBodiesFound", "SystemName":"Alpha", "SystemAddress":100, "Count":3 }"#,
        ],
    );
    let ex = |m: &Model| m.active().unwrap().exploration.clone();
    let alpha = |m: &Model| m.active().unwrap().exploration.systems[&100].clone();
    // Never sold: everything scanned is unsold, the full-scan bonus with it, as before.
    assert_eq!(ex(&m).unsold_value(), alpha(&m).value());
    assert!(alpha(&m).value() > alpha(&m).bodies.values().map(|b| b.value.current).sum::<i64>(), "with the full-scan bonus");

    let sell = |m: &mut Model, at: &str| {
        let lines = [
            jump(&format!("2026-10-01T{at}Z"), "Gamma", 300, 30.0),
            format!(r#"{{ "timestamp":"2026-10-01T{at}Z", "event":"MultiSellExplorationData", "Discovered":[ {{ "SystemName":"Alpha", "NumBodies":3 }} ], "BaseValue":300000, "Bonus":400000, "TotalEarnings":700000 }}"#),
        ];
        feed(m, &lines.iter().map(String::as_str).collect::<Vec<_>>());
    };
    sell(&mut m, "10:10:00");
    let sold = t("2026-10-01T10:10:00Z");
    assert_eq!((ex(&m).unsold().count(), alpha(&m).status()), (0, DataStatus::Sold(sold)));

    // Back in Alpha: the star scanned again on arrival and a ring probed are no new data.
    feed(
        &mut m,
        &[
            jump("2026-10-01T11:00:00Z", "Alpha", 100, 0.0).as_str(),
            r#"{ "timestamp":"2026-10-01T11:00:30Z", "event":"Scan", "ScanType":"AutoScan", "BodyName":"Alpha", "BodyID":0, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":0.0, "StarType":"K", "StellarMass":0.7, "WasDiscovered":true, "WasMapped":false }"#,
            r#"{ "timestamp":"2026-10-01T11:05:00Z", "event":"SAAScanComplete", "BodyName":"Alpha 2 A Ring", "SystemAddress":100, "BodyID":3, "ProbesUsed":2, "EfficiencyTarget":0 }"#,
        ],
    );
    assert_eq!((ex(&m).unsold().count(), alpha(&m).status()), (0, DataStatus::Sold(sold)), "still sold");

    // Mapping a planet is new data: that planet alone is unsold, at its mapped value.
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T11:10:00Z", "event":"SAAScanComplete", "BodyName":"Alpha 2", "SystemAddress":100, "BodyID":2, "ProbesUsed":3, "EfficiencyTarget":4 }"#]);
    let rocky = alpha(&m).bodies[&2].value;
    assert_eq!(rocky.current, rocky.mapped);
    assert_eq!(ex(&m).unsold().count(), 1);
    assert_eq!(ex(&m).unsold_value(), rocky.current, "not the star, the Earth-like or the bonus sold before");
    assert_eq!(alpha(&m).unsold_potential_value(), rocky.mapped);
    assert_eq!((alpha(&m).unsold_bodies(), alpha(&m).scanned_bodies()), (1, 3));
    assert_eq!(alpha(&m).status(), DataStatus::SoldThenUnsold(sold), "the sale stays in the history");
    assert!(alpha(&m).value() > rocky.current, "the system's own value is still all of it");

    // Sold again, then the Earth-like mapped: the full-map bonus it completes was not counted again, nor the rest.
    sell(&mut m, "11:20:00");
    let sold = t("2026-10-01T11:20:00Z");
    assert_eq!((ex(&m).unsold().count(), alpha(&m).status()), (0, DataStatus::Sold(sold)));
    feed(
        &mut m,
        &[
            jump("2026-10-01T12:00:00Z", "Alpha", 100, 0.0).as_str(),
            r#"{ "timestamp":"2026-10-01T12:05:00Z", "event":"SAAScanComplete", "BodyName":"Alpha 1", "SystemAddress":100, "BodyID":1, "ProbesUsed":5, "EfficiencyTarget":7 }"#,
        ],
    );
    let earthlike = alpha(&m).bodies[&1].value.current;
    assert_eq!(ex(&m).unsold_value(), earthlike);
    assert_eq!(alpha(&m).status(), DataStatus::SoldThenUnsold(sold));

    // Destroyed: only the Earth-like's mapping is lost; the system was sold and stays so in the history.
    let alerts = feed(&mut m, &[r#"{ "timestamp":"2026-10-01T12:30:00Z", "event":"Died", "KillerName":"Gravity" }"#]);
    let lost = format!("Unsold data lost: about {} Cr", nexus_data::names::compact_credits(earthlike));
    assert!(alerts.iter().any(|a| a.detail == lost), "{lost}: {:?}", alerts.iter().map(|a| &a.detail).collect::<Vec<_>>());
    assert_eq!((ex(&m).unsold().count(), ex(&m).unsold_value()), (0, 0));
    assert_eq!(alpha(&m).status(), DataStatus::SoldThenLost(sold));
    assert_eq!(ex(&m).history().count(), 1);

    // Mapped again after the loss, then sold: sold as a whole once more.
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T13:05:00Z", "event":"SAAScanComplete", "BodyName":"Alpha 1", "SystemAddress":100, "BodyID":1, "ProbesUsed":5, "EfficiencyTarget":7 }"#]);
    assert_eq!((ex(&m).unsold_value(), alpha(&m).status()), (earthlike, DataStatus::SoldThenUnsold(sold)));
    sell(&mut m, "13:30:00");
    let sold = t("2026-10-01T13:30:00Z");
    assert_eq!(alpha(&m).status(), DataStatus::Sold(sold));
    // Its mapping sold, the Earth-like mapped once more is no new data, as a sold scan scanned again isn't.
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T14:05:00Z", "event":"SAAScanComplete", "BodyName":"Alpha 1", "SystemAddress":100, "BodyID":1, "ProbesUsed":5, "EfficiencyTarget":7 }"#]);
    assert_eq!((ex(&m).unsold().count(), alpha(&m).status()), (0, DataStatus::Sold(sold)));

    // A system never sold and lost with the ship is lost as a whole, as before.
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T10:02:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 1", "BodyID":1, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":400.0, "PlanetClass":"Earthlike body", "MassEM":1.0, "TerraformState":"", "Landable":false, "WasDiscovered":false, "WasMapped":false }"#]);
    let whole = alpha(&m).value();
    let alerts = feed(&mut m, &[r#"{ "timestamp":"2026-10-01T12:30:00Z", "event":"Died", "KillerName":"Gravity" }"#]);
    assert!(alerts.iter().any(|a| a.detail == format!("Unsold data lost: about {} Cr", nexus_data::names::compact_credits(whole))));
    assert_eq!(alpha(&m).status(), DataStatus::Lost);
    // Back after the loss: a body scanned for the first time is unsold on its own, not with the Earth-like that was lost.
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T13:02:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 2", "BodyID":2, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":10.0, "PlanetClass":"Rocky body", "MassEM":0.1, "Landable":true, "WasDiscovered":false, "WasMapped":false, "WasFootfalled":false }"#]);
    assert_eq!((alpha(&m).status(), ex(&m).unsold_value()), (DataStatus::Unsold, alpha(&m).bodies[&2].value.current));
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
    let alerts = feed(&mut m, &[&l2]);
    let second = alerts.iter().find(|a| a.title.contains("sample 2 of 3")).expect("second sample alert");
    assert_eq!(second.priority, nexus_model::Priority::Normal);
    assert_eq!(second.detail, "Move at least 500 m before the final sample");
    assert_eq!(second.speech.as_deref(), Some("Second sample collected. Move another 500 metres for the final sample."));
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
fn exobiology_sale_takes_sold_samples_off_the_list() {
    // One organism's three scans: (codex number, display name, body).
    let analyse = |n: &str, name: &str, body: u32, minute: u32| -> Vec<String> {
        ["Log", "Sample", "Analyse"]
            .iter()
            .enumerate()
            .map(|(i, kind)| {
                format!(
                    r#"{{ "timestamp":"2026-10-01T10:{:02}:00Z", "event":"ScanOrganic", "ScanType":"{kind}", "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum", "Species":"$Codex_Ent_Stratum_{n}_Name;", "Species_Localised":"{name}", "Variant":"$Codex_Ent_Stratum_{n}_K_Name;", "Variant_Localised":"{name} - Lime", "WasLogged":true, "SystemAddress":100, "Body":{body} }}"#,
                    minute + i as u32
                )
            })
            .collect()
    };
    let sold = |n: &str, name: &str, value: i64| {
        format!(
            r#"{{ "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum", "Species":"$Codex_Ent_Stratum_{n}_Name;", "Species_Localised":"{name}", "Variant":"$Codex_Ent_Stratum_{n}_K_Name;", "Variant_Localised":"{name} - Lime", "Value":{value}, "Bonus":0 }}"#
        )
    };
    let sale = |ts: &str, items: &[String]| format!(r#"{{ "timestamp":"{ts}", "event":"SellOrganicData", "MarketID":3228018944, "BioData":[ {} ] }}"#, items.join(", "));
    let scan_all = |m: &mut Model| {
        for lines in [analyse("07", "Stratum Tectonicas", 2, 0), analyse("07", "Stratum Tectonicas", 3, 10), analyse("02", "Stratum Paleas", 4, 20)] {
            feed(m, &lines.iter().map(String::as_str).collect::<Vec<_>>());
        }
        assert_eq!(m.active().unwrap().exobio.unsold.len(), 3);
    };

    // The sale names the species as the scans did: what was sold comes off, the other Tectonicas stays.
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    scan_all(&mut m);
    feed(&mut m, &[&sale("2026-10-01T12:00:00Z", &[sold("07", "Stratum Tectonicas", 19_010_800), sold("02", "Stratum Paleas", 1_362_000)])]);
    let bio = &m.active().unwrap().exobio;
    assert_eq!(bio.unsold.iter().map(|r| (r.species.as_str(), r.body_id)).collect::<Vec<_>>(), [("Stratum Tectonicas", 3)]);
    assert_eq!(bio.sold_total, 19_010_800 + 1_362_000);

    // The sale names them differently (another game language): the codex symbol still matches.
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    scan_all(&mut m);
    let all = [sold("07", "Stratum tectonicas (FR)", 19_010_800), sold("07", "Stratum tectonicas (FR)", 19_010_800), sold("02", "Stratum paleas (FR)", 1_362_000)];
    feed(&mut m, &[&sale("2026-10-01T12:00:00Z", &all)]);
    let bio = &m.active().unwrap().exobio;
    assert!(bio.unsold.is_empty(), "every sold sample comes off the list, whatever its display name");
    assert_eq!(bio.unsold_value(), 0);
    assert_eq!(bio.sold_total, 2 * 19_010_800 + 1_362_000);
    assert_eq!(bio.sales.len(), 1);

    // A sale without the symbol (not seen from the game) still matches on the name.
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    scan_all(&mut m);
    let no_symbol = r#"{ "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Species_Localised":"Stratum Paleas", "Value":1362000, "Bonus":0 }"#.to_string();
    feed(&mut m, &[&sale("2026-10-01T12:00:00Z", &[no_symbol])]);
    let bio = &m.active().unwrap().exobio;
    assert_eq!(bio.unsold.len(), 2);
    assert!(bio.unsold.iter().all(|r| r.species == "Stratum Tectonicas"));

    // Death loses unsold data: the list empties, and a later sale of new samples works as before.
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    scan_all(&mut m);
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T11:00:00Z", "event":"Died", "KillerName":"Gravity" }"#]);
    assert!(m.active().unwrap().exobio.unsold.is_empty());
    feed(&mut m, &analyse("02", "Stratum Paleas", 4, 30).iter().map(String::as_str).collect::<Vec<_>>());
    feed(&mut m, &[&sale("2026-10-01T12:00:00Z", &[sold("02", "Stratum paleas (FR)", 1_362_000)])]);
    let bio = &m.active().unwrap().exobio;
    assert!(bio.unsold.is_empty());
    assert_eq!(bio.sold_total, 1_362_000);
}

/// A Stratum analysed (its third scan) on 1 October: the codex number (07 Tectonicas, 02 Paleas), body and time.
fn stratum_analysed(n: &str, body: u32, time: &str) -> String {
    let name = if n == "07" { "Stratum Tectonicas" } else { "Stratum Paleas" };
    format!(
        r#"{{ "timestamp":"2026-10-01T{time}Z", "event":"ScanOrganic", "ScanType":"Analyse", "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum", "Species":"$Codex_Ent_Stratum_{n}_Name;", "Species_Localised":"{name}", "Variant":"$Codex_Ent_Stratum_{n}_K_Name;", "Variant_Localised":"{name} - Lime", "WasLogged":true, "SystemAddress":100, "Body":{body} }}"#
    )
}

/// A sale at Vista Genomics on 1 October of these Stratum samples (codex numbers); none: the game listed no samples.
fn stratum_sale(time: &str, sold: &[&str]) -> String {
    let items: Vec<String> = sold
        .iter()
        .map(|n| format!(r#"{{ "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum", "Species":"$Codex_Ent_Stratum_{n}_Name;", "Value":1000000, "Bonus":0 }}"#))
        .collect();
    format!(r#"{{ "timestamp":"2026-10-01T{time}Z", "event":"SellOrganicData", "MarketID":3228018944, "BioData":[ {} ] }}"#, items.join(", "))
}

fn ts(time: &str) -> nexus_journal::Timestamp {
    nexus_journal::Timestamp::parse(&format!("2026-10-01T{time}Z")).expect("time")
}

/// The species and analysis times of the unsold list, oldest first.
fn unsold_list(m: &Model) -> Vec<(String, nexus_journal::Timestamp)> {
    m.active().unwrap().exobio.unsold.iter().map(|r| (r.species.clone(), r.ts)).collect()
}

/// GitHub issue 20: the game logged sales at Vista Genomics with an empty sample list. Such a sale is remembered, takes
/// nothing off the list (nothing says what it sold), says so in the feed, and is doubted only for samples analysed
/// before it. Death loses the samples, and the doubt with them.
#[test]
fn exobiology_sale_logged_without_its_samples() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(&mut m, &[&stratum_analysed("07", 2, "10:10:00"), &stratum_analysed("02", 4, "10:20:00"), &stratum_sale("12:00:00", &[])]);
    let gs = m.active().unwrap();
    let bio = &gs.exobio;
    assert_eq!(bio.unsold.len(), 2, "nothing comes off");
    assert_eq!(bio.unlisted_sales, [ts("12:00:00")]);
    assert_eq!(bio.doubtful_sales().collect::<Vec<_>>(), [ts("12:00:00")]);
    assert_eq!(bio.clear_point(), Some(ts("12:00:00")), "the newest of the samples and the unlisted sales");
    assert_eq!((bio.sold_total, bio.cleared), (0, None));
    assert_eq!(gs.feed.back().map(|f| f.text.as_str()), Some("Sold exobiology data: the game logged the sale without its samples or their value"));
    // A listed sale says what it paid, as before.
    feed(&mut m, &[&stratum_sale("12:01:00", &["02"])]);
    let gs = m.active().unwrap();
    assert_eq!(gs.feed.back().map(|f| f.text.as_str()), Some("Sold exobiology data: 1,000,000 Cr"));
    assert_eq!(unsold_list(&m), [("Stratum Tectonicas".to_string(), ts("10:10:00"))]);

    // Death loses the samples; a sample analysed afterwards can't have been sold in the earlier sale.
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T13:00:00Z", "event":"Died", "KillerName":"Gravity" }"#]);
    let bio = &m.active().unwrap().exobio;
    assert!(bio.unsold.is_empty() && bio.unlisted_sales.is_empty());
    feed(&mut m, &[&stratum_analysed("07", 2, "14:00:00")]);
    assert_eq!(m.active().unwrap().exobio.doubtful_sales().count(), 0);

    // Nor is a sale made before every sample listed was analysed in doubt.
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(&mut m, &[&stratum_sale("09:00:00", &[]), &stratum_analysed("07", 2, "10:10:00")]);
    let bio = &m.active().unwrap().exobio;
    assert_eq!(bio.unlisted_sales.len(), 1);
    assert_eq!(bio.doubtful_sales().count(), 0);
}

/// Clear list (Exobiology) holds when the journals are read again: the model starts with the cleared time, so the
/// samples analysed up to it never enter the list, the unlisted sales up to it aren't remembered, and a later sale takes
/// the right sample off. Cleared live, the list is the same as read again.
#[test]
fn cleared_unsold_exobiology_list_holds_on_replay() {
    let before = [stratum_analysed("07", 2, "10:10:00"), stratum_analysed("02", 4, "10:20:00"), stratum_sale("12:00:00", &[])];
    let after = [stratum_analysed("07", 5, "13:00:00"), stratum_analysed("07", 6, "13:10:00"), stratum_sale("14:00:00", &["07"])];
    fn lines(l: &[String]) -> Vec<&str> {
        l.iter().map(String::as_str).collect()
    }

    // Cleared live, after the sale the game logged without its samples: the list empties at once.
    let mut live = Model::default();
    feed(&mut live, LOGIN);
    feed(&mut live, &lines(&before));
    let upto = live.active().unwrap().exobio.clear_point().expect("something to clear");
    live.clear_exobio_unsold("F1", upto);
    let bio = &live.active().unwrap().exobio;
    assert!(bio.unsold.is_empty() && bio.unlisted_sales.is_empty());
    assert_eq!((bio.cleared, live.exobio_cleared.get("F1")), (Some(ts("12:00:00")), Some(&ts("12:00:00"))));
    // A Clear never goes back.
    live.clear_exobio_unsold("F1", ts("11:00:00"));
    assert_eq!(live.active().unwrap().exobio.cleared, Some(ts("12:00:00")));
    // Two new Tectonicas, one sold: the older of the two comes off, not the cleared one from before.
    feed(&mut live, &lines(&after));
    assert_eq!(unsold_list(&live), [("Stratum Tectonicas".to_string(), ts("13:10:00"))]);

    // Elite Nexus starts again: settings.json gives the model the cleared time before the journals are read.
    let mut replayed = Model::default().with_exobio_cleared([("F1".to_string(), upto)].into());
    feed(&mut replayed, LOGIN);
    feed(&mut replayed, &lines(&before));
    let bio = &replayed.active().unwrap().exobio;
    assert!(bio.unsold.is_empty() && bio.unlisted_sales.is_empty(), "nothing up to the clear, and no doubt");
    assert_eq!(bio.doubtful_sales().count(), 0);
    feed(&mut replayed, &lines(&after));
    assert_eq!(unsold_list(&replayed), unsold_list(&live));
    assert_eq!(replayed.active().unwrap().exobio.sold_total, 1_000_000, "sold so far counts the listed sale");

    // Another commander's clear isn't this one's.
    let mut other = Model::default().with_exobio_cleared([("F2".to_string(), upto)].into());
    feed(&mut other, LOGIN);
    feed(&mut other, &lines(&before));
    assert_eq!(other.active().unwrap().exobio.unsold.len(), 2);
}

/// Why the clear acts while the journals are read, not on what's shown: an old Tectonicas A cleared, a new one B, and
/// a sale of one. Read without the clear, the sale takes A off and leaves B, so a list that only hid A would show B as
/// unsold although it was sold.
#[test]
fn cleared_exobiology_sample_isnt_taken_for_a_new_one() {
    let lines = [stratum_analysed("07", 2, "10:10:00"), stratum_analysed("07", 3, "11:00:00"), stratum_sale("12:00:00", &["07"])];
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    let mut hidden_only = Model::default();
    feed(&mut hidden_only, LOGIN);
    feed(&mut hidden_only, &lines);
    assert_eq!(unsold_list(&hidden_only), [("Stratum Tectonicas".to_string(), ts("11:00:00"))]);

    let mut m = Model::default().with_exobio_cleared([("F1".to_string(), ts("10:10:00"))].into());
    feed(&mut m, LOGIN);
    feed(&mut m, &lines);
    assert!(unsold_list(&m).is_empty(), "the sale took B, the only Tectonicas listed");
}

#[test]
fn exobiology_scan_in_another_language_keeps_value_and_colony_range() {
    // The display names are in the game's language (made up here); the codex symbols are the same in every language.
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let organic = |kind: &str, ts: &str| {
        format!(
            r#"{{ "timestamp":"{ts}", "event":"ScanOrganic", "ScanType":"{kind}", "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Strate (FR)", "Species":"$Codex_Ent_Stratum_07_Name;", "Species_Localised":"Strate tectonique (FR)", "Variant":"$Codex_Ent_Stratum_07_K_Name;", "Variant_Localised":"Strate tectonique (FR) - Citron vert", "WasLogged":true, "SystemAddress":100, "Body":2 }}"#
        )
    };
    let alerts = feed(&mut m, &[&organic("Log", "2026-10-01T10:10:00Z")]);
    assert!(alerts.iter().any(|a| a.detail.contains("500 m")), "Stratum colony range, found by symbol");
    let cur = m.active().unwrap().exobio.current.clone().expect("sampling");
    assert_eq!((cur.colony_range_m, cur.value), (Some(500), Some(19_010_800)));
    feed(&mut m, &[&organic("Sample", "2026-10-01T10:12:00Z"), &organic("Analyse", "2026-10-01T10:14:00Z")]);
    let rec = &m.active().unwrap().exobio.unsold[0];
    assert_eq!(rec.species, "Strate tectonique (FR)", "the list shows the game's own name");
    assert_eq!(rec.value, 19_010_800);
}

/// The genera a DSS scan reveals are named in the game's language, but the exobiology data is English: predictions
/// take each genus by its codex symbol, so every language gets the predictions English gets, and English gets what it
/// got when they were taken by name.
#[test]
fn dss_genera_predict_the_same_in_every_language() {
    use nexus_data::bio_predict as bp;
    let body_with = |stratum: &str, bacterium: &str, unknown: &str| {
        let mut m = Model::default();
        feed(&mut m, LOGIN);
        let dss = format!(
            r#"{{ "timestamp":"2026-10-01T10:20:00Z", "event":"SAASignalsFound", "BodyName":"Alpha 2", "SystemAddress":100, "BodyID":2, "Signals":[ {{ "Type":"$SAA_SignalType_Biological;", "Type_Localised":"Biological", "Count":3 }} ], "Genuses":[ {{ "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"{stratum}" }}, {{ "Genus":"$Codex_Ent_Bacterial_Genus_Name;", "Genus_Localised":"{bacterium}" }}, {{ "Genus":"$Codex_Ent_NotInTheData_Genus_Name;", "Genus_Localised":"{unknown}" }} ] }}"#
        );
        feed(
            &mut m,
            &[
                r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"Scan", "ScanType":"AutoScan", "BodyName":"Alpha", "BodyID":0, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":0.0, "StarType":"K", "Subclass":3, "StellarMass":0.7, "Luminosity":"V", "WasDiscovered":true, "WasMapped":false }"#,
                r#"{ "timestamp":"2026-10-01T10:03:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 2", "BodyID":2, "Parents":[ { "Star":0 } ], "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":10.0, "PlanetClass":"Rocky body", "Atmosphere":"thin carbon dioxide atmosphere", "AtmosphereType":"CarbonDioxide", "AtmosphereComposition":[ { "Name":"CarbonDioxide", "Percent":99.0 }, { "Name":"SulphurDioxide", "Percent":1.0 } ], "Volcanism":"", "MassEM":0.1, "Radius":2000000.0, "SurfaceGravity":2.5, "SurfaceTemperature":180.0, "SurfacePressure":3000.0, "Landable":true, "WasDiscovered":false, "WasMapped":false, "WasFootfalled":false }"#,
                &dss,
            ],
        );
        m
    };
    let body = |m: &Model| m.active().unwrap().exploration.systems[&100].bodies[&2].clone();
    let english = body_with("Stratum", "Bacterium", "Novelty");
    let german = body_with("Stratum (DE)", "Bakterium", "Neuheit");
    let (en, de) = (body(&english), body(&german));

    // The names stay as the game wrote them, for showing.
    assert_eq!(de.genuses, ["Stratum (DE)", "Bakterium", "Neuheit"]);
    assert_eq!(de.dss_genus(1).map(|g| g.name), Some("Bacterium"));
    assert!(de.dss_genus(2).is_none(), "a genus the data doesn't know");

    // English: exactly what the names gave before.
    let sys = &english.active().unwrap().exploration.systems[&100];
    let facts = nexus_model::exploration::body_facts(&en, sys, None).expect("facts");
    assert_eq!(en.predictions, bp::predict(&facts, &en.genuses), "English predictions unchanged");
    assert!(en.predictions.iter().any(|p| p.genus == "Stratum" && p.value > 0));
    assert!(en.predictions.iter().any(|p| p.species == "Novelty (unknown species)"));

    // Another language: the same species, values and chances as English; the unknown genus keeps the game's name.
    let strip = |b: &nexus_model::exploration::Body| -> Vec<(String, i64, u32)> {
        b.predictions.iter().filter(|p| !p.species.ends_with("(unknown species)")).map(|p| (p.species.clone(), p.value, (p.likelihood * 1000.0) as u32)).collect()
    };
    assert!(!strip(&de).is_empty());
    assert_eq!(strip(&de), strip(&en));
    assert!(de.predictions.iter().any(|p| p.species == "Neuheit (unknown species)"));
    assert_eq!(de.predicted_value(false), en.predicted_value(false));

    // What taking them by name did in another language: no species, nothing to earn.
    let sys = &german.active().unwrap().exploration.systems[&100];
    let by_name = bp::predict(&nexus_model::exploration::body_facts(&de, sys, None).expect("facts"), &de.genuses);
    assert!(by_name.iter().all(|p| p.species.ends_with("(unknown species)") && p.value == 0));
}

/// A codex entry tells the system's galactic region, which rules some species in or out. The journal names the region
/// in the game's language, but the exobiology data knows it in English: predictions take it by its symbol, so every
/// language gets the predictions English gets. The Codex tab keeps the game's own name.
#[test]
fn codex_region_predicts_the_same_in_every_language() {
    use nexus_data::bio_predict as bp;
    // A Tussock on a thin-ammonia rock under a K star: three of its species grow only in some regions.
    let body_in = |region: Option<&str>| {
        let mut m = Model::default();
        feed(&mut m, LOGIN);
        feed(
            &mut m,
            &[
                r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"Scan", "ScanType":"AutoScan", "BodyName":"Alpha", "BodyID":0, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":0.0, "StarType":"K", "Subclass":3, "StellarMass":0.7, "Luminosity":"Vab", "WasDiscovered":true, "WasMapped":false }"#,
                r#"{ "timestamp":"2026-10-01T10:03:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 3", "BodyID":3, "Parents":[ { "Star":0 } ], "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":777.0, "PlanetClass":"Rocky body", "Atmosphere":"thin ammonia atmosphere", "AtmosphereType":"Ammonia", "AtmosphereComposition":[ { "Name":"Ammonia", "Percent":100.0 } ], "Volcanism":"", "MassEM":0.01, "Radius":1500000.0, "SurfaceGravity":0.563, "SurfaceTemperature":167.06, "SurfacePressure":347.5, "Landable":true, "Materials":[ { "Name":"iron", "Percent":19.6 }, { "Name":"cadmium", "Percent":1.5 }, { "Name":"ruthenium", "Percent":1.2 }, { "Name":"tin", "Percent":1.2 } ], "WasDiscovered":false, "WasMapped":false, "WasFootfalled":false }"#,
                r#"{ "timestamp":"2026-10-01T10:05:00Z", "event":"SAASignalsFound", "BodyName":"Alpha 3", "SystemAddress":100, "BodyID":3, "Signals":[ { "Type":"$SAA_SignalType_Biological;", "Type_Localised":"Biological", "Count":1 } ], "Genuses":[ { "Genus":"$Codex_Ent_Tussocks_Genus_Name;", "Genus_Localised":"Tussock" } ] }"#,
            ],
        );
        if let Some(region) = region {
            let codex = format!(
                r#"{{ "timestamp":"2026-10-01T10:06:00Z", "event":"CodexEntry", "EntryID":1400160, "Name":"$Codex_Ent_Standard_Rocky_Ammonia_Name;", "Name_Localised":"Rocky ammonia world", "SubCategory":"$Codex_SubCategory_Terrestrials;", "SubCategory_Localised":"Terrestrial planets", "Category":"$Codex_Category_StellarBodies;", "Category_Localised":"Astronomical Bodies", "Region":"$Codex_RegionName_18;", "Region_Localised":"{region}", "System":"Alpha", "SystemAddress":100, "BodyID":3, "IsNewEntry":false }}"#
            );
            feed(&mut m, &[&codex]);
        }
        m
    };
    let body = |m: &Model| m.active().unwrap().exploration.systems[&100].bodies[&3].clone();
    let english = body_in(Some("Inner Orion Spur"));
    let other = body_in(Some("Innerer Orionsporn (DE)"));
    let unknown = body_in(None);
    let (en, de, none) = (body(&english), body(&other), body(&unknown));
    assert_ne!(en.predictions, none.predictions, "the region decides between species here");

    // English: what the region's name gave before.
    let sys = &english.active().unwrap().exploration.systems[&100];
    let facts = nexus_model::exploration::body_facts(&en, sys, Some("Inner Orion Spur".into())).expect("facts");
    assert_eq!(en.predictions, bp::predict(&facts, &en.genera_for_prediction()), "English predictions unchanged");

    // Another language: the same species, chances and values as English.
    assert_eq!(de.predictions, en.predictions);
    assert_eq!(de.predicted_value(false), en.predicted_value(false));
    let ex = &other.active().unwrap().exploration;
    assert_eq!(ex.region_of("Alpha").as_deref(), Some("Inner Orion Spur"), "for predictions and Ask");
    assert_eq!(ex.codex[0].region, "Innerer Orionsporn (DE)", "the Codex tab shows the game's own name");
}

/// Guardian and Thargoid signals on a body tag it as interesting (Exploration, Dashboard) by the signal's symbol, so the
/// tag shows in every language; the body lists the signals as the game names them.
#[test]
fn guardian_and_thargoid_signals_are_tagged_in_every_language() {
    use nexus_model::exploration::{interesting, Interest};
    let tags_for = |kind: &str, label: &str| {
        let mut m = Model::default();
        feed(&mut m, LOGIN);
        let dss = format!(
            r#"{{ "timestamp":"2026-10-01T10:05:00Z", "event":"SAASignalsFound", "BodyName":"Alpha 2", "SystemAddress":100, "BodyID":2, "Signals":[ {{ "Type":"$SAA_SignalType_Geological;", "Type_Localised":"Geological (made up)", "Count":2 }}, {{ "Type":"{kind}", "Type_Localised":"{label}", "Count":3 }} ] }}"#
        );
        feed(
            &mut m,
            &[
                r#"{ "timestamp":"2026-10-01T10:03:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 2", "BodyID":2, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":10.0, "PlanetClass":"Rocky body", "MassEM":0.1, "Landable":true, "WasDiscovered":true, "WasMapped":true, "WasFootfalled":true }"#,
                &dss,
            ],
        );
        let sys = m.active().unwrap().exploration.systems[&100].clone();
        let body = &sys.bodies[&2];
        assert!(body.signals.iter().any(|(k, n)| k == label && *n == 3), "listed as the game names it: {:?}", body.signals);
        interesting(body, &sys)
    };
    for (kind, english, other) in [("$SAA_SignalType_Guardian;", "Guardian", "Uralte Zeichen (DE)"), ("$SAA_SignalType_Thargoid;", "Thargoid", "Fremde Zeichen (DE)")] {
        assert!(tags_for(kind, english).contains(&Interest::GuardianOrThargoid), "{english}, as before");
        assert!(tags_for(kind, other).contains(&Interest::GuardianOrThargoid), "{other}");
    }
    assert!(!tags_for("$SAA_SignalType_Human;", "Human").contains(&Interest::GuardianOrThargoid));
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

/// A promotion mid-session leaves the new rank's progress unknown until the next login reports it, and a squadron
/// promotion brings the new rank's name.
#[test]
fn promotions_drop_the_old_progress_and_rank_name() {
    use nexus_data::ranks::RankKind;
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:00:05Z", "event":"Rank", "Combat":3, "Trade":4, "Explore":6, "Soldier":0, "Exobiologist":2, "Empire":7, "Federation":1, "CQC":0 }"#,
            r#"{ "timestamp":"2026-10-01T10:00:06Z", "event":"Progress", "Combat":30, "Trade":46, "Explore":97, "Soldier":0, "Exobiologist":35, "Empire":37, "Federation":100, "CQC":0 }"#,
            r#"{ "timestamp":"2026-10-01T10:00:07Z", "event":"SquadronStartup", "SquadronName":"VANGUARDS OF TEST", "SquadronID":42, "CurrentRank":4, "CurrentRankName":"$Squadron_DefaultRankName_Rank4;", "CurrentRankName_Localised":"Rookie" }"#,
            r#"{ "timestamp":"2026-10-01T11:00:00Z", "event":"Promotion", "Explore":7 }"#,
            r#"{ "timestamp":"2026-10-01T11:05:00Z", "event":"SquadronPromotion", "SquadronID":42, "SquadronName":"VANGUARDS OF TEST", "OldRank":4, "NewRank":3, "OldRankName":"$Squadron_DefaultRankName_Rank4;", "OldRankName_Localised":"Rookie", "NewRankName":"$Squadron_DefaultRankName_Rank3;", "NewRankName_Localised":"Agent" }"#,
        ],
    );
    let c = &m.active().unwrap().cmdr;
    assert_eq!(c.rank_name(RankKind::Explore), "Pioneer");
    assert_eq!(c.rank_progress(RankKind::Explore), None, "97% was progress towards Pioneer");
    assert_eq!(c.rank_progress(RankKind::Trade), Some(46), "the other ranks keep theirs");
    let sq = c.squadron.as_ref().unwrap();
    assert_eq!((sq.rank, sq.rank_name.as_deref()), (Some(3), Some("Agent")));
    // The next login reports progress again; a squadron rank with no name has none rather than the old one's.
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-02T10:00:06Z", "event":"Progress", "Combat":30, "Trade":46, "Explore":4, "Soldier":0, "Exobiologist":35, "Empire":37, "Federation":100, "CQC":0 }"#,
            r#"{ "timestamp":"2026-10-02T11:05:00Z", "event":"SquadronPromotion", "SquadronID":42, "SquadronName":"VANGUARDS OF TEST", "OldRank":3, "NewRank":2 }"#,
        ],
    );
    let c = &m.active().unwrap().cmdr;
    assert_eq!(c.rank_progress(RankKind::Explore), Some(4));
    let sq = c.squadron.as_ref().unwrap();
    assert_eq!((sq.rank, sq.rank_name.as_deref()), (Some(2), None));
}

/// The ship you fly stays where you stepped out of it while an Apex taxi or another commander's ship takes you
/// elsewhere; a transferred ship has arrived once its time is up.
#[test]
fn the_ship_stays_where_you_left_it() {
    use nexus_model::fleet::ShipPlace;
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let place = |m: &Model| {
        let gs = m.active().unwrap();
        gs.ship_place(&gs.fleet.ships[&22])
    };
    let left_at = |m: &Model| match place(m) {
        ShipPlace::Left(l) => l.text(),
        other => panic!("{other:?}"),
    };
    let docked = |ts: &str, taxi: bool, system: &str, station: &str, market: u64| {
        format!(r#"{{ "timestamp":"{ts}", "event":"Docked", "StationName":"{station}", "StationType":"Coriolis", "Taxi":{taxi}, "Multicrew":false, "StarSystem":"{system}", "SystemAddress":100, "MarketID":{market}, "DistFromStarLS":10.0 }}"#)
    };
    let step = |event: &str, ts: &str, taxi: bool, system: &str, station: &str, market: u64| {
        format!(r#"{{ "timestamp":"{ts}", "event":"{event}", "SRV":false, "Taxi":{taxi}, "Multicrew":false, "StarSystem":"{system}", "SystemAddress":100, "Body":"{station}", "BodyID":3, "OnStation":true, "OnPlanet":false, "StationName":"{station}", "StationType":"Coriolis", "MarketID":{market} }}"#)
    };
    let lines = [
        docked("2026-10-01T10:01:00Z", false, "Alpha", "Alpha Port", 5001),
        step("Disembark", "2026-10-01T10:02:00Z", false, "Alpha", "Alpha Port", 5001),
    ];
    feed(&mut m, &lines.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(place(&m), ShipPlace::WithYou, "on foot beside it");
    let lines = [
        step("Embark", "2026-10-01T10:05:00Z", true, "Alpha", "Alpha Port", 5001),
        r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"FSDJump", "Taxi":true, "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":0, "JumpDist":10.0, "FuelUsed":0.0, "FuelLevel":0.0 }"#.to_string(),
        docked("2026-10-01T10:25:00Z", true, "Beta", "Beta Hub", 5002),
        step("Disembark", "2026-10-01T10:26:00Z", true, "Beta", "Beta Hub", 5002),
    ];
    feed(&mut m, &lines.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(m.active().unwrap().location.system, "Beta");
    assert_eq!(left_at(&m).as_deref(), Some("Alpha Port (Alpha)"), "the taxi took you, not your ship");
    // Back by taxi, and aboard: with you again.
    let lines = [
        step("Embark", "2026-10-01T10:30:00Z", true, "Beta", "Beta Hub", 5002),
        r#"{ "timestamp":"2026-10-01T10:45:00Z", "event":"FSDJump", "Taxi":true, "StarSystem":"Alpha", "SystemAddress":100, "StarPos":[0.0,0.0,0.0], "Population":0, "JumpDist":10.0, "FuelUsed":0.0, "FuelLevel":0.0 }"#.to_string(),
        docked("2026-10-01T10:50:00Z", true, "Alpha", "Alpha Port", 5001),
        step("Disembark", "2026-10-01T10:51:00Z", true, "Alpha", "Alpha Port", 5001),
        step("Embark", "2026-10-01T10:52:00Z", false, "Alpha", "Alpha Port", 5001),
    ];
    feed(&mut m, &lines.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(place(&m), ShipPlace::WithYou);
    // Crewing another commander's ship: yours stays docked at Alpha Port until you're back aboard it.
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T11:00:00Z", "event":"JoinACrew", "Captain":"Bob", "Telepresence":true }"#,
            r#"{ "timestamp":"2026-10-01T11:01:00Z", "event":"Location", "Docked":false, "Multicrew":true, "StarSystem":"Gamma", "SystemAddress":300, "StarPos":[20.0,0.0,0.0], "Population":0, "Body":"Gamma", "BodyID":0, "BodyType":"Star" }"#,
        ],
    );
    assert_eq!(left_at(&m).as_deref(), Some("Alpha Port (Alpha)"));
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T11:30:00Z", "event":"QuitACrew", "Captain":"Bob", "Telepresence":true }"#,
            r#"{ "timestamp":"2026-10-01T11:30:05Z", "event":"Location", "Docked":true, "Multicrew":false, "Taxi":false, "StationName":"Alpha Port", "StationType":"Coriolis", "MarketID":5001, "StarSystem":"Alpha", "SystemAddress":100, "StarPos":[0.0,0.0,0.0], "Population":0, "Body":"Alpha Port", "BodyID":3, "BodyType":"Station" }"#,
        ],
    );
    assert_eq!(place(&m), ShipPlace::WithYou);
    // In a taxi with no record of where you left your ship: away, somewhere the journal doesn't say.
    let mut fresh = Model::default();
    feed(&mut fresh, LOGIN);
    feed(&mut fresh, &[r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"FSDJump", "Taxi":true, "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":0 }"#]);
    assert_eq!(place(&fresh), ShipPlace::Away);

    // A transfer: in transit for its 10 minutes, then there, though only a later StoredShips says so.
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T12:00:00Z", "event":"StoredShips", "StationName":"Alpha Port", "MarketID":5001, "StarSystem":"Alpha", "ShipsHere":[ ], "ShipsRemote":[ { "ShipID":5, "ShipType":"python", "Name":"Mule", "StarSystem":"Beta", "ShipMarketID":5002, "TransferPrice":1000, "TransferTime":600, "Value":55000000, "Hot":false } ] }"#,
            r#"{ "timestamp":"2026-10-01T12:01:00Z", "event":"ShipyardTransfer", "ShipType":"python", "ShipID":5, "System":"Beta", "ShipMarketID":5002, "Distance":10.0, "TransferPrice":1000, "TransferTime":600, "MarketID":5001 }"#,
        ],
    );
    let gs = m.active().unwrap();
    let ShipPlace::Stored(l) = gs.ship_place(&gs.fleet.ships[&5]) else { panic!() };
    assert_eq!(l.text().as_deref(), Some("Alpha Port (Alpha)"));
    let t = |s: &str| nexus_journal::Timestamp::parse(s).unwrap();
    assert!(l.in_transit_at(t("2026-10-01T12:05:00Z")));
    assert!(!l.in_transit_at(t("2026-10-01T12:11:01Z")), "arrived after its 600 s");
}

/// Suit loadouts follow what the game says happened to them: deleted ones go, renames and weapon changes show.
#[test]
fn suit_loadouts_follow_deletions_renames_and_weapons() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:10:00Z", "event":"SuitLoadout", "SuitID":17, "SuitName":"tacticalsuit_class3", "SuitName_Localised":"$TacticalSuit_Class1_Name;", "SuitMods":[], "LoadoutID":4293000001, "LoadoutName":"Fighter", "Modules":[ { "SlotName":"PrimaryWeapon1", "SuitModuleID":1, "ModuleName":"wpn_m_assaultrifle_plasma_fauto", "ModuleName_Localised":"Manticore Tormentor", "Class":3, "WeaponMods":[] }, { "SlotName":"SecondaryWeapon", "SuitModuleID":2, "ModuleName":"wpn_s_pistol_kinetic_sauto", "ModuleName_Localised":"Karma P-15", "Class":1, "WeaponMods":[] } ] }"#,
            r#"{ "timestamp":"2026-10-01T10:11:00Z", "event":"CreateSuitLoadout", "SuitID":18, "SuitName":"explorationsuit_class1", "SuitName_Localised":"Artemis Suit", "SuitMods":[], "LoadoutID":4293000002, "LoadoutName":"Loadout 2", "Modules":[] }"#,
            r#"{ "timestamp":"2026-10-01T10:12:00Z", "event":"RenameSuitLoadout", "SuitID":17, "SuitName":"tacticalsuit_class3", "SuitName_Localised":"$TacticalSuit_Class1_Name;", "LoadoutID":4293000001, "LoadoutName":"Assault" }"#,
            r#"{ "timestamp":"2026-10-01T10:13:00Z", "event":"LoadoutEquipModule", "LoadoutName":"Assault", "SuitID":17, "SuitName":"tacticalsuit_class3", "SuitName_Localised":"Dominator Suit", "LoadoutID":4293000001, "SlotName":"PrimaryWeapon2", "ModuleName":"wpn_m_sniper_plasma_charged", "ModuleName_Localised":"Manticore Executioner", "Class":4, "WeaponMods":["weapon_stability"], "SuitModuleID":3 }"#,
            r#"{ "timestamp":"2026-10-01T10:14:00Z", "event":"LoadoutRemoveModule", "LoadoutName":"Assault", "SuitID":17, "SuitName":"tacticalsuit_class3", "SuitName_Localised":"Dominator Suit", "LoadoutID":4293000001, "SlotName":"SecondaryWeapon", "ModuleName":"wpn_s_pistol_kinetic_sauto", "ModuleName_Localised":"Karma P-15", "Class":1, "WeaponMods":[], "SuitModuleID":2 }"#,
            r#"{ "timestamp":"2026-10-01T10:15:00Z", "event":"DeleteSuitLoadout", "SuitID":18, "SuitName":"explorationsuit_class1", "SuitName_Localised":"Artemis Suit", "LoadoutID":4293000002, "LoadoutName":"Loadout 2" }"#,
            // A loadout never listed: a single weapon change doesn't make it up.
            r#"{ "timestamp":"2026-10-01T10:16:00Z", "event":"LoadoutEquipModule", "LoadoutName":"Ghost", "SuitID":19, "SuitName":"utilitysuit_class1", "LoadoutID":4293000009, "SlotName":"PrimaryWeapon1", "ModuleName":"wpn_m_shotgun_plasma_doublebarrel", "ModuleName_Localised":"Manticore Intimidator", "Class":2, "WeaponMods":[], "SuitModuleID":4 }"#,
        ],
    );
    let inv = &m.active().unwrap().inventory;
    assert_eq!(inv.suit_loadouts.keys().copied().collect::<Vec<_>>(), [4293000001], "the deleted and the unknown loadouts aren't listed");
    let l = &inv.suit_loadouts[&4293000001];
    assert_eq!((l.name.as_str(), l.suit_name.as_str()), ("Assault", "Dominator Suit"), "renamed; the game's placeholder isn't shown as the suit's name");
    let weapons: Vec<(&str, &str, u32)> = l.weapons.iter().map(|w| (w.0.as_str(), w.1.as_str(), w.2)).collect();
    assert_eq!(weapons, [("PrimaryWeapon1", "Manticore Tormentor", 3), ("PrimaryWeapon2", "Manticore Executioner", 4)]);
    assert_eq!(l.weapons[1].3, ["weapon_stability"]);
    assert_eq!(inv.current_suit_loadout, Some(4293000001));
    // Deleting the loadout in use leaves none in use.
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T10:17:00Z", "event":"DeleteSuitLoadout", "SuitID":17, "SuitName":"tacticalsuit_class3", "LoadoutID":4293000001, "LoadoutName":"Assault" }"#]);
    let inv = &m.active().unwrap().inventory;
    assert!(inv.suit_loadouts.is_empty() && inv.current_suit_loadout.is_none());
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

/// Handed-in vouchers reach BGS & Powerplay (each faction's share, in the system they're handed in) and Statistics (the
/// credits received). A bounty voucher lists its factions with `Amount`, where the `Bounty` event says `Reward`: read as
/// a `Reward`, every share came out 0 and no bounty reached the BGS page.
#[test]
fn redeemed_vouchers_count_for_the_bgs_and_the_ledger() {
    use nexus_journal::Timestamp;
    use nexus_model::bgs::BgsKind;
    use nexus_model::history::LedgerCategory;
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[
            // Bounties for two factions and one with no name (the game writes that too), then a conflict zone's bonds.
            r#"{ "timestamp":"2026-10-01T10:30:00Z", "event":"RedeemVoucher", "Type":"bounty", "Amount":155000, "Factions":[ { "Faction":"Alpha Jet Group", "Amount":120000 }, { "Faction":"", "Amount":5000 }, { "Faction":"Alpha Patron's Party", "Amount":30000 } ] }"#,
            r#"{ "timestamp":"2026-10-01T10:31:00Z", "event":"RedeemVoucher", "Type":"CombatBond", "Amount":80000, "Faction":"Alpha Jet Group" }"#,
            // A codex voucher pays too; the BGS page counts bounties and bonds.
            r#"{ "timestamp":"2026-10-01T10:32:00Z", "event":"RedeemVoucher", "Type":"codex", "Amount":25000, "Faction":"" }"#,
        ],
    );
    let gs = m.active().expect("commander");
    let summary = gs.bgs.summary(Timestamp::ZERO);
    let found: Vec<_> = summary.iter().flat_map(|s| s.factions.iter().map(|f| (s.system.as_str(), f.faction.as_str(), f.kinds.clone()))).collect();
    assert_eq!(
        found,
        [
            ("Alpha", "Alpha Jet Group", vec![(BgsKind::Bounty, 120_000, 1), (BgsKind::CombatBond, 80_000, 1)]),
            ("Alpha", "Alpha Patron's Party", vec![(BgsKind::Bounty, 30_000, 1)]),
        ]
    );
    let earned = |cat| gs.history.totals_since(Timestamp::ZERO).into_iter().find(|(c, _, _)| *c == cat).map(|(_, i, _)| i);
    assert_eq!(earned(LedgerCategory::Bounties), Some(155_000), "Statistics counts what was paid out");
    assert_eq!(earned(LedgerCategory::CombatBonds), Some(80_000));
    assert_eq!(earned(LedgerCategory::Exploration), Some(25_000));
}

/// A sale of cartographic data counts for the station's faction in either form the journal writes it. Journals from
/// older versions of the game name the station's faction as a plain string when you dock, and the sale is a
/// `SellExplorationData`; newer ones give the faction with its state, and the sale is a `MultiSellExplorationData`.
#[test]
fn both_forms_of_a_data_sale_count_for_the_bgs() {
    use nexus_journal::Timestamp;
    use nexus_model::bgs::BgsKind;
    use nexus_model::history::LedgerCategory;
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[
            // An older journal: docked, then the sale.
            r#"{ "timestamp":"2018-05-12T18:30:01Z", "event":"Docked", "StationName":"Alpha Hub", "StationType":"Coriolis", "StarSystem":"Alpha", "MarketID":128000002, "StationFaction":"Alpha Jet Group", "FactionState":"Boom", "StationGovernment":"$government_Corporate;", "StationGovernment_Localised":"Corporate", "StationEconomy":"$economy_Industrial;", "StationEconomy_Localised":"Industrial", "DistFromStarLS":12.0 }"#,
            r#"{ "timestamp":"2018-05-12T18:31:00Z", "event":"SellExplorationData", "Systems":[ "Beta", "Gamma" ], "Discovered":[ "Beta 1" ], "BaseValue":40000, "Bonus":5000, "TotalEarnings":45000 }"#,
            r#"{ "timestamp":"2018-05-12T18:35:00Z", "event":"Undocked", "StationName":"Alpha Hub", "StationType":"Coriolis" }"#,
            // A newer one: another station of the system, another faction.
            r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"Docked", "StationName":"Alpha Port", "StationType":"Orbis", "StarSystem":"Alpha", "SystemAddress":100, "MarketID":128000003, "StationFaction":{ "Name":"Alpha Patron's Party", "FactionState":"None" }, "StationServices":[ "dock", "exploration" ], "DistFromStarLS":40.0 }"#,
            r#"{ "timestamp":"2026-10-01T10:22:00Z", "event":"MultiSellExplorationData", "Discovered":[ { "SystemName":"Delta", "NumBodies":3 } ], "BaseValue":100000, "Bonus":20000, "TotalEarnings":120000 }"#,
        ],
    );
    let gs = m.active().expect("commander");
    let summary = gs.bgs.summary(Timestamp::ZERO);
    let found: Vec<_> = summary.iter().flat_map(|s| s.factions.iter().map(|f| (s.system.as_str(), f.faction.as_str(), f.kinds.clone()))).collect();
    assert_eq!(
        found,
        [
            ("Alpha", "Alpha Jet Group", vec![(BgsKind::Exploration, 45_000, 1)]),
            ("Alpha", "Alpha Patron's Party", vec![(BgsKind::Exploration, 120_000, 1)]),
        ]
    );
    let earned = gs.history.totals_since(Timestamp::ZERO).into_iter().find(|(c, _, _)| *c == LedgerCategory::Exploration).map(|(_, i, _)| i);
    assert_eq!(earned, Some(165_000), "the ledger counted both already");
    assert!(gs.feed.iter().any(|f| f.event == "SellExplorationData" && f.text == "Sold cartographic data: 45,000 Cr"), "and the activity feed lists it");
    assert_eq!(gs.history.docks.iter().map(|d| d.station.as_str()).collect::<Vec<_>>(), ["Alpha Hub", "Alpha Port"], "both dockings are in the history");
}

/// `PowerplayMerits` names no system: each award goes to the system the commander was in when the game wrote it,
/// and the period's total stays the sum of the awards.
#[test]
fn merits_are_placed_where_the_commander_was() {
    use nexus_journal::Timestamp;
    let merits = |ts: &str, gained: i64, total: i64| {
        format!(r#"{{ "timestamp":"{ts}", "event":"PowerplayMerits", "Power":"Aisling Duval", "MeritsGained":{gained}, "TotalMerits":{total} }}"#)
    };
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let lines = [
        merits("2026-10-01T10:10:00Z", 100, 1100),
        r#"{ "timestamp":"2026-10-01T10:30:00Z", "event":"FSDJump", "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":5000, "JumpDist":10.0, "FuelUsed":1.0, "FuelLevel":63.0 }"#.to_string(),
        merits("2026-10-01T10:40:00Z", 30, 1130),
        r#"{ "timestamp":"2026-10-01T11:00:00Z", "event":"Shutdown" }"#.to_string(),
        // The next session: an award written before the game says where the commander is goes where they logged out.
        r#"{ "timestamp":"2026-10-02T09:00:00Z", "event":"LoadGame", "FID":"F1", "Commander":"Test", "Horizons":true, "Odyssey":true, "Ship":"explorer_nx", "ShipID":22, "GameMode":"Solo", "Credits":1000000, "Loan":0 }"#.to_string(),
        merits("2026-10-02T09:00:01Z", 10, 1140),
        r#"{ "timestamp":"2026-10-02T09:00:05Z", "event":"Location", "Docked":false, "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0], "Population":5000 }"#.to_string(),
    ];
    feed(&mut m, &lines.iter().map(String::as_str).collect::<Vec<_>>());
    let gs = m.active().expect("commander");
    let per_system: Vec<(String, i64)> = gs.bgs.summary(Timestamp::ZERO).into_iter().map(|s| (s.system, s.merits)).collect();
    assert_eq!(per_system, [("Alpha".to_string(), 100), ("Beta".to_string(), 40)]);
    assert_eq!(gs.bgs.merits_since(Timestamp::ZERO), 140, "the period's total is the sum of the awards");
    assert_eq!(gs.bgs.merits[1].system_address, 200);
    assert_eq!(gs.cmdr.powerplay.as_ref().map(|p| p.merits), Some(1140));
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

fn docking(event: &str, ts: &str, extra: &str) -> String {
    format!(r#"{{ "timestamp":"{ts}", "event":"{event}", "MarketID":3700000000, "StationName":"Q2K-71N", "StationType":"FleetCarrier"{extra} }}"#)
}

/// Play-test, 2026-11-04: a denied request showed on the overlay, the granted one a few seconds later showed nothing.
/// Both raise an alert now, and the pad is kept until the ship docks.
#[test]
fn docking_granted_and_denied_both_say_so_and_the_pad_stays_until_docked() {
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let denied = feed(&mut m, &[docking("DockingDenied", "2026-10-01T10:01:00Z", r#", "Reason":"Distance""#).as_str()]);
    assert_eq!(denied.len(), 1);
    assert_eq!((denied[0].title.as_str(), denied[0].detail.as_str()), ("Docking denied", "Q2K-71N: too far away: ask again within 7.5 km"));
    assert!(m.active().unwrap().location.docking.is_none());
    let granted = feed(&mut m, &[docking("DockingGranted", "2026-10-01T10:01:20Z", r#", "LandingPad":12"#).as_str()]);
    assert_eq!((granted[0].title.as_str(), granted[0].detail.as_str()), ("Docking granted", "Pad 12 · Q2K-71N"));
    assert_eq!(granted[0].priority, denied[0].priority, "shown alike");
    let grant = m.active().unwrap().location.docking.clone().expect("the pad is kept");
    assert_eq!((grant.pad, grant.market_id), (Some(12), 3_700_000_000));
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T10:02:30Z", "event":"Docked", "StationName":"Q2K-71N", "StationType":"FleetCarrier", "StarSystem":"Alpha", "SystemAddress":100, "MarketID":3700000000 }"#]);
    assert!(m.active().unwrap().location.docking.is_none(), "docked: no pad to look for");
    feed(&mut m, &[docking("DockingGranted", "2026-10-01T11:00:00Z", r#", "LandingPad":3"#).as_str(), docking("DockingTimeout", "2026-10-01T11:05:00Z", "").as_str()]);
    assert!(m.active().unwrap().location.docking.is_none(), "the permission ran out");
    feed(&mut m, &[docking("DockingGranted", "2026-10-01T11:10:00Z", r#", "LandingPad":3"#).as_str(), r#"{ "timestamp":"2026-10-01T11:10:30Z", "event":"SupercruiseEntry", "StarSystem":"Alpha", "SystemAddress":100 }"#]);
    assert!(m.active().unwrap().location.docking.is_none(), "left for supercruise");
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
    assert_eq!(site.spots[0].points, [(10.01, 20.0)], "refines 5 m apart are one place");
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

/// One way Chequers2's runs (discussion 5) could have come out "location not named". Dismissed from the Rhino, the
/// ship lifts off with no position and, recalled, lands with none: the location the landing named holds for the whole
/// run. Lifting off yourself leaves it: a Rhino dropped later from the hovering ship, or after landing far from every
/// location, isn't named.
#[test]
fn surface_mining_keeps_the_location_when_the_ship_is_dismissed() {
    use nexus_model::mining::SiteKey;
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let unpiloted = |event: &str, ts: &str, nearest: Option<&str>| {
        let nearest = nearest.map(|n| format!(r#", "NearestDestination":"{n}""#)).unwrap_or_default();
        format!(r#"{{ "timestamp":"{ts}", "event":"{event}", "PlayerControlled":false, "Taxi":false, "Multicrew":false, "StarSystem":"Alpha", "SystemAddress":100, "Body":"Alpha 1", "BodyID":1, "OnStation":false, "OnPlanet":true{nearest} }}"#)
    };
    let touchdown = landing("Touchdown", "2026-10-01T10:03:00Z", 9, 10.0, 20.0, true);
    let dismissed = unpiloted("Liftoff", "2026-10-01T10:05:00Z", None);
    let recalled = unpiloted("Touchdown", "2026-10-01T10:40:00Z", Some(""));
    let (first, second) = (refined("2026-10-01T10:10:00Z", "iridium", "Iridium"), refined("2026-10-01T10:45:00Z", "iridium", "Iridium"));
    feed(&mut m, &[touchdown.as_str(), RHINO_OUT, dismissed.as_str(), first.as_str(), recalled.as_str(), second.as_str()]);
    let named = SiteKey { system_address: 100, body_id: 1, location: Some(9) };
    let sites: Vec<SiteKey> = m.active().unwrap().mining.sites.keys().copied().collect();
    assert_eq!(sites, [named], "both tonnes at mining location 9");
    assert_eq!(m.active().unwrap().mining.sites[&named].total(), 2);

    // Lifted off and flew on: the Rhino dropped from the hovering ship (no landing at all) isn't credited to location 9.
    let liftoff = landing("Liftoff", "2026-10-01T11:00:00Z", 9, 10.0, 20.0, true);
    let third = refined("2026-10-01T11:20:00Z", "iridium", "Iridium");
    feed(&mut m, &[RHINO_IN, liftoff.as_str(), RHINO_OUT, third.as_str()]);
    let unnamed = SiteKey { location: None, ..named };
    assert_eq!(m.active().unwrap().mining.sites.keys().copied().collect::<Vec<_>>(), [unnamed, named]);
    // Nor is a run after setting down somewhere the game names no location.
    let far = r#"{ "timestamp":"2026-10-01T11:40:00Z", "event":"Touchdown", "PlayerControlled":true, "Taxi":false, "Multicrew":false, "StarSystem":"Alpha", "SystemAddress":100, "Body":"Alpha 1", "BodyID":1, "OnStation":false, "OnPlanet":true, "Latitude":-30.0, "Longitude":100.0, "NearestDestination":"" }"#;
    let fourth = refined("2026-10-01T11:50:00Z", "iridium", "Iridium");
    feed(&mut m, &[RHINO_IN, far, RHINO_OUT, fourth.as_str()]);
    assert_eq!((m.active().unwrap().mining.sites[&unnamed].total(), m.active().unwrap().mining.sites.len()), (2, 2));
}

/// A Rhino dropped from a hovering ship (out of glide and under 30 m) has no Touchdown, so the game names
/// no location: the run takes the planetary mining location targeted then (Status.json, live) and records it. Read
/// again on a later start with the recorded drops, the journal names the run the same; without them (runs from before),
/// it isn't named. A ship flown down far from every location takes nothing from a target.
#[test]
fn surface_mining_names_a_hover_drop_from_the_target() {
    use nexus_model::mining::SiteKey;
    let status = |m: &mut Model, destination: Option<&str>| {
        let destination = destination.map(|n| format!(r#", "Destination":{{ "System":100, "Body":1, "Name":"{n}" }}"#)).unwrap_or_default();
        let text = format!(
            r#"{{ "timestamp":"2026-10-01T10:02:30Z", "event":"Status", "Flags":18874368, "Flags2":0, "Latitude":10.0, "Longitude":20.0, "Heading":90, "Altitude":25, "BodyName":"Alpha 1", "PlanetRadius":1000000.0{destination} }}"#
        );
        let st = nexus_journal::Companion::parse(nexus_journal::CompanionKind::Status, &text).expect("Status.json");
        m.apply_companion(&st, true, &mut Alerts::live());
    };
    let launch = |ts: &str| format!(r#"{{ "timestamp":"{ts}", "event":"LaunchSRV", "SRVType":"mev_rhino", "SRVType_Localised":"Rhino MEV", "Loadout":"default", "ID":53, "PlayerControlled":true }}"#);
    let replay = |m: &mut Model, lines: &[&str]| {
        let mut quiet = Alerts::disabled();
        for line in lines {
            let (ts, name) = scan_line(line).expect("valid journal line");
            let ev = Event::parse(name, line).expect("typed parse");
            m.apply(&EventCtx { ts, name, line, live: false }, &ev, &mut quiet);
        }
    };
    let signals = r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"FSSBodySignals", "BodyName":"Alpha 1", "BodyID":1, "SystemAddress":100, "Signals":[ { "Type":"$PlanetaryMiningLocation_Name;", "Type_Localised":"Planetary Mining Location", "Count":13 } ] }"#;
    let approach = r#"{ "timestamp":"2026-10-01T10:02:00Z", "event":"ApproachBody", "StarSystem":"Alpha", "SystemAddress":100, "Body":"Alpha 1", "BodyID":1 }"#;
    let (out, iridium) = (launch("2026-10-01T10:04:00Z"), refined("2026-10-01T10:10:00Z", "iridium", "Iridium"));
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(&mut m, &[signals, approach]);
    status(&mut m, Some("$SAA_Unknown_Signal:#type=$PlanetaryMiningLocation_Name;:#index=6;"));
    status(&mut m, None);
    feed(&mut m, &[out.as_str(), iridium.as_str()]);
    let six = SiteKey { system_address: 100, body_id: 1, location: Some(6) };
    let gs = m.active().unwrap();
    assert_eq!(gs.mining.sites.keys().copied().collect::<Vec<_>>(), [six], "the target holds though deselected on arrival");
    assert_eq!((gs.mining.drops.values().copied().collect::<Vec<_>>(), gs.mining.drops_rev), (vec![six], 1));
    let drops = gs.mining.drops.clone();

    // A second drop from the same hover with no record (Elite Nexus wasn't running): the ship may have flown on.
    let (out2, more) = (launch("2026-10-01T10:35:00Z"), refined("2026-10-01T10:36:00Z", "iridium", "Iridium"));
    let journal: Vec<&str> = LOGIN.iter().copied().chain([signals, approach, out.as_str(), iridium.as_str(), RHINO_IN, out2.as_str(), more.as_str()]).collect();
    let mut again = Model::with_mining_drops(drops);
    replay(&mut again, &journal);
    let unnamed = SiteKey { location: None, ..six };
    let tonnes = |m: &Model| m.active().unwrap().mining.sites.iter().map(|(k, s)| (*k, s.total())).collect::<Vec<_>>();
    assert_eq!(tonnes(&again), [(unnamed, 1), (six, 1)], "read again: the recorded drop named, the other not");
    let mut before = Model::default();
    replay(&mut before, &journal);
    assert_eq!(tonnes(&before), [(unnamed, 2)], "no drop recorded");

    // Live, back aboard the hovering ship and out again: the target still holds and the new drop is recorded too.
    feed(&mut m, &[RHINO_IN, launch("2026-10-01T10:30:30Z").as_str()]);
    assert_eq!((m.active().unwrap().mining.drops.len(), m.active().unwrap().mining.here.as_ref().map(|h| h.key)), (2, Some(six)));

    // Back aboard, off and over to another location: targeted the bare way Status.json may write it, on a body whose
    // scans found mining locations.
    let liftoff = landing("Liftoff", "2026-10-01T10:31:00Z", 6, 10.0, 20.0, true);
    feed(&mut m, &[RHINO_IN, liftoff.as_str()]);
    status(&mut m, Some("$SAA_Unknown_Signal:#index=8;"));
    feed(&mut m, &[launch("2026-10-01T10:40:00Z").as_str(), refined("2026-10-01T10:45:00Z", "iridium", "Iridium").as_str()]);
    let eight = SiteKey { location: Some(8), ..six };
    assert!(m.active().unwrap().mining.sites.contains_key(&eight));

    // Flown down far from every location (the game named none within 50 km): a target doesn't place the Rhino.
    let far = r#"{ "timestamp":"2026-10-01T11:00:00Z", "event":"Touchdown", "PlayerControlled":true, "Taxi":false, "Multicrew":false, "StarSystem":"Alpha", "SystemAddress":100, "Body":"Alpha 1", "BodyID":1, "OnStation":false, "OnPlanet":true, "Latitude":-30.0, "Longitude":100.0 }"#;
    feed(&mut m, &[r#"{ "timestamp":"2026-10-01T10:50:00Z", "event":"DockSRV", "SRVType":"mev_rhino", "ID":53 }"#, far]);
    status(&mut m, Some("$SAA_Unknown_Signal:#type=$PlanetaryMiningLocation_Name;:#index=6;"));
    feed(&mut m, &[launch("2026-10-01T11:01:00Z").as_str(), refined("2026-10-01T11:05:00Z", "iridium", "Iridium").as_str()]);
    let gs = m.active().unwrap();
    assert_eq!((gs.mining.sites[&unnamed].total(), gs.mining.drops.len()), (1, 3));
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

/// A system is done once every body is found, the planets worth mapping are mapped and every biological signal is
/// analysed: each of the three is needed, so each is checked as the one still missing.
#[test]
fn fully_explored_needs_scan_map_and_bio() {
    const HIGH_VALUE: i64 = 1_000_000;
    let scans = [
        r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"Scan", "ScanType":"AutoScan", "BodyName":"Alpha", "BodyID":0, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":0.0, "StarType":"K", "StellarMass":0.7, "WasDiscovered":true, "WasMapped":false }"#,
        r#"{ "timestamp":"2026-10-01T10:02:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 1", "BodyID":1, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":400.0, "PlanetClass":"Earthlike body", "MassEM":1.0, "TerraformState":"", "Landable":false, "WasDiscovered":false, "WasMapped":false }"#,
        r#"{ "timestamp":"2026-10-01T10:03:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 2", "BodyID":2, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":10.0, "PlanetClass":"Rocky body", "MassEM":0.1, "Landable":true, "WasDiscovered":false, "WasMapped":false, "WasFootfalled":false }"#,
        r#"{ "timestamp":"2026-10-01T10:03:10Z", "event":"FSSBodySignals", "BodyName":"Alpha 2", "BodyID":2, "SystemAddress":100, "Signals":[ { "Type":"$SAA_SignalType_Biological;", "Type_Localised":"Biological", "Count":1 } ] }"#,
    ];
    let organic = |kind: &str, ts: &str| {
        format!(
            r#"{{ "timestamp":"{ts}", "event":"ScanOrganic", "ScanType":"{kind}", "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum", "Species":"$Codex_Ent_Stratum_07_Name;", "Species_Localised":"Stratum Tectonicas", "Variant":"$Codex_Ent_Stratum_07_K_Name;", "Variant_Localised":"Stratum Tectonicas - Lime", "WasLogged":false, "SystemAddress":100, "Body":2 }}"#
        )
    };
    let (log, sample, analyse) = (organic("Log", "2026-10-01T10:30:00Z"), organic("Sample", "2026-10-01T10:32:00Z"), organic("Analyse", "2026-10-01T10:34:00Z"));
    let found = [r#"{ "timestamp":"2026-10-01T10:04:00Z", "event":"FSSAllBodiesFound", "SystemName":"Alpha", "SystemAddress":100, "Count":3 }"#];
    let mapped = [r#"{ "timestamp":"2026-10-01T10:10:00Z", "event":"SAAScanComplete", "BodyName":"Alpha 1", "SystemAddress":100, "BodyID":1, "ProbesUsed":5, "EfficiencyTarget":7 }"#];
    let analysed = [
        r#"{ "timestamp":"2026-10-01T10:20:00Z", "event":"SAASignalsFound", "BodyName":"Alpha 2", "SystemAddress":100, "BodyID":2, "Signals":[ { "Type":"$SAA_SignalType_Biological;", "Type_Localised":"Biological", "Count":1 } ], "Genuses":[ { "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum" } ] }"#,
        log.as_str(),
        sample.as_str(),
        analyse.as_str(),
    ];
    let alpha = |m: &Model| m.active().unwrap().exploration.systems[&100].fully_explored(HIGH_VALUE);
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(&mut m, &scans);
    {
        let sys = &m.active().unwrap().exploration.systems[&100];
        assert!(sys.bodies[&1].value.mapped >= HIGH_VALUE, "the ELW is worth mapping");
        assert!(sys.bodies[&2].value.mapped < HIGH_VALUE, "the rocky bio body isn't");
        assert!(!sys.bodies[&2].bio_done());
    }
    feed(&mut m, &analysed[..3]);
    assert!(!m.active().unwrap().exploration.systems[&100].bodies[&2].bio_done(), "two samples out of three");
    feed(&mut m, &analysed[3..]);
    assert!(m.active().unwrap().exploration.systems[&100].bodies[&2].bio_done());

    // Two of the three done, then the third.
    let steps: [(&[&str], &str); 3] = [(&found, "the FSS hasn't found every body"), (&mapped, "the ELW isn't mapped"), (&analysed, "biology left on Alpha 2")];
    for (last, (lines, missing)) in steps.iter().enumerate() {
        let mut m = Model::default();
        feed(&mut m, LOGIN);
        feed(&mut m, &scans);
        for (i, (other, _)) in steps.iter().enumerate() {
            if i != last {
                feed(&mut m, other);
            }
        }
        assert!(!alpha(&m), "{missing}");
        feed(&mut m, lines);
        assert!(alpha(&m), "found, mapped and analysed");
    }

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

/// Each biological signal is a genus. Brain Trees can grow several species on one body, so two of them analysed still
/// leave the second genus, Stratum, to do.
#[test]
fn bio_done_counts_genera_not_species() {
    let analyse = |ts: &str, genus: &str, genus_name: &str, species: &str, species_name: &str| {
        format!(
            r#"{{ "timestamp":"{ts}", "event":"ScanOrganic", "ScanType":"Analyse", "Genus":"{genus}", "Genus_Localised":"{genus_name}", "Species":"{species}", "Species_Localised":"{species_name}", "WasLogged":false, "SystemAddress":100, "Body":3 }}"#
        )
    };
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 3", "BodyID":3, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":20.0, "PlanetClass":"Rocky body", "MassEM":0.05, "Landable":true, "Volcanism":"minor silicate vapour geysers volcanism", "WasDiscovered":false, "WasMapped":false, "WasFootfalled":false }"#,
            r#"{ "timestamp":"2026-10-01T10:05:00Z", "event":"SAASignalsFound", "BodyName":"Alpha 3", "SystemAddress":100, "BodyID":3, "Signals":[ { "Type":"$SAA_SignalType_Biological;", "Type_Localised":"Biological", "Count":2 } ], "Genuses":[ { "Genus":"$Codex_Ent_Brancae_Name;", "Genus_Localised":"Brain Trees" }, { "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum" } ] }"#,
        ],
    );
    let done = |m: &Model| m.active().unwrap().exploration.systems[&100].bodies[&3].bio_done();
    let brain_trees = [
        analyse("2026-10-01T10:20:00Z", "$Codex_Ent_Brancae_Name;", "Brain Trees", "$Codex_Ent_SeedABCD_01_Name;", "Gypseeum Brain Tree"),
        analyse("2026-10-01T10:40:00Z", "$Codex_Ent_Brancae_Name;", "Brain Trees", "$Codex_Ent_SeedABCD_02_Name;", "Ostrinum Brain Tree"),
    ];
    feed(&mut m, &[brain_trees[0].as_str(), brain_trees[1].as_str()]);
    let body = &m.active().unwrap().exploration.systems[&100].bodies[&3];
    assert_eq!(body.organics.iter().filter(|o| o.analysed).count(), 2, "two species analysed");
    assert!(!done(&m), "one genus of two");
    feed(&mut m, &[analyse("2026-10-01T11:00:00Z", "$Codex_Ent_Stratum_Genus_Name;", "Stratum", "$Codex_Ent_Stratum_07_Name;", "Stratum Tectonicas").as_str()]);
    assert!(done(&m), "both genera");

    // A game in another language names the genus in it. A species the exobiology data knows and one it doesn't (one
    // added since) of the same genus are still one genus.
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    feed(
        &mut m,
        &[
            r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"Scan", "ScanType":"Detailed", "BodyName":"Alpha 3", "BodyID":3, "StarSystem":"Alpha", "SystemAddress":100, "DistanceFromArrivalLS":20.0, "PlanetClass":"Rocky body", "MassEM":0.05, "Landable":true, "WasDiscovered":false, "WasMapped":false, "WasFootfalled":false }"#,
            r#"{ "timestamp":"2026-10-01T10:05:00Z", "event":"FSSBodySignals", "BodyName":"Alpha 3", "BodyID":3, "SystemAddress":100, "Signals":[ { "Type":"$SAA_SignalType_Biological;", "Type_Localised":"Biologisch", "Count":2 } ] }"#,
        ],
    );
    feed(
        &mut m,
        &[
            analyse("2026-10-01T10:20:00Z", "$Codex_Ent_Brancae_Name;", "Hirnbäume", "$Codex_Ent_SeedABCD_01_Name;", "Gypseeum-Hirnbaum").as_str(),
            analyse("2026-10-01T10:40:00Z", "$Codex_Ent_Brancae_Name;", "Hirnbäume", "$Codex_Ent_SeedZZZZ_09_Name;", "Neuer Hirnbaum").as_str(),
        ],
    );
    assert!(!done(&m), "one genus of two, in any language");
}

#[test]
fn hyperspace_announces_once_at_normal_priority_before_arrival() {
    use nexus_journal::companion::flags;
    use nexus_journal::{Companion, CompanionKind};
    use nexus_model::{AlertKind, Priority};

    let start = r#"{ "timestamp":"2026-10-01T10:01:00Z", "event":"StartJump", "JumpType":"Hyperspace", "StarSystem":"Beta", "StarClass":"F" }"#;
    let arrived = r#"{ "timestamp":"2026-10-01T10:01:20Z", "event":"FSDJump", "StarSystem":"Beta", "SystemAddress":200, "StarPos":[10.0,0.0,0.0] }"#;
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let status = |m: &mut Model, time: &str, extra: u32, live: bool| {
        let flags = flags::IN_MAIN_SHIP | extra;
        let text = format!(r#"{{"timestamp":"{time}","event":"Status","Flags":{flags},"Flags2":0}}"#);
        let st = Companion::parse(CompanionKind::Status, &text).expect("Status.json");
        let mut alerts = if live { Alerts::live() } else { Alerts::disabled() };
        m.apply_companion(&st, live, &mut alerts);
        alerts.items
    };
    assert!(feed(&mut m, &[start]).is_empty(), "countdown is still silent");
    assert!(status(&mut m, "2026-10-01T10:01:04Z", flags::FSD_CHARGING, true).is_empty());

    // A startup snapshot updates the state silently. The first live confirmation can announce it.
    assert!(status(&mut m, "2026-10-01T10:01:06Z", flags::FSD_JUMP, false).is_empty());
    let alerts = status(&mut m, "2026-10-01T10:01:07Z", flags::FSD_JUMP, true);
    assert_eq!(alerts.len(), 1);
    assert_eq!((alerts[0].kind, alerts[0].priority), (AlertKind::Navigation, Priority::Normal));
    assert_eq!(alerts[0].speech.as_deref(), Some("Arriving at Beta. This system has a scoopable star."));
    assert!(status(&mut m, "2026-10-01T10:01:09Z", flags::FSD_JUMP, true).is_empty());
    let alerts = feed(&mut m, &[arrived]);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].title, "Arrived in Beta");
    assert!(alerts[0].speech.is_none(), "arrival is visual only");

    // A delayed journal event can follow the matching status, and must not use the previous jump's flag.
    let next = r#"{ "timestamp":"2026-10-01T10:02:00Z", "event":"StartJump", "JumpType":"Hyperspace", "StarSystem":"Gamma", "StarClass":"L" }"#;
    assert!(status(&mut m, "2026-10-01T10:02:06Z", flags::FSD_JUMP, true).is_empty());
    let alerts = feed(&mut m, &[next]);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].speech.as_deref(), Some("Arriving at Gamma. This system has a brown dwarf star. The arrival star is not scoopable."));

    for (class, description) in [
        ("N", "This system has a neutron star. The arrival star is not scoopable."),
        ("DA", "This system has a white dwarf star. The arrival star is not scoopable."),
        ("M", "This system has a scoopable star."),
    ] {
        let mut model = Model::default();
        feed(&mut model, LOGIN);
        let start = start.replace("\"F\"", &format!("\"{class}\""));
        feed(&mut model, &[&start]);
        let alerts = status(&mut model, "2026-10-01T10:01:08Z", flags::FSD_JUMP, true);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].speech.as_deref(), Some(format!("Arriving at Beta. {description}").as_str()));
    }
}

#[test]
fn navigation_voice_follows_map_targets_and_live_route() {
    use nexus_journal::companion::flags;
    use nexus_journal::{Companion, CompanionKind};
    use nexus_model::Priority;
    let mut m = Model::default();
    feed(&mut m, LOGIN);
    let status = |m: &mut Model, ts: &str, extra: u32, focus: u32| {
        let flags = flags::IN_MAIN_SHIP | extra;
        let json = format!(r#"{{"timestamp":"{ts}","event":"Status","Flags":{flags},"GuiFocus":{focus},"Fuel":{{"FuelMain":20.0,"FuelReservoir":0.5}}}}"#);
        let st = Companion::parse(CompanionKind::Status, &json).expect("Status.json");
        let mut alerts = Alerts::live();
        m.apply_companion(&st, true, &mut alerts);
        alerts.items
    };
    let target = r#"{ "timestamp":"2026-10-01T10:01:01Z", "event":"FSDTarget", "Name":"Beta", "SystemAddress":200, "StarClass":"F" }"#;
    status(&mut m, "2026-10-01T10:01:00Z", 0, 6);
    let alerts = feed(&mut m, &[target, target]);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].priority, Priority::Normal);
    assert_eq!(alerts[0].speech.as_deref(), Some("Set course to Beta."));

    m.apply_companion(&nav_route(&[("Alpha", 100, 0.0), ("Beta", 200, 10.0), ("Gamma", 300, 20.0)]), true, &mut Alerts::live());
    let ship = m.active_mut().unwrap().fleet.current_mut().expect("own ship");
    ship.fuel_main_capacity = 64.0;
    ship.modules.push(nexus_model::fleet::ShipModule { item: "int_fuelscoop_size4_class5".into(), on: true, ..Default::default() });

    status(&mut m, "2026-10-01T10:01:02Z", 0, 0);
    let start = r#"{ "timestamp":"2026-10-01T10:01:03Z", "event":"StartJump", "JumpType":"Hyperspace", "StarSystem":"Beta", "SystemAddress":200, "StarClass":"F" }"#;
    let automatic_target = r#"{ "timestamp":"2026-10-01T10:01:06Z", "event":"FSDTarget", "Name":"Gamma", "SystemAddress":300, "StarClass":"DA" }"#;
    assert!(feed(&mut m, &[start, automatic_target]).is_empty());
    let alerts = status(&mut m, "2026-10-01T10:01:07Z", flags::FSD_JUMP, 0);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].speech.as_deref(), Some("Arriving at Beta. This system has a scoopable star. One jump remaining after this. Fuel is below half. Consider refuelling here."));

    feed(&mut m, &[&jump("2026-10-01T10:01:17Z", "Beta", 200, 10.0)]);
    let start = r#"{ "timestamp":"2026-10-01T10:02:00Z", "event":"StartJump", "JumpType":"Hyperspace", "StarSystem":"Gamma", "SystemAddress":300, "StarClass":"M" }"#;
    assert!(feed(&mut m, &[start]).is_empty(), "the previous jump status is stale");
    let alerts = status(&mut m, "2026-10-01T10:02:07Z", flags::FSD_JUMP, 0);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].speech.as_deref(), Some("Arriving at Gamma. This system has a scoopable star. This is the final jump."));
}
