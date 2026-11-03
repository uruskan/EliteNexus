//! Engineering: materials, blueprint planner with shopping list & trader plan, engineers,
//! Odyssey upgrades, synthesis.

use crate::settings::WishItem;
use crate::theme::{display, pal};
use crate::widgets::{badge, bar, card, empty};
use egui::{Grid, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use nexus_data::engineering::{self as eng, Blueprint, EngineerKind, Ingredient, OdysseyKind};
use nexus_data::materials::MaterialKind;
use nexus_data::trader::{plan_shopping_with, ShoppingOptions};
use nexus_model::inventory::{Locker, MaterialCategory};
use nexus_model::GameState;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Planner,
    Materials,
    Engineers,
    Odyssey,
    Synthesis,
}

pub struct EngineeringUi {
    pub tab: Tab,
    pub filter: String,
    pub module: String,
    pub blueprint: Option<String>,
    pub from: u8,
    pub to: u8,
    pub rolls: u32,
    pub experimental: Option<String>,
    pub cross_category: bool,
}

impl EngineeringUi {
    /// `--tab` for screenshot runs: materials, engineers, suits, synthesis, or merc (the planner on a
    /// Merc blueprint).
    pub fn new(tab: Option<&str>) -> Self {
        let tab = match tab {
            Some("materials") => Tab::Materials,
            Some("engineers") => Tab::Engineers,
            Some("suits") => Tab::Odyssey,
            Some("synthesis") => Tab::Synthesis,
            Some("merc") => {
                let blueprint = Some("cargoracks5c1_extended".into());
                return EngineeringUi { module: "Cargo Rack".into(), blueprint, ..Default::default() };
            }
            _ => Tab::Planner,
        };
        EngineeringUi { tab, ..Default::default() }
    }
}

impl Default for EngineeringUi {
    fn default() -> Self {
        EngineeringUi {
            tab: Tab::Planner,
            filter: String::new(),
            module: "FrameShiftDrive".into(),
            blueprint: None,
            from: 1,
            to: 5,
            rolls: 3,
            experimental: None,
            cross_category: true,
        }
    }
}

/// Everything the commander holds that engineering can consume: materials, micro-resources, cargo.
fn inventory_map(gs: &GameState) -> HashMap<String, i64> {
    let mut inv: HashMap<String, i64> = HashMap::new();
    let m = &gs.inventory.materials;
    for cat in [MaterialCategory::Raw, MaterialCategory::Manufactured, MaterialCategory::Encoded] {
        for (k, v) in m.map(cat) {
            *inv.entry(k.clone()).or_default() += *v;
        }
    }
    let l = &gs.inventory.ship_locker;
    for map in [&l.items, &l.components, &l.data, &l.consumables] {
        for (k, v) in map {
            *inv.entry(k.clone()).or_default() += v.count;
        }
    }
    for c in &gs.inventory.cargo.items {
        *inv.entry(c.symbol.clone()).or_default() += c.count;
    }
    inv
}

/// Returns true when the wishlist changed (so settings get saved).
pub fn show(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi, wishlist: &mut Vec<WishItem>) -> bool {
    let mut changed = false;
    if let Some(t) = crate::widgets::tabs(ui, st.tab, &[(Tab::Planner, "Blueprint planner"), (Tab::Materials, "Materials"), (Tab::Engineers, "Engineers"), (Tab::Odyssey, "Suits & weapons"), (Tab::Synthesis, "Synthesis")]) {
        st.tab = t;
    }
    ui.add_space(crate::widgets::GAP);
    match st.tab {
        Tab::Planner => changed |= planner(ui, gs, st, wishlist),
        Tab::Materials => materials(ui, gs, st),
        Tab::Engineers => engineers(ui, gs),
        Tab::Odyssey => odyssey(ui, gs),
        Tab::Synthesis => synthesis(ui, gs),
    }
    changed
}

fn ingredient_line(ui: &mut Ui, ing: &Ingredient, have: i64) {
    let ok = have >= i64::from(ing.count);
    ui.label(RichText::new(format!("{}× {}", ing.count, ing.name())).color(if ok { pal::TEXT } else { pal::WARN }));
    ui.label(RichText::new(format!("({have})")).small().color(if ok { pal::GOOD } else { pal::BAD }));
}

