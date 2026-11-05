//! End-to-end tests of the running uplink with a recording transport (nothing is sent anywhere).

use super::*;
use crate::config::{EdsmCredentials, InaraCredentials};
use crate::context::ContextTracker;
use crate::json::{form_urldecode, gunzip};
use crate::queue::tests::TempDir;
use crate::transport::{HttpResponse, Method, RecordingTransport, TransportError, simulated_response};
use nexus_journal::{Event, LineSource};

const FILEHEADER: &str = r#""event":"Fileheader", "part":1, "language":"English/UK", "Odyssey":true, "gameversion":"4.4.1.1", "build":"r332841/r0 ""#;
const LOADGAME: &str = r#""event":"LoadGame", "FID":"F1", "Commander":"Jameson", "Horizons":true, "Odyssey":true, "Ship":"explorer_nx", "ShipID":22, "ShipName":"", "ShipIdent":"AL-11E", "GameMode":"Solo", "Credits":100, "Loan":0, "language":"English/UK", "gameversion":"4.4.1.1", "build":"r332841/r0 ""#;
const FSDJUMP: &str = r#""event":"FSDJump", "Taxi":false, "Multicrew":false, "StarSystem":"Synuefe BR-L c24-23", "SystemAddress":6406446944978, "StarPos":[220.96875,-124.81250,-12.59375], "SystemAllegiance":"Independent", "Population":0, "Body":"Synuefe BR-L c24-23 A", "BodyID":1, "BodyType":"Star", "JumpDist":9.851, "FuelUsed":0.484949, "FuelLevel":7.515051, "Factions":[ { "Name":"Sacra Oculus", "FactionState":"None", "Influence":0.16, "MyReputation":12.5 } ]"#;
const DISCOVERY: &str = r#""event":"FSSDiscoveryScan", "Progress":0.465262, "BodyCount":22, "NonBodyCount":27, "SystemName":"Synuefe BR-L c24-23", "SystemAddress":6406446944978"#;
const MUSIC: &str = r#""event":"Music", "MusicTrack":"Supercruise""#;
const ORGANIC: &str = r#""event":"ScanOrganic", "ScanType":"Log", "Genus":"$Codex_Ent_Stratum_Genus_Name;", "Genus_Localised":"Stratum", "Species":"$Codex_Ent_Stratum_07_Name;", "Species_Localised":"Stratum Tectonicas", "Variant":"$Codex_Ent_Stratum_07_K_Name;", "Variant_Localised":"Stratum Tectonicas - Lime", "WasLogged":false, "SystemAddress":6406446944978, "Body":6"#;

