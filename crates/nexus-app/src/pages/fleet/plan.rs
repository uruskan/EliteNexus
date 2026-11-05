//! Fleet › Plan a loadout: a hull and what's in each slot, what it comes to as you change it (jump range, speed,
//! shields, armour, power, mass, cost), and where to buy what's new. Figures: `nexus_data::fitting`.

use crate::theme::{display, pal, size};
use crate::widgets::{self, badge, card, card_with, footnote, short_credits};
use egui::{Color32, Label, RichText, Sense, Ui};
use egui_phosphor::regular as icon;
use nexus_data::fitting::{self, Figures, Fitted, Goal, Hull, ModuleData, Plan, SlotDef};
use nexus_data::outfitting::{self, SlotKind};
use nexus_model::fleet::Ship;
use nexus_model::GameState;

/// The planner's state.
#[derive(Default)]
pub struct PlanUi {
    plan: Option<Plan>,
    /// What the plan started from: Undo changes goes back to it, and a module picked back comes back as it was there.
    start: Option<Plan>,
    /// It started from one of the commander's ships, so the figures and what's new go against that ship; otherwise
    /// against the hull as a shipyard sells it.
    owned: bool,
    /// "your Kestrel Mk II", "a new Anaconda", "a pasted build".
    source: String,
    /// What the game reported for the ship it started from, to show beside the sums.
    game: Option<Reported>,
    /// The slot whose module list is open.
    open: Option<String>,
    filter: String,
    hull_text: String,
    paste: String,
    error: Option<String>,
}

