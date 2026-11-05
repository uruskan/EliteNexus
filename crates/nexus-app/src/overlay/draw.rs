//! Paints the island: lays an [`IslandView`] out with `widgets::island`, and springs its shape from one size to the
//! next. Over the game (see-through) the island is a black shape that morphs. With see-through off it's an opaque
//! window of its own cut to the island's size ([`cutout`]), and on Windows to its shape; in VR an opaque black panel
//! of fixed size. Both crossfade their content. Nothing here reads the model.

use super::{Compact, Expanded, Glyph, IslandView, Moment, Piece, Row, Side, Size, Span, Style, Tint};
use crate::theme::pal;
use crate::widgets::island as paint;
use egui::emath::TSTransform;
use egui::{Align2, Color32, CornerRadius, LayerId, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2, ViewportCommand};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The see-through canvas, in the island's points: the island and its shadow fit with room to spare, so the window
/// never resizes while the island changes shape. With see-through off, the island's own window sits where the island
/// would be in it.
pub const CANVAS: Vec2 = Vec2::new(440.0, 260.0);
/// VR: an opaque canvas of fixed size, as wide as the open island with room around it.
pub const VR_CANVAS: Vec2 = Vec2::new(440.0, 180.0);
/// Where the open island's top-left sits in the canvas, in the island's points. With see-through off, earlier versions
/// drew a panel the open island's width and kept its top-left as the place it was dragged to.
pub const PANEL_OFFSET: Vec2 = Vec2::new((CANVAS.x - WIDE) / 2.0, TOP);
/// Unlocked with see-through off, an island with nothing to say says this instead.
const HINT: &str = "Drag to move it · click to open its page";
/// The island's top below the canvas's, and its inset from the sides when it sits to the left or right.
const TOP: f32 = 12.0;
const INSET: f32 = 16.0;
/// The expanded island's width, padding, gap between rows, and corner radius at most.
const WIDE: f32 = 408.0;
const PAD_X: f32 = 24.0;
const PAD_Y: f32 = 18.0;
const ROW_GAP: f32 = 14.0;
const CORNER: f32 = 44.0;
/// The capsule's height, the minimal island's width, and the space between pieces.
const TALL: f32 = 37.0;
const MINIMAL_W: f32 = 126.0;
const GAP: f32 = 7.0;
/// The side capsule's distance from the island.
const SIDE_GAP: f32 = 8.0;
/// The morph's length, and how long the island takes to fade in and out.
const SPRING: f64 = 0.6;
const FADE_IN: f64 = 0.2;
const FADE_OUT: f64 = 0.3;

/// Where the island sits along the top of the game's screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Anchor {
    #[default]
    Centre,
    Left,
    Right,
}

/// How the island looks (Settings › Overlay).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// The overlay's own scale, 1 to 2.5.
    pub scale: f32,
    /// A see-through window with the island alone, morphing; otherwise an opaque black panel.
    pub see_through: bool,
    /// VR: an opaque panel that never hides, nothing under 14 pt.
    pub vr: bool,
    pub anchor: Anchor,
    /// Clicks go through to the game. Unlocked, it can be dragged, and a click opens its page.
    pub locked: bool,
}

impl Look {
    /// The canvas the island is placed in, in the island's points: the see-through window, or VR's panel. With
    /// see-through off the window is the [`cutout`] of the island in it.
    pub fn canvas(&self) -> Vec2 {
        if self.vr { VR_CANVAS } else { CANVAS }
    }

    fn morphs(&self) -> bool {
        self.see_through && !self.vr
    }

    /// See-through off (and not VR): the island is a window of its own, cut to its size.
    pub fn cut(&self) -> bool {
        !self.see_through && !self.vr
    }

    fn min_text(&self) -> f32 {
        if self.vr { 14.0 } else { 12.0 }
    }
}

/// The morph's spring at `t` seconds: 0 at the start, 1 from 0.6 s on, overshooting by about 2 % on the way.
pub fn spring(t: f64) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= SPRING {
        return 1.0;
    }
    // A damped spring: damping ratio 0.79, natural frequency 13 rad/s.
    let (decay, freq) = (10.3, 7.94);
    (1.0 - (-decay * t).exp() * ((freq * t).cos() + decay / freq * (freq * t).sin())) as f32
}

/// A value going from `from` to `to` since `start`.
#[derive(Debug, Clone, Copy, Default)]
struct Tween {
    from: f32,
    to: f32,
    start: f64,
}

impl Tween {
    /// Linear over `secs`.
    fn linear(&self, t: f64, secs: f64) -> f32 {
        let f = ((t - self.start) / secs).clamp(0.0, 1.0) as f32;
        self.from + (self.to - self.from) * f
    }

    fn sprung(&self, t: f64) -> f32 {
        self.from + (self.to - self.from) * spring(t - self.start)
    }

    fn set(&mut self, to: f32, now: f32, t: f64) {
        if to != self.to {
            *self = Tween { from: now, to, start: t };
        }
    }
}

