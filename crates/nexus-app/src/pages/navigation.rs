//! Navigation: the in-game route plus the Spansh travel planners (exact, neutron, carrier).
//! Scan tours are planned where they're used (Exploration › Find valuable bodies, Exobiology ›
//! Find planets) and land here. Whatever route is active drives the "next system → clipboard"
//! automation. The fleet carrier planner also shows where the carrier is, routes the ship to it,
//! and finds the nearest Fleet Carrier Administration.

use crate::jobs::Slot;
use crate::theme::{display, pal, size};
use crate::tours::limits;
use crate::widgets::{self, Choice, GAP, Steps, badge, card, card_tinted, cards, empty, short_credits, th, toggle};
use egui::{RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use nexus_data::exploration::{is_scoopable, star_description};
use nexus_galaxy::{GalaxyDb, Services, StationQuery};
use nexus_journal::Timestamp;
use nexus_model::carrier::CarrierKind;
use nexus_model::location::distance;
use nexus_model::GameState;
use nexus_spansh::{CarrierRouteRequest, GalaxyAlgorithm, GalaxyRouteRequest, NeutronRouteRequest, RouteWaypoints, ServiceQuery, ShipProfile, Spansh};

/// A route waypoint, whichever planner produced it.
#[derive(Debug, Clone)]
pub struct Waypoint {
    pub system: String,
    pub pos: Option<[f64; 3]>,
    pub star_class: Option<String>,
    pub jumps: u32,
    pub neutron: bool,
    pub refuel: bool,
    pub scoopable: Option<bool>,
    pub value: Option<u64>,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct PlannedRoute {
    pub kind: &'static str,
    pub summary: String,
    /// The route on Spansh's website; empty for tours planned here.
    pub web_url: String,
    pub waypoints: Vec<Waypoint>,
    /// Waypoints already reached.
    done: usize,
    /// The system we were in when progress was last checked.
    at: String,
}

impl PlannedRoute {
    pub fn new(kind: &'static str, summary: String, web_url: String, waypoints: Vec<Waypoint>) -> Self {
        PlannedRoute { kind, summary, web_url, waypoints, done: 0, at: String::new() }
    }

    /// Follows the commander along the route: arriving at a waypoint (or one further on) marks it
    /// reached. Only arrivals count, so a loop that starts and ends here isn't finished at once.
    pub fn advance(&mut self, here: &str) {
        if here.is_empty() || self.at.eq_ignore_ascii_case(here) {
            return;
        }
        self.at = here.to_string();
        if let Some(i) = self.waypoints[self.done.min(self.waypoints.len())..].iter().position(|w| w.system.eq_ignore_ascii_case(here)) {
            self.done += i + 1;
        }
    }

    /// Waypoints still ahead.
    pub fn remaining(&self) -> &[Waypoint] {
        &self.waypoints[self.done.min(self.waypoints.len())..]
    }

    /// Every stop has been reached: the in-game route takes over.
    pub fn complete(&self) -> bool {
        self.remaining().is_empty() && !self.waypoints.is_empty()
    }

    /// The carrier a fleet carrier route was planned for.
    pub fn carrier(&self) -> Option<CarrierKind> {
        match self.kind {
            "Carrier" => Some(CarrierKind::Personal),
            "Squadron carrier" => Some(CarrierKind::Squadron),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Planner {
    #[default]
    InGame,
    Neutron,
    Exact,
    Carrier,
}

/// Pages the Navigation page can send the commander to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goto {
    Exploration,
    Exobiology,
}

/// What the Fleet Carrier Administration list is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Near {
    #[default]
    Carrier,
    Me,
}

/// Spansh's name for the station service.
const ADMIN: &str = "Fleet Carrier Administration";
/// Stations in the list.
const ADMIN_LIST: usize = 8;

/// The nearest stations with Fleet Carrier Administration, as found.
#[derive(Debug, Clone)]
struct AdminList {
    /// What the list was searched around.
    key: (Near, String),
    /// Nearest first.
    stations: Vec<AdminStation>,
    /// How many stations Spansh lists with the service; None when the list came from the local database.
    total: Option<u64>,
    /// Spansh couldn't be reached, and why: the list came from the local database.
    offline: Option<String>,
}

#[derive(Debug, Clone)]
struct AdminStation {
    name: String,
    system: String,
    /// From where the list was measured, ly.
    distance: Option<f64>,
    kind: Option<String>,
    large_pad: Option<bool>,
}

#[derive(Default)]
struct AdminSearch {
    near: Near,
    /// What the list was searched around; another place searches again.
    key: Option<(Near, String)>,
    job: Slot<Result<AdminList, String>>,
    /// The station picked in the list (by name and system); the nearest until another is clicked.
    picked: Option<(String, String)>,
}

pub struct NavUi {
    pub planner: Planner,
    /// Where a ship route starts: the current system, until something else is typed.
    pub from: String,
    /// Where a ship route goes.
    pub to: String,
    /// Where a carrier route starts (where the carrier is, until something else is typed) and where it goes.
    pub carrier_from: String,
    pub carrier_to: String,
    /// What the From fields were last filled with: a field that still holds it follows along.
    auto_from: String,
    auto_carrier_from: String,
    pub range: f64,
    pub efficiency: u8,
    /// Exact plotter options (as on spansh.co.uk/exact-plotter).
    pub exact: ExactOptions,
    pub job: Slot<Result<PlannedRoute, String>>,
    pub planned: Option<PlannedRoute>,
    /// A link to another page was clicked.
    pub goto: Option<Goto>,
    /// A route asked for with one click ("Route my ship here"), for this planner: plotted as soon as no other plot
    /// runs, and dropped if another planner is picked first.
    pub autoplot: Option<Planner>,
    /// The one-click route was planned with Neutron, the ship's loadout not being known.
    fell_back: bool,
    /// The ship's range `range` was last set to: while it holds that, it follows the ship you fly.
    auto_range: f64,
    /// The nearest Fleet Carrier Administration (fleet carrier planner).
    admin: AdminSearch,
    /// Route the ship to the carrier on the first frame (screenshot runs: `--tab route-carrier`).
    route_to_carrier: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ExactOptions {
    pub algorithm: GalaxyAlgorithm,
    pub use_supercharge: bool,
    pub use_injections: bool,
    pub exclude_secondary: bool,
    pub refuel_every_scoopable: bool,
    pub already_supercharged: bool,
    /// Fuel the route must leave in the tank, t.
    pub reserve: f64,
}

impl Default for ExactOptions {
    fn default() -> Self {
        ExactOptions {
            algorithm: GalaxyAlgorithm::Optimistic,
            use_supercharge: true,
            use_injections: false,
            exclude_secondary: false,
            refuel_every_scoopable: true,
            already_supercharged: false,
            reserve: 0.0,
        }
    }
}

impl NavUi {
    /// `--tab carrier` (or `exact`, `neutron`) opens that planner (for screenshots); `route-carrier` presses Route my
    /// ship here on the first frame.
    pub fn new(tab: Option<&str>) -> Self {
        let planner = match tab {
            Some("carrier" | "route-carrier") => Planner::Carrier,
            Some("exact") => Planner::Exact,
            Some("neutron") => Planner::Neutron,
            _ => Planner::InGame,
        };
        NavUi { planner, route_to_carrier: tab == Some("route-carrier"), ..Default::default() }
    }

    /// Plots a route for the current ship from where it is to `system` at once: the exact plotter, or the neutron
    /// planner when the ship's loadout isn't known yet.
    pub fn route_ship_to(&mut self, system: &str) {
        self.pick(Planner::Exact);
        self.to = system.to_string();
        self.from.clone_from(&self.auto_from);
        self.autoplot = Some(Planner::Exact);
    }

    /// Plots the carrier's jumps from where it is to `system` at once.
    fn route_carrier_to(&mut self, system: &str) {
        self.pick(Planner::Carrier);
        self.carrier_to = system.to_string();
        self.carrier_from.clone_from(&self.auto_carrier_from);
        self.autoplot = Some(Planner::Carrier);
    }

    /// Shows another planner: a one-click route still waiting for it is dropped.
    pub fn pick(&mut self, planner: Planner) {
        if planner != self.planner {
            self.autoplot = None;
            self.fell_back = false;
        }
        self.planner = planner;
    }
}

impl Default for NavUi {
    fn default() -> Self {
        NavUi {
            planner: Planner::InGame,
            from: String::new(),
            to: String::new(),
            carrier_from: String::new(),
            carrier_to: String::new(),
            auto_from: String::new(),
            auto_carrier_from: String::new(),
            range: 0.0,
            efficiency: 60,
            exact: ExactOptions::default(),
            job: Slot::default(),
            planned: None,
            goto: None,
            autoplot: None,
            fell_back: false,
            auto_range: 0.0,
            admin: AdminSearch::default(),
            route_to_carrier: false,
        }
    }
}

/// Keeps a field filled in automatically: while it holds what it was last given, it follows `now`; once something
/// else is typed (or it's emptied), it's left alone.
fn follow(field: &mut String, last: &mut String, now: &str) {
    if *field == *last && *last != now {
        *field = now.to_string();
    }
    if *last != now {
        *last = now.to_string();
    }
}

fn algorithm_label(a: GalaxyAlgorithm) -> &'static str {
    match a {
        GalaxyAlgorithm::Optimistic => "Optimistic (fastest)",
        GalaxyAlgorithm::Pessimistic => "Pessimistic",
        GalaxyAlgorithm::Guided => "Neutron-guided",
        GalaxyAlgorithm::Fuel => "Least fuel",
        GalaxyAlgorithm::FuelJumps => "Least fuel, fewer jumps",
    }
}

pub(crate) fn from_spansh(w: &nexus_spansh::Waypoint) -> Waypoint {
    let mut note = w.note.clone().unwrap_or_default();
    if !w.bodies.is_empty() {
        if !note.is_empty() {
            note.push_str(" · ");
        }
        note.push_str(&w.bodies.join(", "));
    }
    Waypoint {
        system: w.system.clone(),
        pos: w.coords.map(|c| [c.x, c.y, c.z]),
        star_class: None,
        jumps: w.jumps,
        neutron: w.neutron,
        refuel: w.refuel,
        scoopable: w.scoopable,
        value: w.value,
        note,
    }
}

/// Remaining waypoints of the active route (planner route if set and not finished, else the in-game route).
pub fn active_route(gs: &GameState, planned: Option<&PlannedRoute>) -> Vec<Waypoint> {
    if let Some(p) = planned
        && !p.complete()
    {
        return p.remaining().to_vec();
    }
    gs.route_remaining()
        .iter()
        .map(|h| Waypoint {
            system: h.star_system.clone(),
            pos: Some(h.star_pos),
            star_class: Some(h.star_class.clone()),
            jumps: 1,
            neutron: h.star_class == "N",
            refuel: false,
            scoopable: Some(is_scoopable(&h.star_class)),
            value: None,
            note: String::new(),
        })
        .collect()
}

/// Returns true if a setting changed. `carrier` is the carrier the fleet carrier planner uses, for
/// commanders with their own and a squadron carrier. `db` is the local galaxy database, if it opened.
#[allow(clippy::too_many_arguments)]
pub fn show(ui: &mut Ui, gs: &GameState, st: &mut NavUi, auto_copy: &mut bool, carrier: &mut CarrierKind, spansh: &Spansh, db: Option<&GalaxyDb>, rt: &tokio::runtime::Handle) -> bool {
    if st.job.tick()
        && let Some(Ok(route)) = &st.job.last
    {
        st.planned = Some(route.clone());
    }
    if st.admin.job.tick() {
        st.admin.picked = None;
    }
    let now = Timestamp::now();
    // Ship routes start where you are, carrier routes where the carrier is, until something else is typed.
    follow(&mut st.from, &mut st.auto_from, &gs.location.system);
    let picked = crate::pages::carrier::pick(gs, *carrier, now);
    let carrier_at = picked.and_then(|k| gs.carrier_of(k).system.clone()).unwrap_or_else(|| gs.location.system.clone());
    follow(&mut st.carrier_from, &mut st.auto_carrier_from, &carrier_at);
    if std::mem::take(&mut st.route_to_carrier)
        && let Some(sys) = picked.and_then(|k| gs.carrier_of(k).system.clone())
    {
        st.route_ship_to(&sys);
    }
    crate::tours::follow_range(&mut st.range, &mut st.auto_range, gs);
    let mut changed = false;
    // The whole page scrolls (the route table draws only the rows in view).
    egui::ScrollArea::vertical().id_salt("navigation").auto_shrink([false, false]).show(ui, |ui| {
        changed = page(ui, gs, st, auto_copy, carrier, spansh, db, rt);
    });
    changed
}

#[allow(clippy::too_many_arguments)]
fn page(ui: &mut Ui, gs: &GameState, st: &mut NavUi, auto_copy: &mut bool, carrier: &mut CarrierKind, spansh: &Spansh, db: Option<&GalaxyDb>, rt: &tokio::runtime::Handle) -> bool {
    let mut changed = false;
    // Each planner says what it's for, so the choice is made knowing the difference.
    let ship = gs.fleet.current().map(|s| s.type_name.clone()).filter(|n| !n.is_empty());
    let exact = match &ship {
        Some(s) => format!("Every jump for your {s}: its FSD, fuel and cargo, where to scoop. Best for most trips."),
        None => "Every jump for your ship: its FSD, fuel and cargo, where to scoop. Best for most trips.".to_string(),
    };
    let options = [
        Choice { value: Planner::InGame, icon: icon::NAVIGATION_ARROW, title: "In-game route", detail: "The route plotted in the game's galaxy map, or a tour you follow." },
        Choice { value: Planner::Exact, icon: icon::PATH, title: "Exact plotter", detail: &exact },
        Choice { value: Planner::Neutron, icon: icon::LIGHTNING, title: "Neutron", detail: "Long trips: hops between neutron stars to boost your jump range. Needs only the range; fuel isn't planned." },
        Choice { value: Planner::Carrier, icon: icon::ANCHOR, title: "Fleet carrier", detail: "Where your carrier is and a route to it, its jumps and tritium, the nearest carrier administration." },
    ];
    if let Some(p) = widgets::choices(ui, st.planner, &options, 190.0) {
        st.pick(p);
    }
    ui.add_space(6.0);
    // Scan tours live with the activity they serve; their routes come back here.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new("Looking for something to scan? Plan a tour in").size(size::FOOTNOTE).color(pal::DIM));
        if ui.link(RichText::new("Exploration › Find valuable bodies").size(size::FOOTNOTE)).clicked() {
            st.goto = Some(Goto::Exploration);
        }
        ui.label(RichText::new("or").size(size::FOOTNOTE).color(pal::DIM));
        if ui.link(RichText::new("Exobiology › Find planets").size(size::FOOTNOTE)).clicked() {
            st.goto = Some(Goto::Exobiology);
        }
        ui.label(RichText::new("and it shows up here.").size(size::FOOTNOTE).color(pal::DIM));
    });
    ui.add_space(GAP);
    let now = Timestamp::now();
    let picked = crate::pages::carrier::pick(gs, *carrier, now);
    if st.planner == Planner::Carrier {
        changed |= carrier_panel(ui, gs, st, carrier, spansh, db, rt);
        ui.add_space(GAP);
    }
    if st.planner != Planner::InGame {
        planner_form(ui, gs, st, crate::pages::carrier::pick(gs, *carrier, now), spansh, rt);
        ui.add_space(GAP);
    }
    // The carrier's countdown or cooldown while one runs, and its route's progress: for the carrier picked for the
    // planner, else the one a carrier route is for.
    let timed = if st.planner == Planner::Carrier { picked } else { st.planned.as_ref().and_then(|p| p.carrier()) };
    if let Some(kind) = timed.filter(|k| gs.carrier_of(*k).active(now)) {
        let running = gs.carrier_of(kind).jump_timer(now).is_some();
        let route = st.planned.as_ref().is_some_and(|p| p.carrier() == Some(kind) && !p.remaining().is_empty());
        if running || route {
            carrier_timer(ui, gs, kind, st);
            ui.add_space(GAP);
        }
    }
    ui.horizontal_wrapped(|ui| {
        changed |= toggle(ui, auto_copy, "Copy the next system to the clipboard after every jump").changed();
        ui.add_space(12.0);
        let route = active_route(gs, st.planned.as_ref());
        if let Some(next) = route.first()
            && widgets::button(ui, &format!("{} Copy {}", icon::COPY, next.system)).clicked()
        {
            ui.ctx().copy_text(next.system.clone());
        }
        if st.planned.is_some() && widgets::button(ui, &format!("{} Back to the in-game route", icon::ARROW_U_UP_LEFT)).clicked() {
            st.planned = None;
        }
    });
    ui.add_space(GAP);
    route_view(ui, gs, st);
    changed
}

/// The ship planners' and the carrier planner's form: from, to, plot, and every setting in view.
fn planner_form(ui: &mut Ui, gs: &GameState, st: &mut NavUi, carrier: Option<CarrierKind>, spansh: &Spansh, rt: &tokio::runtime::Handle) {
    // A one-click route without the ship's loadout: the exact plotter can't plan it, the neutron planner can.
    let loadout = gs.fleet.current().and_then(|s| s.loadout_json.as_ref()).is_some();
    if st.autoplot == Some(Planner::Exact) && st.planner == Planner::Exact && !loadout {
        st.pick(Planner::Neutron);
        st.autoplot = Some(Planner::Neutron);
        st.fell_back = true;
    }
    let for_carrier = st.planner == Planner::Carrier;
    let ship = gs.fleet.current().map(|s| s.display_name());
    // The exact plotter keeps at most a full main tank in reserve.
    let tank = gs.fleet.current().map(|s| s.fuel_main_capacity).filter(|t| *t > 0.0).unwrap_or(32.0);
    let title = if for_carrier { "Plan the carrier's jumps" } else { "" };
    card(ui, title, |ui| {
        // The simple surface: where from, where to, go.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            if for_carrier {
                field(ui, "From", &mut st.carrier_from, "Where your carrier is");
                ui.label(RichText::new(icon::ARROW_RIGHT).size(16.0).color(pal::DIM));
                field(ui, "To", &mut st.carrier_to, "Destination system");
            } else {
                field(ui, "From", &mut st.from, "Where you are");
                ui.label(RichText::new(icon::ARROW_RIGHT).size(16.0).color(pal::DIM));
                field(ui, "To", &mut st.to, "Destination system");
            }
            let busy = st.job.busy();
            let label = if busy { format!("{} Plotting…", icon::HOURGLASS) } else { "Plot route".to_string() };
            let clicked = ui.add_enabled_ui(!busy, |ui| widgets::primary_button(ui, &label)).inner.clicked();
            // A one-click route waits for a plot still running, and is for its own planner only.
            let auto = !busy && st.autoplot == Some(st.planner);
            if auto {
                st.autoplot = None;
            }
            if (clicked || auto) && !busy {
                match make_request(gs, st, carrier, spansh) {
                    Some(fut) => st.job.run_async(ui.ctx(), rt, fut),
                    None => {
                        let why = if st.planner == Planner::Exact && !loadout {
                            "The exact plotter needs your ship's loadout, which the game writes to its journal when you fly the ship. Until then, use Neutron."
                        } else {
                            "Type a destination system first."
                        };
                        st.job.last = Some(Err(why.into()));
                    }
                }
            }
            if busy {
                ui.spinner();
            }
        });
        // When to use this planner, and what it plans with.
        let hint = match st.planner {
            Planner::Exact => match &ship {
                Some(n) if loadout => format!("Plans every jump for {n} from its loadout: FSD, mass, fuel and cargo, with scooping stops and neutron boosts. Very long trips can take Spansh a while: Neutron is quicker for those."),
                _ => "Plans every jump from your ship's loadout, which the game writes to its journal when you fly the ship; it hasn't yet. Until then, use Neutron.".into(),
            },
            Planner::Neutron => {
                let instead = if st.fell_back { "Your ship's loadout isn't in the journal yet, so this route was planned here instead of with the exact plotter. " } else { "" };
                format!("{instead}Routes through neutron stars, whose jets supercharge the FSD for a much longer jump: the fewest jumps on long trips. Waypoints are a few jumps apart; the game's galaxy map plots the jumps between them, and fuel is up to you.")
            }
            Planner::Carrier => match carrier {
                Some(k) => {
                    let c = gs.carrier_of(k);
                    let squadron = if k == CarrierKind::Squadron { " Planned as a squadron carrier: 60,000 t capacity and 15,000 t mass, Spansh's figures." } else { "" };
                    let when = if c.stats_updated.is_zero() { String::new() } else { format!(", as carrier management last showed them ({})", widgets::ago(c.stats_updated)) };
                    format!("Plans {}'s jumps with its tritium ({} t in the tank) and cargo ({} t){when}.{squadron}", crate::pages::carrier::carrier_name(k, c), c.fuel, c.space.cargo)
                }
                None => "No carrier of yours in the journal: Spansh works out the tritium an empty fleet carrier needs.".into(),
            },
            Planner::InGame => String::new(),
        };
        if !hint.is_empty() {
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(hint).size(size::FOOTNOTE).color(pal::DIM)).wrap());
        }
        // Everything the Spansh website offers, in view.
        match st.planner {
            Planner::Neutron => {
                ui.add_space(10.0);
                widgets::settings_bar(ui, |bar| {
                    bar.field("Jump range", "Starts at your ship's range. Spansh plans with 10 to 100 ly.", |ui| {
                        widgets::number(ui, &mut st.range, limits::RANGE_LY, Steps::By(1.0), 1, " ly");
                    });
                    bar.field("Efficiency", "How far the route may stray to reach a neutron star: lower takes more detours for fewer jumps, higher keeps it straighter.", |ui| {
                        let mut e = u32::from(st.efficiency);
                        widgets::whole(ui, &mut e, 1..=100, Steps::By(5.0), " %");
                        st.efficiency = e.clamp(1, 100) as u8;
                    });
                });
            }
            Planner::Exact => {
                ui.add_space(10.0);
                let o = &mut st.exact;
                widgets::settings_bar(ui, |bar| {
                    bar.field("Routing", "Spansh's routing strategy: Optimistic is usually fastest; Least fuel avoids scooping; Neutron-guided follows a neutron route.", |ui| {
                        egui::ComboBox::from_id_salt("exact-algo").selected_text(algorithm_label(o.algorithm)).show_ui(ui, |ui| {
                            for a in [GalaxyAlgorithm::Optimistic, GalaxyAlgorithm::Pessimistic, GalaxyAlgorithm::Guided, GalaxyAlgorithm::Fuel, GalaxyAlgorithm::FuelJumps] {
                                ui.selectable_value(&mut o.algorithm, a, algorithm_label(a));
                            }
                        });
                    });
                    bar.field("Neutron boosts", "Supercharge the FSD at neutron stars on the way.", |ui| widgets::switch(ui, &mut o.use_supercharge));
                    bar.field("FSD injections", "Use FSD injections (synthesis) for longer jumps.", |ui| widgets::switch(ui, &mut o.use_injections));
                    bar.field("Skip secondary stars", "Don't plan stops at a system's secondary stars, which can be far from where you arrive.", |ui| widgets::switch(ui, &mut o.exclude_secondary));
                    bar.field("Refuel at every scoopable", "Top up at every scoopable star, not only when the tank runs low.", |ui| widgets::switch(ui, &mut o.refuel_every_scoopable));
                    bar.field("Already supercharged", "Your FSD is supercharged now: you've just boosted at a neutron star.", |ui| widgets::switch(ui, &mut o.already_supercharged));
                    bar.field("Fuel reserve", "Fuel to keep in the tank, up to a full main tank.", |ui| {
                        widgets::number(ui, &mut o.reserve, 0.0..=tank, Steps::By(1.0), 1, " t");
                    });
                });
            }
            Planner::Carrier | Planner::InGame => {}
        }
        if let Some(Err(e)) = &st.job.last {
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(format!("{} {e}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
        }
        ui.add_space(6.0);
        ui.label(RichText::new("Route data © Spansh (spansh.co.uk)").size(size::CAPTION).color(pal::FAINT));
    });
}

