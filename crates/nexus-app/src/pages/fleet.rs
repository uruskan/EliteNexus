use crate::jobs::Slot;
use crate::shopping::{self, Build, Need, Offer, Outcome, Search};
use crate::theme::{display, pal, size};
use crate::widgets::{self, badge, badge_size, card, card_with, empty, footnote, short_credits};
use egui::{Grid, Label, RichText, Sense, TextEdit, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use nexus_journal::Timestamp;
use nexus_model::fleet::{Ship, ShipModule, ShipPlace};
use nexus_model::GameState;
use nexus_spansh::{Coords, Reference, Spansh};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Ships,
    /// Where to buy a build.
    Buy,
}

#[derive(Default)]
pub struct FleetUi {
    pub selected: Option<u64>,
    pub tab: Tab,
    buy: BuyUi,
}

impl FleetUi {
    /// `--tab buy` opens Where to buy; in a screenshot run it starts from the current ship's build
    /// and searches on the first frame.
    pub fn new(tab: Option<&str>, shot: bool) -> Self {
        let mut st = FleetUi::default();
        if tab.is_some_and(|t| t.eq_ignore_ascii_case("buy")) {
            st.tab = Tab::Buy;
            st.buy.autorun = shot;
        }
        st
    }
}

/// What a click on the Fleet page asks the app to do.
pub enum FleetAction {
    /// Open Navigation with this system as the destination.
    PlotTo(String),
}

/// Where to buy: the build, its shopping list, the search and what it found.
struct BuyUi {
    paste: String,
    error: Option<String>,
    build: Option<Build>,
    /// Where the build came from: "pasted", or the ship's name.
    source: String,
    needs: Vec<Need>,
    /// Search near this system; empty: where the commander is.
    near: String,
    carriers: bool,
    max_age: Option<u32>,
    show_options: bool,
    job: Slot<Result<Outcome, String>>,
    /// What the shown results were searched for, and from where.
    searched: Option<(Search, String)>,
    autorun: bool,
}

impl Default for BuyUi {
    fn default() -> Self {
        BuyUi {
            paste: String::new(),
            error: None,
            build: None,
            source: String::new(),
            needs: Vec::new(),
            near: String::new(),
            carriers: true,
            max_age: None,
            show_options: false,
            job: Slot::default(),
            searched: None,
            autorun: false,
        }
    }
}

impl BuyUi {
    fn load(&mut self, build: Build, source: String, gs: &GameState) {
        self.needs = shopping::needs(&build, &gs.fleet);
        self.build = Some(build);
        self.source = source;
        self.error = None;
        self.paste.clear();
        self.job = Slot::default();
        self.searched = None;
    }

    fn load_ship(&mut self, s: &Ship, gs: &GameState) {
        match s.loadout_json.as_deref().map(shopping::parse_build) {
            Some(Ok(b)) => self.load(b, s.display_name(), gs),
            Some(Err(e)) => self.error = Some(e),
            None => self.error = Some("No loadout recorded for this ship yet: fly it once and Elite Nexus captures the build.".into()),
        }
    }
}

pub fn show(ui: &mut Ui, gs: &GameState, st: &mut FleetUi, spansh: &Spansh, rt: &tokio::runtime::Handle) -> Option<FleetAction> {
    st.buy.job.tick();
    if let Some(t) = widgets::tabs(ui, st.tab, &[(Tab::Ships, "Ships"), (Tab::Buy, "Where to buy")]) {
        st.tab = t;
    }
    ui.add_space(12.0);
    match st.tab {
        Tab::Ships => {
            if let Some(id) = ships_tab(ui, gs, st)
                && let Some(s) = gs.fleet.ships.get(&id)
            {
                st.buy.load_ship(s, gs);
                st.tab = Tab::Buy;
            }
            None
        }
        Tab::Buy => buy_tab(ui, gs, &mut st.buy, spansh, rt),
    }
}

/// The ships and the selected one's loadout. Returns a ship whose "Where to buy" was clicked.
fn ships_tab(ui: &mut Ui, gs: &GameState, st: &mut FleetUi) -> Option<u64> {
    if gs.fleet.ships.is_empty() {
        empty(ui, icon::ROCKET, "No ships yet", "Ships appear after the game writes a Loadout or StoredShips event.");
        return None;
    }
    let selected = st.selected.or(gs.fleet.current_ship_id);
    let mut ships: Vec<&Ship> = gs.fleet.ships.values().collect();
    ships.sort_by(|a, b| (Some(b.id) == gs.fleet.current_ship_id).cmp(&(Some(a.id) == gs.fleet.current_ship_id)).then(b.total_value().cmp(&a.total_value())));
    let mut buy = None;
    // The ship list goes beside the loadout only where that leaves the loadout about 700 pt, which its module table
    // needs for long module names and engineering badges; in a narrower window the ships are pills above it.
    if ui.available_width() < 1050.0 {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for s in &ships {
                let name = if s.name.trim().is_empty() { s.type_name.clone() } else { s.name.clone() };
                if crate::widgets::pill(ui, selected == Some(s.id), &name).clicked() {
                    st.selected = Some(s.id);
                }
            }
        });
        ui.add_space(crate::widgets::GAP);
        if let Some(s) = selected.and_then(|id| gs.fleet.ships.get(&id))
            && ship_detail(ui, gs, s)
        {
            buy = Some(s.id);
        }
        return buy;
    }
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(330.0);
            ui.label(RichText::new(format!("{} ships · {}", ships.len(), short_credits(gs.fleet.total_value()))).font(display(16.0)).color(pal::GOLD));
            egui::ScrollArea::vertical().id_salt("shiplist").show(ui, |ui| {
                for s in &ships {
                    ship_tile(ui, gs, s, selected == Some(s.id), st);
                    ui.add_space(4.0);
                }
            });
        });
        ui.add_space(crate::widgets::GAP - ui.spacing().item_spacing.x);
        ui.vertical(|ui| {
            if let Some(s) = selected.and_then(|id| gs.fleet.ships.get(&id))
                && ship_detail(ui, gs, s)
            {
                buy = Some(s.id);
            }
        });
    });
    buy
}

