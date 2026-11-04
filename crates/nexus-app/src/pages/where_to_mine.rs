//! Mining › Where to mine: bodies worth landing on for Rhino surface mining, near you and along a route (Spansh's body
//! search), in your system (your own scans), and what your own runs gave on each ground. See surface_finder.rs for
//! what the data can and can't say.

use crate::jobs::Slot;
use crate::pages::navigation::PlannedRoute;
use crate::surface_finder::{self as sf, Around, Candidate, Found, Query, RouteStop};
use crate::theme::{display, pal, size};
use crate::tours::{self, limits};
use crate::widgets::{self, GAP, badge, card_tinted, footnote, th};
use egui::{Grid, RichText, Ui};
use egui_phosphor::regular as icon;
use nexus_data::grounds::{Class, Ground, Volcanism};
use nexus_data::names::group_digits;
use nexus_model::GameState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Near,
    Route,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Order {
    #[default]
    Nearest,
    MostLocations,
}

/// State of the Where to mine tab.
pub struct WhereUi {
    pub mode: Mode,
    class: Option<Class>,
    volcanism: Option<Volcanism>,
    min_locations: u32,
    /// Furthest from the arrival star, ls (0: any).
    max_ls: u32,
    /// Near you, ly.
    radius: u32,
    /// Along a route: how far from the line, ly.
    corridor: u32,
    stops: u32,
    /// The destination typed; empty: the end of the active route.
    to: String,
    order: Order,
    show_options: bool,
    /// Rows shown near you.
    shown: usize,
    near: Slot<Result<Found, String>>,
    route: Slot<Result<Found, String>>,
    /// What the results were searched with (a route also its stops and destination).
    near_query: Option<Query>,
    route_query: Option<(Query, u32)>,
    /// Start a search on the first frame (screenshot runs: `--tab where` / `--tab where-route`).
    autorun: bool,
}

impl Default for WhereUi {
    fn default() -> Self {
        WhereUi {
            mode: Mode::Near,
            class: None,
            volcanism: None,
            min_locations: 1,
            max_ls: 0,
            radius: 100,
            corridor: 25,
            stops: 10,
            to: String::new(),
            order: Order::Nearest,
            show_options: false,
            shown: ROWS,
            near: Slot::default(),
            route: Slot::default(),
            near_query: None,
            route_query: None,
            autorun: false,
        }
    }
}

impl WhereUi {
    /// `where` searches near you on the first frame when `autorun`, `where-route` towards Colonia (screenshot runs).
    pub fn new(tab: Option<&str>, autorun: bool) -> Self {
        let mut ui = WhereUi::default();
        match tab {
            Some("where") => ui.autorun = autorun,
            Some("where-route") => (ui.mode, ui.to, ui.autorun) = (Mode::Route, "Colonia".into(), autorun),
            _ => {}
        }
        ui
    }
}

/// Rows shown at a time.
const ROWS: usize = 25;
/// Systems on a tour near you.
const TOUR_SYSTEMS: usize = 10;

/// What the tab asks the app to do.
pub enum Action {
    /// Open Navigation with this system as the destination.
    PlotTo(String),
    /// Make this the active route on Navigation.
    Follow(PlannedRoute),
}

const NO_PROMISE: &str = "No journal or feed says what a mining location holds: the game records what the Rhino's refinery made, not what was in the deposit. A ground and your own runs are leads, not promises.";

pub fn show(ui: &mut Ui, gs: &GameState, st: &mut WhereUi, spansh: &nexus_spansh::Spansh, rt: &tokio::runtime::Handle, route_end: Option<&str>) -> Option<Action> {
    let mut action = None;
    let own = sf::own_grounds(gs);
    // Wide windows put this system and your grounds beside the search, where a long list doesn't push them away.
    widgets::split(ui, 0.66, 620.0, |ui| search(ui, gs, st, spansh, rt, route_end, &own, &mut action), |ui| {
        this_system(ui, gs);
        ui.add_space(GAP);
        your_grounds(ui, &own);
    });
    action
}

