use crate::theme::{display, pal, size};
use crate::widgets::{self, GAP, badge, button, card_tinted, cards, credits, empty, footnote, kv, short_credits, th};
use egui::{Color32, Grid, Pos2, RichText, Sense, Stroke, Ui, Vec2};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use nexus_journal::Timestamp;
use nexus_model::exobio::{ExoBio, SampleInProgress};
use nexus_model::location::surface_distance;
use nexus_model::GameState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::jobs::Slot;
use crate::pages::navigation::PlannedRoute;
use crate::tours::{self, Tour, limits};
use crate::untouched::{self, Candidate, Evidence, Stats};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    ThisSystem,
    Find,
}

/// Two ways to pick planets, with opposite trade-offs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strategy {
    /// Nobody has a record of sampling there: ×5 if first, but the species is predicted.
    #[default]
    Bonus,
    /// Species already logged there (Spansh): certain, but base value only.
    Known,
}

/// What the bonus hunt shows (applied to every candidate found, no new search).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Filters {
    no_record: bool,
    mapped: bool,
    /// Least chance of the species, 0–1.
    min_chance: f32,
    /// Furthest from the arrival star, ls (0 = any).
    max_ls: u32,
    /// Strongest gravity, g (0 = any).
    max_g: f32,
}

impl Default for Filters {
    fn default() -> Self {
        Filters { no_record: true, mapped: false, min_chance: 0.0, max_ls: 0, max_g: 0.0 }
    }
}

impl Filters {
    fn keep(&self, c: &Candidate) -> bool {
        let evidence = match c.evidence {
            Evidence::NoTrace => self.no_record,
            Evidence::GenusConfirmed => self.mapped,
        };
        evidence
            && c.chance >= self.min_chance
            && (self.max_ls == 0 || c.distance_ls.is_none_or(|d| d <= f64::from(self.max_ls)))
            && (self.max_g <= 0.0 || c.gravity_g.is_none_or(|g| g <= f64::from(self.max_g)))
    }
}

/// "Sure value": Spansh's exobiology planner.
struct KnownSpecies {
    radius: u32,
    systems: u32,
    min_value: u64,
    show_options: bool,
    job: Slot<Result<Tour, String>>,
}

/// State of the Exobiology page.
pub struct ExobioUi {
    pub tab: Tab,
    strategy: Strategy,
    target: usize,
    radius: u32,
    filters: Filters,
    /// Rows of the bonus hunt shown.
    shown: usize,
    job: Slot<Result<(Vec<Candidate>, Stats), String>>,
    /// The search the results belong to (species, radius).
    searched: Option<(&'static str, u32)>,
    known: KnownSpecies,
    /// Start a search on the first frame (screenshot runs: `--tab untouched` / `--tab known`).
    autorun: bool,
    /// Clear list was clicked on the unsold list: it asks before clearing.
    confirm_clear: bool,
}

/// What the Exobiology page keeps in settings.json.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExobioPrefs {
    /// How far each commander cleared their unsold Vista Genomics list (by FID, or by name where the journals give
    /// none): the newest sample listed then, or the newest sale the game logged without its samples. The list is
    /// rebuilt from the journals at every start, and the model starts with this, so those samples stay off it; the
    /// journals aren't touched.
    pub cleared: BTreeMap<String, Timestamp>,
}

impl ExobioPrefs {
    /// Keeps that `cmdr` cleared the list up to `upto`; never an earlier time than a Clear before.
    pub fn clear(&mut self, cmdr: &str, upto: Timestamp) {
        let kept = self.cleared.entry(cmdr.to_string()).or_insert(upto);
        *kept = (*kept).max(upto);
    }
}

impl Default for ExobioUi {
    fn default() -> Self {
        let target = untouched::targets().iter().position(|s| s.name == "Stratum Tectonicas").unwrap_or(0);
        ExobioUi {
            tab: Tab::ThisSystem,
            strategy: Strategy::Bonus,
            target,
            radius: 500,
            filters: Filters::default(),
            shown: ROWS,
            job: Slot::default(),
            searched: None,
            known: KnownSpecies { radius: 50, systems: 25, min_value: 5_000_000, show_options: false, job: Slot::default() },
            autorun: false,
            confirm_clear: false,
        }
    }
}

/// Rows of results shown at a time.
const ROWS: usize = 25;
/// Planets on a bonus-hunt tour.
const TOUR_PLANETS: usize = 15;

impl ExobioUi {
    /// Opens on `tab` ("untouched" or "known": Find planets with that strategy; "clear-unsold": This system, with the
    /// unsold list's Clear list asking to confirm); `autorun` starts its search on the first frame (screenshot runs).
    pub fn new(tab: Option<&str>, autorun: bool) -> Self {
        let mut ui = ExobioUi::default();
        match tab {
            Some("untouched") => (ui.tab, ui.strategy, ui.autorun) = (Tab::Find, Strategy::Bonus, autorun),
            Some("known") => (ui.tab, ui.strategy, ui.autorun) = (Tab::Find, Strategy::Known, autorun),
            Some("clear-unsold") => ui.confirm_clear = true,
            _ => {}
        }
        ui
    }
}

/// What the page asks the app to do.
pub enum ExoAction {
    /// Open Navigation with this system as the destination.
    PlotTo(String),
    /// Make this the active route on Navigation.
    Follow(PlannedRoute),
    /// Clear the active commander's unsold list up to this time ([`ExoBio::clear_point`]), in the model and in the
    /// settings.
    ClearUnsold(Timestamp),
}