fn ship_tile(ui: &mut Ui, gs: &GameState, s: &Ship, selected: bool, st: &mut FleetUi) {
    let current = Some(s.id) == gs.fleet.current_ship_id;
    let fill = if selected { pal::CARD_HI } else { pal::CARD };
    let stroke = if selected { egui::Stroke::new(1.5, pal::ACCENT) } else { egui::Stroke::NONE };
    let resp = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(14)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(if s.name.trim().is_empty() { &s.type_name } else { &s.name }).font(display(16.0)).color(pal::TEXT));
                if current {
                    badge(ui, "Active", pal::GOOD);
                }
                if s.hot {
                    badge(ui, "Hot", pal::BAD);
                }
            });
            // A hull newer than this build's ship list has no pad to show, rather than a wrong one.
            let kind = match nexus_data::ships::known_pad_size(&s.ship_type) {
                Some(pad) => format!("{} · {pad} pad", s.type_name),
                None => s.type_name.clone(),
            };
            ui.label(RichText::new(kind).color(pal::DIM));
            ui.horizontal(|ui| {
                if s.max_jump_range > 0.0 {
                    ui.label(RichText::new(format!("{} {:.1} ly", icon::ARROW_FAT_RIGHT, s.max_jump_range)).color(pal::ACCENT_SOFT));
                }
                ui.label(RichText::new(short_credits(s.total_value())).color(pal::GOLD));
                if s.engineered_modules() > 0 {
                    ui.label(RichText::new(format!("{} {}", icon::WRENCH, s.engineered_modules())).color(pal::PURPLE));
                }
            });
            let loc = match gs.ship_place(s) {
                ShipPlace::WithYou => gs.location.situation(),
                ShipPlace::Left(l) => format!("{} Left at {}", icon::MAP_PIN, l.text().unwrap_or_else(|| "a place your journal doesn't name".into())),
                ShipPlace::Away => format!("{} Not with you: your journal doesn't say where you left it", icon::MAP_PIN),
                ShipPlace::Stored(l) if l.in_transit_at(Timestamp::now()) => format!("{} In transit → {}", icon::TRUCK, l.text().unwrap_or_default()),
                ShipPlace::Stored(l) => format!("{} {}", icon::MAP_PIN, l.text().unwrap_or_default()),
                ShipPlace::Unknown => "Location unknown".into(),
            };
            ui.add(Label::new(RichText::new(loc).small().color(pal::DIM)).wrap());
        })
        .response
        .interact(Sense::click());
    if resp.clicked() {
        st.selected = Some(s.id);
    }
    ui.add_space(4.0);
}

fn slot_group(slot: &str) -> (u8, &'static str) {
    let s = slot.to_ascii_lowercase();
    if s.contains("hardpoint") {
        if s.starts_with("tiny") { (1, "Utility") } else { (0, "Hardpoints") }
    } else if matches!(
        s.as_str(),
        "armour" | "powerplant" | "mainengines" | "frameshiftdrive" | "lifesupport" | "powerdistributor" | "radar" | "fueltank"
    ) {
        (2, "Core internals")
    } else if s.starts_with("slot") || s.contains("military") {
        (3, "Optional internals")
    } else {
        (4, "Other")
    }
}

/// The Loadout card's buttons for the ship's build, as they read from left to right.
#[derive(Clone, Copy)]
enum BuildButton {
    WhereToBuy,
    CopyJson,
    Edsy,
    Coriolis,
}

impl BuildButton {
    const ALL: [BuildButton; 4] = [BuildButton::WhereToBuy, BuildButton::CopyJson, BuildButton::Edsy, BuildButton::Coriolis];

    fn label(self) -> String {
        match self {
            BuildButton::WhereToBuy => format!("{} Where to buy", icon::SHOPPING_CART),
            BuildButton::CopyJson => format!("{} Copy JSON", icon::COPY),
            BuildButton::Edsy => format!("{} EDSY", icon::ARROW_SQUARE_OUT),
            BuildButton::Coriolis => format!("{} Coriolis", icon::ARROW_SQUARE_OUT),
        }
    }

    fn tip(self) -> Option<&'static str> {
        match self {
            BuildButton::WhereToBuy => Some("Find the nearest stations that sell this build's modules and hull"),
            BuildButton::CopyJson => None,
            BuildButton::Edsy => Some("Open this exact build in edsy.org"),
            BuildButton::Coriolis => Some("Open this exact build in coriolis.io"),
        }
    }
}

