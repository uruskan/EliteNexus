//! The overlay's window: borderless and always on top, at the top of the game's screen, beside the main window rather
//! than instead of it. The main window builds the view in `App::logic` (only when what it reads has changed) and hands
//! it over; the overlay's own frames draw it and never touch the model.
//!
//! It never takes the focus from the game: created inactive and shown without activation, click-through while locked,
//! and on Windows marked WS_EX_NOACTIVATE. Click-through is switched on only after the window has shown (set at
//! creation, Windows never shows it), and once it's on the window gets no input at all, so locking and unlocking live
//! in the main window. It hides (no frames at all) while the game isn't in front, and when there's nothing to say.
//!
//! See-through, the window is a fixed transparent canvas the island morphs in, and nothing of the window itself shows
//! ([`Desktop::see_through`]). With see-through off it's the island
//! itself ([`draw::cutout`]): sized to what it shows, where the island would be in that canvas, and on Windows cut to
//! its shape. It changes size only when the island changes shape, which costs a resize of a small window then, and
//! nothing while it's still: it isn't drawn at all between changes.

use super::draw::{self, Anchor, Anim, Look};
use super::platform::{self, Desktop};
use super::{Inputs, IslandView, Rhythm, Size, Sources};
use crate::pages::Page;
use egui::{Context, Pos2, Rect, Vec2, ViewportBuilder, ViewportCommand, ViewportId, WindowLevel, X11WindowType};
use nexus_model::{Alert, GameState};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The window's title. It has no title bar; Windows finds the window by it.
const TITLE: &str = "Elite Nexus overlay";

/// Why this computer can't draw a see-through window, as `main.rs` found at start. The island is then a window of its
/// own, as with see-through off, and Settings › Overlay says why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoSeeThrough {
    /// Windows: the graphics adapter isn't a DirectX 12 one, the only kind that draws through DirectComposition. On
    /// another, a see-through window is an opaque black canvas.
    NoDirectX12,
    /// The graphics driver offers no window surface with transparency.
    NoTransparency,
    /// X11 with no compositing manager running: nothing puts a window over what's behind it.
    NoCompositor,
}

/// What [`NoSeeThrough::record`] noted: 0 for nothing, otherwise the reason's place in [`NoSeeThrough::ALL`] plus one.
static NO_SEE_THROUGH: AtomicU8 = AtomicU8::new(0);

impl NoSeeThrough {
    const ALL: [NoSeeThrough; 3] = [NoSeeThrough::NoDirectX12, NoSeeThrough::NoTransparency, NoSeeThrough::NoCompositor];

    /// Notes what `main.rs` found at start; None: see-through can work. (macOS has nothing to rule it out.)
    #[cfg(any(windows, target_os = "linux"))]
    pub fn record(why: Option<NoSeeThrough>) {
        let n = why.and_then(|w| Self::ALL.iter().position(|a| *a == w)).map_or(0, |i| i as u8 + 1);
        NO_SEE_THROUGH.store(n, Ordering::Relaxed);
    }

    /// Why see-through can't work on this computer, when it can't.
    pub fn found() -> Option<NoSeeThrough> {
        Self::ALL.get(usize::from(NO_SEE_THROUGH.load(Ordering::Relaxed)).checked_sub(1)?).copied()
    }

    /// Why, in plain words (Settings › Overlay).
    pub fn words(self) -> &'static str {
        match self {
            NoSeeThrough::NoDirectX12 => "This PC's graphics can't draw a see-through window (on Windows that takes DirectX 12), so the island stays an opaque window of its own.",
            NoSeeThrough::NoTransparency => "This computer's graphics driver can't draw a see-through window, so the island stays an opaque window of its own.",
            NoSeeThrough::NoCompositor => "No compositor was running on this X11 desktop when Elite Nexus started, and without one nothing can be see-through, so the island stays an opaque window of its own. Start one, then restart Elite Nexus.",
        }
    }
}