pub fn show(ui: &mut Ui, gs: &GameState, st: &mut ExobioUi, spansh: &nexus_spansh::Spansh, rt: &tokio::runtime::Handle, db: Option<&nexus_galaxy::GalaxyDb>) -> Option<ExoAction> {
    if let Some(t) = widgets::tabs(ui, st.tab, &[(Tab::ThisSystem, "This system"), (Tab::Find, "Find planets")]) {
        st.tab = t;
    }
    ui.add_space(GAP);
    match st.tab {
        Tab::ThisSystem => {
            let clear = show_page(ui, gs, &mut st.confirm_clear);
            ui.add_space(GAP);
            bio_bodies(ui, gs);
            clear.map(ExoAction::ClearUnsold)
        }
        Tab::Find => {
            widgets::segmented(ui, &mut st.strategy, &[(Strategy::Bonus, "Bonus hunt: no one's been"), (Strategy::Known, "Sure value: species on record")]);
            ui.add_space(4.0);
            footnote(
                ui,
                match st.strategy {
                    Strategy::Bonus => "Planets where a valuable species should grow and nobody has a record of sampling it. Be the first to log it and it pays 5× (the first-logged bonus), but the species there is predicted, not certain.",
                    Strategy::Known => "Planets where commanders have already logged valuable species. You know what's there, so the payout is certain, but someone logged it before you: expect the base value, the bonus has most likely been claimed.",
                },
            );
            ui.add_space(GAP);
            match st.strategy {
                Strategy::Bonus => bonus_hunt(ui, gs, st, spansh, rt, db),
                Strategy::Known => known_species(ui, gs, st, spansh, rt),
            }
        }
    }
}

/// The bonus hunt: where a valuable species should grow and nobody sharing data has mapped or
/// sampled (see untouched.rs for what that can and cannot prove).
fn bonus_hunt(ui: &mut Ui, gs: &GameState, st: &mut ExobioUi, spansh: &nexus_spansh::Spansh, rt: &tokio::runtime::Handle, db: Option<&nexus_galaxy::GalaxyDb>) -> Option<ExoAction> {
    if st.job.tick() {
        st.shown = ROWS;
    }
    let targets = untouched::targets();
    let mut action = None;
    card_tinted(ui, icon::SPARKLE, "Bonus hunt", pal::ACCENT, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            let cur = targets.get(st.target).or(targets.first()).copied();
            if let Some(cur) = cur {
                egui::ComboBox::from_id_salt("untouched-species")
                    .width(300.0)
                    .selected_text(format!("{} · {} first logged", cur.name, short_credits(nexus_data::bio_predict::payout(cur.value, true))))
                    .show_ui(ui, |ui| {
                        for (i, s) in targets.iter().enumerate() {
                            ui.selectable_value(&mut st.target, i, format!("{} · {} ({} first logged)", s.name, short_credits(s.value), short_credits(nexus_data::bio_predict::payout(s.value, true))));
                        }
                    });
            }
            ui.label(RichText::new("within").color(pal::DIM));
            widgets::segmented(ui, &mut st.radius, &[(250, "250 ly"), (500, "500 ly"), (1000, "1,000 ly"), (2000, "2,000 ly")]);
            let origin = gs.location.star_pos;
            let busy = st.job.busy();
            let label = if busy { format!("{} Searching…", icon::HOURGLASS) } else { format!("{} Search", icon::MAGNIFYING_GLASS) };
            let clicked = ui.add_enabled_ui(!busy && origin.is_some(), |ui| widgets::primary_button(ui, &label)).inner.clicked();
            if (clicked || std::mem::take(&mut st.autorun))
                && !busy
                && let (Some(origin), Some(species)) = (origin, cur)
            {
                // Collect plenty, so the filters below have something to choose from.
                let q = untouched::Query { species, origin, radius_ly: f64::from(st.radius), limit: 200 };
                st.searched = Some((species.name, st.radius));
                let sp = spansh.clone();
                let db = db.cloned();
                st.job.run_async(ui.ctx(), rt, async move {
                    let footfalled = move |name: &str| db.as_ref().and_then(|d| d.body_by_name(name).ok().flatten()).is_some_and(|b| b.was_footfalled.is_some_and(|s| s.value));
                    untouched::find(&sp, &q, footfalled).await
                });
            }
            if busy {
                ui.spinner();
            }
        });
        ui.add_space(6.0);
        match &st.job.last {
            None if !st.job.busy() => {
                footnote(ui, "Pick a species and a range, then Search. Every planet that suits it is checked against everything commanders have shared through EDDN.");
            }
            None => {}
            Some(Err(e)) => {
                ui.add(egui::Label::new(RichText::new(format!("{} {e}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
            }
            Some(Ok((list, stats))) => {
                let (name, radius) = st.searched.unwrap_or(("", st.radius));
                let genus = untouched::targets().into_iter().find(|s| s.name == name).map_or("the genus", |s| s.genus);
                funnel(ui, list, stats, name, genus, radius);
                ui.add_space(8.0);
                bonus_filters(ui, &mut st.filters, list);
                ui.add_space(8.0);
                let shown: Vec<&Candidate> = list.iter().filter(|c| st.filters.keep(c)).collect();
                if shown.is_empty() {
                    let hint = if list.is_empty() {
                        "Every planet with these conditions in range has a record of someone mapping or sampling it. Try a larger range: the further from the bubble, the more untouched planets."
                    } else {
                        "The filters hide every planet found. Loosen them above."
                    };
                    empty(ui, icon::PLANET, "No planets to show", hint);
                    return;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{} planet{} to try", shown.len(), if shown.len() == 1 { "" } else { "s" })).font(display(size::HEADLINE)).color(pal::TEXT));
                    let n = shown.len().min(TOUR_PLANETS);
                    if widgets::primary_button(ui, &format!("{} Tour the best {n}", icon::PATH))
                        .on_hover_text("A route through the best of these, nearest first, on the Navigation page")
                        .clicked()
                    {
                        action = Some(ExoAction::Follow(bonus_tour(gs, name, &shown[..n])));
                    }
                });
                ui.add_space(6.0);
                candidates_table(ui, &shown[..shown.len().min(st.shown)], &mut action);
                if shown.len() > st.shown {
                    ui.add_space(6.0);
                    if widgets::button(ui, &format!("Show {} more", (shown.len() - st.shown).min(ROWS))).clicked() {
                        st.shown += ROWS;
                    }
                }
            }
        }
    });
    action
}

