//! The Elite Nexus component library (see theme.rs for colours and type): cards, grouped lists in the
//! style of System Settings, segmented controls, switches, capsule buttons, badges, meters.

use crate::theme::{display, pal, size};
use egui::{Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Pos2, Rect, Response, RichText, Sense, Shadow, Stroke, Ui, Vec2, WidgetInfo, WidgetType};

// ---------------------------------------------------------------------------------------------
// Surfaces

/// The card surface: a flat rounded panel one step lighter than the window.
pub fn card_frame() -> Frame {
    Frame::new().fill(pal::CARD).corner_radius(CornerRadius::same(16)).inner_margin(Margin::same(18))
}

/// A titled panel ("card").
pub fn card<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_with(ui, title, |_| {}, add)
}

/// A card with extra widgets right-aligned in the title row.
pub fn card_with<R>(ui: &mut Ui, title: &str, title_right: impl FnOnce(&mut Ui), add: impl FnOnce(&mut Ui) -> R) -> R {
    card_impl(ui, |ui| {
        if !title.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).font(display(size::HEADLINE)).color(pal::TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), title_right);
            });
            ui.add_space(4.0);
        }
        add(ui)
    })
}

/// A card headed like Apple's Health summaries: a tinted icon and label, then the content.
pub fn card_tinted<R>(ui: &mut Ui, icon: &str, title: &str, tint: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_impl(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new(icon).size(15.0).color(tint));
            ui.label(RichText::new(title).font(display(size::CALLOUT)).color(tint));
        });
        ui.add_space(2.0);
        add(ui)
    })
}

fn card_impl<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    // Inside a `card_row`, the first card of each column stretches to the row's tallest card.
    let fill = ui.ctx().data_mut(|d| d.remove_temp::<f32>(ui.id().with(ROW_FILL)));
    let (inner, natural) = card_frame()
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Left-aligned and never justified (columns justify by default, which spreads words apart).
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                let inner = add(ui);
                let natural = ui.min_rect().height();
                if let Some(h) = fill.filter(|h| *h > natural) {
                    // (`set_min_height` would add the height below the content instead.)
                    let top = ui.min_rect().min;
                    ui.expand_to_include_rect(Rect::from_min_size(top, Vec2::new(0.0, h)));
                }
                (inner, natural)
            })
            .inner
        })
        .inner;
    if fill.is_some() {
        ui.ctx().data_mut(|d| d.insert_temp(ui.id().with(ROW_NATURAL), natural));
    }
    inner
}

const ROW_FILL: &str = "card-row-fill";
const ROW_NATURAL: &str = "card-row-natural";
/// Space between cards.
pub const GAP: f32 = 14.0;

/// Cards in a responsive grid: up to `max_cols` per row, each at least `min_width` wide, wrapping to
/// fewer columns as the window narrows. Cards in a row share the height of the tallest.
pub fn cards(ui: &mut Ui, max_cols: usize, min_width: f32, add: impl FnOnce(&mut Cards)) {
    let avail = ui.available_width();
    let cols = (((avail + GAP) / (min_width + GAP)).floor() as usize).clamp(1, max_cols.max(1));
    let width = (avail - GAP * (cols as f32 - 1.0)) / cols as f32;
    let key = ui.id().with(("cards", ui.next_auto_id()));
    let targets: Vec<f32> = ui.ctx().data(|d| d.get_temp(key)).unwrap_or_default();
    let origin = ui.cursor().min;
    let mut c = Cards { ui, cols, width, origin, key, targets, naturals: Vec::new(), index: 0, y: origin.y, row_h: 0.0, row_natural: 0.0, avail };
    add(&mut c);
    c.finish();
}

/// Builder for [`cards`].
pub struct Cards<'a> {
    ui: &'a mut Ui,
    cols: usize,
    width: f32,
    origin: Pos2,
    key: egui::Id,
    targets: Vec<f32>,
    naturals: Vec<f32>,
    index: usize,
    y: f32,
    row_h: f32,
    row_natural: f32,
    avail: f32,
}

impl Cards<'_> {
    /// Adds one grid cell (normally one card).
    pub fn add(&mut self, add: impl FnOnce(&mut Ui)) {
        let col = self.index % self.cols;
        if col == 0 && self.index > 0 {
            self.end_row();
        }
        let row = self.naturals.len();
        let x = self.origin.x + col as f32 * (self.width + GAP);
        let rect = Rect::from_min_size(Pos2::new(x, self.y), Vec2::new(self.width, f32::INFINITY));
        let mut child = self.ui.new_child(egui::UiBuilder::new().id_salt(self.key.with(self.index)).max_rect(rect).layout(Layout::top_down(Align::Min)));
        let target = self.targets.get(row).copied().unwrap_or(0.0);
        let id = child.id();
        child.ctx().data_mut(|d| d.insert_temp(id.with(ROW_FILL), target));
        add(&mut child);
        let natural = child.ctx().data_mut(|d| {
            d.remove_temp::<f32>(id.with(ROW_FILL));
            d.remove_temp::<f32>(id.with(ROW_NATURAL))
        });
        self.row_natural = self.row_natural.max(natural.unwrap_or(0.0));
        self.row_h = self.row_h.max(child.min_rect().height());
        self.index += 1;
    }

    fn end_row(&mut self) {
        self.naturals.push(self.row_natural);
        self.y += self.row_h + GAP;
        self.row_h = 0.0;
        self.row_natural = 0.0;
    }

    fn finish(mut self) {
        if self.index == 0 {
            return;
        }
        self.end_row();
        let bottom = self.y - GAP;
        self.ui.advance_cursor_after_rect(Rect::from_min_max(self.origin, Pos2::new(self.origin.x + self.avail, bottom)));
        let changed = self.naturals.len() != self.targets.len() || self.naturals.iter().zip(&self.targets).any(|(a, b)| (a - b).abs() > 0.5);
        if changed {
            let n = self.naturals;
            self.ui.ctx().data_mut(|d| d.insert_temp(self.key, n));
            self.ui.ctx().request_repaint();
        }
    }
}

/// Cards of different heights in columns, each card going under the shortest column so far (a masonry layout): no gaps
/// under short cards, unlike [`cards`], whose rows share a height. Up to `max_cols` columns, each at least `min_width`
/// wide. `draw` adds the card for one item.
pub fn masonry<T>(ui: &mut Ui, max_cols: usize, min_width: f32, items: &[T], mut draw: impl FnMut(&mut Ui, &T)) {
    let avail = ui.available_width();
    let cols = (((avail + GAP) / (min_width + GAP)).floor() as usize).clamp(1, max_cols.max(1));
    let width = (avail - GAP * (cols as f32 - 1.0)) / cols as f32;
    let origin = ui.cursor().min;
    let key = ui.id().with(("masonry", ui.next_auto_id()));
    let mut heights = vec![0.0f32; cols];
    for (i, item) in items.iter().enumerate() {
        let col = (0..cols).min_by(|a, b| heights[*a].total_cmp(&heights[*b])).unwrap_or(0);
        let rect = Rect::from_min_size(Pos2::new(origin.x + col as f32 * (width + GAP), origin.y + heights[col]), Vec2::new(width, f32::INFINITY));
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt(key.with(i)).max_rect(rect).layout(Layout::top_down(Align::Min)));
        draw(&mut child, item);
        heights[col] += child.min_rect().height() + GAP;
    }
    let bottom = heights.iter().copied().fold(0.0, f32::max) - GAP;
    ui.advance_cursor_after_rect(Rect::from_min_size(origin, Vec2::new(avail, bottom.max(0.0))));
}

/// Two panes side by side (`left_frac` of the width for the first) when there is room for both at
/// `min_width`, otherwise stacked.
pub fn split(ui: &mut Ui, left_frac: f32, min_width: f32, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    let avail = ui.available_width();
    if avail < min_width * 2.0 + GAP {
        left(ui);
        ui.add_space(GAP - ui.spacing().item_spacing.y);
        right(ui);
        return;
    }
    let lw = ((avail - GAP) * left_frac).max(min_width);
    let rw = avail - GAP - lw;
    let origin = ui.cursor().min;
    // Panes get the parent's height, so a pane can fill it (lists with their own scroll area).
    let h = ui.available_height();
    let key = ui.id().with(("split", ui.next_auto_id()));
    let mut a = ui.new_child(egui::UiBuilder::new().id_salt(key.with(0)).max_rect(Rect::from_min_size(origin, Vec2::new(lw, h))).layout(Layout::top_down(Align::Min)));
    left(&mut a);
    let mut b = ui.new_child(
        egui::UiBuilder::new().id_salt(key.with(1)).max_rect(Rect::from_min_size(Pos2::new(origin.x + lw + GAP, origin.y), Vec2::new(rw, h))).layout(Layout::top_down(Align::Min)),
    );
    right(&mut b);
    let bottom = a.min_rect().bottom().max(b.min_rect().bottom());
    ui.advance_cursor_after_rect(Rect::from_min_max(origin, Pos2::new(origin.x + avail, bottom)));
}

/// Paints a floating "glass" surface: a lifted tint with a soft shadow and a light rim, brighter at
/// the top edge (sidebar, toolbar capsules, banners).
pub fn glass(p: &egui::Painter, rect: Rect, radius: u8, fill: Color32) {
    let r = CornerRadius::same(radius);
    p.add(Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(90) }.as_shape(rect, r));
    p.rect_filled(rect, r, fill);
    p.rect_stroke(rect, r, Stroke::new(1.0, Color32::from_white_alpha(16)), egui::StrokeKind::Inside);
    // Specular rim along the top edge.
    let top = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + f32::from(radius).max(8.0)));
    p.with_clip_rect(top).rect_stroke(rect, r, Stroke::new(1.0, Color32::from_white_alpha(22)), egui::StrokeKind::Inside);
}

// ---------------------------------------------------------------------------------------------
// Grouped lists (System Settings style): rows in one rounded box, separated by hairlines.

/// A grouped list. Add rows through the `Group`; hairlines go between them.
pub fn group<R>(ui: &mut Ui, add: impl FnOnce(&mut Group) -> R) -> R {
    Frame::new()
        .fill(pal::CARD)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut g = Group { ui, rows: 0 };
                add(&mut g)
            })
            .inner
        })
        .inner
}

pub struct Group<'a> {
    pub ui: &'a mut Ui,
    rows: usize,
}

impl Group<'_> {
    fn separator(&mut self) {
        if self.rows > 0 {
            let rect = self.ui.available_rect_before_wrap();
            let y = rect.top();
            self.ui.painter().hline(rect.x_range(), y, Stroke::new(1.0, pal::SEPARATOR));
        }
        self.rows += 1;
    }

    /// A row: title (+ optional secondary detail underneath) on the left, `trailing` on the right.
    pub fn row<R>(&mut self, title: &str, detail: &str, trailing: impl FnOnce(&mut Ui) -> R) -> R {
        self.separator();
        let ui = &mut *self.ui;
        ui.add_space(9.0);
        let r = ui
            .horizontal(|ui| {
                ui.set_min_height(26.0);
                let full = ui.available_width();
                // Keep room for the title (up to half the row), so a long value such as a path
                // truncates instead of squeezing the title into one letter per line.
                let title_w = ui.painter().layout_no_wrap(title.to_owned(), FontId::proportional(size::BODY), pal::TEXT).size().x;
                let reserve = (title_w + 16.0).min(full * 0.5);
                // Trailing control first (right-to-left), then the text in the remaining width.
                let r = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let r = ui
                        .scope(|ui| {
                            ui.set_max_width(full - reserve);
                            trailing(ui)
                        })
                        .inner;
                    let rest = ui.available_width();
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.set_max_width(rest);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.add(egui::Label::new(RichText::new(title).size(size::BODY).color(pal::TEXT)).wrap());
                            if !detail.is_empty() {
                                ui.add(egui::Label::new(RichText::new(detail).size(size::FOOTNOTE).color(pal::DIM)).wrap());
                            }
                        });
                    });
                    r
                });
                r.inner
            })
            .inner;
        ui.add_space(9.0);
        r
    }

    /// A row with a value on the right.
    pub fn value(&mut self, title: &str, value: impl Into<RichText>) {
        let v = value.into();
        self.row(title, "", |ui| {
            ui.add(egui::Label::new(v.color_if_default(pal::DIM)).truncate());
        });
    }

    /// A row with a switch on the right. Returns true when flipped.
    pub fn switch(&mut self, on: &mut bool, title: &str, detail: &str) -> bool {
        self.row(title, detail, |ui| switch(ui, on).changed())
    }

    /// Free content as a row (wraps).
    pub fn content<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.separator();
        self.ui.add_space(9.0);
        let r = add(self.ui);
        self.ui.add_space(9.0);
        r
    }
}