/// The island's outline, in its points: size, corner radius, and how far its centre moves sideways to make room for
/// the side capsule.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Outline {
    w: f32,
    h: f32,
    r: f32,
    shift: f32,
}

impl Outline {
    fn lerp(a: Outline, b: Outline, f: f32) -> Outline {
        let l = |x: f32, y: f32| x + (y - x) * f;
        Outline { w: l(a.w, b.w), h: l(a.h, b.h), r: l(a.r, b.r), shift: l(a.shift, b.shift) }
    }

    const MINIMAL: Outline = Outline { w: MINIMAL_W, h: TALL, r: TALL / 2.0, shift: 0.0 };
}

/// The island's motion, kept between the overlay's frames.
#[derive(Debug, Default)]
pub struct Anim {
    from: Outline,
    to: Outline,
    start: f64,
    started: bool,
    /// The island's opacity.
    fade: Tween,
    /// What's inside (its size and moment), and when that changed.
    content: Option<(Size, Option<Moment>)>,
    content_start: f64,
    /// The side capsule, 0 (hidden) to 1.
    side: Tween,
}

impl Anim {
    fn outline(&self, t: f64) -> Outline {
        Outline::lerp(self.from, self.to, spring(t - self.start))
    }
}

/// What happened in the overlay this frame.
#[derive(Debug, Default)]
pub struct Output {
    /// Still moving: paint again next frame.
    pub animating: bool,
    /// The island has faded away: nothing is drawn.
    pub faded_out: bool,
    pub clicked: bool,
    /// The window's shape (Windows): the island's parts in the window, in points, once the window has the size they
    /// were laid out for; empty for its whole rectangle (see-through, VR); None while it's still being resized.
    pub shape: Option<Vec<(Rect, f32)>>,
}

/// Lays out and paints the island's parts with one painter.
struct Lay {
    p: Painter,
    k: f32,
    min: f32,
}

impl Lay {
    fn galley(&self, s: &Span, max_w: f32) -> Arc<egui::Galley> {
        paint::text(&self.p, &s.text, paint::font(s.style, self.k, self.min), paint::tint(s.tint), s.style != Style::Title, max_w)
    }

    fn piece_width(&self, piece: &Piece) -> f32 {
        match piece {
            Piece::Text(s) => self.galley(s, f32::INFINITY).size().x,
            Piece::Glyph(g) => paint::glyph_width(g, self.k),
            Piece::Fuel(_) => 56.0 * self.k,
        }
    }

    fn width(&self, pieces: &[Piece]) -> f32 {
        pieces.iter().map(|p| self.piece_width(p)).sum::<f32>() + GAP * self.k * pieces.len().saturating_sub(1) as f32
    }

    /// Paints pieces left to right from `x`, centred on `y`, text cut short past `max_w`. Returns the right edge.
    fn pieces(&self, pieces: &[Piece], x: f32, y: f32, max_w: f32) -> f32 {
        let (k, end) = (self.k, x + max_w);
        let mut x = x;
        for (i, piece) in pieces.iter().enumerate() {
            if i > 0 {
                x += GAP * k;
            }
            match piece {
                Piece::Text(s) => {
                    let g = self.galley(s, (end - x).max(0.0));
                    let size = g.size();
                    self.p.galley(Pos2::new(x, y - size.y / 2.0), g, paint::tint(s.tint));
                    x += size.x;
                }
                Piece::Glyph(g) => {
                    let w = paint::glyph_width(g, k);
                    paint::glyph(&self.p, g, Pos2::new(x + w / 2.0, y), k);
                    x += w;
                }
                Piece::Fuel(pct) => {
                    paint::fuel(&self.p, Rect::from_min_size(Pos2::new(x, y - 3.0 * k), Vec2::new(56.0 * k, 6.0 * k)), *pct);
                    x += 56.0 * k;
                }
            }
        }
        x
    }

    /// The compact island's width, in its points.
    fn compact_width(&self, c: &Compact) -> f32 {
        (9.0 + 14.0 + 16.0 + (self.width(&c.lead) + self.width(&c.trail)) / self.k).clamp(196.0, 300.0)
    }

    /// The compact row: the lead pieces from the left, the trail ones against the right.
    fn compact(&self, c: &Compact, rect: Rect) {
        let k = self.k;
        let y = rect.center().y;
        let trail = self.width(&c.trail).min(rect.width() * 0.6);
        self.pieces(&c.trail, rect.right() - 14.0 * k - trail, y, trail);
        self.pieces(&c.lead, rect.left() + 9.0 * k, y, rect.width() - 23.0 * k - trail - 12.0 * k);
    }

    /// The side capsule's width, in the island's points.
    fn side_width(&self, s: &Side) -> f32 {
        24.0 + (paint::glyph_width(&side_glyph(s), self.k) + self.width(&side_pieces(s)) + GAP * self.k) / self.k
    }

    fn side(&self, s: &Side, rect: Rect) {
        paint::shape(&self.p, rect, rect.height() / 2.0, true, self.k);
        self.side_content(s, rect);
    }

