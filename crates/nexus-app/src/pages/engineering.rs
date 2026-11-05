//! Engineering: materials, blueprint planner with shopping list & trader plan, engineers,
//! Odyssey upgrades, synthesis.

use crate::pages::where_to_get::{Goto, Sources, WhereToGet};
use crate::settings::WishItem;
use crate::theme::{display, pal, size};
use crate::widgets::{Steps, badge, bar, card, empty};
use egui::{Align, Color32, Grid, Layout, RichText, Ui};
use egui_extras::Column;
use egui_phosphor::regular as icon;
use nexus_data::engineering::{self as eng, Blueprint, Engineer, EngineerKind, Ingredient, OdysseyKind, Region, Step};
use nexus_data::materials::MaterialKind;
use nexus_data::trader::{ShoppingOptions, blueprint_merc_coins_by_grade, blueprint_shopping_by_grade, plan_shopping_with};
use nexus_model::GameState;
use nexus_model::inventory::{Locker, MaterialCategory};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The most modules one plan rolls a blueprint on: more than any ship has slots of a kind, for a plan that covers two
/// ships.
const MAX_MODULES: u32 = 99;

/// The Blueprint planner's settings, kept between runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineeringPrefs {
    /// Rolls planned at each grade, G1 first: the top rank's counts until the commander changes them. Fleet › Plan a
    /// loadout can plan with them too (through [`EngineeringPrefs::rolls`]).
    pub rolls: [u32; 5],
}

impl Default for EngineeringPrefs {
    fn default() -> Self {
        EngineeringPrefs { rolls: eng::GRADE_ROLLS }
    }
}

impl EngineeringPrefs {
    /// The rolls at each grade, G1 first, each from 1 to 5 (a grade never takes more; a hand-edited file may say
    /// anything).
    pub fn rolls(&self) -> [u32; 5] {
        self.rolls.map(|n| n.clamp(1, eng::MAX_ROLLS))
    }
}

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
    /// How many modules get the blueprint: four hull reinforcement packages take four times the rolls.
    pub modules: u32,
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
    /// The rolls at each grade, open to change (the pen).
    pub edit_rolls: bool,
    /// Engineers: the one whose whole way is open (journal EngineerID), and whether to scroll to it.
    pub engineer: Option<u32>,
    reveal: bool,
    /// "Where to get it" on the materials you're short of: the blueprint's, and the shopping list's.
    pub get_blueprint: WhereToGet,
    pub get_list: WhereToGet,
}