#[allow(clippy::too_many_arguments)]
fn search(
    ui: &mut Ui,
    gs: &GameState,
    st: &mut WhereUi,
    spansh: &nexus_spansh::Spansh,
    rt: &tokio::runtime::Handle,
    route_end: Option<&str>,
    own: &[sf::GroundYield],
    action: &mut Option<Action>,
) {
    card_tinted(ui, icon::MAGNIFYING_GLASS, "Bodies to mine", pal::ACCENT, |ui| {
        widgets::segmented(ui, &mut st.mode, &[(Mode::Near, "Near you"), (Mode::Route, "Along a route")]);
        ui.add_space(4.0);
        footnote(
            ui,
            match st.mode {
                Mode::Near => "Landable bodies with planetary mining locations around you, nearest first, from the scans commanders share.",
                Mode::Route => "Stops on the way to a destination: in each stretch of the trip, the system nearby with the most mining locations.",
            },
        );
        ui.add_space(6.0);
        match st.mode {
            Mode::Near => near_you(ui, gs, st, spansh, rt, own, action),
            Mode::Route => along_route(ui, gs, st, spansh, rt, route_end, own, action),
        }
        ui.add_space(4.0);
        footnote(
            ui,
            "Mining-location counts come from FSS and DSS scans commanders share through EDDN (EDMC, EDDiscovery…), collected by Spansh. Only scans since the Rhino update (September 2026) carry them, so a body without a count may still have mining locations.",
        );
        footnote(ui, NO_PROMISE);
        ui.label(RichText::new("Body data © Spansh (spansh.co.uk)").size(size::CAPTION).color(pal::FAINT));
    });
}

/// One row of controls; it wraps only when the window is too narrow for it.
fn controls_row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        add(ui)
    })
    .inner
}

fn label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(pal::DIM));
}

/// "Rocky ice · you worked 3": an option, with how many locations you worked on such ground.
fn option_text(label: &str, worked: usize) -> String {
    if worked > 0 { format!("{label} · you worked {worked}") } else { label.to_string() }
}

fn class_control(ui: &mut Ui, st: &mut WhereUi, own: &[sf::GroundYield]) {
    label(ui, "Class");
    egui::ComboBox::from_id_salt("where-class")
        .width(170.0)
        .selected_text(st.class.map_or("Any class", Class::label))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut st.class, None, "Any class");
            for c in Class::ALL {
                let worked = own.iter().filter(|y| y.ground.is_some_and(|g| g.class == c)).map(|y| y.locations).sum();
                ui.selectable_value(&mut st.class, Some(c), option_text(c.label(), worked));
            }
        })
        .response
        .on_hover_text("The body's class, as the game reports it. Your grounds shows what your own runs gave on each class and volcanism.");
}

fn volcanism_control(ui: &mut Ui, st: &mut WhereUi, own: &[sf::GroundYield]) {
    label(ui, "Volcanism");
    egui::ComboBox::from_id_salt("where-volcanism")
        .width(200.0)
        .selected_text(st.volcanism.map_or("Any volcanism", |v| v.label()))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut st.volcanism, None, "Any volcanism");
            for v in sf::searchable_volcanism() {
                let worked = own.iter().filter(|y| y.ground.is_some_and(|g| g.volcanism == v)).map(|y| y.locations).sum();
                ui.selectable_value(&mut st.volcanism, Some(v), option_text(v.label(), worked));
            }
        })
        .response
        .on_hover_text("What the body's volcanism erupts, as the game reports it, minor or major alike. Silicate magma isn't offered: no body Spansh knows has it.");
}

fn locations_control(ui: &mut Ui, st: &mut WhereUi) {
    label(ui, "Mining locations");
    widgets::segmented(ui, &mut st.min_locations, &[(1, "1+"), (5, "5+"), (10, "10+"), (20, "20+")]);
}

fn search_button(ui: &mut Ui, busy: bool, enabled: bool) -> bool {
    let label = if busy { format!("{} Searching…", icon::HOURGLASS) } else { format!("{} Search", icon::MAGNIFYING_GLASS) };
    let clicked = ui.add_enabled_ui(!busy && enabled, |ui| widgets::primary_button(ui, &label)).inner.clicked();
    if busy {
        ui.spinner();
    }
    clicked
}

fn arrival_option(ui: &mut Ui, st: &mut WhereUi) {
    label(ui, "From the star");
    widgets::segmented(ui, &mut st.max_ls, &[(0, "Any"), (25_000, "≤ 25,000 ls"), (5_000, "≤ 5,000 ls"), (1_000, "≤ 1,000 ls")]);
}

