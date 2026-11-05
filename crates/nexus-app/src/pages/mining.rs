//! Mining: the live session and earlier ones. For Rhino surface mining, the planetary mining locations worked (with
//! the number to pick in the game's navigation panel to go back) and a radar of the deposits around you; and on its
//! own tab, where to mine next (where_to_mine.rs).

use crate::pages::galaxy_map::MapAction;
use crate::pages::navigation::PlannedRoute;
use crate::pages::where_to_mine::{self, WhereUi};
use crate::theme::{display, pal, size};
use crate::widgets::{self, GAP, badge, card, card_tinted, duration, empty, footnote, th};
use egui::{Align, Align2, Grid, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use egui_phosphor::regular as icon;
use nexus_journal::Timestamp;
use nexus_model::context::Vehicle;
use nexus_model::location::{surface_bearing, surface_distance};
use nexus_model::mining::{MiningSession, SPOT_RADIUS_M, SiteKey, Spot, SurfaceSite, add_point};
use nexus_model::GameState;
use serde::{Deserialize, Serialize};

/// What the Mining page keeps between runs (settings.json).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MiningPrefs {
    /// Deposits the Rhino refined at. The journal records no positions, so they're taken live and kept here.
    pub deposits: Vec<Deposit>,
    /// The commander's notes on mining locations.
    pub notes: Vec<SiteNote>,
    /// Surface mining sites: only those that gave this commodity ("" = all).
    pub commodity: String,
    /// Rhinos dropped from a hovering ship, with the mining location targeted then. The journal names no location
    /// for those runs, so they're taken live, kept here and given back each time the journal is read.
    pub drops: Vec<HoverDrop>,
}

/// A Rhino dropped from a hovering ship: its launch's journal time and the planetary mining location targeted then.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HoverDrop {
    pub at: Timestamp,
    pub system_address: u64,
    pub body_id: u32,
    pub location: u32,
}

/// The drops to give the model before it reads the journal ([`crate::core::Core::start`]).
pub fn drops(prefs: &MiningPrefs) -> crate::core::Drops {
    prefs.drops.iter().map(|d| (d.at, key(d.system_address, d.body_id, Some(d.location)))).collect()
}

/// A deposit: where the Rhino refined on a body, and what came out there.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Deposit {
    pub system_address: u64,
    pub body_id: u32,
    /// The mining location's number (None: the game named none).
    pub location: Option<u32>,
    pub lat: f64,
    pub lon: f64,
    pub commodities: Vec<String>,
    pub found: Timestamp,
    /// Its number on the body, in the order the deposits there were found (1, 2, 3…), shown beside it on the radar.
    /// 0: not numbered yet (kept before deposits had numbers).
    pub number: u32,
    /// The commander's note: how many rigs fit, say.
    pub note: String,
    /// Where the Rhino refined at it, at least [`nexus_model::mining::POINT_GAP_M`] apart: the ground its outline is
    /// drawn round. Empty for deposits kept before outlines.
    pub points: Vec<(f64, f64)>,
}

impl Deposit {
    fn key(&self) -> SiteKey {
        key(self.system_address, self.body_id, self.location)
    }
}

