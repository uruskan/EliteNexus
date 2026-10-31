//! Navigation: the in-game route plus the Spansh travel planners (neutron, exact, carrier).
//! Scan tours are planned where they're used (Exploration › Find valuable bodies, Exobiology ›
//! Find planets) and land here. Whatever route is active drives the "next system → clipboard"
//! automation.

use crate::jobs::Slot;
use crate::theme::{display, pal, size};
use crate::tours::limits;
use crate::widgets::{self, GAP, badge, card, card_tinted, cards, empty, short_credits, th, toggle};
use egui::{RichText, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use nexus_data::exploration::{is_scoopable, star_description};
use nexus_journal::Timestamp;
use nexus_model::carrier::CarrierKind;
use nexus_model::location::distance;
use nexus_model::GameState;
use nexus_spansh::{CarrierRouteRequest, GalaxyAlgorithm, GalaxyRouteRequest, NeutronRouteRequest, RouteWaypoints, ShipProfile, Spansh};

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

pub struct NavUi {
    pub planner: Planner,
    pub from: String,
    pub to: String,
    pub range: f64,
    pub efficiency: u8,
    /// Exact plotter options (as on spansh.co.uk/exact-plotter).
    pub exact: ExactOptions,
    /// The planner's advanced options are shown (hidden by default).
    pub show_options: bool,
    pub job: Slot<Result<PlannedRoute, String>>,
    pub planned: Option<PlannedRoute>,
    /// A link to another page was clicked.
    pub goto: Option<Goto>,
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
    /// `--tab carrier` opens the fleet carrier planner (for screenshots).
    pub fn new(tab: Option<&str>) -> Self {
        NavUi { planner: if tab == Some("carrier") { Planner::Carrier } else { Planner::InGame }, ..Default::default() }
    }
}

impl Default for NavUi {
    fn default() -> Self {
        NavUi {
            planner: Planner::InGame,
            from: String::new(),
            to: String::new(),
            range: 0.0,
            efficiency: 60,
            exact: ExactOptions::default(),
            show_options: false,
            job: Slot::default(),
            planned: None,
            goto: None,
        }
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
        && !planned_complete(p)
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

/// A planned route whose every stop has been reached: the in-game route takes over.
fn planned_complete(p: &PlannedRoute) -> bool {
    p.remaining().is_empty() && !p.waypoints.is_empty()
}

/// Returns true if a setting changed. `carrier` is the carrier the fleet carrier planner uses, for
/// commanders with their own and a squadron carrier.
pub fn show(ui: &mut Ui, gs: &GameState, st: &mut NavUi, auto_copy: &mut bool, carrier: &mut CarrierKind, spansh: &Spansh, rt: &tokio::runtime::Handle) -> bool {
    let mut changed = false;
    if st.job.tick()
        && let Some(Ok(route)) = &st.job.last {
            st.planned = Some(route.clone());
        }
    if st.from.is_empty() {
        st.from = gs.location.system.clone();
    }
    if st.range <= 0.0 {
        st.range = crate::tours::jump_range(gs);
    }
    if let Some(p) = widgets::tabs(ui, st.planner, &[(Planner::InGame, "In-game route"), (Planner::Neutron, "Neutron"), (Planner::Exact, "Exact plotter"), (Planner::Carrier, "Fleet carrier")]) {
        st.planner = p;
    }
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
    if st.planner != Planner::InGame {
        changed |= planner_form(ui, gs, st, carrier, spansh, rt);
        ui.add_space(GAP);
    }
    // The timer follows the carrier picked for the planner, else the one a carrier route is for.
    let now = Timestamp::now();
    let timed = if st.planner == Planner::Carrier { crate::pages::carrier::pick(gs, *carrier, now) } else { st.planned.as_ref().and_then(|p| p.carrier()) };
    if let Some(kind) = timed.filter(|k| gs.carrier_of(*k).active(now)) {
        carrier_timer(ui, gs, kind, st);
        ui.add_space(GAP);
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

/// Returns true when the carrier picked for the carrier planner changed.
fn planner_form(ui: &mut Ui, gs: &GameState, st: &mut NavUi, carrier: &mut CarrierKind, spansh: &Spansh, rt: &tokio::runtime::Handle) -> bool {
    let mut changed = false;
    let now = Timestamp::now();
    let ship_name = gs.fleet.current().map(|s| s.display_name());
    // Smart defaults: start here, with the current ship's jump range.
    if st.range <= 0.0 {
        st.range = crate::tours::jump_range(gs);
    }
    // The exact plotter keeps at most a full main tank in reserve.
    let tank = gs.fleet.current().map(|s| s.fuel_main_capacity).filter(|t| *t > 0.0).unwrap_or(32.0);
    card(ui, "", |ui| {
        // The simple surface: where from, where to, go.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            field(ui, "From", &mut st.from, gs.location.system.as_str());
            if matches!(st.planner, Planner::Neutron | Planner::Exact | Planner::Carrier) {
                ui.label(RichText::new(icon::ARROW_RIGHT).size(16.0).color(pal::DIM));
                field(ui, "To", &mut st.to, "Destination system");
            }
            let busy = st.job.busy();
            let label = if busy { format!("{} Plotting…", icon::HOURGLASS) } else { "Plot route".to_string() };
            if ui.add_enabled_ui(!busy, |ui| widgets::primary_button(ui, &label)).inner.clicked() {
                match make_request(gs, st, crate::pages::carrier::pick(gs, *carrier, now), spansh) {
                    Some(fut) => st.job.run_async(ui.ctx(), rt, fut),
                    None => st.job.last = Some(Err("This planner needs your ship's loadout (fly it once) and a destination.".into())),
                }
            }
            if busy {
                ui.spinner();
            }
        });
        if st.planner == Planner::Carrier && gs.carriers(now).len() > 1 {
            ui.add_space(4.0);
            changed |= crate::pages::carrier::carrier_switch(ui, gs, carrier, now);
        }
        let hint = match st.planner {
            Planner::Exact => ship_name.clone().map(|n| format!("Plans every jump for {n}: its real FSD, mass, fuel and cargo, scooping stops and neutron boosts.")),
            Planner::Neutron => Some(format!("Follows neutron stars with a {:.1} ly jump range.", st.range)),
            Planner::Carrier => Some(match crate::pages::carrier::pick(gs, *carrier, now) {
                Some(k) => {
                    let c = gs.carrier_of(k);
                    let squadron = if k == CarrierKind::Squadron { " Planned as a squadron carrier: 60,000 t capacity and 15,000 t mass, Spansh's figures." } else { "" };
                    format!("{}: tritium in the tank {} t · cargo {} t.{squadron}", crate::pages::carrier::carrier_name(k, c), c.fuel, c.space.cargo)
                }
                None => "No carrier of yours in the journal: Spansh works out the tritium an empty fleet carrier needs.".into(),
            }),
            Planner::InGame => None,
        };
        if let Some(h) = hint {
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(h).size(size::FOOTNOTE).color(pal::DIM)).wrap());
        }
        ui.add_space(4.0);
        // The hidden depth: everything the Spansh website offers, collapsed by default.
        if widgets::disclosure(ui, &mut st.show_options, "Options") {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| match st.planner {
                Planner::Neutron => {
                    ui.label("Range");
                    ui.add(egui::DragValue::new(&mut st.range).range(limits::RANGE_LY).suffix(" ly")).on_hover_text("Spansh plans with jump ranges from 10 to 100 ly.");
                    ui.label("Efficiency");
                    ui.add(egui::DragValue::new(&mut st.efficiency).range(1..=100).suffix(" %"))
                        .on_hover_text("Lower = more neutron detours for fewer jumps; higher = straighter route.");
                }
                Planner::Exact => {
                    let o = &mut st.exact;
                    egui::ComboBox::from_id_salt("exact-algo")
                        .selected_text(algorithm_label(o.algorithm))
                        .show_ui(ui, |ui| {
                            for a in [GalaxyAlgorithm::Optimistic, GalaxyAlgorithm::Pessimistic, GalaxyAlgorithm::Guided, GalaxyAlgorithm::Fuel, GalaxyAlgorithm::FuelJumps] {
                                ui.selectable_value(&mut o.algorithm, a, algorithm_label(a));
                            }
                        })
                        .response
                        .on_hover_text("Spansh routing strategy: Optimistic is usually fastest; Least fuel avoids scooping; Neutron-guided follows a neutron route.");
                    ui.end_row();
                    toggle(ui, &mut o.use_supercharge, "Neutron boosts");
                    toggle(ui, &mut o.use_injections, "FSD injections");
                    toggle(ui, &mut o.exclude_secondary, "Skip secondary stars");
                    toggle(ui, &mut o.refuel_every_scoopable, "Refuel at every scoopable");
                    toggle(ui, &mut o.already_supercharged, "Already supercharged");
                    ui.end_row();
                    ui.label("Reserve");
                    ui.add(egui::DragValue::new(&mut o.reserve).range(0.0..=tank).speed(0.1).suffix(" t")).on_hover_text("Fuel to keep in the tank, up to a full main tank.");
                }
                Planner::Carrier | Planner::InGame => {
                    ui.label(RichText::new("No extra options for this planner.").size(size::FOOTNOTE).color(pal::DIM));
                }
            });
        }
        if let Some(Err(e)) = &st.job.last {
            ui.add(egui::Label::new(RichText::new(format!("{} {e}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
        }
        ui.add_space(2.0);
        ui.label(RichText::new("Route data © Spansh (spansh.co.uk)").size(size::CAPTION).color(pal::FAINT));
    });
    changed
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

type PlanFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<PlannedRoute, String>> + Send>>;

/// `carrier` is the carrier the fleet carrier planner plans for, if the commander has one.
fn make_request(gs: &GameState, st: &NavUi, carrier: Option<CarrierKind>, spansh: &Spansh) -> Option<PlanFuture> {
    let sp = spansh.clone();
    let from = match st.from.trim() {
        "" => gs.location.system.clone(),
        f => f.to_string(),
    };
    let to = st.to.trim().to_string();
    if from.is_empty() {
        return None;
    }
    match st.planner {
        Planner::InGame => None,
        Planner::Neutron => {
            if to.is_empty() {
                return None;
            }
            let req = NeutronRouteRequest::new(from, to, st.range.clamp(*limits::RANGE_LY.start(), *limits::RANGE_LY.end())).efficiency(st.efficiency);
            Some(Box::pin(async move {
                let p = sp.neutron_route(&req).await.map_err(|e| e.to_string())?;
                let wps: Vec<Waypoint> = p.route.waypoints().iter().map(from_spansh).collect();
                let summary = format!("{} waypoints · {} jumps · {} neutron boosts", wps.len(), p.route.total_jumps(), p.route.neutron_count());
                Ok(PlannedRoute::new("Neutron", summary, p.web_url, wps))
            }))
        }
        Planner::Exact => {
            if to.is_empty() {
                return None;
            }
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
                    "{} jumps · {:.0} ly · {} refuels · {} neutron boosts · {:.1} t fuel",
                    p.route.jump_count(),
                    p.route.distance(),
                    p.route.refuel_stops(),
                    p.route.neutron_boosts(),
                    p.route.fuel_used()
                );
                Ok(PlannedRoute::new("Exact", summary, p.web_url, wps))
            }))
        }
        Planner::Carrier => {
            if to.is_empty() {
                return None;
            }
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
                let summary = format!("{} carrier jumps · {:.0} ly · {} t tritium", p.route.jump_count(), p.route.distance(), p.route.tritium_used());
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
        if planned_complete(p) {
            if route.is_empty() {
                empty(ui, icon::FLAG_CHECKERED, "Route complete", "You've reached every stop. Go back to the in-game route, or plan another one.");
                return;
            }
            ui.add(egui::Label::new(RichText::new(format!("{} Tour complete, showing the route plotted in the game.", icon::FLAG_CHECKERED)).size(size::FOOTNOTE).color(pal::DIM)).wrap());
            ui.add_space(8.0);
        }
    }
    if route.is_empty() {
        empty(ui, icon::NAVIGATION_ARROW, "No route", "Plot a route in the galaxy map, or use a Spansh planner above.");
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
                note(ui, format!("{} waypoints · {jumps} jumps", route.len()), pal::DIM);
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
        .striped(true)
        .resizable(false)
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