/// The fleet carrier planner's own cards: where the carrier is, with a route for the ship to it, and the nearest
/// Fleet Carrier Administration. Returns true when the carrier picked changed.
fn carrier_panel(ui: &mut Ui, gs: &GameState, st: &mut NavUi, carrier: &mut CarrierKind, spansh: &Spansh, db: Option<&GalaxyDb>, rt: &tokio::runtime::Handle) -> bool {
    let now = Timestamp::now();
    let mut changed = false;
    if gs.carriers(now).len() > 1 {
        changed |= crate::pages::carrier::carrier_switch(ui, gs, carrier, now);
        ui.add_space(GAP);
    }
    let kind = crate::pages::carrier::pick(gs, *carrier, now);
    cards(ui, 2, 340.0, |col| {
        col.add(|ui| carrier_card(ui, gs, st, kind));
        col.add(|ui| admin_card(ui, gs, st, kind, spansh, db, rt));
    });
    changed
}

/// Where a system is, from the commander's own travels (the galaxy database is asked off the UI thread, by the
/// administration search, when this knows nothing).
fn system_pos(gs: &GameState, address: u64) -> Option<[f64; 3]> {
    gs.history.visited.get(&address).and_then(|v| v.pos).or_else(|| gs.exploration.systems.get(&address).and_then(|s| s.star_pos))
}

