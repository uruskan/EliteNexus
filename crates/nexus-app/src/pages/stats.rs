//! Statistics: credits, income and spending, and jumps over a period the commander picks, plus the
//! game's own Statistics event. Every chart fits the period shown, and amounts read as "19.1 B".

use crate::theme::{display, pal, size};
use crate::widgets::{card, short_credits};
use egui::{Grid, RichText, Ui};
use egui_plot::{Bar, BarChart, GridInput, GridMark, Line, Plot, PlotPoints};
use nexus_journal::time::LocalDays;
use nexus_journal::Timestamp;
use nexus_model::GameState;
use nexus_model::history::LedgerCategory;

/// The periods the page can show: (days, label); 0 = all time.
const RANGES: [(u32, &str); 4] = [(7, "7 days"), (30, "30 days"), (90, "90 days"), (0, "All time")];

/// Where the period starts (unix seconds): `days` back from `now`, or the first record.
fn range_start(days: u32, now: i64, first: Option<i64>) -> i64 {
    if days == 0 { first.unwrap_or(now) } else { now - i64::from(days) * 86_400 }
}

/// The balance as a step line over [`start`, `now`], as (unix seconds, credits): it starts at the balance held when
/// the period began (the last sample before it) and runs to today at `current`.
pub(crate) fn credit_steps(samples: &[(Timestamp, i64)], start: i64, now: i64, current: i64) -> Vec<(i64, i64)> {
    let mut pts: Vec<(i64, i64)> = Vec::new();
    let before = samples.iter().rev().find(|(t, _)| t.0 < start).map(|(_, v)| *v);
    let mut last = before;
    if let Some(v) = before {
        pts.push((start, v));
    }
    for (t, v) in samples.iter().filter(|(t, _)| t.0 >= start && t.0 <= now) {
        if let Some(prev) = last {
            pts.push((t.0, prev));
        }
        pts.push((t.0, *v));
        last = Some(*v);
    }
    if let Some(prev) = last {
        pts.push((now, prev));
        if prev != current {
            pts.push((now, current));
        }
    }
    pts
}

/// The y range the Credits chart shows: its line with a little room above and below, so it isn't drawn on the edge.
/// The labels keep three figures ("1.23 B"): a balance that didn't move spanned a tenth of a credit, and every label
/// read "1.23 B". So a flat or nearly flat line gets a band of a tenth of its value above and below it, which labels of
/// two figures ("1.2 M", "12 k", the coarsest they get) still tell apart. On the log scale the line holds powers of
/// ten, and a tenth of one is room enough.
fn credit_y_range(lo: f64, hi: f64, log: bool) -> (f64, f64) {
    if log {
        return (lo - 0.1, hi + 0.1);
    }
    let span = ((hi - lo) * 1.1).max(lo.abs().max(hi.abs()) * 0.2).max(20.0);
    let mid = (lo + hi) / 2.0;
    (mid - span / 2.0, mid + span / 2.0)
}

/// The finest step the Credits chart's labels (`compact_credits`) tell apart up to `v`: 0.01 B among billions, 0.1 M
/// among millions, 1 k from 10 k, single credits below that.
fn credit_resolution(v: f64) -> f64 {
    match v.abs() {
        a if a >= 1e9 => 1e7,
        a if a >= 1e6 => 1e5,
        a if a >= 1e4 => 1e3,
        _ => 1.0,
    }
}

/// The marks up the Credits chart's side, only within `band` (its y range: a label in the room egui_plot adds around
/// it sits on the chart's edge and runs into the dates), at steps its labels tell apart.
fn credit_marks(band: (f64, f64), log: bool) -> impl Fn(GridInput) -> Vec<GridMark> {
    move |mut input| {
        input.bounds = (input.bounds.0.max(band.0), input.bounds.1.min(band.1));
        if log { log_credit_marks(&input) } else { crate::widgets::round_marks(credit_resolution(band.0.abs().max(band.1.abs())))(input) }
    }
}