fn near_you(ui: &mut Ui, gs: &GameState, st: &mut WhereUi, spansh: &nexus_spansh::Spansh, rt: &tokio::runtime::Handle, own: &[sf::GroundYield], action: &mut Option<Action>) {
    if st.near.tick() {
        st.shown = ROWS;
    }
    let origin = gs.location.star_pos;
    controls_row(ui, |ui| {
        class_control(ui, st, own);
        volcanism_control(ui, st, own);
    });
    controls_row(ui, |ui| locations_control(ui, st));
    controls_row(ui, |ui| {
        label(ui, "Within");
        widgets::segmented(ui, &mut st.radius, &[(25, "25 ly"), (50, "50 ly"), (100, "100 ly"), (250, "250 ly"), (500, "500 ly"), (1000, "1,000 ly")]);
        let busy = st.near.busy();
        if (search_button(ui, busy, origin.is_some()) || std::mem::take(&mut st.autorun))
            && !busy
            && let Some(origin) = origin
        {
            let q = Query {
                around: Around::Here(origin),
                radius_ly: f64::from(st.radius),
                class: st.class,
                volcanism: st.volcanism,
                min_locations: st.min_locations,
                max_ls: st.max_ls,
            };
            st.near_query = Some(q.clone());
            let sp = spansh.clone();
            st.near.run_async(ui.ctx(), rt, async move { sf::near(&sp, &q).await });
        }
    });
    if widgets::disclosure(ui, &mut st.show_options, "Options") {
        controls_row(ui, |ui| arrival_option(ui, st));
    }
    if origin.is_none() {
        footnote(ui, "Your position isn't known yet: jump or relog and Elite Nexus reads it from the journal.");
    }
    ui.add_space(6.0);
    let Some(result) = &st.near.last else { return };
    let found = match result {
        Err(e) => {
            ui.add(egui::Label::new(RichText::new(format!("{} {e}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
            return;
        }
        Ok(f) => f,
    };
    let Some(q) = &st.near_query else { return };
    if found.list.is_empty() {
        widgets::empty(ui, icon::PLANET, &format!("Nothing within {:.0} ly", q.radius_ly), "No landable body there has as many mining locations on record. Search further, or ask for fewer locations or any class or volcanism.");
        return;
    }
    ui.add(egui::Label::new(RichText::new(found_line(found, q)).color(pal::TEXT)).wrap());
    ui.add_space(6.0);
    let mut list: Vec<&Candidate> = found.list.iter().collect();
    if st.order == Order::MostLocations {
        list.sort_by(|a, b| b.locations.cmp(&a.locations).then(a.distance_ly.total_cmp(&b.distance_ly)));
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        ui.label(RichText::new("Sort").color(pal::DIM));
        widgets::segmented(ui, &mut st.order, &[(Order::Nearest, "Nearest"), (Order::MostLocations, "Most locations")]);
        let systems = count_systems(&list).min(TOUR_SYSTEMS);
        if widgets::primary_button(ui, &format!("{} Tour {systems} systems", icon::PATH))
            .on_hover_text("A route through the first systems in this list, each leg to the nearest one left, on the Navigation page")
            .clicked()
        {
            *action = Some(Action::Follow(sf::near_tour(&list, TOUR_SYSTEMS, gs.location.star_pos, tours::jump_range(gs))));
        }
    });
    ui.add_space(6.0);
    bodies_table(ui, gs, &list[..list.len().min(st.shown)], action);
    if list.len() > st.shown {
        ui.add_space(6.0);
        if widgets::button(ui, &format!("Show {} more", (list.len() - st.shown).min(ROWS))).clicked() {
            st.shown += ROWS;
        }
    }
}

/// "rocky bodies with rocky magma", "icy bodies without volcanism", "bodies".
fn subject(q: &Query, several: bool) -> String {
    let noun = q.class.map_or_else(|| (if several { "bodies" } else { "body" }).to_string(), |c| c.noun(several));
    match q.volcanism {
        None => noun,
        Some(Volcanism::Inactive) => format!("{noun} without volcanism"),
        Some(v) => format!("{noun} with {}", v.label()),
    }
}

/// "Within 100 ly, Spansh lists 30 rocky bodies with rocky magma that have 5 or more mining locations. These are the 25
/// nearest, out to 87 ly."
fn found_line(found: &Found, q: &Query) -> String {
    let matched = if found.matched >= 10_000 { "10,000+".to_string() } else { group_digits(found.matched as i64) };
    let several = found.matched != 1;
    let have = if several { "have" } else { "has" };
    let locations = if q.min_locations <= 1 { "mining locations".to_string() } else { format!("{} or more mining locations", q.min_locations) };
    let mut line = format!("Within {:.0} ly, Spansh lists {matched} {} that {have} {locations}.", q.radius_ly, subject(q, several));
    if (found.list.len() as u64) < found.matched {
        line.push_str(&format!(" These are the {} nearest, out to {:.0} ly.", group_digits(found.list.len() as i64), found.reach_ly));
    }
    line
}

fn count_systems(list: &[&Candidate]) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for c in list {
        if !seen.iter().any(|s| s.eq_ignore_ascii_case(&c.system)) {
            seen.push(&c.system);
        }
    }
    seen.len()
}

fn ground_text(g: Option<Ground>) -> String {
    g.map_or_else(|| "?".to_string(), Ground::label)
}

/// A ground in a table cell: the class, and under it the volcanism, so the column stays narrow.
fn ground_cell(ui: &mut Ui, g: Option<Ground>) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        match g {
            Some(g) => {
                ui.label(RichText::new(g.class.label()).color(pal::TEXT));
                if g.volcanism != Volcanism::Inactive {
                    ui.label(RichText::new(g.volcanism.label()).size(size::CAPTION).color(pal::DIM));
                }
            }
            None => {
                ui.label(RichText::new("?").color(pal::DIM));
            }
        }
    });
}