fn key(system_address: u64, body_id: u32, location: Option<u32>) -> SiteKey {
    SiteKey { system_address, body_id, location }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteNote {
    pub system_address: u64,
    pub body_id: u32,
    pub location: Option<u32>,
    pub text: String,
}

impl SiteNote {
    fn key(&self) -> SiteKey {
        key(self.system_address, self.body_id, self.location)
    }
}

impl MiningPrefs {
    fn note(&self, k: SiteKey) -> &str {
        self.notes.iter().find(|n| n.key() == k).map_or("", |n| n.text.as_str())
    }

    /// An empty note is no note.
    fn set_note(&mut self, k: SiteKey, text: String) {
        let keep = !text.trim().is_empty();
        match self.notes.iter().position(|n| n.key() == k) {
            Some(i) if keep => self.notes[i].text = text,
            Some(i) => {
                self.notes.remove(i);
            }
            None if keep => self.notes.push(SiteNote { system_address: k.system_address, body_id: k.body_id, location: k.location, text }),
            None => {}
        }
    }
}

/// Copies the deposits and hover drops recorded this run (live only) into `prefs` and numbers new deposits; true when
/// anything changed. A deposit is matched by its place on the body, so it keeps its number and note on later runs,
/// and one first kept under "location not named" takes the location's number when a later landing names it.
pub fn remember(gs: &GameState, prefs: &mut MiningPrefs) -> bool {
    let mut changed = false;
    for (&at, k) in &gs.mining.drops {
        if let Some(location) = k.location
            && !prefs.drops.iter().any(|d| d.at == at)
        {
            prefs.drops.push(HoverDrop { at, system_address: k.system_address, body_id: k.body_id, location });
            changed = true;
        }
    }
    for site in gs.mining.sites.values() {
        for spot in &site.spots {
            let k = site.key;
            match prefs.deposits.iter().position(|d| d.system_address == k.system_address && d.body_id == k.body_id && same_place(d, spot, site.radius_m)) {
                Some(i) => {
                    let d = &mut prefs.deposits[i];
                    if d.location.is_none() && k.location.is_some() {
                        d.location = k.location;
                        changed = true;
                    }
                    for c in spot.refined.keys() {
                        if !d.commodities.contains(c) {
                            d.commodities.push(c.clone());
                            changed = true;
                        }
                    }
                    if let Some(r) = site.radius_m {
                        for p in &spot.points {
                            changed |= add_point(&mut d.points, *p, r);
                        }
                    }
                }
                None => {
                    prefs.deposits.push(Deposit {
                        system_address: site.key.system_address,
                        body_id: site.key.body_id,
                        location: site.key.location,
                        lat: spot.lat,
                        lon: spot.lon,
                        commodities: spot.refined.keys().cloned().collect(),
                        found: spot.found,
                        points: spot.points.clone(),
                        ..Default::default()
                    });
                    changed = true;
                }
            }
        }
    }
    number_deposits(&mut prefs.deposits) || changed
}

/// Numbers the deposits that have no number yet, on each body in the order they were found, after the highest
/// number there: new ones, and those kept before deposits were numbered. True when any got one.
fn number_deposits(deposits: &mut [Deposit]) -> bool {
    let mut new: Vec<usize> = (0..deposits.len()).filter(|&i| deposits[i].number == 0).collect();
    new.sort_by_key(|&i| (deposits[i].found, i));
    for &i in &new {
        let body = (deposits[i].system_address, deposits[i].body_id);
        let top = deposits.iter().filter(|d| (d.system_address, d.body_id) == body).map(|d| d.number).max().unwrap_or(0);
        deposits[i].number = top + 1;
    }
    !new.is_empty()
}

fn same_place(d: &Deposit, spot: &Spot, radius_m: Option<f64>) -> bool {
    match radius_m {
        Some(r) => surface_distance((d.lat, d.lon), (spot.lat, spot.lon), r) <= SPOT_RADIUS_M,
        None => (d.lat - spot.lat).abs() < 1e-9 && (d.lon - spot.lon).abs() < 1e-9,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Sites,
    Where,
}

/// State of the Mining page while the app runs.
#[derive(Default)]
pub struct MiningUi {
    pub tab: Tab,
    pub find: WhereUi,
}

impl MiningUi {
    /// `where` (or `where-route`, or `where:Monazite`) opens Where to mine; `autorun` starts its search on the first frame
    /// (screenshots).
    pub fn new(tab: Option<&str>, autorun: bool) -> Self {
        let find = WhereUi::new(tab, autorun);
        let tab = if tab.is_some_and(|t| matches!(t, "where" | "where-route") || t.starts_with("where:")) { Tab::Where } else { Tab::Sites };
        MiningUi { tab, find }
    }
}

/// Returns whether `prefs` changed (to be saved), a system to plot a route to, and a route to follow. `route_end` is
/// where the active route ends, Where to mine's destination when none is typed.
pub fn show(
    ui: &mut Ui,
    gs: &GameState,
    prefs: &mut MiningPrefs,
    st: &mut MiningUi,
    spansh: &nexus_spansh::Spansh,
    rt: &tokio::runtime::Handle,
    route_end: Option<&str>,
) -> (bool, Option<MapAction>, Option<PlannedRoute>) {
    if let Some(t) = widgets::tabs(ui, st.tab, &[(Tab::Sites, "Sessions and sites"), (Tab::Where, "Where to mine")]) {
        st.tab = t;
    }
    ui.add_space(GAP);
    match st.tab {
        Tab::Sites => {
            let (changed, action) = sessions_and_sites(ui, gs, prefs);
            (changed, action, None)
        }
        Tab::Where => match where_to_mine::show(ui, gs, &mut st.find, spansh, rt, route_end) {
            Some(where_to_mine::Action::PlotTo(s)) => (false, Some(MapAction::PlotTo(s)), None),
            Some(where_to_mine::Action::Follow(r)) => (false, None, Some(r)),
            None => (false, None, None),
        },
    }
}

/// The live session, the surface radar, the sites the Rhino worked and earlier sessions.
fn sessions_and_sites(ui: &mut Ui, gs: &GameState, prefs: &mut MiningPrefs) -> (bool, Option<MapAction>) {
    let mut changed = false;
    let mut action = None;
    match &gs.mining.session {
        Some(s) => session(ui, gs, s, true),
        None => empty(
            ui,
            icon::DIAMONDS_FOUR,
            "Not mining right now",
            "Fire a prospector limpet, or refine with the Rhino on a planet, and a live session starts: tonnes/hour, prospect quality, motherlodes.",
        ),
    }
    if let Some(v) = radar_view(gs, prefs) {
        ui.add_space(10.0);
        changed |= card_tinted(ui, icon::CROSSHAIR, "Surface radar", pal::ACCENT, |ui| surface_radar(ui, &v, prefs));
    } else if in_rhino(gs) {
        // Before the first deposit: how to place the rigs.
        ui.add_space(10.0);
        card_tinted(ui, icon::CROSSHAIR, "Placing rigs", pal::ACCENT, |ui| rig_guide(ui, false));
    }
    if !gs.mining.sites.is_empty() {
        ui.add_space(10.0);
        ui.label(RichText::new("SURFACE MINING SITES").font(display(14.0)).color(pal::ACCENT));
        changed |= sites(ui, gs, prefs, &mut action);
    }
    if !gs.mining.history.is_empty() {
        ui.add_space(10.0);
        ui.label(RichText::new("PREVIOUS SESSIONS").font(display(14.0)).color(pal::ACCENT));
        for s in gs.mining.history.iter().rev().take(10) {
            session(ui, gs, s, false);
            ui.add_space(6.0);
        }
    }
    (changed, action)
}

fn session(ui: &mut Ui, gs: &GameState, s: &MiningSession, live: bool) {
    let title = format!("{}{}", if live { "Live session — " } else { "" }, s.body.clone().unwrap_or_else(|| s.system.clone()));
    card(ui, &title, |ui| {
        // Rhino runs: where exactly, as the game's navigation panel names it.
        for site in s.sites.iter().filter_map(|k| gs.mining.sites.get(k)) {
            let r = ui.label(RichText::new(format!("{} {} · {}", icon::MAP_PIN, site.system, site_title(site))).color(pal::ACCENT_SOFT));
            if site.key.location.is_none() {
                r.on_hover_text(NOT_NAMED);
            }
        }
        ui.horizontal(|ui| {
            crate::widgets::stat(ui, "refined", format!("{} t", s.total_refined()), pal::ACCENT);
            ui.add_space(16.0);
            crate::widgets::stat(ui, "per hour", format!("{:.1} t", s.tons_per_hour()), pal::GOOD);
            ui.add_space(16.0);
            crate::widgets::stat(ui, "duration", duration(s.duration_secs()), pal::TEXT);
            ui.add_space(16.0);
            crate::widgets::stat(ui, "prospected", s.prospected.to_string(), pal::TEXT);
            ui.add_space(16.0);
            let (h, m, l) = s.content_counts();
            crate::widgets::stat(ui, "high / med / low", format!("{h} / {m} / {l}"), pal::CYAN);
        });
        ui.add_space(4.0);
        crate::widgets::split(ui, 0.5, 260.0, |ui| {
            Grid::new(("refined", s.start.0)).max_col_width(ui.available_width()).num_columns(2).striped(true).show(ui, |ui| {
                for (name, n) in &s.refined {
                    ui.label(name);
                    ui.label(RichText::new(format!("{n} t")).color(pal::ACCENT_SOFT));
                    ui.end_row();
                }
            });
        }, |ui| {
            if !s.motherlodes.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for (m, n) in &s.motherlodes {
                        badge(ui, &format!("{n}× {m} core"), pal::GOLD);
                    }
                });
            }
            let mut best: Vec<_> = s.best_proportion.iter().collect();
            best.sort_by(|a, b| b.1.total_cmp(a.1));
            for (m, p) in best.iter().take(6) {
                ui.label(RichText::new(format!("Best {m}: {p:.1}%")).color(pal::DIM));
            }
        });
    });
}

const NOT_NAMED: &str = "The game names a mining location only when your ship touches down or lifts off within 50 km of one. This time it named none: the Rhino was dropped while the ship hovered with no mining location targeted (or before Elite Nexus was running), or the ship set down further from every location. Target the location, or touch down by it, before you deploy the Rhino and its number is recorded; if you know it, your note can keep it.";

/// "Body 1 · Mining location 4": the body, then the number to pick in the navigation panel.
fn site_title(site: &SurfaceSite) -> String {
    let short = site.short_body();
    let body = if short == site.body { short.to_string() } else { format!("Body {short}") };
    match site.key.location {
        Some(n) => format!("{body} · Mining location {n}"),
        None => format!("{body} · location not named"),
    }
}

