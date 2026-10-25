//! The overlay: one black island at the top of the game's screen. It says what's going on in a word or two, opens up
//! for the moments that need you, and fades away when nothing does.
//!
//! This file is its view model, pure and tested: [`Inputs::read`] copies what the island needs out of the model, and
//! [`Rhythm::view`] picks the moment, its size and its words. `App::logic` rebuilds it only when what it reads has
//! changed; [`window`] shows it in a window of its own, [`draw`] paints it.

pub mod draw;
pub mod platform;
pub mod window;

use crate::finder::{carriers_here, presence, Presence};
use crate::pages::market::Pin;
use crate::pages::mining::{distance_text, MarkKind, MiningPrefs};
use crate::pages::navigation::PlannedRoute;
use crate::pages::Page;
use egui_phosphor::fill as glyph;
use nexus_data::exploration::is_scoopable;
use nexus_data::names::{compact_credits, group_digits};
use nexus_journal::companion::GuiFocus;
use nexus_journal::Timestamp;
use nexus_model::carrier::JumpTimer;
use nexus_model::context::{Activity, Context, Vehicle};
use nexus_model::location::{surface_bearing, surface_distance};
use nexus_model::{Alert, AlertKind, GameState, Priority};
use std::collections::VecDeque;

// ---------------------------------------------------------------------------------------------
// What the island shows

/// The moment the island is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    NextJump,
    Arrival,
    Approach,
    Sampling,
    Mining,
    Trade,
    Docked,
    Carrier,
    Alert,
}

impl Moment {
    /// The page a click on the (unlocked) island opens in the main window.
    pub fn page(self) -> Page {
        match self {
            Moment::NextJump => Page::Navigation,
            Moment::Arrival => Page::Exploration,
            Moment::Approach | Moment::Sampling => Page::Exobiology,
            Moment::Mining => Page::Mining,
            Moment::Trade => Page::Market,
            Moment::Carrier => Page::Carrier,
            Moment::Docked | Moment::Alert => Page::Dashboard,
        }
    }
}

/// How much of the island there is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Size {
    /// Faded away.
    #[default]
    Gone,
    /// An empty capsule: idle (when kept), or a cockpit panel is open.
    Minimal,
    /// A capsule with a glyph and a word or two on the left, one value on the right.
    Compact,
    /// Opened up for a moment that needs you.
    Expanded,
}

/// One tint per activity; green, yellow and red only mean go, attention and danger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    White,
    /// Secondary text.
    Dim,
    Mint,
    Orange,
    Blue,
    Cyan,
    Gold,
    Indigo,
    Green,
    Yellow,
    Red,
}

/// A star's colour, from its class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarTone {
    Blue,
    White,
    YellowWhite,
    Yellow,
    Orange,
    Red,
    Brown,
    Neutron,
    Dwarf,
    Black,
}

impl StarTone {
    /// `K`, `K_OrangeGiant`, `DA`, `N`, `TTS`, …
    pub fn of(class: &str) -> StarTone {
        match class.split('_').next().unwrap_or_default() {
            "O" | "B" => StarTone::Blue,
            "A" => StarTone::White,
            "F" => StarTone::YellowWhite,
            "G" => StarTone::Yellow,
            "K" | "TTS" | "AeBe" => StarTone::Orange,
            "M" | "C" | "CN" | "CJ" | "CH" | "CHd" | "MS" | "S" => StarTone::Red,
            "L" | "T" | "Y" => StarTone::Brown,
            "N" => StarTone::Neutron,
            "H" | "SupermassiveBlackHole" => StarTone::Black,
            c if c.starts_with('D') => StarTone::Dwarf,
            c if c.starts_with('W') => StarTone::Blue,
            _ => StarTone::Orange,
        }
    }
}

/// The island's type styles: Inter, as the app's stand-in for SF Pro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Medium 13: secondary lines.
    Caption,
    /// Semibold 13: the compact island's words.
    Label,
    /// Semibold 15: the compact island's value.
    Value,
    /// Semibold 17: titles.
    Title,
    /// Bold 20: a value beside a title.
    Figure,
    /// Bold 34: the one big value.
    Hero,
}