/// "12 ly", and under it the distance from the arrival star.
fn distance_cell(ui: &mut Ui, ly: f64, ls: Option<f64>) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(format!("{ly:.0} ly")).color(pal::TEXT));
        ui.label(RichText::new(ls.map(|d| format!("{} ls", group_digits(d.round() as i64))).unwrap_or_else(|| "? ls".into())).size(size::CAPTION).color(pal::DIM));
    });
}

fn bodies_table(ui: &mut Ui, gs: &GameState, list: &[&Candidate], action: &mut Option<Action>) {
    Grid::new("where-bodies").max_col_width(ui.available_width()).num_columns(7).striped(true).spacing([14.0, 10.0]).show(ui, |ui| {
        for t in ["Body", "Ground", "Locations", "Distance", "Gravity", "Reported", ""] {
            th(ui, t);
        }
        ui.end_row();
        for c in list {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(c.short_body()).font(display(size::CALLOUT)).color(pal::TEXT));
                    let worked = match (c.system_address, c.body_id) {
                        (Some(a), Some(b)) => sf::worked(gs, a, b),
                        _ => 0,
                    };
                    if worked > 0 {
                        badge(ui, "Mined", pal::GOOD).on_hover_text(format!("You've worked {worked} mining location{} here: see Sessions and sites.", if worked == 1 { "" } else { "s" }));
                    }
                });
                ui.label(RichText::new(&c.system).size(size::CAPTION).color(pal::DIM));
            })
            .response
            .on_hover_text(format!(
                "{}{} · {}",
                c.class,
                c.volcanism.as_deref().filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("No volcanism")).map(|v| format!(", {}", v.to_lowercase())).unwrap_or_default(),
                c.atmosphere.as_deref().unwrap_or("atmosphere not on record"),
            ));
            ground_cell(ui, c.ground);
            ui.label(RichText::new(c.locations.to_string()).font(display(size::CALLOUT)).color(pal::ACCENT));
            distance_cell(ui, c.distance_ly, c.distance_ls);
            ui.label(RichText::new(c.gravity_g.map(|g| format!("{g:.2} g")).unwrap_or_else(|| "?".into())).color(pal::DIM));
            ui.label(RichText::new(c.reported.map(|t| widgets::ago(nexus_journal::Timestamp(t))).unwrap_or_else(|| "?".into())).color(pal::DIM))
                .on_hover_text("When a commander's scan last reported this body's signals to Spansh");
            ui.horizontal(|ui| {
                if widgets::icon_button(ui, icon::COPY, "Copy the system name").clicked() {
                    ui.ctx().copy_text(c.system.clone());
                }
                if widgets::icon_button(ui, icon::PATH, "Plot a route there").clicked() {
                    *action = Some(Action::PlotTo(c.system.clone()));
                }
            });
            ui.end_row();
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn along_route(
    ui: &mut Ui,
    gs: &GameState,
    st: &mut WhereUi,
    spansh: &nexus_spansh::Spansh,
    rt: &tokio::runtime::Handle,
    route_end: Option<&str>,
    own: &[sf::GroundYield],
    action: &mut Option<Action>,
) {
    st.route.tick();
    let from = gs.location.system.trim().to_string();
    let typed = st.to.trim().to_string();
    let to = if typed.is_empty() { route_end.map(str::to_string) } else { Some(typed) };
    controls_row(ui, |ui| {
        let hint = route_end.map_or_else(|| "Destination system".to_string(), |e| format!("{e} (your route's end)"));
        label(ui, "To");
        ui.add(egui::TextEdit::singleline(&mut st.to).hint_text(hint).desired_width(230.0).margin(egui::Margin::symmetric(10, 6)));
        class_control(ui, st, own);
    });
    controls_row(ui, |ui| {
        volcanism_control(ui, st, own);
        locations_control(ui, st);
    });
    controls_row(ui, |ui| {
        label(ui, "Off the line");
        widgets::segmented(ui, &mut st.corridor, &[(10, "10 ly"), (25, "25 ly"), (50, "50 ly"), (100, "100 ly")]);
        label(ui, "Stops");
        widgets::segmented(ui, &mut st.stops, &[(5, "5"), (10, "10"), (25, "25"), (50, "50")]);
        let busy = st.route.busy();
        let ready = !from.is_empty() && to.as_deref().is_some_and(|t| !t.eq_ignore_ascii_case(&from));
        if (search_button(ui, busy, ready) || std::mem::take(&mut st.autorun))
            && !busy
            && ready
            && let Some(to) = to.clone()
        {
            let q = Query {
                around: Around::Route { from: from.clone(), to },
                radius_ly: f64::from(st.corridor),
                class: st.class,
                volcanism: st.volcanism,
                min_locations: st.min_locations,
                max_ls: st.max_ls,
            };
            st.route_query = Some((q.clone(), st.stops));
            let (sp, stops) = (spansh.clone(), st.stops as usize);
            st.route.run_async(ui.ctx(), rt, async move { sf::along(&sp, &q, stops).await });
        }
    });
    if widgets::disclosure(ui, &mut st.show_options, "Options") {
        controls_row(ui, |ui| {
            arrival_option(ui, st);
            label(ui, "Off the line");
            ui.add(egui::DragValue::new(&mut st.corridor).range(limits::RADIUS_LY).suffix(" ly")).on_hover_text("Up to 1,000 ly, as Spansh's own planners.");
            label(ui, "Stops");
            ui.add(egui::DragValue::new(&mut st.stops).range(limits::SYSTEMS)).on_hover_text("Up to 2,000, as Spansh's own planners.");
        });
    }
    if to.is_none() {
        footnote(ui, "Type a destination, or plot a route in the game's galaxy map or on the Navigation page: its end is used.");
    }
    ui.add_space(6.0);
    let Some(result) = &st.route.last else { return };
    let found = match result {
        Err(e) => {
            ui.add(egui::Label::new(RichText::new(format!("{} {e}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
            return;
        }
        Ok(f) => f,
    };
    let Some((q, stops_asked)) = &st.route_query else { return };
    let Around::Route { from, to } = &q.around else { return };
    let stops = sf::route_stops(found, *stops_asked as usize);
    let length = found.ends.map_or(0.0, |(a, b)| nexus_model::location::distance(a, b));
    if stops.is_empty() {
        widgets::empty(
            ui,
            icon::PATH,
            "No stops on this route",
            &format!("Spansh lists no landable body with mining locations within {:.0} ly of the line from {from} to {to}. Widen the search, or ask for fewer locations or any class or volcanism.", q.radius_ly),
        );
        return;
    }
    let matched = if found.matched >= 10_000 { "10,000+".to_string() } else { group_digits(found.matched as i64) };
    let stretch = length / f64::from((*stops_asked).max(1));
    let mut line = format!(
        "From {from} to {to}, {} ly. Within {:.0} ly of the line Spansh lists {matched} {} with mining locations; of the {} nearest it, here is the best system in each {} ly stretch.",
        group_digits(length.round() as i64),
        q.radius_ly,
        subject(q, found.matched != 1),
        group_digits(found.list.len() as i64),
        group_digits(stretch.round() as i64),
    );
    let empty = (*stops_asked as usize).saturating_sub(stops.len());
    if empty > 0 {
        line.push_str(&format!(" {empty} stretch{} had none.", if empty == 1 { "" } else { "es" }));
    }
    ui.add(egui::Label::new(RichText::new(line).color(pal::TEXT)).wrap());
    ui.add_space(6.0);
    if widgets::primary_button(ui, &format!("{} Follow this route", icon::NAVIGATION_ARROW))
        .on_hover_text("The stops in order, then your destination, on the Navigation page; the next system goes to the clipboard after every jump")
        .clicked()
    {
        let to_pos = found.ends.map(|(_, b)| b);
        *action = Some(Action::Follow(sf::route_plan(&stops, gs.location.star_pos, to, to_pos, tours::jump_range(gs))));
    }
    ui.add_space(6.0);
    stops_table(ui, gs, &stops, action);
    footnote(
        ui,
        "Spansh measures how far a body is from the straight line between your system and the destination; a plotted route stays close to it. \
         It reads at most 2,000 bodies nearest the line (four pages), which keeps the load on Spansh's server light.",
    );
}

fn stops_table(ui: &mut Ui, gs: &GameState, stops: &[RouteStop], action: &mut Option<Action>) {
    // The wrapping column goes last: a grid gives the last column the width that's left.
    Grid::new("where-stops").max_col_width(ui.available_width()).num_columns(5).striped(true).spacing([14.0, 8.0]).show(ui, |ui| {
        for t in ["#", "System", "On the way", "Off the line", "Bodies with mining locations"] {
            th(ui, t);
        }
        ui.end_row();
        for (i, s) in stops.iter().enumerate() {
            ui.label(RichText::new((i + 1).to_string()).color(pal::FAINT));
            ui.horizontal(|ui| {
                if ui.link(RichText::new(&s.system).color(pal::TEXT)).on_hover_text("Click to copy").clicked() {
                    ui.ctx().copy_text(s.system.clone());
                }
                if widgets::icon_button(ui, icon::PATH, "Plot a route there").clicked() {
                    *action = Some(Action::PlotTo(s.system.clone()));
                }
            });
            ui.label(RichText::new(format!("{} ly", group_digits(s.along_ly.round() as i64))).color(pal::DIM));
            ui.label(RichText::new(format!("{:.0} ly", s.off_line_ly)).color(pal::DIM));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for c in s.bodies.iter().take(4) {
                    let worked = match (c.system_address, c.body_id) {
                        (Some(a), Some(b)) => sf::worked(gs, a, b),
                        _ => 0,
                    };
                    let mined = if worked > 0 { " · mined" } else { "" };
                    let ls = c.distance_ls.map(|d| format!(" · {} ls", group_digits(d.round() as i64))).unwrap_or_default();
                    ui.add(
                        egui::Label::new(RichText::new(format!("{} · {} · {} locations{ls}{mined}", c.short_body(), ground_text(c.ground), c.locations)).color(pal::TEXT))
                            .wrap(),
                    );
                }
                if s.bodies.len() > 4 {
                    ui.label(RichText::new(format!("and {} more", s.bodies.len() - 4)).size(size::FOOTNOTE).color(pal::DIM));
                }
            });
            ui.end_row();
        }
    });
}

/// The landable planets of this system, from your own scans.
fn this_system(ui: &mut Ui, gs: &GameState) {
    card_tinted(ui, icon::PLANET, "In this system", pal::CYAN, |ui| {
        let list = sf::this_system(gs);
        if list.is_empty() {
            footnote(ui, "Scan this system with the FSS: its landable planets show here with their ground and mining locations.");
            return;
        }
        Grid::new("where-here").max_col_width(ui.available_width()).num_columns(3).striped(true).spacing([12.0, 6.0]).show(ui, |ui| {
            for t in ["Body", "Ground", "Locations"] {
                th(ui, t);
            }
            ui.end_row();
            for b in &list {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&b.name).color(pal::TEXT));
                        if b.worked > 0 {
                            badge(ui, "Mined", pal::GOOD).on_hover_text(format!("You've worked {} mining location{} here", b.worked, if b.worked == 1 { "" } else { "s" }));
                        }
                    });
                    let gravity = b.gravity_g.map(|g| format!(" · {g:.2} g")).unwrap_or_default();
                    ui.label(RichText::new(format!("{} ls{gravity}", group_digits(b.distance_ls.round() as i64))).size(size::CAPTION).color(pal::DIM));
                });
                ground_cell(ui, b.ground);
                if b.locations > 0 {
                    ui.label(RichText::new(b.locations.to_string()).color(pal::ACCENT));
                } else {
                    ui.label(RichText::new("–").color(pal::FAINT)).on_hover_text("None in your scans: the FSS and the DSS report them, and scans from before September 2026 don't.");
                }
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        footnote(ui, "From your own scans. To mine one, target the body and pick a Planetary Mining Location in the navigation panel.");
    });
}

