//! Engineering: materials, blueprint planner with shopping list & trader plan, engineers,
//! Odyssey upgrades, synthesis.

use crate::settings::WishItem;
use crate::theme::{display, pal, size};
use crate::widgets::{badge, bar, card, empty};
use egui::{Align, Color32, Grid, Layout, RichText, Ui};
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
    /// Disclosures: the blueprint's options, what you already have, the recipes, the other
    /// engineers; the shopping list's "already have" and options.
    pub show_options: bool,
    pub show_have: bool,
    pub show_recipes: bool,
    pub show_engineers: bool,
    pub show_list_have: bool,
    pub show_list_options: bool,
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
            show_options: false,
            show_have: false,
            show_recipes: false,
            show_engineers: false,
            show_list_have: false,
            show_list_options: false,
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

/// Grade labels for segmented controls.
const GRADE_LABELS: [&str; 5] = ["G1", "G2", "G3", "G4", "G5"];

fn grade_options(grades: std::ops::RangeInclusive<u8>) -> Vec<(u8, &'static str)> {
    grades.filter_map(|g| GRADE_LABELS.get(usize::from(g).checked_sub(1)?).map(|l| (g, *l))).collect()
}

fn ingredient_line(ui: &mut Ui, ing: &Ingredient, have: i64) {
    let ok = have >= i64::from(ing.count);
    ui.label(RichText::new(format!("{}× {}", ing.count, ing.name())).color(if ok { pal::TEXT } else { pal::WARN }));
    ui.label(RichText::new(format!("({have})")).small().color(if ok { pal::GOOD } else { pal::BAD }));
}

/// English name of a material, micro-resource or commodity symbol.
fn item_name(symbol: &str) -> String {
    Ingredient { symbol: symbol.to_string(), count: 0 }.name()
}

fn rolls_text(n: u32) -> String {
    if n == 1 { "1 roll".into() } else { format!("{n} rolls") }
}

/// Adds `(symbol, count)` pairs to `into`, summing duplicates.
fn merge_into(into: &mut Vec<(String, u32)>, items: impl IntoIterator<Item = (String, u32)>) {
    for (s, n) in items {
        match into.iter_mut().find(|(k, _)| *k == s) {
            Some((_, c)) => *c = c.saturating_add(n),
            None => into.push((s, n)),
        }
    }
}

fn planner(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi, wishlist: &mut Vec<WishItem>) -> bool {
    let mut changed = false;
    let inv = inventory_map(gs);
    crate::widgets::cards(ui, 2, 380.0, |c| {
        c.add(|ui| {
            card(ui, "Blueprints", |ui| changed |= blueprint_card(ui, gs, st, &inv, wishlist));
            ui.add_space(8.0);
            card(ui, "From your ship", |ui| changed |= from_your_ship(ui, gs, wishlist));
        });
        // One card for the plan, so what the items need sits right under them.
        c.add(|ui| {
            card(ui, &format!("Wishlist ({})", wishlist.len()), |ui| {
                changed |= wishlist_rows(ui, wishlist);
                if !wishlist.is_empty() {
                    ui.add_space(6.0);
                    crate::widgets::section(ui, "Shopping list");
                    shopping_list(ui, gs, st, &inv, wishlist);
                }
            });
        });
    });
    changed
}

