//! The right side of Plan a loadout: what the build comes to, in panels. Overview (the main figures as tiles),
//! Defence (shields and armour against each damage type, recharge), Weapons (damage, draw, heat, ammunition), Power &
//! heat (priority groups, switching modules, heat), Travel (jump range, speed and turning by pips) and Compare (beside
//! another build). The pips, what's carried and a discount sit above the panels they change.

use super::compare::{Choice, Pick};
use crate::theme::{display, pal, size};
use crate::widgets::{self, card, card_tinted, footnote, short_credits};
use egui::{Color32, CornerRadius, Label, Rect, RichText, Sense, Ui, Vec2};
use egui_extras::Column;
use egui_phosphor::regular as icon;
use nexus_data::fitting::{self, Figures, Hull, Pips, Plan, Resistances, Setup, Shield};
use nexus_data::outfitting;

/// The panels of the right side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    #[default]
    Overview,
    Defence,
    Weapons,
    Power,
    Travel,
    Compare,
}

impl Panel {
    /// Pips, what's carried or a discount change its figures.
    pub fn uses_setup(self) -> bool {
        matches!(self, Panel::Overview | Panel::Defence | Panel::Weapons | Panel::Travel)
    }
}

pub fn tabs(ui: &mut Ui, panel: &mut Panel) {
    if let Some(p) = widgets::tabs(ui, *panel, &[(Panel::Overview, "Overview"), (Panel::Defence, "Defence"), (Panel::Weapons, "Weapons"), (Panel::Power, "Power & heat"), (Panel::Travel, "Travel"), (Panel::Compare, "Compare")]) {
        *panel = p;
    }
}

// ---------------------------------------------------------------------------------------------
// Pips, what's carried, a discount

/// The pips, and what `panel` shows besides (the cargo and fuel carried on Travel, a discount on Overview), in a
/// settings bar. True when something changed.
pub fn setup_bar(ui: &mut Ui, setup: &mut Setup, f: &Figures, panel: Panel) -> bool {
    let mut changed = false;
    let carried = panel == Panel::Travel;
    widgets::settings_bar(ui, |bar| {
        bar.field("Pips", "Systems, engines, weapons: a click moves a pip there, as the game's buttons do. Systems pips harden the shields and recharge them, engine pips speed the ship and its turns, weapon pips recharge the weapons capacitor.", |ui| {
            changed |= pips(ui, &mut setup.pips);
        });
        if panel == Panel::Overview {
            bar.field("Discount", "Stations in some systems sell ships and modules for less than their list price: the hull's and modules' prices, and the rebuy, are worked out at this discount.", |ui| {
                let mut pct = (setup.discount * 100.0).round();
                if widgets::number(ui, &mut pct, 0.0..=50.0, widgets::Steps::Through(&[0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0]), 0, " %").changed() {
                    setup.discount = (pct / 100.0).clamp(0.0, 0.5);
                    changed = true;
                }
            });
        }
        if carried && f.cargo > 0.0 {
            bar.field("Cargo carried", "Tonnes in the hold: the jump range and speed below are for this much", |ui| {
                let mut c = setup.cargo.min(f.cargo);
                if widgets::number(ui, &mut c, 0.0..=f.cargo, widgets::Steps::By((f.cargo / 4.0).max(1.0).round()), 0, " t").changed() {
                    setup.cargo = c;
                    changed = true;
                }
            });
        }
        if carried && f.fuel > 0.0 {
            bar.field("Fuel", "Tonnes in the tanks: the jump range and speed below are for this much", |ui| {
                let mut fuel = setup.fuel.unwrap_or(f.fuel).min(f.fuel);
                if widgets::number(ui, &mut fuel, 0.0..=f.fuel, widgets::Steps::By((f.fuel / 4.0).max(1.0).round()), 0, " t").changed() {
                    setup.fuel = if (fuel - f.fuel).abs() < 1e-9 { None } else { Some(fuel) };
                    changed = true;
                }
            });
        }
    });
    changed
}

/// The game's pip panel: a column of four pips (in halves) over each of SYS, ENG and WEP, and RST. A click on a column
/// moves a pip there. True when the pips changed.
pub fn pips(ui: &mut Ui, pips: &mut Pips) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (i, (name, value)) in [("SYS", pips.sys), ("ENG", pips.eng), ("WEP", pips.wep)].into_iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(38.0, 52.0), Sense::click());
            let resp = resp.on_hover_text(format!("{name}: {value} pips. Click to move a pip here")).on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                pips.press(i);
                changed = true;
            }
            if ui.is_rect_visible(rect) {
                let p = ui.painter();
                let lit = if resp.hovered() { pal::ACCENT_SOFT } else { pal::ACCENT };
                // Four pips from the bottom up, each empty, half or full.
                for j in 0..4 {
                    let y = rect.bottom() - 16.0 - (j as f32 + 1.0) * 8.0;
                    let pip = Rect::from_min_size(egui::pos2(rect.left() + 5.0, y), Vec2::new(28.0, 6.0));
                    p.rect_filled(pip, CornerRadius::same(2), Color32::from_white_alpha(18));
                    let fill = (value - j as f64).clamp(0.0, 1.0) as f32;
                    if fill > 0.0 {
                        let lit_rect = Rect::from_min_size(pip.min, Vec2::new(pip.width() * fill, pip.height()));
                        p.rect_filled(lit_rect, CornerRadius::same(2), lit);
                    }
                }
                p.text(egui::pos2(rect.center().x, rect.bottom() - 6.0), egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(size::CAPTION), pal::DIM);
            }
        }
        if widgets::button(ui, "Reset").on_hover_text("Two pips each").clicked() && *pips != Pips::default() {
            *pips = Pips::default();
            changed = true;
        }
    });
    changed
}