impl EngineeringUi {
    /// `--tab` for screenshot runs: materials, engineers (`engineers:selene` with that engineer's whole way open), suits,
    /// synthesis, merc (the planner on a Merc blueprint) or rolls (the planner on four hull reinforcement packages, the
    /// rolls at each grade open).
    pub fn new(tab: Option<&str>) -> Self {
        if let Some(name) = tab.and_then(|t| t.strip_prefix("engineers:")) {
            let key = nexus_data::commodities::name_key(name);
            let engineer = eng::engineers().iter().find(|e| !key.is_empty() && nexus_data::commodities::name_key(&e.name).contains(&key)).map(|e| e.id);
            return EngineeringUi { tab: Tab::Engineers, engineer, reveal: true, ..Default::default() };
        }
        let tab = match tab {
            Some("materials") => Tab::Materials,
            Some("engineers") => Tab::Engineers,
            Some("suits") => Tab::Odyssey,
            Some("synthesis") => Tab::Synthesis,
            Some("merc") => {
                let blueprint = Some("cargoracks5c1_extended".into());
                return EngineeringUi { module: "Cargo Rack".into(), blueprint, ..Default::default() };
            }
            Some("rolls") => {
                let blueprint = Some("HullReinforcement_HeavyDuty".into());
                return EngineeringUi { module: "Hull Reinforcement Package".into(), blueprint, modules: 4, edit_rolls: true, ..Default::default() };
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
            modules: 1,
            experimental: None,
            cross_category: true,
            show_options: false,
            show_have: false,
            show_recipes: false,
            show_engineers: false,
            show_list_have: false,
            show_list_options: false,
            edit_rolls: false,
            engineer: None,
            reveal: false,
            get_blueprint: WhereToGet::default(),
            get_list: WhereToGet::default(),
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

/// Returns true when settings changed (the wishlist, the rolls at each grade), so they get saved, and what a "where to
/// get it" panel asks the Market page to open.
pub fn show(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi, wishlist: &mut Vec<WishItem>, prefs: &mut EngineeringPrefs, src: &Sources) -> (bool, Option<Goto>) {
    let mut changed = false;
    let mut goto = None;
    if let Some(t) = crate::widgets::tabs(ui, st.tab, &[(Tab::Planner, "Blueprint planner"), (Tab::Materials, "Materials"), (Tab::Engineers, "Engineers"), (Tab::Odyssey, "Suits & weapons"), (Tab::Synthesis, "Synthesis")]) {
        st.tab = t;
    }
    ui.add_space(crate::widgets::GAP);
    match st.tab {
        Tab::Planner => (changed, goto) = planner(ui, gs, st, wishlist, prefs, src),
        Tab::Materials => materials(ui, gs, st),
        Tab::Engineers => engineers(ui, gs, st),
        Tab::Odyssey => odyssey(ui, gs),
        Tab::Synthesis => synthesis(ui, gs),
    }
    (changed, goto)
}

/// Grade labels for segmented controls.
const GRADE_LABELS: [&str; 5] = ["G1", "G2", "G3", "G4", "G5"];

fn grade_options(grades: std::ops::RangeInclusive<u8>) -> Vec<(u8, &'static str)> {
    grades.filter_map(|g| GRADE_LABELS.get(usize::from(g).checked_sub(1)?).map(|l| (g, *l))).collect()
}

/// English name of a material, micro-resource or commodity symbol.
fn item_name(symbol: &str) -> String {
    Ingredient { symbol: symbol.to_string(), count: 0 }.name()
}

fn rolls_text(n: u32) -> String {
    if n == 1 { "1 roll".into() } else { format!("{n} rolls") }
}

/// The rolls a plan makes from grade `from` to `to` on one module.
fn rolls_in(rolls: &[u32; 5], from: u8, to: u8) -> u32 {
    (from.max(1)..=to.min(5)).map(|g| rolls[usize::from(g) - 1]).sum()
}

/// What an experimental effect takes on `modules` modules (each gets its own).
fn experimental_cost(x: &eng::Experimental, modules: u32) -> impl Iterator<Item = (String, u32)> + '_ {
    x.cost.iter().map(move |i| (i.symbol.clone(), i.count.saturating_mul(modules)))
}

/// A plan as a wishlist item: its rolls at each grade, and on how many modules.
#[allow(clippy::too_many_arguments)]
fn wish_item(label: String, bp: &Blueprint, module: &str, from: u8, to: u8, rolls: [u32; 5], modules: u32, experimental: Option<String>) -> WishItem {
    WishItem {
        label,
        blueprint: bp.symbol.clone(),
        module: module.to_string(),
        from,
        to,
        rolls: rolls.iter().copied().max().unwrap_or(1),
        grade_rolls: rolls,
        modules,
        experimental,
    }
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

fn planner(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi, wishlist: &mut Vec<WishItem>, prefs: &mut EngineeringPrefs, src: &Sources) -> (bool, Option<Goto>) {
    let mut changed = false;
    let mut goto = None;
    let inv = inventory_map(gs);
    crate::widgets::cards(ui, 2, 380.0, |c| {
        c.add(|ui| {
            card(ui, "Blueprints", |ui| {
                let (c, g) = blueprint_card(ui, gs, st, &inv, wishlist, prefs, src);
                changed |= c;
                goto = goto.take().or(g);
            });
            ui.add_space(8.0);
            card(ui, "From your ship", |ui| changed |= from_your_ship(ui, gs, wishlist, prefs.rolls()));
        });
        // One card for the plan, so what the items need sits right under them.
        c.add(|ui| {
            card(ui, &format!("Wishlist ({})", wishlist.len()), |ui| {
                changed |= wishlist_rows(ui, wishlist);
                if !wishlist.is_empty() {
                    ui.add_space(6.0);
                    crate::widgets::section(ui, "Shopping list");
                    goto = goto.take().or(shopping_list(ui, gs, st, &inv, wishlist, src));
                }
            });
        });
    });
    (changed, goto)
}

/// Pick a module and blueprint, choose the grade to reach and on how many modules, and see what you're short of. The
/// recipes and the engineers sit one click deeper. Returns true when settings changed (the wishlist, the rolls at each
/// grade), and what a "where to get it" panel asks the Market page to open.
fn blueprint_card(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi, inv: &HashMap<String, i64>, wishlist: &mut Vec<WishItem>, prefs: &mut EngineeringPrefs, src: &Sources) -> (bool, Option<Goto>) {
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
    let Some(bp) = st.blueprint.as_deref().and_then(|s| bps.iter().find(|b| b.is(s)).copied()) else {
        crate::widgets::footnote(ui, "Pick a blueprint to see what it needs and who can do it.");
        return (false, None);
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

    // The grade to reach, on how many modules, and the rolls at each grade.
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
            ui.label(RichText::new(format!("Modules {}", icon::INFO)).size(size::FOOTNOTE).color(pal::DIM)).on_hover_text(
                "How many modules get this blueprint, on one ship or several. Each is rolled on its own, so four hull reinforcement packages take four times the rolls and materials.",
            );
            crate::widgets::whole(ui, &mut st.modules, 1..=MAX_MODULES, Steps::By(1.0), "");
        });
    });
    ui.add_space(10.0);
    let mut changed = rolls_editor(ui, gs, st, prefs, bp);
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
    let rolls = prefs.rolls();
    let mut need = blueprint_shopping_by_grade(bp, st.from, st.to, &rolls, st.modules);
    if let Some(x) = st.experimental.as_deref().and_then(eng::experimental) {
        merge_into(&mut need, experimental_cost(x, st.modules));
    }
    let coins = blueprint_merc_coins_by_grade(bp, st.from, st.to, &rolls, st.modules);
    ui.add_space(14.0);
    let goto = needs(ui, gs, inv, &need, coins, &mut st.show_have, &mut st.get_blueprint, src);

    ui.add_space(12.0);
    if crate::widgets::primary_button(ui, &format!("{} Add to wishlist", icon::PLUS)).clicked() {
        let label = format!("{} — {}", nexus_data::names::split_camel(&st.module), bp.name);
        wishlist.push(wish_item(label, bp, &st.module, st.from, st.to, rolls, st.modules, st.experimental.clone()));
        changed = true;
    }

    // One click deeper: the recipes, and who can do it.
    ui.add_space(10.0);
    if crate::widgets::disclosure(ui, &mut st.show_recipes, "Recipe for each grade") {
        recipes(ui, bp);
    }
    where_to_engineer(ui, gs, bp, st.to, &mut st.show_engineers);
    (changed, goto)
}

/// What a roll is, and why the counts differ by grade and rank.
const ROLLS_TIP: &str = "A roll is one craft at the engineer: it takes one set of the grade's materials. Since the game's August 2024 update (18.08) each roll fills a fixed share of its grade, set by the grade and your rank with the engineer, so luck no longer comes into it. At an engineer's top rank, G1 takes 1 roll and G5 takes 5; at the rank that opens a grade, that grade takes 5. The pen changes the counts, and Elite Nexus keeps yours for your next plans.";

/// The rolls planned at each of the plan's grades, on chips, and a pen that opens all five to change: the commander's
/// own counts, kept for later plans. Says when the journal's rank with the engineer for the plan needs more. Returns
/// true when the counts changed.
fn rolls_editor(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi, prefs: &mut EngineeringPrefs, bp: &Blueprint) -> bool {
    let before = prefs.rolls();
    let mut rolls = before;
    // The engineer the plan is likely rolled with: the unlocked one with the highest rank among those who offer it.
    let lead = bp
        .engineers_for_grade(st.to)
        .filter_map(|g| eng::engineer_by_name(&g.engineer))
        .filter(|e| unlocked(gs, e))
        .map(|e| (e, gs.cmdr.engineers.get(&e.id).and_then(|s| s.rank).map_or(1, |r| r.clamp(1, 5) as u8)))
        .max_by_key(|(_, r)| *r);
    let planned = st.from..=st.to;
    let short_at = |r: u8, rolls: &[u32; 5]| planned.clone().any(|g| eng::rolls_at_rank(g, r) > rolls[usize::from(g) - 1]);
    ui.label(RichText::new(format!("Rolls at each grade {}", icon::INFO)).size(size::FOOTNOTE).color(pal::DIM)).on_hover_text(ROLLS_TIP);
    ui.horizontal_wrapped(|ui| {
        for g in planned.clone() {
            let n = rolls[usize::from(g) - 1];
            if crate::widgets::count_chip(ui, &format!("G{g}"), &n.to_string()).on_hover_text(format!("{} at G{g}: click to change", rolls_text(n))).clicked() {
                st.edit_rolls = !st.edit_rolls;
            }
        }
        let (glyph, tip) = if st.edit_rolls { (icon::CHECK, "Done") } else { (icon::PENCIL_SIMPLE, "Change the rolls at each grade") };
        if crate::widgets::icon_button(ui, glyph, tip).clicked() {
            st.edit_rolls = !st.edit_rolls;
        }
        let one = rolls_in(&rolls, st.from, st.to);
        let total = if st.modules > 1 { format!("{} a module, {} in all", rolls_text(one), one.saturating_mul(st.modules)) } else { rolls_text(one) };
        // Whole, on the next line when the chips leave no room.
        ui.add(egui::Label::new(RichText::new(total).size(size::FOOTNOTE).color(pal::DIM)).extend());
    });
    if st.edit_rolls {
        ui.add_space(6.0);
        crate::widgets::group(ui, |g| {
            for grade in 1..=5u8 {
                let detail = match grade {
                    5 => "5 rolls: only rank 5 opens it".to_string(),
                    _ => format!("{} at rank 5, up to 5 at rank {grade}", rolls_text(eng::rolls_at_rank(grade, 5))),
                };
                g.row(&format!("Grade {grade}"), &detail, |ui| {
                    crate::widgets::whole(ui, &mut rolls[usize::from(grade) - 1], 1..=eng::MAX_ROLLS, Steps::By(1.0), "");
                });
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if rolls != eng::GRADE_ROLLS && crate::widgets::button(ui, "Top rank's counts").on_hover_text("1 roll at G1 up to 5 at G5, as at an engineer's rank 5").clicked() {
                rolls = eng::GRADE_ROLLS;
            }
            if let Some((e, r)) = lead.filter(|(_, r)| *r < 5) {
                let at = eng::grade_rolls_at_rank(r);
                if at != rolls && crate::widgets::button(ui, &format!("For rank {r} with {}", e.name)).on_hover_text("Your rank with them, as your journal last reported it: until it climbs, grades take more rolls").clicked() {
                    rolls = at;
                }
            }
        });
        crate::widgets::footnote(ui, "Since the game's August 2024 update each roll fills a fixed share of its grade, set by your rank with the engineer rather than by luck. Your counts are kept for your next plans.");
    } else if let Some((e, r)) = lead.filter(|(_, r)| *r < 5 && short_at(*r, &rolls)) {
        crate::widgets::footnote(ui, &format!("Your rank with {} is {r} of 5: until it climbs, these grades take more rolls than at the top rank, up to 5 each. The pen sets the counts for rank {r}.", e.name));
    } else if lead.is_none() && !gs.cmdr.engineers.is_empty() && short_at(1, &rolls) {
        crate::widgets::footnote(ui, &format!("No engineer who does G{} works for you yet. With a new engineer each grade takes 5 rolls until your rank climbs: the pen changes the counts.", st.to));
    }
    if rolls == before {
        return false;
    }
    prefs.rolls = rolls;
    true
}

/// What a plan needs against what you have: a status line and a bar, a row for each thing you're
/// short of (with where to get it), and one row that opens what you already have enough of. Returns what a "where to
/// get it" panel asks the Market page to open.
#[allow(clippy::too_many_arguments)]
fn needs(ui: &mut Ui, gs: &GameState, inv: &HashMap<String, i64>, need: &[(String, u32)], coins: u32, show_have: &mut bool, get: &mut WhereToGet, src: &Sources) -> Option<Goto> {
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
    let mut goto = None;
    crate::widgets::group(ui, |g| {
        if coins > 0 {
            let note = balance.is_none().then_some("Your balance shows after you next log in.");
            g.content(|ui| need_row(ui, icon::COINS, pal::GOLD, "Merc Coins", balance, u64::from(coins), note, |_| {}));
        }
        for (s, n) in &short {
            g.content(|ui| need_row(ui, icon::CIRCLE_DASHED, pal::WARN, &item_name(s), Some(have(s)), u64::from(*n), None, |ui| get.button(ui, s)));
            // Where to get it, right under its row.
            if get.is_open(s) {
                g.content(|ui| goto = get.panel(ui, gs, src, s));
            }
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
                        need_row(ui, icon::CHECK_CIRCLE, pal::GOOD, &item_name(s), Some(have(s)), u64::from(*n), None, |_| {});
                    }
                }
            });
        }
    });
    goto
}

