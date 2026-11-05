//! Raw JSON (EDDN envelopes, journal events, Market.json) -> [`Ingest`] items.
//!
//! Everything here treats its input as untrusted: missing or mistyped fields drop the field (or the
//! item), never panic. EDDN journal messages have had their `*_Localised` keys stripped, so every
//! name goes through [`crate::names`] to get the same display strings as Spansh.

use crate::commodities::normalize_symbol;
use crate::model::*;
use crate::names;
use crate::services::Services;
use crate::util::*;
use serde_json::Value;

/// Seconds a message may be ahead of the EDDN gateway timestamp before it is clamped.
const GATEWAY_FUTURE_TOLERANCE: i64 = 300;
/// Seconds a message may be ahead of the local clock before it is clamped.
const LOCAL_FUTURE_TOLERANCE: i64 = 600;

/// `https://eddn.edcd.io/schemas/commodity/3` -> `commodity/3`; `None` for `/test` schemas and
/// anything that doesn't look like an EDDN schema reference.
pub fn eddn_schema_name(schema_ref: &str) -> Option<&str> {
    let s = schema_ref.trim().trim_end_matches('/');
    if s.ends_with("/test") {
        return None;
    }
    let i = s.find("/schemas/")?;
    let rest = &s[i + "/schemas/".len()..];
    (!rest.is_empty()).then_some(rest)
}

impl Ingest {
    /// Converts one EDDN envelope. Handles `commodity/3`, `journal/1` (`FSDJump`, `Location`,
    /// `CarrierJump`, `Docked`, `SAASignalsFound`, `Scan`), `fssbodysignals/1`, `scanorganic/1` and
    /// `approachsettlement/1`; returns nothing for `/test` schemas, other schemas and malformed
    /// messages. From Legacy-galaxy clients (game 3.x) body data is dropped and systems keep only
    /// their position: discovery flags, factions and conflicts there belong to another galaxy.
    pub fn from_eddn(envelope: &Value) -> Vec<Ingest> {
        let Some(schema) = envelope.get("$schemaRef").and_then(Value::as_str) else {
            return Vec::new();
        };
        let Some(name) = eddn_schema_name(schema) else {
            return Vec::new();
        };
        let Some(msg) = envelope.get("message").filter(|m| m.is_object()) else {
            return Vec::new();
        };
        let header = envelope.get("header");
        let gateway = header.and_then(|h| get_ts(h, "gatewayTimestamp"));
        let mut items = match name {
            "commodity/3" => commodity_snapshot(msg, gateway)
                .map(Ingest::Market)
                .into_iter()
                .collect(),
            "journal/1" => match get_str(msg, "event") {
                Some(event) => convert_journal(event, msg, Source::Eddn, gateway),
                None => Vec::new(),
            },
            "fssbodysignals/1" => convert_journal("FSSBodySignals", msg, Source::Eddn, gateway),
            "scanorganic/1" => convert_journal("ScanOrganic", msg, Source::Eddn, gateway),
            "approachsettlement/1" => {
                convert_journal("ApproachSettlement", msg, Source::Eddn, gateway)
            }
            _ => Vec::new(),
        };
        if header
            .and_then(|h| get_str(h, "gameversion"))
            .is_some_and(is_legacy_galaxy)
        {
            // Its bodies' flags and its systems' factions and conflicts belong to that galaxy: keep only where
            // systems are.
            items.retain(|i| !matches!(i, Ingest::Body(_)));
            for item in &mut items {
                if let Ingest::System(s) = item {
                    s.details = None;
                }
            }
        }
        items
    }

    /// Converts one of our own journal events. `FSDJump` / `Location` / `CarrierJump` (system state,
    /// factions, powers; plus the station when docked), `Docked` (station details), `SAASignalsFound`
    /// (ring hotspots; planet signals and genera), `Scan` and `FSSBodySignals` (planets and moons),
    /// `Disembark` on a planet (footfall), `ApproachSettlement`, and `Market` **with** the `Items` of
    /// Market.json (pass the companion file's JSON as the event).
    pub fn from_journal_event(event: &str, json: &Value) -> Vec<Ingest> {
        convert_journal(event, json, Source::Journal, None)
    }

    /// Expands [`Ingest::Raw`] into concrete items; anything else is returned as is.
    pub fn expand(self) -> Vec<Ingest> {
        match self {
            Ingest::Raw(RawIngest::Eddn(v)) => Ingest::from_eddn(&v),
            Ingest::Raw(RawIngest::Journal { event, json }) => {
                Ingest::from_journal_event(&event, &json)
            }
            other => vec![other],
        }
    }
}

fn clamp_ts(ts: Option<i64>, gateway: Option<i64>) -> i64 {
    let now = now_unix();
    let mut t = ts.or(gateway).unwrap_or(now);
    if let Some(g) = gateway
        && t > g + GATEWAY_FUTURE_TOLERANCE
    {
        t = g;
    }
    if t > now + LOCAL_FUTURE_TOLERANCE {
        t = now;
    }
    t
}

fn bracket(v: Option<&Value>) -> Option<i8> {
    match v? {
        Value::String(s) if s.trim().is_empty() => Some(-1),
        other => as_i64(other).map(|n| n.clamp(-1, 3) as i8),
    }
}

fn price(v: &Value, key: &str) -> i64 {
    get_i64(v, key).unwrap_or(0).max(0)
}

fn commodity_snapshot(msg: &Value, gateway: Option<i64>) -> Option<MarketSnapshot> {
    let market_id = get_id(msg, "marketId")?;
    let system_name = get_text(msg, "systemName")?;
    let station_name = get_text(msg, "stationName")?;
    let list = msg.get("commodities")?.as_array()?;
    let mut items = Vec::with_capacity(list.len());
    for c in list {
        let Some(name) = get_str(c, "name") else {
            continue;
        };
        let symbol = normalize_symbol(name);
        if symbol.is_empty() {
            continue;
        }
        items.push(MarketItem {
            symbol,
            buy: price(c, "buyPrice"),
            sell: price(c, "sellPrice"),
            mean: price(c, "meanPrice"),
            stock: price(c, "stock"),
            stock_bracket: bracket(c.get("stockBracket")),
            demand: price(c, "demand"),
            demand_bracket: bracket(c.get("demandBracket")),
        });
    }
    let economies = msg.get("economies").and_then(Value::as_array).map(|a| {
        let mut e: Vec<(String, f64)> = a
            .iter()
            .filter_map(|x| {
                Some((
                    names::economy(get_str(x, "name")?)?,
                    get_f64(x, "proportion").unwrap_or(0.0),
                ))
            })
            .collect();
        sort_economies(&mut e);
        e
    });
    let prohibited =
        get_strings(msg, "prohibited").map(|v| v.iter().map(|s| normalize_symbol(s)).collect());
    Some(MarketSnapshot {
        market_id,
        system_name,
        station_name,
        station_type: get_text(msg, "stationType"),
        carrier_docking_access: get_text(msg, "carrierDockingAccess"),
        timestamp: clamp_ts(get_ts(msg, "timestamp"), gateway),
        items,
        prohibited,
        economies,
        source: Source::Eddn,
    })
}

fn sort_economies(e: &mut [(String, f64)]) {
    e.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
}

