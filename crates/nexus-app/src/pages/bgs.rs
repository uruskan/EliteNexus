//! BGS contributions per system/faction and Powerplay merits (OD Elite Tracker style).

use crate::theme::{display, pal};
use crate::widgets::{card, card_with, empty, short_credits};
use egui::{Color32, Grid, RichText, Ui};
use egui_phosphor::regular as icon;
use egui_plot::{Bar, BarChart, Plot};
use nexus_data::names::group_digits;
use nexus_journal::time::LocalDays;
use nexus_journal::Timestamp;
use nexus_model::bgs::{last_tick, BgsKind, FactionSummary, SystemSummary};
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
    let first = gs.bgs.actions.iter().map(|a| a.ts).chain(gs.bgs.merits.iter().map(|m| m.ts)).min();
    ui.label(RichText::new(counting(st.period, since, first)).small().color(pal::DIM));
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
            factions(ui, sys);
        });
        ui.add_space(6.0);
    }
    changed
}

/// A system's factions: a table when it fits the card, and when it doesn't, each faction with its activity on the
/// lines under its name (the table, with a column for every kind of activity, ran past a 1,000-pixel window). Only
/// the kinds of activity the system saw in the period get a column.
fn factions(ui: &mut Ui, sys: &SystemSummary) {
    let kinds: Vec<BgsKind> = COLUMNS.into_iter().filter(|k| sys.factions.iter().any(|f| cell(f, *k).is_some())).collect();
    const GAP: f32 = 14.0;
    let (head, body) = (egui::TextStyle::Small.resolve(ui.style()), egui::TextStyle::Body.resolve(ui.style()));
    let width = |text: &str, font: &egui::FontId| ui.painter().layout_no_wrap(text.to_owned(), font.clone(), pal::TEXT).size().x;
    let names = sys.factions.iter().map(|f| width(&f.faction, &body)).fold(width("Faction", &body), f32::max);
    let columns: f32 =
        kinds.iter().map(|k| sys.factions.iter().filter_map(|f| cell(f, *k)).map(|(t, _)| width(&t, &display(13.0))).fold(width(k.label(), &head), f32::max)).sum();
    if names + columns + GAP * kinds.len() as f32 <= ui.available_width() {
        Grid::new(("bgs", &sys.system)).num_columns(kinds.len() + 1).striped(true).spacing([GAP, 4.0]).show(ui, |ui| {
            ui.label(RichText::new("Faction").color(pal::DIM));
            for k in &kinds {
                ui.label(RichText::new(k.label()).small().color(pal::DIM));
            }
            ui.end_row();
            for f in &sys.factions {
                ui.label(RichText::new(&f.faction).color(pal::TEXT));
                for k in &kinds {
                    match cell(f, *k) {
                        Some((text, col)) => ui.label(RichText::new(text).font(display(13.0)).color(col)),
                        None => ui.label(RichText::new("·").color(pal::FAINT)),
                    };
                }
                ui.end_row();
            }
        });
        return;
    }
    for (i, f) in sys.factions.iter().enumerate() {
        if i > 0 {
            ui.separator();
        }
        ui.label(RichText::new(&f.faction).color(pal::TEXT));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(GAP, 2.0);
            for k in &kinds {
                if let Some((text, col)) = cell(f, *k) {
                    // One label, so a kind and its amount move to the next line together.
                    let format = |font_id: egui::FontId, color| egui::TextFormat { font_id, color, valign: egui::Align::Center, ..Default::default() };
                    let mut job = egui::text::LayoutJob::default();
                    job.append(k.label(), 0.0, format(head.clone(), pal::DIM));
                    job.append(&text, 6.0, format(display(13.0), col));
                    ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
                }
            }
        });
    }
}

/// What a faction's entry for `k` says, in its colour; None when it had none of that kind.
fn cell(f: &FactionSummary, k: BgsKind) -> Option<(String, Color32)> {
    let (_, amount, n) = f.kinds.iter().find(|(kind, _, _)| *kind == k)?;
    let text = match k {
        BgsKind::MissionInfluence => format!("{}{amount} ({n})", if *amount >= 0 { "+" } else { "" }),
        BgsKind::MissionFailed | BgsKind::Murder | BgsKind::Crime => n.to_string(),
        _ => short_credits(*amount),
    };
    Some((text, if k.positive() { pal::GOOD } else { pal::BAD }))
}

/// What the period counts, under its buttons. "All time" has no start of its own (the Unix epoch it counts from read
/// "01 Jan 02:00"): it counts every journal, and `first` is the oldest activity in them.
fn counting(period: Period, since: Timestamp, first: Option<Timestamp>) -> String {
    match (period, first) {
        (Period::All, Some(first)) => format!("Counting all your journals (first activity {})", first.to_local_string("%d %b %Y")),
        (Period::All, None) => "Counting all your journals".into(),
        _ => format!("Counting from {}", since.to_local_string("%d %b %H:%M")),
    }
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
                .show(ui,|p| p.bar_chart(BarChart::new("merits", bars).element_formatter(Box::new(|_, _| String::new()))));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_time_counts_every_journal() {
        // "All time" counts from the Unix epoch, which in local time read "01 Jan 02:00" (or 31 Dec, west of UTC).
        let first = Timestamp::parse("2025-03-14T12:00:00Z").unwrap();
        let all = counting(Period::All, Timestamp::ZERO, Some(first));
        assert_eq!(all, format!("Counting all your journals (first activity {})", first.to_local_string("%d %b %Y")));
        assert!(all.ends_with("2025)"), "{all}");
        assert_eq!(counting(Period::All, Timestamp::ZERO, None), "Counting all your journals");
        // The other periods start at their own time.
        let tick = Timestamp::parse("2026-11-03T13:00:00Z").unwrap();
        assert_eq!(counting(Period::SinceTick, tick, Some(first)), format!("Counting from {}", tick.to_local_string("%d %b %H:%M")));
    }
}
