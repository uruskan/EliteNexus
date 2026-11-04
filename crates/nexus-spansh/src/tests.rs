//! Deserialize every recorded Spansh response (docs/research/spansh-samples) and check that the
//! request builders reproduce the recorded (known-good) request bodies.

use serde_json::{Value, json};

use crate::dump;
use crate::records::SearchableFields;
use crate::*;

macro_rules! sample {
    ($file:literal) => {
        include_str!(concat!("../../../docs/research/spansh-samples/", $file))
    };
}

fn parse<T: serde::de::DeserializeOwned>(name: &str, text: &str) -> T {
    let mut de = serde_json::Deserializer::from_str(text);
    match serde_path_to_error::deserialize(&mut de) {
        Ok(v) => v,
        Err(e) => panic!("{name}: {} at {}", e.inner(), e.path()),
    }
}

#[test]
fn neutron_submit_envelope() {
    let env: JobEnvelope = parse("submit", sample!("neutron_route.submit.response.json"));
    assert_eq!(
        env.job.as_deref(),
        Some("C6CBECF4-C61D-11F1-80DB-B431641780ED")
    );
    assert_eq!(env.status.as_deref(), Some("queued"));
    assert!(!env.is_finished());
    assert_eq!(env.failure(), None);
}

#[test]
fn neutron_result() {
    let env: JobEnvelope = parse("neutron", sample!("neutron_route_sol_colonia.result.json"));
    assert!(env.is_finished());
    assert_eq!(env.state.as_deref(), Some("completed"));
    assert_eq!(
        env.created_at.as_deref().and_then(parse_timestamp),
        Some(1_791_796_661)
    );
    assert_eq!(env.parameters["efficiency"], "60");

    let route: NeutronRoute = env.route().expect("neutron result decodes");
    assert_eq!(route.job, "C6CBECF4-C61D-11F1-80DB-B431641780ED");
    assert_eq!(
        (
            route.source_system.as_str(),
            route.destination_system.as_str()
        ),
        ("Sol", "Colonia")
    );
    assert_eq!(route.range, Some(50.0));
    assert_eq!(route.efficiency, Some(60.0));
    assert!((route.distance - 22_000.474_045_341_1).abs() < 1e-6);
    assert_eq!(route.system_jumps.len(), 129);
    assert_eq!(route.total_jumps, 128);
    assert_eq!(route.estimated_jumps(), 166);

    let first = &route.system_jumps[0];
    assert_eq!(
        (
            first.system.as_str(),
            first.id64,
            first.jumps,
            first.neutron_star
        ),
        ("Sol", Some(10_477_373_803), 0, false)
    );
    assert_eq!(first.coords(), Some(Coords::new(0.0, 0.0, 0.0)));
    let last = route.system_jumps.last().expect("last");
    assert_eq!(
        (last.system.as_str(), last.id64),
        ("Colonia", Some(3_238_296_097_059))
    );
    assert_eq!(last.distance_left, 0.0);
    assert_eq!(route.neutron_count(), 127);

    let waypoints = route.waypoints();
    assert_eq!(waypoints.len(), 129);
    assert_eq!(waypoints[1].system, "PSR J1752-2806");
    assert!(waypoints[1].neutron);
    assert_eq!(waypoints[1].jumps, 10);
    assert_eq!(route.total_jumps(), 166);
}

#[test]
fn neutron_request_body_matches_recording() {
    // Recorded submit: efficiency=60&range=50&from=Sol&to=Colonia&supercharge_multiplier=4
    let req = NeutronRouteRequest::new("Sol", "Colonia", 50.0);
    let PlannerBody::Form(form) = req.body() else {
        panic!("form body")
    };
    let mut pairs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
    pairs.sort_unstable();
    assert_eq!(
        pairs,
        [
            ("efficiency", "60"),
            ("from", "Sol"),
            ("range", "50"),
            ("supercharge_multiplier", "4"),
            ("to", "Colonia")
        ]
    );
    assert!(req.validate().is_ok());
    assert!(
        NeutronRouteRequest::new("Sol", "", 50.0)
            .validate()
            .is_err()
    );
    assert!(
        NeutronRouteRequest::new("Sol", "Colonia", 50.0)
            .efficiency(0)
            .validate()
            .is_err()
    );

    let via = NeutronRouteRequest::new("Sol", "Colonia", 50.0)
        .via("Achenar")
        .via("Shinrarta Dezhra");
    let PlannerBody::Form(form) = via.body() else {
        panic!("form body")
    };
    let vias: Vec<&str> = form
        .iter()
        .filter(|(k, _)| *k == "via")
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(vias, ["Achenar", "Shinrarta Dezhra"]);
}