fn planner(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi, wishlist: &mut Vec<WishItem>) -> bool {
    let mut changed = false;
    let inv = inventory_map(gs);
    crate::widgets::cards(ui, 2, 380.0, |c| {
        // ---------------- left: browse blueprints
        c.add(|ui| {
        card(ui, "Blueprints", |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("modtype").width(220.0).selected_text(nexus_data::names::split_camel(&st.module)).show_ui(ui, |ui| {
                    for m in eng::module_types() {
                        if ui.selectable_label(st.module == *m, nexus_data::names::split_camel(m)).clicked() {
                            st.module = (*m).to_string();
                            st.blueprint = None;
                            st.experimental = None;
                        }
                    }
                });
            });
            let bps = eng::blueprints_for_module(&st.module);
            ui.horizontal_wrapped(|ui| {
                for bp in &bps {
                    let label = if bp.is_merc() { format!("{} {}", icon::COINS, bp.name) } else { bp.name.clone() };
                    if crate::widgets::pill(ui, st.blueprint.as_deref() == Some(bp.symbol.as_str()), &label).clicked() {
                        st.blueprint = Some(bp.symbol.clone());
                        (st.from, st.to) = (bp.min_grade(), bp.max_grade());
                    }
                }
            });
            let Some(bp) = st.blueprint.as_deref().and_then(|s| bps.iter().find(|b| b.symbol == s).copied()) else {
                ui.label(RichText::new("Pick a blueprint.").color(pal::DIM));
                return;
            };
            blueprint_detail(ui, gs, bp, &inv);
            ui.add_space(6.0);
            // Merc blueprints on MercGear start at G2, and some blueprints stop below G5.
            let grades = bp.min_grade()..=bp.max_grade();
            st.from = st.from.clamp(*grades.start(), *grades.end());
            st.to = st.to.clamp(*grades.start(), *grades.end());
            ui.horizontal(|ui| {
                ui.label("Grades");
                ui.add(egui::DragValue::new(&mut st.from).range(grades.clone()));
                ui.label("to");
                ui.add(egui::DragValue::new(&mut st.to).range(grades));
                ui.label("rolls/grade");
                ui.add(egui::DragValue::new(&mut st.rolls).range(1..=10));
            });
            let exps = eng::experimentals_for(&st.module);
            if !exps.is_empty() {
                egui::ComboBox::from_id_salt("exp").width(260.0).selected_text(st.experimental.as_deref().and_then(eng::experimental).map(|e| e.name.clone()).unwrap_or_else(|| "No experimental".into())).show_ui(ui, |ui| {
                    ui.selectable_value(&mut st.experimental, None, "No experimental");
                    for e in exps {
                        ui.selectable_value(&mut st.experimental, Some(e.symbol.clone()), &e.name);
                    }
                });
            }
            if crate::widgets::primary_button(ui, &format!("{} Add to wishlist", icon::PLUS)).clicked() {
                wishlist.push(WishItem {
                    label: format!("{} — {}", nexus_data::names::split_camel(&st.module), bp.name),
                    blueprint: bp.symbol.clone(),
                    module: st.module.clone(),
                    from: st.from.min(st.to),
                    to: st.to.max(st.from),
                    rolls: st.rolls,
                    experimental: st.experimental.clone(),
                });
                changed = true;
            }
        });
        ui.add_space(8.0);
        card(ui, "Quick add from your ship", |ui| {
            let Some(ship) = gs.fleet.current() else {
                ui.label(RichText::new("No ship loadout yet.").color(pal::DIM));
                return;
            };
            let mut any = false;
            for m in &ship.modules {
                let Some(e) = &m.engineering else { continue };
                let Some(bp) = eng::blueprint_for(&m.item, &e.blueprint).or_else(|| eng::blueprint(&e.blueprint)) else { continue };
                let top = bp.max_grade();
                if e.level >= u32::from(top) {
                    continue;
                }
                any = true;
                ui.horizontal(|ui| {
                    ui.label(format!("{} — {} G{} → G{top}", nexus_data::outfitting::display_name(&m.item), bp.name, e.level));
                    if ui.small_button(icon::PLUS).clicked() {
                        wishlist.push(WishItem {
                            label: format!("{} — {}", nexus_data::outfitting::display_name(&m.item), bp.name),
                            blueprint: bp.symbol.clone(),
                            module: eng::module_type(&m.item).unwrap_or("").to_string(),
                            from: (e.level + 1).min(u32::from(top)) as u8,
                            to: top,
                            rolls: 3,
                            experimental: None,
                        });
                        changed = true;
                    }
                });
            }
            if !any {
                ui.label(RichText::new("Every engineered module on this ship is already at its top grade.").color(pal::DIM));
            }
        });

        // ---------------- right: wishlist + shopping
        });
        c.add(|ui| {
        card(ui, &format!("Wishlist ({})", wishlist.len()), |ui| {
            if wishlist.is_empty() {
                ui.label(RichText::new("Add blueprints on the left — the shopping list below adds everything up.").color(pal::DIM));
            }
            let mut remove = None;
            for (i, w) in wishlist.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.small_button(icon::TRASH).clicked() {
                        remove = Some(i);
                    }
                    ui.label(RichText::new(&w.label).color(pal::TEXT));
                    badge(ui, &format!("G{}–G{} ×{}", w.from, w.to, w.rolls), pal::PURPLE);
                    if let Some(x) = w.experimental.as_deref().and_then(eng::experimental) {
                        badge(ui, &x.name, pal::ACCENT);
                    }
                });
            }
            if let Some(i) = remove {
                wishlist.remove(i);
                changed = true;
            }
        });
        ui.add_space(8.0);
        // Total requirements.
        let mut need: Vec<(String, u32)> = Vec::new();
        let mut coins = 0u32;
        for w in wishlist.iter() {
            if let Some(bp) = eng::blueprint_for(&w.module, &w.blueprint).or_else(|| eng::blueprint(&w.blueprint)) {
                need.extend(nexus_data::trader::blueprint_shopping(bp, w.from, w.to, w.rolls));
                coins = coins.saturating_add(nexus_data::trader::blueprint_merc_coins(bp, w.from, w.to, w.rolls));
            }
            if let Some(x) = w.experimental.as_deref().and_then(eng::experimental) {
                need.extend(x.cost.iter().map(|i| (i.symbol.clone(), i.count)));
            }
        }
        let mut merged: Vec<(String, u32)> = Vec::new();
        for (s, n) in need {
            match merged.iter_mut().find(|(k, _)| *k == s) {
                Some((_, c)) => *c += n,
                None => merged.push((s, n)),
            }
        }
        card(ui, "Shopping list", |ui| {
            if merged.is_empty() {
                ui.label(RichText::new("Nothing needed yet.").color(pal::DIM));
                return;
            }
            let total: u32 = merged.iter().map(|(_, n)| *n).sum();
            let have_total: u32 = merged.iter().map(|(s, n)| (*inv.get(s).unwrap_or(&0)).clamp(0, i64::from(*n)) as u32).sum();
            bar(ui, have_total as f32 / total.max(1) as f32, pal::GOOD, 10.0, Some(&format!("{have_total} / {total} materials in hand")));
            if coins > 0 {
                ui.horizontal_wrapped(|ui| match merc_coin_balance(gs) {
                    Some(have) => {
                        let col = if have >= u64::from(coins) { pal::GOOD } else { pal::WARN };
                        ui.label(RichText::new(format!("{} {have} / {coins} Merc Coins", icon::COINS)).color(col));
                        ui.label(RichText::new("(balance at your last login)").small().color(pal::DIM));
                    }
                    None => {
                        ui.label(RichText::new(format!("{} {coins} Merc Coins", icon::COINS)).color(pal::TEXT));
                    }
                });
            }
            Grid::new("shopping").num_columns(4).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
                for (sym, n) in &merged {
                    let have = *inv.get(sym).unwrap_or(&0);
                    let mat = nexus_data::materials::material(sym);
                    let name = mat.map(|m| m.name.clone()).unwrap_or_else(|| nexus_data::names::split_camel(sym));
                    ui.label(RichText::new(name).color(pal::TEXT));
                    ui.label(RichText::new(mat.map(|m| format!("G{} {:?}", m.grade, m.kind)).unwrap_or_default()).small().color(pal::DIM));
                    ui.label(RichText::new(format!("{have} / {n}")).color(if have >= i64::from(*n) { pal::GOOD } else { pal::WARN }));
                    let missing = i64::from(*n) - have;
                    ui.label(RichText::new(if missing > 0 { format!("need {missing}") } else { "✓".into() }).color(if missing > 0 { pal::BAD } else { pal::GOOD }));
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            crate::widgets::toggle(ui, &mut st.cross_category, "Allow cross-category trades (costs 6× more)");
            let plan = plan_shopping_with(&merged, &inv, &ShoppingOptions { cross_category: st.cross_category, ..Default::default() });
            if !plan.trades.is_empty() {
                ui.label(RichText::new("MATERIAL TRADER PLAN").small().color(pal::ACCENT));
                for t in &plan.trades {
                    let trader = match t.trader {
                        MaterialKind::Raw => "raw",
                        MaterialKind::Manufactured => "manufactured",
                        MaterialKind::Encoded => "encoded",
                    };
                    let give = nexus_data::materials::material(&t.give).map(|m| m.name.clone()).unwrap_or_else(|| t.give.clone());
                    let recv = nexus_data::materials::material(&t.receive).map(|m| m.name.clone()).unwrap_or_else(|| t.receive.clone());
                    ui.label(format!("{} {}× {} → {}× {}  ({trader} trader)", icon::ARROWS_LEFT_RIGHT, t.give_qty, give, t.receive_qty, recv));
                }
            }
            if !plan.unresolved.is_empty() {
                ui.label(RichText::new("STILL TO COLLECT").small().color(pal::WARN));
                for u in &plan.unresolved {
                    let name = nexus_data::materials::material(&u.symbol).map(|m| m.name.clone()).unwrap_or_else(|| u.symbol.clone());
                    ui.label(RichText::new(format!("{}× {}", u.qty, name)).color(pal::WARN));
                }
            }
        });
            });
    });
    changed
}