/// The carrier's card: where it is, how far from you, and a route for your ship to it.
fn carrier_card(ui: &mut Ui, gs: &GameState, st: &mut NavUi, kind: Option<CarrierKind>) {
    let Some(kind) = kind else {
        card_tinted(ui, icon::ANCHOR, "Your carrier", pal::INDIGO, |ui| {
            ui.label(RichText::new("No fleet carrier in your journal").font(display(size::TITLE3)).color(pal::TEXT));
            note(ui, "Your carrier shows here once the game reports it: it does when you log in, and when you open the carrier management screen. The planner below still works without one: Spansh works out the tritium an empty carrier needs.");
        });
        return;
    };
    let c = gs.carrier_of(kind);
    let now = Timestamp::now();
    card_tinted(ui, icon::ANCHOR, kind.label(), pal::INDIGO, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(crate::pages::carrier::carrier_name(kind, c)).font(display(size::HEADLINE)).color(pal::TEXT));
            if !c.callsign.is_empty() && c.callsign != c.name {
                ui.label(RichText::new(&c.callsign).color(pal::DIM));
            }
        });
        let Some(sys) = c.system.clone() else {
            note(ui, "Where it is isn't in your journal yet: the game reports it when you log in, and after the carrier jumps.");
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::MAP_PIN).size(20.0).color(pal::INDIGO));
            if ui.link(RichText::new(&sys).font(display(size::TITLE)).color(pal::TEXT)).on_hover_text("Click to copy").clicked() {
                ui.ctx().copy_text(sys.clone());
            }
        });
        let here = c.system_address.map_or(sys.eq_ignore_ascii_case(&gs.location.system), |a| a == gs.location.system_address);
        let away = c.system_address.and_then(|a| system_pos(gs, a)).zip(gs.location.star_pos).map(|(a, b)| distance(a, b));
        let known = if c.located.is_zero() { String::new() } else { format!(" Last known position, {}.", widgets::ago(c.located)) };
        let squadron = if kind == CarrierKind::Squadron { " Other members may have moved it since: its place updates when the game next reports it." } else { "" };
        let whereabouts = match (here, away) {
            (true, _) => format!("You're in {sys} too.{known}{squadron}"),
            (false, Some(d)) => format!("{d:.1} ly from you (you're in {}).{known}{squadron}", gs.location.system),
            (false, None) => format!("You're in {}.{known}{squadron}", gs.location.system),
        };
        note(ui, &whereabouts);
        if let Some((line, colr)) = crate::pages::carrier::jump_timer_line(c, now) {
            ui.add(egui::Label::new(RichText::new(line).size(size::FOOTNOTE).color(colr)).wrap());
        }
        ui.add_space(8.0);
        // Scheduled to jump: going to where it will be may be the better trip.
        let leaving = c.pending_jump.as_ref().map(|p| p.system.clone()).filter(|s| !s.eq_ignore_ascii_case(&sys));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            if !here
                && widgets::primary_button(ui, &format!("{} Route my ship here", icon::NAVIGATION_ARROW))
                    .on_hover_text(format!("Plots a route from {} to {sys} for your ship: every jump with the exact plotter, or with Neutron while your ship's loadout isn't known.", gs.location.system))
                    .clicked()
            {
                st.route_ship_to(&sys);
            }
            if let Some(next) = &leaving
                && widgets::button(ui, &format!("{} Route to {next}", icon::ARROW_FAT_RIGHT)).on_hover_text("Where the carrier is jumping to.").clicked()
            {
                st.route_ship_to(next);
            }
            if widgets::button(ui, &format!("{} Copy", icon::COPY)).on_hover_text(format!("Copies {sys}, to paste into the galaxy map")).clicked() {
                ui.ctx().copy_text(sys.clone());
            }
        });
        // What the carrier planner below plans with.
        if !c.stats_updated.is_zero() {
            ui.add_space(14.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(28.0, 10.0);
                widgets::stat(ui, "Tritium in the tank", format!("{} t", c.fuel), pal::CYAN);
                widgets::stat(ui, "Cargo", format!("{} t", c.space.cargo), pal::TEXT);
                if c.jump_range_curr > 0.0 {
                    widgets::stat(ui, "Jump range now", format!("{:.0} ly", c.jump_range_curr), pal::TEXT);
                }
            });
            ui.add_space(2.0);
            let idle = if c.jump_timer(now).is_none() && kind == CarrierKind::Personal { " No jump scheduled." } else { "" };
            note(ui, &format!("From carrier management, {}.{idle}", widgets::ago(c.stats_updated)));
        }
    });
}

