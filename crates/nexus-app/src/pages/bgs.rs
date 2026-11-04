//! BGS contributions per system/faction and Powerplay merits (OD Elite Tracker style).

use crate::theme::{display, pal};
use crate::widgets::{card, card_with, empty, short_credits};
use egui::{Grid, RichText, Ui};
use egui_phosphor::regular as icon;
use egui_plot::{Bar, BarChart, Plot};
use nexus_data::names::group_digits;
use nexus_journal::time::LocalDays;
use nexus_journal::Timestamp;
use nexus_model::bgs::{last_tick, BgsKind};
use nexus_model::GameState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Period {
    #[default]
    SinceTick,
    Day,
    Week,
    All,
}

/// The BGS page's choices, kept in settings.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BgsPrefs {
    pub period: Period,
    /// The hour (UTC) of the commander's BGS tick, for "Since last tick".
    pub tick_hour_utc: u32,
}

impl Default for BgsPrefs {
    fn default() -> Self {
        BgsPrefs { period: Period::SinceTick, tick_hour_utc: 13 }
    }
}

const COLUMNS: [BgsKind; 9] = [
    BgsKind::MissionInfluence,
    BgsKind::Bounty,
    BgsKind::CombatBond,
    BgsKind::Trade,
    BgsKind::Exploration,
    BgsKind::Exobiology,
    BgsKind::BlackMarket,
    BgsKind::MissionFailed,
    BgsKind::Murder,
];

/// Returns true when `st` changed (to be saved).
pub fn show(ui: &mut Ui, gs: &GameState, st: &mut BgsPrefs) -> bool {
    let before = *st;
    let now = Timestamp::now();
    ui.horizontal(|ui| {
        crate::widgets::segmented(ui, &mut st.period, &[(Period::SinceTick, "Since last tick"), (Period::Day, "Last 24 h"), (Period::Week, "7 days"), (Period::All, "All time")]);
        if st.period == Period::SinceTick {
            ui.label(RichText::new("tick at").color(pal::DIM));
            ui.add(egui::DragValue::new(&mut st.tick_hour_utc).range(0..=23).suffix(":00 UTC"));
        }
    });
    let changed = *st != before;
    let since = match st.period {
        Period::SinceTick => last_tick(now, st.tick_hour_utc),
        Period::Day => Timestamp(now.0 - 86_400),
        Period::Week => Timestamp(now.0 - 7 * 86_400),
        Period::All => Timestamp::ZERO,
    };
    ui.label(RichText::new(format!("Counting from {}", since.to_local_string("%d %b %H:%M"))).small().color(pal::DIM));
    ui.add_space(6.0);

    powerplay(ui, gs, since);
    ui.add_space(8.0);

    let summary = gs.bgs.summary(since);
    if summary.is_empty() {
        empty(ui, icon::SCALES, "No BGS activity in this period", "Missions, bounties, bonds, trade and data sales are attributed to the factions they affect, and Powerplay merits to the system you were in.");
        return changed;
    }
    for sys in &summary {
        let title = if sys.system.is_empty() { "Unknown system" } else { sys.system.as_str() };
        card_with(ui, title, |ui| merits_here(ui, sys.merits), |ui| {
            if sys.factions.is_empty() {
                ui.label(RichText::new("No faction activity recorded here in this period.").color(pal::DIM));
                return;
            }
            Grid::new(("bgs", &sys.system)).num_columns(COLUMNS.len() + 1).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("Faction").color(pal::DIM));
                for k in COLUMNS {
                    ui.label(RichText::new(k.label()).small().color(pal::DIM));
                }
                ui.end_row();
                for f in &sys.factions {
                    ui.label(RichText::new(&f.faction).color(pal::TEXT));
                    for k in COLUMNS {
                        match f.kinds.iter().find(|(kk, _, _)| *kk == k) {
                            Some((_, amount, n)) => {
                                let text = match k {
                                    BgsKind::MissionInfluence => format!("{}{} ({n})", if *amount >= 0 { "+" } else { "" }, amount),
                                    BgsKind::MissionFailed | BgsKind::Murder | BgsKind::Crime => n.to_string(),
                                    _ => short_credits(*amount),
                                };
                                let col = if k.positive() { pal::GOOD } else { pal::BAD };
                                ui.label(RichText::new(text).font(display(13.0)).color(col));
                            }
                            None => {
                                ui.label(RichText::new("·").color(pal::FAINT));
                            }
                        }
                    }
                    ui.end_row();
                }
            });
        });
        ui.add_space(6.0);
    }
    changed
}

