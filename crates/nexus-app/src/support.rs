//! The support card: at each start, a small sheet over the main window with the GitHub Sponsors and Buy Me a Coffee
//! buttons. It can be closed after a 10-second countdown, and nothing closes it before then (not Esc, not a click
//! outside). Only in the main window: never on the overlay, in VR or in a test run, and it never takes focus from
//! the game.

use crate::theme::{bold, display, icon_fill, pal, size};
use crate::widgets;
use egui::{Align, Align2, Color32, CornerRadius, Layout, RichText, Sense, Shape, Stroke, Ui, Vec2};
use egui_phosphor::regular as icon;
use std::time::{Duration, Instant};

pub const SPONSOR_URL: &str = "https://github.com/sponsors/uruskan";
pub const COFFEE_URL: &str = "https://buymeacoffee.com/uruskan";

/// How long the card stays before it can be closed.
pub const WAIT: Duration = Duration::from_secs(10);

/// The colours of the README's support badges.
const SPONSOR_PINK: Color32 = Color32::from_rgb(0xea, 0x4a, 0xaa);
const COFFEE_YELLOW: Color32 = Color32::from_rgb(0xff, 0xdd, 0x00);

/// Whether the card opens at this start: at every start of a run that follows the game, never in a test run
/// (`--screenshot`, `--journal-dir`). The one place that decides it: stable releases will later skip it for
/// supporters, and that check goes here.
pub fn wanted(live_run: bool) -> bool {
    live_run
}

/// The card for this start, if it opens: `wanted`, or in a screenshot run `--tab support` (the countdown running) or
/// `--tab support-close` (the countdown over, Close showing).
pub fn at_start(live_run: bool, shot_tab: Option<&str>) -> Option<Card> {
    match shot_tab {
        Some(t) if t.eq_ignore_ascii_case("support") => Some(Card::default()),
        Some(t) if t.eq_ignore_ascii_case("support-close") => Some(Card::finished(Instant::now())),
        _ if wanted(live_run) => Some(Card::default()),
        _ => None,
    }
}

/// The 10 seconds from when the card was first drawn.
#[derive(Clone, Copy)]
pub struct Countdown {
    shown: Instant,
}

impl Countdown {
    pub fn new(shown: Instant) -> Self {
        Countdown { shown }
    }

    /// Time left before the card can be closed.
    pub fn left(&self, now: Instant) -> Duration {
        WAIT.saturating_sub(now.saturating_duration_since(self.shown))
    }

    pub fn done(&self, now: Instant) -> bool {
        self.left(now).is_zero()
    }

    /// The whole seconds to show, rounded up: 10 at the start, 1 in the last second, 0 when it's over.
    pub fn seconds(&self, now: Instant) -> u64 {
        let left = self.left(now);
        left.as_secs() + u64::from(left.subsec_nanos() > 0)
    }

    /// How much of the wait is left, from 1 at the start to 0 at the end (the ring).
    pub fn fraction_left(&self, now: Instant) -> f32 {
        self.left(now).as_secs_f32() / WAIT.as_secs_f32()
    }
}

/// The card while it's open. Its countdown starts on the frame it's first drawn, so time spent behind the welcome
/// sheet doesn't count.
#[derive(Default)]
pub struct Card {
    countdown: Option<Countdown>,
}

impl Card {
    /// A card whose countdown is already over (screenshots of the Close button).
    fn finished(now: Instant) -> Self {
        Card { countdown: Some(Countdown::new(now.checked_sub(WAIT).unwrap_or(now))) }
    }

    /// Draws the card; true once Close was clicked.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let now = Instant::now();
        let countdown = *self.countdown.get_or_insert_with(|| Countdown::new(now));
        let done = countdown.done(now);
        if !done {
            // The ring empties smoothly; once Close shows nothing moves.
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        let mut closed = false;
        // The modal's own close requests (Esc, a click outside) are ignored: only Close closes it.
        let _ = egui::Modal::new(egui::Id::new("support"))
            .frame(egui::Frame::new().fill(pal::SIDEBAR).corner_radius(CornerRadius::same(20)).inner_margin(egui::Margin::same(28)))
            .show(ctx, |ui: &mut Ui| {
                // Clearly smaller than the window, also at 1000 px wide.
                ui.set_width(460.0);
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(60.0), Sense::hover());
                    ui.painter().rect_filled(rect, CornerRadius::same(16), SPONSOR_PINK);
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, egui_phosphor::fill::HEART, icon_fill(32.0), Color32::WHITE);
                    ui.add_space(14.0);
                    ui.label(RichText::new("Elite Nexus is free, and stays free").font(bold(size::TITLE3)).color(pal::TEXT));
                    ui.add_space(8.0);
                    for line in [
                        "One commander builds it, with no company, no ads and no subscription, and nothing in it depends on this.",
                        "I never made it for money, but it takes more of my time every week. Your support keeps me going, and pays for things like early-access ships, so new features are tested in the game instead of guessed at.",
                    ] {
                        ui.add(egui::Label::new(RichText::new(line).size(size::CALLOUT).color(pal::DIM)).wrap().halign(Align::Center));
                        ui.add_space(6.0);
                    }
                });
                ui.add_space(14.0);
                support_buttons(ui);
                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.set_min_height(30.0);
                    ui.label(RichText::new("Shown at every start during the beta.").size(size::FOOTNOTE).color(pal::FAINT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if done {
                            closed = widgets::primary_button(ui, "Close").clicked();
                        } else {
                            ring(ui, &countdown, now);
                            ui.label(RichText::new("You can close this in").size(size::FOOTNOTE).color(pal::DIM));
                        }
                    });
                });
            });
        closed
    }
}