/// The nearest stations with Fleet Carrier Administration, near the carrier or near you, the nearest picked: a route
/// there for your ship or the carrier.
fn admin_card(ui: &mut Ui, gs: &GameState, st: &mut NavUi, kind: Option<CarrierKind>, spansh: &Spansh, db: Option<&GalaxyDb>, rt: &tokio::runtime::Handle) {
    let carrier = kind.map(|k| gs.carrier_of(k)).filter(|c| c.system.is_some());
    // Without a carrier whose place is known the list is measured from you; the switch keeps its own setting.
    let near = if carrier.is_some() { st.admin.near } else { Near::Me };
    let (reference, place) = match (near, carrier) {
        (Near::Carrier, Some(c)) => (c.system.clone().unwrap_or_default(), Place { pos: c.system_address.and_then(|a| system_pos(gs, a)), address: c.system_address }),
        _ => (gs.location.system.clone(), Place { pos: gs.location.star_pos, address: Some(gs.location.system_address) }),
    };
    // Searches when the card first shows, and again when the place it's measured from changes.
    let key = (near, reference.clone());
    if !reference.is_empty() && st.admin.key.as_ref() != Some(&key) && !st.admin.job.busy() {
        st.admin.key = Some(key.clone());
        st.admin.job.run_async(ui.ctx(), rt, find_admin(spansh.clone(), key.clone(), db.cloned(), place));
    }
    card_tinted(ui, icon::BUILDINGS, ADMIN, pal::TEAL, |ui| {
        if let Some(c) = carrier {
            let label = format!("Near {}", crate::pages::carrier::carrier_name(kind.unwrap_or_default(), c));
            widgets::segmented(ui, &mut st.admin.near, &[(Near::Carrier, label.as_str()), (Near::Me, "Near me")]);
            ui.add_space(6.0);
        }
        // A list searched around another place (you jumped, or the switch moved) waits for the new one.
        let current = matches!(&st.admin.job.last, Some(Ok(l)) if l.key == key);
        if st.admin.job.busy() || (!current && matches!(&st.admin.job.last, Some(Ok(_)))) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(format!("Asking Spansh for the stations nearest {reference}…")).size(size::FOOTNOTE).color(pal::DIM));
            });
        }
        // After an error or a local answer, the search can be asked again.
        let retry = |ui: &mut Ui, st: &mut NavUi| {
            if !st.admin.job.busy() && widgets::button(ui, &format!("{} Try Spansh again", icon::ARROWS_CLOCKWISE)).clicked() {
                st.admin.key = None;
            }
        };
        let list = match &st.admin.job.last {
            None => return,
            Some(Err(e)) => {
                ui.add(egui::Label::new(RichText::new(format!("{} {e}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
                retry(ui, st);
                return;
            }
            Some(Ok(_)) if !current => return,
            Some(Ok(list)) => list.clone(),
        };
        if list.stations.is_empty() {
            note(ui, match &list.offline {
                Some(_) => "Spansh couldn't be reached, and the local database knows no station with this service: it learns it from the Spansh galaxy import (Market › Database & feed).",
                None => "Spansh lists no station with this service.",
            });
            if list.offline.is_some() {
                retry(ui, st);
            }
            return;
        }
        // The nearest is picked until another is clicked.
        let picked = st.admin.picked.clone().and_then(|(n, s)| list.stations.iter().position(|a| a.name == n && a.system == s)).unwrap_or(0);
        for (i, a) in list.stations.iter().enumerate() {
            if admin_row(ui, a, i == picked).clicked() {
                st.admin.picked = Some((a.name.clone(), a.system.clone()));
            }
        }
        let a = &list.stations[picked];
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            let here = a.system.eq_ignore_ascii_case(&gs.location.system);
            if !here
                && widgets::primary_button(ui, &format!("{} Route my ship there", icon::NAVIGATION_ARROW))
                    .on_hover_text(format!("Plots a route to {} for your ship: every jump with the exact plotter, or with Neutron while your ship's loadout isn't known.", a.system))
                    .clicked()
            {
                st.route_ship_to(&a.system);
            }
            let carrier_there = carrier.is_some_and(|c| c.system.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&a.system)));
            if !carrier_there
                && widgets::button(ui, &format!("{} Plan the carrier's jumps there", icon::ANCHOR))
                    .on_hover_text(format!("Plots the carrier's jumps to {} with its tritium and cargo.", a.system))
                    .clicked()
            {
                st.route_carrier_to(&a.system);
            }
            if widgets::button(ui, &format!("{} Copy", icon::COPY)).on_hover_text(format!("Copies {}, to paste into the galaxy map", a.system)).clicked() {
                ui.ctx().copy_text(a.system.clone());
            }
        });
        ui.add_space(4.0);
        if list.offline.is_some() {
            retry(ui, st);
        }
        let source = match (&list.offline, list.total) {
            (Some(why), _) => format!(
                "Spansh couldn't be reached ({why}), so these come from the local database, which learns this service from the Spansh galaxy import: it may miss stations or be out of date."
            ),
            (None, Some(total)) => format!(
                "The {} nearest of the {} stations Spansh lists with this service, by straight-line distance from {reference}. Spansh learns station services from players' game data, so a recent change may not show yet.",
                list.stations.len(),
                nexus_data::names::group_digits(total as i64)
            ),
            (None, None) => String::new(),
        };
        note(ui, &source);
    });
}