#[test]
fn stations_search_sample() {
    let resp: SearchResponse<StationRecord> = parse(
        "stations",
        sample!("stations_search_tritium_near_sol.response.trimmed.json"),
    );
    assert_eq!(resp.count, 1720);
    assert_eq!(resp.results.len(), 3);
    assert!(resp.has_more());
    assert_eq!(
        resp.search_reference.as_deref(),
        Some("D89314F8-C61D-11F1-8DFD-D2F019E66765")
    );
    let reference = resp.reference.first().expect("reference");
    assert_eq!(
        (reference.name.as_str(), reference.id64),
        ("Sol", Some(10_477_373_803))
    );

    let galileo = &resp.results[0];
    assert_eq!(galileo.name, "Galileo");
    assert_eq!(galileo.market_id, 128_016_640);
    assert_eq!(galileo.station_type.as_deref(), Some("Ocellus Starport"));
    assert_eq!(galileo.system_name, "Sol");
    assert_eq!(galileo.system_coords(), Some(Coords::new(0.0, 0.0, 0.0)));
    assert_eq!(galileo.system_population, Some(18_320_926_115));
    assert_eq!(galileo.large_pads, Some(6));
    assert_eq!(galileo.services, ["Dock", "Autodock", "Market", "Contacts"]);
    assert_eq!(
        galileo.export_commodities,
        ["Copper", "Polymers", "Synthetic Fabrics"]
    );
    assert_eq!(galileo.system_power.len(), 4);
    assert!(galileo.has_service("market"));
    assert!(!galileo.is_fleet_carrier());
    let egg = galileo.commodity("aepyornis egg").expect("market row");
    assert!(egg.is_rare);
    assert_eq!((egg.buy_price, egg.sell_price), (2146, 2124));
    assert_eq!(galileo.modules[0].class, Some(1));
    assert_eq!(galileo.modules[0].price, None);
    assert_eq!(galileo.modules[1].price, Some(35_120));
    assert_eq!(galileo.ships[1].symbol.as_deref(), Some("BelugaLiner"));
    assert_eq!(galileo.market_updated_unix(), Some(1_791_789_343));
    // Exact symbols, any case: what an outfitting search checks its results against.
    assert_eq!(
        galileo.module("adder_armour_grade2").and_then(|m| m.price),
        Some(35_120)
    );
    assert!(galileo.module("anaconda_armour_grade2").is_none());
    assert!(galileo.ship("belugaliner").is_some());
    assert!(galileo.ship("anaconda").is_none());
    assert_eq!(
        galileo.outfitting_updated_at.as_deref(),
        Some("2026-10-12T07:16:09Z")
    );
    assert!(galileo.outfitting_updated_unix().is_some());
    assert!(galileo.shipyard_updated_unix() < galileo.outfitting_updated_unix());
    assert_eq!(resp.results[1].prohibited_commodities[0], "Narcotics");
}

#[test]
fn stations_search_request_matches_recording() {
    let recorded: Value = parse(
        "request",
        sample!("stations_search_tritium_near_sol.request.json"),
    );
    let built = MarketQuery::buy("Tritium", "Sol")
        .min_quantity(1000)
        .large_pad(true)
        .within(100.0)
        .size(3)
        .to_request()
        .to_json();
    assert_eq!(built, recorded);
}

#[test]
fn bodies_search_sample() {
    let resp: SearchResponse<BodyRecord> = parse(
        "bodies",
        sample!("bodies_search_painite_rings.response.json"),
    );
    assert_eq!(resp.count, 3029);
    let barnard = &resp.results[0];
    assert_eq!(barnard.name, "Barnard's Star 5");
    assert_eq!(barnard.id64, 468_392_624_387_073_449);
    assert_eq!(barnard.body_id, Some(13));
    assert_eq!(barnard.is_main_star, None);
    assert_eq!(barnard.subtype.as_deref(), Some("Class I gas giant"));
    assert_eq!(barnard.system_name, "Barnard's Star");
    assert_eq!(barnard.rings.len(), 2);
    assert_eq!(barnard.hotspots("painite"), 2);
    assert_eq!(barnard.hotspots("Low Temperature Diamonds"), 1);
    assert_eq!(
        barnard.best_ring("Painite").map(|r| r.name.as_str()),
        Some("Barnard's Star 5 A Ring")
    );
    assert_eq!(barnard.rings[0].ring_type.as_deref(), Some("Metal Rich"));
    assert_eq!(
        barnard.rings[0].signals_updated_at.as_deref(),
        Some("2026-10-07T21:22:54Z")
    );
    assert_eq!(barnard.signal("human"), 6);
    assert_eq!(barnard.parents[0].kind.as_deref(), Some("Star"));
    assert_eq!(barnard.atmosphere_composition.len(), 2);
    assert_eq!(barnard.estimated_mapping_value, Some(17_980));
    // A ring without signals.
    assert!(resp.results[1].rings[1].signals.is_empty());
    assert_eq!(resp.results[2].hotspots("Painite"), 3);
}

#[test]
fn bodies_search_request_matches_recording() {
    let recorded: Value = parse(
        "request",
        sample!("bodies_search_painite_rings.request.json"),
    );
    let built = HotspotQuery::new("Painite", 2, "Sol")
        .within(200.0)
        .size(3)
        .to_request()
        .to_json();
    assert_eq!(built, recorded);
}

#[test]
fn smart_search_sample() {
    let resp: SmartSearchResponse = parse(
        "smart",
        sample!("smart_search_q_shinrarta.response.trimmed.json"),
    );
    assert_eq!(resp.count, 43);
    assert_eq!(resp.query, "Shinrarta Dezhra");
    assert_eq!(resp.results.len(), 3);
    let SmartSearchHit::System(system) = &resp.results[0] else {
        panic!("system hit")
    };
    assert_eq!(
        (system.name.as_str(), system.id64),
        ("Shinrarta Dezhra", 3_932_277_478_106)
    );
    assert_eq!(system.needs_permit, Some(true));
    assert_eq!(system.population, Some(85_287_324));
    assert_eq!(
        system.minor_faction_presences[0].active_states,
        ["Boom", "Civil Liberty"]
    );
    assert_eq!(system.stations[0].name, "Jameson Memorial");
    assert_eq!(system.stations[0].services, ["Dock", "Autodock", "Market"]);
    assert_eq!(system.bodies[0].id64, 36_032_729_296_442_074);
    assert_eq!(
        system.main_star().map(|b| b.name.as_str()),
        Some("Shinrarta Dezhra")
    );
    assert_eq!(system.bodies[2].landmarks.len(), 3);
    assert_eq!(system.synthesis_recipes[0].level, "Premium");
    let SmartSearchHit::Body(body) = &resp.results[1] else {
        panic!("body hit")
    };
    assert_eq!(body.belts[0].name, "Shinrarta Dezhra A A Belt");
    assert_eq!(body.luminosity_class.as_deref(), Some("V"));
    let SmartSearchHit::Station(station) = &resp.results[2] else {
        panic!("station hit")
    };
    assert_eq!(station.market_id, 4_218_631_171);
    assert_eq!(station.state.as_deref(), Some("Construction"));
    assert_eq!(resp.results[2].name(), "Shinrarta Dezhra");
}