/// Merc Coins at the last login: the journal's Statistics event is the only place the game writes them.
fn merc_coin_balance(gs: &GameState) -> Option<u64> {
    gs.cmdr.statistics.as_ref()?.bank_account.get("MercCoins_Current")?.as_u64()
}

fn blueprint_detail(ui: &mut Ui, gs: &GameState, bp: &Blueprint, inv: &HashMap<String, i64>) {
    ui.label(RichText::new(&bp.name).font(display(18.0)));
    for (grade, ingredients) in (1u8..).zip(&bp.grades).filter(|(_, g)| !g.is_empty()) {
        ui.horizontal_wrapped(|ui| {
            badge(ui, &format!("G{grade}"), pal::PURPLE);
            for ing in ingredients {
                ingredient_line(ui, ing, *inv.get(&ing.symbol.to_ascii_lowercase()).unwrap_or(&0));
            }
            let coins = bp.merc_coins_for(grade);
            if coins > 0 {
                ui.label(RichText::new(format!("{} {coins} Merc Coins", icon::COINS)).color(pal::TEXT));
            }
        });
    }
    if bp.is_merc() {
        let text = match &bp.merc_module {
            Some(m) => format!(
                "Merc blueprint, only for the {m} bought with Merc Coins in outfitting (it comes at G1). Every roll costs Merc Coins as well as materials."
            ),
            None => "Merc blueprint: every roll costs Merc Coins as well as materials.".into(),
        };
        crate::widgets::footnote(ui, &text);
    }
    if let Some(effects) = bp.effects.last() {
        ui.horizontal_wrapped(|ui| {
            for e in effects {
                ui.label(RichText::new(format!("{} {}", e.property, e.value)).small().color(if e.good { pal::GOOD } else { pal::BAD }));
            }
        });
    }
    ui.add_space(4.0);
    ui.label(RichText::new("ENGINEERS").small().color(pal::ACCENT));
    let here = gs.location.star_pos;
    let mut list: Vec<_> = bp.engineers.iter().filter_map(|eg| eng::engineer_by_name(&eg.engineer).map(|e| (e, eg.max_grade))).collect();
    list.sort_by(|a, b| {
        let da = here.map(|p| nexus_model::location::distance(p, a.0.pos)).unwrap_or(0.0);
        let db = here.map(|p| nexus_model::location::distance(p, b.0.pos)).unwrap_or(0.0);
        da.total_cmp(&db)
    });
    for (e, max) in list {
        let status = gs.cmdr.engineers.get(&e.id);
        let unlocked = status.is_some_and(|s| s.progress == "Unlocked");
        ui.horizontal(|ui| {
            ui.label(RichText::new(&e.name).color(if unlocked { pal::GOOD } else { pal::TEXT }));
            badge(ui, &format!("max G{max}"), pal::PURPLE);
            if let Some(s) = status {
                badge(ui, &s.progress, if unlocked { pal::GOOD } else { pal::DIM });
            }
            let dist = here.map(|p| format!("{:.0} ly", nexus_model::location::distance(p, e.pos))).unwrap_or_default();
            ui.label(RichText::new(format!("{} · {}", e.system, dist)).small().color(pal::DIM));
        });
    }
}