/// The build's buttons in a row; `right_to_left` adds them from the right edge, so they still read in the same
/// order. Returns the one clicked.
fn build_buttons(ui: &mut Ui, right_to_left: bool) -> Option<BuildButton> {
    let mut clicked = None;
    let mut add = |ui: &mut Ui, b: BuildButton| {
        let r = ui.button(b.label());
        let r = match b.tip() {
            Some(tip) => r.on_hover_text(tip),
            None => r,
        };
        if r.clicked() {
            clicked = Some(b);
        }
    };
    if right_to_left {
        BuildButton::ALL.into_iter().rev().for_each(|b| add(ui, b));
    } else {
        BuildButton::ALL.into_iter().for_each(|b| add(ui, b));
    }
    clicked
}

/// The selected ship's loadout. Returns true when "Where to buy" was clicked.
fn ship_detail(ui: &mut Ui, _gs: &GameState, s: &Ship) -> bool {
    let mut buy = false;
    card(ui, "Loadout", |ui| {
        let name = RichText::new(s.display_name()).font(display(22.0));
        match &s.loadout_json {
            None => {
                ui.add(Label::new(name).wrap());
            }
            Some(json) => {
                // The name and the buttons share a row while both fit whole; in a narrower card the buttons go under
                // the name (and wrap if they must) instead of running over it.
                let gap = ui.spacing().item_spacing.x;
                let font = egui::TextStyle::Button.resolve(ui.style());
                let pad = 2.0 * ui.spacing().button_padding.x;
                let buttons_w: f32 = BuildButton::ALL.iter().map(|b| ui.painter().layout_no_wrap(b.label(), font.clone(), pal::TEXT).size().x + pad + gap).sum();
                let name_w = ui.painter().layout_no_wrap(s.display_name(), display(22.0), pal::TEXT).size().x;
                let clicked = if name_w + gap + buttons_w <= ui.available_width() {
                    ui.horizontal(|ui| {
                        ui.label(name);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| build_buttons(ui, true)).inner
                    })
                    .inner
                } else {
                    ui.add(Label::new(name).wrap());
                    ui.horizontal_wrapped(|ui| build_buttons(ui, false)).inner
                };
                match clicked {
                    Some(BuildButton::WhereToBuy) => buy = true,
                    Some(BuildButton::CopyJson) => ui.ctx().copy_text(json.clone()),
                    Some(BuildButton::Edsy) => {
                        if let Some(url) = crate::export::edsy_url(json) {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        }
                    }
                    Some(BuildButton::Coriolis) => {
                        if let Some(url) = crate::export::coriolis_url(json) {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        }
                    }
                    None => {}
                }
            }
        }
        Grid::new("shipstats").num_columns(6).spacing([14.0, 3.0]).show(ui, |ui| {
            kv_inline(ui, "Jump range", format!("{:.2} ly", s.max_jump_range));
            kv_inline(ui, "Cargo", format!("{} t", s.cargo_capacity));
            kv_inline(ui, "Fuel", format!("{:.0} t", s.fuel_main_capacity));
            ui.end_row();
            kv_inline(ui, "Hull value", short_credits(s.hull_value));
            kv_inline(ui, "Modules", short_credits(s.modules_value));
            kv_inline(ui, "Rebuy", short_credits(s.rebuy));
            ui.end_row();
            kv_inline(ui, "Unladen mass", format!("{:.1} t", s.unladen_mass));
            kv_inline(ui, "Hull", format!("{:.0}%", s.hull_health * 100.0));
            kv_inline(ui, "Loadout seen", if s.loadout_ts.is_zero() { "never".into() } else { s.loadout_ts.to_local_string("%d %b %Y") });
            ui.end_row();
            if let Some(pad) = nexus_data::ships::known_pad_size(&s.ship_type) {
                kv_inline(ui, "Landing pad", match pad {
                    'L' => "Large",
                    'M' => "Medium",
                    _ => "Small",
                }.into());
                ui.end_row();
            }
        });
    });
    ui.add_space(8.0);
    if s.modules.is_empty() {
        card(ui, "Modules", |ui| {
            ui.label(RichText::new("No loadout recorded for this ship yet — fly it once and Elite Nexus captures the full build.").color(pal::DIM));
        });
        return buy;
    }
    let mut modules: Vec<&ShipModule> = s.modules.iter().filter(|m| !m.item.contains("_cockpit") && !m.slot.eq_ignore_ascii_case("VesselVoice") && !m.slot.contains("Paint") && !m.slot.contains("Decal") && !m.slot.contains("Name") && !m.slot.contains("Bobble") && !m.slot.contains("Livery")).collect();
    modules.sort_by(|a, b| slot_group(&a.slot).0.cmp(&slot_group(&b.slot).0).then(a.slot.cmp(&b.slot)));
    // Slot and Module are as wide as their longest text, Module only as far as the engineering badges leave room, and
    // together at least the 370 they always had while the window allows. A name that still doesn't fit ends in "…" and
    // shows whole on hover ("8A Mk II Supercharge Optimised Frame Shift Drive (SCO)"). Where a blueprint's badge and
    // its experimental effect's side by side would squeeze Module below that, the effect goes under the blueprint in a
    // taller row, so the badges always stay inside the table; Module then stops at those 370 and leaves the badges the
    // rest, so few rows need the second line.
    let slots: Vec<String> = modules.iter().map(|m| nexus_data::outfitting::slot_display(&m.slot)).collect();
    let names: Vec<String> = modules.iter().map(|m| nexus_data::outfitting::display_name(&m.item)).collect();
    let values: Vec<String> = modules.iter().map(|m| m.value.map(short_credits).unwrap_or_default()).collect();
    let blueprints: Vec<Option<String>> = modules.iter().map(|m| m.engineering.as_ref().map(|e| format!("G{} {}", e.level, nexus_data::names::split_camel(&e.blueprint.replace('_', " "))))).collect();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let widest = |texts: &[String]| texts.iter().map(|t| ui.painter().layout_no_wrap(t.clone(), font.clone(), pal::TEXT).size().x + 8.0).fold(0.0, f32::max);
    let gap = ui.spacing().item_spacing.x;
    // Each row's blueprint and experimental badge widths (0: none), and the column they need side by side or stacked.
    let badge_w: Vec<[f32; 2]> = modules
        .iter()
        .zip(&blueprints)
        .map(|(m, bp)| [bp.as_deref(), m.engineering.as_ref().and_then(|e| e.experimental.as_deref())].map(|t| t.map_or(0.0, |t| badge_size(ui, t).x)))
        .collect();
    let pair = |[b, x]: [f32; 2]| if x > 0.0 { b + gap + x } else { b };
    let side_by_side = badge_w.iter().map(|w| pair(*w)).fold(120.0, f32::max);
    let stacked = badge_w.iter().map(|[b, x]| b.max(*x)).fold(120.0, f32::max);
    let slot_w = widest(&slots).clamp(130.0, 180.0);
    let value_w = widest(&values).max(80.0);
    let room = ui.available_width() - slot_w - value_w - 3.0 * gap;
    let (module_want, floor) = (widest(&names), 370.0 - slot_w);
    let stack = room - side_by_side < module_want.min(floor);
    let (eng_w, module_max) = if stack { (stacked, module_want.min(floor)) } else { (side_by_side, module_want.max(floor)) };
    let module_w = (room - eng_w).min(module_max).max(100.0);
    // A row whose badges are stacked is one badge, and the 4 pt between them, taller.
    let badge_h = badge_size(ui, "G5").y;
    let heights: Vec<f32> = badge_w.iter().map(|w| if w[1] > 0.0 && pair(*w) > room - module_w { 36.0 + badge_h } else { 32.0 }).collect();
    TableBuilder::new(ui)
        .striped(true)
        .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(slot_w).clip(true))
        .column(Column::exact(module_w).clip(true))
        .column(Column::exact(value_w))
        .column(Column::remainder().clip(true))
        .header(30.0, |mut h| {
            for t in ["Slot", "Module", "Value", "Engineering"] {
                h.col(|ui| {
                    crate::widgets::th(ui, t);
                });
            }
        })
        .body(|body| {
            body.heterogeneous_rows(heights.iter().copied(), |mut row| {
                let i = row.index();
                let m = modules[i];
                row.col(|ui| {
                    ui.add(egui::Label::new(RichText::new(&slots[i]).color(pal::DIM)).truncate());
                });
                row.col(|ui| {
                    let t = RichText::new(&names[i]);
                    ui.add(egui::Label::new(if m.on { t } else { t.color(pal::FAINT) }).truncate());
                });
                row.col(|ui| {
                    if m.value.is_some() {
                        ui.label(RichText::new(&values[i]).color(pal::DIM));
                    }
                });
                row.col(|ui| {
                    if let (Some(e), Some(bp)) = (&m.engineering, &blueprints[i]) {
                        let badges = |ui: &mut Ui| {
                            badge(ui, bp, pal::PURPLE);
                            if let Some(x) = &e.experimental {
                                badge(ui, x, pal::ACCENT);
                            }
                        };
                        let shown = if heights[i] > 32.0 {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 4.0;
                                ui.add_space((ui.available_height() - 2.0 * badge_h - 4.0) / 2.0);
                                badges(ui);
                            })
                        } else {
                            ui.horizontal(badges)
                        };
                        shown.response.on_hover_ui(|ui| {
                            for md in &e.modifiers {
                                if let Some(p) = md.improvement_pct() {
                                    let col = if p >= 0.0 { pal::GOOD } else { pal::BAD };
                                    ui.label(RichText::new(format!("{}: {:+.1}%", md.label, p)).color(col));
                                }
                            }
                        });
                    }
                });
            });
        });
    buy
}