#[test]
fn autocomplete_and_nearest_samples() {
    let names: SystemNames = parse(
        "names",
        sample!("systems_field_values_system_names_q_Colon.json"),
    );
    assert_eq!(names.values, ["Colonia"]);
    let colonia = &names.min_max[0];
    assert_eq!(
        (colonia.name.as_str(), colonia.id64),
        ("Colonia", Some(3_238_296_097_059))
    );
    assert_eq!(
        colonia.coords(),
        Coords::new(-9530.5, -910.28125, 19808.125)
    );

    let legacy: Vec<String> = parse(
        "legacy",
        sample!("systems_q_Colon.legacy_autocomplete.json"),
    );
    assert_eq!(legacy, ["Colonia"]);

    let nearest: Value = parse("nearest", sample!("nearest_colonia_coords.json"));
    let point: SystemPoint =
        serde_json::from_value(nearest["system"].clone()).expect("nearest system");
    assert_eq!(point.name, "Colonia");
    assert!((point.distance.expect("distance") - 0.587_134).abs() < 1e-6);
}

#[test]
fn searchable_fields_samples() {
    for (kind, text, expect) in [
        (
            "systems",
            sample!("searchable_fields_systems.json"),
            "population",
        ),
        (
            "bodies",
            sample!("searchable_fields_bodies.json"),
            "ring_signals",
        ),
        (
            "stations",
            sample!("searchable_fields_stations.json"),
            "market",
        ),
    ] {
        let fields: SearchableFields = parse(kind, text);
        assert!(
            fields.fields.len() > 30,
            "{kind}: {} fields",
            fields.fields.len()
        );
        assert!(
            fields.fields.iter().any(|f| f.field == expect),
            "{kind} lacks {expect}"
        );
        assert!(
            fields
                .fields
                .iter()
                .all(|f| !f.field.is_empty() && !f.kind.is_empty()),
            "{kind}"
        );
    }
    let stations: SearchableFields = parse("stations", sample!("searchable_fields_stations.json"));
    let economies = stations
        .fields
        .iter()
        .find(|f| f.field == "economies")
        .expect("economies");
    assert_eq!(economies.kind, "value_struct");
    assert!(economies.multiple);
    assert!(economies.valuefield.is_some());
    let faction = stations
        .fields
        .iter()
        .find(|f| f.field == "controlling_minor_faction")
        .expect("faction");
    assert_eq!(
        faction.field_values_url.as_deref(),
        Some("/api/stations/field_values/autocomplete_controlling_minor_faction")
    );
}

#[test]
fn dump_sol_sample() {
    let env: Value = parse("dump", sample!("dump_sol.response.trimmed.json"));
    let sol: dump::System = parse("dump.system", &env["system"].to_string());
    assert_eq!((sol.id64, sol.name.as_str()), (10_477_373_803, "Sol"));
    assert_eq!(sol.population, Some(18_320_926_115));
    assert_eq!(sol.body_count, Some(40));
    assert_eq!(sol.controlling_power.as_deref(), Some("Jerome Archer"));
    assert_eq!(sol.powers.len(), 3);
    assert_eq!(
        sol.timestamps.get("factions").map(String::as_str),
        Some("2026-10-12T09:12:37Z")
    );
    let cf = sol
        .controlling_faction
        .as_ref()
        .expect("controlling faction");
    assert_eq!(cf.name, "Mother Gaia");
    assert_eq!(cf.pending_states[0].state, "Expansion");
    assert_eq!(sol.factions.len(), 3);
    assert_eq!(sol.factions[1].active_states[0].state, "Civil Liberty");

    let star = sol.main_star().expect("main star");
    assert_eq!(star.sub_type.as_deref(), Some("G (White-Yellow) Star"));
    assert_eq!(star.belts[0].ring_type.as_deref(), Some("Rocky"));
    assert!(star.is_star());

    let mercury = sol.body("Mercury").expect("Mercury");
    assert_eq!(
        mercury.parents,
        [dump::ParentRef {
            kind: "Star".into(),
            body_id: 0
        }]
    );
    assert_eq!(mercury.materials.get("Iron"), Some(&23.508457));
    assert_eq!(mercury.signal_count("Human"), 6);
    assert_eq!(
        mercury
            .signals
            .as_ref()
            .map(|s| s.count("$PlanetaryMiningLocation_Name;")),
        Some(6)
    );
    assert_eq!(mercury.atmosphere_type, None);
    assert_eq!(mercury.stations.len(), 3);
    let walz = &mercury.stations[0];
    assert_eq!((walz.name.as_str(), walz.id), ("Walz Depot", 3_534_389_760));
    assert_eq!(walz.landing_pads.as_ref().map(|p| p.large), Some(2));
    assert_eq!(walz.controlling_faction_state, None);
    assert_eq!(
        walz.commodity("CetiAepyornisEgg").map(|c| c.buy_price),
        Some(2125)
    );
    assert_eq!(walz.outfitting.as_ref().map(|o| o.modules.len()), Some(3));
    assert_eq!(
        walz.shipyard.as_ref().map(|s| s.ships[0].ship_id),
        Some(Some(128_049_303))
    );

    let venus = sol.body("venus").expect("Venus");
    assert_eq!(
        venus.atmosphere_composition.get("Carbon dioxide"),
        Some(&96.5)
    );
    assert_eq!(venus.volcanism_type.as_deref(), Some("Minor Rocky Magma"));

    assert_eq!(sol.stations.len(), 3);
    assert_eq!(sol.all_stations().count(), 6);
    assert_eq!(sol.stations[0].economies.get("Refinery"), Some(&100.0));
    assert_eq!(
        sol.stations[1]
            .market
            .as_ref()
            .map(|m| m.prohibited_commodities.len()),
        Some(3)
    );
}