pub(crate) fn convert_journal(
    event: &str,
    v: &Value,
    source: Source,
    gateway: Option<i64>,
) -> Vec<Ingest> {
    if !v.is_object() {
        return Vec::new();
    }
    let ts = clamp_ts(get_ts(v, "timestamp"), gateway);
    match event {
        "FSDJump" | "Location" | "CarrierJump" => {
            // EDDN relays every commander's jumps (~250/min, mostly into empty systems). Keep what
            // the queries can use: populated systems, carrier jumps and arrivals docked at a
            // station; everything else would grow the database without bound.
            let docked = event != "FSDJump" && get_bool(v, "Docked") == Some(true);
            if source == Source::Eddn
                && event != "CarrierJump"
                && !docked
                && get_i64(v, "Population").unwrap_or(0) <= 0
            {
                return Vec::new();
            }
            let mut out = Vec::with_capacity(2);
            let Some(sys) = system_arrival(v, ts) else {
                return out;
            };
            let (id, name) = (sys.id64, sys.name.clone());
            out.push(Ingest::System(sys));
            if event != "FSDJump"
                && get_bool(v, "Docked") == Some(true)
                && let Some(st) = station_from_event(v, ts, Some(id), Some(name))
            {
                out.push(Ingest::Station(st));
            }
            out
        }
        "Docked" => {
            let mut out = Vec::with_capacity(2);
            let sys_id = get_id(v, "SystemAddress").filter(|a| *a > 1);
            let sys_name = get_text(v, "StarSystem");
            out.extend(position_only(v, ts, sys_id, sys_name.as_deref()));
            if let Some(st) = station_from_event(v, ts, sys_id, sys_name) {
                out.push(Ingest::Station(st));
            }
            out
        }
        "ApproachSettlement" => {
            let mut out = Vec::with_capacity(2);
            let sys_id = get_id(v, "SystemAddress").filter(|a| *a > 1);
            let sys_name = get_text(v, "StarSystem");
            out.extend(position_only(v, ts, sys_id, sys_name.as_deref()));
            if let Some(market_id) = get_id(v, "MarketID")
                && let Some(name) = get_text(v, "Name")
                && !name.starts_with('$')
            {
                let mut st = station_common(v, ts, market_id, name, sys_id, sys_name);
                st.planetary = Some(true);
                out.push(Ingest::Station(st));
            }
            out
        }
        "SAASignalsFound" => {
            let mut out = Vec::with_capacity(2);
            let sys_id = get_id(v, "SystemAddress").filter(|a| *a > 1);
            let sys_name = get_text(v, "StarSystem");
            out.extend(position_only(v, ts, sys_id, sys_name.as_deref()));
            match ring_signals(v, ts, sys_id, sys_name) {
                Some(r) => out.push(Ingest::RingSignals(r)),
                None => out.extend(body_signals(v, ts, source).map(Ingest::Body)),
            }
            out
        }
        // Planets and moons. No system rows from these: EDDN relays ~200 scans a minute, mostly of
        // unpopulated systems (the same volume FSDJump filtering keeps out).
        "Scan" => body_scan(v, ts, source)
            .map(Ingest::Body)
            .into_iter()
            .collect(),
        "FSSBodySignals" => body_signals(v, ts, source)
            .map(Ingest::Body)
            .into_iter()
            .collect(),
        // On foot on a planet: the body has been footfalled. `ScanOrganic` needs the genetic sampler
        // (EDDN `scanorganic/1`); its "Analyse" step can fire later, even in another system.
        "ScanOrganic" => {
            if get_str(v, "ScanType").is_some_and(|t| t.eq_ignore_ascii_case("Analyse")) {
                return Vec::new();
            }
            let body_id = get_body_id(v, "BodyID").or_else(|| get_body_id(v, "Body"));
            on_foot(v, ts, source, body_id, get_text(v, "BodyName"))
        }
        "Disembark" => {
            if get_bool(v, "OnPlanet") != Some(true) || get_bool(v, "OnStation") == Some(true) {
                return Vec::new();
            }
            on_foot(v, ts, source, get_body_id(v, "BodyID"), get_text(v, "Body"))
        }
        "Market" => market_json(v, ts, source)
            .map(Ingest::Market)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// Journal `SurfaceGravity` (m/s²) per g and `SurfacePressure` (Pa) per atmosphere: the standard
/// values, as Spansh and EDSM use.
const STANDARD_GRAVITY: f64 = 9.80665;
const STANDARD_ATMOSPHERE: f64 = 101_325.0;

/// Body ids start at 0 (the main star).
fn get_body_id(v: &Value, key: &str) -> Option<u32> {
    u32::try_from(get_i64(v, key)?).ok()
}

/// Journal text that says "nothing" (`""`, `None`, `No atmosphere`, `No volcanism`) -> `None`.
fn get_present(v: &Value, key: &str) -> Option<String> {
    get_text(v, key).filter(|s| {
        let l = s.to_ascii_lowercase();
        l != "none" && !l.starts_with("no ")
    })
}

/// Cheap check on a raw journal line before it is parsed: `false` when it can't produce anything
/// for the database. Only `Scan` lines are filtered (most of them are stars, belt clusters and
/// planets nobody lands on); positive matches only, so an unexpected format just costs a parse.
pub fn journal_line_useful(event: &str, line: &str) -> bool {
    if event != "Scan" {
        return true;
    }
    if line.contains("\"StarType\"") || !line.contains("\"PlanetClass\"") {
        return false;
    }
    let notable = ["\"Earthlike body\"", "\"Water world\"", "\"Ammonia world\""]
        .iter()
        .any(|c| line.contains(c));
    notable || !(line.contains("\"Landable\":false") && line.contains("\"TerraformState\":\"\""))
}

/// `gameversion` of a Legacy-galaxy (Horizons 3.x) client.
fn is_legacy_galaxy(game_version: &str) -> bool {
    let v = game_version.trim();
    v.starts_with("CAPI-Legacy")
        || v.split('.')
            .next()
            .and_then(|major| major.parse::<u32>().ok())
            .is_some_and(|major| major < 4)
}

/// A planet or moon `Scan` that can matter: landable, Earth-like / water / ammonia world or
/// terraformable. Stars, belt clusters, rings and other planets give `None`.
fn body_scan(v: &Value, ts: i64, source: Source) -> Option<BodyUpdate> {
    let scan = BodyScan {
        planet_class: get_text(v, "PlanetClass")?,
        atmosphere: get_present(v, "Atmosphere"),
        atmosphere_type: get_present(v, "AtmosphereType"),
        gravity_g: get_f64(v, "SurfaceGravity").map(|g| g / STANDARD_GRAVITY),
        temperature_k: get_f64(v, "SurfaceTemperature"),
        pressure_atm: get_f64(v, "SurfacePressure").map(|p| p / STANDARD_ATMOSPHERE),
        volcanism: get_present(v, "Volcanism"),
        terraform_state: get_text(v, "TerraformState").filter(|t| is_terraformable(Some(t))),
        distance_ls: get_f64(v, "DistanceFromArrivalLS").filter(|d| *d >= 0.0),
        landable: get_bool(v, "Landable").unwrap_or(false),
    };
    if !scan.landable && !scan.is_notable() {
        return None;
    }
    Some(BodyUpdate {
        system_id64: get_id(v, "SystemAddress").filter(|a| *a > 1)?,
        body_id: get_body_id(v, "BodyID")?,
        name: get_text(v, "BodyName"),
        scan: Some(scan),
        was_discovered: get_bool(v, "WasDiscovered"),
        was_mapped: get_bool(v, "WasMapped"),
        was_footfalled: get_bool(v, "WasFootfalled"),
        signals: None,
        timestamp: ts,
        source,
        checked_online: None,
    })
}

/// `$SAA_SignalType_Biological;` -> `Biological`.
fn signal_type(raw: &str) -> &str {
    let t = raw.trim().trim_start_matches('$').trim_end_matches(';');
    t.strip_prefix("SAA_SignalType_").unwrap_or(t)
}

/// Biological / geological signal counts of a planet (`FSSBodySignals`, `SAASignalsFound`), with the
/// genera a DSS scan revealed (`SAASignalsFound.Genuses`).
fn body_signals(v: &Value, ts: i64, source: Source) -> Option<BodyUpdate> {
    let list = v.get("Signals")?.as_array()?;
    let (mut bio, mut geo) = (0u32, 0u32);
    for s in list {
        let count = get_i64(s, "Count").unwrap_or(0).clamp(0, 1000) as u32;
        let t = signal_type(get_str(s, "Type").unwrap_or_default());
        if t.eq_ignore_ascii_case("Biological") {
            bio += count;
        } else if t.eq_ignore_ascii_case("Geological") {
            geo += count;
        }
    }
    let mut genera: Vec<String> = Vec::new();
    for g in v.get("Genuses").and_then(Value::as_array).into_iter().flatten() {
        if let Some(s) = get_text(g, "Genus")
            && !genera.contains(&s)
        {
            genera.push(s);
        }
    }
    Some(BodyUpdate {
        system_id64: get_id(v, "SystemAddress").filter(|a| *a > 1)?,
        body_id: get_body_id(v, "BodyID")?,
        name: get_text(v, "BodyName"),
        signals: Some(BodySignals { bio, geo, genera }),
        timestamp: ts,
        source,
        ..BodyUpdate::default()
    })
}

/// Somebody was on foot on the body: it has been footfalled.
fn on_foot(
    v: &Value,
    ts: i64,
    source: Source,
    body_id: Option<u32>,
    name: Option<String>,
) -> Vec<Ingest> {
    let (Some(system_id64), Some(body_id)) =
        (get_id(v, "SystemAddress").filter(|a| *a > 1), body_id)
    else {
        return Vec::new();
    };
    vec![Ingest::Body(BodyUpdate {
        system_id64,
        body_id,
        name,
        was_footfalled: Some(true),
        timestamp: ts,
        source,
        ..BodyUpdate::default()
    })]
}

/// A position-only system update when the event carries `StarSystem` + `SystemAddress` + `StarPos`
/// (EDDN senders add `StarPos` to `Docked` / `SAASignalsFound`; our own journal doesn't have it).
fn position_only(v: &Value, ts: i64, id: Option<u64>, name: Option<&str>) -> Option<Ingest> {
    let pos = get_pos(v, "StarPos")?;
    Some(Ingest::System(SystemUpdate {
        id64: id?,
        name: name?.to_string(),
        pos: Some(pos),
        timestamp: ts,
        details: None,
    }))
}

fn system_arrival(v: &Value, ts: i64) -> Option<SystemUpdate> {
    let id64 = get_id(v, "SystemAddress").filter(|a| *a > 1)?;
    let name = get_text(v, "StarSystem")?;
    let (controlling_faction, controlling_faction_state) = match v.get("SystemFaction") {
        Some(Value::String(s)) if !s.trim().is_empty() => (Some(s.trim().to_string()), None),
        Some(f @ Value::Object(_)) => (
            get_text(f, "Name"),
            get_str(f, "FactionState").and_then(names::state),
        ),
        _ => (None, None),
    };
    let factions: Option<Vec<FactionUpdate>> = v
        .get("Factions")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(faction).collect());
    let conflicts = factions.as_deref().and_then(|f| conflicts(v, f));
    let details = SystemDetails {
        population: get_i64(v, "Population")
            .filter(|p| *p >= 0)
            .map(|p| p as u64),
        allegiance: get_str(v, "SystemAllegiance").and_then(names::allegiance),
        government: get_str(v, "SystemGovernment").and_then(names::government),
        economy: get_str(v, "SystemEconomy").and_then(names::economy),
        second_economy: get_str(v, "SystemSecondEconomy").and_then(names::economy),
        security: get_str(v, "SystemSecurity").and_then(names::security),
        controlling_faction,
        controlling_faction_state,
        controlling_power: get_text(v, "ControllingPower"),
        powerplay_state: get_str(v, "PowerplayState").and_then(names::state),
        powers: get_strings(v, "Powers").unwrap_or_default(),
        factions,
        conflicts,
    };
    Some(SystemUpdate {
        id64,
        name,
        pos: get_pos(v, "StarPos"),
        timestamp: ts,
        details: Some(details),
    })
}

fn states(f: &Value, key: &str) -> Vec<String> {
    f.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| get_str(s, "State").and_then(names::state))
                .collect()
        })
        .unwrap_or_default()
}