trait ColorIfDefault {
    fn color_if_default(self, c: Color32) -> RichText;
}

impl ColorIfDefault for RichText {
    fn color_if_default(self, c: Color32) -> RichText {
        // RichText has no getter for its colour; values passed as plain text get the secondary colour.
        self.color(c)
    }
}

/// Small secondary heading above a group or a block of cards.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(10.0);
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
    ui.painter().text(r.left_center() + Vec2::new(4.0, 0.0), Align2::LEFT_CENTER, title, display(size::FOOTNOTE), pal::DIM);
    ui.add_space(2.0);
}

/// Explanatory text under a group.
pub fn footnote(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.add(egui::Label::new(RichText::new(text).size(size::FOOTNOTE).color(pal::DIM)).wrap());
}

// ---------------------------------------------------------------------------------------------
// Text and figures

/// Large number with a small caption underneath. Never breaks inside: measured first, so in a
/// wrapping row the whole figure moves to the next line.
pub fn stat(ui: &mut Ui, caption: &str, value: impl Into<String>, color: Color32) -> Response {
    let v = ui.painter().layout_no_wrap(value.into(), display(24.0), color);
    let c = ui.painter().layout_no_wrap(caption.to_owned(), FontId::proportional(size::FOOTNOTE), pal::DIM);
    let size = Vec2::new(v.size().x.max(c.size().x), v.size().y + c.size().y);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let vh = v.size().y;
        ui.painter().galley(rect.min, v, color);
        ui.painter().galley(rect.min + Vec2::new(0.0, vh), c, pal::DIM);
    }
    resp
}

/// Key/value line for inside a `Grid`.
pub fn kv(ui: &mut Ui, key: &str, value: impl Into<RichText>) {
    ui.label(RichText::new(key).color(pal::DIM));
    ui.label(value.into());
    ui.end_row();
}

/// Big centred hint for empty pages.
pub fn empty(ui: &mut Ui, icon: &str, title: &str, hint: &str) {
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(icon).size(44.0).color(pal::FAINT));
        ui.add_space(8.0);
        ui.label(RichText::new(title).font(display(size::TITLE3)).color(pal::TEXT));
        ui.add_space(2.0);
        ui.scope(|ui| {
            ui.set_max_width(ui.available_width().min(460.0));
            ui.add(egui::Label::new(RichText::new(hint).size(size::CALLOUT).color(pal::DIM)).wrap().halign(Align::Center));
        });
    });
    ui.add_space(24.0);
}

/// A table column heading.
pub fn th(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(display(size::FOOTNOTE)).color(pal::DIM));
}


/// How a table is sorted: a column (the table's own numbering) and the direction; no column = the
/// order the rows came in. Remembered per table ([`TableSort::load`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct TableSort {
    pub column: Option<u8>,
    pub descending: bool,
}

impl TableSort {
    pub fn load(ui: &Ui, table: &str) -> TableSort {
        ui.data_mut(|d| *d.get_persisted_mut_or_default::<TableSort>(egui::Id::new(("table-sort", table))))
    }

    fn store(self, ui: &Ui, table: &str) {
        ui.data_mut(|d| d.insert_persisted(egui::Id::new(("table-sort", table)), self));
    }

    /// After a click on `column`'s heading: sorted by it, then reversed, then back to the usual order.
    pub fn clicked(self, column: u8, desc_first: bool) -> TableSort {
        match (self.column == Some(column), self.descending == desc_first) {
            (false, _) => TableSort { column: Some(column), descending: desc_first },
            (true, true) => TableSort { column: Some(column), descending: !desc_first },
            (true, false) => TableSort::default(),
        }
    }

    /// The order to show `rows` in: `cmp` compares two rows by a column, smallest first.
    pub fn order<T>(self, rows: &[T], cmp: impl Fn(&T, &T, u8) -> std::cmp::Ordering) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..rows.len()).collect();
        if let Some(c) = self.column {
            idx.sort_by(|&a, &b| {
                let o = cmp(&rows[a], &rows[b], c);
                if self.descending { o.reverse() } else { o }
            });
        }
        idx
    }
}

/// A column heading that sorts its table: the first click sorts by it (`desc_first` for amounts and
/// times, so the biggest or newest comes first), the second reverses, the third goes back to the
/// table's own order. The sorted column shows an arrow. `table` names the table, for remembering.
pub fn th_sort(ui: &mut Ui, text: &str, column: u8, sort: &mut TableSort, table: &str, desc_first: bool) {
    let on = sort.column == Some(column);
    let arrow = match (on, sort.descending) {
        (true, true) => format!(" {}", egui_phosphor::regular::CARET_DOWN),
        (true, false) => format!(" {}", egui_phosphor::regular::CARET_UP),
        _ => String::new(),
    };
    let r = ui
        .add(egui::Label::new(RichText::new(format!("{text}{arrow}")).font(display(size::FOOTNOTE)).color(if on { pal::TEXT } else { pal::DIM })).sense(Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if on { "Click to reverse; a third click goes back to the usual order" } else { "Click to sort by this column" });
    if r.clicked() {
        *sort = sort.clicked(column, desc_first);
        sort.store(ui, table);
    }
}

/// Compares two optional numbers, smallest first; an unknown value counts as the largest.
pub fn cmp_opt(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    a.unwrap_or(f64::INFINITY).total_cmp(&b.unwrap_or(f64::INFINITY))
}

/// Case-insensitive text order.
pub fn cmp_text(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

// ---------------------------------------------------------------------------------------------
// Meters and tags

/// Slim rounded meter with optional overlay text.
pub fn bar(ui: &mut Ui, frac: f32, color: Color32, height: f32, text: Option<&str>) -> Response {
    let width = ui.available_width().max(40.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let p = ui.painter();
    let r = CornerRadius::same((height / 2.0).min(255.0) as u8);
    p.rect_filled(rect, r, Color32::from_white_alpha(18));
    let f = frac.clamp(0.0, 1.0);
    if f > 0.0 {
        let mut fill = rect;
        fill.set_width((rect.width() * f).max(height));
        p.rect_filled(fill, r, color);
    }
    if let Some(t) = text {
        p.text(rect.center(), Align2::CENTER_CENTER, t, display((height - 3.0).clamp(10.0, 12.0)), Color32::WHITE);
    }
    resp
}

/// Shares of a whole as one capsule, like the storage bar in Apple's settings: each part in its colour, as wide as its
/// share (a small share keeps a sliver, so it still shows), with a hairline gap between parts. Returns the bar's
/// response and the part under the pointer, for its tooltip.
pub fn share_bar(ui: &mut Ui, parts: &[(f64, Color32)], height: f32) -> (Response, Option<usize>) {
    let width = ui.available_width().max(40.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let widths = share_widths(parts.iter().map(|(v, _)| *v), width, 2.0, height.min(6.0));
    let mut hovered = None;
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let r = (height / 2.0).min(255.0) as u8;
        p.rect_filled(rect, CornerRadius::same(r), Color32::from_white_alpha(18));
        let shown: Vec<usize> = (0..parts.len()).filter(|i| widths[*i] > 0.0).collect();
        let pointer = resp.hover_pos();
        let mut x = rect.left();
        for (n, &i) in shown.iter().enumerate() {
            let seg = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(widths[i], height));
            // Only the capsule's two ends are round.
            let (first, last) = (n == 0, n + 1 == shown.len());
            let round = CornerRadius { nw: if first { r } else { 0 }, sw: if first { r } else { 0 }, ne: if last { r } else { 0 }, se: if last { r } else { 0 } };
            p.rect_filled(seg, round, parts[i].1);
            if pointer.is_some_and(|q| q.x >= seg.left() - 1.0 && q.x <= seg.right() + 1.0) {
                hovered = Some(i);
            }
            x = seg.right() + 2.0;
        }
    }
    (resp, hovered)
}

/// How wide each part of a [`share_bar`] is: its share of `width` less the gaps between parts, at least `least` for any
/// part above zero (the room comes off the widest), none for the rest.
pub fn share_widths(values: impl Iterator<Item = f64>, width: f32, gap: f32, least: f32) -> Vec<f32> {
    let values: Vec<f64> = values.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 }).collect();
    let total: f64 = values.iter().sum();
    let parts = values.iter().filter(|v| **v > 0.0).count();
    if total <= 0.0 || parts == 0 {
        return vec![0.0; values.len()];
    }
    let room = (width - gap * (parts - 1) as f32).max(0.0);
    let mut w: Vec<f32> = values.iter().map(|v| if *v > 0.0 { ((v / total) as f32 * room).max(least) } else { 0.0 }).collect();
    let over = w.iter().sum::<f32>() - room;
    if over > 0.0
        && let Some(widest) = (0..w.len()).max_by(|a, b| w[*a].total_cmp(&w[*b]))
    {
        w[widest] = (w[widest] - over).max(least);
    }
    w
}

/// A small tinted pill (status, count, tag). Never wraps: in a wrapping row the whole pill moves to
/// the next line.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), display(size::CAPTION), color);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + BADGE_PAD * 2.0, Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, CornerRadius::same(255), color.gamma_multiply(0.18));
        ui.painter().galley(rect.min + BADGE_PAD, galley, color);
    }
    resp
}

const BADGE_PAD: Vec2 = Vec2::new(8.0, 3.0);

/// The size [`badge`] takes for `text`, for sizing a table column before its rows are drawn.
pub fn badge_size(ui: &Ui, text: &str) -> Vec2 {
    ui.painter().layout_no_wrap(text.to_owned(), display(size::CAPTION), pal::TEXT).size() + BADGE_PAD * 2.0
}

// ---------------------------------------------------------------------------------------------
// Controls

/// Apple-style segmented control. Returns true when the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    match tabs(ui, *value, options) {
        Some(v) if v != *value => {
            *value = v;
            true
        }
        _ => false,
    }
}

/// Segmented control used for a page's sections. Returns the segment clicked this frame, including
/// the one already selected (pages use that to reset a drill-down). The selection pill slides.
pub fn tabs<T: PartialEq + Copy>(ui: &mut Ui, value: T, options: &[(T, &str)]) -> Option<T> {
    const INSET: f32 = 2.0;
    const H: f32 = 26.0;
    let sel_font = display(size::CALLOUT);
    let font = FontId::proportional(size::CALLOUT);
    // Width from the semibold face, so segments don't shift as the selection moves.
    let text_w: Vec<f32> = options.iter().map(|(_, l)| ui.painter().layout_no_wrap((*l).to_owned(), sel_font.clone(), pal::TEXT).size().x).collect();
    let n = options.len().max(1) as f32;
    let room = ui.available_width() - 2.0 * INSET - text_w.iter().sum::<f32>();
    if room < 2.0 * n * 6.0 {
        // Too narrow for one control: wrap the choices as pills instead of overflowing.
        let mut clicked = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
            for (v, label) in options {
                if pill(ui, *v == value, label).clicked() {
                    clicked = Some(*v);
                }
            }
        });
        return clicked;
    }
    let pad = (room / (2.0 * n)).clamp(6.0, 14.0);
    let widths: Vec<f32> = text_w.iter().map(|w| w + 2.0 * pad).collect();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(widths.iter().sum::<f32>() + 2.0 * INSET, H + 2.0 * INSET), Sense::hover());
    let mut clicked = None;
    let mut segs = Vec::with_capacity(options.len());
    let mut x = rect.left() + INSET;
    for (i, w) in widths.iter().enumerate() {
        let seg = Rect::from_min_size(Pos2::new(x, rect.top() + INSET), Vec2::new(*w, H));
        let r = ui.interact(seg, resp.id.with(i), Sense::click());
        if r.clicked() {
            clicked = Some(options[i].0);
        }
        segs.push((seg, r.hovered()));
        x += w;
    }
    if !ui.is_rect_visible(rect) {
        return clicked;
    }
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(9), Color32::from_white_alpha(14));
    let sel = options.iter().position(|(v, _)| *v == value);
    if let Some(i) = sel {
        let target = segs[i].0;
        let ctx = ui.ctx();
        let x0 = ctx.animate_value_with_time(resp.id.with("x0"), target.left(), 0.16);
        let x1 = ctx.animate_value_with_time(resp.id.with("x1"), target.right(), 0.16);
        let pill = Rect::from_x_y_ranges(x0..=x1, target.y_range());
        p.add(Shadow { offset: [0, 1], blur: 3, spread: 0, color: Color32::from_black_alpha(80) }.as_shape(pill, CornerRadius::same(7)));
        p.rect_filled(pill, CornerRadius::same(7), Color32::from_rgb(99, 99, 102));
    }
    for (i, ((v, label), (seg, hovered))) in options.iter().zip(&segs).enumerate() {
        let selected = *v == value;
        // Hairline separators between unselected neighbours, as in macOS.
        if i > 0 && !selected && sel != Some(i - 1) {
            p.vline(seg.left(), seg.y_range().shrink(7.0), Stroke::new(1.0, Color32::from_white_alpha(22)));
        }
        let col = if selected || *hovered { pal::TEXT } else { Color32::from_rgb(206, 206, 210) };
        p.text(seg.center(), Align2::CENTER_CENTER, *label, if selected { sel_font.clone() } else { font.clone() }, col);
    }
    clicked
}

