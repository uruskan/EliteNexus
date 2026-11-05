//! Fleet › Plan a loadout: a hull and what's in each slot, what it comes to as you change it (jump range, speed,
//! shields, armour, power, mass, cost), and where to buy what's new. Builds can be saved, compared and shared.
//! Figures: `nexus_data::fitting`.

use crate::theme::{display, pal, size};
use crate::widgets::{self, badge, card, card_with, footnote, short_credits};
use compare::{Choice, CompareWith, Pick};
use egui::{Color32, Label, RichText, Sense, Ui};
use egui_phosphor::regular as icon;
use nexus_data::fitting::{self, Figures, Fitted, Goal, Hull, Live, ModuleData, Plan, Setup, SlotDef};
use nexus_data::outfitting::{self, SlotKind};
use nexus_model::fleet::Ship;
use nexus_model::GameState;
use panels::{Panel, PowerEdit, Reported};
use saved::{Builds, SavedBuild};

mod compare;
mod hulls;
mod panels;
mod saved;

/// The planner's state.
#[derive(Default)]
pub struct PlanUi {
    plan: Option<Plan>,
    /// What the plan started from: Undo changes goes back to it, and a module picked back comes back as it was there.
    start: Option<Plan>,
    /// The commander's ship the plan is for, as it is: the figures and what's new go against it. None: against the
    /// hull as a shipyard sells it.
    base: Option<Plan>,
    /// That ship's id, kept with a saved build.
    ship: Option<u64>,
    /// "your Kestrel Mk II", "a new Anaconda", "a pasted build".
    source: String,
    /// What the game reported for the commander's ship, to show beside the sums.
    game: Option<Reported>,
    /// The slot whose module list is open; "Armour" for the bulkheads' engineering.
    open: Option<String>,
    filter: String,
    hull_text: String,
    paste: String,
    error: Option<String>,
    /// The right side's panel, the pips, what's carried and the discount; the ship the weapons are tried against, and
    /// what's typed to pick it.
    panel: Panel,
    setup: Setup,
    target: String,
    target_with: Option<CompareWith>,
    /// Saved builds; the name the plan was saved or opened under; the Save field while it's open.
    builds: Builds,
    saved_as: Option<String>,
    save_name: Option<String>,
    /// A saved build being renamed (its place in the list, the name typed), and one waiting for Delete's second click.
    renaming: Option<(usize, String)>,
    deleting: Option<usize>,
    /// Why a rename didn't happen.
    rename_problem: Option<String>,
    /// The name a build was just saved under, so what shows it elsewhere is read again.
    just_saved: Option<String>,
    /// What the Compare panel puts the plan beside (None: the ship as it is, or the hull as a shipyard sells it), and
    /// what's typed to pick another.
    compare: Option<CompareWith>,
    compare_text: String,
}

/// What a click in the planner asks the Fleet page to do.
pub enum PlanOut {
    /// Find stations that sell what's new in this build (a journal Loadout).
    Buy { loadout: String, source: String },
    /// Find stations that sell one module.
    BuyOne(String),
}

impl PlanUi {
    /// Starts from one of the commander's ships: changes to it, with what's on it now to compare with. A ship without
    /// a recorded build says why on the start screen.
    pub fn start_from_ship(&mut self, s: &Ship) {
        match plan_of(s) {
            Ok(plan) => {
                self.begin(plan.clone(), Some(plan), Some(s), None);
            }
            Err(e) => {
                self.plan = None;
                self.error = Some(e);
            }
        }
    }

    /// Plans `plan`, which Undo changes goes back to, against `base` (the commander's ship `ship` as it is) or else
    /// the hull as a shipyard sells it; `saved_as`: the saved build it is.
    fn begin(&mut self, plan: Plan, base: Option<Plan>, ship: Option<&Ship>, saved_as: Option<String>) {
        let hull = fitting::hull(&plan.hull).map_or_else(|| nexus_data::ships::ship_name(&plan.hull), |h| h.name.clone());
        self.start = Some(plan.clone());
        self.plan = Some(plan);
        self.base = base.filter(|_| ship.is_some());
        let ship = ship.filter(|_| self.base.is_some());
        self.ship = ship.map(|s| s.id);
        self.game = ship.map(|s| Reported { mass: s.unladen_mass, range: s.max_jump_range, value: s.hull_value + s.modules_value, rebuy: s.rebuy });
        self.source = match ship {
            Some(s) => format!("your {}", s.display_name()),
            None => format!("a new {hull}"),
        };
        self.saved_as = saved_as;
        (self.save_name, self.renaming, self.deleting, self.rename_problem) = (None, None, None, None);
        self.compare = None;
        self.open = None;
        self.error = None;
    }

    /// Opens a saved build: against the ship it was planned from while that ship's build is recorded, else against
    /// the hull as a shipyard sells it.
    fn open_saved(&mut self, gs: &GameState, b: &SavedBuild) {
        let Some(plan) = b.to_plan() else {
            self.error = Some(format!("The saved build \u{201c}{}\u{201d} can't be read.", b.name));
            return;
        };
        if fitting::hull(&plan.hull).is_none() {
            self.error = Some(format!("This version of Elite Nexus has no figures for the {} yet.", b.hull()));
            return;
        }
        let ship = b.ship.and_then(|id| gs.fleet.ships.get(&id)).filter(|s| s.ship_type.eq_ignore_ascii_case(&plan.hull));
        let base = ship.and_then(|s| plan_of(s).ok());
        self.begin(plan, base, ship, Some(b.name.clone()));
    }

    /// For a screenshot run: the frame shift drive's list open, with Increased FSD Range to grade 5 and Mass Manager
    /// planned for it, so the engineering and its materials show.
    pub fn demo_fsd(&mut self) {
        let Some(plan) = self.plan.as_mut() else { return };
        if let Some(f) = plan.fitted.iter_mut().find(|f| f.slot == "FrameShiftDrive") {
            f.goal = Some(Goal { experimental: Some("special_fsd_heavy".into()), ..Goal::new("FSD_LongRange", 5) });
            self.open = Some(f.slot.clone());
        }
    }

    /// For a screenshot run: one of the right side's panels. `compare` puts the plan beside the hull as a shipyard
    /// sells it; `weapons` tries them against an Anaconda as sold.
    pub fn show_panel(&mut self, name: &str) {
        self.panel = match name {
            "defence" => Panel::Defence,
            "weapons" => Panel::Weapons,
            "power" => Panel::Power,
            "travel" => Panel::Travel,
            "compare" => Panel::Compare,
            _ => Panel::Overview,
        };
        if self.panel == Panel::Compare
            && let Some(h) = self.plan.as_ref().and_then(|p| fitting::hull(&p.hull))
        {
            self.compare = Some(new_hull(h, false));
        }
        // Weapons against a ship to show what that takes: an Anaconda as a shipyard sells it.
        if self.panel == Panel::Weapons
            && let Some(h) = fitting::hull("anaconda")
        {
            self.target_with = Some(new_hull(h, true));
        }
    }

    /// Starts from a new hull by its journal symbol (a screenshot run's new Python).
    pub fn start_new(&mut self, symbol: &str) {
        if let Some(h) = fitting::hull(symbol) {
            self.start_stock(h);
        }
    }

    fn start_stock(&mut self, hull: &Hull) {
        self.begin(Plan::stock(hull), None, None, None);
    }

    fn start_pasted(&mut self, text: &str) -> Result<(), String> {
        let build = crate::shopping::parse_build(text)?;
        let value: serde_json::Value = serde_json::from_str(&build.loadout).map_err(|_| "That build can't be read.".to_string())?;
        let plan = Plan::from_loadout(&value).ok_or("That build can't be read.")?;
        if fitting::hull(&plan.hull).is_none() {
            return Err(format!("This version of Elite Nexus has no figures for the {} yet.", nexus_data::ships::ship_name(&plan.hull)));
        }
        self.begin(plan, None, None, None);
        self.source = "a pasted build".into();
        self.paste.clear();
        Ok(())
    }

    /// The plan is for one of the commander's ships, so it goes against that ship as it is.
    fn owned(&self) -> bool {
        self.base.is_some()
    }

    /// What the figures and what's new go against: the commander's ship as it is, else the hull as a shipyard sells
    /// it.
    fn reference(&self, hull: &Hull) -> Option<Plan> {
        match &self.base {
            Some(b) => Some(b.clone()),
            None => Some(Plan::stock(hull)),
        }
    }

    /// What the Compare panel shows the plan beside: the one picked, else the reference.
    fn compare_with(&self, hull: &Hull) -> CompareWith {
        if let Some(c) = &self.compare {
            return c.clone();
        }
        match &self.base {
            Some(b) => CompareWith { label: format!("{} as it is", capitalise(&self.source)), short: "As it is".into(), plan: b.clone(), pick: Some(Pick::AsItIs) },
            None => new_hull(hull, false),
        }
    }
}