/// A system's merits, in its card's title row.
fn merits_here(ui: &mut Ui, merits: i64) {
    if merits == 0 {
        return;
    }
    ui.label(RichText::new(format!("{} {} merits", icon::CROWN, group_digits(merits))).font(display(14.0)).color(pal::PURPLE))
        .on_hover_text("Powerplay merits the game awarded while you were in this system. The journal doesn't say which system an award counts for, so Elite Nexus places it where you were.");
}

fn powerplay(ui: &mut Ui, gs: &GameState, since: Timestamp) {
    card(ui, "Powerplay", |ui| {
        let Some(pp) = &gs.cmdr.powerplay else {
            ui.label(RichText::new("Not pledged to a power.").color(pal::DIM));
            return;
        };
        ui.horizontal(|ui| {
            crate::widgets::stat(ui, "power", pp.power.clone(), pal::PURPLE);
            ui.add_space(16.0);
            crate::widgets::stat(ui, "rank", pp.rank.to_string(), pal::TEXT);
            ui.add_space(16.0);
            crate::widgets::stat(ui, "total merits", group_digits(pp.merits), pal::GOLD);
            ui.add_space(16.0);
            crate::widgets::stat(ui, "merits this period", group_digits(gs.bgs.merits_since(since)), pal::ACCENT);
            ui.add_space(16.0);
            crate::widgets::stat(ui, "pledged", crate::widgets::duration(pp.time_pledged_secs), pal::DIM);
        });
        if gs.bgs.merits.len() > 1 {
            // Days are the commander's calendar days (local time), for the bars, the axis and the tooltip alike.
            let mut calendar = LocalDays::default();
            let mut per_day: std::collections::BTreeMap<i64, i64> = Default::default();
            for m in &gs.bgs.merits {
                *per_day.entry(calendar.day(m.ts)).or_default() += m.gained;
            }
            // Whole days, so their dates are on the axis however few there are.
            let first = per_day.keys().next().copied().unwrap_or_default();
            let last = per_day.keys().next_back().copied().unwrap_or_default();
            let bars: Vec<Bar> = per_day.iter().map(|(d, m)| Bar::new(*d as f64 + 0.5, *m as f64).width(0.8).fill(pal::PURPLE)).collect();
            let plot = Plot::new("merits")
                .show_background(false)
                .show_grid([false, true])
                .height(140.0)
                .allow_scroll(false)
                .allow_drag(false)
                .allow_zoom(false)
                .include_x(first as f64)
                .include_x(last as f64 + 1.0)
                .include_y(0.0)
                .x_axis_formatter(crate::widgets::day_axis)
                .show(ui, |p| p.bar_chart(BarChart::new("merits", bars).element_formatter(Box::new(|_, _| String::new()))));
            crate::widgets::bar_tooltip(&plot, |day| {
                let date = LocalDays::label(day, "%d %b");
                Some(match per_day.get(&day) {
                    Some(m) => format!("{date}\n{} merits", group_digits(*m)),
                    None => format!("{date}\nno merits"),
                })
            });
        }
        if gs.bgs.merits_since(since) != 0 {
            crate::widgets::footnote(ui, "Each system below shows the merits you earned there. The journal doesn't say which system a merit award counts for, so it's placed in the system you were in when the game awarded it.");
        }
    });
}
