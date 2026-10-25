//! Mining: the live session and earlier ones. For Rhino surface mining, the planetary mining locations worked (with
//! the number to pick in the game's navigation panel to go back) and a radar of the deposits around you.

use crate::pages::galaxy_map::MapAction;
use crate::theme::{display, pal, size};
use crate::widgets::{self, badge, card, card_tinted, duration, empty, footnote};
use egui::{Align, Align2, Grid, Layout, Pos2, RichText, Sense, Stroke, Ui, Vec2};
use egui_phosphor::regular as icon;
use nexus_journal::Timestamp;
use nexus_model::location::{surface_bearing, surface_distance};
use nexus_model::mining::{MiningSession, SPOT_RADIUS_M, SiteKey, Spot, SurfaceSite};
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

/// Copies the deposits recorded this run (positions exist only live) into `prefs`; true when anything was added. A
/// deposit is matched by its place on the body, so one first kept under "location not named" takes the number when
/// a later landing names it.
pub fn remember(gs: &GameState, prefs: &mut MiningPrefs) -> bool {
    let mut changed = false;
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
                    });
                    changed = true;
                }
            }
        }
    }
    changed
}

fn same_place(d: &Deposit, spot: &Spot, radius_m: Option<f64>) -> bool {
    match radius_m {
        Some(r) => surface_distance((d.lat, d.lon), (spot.lat, spot.lon), r) <= SPOT_RADIUS_M,
        None => (d.lat - spot.lat).abs() < 1e-9 && (d.lon - spot.lon).abs() < 1e-9,
    }
}