/// A bare switch (iOS/macOS style). The response reports `changed()` when flipped.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(38.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *on, ""));
    paint_switch(ui, rect, resp.id, *on);
    resp
}

fn paint_switch(ui: &Ui, rect: Rect, id: egui::Id, on: bool) {
    let t = ui.ctx().animate_bool_responsive(id, on);
    let bg = lerp_color(Color32::from_rgb(72, 72, 76), pal::GOOD, t);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(11), bg);
    let x = egui::lerp((rect.left() + 11.0)..=(rect.right() - 11.0), t);
    p.circle_filled(Pos2::new(x, rect.center().y + 1.0), 9.5, Color32::from_black_alpha(50));
    p.circle_filled(Pos2::new(x, rect.center().y), 9.0, Color32::WHITE);
}

/// A switch with its label to the right (inline use). The response reports `changed()` when flipped.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(38.0, 22.0), Sense::click());
        let text = ui.add(egui::Label::new(RichText::new(label).color(pal::TEXT)).sense(Sense::click()));
        if resp.clicked() || text.clicked() {
            *on = !*on;
            resp.mark_changed();
        }
        resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *on, label));
        paint_switch(ui, rect, resp.id, *on);
        resp
    })
    .inner
}

/// A − n + stepper on a glass capsule (macOS 26). Returns true when the value changed.
pub fn stepper(ui: &mut Ui, value: &mut u32, range: std::ops::RangeInclusive<u32>) -> bool {
    const H: f32 = 28.0;
    const SIDE: f32 = 30.0;
    let (lo, hi) = (*range.start(), *range.end());
    let before = *value;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(SIDE * 2.0 + 34.0, H), Sense::hover());
    let minus = Rect::from_min_size(rect.min, Vec2::new(SIDE, H));
    let plus = Rect::from_min_size(Pos2::new(rect.right() - SIDE, rect.top()), Vec2::new(SIDE, H));
    let m = ui.interact(minus, resp.id.with("minus"), Sense::click());
    let p = ui.interact(plus, resp.id.with("plus"), Sense::click());
    if m.clicked() && *value > lo {
        *value -= 1;
    }
    if p.clicked() && *value < hi {
        *value += 1;
    }
    *value = (*value).clamp(lo, hi);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        control_glass(painter, rect);
        let r = CornerRadius::same((H / 2.0) as u8);
        for (half, sense, enabled, glyph) in [(minus, &m, *value > lo, egui_phosphor::regular::MINUS), (plus, &p, *value < hi, egui_phosphor::regular::PLUS)] {
            if enabled && sense.hovered() {
                let wash = if sense.is_pointer_button_down_on() { 30 } else { 14 };
                painter.with_clip_rect(half).rect_filled(rect, r, Color32::from_white_alpha(wash));
            }
            painter.text(half.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(size::CALLOUT), if enabled { pal::TEXT } else { pal::FAINT });
        }
        for x in [minus.right(), plus.left()] {
            painter.vline(x, rect.y_range().shrink(7.0), Stroke::new(1.0, Color32::from_white_alpha(22)));
        }
        painter.text(rect.center(), Align2::CENTER_CENTER, value.to_string(), display(size::CALLOUT), pal::TEXT);
    }
    *value != before
}

/// The glass of a control (macOS 26): a light translucent fill, a hairline rim that brightens along
/// the top edge, and a small contact shadow. [`glass`] is the floating version.
fn control_glass(p: &egui::Painter, rect: Rect) {
    let r = CornerRadius::same((rect.height() / 2.0).min(255.0) as u8);
    p.add(Shadow { offset: [0, 1], blur: 4, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(rect, r));
    p.rect_filled(rect, r, Color32::from_white_alpha(16));
    p.rect_stroke(rect, r, Stroke::new(1.0, Color32::from_white_alpha(14)), egui::StrokeKind::Inside);
    let top = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.center().y));
    p.with_clip_rect(top).rect_stroke(rect, r, Stroke::new(1.0, Color32::from_white_alpha(34)), egui::StrokeKind::Inside);
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// The main action of a view: a filled accent capsule.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).font(display(size::CALLOUT)).color(pal::ON_ACCENT))
            .fill(pal::ACCENT)
            .corner_radius(CornerRadius::same(255))
            .min_size(Vec2::new(64.0, 30.0)),
    )
}

/// A secondary action: a grey capsule.
pub fn button(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(text).size(size::CALLOUT).color(pal::TEXT)).corner_radius(CornerRadius::same(255)).min_size(Vec2::new(0.0, 30.0)))
}

/// A frameless icon button with a tooltip.
pub fn icon_button(ui: &mut Ui, icon: &str, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    if resp.hovered() {
        ui.painter().circle_filled(rect.center(), 15.0, Color32::from_white_alpha(14));
    }
    let col = if resp.hovered() { pal::TEXT } else { pal::DIM };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, FontId::proportional(17.0), col);
    resp.on_hover_text(tip)
}

/// A selectable pill (filters): accent when selected.
pub fn pill(ui: &mut Ui, selected: bool, text: &str) -> Response {
    let (fill, col) = if selected { (pal::ACCENT, pal::ON_ACCENT) } else { (pal::CARD_HI, pal::TEXT) };
    let font = if selected { display(size::CALLOUT) } else { FontId::proportional(size::CALLOUT) };
    ui.add(egui::Button::new(RichText::new(text).font(font).color(col)).fill(fill).corner_radius(CornerRadius::same(255)).min_size(Vec2::new(0.0, 30.0)))
}

/// A rounded suggestion chip.
pub fn chip(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(text).size(size::CALLOUT).color(pal::TEXT)).fill(pal::CARD).corner_radius(CornerRadius::same(255)).min_size(Vec2::new(0.0, 32.0)))
}

// ---------------------------------------------------------------------------------------------
// Search settings: every setting of a search in view, in one bar

/// Space between two fields of a [`filter_bar`] (its hairline runs down the middle), and between its lines.
const BAR_GAP: f32 = 24.0;
const BAR_LINE_GAP: f32 = 12.0;
/// Space between a field's caption and its controls.
const BAR_CAPTION_GAP: f32 = 5.0;

/// A bar that shows every setting of a search at once, instead of hiding them behind an "Options" click: each field is
/// a small caption over its controls, fields sit side by side with hairlines between them, and they wrap onto more lines
/// when the window is narrow. `id_salt` names the bar: it remembers its fields' widths to know where to wrap.
///
/// ```ignore
/// widgets::filter_bar(ui, "find-commodity", |bar| {
///     bar.field("Within", |ui| ui.add(widgets::Number::new(&mut p.max_dist, 1.0..=100_000.0).ladder(LY).unit(" ly")));
///     bar.field("Include", |ui| widgets::chip_toggle(ui, &mut p.carriers, "Fleet carriers"));
///     bar.end(|ui| widgets::icon_button(ui, icon::ARROW_COUNTER_CLOCKWISE, "Reset"));
/// });
/// ```
pub fn filter_bar<R>(ui: &mut Ui, id_salt: &str, add: impl FnOnce(&mut FilterBar) -> R) -> R {
    let id = ui.make_persistent_id(id_salt);
    Frame::new()
        .fill(pal::CARD)
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(10)))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin { left: 16, right: 16, top: 10, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let widths: Vec<f32> = ui.data(|d| d.get_temp(id)).unwrap_or_default();
            let origin = ui.cursor().min;
            let right = origin.x + ui.available_width();
            let caption_h = ui.painter().layout_no_wrap("Ag".into(), FontId::proportional(size::CAPTION), pal::DIM).size().y;
            let mut bar = FilterBar { ui, id, widths, measured: Vec::new(), left: origin.x, right, top: origin.y, x: origin.x, y: origin.y, line_h: 0.0, seps: Vec::new(), caption_h };
            let r = add(&mut bar);
            bar.finish();
            r
        })
        .inner
}

/// Builder for [`filter_bar`].
pub struct FilterBar<'a> {
    ui: &'a mut Ui,
    id: egui::Id,
    /// Each field's width last frame: where a line has to wrap is known before the field is drawn.
    widths: Vec<f32>,
    measured: Vec<f32>,
    left: f32,
    right: f32,
    top: f32,
    x: f32,
    y: f32,
    line_h: f32,
    /// Where this line's hairlines go, drawn once the line's height is known.
    seps: Vec<f32>,
    caption_h: f32,
}

impl FilterBar<'_> {
    /// A field: `caption` over the controls `add` draws, side by side.
    pub fn field<R>(&mut self, caption: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.place(Some(caption), None, add)
    }

    /// A field with a tooltip over the whole of it (for controls that have none of their own, like a segmented one).
    pub fn field_tip<R>(&mut self, caption: &str, tip: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.place(Some(caption), Some(tip), add)
    }

    /// Controls at the right end of the bar, level with the fields' controls: a reset button, the search button, a
    /// quiet note. Call it last.
    pub fn end<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.place(None, None, add)
    }

    fn place<R>(&mut self, caption: Option<&str>, tip: Option<&str>, add: impl FnOnce(&mut Ui) -> R) -> R {
        let i = self.measured.len();
        let w = self.widths.get(i).copied().unwrap_or(110.0);
        let first = self.x <= self.left;
        if !first && self.x + BAR_GAP + w > self.right {
            self.new_line();
        }
        let first = self.x <= self.left;
        let mut x = if first { self.left } else { self.x + BAR_GAP };
        let top = match caption {
            Some(_) => {
                if !first {
                    self.seps.push(self.x + BAR_GAP / 2.0);
                }
                self.y
            }
            // The end: pushed to the right edge, level with the controls under the captions.
            None => {
                x = x.max(self.right - w);
                self.y + self.caption_h + BAR_CAPTION_GAP
            }
        };
        // A field may take a whole line, so it's measured at its natural width (a segmented control squeezed into the
        // rest of a line would fold into pills and stay there); one wider than the room left moves on next frame.
        let room = if caption.is_some() { self.right - self.left } else { self.right - x };
        let rect = Rect::from_min_size(Pos2::new(x, top), Vec2::new(room.max(10.0), f32::INFINITY));
        let mut child = self.ui.new_child(egui::UiBuilder::new().id_salt(self.id.with(i)).max_rect(rect).layout(Layout::top_down(Align::Min)));
        let r = match caption {
            Some(c) => {
                child.spacing_mut().item_spacing.y = BAR_CAPTION_GAP;
                child.add(egui::Label::new(RichText::new(c).size(size::CAPTION).color(pal::DIM)).extend());
                child.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    add(ui)
                })
                .inner
            }
            None => {
                child.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    add(ui)
                })
                .inner
            }
        };
        let used = child.min_rect();
        if let Some(t) = tip {
            self.ui.interact(used, self.id.with((i, "tip")), Sense::hover()).on_hover_text(t);
        }
        self.measured.push(used.width());
        self.x = x + used.width();
        self.line_h = self.line_h.max(used.bottom() - self.y);
        r
    }

    fn close_line(&mut self) {
        let p = self.ui.painter();
        for x in self.seps.drain(..) {
            p.vline(x, (self.y + 2.0)..=(self.y + self.line_h - 2.0), Stroke::new(1.0, Color32::from_white_alpha(20)));
        }
    }

    fn new_line(&mut self) {
        self.close_line();
        self.y += self.line_h + BAR_LINE_GAP;
        self.x = self.left;
        self.line_h = 0.0;
    }

    fn finish(mut self) {
        self.close_line();
        let bottom = self.y + self.line_h;
        self.ui.advance_cursor_after_rect(Rect::from_min_max(Pos2::new(self.left, self.top), Pos2::new(self.right, bottom)));
        let moved = self.measured.len() != self.widths.len() || self.measured.iter().zip(&self.widths).any(|(a, b)| (a - b).abs() > 0.5);
        if moved {
            let (id, m) = (self.id, std::mem::take(&mut self.measured));
            self.ui.data_mut(|d| d.insert_temp(id, m));
            self.ui.ctx().request_repaint();
        }
    }
}