/// Pick a module and blueprint, choose the grade to reach, and see what you're short of. The recipes
/// and the engineers sit one click deeper. Returns true when the wishlist changed.
fn blueprint_card(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi, inv: &HashMap<String, i64>, wishlist: &mut Vec<WishItem>) -> bool {
    egui::ComboBox::from_id_salt("modtype").width(240.0).selected_text(nexus_data::names::split_camel(&st.module)).show_ui(ui, |ui| {
        for m in eng::module_types() {
            if ui.selectable_label(st.module == *m, nexus_data::names::split_camel(m)).clicked() {
                st.module = (*m).to_string();
                st.blueprint = None;
                st.experimental = None;
            }
        }
    });
    ui.add_space(6.0);
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
        crate::widgets::footnote(ui, "Pick a blueprint to see what it needs and who can do it.");
        return false;
    };
    // MercGear blueprints start at G2, and a few blueprints stop below G5.
    let (lo, hi) = (bp.min_grade(), bp.max_grade());
    st.to = st.to.clamp(lo, hi);
    st.from = st.from.clamp(lo, st.to);

    // What it is.
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&bp.name).font(display(20.0)).color(pal::TEXT));
        if bp.is_merc() {
            badge(ui, &format!("{} Merc", icon::COINS), pal::GOLD);
        }
    });
    match &bp.merc_module {
        Some(m) => {
            ui.label(RichText::new(format!("For the {m} you buy with Merc Coins. It comes at G1.")).color(pal::DIM));
        }
        None if bp.is_merc() => {
            ui.label(RichText::new("Each roll costs Merc Coins as well as materials.").color(pal::DIM));
        }
        None => {}
    }
    if let Some(effects) = bp.effects.get(usize::from(st.to) - 1).filter(|e| !e.is_empty()) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("At G{}:", st.to)).size(size::FOOTNOTE).color(pal::DIM));
            for e in effects {
                ui.label(RichText::new(format!("{} {}", e.property, e.value)).size(size::FOOTNOTE).color(if e.good { pal::GOOD } else { pal::BAD }));
            }
        });
    }

    // The grade to reach, and how many rolls to plan for.
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 28.0;
        ui.vertical(|ui| {
            ui.label(RichText::new("Upgrade to").size(size::FOOTNOTE).color(pal::DIM));
            if crate::widgets::segmented(ui, &mut st.to, &grade_options(lo..=hi)) {
                st.from = st.from.min(st.to);
            }
        });
        ui.vertical(|ui| {
            ui.label(RichText::new(format!("Rolls per grade {}", icon::INFO)).size(size::FOOTNOTE).color(pal::DIM)).on_hover_text(
                "A roll is one craft at the engineer: it uses one set of materials and fills part of the grade. Most grades take 2 to 4 rolls.",
            );
            crate::widgets::stepper(ui, &mut st.rolls, 1..=10);
        });
    });
    let exps = eng::experimentals_for(&st.module);
    if !exps.is_empty() {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Experimental effect").color(pal::TEXT));
            let selected = st.experimental.as_deref().and_then(eng::experimental).map(|e| e.name.clone()).unwrap_or_else(|| "None".into());
            egui::ComboBox::from_id_salt("exp").width(220.0).selected_text(selected).show_ui(ui, |ui| {
                ui.selectable_value(&mut st.experimental, None, "None");
                for e in exps {
                    ui.selectable_value(&mut st.experimental, Some(e.symbol.clone()), &e.name);
                }
            });
        });
    }
    if lo < st.to {
        ui.add_space(4.0);
        if crate::widgets::disclosure(ui, &mut st.show_options, "Options") {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Start at").color(pal::TEXT));
                crate::widgets::segmented(ui, &mut st.from, &grade_options(lo..=st.to));
            });
            crate::widgets::footnote(ui, "For a module that already has some grades of this blueprint.");
        }
    }

    // What it needs, against what you have.
    let mut need = nexus_data::trader::blueprint_shopping(bp, st.from, st.to, st.rolls);
    if let Some(x) = st.experimental.as_deref().and_then(eng::experimental) {
        merge_into(&mut need, x.cost.iter().map(|i| (i.symbol.clone(), i.count)));
    }
    let coins = nexus_data::trader::blueprint_merc_coins(bp, st.from, st.to, st.rolls);
    ui.add_space(14.0);
    needs(ui, gs, inv, &need, coins, &mut st.show_have);

    ui.add_space(12.0);
    let mut changed = false;
    if crate::widgets::primary_button(ui, &format!("{} Add to wishlist", icon::PLUS)).clicked() {
        wishlist.push(WishItem {
            label: format!("{} — {}", nexus_data::names::split_camel(&st.module), bp.name),
            blueprint: bp.symbol.clone(),
            module: st.module.clone(),
            from: st.from,
            to: st.to,
            rolls: st.rolls,
            experimental: st.experimental.clone(),
        });
        changed = true;
    }

    // One click deeper: the recipes, and who can do it.
    ui.add_space(10.0);
    if crate::widgets::disclosure(ui, &mut st.show_recipes, "Recipe for each grade") {
        recipes(ui, bp);
    }
    where_to_engineer(ui, gs, bp, st.to, &mut st.show_engineers);
    changed
}

