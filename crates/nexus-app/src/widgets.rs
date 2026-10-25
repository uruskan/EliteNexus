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
            ui.set_max_width(460.0);
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

/// A small tinted pill (status, count, tag). Never wraps: in a wrapping row the whole pill moves to
/// the next line.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), display(size::CAPTION), color);
    let pad = Vec2::new(8.0, 3.0);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, CornerRadius::same(255), color.gamma_multiply(0.18));
        ui.painter().galley(rect.min + pad, galley, color);
    }
    resp
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

/// Collapsible "› Options" disclosure; returns true while open.
pub fn disclosure(ui: &mut Ui, open: &mut bool, label: &str) -> bool {
    let caret = if *open { egui_phosphor::regular::CARET_DOWN } else { egui_phosphor::regular::CARET_RIGHT };
    if ui.add(egui::Button::new(RichText::new(format!("{caret}  {label}")).size(size::CALLOUT).color(pal::DIM)).frame(false)).clicked() {
        *open = !*open;
    }
    *open
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
}

/// What an [`autocomplete`] field did this frame.
pub struct Completion {
    /// The suggestion taken this frame (a click, or Enter on the highlighted one); it is in the field now.
    pub picked: Option<String>,
    /// Enter pressed while no suggestion list was open.
    pub submitted: bool,
}

#[derive(Clone, Copy, Default)]
struct CompletionState {
    open: bool,
    sel: usize,
    /// Where the list was drawn last frame (a click on it must not close it).
    list: Option<Rect>,
}

/// A capsule search field with a suggestion list underneath. The list opens as you type (or when you
/// click into the empty field), ↑/↓ move the highlight, Enter or a click takes a suggestion, Escape
/// or a click elsewhere closes it. `suggest` gets the text and returns suggestions, best first.
pub fn autocomplete(ui: &mut Ui, id_salt: &str, text: &mut String, hint: &str, width: f32, suggest: impl Fn(&str) -> Vec<Suggestion>) -> Completion {
    let id = ui.make_persistent_id(id_salt);
    let edit_id = id.with("edit");
    let mut s: CompletionState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let mut items = if s.open { suggest(text) } else { Vec::new() };
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
    if resp.changed() || (resp.gained_focus() && text.trim().is_empty()) {
        s.open = true;
        s.sel = 0;
        items = suggest(text);
    }
    s.sel = s.sel.min(items.len().saturating_sub(1));
    let mut picked = None;
    let mut submitted = false;
    if resp.lost_focus() {
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
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
    s.list = None;
    if s.open && !items.is_empty() {
        let mut open = true;
        let shown = egui::Popup::new(id.with("list"), ui.ctx().clone(), &capsule.response, ui.layer_id())
            .open_bool(&mut open)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
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
                        if moved && i == s.sel {
                            r.scroll_to_me(None);
                        }
                    }
                });
            });
        s.list = shown.map(|r| r.response.rect);
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
    Completion { picked, submitted }
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
        let mut job = egui::text::LayoutJob::simple_singleline(it.text.clone(), FontId::proportional(size::CALLOUT), if selected { pal::ACCENT_SOFT } else { pal::TEXT });
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
                DialKind::Deposit { nearest } => {
                    let col = if nearest { pal::ACCENT } else { pal::ACCENT.gamma_multiply(0.65) };
                    let s = 4.2 * k;
                    p.add(Shape::convex_polygon(vec![at + Vec2::new(0.0, -s), at + Vec2::new(s, 0.0), at + Vec2::new(0.0, s), at + Vec2::new(-s, 0.0)], col, Stroke::NONE));
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn rows_come_in_the_chosen_order() {
        let rows = [("b", Some(3.0)), ("A", None), ("c", Some(1.0))];
        let cmp = |a: &(&str, Option<f64>), b: &(&str, Option<f64>), c: u8| if c == 0 { cmp_text(a.0, b.0) } else { cmp_opt(a.1, b.1) };
        assert_eq!(TableSort::default().order(&rows, cmp), vec![0, 1, 2], "unsorted: as they came");
        assert_eq!(TableSort { column: Some(0), descending: false }.order(&rows, cmp), vec![1, 0, 2], "text ignores case");
        assert_eq!(TableSort { column: Some(1), descending: false }.order(&rows, cmp), vec![2, 0, 1], "unknown last");
        assert_eq!(TableSort { column: Some(1), descending: true }.order(&rows, cmp), vec![1, 0, 2]);
    }
}