/// One row of a needs list: an icon and the name, then what you have of what's needed and how many more, and
/// `trailing` at the end (a "where to get it" button).
#[allow(clippy::too_many_arguments)]
fn need_row(ui: &mut Ui, glyph: &str, tint: Color32, name: &str, have: Option<u64>, need: u64, note: Option<&str>, trailing: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(glyph).color(tint));
        // The counts first, from the right, so a long name truncates instead of widening the card.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            trailing(ui);
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

/// Where the commander stands with an engineer: `Progress` from the journal's `EngineerProgress` ("Known", "Invited",
/// "Unlocked"…), `None` when the journal doesn't list them.
fn progress<'a>(gs: &'a GameState, e: &Engineer) -> Option<&'a str> {
    gs.cmdr.engineers.get(&e.id).map(|s| s.progress.as_str()).filter(|p| !p.is_empty())
}

fn unlocked(gs: &GameState, e: &eng::Engineer) -> bool {
    progress(gs, e) == Some("Unlocked")
}

/// An engineer as a row: where, how far, the top grade they offer, and your standing with them (on hover, what's next
/// for one not unlocked yet).
fn engineer_row(g: &mut crate::widgets::Group<'_>, gs: &GameState, e: &eng::Engineer, max: u8) {
    let detail = match gs.location.star_pos {
        Some(p) => format!("{} · {:.0} ly · up to G{max}", e.system, nexus_model::location::distance(p, e.pos)),
        None => format!("{} · up to G{max}", e.system),
    };
    let progress = progress(gs, e);
    let next = Step::next(progress).map(|s| format!("{}: {}", s.label(), e.step(s)));
    let tint = if unlocked(gs, e) { pal::GOOD } else { pal::DIM };
    g.row(&e.name, &detail, |ui| {
        let status = ui.label(RichText::new(progress.unwrap_or("Not known yet")).color(tint));
        if let Some(next) = next {
            status.on_hover_text(next);
        }
    });
}