/// What the search found, in words: where the bonus is probably gone, and why.
fn funnel(ui: &mut Ui, list: &[Candidate], s: &Stats, name: &str, genus: &str, radius: u32) {
    let no_record = list.iter().filter(|c| c.evidence == Evidence::NoTrace).count();
    let mapped = list.len() - no_record;
    let matched = if s.matched >= 10_000 { "10,000+".to_string() } else { nexus_data::names::group_digits(s.matched as i64) };
    ui.add(
        egui::Label::new(
            RichText::new(format!(
                "Within {radius} ly, Spansh lists {matched} planets where {name} can grow. Of the {} nearest (out to {:.0} ly):",
                nexus_data::names::group_digits(s.examined as i64),
                s.reach_ly
            ))
            .color(pal::TEXT),
        )
        .wrap(),
    );
    ui.add_space(4.0);
    let line = |ui: &mut Ui, mark: &str, color: Color32, n: usize, text: String| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(mark).color(color));
            ui.add_sized([48.0, 18.0], egui::Label::new(RichText::new(nexus_data::names::group_digits(n as i64)).font(display(size::CALLOUT)).color(color)));
            ui.add(egui::Label::new(RichText::new(text).color(pal::DIM)).wrap());
        });
    };
    line(ui, icon::CHECK_CIRCLE, pal::GOOD, no_record, "have no record of anyone mapping or sampling them: your best chance at the bonus".into());
    line(ui, icon::QUESTION, pal::CYAN, mapped, format!("were mapped by someone ({genus} confirmed) but have no samples on record: the bonus may be gone"));
    line(ui, icon::X_CIRCLE, pal::FAINT, s.sampled, "have samples on record: someone has probably claimed the bonus".into());
    line(ui, icon::X_CIRCLE, pal::FAINT, s.mapped_without, format!("were mapped and {genus} isn't there"));
    line(ui, icon::X_CIRCLE, pal::FAINT, s.populated, "are in populated systems, where there's no bonus".into());
    if s.footfalled > 0 {
        line(ui, icon::X_CIRCLE, pal::FAINT, s.footfalled, "were landed on, according to your journal or EDDN".into());
    }
    if s.ruled_out > 0 {
        line(ui, icon::X_CIRCLE, pal::FAINT, s.ruled_out, format!("don't suit {name} after all"));
    }
    ui.add_space(4.0);
    footnote(
        ui,
        "This is only what commanders share through EDDN (EDMC, EDDiscovery…), collected by Spansh. Anyone who sampled without such a tool, or who already had the species in their codex for this region, left no record, so nothing here is guaranteed.",
    );
}

/// Which candidates to show, applied instantly.
fn bonus_filters(ui: &mut Ui, f: &mut Filters, list: &[Candidate]) {
    let no_record = list.iter().filter(|c| c.evidence == Evidence::NoTrace).count();
    let mapped = list.len() - no_record;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        ui.label(RichText::new("Show").color(pal::DIM));
        widgets::toggle(ui, &mut f.no_record, &format!("No record ({no_record})")).on_hover_text(Evidence::NoTrace.explain());
        widgets::toggle(ui, &mut f.mapped, &format!("Mapped, no samples ({mapped})")).on_hover_text(Evidence::GenusConfirmed.explain());
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        ui.label(RichText::new("Chance").color(pal::DIM));
        let mut pct = (f.min_chance * 100.0).round() as u32;
        if widgets::segmented(ui, &mut pct, &[(0, "Any"), (50, "50 %+"), (80, "80 %+")]) {
            f.min_chance = pct as f32 / 100.0;
        }
        ui.label(RichText::new("From the star").color(pal::DIM));
        widgets::segmented(ui, &mut f.max_ls, &[(0, "Any"), (25_000, "≤ 25,000 ls"), (5_000, "≤ 5,000 ls")]);
        ui.label(RichText::new("Gravity").color(pal::DIM));
        let mut g10 = (f.max_g * 10.0).round() as u32;
        if widgets::segmented(ui, &mut g10, &[(0, "Any"), (10, "≤ 1 g"), (5, "≤ 0.5 g")]) {
            f.max_g = g10 as f32 / 10.0;
        }
    });
}