/// What a plan needs against what you have: a status line and a bar, a row for each thing you're
/// short of, and one row that opens what you already have enough of.
fn needs(ui: &mut Ui, gs: &GameState, inv: &HashMap<String, i64>, need: &[(String, u32)], coins: u32, show_have: &mut bool) {
    let have = |s: &str| u64::try_from(*inv.get(&s.to_ascii_lowercase()).unwrap_or(&0)).unwrap_or(0);
    let (short, enough): (Vec<_>, Vec<_>) = need.iter().partition(|(s, n)| have(s) < u64::from(*n));
    let balance = merc_coin_balance(gs);
    let coins_short = balance.map_or(0, |b| u64::from(coins).saturating_sub(b));

    let mut gaps = Vec::new();
    match short.len() {
        0 => {}
        1 => gaps.push("1 material".to_string()),
        n => gaps.push(format!("{n} materials")),
    }
    if coins_short > 0 {
        gaps.push(format!("{coins_short} Merc Coins"));
    }
    let (glyph, text, tint) = if !gaps.is_empty() {
        (icon::WARNING, format!("Short of {}", gaps.join(" and ")), pal::WARN)
    } else if coins > 0 && balance.is_none() {
        (icon::INFO, format!("You have the materials; it also takes {coins} Merc Coins"), pal::DIM)
    } else {
        (icon::CHECK_CIRCLE, "You have everything it needs".to_string(), pal::GOOD)
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(glyph).size(size::HEADLINE).color(tint));
        ui.label(RichText::new(text).font(display(size::HEADLINE)).color(pal::TEXT));
    });
    let total: u64 = need.iter().map(|(_, n)| u64::from(*n)).sum();
    if total > 0 {
        let in_hand: u64 = need.iter().map(|(s, n)| have(s).min(u64::from(*n))).sum();
        ui.add_space(4.0);
        bar(ui, in_hand as f32 / total as f32, pal::GOOD, 6.0, None);
        crate::widgets::footnote(ui, &format!("{in_hand} of {total} materials in hand"));
    }
    ui.add_space(6.0);
    crate::widgets::group(ui, |g| {
        if coins > 0 {
            let note = balance.is_none().then_some("Your balance shows after you next log in.");
            g.content(|ui| need_row(ui, icon::COINS, pal::GOLD, "Merc Coins", balance, u64::from(coins), note));
        }
        for (s, n) in &short {
            g.content(|ui| need_row(ui, icon::CIRCLE_DASHED, pal::WARN, &item_name(s), Some(have(s)), u64::from(*n), None));
        }
        if !enough.is_empty() {
            g.content(|ui| {
                let label = match (enough.len(), short.is_empty()) {
                    (1, false) => "1 more you already have".to_string(),
                    (n, false) => format!("{n} more you already have"),
                    (n, true) => format!("All {n} materials in hand"),
                };
                if crate::widgets::disclosure(ui, show_have, &label) {
                    for (s, n) in &enough {
                        need_row(ui, icon::CHECK_CIRCLE, pal::GOOD, &item_name(s), Some(have(s)), u64::from(*n), None);
                    }
                }
            });
        }
    });
}

/// One row of a needs list: an icon and the name, then what you have of what's needed and how many more.
fn need_row(ui: &mut Ui, glyph: &str, tint: Color32, name: &str, have: Option<u64>, need: u64, note: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(glyph).color(tint));
        // The counts first, from the right, so a long name truncates instead of widening the card.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            match have {
                Some(h) if h < need => {
                    ui.label(RichText::new(format!("{} more", need - h)).color(pal::WARN));
                    ui.label(RichText::new(format!("{h} of {need}")).color(pal::DIM));
                }
                Some(h) => {
                    ui.label(RichText::new(format!("{h} of {need}")).color(pal::DIM));
                }
                None => {
                    ui.label(RichText::new(format!("{need} needed")).color(pal::DIM));
                }
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| match note {
                Some(n) => {
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(RichText::new(name).color(pal::TEXT)).truncate());
                        ui.add(egui::Label::new(RichText::new(n).size(size::FOOTNOTE).color(pal::DIM)).wrap());
                    });
                }
                None => {
                    ui.add(egui::Label::new(RichText::new(name).color(pal::TEXT)).truncate());
                }
            });
        });
    });
}