/// Upgrades for the engineered modules on the current ship, each one click from the wishlist. The same module with the
/// same blueprint at the same grade is one row (four hull reinforcement packages), added as one plan for all of them.
fn from_your_ship(ui: &mut Ui, gs: &GameState, wishlist: &mut Vec<WishItem>, rolls: [u32; 5]) -> bool {
    let Some(ship) = gs.fleet.current() else {
        ui.label(RichText::new("No ship loadout yet.").color(pal::DIM));
        return false;
    };
    // (name, module type, blueprint, grade it has, how many)
    let mut upgrades: Vec<(String, &'static str, &'static Blueprint, u32, u32)> = Vec::new();
    for m in &ship.modules {
        let Some(e) = m.engineering.as_ref() else { continue };
        let Some(bp) = eng::blueprint_for(&m.item, &e.blueprint).or_else(|| eng::blueprint(&e.blueprint)) else { continue };
        if e.level >= u32::from(bp.max_grade()) {
            continue;
        }
        let kind = eng::module_type(&m.item).unwrap_or("");
        let name = nexus_data::outfitting::display_name(&m.item);
        match upgrades.iter_mut().find(|u| u.1 == kind && std::ptr::eq(u.2, bp) && u.3 == e.level) {
            Some(u) => {
                u.4 += 1;
                // Sizes or classes differ: the module type names them all.
                if u.0 != name {
                    u.0 = kind.to_string();
                }
            }
            None => upgrades.push((name, kind, bp, e.level, 1)),
        }
    }
    if upgrades.is_empty() {
        ui.label(RichText::new("Every engineered module on this ship is at its top grade.").color(pal::DIM));
        return false;
    }
    let mut changed = false;
    crate::widgets::group(ui, |g| {
        for (name, kind, bp, level, count) in upgrades {
            let top = bp.max_grade();
            let title = if count > 1 { format!("{count} × {name}") } else { name.clone() };
            if g.row(&title, &format!("{} · G{level} to G{top}", bp.name), |ui| crate::widgets::button(ui, "Add").clicked()) {
                // The wishlist row says on how many modules.
                let from = u8::try_from(level + 1).unwrap_or(top).min(top);
                wishlist.push(wish_item(format!("{name} — {}", bp.name), bp, kind, from, top, rolls, count, None));
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
            let rolls = rolls_text(rolls_in(&w.rolls_by_grade(), w.from, w.to));
            let mut detail = match w.module_count() {
                1 => format!("{grades}, {rolls}"),
                n => format!("{grades} on {n} modules, {rolls} each"),
            };
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

/// Everything the wishlist needs against what you have, and the trades that cover the gaps. Returns what a "where to
/// get it" panel asks the Market page to open.
fn shopping_list(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi, inv: &HashMap<String, i64>, wishlist: &[WishItem], src: &Sources) -> Option<Goto> {
    let mut need: Vec<(String, u32)> = Vec::new();
    let mut coins = 0u32;
    for w in wishlist {
        let (rolls, modules) = (w.rolls_by_grade(), w.module_count());
        if let Some(bp) = eng::blueprint_for(&w.module, &w.blueprint).or_else(|| eng::blueprint(&w.blueprint)) {
            merge_into(&mut need, blueprint_shopping_by_grade(bp, w.from, w.to, &rolls, modules));
            coins = coins.saturating_add(blueprint_merc_coins_by_grade(bp, w.from, w.to, &rolls, modules));
        }
        if let Some(x) = w.experimental.as_deref().and_then(eng::experimental) {
            merge_into(&mut need, experimental_cost(x, modules));
        }
    }
    if need.is_empty() {
        ui.label(RichText::new("Nothing needed yet.").color(pal::DIM));
        return None;
    }
    let goto = needs(ui, gs, inv, &need, coins, &mut st.show_list_have, &mut st.get_list, src);
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
    goto
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
                ui.push_id(("mats", i), |ui| {
                    // As close together as the grid they replace.
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (row, (name, grade, n)) in items.into_iter().enumerate() {
                        material_row(ui, &name, grade, n, row % 2 == 1);
                    }
                });
            });
            });
        }
    });
}