    /// What the side capsule says, without its shape.
    fn side_content(&self, s: &Side, rect: Rect) {
        let k = self.k;
        let g = side_glyph(s);
        let w = paint::glyph_width(&g, k);
        paint::glyph(&self.p, &g, Pos2::new(rect.left() + 10.0 * k + w / 2.0, rect.center().y), k);
        self.pieces(&side_pieces(s), rect.left() + 10.0 * k + w + GAP * k, rect.center().y, rect.width());
    }

    /// The expanded island's rows inside `rect` (its padding already taken off).
    fn rows(&self, e: &Expanded, rect: Rect) {
        let k = self.k;
        let mut y = rect.top();
        for row in &e.rows {
            let h = row_height(row) * k;
            self.row(row, Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(rect.width(), h)));
            y += h + ROW_GAP * k;
        }
    }

    fn row(&self, row: &Row, rect: Rect) {
        let k = self.k;
        let cy = rect.center().y;
        match row {
            Row::Header { lead, top, bottom, right } => {
                let mut x = rect.left();
                if let Some(g) = lead {
                    let w = paint::glyph_width(g, k);
                    paint::glyph(&self.p, g, Pos2::new(x + w / 2.0, cy), k);
                    x += w + 14.0 * k;
                }
                let right_w = right.iter().map(|l| self.width(l)).fold(0.0, f32::max);
                let room = (rect.right() - x - if right_w > 0.0 { right_w + 14.0 * k } else { 0.0 }).max(0.0);
                let (a, b) = (self.galley(top, room), self.galley(bottom, room));
                let gap = 2.0 * k;
                let ty = cy - (a.size().y + b.size().y + gap) / 2.0;
                let hb = a.size().y;
                self.p.galley(Pos2::new(x, ty), a, paint::tint(top.tint));
                self.p.galley(Pos2::new(x, ty + hb + gap), b, paint::tint(bottom.tint));
                let heights: Vec<f32> = right.iter().map(|l| self.line_height(l)).collect();
                let mut ry = cy - (heights.iter().sum::<f32>() + gap * heights.len().saturating_sub(1) as f32) / 2.0;
                for (line, h) in right.iter().zip(heights) {
                    let w = self.width(line);
                    self.pieces(line, rect.right() - w, ry + h / 2.0, w);
                    ry += h + gap;
                }
            }
            Row::Spread(groups) => {
                let mut widths: Vec<f32> = groups.iter().map(|g| self.width(g)).collect();
                let n = groups.len();
                // Too wide: the first group (a name, say) is cut short, so the values after it stay whole.
                let others: f32 = widths.iter().skip(1).sum::<f32>() + 12.0 * k * n.saturating_sub(1) as f32;
                let mut cut = None;
                if let Some(first) = widths.first_mut()
                    && *first > rect.width() - others
                {
                    *first = (rect.width() - others).max(0.0);
                    cut = Some(*first);
                }
                let gap = if n > 1 { ((rect.width() - widths.iter().sum::<f32>()) / (n - 1) as f32).max(12.0 * k) } else { 0.0 };
                let mut x = rect.left();
                for (i, (g, w)) in groups.iter().zip(widths).enumerate() {
                    let room = if i == 0 { cut.unwrap_or(rect.right() - x) } else { rect.right() - x };
                    self.pieces(g, x, cy, room.max(0.0));
                    x += w + gap;
                }
            }
            Row::Chips(chips) => {
                let mut x = rect.left();
                for c in chips {
                    if x >= rect.right() {
                        break;
                    }
                    x += paint::chip(&self.p, Pos2::new(x, cy), &c.text, c.done, k, self.min) + 6.0 * k;
                }
            }
            Row::Dial { dial, lines, ring } => {
                let r = rect.height() / 2.0;
                paint::dial(&self.p, Pos2::new(rect.left() + r, cy), r, dial, k);
                let mut right = rect.right();
                if let Some((done, of)) = ring {
                    let rc = Pos2::new(rect.right() - 20.0 * k, cy - 9.0 * k);
                    paint::ring(&self.p, rc, 18.0 * k, *done, *of, k);
                    let label = self.galley(&Span { text: "samples".into(), style: Style::Caption, tint: Tint::Dim }, f32::INFINITY);
                    self.p.galley(Pos2::new(rc.x - label.size().x / 2.0, rc.y + 24.0 * k), label, pal::ISLAND_DIM);
                    right -= 40.0 * k + 16.0 * k;
                }
                let x = rect.left() + 2.0 * r + 16.0 * k;
                let room = (right - x).max(0.0);
                // A big value that doesn't fit drops to the smaller figure size.
                let fitted: Vec<Vec<Piece>> = lines
                    .iter()
                    .map(|l| {
                        l.iter()
                            .map(|p| match p {
                                Piece::Text(s) if s.style == Style::Hero && self.piece_width(p) > room => Piece::Text(Span { style: Style::Figure, ..s.clone() }),
                                other => other.clone(),
                            })
                            .collect()
                    })
                    .collect();
                let heights: Vec<f32> = fitted.iter().map(|l| self.line_height(l)).collect();
                let gap = 3.0 * k;
                let mut y = cy - (heights.iter().sum::<f32>() + gap * heights.len().saturating_sub(1) as f32) / 2.0;
                for (line, h) in fitted.iter().zip(heights) {
                    self.pieces(line, x, y + h / 2.0, room);
                    y += h + gap;
                }
            }
        }
    }

    fn line_height(&self, pieces: &[Piece]) -> f32 {
        pieces
            .iter()
            .map(|p| match p {
                Piece::Text(s) => self.galley(s, f32::INFINITY).size().y,
                Piece::Glyph(g) => paint::glyph_width(g, self.k),
                Piece::Fuel(_) => 6.0 * self.k,
            })
            .fold(0.0, f32::max)
    }
}