/// The system's conflicts from a report that lists its factions. The game writes `Conflicts` only when there are
/// some, so no list means none, unless a faction has a war or election among its active or pending states: then the
/// sender dropped the list and they're unknown (`None`, the faction states stand in). A faction's main state alone
/// doesn't count: it can still say War while the war is only a recovering state.
fn conflicts(v: &Value, factions: &[FactionUpdate]) -> Option<Vec<ConflictUpdate>> {
    let list: Vec<ConflictUpdate> = v
        .get("Conflicts")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(conflict).collect())
        .unwrap_or_default();
    let in_conflict = |f: &FactionUpdate| f.active_states.iter().chain(&f.pending_states).any(|s| ConflictKind::parse(s).is_some());
    if list.is_empty() && factions.iter().any(in_conflict) {
        return None;
    }
    Some(list)
}

fn conflict(c: &Value) -> Option<ConflictUpdate> {
    let side = |key: &str| -> Option<ConflictSideUpdate> {
        let f = c.get(key)?;
        Some(ConflictSideUpdate {
            faction: get_text(f, "Name")?,
            stake: get_text(f, "Stake").map(|s| names::clean_station_name(&s)).filter(|s| !s.is_empty()),
            won_days: get_i64(f, "WonDays").unwrap_or(0).clamp(0, 99) as u32,
        })
    };
    Some(ConflictUpdate {
        kind: ConflictKind::parse(get_str(c, "WarType")?)?,
        status: ConflictStatus::parse(get_str(c, "Status").unwrap_or("")),
        sides: [side("Faction1")?, side("Faction2")?],
    })
}

fn faction(f: &Value) -> Option<FactionUpdate> {
    Some(FactionUpdate {
        name: get_text(f, "Name")?,
        influence: get_f64(f, "Influence"),
        state: get_str(f, "FactionState").and_then(names::state),
        government: get_str(f, "Government").and_then(names::government),
        allegiance: get_str(f, "Allegiance").and_then(names::allegiance),
        happiness: get_str(f, "Happiness").and_then(names::happiness),
        active_states: states(f, "ActiveStates"),
        pending_states: states(f, "PendingStates"),
        recovering_states: states(f, "RecoveringStates"),
    })
}

