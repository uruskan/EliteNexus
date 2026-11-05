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
        Some(t) if t.eq_ignore_ascii_case("support-close") => Some(Card::finished()),
        _ if wanted(live_run) => Some(Card::default()),
        _ => None,
    }
}

/// The longest gap between two frames that counts in full: more means the card wasn't being drawn (the window was
/// minimised), and only this much of it counts.
const MAX_STEP: Duration = Duration::from_millis(100);

/// The 10 seconds, counted only while the card is in front of the commander: drawn, with Elite Nexus the window in
/// front. Minimised, behind the game or behind the welcome sheet, it waits.
#[derive(Clone, Copy, Default)]
pub struct Countdown {
    /// Time the card has been seen so far.
    seen: Duration,
    /// The last frame it was seen, to count from on the next.
    last: Option<Instant>,
}

impl Countdown {
    /// One frame of the card: `seen` when it's in front of the commander.
    pub fn tick(&mut self, now: Instant, seen: bool) {
        if seen && let Some(last) = self.last {
            self.seen = (self.seen + now.saturating_duration_since(last).min(MAX_STEP)).min(WAIT);
        }
        self.last = seen.then_some(now);
    }

    /// Time left before the card can be closed.
    pub fn left(&self) -> Duration {
        WAIT.saturating_sub(self.seen)
    }

    pub fn done(&self) -> bool {
        self.left().is_zero()
    }

    /// The whole seconds to show, rounded up: 10 at the start, 1 in the last second, 0 when it's over.
    pub fn seconds(&self) -> u64 {
        let left = self.left();
        left.as_secs() + u64::from(left.subsec_nanos() > 0)
    }

    /// How much of the wait is left, from 1 at the start to 0 at the end (the ring).
    pub fn fraction_left(&self) -> f32 {
        self.left().as_secs_f32() / WAIT.as_secs_f32()
    }
}

/// The card while it's open.
#[derive(Default)]
pub struct Card {
    countdown: Countdown,
}

impl Card {
    /// A card whose countdown is already over (screenshots of the Close button).
    fn finished() -> Self {
        Card { countdown: Countdown { seen: WAIT, last: None } }
    }

    /// Draws the card; true once Close was clicked.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        // Drawn means not minimised (the window draws nothing then); in front means focused (unknown counts as yes).
        let in_front = ctx.input(|i| i.viewport().focused.unwrap_or(true) && i.viewport().minimized != Some(true));
        self.countdown.tick(Instant::now(), in_front);
        let countdown = self.countdown;
        let done = countdown.done();
        if !done && in_front {
            // The ring empties smoothly; once Close shows nothing moves, and behind the game it waits without
            // drawing (coming to the front repaints by itself).
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
                        "I never started it to earn money, and nothing in it depends on this: no ads, no subscription. But it takes more of my time every week, much of it on things I don't need myself, and your support is what keeps me motivated.",
                        "It also buys what I can't test yet: I don't own a Nomad, an Anaconda Mk II or a Rhino, so working on those parts is really miserable for me right now.",
                    ] {
                        ui.add(egui::Label::new(RichText::new(line).size(size::CALLOUT).color(pal::DIM)).wrap().halign(Align::Center));
                        ui.add_space(6.0);
                    }
                });
                ui.add_space(14.0);
                support_buttons(ui);
                ui.add_space(18.0);
                thanks_lines(ui, &THANKS);
                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.set_min_height(30.0);
                    // Room on the right for the countdown or Close, whichever is wider.
                    let note = (ui.available_width() - 190.0).max(120.0);
                    ui.allocate_ui(Vec2::new(note, 30.0), |ui| {
                        ui.add(
                            egui::Label::new(RichText::new("Shown at every start. A later version will skip it for supporters and everyone who bought a coffee.").size(size::FOOTNOTE).color(pal::FAINT))
                                .wrap(),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if done {
                            closed = widgets::primary_button(ui, "Close").clicked();
                        } else {
                            ring(ui, &countdown);
                            ui.label(RichText::new("You can close this in").size(size::FOOTNOTE).color(pal::DIM));
                        }
                    });
                });
            });
        closed
    }
}

/// The README, whose Thanks section names the supporters (the release notes thank the same people).
const README: &str = include_str!("../../../README.md");