/// One station of the administration list: name and system, distance on the right; the picked one highlighted.
fn admin_row(ui: &mut Ui, a: &AdminStation, picked: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if picked {
            p.rect_filled(rect, egui::CornerRadius::same(8), pal::ACCENT.gamma_multiply(0.14));
        } else if resp.hovered() {
            p.rect_filled(rect, egui::CornerRadius::same(8), egui::Color32::from_white_alpha(10));
        }
        let right = a.distance.map(|d| format!("{} ly", widgets::grouped(d, 1))).unwrap_or_default();
        let right = p.layout_no_wrap(right, egui::FontId::proportional(size::CALLOUT), pal::DIM);
        let pad = if a.large_pad == Some(false) { " · no large pad" } else { "" };
        let kind = a.kind.as_deref().map(|k| format!(" · {k}")).unwrap_or_default();
        let mut job = egui::text::LayoutJob::default();
        job.append(&a.name, 0.0, egui::TextFormat::simple(display(size::CALLOUT), if picked { pal::ACCENT_SOFT } else { pal::TEXT }));
        job.append(&format!("  {}{kind}{pad}", a.system), 0.0, egui::TextFormat::simple(egui::FontId::proportional(size::FOOTNOTE), pal::DIM));
        job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - right.size().x - 30.0).max(40.0));
        let left = p.layout_job(job);
        p.galley(egui::pos2(rect.left() + 10.0, rect.center().y - left.size().y / 2.0), left, pal::TEXT);
        p.galley(egui::pos2(rect.right() - 10.0 - right.size().x, rect.center().y - right.size().y / 2.0), right, pal::DIM);
    }
    resp
}