#[test]
fn galaxy_dump_line_samples() {
    let populated: Vec<dump::System> = parse(
        "populated",
        sample!("galaxy_populated.first_record.trimmed.json"),
    );
    let hd = &populated[0];
    assert_eq!(hd.name, "HD 219286");
    assert_eq!(
        hd.controlling_faction
            .as_ref()
            .map(|f| f.active_states[0].state.as_str()),
        Some("Expansion")
    );
    assert_eq!(hd.bodies[2].solid_composition.get("Rock"), Some(&66.8406));
    assert_eq!(
        hd.stations[0].controlling_faction_state.as_deref(),
        Some("Expansion")
    );
    assert_eq!(hd.stations[0].economies.len(), 3);

    let stations: Vec<dump::System> = parse(
        "stations",
        sample!("galaxy_stations.first_record.trimmed.json"),
    );
    let phua = &stations[0];
    assert_eq!(phua.allegiance, None);
    assert_eq!(phua.bodies[0].body_type.as_deref(), Some("Barycentre"));
    assert_eq!(phua.bodies[1].parents.len(), 2);
    let carrier = &phua.stations[0];
    assert!(carrier.is_fleet_carrier());
    assert_eq!(carrier.carrier_docking_access.as_deref(), Some("All"));
    assert_eq!(carrier.commodity("Tritium").map(|c| c.supply), Some(19_966));

    let systems: Vec<dump::SystemSummary> =
        parse("systems", sample!("systems_1day.first_records.json"));
    assert_eq!(systems.len(), 3);
    assert_eq!(systems[2].main_star.as_deref(), Some("Black Hole"));

    let factions: Vec<dump::Faction> =
        parse("factions", sample!("factions.first_record.trimmed.json"));
    let fearless = &factions[0];
    assert_eq!(fearless.systems.len(), 4);
    let deduped = fearless.systems_deduped();
    assert_eq!(deduped.len(), 2);
    assert!(deduped.iter().all(|s| s.is_controlling_faction));
}

#[test]
fn every_sample_is_covered() {
    // Keep this list in sync with docs/research/spansh-samples: a new recording should get a test.
    let covered = [
        "bodies_search_painite_rings.request.json",
        "bodies_search_painite_rings.response.json",
        "dump_sol.response.trimmed.json",
        "factions.first_record.trimmed.json",
        "galaxy_populated.first_record.trimmed.json",
        "galaxy_stations.first_record.trimmed.json",
        "nearest_colonia_coords.json",
        "neutron_route.submit.response.json",
        "neutron_route_sol_colonia.result.json",
        "searchable_fields_bodies.json",
        "searchable_fields_stations.json",
        "searchable_fields_systems.json",
        "smart_search_q_shinrarta.response.trimmed.json",
        "stations_search_tritium_near_sol.request.json",
        "stations_search_tritium_near_sol.response.trimmed.json",
        "systems_1day.first_records.json",
        "systems_field_values_system_names_q_Colon.json",
        "systems_q_Colon.legacy_autocomplete.json",
    ];
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/research/spansh-samples");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            covered.contains(&name.as_str()),
            "sample {name} has no test"
        );
    }
}