/// How the − and + of a [`Number`] step.
#[derive(Clone, Copy, Debug)]
pub enum NumberSteps<'a> {
    /// Through these round values, smallest first (distances, hours, quantities). A typed value in between steps to the
    /// next one up or down.
    Ladder(&'a [f64]),
    /// By a step that fits the number's size: its second digit (48,300 steps by 1,000, 750 by 10, never less than 1).
    /// Shift steps ten times as far.
    Magnitude,
}

/// The value a step from `v` lands on, up or down (`big`: Shift held), within `lo..=hi`.
pub fn step_value(v: f64, steps: NumberSteps<'_>, up: bool, big: bool, lo: f64, hi: f64) -> f64 {
    const EPS: f64 = 1e-6;
    let next = match steps {
        NumberSteps::Ladder(rungs) => {
            let found = if up { rungs.iter().copied().find(|r| *r > v + EPS) } else { rungs.iter().rev().copied().find(|r| *r < v - EPS) };
            found.unwrap_or(if up { hi } else { lo })
        }
        NumberSteps::Magnitude => {
            // Going down from 10,000 steps by 100 (as up to it does), not by 1,000.
            let basis = if up { v.abs() } else { (v.abs() - EPS).max(0.0) };
            let digits = if basis >= 1.0 { basis.log10().floor() } else { 0.0 };
            let mut step = 10f64.powf(digits - 1.0).max(1.0);
            if big {
                step *= 10.0;
            }
            if up { ((v / step).floor() + 1.0) * step } else { ((v / step).ceil() - 1.0) * step }
        }
    };
    next.clamp(lo, hi)
}

/// A typed number in [`Number`]: spaces, commas and the unit are ignored, and k, m or b multiply ("48k", "1.5m").
pub fn parse_typed(text: &str, unit: &str) -> Option<f64> {
    let mut t: String = text.chars().filter(|c| !c.is_whitespace() && !matches!(c, ',' | '\'' | '_')).collect::<String>().to_lowercase();
    let unit = unit.trim().to_lowercase();
    if !unit.is_empty()
        && let Some(stripped) = t.strip_suffix(unit.as_str())
    {
        t = stripped.to_string();
    }
    let (num, mult) = match t.chars().last() {
        Some('k') => (&t[..t.len() - 1], 1e3),
        Some('m') => (&t[..t.len() - 1], 1e6),
        Some('b') => (&t[..t.len() - 1], 1e9),
        _ => (t.as_str(), 1.0),
    };
    num.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v * mult)
}

/// A number to type or step, on a glass capsule: − on the left, + on the right, the value in the middle. A click on the
/// value selects it for typing (Enter or a click elsewhere takes it, Escape keeps the old one; ↑ and ↓ step while
/// typing). `ui.add(Number::new(&mut v, 1.0..=100.0).ladder(LY).unit(" ly"))`; the response says `changed()`.
pub struct Number<'a> {
    value: &'a mut f64,
    range: std::ops::RangeInclusive<f64>,
    steps: NumberSteps<'a>,
    unit: &'a str,
    zero: Option<&'a str>,
    decimals: usize,
}

impl<'a> Number<'a> {
    pub fn new(value: &'a mut f64, range: std::ops::RangeInclusive<f64>) -> Self {
        Number { value, range, steps: NumberSteps::Magnitude, unit: "", zero: None, decimals: 0 }
    }

    /// − and + step through these values ([`NumberSteps::Ladder`]).
    pub fn ladder(mut self, rungs: &'a [f64]) -> Self {
        self.steps = NumberSteps::Ladder(rungs);
        self
    }

    /// Shown after the number (" ly", " Cr").
    pub fn unit(mut self, unit: &'a str) -> Self {
        self.unit = unit;
        self
    }

    fn text(&self, v: f64) -> String {
        match self.zero {
            Some(z) if v == 0.0 => z.to_string(),
            _ if self.decimals == 0 => format!("{}{}", nexus_data::names::group_digits(v.round() as i64), self.unit),
            _ => format!("{v:.*}{}", self.decimals, self.unit),
        }
    }
}

impl egui::Widget for Number<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        const H: f32 = 30.0;
        const SIDE: f32 = 28.0;
        let (lo, hi) = (*self.range.start(), *self.range.end());
        let before = *self.value;
        let shown = self.text(before);
        let font = display(size::CALLOUT);
        let text_w = ui.painter().layout_no_wrap(shown.clone(), font.clone(), pal::TEXT).size().x;
        let mid_w = (text_w + 18.0).max(48.0);
        let (rect, base) = ui.allocate_exact_size(Vec2::new(SIDE * 2.0 + mid_w, H), Sense::hover());
        let minus = Rect::from_min_size(rect.min, Vec2::new(SIDE, H));
        let plus = Rect::from_min_size(Pos2::new(rect.right() - SIDE, rect.top()), Vec2::new(SIDE, H));
        let mid = Rect::from_min_max(Pos2::new(minus.right(), rect.top()), Pos2::new(plus.left(), rect.bottom()));
        let edit_id = base.id.with("edit");
        let text_id = base.id.with("text");
        let big = ui.input(|i| i.modifiers.shift_only());
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let mut value = before;
        // Stays focusable while it shows the value, so a click or Tab can hand it the keyboard (as egui's DragValue
        // does). Tab away moves the focus inside this call.
        let editing = ui.memory_mut(|m| {
            m.interested_in_focus(edit_id, ui.layer_id());
            m.has_focus(edit_id)
        });
        // The keyboard left without the text field seeing it (Tab, Escape, a click elsewhere): what was typed counts,
        // unless it was Escape.
        if !editing
            && let Some(typed) = ui.data_mut(|d| d.remove_temp::<String>(text_id))
            && !escape
            && let Some(v) = parse_typed(&typed, self.unit)
        {
            value = v.clamp(lo, hi);
        }
        // Whether it was being typed in last frame: the frame it starts, it gets the value to type over.
        let was_id = edit_id.with("was");
        let was = ui.data(|d| d.get_temp::<bool>(was_id)).unwrap_or(false);
        ui.data_mut(|d| d.insert_temp(was_id, editing));
        if editing {
            // Clicked or reached by Tab: the value to type over, selected.
            if !was || ui.data(|d| d.get_temp::<String>(text_id)).is_none() {
                let raw = raw_number(value, self.decimals);
                let mut state = egui::TextEdit::load_state(ui.ctx(), edit_id).unwrap_or_default();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(raw.chars().count()))));
                state.store(ui.ctx(), edit_id);
                ui.data_mut(|d| d.insert_temp(text_id, raw));
            }
            let mut typed: String = ui.data(|d| d.get_temp(text_id)).unwrap_or_default();
            let (up, down) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)));
            if up || down {
                value = step_value(parse_typed(&typed, self.unit).unwrap_or(value), self.steps, up, big, lo, hi);
                typed = raw_number(value, self.decimals);
            }
            let edit = egui::TextEdit::singleline(&mut typed)
                .id(edit_id)
                .frame(Frame::NONE)
                .font(font.clone())
                .horizontal_align(Align::Center)
                .vertical_align(Align::Center)
                .desired_width(mid.width() - 8.0)
                .margin(Margin::ZERO);
            let r = ui.put(mid.shrink2(Vec2::new(4.0, 3.0)), edit);
            if r.lost_focus() {
                // Enter (Escape never gets here: egui takes the focus away before the field is drawn).
                if !escape && let Some(v) = parse_typed(&typed, self.unit) {
                    value = v.clamp(lo, hi);
                }
                ui.data_mut(|d| d.remove_temp::<String>(text_id));
            } else {
                ui.data_mut(|d| d.insert_temp(text_id, typed));
            }
        } else if ui.interact(mid, base.id.with("mid"), Sense::click()).on_hover_cursor(egui::CursorIcon::Text).clicked() {
            ui.memory_mut(|mem| mem.request_focus(edit_id));
        }
        let m = ui.interact(minus, base.id.with("minus"), Sense::click());
        let p = ui.interact(plus, base.id.with("plus"), Sense::click());
        if m.clicked() && value > lo {
            value = step_value(value, self.steps, false, big, lo, hi);
        }
        if p.clicked() && value < hi {
            value = step_value(value, self.steps, true, big, lo, hi);
        }
        if (m.clicked() || p.clicked()) && editing {
            ui.data_mut(|d| d.insert_temp(text_id, raw_number(value, self.decimals)));
        }
        // The whole capsule's hover, registered after the click areas so it's on top of them (hovering takes no click):
        // a caller's tooltip shows over any part of it.
        let mut resp = ui.interact(rect, base.id.with("hover"), Sense::hover());
        if value != before {
            *self.value = value;
            resp.mark_changed();
        }
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            control_glass(painter, rect);
            let r = CornerRadius::same((H / 2.0) as u8);
            for (half, sense, enabled, glyph) in [(minus, &m, value > lo, egui_phosphor::regular::MINUS), (plus, &p, value < hi, egui_phosphor::regular::PLUS)] {
                if enabled && sense.hovered() {
                    let wash = if sense.is_pointer_button_down_on() { 30 } else { 14 };
                    painter.with_clip_rect(half).rect_filled(rect, r, Color32::from_white_alpha(wash));
                }
                painter.text(half.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(size::CALLOUT), if enabled { pal::TEXT } else { pal::FAINT });
            }
            for x in [minus.right(), plus.left()] {
                painter.vline(x, rect.y_range().shrink(7.0), Stroke::new(1.0, Color32::from_white_alpha(22)));
            }
            if editing {
                painter.rect_stroke(mid.shrink(2.0), CornerRadius::same(6), Stroke::new(1.0, pal::ACCENT), egui::StrokeKind::Inside);
            } else {
                painter.text(mid.center(), Align2::CENTER_CENTER, self.text(value), font, pal::TEXT);
            }
        }
        let shown = self.text(value);
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, ui.is_enabled(), &shown));
        resp
    }
}

/// A number as it's typed: no grouping, no unit.
fn raw_number(v: f64, decimals: usize) -> String {
    if decimals == 0 { format!("{}", v.round() as i64) } else { format!("{v:.decimals$}") }
}

/// A filter chip that switches something on and off: "✓ Fleet carriers" on an orange tint when on, "+ Fleet carriers"
/// on grey when off. The response reports `changed()` when flipped.
pub fn chip_toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let (glyph, fill, col) = if *on {
        (egui_phosphor::regular::CHECK, pal::ACCENT_BG, pal::ACCENT_SOFT)
    } else {
        (egui_phosphor::regular::PLUS, Color32::from_white_alpha(16), pal::TEXT)
    };
    let mut r = ui.add(
        egui::Button::new(RichText::new(format!("{glyph}  {label}")).size(size::CALLOUT).color(col)).fill(fill).corner_radius(CornerRadius::same(255)).min_size(Vec2::new(0.0, 30.0)),
    );
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *on, label));
    r
}

/// Collapsible "› Options" disclosure; returns true while open.
pub fn disclosure(ui: &mut Ui, open: &mut bool, label: &str) -> bool {
    let caret = if *open { egui_phosphor::regular::CARET_DOWN } else { egui_phosphor::regular::CARET_RIGHT };
    if ui.add(egui::Button::new(RichText::new(format!("{caret}  {label}")).size(size::CALLOUT).color(pal::DIM)).frame(false)).clicked() {
        *open = !*open;
    }
    *open
}

// ---------------------------------------------------------------------------------------------
// Settings in view

/// A search's settings, all in view where they're used, never behind an "Options" click (Apple's inline filter bar):
/// a rounded bar of fields, each a small caption over its control, side by side with hairlines between them, wrapping
/// onto more rows as the window narrows. Add the fields through the [`SettingsBar`].
pub fn settings_bar<R>(ui: &mut Ui, add: impl FnOnce(&mut SettingsBar) -> R) -> R {
    let id = ui.id().with(("settings-bar", ui.next_auto_id()));
    Frame::new()
        .fill(Color32::from_white_alpha(6))
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(10)))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Rows start one control high and grow to their tallest field (as `horizontal_wrapped` does): a row as tall
            // as all the space left would send the next row to the bottom of it.
            let width = ui.available_width();
            let first_row = Vec2::new(width, ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(first_row, Layout::left_to_right(Align::Min).with_main_wrap(true), |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(28.0, 14.0);
                let mut bar = SettingsBar { ui, id, width, last: None };
                add(&mut bar)
            })
            .inner
        })
        .inner
}