/// The planetary mining locations the Rhino worked, newest first. True when `prefs` changed.
fn sites(ui: &mut Ui, gs: &GameState, prefs: &mut MiningPrefs, action: &mut Option<MapAction>) -> bool {
    let mut changed = false;
    let mut list: Vec<&SurfaceSite> = gs.mining.sites.values().collect();
    list.sort_by_key(|s| std::cmp::Reverse(s.last));
    let mut commodities: Vec<&str> = list.iter().flat_map(|s| s.refined.keys().map(String::as_str)).collect();
    commodities.sort_unstable();
    commodities.dedup();
    let filter = Some(prefs.commodity.as_str()).filter(|c| commodities.contains(c)).map(str::to_string);
    card(ui, "", |ui| {
        if commodities.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                if widgets::pill(ui, filter.is_none(), "All").clicked() {
                    prefs.commodity.clear();
                    changed = true;
                }
                for c in &commodities {
                    if widgets::pill(ui, filter.as_deref() == Some(*c), c).clicked() {
                        prefs.commodity = (*c).to_string();
                        changed = true;
                    }
                }
            });
            ui.add_space(8.0);
        }
        let shown = list.iter().filter(|s| filter.as_ref().is_none_or(|c| s.refined.contains_key(c)));
        for (i, site) in shown.enumerate() {
            if i > 0 {
                ui.separator();
            }
            changed |= site_row(ui, gs, site, prefs, action);
        }
        footnote(
            ui,
            "To go back: jump to the system, target the body, then pick the Planetary Mining Location with that number in the navigation panel. \
             The number is the location the game named nearest to where your ship landed or lifted off; for a Rhino dropped from a hovering ship, \
             the one you had targeted (taken while Elite Nexus was running). Tonnes are what the Rhino's refinery made there, by-products included.",
        );
    });
    changed
}

/// One mining location: where, what came out, the commander's note. True when the note changed.
fn site_row(ui: &mut Ui, gs: &GameState, site: &SurfaceSite, prefs: &mut MiningPrefs, action: &mut Option<MapAction>) -> bool {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon::MAP_PIN).color(pal::ACCENT));
        let title = ui.label(RichText::new(site_title(site)).font(display(size::HEADLINE)).color(pal::TEXT));
        if site.key.location.is_none() {
            title.on_hover_text(NOT_NAMED);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, icon::PATH, "Plot a route there").clicked() {
                *action = Some(MapAction::PlotTo(site.system.clone()));
            }
            if widgets::icon_button(ui, icon::COPY, "Copy the system name").clicked() {
                ui.ctx().copy_text(site.system.clone());
            }
        });
    });
    let visits = if site.visits == 1 { "1 visit".to_string() } else { format!("{} visits", site.visits) };
    ui.label(RichText::new(format!("{} · {visits} · last {}", site.system, widgets::ago(site.last))).size(size::CAPTION).color(pal::DIM));
    ui.horizontal_wrapped(|ui| {
        for (c, t) in site.by_tonnes() {
            badge(ui, &format!("{c} {t} t"), pal::ACCENT_SOFT);
        }
    });
    if let Some(line) = locations_line(gs, site) {
        ui.label(RichText::new(line).size(size::CAPTION).color(pal::DIM))
            .on_hover_text("From your FSS or DSS scan of the body. \"At least\" when the game didn't name a location you worked here: it may be one of those it named.");
    }
    let mut text = prefs.note(site.key).to_string();
    let edit = egui::TextEdit::singleline(&mut text)
        .hint_text("Note: rigs that fit, density, worked out…")
        .desired_width(ui.available_width().min(460.0))
        .id(egui::Id::new(("mining-note", site.key)));
    if ui.add(edit).changed() {
        prefs.set_note(site.key, text);
        return true;
    }
    false
}

/// "13 mining locations on this body; you've worked 2", from the commander's own scans of it. A location the game
/// didn't name may be one of the named ones, so with one the count is "at least".
fn locations_line(gs: &GameState, site: &SurfaceSite) -> Option<String> {
    let total = gs.exploration.systems.get(&site.key.system_address)?.bodies.get(&site.key.body_id)?.mining_locations;
    if total == 0 {
        return None;
    }
    let on_body = gs.mining.sites.keys().filter(|k| k.system_address == site.key.system_address && k.body_id == site.key.body_id);
    let (named, unnamed) = on_body.fold((0, false), |(n, u), k| if k.location.is_some() { (n + 1, u) } else { (n, true) });
    let worked = if unnamed { format!("at least {}", named.max(1)) } else { named.to_string() };
    Some(format!("{total} mining location{} on this body; you've worked {worked}", if total == 1 { "" } else { "s" }))
}

// ---------------------------------------------------------------------------------------------
// Surface radar

/// What the radar shows on the body you're at: the deposits the Rhino refined at, your ship, and where it set down
/// on the last visit to each location.
pub struct RadarView {
    pub(crate) heading: f64,
    pub(crate) marks: Vec<Mark>,
}

pub(crate) struct Mark {
    pub(crate) kind: MarkKind,
    /// Beside the mark on the radar.
    pub(crate) tag: String,
    /// In the list under it.
    pub(crate) label: String,
    pub(crate) dist_m: f64,
    pub(crate) bearing: f64,
    /// The mining location's number (deposits and landings), when the game named one.
    pub(crate) location: Option<u32>,
    /// A deposit's own number on the body.
    pub(crate) number: Option<u32>,
    /// Which of the saved deposits it is ([`MiningPrefs::deposits`]), for its note.
    pub(crate) deposit: Option<usize>,
    /// A deposit's outline: the ground the Rhino refined on there.
    pub(crate) outline: Option<Outline>,
}

/// The ground the Rhino worked at a deposit: a circle round the places it refined there, and those places, in metres
/// east and north of you.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Outline {
    pub(crate) east: f64,
    pub(crate) north: f64,
    pub(crate) radius: f64,
    pub(crate) places: Vec<(f64, f64)>,
}

/// Metres east and north of you of a point `dist` metres away on a compass `bearing`.
fn east_north(dist: f64, bearing: f64) -> (f64, f64) {
    let b = bearing.to_radians();
    (dist * b.sin(), dist * b.cos())
}

/// The circle round the places the Rhino refined at a deposit, seen from `me` on a body of `radius_m`; None until two
/// of them stand a few metres apart.
fn outline(me: (f64, f64), places: impl Iterator<Item = (f64, f64)>, radius_m: f64) -> Option<Outline> {
    let places: Vec<(f64, f64)> = places.map(|p| east_north(surface_distance(me, p, radius_m), surface_bearing(me, p))).collect();
    let (lo, hi) = places.iter().fold(((f64::MAX, f64::MAX), (f64::MIN, f64::MIN)), |(lo, hi), &(e, n)| ((lo.0.min(e), lo.1.min(n)), (hi.0.max(e), hi.1.max(n))));
    let (east, north) = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
    let radius = places.iter().map(|(e, n)| (e - east).hypot(n - north)).fold(0.0, f64::max);
    (radius >= 2.0).then_some(Outline { east, north, radius, places })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkKind {
    Deposit,
    Ship,
    Landing,
}

/// None away from a body, or on a body with nothing to show.
pub fn radar_view(gs: &GameState, prefs: &MiningPrefs) -> Option<RadarView> {
    let st = gs.status.as_ref()?;
    let me = st.lat_lon()?;
    let radius_m = st.planet_radius.filter(|r| *r > 0.0)?;
    let loc = &gs.location;
    let body_id = loc.body_id?;
    if let (Some(a), Some(b)) = (&st.body_name, &loc.body) && a != b {
        return None;
    }
    let on_body = |k: SiteKey| k.system_address == loc.system_address && k.body_id == body_id;
    let at_location = |n: Option<u32>| n.map(|n| format!(" (mining location {n})")).unwrap_or_default();
    let mark = |kind: MarkKind, pos: (f64, f64), tag: String, label: String, location: Option<u32>| Mark {
        kind,
        tag,
        label,
        dist_m: surface_distance(me, pos, radius_m),
        bearing: surface_bearing(me, pos),
        location,
        number: None,
        deposit: None,
        outline: None,
    };
    let mut marks = Vec::new();
    for (i, d) in prefs.deposits.iter().enumerate().filter(|(_, d)| on_body(d.key())) {
        let tag = d.commodities.first().cloned().unwrap_or_default();
        let label = format!("{} deposit{}", d.commodities.join(", "), at_location(d.location));
        let outline = outline(me, std::iter::once((d.lat, d.lon)).chain(d.points.iter().copied()), radius_m);
        marks.push(Mark { number: Some(d.number).filter(|n| *n > 0), deposit: Some(i), outline, ..mark(MarkKind::Deposit, (d.lat, d.lon), tag, label, d.location) });
    }
    let ship = gs.mining.here.as_ref().filter(|h| on_body(h.key)).and_then(|h| h.ship);
    if let Some(p) = ship {
        marks.push(mark(MarkKind::Ship, p, "Ship".into(), "Your ship".into(), None));
    }
    for site in gs.mining.sites.values().filter(|s| on_body(s.key)) {
        if let Some(&p) = site.landings.last() && Some(p) != ship {
            marks.push(mark(MarkKind::Landing, p, "Landed".into(), format!("Where your ship landed last time{}", at_location(site.key.location)), site.key.location));
        }
    }
    (!marks.is_empty()).then(|| RadarView { heading: st.heading.unwrap_or(0.0), marks })
}

/// How far the surface radar reaches and where a mark sits on it, for the Mining page and the overlay's dial alike.
/// It's scaled for the deposits near you, so one far away doesn't squeeze them into the middle: the reach takes in
/// every mark up to three times as far as the nearest deposit, and anything within a kilometre, and is at least
/// 250 m. Marks further out wait on the rim, in their direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RadarScale {
    /// The distance the rim stands for, metres.
    pub(crate) reach_m: f64,
}