/// Station fields shared by `Docked`, `Location`/`CarrierJump` (docked) and `ApproachSettlement`.
fn station_common(
    v: &Value,
    ts: i64,
    market_id: u64,
    name: String,
    system_id64: Option<u64>,
    system_name: Option<String>,
) -> StationUpdate {
    let economies = v
        .get("StationEconomies")
        .and_then(Value::as_array)
        .map(|a| {
            let mut e: Vec<(String, f64)> = a
                .iter()
                .filter_map(|x| {
                    Some((
                        names::economy(get_str(x, "Name")?)?,
                        get_f64(x, "Proportion").unwrap_or(0.0),
                    ))
                })
                .collect();
            sort_economies(&mut e);
            e
        });
    let primary_economy = get_str(v, "StationEconomy")
        .and_then(names::economy)
        .or_else(|| {
            economies
                .as_ref()
                .and_then(|e| e.first().map(|(n, _)| n.clone()))
        });
    let (faction, faction_state) = match v.get("StationFaction") {
        Some(Value::String(s)) if !s.trim().is_empty() => (Some(s.trim().to_string()), None),
        Some(f @ Value::Object(_)) => (
            get_text(f, "Name"),
            get_str(f, "FactionState").and_then(names::state),
        ),
        _ => (None, None),
    };
    StationUpdate {
        market_id,
        name,
        system_id64,
        system_name,
        station_type: None,
        dist_ls: None,
        landing_pads: None,
        services: get_strings(v, "StationServices").map(Services::from_names),
        primary_economy,
        economies,
        faction,
        faction_state,
        government: get_str(v, "StationGovernment").and_then(names::government),
        allegiance: get_str(v, "StationAllegiance").and_then(names::allegiance),
        carrier_docking_access: None,
        planetary: None,
        timestamp: ts,
    }
}

fn station_from_event(
    v: &Value,
    ts: i64,
    sys_id: Option<u64>,
    sys_name: Option<String>,
) -> Option<StationUpdate> {
    let market_id = get_id(v, "MarketID")?;
    let name = get_text(v, "StationName")?;
    let mut st = station_common(v, ts, market_id, name, sys_id, sys_name);
    st.station_type = get_text(v, "StationType");
    st.dist_ls = get_f64(v, "DistFromStarLS").filter(|d| *d >= 0.0);
    st.landing_pads = v
        .get("LandingPads")
        .filter(|p| p.is_object())
        .map(|p| LandingPads {
            small: get_i64(p, "Small").unwrap_or(0).clamp(0, 1000) as u32,
            medium: get_i64(p, "Medium").unwrap_or(0).clamp(0, 1000) as u32,
            large: get_i64(p, "Large").unwrap_or(0).clamp(0, 1000) as u32,
        });
    Some(st)
}

fn is_ring_name(body: &str) -> bool {
    let l = body.trim_end().to_ascii_lowercase();
    l.ends_with(" ring") || l.ends_with(" belt")
}

fn ring_signals(
    v: &Value,
    ts: i64,
    sys_id: Option<u64>,
    sys_name: Option<String>,
) -> Option<RingSignalsUpdate> {
    let body_name = get_text(v, "BodyName")?;
    let list = v.get("Signals")?.as_array()?;
    let mut signals = Vec::with_capacity(list.len());
    let mut any_commodity = false;
    for s in list {
        let Some(t) = get_str(s, "Type") else {
            continue;
        };
        if t.starts_with('$') {
            continue; // biological / geological / human ... signals of planets
        }
        any_commodity = true;
        let sym = normalize_symbol(t);
        if !sym.is_empty() {
            signals.push((sym, get_i64(s, "Count").unwrap_or(0).max(0)));
        }
    }
    if !is_ring_name(&body_name) && !any_commodity {
        return None;
    }
    Some(RingSignalsUpdate {
        system_id64: sys_id?,
        system_name: sys_name,
        body_name,
        ring_type: None,
        signals,
        timestamp: ts,
    })
}

