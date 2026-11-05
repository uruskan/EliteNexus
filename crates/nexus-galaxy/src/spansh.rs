//! Streaming importer for Spansh dumps (`https://downloads.spansh.co.uk/`).
//!
//! Accepts gzip or plain JSON; one record per line, optionally wrapped in `[` / `]` with trailing
//! commas (the dump framing), and the `{"system": {...}}` wrapper of `/api/dump/{id64}`. Record kinds
//! are detected per line:
//!
//! * galaxy records (`galaxy*.json.gz`): system state + factions, stations (orbital and on bodies)
//!   with services, pads, economies and markets, ring hotspots;
//! * system records (`systems*.json.gz`): name + coordinates only;
//! * faction records (`factions.json.gz`): faction presence and controlling faction.
//!
//! Lines are read in chunks of at most [`CHUNK_BYTES`], parsed in parallel and handed to the writer
//! thread with at most [`PIPELINE_DEPTH`] chunks in flight, so memory stays bounded however large
//! the dump is.

use crate::commodities::normalize_symbol;
use crate::error::{GalaxyError, Result};
use crate::model::*;
use crate::names;
use crate::services::Services;
use crate::util::parse_ts;
use crate::writer::{
    CommodityHint, FactionPresence, ImportAck, ImportBatch, ImportOp, Shared, WriterMsg,
};
use crossbeam_channel::{Receiver, Sender};
use flate2::read::MultiGzDecoder;
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::io::{BufRead, BufReader, Read};
use std::rc::Rc;
use std::time::{Duration, Instant};

const CHUNK_BYTES: usize = 24 << 20;
const CHUNK_LINES: usize = 50_000;
const PIPELINE_DEPTH: usize = 2;

