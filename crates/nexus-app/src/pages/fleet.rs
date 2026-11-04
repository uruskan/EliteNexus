use crate::theme::{display, pal};
use crate::widgets::{badge, badge_size, card, empty, short_credits};
use egui::{Grid, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use nexus_model::fleet::{Ship, ShipModule};
use nexus_model::GameState;

#[derive(Default)]
pub struct FleetUi {
    pub selected: Option<u64>,
}

pub fn show(ui: &mut Ui, gs: &GameState, st: &mut FleetUi) {
    if gs.fleet.ships.is_empty() {
        empty(ui, icon::ROCKET, "No ships yet", "Ships appear after the game writes a Loadout or StoredShips event.");
        return;
    }
    let selected = st.selected.or(gs.fleet.current_ship_id);
    let mut ships: Vec<&Ship> = gs.fleet.ships.values().collect();
    ships.sort_by(|a, b| (Some(b.id) == gs.fleet.current_ship_id).cmp(&(Some(a.id) == gs.fleet.current_ship_id)).then(b.total_value().cmp(&a.total_value())));
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
        if let Some(s) = selected.and_then(|id| gs.fleet.ships.get(&id)) {
            ship_detail(ui, gs, s);
        }
        return;
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
            if let Some(s) = selected.and_then(|id| gs.fleet.ships.get(&id)) {
                ship_detail(ui, gs, s);
            }
        });
    });
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
            let loc = if current {
                gs.location.situation()
            } else {
                match (&s.location.system, &s.location.station, s.location.in_transit) {
                    (Some(sys), Some(stn), true) => format!("{} In transit → {} ({})", icon::TRUCK, stn, sys),
                    (Some(sys), Some(stn), false) => format!("{} {} ({})", icon::MAP_PIN, stn, sys),
                    (Some(sys), None, _) => format!("{} {}", icon::MAP_PIN, sys),
                    _ => "Location unknown".into(),
                }
            };
            ui.label(RichText::new(loc).small().color(pal::DIM));
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

fn ship_detail(ui: &mut Ui, _gs: &GameState, s: &Ship) {
    card(ui, "Loadout", |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(s.display_name()).font(display(22.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(json) = &s.loadout_json {
                    if ui.button(format!("{} Coriolis", icon::ARROW_SQUARE_OUT)).on_hover_text("Open this exact build in coriolis.io").clicked()
                        && let Some(url) = crate::export::coriolis_url(json) {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        }
                    if ui.button(format!("{} EDSY", icon::ARROW_SQUARE_OUT)).on_hover_text("Open this exact build in edsy.org").clicked()
                        && let Some(url) = crate::export::edsy_url(json) {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        }
                    if ui.button(format!("{} Copy JSON", icon::COPY)).clicked() {
                        ui.ctx().copy_text(json.clone());
                    }
                }
            });
        });
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
        return;
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
}

fn kv_inline(ui: &mut Ui, k: &str, v: String) {
    ui.label(RichText::new(k).color(pal::DIM));
    ui.label(RichText::new(v).color(pal::TEXT));
}