#[test]
fn route_shapes_from_website_code() {
    // Shapes documented from the website/EDDiscovery code (spansh.md §3), incl. string/0-1 forms.
    let carrier: CarrierRoute = serde_json::from_value(json!({"jumps": [
        {"name": "Sol", "id64": 10477373803u64, "x": 0, "y": 0, "z": 0, "distance": 0, "distance_to_destination": 140.1,
         "fuel_in_tank": 1000, "fuel_used": 0, "tritium_in_market": 0, "has_icy_ring": true, "is_system_pristine": false,
         "must_restock": false, "restock_amount": 0, "is_desired_destination": false},
        {"name": "Achenar", "id64": 164098653, "x": 67.5, "y": -119.46875, "z": 24.84375, "distance": "140.1",
         "distance_to_destination": 0, "fuel_in_tank": 988, "fuel_used": 12, "tritium_in_market": 0, "has_icy_ring": 0,
         "is_system_pristine": 0, "must_restock": 1, "restock_amount": 300, "is_desired_destination": 1}
    ]}))
    .expect("carrier");
    assert_eq!(carrier.jump_count(), 1);
    assert_eq!(carrier.tritium_used(), 12);
    assert!(carrier.jumps[1].must_restock && carrier.jumps[1].is_desired_destination);
    assert_eq!(
        carrier.waypoints()[1].note.as_deref(),
        Some("restock 300 t tritium")
    );

    let exo: ScanRoute = serde_json::from_value(json!([
        {"name": "HIP 1", "id64": 1, "x": 1, "y": 2, "z": 3, "jumps": 2, "bodies": [
            {"name": "HIP 1 A 1", "subtype": "Rocky body", "type": "Planet", "distance_to_arrival": 12.5,
             "estimated_scan_value": 500, "estimated_mapping_value": 1500, "landmark_value": 19010800,
             "landmarks": [
                {"type": "Bacterium", "subtype": "Bacterium Aurasus", "count": 2, "value": 1000000},
                {"type": "Stratum", "subtype": "Stratum Tectonicas", "value": 19010800},
                {"type": "Bacterium", "subtype": "Bacterium Aurasus", "count": 1, "value": 1000000}]}]}
    ]))
    .expect("exobiology");
    let species = exo.systems[0].bodies[0].species();
    assert_eq!(species.len(), 2);
    assert_eq!(
        (species[0].species.as_str(), species[0].count),
        ("Bacterium Aurasus", 3)
    );
    assert_eq!(exo.total_value(), 1500 + 19_010_800);
    assert_eq!(
        exo.waypoints()[0].note.as_deref(),
        Some("Bacterium Aurasus, Stratum Tectonicas")
    );

    let trade: TradeRoute = serde_json::from_value(json!([
        {"source": {"system": "Sol", "station": "Galileo", "market_id": 128016640, "x": 0, "y": 0, "z": 0},
         "destination": {"system": "Alpha Centauri", "station": "Hutton Orbital", "market_id": 128000000},
         "distance": 4.38, "commodities": [{"name": "Gold", "amount": 7, "profit": 1000, "total_profit": 7000,
            "source_commodity": {"buy_price": 9000, "supply": 500}, "destination_commodity": {"sell_price": 10000, "demand": 90}}],
         "total_profit": 7000, "cumulative_profit": 7000}
    ]))
    .expect("trade");
    assert_eq!(trade.total_profit(), 7000);
    let w = trade.waypoints();
    assert_eq!(w.len(), 2);
    assert_eq!(w[0].note.as_deref(), Some("buy Gold ×7"));
    assert_eq!(w[1].bodies, ["Hutton Orbital"]);
}

// =============================================================================================
// Live recordings from the smoke example (crates/nexus-spansh/tests/fixtures, 2026-10-12).

macro_rules! fixture {
    ($file:literal) => {
        include_str!(concat!("../tests/fixtures/", $file))
    };
}

#[test]
fn live_neutron_60ly() {
    let env: JobEnvelope = parse(
        "neutron60",
        fixture!("neutron_sol_colonia_60ly.result.json"),
    );
    let route: NeutronRoute = env.route().expect("route");
    assert_eq!(route.range, Some(60.0));
    assert_eq!(route.system_jumps.len(), 110);
    assert_eq!(route.estimated_jumps(), 133);
    assert_eq!(route.neutron_count(), 108);
    assert_eq!(
        route.system_jumps.last().map(|j| j.system.as_str()),
        Some("Colonia")
    );
}

#[test]
fn live_carrier_route() {
    let env: JobEnvelope = parse(
        "carrier",
        fixture!("carrier_sol_achenar_shinrarta.result.json"),
    );
    assert_eq!(
        env.parameters["destination_systems"],
        json!(["Achenar", "Shinrarta Dezhra"])
    );
    let route: CarrierRoute = env.route().expect("route");
    assert_eq!(route.jumps.len(), 4);
    assert_eq!(route.jump_count(), 2);
    assert_eq!(route.tritium_used(), 46);
    assert!((route.distance() - 277.037).abs() < 0.01);
    assert_eq!(route.source.as_deref(), Some("Sol"));
    assert_eq!(route.destinations, ["Achenar", "Shinrarta Dezhra"]);
    assert_eq!(
        (
            route.capacity,
            route.capacity_used,
            route.calculate_starting_fuel
        ),
        (Some(25_000), Some(1_000), Some(true))
    );
    // 0/1 integers decode as booleans.
    let sol = &route.jumps[0];
    assert!(sol.must_restock && sol.is_desired_destination);
    assert_eq!((sol.restock_amount, sol.fuel_in_tank), (46, 46));
    assert!(route.jumps[3].has_icy_ring);

    let stops = route.waypoints();
    let names: Vec<&str> = stops.iter().map(|w| w.system.as_str()).collect();
    assert_eq!(names, ["Sol", "Achenar", "Shinrarta Dezhra"]);
    assert_eq!(stops[0].note.as_deref(), Some("restock 46 t tritium"));
    assert_eq!(stops.iter().map(|w| w.jumps).sum::<u32>(), 2);
    assert_eq!(stops[2].fuel_in_tank, Some(0.0));
}