/// The game's own figures for a ship of the commander's, from its last recorded build (0: not reported).
#[derive(Clone, Copy)]
struct Reported {
    mass: f64,
    range: f64,
    value: i64,
    rebuy: i64,
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
                self.start = Some(plan.clone());
                self.owned = true;
                self.plan = Some(plan);
                self.source = format!("your {}", s.display_name());
                self.game = Some(Reported { mass: s.unladen_mass, range: s.max_jump_range, value: s.hull_value + s.modules_value, rebuy: s.rebuy });
                self.open = None;
                self.error = None;
            }
            Err(e) => {
                self.plan = None;
                self.error = Some(e);
            }
        }
    }

    /// For a screenshot run: the frame shift drive's list open, with Increased FSD Range to grade 5 and Mass Manager
    /// planned for it, so the engineering and its materials show.
    pub fn demo_fsd(&mut self) {
        let Some(plan) = self.plan.as_mut() else { return };
        if let Some(f) = plan.fitted.iter_mut().find(|f| f.slot == "FrameShiftDrive") {
            f.goal = Some(Goal { blueprint: "FSD_LongRange".into(), grade: 5, experimental: Some("special_fsd_heavy".into()) });
            self.open = Some(f.slot.clone());
        }
    }

    /// Starts from a new hull by its journal symbol (a screenshot run's new Python).
    pub fn start_new(&mut self, symbol: &str) {
        if let Some(h) = fitting::hull(symbol) {
            self.start_stock(h);
        }
    }

    fn start_stock(&mut self, hull: &Hull) {
        self.plan = Some(Plan::stock(hull));
        self.start = self.plan.clone();
        self.owned = false;
        self.game = None;
        self.source = format!("a new {}", hull.name);
        self.open = None;
        self.error = None;
    }

    fn start_pasted(&mut self, text: &str) -> Result<(), String> {
        let build = crate::shopping::parse_build(text)?;
        let value: serde_json::Value = serde_json::from_str(&build.loadout).map_err(|_| "That build can't be read.".to_string())?;
        let plan = Plan::from_loadout(&value).ok_or("That build can't be read.")?;
        if fitting::hull(&plan.hull).is_none() {
            return Err(format!("This version of Elite Nexus has no figures for the {} yet.", nexus_data::ships::ship_name(&plan.hull)));
        }
        self.plan = Some(plan);
        self.start = self.plan.clone();
        self.owned = false;
        self.game = None;
        self.source = "a pasted build".into();
        self.open = None;
        self.error = None;
        self.paste.clear();
        Ok(())
    }

    /// What the figures and what's new go against: the commander's ship as it is, else the hull as a shipyard sells
    /// it.
    fn reference(&self, hull: &Hull) -> Option<Plan> {
        if self.owned { self.start.clone() } else { Some(Plan::stock(hull)) }
    }
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
        start(ui, gs, st);
        return None;
    };
    let Some(hull) = fitting::hull(&plan.hull) else {
        st.plan = None;
        return None;
    };
    let figures = fitting::figures(&plan).unwrap_or_default();
    let reference = st.reference(hull);
    let before = reference.as_ref().and_then(fitting::figures);
    let mut out = None;
    egui::ScrollArea::vertical().id_salt("plan").auto_shrink([false, false]).show(ui, |ui| {
        out = header(ui, st, hull, &plan, &figures);
        // Modules newer than this data count as nothing: say which.
        if !figures.unknown.is_empty() {
            let names: Vec<String> = figures.unknown.iter().map(|s| outfitting::display_name(s)).collect();
            ui.add_space(6.0);
            ui.add(Label::new(RichText::new(format!("{} No figures for {}: newer than this version's data, so their mass, power and the rest aren't counted.", icon::WARNING, names.join(", "))).color(pal::WARN)).wrap());
        }
        ui.add_space(widgets::GAP);
        let (start, owned, source, game) = (st.start.clone(), st.owned, st.source.clone(), st.game);
        let (mut changed, mut from_slots, mut from_list) = (None, None, None);
        widgets::split(
            ui,
            0.56,
            420.0,
            |ui| changed = slots(ui, st, hull, &plan, reference.as_ref(), &mut from_slots),
            |ui| {
                // The game's own figures beside the sums, while the plan's modules are still the ship's (planned
                // engineering doesn't change the figures).
                let modules_as_now = owned
                    && start.as_ref().is_some_and(|b| {
                        let mut p = plan.clone();
                        p.fitted.iter_mut().for_each(|f| f.goal = None);
                        &p == b
                    });
                let game = game.filter(|_| modules_as_now);
                figures_cards(ui, hull, &plan, &figures, before.as_ref(), owned, game);
                ui.add_space(widgets::GAP);
                from_list = whats_new(ui, hull, reference.as_ref(), owned, &source, &plan);
                materials_card(ui, gs, &plan);
            },
        );
        if let Some(p) = changed {
            st.plan = Some(p);
        }
        out = out.take().or(from_slots).or(from_list);
        ui.add_space(8.0);
        footnote(
            ui,
            "Hull and module figures come from EDCD's coriolis-data (October 2026), summed the way the game sums them: an unengineered Kestrel Mk II comes to the unladen mass and jump range its journal reports. Engineering read from your ship keeps the game's own engineered values; engineering you plan here adds up its materials but doesn't change the figures yet, and power priorities, heat and weapon figures aren't worked out. Prices are list prices: a station may sell for less, and the game may have changed a price since.",
        );
    });
    out
}