fn side_glyph(s: &Side) -> Glyph {
    Glyph::Icon { icon: s.icon, tint: s.tint, size: 16 }
}

fn side_pieces(s: &Side) -> Vec<Piece> {
    vec![Piece::Text(Span { text: s.label.clone(), style: Style::Label, tint: Tint::Dim }), Piece::Text(Span { text: s.value.clone(), style: Style::Value, tint: s.tint })]
}

/// A row's height, in the island's points.
fn row_height(row: &Row) -> f32 {
    match row {
        Row::Header { lead: Some(Glyph::Star { size, .. }), .. } => f32::from(*size).max(44.0),
        Row::Header { .. } => 44.0,
        Row::Spread(_) => 20.0,
        Row::Chips(_) => 22.0,
        Row::Dial { .. } => 88.0,
    }
}

/// The expanded island's height, in its points: 84 to 168.
fn expanded_height(e: &Expanded) -> f32 {
    let rows: f32 = e.rows.iter().map(row_height).sum();
    (2.0 * PAD_Y + rows + ROW_GAP * e.rows.len().saturating_sub(1) as f32).clamp(84.0, 168.0)
}

/// The outline a view asks for.
fn outline(lay: &Lay, view: &IslandView, size: Size, anchor: Anchor) -> Outline {
    match (size, &view.compact, &view.expanded) {
        (Size::Compact, Some(c), _) => {
            let side = view.side.as_ref().map_or(0.0, |s| SIDE_GAP + lay.side_width(s));
            let shift = match anchor {
                Anchor::Centre => -side / 2.0,
                Anchor::Left => 0.0,
                Anchor::Right => -side,
            };
            Outline { w: lay.compact_width(c), h: TALL, r: TALL / 2.0, shift }
        }
        (Size::Expanded, _, Some(e)) => {
            let h = expanded_height(e);
            Outline { w: WIDE, h, r: (h / 2.0).min(CORNER), shift: 0.0 }
        }
        _ => Outline::MINIMAL,
    }
}

/// Where the island is in the canvas.
fn island_rect(canvas: Rect, o: Outline, anchor: Anchor, k: f32) -> Rect {
    let (w, h) = (o.w * k, o.h * k);
    let x = match anchor {
        Anchor::Centre => canvas.center().x,
        Anchor::Left => canvas.left() + INSET * k + w / 2.0,
        Anchor::Right => canvas.right() - INSET * k - w / 2.0,
    } + o.shift * k;
    Rect::from_center_size(Pos2::new(x, canvas.top() + TOP * k + h / 2.0), Vec2::new(w, h))
}

/// How much of the island shows: unlocked, something stays there to take hold of.
fn shown(view: &IslandView, look: &Look) -> Size {
    match view.size {
        Size::Gone if !look.locked => Size::Minimal,
        s => s,
    }
}

/// The island as a window of its own (see-through off), in egui points: where that window sits in the see-through
/// canvas, and its parts, the island and the side capsule beside it, with their corner radii. On Windows the window
/// takes the parts' shape; elsewhere it's the rectangle around them.
#[derive(Debug, Clone, PartialEq)]
pub struct Cutout {
    /// The window, in the canvas (whose top-left is 0, 0), in whole points.
    pub rect: Rect,
    /// The island and the side capsule in the window, with their corner radii.
    pub parts: Vec<(Rect, f32)>,
}

/// The window a view asks for with see-through off; None when nothing shows (the window keeps its last size).
/// Measured with the main window's fonts, the same as the overlay's own frames lay it out with.
pub fn cutout(ctx: &egui::Context, view: &IslandView, look: &Look) -> Option<Cutout> {
    let size = shown(view, look);
    if size == Size::Gone {
        return None;
    }
    let lay = Lay { p: Painter::new(ctx.clone(), LayerId::background(), Rect::EVERYTHING), k: scale_of(ctx, look), min: look.min_text() };
    Some(cut(&lay, view, size, look))
}