#[test]
fn live_galaxy_route() {
    let started: JobEnvelope = parse(
        "galaxy started",
        fixture!("galaxy_sol_maia.started.http202.json"),
    );
    assert_eq!(
        (started.state.as_deref(), started.status.as_deref()),
        (Some("started"), Some("queued"))
    );
    assert!(!started.is_finished());
    assert_eq!(started.parameters["optimal_mass"], 2117.5);

    let env: JobEnvelope = parse("galaxy", fixture!("galaxy_sol_maia.result.json"));
    // The plotter used its whole max_time (60 s).
    let created = env
        .created_at
        .as_deref()
        .and_then(parse_timestamp)
        .expect("created");
    let updated = env
        .updated_at
        .as_deref()
        .and_then(parse_timestamp)
        .expect("updated");
    assert_eq!(updated - created, 61);
    let route: GalaxyRoute = env.route().expect("route");
    assert!(!route.incomplete);
    assert_eq!(route.jumps.len(), 7);
    assert_eq!(route.jump_count(), 6);
    assert_eq!(
        (
            route.refuel_stops(),
            route.neutron_boosts(),
            route.injections()
        ),
        (1, 1, 0)
    );
    assert!((route.fuel_used() - 29.58).abs() < 0.01);
    let lighthouse = &route.jumps[3];
    assert_eq!(lighthouse.name, "Jackson's Lighthouse");
    assert!(lighthouse.has_neutron);
    let refuel = &route.jumps[4];
    assert!(refuel.must_refuel && refuel.is_scoopable);
    assert_eq!(refuel.fuel_in_tank, 32.0);
    assert_eq!(route.jumps[6].name, "Maia");
    assert_eq!(route.waypoints()[4].scoopable, Some(true));
    assert_eq!(route.total_jumps(), 6);
}

#[test]
fn live_tritium_search() {
    let resp: SearchResponse<StationRecord> = parse(
        "tritium",
        fixture!("stations_search_tritium_7d.response.trimmed.json"),
    );
    assert_eq!(resp.count, 5761);
    let galileo = &resp.results[0];
    let tritium = galileo.commodity("Tritium").expect("Tritium row");
    assert_eq!((tritium.supply, tritium.buy_price), (355_413, 50_137));
    assert!(
        resp.results
            .iter()
            .all(|s| s.commodity("tritium").is_some_and(|t| t.supply >= 1000))
    );

    let built = MarketQuery::buy("Tritium", "Sol")
        .min_quantity(1000)
        .max_age(std::time::Duration::from_secs(7 * 86_400))
        .size(5)
        .to_request()
        .to_json();
    assert_eq!(
        built["filters"]["market_updated_at"],
        json!({"comparison": "<=>", "value": ["now-7d", "now"]})
    );
    let echo: Value = parse(
        "tritium echo",
        fixture!("stations_search_tritium_7d.response.trimmed.json"),
    );
    assert_eq!(
        echo["search"]["filters"]["market_updated_at"],
        built["filters"]["market_updated_at"]
    );
}

#[test]
fn live_ltd_hotspots() {
    let resp: SearchResponse<BodyRecord> =
        parse("ltd", fixture!("bodies_search_ltd_x2.response.json"));
    assert_eq!(resp.count, 10_000);
    assert_eq!(resp.results.len(), 5);
    let first = &resp.results[0];
    assert_eq!(first.name, "WISE 1506+7027 3");
    assert_eq!(first.hotspots("Low Temperature Diamonds"), 2);
    assert_eq!(
        first
            .best_ring("Low Temperature Diamonds")
            .map(|r| r.name.as_str()),
        Some("WISE 1506+7027 3 B Ring")
    );
    assert!(
        resp.results
            .iter()
            .all(|b| b.hotspots("Low Temperature Diamonds") >= 2)
    );
    assert!(
        resp.results
            .windows(2)
            .all(|w| w[0].distance <= w[1].distance)
    );
}

#[test]
fn live_system_by_name_search() {
    let resp: SearchResponse<SystemRecord> = parse(
        "system",
        fixture!("systems_search_name_shinrarta.response.trimmed.json"),
    );
    assert_eq!(resp.count, 1);
    let sd = &resp.results[0];
    assert_eq!(
        (sd.name.as_str(), sd.id64),
        ("Shinrarta Dezhra", 3_932_277_478_106)
    );
    assert_eq!(sd.needs_permit, Some(true));
    assert_eq!(
        sd.controlling_minor_faction.as_deref(),
        Some("Pilots' Federation Local Branch")
    );
    assert_eq!(sd.stations.len(), 4);
}

#[test]
fn every_fixture_is_covered() {
    let covered = [
        "bodies_search_ltd_x2.response.json",
        "carrier_sol_achenar_shinrarta.result.json",
        "galaxy_sol_maia.result.json",
        "galaxy_sol_maia.started.http202.json",
        "neutron_sol_colonia_60ly.result.json",
        "stations_search_tritium_7d.response.trimmed.json",
        "systems_search_name_shinrarta.response.trimmed.json",
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let entries = std::fs::read_dir(&dir).expect("fixtures dir");
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            covered.contains(&name.as_str()),
            "fixture {name} has no test"
        );
    }
}

// =============================================================================================
// Job machinery against a localhost mock (no Spansh traffic).

mod mock {
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[derive(Debug, Clone)]
    pub struct Seen {
        pub method: String,
        pub path: String,
        pub body: String,
    }

    pub struct Mock {
        pub base: String,
        pub seen: Arc<Mutex<Vec<Seen>>>,
    }

    impl Mock {
        pub fn requests(&self) -> Vec<Seen> {
            self.seen.lock().expect("lock").clone()
        }
    }