static THANKS: std::sync::LazyLock<Thanks> = std::sync::LazyLock::new(|| thanks(README));

/// Who to thank, from README › Thanks, as the release notes read it: under **Sponsors**, every GitHub profile linked
/// and every item that starts with a bold name without a link; under the other headings, every GitHub profile linked
/// (code, bug reports and ideas). Each name once, ignoring case, a supporter only among the supporters.
#[derive(Debug, Default, PartialEq)]
pub struct Thanks {
    pub supporters: Vec<String>,
    pub others: Vec<String>,
}

pub fn thanks(readme: &str) -> Thanks {
    let mut t = Thanks::default();
    let mut seen = std::collections::HashSet::new();
    let mut add = |list: &mut Vec<String>, name: &str| {
        if seen.insert(name.to_ascii_lowercase()) {
            list.push(name.to_string());
        }
    };
    let mut in_thanks = false;
    let mut sponsors = false;
    for line in readme.lines() {
        if line.starts_with("## ") {
            if in_thanks {
                break;
            }
            in_thanks = line.starts_with("## Thanks");
            continue;
        }
        if !in_thanks {
            continue;
        }
        let trimmed = line.trim();
        if let Some(group) = trimmed.strip_prefix("**").and_then(|s| s.strip_suffix("**"))
            && !group.contains(['*', '['])
        {
            sponsors = group.starts_with("Sponsor");
            continue;
        }
        let logins = github_logins(line);
        if sponsors {
            for l in &logins {
                add(&mut t.supporters, l);
            }
            // A bold name without a link (a supporter from elsewhere, added by hand).
            if let Some(rest) = trimmed.strip_prefix("* **").or_else(|| trimmed.strip_prefix("- **"))
                && let Some((name, _)) = rest.split_once("**")
                && !name.is_empty()
                && !name.contains(['[', ']'])
            {
                add(&mut t.supporters, name);
            }
        } else {
            for l in &logins {
                add(&mut t.others, l);
            }
        }
    }
    t
}

/// The GitHub logins a line links to (`](https://github.com/<login>)`), in order.
fn github_logins(line: &str) -> Vec<&str> {
    const LINK: &str = "](https://github.com/";
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find(LINK) {
        rest = &rest[i + LINK.len()..];
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(rest.len());
        if end > 0 && rest[end..].starts_with(')') {
            out.push(&rest[..end]);
        }
        rest = &rest[end..];
    }
    out
}