/// The overlay's settings (Settings › Overlay).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayPrefs {
    /// The overlay is on.
    pub enabled: bool,
    /// Clicks go through to the game; unlocked, it can be dragged and a click opens its page.
    pub locked: bool,
    pub anchor: Anchor,
    /// Where it was dragged to: the see-through canvas's top-left corner, in points (the island's own window sits in
    /// it); None: at its anchor on the game's monitor.
    pub position: Option<[f32; 2]>,
    /// `position` is the canvas's. Earlier versions kept the opaque panel's top-left with see-through off; such a
    /// position is moved once so the island stays where the panel was ([`OverlayPrefs::settle_position`]).
    pub position_is_canvas: bool,
    /// Its own scale, 100 to 250 %.
    pub scale: f32,
    /// Keep the small empty island when there's nothing to say, instead of fading away (see-through or not).
    pub keep_minimal: bool,
    /// Stay visible whichever window is in front, not only the game or Elite Nexus.
    pub always_visible: bool,
    /// A see-through window around the island (applies after a restart). Off: the island is an opaque window of its
    /// own, its size and (on Windows) its shape.
    pub see_through: bool,
    /// VR: an opaque panel of fixed size that never hides, with nothing under 14 pt, for mirroring into a headset.
    pub vr: bool,
    /// Important alerts also go to XSOverlay on this computer (VR).
    pub xsoverlay: bool,
}

impl Default for OverlayPrefs {
    fn default() -> Self {
        OverlayPrefs {
            enabled: false,
            locked: true,
            anchor: Anchor::Centre,
            position: None,
            position_is_canvas: false,
            scale: 1.0,
            keep_minimal: false,
            always_visible: false,
            see_through: false,
            vr: false,
            xsoverlay: false,
        }
    }
}

impl OverlayPrefs {
    /// A position saved by an earlier version with see-through off was its opaque panel's top-left, where the open
    /// island now sits in the canvas: it moves by that much once, so the island stays where it was. `cut`: the island
    /// is its own window now; `k`: points per island point. True when the settings changed.
    pub fn settle_position(&mut self, cut: bool, k: f32) -> bool {
        if self.position_is_canvas {
            return false;
        }
        self.position_is_canvas = true;
        if cut && let Some([x, y]) = self.position {
            let d = draw::PANEL_OFFSET * k;
            self.position = Some([x - d.x, y - d.y]);
        }
        true
    }
}

/// The overlay's scales, in fixed steps (a slider would rescale under the pointer).
pub const SCALES: [(f32, &str); 6] = [(1.0, "100 %"), (1.25, "125 %"), (1.5, "150 %"), (1.75, "175 %"), (2.0, "200 %"), (2.5, "250 %")];

/// Shared with the overlay's own frames, which run on the same thread but can't borrow the app.
#[derive(Default)]
struct Shared {
    view: IslandView,
    look: Option<Look>,
    anim: Anim,
    /// Frames the overlay has painted since its window opened.
    painted: u64,
    /// Nothing is drawn any more: the island has faded away.
    faded_out: bool,
    /// The window is hidden: its frames ask for no more.
    hidden: bool,
    /// Closed from outside (Alt+F4 while it had the focus, unlocked).
    closed: bool,
    clicked: bool,
    /// Windows: the overlay's window, and the shape it was last given (its own pixels).
    window: Option<isize>,
    shaped: Option<(isize, Vec<[i32; 5]>)>,
}

/// What the main window last told the overlay's window.
#[derive(Default)]
struct Sent {
    /// The window has been asked for (it may not exist yet).
    open: bool,
    visible: Option<bool>,
    /// Frames painted when it was last shown.
    shown_at: Option<u64>,
    passthrough: Option<bool>,
    /// Its size (points).
    size: Option<Vec2>,
}

/// What the overlay asks of the main window.
#[derive(Default)]
pub struct Requests {
    /// A click on the unlocked island: open this page.
    pub open: Option<Page>,
    /// The overlay's settings changed (it was dragged, or its window closed).
    pub prefs_changed: bool,
}