/// A hull as a shipyard sells it, to compare with ("A new Python") or to try the weapons against ("Python, as a
/// shipyard sells it").
fn new_hull(h: &Hull, target: bool) -> CompareWith {
    let (label, short) = if target { (format!("{}, as a shipyard sells it", h.name), h.name.clone()) } else { (format!("A new {}", h.name), "New hull".to_string()) };
    CompareWith { label, short, plan: Plan::stock(h), pick: Some(Pick::NewHull(h.symbol.clone())) }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// "A new Python" → "a new Python", "Your Kestrel…" → "your Kestrel…"; a saved build's name stays as it is.
fn lower_first(s: &str) -> String {
    for lead in ["A new ", "Your "] {
        if let Some(rest) = s.strip_prefix(lead) {
            return format!("{}{rest}", lead.to_lowercase());
        }
    }
    s.to_string()
}

/// A ship's recorded build as a plan.
fn plan_of(s: &Ship) -> Result<Plan, String> {
    let json = s.loadout_json.as_deref().ok_or_else(|| format!("No recorded build for your {} yet: the game writes a ship's modules to its journal only while you fly it. Fly it once and it can be planned.", s.display_name()))?;
    let value: serde_json::Value = serde_json::from_str(json).map_err(|_| "This ship's recorded build can't be read.".to_string())?;
    let plan = Plan::from_loadout(&value).ok_or("This ship's recorded build can't be read.")?;
    if fitting::hull(&plan.hull).is_none() {
        return Err(format!("This version of Elite Nexus has no figures for the {} yet.", s.type_name));
    }
    Ok(plan)
}

pub fn show(ui: &mut Ui, gs: &GameState, st: &mut PlanUi) -> Option<PlanOut> {
    let Some(plan) = st.plan.clone() else {
        egui::ScrollArea::vertical().id_salt("plan-start").auto_shrink([false, false]).show(ui, |ui| start(ui, gs, st));
        return None;
    };
    let Some(hull) = fitting::hull(&plan.hull) else {
        st.plan = None;
        return None;
    };
    let figures = fitting::figures_with(&plan, &st.setup).unwrap_or_default();
    let reference = st.reference(hull);
    let before = reference.as_ref().and_then(|r| fitting::figures_with(r, &st.setup));
    let mut out = None;
    egui::ScrollArea::vertical().id_salt("plan").auto_shrink([false, false]).show(ui, |ui| {
        out = header(ui, gs, st, hull, &plan, &figures);
        // A build saved over: what Compare and the weapons' target show of it is read again.
        if let Some(name) = st.just_saved.take() {
            let again = |c: &Option<CompareWith>| c.as_ref().and_then(|c| c.pick.clone()).filter(|p| matches!(p, Pick::Saved(n) if n.eq_ignore_ascii_case(&name)));
            if let Some(p) = again(&st.compare) {
                st.compare = resolve(gs, st, &p, None, false);
            }
            if let Some(p) = again(&st.target_with) {
                st.target_with = resolve(gs, st, &p, None, true);
            }
        }
        // Modules newer than this data count as nothing: say which.
        if !figures.unknown.is_empty() {
            let names: Vec<String> = figures.unknown.iter().map(|s| outfitting::display_name(s)).collect();
            ui.add_space(6.0);
            ui.add(Label::new(RichText::new(format!("{} No figures for {}: newer than this version's data, so their mass, power and the rest aren't counted.", icon::WARNING, names.join(", "))).color(pal::WARN)).wrap());
        }
        ui.add_space(widgets::GAP);
        let (base, owned, source, game) = (st.base.clone(), st.owned(), st.source.clone(), st.game);
        // What Compare puts the plan beside, and what else it could.
        let with = st.compare_with(hull);
        let others = if st.panel == Panel::Compare {
            st.builds.ensure_loaded();
            compare_options(gs, st, hull)
        } else {
            Vec::new()
        };
        // What the weapons can be tried against, and the one picked, as it comes to.
        let targets = if st.panel == Panel::Weapons {
            st.builds.ensure_loaded();
            target_options(gs, st)
        } else {
            Vec::new()
        };
        let chosen = st.target_with.as_ref().and_then(|t| fitting::figures(&t.plan).map(|f| (t.label.clone(), f)));
        // The right side's settings change here and go back to the state after the frame.
        let (mut panel, mut setup, mut target, mut compare_text) = (st.panel, st.setup, std::mem::take(&mut st.target), std::mem::take(&mut st.compare_text));
        let (mut changed, mut from_slots, mut from_list, mut power_edit, mut compare_pick, mut target_pick) = (None, None, None, None, None, None);
        widgets::split(
            ui,
            0.5,
            420.0,
            |ui| changed = slots(ui, gs, st, hull, &plan, reference.as_ref(), &mut from_slots),
            |ui| {
                panels::tabs(ui, &mut panel);
                ui.add_space(widgets::GAP);
                if panel.uses_setup() {
                    panels::setup_bar(ui, &mut setup, &figures, panel);
                    ui.add_space(widgets::GAP);
                }
                match panel {
                    Panel::Overview => {
                        // The game's own figures beside the sums, while the plan is still the ship as it is.
                        let as_now = owned && base.as_ref() == Some(&plan);
                        panels::overview(ui, hull, &plan, &figures, before.as_ref(), owned, game.filter(|_| as_now));
                        ui.add_space(widgets::GAP);
                        from_list = whats_new(ui, hull, reference.as_ref(), owned, &source, &plan, setup.discount);
                        materials_card(ui, gs, &plan);
                    }
                    Panel::Defence => panels::defence(ui, &figures),
                    Panel::Weapons => target_pick = panels::weapons(ui, &figures, &mut target, chosen.as_ref().map(|(l, f)| (l.as_str(), f)), &targets),
                    Panel::Power => power_edit = panels::power(ui, &plan, &figures),
                    Panel::Travel => panels::travel(ui, &figures),
                    Panel::Compare => compare_pick = compare_panel(ui, hull, &figures, &with, &others, &setup, &mut compare_text),
                }
            },
        );
        (st.panel, st.setup, st.target, st.compare_text) = (panel, setup, target, compare_text);
        if let Some((pick, label)) = compare_pick {
            st.compare = resolve(gs, st, &pick, Some(&label), false);
        }
        if let Some((pick, label)) = target_pick {
            st.target_with = resolve(gs, st, &pick, Some(&label), true);
        }
        if let Some(e) = power_edit {
            let mut p = changed.take().unwrap_or_else(|| plan.clone());
            match e {
                PowerEdit::Priority(slot, g) => p.fitted.iter_mut().filter(|f| f.slot == slot).for_each(|f| f.priority = g),
                PowerEdit::On(slot, on) => p.fitted.iter_mut().filter(|f| f.slot == slot).for_each(|f| f.on = on),
                PowerEdit::HatchPriority(g) => p.hatch_priority = g,
                PowerEdit::Hatch(on) => p.hatch = on,
            }
            changed = Some(p);
        }
        if let Some(p) = changed {
            st.plan = Some(p);
        }
        out = out.take().or(from_slots).or(from_list);
        ui.add_space(8.0);
        footnote(
            ui,
            "Hull, module and engineering figures come from EDCD's coriolis-data (October 2026), summed the way the game sums them: an unengineered Kestrel Mk II comes to the unladen mass and jump range its journal reports. Engineering read from your ship keeps the game's own engineered values; engineering you plan here is worked out from each grade's range at the roll you pick. Prices are list prices, less the discount set on Overview: a station may sell for less, and the game may have changed a price since.",
        );
    });
    out
}

/// No plan yet: pick a hull, one of your ships or a saved build, or paste a build; or pick a hull from the table of
/// them all.
fn start(ui: &mut Ui, gs: &GameState, st: &mut PlanUi) {
    st.builds.ensure_loaded();
    start_card(ui, gs, st);
    if st.plan.is_some() {
        return;
    }
    if !st.builds.list.is_empty() || st.builds.problem.is_some() || st.builds.notice.is_some() {
        ui.add_space(widgets::GAP);
        saved_card(ui, gs, st);
        if st.plan.is_some() {
            return;
        }
    }
    ui.add_space(widgets::GAP);
    card(ui, "Every hull", |ui| {
        ui.add(Label::new(RichText::new("Sort them by price, speed, shields, armour, mass or slots; a ship's name starts a plan from it as a shipyard sells it.").color(pal::DIM)).wrap());
        ui.add_space(6.0);
        if let Some(h) = hulls::table(ui) {
            st.start_stock(h);
        }
    });
}

/// The saved builds: open, rename or delete each.
fn saved_card(ui: &mut Ui, gs: &GameState, st: &mut PlanUi) {
    card(ui, "Your saved builds", |ui| {
        if let Some(p) = &st.builds.problem {
            ui.add(Label::new(RichText::new(format!("{} {p}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
            ui.add_space(4.0);
        }
        if let Some(n) = &st.builds.notice {
            ui.add(Label::new(RichText::new(format!("{} {n}", icon::INFO)).color(pal::WARN)).wrap());
            ui.add_space(4.0);
        }
        let (mut open, mut rename, mut delete, mut cancel) = (None, None, None, false);
        let list = &st.builds.list;
        let field = |i: usize| egui::Id::new(("plan-rename", i));
        widgets::table(ui, "plan-saved", |t| {
            t.vscroll(false)
                .column(widgets::flex_column(160.0))
                .column(egui_extras::Column::initial(150.0).at_least(110.0))
                .column(egui_extras::Column::initial(110.0).at_least(96.0))
                .column(egui_extras::Column::exact(180.0).resizable(false))
                .header(28.0, |mut h| {
                    for t in ["Build", "Ship", "Saved", ""] {
                        h.col(|ui| widgets::th(ui, t));
                    }
                })
                .body(|body| {
                    body.rows(38.0, list.len(), |mut row| {
                        let i = row.index();
                        let Some(b) = list.get(i) else { return };
                        // The name, or while it's renamed the field to type it in: Enter keeps it, Escape doesn't.
                        row.col(|ui| match &mut st.renaming {
                            Some((at, text)) if *at == i => {
                                let edit = ui.add(egui::TextEdit::singleline(text).id(field(i)).desired_width(ui.available_width() - 8.0));
                                if edit.lost_focus() {
                                    if ui.input(|k| k.key_pressed(egui::Key::Enter)) {
                                        rename = Some((i, text.clone()));
                                    } else if ui.input(|k| k.key_pressed(egui::Key::Escape)) {
                                        cancel = true;
                                    }
                                }
                            }
                            _ => {
                                ui.add(Label::new(RichText::new(&b.name).color(pal::TEXT)).truncate());
                            }
                        });
                        row.col(|ui| {
                            let mine = b.ship.and_then(|id| gs.fleet.ships.get(&id)).map(|s| format!(", planned from your {}", s.display_name())).unwrap_or_default();
                            ui.add(Label::new(b.hull()).truncate()).on_hover_text(format!("{}{mine}", b.hull()));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(nexus_journal::Timestamp(b.saved).to_local_string("%d %b %Y")).color(pal::DIM));
                        });
                        row.col(|ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                // Delete asks again; rename has its own buttons.
                                if st.deleting == Some(i) {
                                    if widgets::button(ui, &format!("{} Delete", icon::TRASH)).on_hover_text("Delete this saved build for good").clicked() {
                                        delete = Some(i);
                                    }
                                    if widgets::button(ui, "Keep").clicked() {
                                        st.deleting = None;
                                    }
                                    return;
                                }
                                if let Some((at, text)) = &st.renaming
                                    && *at == i
                                {
                                    if widgets::button(ui, "Rename").clicked() {
                                        rename = Some((i, text.clone()));
                                    }
                                    if widgets::button(ui, "Cancel").clicked() {
                                        cancel = true;
                                    }
                                    return;
                                }
                                if widgets::button(ui, "Open").on_hover_text("Plan from this build").clicked() {
                                    open = Some(i);
                                }
                                if widgets::icon_button(ui, icon::PENCIL_SIMPLE, "Rename").clicked() {
                                    st.renaming = Some((i, b.name.clone()));
                                    st.deleting = None;
                                    ui.memory_mut(|m| m.request_focus(field(i)));
                                }
                                if widgets::icon_button(ui, icon::TRASH, "Delete").clicked() {
                                    st.deleting = Some(i);
                                    st.renaming = None;
                                }
                            });
                        });
                    });
                });
        });
        if cancel {
            st.renaming = None;
        }
        if let Some(p) = &st.rename_problem {
            ui.add(Label::new(RichText::new(format!("{} {p}", icon::WARNING_CIRCLE)).color(pal::WARN)).wrap());
        }
        footnote(ui, "Saved on this computer, in Elite Nexus's data folder (builds.json), with the engineering planned for each.");
        if let Some((i, name)) = rename {
            // A name taken already keeps the field open, saying so.
            match st.builds.rename(i, &name) {
                Ok(()) => {
                    st.renaming = None;
                    st.rename_problem = None;
                }
                Err(e) => st.rename_problem = Some(e),
            }
        }
        if cancel || st.renaming.is_none() {
            st.rename_problem = None;
        }
        // A failed delete keeps the build, and the card says why (builds.problem).
        if let Some(i) = delete
            && st.builds.delete(i).is_ok()
        {
            st.deleting = None;
        }
        if let Some(b) = open.and_then(|i| st.builds.list.get(i).cloned()) {
            st.open_saved(gs, &b);
        }
    });
}

/// Where to start: a new hull, one of your ships, or a pasted build.
fn start_card(ui: &mut Ui, gs: &GameState, st: &mut PlanUi) {
    card(ui, "Plan a loadout", |ui| {
        ui.add(Label::new(RichText::new("Plan a ship's modules and see what they come to as you change them: jump range, speed, shields, armour, power, mass and cost. Then find the stations that sell what you'd buy.").color(pal::DIM)).wrap());
        ui.add_space(10.0);
        ui.label(RichText::new("Start from a new hull").font(display(size::CALLOUT)));
        ui.add_space(4.0);
        // In a row, so the field's ▾ (the whole list) sits beside it.
        let done = ui
            .horizontal(|ui| {
                widgets::combobox(ui, "plan-hull", &mut st.hull_text, "Type a ship: Python, Mandalay, Type-9…", 360.0, |typed| {
                    let k = nexus_data::commodities::name_key(typed);
                    fitting::hulls()
                        .iter()
                        .filter(|h| k.is_empty() || nexus_data::commodities::name_key(&h.name).contains(&k))
                        .map(|h| widgets::Suggestion { text: h.name.clone(), detail: format!("{} pad · {}", h.pad, short_credits(hulls::shipyard_price(h) as i64)), starred: false })
                        .collect()
                })
            })
            .inner;
        if let Some(name) = done.picked
            && let Some(h) = fitting::hulls().iter().find(|h| h.name == name)
        {
            st.start_stock(h);
            return;
        }
        let mut mine: Vec<&Ship> = gs.fleet.ships.values().filter(|s| s.loadout_json.is_some()).collect();
        mine.sort_by(|a, b| (Some(b.id) == gs.fleet.current_ship_id).cmp(&(Some(a.id) == gs.fleet.current_ship_id)).then(b.total_value().cmp(&a.total_value())));
        if !mine.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new("Or plan changes to one of your ships").font(display(size::CALLOUT)));
            ui.add_space(4.0);
            let mut pick = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for s in &mine {
                    if widgets::pill(ui, false, &super::ship_label(s)).clicked() {
                        pick = Some(*s);
                    }
                }
            });
            if let Some(s) = pick {
                st.start_from_ship(s);
            }
            let without = gs.fleet.ships.len() - mine.len();
            if without > 0 {
                footnote(ui, &format!("{without} more of your ships have no recorded build: the game writes a ship's modules to its journal only while you fly it. Fly one once and it appears here."));
            }
        }
        ui.add_space(12.0);
        ui.label(RichText::new("Or paste a build").font(display(size::CALLOUT)));
        ui.add_space(4.0);
        let edit = ui.add(egui::TextEdit::multiline(&mut st.paste).hint_text("A SLEF export from EDSY or Coriolis, a link from either that opens a build, or a build copied on the Ships tab").desired_rows(2).desired_width(f32::INFINITY));
        if edit.changed() && !st.paste.trim().is_empty() {
            let text = st.paste.clone();
            if let Err(e) = st.start_pasted(&text) {
                st.error = Some(e);
            }
        }
        if let Some(e) = &st.error {
            ui.add_space(4.0);
            ui.add(Label::new(RichText::new(format!("{} {e}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
        }
    });
}

/// The hull's name, where the plan came from, and what to do with it: find where to buy it, save it, share it, undo
/// the changes, start over.
fn header(ui: &mut Ui, gs: &GameState, st: &mut PlanUi, hull: &Hull, plan: &Plan, f: &Figures) -> Option<PlanOut> {
    let mut out = None;
    let mut reset = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        ui.label(RichText::new(&hull.name).font(display(22.0)));
        badge(ui, &format!("{} pad", hull.pad), pal::GRAY);
        let mut what = Vec::new();
        if let Some(name) = &st.saved_as {
            what.push(format!("\u{201c}{name}\u{201d}"));
        }
        if st.owned() {
            what.push(format!("changes to {}", st.source));
        } else if st.saved_as.is_none() {
            what.push(st.source.clone());
        }
        what.push(short_credits(f.total_cost() as i64));
        ui.label(RichText::new(what.join(" · ")).color(pal::DIM));
    });
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        let loadout = plan.to_loadout().to_string();
        if widgets::primary_button(ui, &format!("{} Where to buy it", icon::STOREFRONT)).on_hover_text("Find the nearest stations that sell what you'd buy for this build: what you don't have already, on a ship of yours of this type or in your storage").clicked() {
            out = Some(PlanOut::Buy { loadout: loadout.clone(), source: format!("planned ({})", st.saved_as.clone().unwrap_or_else(|| st.source.clone())) });
        }
        if widgets::button(ui, &format!("{} Save", icon::FLOPPY_DISK)).on_hover_text("Keep this build on this computer, with the engineering planned for it, to open, compare or share later").clicked() {
            st.builds.ensure_loaded();
            let next = if st.save_name.is_some() { None } else { Some(st.saved_as.clone().unwrap_or_else(|| default_name(gs, st, hull))) };
            st.save_name = next;
        }
        // Shared with the engineering planned here, which EDSY and Coriolis show; Where to buy needs only the modules.
        share_menu(ui, &plan.to_loadout_planned().to_string());
        if st.start.is_some() && st.plan.as_ref() != st.start.as_ref() && widgets::button(ui, &format!("{} Undo changes", icon::ARROW_COUNTER_CLOCKWISE)).on_hover_text(format!("Back to {}", st.saved_as.as_ref().map_or_else(|| st.source.clone(), |n| format!("\u{201c}{n}\u{201d} as it was saved")))).clicked() {
            st.plan = st.start.clone();
        }
        reset = widgets::button(ui, "Start over").on_hover_text("Pick another hull, ship or saved build").clicked();
    });
    if st.save_name.is_some() {
        save_row(ui, st, plan);
    }
    if reset {
        // The saved builds stay read.
        let builds = std::mem::take(&mut st.builds);
        *st = PlanUi { builds, ..PlanUi::default() };
    }
    out
}