impl RadarScale {
    /// Marks up to this many times as far as the nearest deposit are near.
    const SPREAD: f64 = 3.0;
    /// So are marks within this many metres, however close the nearest deposit is.
    const NEAR_M: f64 = 1_000.0;
    /// The shortest reach, metres.
    const MIN_M: f64 = 250.0;
    /// Room between the farthest mark near you and the rim.
    const MARGIN: f64 = 1.15;

    pub(crate) fn of(v: &RadarView) -> RadarScale {
        let deposits: Vec<f64> = v.marks.iter().filter(|m| m.kind == MarkKind::Deposit).map(|m| m.dist_m).collect();
        let all: Vec<f64> = v.marks.iter().map(|m| m.dist_m).collect();
        RadarScale::fit(&deposits, &all)
    }

    /// Scaled around the nearest of `deposits` (with none, the nearest of `all`), taking in every one of `all` near it.
    fn fit(deposits: &[f64], all: &[f64]) -> RadarScale {
        let nearest = |d: &[f64]| d.iter().copied().reduce(f64::min);
        let near = nearest(deposits).or_else(|| nearest(all)).map_or(0.0, |d| d * Self::SPREAD).max(Self::NEAR_M);
        let far = all.iter().copied().filter(|d| *d <= near).fold(0.0, f64::max);
        RadarScale { reach_m: (far * Self::MARGIN).max(Self::MIN_M) }
    }

    /// How far out from the centre a mark `dist_m` metres away sits, as a share of the radius: 1 on the rim, where the
    /// marks beyond the reach wait.
    pub(crate) fn at(&self, dist_m: f64) -> f64 {
        (dist_m / self.reach_m).min(1.0)
    }

    /// Beyond the reach: on the rim, with its distance beside it.
    pub(crate) fn beyond(&self, dist_m: f64) -> bool {
        dist_m > self.reach_m
    }
}

/// The radar, the minimap and how to place rigs, with the distances and bearings under them and a note on each deposit
/// (Mining page). True when a note changed.
pub fn surface_radar(ui: &mut Ui, v: &RadarView, prefs: &mut MiningPrefs) -> bool {
    const RADAR: f32 = 320.0;
    const MAP: f32 = 250.0;
    const GUIDE: f32 = 290.0;
    let scale = RadarScale::of(v);
    let map = |ui: &mut Ui| {
        ui.set_width(MAP);
        minimap(ui, v, scale, MAP);
        ui.label(RichText::new("North up · a ring is the ground the Rhino refined on").size(size::CAPTION).color(pal::DIM));
    };
    let avail = ui.available_width();
    if avail >= RADAR + MAP + GUIDE + 2.0 * GAP {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            ui.vertical(|ui| radar(ui, v, RADAR));
            ui.vertical(map);
            ui.vertical(|ui| rig_guide(ui, true));
        });
    } else if avail >= RADAR + MAP + GAP {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            ui.vertical(|ui| radar(ui, v, RADAR));
            ui.vertical(map);
        });
        ui.add_space(8.0);
        rig_guide(ui, true);
    } else {
        radar(ui, v, RADAR);
        ui.add_space(8.0);
        ui.vertical(map);
        ui.add_space(8.0);
        rig_guide(ui, true);
    }
    ui.add_space(6.0);
    let changed = radar_list(ui, v, 8, prefs);
    footnote(
        ui,
        "Up is the way you're facing; turn until your heading shows the bearing. The radar is scaled to the deposits near you: anything \
         further out waits on the rim, in its direction, with its distance. Deposits are where the Rhino refined while Elite Nexus was running \
         (the journal records no positions), numbered on each body in the order they were found; numbers and notes are kept between runs. \
         Rigs aren't in the journal either: a deposit's ring is the ground the Rhino refined on there, so its rigs stood inside it, and the \
         deposit may reach further.",
    );
    changed
}

/// Driving the Rhino.
fn in_rhino(gs: &GameState) -> bool {
    gs.location.in_srv && Vehicle::from_srv_type(gs.location.srv_type.as_deref()) == Vehicle::Rhino
}

/// How far apart rigs go at the least: what commanders report (docs/research/surface-mining.md), as Frontier hasn't
/// published it.
const RIG_SPACING_M: f64 = 75.0;
/// Rigs that work at once, at most.
const MAX_RIGS: u32 = 6;

/// The narrowest round deposit that takes `rigs` rigs at least `spacing` metres apart: the best packing of that many
/// points in a disc (for six, one in the middle and five on a ring `spacing` out).
fn patch_width(rigs: u32, spacing: f64) -> f64 {
    match rigs {
        0 | 1 => 0.0,
        2 => spacing,
        3 => 2.0 * spacing / 3f64.sqrt(),
        4 => spacing * 2f64.sqrt(),
        5 => spacing / 36f64.to_radians().sin(),
        _ => 2.0 * spacing,
    }
}

/// How far apart to place rigs, and how many fit a deposit of a given width, saying what that rests on.
fn rig_guide(ui: &mut Ui, heading: bool) {
    if heading {
        ui.label(RichText::new("Placing rigs").font(display(size::HEADLINE)).color(pal::TEXT));
    }
    let spacing = distance_text(RIG_SPACING_M);
    ui.add(egui::Label::new(RichText::new(format!("Rigs need about {spacing} between them, and up to {MAX_RIGS} work at once. A round deposit takes:")).color(pal::TEXT)).wrap());
    ui.add_space(2.0);
    Grid::new("rigs-fit").num_columns(MAX_RIGS as usize).spacing(Vec2::new(14.0, 4.0)).show(ui, |ui| {
        th(ui, "Across");
        for n in 2..=MAX_RIGS {
            ui.label(RichText::new(distance_text(patch_width(n, RIG_SPACING_M).round())).color(pal::TEXT));
        }
        ui.end_row();
        th(ui, "Rigs");
        for n in 2..=MAX_RIGS {
            ui.label(RichText::new(n.to_string()).font(display(size::BODY)).color(pal::ACCENT));
        }
        ui.end_row();
    });
    ui.add_space(2.0);
    footnote(
        ui,
        &format!("For six, one in the middle and five on a ring {spacing} out, evenly spaced. Deposits aren't round, so take the widths as a guide; a deposit's note keeps what really fitted."),
    );
    footnote(
        ui,
        &format!(
            "Frontier hasn't published the spacing: about {spacing} is what commanders reported in September 2026 (their estimates ran from 75 to 100 m), \
             and the game refuses a rig closer than that. Deposits run from a few metres to 300 m across (the game's codex, as commanders quote it), \
             and a rig extracts up to 12 chunks."
        ),
    );
}

