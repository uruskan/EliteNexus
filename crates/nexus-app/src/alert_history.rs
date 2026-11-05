//! The alert history behind the bell in the toolbar: this run's alerts, newest first, each with its time, so one whose
//! banner went by unseen can still be read. Kept in memory only: alerts come from what happens while Elite Nexus runs.

use crate::theme::{display, pal, size};
use crate::widgets;
use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Stroke, Ui};
use nexus_journal::Timestamp;
use nexus_model::{Alert, Priority};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// The most alerts kept; the oldest go first.
pub const KEEP: usize = 200;
/// The list's width.
const WIDTH: f32 = 380.0;

/// The colour of an alert's kind, by its priority (the banners use it too).
pub fn tint(p: Priority) -> Color32 {
    match p {
        Priority::Critical => pal::BAD,
        Priority::High => pal::ACCENT,
        Priority::Normal => pal::BLUE,
        Priority::Low => pal::GRAY,
    }
}

pub struct AlertHistory {
    /// Newest last, each with the moment it came in.
    items: VecDeque<(Alert, Instant)>,
    /// Alerts that came in while the list was closed: the bell's badge.
    unseen: usize,
    /// The list is open.
    open: bool,
    /// When this run started: the list counts from then.
    since: Timestamp,
}

impl Default for AlertHistory {
    fn default() -> Self {
        AlertHistory::since(Timestamp::now())
    }
}

impl AlertHistory {
    /// An empty history that counts from `since` (the default: now).
    pub fn since(since: Timestamp) -> Self {
        AlertHistory { items: VecDeque::new(), unseen: 0, open: false, since }
    }

    /// Keeps an alert. False for a repeat, the same title and detail as one of the last few within 20 seconds: it
    /// isn't kept, nor shown as a banner again.
    pub fn push(&mut self, a: &Alert) -> bool {
        let repeat = self.items.iter().rev().take(4).any(|(b, at)| b.title == a.title && b.detail == a.detail && at.elapsed() < Duration::from_secs(20));
        if repeat {
            return false;
        }
        if self.items.len() >= KEEP {
            self.items.pop_front();
        }
        self.items.push_back((a.clone(), Instant::now()));
        if !self.open {
            self.unseen += 1;
        }
        true
    }

    /// Alerts that came in since the list was last open.
    pub fn unseen(&self) -> usize {
        self.unseen
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens or closes the list; opening it counts every alert as seen.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.unseen = 0;
        }
    }

    /// The list, under `bell` while it's open. A click elsewhere or Escape closes it.
    pub fn show(&mut self, bell: &egui::Response) {
        let ctx = bell.ctx.clone();
        let max_h = (ctx.content_rect().height() * 0.62).clamp(160.0, 620.0);
        let frame = egui::Frame::new()
            .fill(pal::CARD)
            .corner_radius(CornerRadius::same(14))
            .stroke(Stroke::new(1.0, Color32::from_white_alpha(18)))
            .shadow(ctx.style_of(egui::Theme::Dark).visuals.popup_shadow)
            .inner_margin(Margin::same(16));
        let (items, since) = (&self.items, self.since);
        let mut clear = false;
        egui::Popup::from_response(bell)
            .id(egui::Id::new("alert-history"))
            .open_bool(&mut self.open)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::RectAlign::BOTTOM_END)
            .gap(8.0)
            .width(WIDTH)
            .frame(frame)
            .show(|ui| {
                ui.set_width(WIDTH - 32.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Alerts").font(display(size::HEADLINE)).color(pal::TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !items.is_empty() && widgets::button(ui, "Clear").on_hover_text("Empties the list. Alerts still to come are kept.").clicked() {
                            clear = true;
                        }
                    });
                });
                let started = since.to_local_string("%H:%M");
                let note = if items.len() >= KEEP {
                    format!("Since Elite Nexus started at {started}, newest first: the last {KEEP}.")
                } else {
                    format!("Since Elite Nexus started at {started}, newest first.")
                };
                ui.add(egui::Label::new(RichText::new(note).size(size::FOOTNOTE).color(pal::DIM)).wrap());
                ui.add_space(6.0);
                if items.is_empty() {
                    ui.add_space(10.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new("No alerts yet. The alerts shown as banners while Elite Nexus runs are listed here too, so one you missed can still be read.")
                                .size(size::CALLOUT)
                                .color(pal::DIM),
                        )
                        .wrap(),
                    );
                    ui.add_space(6.0);
                    return;
                }
                // As tall as the list, up to `max_h`: a popup keeps the size it had when it opened, so the scroll area
                // asks for its height itself instead of filling what the first frame allowed.
                egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (i, (a, _)) in items.iter().rev().enumerate() {
                        if i > 0 {
                            let r = ui.available_rect_before_wrap();
                            ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, pal::SEPARATOR));
                        }
                        row(ui, a);
                    }
                });
            });
        if clear {
            self.items.clear();
        }
    }
}

/// One alert: its kind and time, the title, the detail.
fn row(ui: &mut Ui, a: &Alert) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(a.kind.label().to_uppercase()).font(display(size::CAPTION)).color(tint(a.priority)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let when = format!("{} · {}", a.ts.to_local_string("%H:%M"), widgets::ago(a.ts));
            ui.label(RichText::new(when).size(size::CAPTION).color(pal::DIM)).on_hover_text(a.ts.to_local_string("%d %b %Y %H:%M:%S"));
        });
    });
    ui.add(egui::Label::new(RichText::new(&a.title).font(display(size::BODY)).color(pal::TEXT)).wrap());
    if !a.detail.is_empty() {
        ui.add(egui::Label::new(RichText::new(&a.detail).size(size::FOOTNOTE).color(pal::DIM)).wrap());
    }
    ui.add_space(8.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_model::AlertKind;

    fn alert(title: &str) -> Alert {
        Alert::new(Timestamp::now(), AlertKind::Navigation, Priority::High, title).detail("12.5 t remaining")
    }

    #[test]
    fn keeps_what_came_in_while_closed() {
        let mut h = AlertHistory::default();
        assert!(h.push(&alert("Fuel low")));
        assert!(!h.push(&alert("Fuel low")), "the same alert within 20 seconds is kept once");
        assert!(h.push(&alert("Interdiction")));
        assert_eq!((h.items.len(), h.unseen()), (2, 2));
        h.toggle();
        assert_eq!(h.unseen(), 0, "opening the list shows them all");
        assert!(h.push(&alert("Hull critical")));
        assert_eq!(h.unseen(), 0, "one that comes in while it's open is seen");
        h.toggle();
        for i in 0..KEEP + 5 {
            h.push(&alert(&format!("Alert {i}")));
        }
        assert_eq!(h.items.len(), KEEP, "the oldest go first");
        assert_eq!(h.items.back().map(|(a, _)| a.title.as_str()), Some(format!("Alert {}", KEEP + 4).as_str()));
    }
}