/// Text in one style and tint. Values get tabular figures, so they don't shift as they change.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    pub tint: Tint,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Glyph {
    /// A filled Phosphor icon (`egui_phosphor::fill`), `size` points.
    Icon { icon: &'static str, tint: Tint, size: u8 },
    /// A star: a disc in its colour with its class letter.
    Star { class: String, size: u8 },
    /// Which way to go: an arrow turned `turn` degrees clockwise from straight ahead.
    Arrow { turn: i16, tint: Tint, size: u8 },
    /// A dot, filled or hollow.
    Dot { tint: Tint, hollow: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Text(Span),
    Glyph(Glyph),
    /// The fuel gauge: a short bar, 0 to 100 %.
    Fuel(u8),
}

/// The compact island: pieces on the left, pieces on the right.
#[derive(Debug, Clone, PartialEq)]
pub struct Compact {
    pub lead: Vec<Piece>,
    pub trail: Vec<Piece>,
}

/// One row of the expanded island.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// A glyph, two lines (they truncate), and lines of pieces aligned right.
    Header { lead: Option<Glyph>, top: Span, bottom: Span, right: Vec<Vec<Piece>> },
    /// Groups of pieces spread across the row: the first at the left, the last at the right.
    Spread(Vec<Vec<Piece>>),
    /// Small capsules (the genera on a body); done ones dimmed.
    Chips(Vec<Chip>),
    /// A radar dial, lines beside it, and a ring of progress (done, of).
    Dial { dial: Dial, lines: Vec<Vec<Piece>>, ring: Option<(u8, u8)> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expanded {
    pub rows: Vec<Row>,
}

/// A radar centred on you, turned so that up is the way you're facing.
#[derive(Debug, Clone, PartialEq)]
pub struct Dial {
    /// Rings evenly spaced out to the edge.
    pub rings: u8,
    /// Where north is, in degrees clockwise from up.
    pub north: i16,
    pub marks: Vec<DialMark>,
}

/// A mark on the dial, in hundredths of its radius from the centre (y up).
#[derive(Debug, Clone, PartialEq)]
pub struct DialMark {
    pub x: i16,
    pub y: i16,
    pub kind: DialKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialKind {
    /// An earlier sample and its colony range (hundredths of the radius); `close` while you're inside it.
    Sample { range: u8, close: bool },
    Deposit { nearest: bool },
    Ship,
    Landing,
}

/// The small capsule beside the island for a second thing going on.
#[derive(Debug, Clone, PartialEq)]
pub struct Side {
    pub icon: &'static str,
    pub tint: Tint,
    pub label: String,
    pub value: String,
}

/// Everything the window needs to draw the island.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IslandView {
    pub moment: Option<Moment>,
    pub size: Size,
    pub compact: Option<Compact>,
    pub expanded: Option<Expanded>,
    pub side: Option<Side>,
    /// When the view changes by itself (an expansion settles, an alert ends), on the clock `view` was given.
    pub next_change: Option<f64>,
}

// ---------------------------------------------------------------------------------------------
// What the island reads

/// What the island needs from the model: small, owned and comparable, so it's rebuilt only when it changes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Inputs {
    pub context: Context,
    /// Unix seconds, for countdowns.
    pub now: i64,
    pub jump: Option<Jump>,
    pub arrival: Option<Arrival>,
    pub approach: Option<Approach>,
    pub sampling: Option<Sampling>,
    pub mining: Option<Mining>,
    pub trade: Option<Trade>,
    pub docked: Option<Docked>,
    pub carrier: Option<CarrierJump>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Jump {
    /// The next system: the FSD's target, or the route's next stop.
    pub system: String,
    pub star_class: String,
    pub jumps_left: Option<u32>,
    /// Jumps to the next neutron star on the route (1: the next jump).
    pub neutron_in: Option<u32>,
    /// The main tank, 0 to 100 %.
    pub fuel: Option<u8>,
    /// Its name is on the clipboard (copied after each jump).
    pub on_clipboard: bool,
    /// In hyperspace: the system being jumped to and its star class.
    pub arriving: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arrival {
    pub system_address: u64,
    pub system: String,
    /// Bodies scanned, and how many the discovery scan found.
    pub scanned: u32,
    pub total: Option<u32>,
    /// What the system is worth once every planet worth mapping is mapped.
    pub value: i64,
    pub bio_bodies: u32,
    pub first_discovery: bool,
    /// The kinds of the notable worlds: Earth-like, water, ammonia.
    pub highlights: Vec<String>,
    /// Worth opening the island for: a valuable body, biology or a first discovery.
    pub worth_it: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Approach {
    /// The body's name without the system's.
    pub body: String,
    /// Gravity, atmosphere, temperature ("0.31 g · thin ammonia · 162 K").
    pub conditions: Vec<String>,
    pub bio_signals: u32,
    /// The genera found there (the surface scanner names them), and whether each is analysed.
    pub genera: Vec<Chip>,
    /// What its biology is likely worth, and whether the first-logged bonus (×5) is likely.
    pub value: i64,
    pub bonus: bool,
    /// Where to land: the nearest place known on this body.
    pub target: Option<Target>,
    /// Mapping it with the surface scanner, and what it's worth mapped.
    pub mapping: bool,
    pub mapped_value: i64,
}

/// A place on the surface: how far, the compass bearing, and how far to turn from your heading.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub label: String,
    pub dist_m: u32,
    pub bearing: u16,
    pub turn: i16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sampling {
    pub species: String,
    pub value: Option<i64>,
    /// Samples taken, 1 to 3.
    pub stage: u8,
    pub range_m: Option<u32>,
    /// From the nearest earlier sample (None when no positions were recorded).
    pub nearest_m: Option<u32>,
    pub far_enough: Option<bool>,
    pub dial: Option<Dial>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mining {
    /// The nearest deposit, what came out there, and its mining location's number.
    pub deposit: Option<Target>,
    pub commodity: String,
    pub location: Option<u32>,
    pub ship: Option<Target>,
    pub landing: Option<Target>,
    pub dial: Dial,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trade {
    /// The carrier's name as the navigation panel lists it, or the station's.
    pub name: String,
    /// A carrier's callsign: the navigation panel lists carriers by it.
    pub callsign: Option<String>,
    pub presence: Presence,
    /// You buy there (the pin is a seller), or sell.
    pub buy: bool,
    pub commodity: String,
    pub tonnes: Option<i64>,
    pub price: i64,
    /// How old the price is: when it was pinned.
    pub age_secs: i64,
    /// The next stop of the route: its system and the jumps to it.
    pub then: Option<(String, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Docked {
    pub station: String,
    pub system: String,
    pub market_id: u64,
    /// Exobiology data this station buys (Vista Genomics), with the bonuses expected.
    pub exobio: i64,
    /// Exploration data it buys (Universal Cartographics).
    pub carto: i64,
    /// Pinned orders here: (you buy, commodity, price).
    pub orders: Vec<(bool, String, i64)>,
}

impl Docked {
    fn has_news(&self) -> bool {
        self.exobio > 0 || self.carto > 0 || !self.orders.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CarrierJump {
    pub to: String,
    /// Unix seconds.
    pub departs: i64,
}

/// What [`Inputs::read`] needs besides the model.
pub struct Sources<'a> {
    pub route: Option<&'a PlannedRoute>,
    pub pins: &'a [Pin],
    pub mining: &'a MiningPrefs,
    /// The next system is copied to the clipboard after each jump.
    pub auto_copy: bool,
    /// A body worth at least this (mapped) makes an arrival worth opening the island for.
    pub high_value: i64,
    /// Unix seconds.
    pub now: i64,
}

impl Inputs {
    pub fn read(gs: &GameState, src: &Sources<'_>) -> Inputs {
        let context = Context::of(gs);
        let here = position(gs);
        Inputs {
            jump: read_jump(gs, src),
            arrival: read_arrival(gs, src.high_value),
            approach: read_approach(gs, &context, src.mining, here),
            sampling: read_sampling(gs, here),
            mining: read_mining(gs, src.mining),
            trade: read_trade(gs, src),
            docked: read_docked(gs, src.pins),
            carrier: read_carrier(gs, src.now),
            now: src.now,
            context,
        }
    }
}

/// Where the commander is on the body (Status.json): latitude and longitude, the body's radius, the heading.
#[derive(Clone, Copy)]
struct Here {
    pos: (f64, f64),
    radius_m: f64,
    heading: f64,
}

fn position(gs: &GameState) -> Option<Here> {
    let st = gs.status.as_ref()?;
    Some(Here { pos: st.lat_lon()?, radius_m: st.planet_radius.filter(|r| *r > 0.0)?, heading: st.heading.unwrap_or(0.0) })
}

/// Degrees to turn from `heading` to face `bearing`: −180 to 180, clockwise positive.
fn turn(bearing: f64, heading: f64) -> i16 {
    ((bearing - heading + 540.0).rem_euclid(360.0) - 180.0).round() as i16
}

fn target(label: String, dist_m: f64, bearing: f64, heading: f64) -> Target {
    Target { label, dist_m: dist_m.round().clamp(0.0, f64::from(u32::MAX)) as u32, bearing: (bearing.round() as i64).rem_euclid(360) as u16, turn: turn(bearing, heading) }
}

fn read_jump(gs: &GameState, src: &Sources<'_>) -> Option<Jump> {
    let loc = &gs.location;
    let (next, jumps, neutron_in) = match src.route {
        Some(p) => summarise(p.remaining().iter().map(|w| (w.system.as_str(), w.star_class.as_deref().unwrap_or_default(), w.jumps.max(1), w.neutron))),
        None => summarise(gs.route_remaining().iter().map(|h| (h.star_system.as_str(), h.star_class.as_str(), 1, h.star_class == "N"))),
    };
    let fsd = loc.fsd_target.as_ref().filter(|t| !t.name.is_empty() && t.system_address != loc.system_address);
    let (system, star_class) = match (fsd, next) {
        (Some(t), _) => (t.name.clone(), t.star_class.clone()),
        (None, Some((s, c))) => (s.to_string(), c.to_string()),
        (None, None) => (String::new(), String::new()),
    };
    let arriving = gs.jumping_to().map(|(s, c)| (s.to_string(), c.to_string()));
    if system.is_empty() && arriving.is_none() {
        return None;
    }
    let fuel = gs.fleet.current().filter(|s| s.fuel_main_capacity > 0.0).map(|s| (gs.fleet.fuel_main / s.fuel_main_capacity * 100.0).round().clamp(0.0, 100.0) as u8);
    Some(Jump {
        on_clipboard: src.auto_copy && next.is_some_and(|(s, _)| s.eq_ignore_ascii_case(&system)),
        jumps_left: (jumps > 0).then_some(jumps).or_else(|| fsd.and_then(|t| t.remaining_jumps)),
        neutron_in,
        system,
        star_class,
        fuel,
        arriving,
    })
}

/// The route ahead from its stops (system, star class, jumps to it, neutron): the next stop, the jumps left, and the
/// jumps to the first neutron star.
fn summarise<'a>(stops: impl Iterator<Item = (&'a str, &'a str, u32, bool)>) -> (Option<(&'a str, &'a str)>, u32, Option<u32>) {
    let (mut next, mut jumps, mut neutron) = (None, 0, None);
    for (system, class, n, is_neutron) in stops {
        if next.is_none() {
            next = Some((system, class));
        }
        jumps += n;
        if is_neutron && neutron.is_none() {
            neutron = Some(jumps);
        }
    }
    (next, jumps, neutron)
}

fn read_arrival(gs: &GameState, high_value: i64) -> Option<Arrival> {
    let sys = gs.exploration.current_system().filter(|s| s.address == gs.location.system_address)?;
    let (mut valuable, mut first, mut bio_bodies) = (false, false, 0);
    let mut highlights: Vec<String> = Vec::new();
    for b in sys.bodies.values() {
        if b.bio_signals > 0 {
            bio_bodies += 1;
        }
        if !b.has_scan {
            continue;
        }
        valuable |= b.value.mapped.max(b.value.current) >= high_value;
        first |= b.first_discovery();
        let kind = match b.planet_class.as_deref() {
            Some("Earthlike body") => "Earth-like world",
            Some("Water world") => "Water world",
            Some("Ammonia world") => "Ammonia world",
            _ => continue,
        };
        if !highlights.iter().any(|h| h == kind) {
            highlights.push(kind.to_string());
        }
    }
    Some(Arrival {
        system_address: sys.address,
        system: sys.name.clone(),
        scanned: sys.scanned_bodies() as u32,
        total: sys.fss_body_count,
        value: sys.potential_value(),
        bio_bodies,
        first_discovery: first,
        worth_it: valuable || bio_bodies > 0 || first || !highlights.is_empty(),
        highlights,
    })
}

fn read_approach(gs: &GameState, c: &Context, prefs: &MiningPrefs, here: Option<Here>) -> Option<Approach> {
    let body_ref = c.body.as_ref()?;
    let loc = &gs.location;
    let sys = gs.exploration.systems.get(&loc.system_address);
    let body = sys.and_then(|s| body_ref.id.and_then(|id| s.bodies.get(&id)).or_else(|| s.bodies.values().find(|b| b.name == body_ref.name)));
    let short = body_ref.name.strip_prefix(loc.system.as_str()).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&body_ref.name).to_string();
    let mut conditions = Vec::new();
    let gravity = body.and_then(|b| b.gravity_g()).or_else(|| gs.status.as_ref().and_then(|s| s.gravity));
    if let Some(g) = gravity {
        conditions.push(format!("{g:.2} g"));
    }
    let atmosphere = body.and_then(|b| b.atmosphere.as_deref()).map(|a| a.trim().trim_end_matches(" atmosphere").to_string()).filter(|a| !a.is_empty() && !a.eq_ignore_ascii_case("no"));
    conditions.extend(atmosphere);
    if let Some(t) = body.and_then(|b| b.temperature_k) {
        conditions.push(format!("{t:.0} K"));
    }
    let populated = loc.info.population > 0;
    let bonus = body.is_some_and(|b| nexus_data::bio_predict::first_logged_bonus_likely(b.was_footfalled, populated, None));
    let genera = body
        .map(|b| b.genuses.iter().map(|g| Chip { text: g.clone(), done: b.organics.iter().any(|o| o.analysed && o.genus.eq_ignore_ascii_case(g)) }).collect())
        .unwrap_or_default();
    Some(Approach {
        body: short,
        conditions,
        bio_signals: body.map_or(0, |b| b.bio_signals),
        genera,
        value: body.map_or(0, |b| b.predicted_value(bonus)),
        bonus,
        target: here.and_then(|h| landing_target(gs, prefs, body_ref.id, h)),
        mapping: c.focus == GuiFocus::Saa,
        mapped_value: body.map_or(0, |b| b.value.mapped),
    })
}

/// Where to land on the body: the settlement you're approaching, the nearest deposit the Rhino refined at, or where the
/// ship set down by a mining location last time.
fn landing_target(gs: &GameState, prefs: &MiningPrefs, body_id: Option<u32>, h: Here) -> Option<Target> {
    let loc = &gs.location;
    let at = |label: String, p: (f64, f64)| target(label, surface_distance(h.pos, p, h.radius_m), surface_bearing(h.pos, p), h.heading);
    if let (Some(name), Some(p)) = (&loc.settlement, loc.settlement_pos) {
        return Some(at(name.clone(), p));
    }
    let body_id = body_id?;
    let on_body = |sa: u64, b: u32| sa == loc.system_address && b == body_id;
    let deposit = prefs
        .deposits
        .iter()
        .filter(|d| on_body(d.system_address, d.body_id))
        .map(|d| (surface_distance(h.pos, (d.lat, d.lon), h.radius_m), d))
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, d)) = deposit {
        let what = d.commodities.first().cloned().unwrap_or_else(|| "Mining".into());
        let label = match d.location {
            Some(n) => format!("{what} deposit · location {n}"),
            None => format!("{what} deposit"),
        };
        return Some(at(label, (d.lat, d.lon)));
    }
    gs.mining.sites.values().filter(|s| on_body(s.key.system_address, s.key.body_id)).filter_map(|s| s.landings.last().map(|p| (s.last, *p))).max_by_key(|(t, _)| t.0).map(|(_, p)| at("Your last landing here".into(), p))
}

fn read_sampling(gs: &GameState, here: Option<Here>) -> Option<Sampling> {
    let cur = gs.exobio.current.as_ref()?;
    let range = cur.colony_range_m;
    let near = here.filter(|_| !cur.points.is_empty()).and_then(|h| cur.nearest_sample_m(h.pos, h.radius_m).map(|d| (h, d)));
    let dial = near.map(|(h, _)| {
        let r = f64::from(range.unwrap_or(100));
        // Earlier samples in metres on the local tangent plane (x east, y north).
        let pts: Vec<(f64, f64)> = cur
            .points
            .iter()
            .map(|p| ((p.lon - h.pos.1).to_radians() * h.radius_m * h.pos.0.to_radians().cos(), (p.lat - h.pos.0).to_radians() * h.radius_m))
            .collect();
        let max_d = pts.iter().map(|(x, y)| x.hypot(*y) + r).fold(r * 1.5, f64::max);
        let (sin, cos) = h.heading.to_radians().sin_cos();
        let marks = pts
            .iter()
            .map(|&(x, y)| DialMark {
                x: hundredths((x * cos - y * sin) / max_d),
                y: hundredths((x * sin + y * cos) / max_d),
                kind: DialKind::Sample { range: (r / max_d * 100.0).round().clamp(1.0, 100.0) as u8, close: x.hypot(y) < r },
            })
            .collect();
        Dial { rings: 2, north: north(h.heading), marks }
    });
    Some(Sampling {
        species: cur.species.clone(),
        value: cur.value,
        stage: cur.stage,
        range_m: range,
        nearest_m: near.map(|(_, d)| d.round() as u32),
        far_enough: near.and_then(|(_, d)| range.map(|r| d >= f64::from(r))),
        dial,
    })
}

fn hundredths(v: f64) -> i16 {
    (v * 100.0).round().clamp(-150.0, 150.0) as i16
}

/// Where north is on a dial turned to `heading`: degrees clockwise from up.
fn north(heading: f64) -> i16 {
    turn(0.0, heading)
}

fn read_mining(gs: &GameState, prefs: &MiningPrefs) -> Option<Mining> {
    let v = crate::pages::mining::radar_view(gs, prefs)?;
    let nearest = |kind: MarkKind| v.marks.iter().filter(|m| m.kind == kind).min_by(|a, b| a.dist_m.total_cmp(&b.dist_m));
    let deposit = nearest(MarkKind::Deposit);
    let as_target = |m: &crate::pages::mining::Mark, label: &str| target(label.to_string(), m.dist_m, m.bearing, v.heading);
    let far = v.marks.iter().map(|m| m.dist_m).fold(0.0, f64::max);
    let max_d = (far * 1.15).max(250.0);
    let marks = v
        .marks
        .iter()
        .map(|m| {
            let a = (m.bearing - v.heading).to_radians();
            let kind = match m.kind {
                MarkKind::Deposit => DialKind::Deposit { nearest: deposit.is_some_and(|d| std::ptr::eq(d, m)) },
                MarkKind::Ship => DialKind::Ship,
                MarkKind::Landing => DialKind::Landing,
            };
            DialMark { x: hundredths(a.sin() * m.dist_m / max_d), y: hundredths(a.cos() * m.dist_m / max_d), kind }
        })
        .collect();
    Some(Mining {
        deposit: deposit.map(|d| as_target(d, &d.tag)),
        commodity: deposit.map(|d| d.tag.clone()).unwrap_or_default(),
        location: deposit.and_then(|d| d.location),
        ship: nearest(MarkKind::Ship).map(|m| as_target(m, "Ship")),
        landing: nearest(MarkKind::Landing).map(|m| as_target(m, "Landed")),
        dial: Dial { rings: 3, north: north(v.heading), marks },
    })
}

fn read_trade(gs: &GameState, src: &Sources<'_>) -> Option<Trade> {
    if src.pins.is_empty() {
        return None;
    }
    let loc = &gs.location;
    let listed = carriers_here(gs);
    let pin = src.pins.iter().find(|p| p.system.eq_ignore_ascii_case(&loc.system) || (p.carrier && presence(&listed, &p.station) == Presence::Here))?;
    let (name, callsign, presence) = if pin.carrier {
        let cs = pin.station.trim().to_ascii_uppercase();
        let signal = listed.iter().find(|(_, c)| *c == cs).and_then(|(s, _)| s.trim().rsplit_once(char::is_whitespace)).map(|(name, _)| name.trim().to_string()).filter(|s| !s.is_empty());
        (signal.unwrap_or_else(|| "Fleet carrier".into()), Some(cs), presence(&listed, &pin.station))
    } else {
        (pin.station.clone(), None, Presence::Here)
    };
    let tonnes = if pin.sell_here {
        Some(gs.inventory.cargo.count(&pin.symbol)).filter(|n| *n > 0)
    } else {
        let free = gs.fleet.current().map(|s| i64::from(s.cargo_capacity) - gs.inventory.cargo.total()).filter(|f| *f > 0);
        Some(free.map_or(pin.qty, |f| f.min(pin.qty))).filter(|n| *n > 0)
    };
    let then = src.route.and_then(|r| r.remaining().iter().find(|w| !w.system.eq_ignore_ascii_case(&loc.system)).map(|w| (w.system.clone(), w.jumps.max(1))));
    Some(Trade {
        name,
        callsign,
        presence,
        buy: !pin.sell_here,
        commodity: pin.name.clone(),
        tonnes,
        price: pin.price,
        age_secs: (src.now - pin.pinned_unix).max(0),
        then,
    })
}

fn read_docked(gs: &GameState, pins: &[Pin]) -> Option<Docked> {
    let loc = &gs.location;
    let st = loc.station.as_ref().filter(|_| loc.docked)?;
    Some(Docked {
        station: st.name.clone(),
        system: loc.system.clone(),
        market_id: st.market_id,
        exobio: if st.has_service("vistagenomics") { gs.exobio.unsold_value() } else { 0 },
        carto: if st.has_service("exploration") { gs.exploration.unsold_value() } else { 0 },
        orders: pins.iter().filter(|p| p.market_id == st.market_id).map(|p| (!p.sell_here, p.name.clone(), p.price)).take(2).collect(),
    })
}

fn read_carrier(gs: &GameState, now: i64) -> Option<CarrierJump> {
    let t = Timestamp(now);
    gs.carriers(t).into_iter().find_map(|(_, c)| match c.jump_timer(t)? {
        JumpTimer::Countdown { to, departs: Some(d), .. } => Some(CarrierJump { to, departs: d.0 }),
        _ => None,
    })
}

// ---------------------------------------------------------------------------------------------
// When the island opens

/// How long the island stays open after each kind of moment, in seconds.
const OPEN_ARRIVAL: f64 = 5.0;
const OPEN_SAMPLE: f64 = 3.0;
const OPEN_MINING: f64 = 4.0;
const OPEN_DOCKED: f64 = 6.0;
const OPEN_TRADE: f64 = 5.0;
const OPEN_ALERT: f64 = 4.0;
/// It opens by itself at most once every this many seconds.
const OPEN_GAP: f64 = 2.0;
/// Within this of a deposit, you've reached it.
const AT_DEPOSIT_M: u32 = 150;

/// The island's breathing: it opens for a moment's trigger (a sample taken, an arrival worth it, docking, an alert)
/// and settles back after a few seconds, except for moments that last (the galaxy map, the FSS, an approach).
#[derive(Debug, Default)]
pub struct Rhythm {
    seen: Seen,
    /// The island opens for this moment from the first time to the second.
    open: Option<(Moment, f64, f64)>,
    /// When it last opened by itself.
    last_open: Option<f64>,
    /// Alerts waiting their turn, oldest first.
    queue: VecDeque<Alert>,
    /// The alert showing, until then.
    showing: Option<(Alert, f64)>,
}

/// What opened the island last, so the same thing doesn't open it twice.
#[derive(Debug, Default)]
struct Seen {
    activity: Option<Activity>,
    arrival: Option<u64>,
    /// The species being sampled, its stage, and whether you were far enough.
    sample: Option<(String, u8, Option<bool>)>,
    at_deposit: bool,
    facing_ship: bool,
    docked: Option<u64>,
}

/// Alerts the island opens for: the important ones, except sampling progress (the Sampling moment says it).
fn worth_showing(a: &Alert) -> bool {
    match a.priority {
        Priority::Critical | Priority::High => true,
        Priority::Normal => a.kind != AlertKind::Exobiology,
        Priority::Low => false,
    }
}

impl Rhythm {
    /// An alert the app raised: the island opens for it when it's worth it, in turn.
    pub fn alert(&mut self, a: &Alert) {
        if worth_showing(a) {
            if self.queue.len() >= 3 {
                self.queue.pop_front();
            }
            self.queue.push_back(a.clone());
        }
    }

    /// The island for these inputs at `t` seconds (any steady clock). `keep_minimal`: an empty capsule instead of
    /// nothing when idle.
    pub fn view(&mut self, i: &Inputs, keep_minimal: bool, t: f64) -> IslandView {
        let c = &i.context;
        if !c.in_game() {
            self.queue.clear();
            self.showing = None;
            self.open = None;
            self.seen.activity = Some(c.activity);
            return quiet(keep_minimal);
        }
        self.watch(i, t);
        if self.showing.as_ref().is_some_and(|(_, until)| t >= *until) {
            self.showing = None;
        }
        if c.danger {
            self.queue.retain(|a| a.priority == Priority::Critical);
        }
        let may_open = self.last_open.is_none_or(|l| t - l >= OPEN_GAP);
        if self.showing.is_none() && may_open && let Some(a) = self.queue.pop_front() {
            self.showing = Some((a, t + OPEN_ALERT));
            self.last_open = Some(t);
        }
        let mut next = [self.showing.as_ref().map(|(_, until)| *until), self.open.map(|(_, from, until)| if from > t { from } else { until }).filter(|u| *u > t)].into_iter().flatten().fold(None, |a: Option<f64>, b| Some(a.map_or(b, |a| a.min(b))));
        if !self.queue.is_empty() {
            let gap = self.last_open.map_or(t, |l| l + OPEN_GAP);
            let due = self.showing.as_ref().map_or(gap, |(_, until)| until.max(gap));
            next = Some(next.map_or(due, |n| n.min(due)));
        }
        if c.danger {
            // Gone in danger, except a critical alert.
            return match &self.showing {
                Some((a, _)) if a.priority == Priority::Critical => expanded(alert_card(a), next),
                _ => IslandView { next_change: next, ..IslandView::default() },
            };
        }
        let card = main_card(i);
        if c.in_panel() {
            let size = if card.is_some() || keep_minimal { Size::Minimal } else { Size::Gone };
            return IslandView { size, next_change: next, ..IslandView::default() };
        }
        if let Some((a, _)) = &self.showing {
            return expanded(alert_card(a), next);
        }
        let Some(card) = card else { return IslandView { next_change: next, ..quiet(keep_minimal) } };
        let opened = self.open.is_some_and(|(m, from, until)| m == card.moment && (from..until).contains(&t));
        let side = i.carrier.as_ref().filter(|_| card.moment != Moment::Carrier).map(|cj| Side {
            icon: glyph::ANCHOR,
            tint: Tint::Indigo,
            label: "Carrier".into(),
            value: countdown(cj.departs - i.now),
        });
        if card.expanded.is_some() && (lasting(c, card.moment) || opened) {
            return expanded(card, next);
        }
        IslandView { moment: Some(card.moment), size: Size::Compact, compact: Some(card.compact), expanded: None, side, next_change: next }
    }

    /// Opens the island for the moments that just happened.
    fn watch(&mut self, i: &Inputs, t: f64) {
        let c = &i.context;
        let mut open: Option<(Moment, f64)> = None;
        if let Some(a) = i.arrival.as_ref().filter(|a| a.worth_it)
            && self.seen.arrival != Some(a.system_address)
        {
            self.seen.arrival = Some(a.system_address);
            open = Some((Moment::Arrival, OPEN_ARRIVAL));
        }
        match &i.sampling {
            Some(s) => {
                let was = self.seen.sample.replace((s.species.clone(), s.stage, s.far_enough));
                let sampled = was.as_ref().is_none_or(|(sp, st, _)| *sp != s.species || *st != s.stage);
                let now_far = s.far_enough == Some(true) && was.as_ref().is_some_and(|(sp, st, far)| *sp == s.species && *st == s.stage && *far != Some(true));
                if (sampled || now_far) && s.stage > 0 {
                    open = Some((Moment::Sampling, OPEN_SAMPLE));
                }
            }
            None => self.seen.sample = None,
        }
        if let Some(m) = i.mining.as_ref().filter(|_| c.activity == Activity::Srv(Vehicle::Rhino)) {
            let at = m.deposit.as_ref().is_some_and(|d| d.dist_m <= AT_DEPOSIT_M);
            if at && !self.seen.at_deposit {
                open = Some((Moment::Mining, OPEN_MINING));
            }
            self.seen.at_deposit = at;
            // Heading back: facing the ship from afar (with some slack before it counts again).
            let facing = m.ship.as_ref().is_some_and(|s| s.dist_m >= 500 && if self.seen.facing_ship { s.turn.abs() <= 45 } else { s.turn.abs() <= 15 });
            if facing && !self.seen.facing_ship {
                open = Some((Moment::Mining, OPEN_MINING));
            }
            self.seen.facing_ship = facing;
        }
        match i.docked.as_ref().filter(|_| c.activity == Activity::Docked) {
            Some(d) => {
                if self.seen.docked != Some(d.market_id) && d.has_news() {
                    open = Some((Moment::Docked, OPEN_DOCKED));
                }
                self.seen.docked = Some(d.market_id);
            }
            None if c.activity != Activity::Docked => self.seen.docked = None,
            None => {}
        }
        // Dropping out of supercruise where a pinned stop is.
        let dropped = self.seen.activity == Some(Activity::Supercruise) && c.activity == Activity::Space;
        if dropped && i.trade.is_some() {
            open = Some((Moment::Trade, OPEN_TRADE));
        }
        self.seen.activity = Some(c.activity);
        if let Some((moment, secs)) = open {
            // At most one opening every OPEN_GAP seconds: one that comes sooner opens when the gap is over.
            let from = self.last_open.map_or(t, |l| t.max(l + OPEN_GAP));
            self.open = Some((moment, from, from + secs));
            self.last_open = Some(from);
        }
    }
}

/// Moments that stay open while they last.
fn lasting(c: &Context, moment: Moment) -> bool {
    match moment {
        Moment::NextJump => matches!(c.activity, Activity::ChargingJump | Activity::Hyperspace) || c.focus == GuiFocus::GalaxyMap,
        Moment::Arrival => matches!(c.focus, GuiFocus::Fss | GuiFocus::SystemMap | GuiFocus::Orrery),
        Moment::Approach => matches!(c.activity, Activity::NearBody | Activity::Glide) && c.focus != GuiFocus::Saa,
        _ => false,
    }
}

fn quiet(keep_minimal: bool) -> IslandView {
    IslandView { size: if keep_minimal { Size::Minimal } else { Size::Gone }, ..IslandView::default() }
}

fn expanded(card: Card, next_change: Option<f64>) -> IslandView {
    IslandView { moment: Some(card.moment), size: Size::Expanded, compact: Some(card.compact), expanded: card.expanded, side: None, next_change }
}

// ---------------------------------------------------------------------------------------------
// The moments' words

struct Card {
    moment: Moment,
    compact: Compact,
    expanded: Option<Expanded>,
}

/// The moment for what the commander is doing.
fn main_card(i: &Inputs) -> Option<Card> {
    let c = &i.context;
    let jump = || i.jump.as_ref().map(|j| jump_card(j, c));
    let arrival = || i.arrival.as_ref().map(arrival_card);
    let worth = || i.arrival.as_ref().filter(|a| a.worth_it).map(arrival_card);
    let approach = || i.approach.as_ref().map(approach_card);
    let trade = || i.trade.as_ref().map(trade_card);
    let surface = || {
        i.sampling
            .as_ref()
            .map(sampling_card)
            .or_else(|| i.mining.as_ref().filter(|_| c.activity == Activity::Srv(Vehicle::Rhino)).map(mining_card))
            .or_else(approach)
    };
    let card = match (c.activity, c.focus) {
        (Activity::ChargingJump | Activity::Hyperspace, _) | (_, GuiFocus::GalaxyMap) => jump(),
        (_, GuiFocus::Fss | GuiFocus::SystemMap | GuiFocus::Orrery) => arrival(),
        (Activity::Docked, _) => i.docked.as_ref().filter(|d| d.has_news()).map(docked_card),
        (Activity::NearBody | Activity::Glide, _) => approach(),
        (Activity::Landed | Activity::Srv(_) | Activity::OnFootPlanet, _) => surface(),
        (Activity::Supercruise, _) => trade().or_else(worth).or_else(jump),
        (Activity::Space, _) => trade().or_else(worth),
        (Activity::NotInGame, _) => None,
    };
    card.or_else(|| i.carrier.as_ref().map(|cj| carrier_card(cj, i.now)))
}

fn text(text: impl Into<String>, style: Style, tint: Tint) -> Piece {
    Piece::Text(span(text, style, tint))
}

fn span(text: impl Into<String>, style: Style, tint: Tint) -> Span {
    Span { text: text.into(), style, tint }
}

fn icon(icon: &'static str, tint: Tint, size: u8) -> Glyph {
    Glyph::Icon { icon, tint, size }
}

fn plural(n: impl Into<u64>, one: &str, many: &str) -> String {
    let n = n.into();
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What the next star means for the jump, in a word, and its tint.
fn star_word(class: &str) -> (&'static str, Tint) {
    let c = class.split('_').next().unwrap_or_default();
    if class.is_empty() {
        ("Next jump", Tint::White)
    } else if is_scoopable(class) {
        ("Scoopable", Tint::Green)
    } else if c == "N" {
        ("Neutron star", Tint::Cyan)
    } else if c.starts_with('D') {
        ("White dwarf", Tint::Yellow)
    } else if c == "H" || c == "SupermassiveBlackHole" {
        ("Black hole", Tint::Yellow)
    } else {
        ("Not scoopable", Tint::Yellow)
    }
}

fn jump_card(j: &Jump, c: &Context) -> Card {
    let jumping = c.activity == Activity::Hyperspace;
    let (system, class) = match (&j.arriving, jumping) {
        (Some((s, cl)), true) => (s.as_str(), cl.as_str()),
        _ => (j.system.as_str(), j.star_class.as_str()),
    };
    let (word, tint) = star_word(class);
    let lead_glyph = if class.is_empty() { icon(glyph::NAVIGATION_ARROW, Tint::White, 20) } else { Glyph::Star { class: class.to_string(), size: 22 } };
    let trail = match j.jumps_left {
        Some(n) => vec![text(n.to_string(), Style::Value, Tint::White), text(if n == 1 { "jump left" } else { "jumps left" }, Style::Caption, Tint::Dim)],
        None => vec![text(system, Style::Value, Tint::White)],
    };
    let compact = Compact { lead: vec![Piece::Glyph(lead_glyph), text(word, Style::Label, tint)], trail };
    let top = if jumping {
        "Jumping to"
    } else if j.on_clipboard {
        "Next jump · on your clipboard"
    } else {
        "Next jump"
    };
    let right = j.jumps_left.map(|n| vec![vec![text(n.to_string(), Style::Figure, Tint::White)], vec![text(if n == 1 { "jump" } else { "jumps" }, Style::Caption, Tint::Dim)]]).unwrap_or_default();
    let lead = if class.is_empty() { icon(glyph::NAVIGATION_ARROW, Tint::White, 30) } else { Glyph::Star { class: class.to_string(), size: 48 } };
    let mut spread = Vec::new();
    if !class.is_empty() {
        let mark = if tint == Tint::Green { icon(glyph::CHECK_CIRCLE, Tint::Green, 16) } else { icon(glyph::WARNING, tint, 16) };
        spread.push(vec![Piece::Glyph(mark), text(word, Style::Label, tint)]);
    }
    if let Some(n) = j.neutron_in {
        spread.push(vec![text(if n <= 1 { "Neutron next".to_string() } else { format!("Neutron in {n}") }, Style::Label, Tint::Cyan)]);
    }
    if let Some(f) = j.fuel {
        spread.push(vec![Piece::Fuel(f), text(format!("{f} %"), Style::Label, fuel_tint(f))]);
    }
    let mut rows = vec![Row::Header { lead: Some(lead), top: span(top, Style::Caption, Tint::Dim), bottom: span(system, Style::Title, Tint::White), right }];
    if !spread.is_empty() {
        rows.push(Row::Spread(spread));
    }
    Card { moment: Moment::NextJump, compact, expanded: Some(Expanded { rows }) }
}

fn fuel_tint(pct: u8) -> Tint {
    match pct {
        0..25 => Tint::Red,
        25..50 => Tint::Yellow,
        _ => Tint::White,
    }
}

fn bodies_words(a: &Arrival) -> String {
    match a.total {
        Some(t) => format!("{} of {t} bodies", a.scanned),
        None => plural(a.scanned, "body", "bodies"),
    }
}

fn bio_words(n: u32) -> String {
    if n == 1 { "Bio on 1 body".into() } else { format!("Bio on {n} bodies") }
}

fn arrival_card(a: &Arrival) -> Card {
    let (word, tint) = if a.bio_bodies > 0 {
        (bio_words(a.bio_bodies), Tint::Mint)
    } else if let Some(h) = a.highlights.first() {
        (h.clone(), Tint::Cyan)
    } else if a.first_discovery {
        ("First discovery".into(), Tint::Gold)
    } else {
        (bodies_words(a), Tint::Dim)
    };
    let value = (a.value > 0).then(|| compact_credits(a.value));
    let compact = Compact {
        lead: vec![Piece::Glyph(icon(glyph::SPARKLE, Tint::Gold, 20)), text(word, Style::Label, tint)],
        trail: value.iter().map(|v| text(v.clone(), Style::Value, Tint::Gold)).collect(),
    };
    let top = format!("{} · {}", if a.first_discovery { "New system" } else { "Arrived" }, bodies_words(a));
    let right = value.map(|v| vec![vec![text(v, Style::Figure, Tint::Gold)], vec![text("if mapped", Style::Caption, Tint::Dim)]]).unwrap_or_default();
    let mut spread: Vec<Vec<Piece>> = a.highlights.iter().take(2).map(|h| vec![text(h.clone(), Style::Label, Tint::Cyan)]).collect();
    if a.bio_bodies > 0 {
        spread.push(vec![text(bio_words(a.bio_bodies), Style::Label, Tint::Mint)]);
    }
    if a.first_discovery {
        spread.push(vec![text("First discovery", Style::Label, Tint::Gold)]);
    }
    let mut rows = vec![Row::Header { lead: Some(icon(glyph::SPARKLE, Tint::Gold, 30)), top: span(top, Style::Caption, Tint::Dim), bottom: span(a.system.clone(), Style::Title, Tint::White), right }];
    if !spread.is_empty() {
        rows.push(Row::Spread(spread));
    }
    Card { moment: Moment::Arrival, compact, expanded: Some(Expanded { rows }) }
}

fn signals_words(n: u32) -> String {
    match n {
        0 => "No bio signals".into(),
        1 => "1 bio signal".into(),
        n => format!("{n} bio signals"),
    }
}

fn approach_card(a: &Approach) -> Card {
    let tint = if a.bio_signals > 0 { Tint::Mint } else { Tint::White };
    let compact = if a.mapping {
        let trail = if a.genera.is_empty() {
            text(compact_credits(a.mapped_value), Style::Value, Tint::Gold)
        } else {
            text(plural(a.genera.len() as u64, "genus", "genera"), Style::Value, Tint::Mint)
        };
        Compact { lead: vec![Piece::Glyph(icon(glyph::PLANET, tint, 20)), text("Mapping", Style::Label, Tint::Dim)], trail: vec![trail] }
    } else {
        let word_tint = if a.bio_signals > 0 { Tint::Mint } else { Tint::Dim };
        let trail = match a.conditions.first() {
            Some(g) => text(g.clone(), Style::Value, Tint::White),
            None => text(a.body.clone(), Style::Value, Tint::White),
        };
        Compact { lead: vec![Piece::Glyph(icon(glyph::PLANET, tint, 20)), text(signals_words(a.bio_signals), Style::Label, word_tint)], trail: vec![trail] }
    };
    let bottom = if a.conditions.is_empty() { signals_words(a.bio_signals) } else { a.conditions.join(" · ") };
    let right = if a.value > 0 {
        vec![vec![text(compact_credits(a.value), Style::Figure, Tint::Gold)], vec![text(if a.bonus { "×5 likely" } else { "likely" }, Style::Caption, Tint::Dim)]]
    } else {
        Vec::new()
    };
    let mut rows = vec![Row::Header { lead: Some(icon(glyph::PLANET, tint, 30)), top: span(a.body.clone(), Style::Title, Tint::White), bottom: span(bottom, Style::Caption, Tint::Dim), right }];
    if !a.genera.is_empty() {
        rows.push(Row::Chips(a.genera.clone()));
    }
    if let Some(t) = &a.target {
        rows.push(Row::Spread(vec![
            vec![Piece::Glyph(Glyph::Arrow { turn: t.turn, tint: Tint::Orange, size: 16 }), text(t.label.clone(), Style::Label, Tint::White)],
            vec![text(format!("{} · {:03}°", distance_text(f64::from(t.dist_m)), t.bearing), Style::Label, Tint::White)],
        ]));
    }
    Card { moment: Moment::Approach, compact, expanded: Some(Expanded { rows }) }
}

fn sampling_card(s: &Sampling) -> Card {
    let count = format!("{} of 3 samples", s.stage);
    let (status, tint) = match (s.far_enough, s.nearest_m, s.range_m) {
        (Some(true), _, _) => ("Far enough".to_string(), Tint::Green),
        (Some(false), Some(d), Some(r)) => (format!("{} m to go", r.saturating_sub(d).max(1)), Tint::Yellow),
        _ => (String::new(), Tint::Dim),
    };
    let trail = match (status.is_empty(), s.range_m) {
        (false, _) if tint == Tint::Green => vec![text(status.clone(), Style::Value, tint), Piece::Glyph(icon(glyph::CHECK_CIRCLE, Tint::Green, 16))],
        (false, _) => vec![text(status.clone(), Style::Value, tint)],
        (true, Some(r)) => vec![text(format!("Colony range {r} m"), Style::Caption, Tint::Dim)],
        (true, None) => Vec::new(),
    };
    let compact = Compact { lead: vec![Piece::Glyph(icon(glyph::LEAF, Tint::Mint, 20)), text(count.clone(), Style::Label, Tint::Dim)], trail };
    let mut top = vec![vec![Piece::Glyph(icon(glyph::LEAF, Tint::Mint, 16)), text(s.species.clone(), Style::Value, Tint::White)]];
    if let Some(v) = s.value {
        top.push(vec![text(compact_credits(v), Style::Value, Tint::Gold)]);
    }
    let mut rows = vec![Row::Spread(top)];
    match (&s.dial, s.nearest_m) {
        (Some(dial), Some(d)) => rows.push(Row::Dial {
            dial: dial.clone(),
            lines: vec![vec![text(status, Style::Hero, tint)], vec![text(format!("Nearest sample {d} m away"), Style::Caption, Tint::Dim)]],
            ring: Some((s.stage.min(3), 3)),
        }),
        _ => {
            let range = s.range_m.map(|r| format!("Colony range {r} m")).unwrap_or_default();
            rows.push(Row::Header { lead: None, top: span(range, Style::Caption, Tint::Dim), bottom: span(count, Style::Title, Tint::White), right: Vec::new() });
            rows.push(Row::Spread(vec![vec![text("Positions are recorded while Elite Nexus runs", Style::Caption, Tint::Dim)]]));
        }
    }
    Card { moment: Moment::Sampling, compact, expanded: Some(Expanded { rows }) }
}

fn mining_card(m: &Mining) -> Card {
    let (word, goal) = match &m.deposit {
        Some(d) => (m.commodity.clone(), Some(d)),
        None => ("Ship".to_string(), m.ship.as_ref()),
    };
    let trail = goal
        .map(|g| vec![Piece::Glyph(Glyph::Arrow { turn: g.turn, tint: Tint::White, size: 16 }), text(distance_text(f64::from(g.dist_m)), Style::Value, Tint::White)])
        .unwrap_or_default();
    let compact = Compact { lead: vec![Piece::Glyph(icon(glyph::DIAMOND, Tint::Orange, 20)), text(word.clone(), Style::Label, Tint::White)], trail };
    let mut lines = Vec::new();
    if let Some(g) = goal {
        let what = match (m.deposit.is_some(), m.location) {
            (true, Some(n)) => format!("{word} deposit · location {n}"),
            (true, None) => format!("{word} deposit"),
            (false, _) => "Your ship".to_string(),
        };
        lines.push(vec![text(what, Style::Value, Tint::White)]);
        lines.push(vec![text(distance_text(f64::from(g.dist_m)), Style::Hero, Tint::White)]);
        lines.push(vec![Piece::Glyph(Glyph::Arrow { turn: g.turn, tint: Tint::Orange, size: 14 }), text(format!("Bearing {:03}°", g.bearing), Style::Label, Tint::Orange)]);
    }
    let mut rows = vec![Row::Dial { dial: m.dial.clone(), lines, ring: None }];
    let mut spread = Vec::new();
    for (t, dot, name) in [(&m.ship, Glyph::Dot { tint: Tint::Cyan, hollow: false }, "Ship"), (&m.landing, Glyph::Dot { tint: Tint::Dim, hollow: true }, "Landed")] {
        if let Some(t) = t.as_ref().filter(|_| m.deposit.is_some() || name != "Ship") {
            spread.push(vec![Piece::Glyph(dot), text(name, Style::Label, Tint::White), text(format!("{} · {:03}°", distance_text(f64::from(t.dist_m)), t.bearing), Style::Caption, Tint::Dim)]);
        }
    }
    if !spread.is_empty() {
        rows.push(Row::Spread(spread));
    }
    Card { moment: Moment::Mining, compact, expanded: Some(Expanded { rows }) }
}

fn presence_words(p: Presence) -> (Glyph, &'static str, Tint) {
    match p {
        Presence::Here => (Glyph::Dot { tint: Tint::Green, hollow: false }, "In system", Tint::Green),
        Presence::NotHere(_) => (Glyph::Dot { tint: Tint::Yellow, hollow: false }, "Not here", Tint::Yellow),
        Presence::Unknown => (Glyph::Dot { tint: Tint::Dim, hollow: true }, "Not listed yet", Tint::Dim),
    }
}

/// "just now", "5 min ago", "2 h ago", "3 d ago".
fn ago(secs: i64) -> String {
    match secs.max(0) {
        0..60 => "just now".into(),
        s @ 60..3600 => format!("{} min ago", s / 60),
        s @ 3600..86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    }
}

fn trade_card(t: &Trade) -> Card {
    let (dot, word, tint) = presence_words(t.presence);
    let name = t.callsign.clone().unwrap_or_else(|| t.name.clone());
    let compact = Compact { lead: vec![Piece::Glyph(icon(glyph::PACKAGE, Tint::Blue, 20)), text(name, Style::Value, Tint::White)], trail: vec![Piece::Glyph(dot), text(word, Style::Label, tint)] };
    let top = match &t.callsign {
        Some(cs) => format!("{} · {cs}", t.name),
        None => t.name.clone(),
    };
    let what = match t.tonnes {
        Some(n) => format!("{} {n} t {}", if t.buy { "Buy" } else { "Sell" }, t.commodity),
        None => format!("{} {}", if t.buy { "Buy" } else { "Sell" }, t.commodity),
    };
    let right = match t.presence {
        Presence::Here => vec![vec![Piece::Glyph(icon(glyph::CHECK_CIRCLE, Tint::Green, 16)), text("Here", Style::Label, Tint::Green)]],
        p => vec![vec![text(presence_words(p).1, Style::Label, presence_words(p).2)]],
    };
    let mut spread = vec![vec![text(format!("{} Cr/t", group_digits(t.price)), Style::Label, Tint::White), text(format!("· {}", ago(t.age_secs)), Style::Caption, Tint::Dim)]];
    if let Some((system, jumps)) = &t.then {
        spread.push(vec![text(format!("Then {system}, {}", plural(*jumps, "jump", "jumps")), Style::Caption, Tint::Dim)]);
    }
    let rows = vec![Row::Header { lead: Some(icon(glyph::PACKAGE, Tint::Blue, 30)), top: span(top, Style::Caption, Tint::Dim), bottom: span(what, Style::Title, Tint::White), right }, Row::Spread(spread)];
    Card { moment: Moment::Trade, compact, expanded: Some(Expanded { rows }) }
}

fn docked_card(d: &Docked) -> Card {
    let total = d.exobio + d.carto;
    let compact = match d.orders.first() {
        Some((buy, name, _)) if total == 0 => Compact {
            lead: vec![Piece::Glyph(icon(glyph::BUILDINGS, Tint::White, 20)), text("Pinned here", Style::Label, Tint::Dim)],
            trail: vec![text(format!("{} {name}", if *buy { "Buy" } else { "Sell" }), Style::Value, Tint::Blue)],
        },
        _ => Compact { lead: vec![Piece::Glyph(icon(glyph::BUILDINGS, Tint::White, 20)), text("Sell here", Style::Label, Tint::Dim)], trail: vec![text(compact_credits(total), Style::Value, Tint::Gold)] },
    };
    let mut rows = vec![Row::Header {
        lead: Some(icon(glyph::BUILDINGS, Tint::White, 30)),
        top: span(format!("Docked · {}", d.system), Style::Caption, Tint::Dim),
        bottom: span(d.station.clone(), Style::Title, Tint::White),
        right: Vec::new(),
    }];
    let mut lines: Vec<(Glyph, String, String, Tint)> = Vec::new();
    if d.exobio > 0 {
        lines.push((icon(glyph::LEAF, Tint::Mint, 16), "Vista Genomics".into(), compact_credits(d.exobio), Tint::Gold));
    }
    if d.carto > 0 {
        lines.push((icon(glyph::SPARKLE, Tint::Cyan, 16), "Universal Cartographics".into(), compact_credits(d.carto), Tint::Gold));
    }
    for (buy, name, price) in &d.orders {
        lines.push((icon(glyph::PACKAGE, Tint::Blue, 16), format!("{} {name}", if *buy { "Buy" } else { "Sell" }), format!("{} Cr/t", group_digits(*price)), Tint::White));
    }
    for (g, label, value, tint) in lines.into_iter().take(3) {
        rows.push(Row::Spread(vec![vec![Piece::Glyph(g), text(label, Style::Label, Tint::White)], vec![text(value, Style::Value, tint)]]));
    }
    Card { moment: Moment::Docked, compact, expanded: Some(Expanded { rows }) }
}

/// A countdown: "7:42", or "1:05:00" from an hour.
pub fn countdown(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn carrier_card(cj: &CarrierJump, now: i64) -> Card {
    let compact = Compact {
        lead: vec![Piece::Glyph(icon(glyph::ANCHOR, Tint::Indigo, 20)), text(format!("Carrier → {}", cj.to), Style::Label, Tint::Dim)],
        trail: vec![text(countdown(cj.departs - now), Style::Value, Tint::Indigo)],
    };
    Card { moment: Moment::Carrier, compact, expanded: None }
}

fn alert_glyph(a: &Alert) -> Glyph {
    let (g, tint) = if a.priority == Priority::Critical {
        (glyph::WARNING, Tint::Red)
    } else {
        match a.kind {
            AlertKind::Exploration => (glyph::SPARKLE, Tint::Gold),
            AlertKind::Exobiology => (glyph::LEAF, Tint::Mint),
            AlertKind::Navigation => (glyph::NAVIGATION_ARROW, Tint::Cyan),
            AlertKind::Trade => (glyph::PACKAGE, Tint::Blue),
            AlertKind::Mining => (glyph::DIAMOND, Tint::Orange),
            AlertKind::Missions => (glyph::TARGET, Tint::White),
            AlertKind::Combat => (glyph::WARNING, Tint::Yellow),
            AlertKind::Carrier => (glyph::ANCHOR, Tint::Indigo),
            AlertKind::Colonisation => (glyph::BUILDINGS, Tint::White),
            AlertKind::Engineering => (glyph::WRENCH, Tint::White),
            AlertKind::Commander => (glyph::STAR, Tint::Gold),
            AlertKind::System => (glyph::INFO, Tint::White),
        }
    };
    icon(g, tint, 30)
}

fn alert_card(a: &Alert) -> Card {
    let detail = if a.detail.is_empty() { a.kind.label().to_string() } else { a.detail.clone() };
    let small = match alert_glyph(a) {
        Glyph::Icon { icon: g, tint, .. } => icon(g, tint, 20),
        other => other,
    };
    Card {
        moment: Moment::Alert,
        compact: Compact { lead: vec![Piece::Glyph(small), text(a.title.clone(), Style::Label, Tint::White)], trail: Vec::new() },
        expanded: Some(Expanded { rows: vec![Row::Header { lead: Some(alert_glyph(a)), top: span(a.title.clone(), Style::Title, Tint::White), bottom: span(detail, Style::Caption, Tint::Dim), right: Vec::new() }] }),
    }
}

// ---------------------------------------------------------------------------------------------
// Demo moments (`--overlay <moment>`)

/// The moments `--overlay` shows with sample data.
pub const DEMOS: &[&str] = &["jump", "hyperspace", "arrival", "approach", "sampling", "close", "mining", "trade", "docked", "carrier", "two", "alert", "quiet"];

/// A moment with sample data, compact and expanded: what `--overlay <name>` shows. None for an unknown name.
pub fn demo(name: &str) -> Option<(IslandView, IslandView)> {
    let in_game = |activity: Activity| Context { activity, ..Context::default() };
    let jump = Jump {
        system: "Pru Aescs NC-M d7-192".into(),
        star_class: "K".into(),
        jumps_left: Some(12),
        neutron_in: Some(3),
        fuel: Some(78),
        on_clipboard: true,
        arriving: Some(("Pru Aescs NC-M d7-192".into(), "K".into())),
    };
    let sampling = |far: bool| {
        let marks = vec![
            DialMark { x: -27, y: 36, kind: DialKind::Sample { range: 32, close: false } },
            DialMark { x: 38, y: 14, kind: DialKind::Sample { range: 32, close: !far } },
        ];
        Sampling {
            species: "Stratum Tectonicas".into(),
            value: Some(19_010_800),
            stage: 2,
            range_m: Some(100),
            nearest_m: Some(if far { 129 } else { 52 }),
            far_enough: Some(far),
            dial: Some(Dial { rings: 2, north: -40, marks }),
        }
    };
    let card_views = |card: Card, side: Option<Side>| {
        let compact = IslandView { moment: Some(card.moment), size: Size::Compact, compact: Some(card.compact.clone()), expanded: card.expanded.clone(), side, next_change: None };
        let expanded = IslandView { size: if card.expanded.is_some() { Size::Expanded } else { Size::Compact }, side: None, ..compact.clone() };
        (compact, expanded)
    };
    let now = 1_000_000;
    let carrier = CarrierJump { to: "Colonia".into(), departs: now + 7 * 60 + 42 };
    let card = match name {
        "jump" => jump_card(&jump, &in_game(Activity::Supercruise)),
        "hyperspace" => jump_card(&jump, &in_game(Activity::Hyperspace)),
        "arrival" => arrival_card(&Arrival {
            system_address: 1,
            system: "Brestla AB-C d1-23".into(),
            scanned: 19,
            total: Some(31),
            value: 18_600_000,
            bio_bodies: 3,
            first_discovery: true,
            highlights: vec!["Earth-like world".into()],
            worth_it: true,
        }),
        "approach" => approach_card(&Approach {
            body: "2 a".into(),
            conditions: vec!["0.31 g".into(), "thin ammonia".into(), "162 K".into()],
            bio_signals: 4,
            genera: ["Bacterium", "Stratum", "Osseus", "Fonticulua"].iter().enumerate().map(|(i, g)| Chip { text: (*g).into(), done: i == 0 }).collect(),
            value: 23_400_000,
            bonus: true,
            target: Some(Target { label: "Your last landing here".into(), dist_m: 12_400, bearing: 237, turn: 40 }),
            mapping: false,
            mapped_value: 1_200_000,
        }),
        "sampling" | "two" => sampling_card(&sampling(true)),
        "close" => sampling_card(&sampling(false)),
        "mining" => mining_card(&Mining {
            deposit: Some(Target { label: "Bauxite".into(), dist_m: 1_200, bearing: 237, turn: 30 }),
            commodity: "Bauxite".into(),
            location: Some(4),
            ship: Some(Target { label: "Ship".into(), dist_m: 3_400, bearing: 84, turn: -153 }),
            landing: Some(Target { label: "Landed".into(), dist_m: 2_100, bearing: 192, turn: -45 }),
            dial: Dial {
                rings: 3,
                north: 153,
                marks: vec![
                    DialMark { x: 16, y: 31, kind: DialKind::Deposit { nearest: true } },
                    DialMark { x: -57, y: -18, kind: DialKind::Deposit { nearest: false } },
                    DialMark { x: -65, y: -45, kind: DialKind::Ship },
                    DialMark { x: -14, y: 53, kind: DialKind::Landing },
                ],
            },
        }),
        "trade" => trade_card(&Trade {
            name: "VOYAGER'S REST".into(),
            callsign: Some("Q2K-71N".into()),
            presence: Presence::Here,
            buy: true,
            commodity: "Tritium".into(),
            tonnes: Some(720),
            price: 49_015,
            age_secs: 2 * 3600,
            then: Some(("Shinrarta Dezhra".into(), 3)),
        }),
        "docked" => docked_card(&Docked {
            station: "Jameson Memorial".into(),
            system: "Shinrarta Dezhra".into(),
            market_id: 1,
            exobio: 87_200_000,
            carto: 14_600_000,
            orders: Vec::new(),
        }),
        "carrier" => carrier_card(&carrier, now),
        "alert" => alert_card(&Alert::new(Timestamp(now), AlertKind::Exploration, Priority::High, "Earth-like world").detail("A 1 · first discovery · worth mapping")),
        "quiet" => {
            let minimal = IslandView { size: Size::Minimal, ..IslandView::default() };
            return Some((minimal.clone(), minimal));
        }
        _ => return None,
    };
    let side = (name == "two").then(|| Side { icon: glyph::ANCHOR, tint: Tint::Indigo, label: "Carrier".into(), value: countdown(carrier.departs - now) });
    Some(card_views(card, side))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(activity: Activity) -> Context {
        Context { activity, ..Context::default() }
    }

    fn words(pieces: &[Piece]) -> String {
        pieces
            .iter()
            .filter_map(|p| match p {
                Piece::Text(s) => Some(s.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn compact_words(v: &IslandView) -> (String, String) {
        let c = v.compact.as_ref().expect("compact content");
        (words(&c.lead), words(&c.trail))
    }

    fn jump() -> Jump {
        Jump { system: "Beta".into(), star_class: "K".into(), jumps_left: Some(12), neutron_in: Some(3), fuel: Some(78), on_clipboard: true, arriving: Some(("Beta".into(), "K".into())) }
    }

    fn sampling(stage: u8, nearest: u32) -> Sampling {
        Sampling { species: "Stratum Tectonicas".into(), value: Some(19_010_800), stage, range_m: Some(100), nearest_m: Some(nearest), far_enough: Some(nearest >= 100), dial: Some(Dial { rings: 2, north: 0, marks: Vec::new() }) }
    }

    #[test]
    fn not_in_game_is_gone_or_minimal() {
        let mut r = Rhythm::default();
        let i = Inputs { jump: Some(jump()), ..Inputs::default() };
        assert_eq!(r.view(&i, false, 0.0).size, Size::Gone);
        assert_eq!(r.view(&i, true, 0.0).size, Size::Minimal, "kept as a small capsule when the setting says so");
    }

    #[test]
    fn next_jump_compact_in_supercruise_expanded_while_charging_and_in_the_galaxy_map() {
        let mut r = Rhythm::default();
        let mut i = Inputs { context: ctx(Activity::Supercruise), jump: Some(jump()), ..Inputs::default() };
        let v = r.view(&i, false, 0.0);
        assert_eq!((v.moment, v.size), (Some(Moment::NextJump), Size::Compact));
        assert_eq!(compact_words(&v), ("Scoopable".into(), "12 jumps left".into()), "the compact island says what it means in words");
        i.context.activity = Activity::ChargingJump;
        assert_eq!(r.view(&i, false, 1.0).size, Size::Expanded);
        i.context.activity = Activity::Hyperspace;
        let v = r.view(&i, false, 2.0);
        let Some(Row::Header { top, bottom, .. }) = v.expanded.as_ref().and_then(|e| e.rows.first()) else { panic!("a header") };
        assert_eq!((top.text.as_str(), bottom.text.as_str()), ("Jumping to", "Beta"), "in hyperspace: the star you're arriving at");
        i.context = Context { activity: Activity::Supercruise, focus: GuiFocus::GalaxyMap, ..Context::default() };
        let v = r.view(&i, false, 3.0);
        assert_eq!(v.size, Size::Expanded, "the galaxy map lasts");
        let Some(Row::Header { top, .. }) = v.expanded.as_ref().and_then(|e| e.rows.first()) else { panic!("a header") };
        assert_eq!(top.text, "Next jump · on your clipboard");
    }

    #[test]
    fn star_words() {
        assert_eq!(star_word("K"), ("Scoopable", Tint::Green));
        assert_eq!(star_word("K_OrangeGiant"), ("Scoopable", Tint::Green));
        assert_eq!(star_word("N"), ("Neutron star", Tint::Cyan));
        assert_eq!(star_word("DA"), ("White dwarf", Tint::Yellow));
        assert_eq!(star_word("T"), ("Not scoopable", Tint::Yellow));
        assert_eq!(StarTone::of("K_OrangeGiant"), StarTone::Orange);
        assert_eq!(StarTone::of("TTS"), StarTone::Orange);
        assert_eq!(StarTone::of("T"), StarTone::Brown);
        assert_eq!(StarTone::of("DAB"), StarTone::Dwarf);
        assert_eq!(StarTone::of("WC"), StarTone::Blue);
    }

    #[test]
    fn arrival_opens_for_five_seconds_then_settles_and_the_fss_keeps_it_open() {
        let mut r = Rhythm::default();
        let arrival = Arrival {
            system_address: 7,
            system: "Brestla AB-C d1-23".into(),
            scanned: 19,
            total: Some(31),
            value: 18_600_000,
            bio_bodies: 3,
            first_discovery: false,
            highlights: Vec::new(),
            worth_it: true,
        };
        let mut i = Inputs { context: ctx(Activity::Supercruise), arrival: Some(arrival), ..Inputs::default() };
        let v = r.view(&i, false, 10.0);
        assert_eq!((v.moment, v.size, v.next_change), (Some(Moment::Arrival), Size::Expanded, Some(15.0)));
        assert_eq!(r.view(&i, false, 14.9).size, Size::Expanded);
        let v = r.view(&i, false, 15.0);
        assert_eq!(v.size, Size::Compact, "settles back after five seconds");
        assert_eq!(compact_words(&v), ("Bio on 3 bodies".into(), "18.6 M".into()));
        assert_eq!(r.view(&i, false, 60.0).size, Size::Compact, "once per system");
        i.context.focus = GuiFocus::Fss;
        assert_eq!(r.view(&i, false, 61.0).size, Size::Expanded, "the FSS lasts");
    }

    #[test]
    fn approach_while_near_and_compact_on_the_surface() {
        let mut r = Rhythm::default();
        let approach = Approach {
            body: "2 a".into(),
            conditions: vec!["0.31 g".into(), "thin ammonia".into()],
            bio_signals: 4,
            genera: vec![Chip { text: "Bacterium".into(), done: true }],
            value: 23_400_000,
            bonus: true,
            target: None,
            mapping: false,
            mapped_value: 0,
        };
        let mut i = Inputs { context: ctx(Activity::NearBody), approach: Some(approach), ..Inputs::default() };
        assert_eq!(r.view(&i, false, 0.0).size, Size::Expanded);
        i.context.activity = Activity::Glide;
        assert_eq!(r.view(&i, false, 1.0).size, Size::Expanded);
        i.context.activity = Activity::Landed;
        let v = r.view(&i, false, 2.0);
        assert_eq!((v.moment, v.size), (Some(Moment::Approach), Size::Compact));
        assert_eq!(compact_words(&v), ("4 bio signals".into(), "0.31 g".into()));
        i.context = Context { activity: Activity::NearBody, focus: GuiFocus::Saa, ..Context::default() };
        // As `Inputs::read` has it: with the surface scanner's view open, the body is being mapped.
        if let Some(a) = i.approach.as_mut() {
            a.mapping = true;
        }
        let v = r.view(&i, false, 3.0);
        assert_eq!((v.size, compact_words(&v)), (Size::Compact, ("Mapping".into(), "1 genus".into())), "mapping the body: compact, its genera");
    }

    #[test]
    fn sampling_says_how_far_and_opens_on_a_sample_and_when_far_enough() {
        let mut r = Rhythm::default();
        let mut i = Inputs { context: ctx(Activity::OnFootPlanet), sampling: Some(sampling(2, 52)), ..Inputs::default() };
        let v = r.view(&i, false, 0.0);
        assert_eq!(v.size, Size::Expanded, "a sample was taken");
        assert_eq!(r.view(&i, false, 3.0).size, Size::Compact);
        let v = r.view(&i, false, 3.5);
        assert_eq!(compact_words(&v), ("2 of 3 samples".into(), "48 m to go".into()));
        i.sampling = Some(sampling(2, 129));
        let v = r.view(&i, false, 10.0);
        assert_eq!(v.size, Size::Expanded, "far enough opens it");
        assert_eq!(compact_words(&v), ("2 of 3 samples".into(), "Far enough".into()));
        assert_eq!(r.view(&i, false, 13.0).size, Size::Compact);
        i.sampling = Some(sampling(3, 0));
        assert_eq!(r.view(&i, false, 14.0).size, Size::Expanded, "the third sample");
    }

    #[test]
    fn rhino_mining_opens_at_a_deposit() {
        let mut r = Rhythm::default();
        let mining = |d: u32| Mining {
            deposit: Some(Target { label: "Bauxite".into(), dist_m: d, bearing: 237, turn: 30 }),
            commodity: "Bauxite".into(),
            location: Some(4),
            ship: None,
            landing: None,
            dial: Dial { rings: 3, north: 0, marks: Vec::new() },
        };
        let mut i = Inputs { context: ctx(Activity::Srv(Vehicle::Rhino)), mining: Some(mining(1_200)), ..Inputs::default() };
        let v = r.view(&i, false, 0.0);
        assert_eq!((v.moment, v.size), (Some(Moment::Mining), Size::Compact));
        assert_eq!(compact_words(&v), ("Bauxite".into(), "1.2 km".into()));
        i.mining = Some(mining(120));
        assert_eq!(r.view(&i, false, 30.0).size, Size::Expanded, "reached the deposit");
        assert_eq!(r.view(&i, false, 35.0).size, Size::Compact);
        i.context.activity = Activity::Srv(Vehicle::Scarab);
        assert_eq!(r.view(&i, false, 36.0).moment, None, "deposits are the Rhino's");
    }

    #[test]
    fn docked_opens_for_six_seconds_then_stays_compact() {
        let mut r = Rhythm::default();
        let docked = Docked { station: "Jameson Memorial".into(), system: "Shinrarta Dezhra".into(), market_id: 9, exobio: 87_200_000, carto: 14_600_000, orders: Vec::new() };
        let i = Inputs { context: ctx(Activity::Docked), docked: Some(docked), ..Inputs::default() };
        assert_eq!(r.view(&i, false, 0.0).size, Size::Expanded);
        let v = r.view(&i, false, 6.0);
        assert_eq!((v.size, compact_words(&v)), (Size::Compact, ("Sell here".into(), "101.8 M".into())));
    }

    #[test]
    fn trade_stop_in_supercruise_opens_when_dropping_out() {
        let mut r = Rhythm::default();
        let trade = Trade {
            name: "VOYAGER'S REST".into(),
            callsign: Some("Q2K-71N".into()),
            presence: Presence::Here,
            buy: true,
            commodity: "Tritium".into(),
            tonnes: Some(720),
            price: 49_015,
            age_secs: 7200,
            then: None,
        };
        let mut i = Inputs { context: ctx(Activity::Supercruise), trade: Some(trade), ..Inputs::default() };
        let v = r.view(&i, false, 0.0);
        assert_eq!((v.moment, v.size), (Some(Moment::Trade), Size::Compact));
        assert_eq!(compact_words(&v), ("Q2K-71N".into(), "In system".into()));
        i.context.activity = Activity::Space;
        assert_eq!(r.view(&i, false, 10.0).size, Size::Expanded, "dropped out near it");
        assert_eq!(r.view(&i, false, 15.0).size, Size::Compact);
    }

    #[test]
    fn panels_are_minimal_and_danger_shows_only_critical_alerts() {
        let mut r = Rhythm::default();
        let mut i = Inputs { context: Context { activity: Activity::Supercruise, focus: GuiFocus::InternalPanel, ..Context::default() }, jump: Some(jump()), ..Inputs::default() };
        assert_eq!(r.view(&i, false, 0.0).size, Size::Minimal);
        i.context = Context { activity: Activity::Supercruise, danger: true, ..Context::default() };
        assert_eq!(r.view(&i, false, 1.0).size, Size::Gone, "gone in danger");
        r.alert(&Alert::new(Timestamp(0), AlertKind::Combat, Priority::High, "Interdiction"));
        assert_eq!(r.view(&i, false, 2.0).size, Size::Gone, "only critical alerts in danger");
        r.alert(&Alert::new(Timestamp(0), AlertKind::Combat, Priority::Critical, "Hull critical"));
        let v = r.view(&i, false, 3.0);
        assert_eq!((v.moment, v.size), (Some(Moment::Alert), Size::Expanded));
    }

    #[test]
    fn alerts_open_for_four_seconds_one_at_a_time() {
        let mut r = Rhythm::default();
        let i = Inputs { context: ctx(Activity::Supercruise), jump: Some(jump()), ..Inputs::default() };
        assert_eq!(r.view(&i, false, 0.0).moment, Some(Moment::NextJump));
        r.alert(&Alert::new(Timestamp(0), AlertKind::Exploration, Priority::High, "Earth-like world"));
        r.alert(&Alert::new(Timestamp(0), AlertKind::Mining, Priority::High, "Motherlode!"));
        r.alert(&Alert::new(Timestamp(0), AlertKind::Navigation, Priority::Low, "Arrived in Beta"));
        let v = r.view(&i, false, 1.0);
        assert_eq!((v.moment, v.size, v.next_change), (Some(Moment::Alert), Size::Expanded, Some(5.0)));
        let v = r.view(&i, false, 5.0);
        let Some(Row::Header { top, .. }) = v.expanded.as_ref().and_then(|e| e.rows.first()) else { panic!("a header") };
        assert_eq!(top.text, "Motherlode!", "the next alert in turn");
        let v = r.view(&i, false, 9.0);
        assert_eq!(v.moment, Some(Moment::NextJump), "back to what it was; low-priority alerts don't open it");
        assert!(r.queue.is_empty() && r.showing.is_none(), "nothing waiting");
    }

    #[test]
    fn a_second_thing_gets_a_side_capsule() {
        let mut r = Rhythm::default();
        let now = 1_000;
        let i = Inputs { context: ctx(Activity::OnFootPlanet), now, sampling: Some(sampling(2, 129)), carrier: Some(CarrierJump { to: "Colonia".into(), departs: now + 462 }), ..Inputs::default() };
        r.view(&i, false, 0.0);
        let v = r.view(&i, false, 5.0);
        assert_eq!((v.moment, v.size), (Some(Moment::Sampling), Size::Compact));
        let side = v.side.expect("the carrier beside it");
        assert_eq!((side.label.as_str(), side.value.as_str()), ("Carrier", "7:42"));
        let alone = Inputs { context: ctx(Activity::Supercruise), now, carrier: Some(CarrierJump { to: "Colonia".into(), departs: now + 462 }), ..Inputs::default() };
        let v = r.view(&alone, false, 6.0);
        assert_eq!((v.moment, v.side.is_none()), (Some(Moment::Carrier), true));
        assert_eq!(compact_words(&v), ("Carrier → Colonia".into(), "7:42".into()));
    }

    #[test]
    fn expansions_are_at_least_two_seconds_apart() {
        let mut r = Rhythm::default();
        let docked = Docked { station: "A".into(), system: "S".into(), market_id: 1, exobio: 1, carto: 0, orders: Vec::new() };
        let mut i = Inputs { context: ctx(Activity::Docked), docked: Some(docked), ..Inputs::default() };
        assert_eq!(r.view(&i, false, 0.0).size, Size::Expanded);
        // A sample right after docking can't happen, but any second trigger within two seconds waits its turn.
        i.context.activity = Activity::OnFootPlanet;
        i.sampling = Some(sampling(1, 0));
        let v = r.view(&i, false, 1.0);
        assert_eq!((v.size, v.next_change), (Size::Compact, Some(2.0)));
        assert_eq!(r.view(&i, false, 2.0).size, Size::Expanded, "two seconds after the first");
    }

    #[test]
    fn words_and_countdowns() {
        assert_eq!(countdown(462), "7:42");
        assert_eq!(countdown(3_900), "1:05:00");
        assert_eq!(countdown(-5), "0:00");
        assert_eq!(ago(30), "just now");
        assert_eq!(ago(7_200), "2 h ago");
        assert_eq!(turn(10.0, 350.0), 20);
        assert_eq!(turn(350.0, 10.0), -20);
        assert_eq!(north(90.0), -90, "facing east, north is to the left");
        for name in DEMOS {
            assert!(demo(name).is_some(), "{name}");
        }
        assert!(demo("nonsense").is_none());
    }

    #[test]
    fn reads_the_route_ahead() {
        let stops = [("A", "K", 1, false), ("B", "N", 1, true), ("C", "M", 2, false)];
        let (next, jumps, neutron) = summarise(stops.iter().copied());
        assert_eq!((next, jumps, neutron), (Some(("A", "K")), 4, Some(2)));
    }
}