/// No plan yet: pick a hull, one of your ships, or paste a build.
fn start(ui: &mut Ui, gs: &GameState, st: &mut PlanUi) {
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
                        .map(|h| widgets::Suggestion { text: h.name.clone(), detail: format!("{} pad · {}", h.pad, short_credits(shipyard_price(h) as i64)), starred: false })
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

/// The hull's name, where the plan came from, and what to do with it.
fn header(ui: &mut Ui, st: &mut PlanUi, hull: &Hull, plan: &Plan, f: &Figures) -> Option<PlanOut> {
    let mut out = None;
    let mut reset = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        ui.label(RichText::new(&hull.name).font(display(22.0)));
        badge(ui, &format!("{} pad", hull.pad), pal::GRAY);
        ui.label(RichText::new(format!("{} · {}", if st.owned { format!("changes to {}", st.source) } else { st.source.clone() }, short_credits(f.total_cost() as i64))).color(pal::DIM));
    });
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        let loadout = plan.to_loadout().to_string();
        if widgets::primary_button(ui, &format!("{} Where to buy it", icon::STOREFRONT)).on_hover_text("Find the nearest stations that sell what you'd buy for this build: what you don't have already, on a ship of yours of this type or in your storage").clicked() {
            out = Some(PlanOut::Buy { loadout: loadout.clone(), source: format!("planned ({})", st.source) });
        }
        // The engineering already on the modules goes along; engineering planned here doesn't (it isn't rolled yet).
        if widgets::button(ui, &format!("{} EDSY", icon::ARROW_SQUARE_OUT)).on_hover_text("Open this build in edsy.org, with the engineering already on its modules").clicked()
            && let Some(url) = crate::export::edsy_url(&loadout)
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
        if widgets::button(ui, &format!("{} Coriolis", icon::ARROW_SQUARE_OUT)).on_hover_text("Open this build in coriolis.io, with the engineering already on its modules").clicked()
            && let Some(url) = crate::export::coriolis_url(&loadout)
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
        if widgets::button(ui, &format!("{} Copy build", icon::COPY)).on_hover_text("Copy the build in the game's own format, with the engineering already on its modules, to paste into EDSY, Coriolis or Where to buy").clicked() {
            ui.ctx().copy_text(loadout);
        }
        if st.start.is_some() && st.plan.as_ref() != st.start.as_ref() && widgets::button(ui, &format!("{} Undo changes", icon::ARROW_COUNTER_CLOCKWISE)).on_hover_text(format!("Back to {}", st.source)).clicked() {
            st.plan = st.start.clone();
        }
        reset = widgets::button(ui, "Start over").on_hover_text("Pick another hull or ship").clicked();
    });
    if reset {
        *st = PlanUi { hull_text: String::new(), ..PlanUi::default() };
    }
    out
}

/// What a shipyard asks for a hull: with its standard modules.
fn shipyard_price(h: &Hull) -> f64 {
    if h.retail_cost > 0.0 { h.retail_cost } else { h.hull_cost }
}