/// The overlay, as the main window runs it.
pub struct Overlay {
    id: ViewportId,
    shared: Arc<Mutex<Shared>>,
    rhythm: Rhythm,
    inputs: Inputs,
    /// What the inputs were read from last (see [`Key`]).
    key: Option<Key>,
    view: IslandView,
    keep_minimal: bool,
    /// An alert arrived since the view was built.
    alerted: bool,
    sent: Sent,
    desktop: Desktop,
    /// The game or Elite Nexus is in front (None: the desktop can't say), and when that was asked.
    in_front: Option<bool>,
    asked: Option<f64>,
    /// The game's monitor (Windows), looked up every few seconds.
    monitor: Option<[i32; 4]>,
    monitor_asked: Option<f64>,
    /// Where the window was last put (points), when, and where that is from the canvas's top-left (the island's own
    /// window sits inside the canvas).
    placed: Option<(Pos2, f64, Vec2)>,
    /// See-through off: the island's own window, as the view last asked for it.
    cutout: Option<draw::Cutout>,
    /// The island's clock starts here. egui's input time stands still while eframe runs only `App::logic` (the main
    /// window minimised or covered), and the island's moments and alerts must still run out on time.
    epoch: Instant,
    /// The window renderer was set up for see-through windows at start.
    see_through: bool,
    /// Off in screenshot runs: nothing may pop up over the game.
    allowed: bool,
    /// `--overlay <moment>`: sample content, compact and expanded in turn.
    demo: Option<(IslandView, IslandView)>,
}

/// What the island's inputs are read from: when none of it changed, the model isn't read again.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    events: u64,
    status: u64,
    /// The pinned orders and the planned route, hashed.
    pins: u64,
    route: u64,
    deposits: usize,
    auto_copy: bool,
    high_value: i64,
    /// The clock, while a countdown shows.
    clock: Option<i64>,
}

impl Key {
    fn of(gs: &GameState, src: &Sources<'_>, ticking: bool) -> Key {
        Key {
            events: gs.events_applied,
            status: gs.status_writes,
            pins: hash(src.pins.iter().map(|p| (p.market_id, p.pinned_unix, p.sell_here, p.price, p.qty)).collect::<Vec<_>>()),
            // The stops ahead, where they start and where the route ends (not every stop: a route can have thousands).
            route: hash(src.route.map(|r| (r.remaining().len(), r.waypoints.len(), r.remaining().first().map(|w| w.system.as_str()), r.waypoints.last().map(|w| w.system.as_str())))),
            deposits: src.mining.deposits.len(),
            auto_copy: src.auto_copy,
            high_value: src.high_value,
            clock: ticking.then_some(src.now),
        }
    }
}