fn materials(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi) {
    ui.horizontal(|ui| {
        crate::widgets::search_field(ui, &mut st.filter, "Filter materials", 280.0);
    });
    ui.add_space(6.0);
    let filter = st.filter.to_ascii_lowercase();
    let m = &gs.inventory.materials;
    crate::widgets::cards(ui, 3, 280.0, |c| {
        for (i, cat) in [MaterialCategory::Raw, MaterialCategory::Manufactured, MaterialCategory::Encoded].into_iter().enumerate() {
            c.add(|ui| {
                card(ui, &format!("{} ({})", cat.label(), m.total(cat)), |ui| {
                // Show every known material of this kind, including ones you have none of.
                let kind = match cat {
                    MaterialCategory::Raw => MaterialKind::Raw,
                    MaterialCategory::Manufactured => MaterialKind::Manufactured,
                    MaterialCategory::Encoded => MaterialKind::Encoded,
                };
                let mut items: Vec<(String, u8, i64)> = nexus_data::materials::all()
                    .iter()
                    .filter(|mat| mat.kind == kind)
                    .map(|mat| (mat.name.clone(), mat.grade, *m.map(cat).get(&mat.symbol).unwrap_or(&0)))
                    .collect();
                if items.is_empty() {
                    items = m.map(cat).iter().map(|(k, v)| (m.display_name(k), 0, *v)).collect();
                }
                items.retain(|(n, _, _)| filter.is_empty() || n.to_ascii_lowercase().contains(&filter));
                items.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
                Grid::new(("mats", i)).num_columns(3).striped(true).spacing([10.0, 2.0]).show(ui, |ui| {
                    for (name, grade, n) in items {
                        let cap = nexus_data::materials::cap(grade) as i64;
                        ui.label(RichText::new(name).color(if n == 0 { pal::FAINT } else { pal::TEXT }));
                        ui.label(RichText::new(if grade > 0 { format!("G{grade}") } else { String::new() }).small().color(pal::PURPLE));
                        ui.scope(|ui| {
                            ui.set_width(110.0);
                            let full = cap > 0 && n >= cap;
                            bar(ui, if cap > 0 { n as f32 / cap as f32 } else { 0.0 }, if full { pal::WARN } else { pal::ACCENT }, 12.0, Some(&if cap > 0 { format!("{n}/{cap}") } else { n.to_string() }));
                        });
                        ui.end_row();
                    }
                });
            });
            });
        }
    });
}