    /// Serve the canned `(status, body)` replies in order, one per connection; then HTTP 500.
    pub async fn serve(replies: Vec<(u16, &str)>) -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let mut replies = replies
            .into_iter()
            .map(|(s, b)| (s, b.to_owned()))
            .collect::<Vec<_>>()
            .into_iter();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let Some(req) = read_request(&mut sock).await else {
                    continue;
                };
                log.lock().expect("lock").push(req);
                let (status, body) = replies
                    .next()
                    .unwrap_or((500, r#"{"error":"mock exhausted"}"#.to_owned()));
                let head = format!(
                    "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(body.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        Mock {
            base: format!("http://{addr}/api"),
            seen,
        }
    }

    async fn read_request(sock: &mut TcpStream) -> Option<Seen> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            let n = sock.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let len = head
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                if k.eq_ignore_ascii_case("content-length") {
                    v.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        while buf.len() < head_end + len {
            let n = sock.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let mut first = head.lines().next()?.split_whitespace();
        let method = first.next()?.to_owned();
        let path = first.next()?.to_owned();
        Some(Seen {
            method,
            path,
            body: String::from_utf8_lossy(&buf[head_end..]).into_owned(),
        })
    }
}

fn fast(base: &str) -> Spansh {
    use std::time::Duration;
    Spansh::builder()
        .base_url(base)
        .job_options(
            JobOptions::default()
                .poll_interval(Duration::from_millis(5), Duration::from_millis(20)),
        )
        .build()
        .expect("client")
}

const QUEUED: &str = r#"{"job":"J1","status":"queued"}"#;
const RUNNING: &str = r#"{"job":"J1","state":"started","status":"queued","created_at":"2026-10-12T09:17:41","updated_at":"2026-10-12T09:17:42"}"#;
const DONE: &str = r#"{"job":"J1","state":"completed","status":"ok","parameters":{"from":"Sol"},
    "result":{"job":"J1","source_system":"Sol","destination_system":"Colonia","total_jumps":1,
              "system_jumps":[{"system":"Sol","jumps":0},{"system":"Colonia","jumps":3}]}}"#;

fn sol_colonia() -> NeutronRouteRequest {
    NeutronRouteRequest::new("Sol", "Colonia", 60.0)
}

#[tokio::test]
async fn job_polls_until_done() {
    let mock = mock::serve(vec![(202, QUEUED), (202, RUNNING), (200, DONE)]).await;
    let states = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&states);
    let client = fast(&mock.base);
    let opts = client
        .job_options()
        .clone()
        .on_progress(move |p| sink.lock().expect("lock").push(p.state.clone()));

    let plotted = client
        .with_job_options(opts)
        .neutron_route(&sol_colonia())
        .await
        .expect("plotted");
    assert_eq!(plotted.job, "J1");
    assert_eq!(plotted.polls, 2);
    assert_eq!(plotted.route.estimated_jumps(), 3);
    assert_eq!(plotted.parameters["from"], "Sol");
    assert_eq!(plotted.web_url, "https://spansh.co.uk/plotter/results/J1");
    assert_eq!(
        *states.lock().expect("lock"),
        [JobState::Submitted, JobState::Started, JobState::Completed]
    );

    let seen = mock.requests();
    assert_eq!(seen.len(), 3);
    assert_eq!(
        (seen[0].method.as_str(), seen[0].path.as_str()),
        ("POST", "/api/route")
    );
    for part in [
        "efficiency=60",
        "range=60",
        "from=Sol",
        "to=Colonia",
        "supercharge_multiplier=4",
    ] {
        assert!(
            seen[0].body.split('&').any(|p| p == part),
            "{part} missing from {}",
            seen[0].body
        );
    }
    assert_eq!(
        (seen[1].method.as_str(), seen[1].path.as_str()),
        ("GET", "/api/results/J1")
    );
}