/// Builder for [`settings_bar`].
pub struct SettingsBar<'a> {
    pub ui: &'a mut Ui,
    id: egui::Id,
    /// The bar's inner width: what a field may take.
    width: f32,
    /// The field before, for the hairline between two fields on one row.
    last: Option<Rect>,
}

impl SettingsBar<'_> {
    /// One setting: `caption` over whatever `control` draws; `tip`, shown over the caption, says what it does and its
    /// limits (empty for none). A field that won't fit on the row moves to the next one whole.
    pub fn field<R>(&mut self, caption: &str, tip: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
        let key = self.id.with(caption);
        let ui = &mut *self.ui;
        // The field's width, from the frame before: a wrapping row places a child where the last one ended.
        let width: Option<f32> = ui.data(|d| d.get_temp(key));
        if self.last.is_some() && width.is_some_and(|w| w > ui.available_size_before_wrap().x) {
            ui.end_row();
        }
        // Laid out with the bar's whole width, so a control takes its own size instead of squeezing into what's left
        // of the row (a segmented control would turn into pills).
        let room = Rect::from_min_size(ui.cursor().min, Vec2::new(self.width, f32::INFINITY));
        let r = ui.scope_builder(egui::UiBuilder::new().max_rect(room).layout(Layout::top_down(Align::Min)), |ui| {
            ui.spacing_mut().item_spacing.y = 5.0;
            let c = ui.add(egui::Label::new(RichText::new(caption).size(size::FOOTNOTE).color(pal::DIM)).selectable(false));
            if !tip.is_empty() {
                c.on_hover_text(tip);
            }
            control(ui)
        });
        let rect = r.response.rect;
        if width.is_none_or(|w| (w - rect.width()).abs() > 0.5) {
            ui.data_mut(|d| d.insert_temp(key, rect.width()));
            ui.ctx().request_repaint();
        }
        if let Some(prev) = self.last
            && (prev.top() - rect.top()).abs() < 0.5
            && prev.right() < rect.left()
        {
            let x = (prev.right() + rect.left()) / 2.0;
            ui.painter().vline(x, rect.top() + 2.0..=rect.bottom().max(prev.bottom()) - 2.0, Stroke::new(1.0, Color32::from_white_alpha(16)));
        }
        self.last = Some(rect);
        r.inner
    }
}

/// How [`number`]'s − and + move its value.
#[derive(Debug, Clone, Copy)]
pub enum Steps<'a> {
    /// To the next multiple of this (1 ly, 5 %).
    By(f64),
    /// To the next of these round values, smallest first (25, 50, 100, 250 ly).
    Through(&'a [f64]),
}

impl Steps<'_> {
    /// The value one step up (or down) from `v`, kept within `lo..=hi`.
    pub fn step(self, v: f64, up: bool, lo: f64, hi: f64) -> f64 {
        let eps = 1e-9 * v.abs().max(1.0);
        let next = match self {
            Steps::By(d) if d > 0.0 => d * if up { ((v + eps) / d).floor() + 1.0 } else { ((v - eps) / d).ceil() - 1.0 },
            Steps::By(_) => v,
            Steps::Through(ladder) if up => ladder.iter().copied().find(|s| *s > v + eps).unwrap_or(hi),
            Steps::Through(ladder) => ladder.iter().rev().copied().find(|s| *s < v - eps).unwrap_or(lo),
        };
        next.clamp(lo, hi)
    }
}

/// A number to type or step (macOS 26's stepper and its field in one glass capsule): − and + move it by `steps`, a
/// click on the value types a new one, a drag across it slides it. Kept within `range`, written with `decimals`
/// places, its thousands grouped, and `suffix` after it (" ly"). The response is the value's and reports `changed()`
/// whichever way it changed.
pub fn number(ui: &mut Ui, value: &mut f64, range: std::ops::RangeInclusive<f64>, steps: Steps, decimals: usize, suffix: &str) -> Response {
    const H: f32 = 28.0;
    let (lo, hi) = (*range.start(), *range.end());
    let before = *value;
    // Room for the widest value, so the capsule keeps its size as the value changes.
    let widest = [lo, hi].map(|v| ui.painter().layout_no_wrap(format!("{}{suffix}", grouped(v, decimals)), FontId::proportional(size::CALLOUT), pal::TEXT).size().x);
    let mid = widest[0].max(widest[1]) + 18.0;
    // The glass goes under the controls, so its place is kept before they're drawn.
    let under = ui.painter().add(egui::Shape::Noop);
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let minus = step_half(ui, egui_phosphor::regular::MINUS, *value > lo, true);
        let mut resp = ui
            .scope(|ui| {
                let s = ui.style_mut();
                s.spacing.interact_size = Vec2::new(mid, H);
                s.spacing.button_padding = Vec2::new(8.0, 0.0);
                let w = &mut s.visuals.widgets;
                for v in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                    v.bg_stroke = Stroke::NONE;
                    v.expansion = 0.0;
                    v.corner_radius = CornerRadius::same(6);
                }
                w.inactive.weak_bg_fill = Color32::TRANSPARENT;
                w.hovered.weak_bg_fill = Color32::from_white_alpha(10);
                w.active.weak_bg_fill = Color32::from_white_alpha(18);
                let speed = match steps {
                    Steps::By(d) => d / 4.0,
                    Steps::Through(_) => (hi - lo) / 600.0,
                };
                ui.add(
                    egui::DragValue::new(&mut *value)
                        .range(range.clone())
                        .speed(speed)
                        .fixed_decimals(decimals)
                        .suffix(suffix)
                        .custom_formatter(move |v, _| grouped(v, decimals))
                        .custom_parser(parse_number),
                )
            })
            .inner;
        let plus = step_half(ui, egui_phosphor::regular::PLUS, *value < hi, false);
        let round = |v: f64| {
            let k = 10f64.powi(decimals as i32);
            ((v * k).round() / k).clamp(lo, hi)
        };
        if minus.clicked() {
            *value = round(steps.step(*value, false, lo, hi));
        }
        if plus.clicked() {
            *value = round(steps.step(*value, true, lo, hi));
        }
        if *value != before {
            resp.mark_changed();
        }
        (resp, minus.rect.right(), plus.rect.left())
    });
    let (resp, left, right) = row.inner;
    let rect = row.response.rect;
    let r = CornerRadius::same((rect.height() / 2.0).min(255.0) as u8);
    ui.painter().set(
        under,
        egui::Shape::Vec(vec![
            Shadow { offset: [0, 1], blur: 4, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(rect, r).into(),
            egui::Shape::rect_filled(rect, r, Color32::from_white_alpha(16)),
        ]),
    );
    let p = ui.painter();
    p.rect_stroke(rect, r, Stroke::new(1.0, Color32::from_white_alpha(14)), egui::StrokeKind::Inside);
    let top = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.center().y));
    p.with_clip_rect(top).rect_stroke(rect, r, Stroke::new(1.0, Color32::from_white_alpha(34)), egui::StrokeKind::Inside);
    for x in [left, right] {
        p.vline(x, rect.y_range().shrink(7.0), Stroke::new(1.0, Color32::from_white_alpha(22)));
    }
    resp
}

/// A [`number`] of whole units (stops, light years, percent).
pub fn whole(ui: &mut Ui, value: &mut u32, range: std::ops::RangeInclusive<u32>, steps: Steps, suffix: &str) -> Response {
    let mut v = f64::from(*value);
    let resp = number(ui, &mut v, f64::from(*range.start())..=f64::from(*range.end()), steps, 0, suffix);
    *value = (v.round().max(0.0) as u32).clamp(*range.start(), *range.end());
    resp
}

/// The − or + end of a [`number`] capsule.
fn step_half(ui: &mut Ui, glyph: &str, enabled: bool, left: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), if enabled { Sense::click() } else { Sense::hover() });
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if enabled && resp.hovered() {
            let round = if left { CornerRadius { nw: 14, sw: 14, ne: 0, se: 0 } } else { CornerRadius { nw: 0, sw: 0, ne: 14, se: 14 } };
            p.rect_filled(rect, round, Color32::from_white_alpha(if resp.is_pointer_button_down_on() { 30 } else { 14 }));
        }
        p.text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(size::CALLOUT), if enabled { pal::TEXT } else { pal::FAINT });
    }
    resp
}

/// `v` with `decimals` places and its thousands grouped: 50,000 or 19.6.
pub fn grouped(v: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, v.abs());
    let (int, frac) = s.split_once('.').map_or((s.as_str(), None), |(i, f)| (i, Some(f)));
    let mut out = String::with_capacity(s.len() + int.len() / 3 + 1);
    if v < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0') {
        out.push('-');
    }
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

/// A typed number: separators and spaces are ignored, and so is a unit after it ("50,000 ls" is 50000). One comma
/// before the last one or two digits, with no point, is a decimal comma ("19,6" is 19.6).
pub fn parse_number(s: &str) -> Option<f64> {
    let t: String = s.trim().replace('\u{2212}', "-").chars().filter(|c| !matches!(c, ' ' | '_' | '\u{a0}' | '\u{202f}')).collect();
    let decimals = |i: usize| t[i + 1..].chars().take_while(char::is_ascii_digit).count();
    let t = match (t.matches(',').count(), t.contains('.'), t.find(',')) {
        (1, false, Some(i)) if (1..=2).contains(&decimals(i)) => t.replacen(',', ".", 1),
        _ => t.replace(',', ""),
    };
    let end = t.char_indices().find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && matches!(c, '-' | '+')))).map_or(t.len(), |(i, _)| i);
    t[..end].parse().ok()
}

/// One option of [`choices`]: its icon, its name, and when to pick it.
pub struct Choice<'a, T> {
    pub value: T,
    pub icon: &'a str,
    pub title: &'a str,
    pub detail: &'a str,
}

/// Options as cards that say what each one is for, so the choice is made knowing the difference (macOS's appearance
/// picker, with words): equal cards side by side, each at least `min_width` wide, wrapping onto more rows as the
/// window narrows; the one picked is ringed in the accent colour. Returns the option clicked this frame, the picked
/// one included.
pub fn choices<T: PartialEq + Copy>(ui: &mut Ui, value: T, options: &[Choice<'_, T>], min_width: f32) -> Option<T> {
    const PAD: f32 = 14.0;
    const ICON: f32 = 26.0;
    let base = ui.id().with(("choices", ui.next_auto_id()));
    let avail = ui.available_width();
    let cols = (((avail + GAP) / (min_width + GAP)).floor() as usize).clamp(1, options.len().max(1));
    let w = (avail - GAP * (cols as f32 - 1.0)) / cols as f32;
    // Every card's text is laid out first: the cards of a row share the tallest one's height.
    let text: Vec<_> = options
        .iter()
        .map(|o| {
            let title = ui.painter().layout(o.title.to_owned(), display(size::HEADLINE), pal::TEXT, w - 2.0 * PAD - ICON);
            let detail = ui.painter().layout(o.detail.to_owned(), FontId::proportional(size::FOOTNOTE), pal::DIM, w - 2.0 * PAD);
            (title, detail)
        })
        .collect();
    let mut clicked = None;
    for (row, cells) in text.chunks(cols).enumerate() {
        let h = cells.iter().map(|(t, d)| 2.0 * PAD + t.size().y + 5.0 + d.size().y).fold(0.0, f32::max);
        let (band, _) = ui.allocate_exact_size(Vec2::new(avail, h), Sense::hover());
        for (j, (title, detail)) in cells.iter().enumerate() {
            let i = row * cols + j;
            let o = &options[i];
            let rect = Rect::from_min_size(Pos2::new(band.left() + j as f32 * (w + GAP), band.top()), Vec2::new(w, h));
            let resp = ui.interact(rect, base.with(i), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let picked = o.value == value;
            resp.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, true, picked, o.title));
            if resp.clicked() {
                clicked = Some(o.value);
            }
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let p = ui.painter();
            let round = CornerRadius::same(12);
            p.rect_filled(rect, round, if resp.hovered() && !picked { Color32::from_rgb(52, 52, 55) } else { pal::CARD });
            if picked {
                p.rect_filled(rect, round, pal::ACCENT.gamma_multiply(0.08));
                p.rect_stroke(rect, round, Stroke::new(1.5, pal::ACCENT), egui::StrokeKind::Inside);
            }
            let top = rect.min + Vec2::splat(PAD);
            p.text(Pos2::new(top.x, top.y + title.size().y / 2.0), Align2::LEFT_CENTER, o.icon, FontId::proportional(17.0), if picked { pal::ACCENT } else { pal::DIM });
            p.galley(Pos2::new(top.x + ICON, top.y), title.clone(), pal::TEXT);
            p.galley(Pos2::new(top.x, top.y + title.size().y + 5.0), detail.clone(), pal::DIM);
        }
        if row + 1 < text.len().div_ceil(cols) {
            ui.add_space(GAP - ui.spacing().item_spacing.y);
        }
    }
    clicked
}