fn engineers(ui: &mut Ui, gs: &GameState) {
    let here = gs.location.star_pos;
    let mut list: Vec<_> = eng::engineers().iter().collect();
    list.sort_by(|a, b| {
        let ua = gs.cmdr.engineers.get(&a.id).is_some_and(|s| s.progress == "Unlocked");
        let ub = gs.cmdr.engineers.get(&b.id).is_some_and(|s| s.progress == "Unlocked");
        ub.cmp(&ua).then_with(|| {
            let da = here.map(|p| nexus_model::location::distance(p, a.pos)).unwrap_or(0.0);
            let db = here.map(|p| nexus_model::location::distance(p, b.pos)).unwrap_or(0.0);
            da.total_cmp(&db)
        })
    });
    TableBuilder::new(ui)
        .striped(true)
        .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(190.0))
        .column(Column::initial(80.0))
        .column(Column::initial(200.0))
        .column(Column::initial(80.0))
        .column(Column::initial(120.0))
        .column(Column::remainder())
        .header(30.0, |mut h| {
            for t in ["Engineer", "Type", "Location", "Distance", "Your status", "Unlock / specialities"] {
                h.col(|ui| {
                    crate::widgets::th(ui, t);
                });
            }
        })
        .body(|body| {
            body.rows(40.0, list.len(), |mut row| {
                let e = list[row.index()];
                let status = gs.cmdr.engineers.get(&e.id);
                row.col(|ui| {
                    ui.label(RichText::new(&e.name).color(pal::TEXT));
                });
                row.col(|ui| {
                    ui.label(RichText::new(match e.kind { EngineerKind::Ship => "Ship", EngineerKind::OnFoot => "On foot" }).color(pal::DIM));
                });
                row.col(|ui| {
                    ui.vertical(|ui| {
                        if ui.link(&e.system).on_hover_text("Click to copy").clicked() {
                            ui.ctx().copy_text(e.system.clone());
                        }
                        ui.label(RichText::new(&e.base).small().color(pal::DIM));
                    });
                });
                row.col(|ui| {
                    ui.label(RichText::new(here.map(|p| format!("{:.0} ly", nexus_model::location::distance(p, e.pos))).unwrap_or_default()).color(pal::DIM));
                });
                row.col(|ui| match status {
                    Some(s) => {
                        let col = if s.progress == "Unlocked" { pal::GOOD } else { pal::WARN };
                        ui.label(RichText::new(format!("{}{}", s.progress, s.rank.map(|r| format!(" G{r}")).unwrap_or_default())).color(col));
                    }
                    None => {
                        ui.label(RichText::new("unknown").color(pal::FAINT));
                    }
                });
                row.col(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&e.unlock).small().color(pal::TEXT));
                        let spec: Vec<String> = e.modules.iter().take(6).map(|m| format!("{} G{}", nexus_data::names::split_camel(&m.module), m.max_grade)).collect();
                        let spec = if spec.is_empty() { e.mods.join(", ") } else { spec.join(" · ") };
                        ui.label(RichText::new(spec).small().color(pal::DIM));
                    });
                });
            });
        });
}