#[tokio::test]
async fn job_resubmits_once_when_expired() {
    let done_j2 = DONE.replace("J1", "J2");
    let mock = mock::serve(vec![
        (202, QUEUED),
        (404, r#"{"error":"Not found"}"#),
        (202, r#"{"job":"J2","status":"queued"}"#),
        (200, &done_j2),
    ])
    .await;
    let plotted = fast(&mock.base)
        .neutron_route(&sol_colonia())
        .await
        .expect("plotted");
    assert_eq!(plotted.job, "J2");
    let paths: Vec<String> = mock.requests().into_iter().map(|s| s.path).collect();
    assert_eq!(
        paths,
        [
            "/api/route",
            "/api/results/J1",
            "/api/route",
            "/api/results/J2"
        ]
    );

    let mock = mock::serve(vec![
        (202, QUEUED),
        (404, "{}"),
        (202, r#"{"job":"J2","status":"queued"}"#),
        (404, "{}"),
    ])
    .await;
    let err = fast(&mock.base)
        .neutron_route(&sol_colonia())
        .await
        .expect_err("expired");
    assert!(
        matches!(err, Error::JobExpired { ref job } if job == "J2"),
        "{err:?}"
    );
}

#[tokio::test]
async fn planner_errors_are_reported() {
    let mock = mock::serve(vec![(
        400,
        r#"{"error":"Could not find starting system \"Nowhere\""}"#,
    )])
    .await;
    let err = fast(&mock.base)
        .neutron_route(&NeutronRouteRequest::new("Nowhere", "Colonia", 60.0))
        .await
        .expect_err("rejected");
    assert!(matches!(err, Error::Api { .. }), "{err:?}");
    assert_eq!(
        err.spansh_message(),
        Some("Could not find starting system \"Nowhere\"")
    );
    assert!(!err.is_transient());

    let mock = mock::serve(vec![
        (202, QUEUED),
        (
            200,
            r#"{"job":"J1","status":"error","error":"No route found"}"#,
        ),
    ])
    .await;
    let err = fast(&mock.base)
        .neutron_route(&sol_colonia())
        .await
        .expect_err("failed job");
    assert_eq!(err.spansh_message(), Some("No route found"));

    // Invalid requests never reach the server.
    let mock = mock::serve(vec![]).await;
    let err = fast(&mock.base)
        .neutron_route(&NeutronRouteRequest::new("Sol", "Colonia", -1.0))
        .await
        .expect_err("invalid");
    assert!(matches!(err, Error::InvalidRequest(_)));
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn transient_poll_errors_are_retried() {
    let mock = mock::serve(vec![
        (202, QUEUED),
        (503, "<html>Service Unavailable</html>"),
        (200, DONE),
    ])
    .await;
    let plotted = fast(&mock.base)
        .neutron_route(&sol_colonia())
        .await
        .expect("plotted");
    assert_eq!(plotted.polls, 2);
}

#[tokio::test]
async fn job_times_out() {
    let replies: Vec<(u16, &str)> = std::iter::once((202, QUEUED))
        .chain(std::iter::repeat_n((202, RUNNING), 200))
        .collect();
    let mock = mock::serve(replies).await;
    let client = fast(&mock.base);
    let opts = client
        .job_options()
        .clone()
        .timeout(std::time::Duration::from_millis(120));
    let err = client
        .with_job_options(opts)
        .neutron_route(&sol_colonia())
        .await
        .expect_err("timeout");
    assert!(matches!(err, Error::JobTimeout { .. }), "{err:?}");
    assert!(err.is_transient());
}

#[tokio::test]
async fn jobs_can_be_cancelled() {
    let replies: Vec<(u16, &str)> = std::iter::once((202, QUEUED))
        .chain(std::iter::repeat_n((202, RUNNING), 200))
        .collect();
    let mock = mock::serve(replies).await;
    let token = CancellationToken::new();
    let spansh = fast(&mock.base).with_cancellation(token.clone());
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        token.cancel();
    });
    let err = spansh
        .neutron_route(&sol_colonia())
        .await
        .expect_err("cancelled");
    assert!(matches!(err, Error::Cancelled), "{err:?}");
    canceller.await.expect("canceller");

    // Already cancelled: nothing is sent.
    let mock = mock::serve(vec![(202, QUEUED)]).await;
    let token = CancellationToken::new();
    token.cancel();
    let err = fast(&mock.base)
        .with_cancellation(token)
        .neutron_route(&sol_colonia())
        .await;
    assert!(matches!(err, Err(Error::Cancelled)));
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn lookups_map_http_errors() {
    let mock = mock::serve(vec![
        (404, r#"{"error":"Not found"}"#),
        (503, "<html>busy</html>"),
        (200, r#"{"record":{"id64":10477373803,"name":"Sol","x":0,"y":0,"z":0,"population":18320926115}}"#),
        (200, r#"{"system":{"distance":0.58,"id64":3238296097059,"name":"Colonia","x":-9530.5,"y":-910.28125,"z":19808.125}}"#),
        (200, "not json"),
    ])
    .await;
    let spansh = fast(&mock.base);
    assert!(matches!(
        spansh.system(1).await,
        Err(Error::NotFound { .. })
    ));
    let busy = spansh.system(1).await.expect_err("busy");
    assert_eq!(busy.status(), Some(503));
    assert!(busy.is_transient());
    let sol = spansh.system(10_477_373_803).await.expect("sol");
    assert_eq!(
        (sol.name.as_str(), sol.population),
        ("Sol", Some(18_320_926_115))
    );
    let near = spansh
        .nearest(Coords::new(-9530.0, -910.0, 19808.0))
        .await
        .expect("nearest");
    assert_eq!(near.name, "Colonia");
    let bad = spansh.system(2).await.expect_err("decode");
    assert!(matches!(bad, Error::Decode { .. }), "{bad:?}");

    let paths: Vec<String> = mock.requests().into_iter().map(|s| s.path).collect();
    assert_eq!(paths[3], "/api/nearest?x=-9530&y=-910&z=19808");
}

#[test]
fn client_and_futures_are_send() {
    fn send<T: Send>(_: &T) {}
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Spansh>();
    send_sync::<Error>();
    let spansh = Spansh::builder()
        .base_url("http://127.0.0.1:9/api")
        .build()
        .expect("client");
    let (neutron, galaxy) = (sol_colonia(), GalaxyRouteRequest::default());
    let market = MarketQuery::buy("Tritium", "Sol");
    send(&spansh.neutron_route(&neutron));
    send(&spansh.galaxy_route(&galaxy));
    send(&spansh.market_search(&market));
    send(&spansh.system_by_name("Sol"));
    send(&spansh.dump(1));
}

#[tokio::test]
async fn missing_records_are_decode_errors() {
    let mock = mock::serve(vec![(200, r#"{"error":null}"#), (200, "{}")]).await;
    let spansh = fast(&mock.base);
    assert!(matches!(spansh.station(1).await, Err(Error::Decode { .. })));
    assert!(matches!(
        spansh.nearest(Coords::default()).await,
        Err(Error::NotFound { .. })
    ));
}

#[tokio::test]
async fn searches_post_json() {
    let mock = mock::serve(vec![(
        200,
        sample!("bodies_search_painite_rings.response.json"),
    )])
    .await;
    let query = HotspotQuery::new("Painite", 2, "Sol").within(200.0).size(3);
    let resp = fast(&mock.base)
        .hotspot_search(&query)
        .await
        .expect("search");
    assert_eq!(resp.results.len(), 3);
    let seen = mock.requests();
    assert_eq!(
        (seen[0].method.as_str(), seen[0].path.as_str()),
        ("POST", "/api/bodies/search")
    );
    let sent: Value = serde_json::from_str(&seen[0].body).expect("json body");
    assert_eq!(
        sent,
        serde_json::from_str::<Value>(sample!("bodies_search_painite_rings.request.json"))
            .expect("sample")
    );
}