/// The narrowest the minimap gets, metres: room for a deposit of the largest size (300 m across) beside you.
const MAP_MIN_M: f64 = 300.0;

/// A north-up map of the ground round you, framed to fit it: the deposits near you (each with its outline, the places
/// the Rhino refined there and its number), your ship, where it landed, and you. Marks the radar keeps on its rim are
/// left off. At most `max_side` wide.
fn minimap(ui: &mut Ui, v: &RadarView, scale: RadarScale, max_side: f32) {
    let near: Vec<(&Mark, (f64, f64))> = v.marks.iter().filter(|m| !scale.beyond(m.dist_m)).map(|m| (m, east_north(m.dist_m, m.bearing))).collect();
    // The frame: you, the marks and their outlines, with room round them.
    let (mut lo, mut hi) = ((0.0f64, 0.0f64), (0.0f64, 0.0f64));
    let mut take = |e: f64, n: f64, r: f64| {
        (lo.0, lo.1, hi.0, hi.1) = (lo.0.min(e - r), lo.1.min(n - r), hi.0.max(e + r), hi.1.max(n + r));
    };
    for (m, (e, n)) in &near {
        take(*e, *n, 0.0);
        if let Some(o) = &m.outline {
            take(o.east, o.north, o.radius);
        }
    }
    let mid = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
    let half = ((hi.0 - lo.0).max(hi.1 - lo.1) / 2.0 * 1.15).max(MAP_MIN_M / 2.0);
    let side = ui.available_width().min(max_side);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 10.0, pal::FIELD);
    p.rect_stroke(rect, 10.0, Stroke::new(1.0, pal::SEPARATOR), egui::StrokeKind::Inside);
    let k = (side / 2.0 - 14.0) / half as f32;
    let at = |e: f64, n: f64| rect.center() + Vec2::new((e - mid.0) as f32, (mid.1 - n) as f32) * k;
    for o in near.iter().filter_map(|(m, _)| m.outline.as_ref()) {
        let c = at(o.east, o.north);
        let r = (o.radius as f32 * k).max(5.0);
        p.circle_filled(c, r, pal::ACCENT.gamma_multiply(0.10));
        p.circle_stroke(c, r, Stroke::new(1.0, pal::ACCENT.gamma_multiply(0.55)));
        for &(e, n) in &o.places {
            p.circle_filled(at(e, n), 2.2, pal::ACCENT_SOFT.gamma_multiply(0.8));
        }
    }
    let mut labels = Vec::new();
    for (m, (e, n)) in &near {
        let sp = at(*e, *n);
        match m.kind {
            MarkKind::Deposit => {
                let diamond = [sp + Vec2::new(0.0, -6.0), sp + Vec2::new(5.0, 0.0), sp + Vec2::new(0.0, 6.0), sp + Vec2::new(-5.0, 0.0)];
                p.add(egui::Shape::convex_polygon(diamond.to_vec(), pal::ACCENT, Stroke::NONE));
                if let Some(num) = m.number {
                    labels.push((sp, p.layout_no_wrap(num.to_string(), display(size::CAPTION), pal::ACCENT_SOFT)));
                }
            }
            MarkKind::Ship => {
                p.circle_filled(sp, 5.0, pal::CYAN);
            }
            MarkKind::Landing => {
                p.circle_stroke(sp, 4.5, Stroke::new(1.5, pal::DIM));
            }
        }
    }
    // You: an arrow the way you're facing.
    let me = at(0.0, 0.0);
    let (s, c) = (v.heading.to_radians().sin() as f32, v.heading.to_radians().cos() as f32);
    let turn = |x: f32, y: f32| me + Vec2::new(x * c - y * s, x * s + y * c);
    p.add(egui::Shape::convex_polygon(vec![turn(0.0, -9.0), turn(6.0, 7.0), turn(-6.0, 7.0)], pal::TEXT, Stroke::NONE));
    let north = p.text(rect.right_top() + Vec2::new(-10.0, 8.0), Align2::RIGHT_TOP, "N ↑", display(size::CAPTION), pal::DIM);
    // A scale bar: the rig spacing when the map is zoomed in that far, else a round distance.
    let (len, text) = map_bar(f64::from(side) * 0.3 / f64::from(k));
    let a = rect.left_bottom() + Vec2::new(12.0, -12.0);
    let b = a + Vec2::new(len as f32 * k, 0.0);
    let stroke = Stroke::new(1.5, pal::DIM);
    p.line_segment([a, b], stroke);
    p.line_segment([a, a - Vec2::new(0.0, 5.0)], stroke);
    p.line_segment([b, b - Vec2::new(0.0, 5.0)], stroke);
    let bar = p.text(a - Vec2::new(0.0, 8.0), Align2::LEFT_BOTTOM, text, egui::FontId::proportional(size::CAPTION), pal::DIM);
    let mut taken = vec![Rect::from_center_size(me, Vec2::new(18.0, 18.0)), north, bar];
    for (sp, galley) in labels {
        let pos = place_label(sp, galley.size(), rect.shrink(4.0), 7.0, &mut taken);
        p.galley(pos, galley, pal::TEXT);
    }
}

/// The scale bar's length and label, at most `most` metres long: the rig spacing when the map is zoomed in that far,
/// else the longest round distance that fits.
fn map_bar(most: f64) -> (f64, String) {
    if (RIG_SPACING_M..RIG_SPACING_M * 2.0).contains(&most) {
        return (RIG_SPACING_M, format!("{} · rig spacing", distance_text(RIG_SPACING_M)));
    }
    const STEPS: [f64; 14] = [10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1_000.0, 2_500.0, 5_000.0, 10_000.0, 25_000.0, 50_000.0, 100_000.0, 250_000.0];
    let len = STEPS.into_iter().rev().find(|s| *s <= most).unwrap_or(STEPS[0]);
    (len, distance_text(len))
}