/// The saved builds and the commander's ships (other than the one the plan is for, when `others_only`) that this
/// version has figures for, as choices.
fn build_choices(gs: &GameState, st: &PlanUi, others_only: bool) -> Vec<Choice> {
    let mut v = Vec::new();
    let known = |symbol: &str| fitting::hull(symbol).is_some();
    for b in st.builds.list.iter().filter(|b| !others_only || st.saved_as.as_deref() != Some(b.name.as_str())) {
        if known(b.loadout.get("Ship").and_then(serde_json::Value::as_str).unwrap_or_default()) {
            v.push(Choice { label: format!("{} (saved)", b.name), detail: format!("Saved build · {}", b.hull()), pick: Pick::Saved(b.name.clone()) });
        }
    }
    let mut mine: Vec<&Ship> = gs.fleet.ships.values().filter(|s| s.loadout_json.is_some() && known(&s.ship_type) && !(others_only && Some(s.id) == st.ship)).collect();
    mine.sort_by_key(|s| s.display_name().to_lowercase());
    for s in mine {
        let ident = s.ident.trim();
        v.push(Choice { label: format!("Your {}", s.display_name()), detail: if ident.is_empty() { "Your ship, as the game last reported it".into() } else { format!("Your ship {ident}, as the game last reported it") }, pick: Pick::Ship(s.id) });
    }
    v
}