fn cut(lay: &Lay, view: &IslandView, size: Size, look: &Look) -> Cutout {
    let k = lay.k;
    let mut o = outline(lay, view, size, look.anchor);
    // Unlocked with nothing to say, the island says how to move it.
    if size == Size::Minimal && !look.locked {
        o.w = o.w.max(lay.galley(&hint(), f32::INFINITY).size().x / k + 32.0);
    }
    let canvas = Rect::from_min_size(Pos2::ZERO, CANVAS * k);
    let island = island_rect(canvas, o, look.anchor, k);
    let mut parts = vec![(island, o.r * k)];
    if let Some(side) = view.side.as_ref().filter(|_| size == Size::Compact) {
        parts.push((Rect::from_min_size(Pos2::new(island.right() + SIDE_GAP * k, island.top()), Vec2::new(lay.side_width(side) * k, TALL * k)), TALL * k / 2.0));
    }
    let around = parts.iter().fold(Rect::NOTHING, |b, (r, _)| b.union(*r));
    // Whole points around it, not counting float noise (15.99998 is 16, not 15 and a sliver of black).
    let rect = Rect::from_min_max((around.min + Vec2::splat(0.01)).floor(), (around.max - Vec2::splat(0.01)).ceil());
    Cutout { rect, parts: parts.into_iter().map(|(r, radius)| (r.translate(-rect.min.to_vec2()), radius)).collect() }
}

fn hint() -> Span {
    Span { text: HINT.into(), style: Style::Caption, tint: Tint::Dim }
}

/// Paints what's inside the island, laid out for its full size `rect`.
fn content(lay: &Lay, view: &IslandView, size: Size, rect: Rect) {
    let k = lay.k;
    match (size, &view.compact, &view.expanded) {
        (Size::Compact, Some(c), _) => lay.compact(c, rect),
        (Size::Expanded, _, Some(e)) => lay.rows(e, rect.shrink2(Vec2::new(PAD_X * k, PAD_Y * k))),
        _ => {}
    }
}

/// The scale the island draws at: the overlay's own, independent of the main window's zoom.
pub fn scale_of(ctx: &egui::Context, look: &Look) -> f32 {
    look.scale / ctx.zoom_factor()
}