/// One material in the Materials cards: its name, grade and a bar of how many against the cap. The grade and the bar
/// keep their place at the right, so a long name ("Guardian Sentinel Wreckage Components") ends in "…" (whole on
/// hover) instead of widening the card past its column.
fn material_row(ui: &mut Ui, name: &str, grade: u8, n: i64, stripe: bool) {
    let cap = nexus_data::materials::cap(grade) as i64;
    let background = ui.painter().add(egui::Shape::Noop);
    let row = ui.horizontal(|ui| {
        ui.set_min_height(24.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Narrow cards give the bar less room before the name has to give way.
            let bar_w = (ui.available_width() * 0.32).clamp(72.0, 110.0);
            ui.scope(|ui| {
                ui.set_width(bar_w);
                let full = cap > 0 && n >= cap;
                bar(ui, if cap > 0 { n as f32 / cap as f32 } else { 0.0 }, if full { pal::WARN } else { pal::ACCENT }, 12.0, Some(&if cap > 0 { format!("{n}/{cap}") } else { n.to_string() }));
            });
            ui.label(RichText::new(if grade > 0 { format!("G{grade}") } else { String::new() }).small().color(pal::PURPLE));
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(name).color(if n == 0 { pal::FAINT } else { pal::TEXT })).truncate());
            });
        });
    });
    if stripe {
        let rect = row.response.rect.expand2(egui::vec2(4.0, 0.0));
        ui.painter().set(background, egui::Shape::rect_filled(rect, 4.0, ui.visuals().faint_bg_color));
    }
}

/// Sorting by standing: unlocked first, then invited, known, not heard of, barred.
fn standing_order(progress: Option<&str>) -> u8 {
    match progress {
        Some("Unlocked") => 0,
        Some("Invited" | "Acquainted") => 1,
        Some("Known") => 2,
        None => 3,
        Some(_) => 4,
    }
}

/// The commander's standing in a word and a colour: "Unlocked · G3" (the grades they offer the commander), "Invited",
/// "Known", "Not known yet"; a dash before the journal has listed any engineer.
fn standing(gs: &GameState, e: &Engineer) -> (String, Color32) {
    match progress(gs, e) {
        Some("Unlocked") => match gs.cmdr.engineers.get(&e.id).and_then(|s| s.rank) {
            Some(r) if e.kind == EngineerKind::Ship => (format!("Unlocked · G{r}"), pal::GOOD),
            _ => ("Unlocked".into(), pal::GOOD),
        },
        Some("Barred") => ("Barred".into(), pal::BAD),
        Some(p) => (p.to_string(), if p == "Known" { pal::WARN } else { pal::ACCENT_SOFT }),
        None if gs.cmdr.engineers.is_empty() => ("—".into(), pal::FAINT),
        None => ("Not known yet".into(), pal::FAINT),
    }
}

/// What an engineer works on, and where: "Ship modules · Colonia · system permit".
fn engineer_kind(e: &Engineer) -> String {
    let mut s = String::from(if e.kind == EngineerKind::OnFoot { "Suits & weapons" } else { "Ship modules" });
    if e.region == Region::Colonia {
        s.push_str(" · Colonia");
    }
    if e.permit {
        s.push_str(" · system permit");
    }
    s
}

/// What an engineer offers, best first: module types with the top grade, or on-foot modifications by name.
fn works_on(e: &Engineer) -> Vec<String> {
    if e.kind == EngineerKind::OnFoot {
        return e.mods.iter().map(|s| eng::odyssey_mod(s).map_or_else(|| s.clone(), |m| m.name.clone())).collect();
    }
    e.modules.iter().map(|m| format!("{} G{}", nexus_data::names::split_camel(&m.module), m.max_grade)).collect()
}

/// Engineers: where each is, the commander's standing and their next step, from the journal. A click opens one in
/// full, beside the list in a wide window and above it in a narrow one.
fn engineers(ui: &mut Ui, gs: &GameState, st: &mut EngineeringUi) {
    let dist = |e: &Engineer| gs.location.star_pos.map_or(0.0, |p| e.distance_to(p));
    let mut list: Vec<&'static Engineer> = eng::engineers().iter().collect();
    list.sort_by(|a, b| standing_order(progress(gs, a)).cmp(&standing_order(progress(gs, b))).then_with(|| dist(a).total_cmp(&dist(b))));
    let summary = if gs.cmdr.engineers.is_empty() {
        "Your journal hasn't listed your engineers yet: the game writes them when you log in.".to_string()
    } else {
        let count = |o: u8| list.iter().filter(|e| standing_order(progress(gs, e)) == o).count();
        format!("From your journal: {} unlocked, {} invited, {} known, {} not known yet.", count(0), count(1), count(2), count(3))
    };
    crate::widgets::footnote(ui, &format!("{summary} Click an engineer for the whole way to them, how to rank up and what they work on."));
    ui.add_space(8.0);
    let selected = st.engineer.and_then(eng::engineer_by_id);
    const GAP: f32 = crate::widgets::GAP;
    match selected {
        // Wide windows: the list with the engineer beside it, kept in view as the page scrolls past the list (never
        // below its end); narrow: the engineer first, scrolled into view when picked.
        Some(e) if ui.available_width() >= 1000.0 => {
            let dw = 380.0;
            ui.horizontal_top(|ui| {
                let tw = ui.available_width() - dw - GAP;
                let table = ui
                    .vertical(|ui| {
                        ui.set_width(tw);
                        engineers_table(ui, gs, &list, st);
                    })
                    .response
                    .rect;
                ui.add_space(GAP - ui.spacing().item_spacing.x);
                ui.vertical(|ui| {
                    ui.set_width(dw);
                    let key = ui.id().with("engineer-height");
                    let height: f32 = ui.data(|d| d.get_temp(key)).unwrap_or(0.0);
                    let top = ui.cursor().top();
                    let down = (ui.clip_rect().top() + 8.0 - top).min(table.bottom() - top - height).max(0.0);
                    ui.add_space(down);
                    let card = ui.scope(|ui| engineer_detail(ui, gs, e, st)).response.rect;
                    ui.data_mut(|d| d.insert_temp(key, card.height()));
                    st.reveal = false;
                });
            });
        }
        Some(e) => {
            let card = ui.scope(|ui| engineer_detail(ui, gs, e, st)).response.rect;
            if std::mem::take(&mut st.reveal) {
                ui.scroll_to_rect(card, Some(Align::TOP));
            }
            ui.add_space(GAP);
            engineers_table(ui, gs, &list, st);
        }
        None => engineers_table(ui, gs, &list, st),
    }
}