/// What Compare can put the plan beside: the ship as it is, the hull as a shipyard sells it, the other saved builds,
/// and the commander's other ships with a recorded build.
fn compare_options(gs: &GameState, st: &PlanUi, hull: &Hull) -> Vec<Choice> {
    let mut v = Vec::new();
    if st.base.is_some() {
        v.push(Choice { label: format!("{} as it is", capitalise(&st.source)), detail: "Your ship, as the game last reported it".into(), pick: Pick::AsItIs });
    }
    v.push(Choice { label: format!("A new {}", hull.name), detail: "As a shipyard sells it".into(), pick: Pick::NewHull(hull.symbol.clone()) });
    v.extend(build_choices(gs, st, true));
    compare::unique(v)
}

/// What the weapons can be tried against: the saved builds and the commander's ships with a recorded build, then
/// every hull as a shipyard sells it.
fn target_options(gs: &GameState, st: &PlanUi) -> Vec<Choice> {
    let mut v = build_choices(gs, st, false);
    v.extend(fitting::hulls().iter().map(|h| Choice { label: h.name.clone(), detail: format!("Hardness {:.0}", h.hardness), pick: Pick::NewHull(h.symbol.clone()) }));
    compare::unique(v)
}

/// The build `pick` names, to compare with or (`target`) to try the weapons against; `label`: the name it was picked
/// by, if it was.
fn resolve(gs: &GameState, st: &PlanUi, pick: &Pick, label: Option<&str>, target: bool) -> Option<CompareWith> {
    let (plan, label, short) = match pick {
        Pick::AsItIs => (st.base.clone()?, format!("{} as it is", capitalise(&st.source)), "As it is".to_string()),
        Pick::NewHull(symbol) => return fitting::hull(symbol).map(|h| new_hull(h, target)),
        Pick::Saved(name) => {
            let b = st.builds.find(name)?;
            let label = if target { format!("{}, saved ({})", b.name, b.hull()) } else { format!("{} (saved)", b.name) };
            (b.to_plan()?, label, b.name.clone())
        }
        Pick::Ship(id) => {
            let s = gs.fleet.ships.get(id)?;
            let name = s.name.trim();
            (plan_of(s).ok()?, label.map_or_else(|| format!("Your {}", s.display_name()), str::to_string), if name.is_empty() { s.type_name.clone() } else { name.to_string() })
        }
    };
    Some(CompareWith { label, short, plan, pick: Some(pick.clone()) })
}

/// The choice `label` picks out of `choices`.
fn picked<'a>(choices: &'a [Choice], label: &str) -> Option<&'a Choice> {
    choices.iter().find(|c| c.label == label)
}

/// The Compare panel: what to put the plan beside, then both side by side. Returns another build picked.
#[allow(clippy::too_many_arguments)]
fn compare_panel(ui: &mut Ui, hull: &Hull, figures: &Figures, with: &CompareWith, others: &[Choice], setup: &Setup, text: &mut String) -> Option<(Pick, String)> {
    let mut picked_now = None;
    card(ui, "Compare with", |ui| {
        ui.add(Label::new(RichText::new("Put the plan beside your ship as it is, the hull as a shipyard sells it, a saved build or another of your ships.").color(pal::DIM)).wrap());
        ui.add_space(6.0);
        let width = (ui.available_width() - 44.0).clamp(200.0, 420.0);
        let done = ui
            .horizontal(|ui| {
                widgets::combobox_grouped(ui, "plan-compare-with", text, &with.label, width, |typed| compare::rows(others, typed))
            })
            .inner;
        if let Some(c) = done.picked.and_then(|name| picked(others, &name)) {
            text.clear();
            picked_now = Some((c.pick.clone(), c.label.clone()));
        }
    });
    ui.add_space(widgets::GAP);
    let theirs_hull = fitting::hull(&with.plan.hull);
    card(ui, "Side by side", |ui| match fitting::figures_with(&with.plan, setup) {
        Some(theirs) => {
            // Another hull's build says which.
            let label = match theirs_hull {
                Some(h) if !h.symbol.eq_ignore_ascii_case(&hull.symbol) && !with.label.contains(&h.name) => format!("{} ({})", with.label, h.name),
                _ => with.label.clone(),
            };
            ui.add(Label::new(RichText::new(format!("This plan beside {}", lower_first(&label))).color(pal::DIM)).wrap());
            ui.add_space(4.0);
            compare::table(ui, figures, &label, &with.short, &theirs);
            footnote(ui, "Green where this plan does better, red where it does worse. Both at the same pips, cargo, fuel and discount.");
        }
        None => {
            ui.label(RichText::new("This version of Elite Nexus has no figures for that hull.").color(pal::DIM));
        }
    });
    picked_now
}

/// A name to save a new build under: the hull's, numbered past the ones taken ("Python", "Python 2").
fn default_name(gs: &GameState, st: &PlanUi, hull: &Hull) -> String {
    let stem = st.ship.and_then(|id| gs.fleet.ships.get(&id)).map(|s| s.name.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| hull.name.clone());
    (1..).map(|n| if n == 1 { stem.clone() } else { format!("{stem} {n}") }).find(|n| st.builds.find(n).is_none()).unwrap_or(stem)
}