/// The slots, grouped as the game's outfitting groups them, with the module list of the open one. `reference`: what
/// a module is new against.
fn slots(ui: &mut Ui, st: &mut PlanUi, hull: &Hull, plan: &Plan, reference: Option<&Plan>, out: &mut Option<PlanOut>) -> Option<Plan> {
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
                if open && let Some(edit) = module_list(ui, st, hull, plan, s, fitted, out) {
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
                                if let Some((bp, grade)) = &f.blueprint {
                                    badge(ui, &format!("G{grade} {}", nexus_data::engineering::blueprint_name(&f.item, bp)), pal::PURPLE).on_hover_text("Engineered, as the game reported it: the figures use its engineered values. A different module here would be plain.");
                                }
                                if data.is_none() {
                                    badge(ui, "No figures", pal::WARN).on_hover_text("This module is newer than the figures in this version of Elite Nexus: its mass, power and the rest aren't counted.");
                                }
                            });
                            if let Some(m) = data {
                                ui.add(Label::new(RichText::new(figure_line(m, Some(f))).size(size::CAPTION).color(pal::DIM)).truncate());
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

/// The module's main figure, then its mass and power: "optimal mass 1,800 t · 12.8 t a jump · 40 t · 0.75 MW".
fn figure_line(m: &ModuleData, f: Option<&Fitted>) -> String {
    let g = |k: &str| f.map_or_else(|| m.get(k), |f| f.get(m, k));
    let main = match m.grp.as_str() {
        "fsd" => format!("optimal mass {:.0} t, {:.1} t of fuel a jump", g("optmass"), g("maxfuel")),
        "pp" => format!("{:.1} MW", g("pgen")),
        "t" => format!("optimal mass {:.0} t", g("optmass")),
        "sg" | "bsg" | "psg" => format!("optimal mass {:.0} t", g("optmass")),
        "pd" => format!("weapons {:.0} MJ, {:.1} MW", g("wepcap"), g("weprate")),
        "ft" => format!("{:.0} t of fuel", g("fuel")),
        "sb" => format!("+{:.0} % shields", g("shieldboost") * 100.0),
        "hr" | "ghrp" | "mahr" => format!("+{:.0} armour", g("hullreinforcement")),
        "gfsb" => format!("+{:.2} ly a jump", g("jumpboost")),
        "gsrp" => format!("+{:.0} MJ shields", g("shieldaddition")),
        _ if m.figures.contains_key("cargo") => format!("{:.0} t of cargo", g("cargo")),
        _ if m.figures.contains_key("fuel") => format!("{:.0} t of fuel", g("fuel")),
        _ if m.figures.contains_key("passengers") => format!("{:.0} passengers", g("passengers")),
        _ if m.figures.contains_key("damage") => format!("{:.1} damage a shot", g("damage")),
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

/// Engineering to plan for the slot's module: a blueprint, the grade to reach, an experimental effect.
fn goal_picker(ui: &mut Ui, f: &Fitted) -> Option<SlotEdit> {
    let blueprints: Vec<_> = nexus_data::engineering::blueprints_for_module(&f.item).into_iter().filter(|b| b.merc_module.is_none()).collect();
    if blueprints.is_empty() {
        return None;
    }
    let mut edit = None;
    widgets::section(ui, "Engineering to plan");
    let goal = f.goal.as_ref();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        if widgets::pill(ui, goal.is_none(), "None").clicked() && goal.is_some() {
            edit = Some(SlotEdit::Goal(None));
        }
        for bp in &blueprints {
            let on = goal.is_some_and(|g| bp.is(&g.blueprint));
            if widgets::pill(ui, on, &bp.name).clicked() && !on {
                edit = Some(SlotEdit::Goal(Some(Goal { blueprint: bp.symbol.clone(), grade: bp.max_grade(), experimental: None })));
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
        let mut grade = g.grade;
        if widgets::segmented(ui, &mut grade, &options) {
            edit = Some(SlotEdit::Goal(Some(Goal { grade, ..g.clone() })));
        }
    });
    let experimentals = nexus_data::engineering::experimentals_for(&f.item);
    if !experimentals.is_empty() {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            ui.label(RichText::new("Experimental").color(pal::DIM));
            if widgets::pill(ui, g.experimental.is_none(), "None").clicked() && g.experimental.is_some() {
                edit = Some(SlotEdit::Goal(Some(Goal { experimental: None, ..g.clone() })));
            }
            for x in experimentals {
                let on = g.experimental.as_deref() == Some(x.symbol.as_str());
                if widgets::pill(ui, on, &x.name).clicked() && !on {
                    edit = Some(SlotEdit::Goal(Some(Goal { experimental: Some(x.symbol.clone()), ..g.clone() })));
                }
            }
        });
    }
    edit
}

/// The modules that fit the open slot, to pick one, and the engineering to plan for the one in it.
fn module_list(ui: &mut Ui, st: &mut PlanUi, hull: &Hull, plan: &Plan, s: &SlotDef, fitted: Option<&Fitted>, out: &mut Option<PlanOut>) -> Option<SlotEdit> {
    let mut pick = None;
    egui::Frame::new().fill(pal::FIELD).corner_radius(10).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if let Some(f) = fitted
            && let Some(e) = goal_picker(ui, f)
        {
            pick = Some(e);
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
                        ui.add(Label::new(RichText::new(figure_line(m, None)).size(size::CAPTION).color(pal::DIM)).truncate());
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

/// The figures the plan comes to, as tiles: how each changed against `before` (the commander's ship when `owned`, else
/// the hull as a shipyard sells it), and the game's own figures for their ship (`game`) while it's unchanged.
fn figures_cards(ui: &mut Ui, hull: &Hull, plan: &Plan, f: &Figures, before: Option<&Figures>, owned: bool, game: Option<Reported>) {
    let says = |what: f64, unit: &str, decimals: usize| if what > 0.0 { format!("The game reports {what:.decimals$}{unit} for this ship") } else { String::new() };
    let diff = |now: f64, was: Option<f64>, decimals: usize, more_is_better: bool, unit: &str| -> Option<(String, Color32)> {
        let was = was?;
        let d = now - was;
        if d.abs() < 0.5 * 10f64.powi(-(decimals as i32)) {
            return None;
        }
        let good = (d > 0.0) == more_is_better;
        Some((format!("{}{d:.prec$}{unit}", if d > 0.0 { "+" } else { "" }, prec = decimals), if good { pal::GOOD } else { pal::BAD }))
    };
    // A drive or thrusters this data has no figures for give no range or speed, not a range or speed of 0.
    let missing = |slot: &str, none: &str, newer: &str| match plan.module_in(slot) {
        None => Some(none.to_string()),
        Some(m) if fitting::module(&m.item).is_none() => Some(newer.to_string()),
        Some(_) => None,
    };
    let compared = if owned { "Compared with the ship as it is" } else { "Compared with the hull as a shipyard sells it" };
    widgets::cards(ui, 2, 200.0, |c| {
        c.add(|ui| match missing("FrameShiftDrive", "No frame shift drive fitted", "This frame shift drive is newer than this version's data") {
            Some(why) => tile(ui, icon::ARROW_FAT_RIGHT, "Jump range", pal::ACCENT, "No figures", None, compared, &[why]),
            None => tile(ui, icon::ARROW_FAT_RIGHT, "Jump range", pal::ACCENT, &format!("{:.2} ly", f.max_range), diff(f.max_range, before.map(|b| b.max_range), 2, true, " ly"), compared, &[
                format!("Full tank {:.2} ly · laden {:.2} ly", f.full_tank_range, f.laden_range),
                format!("{:.0} ly on one tank", f.total_range),
                game.map(|g| says(g.range, " ly", 2)).unwrap_or_default(),
            ]),
        });
        c.add(|ui| match missing("MainEngines", "No thrusters fitted", "These thrusters are newer than this version's data") {
            Some(why) => tile(ui, icon::GAUGE, "Speed", pal::CYAN, "No figures", None, compared, &[why]),
            None => {
                let speed = if f.speed > 0.0 { format!("{:.0} m/s", f.speed) } else { "Too heavy".into() };
                let boost = if f.speed <= 0.0 {
                    "More than these thrusters can move".to_string()
                } else if f.boost > 0.0 {
                    format!("Boost {:.0} m/s", f.boost)
                } else {
                    format!("Can't boost: the power distributor's engines capacitor holds {:.0} MJ, a boost takes {:.0} MJ", f.engines_capacity, f.boost_energy)
                };
                tile(ui, icon::GAUGE, "Speed", pal::CYAN, &speed, diff(f.speed, before.map(|b| b.speed), 0, true, " m/s"), compared, &[boost, "Four pips to engines, full tank, empty hold".into()]);
            }
        });
        c.add(|ui| match f.shield {
            Some(s) => tile(ui, icon::SHIELD, "Shields", pal::BLUE, &format!("{:.0} MJ", s.strength), diff(s.strength, before.and_then(|b| b.shield).map(|b| b.strength), 0, true, " MJ"), compared, &[
                format!("Kinetic {:.0} % · thermal {:.0} % · explosive {:.0} %", s.res.kinetic * 100.0, s.res.thermal * 100.0, s.res.explosive * 100.0),
                if s.cells > 0.0 { format!("Shield cells add {:.0} MJ", s.cells) } else { String::new() },
            ]),
            None => tile(ui, icon::SHIELD, "Shields", pal::BLUE, "None", None, compared, &["No shield generator fitted".into()]),
        });
        c.add(|ui| {
            tile(ui, icon::SHIELD_CHEVRON, "Armour", pal::GRAY, &format!("{:.0}", f.armour), diff(f.armour, before.map(|b| b.armour), 0, true, ""), compared, &[
                format!("Kinetic {:.0} % · thermal {:.0} % · explosive {:.0} %", f.hull_res.kinetic * 100.0, f.hull_res.thermal * 100.0, f.hull_res.explosive * 100.0),
                if f.module_protection > 0.0 { format!("Modules protected {:.0} %", f.module_protection * 100.0) } else { format!("Hardness {:.0}", hull.hardness) },
            ]);
        });
        c.add(|ui| power_tile(ui, f));
        c.add(|ui| {
            tile(ui, icon::SCALES, "Mass", pal::MINT, &format!("{:.1} t", f.unladen_mass), diff(f.unladen_mass, before.map(|b| b.unladen_mass), 1, false, " t"), compared, &[
                format!("Laden {:.1} t", f.unladen_mass + f.fuel + f.cargo),
                format!("Cargo {:.0} t · fuel {:.0} t{}", f.cargo, f.fuel, if f.passengers > 0.0 { format!(" · {:.0} passengers", f.passengers) } else { String::new() }),
                game.map(|g| says(g.mass, " t", 1)).unwrap_or_default(),
            ]);
        });
        c.add(|ui| {
            tile(ui, icon::COINS, "Cost", pal::GOLD, &short_credits(f.total_cost() as i64), diff(f.total_cost() / 1e6, before.map(|b| b.total_cost() / 1e6), 1, false, " M"), compared, &[
                format!("Hull {} · modules {}", short_credits(f.hull_cost as i64), short_credits(f.modules_cost as i64)),
                format!("Rebuy {}", short_credits(f.rebuy() as i64)),
                // What the commander paid can differ from list prices.
                game.filter(|g| g.value > 0).map(|g| format!("The game values this ship at {}, rebuy {}", short_credits(g.value), short_credits(g.rebuy))).unwrap_or_default(),
            ]);
        });
    });
}

/// A figure tile: icon and name in its colour, the figure big, how it changed (`compared`: against what), and a few
/// lines under it.
#[allow(clippy::too_many_arguments)]
fn tile(ui: &mut Ui, glyph: &str, title: &str, tint: Color32, value: &str, change: Option<(String, Color32)>, compared: &str, lines: &[String]) {
    widgets::card_tinted(ui, glyph, title, tint, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(value).font(display(24.0)).color(pal::TEXT));
            if let Some((text, col)) = change {
                ui.label(RichText::new(text).font(display(size::CALLOUT)).color(col)).on_hover_text(compared);
            }
        });
        for l in lines.iter().filter(|l| !l.is_empty()) {
            ui.add(Label::new(RichText::new(l).size(size::FOOTNOTE).color(pal::DIM)).wrap());
        }
    });
}