/// The engineers as a table: who, where, the commander's standing (a dot for each step on the way to them) and the next
/// step in plain words, or what an unlocked one works on. A click picks one for the detail, a second click closes it.
fn engineers_table(ui: &mut Ui, gs: &GameState, list: &[&'static Engineer], st: &mut EngineeringUi) {
    let mut clicked = None;
    egui_extras::TableBuilder::new(ui)
        .id_salt("engineers")
        .striped(true)
        .resizable(false)
        .vscroll(false)
        .sense(egui::Sense::click())
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(Column::exact(190.0).clip(true))
        .column(Column::exact(176.0).clip(true))
        .column(Column::exact(162.0).clip(true))
        .column(Column::remainder().at_least(180.0).clip(true))
        .header(28.0, |mut h| {
            for t in ["Engineer", "Where", "Status", "Next step"] {
                h.col(|ui| crate::widgets::th(ui, t));
            }
        })
        .body(|body| {
            body.rows(48.0, list.len(), |mut row| {
                let e = list[row.index()];
                row.set_selected(st.engineer == Some(e.id));
                let p = progress(gs, e);
                let next = Step::next(p);
                let two_lines = |ui: &mut Ui, first: RichText, second: RichText| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.add(egui::Label::new(first).truncate());
                        ui.add(egui::Label::new(second.size(size::FOOTNOTE).color(pal::DIM)).truncate());
                    });
                };
                row.col(|ui| two_lines(ui, RichText::new(&e.name).color(pal::TEXT), RichText::new(engineer_kind(e))));
                row.col(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        if ui.link(&e.system).on_hover_text("Click to copy").clicked() {
                            ui.ctx().copy_text(e.system.clone());
                        }
                        let place = match gs.location.star_pos {
                            Some(here) => format!("{:.0} ly · {}", e.distance_to(here), e.base),
                            None => e.base.clone(),
                        };
                        ui.add(egui::Label::new(RichText::new(place).size(size::FOOTNOTE).color(pal::DIM)).truncate());
                    });
                });
                row.col(|ui| {
                    let (text, tint) = standing(gs, e);
                    if p != Some("Barred") && !gs.cmdr.engineers.is_empty() {
                        let done = next.map_or(Step::ALL.len(), |s| s as usize);
                        crate::widgets::step_dots(ui, Step::ALL.len(), done, pal::ACCENT);
                    }
                    ui.add(egui::Label::new(RichText::new(text).color(tint)).truncate());
                });
                row.col(|ui| match next {
                    Some(s) => two_lines(ui, RichText::new(e.step(s)).color(pal::TEXT), RichText::new(s.label())),
                    None => two_lines(ui, RichText::new(works_on(e).join(" · ")).color(pal::TEXT), RichText::new("Works on")),
                });
                if row.response().clicked() {
                    clicked = Some(e.id);
                }
            });
        });
    if let Some(id) = clicked {
        st.engineer = if st.engineer == Some(id) { None } else { Some(id) };
        st.reveal = true;
    }
}

/// One engineer in full: where, the commander's standing, the whole way to them with the step they're on, how to rank
/// up and who they lead to, and what they work on.
fn engineer_detail(ui: &mut Ui, gs: &GameState, e: &Engineer, st: &mut EngineeringUi) {
    let status = gs.cmdr.engineers.get(&e.id);
    let p = progress(gs, e);
    let next = Step::next(p);
    let close = |ui: &mut Ui| {
        if crate::widgets::icon_button(ui, icon::X, "Close").clicked() {
            st.engineer = None;
        }
    };
    crate::widgets::card_with(ui, &e.name, close, |ui| {
        ui.label(RichText::new(engineer_kind(e)).size(size::FOOTNOTE).color(pal::DIM));
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new(icon::MAP_PIN).color(pal::DIM));
            if ui.link(&e.system).on_hover_text("Click to copy").clicked() {
                ui.ctx().copy_text(e.system.clone());
            }
            let body = if e.body.trim().is_empty() { String::new() } else { format!(", {}", e.body.trim()) };
            let far = gs.location.star_pos.map(|p| format!(" · {:.0} ly", e.distance_to(p))).unwrap_or_default();
            ui.label(RichText::new(format!("{}{body}{far}", e.base)).color(pal::DIM));
        });
        ui.add_space(4.0);
        let (text, tint) = standing(gs, e);
        let rank = match status.and_then(|s| s.rank) {
            Some(r) if e.kind == EngineerKind::Ship && r < 5 => match status.and_then(|s| s.rank_progress) {
                Some(pct) => format!(": grades 1 to {r}, {pct} % of the way to {}", r + 1),
                None => format!(": grades 1 to {r}"),
            },
            Some(_) if e.kind == EngineerKind::Ship => ": every grade they offer".to_string(),
            _ => String::new(),
        };
        let line = if gs.cmdr.engineers.is_empty() { "Your journal hasn't listed your engineers yet.".to_string() } else { format!("{text}{rank}") };
        ui.label(RichText::new(line).color(tint));

        crate::widgets::section(ui, "The way to them");
        crate::widgets::group(ui, |g| {
            for s in Step::ALL {
                let (glyph, tint) = match next {
                    _ if p == Some("Barred") => (icon::PROHIBIT, pal::BAD),
                    Some(n) if s == n => (icon::ARROW_CIRCLE_RIGHT, pal::ACCENT),
                    Some(n) if s > n => (icon::CIRCLE_DASHED, pal::FAINT),
                    _ => (icon::CHECK_CIRCLE, pal::GOOD),
                };
                g.content(|ui| {
                    ui.horizontal_top(|ui| {
                        ui.label(RichText::new(glyph).size(size::HEADLINE).color(tint));
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new(s.label()).font(display(size::FOOTNOTE)).color(if next == Some(s) { pal::TEXT } else { pal::DIM }));
                            ui.add(egui::Label::new(RichText::new(e.step(s)).color(pal::TEXT)).wrap());
                            // Hearing of them is the step: where the commander stands with the engineer who refers them.
                            if s == Step::Learn && next == Some(Step::Learn) && !gs.cmdr.engineers.is_empty() {
                                let you: Vec<String> = e.referred_by.iter().filter_map(|by| eng::engineer_by_name(by)).map(|r| referrer_standing(gs, r)).collect();
                                if !you.is_empty() {
                                    ui.add(egui::Label::new(RichText::new(you.join(" ")).size(size::FOOTNOTE).color(pal::ACCENT_SOFT)).wrap());
                                }
                            }
                        });
                    });
                });
            }
        });
        if p == Some("Barred") {
            crate::widgets::footnote(ui, "Your journal says this engineer has barred you.");
        } else if gs.cmdr.engineers.is_empty() {
            crate::widgets::footnote(ui, "The steps you've done show once your journal lists your engineers (at your next login).");
        }

        if e.kind == EngineerKind::Ship {
            crate::widgets::section(ui, "Higher grades");
            crate::widgets::footnote(ui, &tidy(&e.reputation));
        }
        if !e.referral.trim().is_empty() {
            crate::widgets::section(ui, "Leads to");
            crate::widgets::footnote(ui, &tidy(&e.referral));
        }
        let offers = works_on(e);
        if !offers.is_empty() {
            crate::widgets::section(ui, "Works on");
            ui.add(egui::Label::new(RichText::new(offers.join(" · ")).color(pal::TEXT)).wrap());
        }
    });
}