fn kv_inline(ui: &mut Ui, k: &str, v: String) {
    ui.label(RichText::new(k).color(pal::DIM));
    ui.label(RichText::new(v).color(pal::TEXT));
}

// ---------------------------------------------------------------------------------------------
// Where to buy

fn buy_tab(ui: &mut Ui, gs: &GameState, st: &mut BuyUi, spansh: &Spansh, rt: &tokio::runtime::Handle) -> Option<FleetAction> {
    let mut search_now = false;
    if std::mem::take(&mut st.autorun)
        && let Some(s) = gs.fleet.current()
    {
        st.load_ship(s, gs);
        search_now = st.build.is_some();
    }
    let mut action = None;
    egui::ScrollArea::vertical().id_salt("buy").auto_shrink([false, false]).show(ui, |ui| {
        let avail = ui.available_width();
        if avail >= 880.0 {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((avail * 0.42).clamp(380.0, 560.0));
                    build_card(ui, gs, st);
                });
                ui.add_space(widgets::GAP - ui.spacing().item_spacing.x);
                ui.vertical(|ui| action = search_card(ui, gs, st, spansh, rt, search_now));
            });
        } else if st.build.is_some() {
            // Stacked, the results come first and the list to edit under them.
            action = search_card(ui, gs, st, spansh, rt, search_now);
            ui.add_space(widgets::GAP);
            build_card(ui, gs, st);
        } else {
            build_card(ui, gs, st);
            ui.add_space(widgets::GAP);
            action = search_card(ui, gs, st, spansh, rt, search_now);
        }
    });
    action
}