/// The field to name the build in and its Save button.
fn save_row(ui: &mut Ui, st: &mut PlanUi, plan: &Plan) {
    st.builds.ensure_loaded();
    let Some(name) = st.save_name.as_mut() else { return };
    let (mut save, mut cancel) = (false, false);
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        ui.label(RichText::new("Save as").color(pal::DIM));
        let edit = ui.add(egui::TextEdit::singleline(name).hint_text("A name for this build").desired_width(260.0));
        save = edit.lost_focus() && ui.input(|k| k.key_pressed(egui::Key::Enter));
        let ok = !name.trim().is_empty();
        if ui.add_enabled_ui(ok, |ui| widgets::primary_button(ui, &format!("{} Save", icon::FLOPPY_DISK))).inner.clicked() {
            save = true;
        }
        cancel = widgets::button(ui, "Cancel").clicked();
    });
    if cancel {
        st.save_name = None;
        return;
    }
    let Some(name) = st.save_name.clone() else { return };
    let taken = st.builds.find(&name).is_some();
    if taken {
        ui.label(RichText::new(format!("{} Saving replaces your saved build \u{201c}{}\u{201d}.", icon::INFO, name.trim())).size(size::FOOTNOTE).color(pal::WARN));
    }
    let mut failed = None;
    if save && !name.trim().is_empty() {
        match st.builds.save(SavedBuild::from_plan(&name, plan, st.ship)) {
            Ok(()) => {
                let name = name.trim().to_string();
                // Compare and the weapons' target show it again as saved now, if they showed the build of that name.
                st.just_saved = Some(name.clone());
                st.saved_as = Some(name);
                // Undo changes goes back to it as saved.
                st.start = Some(plan.clone());
                st.save_name = None;
            }
            Err(e) => failed = Some(e),
        }
    }
    if let Some(p) = failed.or_else(|| st.builds.problem.clone().filter(|_| st.save_name.is_some())) {
        ui.add(Label::new(RichText::new(format!("{} {p}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
    }
}

/// Share: open the build in EDSY or Coriolis, or copy it in the game's format or as SLEF, with the engineering on its
/// modules and the engineering planned here, written as the game writes engineering.
fn share_menu(ui: &mut Ui, loadout: &str) {
    let button = widgets::button(ui, &format!("{} Share {}", icon::SHARE_NETWORK, icon::CARET_DOWN)).on_hover_text("Open this build in EDSY or Coriolis, or copy it");
    egui::Popup::menu(&button).frame(widgets::popover_frame(ui.style())).gap(6.0).show(|ui| {
        ui.set_min_width(250.0);
        let item = |ui: &mut Ui, glyph: &str, text: &str, tip: &str| ui.add(egui::Button::new(RichText::new(format!("{glyph}  {text}")).size(size::CALLOUT).color(pal::TEXT)).frame(false).min_size(egui::vec2(0.0, 30.0))).on_hover_text(tip);
        if item(ui, icon::ARROW_SQUARE_OUT, "Open in EDSY", "Opens edsy.org in your browser with this build").clicked()
            && let Some(url) = crate::export::edsy_url(loadout)
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
        if item(ui, icon::ARROW_SQUARE_OUT, "Open in Coriolis", "Opens coriolis.io in your browser with this build").clicked()
            && let Some(url) = crate::export::coriolis_url(loadout)
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
        if item(ui, icon::COPY, "Copy build", "The game's own Loadout format, which EDSY, Coriolis and Where to buy read").clicked() {
            ui.ctx().copy_text(loadout.to_string());
        }
        if item(ui, icon::COPY, "Copy SLEF", "The format EDSY and Coriolis export builds in, with Elite Nexus as the app it came from").clicked()
            && let Some(text) = crate::export::slef(loadout)
        {
            ui.ctx().copy_text(text);
        }
        ui.add_space(4.0);
        ui.add(Label::new(RichText::new("With the engineering on its modules, and the engineering planned here at the rolls you set, as the game writes engineering.").size(size::FOOTNOTE).color(pal::DIM)).wrap());
    });
}

/// The slots, grouped as the game's outfitting groups them, with the module list of the open one. `reference`: what
/// a module is new against.
fn slots(ui: &mut Ui, gs: &GameState, st: &mut PlanUi, hull: &Hull, plan: &Plan, reference: Option<&Plan>, out: &mut Option<PlanOut>) -> Option<Plan> {
    let mut changed = None;
    // Bulkheads first, as in the game.
    card(ui, "Bulkheads", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for b in &hull.bulkheads {
                let current = plan.bulkheads == b.symbol || (plan.bulkheads.is_empty() && hull.bulkheads.first() == Some(b));
                let tip = format!("+{:.0} % armour · {:.0} t · {}", b.hullboost * 100.0, b.mass, short_credits(b.cost as i64));
                if widgets::pill(ui, current, &b.name).on_hover_text(tip).clicked() && !current {
                    let mut p = plan.clone();
                    p.bulkheads = b.symbol.clone();
                    // The bulkheads it started with come back as they were there, engineering and all; others plain.
                    match st.start.as_ref().filter(|s| s.bulkheads == b.symbol) {
                        Some(s) => {
                            p.bulkheads_engineered = s.bulkheads_engineered.clone();
                            p.bulkheads_engineering = s.bulkheads_engineering.clone();
                        }
                        None => {
                            p.bulkheads_engineered.clear();
                            p.bulkheads_engineering = None;
                        }
                    }
                    changed = Some(p);
                }
            }
        });
        // Their engineering, behind a row like a slot's.
        let current = hull.bulkheads.iter().find(|b| b.symbol == plan.bulkheads).or_else(|| hull.bulkheads.first());
        if let Some(b) = current
            && !nexus_data::engineering::blueprints_for_module(&b.symbol).is_empty()
        {
            ui.add_space(8.0);
            let open = st.open.as_deref() == Some("Armour");
            let done = plan.bulkheads_done();
            let now = match (&plan.bulkheads_goal, &done) {
                (Some(g), _) => format!("Planned: G{} {}", g.grade, nexus_data::engineering::blueprint_name(&b.symbol, &g.blueprint)),
                (None, Some((bp, grade))) => format!("G{grade} {}, as the game reported it", nexus_data::engineering::blueprint_name(&b.symbol, bp)),
                (None, None) => "None".into(),
            };
            if disclosure_row(ui, "Engineering", &now, open).clicked() {
                st.open = if open { None } else { Some("Armour".into()) };
            }
            if open && let Some(goal) = goal_picker(ui, gs, &b.symbol, plan.bulkheads_goal.as_ref(), done.as_ref(), plan.bulkheads_experimental()) {
                let mut p = plan.clone();
                p.bulkheads_goal = goal;
                changed = Some(p);
            }
        }
    });
    for (kinds, title) in [(&[SlotKind::Core][..], "Core internals"), (&[SlotKind::Optional, SlotKind::Military][..], "Optional internals"), (&[SlotKind::Hardpoint][..], "Hardpoints"), (&[SlotKind::Utility][..], "Utility mounts")] {
        let in_group: Vec<&SlotDef> = hull.slots.iter().filter(|s| kinds.contains(&s.kind)).collect();
        if in_group.is_empty() {
            continue;
        }
        ui.add_space(widgets::GAP);
        card(ui, title, |ui| {
            for s in in_group {
                let fitted = plan.module_in(&s.name);
                let open = st.open.as_deref() == Some(s.name.as_str());
                if slot_row(ui, s, fitted, reference.and_then(|b| b.module_in(&s.name)), open).clicked() {
                    st.open = if open { None } else { Some(s.name.clone()) };
                    st.filter.clear();
                }
                if open && let Some(edit) = module_list(ui, gs, st, hull, plan, s, fitted, out) {
                    let mut p = plan.clone();
                    match edit {
                        SlotEdit::Module(pick) => {
                            // The module it started with comes back as it was there, engineering and all.
                            let was = st.start.as_ref().and_then(|b| b.module_in(&s.name)).filter(|w| pick.as_deref() == Some(w.item.as_str())).cloned();
                            match was {
                                Some(w) => p.put(w),
                                None => p.set(&s.name, pick.as_deref()),
                            }
                            st.open = None;
                        }
                        SlotEdit::Goal(goal) => {
                            if let Some(f) = p.fitted.iter_mut().find(|f| f.slot.eq_ignore_ascii_case(&s.name)) {
                                f.goal = goal;
                            }
                        }
                    }
                    changed = Some(p);
                }
            }
        });
    }
    changed
}

/// A row that opens what's under it: a caption over a value, and a caret.
fn disclosure_row(ui: &mut Ui, caption: &str, value: &str, open: bool) -> egui::Response {
    egui::Frame::new()
        .fill(if open { pal::CARD_HI } else { Color32::TRANSPARENT })
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(if open { icon::CARET_DOWN } else { icon::CARET_RIGHT }).color(pal::DIM));
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(caption).size(size::CAPTION).color(pal::DIM));
                        ui.add(Label::new(value).truncate());
                    });
                })
            });
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// "Optional 3 · size 6" for an optional slot, the game's name otherwise.
fn slot_label(s: &SlotDef) -> String {
    match s.kind {
        SlotKind::Optional | SlotKind::Military if s.name.to_ascii_lowercase().starts_with("slot") => format!("Optional · size {}", s.size),
        SlotKind::Military => format!("Military · size {}", s.size),
        SlotKind::Optional => format!("{} · size {}", outfitting::slot_display(&s.name), s.size),
        SlotKind::Core => format!("{} · size {}", outfitting::slot_display(&s.name), s.size),
        _ => outfitting::slot_display(&s.name),
    }
}