/// Marks on the log scale, where the chart holds log10 of the balance (labelling whole powers of ten only left a
/// balance between 2 and 5 billion with no label at all): powers of ten, then 2 and 5 times one, then 3, 4, 6, 7, 8
/// and 9 times one, each where it keeps its gap from those already there. Within less than a power of ten, a round
/// step in credits takes over where it gives as many: the finest whose top two marks, the closest on this scale, keep
/// the gap.
fn log_credit_marks(input: &GridInput) -> Vec<GridMark> {
    use crate::widgets::{marks_every, round_step, value_per_point, LABEL_GAP};
    let gap = value_per_point(input) * LABEL_GAP;
    let (lo, hi) = input.bounds;
    if !(gap > 0.0 && lo <= hi && hi - lo < 1_000.0) {
        return Vec::new();
    }
    let mut values: Vec<f64> = Vec::new();
    for tier in [&[1.0][..], &[2.0, 5.0], &[3.0, 4.0, 6.0, 7.0, 8.0, 9.0]] {
        // From 1 Cr up: the line holds nothing below.
        for power in (lo.floor() as i64).max(0)..=hi.ceil() as i64 {
            for m in tier {
                let v = power as f64 + f64::log10(*m);
                if (lo..=hi).contains(&v) && values.iter().all(|c| (c - v).abs() >= gap) {
                    values.push(v);
                }
            }
        }
    }
    if hi - lo < 1.0 {
        let top = 10f64.powf(hi);
        let step = round_step(top * (1.0 - 10f64.powf(-gap)), credit_resolution(top));
        let even: Vec<f64> = marks_every((10f64.powf(lo), top), step).into_iter().filter(|m| m.value >= 1.0).map(|m| m.value.log10()).collect();
        if even.len() >= values.len() {
            values = even;
        }
    }
    values.sort_by(f64::total_cmp);
    values.into_iter().map(|value| GridMark { value, step_size: gap }).collect()
}