fn candidates_table(ui: &mut Ui, list: &[&Candidate], action: &mut Option<ExoAction>) {
    Grid::new("untouched").max_col_width(ui.available_width()).num_columns(9).striped(true).spacing([14.0, 10.0]).show(ui, |ui| {
        for t in ["Planet", "Distance", "From star", "Gravity", "Evidence", "Chance", "If first logged", "Last data", ""] {
            th(ui, t);
        }
        ui.end_row();
        for c in list {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let short = c.body.strip_prefix(&c.system).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&c.body);
                ui.label(RichText::new(short).font(display(size::CALLOUT)).color(pal::TEXT));
                ui.label(RichText::new(&c.system).size(size::CAPTION).color(pal::DIM));
            })
            .response
            .on_hover_text(format!(
                "{} · {:.0} K · {} biological signal{}",
                c.atmosphere,
                c.temperature_k.unwrap_or(0.0),
                c.bio_signals,
                if c.bio_signals == 1 { "" } else { "s" },
            ));
            ui.label(RichText::new(format!("{:.0} ly", c.distance_ly)).color(pal::TEXT));
            ui.label(RichText::new(c.distance_ls.map(|d| format!("{} ls", nexus_data::names::group_digits(d.round() as i64))).unwrap_or_else(|| "?".into())).color(pal::DIM));
            ui.label(RichText::new(c.gravity_g.map(|g| format!("{g:.2} g")).unwrap_or_else(|| "?".into())).color(pal::DIM));
            let col = if c.evidence == Evidence::NoTrace { pal::GOOD } else { pal::CYAN };
            badge(ui, c.evidence.label(), col).on_hover_text(c.evidence.explain());
            ui.label(RichText::new(format!("{:.0}%", c.chance * 100.0)).color(pal::TEXT));
            ui.label(RichText::new(short_credits(c.value_first_logged)).font(display(size::CALLOUT)).color(pal::GOLD));
            ui.label(RichText::new(c.last_data.map(|t| widgets::ago(nexus_journal::Timestamp(t))).unwrap_or_else(|| "?".into())).color(pal::DIM))
                .on_hover_text("Newest data Spansh has about this planet");
            ui.horizontal(|ui| {
                if widgets::icon_button(ui, icon::COPY, "Copy the system name").clicked() {
                    ui.ctx().copy_text(c.system.clone());
                }
                if widgets::icon_button(ui, icon::PATH, "Plot a route there").clicked() {
                    *action = Some(ExoAction::PlotTo(c.system.clone()));
                }
            });
            ui.end_row();
        }
    });
}

/// A tour through the best candidates, nearest first.
fn bonus_tour(gs: &GameState, species: &str, list: &[&Candidate]) -> PlannedRoute {
    let total: i64 = list.iter().map(|c| c.value_first_logged).sum();
    let stops = list
        .iter()
        .map(|c| tours::Stop {
            system: c.system.clone(),
            pos: c.system_pos,
            note: format!(
                "{} · {:.0}% · {}",
                c.body.strip_prefix(&c.system).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(&c.body),
                c.chance * 100.0,
                c.evidence.label()
            ),
            value: c.value_first_logged.max(0) as u64,
        })
        .collect();
    let summary = format!("{} planets · {species} · up to {} if first logged", list.len(), short_credits(total));
    tours::tour_through("Bonus hunt", summary, gs.location.star_pos, tours::jump_range(gs), stops)
}

/// "Sure value": Spansh's exobiology planner, planets with species already on record.
fn known_species(ui: &mut Ui, gs: &GameState, st: &mut ExobioUi, spansh: &nexus_spansh::Spansh, rt: &tokio::runtime::Handle) -> Option<ExoAction> {
    let k = &mut st.known;
    k.job.tick();
    let mut action = None;
    card_tinted(ui, icon::PLANT, "Sure value", pal::GOOD, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            ui.label(RichText::new("Within").color(pal::DIM));
            widgets::segmented(ui, &mut k.radius, &[(25, "25 ly"), (50, "50 ly"), (100, "100 ly"), (250, "250 ly"), (500, "500 ly"), (1000, "1,000 ly")]);
            ui.label(RichText::new("Stops").color(pal::DIM));
            widgets::segmented(ui, &mut k.systems, &[(10, "10"), (25, "25"), (50, "50"), (100, "100")]);
            ui.label(RichText::new("Planets worth").color(pal::DIM));
            widgets::segmented(ui, &mut k.min_value, &[(1_000_000, "1 M+"), (5_000_000, "5 M+"), (10_000_000, "10 M+"), (20_000_000, "20 M+")]);
            let busy = k.job.busy();
            let label = if busy { format!("{} Searching…", icon::HOURGLASS) } else { format!("{} Search", icon::MAGNIFYING_GLASS) };
            let clicked = ui.add_enabled_ui(!busy, |ui| widgets::primary_button(ui, &label)).inner.clicked();
            if (clicked || std::mem::take(&mut st.autorun)) && !busy {
                let req = nexus_spansh::ExobiologyRouteRequest::new(gs.location.system.clone(), tours::jump_range(gs)).radius(k.radius).max_results(k.systems).min_value(k.min_value);
                let sp = spansh.clone();
                k.job.run_async(ui.ctx(), rt, async move { tours::known_species(&sp, req).await });
            }
            if busy {
                ui.spinner();
            }
        });
        ui.add_space(4.0);
        if widgets::disclosure(ui, &mut k.show_options, "Options") {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                ui.label("Within");
                ui.add(egui::DragValue::new(&mut k.radius).range(limits::RADIUS_LY).suffix(" ly")).on_hover_text("Spansh searches up to 1,000 ly around you.");
                ui.label("Stops");
                ui.add(egui::DragValue::new(&mut k.systems).range(limits::SYSTEMS)).on_hover_text("Spansh plans up to 2,000 systems.");
                ui.label("Planets worth at least");
                let mut m = k.min_value as f64 / 1e6;
                if ui.add(egui::DragValue::new(&mut m).range(0.0..=200.0).speed(0.1).suffix(" M Cr")).changed() {
                    k.min_value = (m * 1e6).round() as u64;
                }
            });
        }
        ui.add_space(6.0);
        match &k.job.last {
            None => {}
            Some(Err(e)) => {
                ui.add(egui::Label::new(RichText::new(format!("{} {e}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
            }
            Some(Ok(t)) if t.is_empty() => {
                empty(ui, icon::PLANT, &format!("No species on record within {} ly", t.radius), "Spansh found no planets with logged species worth this much, even with the search widened. Lower the value, or search from somewhere else.");
            }
            Some(Ok(t)) => {
                let stops: Vec<_> = t.route.systems.iter().filter(|s| !s.bodies.is_empty()).collect();
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("{} systems · {} planets · ~{} at base value", stops.len(), t.route.body_count(), short_credits(t.route.total_value() as i64)))
                            .font(display(size::HEADLINE))
                            .color(pal::GOLD),
                    );
                    if widgets::primary_button(ui, &format!("{} Follow this route", icon::NAVIGATION_ARROW)).on_hover_text("Shows it on the Navigation page and copies the next system after every jump").clicked() {
                        action = Some(ExoAction::Follow(t.planned("Exobiology")));
                    }
                    ui.hyperlink_to(format!("{} Open on Spansh", icon::ARROW_SQUARE_OUT), &t.web_url);
                });
                if let Some(asked) = t.widened_from {
                    ui.add(egui::Label::new(RichText::new(format!("{} Nothing within {asked} ly, so the search was widened to {} ly.", icon::INFO, t.radius)).color(pal::WARN)).wrap());
                }
                ui.add_space(6.0);
                // The wrapping column goes last: a grid gives the last column the width that's left.
                Grid::new("known-species").max_col_width(ui.available_width()).num_columns(4).striped(true).spacing([14.0, 8.0]).show(ui, |ui| {
                    for h in ["#", "System", "Base value", "Planets and species on record"] {
                        th(ui, h);
                    }
                    ui.end_row();
                    for (i, s) in stops.iter().enumerate() {
                        ui.label(RichText::new((i + 1).to_string()).color(pal::FAINT));
                        if ui.link(RichText::new(&s.name).color(pal::TEXT)).on_hover_text("Click to copy").clicked() {
                            ui.ctx().copy_text(s.name.clone());
                        }
                        ui.label(RichText::new(short_credits(s.value() as i64)).color(pal::GOLD));
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            for b in &s.bodies {
                                let short = b.name.strip_prefix(&s.name).map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&b.name);
                                let species: Vec<String> = b.species().iter().map(|x| format!("{} {}", x.species, short_credits(x.value as i64))).collect();
                                ui.label(RichText::new(format!("{short} · {:.0} ls", b.distance_to_arrival)).color(pal::TEXT));
                                ui.add(egui::Label::new(RichText::new(species.join(" · ")).size(size::FOOTNOTE).color(pal::DIM)).wrap());
                            }
                        });
                        ui.end_row();
                    }
                });
            }
        }
        ui.add_space(2.0);
        ui.label(RichText::new("Route data © Spansh (spansh.co.uk)").size(size::CAPTION).color(pal::FAINT));
    });
    action
}