/// The build's shopping list, or where to paste one.
fn build_card(ui: &mut Ui, gs: &GameState, st: &mut BuyUi) {
    if st.build.is_none() {
        card(ui, "Build", |ui| paste_box(ui, gs, st));
        return;
    }
    let (title, loadout) = match &st.build {
        Some(b) => (b.name.clone().map_or_else(|| nexus_data::ships::ship_name(&b.ship), |n| format!("{n} · {}", nexus_data::ships::ship_name(&b.ship))), b.loadout.clone()),
        None => return,
    };
    let mut change = false;
    card_with(
        ui,
        &title,
        |ui| {
            change = widgets::button(ui, "Change").on_hover_text("Paste another build or pick another ship").clicked();
            if ui.button(format!("{} Coriolis", icon::ARROW_SQUARE_OUT)).on_hover_text("Open this build in coriolis.io: power, shields, jump range").clicked()
                && let Some(url) = crate::export::coriolis_url(&loadout)
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
            if ui.button(format!("{} EDSY", icon::ARROW_SQUARE_OUT)).on_hover_text("Open this build in edsy.org: power, shields, jump range").clicked()
                && let Some(url) = crate::export::edsy_url(&loadout)
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        },
        |ui| {
            let to_buy = st.needs.iter().filter(|n| n.buy).count();
            ui.label(RichText::new(format!("{to_buy} of {} to buy · from {}", st.needs.len(), st.source)).color(pal::DIM));
            ui.add_space(2.0);
            let mut last = None;
            for n in &mut st.needs {
                if last != Some(n.group) {
                    widgets::section(ui, n.group.label());
                    last = Some(n.group);
                }
                need_row(ui, n);
            }
            ui.add_space(4.0);
            footnote(ui, "Untick what you don't need to buy. Stations sell modules plain: engineer them afterwards.");
        },
    );
    if change {
        *st = BuyUi { near: std::mem::take(&mut st.near), carriers: st.carriers, max_age: st.max_age, ..BuyUi::default() };
    }
}

fn need_row(ui: &mut Ui, n: &mut Need) {
    let name = if n.count > 1 { format!("{} ×{}", n.name, n.count) } else { n.name.clone() };
    let col = if n.buy { pal::TEXT } else { pal::DIM };
    ui.checkbox(&mut n.buy, RichText::new(name).color(col));
    let extra = n.unlock.is_some() || n.note.is_some() || !n.engineering.is_empty();
    if extra {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(26.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            if let Some(u) = n.unlock {
                badge(ui, u.label(), pal::WARN).on_hover_text(u.explain());
            }
            if !n.engineering.is_empty() {
                let mut e = n.engineering.clone();
                e.dedup();
                badge(ui, &e.join(" · "), pal::PURPLE).on_hover_text("Engineered in the build: stations sell the plain module, and engineers do the rest");
            }
            if let Some(note) = &n.note {
                ui.label(RichText::new(note).size(size::FOOTNOTE).color(pal::DIM));
            }
        });
    }
}