/// One slot: what's in it, its main figure, and whether it's new.
fn slot_row(ui: &mut Ui, s: &SlotDef, fitted: Option<&Fitted>, before: Option<&Fitted>, open: bool) -> egui::Response {
    let new = fitted.map(|f| f.item.as_str()) != before.map(|f| f.item.as_str()) && fitted.is_some();
    let frame = egui::Frame::new().fill(if open { pal::CARD_HI } else { Color32::TRANSPARENT }).corner_radius(10).inner_margin(egui::Margin::symmetric(10, 7));
    frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The caret at the right first, so the slot's text takes the rest and truncates in it. (In a row: a
            // right-to-left layout straight in a column centres in all the height left.)
            ui.horizontal(|ui| ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(if open { icon::CARET_DOWN } else { icon::CARET_RIGHT }).color(pal::DIM));
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(slot_label(s)).size(size::CAPTION).color(pal::DIM));
                    match fitted {
                        Some(f) => {
                            let data = fitting::module(&f.item);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                let name = RichText::new(outfitting::display_name(&f.item)).color(if f.on { pal::TEXT } else { pal::FAINT });
                                ui.label(name);
                                if new {
                                    badge(ui, "New", pal::ACCENT);
                                }
                                match (&f.goal, &f.blueprint) {
                                    (Some(g), _) => {
                                        let tip = if fitting::has_figures(&f.item, &g.blueprint, g.grade) { "Planned engineering: the figures include it, rolled as set" } else { "Planned engineering: this version's data has no figures for it, so it adds up its materials but doesn't change the figures" };
                                        badge(ui, &format!("Plan: G{} {}", g.grade, nexus_data::engineering::blueprint_name(&f.item, &g.blueprint)), pal::ACCENT).on_hover_text(tip);
                                    }
                                    (None, Some((bp, grade))) => {
                                        badge(ui, &format!("G{grade} {}", nexus_data::engineering::blueprint_name(&f.item, bp)), pal::PURPLE).on_hover_text("Engineered, as the game reported it: the figures use its engineered values. A different module here would be plain.");
                                    }
                                    (None, None) => {}
                                }
                                if data.is_none() {
                                    badge(ui, "No figures", pal::WARN).on_hover_text("This module is newer than the figures in this version of Elite Nexus: its mass, power and the rest aren't counted.");
                                }
                            });
                            if let Some(m) = data {
                                ui.add(Label::new(RichText::new(figure_line(&Live::new(f, m))).size(size::CAPTION).color(pal::DIM)).truncate());
                            }
                        }
                        None => {
                            ui.label(RichText::new("Empty").color(pal::FAINT));
                        }
                    }
                });
            }));
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The module's main figure, then its mass and power: "optimal mass 1,800 t · 12.8 t a jump · 40 t · 0.75 MW", as fitted
/// (engineering included).
fn figure_line(live: &Live) -> String {
    let m = live.m;
    let g = |k: &str| live.get(k);
    let main = match m.grp.as_str() {
        "fsd" => format!("optimal mass {:.0} t, {:.1} t of fuel a jump", g("optmass"), g("maxfuel")),
        "pp" => format!("{:.1} MW", g("pgen")),
        "t" => format!("optimal mass {:.0} t", g("optmass")),
        "sg" | "bsg" | "psg" => format!("optimal mass {:.0} t", g("optmass")),
        "pd" => format!("weapons {:.0} MJ, {:.1} MW", g("wepcap"), g("weprate")),
        "ft" => format!("{:.0} t of fuel", g("fuel")),
        "sb" => format!("+{:.1} % shields", g("shieldboost") * 100.0),
        "hr" | "ghrp" | "mahr" => format!("+{:.0} armour", g("hullreinforcement")),
        "gfsb" => format!("+{:.2} ly a jump", g("jumpboost")),
        "gsrp" => format!("+{:.0} MJ shields", g("shieldaddition")),
        _ if m.figures.contains_key("cargo") => format!("{:.0} t of cargo", g("cargo")),
        _ if m.figures.contains_key("fuel") => format!("{:.0} t of fuel", g("fuel")),
        _ if m.figures.contains_key("passengers") => format!("{:.0} passengers", g("passengers")),
        // A weapon: damage a second over time, as the Weapons panel counts it.
        _ if m.figures.contains_key("damage") => fitting::weapon(live).map(|w| format!("{:.1} damage a second", w.sustained_dps)).unwrap_or_default(),
        _ => String::new(),
    };
    // Mass and power only where there are any (a power plant draws none, a fuel tank weighs nothing).
    let (mass, power) = (g("mass"), g("power"));
    let parts = [main, if mass > 0.0 { format!("{mass:.1} t") } else { String::new() }, if power > 0.0 { format!("{power:.2} MW") } else { String::new() }];
    parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// A change to the open slot.
enum SlotEdit {
    /// Another module (None: empty the slot).
    Module(Option<String>),
    /// Engineering to plan for the module in it (None: none).
    Goal(Option<Goal>),
}

/// Engineering to plan for a module (`item`, the bulkheads' too): a blueprint, the grade to reach, how far its rolls
/// get, and an experimental effect, with the engineers who offer that grade. `done` and `done_experimental`: the
/// engineering the game reported on it, which a goal of the same blueprint keeps. Returns the goal picked (None in
/// it: none).
fn goal_picker(ui: &mut Ui, gs: &GameState, item: &str, goal: Option<&Goal>, done: Option<&(String, u8)>, done_experimental: Option<&str>) -> Option<Option<Goal>> {
    let blueprints: Vec<_> = nexus_data::engineering::blueprints_for_module(item).into_iter().filter(|b| b.merc_module.is_none()).collect();
    if blueprints.is_empty() {
        return None;
    }
    let mut edit = None;
    widgets::section(ui, "Engineering to plan");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        if widgets::pill(ui, goal.is_none(), "None").clicked() && goal.is_some() {
            edit = Some(None);
        }
        for bp in &blueprints {
            let on = goal.is_some_and(|g| bp.is(&g.blueprint));
            if widgets::pill(ui, on, &bp.name).clicked() && !on {
                // To the top grade, rolled all the way, as most engineering aims. More of the blueprint a module has
                // keeps its experimental effect, as the game does; another blueprint starts it afresh.
                let same = done.is_some_and(|(d, _)| bp.is(d));
                let grade = match done {
                    Some((_, g)) if same && *g >= bp.max_grade() => *g,
                    _ => bp.max_grade(),
                };
                let experimental = if same { done_experimental.map(str::to_string) } else { None };
                edit = Some(Some(Goal { experimental, ..Goal::new(&bp.symbol, grade) }));
            }
        }
    });
    let Some(g) = goal else { return edit };
    let Some(bp) = blueprints.iter().find(|b| b.is(&g.blueprint)) else { return edit };
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
        ui.label(RichText::new("To grade").color(pal::DIM));
        let labels: Vec<String> = (bp.min_grade()..=bp.max_grade()).map(|n| format!("G{n}")).collect();
        let options: Vec<(u8, &str)> = (bp.min_grade()..=bp.max_grade()).zip(labels.iter().map(String::as_str)).collect();
        // A new grade, roll or effect works its figures out afresh: values typed in for the old one go.
        let mut grade = g.grade;
        if widgets::segmented(ui, &mut grade, &options) {
            edit = Some(Some(Goal { grade, exact: Vec::new(), ..g.clone() }));
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Rolled").color(pal::DIM)).on_hover_text("How far through the grade's range its rolls get: 100 % is a full roll, the most the grade gives");
        let mut pct = (g.roll * 100.0).round();
        if widgets::number(ui, &mut pct, 0.0..=100.0, widgets::Steps::Through(&[0.0, 25.0, 50.0, 75.0, 100.0]), 0, " %").changed() {
            edit = Some(Some(Goal { roll: (pct / 100.0).clamp(0.0, 1.0), exact: Vec::new(), ..g.clone() }));
        }
    });
    engineers_line(ui, gs, bp, g.grade);
    let experimentals = nexus_data::engineering::experimentals_for(item);
    if !experimentals.is_empty() {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            ui.label(RichText::new("Experimental").color(pal::DIM));
            if widgets::pill(ui, g.experimental.is_none(), "None").clicked() && g.experimental.is_some() {
                edit = Some(Some(Goal { experimental: None, exact: Vec::new(), ..g.clone() }));
            }
            for x in experimentals {
                let on = g.experimental.as_deref() == Some(x.symbol.as_str());
                if widgets::pill(ui, on, &x.name).clicked() && !on {
                    edit = Some(Some(Goal { experimental: Some(x.symbol.clone()), exact: Vec::new(), ..g.clone() }));
                }
            }
        });
    }
    if !fitting::has_figures(item, &g.blueprint, g.grade) {
        ui.add_space(4.0);
        ui.add(Label::new(RichText::new(format!("{} This version's data has no figures for this blueprint at this grade: it adds up its materials, but the figures stay as they are.", icon::INFO)).size(size::FOOTNOTE).color(pal::WARN)).wrap());
    } else if let Some(x) = g.experimental.as_deref().filter(|x| !fitting::effect_has_figures(x)) {
        let name = nexus_data::engineering::experimental(x).map_or_else(|| x.to_string(), |e| e.name.clone());
        ui.add_space(4.0);
        ui.add(Label::new(RichText::new(format!("{} {name} changes what the figures here don't show (how the weapon behaves), so it adds up its materials only.", icon::INFO)).size(size::FOOTNOTE).color(pal::DIM)).wrap());
    }
    if let Some(e) = exact_figures(ui, item, g) {
        edit = Some(Some(e));
    }
    edit
}