/// Paints the island for this frame and moves it on. `ui` covers the overlay's whole window.
pub fn island(ui: &mut Ui, view: &IslandView, look: &Look, anim: &mut Anim) -> Output {
    let t = ui.input(|i| i.time);
    let k = scale_of(ui.ctx(), look);
    let canvas = ui.max_rect();
    let lay = Lay { p: ui.painter().clone(), k, min: look.min_text() };
    let mut out = Output::default();
    // Unlocked, it can be moved, and a click opens its page.
    let size = shown(view, look);
    if !look.locked {
        let resp = ui.interact(canvas, ui.id().with("island"), Sense::click_and_drag());
        if resp.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        out.clicked = resp.clicked();
    }
    let gone = size == Size::Gone;
    let key = (size, view.moment);
    if anim.content != Some(key) {
        anim.content = Some(key);
        anim.content_start = t;
    }
    let since = t - anim.content_start;
    let alpha_now = anim.fade.linear(t, if anim.fade.to > anim.fade.from { FADE_IN } else { FADE_OUT });
    anim.fade.set(if gone { 0.0 } else { 1.0 }, alpha_now, t);
    let alpha = anim.fade.linear(t, if anim.fade.to > anim.fade.from { FADE_IN } else { FADE_OUT });
    let fading = t - anim.fade.start < FADE_OUT;
    if look.morphs() {
        let target = outline(&lay, view, size, look.anchor);
        if !anim.started {
            // It appears from the minimal capsule.
            *anim = Anim { from: Outline::MINIMAL, to: target, start: t, started: true, ..std::mem::take(anim) };
        } else if target != anim.to {
            anim.from = anim.outline(t);
            anim.to = target;
            anim.start = t;
        }
        let o = anim.outline(t);
        let side_now = anim.side.sprung(t);
        anim.side.set(if view.side.is_some() && size == Size::Compact { 1.0 } else { 0.0 }, side_now, t);
        if alpha > 0.0 {
            let rect = island_rect(canvas, o, look.anchor, k);
            let mut p = lay.p.clone();
            p.multiply_opacity(alpha);
            paint::shape(&p, rect, o.r * k, true, k);
            // The content is laid out for the size it's heading to, scales in from 94 % and fades in as the shape settles.
            let full = island_rect(canvas, anim.to, look.anchor, k);
            let fade_in = ((since - 0.08) / 0.22).clamp(0.0, 1.0) as f32;
            let s = 0.94 + 0.06 * spring(since);
            ui.with_visual_transform(TSTransform::new(rect.center().to_vec2() * (1.0 - s), s), |ui| {
                let mut cp = ui.painter().with_clip_rect(rect);
                cp.multiply_opacity(alpha * fade_in);
                content(&Lay { p: cp, k, min: lay.min }, view, size, Rect::from_center_size(rect.center(), full.size()));
            });
            if let Some(side) = &view.side {
                let v = anim.side.sprung(t).clamp(0.0, 1.1);
                if v > 0.01 {
                    let w = lay.side_width(side) * k;
                    let left = rect.right() + SIDE_GAP * k - 40.0 * k * (1.0 - v);
                    let r = Rect::from_min_size(Pos2::new(left, rect.top()), Vec2::new(w, TALL * k));
                    let sc = 0.7 + 0.3 * v;
                    ui.with_visual_transform(TSTransform::new(r.center().to_vec2() * (1.0 - sc), sc), |ui| {
                        let mut sp = ui.painter().clone();
                        sp.multiply_opacity(alpha * v.min(1.0));
                        Lay { p: sp, k, min: lay.min }.side(side, r);
                    });
                }
            }
        }
        out.animating = t - anim.start < SPRING || since < SPRING || fading || t - anim.side.start < SPRING;
    } else {
        // Opaque: the window is black throughout (on Windows cut to the island's shape), and the content crossfades.
        lay.p.rect_filled(canvas, CornerRadius::ZERO, pal::ISLAND);
        let fade_in = (since / 0.25).clamp(0.0, 1.0) as f32;
        let mut cp = lay.p.clone();
        cp.multiply_opacity(alpha * fade_in);
        let panel = Lay { p: cp, k, min: lay.min };
        if look.vr {
            vr_panel(&panel, view, size, canvas);
        } else {
            let c = cut(&lay, view, size, look);
            let parts = placed(&c, canvas);
            cut_content(&panel, view, size, &parts, look);
            if !look.locked {
                for (r, radius) in &parts {
                    lay.p.rect_stroke(*r, CornerRadius::same(radius.ceil().clamp(0.0, 255.0) as u8), Stroke::new(1.5 * k, pal::ACCENT), egui::StrokeKind::Inside);
                }
            }
            // The shape goes with the size it was cut for (fading out, the window keeps the size and shape it has).
            let sized = (canvas.size() - c.rect.size()).abs().max_elem() < 1.0;
            if sized && !gone {
                out.shape = Some(c.parts.clone());
            }
        }
        out.animating = since < 0.25 || fading;
    }
    if !look.cut() {
        out.shape = Some(Vec::new());
    }
    if !look.locked && !look.cut() {
        lay.p.rect_stroke(canvas.shrink(k), CornerRadius::same((8.0 * k).min(255.0) as u8), Stroke::new(1.5 * k, pal::ACCENT), egui::StrokeKind::Inside);
        let g = lay.galley(&hint(), canvas.width() - 24.0 * k);
        let at = Pos2::new(canvas.center().x - g.size().x / 2.0, canvas.bottom() - g.size().y - 8.0 * k);
        lay.p.rect_filled(Rect::from_min_size(at, g.size()).expand(4.0 * k), CornerRadius::same((6.0 * k).min(255.0) as u8), pal::ISLAND);
        lay.p.galley(at, g, pal::ISLAND_DIM);
    }
    out.faded_out = gone && alpha <= 0.0 && !fading;
    out
}

/// A cutout's parts placed in a window whose top-left is `window.min`, and cut to it: while the window is still being
/// resized to a new cutout (a frame at most), the content fits the window it has.
fn placed(c: &Cutout, window: Rect) -> Vec<(Rect, f32)> {
    c.parts.iter().map(|(r, radius)| (r.translate(window.min.to_vec2()).intersect(window), *radius)).collect()
}

/// What the island's own window shows in its parts: the island's content, the side capsule's, and, unlocked with
/// nothing to say, how to move it.
fn cut_content(lay: &Lay, view: &IslandView, size: Size, parts: &[(Rect, f32)], look: &Look) {
    let Some((island, _)) = parts.first() else { return };
    content(lay, view, size, *island);
    if let (Some(side), Some((r, _))) = (view.side.as_ref().filter(|_| size == Size::Compact), parts.get(1)) {
        lay.side_content(side, *r);
    }
    if size == Size::Minimal && !look.locked {
        let g = lay.galley(&hint(), island.width() - 24.0 * lay.k);
        lay.p.galley(island.center() - g.size() / 2.0, g, pal::ISLAND_DIM);
    }
}

/// VR's panel of fixed size: the open island's rows, or the compact row and the side capsule's words under it.
fn vr_panel(panel: &Lay, view: &IslandView, size: Size, canvas: Rect) {
    let k = panel.k;
    match size {
        Size::Compact => {
            let row = Rect::from_center_size(canvas.center(), Vec2::new(canvas.width() - 30.0 * k, TALL * k));
            if let Some(c) = &view.compact {
                panel.compact(c, row);
            }
            if let Some(side) = &view.side {
                let w = panel.width(&side_pieces(side));
                panel.pieces(&side_pieces(side), canvas.right() - 24.0 * k - w, row.bottom() + 16.0 * k, w);
            }
        }
        Size::Expanded => {
            if let Some(e) = &view.expanded {
                let h = expanded_height(e).min(canvas.height() / k) * k;
                panel.rows(e, Rect::from_center_size(canvas.center(), Vec2::new(canvas.width(), h)).shrink2(Vec2::new(PAD_X * k, PAD_Y * k)));
            }
        }
        Size::Minimal | Size::Gone => {}
    }
}