fn paste_box(ui: &mut Ui, gs: &GameState, st: &mut BuyUi) {
    ui.add(
        Label::new(
            RichText::new("Paste a build: a SLEF export from EDSY (OPS › Export… › SLEF) or Coriolis (name the build, then Export › SLEF), a journal Loadout event, or an EDSY or Coriolis import link.")
                .color(pal::DIM),
        )
        .wrap(),
    );
    ui.add_space(8.0);
    let edit = ui.add(TextEdit::multiline(&mut st.paste).hint_text("Paste the build here").desired_rows(4).desired_width(f32::INFINITY));
    // A pasted build reads at once, and a paste it can't read says why.
    if edit.changed() {
        match shopping::parse_build(&st.paste) {
            Ok(b) => {
                st.load(b, "pasted".into(), gs);
                return;
            }
            Err(e) => st.error = (!st.paste.trim().is_empty()).then_some(e),
        }
    }
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        let ready = !st.paste.trim().is_empty();
        if ui.add_enabled_ui(ready, |ui| widgets::primary_button(ui, "Read build")).inner.clicked() {
            match shopping::parse_build(&st.paste) {
                Ok(b) => st.load(b, "pasted".into(), gs),
                Err(e) => st.error = Some(e),
            }
        }
    });
    if let Some(e) = &st.error {
        ui.add_space(4.0);
        ui.add(Label::new(RichText::new(format!("{} {e}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
    }
    let mine: Vec<&Ship> = gs.fleet.ships.values().filter(|s| s.loadout_json.is_some()).collect();
    if !mine.is_empty() {
        ui.add_space(10.0);
        ui.label(RichText::new("Or start from one of your ships").color(pal::DIM));
        ui.add_space(4.0);
        let mut pick = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for s in mine {
                let name = if s.name.trim().is_empty() { s.type_name.clone() } else { format!("{} · {}", s.name.trim(), s.type_name) };
                if widgets::pill(ui, false, &name).clicked() {
                    pick = Some(s);
                }
            }
        });
        if let Some(s) = pick {
            st.load_ship(s, gs);
        }
    }
}

/// Where to search, the search button and what it found.
fn search_card(ui: &mut Ui, gs: &GameState, st: &mut BuyUi, spansh: &Spansh, rt: &tokio::runtime::Handle, search_now: bool) -> Option<FleetAction> {
    let mut action = None;
    card(ui, "Find stations", |ui| {
        let Some(build) = &st.build else {
            ui.add(Label::new(RichText::new("Paste a build or pick one of your ships, and this finds the nearest stations that sell all of it: starports, outposts, settlements and fleet carriers.").color(pal::DIM)).wrap());
            return;
        };
        let ship = build.ship.clone();
        let name = nexus_data::ships::ship_name(&ship);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            ui.label(RichText::new("Near").color(pal::DIM));
            ui.add(TextEdit::singleline(&mut st.near).hint_text(gs.location.system.as_str()).desired_width(190.0).margin(egui::Margin::symmetric(10, 6)));
            // A hull newer than this build's ship list is searched with any pad (Search::new), and says so.
            match nexus_data::ships::known_pad_size(&ship) {
                Some(pad) => badge(ui, &format!("{pad} pad"), pal::GRAY).on_hover_text(format!("{} {name} lands on {} pads: stations without one are left out.", a_or_an(&name), pad_word(pad))),
                None => badge(ui, "Pad unknown", pal::WARN).on_hover_text(format!("This version of Elite Nexus doesn't know which landing pad the {name} needs (a ship newer than it), so stations with any pad are listed: check a station's pads before you fly there.")),
            };
            widgets::toggle(ui, &mut st.carriers, "Fleet carriers").on_hover_text("Carriers sell what their owner stocks, and they move. Those that let only the owner's friends or squadron dock are left out.");
        });
        ui.add_space(4.0);
        if widgets::disclosure(ui, &mut st.show_options, "Options") {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                ui.label(RichText::new("Outfitting data no older than").color(pal::DIM));
                widgets::segmented(ui, &mut st.max_age, &[(None, "Any age"), (Some(30), "30 days"), (Some(7), "7 days")]);
            });
            ui.add_space(4.0);
        }
        let busy = st.job.busy();
        let wanted = st.needs.iter().filter(|n| n.buy).count();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            let label = if busy { format!("{} Searching…", icon::HOURGLASS) } else { format!("{} Find stations", icon::MAGNIFYING_GLASS) };
            let clicked = ui.add_enabled_ui(!busy && wanted > 0, |ui| widgets::primary_button(ui, &label)).inner.clicked();
            if (clicked || search_now) && !busy && wanted > 0 {
                let typed = st.near.trim().to_string();
                let (near, from) = match (typed.is_empty(), gs.location.star_pos) {
                    (true, Some([x, y, z])) => (Reference::Coords(Coords::new(x, y, z)), gs.location.system.clone()),
                    (true, None) => (Reference::System(gs.location.system.clone()), gs.location.system.clone()),
                    (false, _) => (Reference::System(typed.clone()), typed),
                };
                let q = Search::new(&st.needs, &ship, near, st.carriers, st.max_age);
                let (sp, query) = (spansh.clone(), q.clone());
                st.job.run_async(ui.ctx(), rt, async move { shopping::find(&sp, &query).await });
                st.searched = Some((q, from));
            }
            if busy {
                ui.spinner();
                ui.label(RichText::new("Spansh checks the nearest stations first, a page at a time").color(pal::DIM));
            } else if wanted == 0 {
                ui.label(RichText::new("Nothing ticked to buy").color(pal::DIM));
            }
        });
        ui.add_space(8.0);
        match (&st.job.last, &st.searched) {
            (Some(Err(e)), _) => {
                ui.add(Label::new(RichText::new(format!("{} {e}", icon::WARNING_CIRCLE)).color(pal::BAD)).wrap());
            }
            (Some(Ok(out)), Some((q, from))) => action = outcome(ui, out, q, from),
            _ => {}
        }
        ui.add_space(6.0);
        footnote(
            ui,
            "Stations, prices and data dates come from Spansh, which players' tools keep up to date through EDDN; a station's list is only as new as the last visit by someone who shares data. Outfitting rarely changes, but fleet carriers move and their owners change what they stock. Each station is checked for the exact module, a weapon's mount and the hull's own bulkheads included.",
        );
        ui.label(RichText::new("Station data © Spansh (spansh.co.uk)").size(size::CAPTION).color(pal::FAINT));
    });
    action
}

fn a_or_an(word: &str) -> &'static str {
    if word.starts_with(['A', 'E', 'I', 'O', 'U']) { "An" } else { "A" }
}

fn pad_word(pad: char) -> &'static str {
    match pad {
        'L' => "large",
        'M' => "medium or large",
        _ => "any",
    }
}