/// Power: what's drawn against what the power plant makes, retracted and deployed.
fn power_tile(ui: &mut Ui, f: &Figures) {
    widgets::card_tinted(ui, icon::LIGHTNING, "Power", pal::WARN, |ui| {
        let plant = f.power_generated.max(0.01);
        let over = f.power_deployed > f.power_generated + 1e-9;
        ui.label(RichText::new(format!("{:.1} / {:.1} MW", f.power_deployed, f.power_generated)).font(display(24.0)).color(if over { pal::BAD } else { pal::TEXT }));
        let col = if over { pal::BAD } else if f.power_deployed / plant > 0.9 { pal::WARN } else { pal::GOOD };
        widgets::bar(ui, (f.power_deployed / plant) as f32, col, 8.0, None);
        ui.add(Label::new(RichText::new(format!("Deployed {:.0} % · retracted {:.0} %", f.power_deployed / plant * 100.0, f.power_retracted / plant * 100.0)).size(size::FOOTNOTE).color(pal::DIM)).wrap());
        if over {
            ui.add(Label::new(RichText::new("More than the power plant makes: modules switch off when you deploy hardpoints. Fit a bigger power plant, or switch something off.").size(size::FOOTNOTE).color(pal::BAD)).wrap());
        }
    });
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
            &format!("Each planned blueprint from the grade above the one the module has of it, {} rolls a grade as the Engineering page plans, and each new experimental effect. Planned engineering doesn't change the figures above, and EDSY, Coriolis and Copy build don't carry it: they take the engineering already on the modules. Materials as your journal last listed them.", fitting::ROLLS_PER_GRADE),
        );
    });
}

/// What to buy for the plan: against the commander's ship (`owned`), what's new on it; else the hull at a shipyard's
/// price, with its standard modules, and what's new against those. Each at its list price.
fn whats_new(ui: &mut Ui, hull: &Hull, reference: Option<&Plan>, owned: bool, source: &str, plan: &Plan) -> Option<PlanOut> {
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
    let hull_price = (!owned).then(|| shipyard_price(hull));
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
                total += b.cost;
                row(ui, format!("{} bulkheads", b.name), b.cost);
            }
            for f in &new {
                let price = fitting::module(&f.item).map_or(0.0, |m| m.cost);
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
                ui.label(RichText::new(format!("{} at list prices", short_credits(total as i64))).font(display(size::CALLOUT)).color(pal::GOLD));
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