/// Sampling and unsold data. Returns the time to clear the unsold list up to, once Clear list is confirmed.
fn show_page(ui: &mut Ui, gs: &GameState, confirm_clear: &mut bool) -> Option<Timestamp> {
    let pos = gs.status.as_ref().and_then(|s| s.lat_lon());
    let radius = gs.status.as_ref().and_then(|s| s.planet_radius);
    let heading = gs.status.as_ref().and_then(|s| s.heading);
    let mut clear = None;

    cards(ui, 2, 340.0, |c| {
        c.add(|ui| {
            card_tinted(ui, icon::PLANT, "Sampling", pal::GOOD, |ui| match &gs.exobio.current {
                Some(cur) => sampling(ui, cur, pos, radius, heading),
                None => {
                    ui.label(RichText::new("No sample in progress").font(display(size::TITLE3)).color(pal::TEXT));
                    ui.add(egui::Label::new(RichText::new("Use the genetic sampler on a plant: progress, the distance to the next valid sample and the colony range appear here live.").color(pal::DIM)).wrap());
                }
            });
        });
        c.add(|ui| clear = unsold(ui, &gs.exobio, confirm_clear));
    });
    clear
}

/// What Clear list does, and doesn't (its tooltip, and the text under its question).
const CLEAR_TIP: &str = "This changes only this list in Elite Nexus, nothing in the game or its journals, and it stays cleared when Elite Nexus starts again. Clear it when Vista Genomics has nothing left to sell. \"Sold so far\" can't include sales the game didn't list.";

/// What the unsold list says when the game logged sales without their samples (`sales`, oldest first, at least one),
/// after the oldest sample listed was analysed.
fn unlisted_note(sales: &[Timestamp]) -> String {
    let date = |t: &Timestamp| t.to_local_string("%-d %b %Y");
    let (first, last) = (sales.first().map(date).unwrap_or_default(), sales.last().map(date).unwrap_or_default());
    let when = if first == last { format!("on {first}") } else { format!("between {first} and {last}") };
    let logged = match sales.len() {
        1 => format!("a sale {when} without listing its samples"),
        n => format!("{n} sales {when} without listing their samples"),
    };
    format!("The game logged {logged}, so Elite Nexus can't tell which of these were sold, or for how much. If Vista Genomics has nothing left to sell, use Clear list.")
}

/// The question Clear list asks before it clears `n` samples.
fn clear_question(n: usize) -> String {
    match n {
        1 => "Clear this sample from the list?".to_string(),
        n => format!("Clear these {n} samples from the list?"),
    }
}