/// The search's answer: the stations that sell everything, or the stops that cover it between them.
fn outcome(ui: &mut Ui, out: &Outcome, q: &Search, from: &str) -> Option<FleetAction> {
    let n = q.wanted.len();
    let all: Vec<usize> = (0..n).collect();
    if !out.one_stop.is_empty() {
        let what = if n == 1 { format!("Nearest to {from} that sell {}", q.wanted[0].name) } else { format!("Nearest to {from} that sell all {n}") };
        ui.label(RichText::new(what).font(display(size::HEADLINE)).color(pal::GOLD));
        stats_line(ui, out, q);
        ui.add_space(6.0);
        let rows: Vec<Row> = out.one_stop.iter().map(|o| Row { offer: o, items: all.clone(), distance: format!("{:.1} ly", o.distance_ly) }).collect();
        let action = offers_table(ui, "buy-one-stop", &rows, q);
        arx_note(ui, &rows, q);
        return action;
    }
    let Some(first) = &out.first else {
        let what = if n == 1 { q.wanted[0].name.clone() } else { format!("any of these {n} items") };
        empty(ui, icon::STOREFRONT, "No station found", &format!("Spansh knows no station near {from} that sells {what}{}. Try without the data-age limit, with fleet carriers, or from another system.", pad_clause(q)));
        return None;
    };
    ui.label(RichText::new(format!("No station near {from} sells all {n}")).font(display(size::HEADLINE)).color(pal::WARN));
    stats_line(ui, out, q);
    ui.add_space(6.0);
    let have: Vec<usize> = all.iter().copied().filter(|&i| first.sells(i)).collect();
    let rest: Vec<usize> = all.iter().copied().filter(|&i| !first.sells(i)).collect();
    let mut rows = vec![Row { offer: first, items: have.clone(), distance: format!("{:.1} ly", first.distance_ly) }];
    if let Some(second) = &out.second {
        rows.push(Row { offer: second, items: rest.clone(), distance: format!("+{:.1} ly", second.distance_ly) });
        let action = offers_table(ui, "buy-stops", &rows, q);
        ui.add_space(4.0);
        footnote(ui, &format!("Two stops: {} sells {} of the {n}, and {}, {:.1} ly further, the rest.", first.station, have.len(), second.station, second.distance_ly));
        arx_note(ui, &rows, q);
        return action;
    }
    // No second stop has all the rest: the nearest seller of each piece.
    let mut nowhere = Vec::new();
    for (i, seller) in &out.elsewhere {
        match seller {
            Some(o) => rows.push(Row { offer: o, items: vec![*i], distance: format!("{:.1} ly", o.distance_ly) }),
            None => nowhere.push(q.wanted[*i].name.as_str()),
        }
    }
    let action = offers_table(ui, "buy-stops", &rows, q);
    ui.add_space(4.0);
    footnote(ui, &format!("{} sells {} of the {n}. No one station near it sells the rest, so the rows under it are the nearest seller of each other piece.", first.station, have.len()));
    arx_note(ui, &rows, q);
    if !nowhere.is_empty() {
        ui.add(Label::new(RichText::new(format!("{} No station Spansh knows near {from} sells: {}.", icon::WARNING_CIRCLE, nowhere.join(", "))).color(pal::WARN)).wrap());
    }
    if out.not_looked_up > 0 {
        footnote(ui, &format!("{} more weren't looked up one by one (at most {} are).", out.not_looked_up, shopping::SINGLES));
    }
    action
}

/// When none of the listed stations has a credit price for the hull: new hulls are sold for ARX
/// first (early access), and FDevIDs isn't a reliable guide to when that ends.
fn arx_note(ui: &mut Ui, rows: &[Row], q: &Search) {
    let Some(hull) = q.wanted.iter().position(|w| w.hull) else { return };
    let sellers: Vec<_> = rows.iter().filter_map(|r| r.offer.sells.iter().find(|(i, _)| *i == hull)).collect();
    if !sellers.is_empty() && sellers.iter().all(|(_, price)| price.is_none()) {
        ui.add_space(2.0);
        ui.add(Label::new(RichText::new(format!("{} Spansh lists no credit price for the {} at these stations: it may still be sold for ARX only (early access), so the totals leave it out.", icon::INFO, q.wanted[hull].name)).color(pal::WARN)).wrap());
    }
}

fn pad_clause(q: &Search) -> String {
    match q.pad {
        'L' => " with a large pad".into(),
        'M' => " with a medium or large pad".into(),
        _ => String::new(),
    }
}

/// One station in a results table: what to buy there, and its distance as shown.
struct Row<'a> {
    offer: &'a Offer,
    items: Vec<usize>,
    distance: String,
}

/// Stations with their distance, data age, price and buttons. Inside the tab's own scroll area,
/// so the table doesn't scroll by itself.
fn offers_table(ui: &mut Ui, id: &str, rows: &[Row], q: &Search) -> Option<FleetAction> {
    let mut action = None;
    TableBuilder::new(ui)
        .id_salt(id)
        .vscroll(false)
        .striped(true)
        .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::remainder().at_least(150.0).clip(true))
        .column(Column::initial(72.0))
        .column(Column::initial(78.0))
        .column(Column::initial(92.0))
        .column(Column::initial(68.0))
        .header(28.0, |mut h| {
            for t in ["Station", "Distance", "Data", "Price", ""] {
                h.col(|ui| widgets::th(ui, t));
            }
        })
        .body(|mut body| {
            for r in rows {
                let lines = 2 + usize::from(r.offer.state.is_some()) + usize::from(r.items.len() < q.wanted.len());
                body.row(12.0 + 18.0 * lines as f32, |mut row| {
                    row.col(|ui| station_cell(ui, r, q));
                    row.col(|ui| {
                        ui.label(RichText::new(&r.distance).color(pal::TEXT));
                    });
                    row.col(|ui| {
                        let (age, col) = data_age(r.offer.updated);
                        ui.label(RichText::new(age).color(col)).on_hover_text("When Spansh last had this station's outfitting list (the shipyard too, for a hull)");
                    });
                    row.col(|ui| price_cell(ui, r, q));
                    row.col(|ui| {
                        if widgets::icon_button(ui, icon::NAVIGATION_ARROW, "Plot a route there on the Navigation page").clicked() {
                            action = Some(FleetAction::PlotTo(r.offer.system.clone()));
                        }
                        if r.offer.market_id > 0 && widgets::icon_button(ui, icon::ARROW_SQUARE_OUT, "Open on Spansh: everything this station sells").clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(nexus_spansh::station_url(r.offer.market_id)));
                        }
                    });
                });
            }
        });
    action
}