/// "Elite Nexus is possible because of its supporters", with their names, and the other commanders under it.
fn thanks_lines(ui: &mut Ui, t: &Thanks) {
    let dim = egui::TextFormat { font_id: egui::FontId::proportional(size::FOOTNOTE), color: pal::DIM, ..Default::default() };
    let name_fmt = egui::TextFormat { font_id: display(size::FOOTNOTE), color: pal::TEXT, ..Default::default() };
    ui.vertical_centered(|ui| {
        let mut job = egui::text::LayoutJob { halign: Align::Center, ..Default::default() };
        if t.supporters.is_empty() {
            job.append("Elite Nexus is possible because of its supporters: thank you to ", 0.0, dim.clone());
        } else {
            job.append("Elite Nexus is possible because of its supporters: thank you, ", 0.0, dim.clone());
            for name in &t.supporters {
                job.append(name, 0.0, name_fmt.clone());
                job.append(", ", 0.0, dim.clone());
            }
            job.append("and ", 0.0, dim.clone());
        }
        job.append("everyone who sponsors anonymously or bought a coffee!", 0.0, dim);
        ui.add(egui::Label::new(job).wrap());
        if !t.others.is_empty() {
            ui.add_space(4.0);
            let line = format!("Thanks also to {} for code, bug reports and ideas, and to many more on the Frontier Forums.", t.others.join(", "));
            ui.add(egui::Label::new(RichText::new(line).size(size::FOOTNOTE).color(pal::FAINT)).wrap().halign(Align::Center));
        }
    });
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
fn ring(ui: &mut Ui, countdown: &Countdown) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let r = 12.5;
    p.circle_stroke(c, r, Stroke::new(3.0, pal::SEPARATOR));
    let left = countdown.fraction_left();
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
    p.text(c, Align2::CENTER_CENTER, countdown.seconds().to_string(), display(size::CAPTION), pal::TEXT);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames every 50 ms from `from` for `ms`, seen or not; the time after the last frame.
    fn frames(c: &mut Countdown, from: Instant, ms: u64, seen: bool) -> Instant {
        let mut t = from;
        for _ in 0..ms / 50 {
            t += Duration::from_millis(50);
            c.tick(t, seen);
        }
        t
    }

    #[test]
    fn countdown_runs_ten_seconds_while_seen() {
        let t0 = Instant::now();
        let mut c = Countdown::default();
        c.tick(t0, true);
        assert_eq!(c.seconds(), 10, "starts at 10");
        assert!(!c.done());
        assert!((c.fraction_left() - 1.0).abs() < 1e-6);
        let t = frames(&mut c, t0, 50, true);
        assert_eq!(c.seconds(), 10, "rounded up");
        let t = frames(&mut c, t, 4_950, true);
        assert!((c.fraction_left() - 0.5).abs() < 1e-4, "half way at 5 s");
        let t = frames(&mut c, t, 4_200, true);
        assert_eq!(c.seconds(), 1, "the last second shows 1");
        let t = frames(&mut c, t, 750, true);
        assert!(!c.done(), "no Close before the 10 seconds");
        frames(&mut c, t, 50, true);
        assert!(c.done(), "Close at 10 seconds");
        assert_eq!(c.seconds(), 0);
        assert_eq!(c.fraction_left(), 0.0);
    }

    #[test]
    fn countdown_waits_while_the_card_isnt_seen() {
        let t0 = Instant::now();
        let mut c = Countdown::default();
        c.tick(t0, true);
        let t = frames(&mut c, t0, 3_000, true);
        assert_eq!(c.seconds(), 7);
        // Behind the game (not focused): frames run, nothing counts.
        let t = frames(&mut c, t, 60_000, false);
        assert_eq!(c.seconds(), 7, "waits while Elite Nexus isn't in front");
        // Minimised: no frames at all, then one a minute later, which counts no more than a frame's worth.
        c.tick(t, true);
        let t = t + Duration::from_secs(60);
        c.tick(t, true);
        assert_eq!(c.seconds(), 7, "a minute without frames isn't a minute seen");
        frames(&mut c, t, 7_000, true);
        assert!(c.done());
        // A clock read out of order never panics nor counts.
        let mut c = Countdown::default();
        c.tick(t0 + Duration::from_secs(1), true);
        c.tick(t0, true);
        assert_eq!(c.left(), WAIT);
    }

    #[test]
    fn thanks_come_from_the_readme_as_the_release_notes_read_them() {
        let readme = "## Support\n* **[notme](https://github.com/notme)**\n## Thanks\n\nIntro.\n\n**Code**\n\n\
            * **[coder](https://github.com/coder)**: a fix ([PR #2](https://github.com/uruskan/EliteNexus/pull/2)).\n\n\
            **Sponsors**\n\n* **[Backer](https://github.com/Backer)** (on the forums), and one anonymous.\n* **Coffee Friend**: a coffee.\n\n\
            **Bug reports and ideas on GitHub**\n\n* **[backer](https://github.com/backer)**: twice.\n* **[tester-1](https://github.com/tester-1)**: a report.\n\n\
            ## Licence\n* **[after](https://github.com/after)**\n";
        let t = thanks(readme);
        assert_eq!(t.supporters, ["Backer", "Coffee Friend"]);
        // A repository link isn't a profile, and a supporter is thanked once, as a supporter.
        assert_eq!(t.others, ["coder", "tester-1"]);
        assert_eq!(thanks("no thanks section"), Thanks::default());
        // The real README names its supporters.
        assert!(!THANKS.supporters.is_empty(), "README › Thanks › Sponsors");
    }

    #[test]
    fn opens_at_every_live_start_and_never_in_a_test_run() {
        assert!(at_start(true, None).is_some(), "a live run");
        assert!(at_start(false, None).is_none(), "a test run (--screenshot, --journal-dir)");
        assert!(at_start(false, Some("history")).is_none(), "another page's tab");
        let running = at_start(false, Some("support")).expect("--tab support");
        assert_eq!(running.countdown.seconds(), 10, "its countdown starts when it's seen");
        let over = at_start(false, Some("support-close")).expect("--tab support-close");
        assert!(over.countdown.done(), "Close showing");
    }
}