/// One roll's recipe at each grade.
fn recipes(ui: &mut Ui, bp: &Blueprint) {
    let merc = bp.is_merc();
    Grid::new("recipes").num_columns(if merc { 3 } else { 2 }).spacing([14.0, 6.0]).max_col_width(ui.available_width()).show(ui, |ui| {
        for (grade, ings) in (1u8..).zip(&bp.grades).filter(|(_, g)| !g.is_empty()) {
            ui.label(RichText::new(format!("G{grade}")).font(display(size::FOOTNOTE)).color(pal::DIM));
            if merc {
                ui.label(RichText::new(format!("{} {}", icon::COINS, bp.merc_coins_for(grade))).color(pal::DIM));
            }
            let list: Vec<String> = ings.iter().map(|i| if i.count == 1 { i.name() } else { format!("{} × {}", i.count, i.name()) }).collect();
            ui.add(egui::Label::new(RichText::new(list.join(", ")).color(pal::TEXT)).wrap());
            ui.end_row();
        }
    });
    crate::widgets::footnote(ui, if merc { "One roll at each grade, with its Merc Coins." } else { "One roll at each grade." });
}

/// The engineer to go to for the target grade (unlocked first, then the nearest), the others one click
/// deeper.
fn where_to_engineer(ui: &mut Ui, gs: &GameState, bp: &Blueprint, grade: u8, show_all: &mut bool) {
    let here = gs.location.star_pos;
    let dist = |e: &eng::Engineer| here.map(|p| nexus_model::location::distance(p, e.pos)).unwrap_or(0.0);
    let mut list: Vec<(&eng::Engineer, u8)> =
        bp.engineers_for_grade(grade).filter_map(|g| eng::engineer_by_name(&g.engineer).map(|e| (e, g.max_grade))).collect();
    list.sort_by(|a, b| unlocked(gs, b.0).cmp(&unlocked(gs, a.0)).then_with(|| dist(a.0).total_cmp(&dist(b.0))));
    crate::widgets::section(ui, &format!("Who can do G{grade}"));
    let Some(((first, first_max), rest)) = list.split_first() else {
        crate::widgets::footnote(ui, &format!("No engineer offers G{grade} of this blueprint."));
        return;
    };
    crate::widgets::group(ui, |g| {
        engineer_row(g, gs, first, *first_max);
        if !rest.is_empty() {
            let label = if rest.len() == 1 { "1 other engineer".to_string() } else { format!("{} other engineers", rest.len()) };
            g.content(|ui| crate::widgets::disclosure(ui, show_all, &label));
            if *show_all {
                for (e, max) in rest {
                    engineer_row(g, gs, e, *max);
                }
            }
        }
    });
}

fn unlocked(gs: &GameState, e: &eng::Engineer) -> bool {
    gs.cmdr.engineers.get(&e.id).is_some_and(|s| s.progress == "Unlocked")
}

/// An engineer as a row: where, how far, the top grade they offer, and your standing with them.
fn engineer_row(g: &mut crate::widgets::Group<'_>, gs: &GameState, e: &eng::Engineer, max: u8) {
    let detail = match gs.location.star_pos {
        Some(p) => format!("{} · {:.0} ly · up to G{max}", e.system, nexus_model::location::distance(p, e.pos)),
        None => format!("{} · up to G{max}", e.system),
    };
    let status = gs.cmdr.engineers.get(&e.id).map(|s| s.progress.clone());
    let tint = if unlocked(gs, e) { pal::GOOD } else { pal::DIM };
    g.row(&e.name, &detail, |ui| {
        if let Some(s) = status {
            ui.label(RichText::new(s).color(tint));
        }
    });
}

/// Upgrades for the engineered modules on the current ship, each one click from the wishlist.
fn from_your_ship(ui: &mut Ui, gs: &GameState, wishlist: &mut Vec<WishItem>) -> bool {
    let Some(ship) = gs.fleet.current() else {
        ui.label(RichText::new("No ship loadout yet.").color(pal::DIM));
        return false;
    };
    let upgrades: Vec<_> = ship
        .modules
        .iter()
        .filter_map(|m| {
            let e = m.engineering.as_ref()?;
            let bp = eng::blueprint_for(&m.item, &e.blueprint).or_else(|| eng::blueprint(&e.blueprint))?;
            (e.level < u32::from(bp.max_grade())).then_some((m, e, bp))
        })
        .collect();
    if upgrades.is_empty() {
        ui.label(RichText::new("Every engineered module on this ship is at its top grade.").color(pal::DIM));
        return false;
    }
    let mut changed = false;
    crate::widgets::group(ui, |g| {
        for (m, e, bp) in upgrades {
            let top = bp.max_grade();
            let name = nexus_data::outfitting::display_name(&m.item);
            if g.row(&name, &format!("{} · G{} to G{top}", bp.name, e.level), |ui| crate::widgets::button(ui, "Add").clicked()) {
                wishlist.push(WishItem {
                    label: format!("{name} — {}", bp.name),
                    blueprint: bp.symbol.clone(),
                    module: eng::module_type(&m.item).unwrap_or("").to_string(),
                    from: (e.level + 1).min(u32::from(top)) as u8,
                    to: top,
                    rolls: 3,
                    experimental: None,
                });
                changed = true;
            }
        }
    });
    changed
}