/// The unsold Vista Genomics list, with Clear list for samples sold in a sale the game logged without them. Returns the
/// time to clear the list up to, once the commander confirms.
fn unsold(ui: &mut Ui, bio: &ExoBio, confirm: &mut bool) -> Option<Timestamp> {
        let mut clear = None;
        card_tinted(ui, icon::FLASK, "Unsold Vista Genomics data", pal::GOLD, |ui| {
            let total = bio.unsold_value();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 22.0;
                crate::widgets::stat(ui, "estimated payout", short_credits(total), pal::GOLD);
                crate::widgets::stat(ui, "samples", bio.unsold.len().to_string(), pal::TEXT);
                crate::widgets::stat(ui, "sold so far", short_credits(bio.sold_total), pal::DIM);
            });
            ui.add_space(4.0);
            if bio.unsold.is_empty() {
                *confirm = false;
                let text = if bio.cleared.is_some() { "Nothing unsold since you cleared this list." } else { "Everything sold." };
                ui.label(RichText::new(text).color(pal::DIM));
            } else {
                let doubtful: Vec<Timestamp> = bio.doubtful_sales().collect();
                if !doubtful.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(icon::WARNING).color(pal::WARN));
                        ui.add(egui::Label::new(RichText::new(unlisted_note(&doubtful)).color(pal::TEXT)).wrap());
                    });
                    ui.add_space(6.0);
                }
                if *confirm {
                    ui.label(RichText::new(clear_question(bio.unsold.len())).color(pal::TEXT));
                    footnote(ui, CLEAR_TIP);
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        if button(ui, "Clear").clicked() {
                            clear = bio.clear_point();
                            *confirm = false;
                        }
                        if button(ui, "Cancel").clicked() {
                            *confirm = false;
                        }
                    });
                } else if button(ui, "Clear list").on_hover_text(CLEAR_TIP).clicked() {
                    *confirm = true;
                }
                ui.add_space(8.0);
                Grid::new("unsoldbio").max_col_width(ui.available_width()).num_columns(3).striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
                    for r in bio.unsold.iter().rev() {
                        ui.label(RichText::new(&r.species).color(pal::TEXT));
                        ui.label(RichText::new(&r.body_name).color(pal::DIM));
                        let mut t = RichText::new(short_credits(r.expected_payout())).color(pal::GOLD);
                        if r.bonus_expected {
                            t = t.strong();
                        }
                        ui.label(t).on_hover_text(if r.bonus_expected { "Includes the first-logged bonus (5× total)" } else { "Base value" });
                        ui.end_row();
                    }
                });
            }
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new("Organic data is lost if you die. Sell it at Vista Genomics.").size(size::FOOTNOTE).color(pal::DIM)).wrap());
        });
        clear
}

pub fn sampling(ui: &mut Ui, cur: &SampleInProgress, pos: Option<(f64, f64)>, radius: Option<f64>, heading: Option<f64>) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(RichText::new(&cur.species).font(display(size::TITLE))).wrap());
        if let Some(v) = cur.value {
            badge(ui, &short_credits(v), pal::GOLD);
        }
    });
    if let Some(var) = &cur.variant {
        ui.label(RichText::new(var).color(pal::DIM));
    }
    ui.label(RichText::new(format!("{} {}", icon::PLANET, cur.body_name)).color(pal::DIM));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for i in 1..=3u8 {
            let done = cur.stage >= i;
            let col = if done { pal::GOOD } else { pal::FAINT };
            ui.label(RichText::new(if done { icon::CHECK_CIRCLE } else { icon::CIRCLE }).size(22.0).color(col));
        }
        ui.label(RichText::new(format!("Sample {} of 3", cur.stage)).font(display(size::HEADLINE)));
    });
    let range = cur.colony_range_m.map(f64::from);
    if let Some(r) = range {
        ui.add(egui::Label::new(RichText::new(format!("Colony range {r:.0} m: samples must be at least this far apart.")).color(pal::DIM)).wrap());
    }
    let (Some(p), Some(rad)) = (pos, radius) else {
        ui.add(egui::Label::new(RichText::new("Position appears once you're on the surface.").size(size::FOOTNOTE).color(pal::DIM)).wrap());
        return;
    };
    if cur.points.is_empty() {
        ui.add(egui::Label::new(RichText::new("Sample positions are only recorded while Elite Nexus is running.").size(size::FOOTNOTE).color(pal::DIM)).wrap());
        return;
    }
    let nearest = cur.nearest_sample_m(p, rad).unwrap_or(0.0);
    let ok = range.is_none_or(|r| nearest >= r);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (txt, col) = if ok { (format!("{} {:.0} m · OK to sample", icon::CHECK_CIRCLE, nearest), pal::GOOD) } else { (format!("{} {:.0} m · too close", icon::WARNING, nearest), pal::BAD) };
        ui.label(RichText::new(txt).font(display(size::TITLE)).color(col));
    });
    if let Some(r) = range
        && !ok {
            ui.label(RichText::new(format!("Move another {:.0} m away from the nearest sample", (r - nearest).max(0.0))).color(pal::WARN));
        }
    radar(ui, cur, p, rad, heading.unwrap_or(0.0), range.unwrap_or(100.0));
}