/// Market.json (or a `Market` event that carries `Items`).
fn market_json(v: &Value, ts: i64, source: Source) -> Option<MarketSnapshot> {
    let list = v.get("Items")?.as_array()?;
    let market_id = get_id(v, "MarketID")?;
    let station_name = get_text(v, "StationName")?;
    let system_name = get_text(v, "StarSystem")?;
    let mut items = Vec::with_capacity(list.len());
    for it in list {
        if get_str(it, "Category").is_some_and(|c| c.to_ascii_lowercase().contains("nonmarketable"))
        {
            continue; // limpets; EDDN drops them too
        }
        let Some(name) = get_str(it, "Name") else {
            continue;
        };
        let symbol = normalize_symbol(name);
        if symbol.is_empty() {
            continue;
        }
        items.push(MarketItem {
            symbol,
            buy: price(it, "BuyPrice"),
            sell: price(it, "SellPrice"),
            mean: price(it, "MeanPrice"),
            stock: price(it, "Stock"),
            stock_bracket: bracket(it.get("StockBracket")),
            demand: price(it, "Demand"),
            demand_bracket: bracket(it.get("DemandBracket")),
        });
    }
    Some(MarketSnapshot {
        market_id,
        system_name,
        station_name,
        station_type: get_text(v, "StationType"),
        carrier_docking_access: get_text(v, "CarrierDockingAccess"),
        timestamp: ts,
        items,
        prohibited: None,
        economies: None,
        source,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn eddn_commodity() -> Value {
        json!({
            "$schemaRef": "https://eddn.edcd.io/schemas/commodity/3",
            "header": {"uploaderID": "abc", "softwareName": "E:D Market Connector [Windows]", "softwareVersion": "6.1.2",
                       "gatewayTimestamp": "2026-10-12T09:23:12.361367Z"},
            "message": {
                "systemName": "Sol", "stationName": "Daedalus", "marketId": 128016384, "stationType": "Orbis",
                "timestamp": "2026-10-12T09:23:10Z",
                "commodities": [
                    {"name": "gold", "meanPrice": 47000, "buyPrice": 45000, "stock": 1200, "stockBracket": 2,
                     "sellPrice": 44000, "demand": 0, "demandBracket": ""},
                    {"name": "Tritium", "meanPrice": 50000, "buyPrice": 0, "stock": 0, "stockBracket": 0,
                     "sellPrice": 52000, "demand": 9000, "demandBracket": 3, "statusFlags": ["Consumer"]},
                    {"name": "", "buyPrice": 1}
                ],
                "economies": [{"name": "Refinery", "proportion": 1.0}],
                "prohibited": ["Narcotics", "BattleWeapons"]
            }
        })
    }

    pub(crate) fn eddn_fsdjump() -> Value {
        json!({
            "$schemaRef": "https://eddn.edcd.io/schemas/journal/1",
            "header": {"uploaderID": "x", "softwareName": "EDDI", "softwareVersion": "5.0.6", "gatewayTimestamp": "2026-10-12T09:00:01Z"},
            "message": {
                "timestamp": "2026-10-12T09:00:00Z", "event": "FSDJump", "StarSystem": "Sol", "SystemAddress": 10477373803u64,
                "StarPos": [0.0, 0.0, 0.0], "SystemAllegiance": "Federation", "SystemEconomy": "$economy_Refinery;",
                "SystemSecondEconomy": "$economy_Service;", "SystemGovernment": "$government_Democracy;",
                "SystemSecurity": "$SYSTEM_SECURITY_high;", "Population": 18320926115u64,
                "ControllingPower": "Jerome Archer", "Powers": ["Jerome Archer", "Edmund Mahon"], "PowerplayState": "Stronghold",
                "SystemFaction": {"Name": "Mother Gaia", "FactionState": "None"},
                "Factions": [
                    {"Name": "Mother Gaia", "FactionState": "None", "Government": "Democracy", "Influence": 0.315,
                     "Allegiance": "Federation", "Happiness": "$Faction_HappinessBand2;",
                     "PendingStates": [{"State": "Expansion", "Trend": 0}], "RecoveringStates": [{"State": "PublicHoliday", "Trend": 0}]},
                    {"Name": "Federal Congress", "FactionState": "CivilLiberty", "Government": "Democracy", "Influence": 0.168,
                     "Allegiance": "Federation", "ActiveStates": [{"State": "CivilLiberty"}]}
                ],
                "horizons": true, "odyssey": true
            }
        })
    }

    #[test]
    fn schema_names() {
        assert_eq!(
            eddn_schema_name("https://eddn.edcd.io/schemas/commodity/3"),
            Some("commodity/3")
        );
        assert_eq!(
            eddn_schema_name("https://eddn.edcd.io/schemas/commodity/3/test"),
            None
        );
        assert_eq!(eddn_schema_name("garbage"), None);
    }

    #[test]
    fn eddn_commodity_message() {
        let out = Ingest::from_eddn(&eddn_commodity());
        assert_eq!(out.len(), 1);
        let Ingest::Market(m) = &out[0] else {
            panic!("not a market: {out:?}")
        };
        assert_eq!(m.market_id, 128016384);
        assert_eq!(m.system_name, "Sol");
        assert_eq!(m.station_type.as_deref(), Some("Orbis"));
        assert_eq!(m.timestamp, 1_791_796_990);
        assert_eq!(m.items.len(), 2);
        assert_eq!(m.items[0].symbol, "gold");
        assert_eq!(m.items[0].demand_bracket, Some(-1));
        assert_eq!(m.items[1].symbol, "tritium");
        assert_eq!(m.items[1].sell, 52000);
        assert_eq!(
            m.prohibited.as_deref(),
            Some(&["narcotics".to_string(), "battleweapons".to_string()][..])
        );
        assert_eq!(
            m.economies.as_ref().map(|e| e[0].0.as_str()),
            Some("Refinery")
        );
        assert_eq!(m.source, Source::Eddn);
    }

    #[test]
    fn test_schemas_and_garbage_are_ignored() {
        let mut v = eddn_commodity();
        v["$schemaRef"] = json!("https://eddn.edcd.io/schemas/commodity/3/test");
        assert!(Ingest::from_eddn(&v).is_empty());
        assert!(Ingest::from_eddn(&json!({"foo": 1})).is_empty());
        assert!(Ingest::from_eddn(&json!([1, 2, 3])).is_empty());
        let mut v = eddn_commodity();
        v["message"]["marketId"] = json!("not a number");
        assert!(Ingest::from_eddn(&v).is_empty());
        assert!(Ingest::from_journal_event("FSDJump", &json!("string")).is_empty());
    }

    #[test]
    fn eddn_fsdjump_message() {
        let out = Ingest::from_eddn(&eddn_fsdjump());
        assert_eq!(out.len(), 1);
        let Ingest::System(s) = &out[0] else { panic!() };
        assert_eq!(s.id64, 10477373803);
        assert_eq!(s.pos, Some([0.0, 0.0, 0.0]));
        let d = s.details.as_ref().expect("details");
        assert_eq!(d.economy.as_deref(), Some("Refinery"));
        assert_eq!(d.second_economy.as_deref(), Some("Service"));
        assert_eq!(d.government.as_deref(), Some("Democracy"));
        assert_eq!(d.security.as_deref(), Some("High"));
        assert_eq!(d.controlling_faction.as_deref(), Some("Mother Gaia"));
        assert_eq!(d.powerplay_state.as_deref(), Some("Stronghold"));
        assert_eq!(d.powers.len(), 2);
        let f = d.factions.as_ref().expect("factions");
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].happiness.as_deref(), Some("Happy"));
        assert_eq!(f[0].recovering_states, vec!["Public Holiday".to_string()]);
        assert_eq!(f[1].state.as_deref(), Some("Civil Liberty"));
        // No conflict list and nobody at war: none.
        assert_eq!(d.conflicts, Some(Vec::new()));
    }

    #[test]
    fn conflicts_come_with_the_factions() {
        // As the game writes it (a real civil war, Leesti, 2026-11-04).
        let mut v = eddn_fsdjump();
        v["message"]["Factions"][1]["ActiveStates"] = json!([{"State": "CivilWar"}]);
        v["message"]["Factions"][0]["PendingStates"] = json!([{"State": "War", "Trend": 0}]);
        v["message"]["Conflicts"] = json!([
            {"WarType": "civilwar", "Status": "active",
             "Faction1": {"Name": "Leesti United Steelworks", "Stake": "Villeneuve Manufacturing Enterprise", "WonDays": 0},
             "Faction2": {"Name": "Leesti Alliance Union", "Stake": "Pandey's Point", "WonDays": 2}},
            {"WarType": "war", "Status": "pending",
             "Faction1": {"Name": "Mother Gaia", "Stake": "", "WonDays": 0},
             "Faction2": {"Name": "Federal Congress", "Stake": "Abraham Lincoln", "WonDays": 0}},
            {"WarType": "duel", "Status": "active", "Faction1": {"Name": "A"}, "Faction2": {"Name": "B"}},
            {"WarType": "war", "Status": "active", "Faction1": {"Name": "Only one side"}}
        ]);
        let out = Ingest::from_eddn(&v);
        let Ingest::System(s) = &out[0] else { panic!() };
        let c = s.details.as_ref().and_then(|d| d.conflicts.clone()).expect("conflicts");
        assert_eq!(c.len(), 2, "an unknown kind and a one-sided entry are dropped: {c:?}");
        assert_eq!((c[0].kind, c[0].status), (ConflictKind::CivilWar, ConflictStatus::Active));
        assert_eq!(c[0].sides[1].faction, "Leesti Alliance Union");
        assert_eq!(c[0].sides[1].stake.as_deref(), Some("Pandey's Point"));
        assert_eq!((c[0].sides[0].won_days, c[0].sides[1].won_days), (0, 2));
        assert_eq!((c[1].kind, c[1].status), (ConflictKind::War, ConflictStatus::Pending));
        assert_eq!(c[1].sides[0].stake, None, "an empty stake is nothing at stake");

        // Factions at war but no list: a sender dropped it, so the conflicts are unknown rather than none.
        v["message"].as_object_mut().unwrap().remove("Conflicts");
        let out = Ingest::from_journal_event("Location", &v["message"]);
        let Ingest::System(s) = &out[0] else { panic!() };
        assert_eq!(s.details.as_ref().map(|d| d.conflicts.is_none()), Some(true));

        // A Legacy-galaxy client (game 3.x) reports another galaxy's wars: only the position is kept.
        let mut legacy = v.clone();
        legacy["header"]["gameversion"] = json!("3.8.0.407");
        legacy["message"]["Conflicts"] = json!([{"WarType": "war", "Status": "active", "Faction1": {"Name": "A"}, "Faction2": {"Name": "B"}}]);
        let out = Ingest::from_eddn(&legacy);
        let Ingest::System(s) = &out[0] else { panic!("{out:?}") };
        assert!(s.details.is_none() && s.pos.is_some(), "{s:?}");

        // No factions at all (an unpopulated system): nothing to say about conflicts either.
        v["message"].as_object_mut().unwrap().remove("Factions");
        let out = Ingest::from_journal_event("FSDJump", &v["message"]);
        let Ingest::System(s) = &out[0] else { panic!() };
        assert_eq!(s.details.as_ref().map(|d| d.conflicts.is_none()), Some(true));
        assert_eq!(ConflictStatus::parse(""), ConflictStatus::Ended);
        assert_eq!(ConflictKind::parse("Civil War"), Some(ConflictKind::CivilWar));
        assert_eq!(ConflictKind::parse("Expansion"), None);
    }

    #[test]
    fn eddn_jumps_into_empty_systems_are_skipped() {
        let mut v = eddn_fsdjump();
        v["message"]["Population"] = json!(0);
        assert!(Ingest::from_eddn(&v).is_empty(), "unpopulated EDDN jump");
        v["message"].as_object_mut().unwrap().remove("Population");
        assert!(Ingest::from_eddn(&v).is_empty(), "no population at all");
        // ...but our own journal keeps every system we visit.
        assert_eq!(Ingest::from_journal_event("FSDJump", &v["message"]).len(), 1);
        // A carrier jump from EDDN is kept (carrier positions matter).
        v["message"]["event"] = json!("CarrierJump");
        assert_eq!(Ingest::from_eddn(&v).len(), 1);
    }

    #[test]
    fn eddn_docked_with_starpos() {
        let v = json!({
            "$schemaRef": "https://eddn.edcd.io/schemas/journal/1",
            "header": {"uploaderID": "x", "softwareName": "EDMC", "softwareVersion": "6"},
            "message": {"timestamp": "2026-10-12T09:00:00Z", "event": "Docked", "StationName": "Ehrlich City",
                "StationType": "CraterPort", "StarSystem": "Sol", "SystemAddress": 10477373803u64, "MarketID": 3534390016u64,
                "StationFaction": {"Name": "Sol Workers' Party"}, "StationGovernment": "$government_Democracy;",
                "StationServices": ["dock", "autodock", "commodities", "materialtrader", "facilitator", "refuel"],
                "StationEconomy": "$economy_Industrial;",
                "StationEconomies": [{"Name": "$economy_Industrial;", "Proportion": 0.8}, {"Name": "$economy_HighTech;", "Proportion": 0.2}],
                "DistFromStarLS": 230.8, "LandingPads": {"Small": 8, "Medium": 8, "Large": 8}, "StarPos": [0.0, 0.0, 0.0]}
        });
        let out = Ingest::from_eddn(&v);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(matches!(&out[0], Ingest::System(s) if s.details.is_none() && s.pos.is_some()));
        let Ingest::Station(st) = &out[1] else {
            panic!()
        };
        assert_eq!(st.market_id, 3534390016);
        assert_eq!(st.system_id64, Some(10477373803));
        assert_eq!(st.primary_economy.as_deref(), Some("Industrial"));
        assert_eq!(
            st.economies.as_ref().map(|e| e[1].0.as_str()),
            Some("High Tech")
        );
        assert_eq!(
            st.landing_pads,
            Some(LandingPads {
                small: 8,
                medium: 8,
                large: 8
            })
        );
        let svc = st.services.expect("services");
        assert!(svc.contains(
            Services::MATERIAL_TRADER | Services::INTERSTELLAR_FACTORS | Services::MARKET
        ));
        assert_eq!(st.faction.as_deref(), Some("Sol Workers' Party"));
    }

    #[test]
    fn ring_signals_from_saa() {
        let v = json!({"timestamp": "2026-10-12T09:00:00Z", "event": "SAASignalsFound", "BodyName": "HIP 21991 A 1 A Ring",
            "SystemAddress": 3107509474002u64, "BodyID": 7,
            "Signals": [{"Type": "Painite", "Count": 2}, {"Type": "LowTemperatureDiamond", "Type_Localised": "Low Temperature Diamonds", "Count": 1}]});
        let out = Ingest::from_journal_event("SAASignalsFound", &v);
        assert_eq!(out.len(), 1);
        let Ingest::RingSignals(r) = &out[0] else {
            panic!()
        };
        assert_eq!(
            r.signals,
            vec![
                ("painite".to_string(), 2),
                ("lowtemperaturediamond".to_string(), 1)
            ]
        );
        // planet signals are not ring hotspots
        let planet = json!({"timestamp": "2026-10-12T09:00:00Z", "event": "SAASignalsFound", "BodyName": "Sol 3",
            "SystemAddress": 10477373803u64, "Signals": [{"Type": "$SAA_SignalType_Biological;", "Count": 3}]});
        assert!(Ingest::from_journal_event("SAASignalsFound", &planet).is_empty());
    }

    #[test]
    fn own_market_json() {
        let v = json!({"timestamp": "2026-10-12T09:00:00Z", "event": "Market", "MarketID": 128016384u64, "StationName": "Daedalus",
        "StationType": "Orbis", "StarSystem": "Sol", "Items": [
            {"id": 128049154, "Name": "$gold_name;", "Name_Localised": "Gold", "Category": "$MARKET_category_metals;",
             "BuyPrice": 45000, "SellPrice": 44000, "MeanPrice": 47000, "StockBracket": 2, "DemandBracket": 0,
             "Stock": 1200, "Demand": 1, "Consumer": false, "Producer": true, "Rare": false},
            {"id": 128066403, "Name": "$drones_name;", "Category": "$MARKET_category_nonmarketable;", "BuyPrice": 101, "SellPrice": 101}
        ]});
        let out = Ingest::from_journal_event("Market", &v);
        let [Ingest::Market(m)] = &out[..] else {
            panic!("{out:?}")
        };
        assert_eq!(m.items.len(), 1);
        assert_eq!(m.items[0].symbol, "gold");
        assert_eq!(m.source, Source::Journal);
        // the bare journal event (no Items) carries no market data
        let bare = json!({"timestamp": "2026-10-12T09:00:00Z", "event": "Market", "MarketID": 1, "StationName": "x", "StarSystem": "y"});
        assert!(Ingest::from_journal_event("Market", &bare).is_empty());
    }

    #[test]
    fn location_docked_gives_system_and_station() {
        let v = json!({"timestamp": "2026-10-12T09:00:00Z", "event": "CarrierJump", "Docked": true, "StationName": "T2X-02X",
            "StationType": "FleetCarrier", "MarketID": 3708716800u64, "StarSystem": "Phua Fraae AA-A h0", "SystemAddress": 819519,
            "StarPos": [-34608.78125, 513.34375, 26058.59375], "Population": 0, "SystemAllegiance": "",
            "StationServices": ["dock", "commodities"]});
        let out = Ingest::from_journal_event("CarrierJump", &v);
        assert_eq!(out.len(), 2);
        let Ingest::Station(st) = &out[1] else {
            panic!()
        };
        assert_eq!(st.system_id64, Some(819519));
        assert_eq!(st.station_type.as_deref(), Some("FleetCarrier"));
        let Ingest::System(s) = &out[0] else { panic!() };
        assert_eq!(s.details.as_ref().and_then(|d| d.allegiance.clone()), None);
        assert!(s.details.as_ref().is_some_and(|d| d.factions.is_none()));
    }

    #[test]
    fn future_timestamps_are_clamped() {
        let mut v = eddn_commodity();
        v["message"]["timestamp"] = json!("2099-01-01T00:00:00Z");
        let out = Ingest::from_eddn(&v);
        let [Ingest::Market(m)] = &out[..] else {
            panic!()
        };
        assert_eq!(
            m.timestamp, 1_791_796_992,
            "clamped to the gateway timestamp"
        );
    }

    #[test]
    fn raw_expands() {
        let raw = Ingest::Raw(RawIngest::Eddn(eddn_commodity()));
        assert_eq!(raw.expand().len(), 1);
    }

    fn eddn(schema: &str, gameversion: &str, message: Value) -> Value {
        json!({"$schemaRef": format!("https://eddn.edcd.io/schemas/{schema}"),
               "header": {"uploaderID": "x", "softwareName": "E:D Market Connector [Windows]", "softwareVersion": "6.1.2",
                          "gameversion": gameversion, "gamebuild": "r332841/r0 ", "gatewayTimestamp": "2026-10-12T09:23:12Z"},
               "message": message})
    }

    /// A landable planet as EDMC relays it (journal `Scan`, `_Localised` stripped, `StarPos` added).
    pub(crate) fn eddn_planet_scan() -> Value {
        eddn(
            "journal/1",
            "4.4.1.1",
            json!({"timestamp": "2026-10-12T09:23:11Z", "event": "Scan", "ScanType": "Detailed",
                "BodyName": "Swoilz MH-W c17-3 A 4", "BodyID": 6, "Parents": [{"Star": 1}, {"Null": 0}],
                "StarSystem": "Swoilz MH-W c17-3", "SystemAddress": 908552967066u64, "DistanceFromArrivalLS": 1209.42,
                "TidalLock": true, "TerraformState": "", "PlanetClass": "High metal content body",
                "Atmosphere": "thin carbon dioxide atmosphere", "AtmosphereType": "CarbonDioxide",
                "AtmosphereComposition": [{"Name": "CarbonDioxide", "Percent": 99.009911}, {"Name": "SulphurDioxide", "Percent": 0.990099}],
                "Volcanism": "", "MassEM": 0.02, "Radius": 1500000.0, "SurfaceGravity": 0.876, "SurfaceTemperature": 165.16,
                "SurfacePressure": 1107.5, "Landable": true, "Materials": [{"Name": "iron", "Percent": 23.5}],
                "WasDiscovered": true, "WasMapped": false, "WasFootfalled": false,
                "horizons": true, "odyssey": true, "StarPos": [-28.8, -112.4, -96.9]}),
        )
    }

    #[test]
    fn eddn_planet_scan_carries_the_footfall_flag() {
        let out = Ingest::from_eddn(&eddn_planet_scan());
        let [Ingest::Body(b)] = &out[..] else {
            panic!("{out:?}")
        };
        assert_eq!((b.system_id64, b.body_id), (908552967066, 6));
        assert_eq!(b.name.as_deref(), Some("Swoilz MH-W c17-3 A 4"));
        assert_eq!(b.source, Source::Eddn);
        assert_eq!(b.timestamp, 1_791_796_991);
        assert_eq!(
            (b.was_discovered, b.was_mapped, b.was_footfalled),
            (Some(true), Some(false), Some(false))
        );
        let s = b.scan.as_ref().expect("scan");
        assert_eq!(s.planet_class, "High metal content body");
        assert_eq!(s.atmosphere_type.as_deref(), Some("CarbonDioxide"));
        assert_eq!(s.volcanism, None);
        assert_eq!(s.terraform_state, None);
        assert!(s.landable);
        assert!((s.gravity_g.unwrap() - 0.0893).abs() < 1e-3, "{:?}", s.gravity_g);
        assert!((s.pressure_atm.unwrap() - 0.01093).abs() < 1e-4);
        assert_eq!(s.distance_ls, Some(1209.42));
        assert!(b.signals.is_none());
        // the same event from our own journal
        let own = Ingest::from_journal_event("Scan", &eddn_planet_scan()["message"]);
        assert!(matches!(&own[..], [Ingest::Body(b)] if b.source == Source::Journal && b.was_footfalled == Some(false)));
    }

    #[test]
    fn scans_that_cannot_matter_are_dropped() {
        // the belt cluster of eddn.md §3.1 (real EDMC message)
        let belt = eddn(
            "journal/1",
            "4.4.1.1",
            json!({"timestamp": "2026-10-12T09:23:11Z", "event": "Scan", "ScanType": "AutoScan",
                "BodyName": "Tucanae Sector JC-U b3-1 A A Belt Cluster 1", "BodyID": 6, "Parents": [{"Ring": 5}, {"Star": 1}, {"Null": 0}],
                "StarSystem": "Tucanae Sector JC-U b3-1", "SystemAddress": 2871319274977u64, "DistanceFromArrivalLS": 5.571921,
                "WasDiscovered": true, "WasMapped": false, "WasFootfalled": false, "horizons": true, "odyssey": true,
                "StarPos": [110.65625, -201.375, 152.84375]}),
        );
        assert!(Ingest::from_eddn(&belt).is_empty());
        let mut star = eddn_planet_scan();
        let m = star["message"].as_object_mut().unwrap();
        m.remove("PlanetClass");
        m.insert("StarType".into(), json!("K"));
        assert!(Ingest::from_eddn(&star).is_empty());
        let mut giant = eddn_planet_scan();
        giant["message"]["PlanetClass"] = json!("Sudarsky class I gas giant");
        giant["message"]["Landable"] = json!(false);
        assert!(Ingest::from_eddn(&giant).is_empty());
        // ...but a terraformable or an ammonia world is kept even though nobody lands there
        giant["message"]["TerraformState"] = json!("Terraformable");
        assert_eq!(Ingest::from_eddn(&giant).len(), 1);
        giant["message"]["TerraformState"] = json!("");
        giant["message"]["PlanetClass"] = json!("Ammonia world");
        assert_eq!(Ingest::from_eddn(&giant).len(), 1);
        // Legacy-galaxy (3.x) clients: their flags belong to another galaxy
        let mut legacy = eddn_planet_scan();
        legacy["header"]["gameversion"] = json!("3.8.0.407");
        assert!(Ingest::from_eddn(&legacy).is_empty());
    }

    #[test]
    fn body_signals_from_fss_and_dss() {
        let fss = eddn(
            "fssbodysignals/1",
            "4.4.1.1",
            json!({"timestamp": "2026-10-12T09:23:10Z", "event": "FSSBodySignals", "BodyName": "Swoilz MH-W c17-3 A 4",
                "BodyID": 6, "SystemAddress": 908552967066u64, "StarSystem": "Swoilz MH-W c17-3", "StarPos": [-28.8, -112.4, -96.9],
                "Signals": [{"Type": "$SAA_SignalType_Biological;", "Count": 4}, {"Type": "$SAA_SignalType_Geological;", "Count": 1},
                            {"Type": "$PlanetaryMiningLocation_Name;", "Count": 12}]}),
        );
        let out = Ingest::from_eddn(&fss);
        let [Ingest::Body(b)] = &out[..] else {
            panic!("{out:?}")
        };
        let s = b.signals.as_ref().expect("signals");
        assert_eq!((s.bio, s.geo), (4, 1));
        assert!(s.genera.is_empty());
        assert!(b.scan.is_none() && b.was_footfalled.is_none());
        // DSS: genera (journal symbols; EDDN has no `_Localised`), no ring hotspots
        let dss = json!({"timestamp": "2026-09-28T03:04:04Z", "event": "SAASignalsFound", "BodyName": "Swoilz MH-W c17-3 A 4",
            "SystemAddress": 908552967066u64, "BodyID": 6,
            "Signals": [{"Type": "$SAA_SignalType_Biological;", "Type_Localised": "Biological", "Count": 2},
                        {"Type": "$PlanetaryMiningLocation_Name;", "Count": 12}],
            "Genuses": [{"Genus": "$Codex_Ent_Bacterial_Genus_Name;", "Genus_Localised": "Bacterium"},
                        {"Genus": "$Codex_Ent_Stratum_Genus_Name;"}, {"Genus": "$Codex_Ent_Stratum_Genus_Name;"}]});
        let out = Ingest::from_journal_event("SAASignalsFound", &dss);
        let [Ingest::Body(b)] = &out[..] else {
            panic!("{out:?}")
        };
        let s = b.signals.as_ref().expect("signals");
        assert_eq!(s.bio, 2);
        assert_eq!(
            s.genera,
            vec!["$Codex_Ent_Bacterial_Genus_Name;".to_string(), "$Codex_Ent_Stratum_Genus_Name;".to_string()]
        );
        // a ring's DSS stays a ring hotspot
        let ring = json!({"timestamp": "2026-10-12T09:00:00Z", "event": "SAASignalsFound", "BodyName": "HIP 21991 A 1 A Ring",
            "SystemAddress": 3107509474002u64, "BodyID": 7, "Signals": [{"Type": "Painite", "Count": 2}]});
        assert!(matches!(&Ingest::from_journal_event("SAASignalsFound", &ring)[..], [Ingest::RingSignals(_)]));
    }

    #[test]
    fn being_on_foot_means_footfalled() {
        let organic = |scan_type: &str| {
            eddn(
                "scanorganic/1",
                "4.4.1.1",
                json!({"timestamp": "2026-10-12T09:23:10Z", "event": "ScanOrganic", "ScanType": scan_type,
                    "Genus": "$Codex_Ent_Stratum_Genus_Name;", "Species": "$Codex_Ent_Stratum_07_Name;",
                    "Variant": "$Codex_Ent_Stratum_07_K_Name;", "SystemAddress": 908552967066u64, "BodyID": 6,
                    "StarSystem": "Swoilz MH-W c17-3", "StarPos": [-28.8, -112.4, -96.9]}),
            )
        };
        let out = Ingest::from_eddn(&organic("Log"));
        assert!(
            matches!(&out[..], [Ingest::Body(b)] if b.was_footfalled == Some(true) && b.body_id == 6 && b.scan.is_none()),
            "{out:?}"
        );
        assert!(Ingest::from_eddn(&organic("Analyse")).is_empty(), "may fire in another system");
        // our own journal: ScanOrganic uses "Body" for the id
        let own = json!({"timestamp": "2026-09-28T03:13:55Z", "event": "ScanOrganic", "ScanType": "Log",
            "Genus": "$Codex_Ent_Stratum_Genus_Name;", "Species": "$Codex_Ent_Stratum_07_Name;", "WasLogged": false,
            "SystemAddress": 908552967066u64, "Body": 6});
        assert!(matches!(&Ingest::from_journal_event("ScanOrganic", &own)[..], [Ingest::Body(b)] if b.body_id == 6));
        let disembark = json!({"timestamp": "2026-09-28T01:11:57Z", "event": "Disembark", "SRV": false, "Taxi": false,
            "Multicrew": false, "ID": 19, "StarSystem": "Brestla", "SystemAddress": 673101653409u64, "Body": "Brestla A 1",
            "BodyID": 8, "OnStation": false, "OnPlanet": true});
        let out = Ingest::from_journal_event("Disembark", &disembark);
        let [Ingest::Body(b)] = &out[..] else {
            panic!("{out:?}")
        };
        assert_eq!(b.name.as_deref(), Some("Brestla A 1"));
        assert_eq!((b.was_footfalled, b.source), (Some(true), Source::Journal));
        let mut at_station = disembark.clone();
        at_station["OnStation"] = json!(true);
        assert!(Ingest::from_journal_event("Disembark", &at_station).is_empty());
    }

    #[test]
    fn cheap_scan_line_filter() {
        let line = |s: &str| s.to_string();
        let planet = line(
            r#"{ "timestamp":"2026-09-05T01:06:08Z", "event":"Scan", "BodyName":"X 2", "BodyID":2, "SystemAddress":5, "TerraformState":"", "PlanetClass":"High metal content body", "Landable":true }"#,
        );
        assert_eq!(Ingest::from_journal_event("Scan", &serde_json::from_str(&planet).expect("json")).len(), 1);
        let rock = planet.replace("\"Landable\":true", "\"Landable\":false");
        let tf = rock.replace("\"TerraformState\":\"\"", "\"TerraformState\":\"Terraformable\"");
        let elw = rock.replace("High metal content body", "Earthlike body");
        let star = line(r#"{ "event":"Scan", "BodyName":"X", "BodyID":0, "SystemAddress":5, "StarType":"K", "Subclass":3 }"#);
        let belt = line(r#"{ "event":"Scan", "BodyName":"X A Belt Cluster 1", "BodyID":3, "SystemAddress":5, "WasDiscovered":true }"#);
        assert!(journal_line_useful("Scan", &planet));
        assert!(!journal_line_useful("Scan", &rock));
        assert!(journal_line_useful("Scan", &tf));
        assert!(journal_line_useful("Scan", &elw));
        assert!(!journal_line_useful("Scan", &star));
        assert!(!journal_line_useful("Scan", &belt));
        assert!(journal_line_useful("FSDJump", &star), "only Scan lines are filtered");
        // every line the filter drops converts to nothing anyway
        for l in [&rock, &star, &belt] {
            let v: Value = serde_json::from_str(l).expect("json");
            assert!(Ingest::from_journal_event("Scan", &v).is_empty(), "{l}");
        }
    }
}