/// Progress of [`crate::GalaxyDb::import_spansh_dump`], reported after every chunk.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportProgress {
    /// Bytes consumed from the reader (compressed bytes for `.gz`): compare with the file size.
    pub bytes_read: u64,
    pub records: u64,
    pub systems: u64,
    pub stations: u64,
    pub markets: u64,
    pub market_rows: u64,
    pub ring_signals: u64,
    pub faction_presences: u64,
    /// Lines that were not valid records.
    pub parse_errors: u64,
    #[serde(skip)]
    pub elapsed: Duration,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportSummary {
    pub progress: ImportProgress,
    /// Markets skipped because the database already had newer data (e.g. from EDDN).
    pub stale_markets: u64,
    pub write_errors: u64,
    /// The progress callback returned `false`.
    pub cancelled: bool,
    /// The stream ended with a read/decompression error (truncated download); everything read
    /// before it was imported.
    pub stream_error: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Lenient scalar types
// ---------------------------------------------------------------------------------------------

/// Number that also accepts floats, numeric strings and null (-> None); anything else -> None.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Num(Option<f64>);

impl Num {
    fn f64(self) -> Option<f64> {
        self.0.filter(|f| f.is_finite())
    }
    fn i64(self) -> i64 {
        self.f64().map(|f| f as i64).unwrap_or(0)
    }
    fn u64(self) -> Option<u64> {
        self.f64().filter(|f| *f >= 0.0).map(|f| f as u64)
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Num, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Num;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Num, E> {
                Ok(Num(Some(v as f64)))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Num, E> {
                Ok(Num(Some(v as f64)))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Num, E> {
                Ok(Num(Some(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Num, E> {
                Ok(Num(v.trim().parse().ok()))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Num, E> {
                Ok(Num(None))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Num, E> {
                Ok(Num(None))
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Num, E> {
                Ok(Num(None))
            }
            fn visit_some<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Num, D::Error> {
                Num::deserialize(d)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Num, A::Error> {
                while a.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Num(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> std::result::Result<Num, A::Error> {
                while a.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Num(None))
            }
        }
        d.deserialize_any(V)
    }
}

/// Spansh ids (`id64`, station `id`): integer or string.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Id(Option<u64>);

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Id, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Id;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an integer id")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Id, E> {
                Ok(Id((v > 0).then_some(v as u64)))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Id, E> {
                Ok(Id((v > 0).then_some(v)))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Id, E> {
                Ok(Id((v.is_finite() && v > 0.0).then_some(v as u64)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Id, E> {
                Ok(Id(v.trim().parse().ok().filter(|n| *n > 0)))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Id, E> {
                Ok(Id(None))
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Id, E> {
                Ok(Id(None))
            }
            fn visit_some<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Id, D::Error> {
                Id::deserialize(d)
            }
        }
        d.deserialize_any(V)
    }
}

// ---------------------------------------------------------------------------------------------
// Dump records
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Coords {
    x: Num,
    y: Num,
    z: Num,
}

impl Coords {
    fn pos(&self) -> Option<[f64; 3]> {
        Some([self.x.f64()?, self.y.f64()?, self.z.f64()?])
    }
}

/// `controllingFaction` is an object on systems and a string on stations.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FactionRef {
    Name(String),
    Obj {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        state: Option<String>,
    },
    Other(IgnoredAny),
}

impl FactionRef {
    fn name(&self) -> Option<String> {
        match self {
            FactionRef::Name(n) => Some(n.trim().to_string()).filter(|n| !n.is_empty()),
            FactionRef::Obj { name, .. } => name.clone().filter(|n| !n.trim().is_empty()),
            FactionRef::Other(_) => None,
        }
    }
    fn state(&self) -> Option<String> {
        match self {
            FactionRef::Obj { state: Some(s), .. } => names::state(s),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct StateRef {
    state: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct DumpFaction {
    name: Option<String>,
    allegiance: Option<String>,
    government: Option<String>,
    influence: Num,
    state: Option<String>,
    happiness: Option<String>,
    active_states: Option<Vec<StateRef>>,
    pending_states: Option<Vec<StateRef>>,
    recovering_states: Option<Vec<StateRef>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DumpPads {
    large: Num,
    medium: Num,
    small: Num,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct DumpCommodity {
    name: Option<String>,
    symbol: Option<String>,
    category: Option<String>,
    commodity_id: Num,
    demand: Num,
    supply: Num,
    buy_price: Num,
    sell_price: Num,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct DumpMarket {
    commodities: Option<Vec<DumpCommodity>>,
    prohibited_commodities: Option<Vec<String>>,
    update_time: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct DumpStation {
    name: Option<String>,
    id: Id,
    update_time: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    distance_to_arrival: Num,
    landing_pads: Option<DumpPads>,
    services: Option<Vec<String>>,
    primary_economy: Option<String>,
    economies: Option<BTreeMap<String, Num>>,
    controlling_faction: Option<FactionRef>,
    controlling_faction_state: Option<String>,
    government: Option<String>,
    allegiance: Option<String>,
    carrier_docking_access: Option<String>,
    latitude: Num,
    market: Option<DumpMarket>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct DumpRingSignals {
    signals: Option<BTreeMap<String, Num>>,
    update_time: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DumpRing {
    name: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    signals: Option<DumpRingSignals>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DumpBody {
    rings: Option<Vec<DumpRing>>,
    stations: Option<Vec<DumpStation>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct FactionSystem {
    system_name: Option<String>,
    system_id64: Id,
    is_controlling_faction: Option<bool>,
    coords: Option<Coords>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct DumpRecord {
    id64: Id,
    name: Option<String>,
    coords: Option<Coords>,
    allegiance: Option<String>,
    government: Option<String>,
    primary_economy: Option<String>,
    secondary_economy: Option<String>,
    security: Option<String>,
    population: Num,
    controlling_faction: Option<FactionRef>,
    factions: Option<Vec<DumpFaction>>,
    controlling_power: Option<String>,
    powers: Option<Vec<String>>,
    power_state: Option<String>,
    date: Option<String>,
    update_time: Option<String>,
    stations: Option<Vec<DumpStation>>,
    bodies: Option<Vec<DumpBody>>,
    /// factions dump
    systems: Option<Vec<FactionSystem>>,
    /// `/api/dump/{id64}` wrapper
    system: Option<Box<DumpRecord>>,
}

// ---------------------------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Parsed {
    ops: Vec<ImportOp>,
    hints: Vec<CommodityHint>,
    counts: ImportProgress,
}

fn text(s: &Option<String>) -> Option<String> {
    s.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn states(v: Option<&[StateRef]>) -> Vec<String> {
    v.unwrap_or_default()
        .iter()
        .filter_map(|s| s.state.as_deref().and_then(names::state))
        .collect()
}

/// Parses one trimmed line.
fn parse_record(line: &[u8], known: &FxHashSet<String>) -> serde_json::Result<Parsed> {
    let mut rec: DumpRecord = serde_json::from_slice(line)?;
    if let Some(inner) = rec.system.take() {
        rec = *inner;
    }
    let mut out = Parsed::default();
    out.counts.records = 1;
    if let Some(systems) = &rec.systems
        && rec.id64.0.is_none()
    {
        faction_record(&rec, systems, &mut out);
    } else {
        system_record(rec, known, &mut out);
    }
    Ok(out)
}

fn faction_record(rec: &DumpRecord, systems: &[FactionSystem], out: &mut Parsed) {
    let Some(faction) = text(&rec.name) else {
        return;
    };
    let allegiance = rec.allegiance.as_deref().and_then(names::allegiance);
    let government = rec.government.as_deref().and_then(names::government);
    // Every controlled system is listed twice (with and without the flag): merge by id.
    let mut merged: BTreeMap<u64, FactionPresence> = BTreeMap::new();
    for s in systems {
        let (Some(id), Some(name)) = (s.system_id64.0, text(&s.system_name)) else {
            continue;
        };
        let controlling = s.is_controlling_faction.unwrap_or(false);
        merged
            .entry(id)
            .and_modify(|p| p.controlling |= controlling)
            .or_insert_with(|| FactionPresence {
                system_id64: id,
                system_name: name,
                pos: s.coords.as_ref().and_then(Coords::pos),
                faction: faction.clone(),
                allegiance: allegiance.clone(),
                government: government.clone(),
                controlling,
                timestamp: 0,
            });
    }
    out.counts.faction_presences += merged.len() as u64;
    out.ops
        .extend(merged.into_values().map(ImportOp::FactionPresence));
}

fn system_record(rec: DumpRecord, known: &FxHashSet<String>, out: &mut Parsed) {
    let (Some(id64), Some(name)) = (rec.id64.0, text(&rec.name)) else {
        return;
    };
    let ts = rec
        .date
        .as_deref()
        .or(rec.update_time.as_deref())
        .and_then(parse_ts)
        .unwrap_or(0);
    let is_galaxy = rec.stations.is_some() || rec.bodies.is_some() || rec.factions.is_some();
    let details = is_galaxy.then(|| SystemDetails {
        population: rec.population.u64(),
        allegiance: rec.allegiance.as_deref().and_then(names::allegiance),
        government: rec.government.as_deref().and_then(names::government),
        economy: rec.primary_economy.as_deref().and_then(names::economy),
        second_economy: rec.secondary_economy.as_deref().and_then(names::economy),
        security: rec.security.as_deref().and_then(names::security),
        controlling_faction: rec.controlling_faction.as_ref().and_then(FactionRef::name),
        controlling_faction_state: rec.controlling_faction.as_ref().and_then(FactionRef::state),
        controlling_power: text(&rec.controlling_power),
        powerplay_state: rec.power_state.as_deref().and_then(names::state),
        powers: rec.powers.clone().unwrap_or_default(),
        factions: rec.factions.as_ref().map(|fs| {
            fs.iter()
                .filter_map(|f| {
                    Some(FactionUpdate {
                        name: text(&f.name)?,
                        influence: f.influence.f64(),
                        state: f.state.as_deref().and_then(names::state),
                        government: f.government.as_deref().and_then(names::government),
                        allegiance: f.allegiance.as_deref().and_then(names::allegiance),
                        happiness: f.happiness.as_deref().and_then(names::happiness),
                        active_states: states(f.active_states.as_deref()),
                        pending_states: states(f.pending_states.as_deref()),
                        recovering_states: states(f.recovering_states.as_deref()),
                    })
                })
                .collect()
        }),
        // Dumps carry faction states, not the game's conflict list.
        conflicts: None,
    });
    out.counts.systems += 1;
    out.ops.push(ImportOp::from(Ingest::System(SystemUpdate {
        id64,
        name: name.clone(),
        pos: rec.coords.as_ref().and_then(Coords::pos),
        timestamp: ts,
        details,
    })));

    let mut seen_hint: FxHashSet<String> = FxHashSet::default();
    for st in rec.stations.iter().flatten() {
        station_ops(st, id64, &name, ts, false, known, &mut seen_hint, out);
    }
    for body in rec.bodies.iter().flatten() {
        for st in body.stations.iter().flatten() {
            station_ops(st, id64, &name, ts, true, known, &mut seen_hint, out);
        }
        for ring in body.rings.iter().flatten() {
            let Some(body_name) = text(&ring.name) else {
                continue;
            };
            let Some(sig) = &ring.signals else { continue };
            let signals: Vec<(String, i64)> = sig
                .signals
                .iter()
                .flatten()
                .filter(|(k, _)| !k.starts_with('$'))
                .map(|(k, v)| (k.trim().to_string(), v.i64().max(0)))
                .collect();
            if signals.is_empty() {
                continue;
            }
            out.counts.ring_signals += signals.len() as u64;
            out.ops
                .push(ImportOp::from(Ingest::RingSignals(RingSignalsUpdate {
                    system_id64: id64,
                    system_name: Some(name.clone()),
                    body_name,
                    ring_type: text(&ring.kind),
                    signals,
                    timestamp: sig.update_time.as_deref().and_then(parse_ts).unwrap_or(ts),
                })));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn station_ops(
    st: &DumpStation,
    system_id64: u64,
    system_name: &str,
    system_ts: i64,
    on_body: bool,
    known: &FxHashSet<String>,
    seen_hint: &mut FxHashSet<String>,
    out: &mut Parsed,
) {
    let (Some(market_id), Some(name)) = (st.id.0, text(&st.name)) else {
        return;
    };
    let ts = st
        .update_time
        .as_deref()
        .and_then(parse_ts)
        .unwrap_or(system_ts);
    let mut station_type = text(&st.kind);
    if station_type.is_none() && st.latitude.f64().is_some() {
        station_type = Some("Settlement".to_string());
    }
    let economies = st.economies.as_ref().map(|m| {
        let mut e: Vec<(String, f64)> = m
            .iter()
            .filter_map(|(k, v)| Some((names::economy(k)?, v.f64().unwrap_or(0.0))))
            .collect();
        e.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        e
    });
    out.counts.stations += 1;
    out.ops
        .push(ImportOp::from(Ingest::Station(StationUpdate {
            market_id,
            name: name.clone(),
            system_id64: Some(system_id64),
            system_name: Some(system_name.to_string()),
            station_type: station_type.clone(),
            dist_ls: st.distance_to_arrival.f64().filter(|d| *d >= 0.0),
            landing_pads: st.landing_pads.as_ref().map(|p| LandingPads {
                small: p.small.u64().unwrap_or(0).min(1000) as u32,
                medium: p.medium.u64().unwrap_or(0).min(1000) as u32,
                large: p.large.u64().unwrap_or(0).min(1000) as u32,
            }),
            services: st.services.as_ref().map(Services::from_names),
            primary_economy: st.primary_economy.as_deref().and_then(names::economy),
            economies,
            faction: st.controlling_faction.as_ref().and_then(FactionRef::name),
            faction_state: st
                .controlling_faction_state
                .as_deref()
                .and_then(names::state),
            government: st.government.as_deref().and_then(names::government),
            allegiance: st.allegiance.as_deref().and_then(names::allegiance),
            carrier_docking_access: text(&st.carrier_docking_access),
            planetary: on_body.then_some(true),
            timestamp: ts,
        })));

    let Some(m) = &st.market else { return };
    let commodities = m.commodities.as_deref().unwrap_or_default();
    let mut items = Vec::with_capacity(commodities.len());
    for c in commodities {
        let raw = c
            .symbol
            .as_deref()
            .or(c.name.as_deref())
            .unwrap_or_default();
        let symbol = if c.symbol.is_some() {
            normalize_symbol(raw)
        } else {
            names::key(raw)
        };
        if symbol.is_empty() {
            continue;
        }
        if !known.contains(&symbol) && seen_hint.insert(symbol.clone()) {
            out.hints.push(CommodityHint {
                symbol: symbol.clone(),
                name: c.name.clone().unwrap_or_default(),
                category: c.category.clone().unwrap_or_default(),
                fdev_id: c.commodity_id.f64().map(|f| f as i64),
            });
        }
        items.push(MarketItem {
            symbol,
            buy: c.buy_price.i64().max(0),
            sell: c.sell_price.i64().max(0),
            mean: 0,
            stock: c.supply.i64().max(0),
            stock_bracket: None,
            demand: c.demand.i64().max(0),
            demand_bracket: None,
        });
    }
    out.counts.markets += 1;
    out.counts.market_rows += items.len() as u64;
    out.ops
        .push(ImportOp::from(Ingest::Market(MarketSnapshot {
            market_id,
            system_name: system_name.to_string(),
            station_name: name,
            station_type,
            carrier_docking_access: text(&st.carrier_docking_access),
            timestamp: m.update_time.as_deref().and_then(parse_ts).unwrap_or(ts),
            items,
            prohibited: m.prohibited_commodities.clone(),
            economies: None,
            source: Source::Spansh,
        })));
}

// ---------------------------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------------------------

struct CountingReader<R> {
    inner: R,
    count: Rc<Cell<u64>>,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count.set(self.count.get() + n as u64);
        Ok(n)
    }
}

/// Strips framing (`[`, `]`, tab indentation, trailing commas); `None` for lines without a record.
fn trim_record(line: &[u8]) -> Option<&[u8]> {
    let mut s = line.trim_ascii();
    if let Some(rest) = s.strip_suffix(b",") {
        s = rest.trim_ascii_end();
    }
    if s.is_empty() || s == b"[" || s == b"]" {
        return None;
    }
    // A whole single-line array `[{...},{...}]` is not supported; tolerate a leading '[' on the first record.
    if let Some(rest) = s.strip_prefix(b"[")
        && rest.trim_ascii_start().starts_with(b"{")
    {
        s = rest.trim_ascii_start();
    }
    if let Some(rest) = s.strip_suffix(b"]")
        && rest.trim_ascii_end().ends_with(b"}")
    {
        s = rest.trim_ascii_end();
    }
    Some(s)
}

fn add_counts(total: &mut ImportProgress, c: &ImportProgress) {
    total.records += c.records;
    total.systems += c.systems;
    total.stations += c.stations;
    total.markets += c.markets;
    total.market_rows += c.market_rows;
    total.ring_signals += c.ring_signals;
    total.faction_presences += c.faction_presences;
}

pub(crate) fn import<R: Read>(
    tx: &Sender<WriterMsg>,
    shared: &Shared,
    reader: R,
    mut progress: impl FnMut(&ImportProgress) -> bool,
) -> Result<ImportSummary> {
    let started = Instant::now();
    let count = Rc::new(Cell::new(0u64));
    let mut buf = BufReader::with_capacity(
        1 << 20,
        CountingReader {
            inner: reader,
            count: count.clone(),
        },
    );
    let gz = {
        let head = buf.fill_buf()?;
        head.len() >= 2 && head[0] == 0x1f && head[1] == 0x8b
    };
    let mut lines: Box<dyn BufRead> = if gz {
        Box::new(BufReader::with_capacity(1 << 20, MultiGzDecoder::new(buf)))
    } else {
        Box::new(buf)
    };

    let mut known: FxHashSet<String> = shared
        .commodities
        .read()
        .iter()
        .map(|(_, m)| m.symbol.clone())
        .collect();
    let mut summary = ImportSummary::default();
    let mut inflight: VecDeque<Receiver<ImportAck>> = VecDeque::new();
    let mut line = Vec::with_capacity(1 << 16);
    let mut eof = false;

    let wait = |rx: Receiver<ImportAck>, summary: &mut ImportSummary| -> Result<()> {
        let ack = rx.recv().map_err(|_| GalaxyError::WriterGone)?;
        summary.stale_markets += ack.stale_markets;
        summary.write_errors += ack.errors;
        if !ack.committed {
            return Err(GalaxyError::Other(
                "import chunk could not be committed".into(),
            ));
        }
        Ok(())
    };

    while !eof {
        let mut chunk: Vec<Vec<u8>> = Vec::new();
        let mut bytes = 0usize;
        while bytes < CHUNK_BYTES && chunk.len() < CHUNK_LINES {
            line.clear();
            match lines.read_until(b'\n', &mut line) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(n) => {
                    bytes += n;
                    if let Some(rec) = trim_record(&line) {
                        chunk.push(rec.to_vec());
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "spansh dump stream ended with an error");
                    summary.stream_error = Some(e.to_string());
                    // a trailing partial line is not a record
                    eof = true;
                    break;
                }
            }
        }
        if chunk.is_empty() {
            continue;
        }
        let parsed: Vec<serde_json::Result<Parsed>> =
            chunk.par_iter().map(|l| parse_record(l, &known)).collect();
        drop(chunk);
        let mut ops = Vec::new();
        let mut hints = Vec::new();
        for p in parsed {
            match p {
                Ok(p) => {
                    add_counts(&mut summary.progress, &p.counts);
                    ops.extend(p.ops);
                    for h in p.hints {
                        if known.insert(h.symbol.clone()) {
                            hints.push(h);
                        }
                    }
                }
                Err(e) => {
                    summary.progress.parse_errors += 1;
                    if summary.progress.parse_errors <= 5 {
                        tracing::warn!(error = %e, "unparseable spansh dump line");
                    }
                }
            }
        }
        if !ops.is_empty() || !hints.is_empty() {
            let (done_tx, done_rx) = crossbeam_channel::bounded(1);
            tx.send(WriterMsg::Import(ImportBatch {
                ops,
                hints,
                done: done_tx,
            }))
            .map_err(|_| GalaxyError::WriterGone)?;
            inflight.push_back(done_rx);
            while inflight.len() > PIPELINE_DEPTH {
                if let Some(rx) = inflight.pop_front() {
                    wait(rx, &mut summary)?;
                }
            }
        }
        summary.progress.bytes_read = count.get();
        summary.progress.elapsed = started.elapsed();
        if !progress(&summary.progress) {
            summary.cancelled = true;
            break;
        }
    }
    while let Some(rx) = inflight.pop_front() {
        wait(rx, &mut summary)?;
    }
    let (cp_tx, cp_rx) = crossbeam_channel::bounded(1);
    if tx.send(WriterMsg::Checkpoint(cp_tx)).is_ok() {
        let _ = cp_rx.recv();
    }
    summary.progress.bytes_read = count.get();
    summary.progress.elapsed = started.elapsed();
    tracing::info!(
        records = summary.progress.records,
        stations = summary.progress.stations,
        markets = summary.progress.markets,
        secs = summary.progress.elapsed.as_secs_f64(),
        "spansh import finished"
    );
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing() {
        assert_eq!(trim_record(b"[\n"), None);
        assert_eq!(trim_record(b"]"), None);
        assert_eq!(trim_record(b"\t{\"a\":1},\n"), Some(&b"{\"a\":1}"[..]));
        assert_eq!(trim_record(b"\t{\"a\":1}\r\n"), Some(&b"{\"a\":1}"[..]));
        assert_eq!(trim_record(b"[{\"a\":1},"), Some(&b"{\"a\":1}"[..]));
    }

    #[test]
    fn nulls_and_odd_types_are_tolerated() {
        let known = FxHashSet::default();
        let line = br#"{"id64": "5", "name": "X", "coords": {"x": 1, "y": "2", "z": 3.5}, "population": null,
            "controllingFaction": {"name": "F"}, "factions": [{"name": "F", "influence": 0.5, "activeStates": null}],
            "stations": [{"name": "S", "id": 9, "market": {"commodities": null, "updateTime": "2026-10-12T00:00:00Z"},
                          "services": null, "landingPads": null, "controllingFaction": "F", "distanceToArrival": "12.5"}],
            "bodies": null, "powers": null}"#;
        let p = parse_record(line, &known).expect("parse");
        assert_eq!(
            (p.counts.systems, p.counts.stations, p.counts.markets),
            (1, 1, 1)
        );
        let Some(ImportOp::Ingest(op)) = p.ops.first() else {
            panic!("{:?}", p.ops)
        };
        let Ingest::System(s) = &**op else { panic!("{op:?}") };
        assert_eq!(s.pos, Some([1.0, 2.0, 3.5]));
        assert_eq!(
            s.details
                .as_ref()
                .and_then(|d| d.controlling_faction.clone())
                .as_deref(),
            Some("F")
        );
        let Some(ImportOp::Ingest(op)) = p.ops.get(1) else {
            panic!()
        };
        let Ingest::Station(st) = &**op else { panic!("{op:?}") };
        assert_eq!(st.dist_ls, Some(12.5));
        assert_eq!(st.faction.as_deref(), Some("F"));
    }

    #[test]
    fn lenient_numbers() {
        let v: Vec<Num> =
            serde_json::from_str(r#"[1, 2.5, "3", null, true, [1], {"a": 1}]"#).expect("parse");
        assert_eq!(v[0].i64(), 1);
        assert_eq!(v[1].f64(), Some(2.5));
        assert_eq!(v[2].i64(), 3);
        assert_eq!(v[3].f64(), None);
        assert_eq!(v[4].f64(), None);
        assert_eq!(v[5].f64(), None);
        assert_eq!(v[6].f64(), None);
        let ids: Vec<Id> =
            serde_json::from_str(r#"[3708716800, "128016384", null, -1]"#).expect("ids");
        assert_eq!(
            ids,
            vec![
                Id(Some(3708716800)),
                Id(Some(128016384)),
                Id(None),
                Id(None)
            ]
        );
    }
}