/// Top-down radar centred on the commander, "up" = current heading.
fn radar(ui: &mut Ui, cur: &SampleInProgress, me: (f64, f64), radius_m: f64, heading_deg: f64, range: f64) {
    let size = ui.available_width().min(320.0);
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter_at(rect);
    let c = rect.center();
    let r_px = size / 2.0 - 8.0;
    p.circle_filled(c, r_px, pal::FIELD);
    p.circle_stroke(c, r_px, Stroke::new(1.0, pal::SEPARATOR));
    // Offsets of previous samples in metres (local tangent plane: x east, y north).
    let pts: Vec<(f64, f64)> = cur
        .points
        .iter()
        .map(|s| {
            let dlat = (s.lat - me.0).to_radians();
            let dlon = (s.lon - me.1).to_radians();
            let x = dlon * radius_m * me.0.to_radians().cos();
            let y = dlat * radius_m;
            (x, y)
        })
        .collect();
    let max_d = pts.iter().map(|(x, y)| (x * x + y * y).sqrt() + range).fold(range * 1.5, f64::max);
    let scale = r_px as f64 / max_d;
    // rings every colony range
    let mut ring = range;
    while ring < max_d {
        p.circle_stroke(c, (ring * scale) as f32, Stroke::new(1.0, pal::BORDER.gamma_multiply(0.6)));
        ring += range;
    }
    let h = heading_deg.to_radians();
    let to_screen = |(x, y): (f64, f64)| -> Pos2 {
        // rotate world so that heading points up
        let rx = x * h.cos() - y * h.sin();
        let ry = x * h.sin() + y * h.cos();
        Pos2::new(c.x + (rx * scale) as f32, c.y - (ry * scale) as f32)
    };
    for (i, pt) in pts.iter().enumerate() {
        let sp = to_screen(*pt);
        let d = (pt.0 * pt.0 + pt.1 * pt.1).sqrt();
        let inside = d < range;
        let col = if inside { pal::BAD } else { pal::GOOD };
        p.circle_filled(sp, (range * scale) as f32, col.gamma_multiply(0.12));
        p.circle_stroke(sp, (range * scale) as f32, Stroke::new(1.0, col.gamma_multiply(0.7)));
        p.circle_filled(sp, 5.0, col);
        p.text(sp + Vec2::new(8.0, -8.0), egui::Align2::LEFT_BOTTOM, format!("#{}", i + 1), display(size::CAPTION), pal::TEXT);
    }
    // me
    let tri = [c + Vec2::new(0.0, -9.0), c + Vec2::new(6.0, 7.0), c + Vec2::new(-6.0, 7.0)];
    p.add(egui::Shape::convex_polygon(tri.to_vec(), pal::ACCENT, Stroke::NONE));
    p.text(rect.left_top() + Vec2::new(6.0, 4.0), egui::Align2::LEFT_TOP, format!("{:.0} m", max_d), egui::FontId::proportional(size::CAPTION), pal::DIM);
    let _ = Color32::TRANSPARENT;
}

