//! Paints the island: lays an [`IslandView`] out with `widgets::island`, and springs its shape from one size to the
//! next. Over the game (see-through) the island is a black shape that morphs; in the fallback and in VR it's an opaque
//! black panel whose content crossfades. Nothing here reads the model.

use super::{Compact, Expanded, Glyph, IslandView, Moment, Piece, Row, Side, Size, Span, Style, Tint};
use crate::theme::pal;
use crate::widgets::island as paint;
use egui::emath::TSTransform;
use egui::{Align2, Color32, CornerRadius, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2, ViewportCommand};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The see-through canvas, in the island's points: the island and its shadow fit with room to spare, so the window
/// never resizes while the island changes shape.
pub const CANVAS: Vec2 = Vec2::new(440.0, 260.0);
/// The opaque fallback: a panel the size of the expanded island.
pub const PANEL: Vec2 = Vec2::new(408.0, 168.0);
/// VR: an opaque canvas of fixed size, a little roomier than the panel.
pub const VR_CANVAS: Vec2 = Vec2::new(440.0, 180.0);
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
    /// The window's size in the island's points.
    pub fn canvas(&self) -> Vec2 {
        if self.vr {
            VR_CANVAS
        } else if self.see_through {
            CANVAS
        } else {
            PANEL
        }
    }

    fn morphs(&self) -> bool {
        self.see_through && !self.vr
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
        let k = self.k;
        paint::shape(&self.p, rect, rect.height() / 2.0, true, k);
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
                let widths: Vec<f32> = groups.iter().map(|g| self.width(g)).collect();
                let n = groups.len();
                let gap = if n > 1 { ((rect.width() - widths.iter().sum::<f32>()) / (n - 1) as f32).max(12.0 * k) } else { 0.0 };
                let mut x = rect.left();
                for (g, w) in groups.iter().zip(widths) {
                    self.pieces(g, x, cy, (rect.right() - x).max(0.0));
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
    // Unlocked, it can be moved, and a click opens its page; something stays there to take hold of.
    let size = match view.size {
        Size::Gone if !look.locked => Size::Minimal,
        s => s,
    };
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
        // An opaque panel: the content crossfades.
        lay.p.rect_filled(canvas, CornerRadius::ZERO, pal::ISLAND);
        let fade_in = (since / 0.25).clamp(0.0, 1.0) as f32;
        let mut cp = lay.p.clone();
        cp.multiply_opacity(alpha * fade_in);
        let panel = Lay { p: cp, k, min: lay.min };
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
        out.animating = since < 0.25 || fading;
    }
    if !look.locked {
        lay.p.rect_stroke(canvas.shrink(k), CornerRadius::same((8.0 * k).min(255.0) as u8), Stroke::new(1.5 * k, pal::ACCENT), egui::StrokeKind::Inside);
        let hint = Span { text: "Drag to move it · click to open its page".into(), style: Style::Caption, tint: Tint::Dim };
        let g = lay.galley(&hint, canvas.width() - 24.0 * k);
        let at = Pos2::new(canvas.center().x - g.size().x / 2.0, canvas.bottom() - g.size().y - 8.0 * k);
        lay.p.rect_filled(Rect::from_min_size(at, g.size()).expand(4.0 * k), CornerRadius::same((6.0 * k).min(255.0) as u8), pal::ISLAND);
        lay.p.galley(at, g, pal::ISLAND_DIM);
    }
    out.faded_out = gone && alpha <= 0.0 && !fading;
    out
}

/// `--overlay <moment>`: the moment compact and expanded, as the island draws them over the game and as the opaque
/// panel shows them, for screenshots of every state.
pub fn stage(ui: &mut Ui, name: &str, views: &(IslandView, IslandView), look: &Look) {
    let k = scale_of(ui.ctx(), look);
    let area = ui.max_rect();
    let p = ui.painter().clone();
    p.rect_filled(area, CornerRadius::ZERO, Color32::from_rgb(4, 6, 12));
    p.text(area.left_top() + Vec2::new(24.0, 18.0), Align2::LEFT_TOP, format!("Overlay · {name}"), crate::theme::display(crate::theme::size::HEADLINE), pal::TEXT);
    let lay = Lay { p: p.clone(), k, min: look.min_text() };
    let (compact, expanded) = views;
    let mut y = area.top() + 56.0;
    for (view, caption) in [(compact, "Over the game, compact"), (expanded, "Over the game, expanded")] {
        let canvas = Rect::from_min_size(Pos2::new(area.left() + 24.0, y), CANVAS * k);
        p.text(canvas.left_top(), Align2::LEFT_TOP, caption, crate::theme::display(crate::theme::size::CAPTION), pal::DIM);
        let canvas = canvas.translate(Vec2::new(0.0, 20.0));
        let o = outline(&lay, view, view.size, Anchor::Centre);
        let rect = island_rect(canvas, o, Anchor::Centre, k);
        if view.size != Size::Gone {
            paint::shape(&p, rect, o.r * k, true, k);
            content(&Lay { p: p.with_clip_rect(rect), k, min: lay.min }, view, view.size, rect);
        }
        if let Some(side) = view.side.as_ref().filter(|_| view.size == Size::Compact) {
            let r = Rect::from_min_size(Pos2::new(rect.right() + SIDE_GAP * k, rect.top()), Vec2::new(lay.side_width(side) * k, TALL * k));
            lay.side(side, r);
        }
        y = canvas.top() + (o.h + 2.0 * TOP + 24.0) * k;
    }
    // The opaque panel, as the fallback and VR show the expanded moment.
    let panel = Rect::from_min_size(Pos2::new(area.left() + 24.0 + (CANVAS.x + 32.0) * k, area.top() + 76.0), PANEL * k);
    p.text(panel.left_top() - Vec2::new(0.0, 20.0), Align2::LEFT_TOP, "Opaque panel (see-through off, VR)", crate::theme::display(crate::theme::size::CAPTION), pal::DIM);
    p.rect_filled(panel, CornerRadius::ZERO, pal::ISLAND);
    match (expanded.size, &expanded.expanded, &expanded.compact) {
        (Size::Expanded, Some(e), _) => {
            let h = expanded_height(e) * k;
            lay.rows(e, Rect::from_center_size(panel.center(), Vec2::new(panel.width(), h)).shrink2(Vec2::new(PAD_X * k, PAD_Y * k)));
        }
        (_, _, Some(c)) => lay.compact(c, Rect::from_center_size(panel.center(), Vec2::new(panel.width() - 30.0 * k, TALL * k))),
        _ => {}
    }
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
