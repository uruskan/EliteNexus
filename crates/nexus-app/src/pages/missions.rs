use crate::pages::where_to_get::{Goto, Sources, WhereToGet};
use crate::theme::{display, pal, size};
use crate::widgets::{badge, badge_size, bar, card, duration, empty, short_credits};
use egui::{Color32, Grid, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use nexus_journal::Timestamp;
use nexus_model::missions::{Mission, MissionOutcome, MissionRecord};
use nexus_model::GameState;

/// The Missions page's state: "where to get it" for the goods your missions ask for.
#[derive(Default)]
pub struct MissionsUi {
    get: WhereToGet,
}

/// Returns what a "where to get it" panel asks the Market page to open.
pub fn show(ui: &mut Ui, gs: &GameState, st: &mut MissionsUi, src: &Sources) -> Option<Goto> {
    let stacks = gs.missions.massacre_stacks();
    if !stacks.is_empty() {
        for s in &stacks {
            card(ui, &format!("Massacre stack — {}", s.target_faction), |ui| {
                ui.horizontal(|ui| {
                    crate::widgets::stat(ui, "kills remaining", format!("{} / {}", s.kills_remaining, s.kills_total), pal::ACCENT);
                    ui.add_space(16.0);
                    crate::widgets::stat(ui, "missions", s.missions.to_string(), pal::TEXT);
                    ui.add_space(16.0);
                    crate::widgets::stat(ui, "total reward", short_credits(s.total_reward), pal::GOLD);
                    if let Some(sys) = &s.target_system {
                        ui.add_space(16.0);
                        crate::widgets::stat(ui, "target system", sys.clone(), pal::CYAN);
                    }
                });
                let done = (s.kills_total - s.kills_remaining) as f32 / s.kills_total.max(1) as f32;
                bar(ui, done, pal::ACCENT, 10.0, None);
                ui.add_space(4.0);
                Grid::new(("stack", &s.target_faction)).num_columns(4).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
                    for g in &s.givers {
                        ui.label(&g.faction);
                        ui.label(RichText::new(format!("{} missions", g.missions)).color(pal::DIM));
                        ui.label(RichText::new(format!("{}/{} kills", g.kills_done, g.kills_needed)).color(if g.kills_done >= g.kills_needed { pal::GOOD } else { pal::TEXT }));
                        ui.label(RichText::new(short_credits(g.reward)).color(pal::GOLD));
                        ui.end_row();
                    }
                });
                ui.label(RichText::new("Each kill counts once for every issuing faction (its oldest open mission). The stack is done after the largest per-faction requirement.").small().color(pal::DIM));
            });
            ui.add_space(8.0);
        }
    }

    card(ui, &format!("Active missions ({})", gs.missions.active.len()), |ui| {
        if gs.missions.active.is_empty() {
            empty(ui, icon::TARGET, "No active missions", "");
            return;
        }
        let now = Timestamp::now();
        let mut list: Vec<_> = gs.missions.active.values().collect();
        list.sort_by_key(|m| m.expiry.map(|e| e.0).unwrap_or(i64::MAX));
        let destination = |m: &Mission| match (&m.destination_station, &m.destination_system) {
            (Some(st), Some(sys)) => format!("{st} ({sys})"),
            (None, Some(sys)) => sys.clone(),
            _ => m.destination_settlement.clone().unwrap_or_default(),
        };
        let reward = |m: &Mission| m.reward.map(short_credits).unwrap_or_default();
        let expires = |m: &Mission| {
            m.seconds_left(now).map(|left| {
                let col = if left < 3600 { pal::BAD } else if left < 6 * 3600 { pal::WARN } else { pal::DIM };
                (if left > 0 { duration(left) } else { "expired".into() }, col)
            })
        };
        // What Progress says, in its colour (None: the usual text colour); a redirected mission's is a badge.
        let progress = |m: &Mission| -> Option<(String, Option<Color32>)> {
            if m.redirected {
                Some(("Complete — hand in".into(), None))
            } else if let Some(k) = m.kill_count {
                Some((format!("{}/{} kills", m.kills.min(k), k), None))
            } else if m.cargo_total > 0 {
                Some((format!("{}/{} delivered", m.cargo_delivered, m.cargo_total), None))
            } else if let Some(c) = &m.commodity {
                Some((format!("{} {}", m.count.unwrap_or(0), c), Some(pal::DIM)))
            } else {
                m.passenger_count.map(|p| (format!("{p} passengers"), Some(pal::DIM)))
            }
        };
        // Every column measured, so the table fits the card at any window size: Reward, Expires and Progress as wide
        // as their widest entry, and the three text columns share the rest. A title, faction or destination too long
        // for its share ends in "…" and shows whole on hover; the wing badge always stays.
        let heads = ["Mission", "Faction", "Destination", "Reward", "Expires", "Progress"];
        let gap = ui.spacing().item_spacing.x;
        let font = egui::TextStyle::Body.resolve(ui.style());
        let text_w = |t: &str| ui.painter().layout_no_wrap(t.to_owned(), font.clone(), pal::TEXT).size().x + 8.0;
        let wing_w = badge_size(ui, "wing").x;
        let mut want = heads.map(|t| ui.painter().layout_no_wrap(t.to_owned(), display(size::FOOTNOTE), pal::DIM).size().x + 8.0);
        let floor = want;
        for m in &list {
            let progress_w = match progress(m) {
                Some((t, _)) if m.redirected => badge_size(ui, &t).x + 8.0,
                Some((t, _)) => text_w(&t),
                None => 0.0,
            };
            let row = [
                text_w(&m.title) + if m.wing { gap + wing_w } else { 0.0 },
                text_w(&m.faction),
                text_w(&destination(m)),
                text_w(&reward(m)),
                expires(m).map_or(0.0, |(t, _)| text_w(&t)),
                progress_w,
            ];
            for (w, r) in want.iter_mut().zip(row) {
                *w = w.max(r);
            }
        }
        let room = ui.available_width() - want[3..].iter().sum::<f32>() - gap * (heads.len() - 1) as f32;
        let text = share([want[0], want[1], want[2]], [floor[0], floor[1], floor[2]], room);
        TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(text[0]))
            .column(Column::exact(text[1]))
            .column(Column::exact(text[2]))
            .column(Column::exact(want[3]))
            .column(Column::exact(want[4]))
            .column(Column::remainder().clip(true))
            .header(30.0, |mut h| {
                for t in heads {
                    h.col(|ui| {
                        crate::widgets::th(ui, t);
                    });
                }
            })
            .body(|body| {
                body.rows(32.0, list.len(), |mut row| {
                    let m = list[row.index()];
                    row.col(|ui| {
                        let badge_room = if m.wing { gap + wing_w } else { 0.0 };
                        ui.scope(|ui| {
                            ui.set_max_width((ui.available_width() - badge_room).max(0.0));
                            ui.add(egui::Label::new(&m.title).truncate());
                        });
                        if m.wing {
                            badge(ui, "wing", pal::CYAN);
                        }
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(RichText::new(&m.faction).color(pal::DIM)).truncate());
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(destination(m)).truncate());
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(reward(m)).color(pal::GOLD));
                    });
                    row.col(|ui| {
                        if let Some((t, col)) = expires(m) {
                            ui.label(RichText::new(t).color(col));
                        }
                    });
                    row.col(|ui| match progress(m) {
                        Some((t, _)) if m.redirected => {
                            badge(ui, &t, pal::GOOD);
                        }
                        Some((t, Some(col))) => {
                            ui.label(RichText::new(t).color(col));
                        }
                        Some((t, None)) => {
                            ui.label(t);
                        }
                        None => {}
                    });
                });
            });
    });
    let goto = goods(ui, gs, st, src);
    ui.add_space(8.0);
    card(ui, "Recent mission history", |ui| {
        // Reported by other developers and on Frontier's forums: the wingmate who handed the mission in gets the
        // MissionCompleted line, the others get none (docs/research/journal.md §5.6).
        let shared = "A mission a wingmate shared and handed in isn't counted: the game writes nothing about it in your journal.";
        if gs.missions.history.is_empty() {
            ui.label(RichText::new("None recorded.").color(pal::DIM));
            crate::widgets::footnote(ui, shared);
            return;
        }
        // The title last, in the width the other columns leave: a long one ends in "…" (whole on hover) instead of
        // widening the card past the window.
        Grid::new("mhist").num_columns(4).striped(true).spacing([12.0, 3.0]).max_col_width(ui.available_width()).show(ui, |ui| {
            for r in gs.missions.history.iter().rev().take(40) {
                ui.label(RichText::new(r.ts.to_local_string("%d %b %H:%M")).color(pal::FAINT));
                let (t, c) = match r.outcome {
                    MissionOutcome::Completed => ("completed", pal::GOOD),
                    MissionOutcome::Failed => ("failed", pal::BAD),
                    MissionOutcome::Abandoned => ("abandoned", pal::WARN),
                };
                badge(ui, t, c);
                let (paid, col) = paid(r);
                ui.label(RichText::new(paid).font(display(14.0)).color(col));
                ui.add(egui::Label::new(&r.title).truncate());
                ui.end_row();
            }
        });
        crate::widgets::footnote(ui, shared);
    });
    goto
}