// ---------------------------------------------------------------------------------------------
// Overview

/// The game's own figures for a ship of the commander's, to show beside the sums while the plan is that ship.
#[derive(Clone, Copy)]
pub struct Reported {
    pub mass: f64,
    pub range: f64,
    pub value: i64,
    pub rebuy: i64,
}

/// A change against `before` as "+1.25 ly", green when it's better; None when there's nothing to compare or no change.
fn diff(now: f64, was: Option<f64>, decimals: usize, more_is_better: bool, unit: &str) -> Option<(String, Color32)> {
    let was = was?;
    let d = now - was;
    if !d.is_finite() || d.abs() < 0.5 * 10f64.powi(-(decimals as i32)) {
        return None;
    }
    let good = (d > 0.0) == more_is_better;
    Some((format!("{}{d:.prec$}{unit}", if d > 0.0 { "+" } else { "" }, prec = decimals), if good { pal::GOOD } else { pal::BAD }))
}

/// The main figures as tiles: how each changed against `before` (the commander's ship when `owned`, else the hull as a
/// shipyard sells it), and the game's own figures for their ship (`game`) while it's unchanged.
pub fn overview(ui: &mut Ui, hull: &Hull, plan: &Plan, f: &Figures, before: Option<&Figures>, owned: bool, game: Option<Reported>) {
    let says = |what: f64, unit: &str, decimals: usize| if what > 0.0 { format!("The game reports {what:.decimals$}{unit} for this ship") } else { String::new() };
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
                tile(ui, icon::GAUGE, "Speed", pal::CYAN, &speed, diff(f.speed, before.map(|b| b.speed), 0, true, " m/s"), compared, &[boost_line(f, f.speed, f.boost), "Four pips to engines, full tank, empty hold".into()]);
            }
        });
        c.add(|ui| match f.shield {
            Some(s) => tile(ui, icon::SHIELD, "Shields", pal::BLUE, &format!("{:.0} MJ", s.strength), diff(s.strength, before.and_then(|b| b.shield).map(|b| b.strength), 0, true, " MJ"), compared, &[
                res_line(&s.res, false),
                if s.cells > 0.0 { format!("Shield cells add {:.0} MJ", s.cells) } else { String::new() },
            ]),
            None => tile(ui, icon::SHIELD, "Shields", pal::BLUE, "None", None, compared, &["No shield generator fitted, or it's switched off".into()]),
        });
        c.add(|ui| {
            tile(ui, icon::SHIELD_CHEVRON, "Armour", pal::GRAY, &format!("{:.0}", f.armour), diff(f.armour, before.map(|b| b.armour), 0, true, ""), compared, &[
                res_line(&f.hull_res, false),
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
        if !f.weapons.is_empty() {
            c.add(|ui| {
                let sdps: f64 = f.weapons.iter().map(|w| w.sustained_dps).sum();
                let was = before.map(|b| b.weapons.iter().map(|w| w.sustained_dps).sum::<f64>());
                tile(ui, icon::CROSSHAIR, "Firepower", pal::BAD, &format!("{sdps:.1} DPS"), diff(sdps, was, 1, true, ""), compared, &[
                    format!("{} weapons · {:.1} DPS while the clips last", f.weapons.len(), f.weapons.iter().map(|w| w.dps).sum::<f64>()),
                    drain_line(f),
                ]);
            });
        }
        c.add(|ui| {
            tile(ui, icon::COINS, "Cost", pal::GOLD, &short_credits(f.total_cost() as i64), diff(f.total_cost() / 1e6, before.map(|b| b.total_cost() / 1e6), 1, false, " M"), compared, &[
                format!("Hull {} · modules {}{}", short_credits(f.hull_cost as i64), short_credits(f.modules_cost as i64), if f.setup.discount > 0.0 { format!(", {:.0}\u{a0}% off", f.setup.discount * 100.0) } else { String::new() }),
                format!("Rebuy {}{}", short_credits(f.rebuy() as i64), if f.ammo_cost > 0.0 { format!(" · ammunition {}", short_credits(f.ammo_cost as i64)) } else { String::new() }),
                // What the commander paid can differ from list prices.
                game.filter(|g| g.value > 0).map(|g| format!("The game values this ship at {}, rebuy {}", short_credits(g.value), short_credits(g.rebuy))).unwrap_or_default(),
            ]);
        });
    });
}

/// "Boost 418 m/s, every 6.2 s at 2 engine pips", or why it can't, for a ship at `speed` boosting to `boost`.
fn boost_line(f: &Figures, speed: f64, boost: f64) -> String {
    if speed <= 0.0 {
        return "More than these thrusters can move".into();
    }
    if boost <= 0.0 {
        return format!("Can't boost: the power distributor's engines capacitor holds {:.0} MJ, a boost takes {:.0} MJ", f.engines_capacity, f.boost_energy);
    }
    match f.boost_interval {
        Some(t) => format!("Boost {boost:.0} m/s, every {t:.1} s at {} engine pips", pips_text(f.setup.pips.eng)),
        None => format!("Boost {boost:.0} m/s"),
    }
}

/// "How long the weapons capacitor lasts with everything firing".
fn drain_line(f: &Figures) -> String {
    match f.wep.time_to_empty {
        Some(t) => format!("All firing, the weapons capacitor lasts {t:.0} s at {} weapon pips", pips_text(f.setup.pips.wep)),
        None => format!("All firing, the weapons capacitor keeps up at {} weapon pips", pips_text(f.setup.pips.wep)),
    }
}

/// "5 s", "2.5 s".
fn seconds(s: f64) -> String {
    if (s - s.round()).abs() < 0.05 { format!("{s:.0}\u{a0}s") } else { format!("{s:.1}\u{a0}s") }
}

/// 2, 2.5.
fn pips_text(p: f64) -> String {
    if p.fract().abs() < 1e-9 { format!("{p:.0}") } else { format!("{p:.1}") }
}

/// "Kinetic 40 % · thermal -20 % · explosive 50 %" (and caustic for armour), each figure kept with its sign when the
/// line wraps.
fn res_line(r: &Resistances, caustic: bool) -> String {
    let mut s = format!("Kinetic\u{a0}{:.0}\u{a0}% · thermal\u{a0}{:.0}\u{a0}% · explosive\u{a0}{:.0}\u{a0}%", r.kinetic * 100.0, r.thermal * 100.0, r.explosive * 100.0);
    if caustic {
        s.push_str(&format!(" · caustic\u{a0}{:.0}\u{a0}%", r.caustic * 100.0));
    }
    s
}

/// Figures as rows: a name on the left, its value beside it.
fn facts(ui: &mut Ui, id: &str, rows: &[(&str, String)]) {
    egui::Grid::new(id).num_columns(2).spacing([16.0, 5.0]).max_col_width(ui.available_width()).show(ui, |ui| {
        for (k, v) in rows {
            ui.label(RichText::new(*k).color(pal::DIM));
            ui.label(v);
            ui.end_row();
        }
    });
}

/// A figure tile: icon and name in its colour, the figure big, how it changed (`compared`: against what), and a few
/// lines under it.
#[allow(clippy::too_many_arguments)]
fn tile(ui: &mut Ui, glyph: &str, title: &str, tint: Color32, value: &str, change: Option<(String, Color32)>, compared: &str, lines: &[String]) {
    card_tinted(ui, glyph, title, tint, |ui| {
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
    card_tinted(ui, icon::LIGHTNING, "Power", pal::WARN, |ui| {
        let plant = f.power_generated.max(0.01);
        let over = f.power_deployed > f.power_generated + 1e-9;
        ui.label(RichText::new(format!("{:.1} / {:.1} MW", f.power_deployed, f.power_generated)).font(display(24.0)).color(if over { pal::BAD } else { pal::TEXT }));
        let col = if over { pal::BAD } else if f.power_deployed / plant > 0.9 { pal::WARN } else { pal::GOOD };
        widgets::bar(ui, (f.power_deployed / plant) as f32, col, 8.0, None);
        ui.add(Label::new(RichText::new(format!("Deployed {:.0} % · retracted {:.0} %", f.power_deployed / plant * 100.0, f.power_retracted / plant * 100.0)).size(size::FOOTNOTE).color(pal::DIM)).wrap());
        if over {
            ui.add(Label::new(RichText::new("More than the power plant makes: the lowest priority groups switch off when you deploy hardpoints. See Power & heat.").size(size::FOOTNOTE).color(pal::BAD)).wrap());
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Defence

/// Shields and armour against each damage type, and how fast the shields come back.
pub fn defence(ui: &mut Ui, f: &Figures) {
    widgets::cards(ui, 1, 300.0, |c| {
        c.add(|ui| match f.shield {
            Some(s) => shields_card(ui, f, &s),
            None => {
                card_tinted(ui, icon::SHIELD, "Shields", pal::BLUE, |ui| {
                    ui.label(RichText::new("No shield generator fitted, or it's switched off.").color(pal::DIM));
                });
            }
        });
        c.add(|ui| armour_card(ui, f));
    });
}

fn shields_card(ui: &mut Ui, f: &Figures, s: &Shield) {
    card_tinted(ui, icon::SHIELD, "Shields", pal::BLUE, |ui| {
        ui.label(RichText::new(format!("{:.0} MJ", s.strength)).font(display(24.0)));
        let mut parts = vec![format!("generator {:.0}", s.generator)];
        if s.boosters > 0.0 {
            parts.push(format!("boosters {:.0}", s.boosters));
        }
        if s.addition > 0.0 {
            parts.push(format!("Guardian reinforcement {:.0}", s.addition));
        }
        if s.cells > 0.0 {
            parts.push(format!("and {:.0} MJ of shield cells", s.cells));
        }
        ui.add(Label::new(RichText::new(capitalise(&parts.join(" · "))).size(size::FOOTNOTE).color(pal::DIM)).wrap());
        ui.add_space(8.0);
        let sys = pips_text(f.setup.pips.sys);
        damage_grid(ui, "plan-shield-types", &[
            ("Kinetic", s.res.kinetic, s.against(s.res.kinetic)),
            ("Thermal", s.res.thermal, s.against(s.res.thermal)),
            ("Explosive", s.res.explosive, s.against(s.res.explosive)),
            ("Absolute", 0.0, s.against(0.0)),
        ], &format!("MJ to break at {sys} systems pips"));
        ui.add_space(6.0);
        let when = |t: Option<f64>| t.map_or_else(|| "never at these pips".to_string(), |t| format!("{t:.0} s"));
        ui.add(Label::new(RichText::new(format!("Regenerates {:.1} MJ/s, {:.1} MJ/s while broken", s.regen, s.broken_regen)).color(pal::TEXT)).wrap());
        ui.add(Label::new(RichText::new(format!("Half to full in {} · broken back to half in {}", when(s.recharge), when(s.recover))).color(pal::TEXT)).wrap());
        footnote(ui, &format!("At {sys} systems pips: they harden the shields by {:.0} % on top of their resistances and refill the systems capacitor the shields regenerate from. Coming back from broken takes 16 seconds before it starts.", s.sys_res * 100.0));
    });
}

fn armour_card(ui: &mut Ui, f: &Figures) {
    card_tinted(ui, icon::SHIELD_CHEVRON, "Armour", pal::GRAY, |ui| {
        ui.label(RichText::new(format!("{:.0}", f.armour)).font(display(24.0)));
        let r = f.hull_res;
        let against = |res: f64| if res < 1.0 { f.armour / (1.0 - res) } else { f64::INFINITY };
        damage_grid(ui, "plan-armour-types", &[
            ("Kinetic", r.kinetic, against(r.kinetic)),
            ("Thermal", r.thermal, against(r.thermal)),
            ("Explosive", r.explosive, against(r.explosive)),
            ("Caustic", r.caustic, against(r.caustic)),
            ("Absolute", 0.0, f.armour),
        ], "Damage to destroy the hull");
        ui.add_space(6.0);
        let mut lines = vec![format!("Hardness {:.0}: weapons that pierce less do less to the hull", f.hardness)];
        if f.module_protection > 0.0 {
            lines.push(format!("Module reinforcements take {:.0} % of the damage to modules, {:.0} in all", f.module_protection * 100.0, f.module_armour));
        }
        for l in lines {
            ui.add(Label::new(RichText::new(l).color(pal::TEXT)).wrap());
        }
    });
}

/// A small grid: damage type, resistance, and what it takes of that type.
fn damage_grid(ui: &mut Ui, id: &str, rows: &[(&str, f64, f64)], what: &str) {
    egui::Grid::new(id).num_columns(3).spacing([18.0, 4.0]).max_col_width(ui.available_width()).show(ui, |ui| {
        for t in ["Damage", "Resistance", what] {
            widgets::th(ui, t);
        }
        ui.end_row();
        for (name, res, total) in rows {
            ui.label(*name);
            ui.label(RichText::new(format!("{:.1} %", res * 100.0)).color(if *res < 0.0 { pal::BAD } else { pal::TEXT }));
            ui.label(if total.is_finite() { format!("{total:.0}") } else { "all of it".into() });
            ui.end_row();
        }
    });
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Weapons

/// Each weapon's damage, draw, heat, range and ammunition, what they come to together, and against a target (`target`:
/// what's typed to pick one; `chosen`: its name and figures; `options`: what can be picked). Returns a target picked,
/// and the name it was picked by.
pub fn weapons(ui: &mut Ui, f: &Figures, target: &mut String, chosen: Option<(&str, &Figures)>, options: &[Choice]) -> Option<(Pick, String)> {
    if f.weapons.is_empty() {
        card(ui, "Weapons", |ui| {
            ui.label(RichText::new("No weapons fitted.").color(pal::DIM));
        });
        return None;
    }
    let sdps: f64 = f.weapons.iter().map(|w| w.sustained_dps).sum();
    let dps: f64 = f.weapons.iter().map(|w| w.dps).sum();
    let share = |pick: fn(&fitting::DamageDist) -> f64| f.weapons.iter().map(|w| w.sustained_dps * pick(&w.dist)).sum::<f64>();
    let parts = [
        (share(|d| d.kinetic), pal::BLUE, "kinetic"),
        (share(|d| d.thermal), pal::WARN, "thermal"),
        (share(|d| d.explosive), pal::BAD, "explosive"),
        (share(|d| d.absolute), pal::PURPLE, "absolute"),
        (share(|d| d.caustic), pal::MINT, "caustic"),
    ];
    widgets::cards(ui, 2, 220.0, |c| {
        c.add(|ui| {
            card_tinted(ui, icon::CROSSHAIR, "Firepower", pal::BAD, |ui| {
                ui.label(RichText::new(format!("{sdps:.1} DPS")).font(display(24.0)));
                ui.add(Label::new(RichText::new(format!("Over time, reloads included; {dps:.1} DPS while the clips last")).size(size::FOOTNOTE).color(pal::DIM)).wrap());
                ui.add_space(4.0);
                let bars: Vec<(f64, Color32)> = parts.iter().filter(|p| p.0 > 0.0).map(|p| (p.0, p.1)).collect();
                widgets::share_bar(ui, &bars, 8.0);
                let text: Vec<String> = parts.iter().filter(|p| p.0 > 0.0).map(|p| format!("{} {:.0} %", p.2, p.0 / sdps.max(1e-9) * 100.0)).collect();
                ui.add(Label::new(RichText::new(capitalise(&text.join(" · "))).size(size::FOOTNOTE).color(pal::DIM)).wrap());
            });
        });
        c.add(|ui| {
            card_tinted(ui, icon::BATTERY_HIGH, "Weapons capacitor", pal::WARN, |ui| {
                let w = f.wep;
                let lasts = match w.time_to_empty {
                    Some(t) => format!("{t:.0} s"),
                    None => "Keeps up".into(),
                };
                ui.label(RichText::new(lasts).font(display(24.0)));
                ui.add(Label::new(RichText::new(format!("{:.0} MJ, refilling {:.1} MJ/s at {} weapon pips; all firing draws {:.1} MJ/s", w.capacity, w.recharge, pips_text(f.setup.pips.wep), w.drain)).size(size::FOOTNOTE).color(pal::DIM)).wrap());
            });
        });
    });
    ui.add_space(widgets::GAP);
    card(ui, "Each weapon", |ui| {
        let rows = &f.weapons;
        // A narrow window keeps four columns: the range goes under the weapon, the heat under its draw.
        let narrow = ui.available_width() < 540.0;
        let columns: Vec<Column> = if narrow {
            vec![widgets::flex_column(120.0), Column::exact(48.0), Column::exact(58.0), Column::exact(108.0).clip(true)]
        } else {
            vec![widgets::flex_column(140.0), Column::exact(50.0), Column::exact(50.0), Column::exact(44.0), Column::exact(88.0), Column::exact(120.0).clip(true)]
        };
        let heads: &[&str] = if narrow { &["Weapon", "DPS", "Draw", "Ammunition"] } else { &["Weapon", "DPS", "Draw", "Heat", "Range", "Ammunition"] };
        widgets::table(ui, "plan-weapons", |t| {
            let mut t = t.vscroll(false);
            for c in columns {
                t = t.column(c);
            }
            t.header(28.0, |mut h| {
                    for t in heads {
                        h.col(|ui| widgets::th(ui, t));
                    }
                })
                .body(|body| {
                    body.rows(44.0, rows.len(), |mut row| {
                        let Some(w) = rows.get(row.index()) else { return };
                        let two = |ui: &mut Ui, top: String, under: String| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.add(Label::new(top).truncate());
                                ui.add(Label::new(RichText::new(under).size(size::CAPTION).color(pal::DIM)).truncate());
                            })
                            .response
                        };
                        let falls = if w.range <= 0.0 {
                            String::new()
                        } else if w.falloff < w.range {
                            format!("Full damage to {:.0} m, then less and less to {:.0} m", w.falloff, w.range)
                        } else {
                            format!("Full damage to {:.0} m", w.range)
                        };
                        row.col(|ui| {
                            let under = if narrow && w.range > 0.0 { format!("{} · {:.0}\u{a0}m", w.dist.describe(), w.range) } else { w.dist.describe() };
                            two(ui, outfitting::display_name(&w.item), under).on_hover_text(format!("{} · {}{}", outfitting::display_name(&w.item), outfitting::slot_display(&w.slot), if narrow && !falls.is_empty() { format!("\n{falls}") } else { String::new() }));
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.1}", w.sustained_dps)).on_hover_text(format!("{:.1} while the clip lasts, {:.2} shots a second", w.dps, w.rate));
                        });
                        row.col(|ui| {
                            if narrow {
                                two(ui, format!("{:.2}", w.eps), format!("{:.1} heat", w.hps)).on_hover_text("Draw on the weapons capacitor, MJ a second, and heat a second, while firing");
                            } else {
                                ui.label(format!("{:.2}", w.eps));
                            }
                        });
                        if !narrow {
                            row.col(|ui| {
                                ui.label(format!("{:.1}", w.hps));
                            });
                            row.col(|ui| {
                                if w.range > 0.0 {
                                    two(ui, format!("{:.0}\u{a0}m", w.range), format!("full to {:.0}\u{a0}m", w.falloff)).on_hover_text(falls.clone());
                                } else {
                                    ui.label("—");
                                }
                            });
                        }
                        row.col(|ui| {
                            // Clip and reserve over how long they last firing and the reload; the whole of it on hover.
                            let reload = if w.reload > 0.0 { format!(" · {}\u{a0}reload", seconds(w.reload)) } else { String::new() };
                            let whole = format!("Clip {:.0}, reserve {:.0}{}{}", w.clip, w.ammo, w.time_to_empty.map(|t| format!(": {} of firing", widgets::duration(t.round() as i64))).unwrap_or_default(), if w.reload > 0.0 { format!(", reloading in {}", seconds(w.reload)) } else { String::new() });
                            let r = match w.time_to_empty {
                                Some(t) => two(ui, format!("{:.0} + {:.0}", w.clip, w.ammo), format!("{}{reload}", widgets::duration(t.round() as i64))),
                                None if w.clip > 0.0 => two(ui, "Unlimited".into(), format!("clip {:.0}{reload}", w.clip)),
                                None => ui.label("Unlimited"),
                            };
                            if w.clip > 0.0 {
                                r.on_hover_text(whole);
                            }
                        });
                    });
                });
        });
        footnote(ui, "DPS over time, reloads included; draw on the weapons capacitor in MJ a second and heat a second, while firing; the range it reaches and how far its damage stays full; clip and reserve, how long they last firing, and the reload.");
    });
    ui.add_space(widgets::GAP);
    against_card(ui, f, target, chosen, options)
}

/// "45 s", "3m 05s".
fn span(secs: f64) -> String {
    if secs < 59.5 { format!("{secs:.0}\u{a0}s") } else { widgets::duration(secs.round() as i64) }
}

/// How long the weapons take to break a target's shields and destroy its hull: a ship as a shipyard sells it, a saved
/// build or one of the commander's (`options`, picked by typing in `text`; `chosen`: its name and figures). Returns the
/// name of one picked.
fn against_card(ui: &mut Ui, f: &Figures, text: &mut String, chosen: Option<(&str, &Figures)>, options: &[Choice]) -> Option<(Pick, String)> {
    let mut picked = None;
    card(ui, "Against a target", |ui| {
        ui.add(Label::new(RichText::new("Pick a ship as a shipyard sells it, a saved build or one of yours: how long your weapons take to break its shields and destroy its hull.").color(pal::DIM)).wrap());
        ui.add_space(6.0);
        let width = (ui.available_width() - 44.0).clamp(200.0, 380.0);
        let hint = chosen.map_or("A ship: Anaconda, Python, Federal Corvette…", |(label, _)| label);
        let done = ui
            .horizontal(|ui| {
                widgets::combobox(ui, "plan-target", text, hint, width, |typed| {
                    let k = nexus_data::commodities::name_key(typed);
                    options.iter().filter(|c| k.is_empty() || nexus_data::commodities::name_key(&c.label).contains(&k)).map(|c| widgets::Suggestion { text: c.label.clone(), detail: c.detail.clone(), starred: false }).collect()
                })
            })
            .inner;
        if let Some(c) = done.picked.and_then(|name| options.iter().find(|c| c.label == name)) {
            text.clear();
            picked = Some((c.pick.clone(), c.label.clone()));
        }
        let Some((label, t)) = chosen else { return };
        let a = fitting::against(&f.weapons, t.shield.as_ref(), t.armour, &t.hull_res, t.hardness);
        ui.add_space(8.0);
        ui.label(RichText::new(label).font(display(size::CALLOUT)));
        ui.add_space(4.0);
        let shields = match (t.shield, a.shield_secs) {
            (None, _) => "None".to_string(),
            (Some(s), Some(secs)) => format!("{:.0} MJ, broken in {}", s.strength, span(secs)),
            (Some(s), None) => format!("{:.0} MJ, nothing gets through", s.strength),
        };
        let hull = match a.hull_secs {
            Some(secs) => format!("{:.0}, then destroyed in {}", t.armour, span(secs)),
            None => format!("{:.0}, nothing gets through", t.armour),
        };
        let total = match (t.shield.is_some(), a.shield_secs, a.hull_secs) {
            (false, _, Some(h)) => Some(h),
            (true, Some(s), Some(h)) => Some(s + h),
            _ => None,
        };
        facts(ui, "plan-target-facts", &[
            ("Shields", shields),
            ("On its shields", format!("{:.1} damage a second", a.shield_dps)),
            ("Hull", hull),
            ("On its hull", format!("{:.1} a second, hardness {:.0}", a.hull_dps, t.hardness)),
            ("In all", total.map_or_else(|| "Never, with these weapons".to_string(), span)),
        ]);
        let capacitor = match f.wep.time_to_empty {
            Some(secs) => format!(" Your weapons capacitor lasts {} at {} weapon pips: past that, weapons that draw on it fire slower.", span(secs), pips_text(f.setup.pips.wep)),
            None => String::new(),
        };
        footnote(ui, &format!("Every weapon firing and hitting at full damage, reloads included, against its resistances and two systems pips; its shields don't regenerate meanwhile, and shield cells aren't counted.{capacitor}"));
    });
    picked
}

// ---------------------------------------------------------------------------------------------
// Power and heat

/// A change to a module's power: its priority group or whether it's on.
pub enum PowerEdit {
    Priority(String, u8),
    On(String, bool),
    HatchPriority(u8),
    Hatch(bool),
}

/// Power by priority group, and every module's draw, group and switch; then heat. Returns a change made.
pub fn power(ui: &mut Ui, plan: &Plan, f: &Figures) -> Option<PowerEdit> {
    let mut edit = None;
    card_tinted(ui, icon::LIGHTNING, "Power by priority", pal::WARN, |ui| {
        let plant = f.power_generated.max(0.01);
        ui.add(Label::new(RichText::new(format!("The power plant makes {:.1} MW. When the ship draws more, the highest-numbered groups switch off first, so give what you can't lose a low number. Each group counts what it and the groups before it draw.", f.power_generated)).color(pal::DIM)).wrap());
        ui.add_space(6.0);
        // One row a group: its number, what it and the groups before draw deployed as a bar of the plant's output, and
        // the figures retracted and deployed.
        let bar_w = (ui.available_width() - 56.0 - 2.0 * 80.0 - 3.0 * 12.0).max(60.0);
        egui::Grid::new("plan-power-groups").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
            widgets::th(ui, "Group");
            widgets::th(ui, "Of the plant, deployed");
            widgets::th(ui, "Retracted");
            widgets::th(ui, "Deployed");
            ui.end_row();
            for (i, g) in f.power_groups.iter().enumerate() {
                let over = g.deployed > f.power_generated + 1e-9;
                let col = if over { pal::BAD } else if g.deployed / plant > 0.9 { pal::WARN } else { pal::GOOD };
                ui.label(RichText::new(format!("{}", i + 1)).color(pal::DIM));
                ui.allocate_ui(egui::vec2(bar_w, 18.0), |ui| {
                    ui.set_width(bar_w);
                    widgets::bar(ui, (g.deployed / plant).min(1.0) as f32, col, 8.0, None);
                });
                ui.label(format!("{:.1}\u{a0}MW", g.retracted));
                ui.label(RichText::new(format!("{:.1}\u{a0}MW", g.deployed)).color(if over { pal::BAD } else { pal::TEXT }));
                ui.end_row();
            }
        });
        if f.power_deployed > f.power_generated + 1e-9 {
            let first_off = f.power_groups.iter().position(|g| g.deployed > f.power_generated + 1e-9).map_or(5, |i| i + 1);
            ui.add(Label::new(RichText::new(format!("Deployed, the ship draws more than the plant makes: groups {first_off} to 5 switch off.")).color(pal::BAD)).wrap());
        }
    });
    ui.add_space(widgets::GAP);
    card(ui, "Modules", |ui| {
        let lives: Vec<(String, String, f64, u8, bool)> = std::iter::once(("CargoHatch".to_string(), "Cargo hatch".to_string(), 0.6, plan.hatch_priority, plan.hatch))
            .chain(plan.fitted.iter().filter_map(|m| {
                let l = fitting::Live::of(m)?;
                let power = l.get("power");
                (power > 0.0).then(|| (m.slot.clone(), outfitting::display_name(&m.item), power, m.priority, m.on))
            }))
            .collect();
        widgets::table(ui, "plan-power", |t| {
            t.vscroll(false)
                .column(widgets::flex_column(120.0))
                .column(Column::exact(52.0))
                .column(Column::exact(100.0))
                .column(Column::exact(58.0))
                .header(28.0, |mut h| {
                    for t in ["Module", "MW", "Group", "On"] {
                        h.col(|ui| widgets::th(ui, t));
                    }
                })
                .body(|body| {
                    body.rows(36.0, lives.len(), |mut row| {
                        let Some((slot, name, power, priority, on)) = lives.get(row.index()) else { return };
                        row.col(|ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.add(Label::new(RichText::new(name).color(if *on { pal::TEXT } else { pal::FAINT })).truncate());
                                ui.label(RichText::new(outfitting::slot_display(slot)).size(size::CAPTION).color(pal::DIM));
                            });
                        });
                        row.col(|ui| {
                            ui.label(format!("{power:.2}"));
                        });
                        row.col(|ui| {
                            let mut g = u32::from(*priority) + 1;
                            if widgets::stepper(ui, &mut g, 1..=5) {
                                let p = (g.clamp(1, 5) - 1) as u8;
                                edit = Some(if slot == "CargoHatch" { PowerEdit::HatchPriority(p) } else { PowerEdit::Priority(slot.clone(), p) });
                            }
                        });
                        row.col(|ui| {
                            // Core modules stay on: a ship without them doesn't fly.
                            if outfitting::slot_kind(slot) == outfitting::SlotKind::Core {
                                ui.label(RichText::new("Always").color(pal::FAINT)).on_hover_text("Core modules stay on");
                                return;
                            }
                            let mut v = *on;
                            if widgets::switch(ui, &mut v).changed() {
                                edit = Some(if slot == "CargoHatch" { PowerEdit::Hatch(v) } else { PowerEdit::On(slot.clone(), v) });
                            }
                        });
                    });
                });
        });
    });
    ui.add_space(widgets::GAP);
    heat_card(ui, f);
    edit
}