/// A capsule search field with a magnifying-glass icon.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    search_capsule(ui, text, hint, width, None).inner
}

/// The search capsule: the text field's response inside the capsule's.
fn search_capsule(ui: &mut Ui, text: &mut String, hint: &str, width: f32, id: Option<egui::Id>) -> egui::InnerResponse<Response> {
    Frame::new()
        .fill(pal::FIELD)
        .stroke(Stroke::new(1.0, pal::SEPARATOR))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin { left: 12, right: 10, top: 0, bottom: 0 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS).size(14.0).color(pal::DIM));
                let mut edit = egui::TextEdit::singleline(text).frame(Frame::new().inner_margin(Margin::symmetric(0, 4))).hint_text(hint).vertical_align(Align::Center);
                if let Some(id) = id {
                    edit = edit.id(id);
                }
                ui.add_sized([width - 40.0, 28.0], edit)
            })
            .inner
        })
}

/// One entry of an [`autocomplete`] list: the text it puts in the field, and a dim detail on the right.
pub struct Suggestion {
    pub text: String,
    pub detail: String,
    /// Drawn in yellow behind a ★, as the game marks rare goods.
    pub starred: bool,
}

/// What an [`autocomplete`] field did this frame.
pub struct Completion {
    /// The suggestion taken this frame (a click, or Enter on the highlighted one); it is in the field now.
    pub picked: Option<String>,
    /// Enter pressed while no suggestion list was open.
    pub submitted: bool,
    /// The field has the keyboard focus (it's being typed in).
    pub focused: bool,
    /// `picked` came from the whole list (`suggest("")`: the ▾, or the field empty), not from matches to typed text.
    pub whole_list: bool,
}

#[derive(Clone, Copy, Default)]
struct CompletionState {
    open: bool,
    sel: usize,
    /// Where the list was drawn last frame (a click on it must not close it).
    list: Option<Rect>,
    /// Opened with the ▾ button: the whole list, whatever the field holds.
    all: bool,
    /// Just opened: the list scrolls to the highlighted entry.
    scroll: bool,
}

/// A combobox's list opens on what the field holds (the blank entry when it's empty, if the list has one).
fn current_entry(items: &[Suggestion], text: &str) -> usize {
    items.iter().position(|it| it.text == text).unwrap_or(0)
}

/// A capsule search field with a suggestion list underneath. The list opens as you type (or when you
/// click into the empty field), ↑/↓ move the highlight, Enter or a click takes a suggestion, Escape
/// or a click elsewhere closes it. `suggest` gets the text and returns suggestions, best first.
pub fn autocomplete(ui: &mut Ui, id_salt: &str, text: &mut String, hint: &str, width: f32, suggest: impl Fn(&str) -> Vec<Suggestion>) -> Completion {
    completion(ui, id_salt, text, hint, width, false, suggest)
}

/// An [`autocomplete`] field with a ▾ button that opens the whole list (`suggest("")`): type to narrow it, or pick.
/// Unlike [`autocomplete`]: entering the field selects its text, the list opens on the entry the field holds, and
/// Tab takes the highlighted entry as Enter does.
pub fn combobox(ui: &mut Ui, id_salt: &str, text: &mut String, hint: &str, width: f32, suggest: impl Fn(&str) -> Vec<Suggestion>) -> Completion {
    completion(ui, id_salt, text, hint, width, true, suggest)
}

fn completion(ui: &mut Ui, id_salt: &str, text: &mut String, hint: &str, width: f32, dropdown: bool, suggest: impl Fn(&str) -> Vec<Suggestion>) -> Completion {
    let id = ui.make_persistent_id(id_salt);
    let edit_id = id.with("edit");
    let mut s: CompletionState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let mut items = if s.open { suggest(if s.all { "" } else { text }) } else { Vec::new() };
    // The items are the whole list, not matches to typed text.
    let mut whole = s.all || text.trim().is_empty();
    // Arrow keys move the highlight; they are taken before the text field sees them.
    let mut moved = false;
    if s.open && !items.is_empty() && ui.memory(|m| m.has_focus(edit_id)) {
        let n = items.len();
        ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                s.sel = (s.sel + 1) % n;
                moved = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                s.sel = (s.sel + n - 1) % n;
                moved = true;
            }
        });
    }
    let capsule = search_capsule(ui, text, hint, width, Some(edit_id));
    let resp = capsule.inner;
    // Entering a combobox selects what's in it, so typing replaces it.
    if dropdown
        && resp.gained_focus()
        && let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit_id)
    {
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()))));
        state.store(ui.ctx(), edit_id);
    }
    if resp.changed() || (resp.gained_focus() && text.trim().is_empty()) {
        s.open = true;
        s.all = false;
        items = suggest(text);
        whole = text.trim().is_empty();
        s.sel = if dropdown && !resp.changed() { current_entry(&items, text) } else { 0 };
    }
    let arrow = dropdown.then(|| {
        ui.add(egui::Button::new(RichText::new(egui_phosphor::regular::CARET_DOWN).size(15.0).color(pal::DIM)).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Show the list")
    });
    s.sel = s.sel.min(items.len().saturating_sub(1));
    let mut picked = None;
    let mut submitted = false;
    if resp.lost_focus() {
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let tab = dropdown && s.open && !items.is_empty() && ui.input(|i| i.key_pressed(egui::Key::Tab));
        if enter || tab {
            if s.open && !items.is_empty() {
                picked = Some(s.sel);
            } else {
                submitted = true;
            }
            s.open = false;
        } else if !s.list.zip(ui.ctx().pointer_latest_pos()).is_some_and(|(r, p)| r.contains(p)) {
            // Tab or a click elsewhere; a press on the list keeps it open until the click lands.
            s.open = false;
        }
    }
    // The ▾ click would count as a click outside the list: it shows from the next frame on, with the field focused.
    let opened = arrow.is_some_and(|r| r.clicked());
    if opened {
        s.open = true;
        s.all = true;
        items = suggest("");
        whole = true;
        s.sel = current_entry(&items, text);
        s.scroll = true;
        ui.memory_mut(|m| m.request_focus(edit_id));
    }
    s.list = None;
    if s.open && !items.is_empty() && !opened {
        let mut open = true;
        let shown = egui::Popup::new(id.with("list"), ui.ctx().clone(), &capsule.response, ui.layer_id())
            .open_bool(&mut open)
            // A combobox closes by its own rule (the field loses the focus, not to the list): egui's would take the
            // release of the click that opened it for a click outside, and close it again at once.
            .close_behavior(if dropdown { egui::PopupCloseBehavior::IgnoreClicks } else { egui::PopupCloseBehavior::CloseOnClickOutside })
            .width(capsule.response.rect.width())
            .gap(4.0)
            .show(|ui| {
                ui.set_min_width(capsule.response.rect.width() - 16.0);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, it) in items.iter().enumerate() {
                        let r = suggestion_row(ui, it, i == s.sel);
                        if r.clicked() {
                            picked = Some(i);
                        }
                        if (moved || s.scroll) && i == s.sel {
                            r.scroll_to_me(None);
                        }
                    }
                });
            });
        s.list = shown.map(|r| r.response.rect);
        s.scroll = false;
        if !open {
            s.open = false;
        }
    }
    let picked = picked.and_then(|i| items.into_iter().nth(i)).map(|it| {
        text.clone_from(&it.text);
        s.open = false;
        it.text
    });
    ui.data_mut(|d| d.insert_temp(id, s));
    let whole_list = whole && picked.is_some();
    Completion { picked, submitted, focused: ui.memory(|m| m.has_focus(edit_id)), whole_list }
}

fn suggestion_row(ui: &mut Ui, it: &Suggestion, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if selected || resp.hovered() {
            p.rect_filled(rect, CornerRadius::same(6), if selected { pal::ACCENT_BG } else { Color32::from_white_alpha(14) });
        }
        let detail = p.layout_no_wrap(it.detail.clone(), FontId::proportional(size::CAPTION), pal::DIM);
        let room = (rect.width() - detail.size().x - 28.0).max(40.0);
        let (label, col) = if it.starred { (format!("★ {}", it.text), pal::GOLD) } else { (it.text.clone(), if selected { pal::ACCENT_SOFT } else { pal::TEXT }) };
        let mut job = egui::text::LayoutJob::simple_singleline(label, FontId::proportional(size::CALLOUT), col);
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        let main = p.layout_job(job);
        p.galley(Pos2::new(rect.left() + 8.0, rect.center().y - main.size().y / 2.0), main, pal::TEXT);
        p.galley(Pos2::new(rect.right() - 8.0 - detail.size().x, rect.center().y - detail.size().y / 2.0), detail, pal::DIM);
    }
    resp
}

// ---------------------------------------------------------------------------------------------
// Formatting

pub fn credits(v: i64) -> String {
    format!("{} Cr", nexus_data::names::group_digits(v))
}

pub fn short_credits(v: i64) -> String {
    format!("{} Cr", nexus_data::names::compact_credits(v))
}

/// "3 m ago", "2 h ago" for a unix timestamp.
pub fn ago(ts: nexus_journal::Timestamp) -> String {
    if ts.is_zero() {
        return "never".into();
    }
    let s = (nexus_journal::Timestamp::now().0 - ts.0).max(0);
    match s {
        0..=59 => format!("{s}s ago"),
        60..=3599 => format!("{}m ago", s / 60),
        3600..=86_399 => format!("{}h {}m ago", s / 3600, (s % 3600) / 60),
        _ => format!("{}d ago", s / 86_400),
    }
}

pub fn duration(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 3600 { format!("{}h {:02}m", s / 3600, (s % 3600) / 60) } else { format!("{}m {:02}s", s / 60, s % 60) }
}

/// Plot coordinates under the cursor, whichever hover variant egui_plot reports.
pub fn hover_xy(h: &egui_plot::HoverPosition<'_>) -> (f64, f64) {
    match h {
        egui_plot::HoverPosition::NearDataPoint { position, .. } | egui_plot::HoverPosition::Elsewhere { position } => (position.x, position.y),
    }
}

/// A bar chart's label as a tooltip by the pointer, which always fits: egui_plot writes a hovered bar's label above its
/// corner, where the plot's edge cuts off the tallest bars and the last one. `label` gets the slot under the pointer
/// (x rounded down: bars sit at slot + 0.5); the chart blanks its own with `element_formatter(Box::new(|_, _| String::new()))`.
pub fn bar_tooltip<R>(plot: &egui_plot::PlotResponse<R>, label: impl FnOnce(i64) -> Option<String>) {
    let Some(pos) = plot.response.hover_pos() else { return };
    let Some(text) = label(plot.transform.value_from_position(pos).x.floor() as i64) else { return };
    egui::Tooltip::always_open(plot.response.ctx.clone(), plot.response.layer_id, plot.response.id, egui::PopupAnchor::Pointer).gap(12.0).show(|ui| {
        ui.label(text);
    });
}

/// The x axis of a chart per day (days from `nexus_journal::time::LocalDays`, bars at day + 0.5): each day's mark
/// named by its date. Finer marks stay blank, or a chart of a day or two would write its date on every tenth of a day.
pub fn day_axis(m: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>) -> String {
    if m.step_size < 1.0 { String::new() } else { nexus_journal::time::LocalDays::label(m.value.round() as i64, "%d %b") }
}

/// The points between two labels up a chart's side, at least: egui_plot fades a label that comes closer and drops it
/// within 20.
pub const LABEL_GAP: f64 = 30.0;

/// The value a grid spacer's `base_step_size` spans per point: egui_plot passes the value of `grid_spacing`'s lower
/// bound, 8 points unless a chart sets its own (none here does).
pub fn value_per_point(input: &egui_plot::GridInput) -> f64 {
    input.base_step_size / 8.0
}

/// Marks up a chart's side (its `y_grid_spacer`) at a round step (see [`round_step`]; `unit` is 1 for counts), the
/// finest whose labels stand `LABEL_GAP` apart. egui_plot's own marks go by powers of ten only: a chart up to 800
/// merits had marks every 100, too close together to label, and every 1,000, where only 0 was on it.
pub fn round_marks(unit: f64) -> impl Fn(egui_plot::GridInput) -> Vec<egui_plot::GridMark> {
    move |input| marks_every(input.bounds, round_step(value_per_point(&input) * LABEL_GAP, unit))
}