/// How a figure shows: its name, what to multiply it by (resistances and boosts in percent), decimals and unit. None
/// for figures not worth typing (a curve's ends move with its middle).
fn figure_spec(key: &str) -> Option<(&'static str, f64, usize, &'static str)> {
    Some(match key {
        "optmass" => ("Optimal mass", 1.0, 1, " t"),
        "maxfuel" => ("Fuel a jump", 1.0, 2, " t"),
        "mass" => ("Mass", 1.0, 2, " t"),
        "integrity" => ("Integrity", 1.0, 1, ""),
        "power" => ("Power draw", 1.0, 2, " MW"),
        "boot" => ("Boot time", 1.0, 1, " s"),
        "optmul" => ("Optimal multiplier", 100.0, 1, " %"),
        "pgen" => ("Power", 1.0, 2, " MW"),
        "eff" => ("Heat efficiency", 1.0, 3, ""),
        "engcap" => ("Engines capacity", 1.0, 1, " MJ"),
        "engrate" => ("Engines recharge", 1.0, 2, " MW"),
        "wepcap" => ("Weapons capacity", 1.0, 1, " MJ"),
        "weprate" => ("Weapons recharge", 1.0, 2, " MW"),
        "syscap" => ("Systems capacity", 1.0, 1, " MJ"),
        "sysrate" => ("Systems recharge", 1.0, 2, " MW"),
        "kinres" => ("Kinetic resistance", 100.0, 1, " %"),
        "thermres" => ("Thermal resistance", 100.0, 1, " %"),
        "explres" => ("Explosive resistance", 100.0, 1, " %"),
        "causres" => ("Caustic resistance", 100.0, 1, " %"),
        "shieldboost" => ("Shield boost", 100.0, 1, " %"),
        "shieldaddition" => ("Shields added", 1.0, 1, " MJ"),
        "hullboost" => ("Hull boost", 100.0, 1, " %"),
        "hullreinforcement" => ("Hull reinforcement", 1.0, 0, ""),
        "regen" => ("Regeneration", 1.0, 2, " MJ/s"),
        "brokenregen" => ("Broken regeneration", 1.0, 2, " MJ/s"),
        "distdraw" => ("Distributor draw", 1.0, 3, " MJ"),
        "damage" => ("Damage", 1.0, 2, ""),
        "rof" => ("Rate of fire", 1.0, 3, "/s"),
        "thermload" => ("Thermal load", 1.0, 2, ""),
        "clip" => ("Clip", 1.0, 0, ""),
        "ammo" => ("Reserve", 1.0, 0, ""),
        "reload" => ("Reload", 1.0, 2, " s"),
        "range" => ("Range", 1.0, 0, " m"),
        "falloff" => ("Falloff from", 1.0, 0, " m"),
        "piercing" => ("Armour piercing", 1.0, 0, ""),
        "shotspeed" => ("Shot speed", 1.0, 0, " m/s"),
        "jitter" => ("Jitter", 1.0, 2, "°"),
        "burst" => ("Burst", 1.0, 0, ""),
        "burstrof" => ("Burst rate of fire", 1.0, 2, "/s"),
        "fuel" => ("Fuel", 1.0, 1, " t"),
        "cargo" => ("Cargo", 1.0, 0, " t"),
        "shieldreinforcement" => ("Shield cell charge", 1.0, 1, " MJ"),
        "duration" => ("Duration", 1.0, 1, " s"),
        "spinup" => ("Spin-up", 1.0, 1, " s"),
        _ => return None,
    })
}

/// The figures a goal changes, as rolled, each with a field to type the module's real value (as the game shows it once
/// rolled): it replaces what the roll works out. Returns the goal with a value typed or taken back.
fn exact_figures(ui: &mut Ui, item: &str, g: &Goal) -> Option<Goal> {
    // A range in metres is a weapon's; sensors and limpet controllers keep theirs in other units, not offered here.
    let weapon = fitting::module(item).is_some_and(|m| m.figures.contains_key("damage"));
    let figures: Vec<_> = fitting::goal_figures(item, g).into_iter().filter(|(k, ..)| weapon || k != "range").filter_map(|(k, v, base)| figure_spec(&k).map(|s| (k, v, base, s))).collect();
    if figures.is_empty() {
        return None;
    }
    let mut out = None;
    ui.add_space(6.0);
    ui.label(RichText::new("Its figures").color(pal::DIM)).on_hover_text("As rolled. Type a module's real values, as the game shows them, to plan from it: a value typed replaces what the roll works out, until the blueprint, grade, roll or effect changes.");
    egui::Grid::new(("plan-goal-figures", item)).num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
        for (k, v, base, (name, scale, decimals, unit)) in &figures {
            let typed = g.exact.iter().any(|(x, _)| x == k);
            ui.label(RichText::new(*name).color(if typed { pal::ACCENT_SOFT } else { pal::DIM }));
            let mut shown = v * scale;
            if widgets::number(ui, &mut shown, -1e9..=1e9, widgets::Steps::By(10f64.powi(-(*decimals as i32))), *decimals, unit).changed() && shown.is_finite() {
                let mut exact: Vec<(String, f64)> = g.exact.iter().filter(|(x, _)| x != k).cloned().collect();
                exact.push((k.clone(), shown / scale));
                out = Some(Goal { exact, ..g.clone() });
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("from {:.*}{unit}", *decimals, base * scale)).size(size::FOOTNOTE).color(pal::FAINT));
                if typed && widgets::icon_button(ui, icon::ARROW_COUNTER_CLOCKWISE, "Back to the roll's value").clicked() {
                    out = Some(Goal { exact: g.exact.iter().filter(|(x, _)| x != k).cloned().collect(), ..g.clone() });
                }
            });
            ui.end_row();
        }
    });
    out
}

/// The engineers who offer `grade` of a blueprint: the ones you can craft it with first, ticked (unlocked, at that grade
/// with you), then those unlocked at a lower grade, then the rest; where each is and how far you are on hover.
fn engineers_line(ui: &mut Ui, gs: &GameState, bp: &nexus_data::engineering::Blueprint, grade: u8) {
    // By the journal's id, as the Engineering page has them: a name spelled another way still finds its engineer.
    let status = |name: &str| nexus_data::engineering::engineer_by_name(name).and_then(|e| gs.cmdr.engineers.get(&e.id));
    let unlocked = |name: &str| status(name).filter(|s| s.progress.eq_ignore_ascii_case("Unlocked"));
    // 0: can craft the grade now; 1: unlocked, at a lower grade; 2: not unlocked.
    let standing = |name: &str| match unlocked(name) {
        Some(s) if s.rank.is_none_or(|r| r >= u32::from(grade)) => 0,
        Some(_) => 1,
        None => 2,
    };
    let mut who: Vec<_> = bp.engineers_for_grade(grade).collect();
    who.sort_by_key(|e| standing(&e.engineer));
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
        ui.label(RichText::new(format!("G{grade} from")).color(pal::DIM));
        if who.is_empty() {
            ui.label(RichText::new("no engineer this version knows of").color(pal::WARN));
            return;
        }
        for (i, e) in who.iter().enumerate() {
            let place = nexus_data::engineering::engineer_by_name(&e.engineer).map(|x| format!(", in {}", x.system)).unwrap_or_default();
            let how = match (status(&e.engineer), standing(&e.engineer)) {
                (Some(s), 0) => format!(": unlocked{}", s.rank.map(|r| format!(", grade {r} with you")).unwrap_or_default()),
                (Some(s), 1) => format!(": unlocked, grade {} with you so far", s.rank.unwrap_or(0)),
                (Some(s), _) => format!(": {}", s.progress.to_lowercase()),
                (None, _) => ": not met yet".to_string(),
            };
            let (mark, col, below) = match standing(&e.engineer) {
                0 => (format!("{} ", icon::CHECK), pal::GOOD, String::new()),
                1 => (String::new(), pal::WARN, format!(" (G{} for you)", status(&e.engineer).and_then(|s| s.rank).unwrap_or(0))),
                _ => (String::new(), pal::TEXT, String::new()),
            };
            let text = format!("{mark}{}{below}{}", e.engineer, if i + 1 < who.len() { "," } else { "" });
            ui.label(RichText::new(text).color(col)).on_hover_text(format!("{}{place}{how}", e.engineer));
        }
    });
}