fn bio_bodies(ui: &mut Ui, gs: &GameState) {
    card_tinted(ui, icon::PLANET, "Biological signals in this system", pal::MINT, |ui| {
        let Some(sys) = gs.exploration.current_system() else {
            ui.label(RichText::new("No system data.").color(pal::DIM));
            return;
        };
        let mut bodies: Vec<_> = sys.bodies.values().filter(|b| b.bio_signals > 0 || !b.organics.is_empty()).collect();
        if bodies.is_empty() {
            empty(ui, icon::PLANT, "No biological signals detected", "FSS-scan the system; bodies with life appear here, DSS-map them to reveal the genera.");
            return;
        }
        bodies.sort_by(|a, b| a.distance_ls.total_cmp(&b.distance_ls));
        TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(110.0).clip(true))
            .column(Column::initial(70.0))
            .column(Column::initial(76.0))
            .column(Column::initial(70.0))
            .column(Column::initial(170.0).clip(true))
            .column(Column::remainder().clip(true))
            .header(30.0, |mut h| {
                for t in ["Body", "Signals", "Gravity", "Temp", "Atmosphere", "Genera / species"] {
                    h.col(|ui| th(ui, t));
                }
            })
            .body(|body| {
                body.rows(32.0, bodies.len(), |mut row| {
                    let b = bodies[row.index()];
                    row.col(|ui| {
                        ui.label(b.short_name(&sys.name));
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(b.bio_signals.to_string()).color(pal::GOOD));
                    });
                    row.col(|ui| {
                        if let Some(g) = b.gravity_g() {
                            ui.label(format!("{g:.2} g"));
                        }
                    });
                    row.col(|ui| {
                        if let Some(t) = b.temperature_k {
                            ui.label(format!("{t:.0} K"));
                        }
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(b.atmosphere.clone().unwrap_or_else(|| "—".into())).color(pal::DIM));
                    });
                    row.col(|ui| {
                        ui.horizontal(|ui| {
                            if b.genuses.is_empty() && b.organics.is_empty() {
                                let likely = b.likely_species();
                                if likely.is_empty() {
                                    ui.label(RichText::new("Map with the DSS to reveal the genera").color(pal::DIM));
                                }
                                for p in likely.iter().take(4) {
                                    badge(ui, &format!("{} {:.0}%", p.species, p.likelihood * 100.0), pal::FAINT).on_hover_text(format!("{} Cr · colony {} m", nexus_data::names::group_digits(p.value), p.colony_range_m));
                                }
                            }
                            for (i, g) in b.genuses.iter().enumerate() {
                                match b.organics.iter().find(|o| o.genus.eq_ignore_ascii_case(g)) {
                                    Some(o) if o.analysed => {
                                        badge(ui, &format!("{} {}", icon::CHECK, o.species), pal::GOOD);
                                    }
                                    Some(o) => {
                                        badge(ui, &format!("{} {}/3", o.species, o.samples), pal::WARN);
                                    }
                                    None => {
                                        // Not sampled yet: show the most likely species of this genus. Predictions name genera
                                        // as the exobiology data does (English); `g` is in the game's language.
                                        let known = b.dss_genus(i);
                                        let key = known.map_or(g.as_str(), |x| x.name);
                                        let guess = b.predictions.iter().find(|p| p.genus.eq_ignore_ascii_case(key));
                                        let range = known.map(|x| x.colony_range_m).or_else(|| nexus_data::exobio::colony_range_m(g)).map(|r| format!(" · {r} m")).unwrap_or_default();
                                        match guess {
                                            Some(p) => badge(ui, &format!("{}? {}{range}", p.species, short_credits(p.value)), pal::CYAN),
                                            None => badge(ui, &format!("{g}{range}"), pal::CYAN),
                                        };
                                    }
                                }
                            }
                        });
                    });
                });
            });
    });
    ui.add_space(GAP);
    card_tinted(ui, icon::CHART_BAR, "Totals", pal::BLUE, |ui| {
        Grid::new("biototals").max_col_width(ui.available_width()).num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
            kv(ui, "Species analysed", gs.exobio.analysed_total.to_string());
            kv(ui, "Sold to Vista Genomics", credits(gs.exobio.sold_total));
            kv(ui, "Exobiologist rank", gs.cmdr.rank_name(nexus_data::ranks::RankKind::Exobiologist));
        });
        let _ = surface_distance;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_model::exobio::BioRecord;

    fn record(species: &str, ts: i64) -> BioRecord {
        BioRecord {
            ts: Timestamp(ts),
            genus: "Stratum".into(),
            species: species.into(),
            species_symbol: String::new(),
            variant: None,
            system_address: 100,
            system: "Alpha".into(),
            body_id: 2,
            body_name: "Alpha 2".into(),
            value: 19_010_800,
            bonus_expected: false,
        }
    }

    #[test]
    fn says_what_the_game_left_out() {
        let day = |t: Timestamp| t.to_local_string("%-d %b %Y");
        // Noon UTC on 12 and 14 November 2026.
        let (a, b) = (Timestamp(1_794_484_800), Timestamp(1_794_657_600));
        assert_eq!(
            unlisted_note(&[a]),
            format!("The game logged a sale on {} without listing its samples, so Elite Nexus can't tell which of these were sold, or for how much. If Vista Genomics has nothing left to sell, use Clear list.", day(a))
        );
        assert!(unlisted_note(&[a, Timestamp(a.0 + 6), Timestamp(a.0 + 12)]).starts_with(&format!("The game logged 3 sales on {} without listing their samples,", day(a))));
        assert!(unlisted_note(&[a, b]).starts_with(&format!("The game logged 2 sales between {} and {} without", day(a), day(b))));
        assert_eq!(clear_question(1), "Clear this sample from the list?");
        assert_eq!(clear_question(74), "Clear these 74 samples from the list?");
    }

    /// The unsold card drawn headless, with its buttons and text found the way a screen reader finds them.
    struct Card {
        ctx: egui::Context,
        bio: ExoBio,
        confirm: bool,
        /// What a confirmed Clear returned.
        cleared: Option<Timestamp>,
        /// The buttons of the last drawing: label and centre.
        buttons: Vec<(String, egui::Pos2)>,
        /// Every text of the last drawing.
        texts: Vec<String>,
    }

    impl Card {
        fn new(bio: ExoBio) -> Card {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            ctx.enable_accesskit();
            let mut card = Card { ctx, bio, confirm: false, cleared: None, buttons: Vec::new(), texts: Vec::new() };
            card.draw(Vec::new());
            card
        }

        fn draw(&mut self, events: Vec<egui::Event>) {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 900.0))), events, ..Default::default() };
            let Card { ctx, bio, confirm, cleared, .. } = self;
            let mut out = ctx.run_ui(input, |ui| {
                if let Some(t) = unsold(ui, bio, confirm) {
                    *cleared = Some(t);
                }
            });
            out.textures_delta.clear();
            let nodes = out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default();
            self.buttons = nodes
                .iter()
                .filter(|(_, n)| n.role() == egui::accesskit::Role::Button)
                .filter_map(|(_, n)| Some((n.label()?.to_string(), n.bounds()?)))
                .map(|(label, b)| (label, egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)))
                .collect();
            self.texts = nodes.iter().filter_map(|(_, n)| n.label().or_else(|| n.value()).map(str::to_string)).collect();
        }

        fn buttons(&self) -> Vec<&str> {
            let mut labels: Vec<&str> = self.buttons.iter().map(|(l, _)| l.as_str()).collect();
            labels.sort_unstable();
            labels
        }

        fn shows(&self, text: &str) -> bool {
            self.texts.iter().any(|t| t.contains(text))
        }

        fn click(&mut self, label: &str) {
            let pos = self.buttons.iter().find(|(l, _)| l == label).unwrap_or_else(|| panic!("no {label} button in {:?}", self.buttons)).1;
            let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
            self.draw(vec![egui::Event::PointerMoved(pos), button(true)]);
            self.draw(vec![button(false)]);
            self.draw(Vec::new());
        }
    }

    /// Clear list asks first; Cancel leaves the list alone; Clear gives the newest of the samples and the unlisted
    /// sales, and the list cleared there is empty and says so.
    #[test]
    fn the_unsold_card_clicked_through() {
        let bio = ExoBio { unsold: vec![record("Stratum Tectonicas", 100), record("Bacterium Cerbrus", 200)], unlisted_sales: vec![Timestamp(300)], ..Default::default() };
        let mut card = Card::new(bio);
        assert!(card.shows("The game logged a sale on"), "{:?}", card.texts);
        assert_eq!(card.buttons(), ["Clear list"]);
        card.click("Clear list");
        assert!(card.shows("Clear these 2 samples from the list?"), "{:?}", card.texts);
        assert!(card.shows("nothing in the game or its journals"));
        assert_eq!(card.buttons(), ["Cancel", "Clear"]);
        card.click("Cancel");
        assert_eq!((card.buttons(), card.cleared), (vec!["Clear list"], None));
        card.click("Clear list");
        card.click("Clear");
        assert_eq!(card.cleared, Some(Timestamp(300)));
        // The app clears the model's list there.
        card.bio.clear(Timestamp(300));
        card.draw(Vec::new());
        assert!(card.buttons().is_empty());
        assert!(card.shows("Nothing unsold since you cleared this list."), "{:?}", card.texts);

        // No unlisted sale: Clear list is there all the same, without the note.
        let card = Card::new(ExoBio { unsold: vec![record("Stratum Tectonicas", 100)], ..Default::default() });
        assert_eq!(card.buttons(), ["Clear list"]);
        assert!(!card.shows("The game logged"));
        assert!(Card::new(ExoBio::default()).shows("Everything sold."));
    }
}