/// Where the list is measured from, for the local database: the system's position when the commander's own travels
/// know it, else its address to look it up by.
#[derive(Debug, Clone, Copy)]
struct Place {
    pos: Option<[f64; 3]>,
    address: Option<u64>,
}

/// The nearest stations with Fleet Carrier Administration around the system in `key`: Spansh's station search, which
/// knows every station reported with the service; when Spansh can't be reached, the local database, which learns the
/// service from the Spansh galaxy import (a database fed only by journals and EDDN held no station with it, in
/// November 2026, though it held stations Spansh lists with it).
async fn find_admin(spansh: Spansh, key: (Near, String), db: Option<GalaxyDb>, place: Place) -> Result<AdminList, String> {
    let q = ServiceQuery::named(ADMIN, key.1.as_str()).include_carriers(false).size(ADMIN_LIST as u32);
    let why = match spansh.service_search(&q).await {
        Ok(r) => {
            let stations = r
                .results
                .iter()
                .map(|s| AdminStation { name: s.name.clone(), system: s.system_name.clone(), distance: s.distance, kind: s.station_type.clone(), large_pad: s.has_large_pad })
                .collect();
            return Ok(AdminList { key, stations, total: Some(r.count), offline: None });
        }
        Err(e) => e.to_string(),
    };
    let Some(db) = db else {
        return Err(format!("Spansh couldn't be reached ({why}), and the local database isn't open."));
    };
    // The position of the system the list is measured from, looked up here, off the UI thread, if need be.
    let origin = place.pos.or_else(|| place.address.and_then(|a| db.system_by_id(a).ok().flatten()).and_then(|s| s.pos));
    let Some(origin) = origin else {
        return Err(format!("Spansh couldn't be reached ({why}), and the local database doesn't know where {} is.", key.1));
    };
    let q = StationQuery { origin, services: Services::CARRIER_ADMINISTRATION, limit: ADMIN_LIST, ..Default::default() };
    let hits = db.stations_near(&q).map_err(|e| format!("Spansh couldn't be reached ({why}), and the local database failed: {e}"))?;
    let stations = hits
        .into_iter()
        .map(|h| AdminStation { name: h.station.name, system: h.station.system, distance: Some(h.distance_ly), kind: h.station.station_type, large_pad: h.station.max_pad.map(|p| p == 'L') })
        .collect();
    Ok(AdminList { key, stations, total: None, offline: Some(why) })
}

/// The carrier's jump countdown or cooldown next to the fleet carrier planner; with a route for
/// that carrier active, also the next leg and a rough time to the destination.
fn carrier_timer(ui: &mut Ui, gs: &GameState, kind: CarrierKind, st: &NavUi) {
    let c = gs.carrier_of(kind);
    let route = st.planned.as_ref().filter(|p| p.carrier() == Some(kind)).map(|p| p.remaining()).filter(|r| !r.is_empty());
    let title = format!("{} jump", kind.label());
    cards(ui, if route.is_some() { 2 } else { 1 }, 300.0, |col| {
        col.add(|ui| {
            card_tinted(ui, icon::TIMER, &title, pal::ACCENT, |ui| crate::pages::carrier::jump_timer(ui, kind, c));
        });
        let Some(route) = route else { return };
        col.add(|ui| {
            card_tinted(ui, icon::PATH, "Carrier route", pal::BLUE, |ui| {
                let jumps: u32 = route.iter().map(|w| w.jumps.max(1)).sum();
                let eta = c.route_estimate(Timestamp::now(), jumps);
                ui.label(RichText::new(format!("≈ {}", widgets::duration(eta))).font(display(size::TITLE)).color(pal::TEXT)).on_hover_text(
                    "An estimate, and a minimum: the countdown or cooldown running now, then a 15-minute countdown for each jump and a 5-minute cooldown between jumps (Frontier's figures). Busy servers set later departures and each jump takes a little time, so the trip usually takes longer.",
                );
                ui.add(egui::Label::new(RichText::new(format!("to {} · {jumps} {} left", route.last().map(|w| w.system.as_str()).unwrap_or("?"), if jumps == 1 { "jump" } else { "jumps" })).size(size::FOOTNOTE).color(pal::DIM)).wrap());
                ui.add_space(4.0);
                let next = &route[0];
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Next leg").color(pal::DIM));
                    if ui.link(RichText::new(&next.system).color(pal::ACCENT)).on_hover_text("Click to copy").clicked() {
                        ui.ctx().copy_text(next.system.clone());
                    }
                });
                if let Some(p) = c.pending_jump.as_ref().filter(|p| !p.system.eq_ignore_ascii_case(&next.system)) {
                    ui.add(egui::Label::new(RichText::new(format!("{} The scheduled jump goes to {}, not the route's next stop.", icon::WARNING, p.system)).size(size::FOOTNOTE).color(pal::WARN)).wrap());
                }
            });
        });
    });
}