/// Returns whether `prefs` changed (to be saved), and a system to plot a route to.
pub fn show(ui: &mut Ui, gs: &GameState, prefs: &mut MiningPrefs) -> (bool, Option<MapAction>) {
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
        card_tinted(ui, icon::CROSSHAIR, "Surface radar", pal::ACCENT, |ui| surface_radar(ui, &v));
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

const NOT_NAMED: &str = "The game names the mining location nearest to where your ship lands or lifts off. This time it named none: the ship set down far from every location.";

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
             The number is the location the game named nearest to where your ship landed or lifted off. Tonnes are what the Rhino's refinery made there, by-products included.",
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
        ui.label(RichText::new(line).size(size::CAPTION).color(pal::DIM)).on_hover_text("From your FSS or DSS scan of the body");
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

/// "13 mining locations on this body; you've worked 2", from the commander's own scans of it.
fn locations_line(gs: &GameState, site: &SurfaceSite) -> Option<String> {
    let total = gs.exploration.systems.get(&site.key.system_address)?.bodies.get(&site.key.body_id)?.mining_locations;
    if total == 0 {
        return None;
    }
    let worked = gs.mining.sites.keys().filter(|k| k.system_address == site.key.system_address && k.body_id == site.key.body_id && k.location.is_some()).count();
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
    let mut marks = Vec::new();
    let mut mark = |kind: MarkKind, pos: (f64, f64), tag: String, label: String, location: Option<u32>| {
        marks.push(Mark { kind, tag, label, dist_m: surface_distance(me, pos, radius_m), bearing: surface_bearing(me, pos), location });
    };
    for d in prefs.deposits.iter().filter(|d| on_body(d.key())) {
        let tag = d.commodities.first().cloned().unwrap_or_default();
        mark(MarkKind::Deposit, (d.lat, d.lon), tag, format!("{} deposit{}", d.commodities.join(", "), at_location(d.location)), d.location);
    }
    let ship = gs.mining.here.as_ref().filter(|h| on_body(h.key)).and_then(|h| h.ship);
    if let Some(p) = ship {
        mark(MarkKind::Ship, p, "Ship".into(), "Your ship".into(), None);
    }
    for site in gs.mining.sites.values().filter(|s| on_body(s.key)) {
        if let Some(&p) = site.landings.last() && Some(p) != ship {
            mark(MarkKind::Landing, p, "Landed".into(), format!("Where your ship landed last time{}", at_location(site.key.location)), site.key.location);
        }
    }
    (!marks.is_empty()).then(|| RadarView { heading: st.heading.unwrap_or(0.0), marks })
}

/// The radar and the distances and bearings under it (Mining page).
pub fn surface_radar(ui: &mut Ui, v: &RadarView) {
    radar(ui, v, 320.0);
    ui.add_space(6.0);
    radar_list(ui, v, 8);
    footnote(
        ui,
        "Up is the way you're facing; turn until your heading shows the bearing. Deposits are where the Rhino refined while Elite Nexus was running \
         (the journal records no positions). Rigs aren't in the journal, so they can't be shown.",
    );
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
    let far = v.marks.iter().map(|m| m.dist_m).fold(0.0, f64::max);
    let max_d = (far * 1.15).max(250.0);
    let scale = f64::from(r_px) / max_d;
    let step = ring_step(max_d);
    let mut ring = step;
    while ring < max_d {
        p.circle_stroke(c, (ring * scale) as f32, Stroke::new(1.0, pal::BORDER.gamma_multiply(0.6)));
        ring += step;
    }
    // Screen position of a point `dist` metres away on a compass `bearing`, turned so the heading points up.
    let at = |dist: f64, bearing: f64| -> Pos2 {
        let a = (bearing - v.heading).to_radians();
        Pos2::new(c.x + (a.sin() * dist * scale) as f32, c.y - (a.cos() * dist * scale) as f32)
    };
    p.text(at(max_d * 0.92, 0.0), Align2::CENTER_CENTER, "N", display(size::CAPTION), pal::DIM);
    for m in &v.marks {
        let sp = at(m.dist_m, m.bearing);
        match m.kind {
            MarkKind::Deposit => {
                let diamond = [sp + Vec2::new(0.0, -7.0), sp + Vec2::new(6.0, 0.0), sp + Vec2::new(0.0, 7.0), sp + Vec2::new(-6.0, 0.0)];
                p.add(egui::Shape::convex_polygon(diamond.to_vec(), pal::ACCENT, Stroke::NONE));
            }
            MarkKind::Ship => {
                p.circle_filled(sp, 6.0, pal::CYAN);
            }
            MarkKind::Landing => {
                p.circle_stroke(sp, 5.0, Stroke::new(1.5, pal::DIM));
            }
        }
        p.text(sp + Vec2::new(9.0, -6.0), Align2::LEFT_BOTTOM, &m.tag, display(size::CAPTION), pal::TEXT);
    }
    let me = [c + Vec2::new(0.0, -9.0), c + Vec2::new(6.0, 7.0), c + Vec2::new(-6.0, 7.0)];
    p.add(egui::Shape::convex_polygon(me.to_vec(), pal::TEXT, Stroke::NONE));
    p.text(
        rect.left_top() + Vec2::new(6.0, 4.0),
        Align2::LEFT_TOP,
        format!("{} · rings every {}", distance_text(max_d), distance_text(step)),
        egui::FontId::proportional(size::CAPTION),
        pal::DIM,
    );
}

/// Nearest first, at most `rows`: what each mark is, how far, and the compass bearing to drive or fly.
fn radar_list(ui: &mut Ui, v: &RadarView, rows: usize) {
    let mut marks: Vec<&Mark> = v.marks.iter().collect();
    marks.sort_by(|a, b| a.dist_m.total_cmp(&b.dist_m));
    for m in marks.iter().take(rows) {
        let (glyph, col) = match m.kind {
            MarkKind::Deposit => (icon::DIAMOND, pal::ACCENT),
            MarkKind::Ship => (icon::ROCKET, pal::CYAN),
            MarkKind::Landing => (icon::MAP_PIN, pal::DIM),
        };
        let bearing = (m.bearing.round() as i64).rem_euclid(360);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(glyph).color(col));
            ui.label(RichText::new(&m.label).color(pal::TEXT));
            ui.label(RichText::new(format!("{} · bearing {bearing:03}°", distance_text(m.dist_m))).color(pal::DIM));
        });
    }
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
        site.spots.push(Spot { lat, lon: 20.0, refined: [(commodity.to_string(), 1)].into(), found: Timestamp(1) });
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

    #[test]
    fn a_deposit_takes_the_number_a_later_landing_names() {
        let mut gs = GameState::default();
        let mut prefs = MiningPrefs::default();
        let unnamed = SiteKey { system_address: 100, body_id: 1, location: None };
        let site = gs.mining.sites.entry(unnamed).or_insert_with(|| SurfaceSite::new(unnamed, "Alpha", "Alpha 1"));
        site.radius_m = Some(1_000_000.0);
        site.spots.push(Spot { lat: 10.0, lon: 20.0, refined: [("Iridium".to_string(), 1)].into(), found: Timestamp(1) });
        remember(&gs, &mut prefs);
        gs.mining.sites.clear();
        site_with_spot(&mut gs, 10.0001, "Iridium");
        assert!(remember(&gs, &mut prefs));
        assert_eq!((prefs.deposits.len(), prefs.deposits[0].location), (1, Some(4)), "one deposit, now with its number");
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

    #[test]
    fn rings_and_distances() {
        assert_eq!(ring_step(300.0), 100.0);
        assert_eq!(ring_step(3_500.0), 1_000.0);
        assert_eq!(distance_text(850.4), "850 m");
        assert_eq!(distance_text(1_234.0), "1.2 km");
        assert_eq!(distance_text(48_200.0), "48 km");
    }
}