/// A mark at every multiple of `step` from `lo` to `hi` (none for a step that would make more than a thousand).
pub fn marks_every((lo, hi): (f64, f64), step: f64) -> Vec<egui_plot::GridMark> {
    if !(step.is_finite() && step > 0.0 && lo <= hi) || (hi - lo) / step > 1_000.0 {
        return Vec::new();
    }
    ((lo / step).ceil() as i64..=(hi / step).floor() as i64).map(|i| egui_plot::GridMark { value: i as f64 * step, step_size: step }).collect()
}

/// The smallest of 1, 2, 2.5 and 5 times a power of ten that is at least `least` and a whole number of `unit`s (a power
/// of ten: what the labels can write). 2.5 times a power of ten is one from 25 units up: 25 or 250 merits, not 2.5.
pub fn round_step(least: f64, unit: f64) -> f64 {
    let least = least.max(unit);
    if !(least.is_finite() && least > 0.0) {
        return 1.0;
    }
    let mut power = 10f64.powf(least.log10().floor());
    loop {
        for m in [1.0, 2.0, 2.5, 5.0] {
            // A hair's leeway: 200 can come out of the logarithm as 199.99…
            if (m != 2.5 || power >= 10.0 * unit * (1.0 - 1e-9)) && m * power >= least * (1.0 - 1e-9) {
                return m * power;
            }
        }
        power *= 10.0;
    }
}

// ---------------------------------------------------------------------------------------------
// The overlay's island

/// What the overlay's island is drawn with (overlay/draw.rs lays it out): its colours and type, text with tabular
/// figures, glyphs, a star, the radar dial and the ring of progress. Sizes are the island's points times `k`, the
/// overlay's own scale.
pub mod island {
    use crate::overlay::{Dial, DialKind, Glyph, StarTone, Style, Tint};
    use crate::theme::{bold, display, icon_fill, medium, pal};
    use egui::emath::Rot2;
    use egui::text::{LayoutJob, TextWrapping};
    use egui::{Align2, Color32, CornerRadius, FontId, Galley, Mesh, Painter, Pos2, Rect, Shape, Stroke, TextFormat, Vec2};
    use std::sync::Arc;

    /// A tint's colour.
    pub fn tint(t: Tint) -> Color32 {
        match t {
            Tint::White => Color32::WHITE,
            Tint::Dim => pal::ISLAND_DIM,
            Tint::Mint => pal::MINT,
            Tint::Orange => pal::ACCENT,
            Tint::Blue => pal::BLUE,
            Tint::Cyan => pal::CYAN,
            Tint::Gold => pal::GOLD,
            Tint::Indigo => pal::INDIGO,
            Tint::Green => pal::GOOD,
            Tint::Yellow => pal::WARN,
            Tint::Red => pal::BAD,
        }
    }

    /// A style's font at scale `k`, never below `min` points (12; 14 in VR).
    pub fn font(style: Style, k: f32, min: f32) -> FontId {
        let (size, face): (f32, fn(f32) -> FontId) = match style {
            Style::Caption => (13.0, medium),
            Style::Label => (13.0, display),
            Style::Value => (15.0, display),
            Style::Title => (17.0, display),
            Style::Figure => (20.0, bold),
            Style::Hero => (34.0, bold),
        };
        face(size.max(min) * k)
    }

    /// One line of text, cut short with "…" past `max_width`. With `tabular`, every digit takes the width of the
    /// widest (tabular figures), so a value that changes doesn't make what's beside it jump.
    pub fn text(p: &Painter, text: &str, font: FontId, color: Color32, tabular: bool, max_width: f32) -> Arc<Galley> {
        let mut job = LayoutJob { wrap: TextWrapping::truncate_at_width(max_width), ..Default::default() };
        let format = TextFormat::simple(font.clone(), color);
        if tabular && text.contains(|c: char| c.is_ascii_digit()) {
            let advance = |d: char| p.layout_no_wrap(d.to_string(), font.clone(), color).size().x;
            let cell = ('0'..='9').map(advance).fold(0.0, f32::max);
            // Each digit is centred in its cell: half the slack before it, half before whatever follows.
            let (mut start, mut pending) = (0, 0.0);
            for (i, c) in text.char_indices().filter(|(_, c)| c.is_ascii_digit()) {
                if start < i {
                    job.append(&text[start..i], pending, format.clone());
                    pending = 0.0;
                }
                let pad = ((cell - advance(c)) / 2.0).max(0.0);
                job.append(&text[i..i + 1], pending + pad, format.clone());
                pending = pad;
                start = i + 1;
            }
            if start < text.len() {
                job.append(&text[start..], pending, format);
            }
        } else {
            job.append(text, 0.0, format);
        }
        p.layout_job(job)
    }

    /// A glyph's width (and height) at scale `k`.
    pub fn glyph_width(g: &Glyph, k: f32) -> f32 {
        k * match g {
            Glyph::Icon { size, .. } | Glyph::Star { size, .. } | Glyph::Arrow { size, .. } => f32::from(*size),
            Glyph::Dot { .. } => 8.0,
        }
    }

    /// Paints a glyph centred on `c`.
    pub fn glyph(p: &Painter, g: &Glyph, c: Pos2, k: f32) {
        match g {
            Glyph::Icon { icon, tint: t, size } => {
                p.text(c, Align2::CENTER_CENTER, *icon, icon_fill(f32::from(*size) * k), tint(*t));
            }
            Glyph::Star { class, size } => star(p, c, f32::from(*size) * k / 2.0, class, k),
            Glyph::Arrow { turn, tint: t, size } => arrow(p, c, f32::from(*size) * k, f32::from(*turn), tint(*t), pal::ISLAND),
            Glyph::Dot { tint: t, hollow: true } => {
                p.circle_stroke(c, 3.25 * k, Stroke::new(1.5 * k, tint(*t)));
            }
            Glyph::Dot { tint: t, hollow: false } => {
                p.circle_filled(c, 4.0 * k, tint(*t));
            }
        }
    }

    /// A star's colours: lit, middle, rim, and its letter's.
    fn star_colours(t: StarTone) -> [Color32; 4] {
        let c = Color32::from_rgb;
        match t {
            StarTone::Blue => [c(225, 236, 255), c(150, 185, 255), c(70, 110, 220), c(8, 18, 48)],
            StarTone::White => [c(255, 255, 255), c(222, 230, 255), c(160, 175, 225), c(18, 24, 44)],
            StarTone::YellowWhite => [c(255, 252, 235), c(255, 240, 200), c(222, 200, 140), c(44, 34, 10)],
            StarTone::Yellow => [c(255, 246, 205), c(255, 220, 120), c(218, 165, 45), c(44, 28, 0)],
            StarTone::Orange => [c(255, 226, 184), c(255, 180, 90), c(217, 115, 28), c(42, 20, 0)],
            StarTone::Red => [c(255, 205, 175), c(255, 128, 88), c(196, 66, 38), c(44, 10, 0)],
            StarTone::Brown => [c(232, 176, 146), c(168, 102, 80), c(104, 58, 48), c(255, 238, 228)],
            StarTone::Neutron => [c(225, 252, 255), c(120, 220, 255), c(36, 146, 218), c(0, 24, 44)],
            StarTone::Dwarf => [c(248, 251, 255), c(206, 220, 255), c(146, 166, 220), c(14, 20, 40)],
            StarTone::Black => [c(96, 92, 120), c(44, 42, 58), c(14, 14, 22), c(240, 240, 245)],
        }
    }

    /// A star of radius `r`: a disc shaded in its colour as if lit from the upper left, a soft glow, its class letter.
    fn star(p: &Painter, c: Pos2, r: f32, class: &str, k: f32) {
        const N: u32 = 40;
        let [lit, mid, rim, ink] = star_colours(StarTone::of(class));
        let disc = Rect::from_center_size(c, Vec2::splat(2.0 * r));
        let round = CornerRadius::same(r.ceil().min(255.0) as u8);
        p.add(egui::Shadow { offset: [0, 0], blur: r.min(255.0) as u8, spread: 0, color: mid.gamma_multiply(0.45) }.as_shape(disc, round));
        let hot = c + Vec2::new(-0.32 * r, -0.4 * r);
        let mut mesh = Mesh::default();
        mesh.colored_vertex(hot, lit);
        for i in 0..N {
            let edge = c + Vec2::angled(i as f32 / N as f32 * std::f32::consts::TAU) * r;
            mesh.colored_vertex(hot + (edge - hot) * 0.5, mid);
            mesh.colored_vertex(edge, rim);
        }
        for i in 0..N {
            let (m0, e0, m1, e1) = (1 + 2 * i, 2 + 2 * i, 1 + 2 * ((i + 1) % N), 2 + 2 * ((i + 1) % N));
            mesh.add_triangle(0, m0, m1);
            mesh.add_triangle(m0, e0, e1);
            mesh.add_triangle(m0, e1, m1);
        }
        p.add(Shape::mesh(mesh));
        // The mesh has no anti-aliasing: a hairline smooths its edge.
        p.circle_stroke(c, r, Stroke::new(k, rim));
        let letter: String = class.chars().next().map(|l| l.to_ascii_uppercase()).into_iter().collect();
        p.text(c, Align2::CENTER_CENTER, letter, bold((r * 0.9).max(12.0 * k)), ink);
    }

    /// The navigation arrow, `size` wide, pointing `turn` degrees clockwise from up, on `background`.
    pub fn arrow(p: &Painter, c: Pos2, size: f32, turn: f32, color: Color32, background: Color32) {
        let rot = Rot2::from_angle(turn.to_radians());
        // In a 24-point box: the tip, the wings and the notch between them.
        let at = |x: f32, y: f32| c + rot * (Vec2::new(x - 12.0, y - 12.0) * (size / 24.0));
        let (tip, right, notch, left) = (at(12.0, 2.8), at(18.6, 20.2), at(12.0, 16.6), at(5.4, 20.2));
        p.add(Shape::convex_polygon(vec![tip, right, left], color, Stroke::NONE));
        p.add(Shape::convex_polygon(vec![right, notch, left], background, Stroke::NONE));
    }

    /// The fuel gauge in `rect`: red under a quarter, yellow under half.
    pub fn fuel(p: &Painter, rect: Rect, pct: u8) {
        let round = CornerRadius::same((rect.height() / 2.0).ceil().min(255.0) as u8);
        p.rect_filled(rect, round, pal::ISLAND_TRACK);
        let col = match pct {
            0..25 => pal::BAD,
            25..50 => pal::WARN,
            _ => pal::CYAN,
        };
        p.rect_filled(Rect::from_min_size(rect.min, Vec2::new(rect.width() * f32::from(pct.min(100)) / 100.0, rect.height())), round, col);
    }

    /// The radar dial of radius `r`: rings, a tick where north is, the marks, and you in the middle facing up.
    pub fn dial(p: &Painter, c: Pos2, r: f32, d: &Dial, k: f32) {
        p.circle_filled(c, r, pal::ISLAND_DIAL);
        let p = p.with_clip_rect(Rect::from_center_size(c, Vec2::splat(2.0 * r)));
        let n = f32::from(d.rings) + 1.0;
        for i in 1..=d.rings {
            p.circle_stroke(c, r * f32::from(i) / n, Stroke::new(k, pal::ISLAND_LINE));
        }
        let north = Rot2::from_angle(f32::from(d.north).to_radians());
        p.line_segment([c + north * Vec2::new(0.0, 2.0 * k - r), c + north * Vec2::new(0.0, 8.0 * k - r)], Stroke::new(2.0 * k, pal::ISLAND_DIM));
        let inner = r - 4.0 * k;
        for m in &d.marks {
            let at = c + Vec2::new(f32::from(m.x), -f32::from(m.y)) / 100.0 * inner;
            match m.kind {
                DialKind::Sample { range, close } => {
                    let col = if close { pal::BAD } else { pal::GOOD };
                    let rr = f32::from(range) / 100.0 * inner;
                    p.circle_filled(at, rr, col.gamma_multiply(0.16));
                    p.circle_stroke(at, rr, Stroke::new(k, col.gamma_multiply(0.75)));
                    p.circle_filled(at, 2.6 * k, col);
                }
                DialKind::Deposit { nearest, number } => {
                    let col = if nearest { pal::ACCENT } else { pal::ACCENT.gamma_multiply(0.65) };
                    let s = 4.2 * k;
                    p.add(Shape::convex_polygon(vec![at + Vec2::new(0.0, -s), at + Vec2::new(s, 0.0), at + Vec2::new(0.0, s), at + Vec2::new(-s, 0.0)], col, Stroke::NONE));
                    if let Some(n) = number {
                        p.text(at - Vec2::new(s + 2.0 * k, 0.0), Align2::RIGHT_CENTER, n.to_string(), bold(12.0 * k), col);
                    }
                }
                DialKind::Ship => {
                    p.circle_filled(at, 3.4 * k, pal::CYAN);
                }
                DialKind::Landing => {
                    p.circle_stroke(at, 2.9 * k, Stroke::new(1.4 * k, pal::ISLAND_DIM));
                }
            }
        }
        arrow(&p, c, 12.0 * k, 0.0, Color32::WHITE, pal::ISLAND_DIAL);
    }