fn heat_card(ui: &mut Ui, f: &Figures) {
    let h = f.heat;
    card_tinted(ui, icon::THERMOMETER, "Heat", pal::BAD, |ui| {
        ui.label(RichText::new(format!("Holds {:.0}", h.capacity)).font(display(24.0)));
        let share = |x: f64| if h.capacity > 0.0 { format!(", {:.1} % of what it holds", x / h.capacity * 100.0) } else { String::new() };
        let mut lines = vec![format!("The power plant makes {:.1} heat a second retracted, {:.1} deployed (what's drawn × its heat efficiency)", h.plant_retracted, h.plant_deployed)];
        if h.weapons > 0.0 {
            lines.push(format!("Every weapon firing adds {:.1} a second{}", h.weapons, share(h.weapons)));
        }
        if h.cell > 0.0 {
            lines.push(format!("A shield cell adds {:.0} at once{}", h.cell, share(h.cell)));
        }
        if h.sinks > 0 {
            lines.push(format!("{} heat sinks to vent it", h.sinks));
        }
        for l in lines {
            ui.add(Label::new(RichText::new(l).color(pal::TEXT)).wrap());
        }
        footnote(ui, "How fast a hull sheds heat isn't in this data, so the heat level over time and the time to overheat aren't worked out.");
    });
}

