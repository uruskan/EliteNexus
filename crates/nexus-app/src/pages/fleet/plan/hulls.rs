//! Every hull side by side, as a shipyard sells it: its price, ratings and slots, to sort and start from.

use crate::theme::{pal, size};
use crate::widgets::{self, footnote, short_credits, TableSort};
use egui::{Label, RichText, Sense, Ui};
use egui_extras::Column;
use nexus_data::fitting::{self, Hull};
use nexus_data::outfitting::SlotKind;

const TABLE: &str = "plan-hulls";

/// What a shipyard asks for a hull: with its standard modules.
pub fn shipyard_price(h: &Hull) -> f64 {
    if h.retail_cost > 0.0 { h.retail_cost } else { h.hull_cost }
}

/// Hardpoints by size, huge first, and utility mounts: "1 huge, 2 large · 4 utility".
fn hardpoints(h: &Hull) -> String {
    let count = |kind: SlotKind, size: u8| h.slots.iter().filter(|s| s.kind == kind && s.size == size).count();
    let mut parts: Vec<String> = [(4, "huge"), (3, "large"), (2, "medium"), (1, "small")].iter().filter(|(s, _)| count(SlotKind::Hardpoint, *s) > 0).map(|(s, name)| format!("{} {name}", count(SlotKind::Hardpoint, *s))).collect();
    if parts.is_empty() {
        parts.push("No hardpoints".into());
    }
    let utility = h.slots.iter().filter(|s| s.kind == SlotKind::Utility).count();
    format!("{} · {utility} utility", parts.join(", "))
}

/// Optional internals' sizes, biggest first, and military slots: "Sizes 6, 5, 5, 3 · military 5, 5".
fn internals(h: &Hull) -> String {
    let sizes = |kind: SlotKind| {
        let mut v: Vec<u8> = h.slots.iter().filter(|s| s.kind == kind).map(|s| s.size).collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        v.iter().map(u8::to_string).collect::<Vec<_>>().join(", ")
    };
    let military = sizes(SlotKind::Military);
    format!("Sizes {}{}", sizes(SlotKind::Optional), if military.is_empty() { String::new() } else { format!(" · military {military}") })
}

/// A count of slots and their sizes added up, to sort by.
fn capacity(h: &Hull, kinds: &[SlotKind]) -> (usize, u32) {
    let s: Vec<_> = h.slots.iter().filter(|s| kinds.contains(&s.kind)).collect();
    (s.len(), s.iter().map(|s| u32::from(s.size)).sum())
}

/// The table; returns the hull whose name was clicked.
pub fn table(ui: &mut Ui) -> Option<&'static Hull> {
    let rows = fitting::hulls();
    let mut sort = TableSort::load(ui, TABLE);
    let weapons = [SlotKind::Hardpoint];
    let optional = [SlotKind::Optional, SlotKind::Military];
    let order = sort.order(rows, |a, b, c| match c {
        0 => widgets::cmp_text(&a.name, &b.name),
        1 => shipyard_price(a).total_cmp(&shipyard_price(b)),
        2 => a.speed.total_cmp(&b.speed).then(a.boost.total_cmp(&b.boost)),
        3 => a.base_shield.total_cmp(&b.base_shield),
        4 => a.base_armour.total_cmp(&b.base_armour),
        5 => a.hull_mass.total_cmp(&b.hull_mass),
        6 => capacity(a, &weapons).cmp(&capacity(b, &weapons)),
        _ => capacity(a, &optional).cmp(&capacity(b, &optional)),
    });
    let mut picked = None;
    // Every row at its height: the page scrolls, not the table.
    widgets::table(ui, TABLE, |t| {
        t.vscroll(false)
            .column(widgets::flex_column(130.0))
            .column(Column::initial(80.0).at_least(72.0))
            .column(Column::initial(84.0).at_least(76.0))
            .column(Column::initial(62.0).at_least(56.0))
            .column(Column::initial(62.0).at_least(56.0))
            .column(Column::initial(62.0).at_least(56.0))
            .column(Column::initial(66.0).at_least(60.0))
            .column(Column::initial(66.0).at_least(60.0))
            .header(30.0, |mut h| {
                for (i, (t, desc)) in [("Ship", false), ("Price", true), ("Speed", true), ("Shields", true), ("Armour", true), ("Mass", false), ("Weapons", true), ("Internals", true)].into_iter().enumerate() {
                    h.col(|ui| widgets::th_sort(ui, t, i as u8, &mut sort, TABLE, desc));
                }
            })
            .body(|body| {
                body.rows(40.0, order.len(), |mut row| {
                    let Some(h) = order.get(row.index()).and_then(|&i| rows.get(i)) else { return };
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            let name = ui.add(Label::new(RichText::new(&h.name).color(pal::ACCENT_SOFT)).truncate().sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("Plan a {} from the modules a shipyard sells it with", h.name));
                            if name.clicked() {
                                picked = Some(h);
                            }
                            ui.add(Label::new(RichText::new(format!("{} · {} pad", h.manufacturer, h.pad)).size(size::CAPTION).color(pal::DIM)).truncate());
                        });
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(short_credits(shipyard_price(h) as i64)).color(pal::GOLD));
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.0} / {:.0}", h.speed, h.boost)).on_hover_text("Top speed and boost the hull is rated for, m/s");
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.0}", h.base_shield));
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.0}", h.base_armour));
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.0}\u{a0}t", h.hull_mass));
                    });
                    row.col(|ui| {
                        ui.label(capacity(h, &weapons).0.to_string()).on_hover_text(hardpoints(h));
                    });
                    row.col(|ui| {
                        ui.label(capacity(h, &optional).0.to_string()).on_hover_text(internals(h));
                    });
                });
            });
    });
    footnote(ui, "The hull's own ratings: its thrusters and mass change the speed and boost, a shield generator multiplies the shields, bulkheads and reinforcements add to the armour. Weapons counts hardpoints and Internals optional internal slots: point at a number for their sizes. Prices are list prices, with the standard modules.");
    picked
}