pub fn show(ui: &mut Ui, gs: &GameState) {
    let h = &gs.history;
    let now = Timestamp::now().0;
    // The period, remembered between runs.
    let id = ui.make_persistent_id("stats-range");
    let mut days: u32 = ui.data_mut(|d| *d.get_persisted_mut_or(id, 30u32));
    if crate::widgets::segmented(ui, &mut days, &RANGES) {
        ui.data_mut(|d| d.insert_persisted(id, days));
    }
    let first = [h.credit_samples.first().map(|(t, _)| t.0), h.ledger.first().map(|e| e.ts.0), h.jumps.first().map(|j| j.ts.0)].into_iter().flatten().min();
    let start = range_start(days, now, first);
    let since = Timestamp(start);
    let totals: Vec<(LedgerCategory, i64, i64)> = {
        // Money moved between the commander and their own carrier is neither earned nor spent.
        let mut t: Vec<_> = h.totals_since(since).into_iter().filter(|(c, i, e)| *c != LedgerCategory::Transfer && (*i != 0 || *e != 0)).collect();
        t.sort_by_key(|(_, i, e)| std::cmp::Reverse(i + e.abs()));
        t
    };
    let moved: i64 = h.totals_since(since).iter().filter(|(c, _, _)| *c == LedgerCategory::Transfer).map(|(_, i, e)| i + e).sum();
    let earned: i64 = totals.iter().map(|(_, i, _)| i).sum();
    let spent: i64 = totals.iter().map(|(_, _, e)| e).sum();
    let jumps: Vec<_> = h.jumps.iter().filter(|j| j.ts.0 >= start).collect();
    let ly: f64 = jumps.iter().map(|j| j.dist).sum();
    let deaths = h.deaths.iter().filter(|d| d.ts.0 >= start).count();
    let steps = credit_steps(&h.credit_samples, start, now, gs.cmdr.credits);
    let change = match (steps.first(), steps.last()) {
        (Some(a), Some(b)) => Some(b.1 - a.1),
        _ => None,
    };

    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(32.0, 8.0);
        crate::widgets::stat(ui, "Credits now", short_credits(gs.cmdr.credits), pal::GOLD);
        if let Some(c) = change {
            let sign = if c > 0 { "+" } else { "" };
            crate::widgets::stat(ui, "Change in this period", format!("{sign}{}", short_credits(c)), if c >= 0 { pal::GOOD } else { pal::BAD });
        }
        crate::widgets::stat(ui, "Earned", format!("+{}", short_credits(earned)), pal::GOOD);
        crate::widgets::stat(ui, "Spent", short_credits(spent), pal::BAD);
        crate::widgets::stat(ui, "Jumps", format!("{} · {:.0} ly", jumps.len(), ly), pal::CYAN);
        crate::widgets::stat(ui, "Deaths", deaths.to_string(), if deaths > 0 { pal::BAD } else { pal::TEXT });
    });
    ui.add_space(10.0);
    crate::widgets::cards(ui, 2, 360.0, |c| {
        c.add(|ui| {
            // A log scale shows the hundreds of millions next to the billions; remembered between runs.
            let log_id = ui.make_persistent_id("stats-credits-log");
            // Both the title row (the switch) and the chart read it: a Cell lets the switch change it first.
            let log = std::cell::Cell::new(ui.data_mut(|d| *d.get_persisted_mut_or(log_id, false)));
            crate::widgets::card_with(
                ui,
                "Credits",
                |ui| {
                    let mut on = log.get();
                    if crate::widgets::toggle(ui, &mut on, "Log scale").on_hover_text("Each step up the axis is ten times more: small and large balances both show").changed() {
                        log.set(on);
                        ui.data_mut(|d| d.insert_persisted(log_id, on));
                    }
                },
                |ui| {
                    if steps.len() < 2 {
                        ui.label(RichText::new("Not enough data in this period.").color(pal::DIM));
                        return;
                    }
                    let log = log.get();
                    // On the log scale the line holds log10 of the balance (below 1 Cr counts as 1 Cr).
                    let y = |v: i64| if log { (v.max(1) as f64).log10() } else { v as f64 };
                    // Along the commander's calendar days, as Jumps per day: each midnight falls under its date. (Unix
                    // time over 86,400 put UTC midnight there, which west of UTC is the evening before that date.)
                    let mut calendar = LocalDays::default();
                    let shown: Vec<[f64; 2]> = steps.iter().map(|&(t, v)| [calendar.at(Timestamp(t)), y(v)]).collect();
                    let (lo, hi) = shown.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
                    let (bottom, top) = credit_y_range(lo, hi, log);
                    let credits = move |v: f64| if log { 10f64.powf(v) } else { v };
                    Plot::new(("credits", days, log))
                        .show_background(false)
                        .show_grid([false, true])
                        .height(240.0)
                        .allow_scroll(false)
                        .allow_drag(false)
                        .allow_zoom(false)
                        .include_x(calendar.at(since))
                        .include_x(calendar.at(Timestamp(now)))
                        .include_y(bottom)
                        .include_y(top)
                        .x_axis_formatter(crate::widgets::day_axis)
                        .y_grid_spacer(credit_marks((bottom, top), log))
                        .y_axis_formatter(move |m, _| nexus_data::names::compact_credits(credits(m.value).round() as i64))
                        .label_formatter(move |h| {
                            let (x, v) = crate::widgets::hover_xy(h);
                            Some(format!("{}\n{} Cr", LocalDays::instant(x).to_local_string("%d %b %H:%M"), nexus_data::names::group_digits(credits(v).round() as i64)))
                        })
                        .show(ui, |p| {
                            p.line(Line::new("Credits", PlotPoints::from(shown)).color(pal::GOLD).width(2.0));
                        });
                },
            );
        });
        c.add(|ui| {
            card(ui, "Income and spending", |ui| ledger_rows(ui, &totals, earned, spent, moved));
        });
    });
    ui.add_space(10.0);
    card(ui, "Jumps per day", |ui| {
        // Days are the commander's calendar days (local time), for the bars, the axis and the tooltip alike.
        let mut calendar = LocalDays::default();
        let mut per_day: std::collections::BTreeMap<i64, (u32, f64)> = Default::default();
        for j in &jumps {
            let e = per_day.entry(calendar.day(j.ts)).or_default();
            e.0 += 1;
            e.1 += j.dist;
        }
        if per_day.is_empty() {
            ui.label(RichText::new("No jumps in this period.").color(pal::DIM));
            return;
        }
        let bars: Vec<Bar> = per_day.iter().map(|(d, (n, _))| Bar::new(*d as f64 + 0.5, f64::from(*n)).width(0.8).fill(pal::CYAN)).collect();
        let plot = Plot::new(("jumpsday", days))
            .show_background(false)
            .show_grid([false, true])
            .height(200.0)
            .allow_scroll(false)
            .allow_drag(false)
            .allow_zoom(false)
            .include_x(calendar.day(since) as f64)
            .include_x(calendar.day(Timestamp(now)) as f64 + 1.0)
            .include_y(0.0)
            .x_axis_formatter(crate::widgets::day_axis)
            .y_grid_spacer(crate::widgets::round_marks(1.0))
            .y_axis_formatter(|m, _| if m.value.fract() == 0.0 && m.value >= 0.0 { format!("{}", m.value as i64) } else { String::new() })
            .show(ui, |p| p.bar_chart(BarChart::new("Jumps", bars).element_formatter(Box::new(|_, _| String::new()))));
        crate::widgets::bar_tooltip(&plot, |day| {
            let date = LocalDays::label(day, "%d %b");
            Some(match per_day.get(&day) {
                Some((n, ly)) => format!("{date}\n{n} {} · {ly:.0} ly", if *n == 1 { "jump" } else { "jumps" }),
                None => format!("{date}\nno jumps"),
            })
        });
    });
    if let Some(stats) = &gs.cmdr.statistics {
        ui.add_space(10.0);
        card(ui, "In-game statistics (last Statistics event)", |ui| {
            crate::widgets::cards(ui, 3, 260.0, |c| {
                let sections: [(&str, &serde_json::Value); 9] = [
                    ("Bank account", &stats.bank_account),
                    ("Combat", &stats.combat),
                    ("Exploration", &stats.exploration),
                    ("Trading", &stats.trading),
                    ("Mining", &stats.mining),
                    ("Exobiology", &stats.exobiology),
                    ("Crafting", &stats.crafting),
                    ("Passengers", &stats.passengers),
                    ("Fleet carrier", &stats.fleetcarrier),
                ];
                for (i, (title, v)) in sections.iter().enumerate() {
                    c.add(|ui| {
                    egui::CollapsingHeader::new(RichText::new(*title).font(display(size::CALLOUT)).color(pal::TEXT)).id_salt(("stat", i)).show(ui, |ui| {
                        if let Some(obj) = v.as_object() {
                            Grid::new(("statgrid", i)).max_col_width(ui.available_width()).num_columns(2).striped(true).show(ui, |ui| {
                                for (k, val) in obj {
                                    ui.label(RichText::new(k.replace('_', " ")).small().color(pal::DIM));
                                    let txt = match val {
                                        serde_json::Value::Number(n) => n.as_i64().map(nexus_data::names::group_digits).unwrap_or_else(|| n.to_string()),
                                        other => other.to_string(),
                                    };
                                    ui.label(RichText::new(txt).small());
                                    ui.end_row();
                                }
                            });
                        }
                    });
                    });
                }
            });
        });
    }
}