/// `--overlay <moment>`: the moment compact and expanded as the island draws it over the game (see-through), as its
/// own window (see-through off; the outline of that window dashed), and in VR's panel, for screenshots of every state.
/// A grey stand-in for the game sits behind each, so the island's edges show.
pub fn stage(ui: &mut Ui, name: &str, views: &(IslandView, IslandView), look: &Look) {
    use crate::theme::{display, size as type_size};
    let k = scale_of(ui.ctx(), look);
    let area = ui.max_rect();
    let p = ui.painter().clone();
    p.rect_filled(area, CornerRadius::ZERO, Color32::from_rgb(4, 6, 12));
    p.text(area.left_top() + Vec2::new(24.0, 18.0), Align2::LEFT_TOP, format!("Overlay · {name}"), display(type_size::HEADLINE), pal::TEXT);
    let lay = Lay { p: p.clone(), k, min: look.min_text() };
    let caption = |at: Pos2, text: &str| p.text(at, Align2::LEFT_TOP, text, display(type_size::CAPTION), pal::DIM);
    let game = Color32::from_rgb(52, 58, 72);
    let column = (CANVAS.x + 32.0) * k;
    let (compact, expanded) = views;
    let mut y = area.top() + 56.0;
    for (view, what) in [(compact, "compact"), (expanded, "open")] {
        let o = outline(&lay, view, view.size, Anchor::Centre);
        let band = Vec2::new(CANVAS.x, o.h + 2.0 * TOP + 12.0) * k;
        // Over the game: the see-through window, the island with its shadow.
        let canvas = Rect::from_min_size(Pos2::new(area.left() + 24.0, y + 20.0), CANVAS * k);
        caption(Pos2::new(canvas.left(), y), &format!("See-through, {what}"));
        p.rect_filled(Rect::from_min_size(canvas.min, band), CornerRadius::same(6), game);
        let rect = island_rect(canvas, o, Anchor::Centre, k);
        if view.size != Size::Gone {
            paint::shape(&p, rect, o.r * k, true, k);
            content(&Lay { p: p.with_clip_rect(rect), k, min: lay.min }, view, view.size, rect);
        }
        if let Some(side) = view.side.as_ref().filter(|_| view.size == Size::Compact) {
            let r = Rect::from_min_size(Pos2::new(rect.right() + SIDE_GAP * k, rect.top()), Vec2::new(lay.side_width(side) * k, TALL * k));
            lay.side(side, r);
        }
        // See-through off: the island's own window, cut to its shape (Windows), its rectangle dashed.
        let cut_look = Look { see_through: false, vr: false, ..*look };
        let frame = canvas.translate(Vec2::new(column, 0.0));
        caption(Pos2::new(frame.left(), y), &format!("See-through off: its own window, {what}"));
        p.rect_filled(Rect::from_min_size(frame.min, band), CornerRadius::same(6), game);
        let size = shown(view, &cut_look);
        if size != Size::Gone {
            let c = cut(&lay, view, size, &cut_look);
            let window = c.rect.translate(frame.min.to_vec2());
            p.add(egui::Shape::dashed_line(&[window.left_top(), window.right_top(), window.right_bottom(), window.left_bottom(), window.left_top()], Stroke::new(1.0, pal::DIM), 4.0, 3.0));
            let parts = placed(&c, window);
            for (r, radius) in &parts {
                p.rect_filled(*r, CornerRadius::same(radius.ceil().clamp(0.0, 255.0) as u8), pal::ISLAND);
            }
            cut_content(&lay, view, size, &parts, &cut_look);
        }
        y = canvas.top() + band.y + 24.0;
    }
    // VR: an opaque panel of fixed size that never hides, showing the moment open.
    let vr = Rect::from_min_size(Pos2::new(area.left() + 24.0 + 2.0 * column, area.top() + 76.0), VR_CANVAS * k);
    caption(vr.left_top() - Vec2::new(0.0, 20.0), "VR mode: a panel of fixed size");
    p.rect_filled(vr, CornerRadius::ZERO, pal::ISLAND);
    let vr_lay = Lay { p: p.clone(), k, min: 14.0 };
    vr_panel(&vr_lay, expanded, expanded.size, vr);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spring_overshoots_a_little_and_settles() {
        assert_eq!(spring(0.0), 0.0);
        assert_eq!(spring(SPRING), 1.0);
        assert_eq!(spring(5.0), 1.0);
        let peak = (1..60).map(|i| spring(f64::from(i) / 100.0)).fold(0.0, f32::max);
        assert!((1.01..1.03).contains(&peak), "a slight overshoot: {peak}");
        assert!(spring(0.05) > 0.0 && spring(0.05) < 0.5);
        assert!((spring(0.59) - 1.0).abs() < 0.01, "almost there just before the end");
    }

    #[test]
    fn heights_stay_within_the_islands_range() {
        let alert = Expanded { rows: vec![Row::Header { lead: None, top: Span { text: "A".into(), style: Style::Title, tint: Tint::White }, bottom: Span { text: "B".into(), style: Style::Caption, tint: Tint::Dim }, right: Vec::new() }] };
        assert_eq!(expanded_height(&alert), 84.0, "an alert is the shortest");
        let many = Expanded { rows: vec![Row::Spread(Vec::new()); 9] };
        assert_eq!(expanded_height(&many), 168.0, "never taller than 168");
        for name in super::super::DEMOS {
            let (_, e) = super::super::demo(name).expect("demo");
            if let Some(x) = &e.expanded {
                let h = expanded_height(x);
                assert!((84.0..=168.0).contains(&h), "{name}: {h}");
            }
        }
    }

    /// Play-test, 2026-11-04: with see-through off the island was a 408 × 168 black panel, mostly empty for "K ·
    /// Scoopable · 1 jump left". Its own window now fits what it shows, where the island would be over the game.
    #[test]
    fn the_islands_own_window_fits_what_it_shows() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let locked = Look { scale: 1.0, see_through: false, vr: false, anchor: Anchor::Centre, locked: true };
        let mut checked = 0;
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx();
            for name in super::super::DEMOS {
                let (compact, expanded) = super::super::demo(name).expect("demo");
                for view in [&compact, &expanded] {
                    let c = cutout(ctx, view, &locked).expect("something shows");
                    let (island, radius) = c.parts[0];
                    assert!(c.rect.min.x >= 0.0 && c.rect.max.x <= CANVAS.x && (c.rect.min.y - TOP).abs() < 1.0, "{name}: inside the canvas, below its top: {:?}", c.rect);
                    for (r, _) in &c.parts {
                        assert!(r.min.x >= 0.0 && r.min.y >= 0.0 && r.max.x <= c.rect.width() + 0.01 && r.max.y <= c.rect.height() + 0.01, "{name}: {r:?} in {:?}", c.rect);
                    }
                    match view.size {
                        Size::Expanded => {
                            assert!(near(island.width(), WIDE), "{name}");
                            assert!((83.9..=168.1).contains(&island.height()), "{name}");
                            assert!(near(radius, (island.height() / 2.0).min(CORNER)), "{name}");
                        }
                        Size::Compact => {
                            assert!((195.9..=300.1).contains(&island.width()) && near(island.height(), TALL), "{name}: a capsule, {island:?}");
                            assert!(near(radius, TALL / 2.0), "{name}: fully rounded ends");
                        }
                        _ => assert!(near(island.width(), MINIMAL_W) && near(island.height(), TALL), "{name}"),
                    }
                    checked += 1;
                }
            }
            // A second thing beside it: two capsules, the pair centred.
            let (two, _) = super::super::demo("two").expect("demo");
            let c = cutout(ctx, &two, &locked).expect("shows");
            assert_eq!(c.parts.len(), 2);
            assert!((c.parts[1].0.left() - c.parts[0].0.right() - SIDE_GAP).abs() < 0.01);
            assert!((c.rect.center().x - CANVAS.x / 2.0).abs() <= 1.0, "{:?}", c.rect);
            // Nothing to say: nothing shows, and the window keeps its size. Unlocked, it says how to move it.
            let gone = IslandView::default();
            assert!(cutout(ctx, &gone, &locked).is_none());
            let unlocked = cutout(ctx, &gone, &Look { locked: false, ..locked }).expect("something to take hold of");
            assert!(unlocked.rect.width() > MINIMAL_W + 100.0, "wide enough for the hint: {:?}", unlocked.rect);
            // Anchored to a side, it keeps the island's inset.
            for name in ["jump", "docking", "two"] {
                let left = cutout(ctx, &compact_of(name), &Look { anchor: Anchor::Left, ..locked }).expect("shows");
                assert!((left.rect.min.x - INSET).abs() < 0.5, "{name}: {:?}", left.rect);
            }
        });
        output.drop_without_applying_deltas();
        assert_eq!(checked, 2 * super::super::DEMOS.len());
    }

    fn compact_of(name: &str) -> IslandView {
        super::super::demo(name).expect("demo").0
    }

    #[test]
    fn the_island_sits_where_it_is_anchored() {
        let canvas = Rect::from_min_size(Pos2::ZERO, CANVAS);
        let o = Outline { w: 200.0, h: TALL, r: TALL / 2.0, shift: 0.0 };
        assert_eq!(island_rect(canvas, o, Anchor::Centre, 1.0).center().x, 220.0);
        assert_eq!(island_rect(canvas, o, Anchor::Left, 1.0).left(), INSET);
        assert_eq!(island_rect(canvas, o, Anchor::Right, 1.0).right(), CANVAS.x - INSET);
        assert_eq!(island_rect(canvas, o, Anchor::Centre, 2.0).top(), 2.0 * TOP, "the scale applies to the margins too");
    }
}