/// A labelled text field: small caption above a rounded field.
fn field(ui: &mut Ui, label: &str, text: &mut String, hint: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.label(RichText::new(label).size(size::FOOTNOTE).color(pal::DIM));
        ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(230.0).margin(egui::Margin::symmetric(10, 6)));
    });
}

/// "1 jump", "3 jumps".
fn count(n: u64, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

/// Secondary text in a card.
fn note(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(size::FOOTNOTE).color(pal::DIM)).wrap());
}

type PlanFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<PlannedRoute, String>> + Send>>;

/// `carrier` is the carrier the fleet carrier planner plans for, if the commander has one.
fn make_request(gs: &GameState, st: &NavUi, carrier: Option<CarrierKind>, spansh: &Spansh) -> Option<PlanFuture> {
    let sp = spansh.clone();
    let (from, to, start) = match st.planner {
        Planner::Carrier => (&st.carrier_from, &st.carrier_to, carrier.and_then(|k| gs.carrier_of(k).system.clone()).unwrap_or_else(|| gs.location.system.clone())),
        _ => (&st.from, &st.to, gs.location.system.clone()),
    };
    let from = match from.trim() {
        "" => start,
        f => f.to_string(),
    };
    let to = to.trim().to_string();
    if from.is_empty() || to.is_empty() {
        return None;
    }
    match st.planner {
        Planner::InGame => None,
        Planner::Neutron => {
            let req = NeutronRouteRequest::new(from, to, st.range.clamp(*limits::RANGE_LY.start(), *limits::RANGE_LY.end())).efficiency(st.efficiency);
            Some(Box::pin(async move {
                let p = sp.neutron_route(&req).await.map_err(|e| e.to_string())?;
                let wps: Vec<Waypoint> = p.route.waypoints().iter().map(from_spansh).collect();
                let summary = format!("{} · {} · {}", count(wps.len() as u64, "waypoint"), count(p.route.total_jumps() as u64, "jump"), count(p.route.neutron_count() as u64, "neutron boost"));
                Ok(PlannedRoute::new("Neutron", summary, p.web_url, wps))
            }))
        }
        Planner::Exact => {
            let json = gs.fleet.current().and_then(|s| s.loadout_json.clone())?;
            let cargo = gs.inventory.cargo.total().max(0) as u32;
            let o = st.exact;
            Some(Box::pin(async move {
                let profile = ShipProfile::from_loadout_json(&json).map_err(|e| format!("Ship profile: {e:?}"))?;
                let req = GalaxyRouteRequest::new(from.as_str(), to.as_str(), &profile)
                    .cargo(cargo)
                    .algorithm(o.algorithm)
                    .use_supercharge(o.use_supercharge)
                    .use_injections(o.use_injections, false)
                    .exclude_secondary(o.exclude_secondary)
                    .refuel_every_scoopable(o.refuel_every_scoopable)
                    .already_supercharged(o.already_supercharged)
                    .reserve(o.reserve);
                let p = sp.galaxy_route(&req).await.map_err(|e| e.to_string())?;
                let wps: Vec<Waypoint> = p.route.waypoints().iter().map(from_spansh).collect();
                let summary = format!(
                    "{} · {:.0} ly · {} · {} · {:.1} t fuel",
                    count(p.route.jump_count() as u64, "jump"),
                    p.route.distance(),
                    count(p.route.refuel_stops() as u64, "refuel"),
                    count(p.route.neutron_boosts() as u64, "neutron boost"),
                    p.route.fuel_used()
                );
                Ok(PlannedRoute::new("Exact", summary, p.web_url, wps))
            }))
        }
        Planner::Carrier => {
            // With the picked carrier's tritium and cargo; without a carrier, Spansh works out the tritium needed.
            let req = match carrier {
                Some(k) => {
                    let c = gs.carrier_of(k);
                    let req = if k == CarrierKind::Squadron { CarrierRouteRequest::squadron(from.as_str(), [to.as_str()]) } else { CarrierRouteRequest::new(from.as_str(), [to.as_str()]) };
                    req.capacity_used(c.space.cargo.max(0) as u32).fuel_loaded(c.fuel.max(0) as u32, 0)
                }
                None => CarrierRouteRequest::new(from.as_str(), [to.as_str()]),
            };
            let kind = if carrier == Some(CarrierKind::Squadron) { "Squadron carrier" } else { "Carrier" };
            Some(Box::pin(async move {
                let p = sp.carrier_route(&req).await.map_err(|e| e.to_string())?;
                let wps: Vec<Waypoint> = p.route.waypoints().iter().map(from_spansh).collect();
                let summary = format!("{} · {:.0} ly · {} t tritium", count(p.route.jump_count() as u64, "carrier jump"), p.route.distance(), p.route.tritium_used());
                Ok(PlannedRoute::new(kind, summary, p.web_url, wps))
            }))
        }
    }
}