/// A fixed-width cell of a row, its content on the left or (numbers) on the right.
fn cell(ui: &mut Ui, width: f32, right: bool, add: impl FnOnce(&mut Ui)) {
    let layout = if right { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
    ui.allocate_ui_with_layout(egui::vec2(width, 30.0), layout, |ui| {
        ui.set_min_size(egui::vec2(width, 30.0));
        add(ui);
    });
}

/// One row per category, biggest first: its income and its spending as bars against the largest
/// amount of the period, each with its value written out (a 30 B category doesn't flatten a 5 M one
/// into nothing: its number is still there).
fn ledger_rows(ui: &mut Ui, totals: &[(LedgerCategory, i64, i64)], earned: i64, spent: i64, moved: i64) {
    if totals.is_empty() {
        ui.label(RichText::new("Nothing earned or spent in this period.").color(pal::DIM));
        return;
    }
    let max = totals.iter().map(|(_, i, e)| (*i).max(e.abs())).max().unwrap_or(1).max(1) as f32;
    const LABEL: f32 = 112.0;
    const VALUE: f32 = 86.0;
    for (cat, inc, exp) in totals {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            cell(ui, LABEL, false, |ui| {
                ui.add(egui::Label::new(RichText::new(cat.label()).color(pal::TEXT)).truncate());
            });
            let bar_w = (ui.available_width() - 2.0 * VALUE - 16.0).max(40.0);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(bar_w, 30.0), egui::Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect.shrink2(egui::vec2(0.0, 6.0)), egui::CornerRadius::same(3), egui::Color32::from_white_alpha(8));
            // Income above the middle, spending below; anything above zero stays visible.
            for (amount, colour, top) in [(*inc, pal::GOOD, rect.top() + 6.0), (exp.abs(), pal::BAD, rect.center().y + 1.0)] {
                if amount > 0 {
                    let w = (bar_w * amount as f32 / max).max(2.0);
                    p.rect_filled(egui::Rect::from_min_size(egui::pos2(rect.left(), top), egui::vec2(w, 8.0)), egui::CornerRadius::same(2), colour);
                }
            }
            resp.on_hover_text(format!("{}: earned {} Cr, spent {} Cr", cat.label(), nexus_data::names::group_digits(*inc), nexus_data::names::group_digits(exp.abs())));
            let value = |ui: &mut Ui, v: i64, colour, sign: &str| {
                let text = if v == 0 { String::new() } else { format!("{sign}{}", short_credits(v.abs())) };
                cell(ui, VALUE, true, |ui| {
                    ui.label(RichText::new(text).font(display(size::CALLOUT)).color(colour));
                });
            };
            value(ui, *inc, pal::GOOD, "+");
            value(ui, *exp, pal::BAD, "-");
        });
    }
    ui.add_space(4.0);
    let net = earned + spent;
    ui.horizontal(|ui| {
        cell(ui, LABEL, false, |ui| {
            ui.label(RichText::new("Net").font(display(size::CALLOUT)).color(pal::TEXT));
        });
        let sign = if net > 0 { "+" } else { "" };
        ui.label(RichText::new(format!("{sign}{}", short_credits(net))).font(display(size::CALLOUT)).color(if net >= 0 { pal::GOOD } else { pal::BAD }));
    });
    if moved != 0 {
        let what = if moved < 0 { format!("{} moved to your fleet carrier", short_credits(-moved)) } else { format!("{} taken from your fleet carrier", short_credits(moved)) };
        ui.label(RichText::new(format!("{what} (not counted: it stays yours)")).size(size::FOOTNOTE).color(pal::DIM));
    }
    crate::widgets::footnote(
        ui,
        "From the journal: sales, rewards, purchases and fees, while Elite Nexus has read them. A mission a wingmate shared and handed in isn't among them (the game writes no journal line for it), but its credits are in your balance.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_credit_line_steps_through_the_period() {
        let d = 86_400;
        let samples = [(Timestamp(10 * d), 100), (Timestamp(20 * d), 300), (Timestamp(25 * d), 200)];
        // From day 15: starts at the balance held then (100), steps at days 20 and 25, runs to today.
        let pts = credit_steps(&samples, 15 * d, 30 * d, 250);
        let days: Vec<(i64, i64)> = pts.iter().map(|&(t, v)| (t / d, v)).collect();
        assert_eq!(days, vec![(15, 100), (20, 100), (20, 300), (25, 300), (25, 200), (30, 200), (30, 250)]);
        // All time: from the first sample.
        let pts = credit_steps(&samples, 10 * d, 30 * d, 200);
        assert_eq!(pts.first(), Some(&(10 * d, 100)));
        assert_eq!(pts.last(), Some(&(30 * d, 200)));
        // Nothing before or in the period: nothing to draw.
        assert!(credit_steps(&[], 0, 30 * d, 5).is_empty());
        assert_eq!(range_start(7, 100 * d, Some(3)), 93 * d);
        assert_eq!(range_start(0, 100 * d, Some(3)), 3);
    }

    /// The labels up the Credits chart, `height` points tall, for a line from `lo` to `hi` (log10 of the balance on the
    /// log scale): egui_plot adds 5 % of room above and below the chart's range, and labels the marks it's given. The
    /// chart is 240 points tall with its dates, about 223 without.
    fn labels((lo, hi): (f64, f64), log: bool, height: f64) -> Vec<String> {
        let band = credit_y_range(lo, hi, log);
        let room = (band.1 - band.0) * 0.05;
        let input = || crate::widgets::grid_input((band.0 - room, band.1 + room), height);
        let credits = |v: f64| if log { 10f64.powf(v) } else { v };
        crate::widgets::labelled(credit_marks(band, log)(input()), &input()).into_iter().map(|v| nexus_data::names::compact_credits(credits(v).round() as i64)).collect()
    }

    #[test]
    fn a_flat_balance_gets_labels_that_differ() {
        // What a flat balance used to get: a tenth of a credit, where egui_plot's own marks wrote "1.23 B" on every line.
        let input = || crate::widgets::grid_input((1_234_567_889.945, 1_234_567_890.055), 223.0);
        let old: Vec<String> =
            crate::widgets::labelled(egui_plot::log_grid_spacer(10)(input()), &input()).into_iter().filter(|v| (1_234_567_889.95..=1_234_567_890.05).contains(v)).map(|v| nexus_data::names::compact_credits(v.round() as i64)).collect();
        assert!(old.len() > 2 && old.iter().all(|l| l == "1.23 B"), "{old:?}");
        // Balances from nothing to a trillion, flat and moving by less than the labels show: two labels at least, all
        // different.
        let mut v = 0.0;
        while v < 1e12 {
            for (lo, hi) in [(v, v), (v, v * 1.001 + 1.0)] {
                for height in [200.0, 223.0, 300.0] {
                    let l = labels((lo, hi), false, height);
                    assert!(l.len() >= 2 && l.windows(2).all(|w| w[0] != w[1]), "{lo}..{hi} at {height} pt: {l:?}");
                }
            }
            v = (v * 1.07 + 1.0).round();
        }
        // A flat line sits in the middle of a band a fifth of its value tall, one that moves keeps 5 % of its move
        // above and below, and on the log scale a tenth of a power of ten.
        let near = |(a, b): (f64, f64), (c, d): (f64, f64)| (a - c).abs() < 1e-3 && (b - d).abs() < 1e-3;
        assert!(near(credit_y_range(2e9, 2e9, false), (1.8e9, 2.2e9)));
        assert!(near(credit_y_range(100e6, 300e6, false), (90e6, 310e6)));
        assert!(near(credit_y_range(8.0, 9.0, true), (7.9, 9.1)));
    }

    #[test]
    fn the_log_scale_always_has_labels() {
        // A balance between 2.1 and 4.6 billion reaches no power of ten: labelled at whole powers only, it had none.
        assert_eq!(labels((2.1e9f64.log10(), 4.6e9f64.log10()), true, 223.0), ["2.00 B", "3.00 B", "4.00 B", "5.00 B"]);
        // A climb over powers of ten is labelled at each one, or at 1, 2 and 5 times one where there's room.
        assert_eq!(labels((3.0, 9.0), true, 223.0), ["1000", "10 k", "100 k", "1.0 M", "10.0 M", "100.0 M", "1.00 B"]);
        assert_eq!(labels((6.0, 3e7f64.log10()), true, 223.0), ["1.0 M", "2.0 M", "5.0 M", "10.0 M", "20.0 M"]);
        // From 100 Cr to a trillion, flat or climbing up to ten thousandfold: two labels at least, all different.
        let mut v = 100.0;
        while v < 1e12 {
            for k in [1.0, 1.001, 2.5, 30.0, 1e4] {
                for height in [200.0, 223.0, 300.0] {
                    let l = labels((f64::log10(v), f64::log10(v * k)), true, height);
                    assert!(l.len() >= 2 && l.windows(2).all(|w| w[0] != w[1]), "{v} x{k} at {height} pt: {l:?}");
                }
            }
            v = (v * 1.07).round();
        }
    }
}