/// Where the commander stands with an engineer who refers another, as a sentence: "You're at grade 1 with Tod 'The
/// Blaster' McQuinn."
fn referrer_standing(gs: &GameState, r: &Engineer) -> String {
    match (progress(gs, r), gs.cmdr.engineers.get(&r.id).and_then(|s| s.rank)) {
        (Some("Unlocked"), Some(rank)) if r.kind == EngineerKind::Ship => format!("You're at grade {rank} with {}.", r.name),
        (Some("Unlocked"), _) => format!("{} works for you.", r.name),
        (Some(p), _) => format!("{} isn't unlocked yet ({}).", r.name, p.to_lowercase()),
        (None, _) => format!("You haven't heard of {} yet.", r.name),
    }
}

/// Sentences as the data has them, a space after each full stop ("increase.Sell" in one engineer's text).
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    let mut chars = text.trim().chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '.' && chars.peek().is_some_and(|n| n.is_ascii_uppercase()) {
            out.push(' ');
        }
    }
    out
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
    let have = |s: &str| locker_count(locker, s);
    crate::widgets::cards(ui, 2, 360.0, |c| {
        for (kind, title) in [(OdysseyKind::Suit, "Suits"), (OdysseyKind::Weapon, "Weapons")] {
            c.add(|ui| {
                card(ui, title, |ui| {
                for item in eng::odyssey_items().iter().filter(|i| i.kind == kind) {
                    egui::CollapsingHeader::new(RichText::new(item.name).color(pal::TEXT)).id_salt(item.symbol).show(ui, |ui| {
                        crate::widgets::section(ui, "Upgrades");
                        crate::widgets::group(ui, |g| {
                            for grade in 2..=5u8 {
                                if let Some(up) = eng::odyssey_upgrade(item.symbol, grade) {
                                    let price = item.upgrade_credits(grade).map(credits).unwrap_or_default();
                                    g.content(|ui| recipe_row(ui, (item.symbol, grade), &format!("Grade {grade}"), &price, &up.cost, have));
                                }
                            }
                        });
                        let mods = eng::odyssey_mods_for(item.symbol);
                        if !mods.is_empty() {
                            crate::widgets::section(ui, "Modifications");
                            crate::widgets::group(ui, |g| {
                                for m in mods {
                                    let price = if m.credits > 0 { credits(m.credits) } else { String::new() };
                                    g.content(|ui| recipe_row(ui, (item.symbol, m.symbol.as_str()), &m.name, &price, &m.cost, have));
                                }
                            });
                        }
                        crate::widgets::footnote(ui, "Goods, assets and data counted in your ship locker.");
                    });
                }
            });
            });
        }
    });
}

fn synthesis(ui: &mut Ui, gs: &GameState) {
    let inv = inventory_map(gs);
    let have = |s: &str| *inv.get(&s.to_ascii_lowercase()).unwrap_or(&0);
    let mut families: Vec<&str> = eng::synthesis_recipes().iter().map(|r| r.family.as_str()).collect();
    families.dedup();
    // Two columns where there's room: a family's three rows don't need the whole width.
    crate::widgets::cards(ui, 2, 440.0, |c| {
        for fam in families {
            c.add(|ui| {
                card(ui, fam, |ui| {
                    crate::widgets::group(ui, |g| {
                        for r in eng::synthesis_recipes().iter().filter(|r| r.family == fam) {
                            g.content(|ui| recipe_row(ui, (fam, r.name.as_str()), &r.name, &r.effect, &r.cost, have));
                        }
                    });
                });
            });
        }
    });
}

fn credits(cr: u64) -> String {
    format!("{} Cr", nexus_data::names::group_digits(i64::try_from(cr).unwrap_or(i64::MAX)))
}