    /// A ring of progress of radius `r`, `done` of `of`, with the count inside.
    pub fn ring(p: &Painter, c: Pos2, r: f32, done: u8, of: u8, k: f32) {
        let w = 3.2 * k;
        p.circle_stroke(c, r, Stroke::new(w, pal::ISLAND_TRACK));
        let f = f32::from(done.min(of)) / f32::from(of.max(1));
        if f > 0.0 {
            let n = (48.0 * f).ceil().max(2.0) as usize;
            let arc = (0..=n).map(|i| c + Vec2::angled(-std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * f * i as f32 / n as f32) * r).collect();
            p.add(Shape::line(arc, Stroke::new(w, pal::MINT)));
        }
        p.text(c, Align2::CENTER_CENTER, format!("{done}/{of}"), bold(12.0 * k), Color32::WHITE);
    }

    /// A small capsule whose left edge is at `left` (vertically centred there): mint, or dimmed and struck through
    /// once done. Returns its width.
    pub fn chip(p: &Painter, left: Pos2, text: &str, done: bool, k: f32, min: f32) -> f32 {
        let (fill, col) = if done { (Color32::from_rgb(20, 20, 20), pal::ISLAND_DIM) } else { (Color32::from_rgb(16, 34, 33), pal::MINT) };
        let mut format = TextFormat::simple(display(12.0_f32.max(min) * k), col);
        if done {
            format.strikethrough = Stroke::new(k, col);
        }
        let mut job = LayoutJob::default();
        job.append(text, 0.0, format);
        let g = p.layout_job(job);
        let size = g.size() + Vec2::new(18.0 * k, 6.0 * k);
        let rect = Rect::from_min_size(Pos2::new(left.x, left.y - size.y / 2.0), size);
        p.rect_filled(rect, CornerRadius::same((size.y / 2.0).ceil().min(255.0) as u8), fill);
        p.galley(rect.min + Vec2::new(9.0 * k, 3.0 * k), g, col);
        size.x
    }

    /// The island's black shape with its hairline; over the game, a soft shadow under it.
    pub fn shape(p: &Painter, rect: Rect, radius: f32, shadow: bool, k: f32) {
        let round = CornerRadius::same(radius.ceil().clamp(0.0, 255.0) as u8);
        if shadow {
            p.add(egui::Shadow { offset: [0, (6.0 * k).min(127.0) as i8], blur: (14.0 * k).min(255.0) as u8, spread: 0, color: Color32::from_black_alpha(120) }.as_shape(rect, round));
        }
        p.rect_filled(rect, round, pal::ISLAND);
        p.rect_stroke(rect, round, Stroke::new(0.5 * k, pal::ISLAND_HAIR), egui::StrokeKind::Outside);
    }
}

/// What a chart `height` points tall gives a grid spacer for `bounds`, as egui_plot works it out.
#[cfg(test)]
pub fn grid_input(bounds: (f64, f64), height: f64) -> egui_plot::GridInput {
    egui_plot::GridInput { bounds, base_step_size: (bounds.1 - bounds.0) / height * 8.0 }
}

/// The values egui_plot labels among `marks`: those whose step leaves more than 20 points between labels.
#[cfg(test)]
pub fn labelled(marks: Vec<egui_plot::GridMark>, input: &egui_plot::GridInput) -> Vec<f64> {
    marks.into_iter().filter(|m| m.step_size / value_per_point(input) > 20.0).map(|m| m.value).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_keep_a_sliver_and_fill_the_bar() {
        // 304 points less the two gaps between the three parts above zero: 300.
        let w = share_widths([90.0, 9.0, 1.0, 0.0].into_iter(), 304.0, 2.0, 6.0);
        assert_eq!(w[3], 0.0);
        assert!((w.iter().sum::<f32>() - 300.0).abs() < 1e-3, "{w:?}");
        // The 1 % part keeps a sliver; the room comes off the widest.
        assert!(w[2] >= 6.0 && w[0] > w[1] && w[1] > w[2], "{w:?}");
        assert!(share_widths([0.0, 0.0].into_iter(), 100.0, 2.0, 6.0).iter().all(|x| *x == 0.0));
        assert!(share_widths([f64::NAN, 1.0].into_iter(), 100.0, 2.0, 6.0)[1] == 100.0);
    }

    #[test]
    fn steps_go_through_round_values() {
        let ly = [10.0, 25.0, 50.0, 100.0, 250.0];
        let step = |v, up| step_value(v, NumberSteps::Ladder(&ly), up, false, 1.0, 1000.0);
        assert_eq!((step(25.0, true), step(25.0, false)), (50.0, 10.0));
        // A typed value in between goes to the next rung either way.
        assert_eq!((step(60.0, true), step(60.0, false)), (100.0, 50.0));
        // Past the ends: the range's limits.
        assert_eq!((step(250.0, true), step(10.0, false), step(5.0, false)), (1000.0, 1.0, 1.0));
        // By size: the second digit, both ways the same around a power of ten; Shift ten times as far; never below 1.
        let mag = |v, up, big| step_value(v, NumberSteps::Magnitude, up, big, 0.0, 1e9);
        assert_eq!((mag(48_300.0, true, false), mag(48_300.0, false, false)), (49_000.0, 48_000.0));
        assert_eq!((mag(10_000.0, false, false), mag(9_900.0, true, false)), (9_900.0, 10_000.0));
        assert_eq!(mag(48_300.0, true, true), 50_000.0);
        assert_eq!((mag(0.0, true, false), mag(3.0, false, false), mag(0.0, false, false)), (1.0, 2.0, 0.0));
    }

    #[test]
    fn typed_numbers_read_like_people_write_them() {
        assert_eq!(parse_typed("48,300", " Cr"), Some(48_300.0));
        assert_eq!(parse_typed(" 48 300 cr ", " Cr"), Some(48_300.0));
        assert_eq!(parse_typed("1.5m", " Cr"), Some(1_500_000.0));
        assert_eq!(parse_typed("48k", ""), Some(48_000.0));
        assert_eq!(parse_typed("250ly", " ly"), Some(250.0));
        assert_eq!(parse_typed("72 h", " h"), Some(72.0));
        assert_eq!(parse_typed("anywhere", " ly"), None);
        assert_eq!(parse_typed("", " ly"), None);
    }

    #[test]
    fn a_heading_sorts_reverses_and_lets_go() {
        // Amounts start with the biggest; a second click reverses; a third gives the usual order back.
        let s = TableSort::default().clicked(2, true);
        assert_eq!(s, TableSort { column: Some(2), descending: true });
        let s = s.clicked(2, true);
        assert_eq!(s, TableSort { column: Some(2), descending: false });
        assert_eq!(s.clicked(2, true), TableSort::default());
        // Another column starts over.
        assert_eq!(s.clicked(0, false), TableSort { column: Some(0), descending: false });
    }

    #[test]
    fn numbers_step_through_round_values() {
        // By a fixed amount: to the next multiple, whatever was typed in between.
        assert_eq!(Steps::By(1.0).step(19.6, true, 10.0, 100.0), 20.0);
        assert_eq!(Steps::By(1.0).step(19.6, false, 10.0, 100.0), 19.0);
        assert_eq!(Steps::By(1.0).step(20.0, true, 10.0, 100.0), 21.0);
        assert_eq!(Steps::By(1.0).step(20.0, false, 10.0, 100.0), 19.0);
        assert_eq!(Steps::By(5.0).step(98.0, true, 1.0, 100.0), 100.0, "never past the end");
        // Through a ladder: to the next rung, the ends of the range past the last one.
        let ladder = [25.0, 50.0, 100.0, 250.0];
        assert_eq!(Steps::Through(&ladder).step(50.0, true, 1.0, 1000.0), 100.0);
        assert_eq!(Steps::Through(&ladder).step(60.0, false, 1.0, 1000.0), 50.0);
        assert_eq!(Steps::Through(&ladder).step(250.0, true, 1.0, 1000.0), 1000.0);
        assert_eq!(Steps::Through(&ladder).step(25.0, false, 1.0, 1000.0), 1.0);
    }

    #[test]
    fn numbers_are_written_grouped_and_read_back() {
        assert_eq!(grouped(50_000.0, 0), "50,000");
        assert_eq!(grouped(1_234_567.891, 2), "1,234,567.89");
        assert_eq!(grouped(19.6, 1), "19.6");
        assert_eq!(grouped(999.0, 0), "999");
        assert_eq!(grouped(-1500.0, 0), "-1,500");
        assert_eq!(grouped(-0.01, 0), "0", "no minus on a zero");
        for (typed, value) in [("50,000", 50_000.0), ("50 000 ls", 50_000.0), ("19.6 ly", 19.6), ("−3", -3.0), (" 1.25M Cr", 1.25), ("19,6", 19.6), ("0,5 M Cr", 0.5), ("1,250", 1_250.0), ("1,234,567.5", 1_234_567.5)] {
            assert_eq!(parse_number(typed), Some(value), "{typed}");
        }
        assert_eq!(parse_number("ly"), None);
    }

    #[test]
    fn rows_come_in_the_chosen_order() {
        let rows = [("b", Some(3.0)), ("A", None), ("c", Some(1.0))];
        let cmp = |a: &(&str, Option<f64>), b: &(&str, Option<f64>), c: u8| if c == 0 { cmp_text(a.0, b.0) } else { cmp_opt(a.1, b.1) };
        assert_eq!(TableSort::default().order(&rows, cmp), vec![0, 1, 2], "unsorted: as they came");
        assert_eq!(TableSort { column: Some(0), descending: false }.order(&rows, cmp), vec![1, 0, 2], "text ignores case");
        assert_eq!(TableSort { column: Some(1), descending: false }.order(&rows, cmp), vec![2, 0, 1], "unknown last");
        assert_eq!(TableSort { column: Some(1), descending: true }.order(&rows, cmp), vec![1, 0, 2]);
    }

    #[test]
    fn counts_get_labels_up_the_side() {
        // A merits chart up to 800 a day, 122 points tall, with the 5 % of room egui_plot adds above and below: its own
        // marks, every 10, 100 and 1,000, labelled only 0.
        let input = grid_input((-40.0, 840.0), 122.0);
        assert_eq!(labelled(egui_plot::log_grid_spacer(10)(grid_input((-40.0, 840.0), 122.0)), &input), [0.0]);
        assert_eq!(labelled(round_marks(1.0)(grid_input((-40.0, 840.0), 122.0)), &input), [0.0, 250.0, 500.0, 750.0]);
        // Up to a million a day on charts 100 to 400 points tall: every mark labelled, at least two, at whole numbers.
        for height in [100.0, 122.0, 182.0, 400.0] {
            let mut top = 1.0;
            while top < 1e6 {
                let bounds = (-0.05 * top, 1.05 * top);
                let marks = round_marks(1.0)(grid_input(bounds, height));
                let values = labelled(marks.clone(), &grid_input(bounds, height));
                assert!(values.len() == marks.len() && values.len() >= 2 && values.iter().all(|v| v.fract() == 0.0), "up to {top} at {height} pt: {values:?}");
                top = (top * 1.13).ceil();
            }
        }
        // Steps of 1, 2, 2.5 and 5 times a power of ten, whole numbers of the unit: 2.5 only from 25 units up.
        for (least, unit, step) in [(0.3, 1.0, 1.0), (1.5, 1.0, 2.0), (2.01, 1.0, 5.0), (7.0, 1.0, 10.0), (21.0, 1.0, 25.0), (216.0, 1.0, 250.0), (0.3, 0.1, 0.5), (2.2e5, 1e5, 5e5), (2.2e6, 1e5, 2.5e6)] {
            assert!((round_step(least, unit) - step).abs() < 1e-9 * step, "{least} in units of {unit}: {}", round_step(least, unit));
        }
    }
}
