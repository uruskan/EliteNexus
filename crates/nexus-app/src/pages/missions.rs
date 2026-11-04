use crate::theme::{display, pal};
use crate::widgets::{badge, bar, card, duration, empty, short_credits};
use egui::{Grid, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use nexus_journal::Timestamp;
use nexus_model::missions::MissionOutcome;
use nexus_model::GameState;

pub fn show(ui: &mut Ui, gs: &GameState) {
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
        TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(300.0))
            .column(Column::initial(180.0))
            .column(Column::initial(200.0))
            .column(Column::initial(100.0))
            .column(Column::initial(90.0))
            .column(Column::remainder())
            .header(30.0, |mut h| {
                for t in ["Mission", "Faction", "Destination", "Reward", "Expires", "Progress"] {
                    h.col(|ui| {
                        crate::widgets::th(ui, t);
                    });
                }
            })
            .body(|body| {
                body.rows(32.0, list.len(), |mut row| {
                    let m = list[row.index()];
                    row.col(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(&m.title);
                            if m.wing {
                                badge(ui, "wing", pal::CYAN);
                            }
                        });
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(&m.faction).color(pal::DIM));
                    });
                    row.col(|ui| {
                        let dest = match (&m.destination_station, &m.destination_system) {
                            (Some(st), Some(sys)) => format!("{st} ({sys})"),
                            (None, Some(sys)) => sys.clone(),
                            _ => m.destination_settlement.clone().unwrap_or_default(),
                        };
                        ui.label(dest);
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(m.reward.map(short_credits).unwrap_or_default()).color(pal::GOLD));
                    });
                    row.col(|ui| {
                        if let Some(left) = m.seconds_left(now) {
                            let col = if left < 3600 { pal::BAD } else if left < 6 * 3600 { pal::WARN } else { pal::DIM };
                            ui.label(RichText::new(if left > 0 { duration(left) } else { "expired".into() }).color(col));
                        }
                    });
                    row.col(|ui| {
                        if m.redirected {
                            badge(ui, "Complete — hand in", pal::GOOD);
                        } else if let Some(k) = m.kill_count {
                            ui.label(format!("{}/{} kills", m.kills.min(k), k));
                        } else if m.cargo_total > 0 {
                            ui.label(format!("{}/{} delivered", m.cargo_delivered, m.cargo_total));
                        } else if let Some(c) = &m.commodity {
                            ui.label(RichText::new(format!("{} {}", m.count.unwrap_or(0), c)).color(pal::DIM));
                        } else if let Some(p) = m.passenger_count {
                            ui.label(RichText::new(format!("{p} passengers")).color(pal::DIM));
                        }
                    });
                });
            });
    });
    ui.add_space(8.0);
    card(ui, "Recent mission history", |ui| {
        // Seen live by another tool's author (edda-app, three hand-ins) and on Frontier's forums: the wingmate who
        // handed the mission in gets the MissionCompleted line, the others get none (docs/research/journal.md §5.6).
        let shared = "A mission a wingmate shared and handed in isn't counted: the game writes nothing about it in your journal.";
        if gs.missions.history.is_empty() {
            ui.label(RichText::new("None recorded.").color(pal::DIM));
            crate::widgets::footnote(ui, shared);
            return;
        }
        Grid::new("mhist").num_columns(4).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
            for r in gs.missions.history.iter().rev().take(40) {
                ui.label(RichText::new(r.ts.to_local_string("%d %b %H:%M")).color(pal::FAINT));
                ui.label(&r.title);
                let (t, c) = match r.outcome {
                    MissionOutcome::Completed => ("completed", pal::GOOD),
                    MissionOutcome::Failed => ("failed", pal::BAD),
                    MissionOutcome::Abandoned => ("abandoned", pal::WARN),
                };
                badge(ui, t, c);
                ui.label(RichText::new(short_credits(r.reward)).font(display(14.0)).color(pal::GOLD));
                ui.end_row();
            }
        });
        crate::widgets::footnote(ui, shared);
    });
}