/// What was left out of the stations Spansh matched, and why.
fn stats_line(ui: &mut Ui, out: &Outcome, q: &Search) {
    let s = &out.stats;
    let mut parts = Vec::new();
    if s.not_exact > 0 {
        parts.push(format!("{} sold a different mount or another hull's bulkheads", s.not_exact));
    }
    if s.closed > 0 {
        parts.push(format!("{} carriers let only friends or their squadron dock", s.closed));
    }
    if s.no_pad > 0 {
        parts.push(format!("{} have no {} pad", s.no_pad, pad_word(q.pad)));
    }
    let matched = if s.matched >= 10_000 { "over 10,000".to_string() } else { nexus_data::names::group_digits(s.matched as i64) };
    let mut line = format!("Spansh matched {matched} stations by module name; the nearest {} were checked one by one", s.read);
    if !parts.is_empty() {
        line.push_str(&format!(": {}", parts.join(", ")));
    }
    line.push('.');
    footnote(ui, &line);
}

/// The station's name over its system (click to copy), its state, and what to buy there.
fn station_cell(ui: &mut Ui, r: &Row, q: &Search) {
    let o = r.offer;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        let (mark, tip) = if o.carrier {
            (format!("{} ", icon::ANCHOR), "Fleet carrier: its owner stocks it, and it moves")
        } else if o.planetary {
            (format!("{} ", icon::PLANET), "Planetary port or settlement")
        } else {
            (String::new(), "")
        };
        let name = ui.add(Label::new(RichText::new(format!("{mark}{}", o.station)).color(pal::TEXT)).truncate());
        let kind = o.station_type.as_deref().unwrap_or("Station");
        name.on_hover_text(if tip.is_empty() { kind.to_string() } else { format!("{kind}. {tip}") });
        let sys = match o.arrival_ls {
            Some(ls) => format!("{} · {ls:.0} ls", o.system),
            None => o.system.clone(),
        };
        if ui.add(Label::new(RichText::new(sys).size(size::CAPTION).color(pal::DIM)).truncate().sense(Sense::click())).on_hover_text("Click to copy the system").clicked() {
            ui.ctx().copy_text(o.system.clone());
        }
        if let Some(state) = &o.state {
            badge(ui, state, pal::WARN).on_hover_text("Spansh last saw the station in this state: its services may be down");
        }
        if r.items.len() < q.wanted.len() {
            let names: Vec<&str> = r.items.iter().filter_map(|&i| q.wanted.get(i)).map(|w| w.name.as_str()).collect();
            ui.add(Label::new(RichText::new(format!("Buy {} here", list_or_count(&names))).size(size::CAPTION).color(pal::ACCENT_SOFT)).truncate()).on_hover_text(names.join("\n"));
        }
    });
}

/// What the row's items cost there, and a tooltip for what has no price.
fn price_cell(ui: &mut Ui, r: &Row, q: &Search) {
    let (total, unpriced) = r.offer.cost(&r.items, &q.wanted);
    let price = if total > 0 { short_credits(total as i64) } else { "—".into() };
    let label = ui.label(RichText::new(price).color(pal::GOLD));
    if unpriced > 0 {
        let hull = r.items.iter().any(|&i| q.wanted.get(i).is_some_and(|w| w.hull) && r.offer.sells.iter().any(|(j, p)| *j == i && p.is_none()));
        let why = if hull { " A hull without a credit price may be sold for ARX only (early access)." } else { "" };
        label.on_hover_text(format!("{unpriced} of these have no price on Spansh, so the total leaves them out.{why}"));
    }
}

/// "the Anaconda and 4A Gimballed Multi-cannon", "these 22".
fn list_or_count(names: &[&str]) -> String {
    match names {
        [] => "nothing".into(),
        [a] => (*a).to_string(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        _ => format!("these {}", names.len()),
    }
}

/// "today", "3 d ago", "5 wk ago", "7 mo ago": green within a week, warning past a month.
fn data_age(updated: Option<i64>) -> (String, egui::Color32) {
    let Some(t) = updated else { return ("unknown".into(), pal::FAINT) };
    let days = (nexus_journal::Timestamp::now().0 - t).max(0) / 86_400;
    let text = match days {
        0 => "today".to_string(),
        1..=13 => format!("{days} d ago"),
        14..=59 => format!("{} wk ago", days / 7),
        60..=729 => format!("{} mo ago", days / 30),
        _ => format!("{} y ago", days / 365),
    };
    let col = if days <= 7 { pal::GOOD } else if days <= 30 { pal::DIM } else { pal::WARN };
    (text, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_words() {
        assert_eq!(list_or_count(&["Anaconda", "4A Gimballed Multi-cannon"]), "Anaconda and 4A Gimballed Multi-cannon");
        assert_eq!(list_or_count(&["a", "b", "c", "d"]), "these 4");
        assert_eq!(a_or_an("Anaconda"), "An");
        assert_eq!(a_or_an("Python"), "A");
        assert_eq!(data_age(None).0, "unknown");
        assert_eq!(data_age(Some(nexus_journal::Timestamp::now().0 - 3 * 86_400)).0, "3 d ago");
        assert_eq!(data_age(Some(nexus_journal::Timestamp::now().0 - 200 * 86_400)).0, "6 mo ago");
    }
}