/// What a mission in the history paid, in its colour: a completed one's reward; a failed or abandoned one paid
/// nothing, so it says that, or the fine it cost (`MissionRecord::reward` holds a fine as a negative amount), rather
/// than a reward of "0 Cr".
fn paid(r: &MissionRecord) -> (String, Color32) {
    match r.outcome {
        MissionOutcome::Completed => (short_credits(r.reward), pal::GOLD),
        _ if r.reward < 0 => (format!("fined {}", short_credits(-r.reward)), pal::BAD),
        _ => ("no reward".into(), pal::DIM),
    }
}

/// A commodity your missions ask you to bring: how many tonnes are still to go, and for which missions.
struct Goods<'a> {
    /// The journal's symbol (`$HydrogenFuel_Name;`): what the pin and the lookups go by, whatever the game's language.
    symbol: &'a str,
    /// Its name as the missions give it, in the game's language.
    name: String,
    tonnes: i64,
    missions: Vec<&'a str>,
}

/// What the active missions ask you to bring, one entry per commodity: not a delivery's or a smuggling run's cargo,
/// which the mission hands you, nor a mission that's ready to hand in.
fn wanted_goods(gs: &GameState) -> Vec<Goods<'_>> {
    fn text(s: Option<&str>) -> Option<&str> {
        s.map(str::trim).filter(|s| !s.is_empty())
    }
    let mut out: Vec<Goods> = Vec::new();
    for m in gs.missions.active.values() {
        let Some(symbol) = text(m.commodity_symbol.as_deref()).or(text(m.commodity.as_deref())) else { continue };
        let kind = m.name.to_ascii_lowercase();
        if kind.contains("delivery") || kind.contains("smuggle") || m.redirected {
            continue;
        }
        let tonnes = (m.count.unwrap_or(0) - m.cargo_delivered).max(0);
        if tonnes == 0 {
            continue;
        }
        match out.iter_mut().find(|g| g.symbol.eq_ignore_ascii_case(symbol)) {
            Some(g) => {
                g.tonnes += tonnes;
                g.missions.push(&m.title);
            }
            None => {
                // The game's name for it; without one (it gave only the symbol), the commodity's English name.
                let name = text(m.commodity.as_deref()).filter(|n| *n != symbol).map_or_else(|| nexus_data::commodities::display_name(symbol), str::to_string);
                out.push(Goods { symbol, name, tonnes, missions: vec![&m.title] });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The goods your missions ask for, each with where to get it. Returns what its panel asks the Market page to open.
fn goods(ui: &mut Ui, gs: &GameState, st: &mut MissionsUi, src: &Sources) -> Option<Goto> {
    let list = wanted_goods(gs);
    if list.is_empty() {
        return None;
    }
    let mut goto = None;
    ui.add_space(8.0);
    card(ui, "Goods your missions ask for", |ui| {
        crate::widgets::group(ui, |g| {
            for w in &list {
                g.row(&w.name, &w.missions.join(" · "), |ui| {
                    st.get.button(ui, w.symbol);
                    ui.label(RichText::new(format!("{} t", nexus_data::names::group_digits(w.tonnes))).color(pal::WARN));
                });
                if st.get.is_open(w.symbol) {
                    g.content(|ui| goto = st.get.panel(ui, gs, src, w.symbol));
                }
            }
        });
        crate::widgets::footnote(ui, "What your missions ask you to source, mine or donate. A delivery's cargo is handed to you with the mission, so it isn't here.");
    });
    goto
}

/// Splits `room` between columns that want the widths `want`. When they all fit, the spare room is shared in
/// proportion; when they don't, the widest are cut back to a common width first, so the shorter ones stay whole, and
/// none goes below its `floor` (the width of its heading).
fn share(want: [f32; 3], floor: [f32; 3], room: f32) -> [f32; 3] {
    let total: f32 = want.iter().sum();
    if total <= room {
        return want.map(|w| w * room / total);
    }
    let mut order = [0, 1, 2];
    order.sort_by(|a, b| want[*a].total_cmp(&want[*b]));
    let (mut left, mut cap) = (room, room);
    for (n, i) in order.into_iter().enumerate() {
        cap = left / (want.len() - n) as f32;
        if want[i] > cap {
            break;
        }
        left -= want[i];
    }
    std::array::from_fn(|i| want[i].min(cap).max(floor[i]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_columns_share_the_room() {
        let floor = [60.0; 3];
        // Room to spare: every column grows in proportion.
        assert_eq!(share([100.0, 200.0, 100.0], floor, 800.0), [200.0, 400.0, 200.0]);
        // Too little: the widest is cut first and the short ones stay whole...
        assert_eq!(share([400.0, 100.0, 150.0], floor, 450.0), [200.0, 100.0, 150.0]);
        // ...then the two widest share what the shortest leaves.
        assert_eq!(share([400.0, 100.0, 300.0], floor, 500.0), [200.0, 100.0, 200.0]);
        // Never narrower than a heading.
        assert_eq!(share([400.0, 100.0, 300.0], floor, 120.0), [60.0, 60.0, 60.0]);
    }

    #[test]
    fn failed_missions_show_no_reward() {
        let record = |outcome, reward| MissionRecord { id: 1, title: "Deliver 40 units of Tritium".into(), faction: String::new(), outcome, reward, ts: Timestamp(0) };
        assert_eq!(paid(&record(MissionOutcome::Completed, 18_500_000)).0, "18.5 M Cr");
        assert_eq!(paid(&record(MissionOutcome::Failed, 0)).0, "no reward");
        assert_eq!(paid(&record(MissionOutcome::Abandoned, 0)).0, "no reward");
        // The model keeps MissionFailed's Fine as a negative reward.
        assert_eq!(paid(&record(MissionOutcome::Failed, -50_000)), ("fined 50 k Cr".to_string(), pal::BAD));
    }

    /// A German client's missions: the goods go by the journal's symbol (two missions for one good are one entry),
    /// show the game's name, and the lookup finds the commodity; a delivery's cargo isn't asked for.
    #[test]
    fn goods_go_by_symbol() {
        let mut gs = GameState::default();
        let mission = |id: u64, name: &str, symbol: &str, local: &str, count: i64| Mission {
            id,
            name: name.into(),
            title: format!("Mission {id}"),
            commodity: Some(local.into()),
            commodity_symbol: Some(symbol.into()),
            count: Some(count),
            ..Default::default()
        };
        gs.missions.active.insert(1, mission(1, "Mission_Collect_Industry", "$HydrogenFuel_Name;", "Wasserstoff-Treibstoff", 12));
        gs.missions.active.insert(2, mission(2, "Mission_Mining_Boom", "$HydrogenFuel_Name;", "Wasserstoff-Treibstoff", 8));
        gs.missions.active.insert(3, mission(3, "Mission_Delivery", "$Gold_Name;", "Gold", 20));
        let goods = wanted_goods(&gs);
        assert_eq!(goods.len(), 1, "the delivery's gold comes with the mission");
        assert_eq!((goods[0].name.as_str(), goods[0].tonnes, goods[0].missions.len()), ("Wasserstoff-Treibstoff", 20, 2));
        assert_eq!(nexus_data::commodities::commodity(goods[0].symbol).map(|c| c.name.as_str()), Some("Hydrogen Fuel"));
    }
}