fn route_view(ui: &mut Ui, gs: &GameState, st: &NavUi) {
    let route = active_route(gs, st.planned.as_ref());
    if let Some(p) = &st.planned {
        ui.horizontal_wrapped(|ui| {
            badge(ui, p.kind, pal::CYAN);
            ui.label(RichText::new(&p.summary).font(display(size::HEADLINE)));
            if !p.web_url.is_empty() {
                ui.hyperlink_to(format!("{} Open on Spansh", icon::ARROW_SQUARE_OUT), &p.web_url);
            }
        });
        ui.add_space(8.0);
        if p.complete() {
            if route.is_empty() {
                empty(ui, icon::FLAG_CHECKERED, "Route complete", "You've reached every stop. Plot a route in the galaxy map and it shows here, or plan another one above.");
                return;
            }
            ui.add(egui::Label::new(RichText::new(format!("{} Tour complete, showing the route plotted in the game.", icon::FLAG_CHECKERED)).size(size::FOOTNOTE).color(pal::DIM)).wrap());
            ui.add_space(8.0);
        }
    } else if !route.is_empty() {
        // Where an in-game route comes from.
        ui.horizontal_wrapped(|ui| {
            badge(ui, "In-game", pal::CYAN);
            ui.label(RichText::new("The route plotted in the game's galaxy map").font(display(size::HEADLINE)));
        });
        ui.add(egui::Label::new(RichText::new("It follows the game: plot or clear a route there and this changes with it.").size(size::FOOTNOTE).color(pal::DIM)).wrap());
        ui.add_space(8.0);
    }
    if route.is_empty() {
        let hint = match st.planner {
            Planner::InGame => "This shows the route plotted in the game's galaxy map, and nothing is plotted there now. Plot one in the galaxy map and it appears here at once, or let a planner above plan one.",
            _ => "Plot one above, or in the game's galaxy map: either shows here.",
        };
        empty(ui, icon::NAVIGATION_ARROW, "No route plotted", hint);
        return;
    }
    let mut prev = gs.location.star_pos;
    let mut total = 0.0;
    let mut legs = Vec::with_capacity(route.len());
    for w in &route {
        let d = match (prev, w.pos) {
            (Some(a), Some(b)) => distance(a, b),
            _ => 0.0,
        };
        total += d;
        legs.push((d, total));
        if w.pos.is_some() {
            prev = w.pos;
        }
    }
    let scoopable = route.iter().filter(|w| w.scoopable == Some(true)).count();
    let neutrons = route.iter().filter(|w| w.neutron).count();
    let mut run = 0;
    let mut worst = 0;
    for w in &route {
        if w.scoopable == Some(false) {
            run += 1;
            worst = worst.max(run);
        } else {
            run = 0;
        }
    }
    let jumps: u32 = route.iter().map(|w| w.jumps.max(1)).sum();
    let big = |ui: &mut Ui, text: String, color: egui::Color32| {
        ui.add(egui::Label::new(RichText::new(text).font(display(size::TITLE3)).color(color)).wrap());
    };
    let note = |ui: &mut Ui, text: String, color: egui::Color32| {
        ui.add(egui::Label::new(RichText::new(text).size(size::FOOTNOTE).color(color)).wrap());
    };
    cards(ui, 4, 190.0, |c| {
        c.add(|ui| {
            card_tinted(ui, icon::FLAG_CHECKERED, "Destination", pal::TEAL, |ui| {
                big(ui, route.last().map(|w| w.system.clone()).unwrap_or_else(|| "?".into()), pal::TEXT);
            });
        });
        c.add(|ui| {
            card_tinted(ui, icon::PATH, "Remaining", pal::BLUE, |ui| {
                big(ui, format!("{total:.0} ly"), pal::TEXT);
                note(ui, format!("{} · {}", count(route.len() as u64, "waypoint"), count(u64::from(jumps), "jump")), pal::DIM);
            });
        });
        c.add(|ui| {
            card_tinted(ui, icon::GAS_PUMP, "Fuel stars", pal::GOOD, |ui| {
                big(ui, format!("{scoopable} scoopable"), pal::TEXT);
                if worst >= 3 {
                    note(ui, format!("{} {worst} non-scoopable in a row", icon::WARNING), pal::WARN);
                }
            });
        });
        c.add(|ui| {
            card_tinted(ui, icon::LIGHTNING, "Neutron stars", pal::CYAN, |ui| {
                big(ui, format!("{neutrons} on route"), if neutrons > 0 { pal::TEXT } else { pal::DIM });
            });
        });
    });
    ui.add_space(GAP);
    TableBuilder::new(ui)
        .id_salt("route")
        .striped(true)
        .resizable(false)
        .vscroll(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(44.0))
        .column(Column::initial(250.0).at_least(140.0).clip(true))
        .column(Column::initial(210.0).at_least(120.0).clip(true))
        .column(Column::initial(70.0))
        .column(Column::initial(90.0))
        .column(Column::remainder().at_least(120.0).clip(true))
        .header(30.0, |mut h| {
            for t in ["#", "System", "Star", "Jumps", "Distance", "Notes"] {
                h.col(|ui| th(ui, t));
            }
        })
        .body(|body| {
            body.rows(32.0, route.len(), |mut row| {
                let i = row.index();
                let w = &route[i];
                row.set_selected(i == 0);
                row.col(|ui| {
                    ui.label(RichText::new((i + 1).to_string()).color(pal::FAINT));
                });
                row.col(|ui| {
                    if ui.link(RichText::new(&w.system).color(if i == 0 { pal::ACCENT } else { pal::TEXT })).on_hover_text("Click to copy").clicked() {
                        ui.ctx().copy_text(w.system.clone());
                    }
                });
                row.col(|ui| {
                    ui.horizontal(|ui| {
                        if let Some(c) = &w.star_class {
                            let col = if is_scoopable(c) { pal::GOOD } else if matches!(c.as_str(), "N" | "H") || c.starts_with('D') { pal::WARN } else { pal::DIM };
                            ui.label(RichText::new(star_description(c)).color(col));
                        }
                        if w.neutron {
                            badge(ui, "neutron", pal::CYAN);
                        }
                        if w.refuel {
                            badge(ui, "refuel", pal::GOOD);
                        } else if w.scoopable == Some(true) && w.star_class.is_none() {
                            ui.label(RichText::new(icon::GAS_PUMP).color(pal::GOOD));
                        }
                    });
                });
                row.col(|ui| {
                    ui.label(RichText::new(w.jumps.max(1).to_string()).color(pal::DIM));
                });
                row.col(|ui| {
                    ui.label(RichText::new(format!("{:.1} ly", legs[i].0)).color(pal::DIM));
                });
                row.col(|ui| {
                    ui.horizontal(|ui| {
                        if let Some(v) = w.value.filter(|v| *v > 0) {
                            ui.label(RichText::new(short_credits(v as i64)).color(pal::GOLD));
                        }
                        if !w.note.is_empty() {
                            ui.add(egui::Label::new(RichText::new(&w.note).size(size::FOOTNOTE).color(pal::DIM)).truncate());
                        }
                    });
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_follows_until_something_else_is_typed() {
        let (mut field, mut last) = (String::new(), String::new());
        follow(&mut field, &mut last, "Leesti");
        assert_eq!(field, "Leesti", "filled in at first");
        follow(&mut field, &mut last, "Uszaa");
        assert_eq!(field, "Uszaa", "follows the jump");
        field = "Sol".into();
        follow(&mut field, &mut last, "Orrere");
        assert_eq!(field, "Sol", "typed: kept");
        field.clear();
        follow(&mut field, &mut last, "Orrere");
        assert_eq!(field, "", "emptied while typing: not filled in again under the cursor");
    }

    #[test]
    fn a_one_click_route_starts_where_you_are() {
        let mut st = NavUi::default();
        follow(&mut st.from, &mut st.auto_from, "Leesti");
        st.from = "Sol".into();
        st.route_ship_to("Orrere");
        assert_eq!((st.planner, st.from.as_str(), st.to.as_str(), st.autoplot), (Planner::Exact, "Leesti", "Orrere", Some(Planner::Exact)));
        follow(&mut st.carrier_from, &mut st.auto_carrier_from, "Orrere");
        st.route_carrier_to("Kruger 60");
        assert_eq!((st.planner, st.carrier_from.as_str(), st.carrier_to.as_str(), st.autoplot), (Planner::Carrier, "Orrere", "Kruger 60", Some(Planner::Carrier)));
        assert_eq!(st.to, "Orrere", "the ship's destination is its own");
        // Waiting for a plot still running, then another planner is picked: the one-click route is dropped.
        st.pick(Planner::Neutron);
        assert_eq!(st.autoplot, None);
        st.pick(Planner::Neutron);
        assert_eq!(st.planner, Planner::Neutron);
    }

    #[test]
    fn counts_read_as_words() {
        assert_eq!((count(1, "jump"), count(0, "refuel"), count(12, "neutron boost")), ("1 jump".into(), "0 refuels".into(), "12 neutron boosts".into()));
    }
}