/// Top-down radar centred on you, "up" = the way you're facing (like the exobiology radar), at most `max_side` wide.
fn radar(ui: &mut Ui, v: &RadarView, max_side: f32) {
    let side = ui.available_width().min(max_side);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let p = ui.painter_at(rect);
    let c = rect.center();
    let r_px = side / 2.0 - 8.0;
    p.circle_filled(c, r_px, pal::FIELD);
    p.circle_stroke(c, r_px, Stroke::new(1.0, pal::SEPARATOR));
    let scale = RadarScale::of(v);
    let step = ring_step(scale.reach_m);
    let mut ring = step;
    while ring < scale.reach_m {
        p.circle_stroke(c, r_px * (ring / scale.reach_m) as f32, Stroke::new(1.0, pal::BORDER.gamma_multiply(0.6)));
        ring += step;
    }
    // Screen position a share `r` of the radius out on a compass `bearing`, turned so the heading points up.
    let at = |r: f64, bearing: f64| -> Pos2 {
        let a = (bearing - v.heading).to_radians();
        c + Vec2::new(a.sin() as f32, -a.cos() as f32) * (r as f32 * r_px)
    };
    let north = p.text(at(0.92, 0.0), Align2::CENTER_CENTER, "N", display(size::CAPTION), pal::DIM);
    let header = p.text(
        rect.left_top() + Vec2::new(6.0, 4.0),
        Align2::LEFT_TOP,
        format!("{} · rings every {}", distance_text(scale.reach_m), distance_text(step)),
        egui::FontId::proportional(size::CAPTION),
        pal::DIM,
    );
    // Each deposit's outline under the marks, turned with them.
    let k = r_px / scale.reach_m as f32;
    let (hs, hc) = (v.heading.to_radians().sin() as f32, v.heading.to_radians().cos() as f32);
    for o in v.marks.iter().filter(|m| !scale.beyond(m.dist_m)).filter_map(|m| m.outline.as_ref()) {
        let (e, n) = (o.east as f32, o.north as f32);
        let r = o.radius as f32 * k;
        if r >= 3.0 {
            let mid = c + Vec2::new(e * hc - n * hs, -(n * hc + e * hs)) * k;
            p.circle_filled(mid, r, pal::ACCENT.gamma_multiply(0.10));
            p.circle_stroke(mid, r, Stroke::new(1.0, pal::ACCENT.gamma_multiply(0.55)));
        }
    }
    // Labels go on after every mark, nearest first, so none hides a mark or another label, and each stays inside.
    let mut marks: Vec<&Mark> = v.marks.iter().collect();
    marks.sort_by(|a, b| a.dist_m.total_cmp(&b.dist_m));
    let mut labels = Vec::with_capacity(marks.len());
    for m in marks {
        let beyond = scale.beyond(m.dist_m);
        let sp = at(scale.at(m.dist_m), m.bearing);
        let fade = if beyond { 0.6 } else { 1.0 };
        match m.kind {
            MarkKind::Deposit => {
                let diamond = [sp + Vec2::new(0.0, -7.0), sp + Vec2::new(6.0, 0.0), sp + Vec2::new(0.0, 7.0), sp + Vec2::new(-6.0, 0.0)];
                p.add(egui::Shape::convex_polygon(diamond.to_vec(), pal::ACCENT.gamma_multiply(fade), Stroke::NONE));
            }
            MarkKind::Ship => {
                p.circle_filled(sp, 6.0, pal::CYAN.gamma_multiply(fade));
            }
            MarkKind::Landing => {
                p.circle_stroke(sp, 5.0, Stroke::new(1.5, pal::DIM));
            }
        }
        labels.push((sp, mark_label(&p, m, beyond)));
    }
    let me = [c + Vec2::new(0.0, -9.0), c + Vec2::new(6.0, 7.0), c + Vec2::new(-6.0, 7.0)];
    p.add(egui::Shape::convex_polygon(me.to_vec(), pal::TEXT, Stroke::NONE));
    let inside = rect.shrink(2.0);
    let mut taken = vec![Rect::from_center_size(c, Vec2::new(14.0, 18.0)), north, header];
    for (sp, galley) in labels {
        let pos = place_label(sp, galley.size(), inside, 9.0, &mut taken);
        p.galley(pos, galley, pal::TEXT);
    }
}

/// A mark's label on the radar: a deposit's number, what the mark is, and the distance of one waiting on the rim.
fn mark_label(p: &egui::Painter, m: &Mark, beyond: bool) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    let part = |color| egui::TextFormat { font_id: display(size::CAPTION), color, ..Default::default() };
    if let Some(n) = m.number {
        job.append(&format!("{n} "), 0.0, part(pal::ACCENT_SOFT));
    }
    job.append(&m.tag, 0.0, part(pal::TEXT));
    if beyond {
        job.append(&format!(" · {}", distance_text(m.dist_m)), 0.0, part(pal::DIM));
    }
    p.layout_job(job)
}

/// Where the top left of a label of `size` goes beside a mark at `at`: to its right, or to its left where the right has
/// no room, moved inside `bounds` where it would still cross them.
fn label_pos(at: Pos2, size: Vec2, bounds: Rect, gap: f32) -> Pos2 {
    let x = if at.x + gap + size.x <= bounds.right() { at.x + gap } else { at.x - gap - size.x };
    let y = at.y - size.y / 2.0;
    Pos2::new(x.clamp(bounds.left(), (bounds.right() - size.x).max(bounds.left())), y.clamp(bounds.top(), (bounds.bottom() - size.y).max(bounds.top())))
}

/// [`label_pos`], moved a line down or up (the nearest that's free) when it would cover a label already placed or
/// another rect in `taken`; the place chosen joins `taken`.
fn place_label(at: Pos2, size: Vec2, bounds: Rect, gap: f32, taken: &mut Vec<Rect>) -> Pos2 {
    let line = size.y + 2.0;
    let free = |p: &Pos2| !taken.iter().any(|r| r.intersects(Rect::from_min_size(*p, size)));
    let pos = [0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0]
        .into_iter()
        .map(|k| label_pos(at + Vec2::new(0.0, k * line), size, bounds, gap))
        .find(free)
        .unwrap_or_else(|| label_pos(at, size, bounds, gap));
    taken.push(Rect::from_min_size(pos, size));
    pos
}

/// Nearest first, at most `rows`: what each mark is, how far, and the compass bearing to drive or fly; a deposit also
/// its number and the commander's note on it. True when a note changed.
fn radar_list(ui: &mut Ui, v: &RadarView, rows: usize, prefs: &mut MiningPrefs) -> bool {
    let mut changed = false;
    let mut marks: Vec<&Mark> = v.marks.iter().collect();
    marks.sort_by(|a, b| a.dist_m.total_cmp(&b.dist_m));
    Grid::new("radar-list").num_columns(4).spacing(Vec2::new(10.0, 4.0)).max_col_width(ui.available_width()).show(ui, |ui| {
        for m in marks.iter().take(rows) {
            let (glyph, col) = match m.kind {
                MarkKind::Deposit => (icon::DIAMOND, pal::ACCENT),
                MarkKind::Ship => (icon::ROCKET, pal::CYAN),
                MarkKind::Landing => (icon::MAP_PIN, pal::DIM),
            };
            let bearing = (m.bearing.round() as i64).rem_euclid(360);
            ui.horizontal(|ui| {
                ui.label(RichText::new(glyph).color(col));
                if let Some(n) = m.number {
                    ui.label(RichText::new(n.to_string()).font(display(size::BODY)).color(pal::TEXT));
                }
            });
            ui.label(RichText::new(&m.label).color(pal::TEXT));
            ui.label(RichText::new(format!("{} · bearing {bearing:03}°", distance_text(m.dist_m))).color(pal::DIM));
            match m.deposit.and_then(|i| prefs.deposits.get_mut(i).map(|d| (i, d))) {
                Some((i, d)) => {
                    let edit = egui::TextEdit::singleline(&mut d.note)
                        .hint_text("Note: rigs that fit, density…")
                        .desired_width(ui.available_width().clamp(140.0, 260.0))
                        .id(egui::Id::new(("deposit-note", i)));
                    changed |= ui.add(edit).changed();
                }
                None => {
                    ui.label("");
                }
            }
            ui.end_row();
        }
    });
    changed
}