/// A recipe as one row, as the blueprint planner shows needs: its name and what it does or costs, then whether you
/// have everything it takes. Opening it lists each ingredient against what you have, what you're short of first.
fn recipe_row(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, name: &str, detail: &str, cost: &[Ingredient], have: impl Fn(&str) -> i64) {
    let short = cost.iter().filter(|i| have(&i.symbol) < i64::from(i.count)).count();
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id(id), false)
        .show_header(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                match short {
                    0 => ui.label(RichText::new(format!("{} Ready", icon::CHECK_CIRCLE)).color(pal::GOOD)),
                    1 => ui.label(RichText::new("Short of 1 material").color(pal::WARN)),
                    n => ui.label(RichText::new(format!("Short of {n} materials")).color(pal::WARN)),
                };
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(name).color(pal::TEXT)).truncate());
                    if !detail.is_empty() {
                        ui.add(egui::Label::new(RichText::new(detail).size(size::FOOTNOTE).color(pal::DIM)).truncate());
                    }
                });
            });
        })
        .body(|ui| {
            let mut rows: Vec<&Ingredient> = cost.iter().collect();
            rows.sort_by_key(|i| have(&i.symbol) >= i64::from(i.count));
            for i in rows {
                let h = u64::try_from(have(&i.symbol)).unwrap_or(0);
                let ok = h >= u64::from(i.count);
                let (glyph, tint) = if ok { (icon::CHECK_CIRCLE, pal::GOOD) } else { (icon::CIRCLE_DASHED, pal::WARN) };
                need_row(ui, glyph, tint, &i.name(), Some(h), u64::from(i.count), None, |_| {});
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_model::commander::EngineerStatus;

    /// A wishlist saved before each grade had its own rolls keeps its plan; a new item keeps its rolls and module count.
    #[test]
    fn wishlist_items_old_and_new() {
        let old: WishItem = serde_json::from_str(r#"{"label":"FSD","blueprint":"FSD_LongRange","module":"Frame Shift Drive","from":1,"to":5,"rolls":3}"#).unwrap();
        assert_eq!((old.rolls_by_grade(), old.module_count()), ([3; 5], 1));
        let bp = eng::blueprint("HullReinforcement_HeavyDuty").unwrap();
        let new = wish_item("4 × HRP".into(), bp, "Hull Reinforcement Package", 2, 5, eng::GRADE_ROLLS, 4, None);
        assert_eq!((new.rolls, new.module_count(), rolls_in(&new.rolls_by_grade(), new.from, new.to)), (5, 4, 2 + 3 + 4 + 5));
        let back: WishItem = serde_json::from_str(&serde_json::to_string(&new).unwrap()).unwrap();
        assert_eq!(back, new);
    }

    #[test]
    fn rolls_stay_within_what_a_grade_takes() {
        assert_eq!(EngineeringPrefs::default().rolls(), eng::GRADE_ROLLS);
        let odd: EngineeringPrefs = serde_json::from_str(r#"{"rolls":[0,2,9,4,5]}"#).unwrap();
        assert_eq!(odd.rolls(), [1, 2, 5, 4, 5]);
        let missing: EngineeringPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(missing.rolls(), eng::GRADE_ROLLS);
        assert_eq!((rolls_in(&eng::GRADE_ROLLS, 1, 5), rolls_in(&eng::GRADE_ROLLS, 4, 5), rolls_in(&eng::GRADE_ROLLS, 6, 9)), (15, 9, 0));
    }

    /// Where the commander stands with an engineer, from the journal, in words.
    #[test]
    fn standing_from_the_journal() {
        let mut gs = GameState::default();
        let (selene, tod) = (eng::engineer_by_name("Selene Jean").unwrap(), eng::engineer_by_id(300260).unwrap());
        assert_eq!(standing(&gs, selene).0, "—", "nothing from the journal yet");
        let status = |progress: &str, rank: Option<u32>| EngineerStatus { id: 300260, name: tod.name.clone(), progress: progress.into(), rank, rank_progress: None };
        gs.cmdr.engineers.insert(300260, status("Unlocked", Some(3)));
        assert_eq!((standing(&gs, selene).0, standing(&gs, tod).0), ("Not known yet".to_string(), "Unlocked · G3".to_string()));
        assert_eq!(referrer_standing(&gs, tod), "You're at grade 3 with Tod 'The Blaster' McQuinn.", "Selene Jean's referral, from where the commander stands");
        assert_eq!(referrer_standing(&gs, selene), "You haven't heard of Selene Jean yet.");
        gs.cmdr.engineers.insert(300210, EngineerStatus { id: 300210, progress: "Invited".into(), ..Default::default() });
        assert_eq!((standing(&gs, selene).0.as_str(), Step::next(progress(&gs, selene))), ("Invited", Some(Step::Unlock)));
        let mut order: Vec<u8> = ["Known", "Unlocked", "Barred", "Invited"].map(|p| standing_order(Some(p))).to_vec();
        order.push(standing_order(None));
        assert_eq!(order, vec![2, 0, 4, 1, 3]);
    }

    #[test]
    fn engineer_texts_read_well() {
        assert_eq!(tidy("Craft modules for a major increase.Sell commodities to Vatermann LLC."), "Craft modules for a major increase. Sell commodities to Vatermann LLC.");
        assert_eq!(tidy(" Pay 1.5 M Cr. "), "Pay 1.5 M Cr.");
        let kit = eng::engineer_by_name("Kit Fowler").unwrap();
        assert!(!works_on(kit).is_empty() && works_on(kit).iter().all(|m| !m.contains('_')), "names, not codes: {:?}", works_on(kit));
        assert_eq!(engineer_kind(eng::engineer_by_name("Etienne Dorn").unwrap()), "Ship modules · Colonia");
        assert_eq!(engineer_kind(eng::engineer_by_name("Bill Turner").unwrap()), "Ship modules · system permit");
        assert_eq!(engineer_kind(kit), "Suits & weapons");
        assert_eq!(works_on(eng::engineer_by_name("Selene Jean").unwrap()), vec!["Armour G5", "Hull Reinforcement Package G5"]);
    }
}