/// What your own Rhino runs gave on each ground.
fn your_grounds(ui: &mut Ui, own: &[sf::GroundYield]) {
    card_tinted(ui, icon::DIAMONDS_FOUR, "Your grounds", pal::GOLD, |ui| {
        if own.is_empty() {
            footnote(ui, "Refine with the Rhino and the grounds you mined show here, with what came out on each.");
            return;
        }
        // A list rather than a grid: ground names run long, and this card is narrow beside the search.
        for (i, y) in own.iter().enumerate() {
            if i > 0 {
                ui.add_space(6.0);
            }
            ui.horizontal_wrapped(|ui| {
                let name = ui.label(RichText::new(y.ground.map_or_else(|| "Not scanned".to_string(), Ground::label)).color(pal::TEXT));
                if y.ground.is_none() {
                    name.on_hover_text("Bodies with no class on record: your journal has no scan of them");
                }
                let places = format!(
                    "{} location{} on {} bod{} · {} t",
                    y.locations,
                    if y.locations == 1 { "" } else { "s" },
                    y.bodies,
                    if y.bodies == 1 { "y" } else { "ies" },
                    group_digits(i64::from(y.total()))
                );
                ui.label(RichText::new(places).size(size::CAPTION).color(pal::ACCENT));
            });
            let parts: Vec<String> = y.by_tonnes().iter().take(5).map(|(c, t, n)| format!("{c} {t} t ({n} of {})", y.locations)).collect();
            ui.add(egui::Label::new(RichText::new(parts.join(" · ")).color(pal::DIM)).wrap());
        }
        ui.add_space(4.0);
        footnote(
            ui,
            "From your Surface mining sites, by the ground of each body as your scan of it says. Tonnes are what the refinery made, by-products included, and \"2 of 3\" is how many of the locations gave it. Which deposits you drove to decided that, so a few runs say little about a ground.",
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_found_line_says_what_was_read() {
        let q = Query { around: Around::Here([0.0; 3]), radius_ly: 100.0, class: Some(Class::Rocky), volcanism: Some(Volcanism::RockyMagma), min_locations: 5, max_ls: 0 };
        let body = sf::candidate(nexus_spansh::BodyRecord::default());
        let found = Found { matched: 1234, reach_ly: 87.4, list: vec![body; 500], ends: None };
        assert_eq!(
            found_line(&found, &q),
            "Within 100 ly, Spansh lists 1,234 rocky bodies with rocky magma that have 5 or more mining locations. These are the 500 nearest, out to 87 ly."
        );
        let one = Found { matched: 1, list: found.list[..1].to_vec(), ..found.clone() };
        let quiet = Query { volcanism: Some(Volcanism::Inactive), min_locations: 1, ..q.clone() };
        assert_eq!(found_line(&one, &quiet), "Within 100 ly, Spansh lists 1 rocky body without volcanism that has mining locations.");
        let any = Query { class: None, volcanism: None, min_locations: 1, ..q };
        let capped = Found { matched: 10_000, ..found };
        assert!(found_line(&capped, &any).starts_with("Within 100 ly, Spansh lists 10,000+ bodies that have mining locations."));
    }

    #[test]
    fn screenshot_tabs() {
        let route = WhereUi::new(Some("where-route"), true);
        assert!(route.mode == Mode::Route && route.to == "Colonia" && route.autorun);
        let near = WhereUi::new(Some("where"), true);
        assert!(near.mode == Mode::Near && near.autorun);
        assert!(!WhereUi::new(Some("find"), true).autorun, "another page's tab starts nothing here");
    }
}