/// The modules that fit the open slot, to pick one, and the engineering to plan for the one in it.
#[allow(clippy::too_many_arguments)]
fn module_list(ui: &mut Ui, gs: &GameState, st: &mut PlanUi, hull: &Hull, plan: &Plan, s: &SlotDef, fitted: Option<&Fitted>, out: &mut Option<PlanOut>) -> Option<SlotEdit> {
    let mut pick = None;
    egui::Frame::new().fill(pal::FIELD).corner_radius(10).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if let Some(f) = fitted
            && let Some(goal) = goal_picker(ui, gs, &f.item, f.goal.as_ref(), f.blueprint.as_ref(), f.experimental.as_deref())
        {
            pick = Some(SlotEdit::Goal(goal));
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            widgets::search_field(ui, &mut st.filter, "Filter: 5A, gimballed, scoop…", 260.0);
            if let Some(f) = fitted {
                if s.kind != SlotKind::Core && widgets::button(ui, "Empty this slot").clicked() {
                    pick = Some(SlotEdit::Module(None));
                }
                if widgets::button(ui, &format!("{} Where to buy it", icon::STOREFRONT)).on_hover_text("Find the nearest stations that sell the module in this slot").clicked() {
                    *out = Some(PlanOut::BuyOne(f.item.clone()));
                }
            }
        });
        ui.add_space(6.0);
        let words: Vec<String> = st.filter.split_whitespace().map(nexus_data::commodities::name_key).filter(|w| !w.is_empty()).collect();
        // A ship takes one of some modules (a fuel scoop, a Guardian FSD booster): with one in another slot, no second.
        let taken: Vec<&[&str]> = plan.fitted.iter().filter(|f| !f.slot.eq_ignore_ascii_case(&s.name)).filter_map(|f| fitting::module(&f.item)).filter_map(fitting::one_per_ship).collect();
        let mut list: Vec<&ModuleData> = hull
            .fits(s)
            .into_iter()
            .filter(|m| fitting::one_per_ship(m).is_none_or(|kind| !taken.contains(&kind)))
            .filter(|m| words.iter().all(|w| nexus_data::commodities::name_key(&m.display_name()).contains(w.as_str())))
            .collect();
        // By kind of module, then biggest and best first.
        list.sort_by(|a, b| kind_name(a).cmp(&kind_name(b)).then(b.class.cmp(&a.class)).then(a.rating.cmp(&b.rating)).then(a.mount.cmp(&b.mount)));
        if list.is_empty() {
            ui.label(RichText::new("Nothing that fits here matches.").color(pal::DIM));
            return;
        }
        egui::ScrollArea::vertical().id_salt(("plan-list", &s.name)).max_height(340.0).show(ui, |ui| {
            let mut last = String::new();
            for m in list {
                let kind = kind_name(m);
                if kind != last {
                    widgets::section(ui, &kind);
                    last = kind;
                }
                let current = fitted.is_some_and(|f| f.item == m.symbol);
                let r = egui::Frame::new()
                    .fill(if current { pal::ACCENT_BG } else { Color32::TRANSPARENT })
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        // Name and price on one line, its figures under it.
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(short_credits(m.cost as i64)).color(pal::GOLD));
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.add(Label::new(RichText::new(m.display_name()).color(if current { pal::ACCENT_SOFT } else { pal::TEXT })).truncate());
                                });
                            });
                        });
                        let plain = Fitted::new(&s.name, &m.symbol);
                        ui.add(Label::new(RichText::new(figure_line(&Live::new(&plain, m))).size(size::CAPTION).color(pal::DIM)).truncate());
                    })
                    .response
                    .interact(Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let r = match &m.description {
                    Some(d) => r.on_hover_text(d.as_str()),
                    None => r,
                };
                if r.clicked() && !current {
                    pick = Some(SlotEdit::Module(Some(m.symbol.clone())));
                }
            }
        });
    });
    pick
}

/// What kind of module it is, for the list's headings: FDevIDs' name without size or mount ("Pulse Laser").
fn kind_name(m: &ModuleData) -> String {
    outfitting::module(&m.symbol).map(|x| x.name.clone()).or_else(|| m.name.clone()).unwrap_or_else(|| m.grp.clone())
}

/// The materials the plan's engineering goals take, against what the commander holds.
fn materials_card(ui: &mut Ui, gs: &GameState, plan: &Plan) {
    let mut need = fitting::goal_materials(plan);
    if need.is_empty() {
        return;
    }
    let have = |symbol: &str| gs.inventory.materials.count(symbol).max(0) as u32;
    // What they're short of first, then by name.
    need.sort_by_key(|(s, n)| (have(s) >= *n, nexus_data::engineering::Ingredient { symbol: s.clone(), count: 0 }.name()));
    ui.add_space(widgets::GAP);
    card(ui, "Materials for the engineering", |ui| {
        let short = need.iter().filter(|(s, n)| have(s) < *n).count();
        let summary = if short == 0 { format!("You have all {} materials", need.len()) } else { format!("Short of {short} of {} materials", need.len()) };
        ui.label(RichText::new(summary).font(display(size::CALLOUT)).color(if short == 0 { pal::GOOD } else { pal::WARN }));
        ui.add_space(4.0);
        egui::Grid::new("plan-materials").num_columns(3).spacing([16.0, 4.0]).show(ui, |ui| {
            for (symbol, n) in &need {
                let h = have(symbol);
                ui.label(nexus_data::engineering::Ingredient { symbol: symbol.clone(), count: *n }.name());
                ui.label(RichText::new(format!("{h} / {n}")).color(if h >= *n { pal::GOOD } else { pal::TEXT }));
                if h < *n {
                    ui.label(RichText::new(format!("{} short", n - h)).color(pal::WARN));
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        footnote(
            ui,
            &format!("Each planned blueprint from the grade above the one the module has of it, {} rolls a grade as the Engineering page plans, and each new experimental effect. The figures include planned engineering at the roll set, and Share takes it to EDSY and Coriolis. Materials as your journal last listed them.", fitting::ROLLS_PER_GRADE),
        );
    });
}

/// What to buy for the plan: against the commander's ship (`owned`), what's new on it; else the hull at a shipyard's
/// price, with its standard modules, and what's new against those. Each at its list price less `discount` (0 to 1).
#[allow(clippy::too_many_arguments)]
fn whats_new(ui: &mut Ui, hull: &Hull, reference: Option<&Plan>, owned: bool, source: &str, plan: &Plan, discount: f64) -> Option<PlanOut> {
    let keep = 1.0 - discount.clamp(0.0, 1.0);
    let mut out = None;
    // What the reference has and the plan doesn't comes out; what the plan has and the reference doesn't is new.
    let mut left: Vec<&str> = reference.map(|b| b.fitted.iter().map(|f| f.item.as_str()).collect()).unwrap_or_default();
    let mut new: Vec<&Fitted> = Vec::new();
    for f in &plan.fitted {
        match left.iter().position(|i| *i == f.item) {
            Some(i) => {
                left.remove(i);
            }
            None => new.push(f),
        }
    }
    // Bulkheads count too: the plan's, when they aren't the reference's.
    let bulkheads_of = |p: &Plan| hull.bulkheads.iter().find(|b| b.symbol == p.bulkheads).or_else(|| hull.bulkheads.first());
    let (now_bh, was_bh) = (bulkheads_of(plan), reference.and_then(bulkheads_of));
    let new_bh = now_bh.filter(|b| was_bh.is_some_and(|w| w.symbol != b.symbol));
    // A new hull is bought whole, at a shipyard's price with its standard modules.
    let hull_price = (!owned).then(|| hulls::shipyard_price(hull) * keep);
    let anything = !new.is_empty() || new_bh.is_some() || hull_price.is_some();
    let title = if owned { "What's new" } else { "What to buy" };
    card_with(
        ui,
        title,
        |ui| {
            if anything && widgets::button(ui, &format!("{} Find stations", icon::MAGNIFYING_GLASS)).on_hover_text("Search Where to buy for stations that sell it").clicked() {
                out = Some(PlanOut::Buy { loadout: plan.to_loadout().to_string(), source: format!("planned ({source})") });
            }
        },
        |ui| {
            if !anything && left.is_empty() {
                ui.label(RichText::new("No other modules yet: open a slot and pick one.").color(pal::DIM));
                return;
            }
            let row = |ui: &mut Ui, name: String, price: f64| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::PLUS).color(pal::GOOD));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(short_credits(price as i64)).color(pal::GOLD));
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(Label::new(name).truncate());
                        });
                    });
                });
            };
            let gone = |ui: &mut Ui, name: String| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::MINUS).color(pal::DIM));
                    ui.add(Label::new(RichText::new(name).color(pal::DIM)).truncate());
                });
            };
            let mut total = 0.0;
            if let Some(price) = hull_price {
                total += price;
                row(ui, format!("{} with its standard modules", hull.name), price);
            }
            if let Some(b) = new_bh {
                total += b.cost * keep;
                row(ui, format!("{} bulkheads", b.name), b.cost * keep);
            }
            for f in &new {
                let price = fitting::module(&f.item).map_or(0.0, |m| m.cost) * keep;
                total += price;
                row(ui, outfitting::display_name(&f.item), price);
            }
            if let (Some(_), Some(w)) = (new_bh, was_bh) {
                gone(ui, format!("{} bulkheads come out", w.name));
            }
            for item in &left {
                gone(ui, format!("{} comes out", outfitting::display_name(item)));
            }
            if anything {
                ui.add_space(4.0);
                let at = if discount > 0.0 { format!("{:.0}\u{a0}% off list prices", discount * 100.0) } else { "list prices".into() };
                ui.label(RichText::new(format!("{} at {at}", short_credits(total as i64))).font(display(size::CALLOUT)).color(pal::GOLD));
            }
            footnote(
                ui,
                if owned {
                    "Find stations takes the build to Where to buy, which leaves out what's fitted on your ship of this type or in your storage."
                } else {
                    "Find stations takes the build to Where to buy, which leaves out what a new hull comes with and what you have already: in your storage, or on a ship of yours of this type."
                },
            );
        },
    );
    out
}