/// The two support buttons side by side, centred, in the README badges' colours.
fn support_buttons(ui: &mut Ui) {
    let gap = 10.0;
    let width = ((ui.available_width() - gap) / 2.0).min(200.0);
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - 2.0 * width - gap) / 2.0).max(0.0));
        ui.spacing_mut().item_spacing.x = gap;
        let size = Vec2::new(width, 38.0);
        for (text, fill, ink, url) in [
            (format!("{}  Sponsor on GitHub", icon::HEART), SPONSOR_PINK, Color32::WHITE, SPONSOR_URL),
            (format!("{}  Buy me a coffee", icon::COFFEE), COFFEE_YELLOW, Color32::BLACK, COFFEE_URL),
        ] {
            if widgets::tinted_button(ui, &text, fill, ink, size).on_hover_text(url).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
    });
}

/// A ring that empties over the countdown, with the seconds left in it.
fn ring(ui: &mut Ui, countdown: &Countdown, now: Instant) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let r = 12.5;
    p.circle_stroke(c, r, Stroke::new(3.0, pal::SEPARATOR));
    let left = countdown.fraction_left(now);
    if left > 0.0 {
        // Clockwise from the top, as a clock's hand runs.
        let steps = ((64.0 * left).ceil() as usize).max(2);
        let start = -std::f32::consts::FRAC_PI_2;
        let sweep = std::f32::consts::TAU * left;
        let points = (0..=steps).map(|i| {
            let a = start + sweep * i as f32 / steps as f32;
            c + r * Vec2::new(a.cos(), a.sin())
        });
        p.add(Shape::line(points.collect(), Stroke::new(3.0, pal::ACCENT)));
    }
    p.text(c, Align2::CENTER_CENTER, countdown.seconds(now).to_string(), display(size::CAPTION), pal::TEXT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_runs_ten_seconds() {
        let t0 = Instant::now();
        let c = Countdown::new(t0);
        assert_eq!(c.seconds(t0), 10, "starts at 10");
        assert!(!c.done(t0));
        assert_eq!(c.seconds(t0 + Duration::from_millis(1)), 10, "rounded up");
        assert_eq!(c.seconds(t0 + Duration::from_millis(9_200)), 1, "the last second shows 1");
        assert!(!c.done(t0 + Duration::from_millis(9_999)), "no Close before the 10 seconds");
        assert!(c.done(t0 + WAIT), "Close at 10 seconds");
        assert_eq!(c.seconds(t0 + Duration::from_secs(60)), 0);
        assert!((c.fraction_left(t0) - 1.0).abs() < 1e-6);
        assert!((c.fraction_left(t0 + Duration::from_secs(5)) - 0.5).abs() < 1e-6);
        assert_eq!(c.fraction_left(t0 + Duration::from_secs(60)), 0.0);
        // A clock read before the card was shown (it never goes backwards, but nothing may panic).
        assert_eq!(c.left(t0.checked_sub(Duration::from_secs(1)).unwrap_or(t0)), WAIT);
    }

    #[test]
    fn opens_at_every_live_start_and_never_in_a_test_run() {
        assert!(at_start(true, None).is_some(), "a live run");
        assert!(at_start(false, None).is_none(), "a test run (--screenshot, --journal-dir)");
        assert!(at_start(false, Some("history")).is_none(), "another page's tab");
        let running = at_start(false, Some("support")).expect("--tab support");
        assert!(running.countdown.is_none(), "its countdown starts when it's drawn");
        let over = at_start(false, Some("support-close")).expect("--tab support-close");
        assert!(over.countdown.is_some_and(|c| c.done(Instant::now())), "Close showing");
    }
}