fn live_event(body: &str, ts: Timestamp) -> LiveEvent {
    let line = format!(r#"{{ "timestamp":"{}", {body} }}"#, ts.to_iso());
    let raw = RawEvent::new(&line, LineSource::default()).expect("test line");
    let event = Event::parse(&raw.name, &raw.line).unwrap_or(Event::Other);
    LiveEvent { raw, event }
}

struct Harness {
    rt: tokio::runtime::Runtime,
    dir: TempDir,
    transport: Arc<RecordingTransport>,
    uplink: Option<Uplink>,
    tracker: ContextTracker,
}

impl Harness {
    fn new(cfg: UplinkConfig, transport: Arc<RecordingTransport>) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let dir = TempDir::new("service");
        let uplink = Uplink::with_transport(cfg, &dir.0, rt.handle(), transport.clone());
        Harness { rt, dir, transport, uplink: Some(uplink), tracker: ContextTracker::new() }
    }

    fn uplink(&self) -> &Uplink {
        self.uplink.as_ref().unwrap()
    }

    fn send_at(&mut self, body: &str, ts: Timestamp) {
        let ev = live_event(body, ts);
        self.tracker.apply(&ev.raw);
        let ctx = self.tracker.context().clone();
        self.uplink().on_event(&ev, &ctx);
    }

    fn send(&mut self, body: &str) {
        self.send_at(body, Timestamp::now());
    }

    fn session(&mut self) {
        self.send(FILEHEADER);
        self.send(LOADGAME);
        self.send(FSDJUMP);
    }

    /// Polls until `pred` holds for the recorded requests (5 s max).
    fn wait_for(&self, pred: impl Fn(&[HttpRequest]) -> bool) -> bool {
        for _ in 0..500 {
            if pred(&self.transport.requests()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// Lets the uplink run for a while (to prove that something does *not* happen).
    fn settle(&self) {
        std::thread::sleep(Duration::from_millis(300));
    }

    fn restart(&mut self, cfg: UplinkConfig, transport: Arc<RecordingTransport>) {
        drop(self.uplink.take());
        self.transport = transport.clone();
        self.uplink = Some(Uplink::with_transport(cfg, &self.dir.0, self.rt.handle(), transport));
    }
}

fn eddn_cfg() -> UplinkConfig {
    UplinkConfig { eddn: true, eddn_test_mode: true, ..UplinkConfig::default() }
}

fn eddn_bodies(reqs: &[HttpRequest]) -> Vec<Value> {
    reqs.iter().filter(|r| r.url == eddn::UPLOAD_URL).map(|r| serde_json::from_str(&r.body_text()).unwrap()).collect()
}

#[test]
fn production_timings_respect_service_limits() {
    assert!(PRODUCTION.eddn_spacing >= Duration::from_millis(400), "EDDN: ~2 messages per second at most");
    assert!(EDDN_BACKOFF[0] >= 60 && PRODUCTION.backoff_unit == Duration::from_secs(1), "EDDN: retry after ≥ 1 minute");
    assert!(PRODUCTION.inara_min_gap >= Duration::from_secs(35), "Inara: ≤ 2 requests a minute");
    assert!(PRODUCTION.inara_lockout >= Duration::from_secs(3600));
    assert!(PRODUCTION.edsm_min_gap >= Duration::from_secs(10));
    assert!(PRODUCTION.edastro_min_gap >= Duration::from_secs(10));
    assert!(INARA_BACKOFF[0] >= 35 && EDSM_BACKOFF[0] >= 30);
}

#[test]
fn nothing_is_sent_by_default() {
    let mut h = Harness::new(UplinkConfig::default(), RecordingTransport::simulated());
    h.session();
    h.send(DISCOVERY);
    h.send(ORGANIC);
    h.uplink().flush();
    h.settle();
    assert!(h.transport.requests().is_empty(), "no request of any kind (not even list downloads)");
    let st = h.uplink().status();
    assert!(!st.eddn.enabled && !st.edsm.enabled && !st.inara.enabled && !st.edastro.enabled);
}

#[test]
fn eddn_end_to_end() {
    let mut h = Harness::new(eddn_cfg(), RecordingTransport::simulated());
    h.session();
    h.send(DISCOVERY);
    assert!(h.wait_for(|r| eddn_bodies(r).len() >= 2));
    let reqs = h.transport.requests_to("eddn.edcd.io");
    for r in &reqs {
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.url, "https://eddn.edcd.io:4430/upload/");
        assert_eq!(r.header_value("Content-Encoding"), Some("gzip"));
        assert_eq!(r.header_value("Content-Type"), Some("application/json"));
        assert!(gunzip(&r.body).is_ok(), "body is gzip");
    }
    let bodies = eddn_bodies(&reqs);
    for b in &bodies {
        let (schema, test) = Schema::from_schema_ref(b["$schemaRef"].as_str().unwrap()).unwrap();
        assert!(test, "test mode uses /test schemas");
        let errors = crate::eddn::tests::validate(schema, b);
        assert!(errors.is_empty(), "{errors:?}");
    }
    assert_eq!(bodies[0]["message"]["event"], "FSDJump");
    assert_eq!(bodies[1]["message"]["event"], "FSSDiscoveryScan");
    assert!(h.wait_for(|_| h.uplink().status().eddn.sent == 2));
    let st = h.uplink().status();
    assert_eq!(st.eddn.queued, 0);
    assert!(st.eddn.last_success.is_some());
}

#[test]
fn history_is_never_uploaded() {
    let mut h = Harness::new(eddn_cfg(), RecordingTransport::simulated());
    let old = Timestamp::parse("2026-09-05T01:06:02Z").unwrap();
    for body in [FILEHEADER, LOADGAME, FSDJUMP, DISCOVERY] {
        h.send_at(body, old);
    }
    h.uplink().flush();
    h.settle();
    assert!(h.transport.requests().is_empty());
}

#[test]
fn eddn_rejections_are_dropped_not_retried() {
    let t = RecordingTransport::new(|r| {
        if r.url.contains("eddn") { Ok(HttpResponse::new(400, "FAIL: Schema Validation: [...]")) } else { simulated_response(r) }
    });
    let mut h = Harness::new(eddn_cfg(), t);
    h.session();
    assert!(h.wait_for(|_| h.uplink().status().eddn.failed == 1));
    h.settle();
    assert_eq!(h.transport.requests_to("eddn").len(), 1, "a 400 is never retried");
    let st = h.uplink().status();
    assert_eq!(st.eddn.queued, 0);
    assert!(st.eddn.last_error.as_deref().unwrap_or_default().contains("HTTP 400"));
}

#[test]
fn eddn_transient_errors_back_off_then_give_up() {
    let t = RecordingTransport::new(|r| {
        if r.url.contains("eddn") { Ok(HttpResponse::new(503, "Service Unavailable")) } else { simulated_response(r) }
    });
    let mut h = Harness::new(eddn_cfg(), t);
    h.session();
    assert!(h.wait_for(|r| r.len() >= EDDN_MAX_ATTEMPTS as usize));
    assert!(h.wait_for(|_| h.uplink().status().eddn.failed == 1));
    h.settle();
    assert_eq!(h.transport.requests_to("eddn").len(), EDDN_MAX_ATTEMPTS as usize, "bounded retries");
    assert_eq!(h.uplink().status().eddn.queued, 0);
}

#[test]
fn eddn_repeated_403_stops_until_settings_change() {
    let t = RecordingTransport::new(|r| if r.url.contains("eddn") { Ok(HttpResponse::new(403, "Forbidden")) } else { simulated_response(r) });
    let mut h = Harness::new(eddn_cfg(), t);
    h.session();
    for _ in 0..4 {
        h.send(DISCOVERY);
    }
    assert!(h.wait_for(|r| r.len() >= 3));
    h.settle();
    assert_eq!(h.transport.requests().len(), 3, "stopped after three 403s");
    assert!(h.uplink().status().eddn.paused.is_some());
    h.uplink().set_config(eddn_cfg());
    assert!(h.wait_for(|r| r.len() >= 4), "resumes after a settings change");
}

#[test]
fn queue_survives_a_restart() {
    let offline = RecordingTransport::new(|_| Err(TransportError("offline".into())));
    let mut h = Harness::new(eddn_cfg(), offline);
    h.session();
    assert!(h.wait_for(|r| !r.is_empty()));
    assert_eq!(h.uplink().status().eddn.queued, 1);
    h.restart(eddn_cfg(), RecordingTransport::simulated());
    assert!(h.wait_for(|r| eddn_bodies(r).len() == 1), "the queued message is sent after the restart");
    assert!(h.wait_for(|_| h.uplink().status().eddn.queued == 0));
}

#[test]
fn disabling_a_service_discards_its_queue() {
    let offline = RecordingTransport::new(|_| Err(TransportError("offline".into())));
    let mut h = Harness::new(eddn_cfg(), offline);
    h.session();
    assert!(h.wait_for(|_| h.uplink().status().eddn.queued == 1));
    h.uplink().set_config(UplinkConfig::default());
    assert_eq!(h.uplink().status().eddn.queued, 0);
}

fn edsm_cfg() -> UplinkConfig {
    UplinkConfig {
        edsm: Some(EdsmCredentials { commander_name: "Jameson".into(), api_key: "edsm-secret-key".into() }),
        ..UplinkConfig::default()
    }
}

#[test]
fn edsm_batches_with_transient_fields() {
    let mut h = Harness::new(edsm_cfg(), RecordingTransport::simulated());
    h.session();
    h.send(MUSIC);
    h.send(DISCOVERY);
    h.uplink().flush();
    let posted = |reqs: &[HttpRequest]| -> Vec<Value> {
        reqs.iter()
            .filter(|r| r.url == edsm::JOURNAL_URL)
            .flat_map(|p| {
                let f = form_urldecode(&p.body_text());
                let m = f.into_iter().find(|(k, _)| k == "message").map(|(_, v)| v).unwrap_or_default();
                serde_json::from_str::<Vec<Value>>(&m).unwrap_or_default()
            })
            .collect()
    };
    assert!(h.wait_for(|r| posted(r).iter().filter(|e| e["event"] == "LoadGame" || e["event"] == "FSDJump" || e["event"] == "FSSDiscoveryScan").count() == 3));
    let reqs = h.transport.requests();
    assert_eq!(reqs[0].url, edsm::DISCARD_URL, "discard list fetched before the first upload");
    let posts: Vec<_> = reqs.iter().filter(|r| r.url == edsm::JOURNAL_URL).collect();
    let form = form_urldecode(&posts[0].body_text());
    let get = |k: &str| form.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
    assert_eq!(get("commanderName"), "Jameson");
    assert_eq!(get("apiKey"), "edsm-secret-key");
    assert_eq!(get("fromSoftware"), "Elite Nexus");
    assert_eq!(get("fromGameVersion"), "4.4.1.1");
    assert_eq!(get("fromGameBuild"), "r332841/r0 ", "build sent byte-exact");
    let events: Vec<Value> = posts.iter().flat_map(|p| {
        let f = form_urldecode(&p.body_text());
        let m = f.into_iter().find(|(k, _)| k == "message").map(|(_, v)| v).unwrap_or_default();
        serde_json::from_str::<Vec<Value>>(&m).unwrap()
    }).collect();
    let names: Vec<&str> = events.iter().filter_map(|e| e["event"].as_str()).collect();
    assert!(!names.contains(&"Fileheader") && !names.contains(&"Music"), "discarded events are not sent: {names:?}");
    assert!(names.contains(&"LoadGame") && names.contains(&"FSDJump") && names.contains(&"FSSDiscoveryScan"));
    let jump = events.iter().find(|e| e["event"] == "FSDJump").unwrap();
    assert_eq!(jump["_systemName"], "Synuefe BR-L c24-23");
    assert_eq!(jump["_systemAddress"], 6_406_446_944_978u64);
    assert_eq!(jump["_shipId"], 22);
    assert_eq!(jump["_marketId"], Value::Null);
    assert_eq!(jump["JumpDist"], 9.851, "EDSM gets the event unmodified");
    // The API key is never written to disk.
    let on_disk = std::fs::read_to_string(h.dir.0.join("uplink").join("edsm.jsonl")).unwrap_or_default();
    assert!(!on_disk.contains("edsm-secret-key"));
}

#[test]
fn edsm_bad_credentials_stop_until_settings_change() {
    let t = RecordingTransport::new(|r| {
        if r.url == edsm::JOURNAL_URL {
            Ok(HttpResponse::new(200, r#"{"msgnum":203,"msg":"Commander name/API Key not found","events":[]}"#))
        } else {
            simulated_response(r)
        }
    });
    let mut h = Harness::new(edsm_cfg(), t);
    h.session();
    h.uplink().flush();
    assert!(h.wait_for(|r| r.iter().any(|q| q.url == edsm::JOURNAL_URL)));
    h.send(DISCOVERY);
    h.uplink().flush();
    h.settle();
    let posts = h.transport.requests().iter().filter(|r| r.url == edsm::JOURNAL_URL).count();
    assert_eq!(posts, 1, "no more posts with bad credentials");
    let st = h.uplink().status();
    assert!(st.edsm.paused.is_some());
    assert!(st.edsm.queued > 0, "events are kept for when the key is fixed");
}

fn inara_cfg() -> UplinkConfig {
    UplinkConfig { inara: Some(InaraCredentials { api_key: "inara-secret".into(), is_being_developed: true }), ..UplinkConfig::default() }
}

#[test]
fn inara_posts_batches_with_header() {
    let mut h = Harness::new(inara_cfg(), RecordingTransport::simulated());
    h.session();
    h.uplink().flush();
    assert!(h.wait_for(|r| r.iter().any(|q| q.url == inara::API_URL)));
    let post = &h.transport.requests_to(inara::API_URL)[0];
    let body: Value = serde_json::from_str(&post.body_text()).unwrap();
    assert_eq!(body["header"]["appName"], "Elite Nexus");
    assert_eq!(body["header"]["isBeingDeveloped"], true);
    assert_eq!(body["header"]["APIkey"], "inara-secret");
    assert_eq!(body["header"]["commanderName"], "Jameson");
    assert_eq!(body["header"]["commanderFrontierID"], "F1");
    let names: Vec<&str> = body["events"].as_array().unwrap().iter().filter_map(|e| e["eventName"].as_str()).collect();
    assert!(names.contains(&"addCommanderTravelFSDJump"), "{names:?}");
    assert!(names.contains(&"setCommanderReputationMinorFaction"));
}

#[test]
fn inara_rate_limit_pauses_for_an_hour() {
    let t = RecordingTransport::new(|r| {
        if r.url == inara::API_URL {
            Ok(HttpResponse::new(200, r#"{"header":{"eventStatus":400,"eventStatusText":"Too much requests, slow down, cowboy. ;)"}}"#))
        } else {
            simulated_response(r)
        }
    });
    let mut h = Harness::new(inara_cfg(), t);
    h.session();
    h.uplink().flush();
    assert!(h.wait_for(|r| !r.is_empty()));
    h.send(FSDJUMP);
    h.uplink().flush();
    h.settle();
    assert_eq!(h.transport.requests().len(), 1);
    let st = h.uplink().status();
    assert!(st.inara.paused.is_some());
    assert!(st.inara.queued > 0, "events kept for after the lock-out");
}

#[test]
fn edastro_sends_accepted_whitelisted_events_only() {
    let mut h = Harness::new(UplinkConfig { edastro: true, ..UplinkConfig::default() }, RecordingTransport::simulated());
    h.session();
    h.send(ORGANIC);
    h.send(DISCOVERY);
    h.uplink().flush();
    assert!(h.wait_for(|r| r.iter().any(|q| q.url == edastro::JOURNAL_URL)));
    let reqs = h.transport.requests();
    assert_eq!(reqs[0].url, edastro::ACCEPTING_URL);
    let post = reqs.iter().find(|r| r.url == edastro::JOURNAL_URL).unwrap();
    let body: Vec<Value> = serde_json::from_str(&post.body_text()).unwrap();
    assert_eq!(body[0]["appName"], "Elite Nexus");
    assert_eq!(body[0]["system"], "Synuefe BR-L c24-23");
    assert_eq!(body[0]["odyssey"], true);
    assert_eq!(body.len(), 2, "only the ScanOrganic: {body:?}");
    assert_eq!(body[1]["event"], "ScanOrganic");
    assert!(body[1].get("WasLogged").is_none());
}

#[test]
fn beta_clients_only_use_test_schemas_and_skip_journal_sync() {
    let cfg = UplinkConfig { eddn: true, edsm: edsm_cfg().edsm, inara: inara_cfg().inara, edastro: true, ..UplinkConfig::default() };
    let mut h = Harness::new(cfg, RecordingTransport::simulated());
    h.send(&FILEHEADER.replace("4.4.1.1", "4.5.0.100 Beta"));
    h.send(&LOADGAME.replace("4.4.1.1", "4.5.0.100 Beta"));
    h.send(FSDJUMP);
    h.send(ORGANIC);
    h.uplink().flush();
    assert!(h.wait_for(|r| eddn_bodies(r).len() >= 2));
    h.settle();
    let reqs = h.transport.requests();
    assert!(reqs.iter().all(|r| r.url.contains("eddn")), "beta data never goes to EDSM/Inara/EDAstro: {:?}", reqs.iter().map(|r| &r.url).collect::<Vec<_>>());
    for b in eddn_bodies(&reqs) {
        assert!(b["$schemaRef"].as_str().unwrap().ends_with("/test"));
    }
}

#[test]
fn multicrew_is_never_uploaded() {
    let cfg = UplinkConfig { eddn: true, edsm: edsm_cfg().edsm, ..UplinkConfig::default() };
    let mut h = Harness::new(cfg, RecordingTransport::simulated());
    h.send(FILEHEADER);
    h.send(LOADGAME);
    h.send(r#""event":"JoinACrew", "Captain":"Someone Else""#);
    h.send(FSDJUMP);
    h.send(DISCOVERY);
    h.uplink().flush();
    h.settle();
    assert!(h.transport.requests_to("eddn").is_empty());
    let edsm_events: usize = h
        .transport
        .requests()
        .iter()
        .filter(|r| r.url == edsm::JOURNAL_URL)
        .map(|p| {
            let f = form_urldecode(&p.body_text());
            let m = f.into_iter().find(|(k, _)| k == "message").map(|(_, v)| v).unwrap_or_default();
            serde_json::from_str::<Vec<Value>>(&m).map(|v| v.iter().filter(|e| e["event"] == "FSDJump").count()).unwrap_or(0)
        })
        .sum();
    assert_eq!(edsm_events, 0);
}

#[test]
fn credentials_are_bound_to_one_commander() {
    let mut h = Harness::new(edsm_cfg(), RecordingTransport::simulated());
    h.session();
    h.uplink().flush();
    assert!(h.wait_for(|r| r.iter().any(|q| q.url == edsm::JOURNAL_URL)));
    // The main commander's events may go out in more than one batch: wait until all are sent.
    for _ in 0..500 {
        if h.uplink().status().edsm.queued == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    h.settle();
    let posts_before = h.transport.requests().iter().filter(|r| r.url == edsm::JOURNAL_URL).count();
    // An alt logs in: its journal must never reach Jameson's EDSM account.
    h.send(FILEHEADER);
    h.send(&LOADGAME.replace("Jameson", "Alt Account"));
    h.send(FSDJUMP);
    h.uplink().flush();
    h.settle();
    assert_eq!(h.transport.requests().iter().filter(|r| r.url == edsm::JOURNAL_URL).count(), posts_before);
    assert!(h.uplink().status().edsm.last_error.unwrap_or_default().contains("Alt Account"));

    // Inara has no account name: the first commander seen is bound.
    let mut h = Harness::new(inara_cfg(), RecordingTransport::simulated());
    h.session();
    h.send(FILEHEADER);
    h.send(&LOADGAME.replace("Jameson", "Alt Account"));
    h.send(FSDJUMP);
    h.uplink().flush();
    assert!(h.wait_for(|r| r.iter().any(|q| q.url == inara::API_URL)));
    h.settle();
    for post in h.transport.requests_to(inara::API_URL) {
        let body: Value = serde_json::from_str(&post.body_text()).unwrap();
        assert_eq!(body["header"]["commanderName"], "Jameson");
    }
}

#[test]
fn binding_rules() {
    let mut b = Bindings::default();
    // First commander seen is bound...
    assert!(b.allow("edsm:1".into(), "Alt", Some("Main"), None).is_ok());
    assert_eq!(b.allow("edsm:1".into(), "Other", Some("Main"), None), Err("Alt".to_string()));
    // ...but the commander named like the EDSM account always takes over.
    assert!(b.allow("edsm:1".into(), "main", Some("Main"), None).is_ok());
    assert!(b.allow("edsm:1".into(), "Alt", Some("Main"), None).is_err());
    // Different credentials are bound separately.
    assert!(b.allow("inara:2".into(), "Alt", None, None).is_ok());
}

#[test]
fn inara_lockout_survives_a_restart() {
    let limited = || {
        RecordingTransport::new(|r| {
            if r.url == inara::API_URL {
                Ok(HttpResponse::new(200, r#"{"header":{"eventStatus":400,"eventStatusText":"Too much requests, slow down, cowboy. ;)"}}"#))
            } else {
                simulated_response(r)
            }
        })
    };
    let mut h = Harness::new(inara_cfg(), limited());
    h.session();
    h.uplink().flush();
    assert!(h.wait_for(|r| !r.is_empty()));
    h.restart(inara_cfg(), RecordingTransport::simulated());
    h.send(FSDJUMP);
    h.uplink().flush();
    h.settle();
    assert!(h.transport.requests().is_empty(), "still locked out after a restart");
    assert!(h.uplink().status().inara.paused.is_some());
}

// ---- EDDN pause -----------------------------------------------------------------------------

const DOCKED_CARRIER: &str = r#""event":"Docked", "StationName":"B4Z-LQY", "StationType":"FleetCarrier", "Taxi":false, "Multicrew":false, "StarSystem":"Synuefe BR-L c24-23", "SystemAddress":6406446944978, "MarketID":3704011520, "StationFaction":{ "Name":"FleetCarrier" }, "StationGovernment":"$government_Carrier;", "StationGovernment_Localised":"Private Ownership", "StationServices":[ "dock", "autodock", "commodities", "contacts" ], "StationEconomy":"$economy_Carrier;", "StationEconomy_Localised":"Private Enterprise", "StationEconomies":[ { "Name":"$economy_Carrier;", "Name_Localised":"Private Enterprise", "Proportion":1.000000 } ], "DistFromStarLS":10.5, "LandingPads":{ "Small":4, "Medium":4, "Large":8 }"#;
const UNDOCKED_CARRIER: &str = r#""event":"Undocked", "StationName":"B4Z-LQY", "StationType":"FleetCarrier", "MarketID":3704011520, "Taxi":false, "Multicrew":false"#;

impl Harness {
    fn companion(&mut self, kind: CompanionKind, text: &str) {
        let c = Companion::parse(kind, text).expect("companion parses");
        let ctx = self.tracker.context().clone();
        self.uplink().on_companion(&c, &ctx);
    }

    /// Opens the carrier's commodity market: the journal `Market` event plus Market.json.
    fn open_carrier_market(&mut self) {
        let now = Timestamp::now();
        self.send_at(r#""event":"Market", "MarketID":3704011520, "StationName":"B4Z-LQY", "StationType":"FleetCarrier", "CarrierDockingAccess":"all", "StarSystem":"Synuefe BR-L c24-23""#, now);
        let file = format!(
            r#"{{ "timestamp":"{}", "event":"Market", "MarketID":3704011520, "StationName":"B4Z-LQY", "StationType":"FleetCarrier", "CarrierDockingAccess":"all", "StarSystem":"Synuefe BR-L c24-23", "Items":[ {{ "id":128668547, "Name":"$unknownartifact_name;", "Name_Localised":"Thargoid Sensor", "Category":"$MARKET_category_salvage;", "Category_Localised":"Salvage", "BuyPrice":0, "SellPrice":30418000, "MeanPrice":0, "StockBracket":0, "DemandBracket":2, "Stock":0, "Demand":15, "Consumer":true, "Producer":false, "Rare":false }} ] }}"#,
            now.to_iso()
        );
        self.companion(CompanionKind::Market, &file);
    }
}

fn commodity_count(reqs: &[HttpRequest]) -> usize {
    eddn_bodies(reqs).iter().filter(|b| b["$schemaRef"].as_str().is_some_and(|s| s.contains("/commodity/"))).count()
}

fn paused(on: bool) -> UplinkConfig {
    UplinkConfig { eddn_paused: on, ..eddn_cfg() }
}

#[test]
fn eddn_market_data_waits_until_undocking() {
    let mut h = Harness::new(eddn_cfg(), RecordingTransport::simulated());
    h.session();
    h.send(DOCKED_CARRIER);
    h.open_carrier_market();
    assert!(h.wait_for(|r| eddn_bodies(r).iter().any(|b| b["message"]["event"] == "Docked")), "the docking itself is shared at once");
    h.settle();
    assert_eq!(commodity_count(&h.transport.requests()), 0, "prices are held while docked");
    h.send(UNDOCKED_CARRIER);
    assert!(h.wait_for(|r| commodity_count(r) == 1), "and sent after undocking");
}

#[test]
fn eddn_pause_while_docked_keeps_the_market_private() {
    let mut h = Harness::new(eddn_cfg(), RecordingTransport::simulated());
    h.session();
    h.send(DOCKED_CARRIER);
    h.open_carrier_market();
    // The commander spots a deal and presses Pause before leaving.
    assert!(h.wait_for(|r| eddn_bodies(r).iter().any(|b| b["message"]["event"] == "Docked")));
    h.uplink().set_config(paused(true));
    h.settle();
    let before = eddn_bodies(&h.transport.requests()).len();
    h.send(UNDOCKED_CARRIER);
    h.send(FSDJUMP);
    h.uplink().flush();
    h.settle();
    let reqs = h.transport.requests();
    assert_eq!(commodity_count(&reqs), 0, "held prices were dropped, not sent");
    assert_eq!(eddn_bodies(&reqs).len(), before, "nothing at all is sent while paused");
    assert!(!h.uplink().status().eddn.enabled, "status shows EDDN as not sending");
    // Resume: new data flows again.
    h.uplink().set_config(paused(false));
    h.send(FSDJUMP);
    assert!(h.wait_for(|r| eddn_bodies(r).len() > before), "sending resumes");
}

// ---- EDSM, Inara and EDAstro pause ----------------------------------------------------------

/// The journal events EDSM was sent.
fn edsm_events(reqs: &[HttpRequest]) -> Vec<String> {
    reqs.iter()
        .filter(|r| r.url == edsm::JOURNAL_URL)
        .flat_map(|p| {
            let f = form_urldecode(&p.body_text());
            let m = f.into_iter().find(|(k, _)| k == "message").map(|(_, v)| v).unwrap_or_default();
            serde_json::from_str::<Vec<Value>>(&m).unwrap_or_default()
        })
        .filter_map(|e| e["event"].as_str().map(str::to_string))
        .collect()
}

/// The event names Inara was sent.
fn inara_events(reqs: &[HttpRequest]) -> Vec<String> {
    reqs.iter()
        .filter(|r| r.url == inara::API_URL)
        .flat_map(|p| serde_json::from_str::<Value>(&p.body_text()).ok().and_then(|b| b["events"].as_array().cloned()).unwrap_or_default())
        .filter_map(|e| e["eventName"].as_str().map(str::to_string))
        .collect()
}

/// Paused as EDDN is: nothing goes out while paused (not even a key check or a list download), what was waiting is
/// dropped, and what happened meanwhile isn't sent after resuming either.
#[test]
fn edsm_inara_and_edastro_pause_like_eddn() {
    let on = UplinkConfig { edsm: edsm_cfg().edsm, inara: inara_cfg().inara, edastro: true, ..UplinkConfig::default() };
    let pause = |p: bool| UplinkConfig { edsm_paused: p, inara_paused: p, edastro_paused: p, ..on.clone() };
    let mut h = Harness::new(pause(true), RecordingTransport::simulated());
    h.session();
    h.send(DISCOVERY);
    h.send(ORGANIC);
    h.uplink().flush();
    h.settle();
    assert!(h.transport.requests().is_empty(), "nothing at all goes out while paused");
    let st = h.uplink().status();
    assert_eq!((st.edsm.queued, st.inara.queued, st.edastro.queued), (0, 0, 0), "nothing is kept to send later");
    assert!(!st.edsm.enabled && !st.inara.enabled && !st.edastro.enabled, "status shows them as not sending");

    h.uplink().set_config(pause(false));
    h.send(FSDJUMP);
    h.send(ORGANIC);
    h.uplink().flush();
    assert!(h.wait_for(|r| edsm_events(r).iter().any(|e| e == "FSDJump") && !inara_events(r).is_empty() && r.iter().any(|q| q.url == edastro::JOURNAL_URL)));
    h.settle();
    let reqs = h.transport.requests();
    let edsm = edsm_events(&reqs);
    assert!(!edsm.iter().any(|e| e == "LoadGame" || e == "FSSDiscoveryScan"), "only what happened after resuming: {edsm:?}");
    let inara = inara_events(&reqs);
    assert!(inara.iter().any(|e| e == "addCommanderTravelFSDJump"), "{inara:?}");
    assert!(!inara.iter().any(|e| e == "setCommanderCredits"), "the login during the pause isn't sent: {inara:?}");
    let edastro: Vec<Value> = reqs.iter().filter(|r| r.url == edastro::JOURNAL_URL).flat_map(|p| serde_json::from_str::<Vec<Value>>(&p.body_text()).unwrap_or_default()).collect();
    assert_eq!(edastro.iter().filter(|e| e["event"] == "ScanOrganic").count(), 1, "one scan after resuming, none from the pause");
}

/// Pausing drops what was queued but not sent yet, as switching a service off does.
#[test]
fn pausing_drops_the_queue() {
    let mut h = Harness::new(inara_cfg(), RecordingTransport::simulated());
    h.session();
    // The test timings gather Inara's events until a flush: they wait in the queue.
    assert!(h.wait_for(|_| h.uplink().status().inara.queued > 0));
    h.uplink().set_config(UplinkConfig { inara_paused: true, ..inara_cfg() });
    h.uplink().flush();
    h.settle();
    assert_eq!(h.uplink().status().inara.queued, 0);
    assert!(inara_events(&h.transport.requests()).is_empty(), "nothing of it was sent");
}

// ---- Inara: who is this key, and syncing the current profile -------------------------------

fn wait_status(h: &Harness, pred: impl Fn(&ServiceStatus) -> bool) -> ServiceStatus {
    for _ in 0..500 {
        let st = h.uplink().status().inara;
        if pred(&st) {
            return st;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    h.uplink().status().inara
}

#[test]
fn inara_check_rides_along_with_the_first_batch() {
    let mut h = Harness::new(inara_cfg(), RecordingTransport::simulated());
    assert!(h.uplink().status().inara.checking, "a configured key is confirmed every run");
    h.session();
    h.uplink().flush();
    let st = wait_status(&h, |s| s.account.is_some());
    let account = st.account.clone().expect("account confirmed");
    assert_eq!(account.name, "Jameson");
    assert_eq!(account.url.as_deref(), Some("https://inara.cz/elite/cmdr/1/"));
    assert!(!st.checking);
    let posts = h.transport.requests_to(inara::API_URL);
    assert_eq!(posts.len(), 1, "no extra request for the check");
    let body: Value = serde_json::from_str(&posts[0].body_text()).unwrap();
    assert_eq!(body["events"][0]["eventName"], "getCommanderProfile");
    assert_eq!(st.failed, 0);
    assert!(st.sent > 0 && st.last_error.is_none(), "{st:?}");
}

#[test]
fn inara_new_key_is_checked_at_once() {
    let h = Harness::new(UplinkConfig::default(), RecordingTransport::simulated());
    h.uplink().set_config(inara_cfg());
    let st = wait_status(&h, |s| s.account.is_some());
    assert_eq!(st.account.map(|a| a.name), Some("Jameson".into()));
    let body: Value = serde_json::from_str(&h.transport.requests_to(inara::API_URL)[0].body_text()).unwrap();
    assert_eq!(body["events"].as_array().map(Vec::len), Some(1), "just the check");
    // Removing the key forgets the account.
    h.uplink().set_config(UplinkConfig::default());
    assert!(h.uplink().status().inara.account.is_none());
}

#[test]
fn inara_check_again_right_after_a_request_is_still_sent() {
    let h = Harness::new(UplinkConfig::default(), RecordingTransport::simulated());
    h.uplink().set_config(inara_cfg());
    assert!(h.wait_for(|r| r.iter().any(|q| q.url == inara::API_URL)));
    // "Check again" within the request spacing: the check waits it out, then goes.
    h.uplink().check_inara();
    assert!(h.wait_for(|r| r.iter().filter(|q| q.url == inara::API_URL).count() >= 2), "second check never sent");
    let posts = h.transport.requests_to(inara::API_URL);
    let body: Value = serde_json::from_str(&posts[1].body_text()).unwrap();
    assert_eq!(body["events"][0]["eventName"], "getCommanderProfile");
    let st = wait_status(&h, |s| !s.checking);
    assert_eq!(st.account.map(|a| a.name), Some("Jameson".into()));
}

#[test]
fn inara_rejected_key_is_reported() {
    let t = RecordingTransport::new(|r| {
        if r.url == inara::API_URL {
            Ok(HttpResponse::new(200, r#"{"header":{"eventStatus":400,"eventStatusText":"Invalid API key."}}"#))
        } else {
            simulated_response(r)
        }
    });
    let h = Harness::new(UplinkConfig::default(), t);
    h.uplink().set_config(inara_cfg());
    let st = wait_status(&h, |s| s.paused.is_some());
    assert!(st.paused.unwrap_or_default().contains("Invalid API key"));
    assert!(st.account.is_none() && !st.checking);
}

#[test]
fn inara_profile_snapshot_sends_current_state_only() {
    let h = Harness::new(inara_cfg(), RecordingTransport::simulated());
    // A session that started three hours ago (live syncing would refuse events that old).
    let then = Timestamp(Timestamp::now().0 - 3 * 3600);
    let bodies = [
        FILEHEADER,
        LOADGAME,
        r#""event":"Rank", "Combat":1, "Trade":4, "Explore":6, "Soldier":0, "Exobiologist":2, "Empire":7, "Federation":1, "CQC":0"#,
        r#""event":"Progress", "Combat":30, "Trade":46, "Explore":78, "Soldier":0, "Exobiologist":35, "Empire":37, "Federation":100, "CQC":0"#,
        FSDJUMP,
        r#""event":"Cargo", "Vessel":"Ship", "Count":0, "Inventory":[ ]"#,
    ];
    let events: Vec<RawEvent> = bodies.iter().map(|b| live_event(b, then).raw).collect();
    let mut tracker = ContextTracker::new();
    for e in &events {
        tracker.apply(e);
    }
    h.uplink().sync_inara_profile(events, tracker.context());
    assert!(h.wait_for(|r| r.iter().any(|q| q.url == inara::API_URL)));
    h.settle();
    let names: Vec<String> = h
        .transport
        .requests_to(inara::API_URL)
        .iter()
        .flat_map(|p| {
            let v: Value = serde_json::from_str(&p.body_text()).unwrap();
            v["events"].as_array().cloned().unwrap_or_default()
        })
        .filter_map(|e| e["eventName"].as_str().map(str::to_string))
        .filter(|n| n != "getCommanderProfile")
        .collect();
    assert!(names.contains(&"setCommanderRankPilot".to_string()), "{names:?}");
    assert!(names.iter().all(|n| n.starts_with("set")), "only current state, no past activity: {names:?}");
    assert!(h.transport.requests().iter().all(|r| r.url == inara::API_URL), "nothing goes to EDDN or EDSM");
}