/// A round ring spacing that puts at most four rings inside `max_d` metres.
fn ring_step(max_d: f64) -> f64 {
    const STEPS: [f64; 12] = [50.0, 100.0, 250.0, 500.0, 1_000.0, 2_500.0, 5_000.0, 10_000.0, 25_000.0, 50_000.0, 100_000.0, 250_000.0];
    STEPS.into_iter().find(|s| max_d / s <= 4.0).unwrap_or(500_000.0)
}

pub(crate) fn distance_text(m: f64) -> String {
    if m < 1_000.0 {
        format!("{m:.0} m")
    } else if m < 10_000.0 {
        format!("{:.1} km", m / 1_000.0)
    } else {
        format!("{:.0} km", m / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deposit at `lat` on Alpha 1 (a 1,000 km body), mining location 4.
    fn site_with_spot(gs: &mut GameState, lat: f64, commodity: &str) {
        let key = SiteKey { system_address: 100, body_id: 1, location: Some(4) };
        let site = gs.mining.sites.entry(key).or_insert_with(|| SurfaceSite::new(key, "Alpha", "Alpha 1"));
        site.radius_m = Some(1_000_000.0);
        site.spots.push(Spot { lat, lon: 20.0, refined: [(commodity.to_string(), 1)].into(), found: Timestamp(1), points: vec![(lat, 20.0)] });
    }

    #[test]
    fn deposits_are_saved_once() {
        let mut gs = GameState::default();
        let mut prefs = MiningPrefs::default();
        site_with_spot(&mut gs, 10.0, "Iridium");
        assert!(remember(&gs, &mut prefs));
        assert!(!remember(&gs, &mut prefs), "nothing new the second time");
        assert_eq!(prefs.deposits.len(), 1);
        // The same deposit on a later run (a few metres off) adds what came out there; one 2 km away is another.
        gs.mining.sites.values_mut().for_each(|s| s.spots.clear());
        site_with_spot(&mut gs, 10.0003, "Platinum");
        site_with_spot(&mut gs, 10.12, "Iridium");
        assert!(remember(&gs, &mut prefs));
        assert_eq!(prefs.deposits.len(), 2);
        assert_eq!(prefs.deposits[0].commodities, ["Iridium", "Platinum"]);
    }

    /// Where the Rhino refined at a deposit is kept between runs, a place once, and the deposit's outline is drawn round
    /// those places.
    #[test]
    fn a_deposit_keeps_where_the_rhino_refined_and_its_outline_rounds_them() {
        let mut gs = GameState::default();
        let mut prefs = MiningPrefs::default();
        site_with_spot(&mut gs, 10.0, "Iridium");
        remember(&gs, &mut prefs);
        assert_eq!(prefs.deposits[0].points, [(10.0, 20.0)]);
        // A later run: a rig 75 m north (0.0043° on a 1,000 km body) and the first place again.
        gs.mining.sites.values_mut().for_each(|s| s.spots.clear());
        site_with_spot(&mut gs, 10.0, "Iridium");
        gs.mining.sites.values_mut().for_each(|s| s.spots[0].points.push((10.0043, 20.0)));
        assert!(remember(&gs, &mut prefs));
        assert!(!remember(&gs, &mut prefs), "each place once");
        let d = &prefs.deposits[0];
        assert_eq!(d.points, [(10.0, 20.0), (10.0043, 20.0)]);
        // Seen from 100 m south of the first place: a circle 75 m across, its middle 137.5 m north.
        let o = outline((9.9943, 20.0), std::iter::once((d.lat, d.lon)).chain(d.points.iter().copied()), 1_000_000.0).expect("outline");
        assert_eq!((o.north.round(), o.radius.round()), (137.0, 38.0), "{o:?}");
        assert!(o.east.abs() < 0.01 && o.places.len() == 3, "{o:?}");
        assert!(outline((9.99, 20.0), [(10.0, 20.0), (10.0, 20.0)].into_iter(), 1_000_000.0).is_none(), "one place: no outline");
    }

    #[test]
    fn rigs_that_fit_a_deposit() {
        let widths: Vec<f64> = (2..=MAX_RIGS).map(|n| patch_width(n, RIG_SPACING_M).round()).collect();
        assert_eq!(widths, [75.0, 87.0, 106.0, 128.0, 150.0]);
        assert_eq!(patch_width(1, RIG_SPACING_M), 0.0);
        // The minimap's scale bar shows the rig spacing when zoomed in far enough, else a round distance.
        assert_eq!(map_bar(100.0), (75.0, "75 m · rig spacing".to_string()));
        assert_eq!(map_bar(160.0), (100.0, "100 m".to_string()));
        assert_eq!(map_bar(4_000.0), (2_500.0, "2.5 km".to_string()));
        assert_eq!(map_bar(3.0).0, 10.0);
    }

    #[test]
    fn a_deposit_takes_the_number_a_later_landing_names() {
        let mut gs = GameState::default();
        let mut prefs = MiningPrefs::default();
        let unnamed = SiteKey { system_address: 100, body_id: 1, location: None };
        let site = gs.mining.sites.entry(unnamed).or_insert_with(|| SurfaceSite::new(unnamed, "Alpha", "Alpha 1"));
        site.radius_m = Some(1_000_000.0);
        site.spots.push(Spot { lat: 10.0, lon: 20.0, refined: [("Iridium".to_string(), 1)].into(), found: Timestamp(1), ..Default::default() });
        remember(&gs, &mut prefs);
        gs.mining.sites.clear();
        site_with_spot(&mut gs, 10.0001, "Iridium");
        assert!(remember(&gs, &mut prefs));
        assert_eq!((prefs.deposits.len(), prefs.deposits[0].location), (1, Some(4)), "one deposit, now with its number");
    }

    /// Chequers2's idea (discussion 5): two Iridium deposits on one body, told apart by their numbers and notes.
    #[test]
    fn deposits_are_numbered_on_each_body_and_keep_their_numbers() {
        let mut gs = GameState::default();
        let mut prefs = MiningPrefs::default();
        let numbers = |p: &MiningPrefs| p.deposits.iter().map(|d| d.number).collect::<Vec<_>>();
        site_with_spot(&mut gs, 10.0, "Iridium");
        site_with_spot(&mut gs, 10.12, "Iridium");
        remember(&gs, &mut prefs);
        assert_eq!(numbers(&prefs), [1, 2]);
        prefs.deposits[1].note = "1 rig".into();
        // A later run: the second deposit again (a few metres off) and a new one.
        gs.mining.sites.values_mut().for_each(|s| s.spots.clear());
        site_with_spot(&mut gs, 10.1202, "Iridium");
        site_with_spot(&mut gs, 10.3, "Ruby");
        assert!(remember(&gs, &mut prefs));
        assert_eq!(numbers(&prefs), [1, 2, 3]);
        assert_eq!(prefs.deposits[1].note, "1 rig");
        // Another body starts from 1.
        let other = SiteKey { system_address: 100, body_id: 2, location: None };
        let site = gs.mining.sites.entry(other).or_insert_with(|| SurfaceSite::new(other, "Alpha", "Alpha 2"));
        site.radius_m = Some(1_000_000.0);
        site.spots.push(Spot { lat: 0.0, lon: 0.0, refined: [("Iridium".to_string(), 1)].into(), found: Timestamp(5), ..Default::default() });
        remember(&gs, &mut prefs);
        assert_eq!(numbers(&prefs), [1, 2, 3, 1]);
    }

    /// Deposits kept by 0.7.3-beta.1 have no number: they get one, oldest first, and later ones follow.
    #[test]
    fn deposits_kept_before_numbers_get_them_in_the_order_found() {
        let deposit = |found: i64| Deposit { system_address: 100, body_id: 1, found: Timestamp(found), ..Default::default() };
        let mut deposits = vec![deposit(30), deposit(10), deposit(20)];
        assert!(number_deposits(&mut deposits));
        assert_eq!(deposits.iter().map(|d| d.number).collect::<Vec<_>>(), [3, 1, 2]);
        assert!(!number_deposits(&mut deposits), "nothing new to number");
        deposits.push(deposit(5));
        number_deposits(&mut deposits);
        assert_eq!(deposits[3].number, 4, "after the highest, though found earlier");
    }

    /// A Rhino dropped from a hover gets its location only live: it's kept once and given back to the next start.
    #[test]
    fn hover_drops_are_kept_and_given_back() {
        let mut gs = GameState::default();
        let mut prefs = MiningPrefs::default();
        let k = SiteKey { system_address: 100, body_id: 1, location: Some(6) };
        gs.mining.drops.insert(Timestamp(50), k);
        assert!(remember(&gs, &mut prefs));
        assert!(!remember(&gs, &mut prefs), "kept once");
        assert_eq!(prefs.drops, [HoverDrop { at: Timestamp(50), system_address: 100, body_id: 1, location: 6 }]);
        assert_eq!(drops(&prefs), [(Timestamp(50), k)].into());
    }

    /// Chequers2's A 1 (discussion 5): worked, but the game named no location, so "you've worked 0" was wrong.
    #[test]
    fn worked_counts_a_location_the_game_did_not_name() {
        let mut gs = GameState::default();
        gs.exploration.system_mut(100, "Alpha", Timestamp(1)).body_mut(1, "Alpha 1").mining_locations = 17;
        let unnamed = SiteKey { system_address: 100, body_id: 1, location: None };
        gs.mining.sites.insert(unnamed, SurfaceSite::new(unnamed, "Alpha", "Alpha 1"));
        let line = |gs: &GameState, k: SiteKey| locations_line(gs, &gs.mining.sites[&k]).unwrap();
        assert_eq!(line(&gs, unnamed), "17 mining locations on this body; you've worked at least 1");
        let named = SiteKey { location: Some(9), ..unnamed };
        gs.mining.sites.insert(named, SurfaceSite::new(named, "Alpha", "Alpha 1"));
        assert_eq!(line(&gs, named), "17 mining locations on this body; you've worked at least 1", "the unnamed one may be location 9");
        gs.mining.sites.remove(&unnamed);
        assert_eq!(line(&gs, named), "17 mining locations on this body; you've worked 1");
    }

    #[test]
    fn notes() {
        let mut prefs = MiningPrefs::default();
        let k = SiteKey { system_address: 100, body_id: 1, location: Some(4) };
        prefs.set_note(k, "6 rigs fit".into());
        assert_eq!(prefs.note(k), "6 rigs fit");
        prefs.set_note(k, "6 rigs fit, high".into());
        assert_eq!(prefs.notes.len(), 1);
        prefs.set_note(k, "  ".into());
        assert!(prefs.notes.is_empty(), "an emptied note is gone");
    }

    /// The radar is scaled for the deposits near you; one far away waits on the rim instead of squeezing them together.
    #[test]
    fn the_radar_is_scaled_for_the_deposits_near_you() {
        let reach = |deposits: &[f64], others: &[f64]| {
            let all: Vec<f64> = deposits.iter().chain(others).copied().collect();
            RadarScale::fit(deposits, &all).reach_m.round()
        };
        // At a deposit, two more a few hundred metres off, the ship 2 km away and another location's deposit 25 km away.
        let s = RadarScale::fit(&[50.0, 300.0, 350.0, 25_000.0], &[50.0, 300.0, 350.0, 25_000.0, 2_000.0]);
        assert_eq!(s.reach_m.round(), 402.0, "350 m and a little room");
        assert!(s.beyond(2_000.0) && s.beyond(25_000.0) && !s.beyond(350.0));
        assert_eq!((s.at(25_000.0), s.at(0.0)), (1.0, 0.0), "on the rim; you in the middle");
        // Chequers2's two Iridium deposits (discussion 5), 1.8 and 4.4 km away: both near enough to show in place.
        assert_eq!(reach(&[1_800.0, 4_400.0], &[]), 5_060.0);
        // A chain of deposits each about three times further: what's within a kilometre stays, the rest waits.
        assert_eq!(reach(&[100.0, 250.0, 700.0, 2_000.0, 6_000.0], &[]), 805.0);
        // Flying in: the cluster 30 km off is in view, a deposit on the far side of the body isn't.
        assert_eq!(reach(&[30_000.0, 30_200.0, 30_500.0, 300_000.0], &[]), 35_075.0);
        // No deposit yet: the ship and its last landing.
        assert_eq!(reach(&[], &[20.0, 600.0]), 690.0);
        // Never under 250 m.
        assert_eq!(reach(&[10.0], &[]), 250.0);
        assert_eq!(reach(&[], &[]), 250.0);
    }

    /// A label beside a mark near the right or the bottom edge turns or moves to stay inside the radar.
    #[test]
    fn labels_stay_inside_the_radar() {
        let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::splat(300.0));
        let size = Vec2::new(80.0, 16.0);
        assert_eq!(label_pos(Pos2::new(100.0, 100.0), size, bounds, 9.0), Pos2::new(109.0, 92.0), "to the right");
        assert_eq!(label_pos(Pos2::new(280.0, 100.0), size, bounds, 9.0), Pos2::new(191.0, 92.0), "to the left at the right edge");
        assert_eq!(label_pos(Pos2::new(100.0, 298.0), size, bounds, 9.0), Pos2::new(109.0, 284.0), "up from the bottom edge");
        assert_eq!(label_pos(Pos2::new(40.0, 2.0), Vec2::new(400.0, 16.0), bounds, 9.0), Pos2::new(0.0, 0.0), "wider than the radar: from its left edge");
        // Two deposits at the same place far away: the second label goes a line down; one by you goes past your arrow.
        let mut taken = vec![Rect::from_center_size(Pos2::new(150.0, 150.0), Vec2::new(14.0, 18.0))];
        assert_eq!(place_label(Pos2::new(250.0, 100.0), size, bounds, 9.0, &mut taken), Pos2::new(161.0, 92.0));
        assert_eq!(place_label(Pos2::new(250.0, 100.0), size, bounds, 9.0, &mut taken), Pos2::new(161.0, 110.0));
        assert_eq!(place_label(Pos2::new(140.0, 150.0), size, bounds, 9.0, &mut taken), Pos2::new(149.0, 160.0));
    }

    #[test]
    fn rings_and_distances() {
        assert_eq!(ring_step(300.0), 100.0);
        assert_eq!(ring_step(3_500.0), 1_000.0);
        assert_eq!(distance_text(850.4), "850 m");
        assert_eq!(distance_text(1_234.0), "1.2 km");
        assert_eq!(distance_text(48_200.0), "48 km");
    }
}