fn locker_count(l: &Locker, sym: &str) -> i64 {
    l.count(sym)
}

fn odyssey(ui: &mut Ui, gs: &GameState) {
    let locker = &gs.inventory.ship_locker;
    if eng::odyssey_items().is_empty() {
        empty(ui, icon::BACKPACK, "No Odyssey data", "");
        return;
    }
    crate::widgets::cards(ui, 2, 360.0, |c| {
        for (kind, title) in [(OdysseyKind::Suit, "Suits"), (OdysseyKind::Weapon, "Weapons")] {
            c.add(|ui| {
                card(ui, title, |ui| {
                for item in eng::odyssey_items().iter().filter(|i| i.kind == kind) {
                    egui::CollapsingHeader::new(RichText::new(item.name).color(pal::TEXT)).id_salt(item.symbol).show(ui, |ui| {
                        for g in 2..=5u8 {
                            if let Some(up) = eng::odyssey_upgrade(item.symbol, g) {
                                ui.horizontal_wrapped(|ui| {
                                    badge(ui, &format!("G{g}"), pal::PURPLE);
                                    for ing in &up.cost {
                                        ingredient_line(ui, ing, locker_count(locker, &ing.symbol));
                                    }
                                });
                            }
                        }
                        let mods = eng::odyssey_mods_for(item.symbol);
                        if !mods.is_empty() {
                            ui.label(RichText::new("MODIFICATIONS").small().color(pal::ACCENT));
                            for m in mods {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new(&m.name).color(pal::CYAN));
                                    for ing in &m.cost {
                                        ingredient_line(ui, ing, locker_count(locker, &ing.symbol));
                                    }
                                });
                            }
                        }
                    });
                }
            });
            });
        }
    });
}

fn synthesis(ui: &mut Ui, gs: &GameState) {
    let inv = inventory_map(gs);
    let mut families: Vec<&str> = eng::synthesis_recipes().iter().map(|r| r.family.as_str()).collect();
    families.dedup();
    for fam in families {
        card(ui, fam, |ui| {
            for r in eng::synthesis_recipes().iter().filter(|r| r.family == fam) {
                let can = r.cost.iter().all(|i| *inv.get(&i.symbol.to_ascii_lowercase()).unwrap_or(&0) >= i64::from(i.count));
                ui.horizontal_wrapped(|ui| {
                    badge(ui, &r.name, if can { pal::GOOD } else { pal::DIM });
                    // Basic grades have no effect text, and an empty label in a wrapping row sends the rest of the
                    // row back over the badge.
                    if !r.effect.is_empty() {
                        ui.label(RichText::new(&r.effect).small().color(pal::DIM));
                    }
                    for ing in &r.cost {
                        ingredient_line(ui, ing, *inv.get(&ing.symbol.to_ascii_lowercase()).unwrap_or(&0));
                    }
                });
            }
        });
        ui.add_space(6.0);
    }
}