fn hash(v: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Where the canvas goes on a monitor (both in points): at its top, centred or against a side.
pub fn place(anchor: Anchor, monitor: Rect, canvas: Vec2) -> Pos2 {
    let x = match anchor {
        Anchor::Centre => monitor.center().x - canvas.x / 2.0,
        Anchor::Left => monitor.left(),
        Anchor::Right => monitor.right() - canvas.x,
    };
    Pos2::new(x.round(), monitor.top().round())
}

impl Overlay {
    /// `see_through`: the renderer was set up for it at start (and nothing ruled it out: [`NoSeeThrough`]). `allowed`:
    /// off in screenshot runs. `demo`: the `--overlay` moment.
    pub fn new(see_through: bool, allowed: bool, demo: Option<(IslandView, IslandView)>) -> Self {
        let see_through = see_through && NoSeeThrough::found().is_none();
        Overlay {
            id: ViewportId::from_hash_of("elite-nexus-overlay"),
            shared: Default::default(),
            rhythm: Rhythm::default(),
            inputs: Inputs::default(),
            key: None,
            view: IslandView::default(),
            keep_minimal: false,
            alerted: false,
            sent: Sent::default(),
            desktop: Desktop::default(),
            in_front: None,
            asked: None,
            monitor: None,
            monitor_asked: None,
            placed: None,
            cutout: None,
            epoch: Instant::now(),
            see_through,
            allowed,
            demo,
        }
    }

    /// The window renderer supports a see-through overlay this run.
    pub fn see_through(&self) -> bool {
        self.see_through
    }

    /// An alert the app raised: the island may open for it.
    pub fn alert(&mut self, a: &Alert) {
        self.rhythm.alert(a);
        self.alerted = true;
    }

    fn on(&self, prefs: &OverlayPrefs) -> bool {
        self.allowed && (prefs.enabled || self.demo.is_some())
    }

    fn look(&self, prefs: &OverlayPrefs) -> Look {
        Look { scale: prefs.scale.clamp(1.0, 2.5), see_through: self.see_through, vr: prefs.vr, anchor: prefs.anchor, locked: prefs.locked }
    }

    /// Once per main-window frame, in `App::logic`. `gs` is the active commander (the caller holds the model's read
    /// lock only for this call); `live`: the journal is followed live and the game is running.
    pub fn logic(&mut self, ctx: &Context, gs: Option<&GameState>, src: &Sources<'_>, prefs: &mut OverlayPrefs, live: bool) -> Requests {
        let mut req = Requests::default();
        if !self.on(prefs) {
            if self.sent.open {
                self.close();
            }
            // Alerts raised while it's off don't wait to be shown when it comes back.
            self.rhythm = Rhythm::default();
            return req;
        }
        let t = self.clock();
        // The overlay's window was closed from outside (Alt+F4 while unlocked): the overlay is off.
        let closed = self.shared.lock().closed;
        if closed || ctx.input(|i| i.raw.viewports.get(&self.id).is_some_and(|v| v.close_requested())) {
            // Closing the `--overlay` demo ends the demo, and leaves the overlay's own setting alone.
            if self.demo.take().is_none() {
                prefs.enabled = false;
                req.prefs_changed = true;
            }
            self.close();
            return req;
        }
        if self.asked.is_none_or(|a| t - a >= 0.5) {
            self.asked = Some(t);
            // No answer (Alt+Tab in progress) keeps the last one.
            if let Some(front) = self.desktop.game_in_front() {
                self.in_front = Some(front);
            }
        }
        if self.monitor_asked.is_none_or(|a| t - a >= 5.0) {
            self.monitor_asked = Some(t);
            self.monitor = self.desktop.game_monitor();
        }
        let look = self.look(prefs);
        if prefs.settle_position(look.cut(), draw::scale_of(ctx, &look)) {
            req.prefs_changed = true;
        }
        let view = match &self.demo {
            // The demo turns from compact to expanded and back every four seconds, to show the morph.
            Some((compact, expanded)) => {
                ctx.request_repaint_after(Duration::from_secs_f64(4.0 - t.rem_euclid(4.0)));
                if (t / 4.0) as i64 % 2 == 0 { compact.clone() } else { expanded.clone() }
            }
            None => self.island(gs, src, prefs.keep_minimal, live, t),
        };
        // Shown only while the game or Elite Nexus is in front (unless kept visible), and while there's something to say.
        let in_front = prefs.always_visible || prefs.vr || self.demo.is_some() || self.in_front != Some(false);
        let gone = view.size == Size::Gone;
        let view = if in_front { view } else { IslandView::default() };
        // See-through off, the window is the island's own: sized for this view before it shows (with nothing to show it
        // keeps its size, and fades its content out). Measured once the window exists, so a frame has set up the fonts.
        if look.cut()
            && self.sent.open
            && let Some(c) = draw::cutout(ctx, &view, &look)
        {
            self.cutout = Some(c);
        }
        // The window follows it now, also when only this runs (the main window minimised or behind the game, so `show`
        // doesn't); `show` gives the viewport's builder the same.
        if self.sent.open {
            let (pos, size, offset) = self.frame(ctx, prefs, &look);
            if self.sent.size != Some(size) {
                ctx.send_viewport_cmd_to(self.id, ViewportCommand::InnerSize(size));
                self.sent.size = Some(size);
            }
            if let Some(pos) = pos {
                self.put(ctx, pos, offset);
            }
        }
        // Its own frames give it the island's shape (Windows), once it has the size the island was cut for.
        let window = if self.sent.open { self.desktop.window(TITLE) } else { None };
        // See-through, nothing of the window itself shows, only the island (Windows); before it's first shown.
        if look.see_through
            && !look.vr
            && let Some(w) = window
        {
            self.desktop.see_through(w);
        }
        let mut s = self.shared.lock();
        s.window = window;
        // Nothing to say: gone once it has faded away, and never shown before there's something to show (an opaque
        // window would otherwise flash black while its first frame fades from nothing to nothing).
        let idle = gone && prefs.locked && (s.faded_out || self.sent.shown_at.is_none());
        // macOS brings a window it shows to the front, taking the keyboard from the game: a see-through overlay stays
        // shown there and simply draws nothing.
        let can_hide = !(cfg!(target_os = "macos") && look.see_through && !look.vr);
        let show = prefs.vr || (in_front && !idle) || !can_hide;
        if s.view != view || s.look != Some(look) {
            s.view = view;
            s.look = Some(look);
            if self.sent.visible == Some(true) {
                ctx.request_repaint_of(self.id);
            }
        }
        let painted = s.painted;
        if self.sent.open && painted == 0 {
            // eframe makes the window at the start of a later frame: it's asked to paint until it has once.
            ctx.request_repaint_of(self.id);
        }
        let clicked = std::mem::take(&mut s.clicked);
        let moment = s.view.moment;
        // Shown once it has painted (hidden) at least once, so it never shows an empty frame.
        if show && painted > 0 && self.sent.visible != Some(true) {
            if self.sent.shown_at.is_none() {
                // The first time: it springs open from the small capsule as it appears.
                s.anim = Anim::default();
            }
            ctx.send_viewport_cmd_to(self.id, ViewportCommand::Visible(true));
            self.sent.visible = Some(true);
            self.sent.shown_at = Some(painted);
            s.hidden = false;
            ctx.request_repaint_of(self.id);
        } else if !show && self.sent.visible == Some(true) {
            ctx.send_viewport_cmd_to(self.id, ViewportCommand::Visible(false));
            self.sent.visible = Some(false);
            s.hidden = true;
        }
        drop(s);
        // Click-through only after it has shown and painted since.
        if self.sent.shown_at.is_some_and(|at| painted > at) && self.sent.passthrough != Some(prefs.locked) {
            ctx.send_viewport_cmd_to(self.id, ViewportCommand::MousePassthrough(prefs.locked));
            self.sent.passthrough = Some(prefs.locked);
        }
        if self.sent.visible.is_some() {
            self.desktop.keep_from_focus(TITLE, prefs.locked);
        }
        if clicked {
            req.open = Some(moment.map_or(Page::Dashboard, |m| m.page()));
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus);
        }
        // Unlocked, where it was dragged to is kept. The drag ends in the window manager, out of egui's sight, so it's
        // seen in where the window is: away from where it was put, a second after it was put there.
        let outer = ctx.input(|i| i.raw.viewports.get(&self.id).and_then(|v| v.outer_rect)).map(|r| r.min);
        if !prefs.locked
            && let (Some((placed, at, offset)), Some(outer)) = (self.placed, outer)
            && t - at >= 1.0
            && placed.distance(outer) > 2.0
        {
            // Kept as the canvas's top-left, whatever size the island's own window has.
            let canvas = outer - offset;
            prefs.position = Some([canvas.x, canvas.y]);
            prefs.position_is_canvas = true;
            req.prefs_changed = true;
        }
        // Asked again twice a second while the game runs: whether it's in front, the island's next change.
        ctx.request_repaint_after(Duration::from_millis(if live { 500 } else { 1000 }));
        if let Some(next) = self.view.next_change {
            ctx.request_repaint_after(Duration::from_secs_f64((next - t).max(0.0)));
        }
        req
    }

    /// The island's view, rebuilt only when what it reads has changed, or when it changes by itself.
    fn island(&mut self, gs: Option<&GameState>, src: &Sources<'_>, keep_minimal: bool, live: bool, t: f64) -> IslandView {
        let fresh = match gs.filter(|_| live) {
            Some(gs) => {
                let key = Key::of(gs, src, self.inputs.carrier.is_some());
                if self.key.as_ref() != Some(&key) {
                    self.key = Some(key);
                    let inputs = Inputs::read(gs, src);
                    let changed = inputs != self.inputs;
                    self.inputs = inputs;
                    changed
                } else {
                    false
                }
            }
            None => {
                self.key = None;
                let changed = self.inputs != Inputs::default();
                self.inputs = Inputs::default();
                changed
            }
        };
        let due = self.view.next_change.is_some_and(|n| t >= n);
        if fresh || due || self.alerted || keep_minimal != self.keep_minimal {
            self.alerted = false;
            self.keep_minimal = keep_minimal;
            self.view = self.rhythm.view(&self.inputs, keep_minimal, t);
        }
        self.view.clone()
    }

    /// Where the window goes (points; None while the monitor isn't known), its size, and where it is from the canvas's
    /// top-left: the canvas at its place, or, see-through off, the island's own window where the island would be in it.
    fn frame(&self, ctx: &Context, prefs: &OverlayPrefs, look: &Look) -> (Option<Pos2>, Vec2, Vec2) {
        let canvas = look.canvas() * draw::scale_of(ctx, look);
        let (size, offset) = match self.cutout.as_ref().filter(|_| look.cut()) {
            Some(c) => (c.rect.size(), c.rect.min.to_vec2()),
            None => (canvas, Vec2::ZERO),
        };
        (self.position(ctx, prefs, canvas).map(|p| p + offset), size, offset)
    }

    /// Moves the window to `pos` (points) when that's somewhere new, and notes when.
    fn put(&mut self, ctx: &Context, pos: Pos2, offset: Vec2) {
        if self.placed.is_none_or(|(p, _, _)| p != pos) {
            ctx.send_viewport_cmd_to(self.id, ViewportCommand::OuterPosition(pos));
            self.placed = Some((pos, self.clock(), offset));
        }
    }

    /// Seconds since the overlay was made: the island's clock.
    fn clock(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    /// The overlay is off: its window closes (eframe drops a viewport no frame shows any more).
    fn close(&mut self) {
        self.sent = Sent::default();
        self.placed = None;
        self.cutout = None;
        self.desktop.forget_window();
        *self.shared.lock() = Shared::default();
    }

    /// Where the window goes: where it was dragged, or at its anchor on the game's monitor (points).
    fn position(&self, ctx: &Context, prefs: &OverlayPrefs, canvas: Vec2) -> Option<Pos2> {
        if let Some([x, y]) = prefs.position {
            return Some(Pos2::new(x, y));
        }
        let info = |id: ViewportId| ctx.input(|i| i.raw.viewports.get(&id).cloned());
        let (own, root) = (info(self.id), info(ViewportId::ROOT));
        let monitor = match self.monitor {
            // Windows: the game's monitor, in pixels, turned into the overlay's points.
            Some([x, y, w, h]) => {
                let native = own.as_ref().and_then(|v| v.native_pixels_per_point).or_else(|| root.as_ref().and_then(|v| v.native_pixels_per_point)).unwrap_or(1.0);
                let ppp = native * ctx.zoom_factor();
                Rect::from_min_size(Pos2::new(x as f32 / ppp, y as f32 / ppp), Vec2::new(w as f32 / ppp, h as f32 / ppp))
            }
            // Elsewhere: the monitor it's on, from its top-left corner.
            None => Rect::from_min_size(Pos2::ZERO, own.and_then(|v| v.monitor_size).or_else(|| root.and_then(|v| v.monitor_size))?),
        };
        Some(place(prefs.anchor, monitor, canvas))
    }

    /// Keeps the overlay's window open, in `App::ui` (every frame of the main window, even minimised).
    pub fn show(&mut self, ctx: &Context, prefs: &OverlayPrefs) {
        if !self.on(prefs) {
            return;
        }
        let look = self.look(prefs);
        // See-through off: the island's own window, where the island would be in the canvas.
        let (pos, size, offset) = self.frame(ctx, prefs, &look);
        let mut builder = ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(look.see_through && !look.vr)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_active(false)
            .with_taskbar(false)
            .with_has_shadow(false)
            // X11: window managers don't give the focus to a notification.
            .with_window_type(X11WindowType::Notification)
            .with_inner_size(size);
        if let Some(pos) = pos {
            builder = builder.with_position(pos);
            if self.placed.is_none_or(|(p, _, _)| p != pos) {
                self.placed = Some((pos, self.clock(), offset));
            }
        }
        if !self.sent.open {
            // Created hidden; it shows once it has painted.
            builder = builder.with_visible(false);
            self.sent.open = true;
            self.sent.size = Some(size);
            ctx.request_repaint_of(self.id);
        }
        let shared = self.shared.clone();
        ctx.show_viewport_deferred(self.id, builder, move |ui, _| {
            let mut guard = shared.lock();
            let s = &mut *guard;
            let Some(look) = s.look else { return };
            let out = draw::island(ui, &s.view, &look, &mut s.anim);
            // The window's shape goes with what was just drawn at its size, in its own pixels; kept once Windows takes it.
            // A window that never had one (see-through) is left as it is.
            if let (Some(window), Some(parts)) = (s.window, &out.shape) {
                let ppp = ui.ctx().pixels_per_point();
                let px = |v: f32| (v * ppp).round() as i32;
                let parts: Vec<[i32; 5]> = parts.iter().map(|(r, radius)| [px(r.left()), px(r.top()), px(r.right()), px(r.bottom()), px(*radius)]).collect();
                let was = s.shaped.as_ref().is_some_and(|(w, p)| *w == window && !p.is_empty());
                let changed = s.shaped.as_ref().is_none_or(|(w, p)| *w != window || *p != parts);
                if changed && (was || !parts.is_empty()) && platform::set_shape(window, &parts) {
                    s.shaped = Some((window, parts));
                }
            }
            s.painted += 1;
            s.faded_out = out.faded_out;
            s.closed |= ui.input(|i| i.viewport().close_requested());
            s.clicked |= out.clicked;
            if s.painted == 1 {
                // Painted while hidden: the main window shows it now.
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            } else if out.animating && !s.hidden {
                ui.ctx().request_repaint();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canvas_goes_to_the_top_of_the_monitor() {
        let monitor = Rect::from_min_size(Pos2::new(1920.0, 0.0), Vec2::new(2560.0, 1440.0));
        let canvas = Vec2::new(440.0, 260.0);
        assert_eq!(place(Anchor::Centre, monitor, canvas), Pos2::new(1920.0 + 1280.0 - 220.0, 0.0));
        assert_eq!(place(Anchor::Left, monitor, canvas), Pos2::new(1920.0, 0.0));
        assert_eq!(place(Anchor::Right, monitor, canvas), Pos2::new(1920.0 + 2560.0 - 440.0, 0.0));
    }

    /// With see-through off, earlier versions kept the opaque panel's top-left where it was dragged to; that place is now
    /// the canvas's, with the open island 16 points in and 12 down. Moved once, the island stays where the panel was.
    #[test]
    fn a_place_dragged_to_by_an_earlier_version_keeps_the_island_there() {
        let mut p: OverlayPrefs = serde_json::from_str(r#"{ "enabled": true, "position": [1000.0, 40.0] }"#).expect("prefs");
        assert!(!p.position_is_canvas, "an earlier version's settings");
        assert!(p.settle_position(true, 1.5));
        assert_eq!(p.position, Some([1000.0 - 24.0, 40.0 - 18.0]), "by the open island's place in the canvas, at its scale");
        assert!(!p.settle_position(true, 1.5), "once");
        let mut q: OverlayPrefs = serde_json::from_str(r#"{ "position": [1000.0, 40.0], "see_through": true }"#).expect("prefs");
        assert!(q.settle_position(false, 1.0));
        assert_eq!(q.position, Some([1000.0, 40.0]), "see-through kept the canvas's place already");
        let mut fresh = OverlayPrefs::default();
        assert!(fresh.settle_position(true, 1.0) && fresh.position.is_none());
    }

    #[test]
    fn settings_read_back_and_default_safely() {
        let p: OverlayPrefs = serde_json::from_str(r#"{ "enabled": true, "anchor": "Right", "scale": 1.5 }"#).expect("prefs");
        assert!(p.enabled && p.locked, "locked unless unlocked");
        assert_eq!((p.anchor, p.scale, p.see_through), (Anchor::Right, 1.5, false));
        assert_eq!(serde_json::from_str::<OverlayPrefs>("{}").expect("empty"), OverlayPrefs::default());
    }
}