/// The wishlist as rows. Returns true when an item was removed.
fn wishlist_rows(ui: &mut Ui, wishlist: &mut Vec<WishItem>) -> bool {
    if wishlist.is_empty() {
        empty(ui, icon::LIST_BULLETS, "Nothing planned yet", "Add a blueprint, and everything your plan needs adds up here.");
        return false;
    }
    let mut remove = None;
    crate::widgets::group(ui, |g| {
        for (i, w) in wishlist.iter().enumerate() {
            let grades = if w.from == w.to { format!("G{}", w.to) } else { format!("G{} to G{}", w.from, w.to) };
            let mut detail = format!("{grades}, {} each", rolls_text(w.rolls));
            if let Some(x) = w.experimental.as_deref().and_then(eng::experimental) {
                detail = format!("{detail} · {}", x.name);
            }
            if g.row(&w.label, &detail, |ui| crate::widgets::icon_button(ui, icon::TRASH, "Remove").clicked()) {
                remove = Some(i);
            }
        }
    });
    match remove {
        Some(i) => {
            wishlist.remove(i);
            true
        }
        None => false,
    }
}

/// Everything the wishlist needs against what you have, and the trades that cover the gaps.
fn shopping_list(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi, inv: &HashMap<String, i64>, wishlist: &[WishItem]) {
    let mut need: Vec<(String, u32)> = Vec::new();
    let mut coins = 0u32;
    for w in wishlist {
        if let Some(bp) = eng::blueprint_for(&w.module, &w.blueprint).or_else(|| eng::blueprint(&w.blueprint)) {
            merge_into(&mut need, nexus_data::trader::blueprint_shopping(bp, w.from, w.to, w.rolls));
            coins = coins.saturating_add(nexus_data::trader::blueprint_merc_coins(bp, w.from, w.to, w.rolls));
        }
        if let Some(x) = w.experimental.as_deref().and_then(eng::experimental) {
            merge_into(&mut need, x.cost.iter().map(|i| (i.symbol.clone(), i.count)));
        }
    }
    if need.is_empty() {
        ui.label(RichText::new("Nothing needed yet.").color(pal::DIM));
        return;
    }
    needs(ui, gs, inv, &need, coins, &mut st.show_list_have);
    let plan = plan_shopping_with(&need, inv, &ShoppingOptions { cross_category: st.cross_category, ..Default::default() });
    if !plan.trades.is_empty() {
        crate::widgets::section(ui, "Trade at a material trader");
        crate::widgets::group(ui, |g| {
            for t in &plan.trades {
                let trader = match t.trader {
                    MaterialKind::Raw => "Raw",
                    MaterialKind::Manufactured => "Manufactured",
                    MaterialKind::Encoded => "Encoded",
                };
                let title = format!("{} {} {} {} {}", t.give_qty, item_name(&t.give), icon::ARROW_RIGHT, t.receive_qty, item_name(&t.receive));
                g.row(&title, &format!("{trader} material trader"), |_| {});
            }
        });
    }
    if !plan.unresolved.is_empty() {
        crate::widgets::section(ui, "Still to find");
        crate::widgets::group(ui, |g| {
            for u in &plan.unresolved {
                g.row(&item_name(&u.symbol), "", |ui| {
                    ui.label(RichText::new(format!("{} more", u.qty)).color(pal::WARN));
                });
            }
        });
    }
    ui.add_space(4.0);
    if crate::widgets::disclosure(ui, &mut st.show_list_options, "Options") {
        crate::widgets::toggle(ui, &mut st.cross_category, "Trade across categories (costs 6× more)");
    }
}

/// Merc Coins at the last login: the journal's Statistics event is the only place the game writes them.
fn merc_coin_balance(gs: &GameState) -> Option<u64> {
    gs.cmdr.statistics.as_ref()?.bank_account.get("MercCoins_Current")?.as_u64()
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