// ---------------------------------------------------------------------------------------------
// Travel

/// Jump range with what's carried, speed and turning by pips, mass.
pub fn travel(ui: &mut Ui, f: &Figures) {
    widgets::cards(ui, 2, 240.0, |c| {
        c.add(|ui| {
            card_tinted(ui, icon::ARROW_FAT_RIGHT, "Jump range", pal::ACCENT, |ui| {
                ui.label(RichText::new(format!("{:.2} ly", f.range_carrying)).font(display(24.0)));
                ui.add(Label::new(RichText::new(format!("With {:.0} t of cargo and {:.1} t of fuel", f.setup.cargo.min(f.cargo), f.setup.fuel.unwrap_or(f.fuel).min(f.fuel))).size(size::FOOTNOTE).color(pal::DIM)).wrap());
                ui.add_space(6.0);
                facts(ui, "plan-jump", &[
                    ("The game's maximum", format!("{:.2} ly", f.max_range)),
                    ("Full tank, empty hold", format!("{:.2} ly", f.full_tank_range)),
                    ("Full tank, full hold", format!("{:.2} ly", f.laden_range)),
                    ("On one tank", format!("{:.0} ly", f.total_range)),
                    ("Fuel a jump, at most", format!("{:.2} t", f.max_fuel_per_jump)),
                ]);
                // One jump boosted: a neutron star's or white dwarf's cone supercharges the drive, FSD injection
                // (synthesis) boosts it; each multiplies the drive's own range with what's carried. A Guardian FSD
                // booster's light years are added after, the lower of the two ways the game may count them.
                if f.range_carrying > 0.0 {
                    let own = (f.range_carrying - f.booster).max(0.0);
                    let boosted = |k: f64| format!("{:.2} ly", own * k + f.booster);
                    let times = |k: f64| if k.fract() == 0.0 { format!("{k:.0}") } else { format!("{k}") };
                    let neutron = format!("Neutron star, ×{}", times(f.neutron_boost));
                    let white = f.white_dwarf_boost.map(|k| format!("White dwarf, ×{}", times(k)));
                    let mut rows: Vec<(&str, String)> = vec![(neutron.as_str(), boosted(f.neutron_boost))];
                    if let Some(w) = &white {
                        rows.push((w.as_str(), boosted(f.white_dwarf_boost.unwrap_or(1.0))));
                    }
                    rows.extend([("Basic FSD injection, +25\u{a0}%", boosted(1.25)), ("Standard, +50\u{a0}%", boosted(1.5)), ("Premium, +100\u{a0}%", boosted(2.0))]);
                    ui.add_space(6.0);
                    widgets::th(ui, "One jump boosted");
                    facts(ui, "plan-jump-boost", &rows);
                    let mut notes = Vec::new();
                    if f.booster > 0.0 {
                        notes.push(format!("The Guardian FSD booster's {:.2} ly are added after the boost, not multiplied with the drive's range: the lower reading, as it isn't settled here whether the game boosts them too.", f.booster));
                    }
                    if white.is_none() {
                        notes.push("How far a white dwarf supercharges this drive isn't known here.".to_string());
                    }
                    if !notes.is_empty() {
                        footnote(ui, &notes.join(" "));
                    }
                }
            });
        });
        c.add(|ui| {
            card_tinted(ui, icon::GAUGE, "Speed", pal::CYAN, |ui| {
                let eng = f.setup.pips.eng;
                let at = |p: f64| {
                    let lo = f.speed_by_pips[p.floor().clamp(0.0, 4.0) as usize];
                    let hi = f.speed_by_pips[p.ceil().clamp(0.0, 4.0) as usize];
                    lo + (hi - lo) * p.fract()
                };
                ui.label(RichText::new(if f.speed > 0.0 { format!("{:.0} m/s", at(eng)) } else { "Too heavy".into() }).font(display(24.0)));
                ui.add(Label::new(RichText::new(format!("At {} engine pips, with what's carried", pips_text(eng))).size(size::FOOTNOTE).color(pal::DIM)).wrap());
                ui.add_space(6.0);
                // Narrow columns: a grid's default least width would push six of them past the card.
                egui::Grid::new("plan-speed-pips").num_columns(6).spacing([10.0, 4.0]).min_col_width(26.0).show(ui, |ui| {
                    widgets::th(ui, "Pips");
                    for p in 0..5 {
                        widgets::th(ui, &p.to_string());
                    }
                    ui.end_row();
                    ui.label(RichText::new("m/s").color(pal::DIM));
                    for s in f.speed_by_pips {
                        ui.label(format!("{s:.0}"));
                    }
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.add(Label::new(RichText::new(boost_line(f, f.speed_by_pips[4], f.boost_carrying)).color(pal::TEXT)).wrap());
                ui.add(Label::new(RichText::new(format!("Turns: pitch {:.1}°/s, yaw {:.1}°/s, roll {:.1}°/s", f.pitch, f.yaw, f.roll)).color(pal::TEXT)).wrap());
            });
        });
        c.add(|ui| {
            card_tinted(ui, icon::SCALES, "Mass and hold", pal::MINT, |ui| {
                ui.label(RichText::new(format!("{:.1} t", f.unladen_mass)).font(display(24.0)));
                ui.add(Label::new(RichText::new("Unladen: hull and modules, no fuel or cargo").size(size::FOOTNOTE).color(pal::DIM)).wrap());
                ui.add_space(6.0);
                facts(ui, "plan-mass", &[
                    ("The hull alone", format!("{:.1} t", f.hull_mass)),
                    ("Laden, full tank and hold", format!("{:.1} t", f.unladen_mass + f.fuel + f.cargo)),
                    ("Cargo", format!("{:.0} t", f.cargo)),
                    ("Fuel", format!("{:.0} t", f.fuel)),
                    ("Passengers", format!("{:.0}", f.passengers)),
                    ("Crew seats", format!("{}", f.crew)),
                    ("Mass lock factor", format!("{:.0}", f.masslock)),
                ]);
            });
        });
    });
}
