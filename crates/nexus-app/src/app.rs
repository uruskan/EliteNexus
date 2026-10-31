//! The main window: glass sidebar, page header with toolbar capsules, page routing, banners, voice.

use crate::core::{Core, Phase};
use crate::pages::{self, Page};
use crate::settings::Settings;
use crate::theme::{display, pal, size};
use crate::tts::Speaker;
use crate::widgets;
use egui::{Align, Align2, Color32, CornerRadius, Frame, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2};
use egui_phosphor::regular as icon;
use nexus_journal::companion::{GuiFocus, Status};
use nexus_model::context::{Activity, Context};
use nexus_model::{Alert, Priority};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Toast {
    alert: Alert,
    born: Instant,
}

/// `--screenshot <file>`: render once the journals are loaded, save a PNG, exit (for docs/tests).
/// Command-line overrides (testing / special setups).
#[derive(Default)]
pub struct Overrides {
    pub journal_dir: Option<PathBuf>,
    /// Never speak (tests).
    pub mute: bool,
    /// Sub-tab to open on the start page (screenshots), e.g. `sell` for Market.
    pub tab: Option<String>,
    /// `--demo` (screenshot runs only): sample content for pages that need an account, e.g. Ask.
    pub demo: bool,
    /// `--overlay <moment>`: the overlay with sample content (see `overlay::DEMOS`), and in the main window the moment
    /// compact and expanded, for screenshots.
    pub overlay_demo: Option<String>,
    /// The window renderer was set up for a see-through overlay (Settings › Overlay, read at start).
    pub overlay_see_through: bool,
}

pub struct ScreenshotJob {
    pub path: PathBuf,
    pub frames: u32,
    pub requested: bool,
    pub delay: Duration,
    pub started: Instant,
}

impl ScreenshotJob {
    pub fn new(path: PathBuf, delay: Duration) -> Self {
        ScreenshotJob { path, frames: 0, requested: false, delay, started: Instant::now() }
    }
}

fn save_png(path: &std::path::Path, img: &egui::ColorImage) {
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    match image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, bytes) {
        Some(buf) => match buf.save(path) {
            Ok(()) => tracing::info!(path = %path.display(), "screenshot saved"),
            Err(e) => tracing::error!(path = %path.display(), "screenshot save failed: {e}"),
        },
        None => tracing::error!("screenshot buffer size mismatch"),
    }
}

/// What Follow my flight follows: the commander's context (`nexus_model::context`), reduced to the pages here the
/// way Elite Dangerous Exploration Buddy reduces it to its tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flight {
    /// Nothing to follow: not in the game, docked, driving, on foot inside.
    Idle,
    /// The galaxy map, a charging hyperspace jump, hyperspace: Navigation.
    Route,
    /// Flying the system, orbital cruise included: Exploration.
    System,
    /// Under the "Planet view below" altitude over a body, landed, or on foot outside: Exobiology when the body has
    /// life still to analyse, Exploration otherwise.
    Planet,
}

impl Flight {
    /// `altitude_m` is Status.json's altitude, when it gives one; `planet_m` is Settings' "Planet view below".
    fn of(cx: &Context, altitude_m: Option<f64>, planet_m: f64) -> Flight {
        if cx.focus == GuiFocus::GalaxyMap {
            return Flight::Route;
        }
        match cx.activity {
            Activity::NotInGame | Activity::Docked | Activity::Srv(_) => Flight::Idle,
            Activity::ChargingJump | Activity::Hyperspace => Flight::Route,
            Activity::Landed | Activity::OnFootPlanet => Flight::Planet,
            Activity::NearBody | Activity::Glide => if altitude_m.is_some_and(|a| a < planet_m) { Flight::Planet } else { Flight::System },
            Activity::Supercruise | Activity::Space => Flight::System,
        }
    }
}

/// What Follow my flight compares from one Status.json update or journal event to the next.
struct FlightSnapshot {
    /// The Status.json the model held when this was taken, compared by pointer (`Arc::ptr_eq`): the model makes a
    /// new `Arc` for every update, while the timestamp inside has one-second resolution and could miss one.
    status: Option<Arc<Status>>,
    /// `GameState::events_applied` when this was taken.
    events: u64,
    flight: Flight,
    /// The current system is fully explored (Settings "High-value body" threshold).
    done: bool,
}

pub struct NexusApp {
    core: Core,
    settings: Settings,
    /// What the commander should know from the start (settings file, last run, …), above every page until closed.
    notices: Vec<crate::notices::Notice>,
    /// settings.json couldn't be read (or copied aside), so it isn't saved over this run.
    settings_read_only: bool,
    data_dir: PathBuf,
    page: Page,
    toasts: Vec<Toast>,
    speaker: Speaker,
    applied_scale: f32,
    settings_dirty: Option<Instant>,
    explo: pages::exploration::ExplorationUi,
    exo: pages::exobio::ExobioUi,
    fleet: pages::fleet::FleetUi,
    eng: pages::engineering::EngineeringUi,
    log: pages::logbook::LogbookUi,
    map: pages::galaxy_map::MapUi,
    binds: pages::bindings::BindsUi,
    hud: pages::hud::HudUi,
    settings_ui: pages::settings::SettingsUi,
    nav: pages::navigation::NavUi,
    /// Shared async runtime for network services (Spansh, EDDN, uploads).
    rt: tokio::runtime::Runtime,
    spansh: nexus_spansh::Spansh,
    accounts: pages::accounts::AccountsUi,
    /// EDDN / EDSM / Inara / EDAstro uploads (all off until enabled in Accounts).
    sync: crate::sync::SyncService,
    /// Local galaxy/market database + live EDDN feed.
    market: crate::market::MarketService,
    market_ui: pages::market::MarketUi,
    colo: pages::colonisation::ColonisationUi,
    /// The "Ask" assistant and its page state.
    assistant: crate::assistant::AssistantService,
    ask_ui: pages::ask::AskUi,
    /// Routes the assistant planned, moved to the Navigation page each frame.
    ai_routes: std::sync::Arc<parking_lot::Mutex<Option<pages::navigation::PlannedRoute>>>,
    // Command-line overrides: applied at runtime, never written to settings.json.
    overrides: Overrides,
    shot: Option<ScreenshotJob>,
    last_jump_count: Option<usize>,
    /// What the commander was doing at the last Status.json update or journal event (Settings › Follow my flight);
    /// `None` until the first frame with a commander.
    flight: Option<FlightSnapshot>,
    clipboard: Option<arboard::Clipboard>,
    /// The overlay: the island at the top of the game's screen, in a window of its own.
    overlay: crate::overlay::window::Overlay,
    /// Alerts to XSOverlay (Settings › Overlay; off by default).
    xsoverlay: crate::overlay::xsoverlay::XsOverlay,
    /// Looking for a cockpit-light dimmer from 0.3.0 in the game folders (a notice if there is one).
    dimmer_check: Option<crate::jobs::Job<bool>>,
    /// When this frame's work began, and when a slow frame was last logged.
    frame_started: Option<Instant>,
    slow_frame_logged: Option<Instant>,
    /// The commander and their model's `mining.spots_rev` when the Rhino's deposits were last copied into the settings.
    deposits_rev: (String, u64),
}

impl NexusApp {
    pub fn new(cc: &eframe::CreationContext<'_>, data_dir: PathBuf, start_page: Option<Page>, shot: Option<ScreenshotJob>, ov: Overrides) -> Self {
        crate::theme::install(&cc.egui_ctx);
        let (settings, settings_problem) = Settings::load(&data_dir);
        let settings_read_only = settings_problem.as_ref().is_some_and(|p| !p.can_save);
        let mut notices = Vec::new();
        if let Some(p) = settings_problem {
            // While nothing is saved it stays: changes made then would otherwise be lost without a word.
            notices.push(crate::notices::Notice { title: "Your settings file couldn't be read as it was", text: p.what, closable: p.can_save, action: Some(crate::notices::Action::DataFolder) });
        }
        // Test runs (screenshots) leave the marker and the notices of real runs alone.
        if shot.is_none() {
            notices.extend(crate::notices::last_run(&data_dir));
            notices.extend(crate::notices::running_from_zip());
        }
        nexus_hud::install::set_game_dir(settings.game_dir.as_deref());
        let page = start_page.or_else(|| cc.storage.and_then(|s| eframe::get_value(s, "page"))).unwrap_or(Page::Dashboard);
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("nexus-net").enable_all().build().expect("tokio runtime");
        let mut market = crate::market::MarketService::open(&data_dir, &cc.egui_ctx);
        market.set_watches(&settings.price_watches);
        let core = start_core(&cc.egui_ctx, ov.journal_dir.as_ref().or(settings.journal_dir.as_ref()), market.history_sink());
        market.attach(&core);
        market.set_listen(settings.accounts.eddn_listen, rt.handle());
        // Test runs (CLI journal folder, screenshots) never upload: replayed journals must not reach the services.
        let sync = if ov.journal_dir.is_none() && shot.is_none() { crate::sync::SyncService::start(&core, &settings.accounts, &data_dir, rt.handle()) } else { crate::sync::SyncService::disabled() };
        let mut market_ui = pages::market::MarketUi::new(settings.market.clone());
        if let Some(t) = ov.tab.as_deref().and_then(pages::market::Tab::from_name) {
            market_ui.tab = t;
        }
        let map = pages::galaxy_map::MapUi::new(ov.tab.as_deref() == Some("galaxy"));
        let speaker = Speaker::start();
        speaker.set_rate(settings.tts_rate);
        speaker.set_volume(settings.tts_volume);
        // Screenshot runs never open it: nothing may pop up over the game.
        let overlay = crate::overlay::window::Overlay::new(ov.overlay_see_through, shot.is_none(), ov.overlay_demo.as_deref().and_then(crate::overlay::demo));
        if let Some(name) = ov.overlay_demo.as_deref()
            && crate::overlay::demo(name).is_none()
        {
            tracing::warn!("--overlay {name}: no such moment (one of {})", crate::overlay::DEMOS.join(", "));
        }
        NexusApp {
            core,
            settings,
            notices,
            settings_read_only,
            data_dir,
            page,
            // `--demo` screenshots show one banner so its design can be checked.
            toasts: if ov.demo && shot.is_some() {
                vec![Toast {
                    alert: Alert::new(nexus_journal::Timestamp::now(), nexus_model::AlertKind::Exploration, Priority::High, "Earth-like world")
                        .detail("Brestla A 1 · 3.2 M Cr if mapped, first discovery"),
                    born: Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(Instant::now),
                }]
            } else {
                Vec::new()
            },
            speaker,
            applied_scale: 0.0,
            settings_dirty: None,
            explo: pages::exploration::ExplorationUi::new(ov.tab.as_deref(), shot.is_some()),
            exo: pages::exobio::ExobioUi::new(ov.tab.as_deref(), shot.is_some()),
            fleet: Default::default(),
            eng: pages::engineering::EngineeringUi::new(ov.tab.as_deref()),
            log: Default::default(),
            map,
            binds: Default::default(),
            hud: pages::hud::HudUi::new(ov.tab.as_deref()),
            settings_ui: Default::default(),
            nav: pages::navigation::NavUi::new(ov.tab.as_deref()),
            rt,
            spansh: nexus_spansh::Spansh::new(),
            accounts: Default::default(),
            sync,
            market,
            market_ui,
            colo: Default::default(),
            assistant: if ov.demo && shot.is_some() { crate::assistant::AssistantService::demo() } else { Default::default() },
            ask_ui: pages::ask::AskUi::new(ov.demo && shot.is_some()),
            ai_routes: Default::default(),
            overrides: ov,
            shot,
            last_jump_count: None,
            flight: None,
            clipboard: None,
            overlay,
            xsoverlay: Default::default(),
            dimmer_check: Some(crate::jobs::Job::spawn(&cc.egui_ctx, crate::notices::old_dimmer_installed)),
            frame_started: None,
            // The first frames load fonts and textures: nothing is logged in the first 10 seconds.
            slow_frame_logged: Some(Instant::now()),
            deposits_rev: (String::new(), 0),
        }
    }

    /// Keeps the deposits the Rhino refined at: their positions come from Status.json, so only a live run has them.
    fn remember_deposits(&mut self) {
        let model = self.core.model.read();
        let Some(gs) = model.active() else { return };
        if self.deposits_rev.0 == gs.cmdr.fid && self.deposits_rev.1 == gs.mining.spots_rev {
            return;
        }
        self.deposits_rev = (gs.cmdr.fid.clone(), gs.mining.spots_rev);
        if pages::mining::remember(gs, &mut self.settings.mining) {
            self.settings_dirty = Some(Instant::now());
        }
    }

    /// After every jump, put the next route system on the clipboard (paste into the galaxy map).
    fn auto_copy_next(&mut self) {
        let next = {
            let model = self.core.model.read();
            let Some(gs) = model.active() else { return };
            // Arrivals tick off planned waypoints; a carrier route follows its carrier, wherever the commander is.
            if let Some(p) = self.nav.planned.as_mut() {
                let carrier = p.carrier().and_then(|k| gs.carrier_of(k).system.as_deref());
                p.advance(carrier.unwrap_or(&gs.location.system));
            }
            let jumps = gs.history.jumps.len();
            let prev = self.last_jump_count.replace(jumps);
            if !(self.settings.auto_copy_next_system && prev.is_some_and(|p| jumps > p)) {
                return;
            }
            match pages::navigation::active_route(gs, self.nav.planned.as_ref()).first() {
                Some(w) => w.system.clone(),
                None => return,
            }
        };
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        if let Some(cb) = self.clipboard.as_mut() {
            match cb.set_text(next.clone()) {
                Ok(()) => self.toasts.push(Toast {
                    alert: Alert::new(nexus_journal::Timestamp::now(), nexus_model::AlertKind::Navigation, Priority::Low, format!("Next: {next}")).detail("Copied to clipboard"),
                    born: Instant::now(),
                }),
                Err(e) => tracing::warn!("clipboard: {e}"),
            }
        }
    }

    /// Follow my flight: read what the commander is doing (`Context::of`, from Status.json while the game runs) and,
    /// when it changes, open the page that fits: Navigation in the galaxy map, while a hyperspace jump charges and in
    /// hyperspace; Exploration while flying the system; Exobiology under the "Planet view below" altitude, landed or
    /// on foot outside, when the body has life still to analyse (Exploration otherwise); Navigation too once the
    /// system is done. Only moves between those three pages. Runs only when a Status.json update or a journal event
    /// arrived: on every other frame it costs the model read lock, one counter comparison and one pointer comparison,
    /// no allocation. The snapshot is kept even with the setting off, so switching it on never acts on an old
    /// transition, and the history read at start never switches.
    fn follow_flight(&mut self) {
        let target = {
            let model = self.core.model.read();
            let Some(gs) = model.active() else { return };
            let same_status = |p: &FlightSnapshot| match (&p.status, &gs.status) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if self.flight.as_ref().is_some_and(|p| p.events == gs.events_applied && same_status(p)) {
                return;
            }
            let cx = Context::of(gs);
            let flight = Flight::of(&cx, gs.status.as_ref().and_then(|s| s.altitude), f64::from(self.settings.auto_page_altitude_km) * 1000.0);
            let system = gs.exploration.current_system();
            let done = system.is_some_and(|s| s.fully_explored(self.settings.high_value_threshold));
            let prev = self.flight.replace(FlightSnapshot { status: gs.status.clone(), events: gs.events_applied, flight, done });
            let Some(prev) = prev else { return };
            if flight != prev.flight {
                match flight {
                    Flight::Route => Some(Page::Navigation),
                    Flight::System => Some(Page::Exploration),
                    Flight::Planet => {
                        // The body by its id when the journal named it, else by Status.json's name.
                        let body = cx.body.as_ref().zip(system).and_then(|(b, s)| b.id.and_then(|id| s.bodies.get(&id)).or_else(|| s.bodies.values().find(|x| x.name == b.name)));
                        Some(if body.is_some_and(|b| b.bio_signals > 0 && !b.bio_done()) { Page::Exobiology } else { Page::Exploration })
                    }
                    Flight::Idle => None,
                }
            } else if done && !prev.done {
                Some(Page::Navigation)
            } else {
                None
            }
        };
        let Some(target) = target else { return };
        let following = matches!(self.page, Page::Navigation | Page::Exploration | Page::Exobiology);
        if self.settings.auto_page && following && self.page != target && self.shot.is_none() && matches!(self.core.status.read().phase, Phase::Live) {
            self.page = target;
        }
    }

    fn screenshot_tick(&mut self, ctx: &egui::Context) {
        let Some(job) = self.shot.as_mut() else { return };
        job.frames += 1;
        // Once the journals are read, or couldn't be (to check the journal problem screen).
        let ready = matches!(self.core.status.read().phase, Phase::Live | Phase::Error(_));
        if ready && !job.requested && job.frames > 40 && job.started.elapsed() >= job.delay {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            job.requested = true;
        }
        let img = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = img {
            save_png(&job.path, &img);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }

    fn drain_alerts(&mut self) {
        // Journal alerts first, then price alerts from the market watcher.
        let mut incoming: Vec<Alert> = self.core.alerts.try_iter().collect();
        incoming.extend(self.market.alerts.try_iter());
        for a in incoming {
            let prio = match a.priority {
                Priority::Low => 0,
                Priority::Normal => 1,
                Priority::High => 2,
                Priority::Critical => 3,
            };
            if self.settings.tts_enabled && !self.overrides.mute && prio >= self.settings.tts_min_priority
                && let Some(s) = &a.speech {
                    self.speaker.say(s.clone());
                }
            self.overlay.alert(&a);
            if self.settings.overlay.xsoverlay && self.shot.is_none() {
                self.xsoverlay.send(&a);
            }
            let dup = self.toasts.iter().rev().take(4).any(|t| t.alert.title == a.title && t.alert.detail == a.detail && t.born.elapsed() < Duration::from_secs(20));
            if !dup {
                self.toasts.push(Toast { alert: a, born: Instant::now() });
            }
            if self.toasts.len() > 5 {
                self.toasts.remove(0);
            }
        }
    }

    /// The floating glass sidebar. `wide` shows labels; narrow windows get an icon rail.
    fn sidebar(&mut self, ui: &mut Ui, wide: bool) {
        let panel = ui.max_rect();
        widgets::glass(ui.painter(), panel, 18, pal::SIDEBAR);
        let pad = if wide { 12.0 } else { 10.0 };
        let inner = panel.shrink2(Vec2::new(pad, 14.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
        let ui = &mut ui;
        // App identity.
        ui.horizontal(|ui| {
            if !wide {
                ui.add_space(((inner.width() - 32.0) / 2.0).max(0.0));
            }
            app_tile(ui, 32.0);
            if wide {
                ui.add_space(2.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new("Elite Nexus").font(display(size::HEADLINE)).color(pal::TEXT));
                    ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).size(size::CAPTION).color(pal::DIM));
                });
            }
        });
        ui.add_space(14.0);
        // The commander card sits at the bottom; the list scrolls in between.
        let footer_h = if wide { 70.0 } else { 52.0 };
        let list_h = (ui.available_height() - footer_h - 8.0).max(80.0);
        egui::ScrollArea::vertical().id_salt("nav").max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for (i, (group, items)) in Page::GROUPS.iter().enumerate() {
                if i > 0 {
                    ui.add_space(if wide { 6.0 } else { 8.0 });
                }
                if wide {
                    // Painted, not a `horizontal` row (rows are at least a button tall).
                    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
                    ui.painter().text(r.left_center() + Vec2::new(10.0, 1.0), Align2::LEFT_CENTER, *group, display(size::CAPTION), pal::FAINT);
                } else if i > 0 {
                    let r = ui.available_rect_before_wrap();
                    ui.painter().hline(r.x_range().shrink(12.0), r.top() - 4.0, Stroke::new(1.0, pal::SEPARATOR));
                }
                for p in *items {
                    if nav_item(ui, *p, self.page == *p, wide) {
                        self.page = *p;
                    }
                }
            }
        });
        let model = self.core.model.read();
        if let Some(gs) = model.active() {
            let rest = ui.available_rect_before_wrap();
            let card = Rect::from_min_max(egui::pos2(rest.left(), rest.bottom() - footer_h), rest.max);
            let mut f = ui.new_child(egui::UiBuilder::new().max_rect(card).layout(Layout::top_down(Align::Min)));
            if commander_card(&mut f, gs, wide) {
                self.page = Page::Dashboard;
            }
        }
    }

    /// Large title on the left; live status, credits and the HUD button as toolbar capsules on the right.
    fn page_header(&mut self, ui: &mut Ui, title: &str, subtitle: &str) {
        let status = self.core.status.read().clone();
        let narrow = ui.available_width() < 860.0;
        // Toolbar items, left to right: credits, live feed, uploads, EDDN pause, status.
        let mut items: Vec<CapItem> = Vec::new();
        {
            let model = self.core.model.read();
            let gs = model.active();
            if let Some(gs) = gs {
                let cr = if narrow { widgets::short_credits(gs.cmdr.credits) } else { widgets::credits(gs.cmdr.credits) };
                items.push(CapItem::new(cr, pal::GOLD, true).tip("Credit balance"));
            }
            if let Some((connected, per_min)) = self.market.feed_state() {
                let text = match (connected, narrow) {
                    (true, false) => format!("{} EDDN {per_min}/min", icon::BROADCAST),
                    (true, true) => format!("{} {per_min}/min", icon::BROADCAST),
                    (false, _) => format!("{} EDDN…", icon::BROADCAST),
                };
                items.push(CapItem::new(text, if connected { pal::DIM } else { pal::WARN }, false).action(CapAction::MarketDb).tip("Live market feed from EDDN. Click for the market database."));
            }
            if self.sync.any_enabled() {
                let st = self.sync.status();
                let all = [&st.eddn, &st.edsm, &st.inara, &st.edastro];
                let sent: u64 = all.iter().map(|x| x.sent).sum();
                let queued: usize = all.iter().map(|x| x.queued).sum();
                let trouble = all.iter().any(|x| x.paused.is_some() || (x.last_error.is_some() && x.last_success.is_none()));
                let text = if queued > 0 { format!("{} {sent} · {queued} queued", icon::CLOUD_ARROW_UP) } else { format!("{} {sent}", icon::CLOUD_ARROW_UP) };
                items.push(CapItem::new(text, if trouble { pal::WARN } else { pal::DIM }, false).action(CapAction::Accounts).tip("Uploads sent this session (EDDN / EDSM / Inara / EDAstro). Click for details."));
            }
            if self.settings.accounts.eddn_upload {
                let paused = self.settings.accounts.eddn_paused;
                let (text, col, tip) = if paused {
                    (format!("{} Resume EDDN", icon::PLAY), pal::WARN, "Sharing with EDDN is paused. Click to share again.")
                } else {
                    (format!("{} Pause EDDN", icon::PAUSE), pal::TEXT, "Stop sharing with EDDN, e.g. to keep a market you just found to yourself (prices are only sent when you undock).")
                };
                items.push(CapItem::new(text, col, false).action(CapAction::TogglePause).tip(tip));
            }
            let (label, col) = match &status.phase {
                Phase::Live => {
                    let fresh = status.last_live.is_some_and(|t| t.elapsed() < Duration::from_secs(120));
                    let playing = gs.is_some_and(|g| !g.game_closed);
                    if fresh || playing { ("Live", pal::GOOD) } else { ("Offline", pal::GRAY) }
                }
                Phase::Loading { .. } | Phase::Starting => ("Loading", pal::WARN),
                Phase::Error(_) => ("Error", pal::BAD),
            };
            let tip = if label == "Live" { "The game is running; the journal is followed live." } else { "Waiting for the game." };
            items.push(CapItem::new(label.to_string(), col, true).dot(col).tip(tip));
        }
        let mut action = None;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new(title).font(crate::theme::bold(size::LARGE_TITLE)).color(pal::TEXT));
                if !subtitle.is_empty() {
                    ui.add(egui::Label::new(RichText::new(subtitle).size(size::CALLOUT).color(pal::DIM)).truncate());
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.add_space(2.0);
                let ov = &mut self.settings.overlay;
                // Wayland doesn't let a window keep itself above the others; XWayland does.
                let wayland = cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
                let tip = match (ov.enabled, wayland) {
                    (true, _) => "Overlay on: the island at the top of the game's screen. Click to switch it off.",
                    (false, true) => {
                        "Overlay: a small island at the top of the game's screen that says what's going on. Wayland doesn't let apps stay on top: to keep it above the game, start Elite Nexus as WAYLAND_DISPLAY= ./EliteNexus (it then runs through XWayland)."
                    }
                    (false, false) => "Overlay: a small island at the top of the game's screen that says what's going on",
                };
                if toolbar_button(ui, icon::PICTURE_IN_PICTURE, tip, ov.enabled).clicked() {
                    ov.enabled = !ov.enabled;
                    self.settings_dirty = Some(Instant::now());
                }
                if ov.enabled {
                    ui.add_space(6.0);
                    let (glyph, tip) = if ov.locked {
                        (icon::LOCK_SIMPLE, "Locked: clicks go to the game. Click to unlock the overlay, then drag it where you want it.")
                    } else {
                        (icon::LOCK_SIMPLE_OPEN, "Unlocked: drag the overlay to move it, or click it to open its page here. Click to lock it again.")
                    };
                    if toolbar_button(ui, glyph, tip, !ov.locked).clicked() {
                        ov.locked = !ov.locked;
                        self.settings_dirty = Some(Instant::now());
                    }
                }
                ui.add_space(8.0);
                action = toolbar_capsule(ui, &items);
            });
        });
        ui.add_space(14.0);
        match action {
            Some(CapAction::MarketDb) => {
                self.page = Page::Market;
                self.market_ui.tab = pages::market::Tab::Database;
            }
            Some(CapAction::Accounts) => self.page = Page::Accounts,
            Some(CapAction::TogglePause) => {
                self.settings.accounts.eddn_paused = !self.settings.accounts.eddn_paused;
                self.sync.apply(&self.settings.accounts);
                self.settings_dirty = Some(Instant::now());
            }
            None => {}
        }
    }

    /// Writes settings.json, unless it couldn't be read at start (then the file stays as the commander left it).
    fn save_settings(&self) {
        if !self.settings_read_only {
            self.settings.save(&self.data_dir);
        }
    }

    /// The notices, above every page until closed.
    fn notices(&mut self, ui: &mut Ui) {
        if let Some(found) = self.dimmer_check.as_mut().and_then(crate::jobs::Job::poll) {
            self.dimmer_check = None;
            if found {
                self.notices.push(crate::notices::old_dimmer());
            }
        }
        if crate::notices::show(ui, &mut self.notices, &self.data_dir) == Some(crate::notices::Action::CockpitLights) {
            self.hud.show_cockpit();
            self.page = Page::Hud;
        }
    }

    fn content(&mut self, ui: &mut Ui) {
        self.notices(ui);
        let status = self.core.status.read().clone();
        match &status.phase {
            Phase::Loading { done, total } => {
                ui.add_space(80.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Reading your journals…").font(display(22.0)));
                    ui.add_space(8.0);
                    ui.scope(|ui| {
                        ui.set_max_width(400.0);
                        widgets::bar(ui, *done as f32 / (*total).max(1) as f32, pal::ACCENT, 10.0, None);
                    });
                    ui.label(RichText::new(format!("{done} / {total} files")).color(pal::DIM));
                });
                return;
            }
            Phase::Error(e) => {
                let hint = format!("{e} If the game runs on another computer, share its journal folder and pick it here.");
                self.before_commander(ui, &status, icon::WARNING, "Journal problem", &hint);
                return;
            }
            Phase::Starting => return,
            Phase::Live => {}
        }
        let model = self.core.model.read();
        let Some(gs) = model.active() else {
            drop(model);
            let hint = "Start Elite Dangerous: Elite Nexus picks everything up live. If the game runs on another computer, share its journal folder and pick it here.";
            self.before_commander(ui, &status, icon::USER, "No commander found", hint);
            return;
        };
        let (title, subtitle) = page_heading(self.page, gs);
        drop(model);
        self.page_header(ui, &title, &subtitle);
        let model = self.core.model.read();
        let Some(gs) = model.active() else { return };
        let scroll = !matches!(self.page, Page::Ask | Page::Exploration | Page::Logbook | Page::GalaxyMap | Page::Fleet | Page::Navigation | Page::Bindings | Page::Market);
        let page = self.page;
        let high_value = self.settings.high_value_threshold;
        let mut settings_changed = false;
        let mut test_voice = false;
        let mut restart = false;
        let mut map_action = None;
        let mut follow = None;
        let route = if page == Page::GalaxyMap { pages::navigation::active_route(gs, self.nav.planned.as_ref()) } else { Vec::new() };
        let mut body = |ui: &mut Ui| match page {
            Page::Dashboard => pages::dashboard::show(ui, gs, &self.settings.colonisation.hidden),
            Page::Exploration => follow = pages::exploration::show(ui, gs, &mut self.explo, high_value, &self.spansh, self.rt.handle()),
            Page::Exobiology => match pages::exobio::show(ui, gs, &mut self.exo, &self.spansh, self.rt.handle(), self.market.db.as_ref()) {
                Some(pages::exobio::ExoAction::PlotTo(s)) => map_action = Some(pages::galaxy_map::MapAction::PlotTo(s)),
                Some(pages::exobio::ExoAction::Follow(r)) => follow = Some(r),
                None => {}
            },
            Page::GalaxyMap => map_action = pages::galaxy_map::show(ui, gs, &mut self.map, &route),
            Page::Mining => {
                let (changed, action) = pages::mining::show(ui, gs, &mut self.settings.mining);
                settings_changed |= changed;
                if action.is_some() {
                    map_action = action;
                }
            }
            Page::Colonisation => settings_changed |= pages::colonisation::show(ui, gs, &self.market, &mut self.colo, &mut self.settings.colonisation),
            Page::Ask => match pages::ask::show(ui, &mut self.assistant, &mut self.ask_ui, &mut self.settings.ai) {
                pages::ask::AskAction::Ask(question) => {
                    let tools = crate::ai_tools::NexusTools {
                        model: self.core.model.clone(),
                        db: self.market.db.clone(),
                        index: self.core.index.clone(),
                        spansh: self.spansh.clone(),
                        rt: self.rt.handle().clone(),
                        routes: self.ai_routes.clone(),
                    };
                    let context = crate::ai_tools::context_line(gs);
                    self.assistant.ask(question, self.settings.ai.provider(), tools, context, ui.ctx());
                }
                pages::ask::AskAction::SettingsChanged => settings_changed = true,
                pages::ask::AskAction::None => {}
            },
            Page::Carrier => settings_changed |= pages::carrier::show(ui, gs, &mut self.settings.carrier),
            Page::Engineering => settings_changed |= pages::engineering::show(ui, gs, &mut self.eng, &mut self.settings.wishlist),
            Page::Fleet => pages::fleet::show(ui, gs, &mut self.fleet),
            Page::Missions => pages::missions::show(ui, gs),
            Page::Bgs => settings_changed |= pages::bgs::show(ui, gs, &mut self.settings.bgs),
            Page::Logbook => pages::logbook::show(ui, gs, &mut self.log, &self.core.index),
            Page::Statistics => pages::stats::show(ui, gs),
            Page::Settings => {
                let mut cx = pages::settings::SettingsCtx {
                    settings: &mut self.settings,
                    ui: &mut self.settings_ui,
                    status: &status,
                    data_dir: &self.data_dir,
                    changed: false,
                    test_voice: false,
                    restart_core: false,
                    voice: self.speaker.status(),
                    overlay_see_through: self.overlay.see_through(),
                };
                pages::settings::show(ui, &mut cx);
                settings_changed = cx.changed;
                test_voice = cx.test_voice;
                restart = cx.restart_core;
            }
            Page::Navigation => {
                settings_changed |= pages::navigation::show(ui, gs, &mut self.nav, &mut self.settings.auto_copy_next_system, &mut self.settings.carrier, &self.spansh, self.rt.handle())
            }
            Page::Bindings => settings_changed |= pages::bindings::show(ui, &mut self.binds, &self.data_dir, &mut self.settings.game_dir),
            Page::Hud => {
                let s = &mut self.settings;
                settings_changed |= pages::hud::show(ui, &mut self.hud, &mut s.hud_themes, &mut s.game_dir, !gs.game_closed, (&gs.cmdr.game_version, &gs.cmdr.game_build), self.rt.handle());
            }
            Page::Accounts => {
                let name = gs.cmdr.name.clone();
                let status = pages::accounts::StatusView { eddn_feed: self.market.feed_summary().map(|(_, line)| line), uplink: self.sync.status() };
                settings_changed |= pages::accounts::show(ui, &mut self.settings.accounts, &mut self.accounts, &name, &status);
                if std::mem::take(&mut self.accounts.check_inara) {
                    self.sync.check_inara();
                }
                if self.sync.any_enabled() {
                    ui.ctx().request_repaint_after(Duration::from_millis(500));
                }
            }
            Page::Market => {
                settings_changed |= pages::market::show(ui, gs, &mut self.market, &mut self.market_ui, self.rt.handle(), &mut self.settings.price_watches);
                // A planned trade run becomes the active route.
                if let Some(r) = self.market_ui.follow.take() {
                    follow = Some(r);
                }
                // Searches, filters and pins are kept for the next run.
                if self.market_ui.prefs != self.settings.market {
                    self.settings.market.clone_from(&self.market_ui.prefs);
                    self.settings_dirty = Some(Instant::now());
                }
                // Feed counters and headlines move every second or so.
                ui.ctx().request_repaint_after(Duration::from_millis(1000));
            }
        };
        if scroll {
            egui::ScrollArea::vertical().id_salt(("page", page as u8)).auto_shrink([false, false]).show(ui, |ui| body(ui));
        } else {
            body(ui);
        }
        drop(model);
        if let Some(pages::galaxy_map::MapAction::PlotTo(system)) = map_action {
            self.nav.to = system;
            self.nav.planner = pages::navigation::Planner::Exact;
            self.page = Page::Navigation;
        }
        // A tour picked on Exploration or Exobiology becomes the active route.
        if let Some(route) = follow {
            self.nav.planned = Some(route);
            self.nav.planner = pages::navigation::Planner::InGame;
            self.page = Page::Navigation;
        }
        match self.nav.goto.take() {
            Some(pages::navigation::Goto::Exploration) => {
                self.explo.tab = pages::exploration::Tab::Find;
                self.page = Page::Exploration;
            }
            Some(pages::navigation::Goto::Exobiology) => {
                self.exo.tab = pages::exobio::Tab::Find;
                self.page = Page::Exobiology;
            }
            None => {}
        }
        self.after_settings(ui.ctx(), settings_changed, test_voice, restart);
    }

    /// Before a commander is loaded: the journal folder can't be read, or holds no commander yet. Settings opens as
    /// usual; every other page shows the problem with the journal folder setting right under it (a laptop reading
    /// another PC's journals has no Saved Games folder of its own to fall back on).
    fn before_commander(&mut self, ui: &mut Ui, status: &crate::core::CoreStatus, icon: &str, title: &str, hint: &str) {
        let settings_page = self.page == Page::Settings;
        if settings_page {
            self.page_header(ui, "Settings", "");
        }
        let mut cx = pages::settings::SettingsCtx {
            settings: &mut self.settings,
            ui: &mut self.settings_ui,
            status,
            data_dir: &self.data_dir,
            changed: false,
            test_voice: false,
            restart_core: false,
            voice: self.speaker.status(),
            overlay_see_through: self.overlay.see_through(),
        };
        // Scrolls: with the settings banner above it, a short window would otherwise cut off the folder's buttons.
        egui::ScrollArea::vertical().id_salt(("page", Page::Settings as u8, settings_page)).auto_shrink([false, false]).show(ui, |ui| {
            if settings_page {
                pages::settings::show(ui, &mut cx);
            } else {
                widgets::empty(ui, icon, title, hint);
                ui.vertical_centered(|ui| {
                    ui.set_max_width(640.0);
                    widgets::card(ui, "Journal folder", |ui| pages::settings::journal_setup(ui, &mut cx));
                });
            }
        });
        let (changed, test_voice, restart) = (cx.changed, cx.test_voice, cx.restart_core);
        self.after_settings(ui.ctx(), changed, test_voice, restart);
    }

    /// Applies what the Settings page changed: services, voice, and reading the journals again from a new folder.
    fn after_settings(&mut self, ctx: &egui::Context, changed: bool, test_voice: bool, restart: bool) {
        if changed {
            self.settings_dirty = Some(Instant::now());
            self.speaker.set_rate(self.settings.tts_rate);
            self.speaker.set_volume(self.settings.tts_volume);
            self.sync.apply(&self.settings.accounts);
            self.market.set_watches(&self.settings.price_watches);
            self.market.set_listen(self.settings.accounts.eddn_listen, self.rt.handle());
        }
        if test_voice {
            self.speaker.say("Elite Nexus voice check. All systems nominal, commander.");
        }
        if restart {
            self.core = start_core(ctx, self.overrides.journal_dir.as_ref().or(self.settings.journal_dir.as_ref()), self.market.history_sink());
            self.sync.attach(&self.core);
            self.market.attach(&self.core);
        }
    }

    /// Alerts as macOS-style notification banners, top right, newest first.
    fn toasts(&mut self, ctx: &egui::Context) {
        let life = Duration::from_secs_f32(self.settings.toast_seconds.max(1.0));
        self.toasts.retain(|t| t.born.elapsed() < life);
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(Align2::RIGHT_TOP, [-18.0, 74.0])
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_width(360.0);
                for t in self.toasts.iter().rev() {
                    let age = t.born.elapsed().as_secs_f32();
                    // Slide in from the right, fade out at the end.
                    let fade = ((life.as_secs_f32() - age) / 0.5).clamp(0.0, 1.0);
                    let enter = (age / 0.25).clamp(0.0, 1.0);
                    let slide = (1.0 - enter).powi(3) * 40.0;
                    let tint = match t.alert.priority {
                        Priority::Critical => pal::BAD,
                        Priority::High => pal::ACCENT,
                        Priority::Normal => pal::BLUE,
                        Priority::Low => pal::GRAY,
                    };
                    let top = ui.cursor().min;
                    let width = 360.0;
                    // Measure first so the glass is drawn under the text.
                    let title = ui.painter().layout(t.alert.title.clone(), display(size::BODY), pal::TEXT.gamma_multiply(fade), width - 76.0);
                    let detail = (!t.alert.detail.is_empty()).then(|| ui.painter().layout(t.alert.detail.clone(), egui::FontId::proportional(size::FOOTNOTE), pal::DIM.gamma_multiply(fade), width - 76.0));
                    let h = 14.0 + 18.0 + title.size().y + detail.as_ref().map_or(0.0, |d| d.size().y + 2.0) + 14.0;
                    let rect = Rect::from_min_size(top + Vec2::new(slide, 0.0), Vec2::new(width, h));
                    let p = ui.painter();
                    p.add(egui::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha((120.0 * fade) as u8) }.as_shape(rect, CornerRadius::same(18)));
                    p.rect_filled(rect, CornerRadius::same(18), Color32::from_rgb(52, 52, 56).gamma_multiply(fade));
                    p.rect_stroke(rect, CornerRadius::same(18), Stroke::new(1.0, Color32::from_white_alpha((20.0 * fade) as u8)), egui::StrokeKind::Inside);
                    // App icon, kind and time, then the message.
                    let icon_rect = Rect::from_min_size(rect.min + Vec2::new(14.0, 14.0), Vec2::splat(34.0));
                    paint_app_tile(p, icon_rect, fade);
                    let x = icon_rect.right() + 12.0;
                    p.text(egui::pos2(x, rect.top() + 14.0), Align2::LEFT_TOP, t.alert.kind.label().to_uppercase(), display(size::CAPTION), tint.gamma_multiply(fade));
                    p.text(egui::pos2(rect.right() - 14.0, rect.top() + 14.0), Align2::RIGHT_TOP, "now", egui::FontId::proportional(size::CAPTION), pal::DIM.gamma_multiply(fade));
                    let mut y = rect.top() + 14.0 + 18.0;
                    let th = title.size().y;
                    p.galley(egui::pos2(x, y), title, pal::TEXT);
                    y += th + 2.0;
                    if let Some(d) = detail {
                        p.galley(egui::pos2(x, y), d, pal::DIM);
                    }
                    ui.allocate_space(Vec2::new(width, h));
                    ui.add_space(10.0);
                }
            });
    }
}

fn start_core(ctx: &egui::Context, journal_dir: Option<&PathBuf>, history: Option<crate::core::HistorySink>) -> Core {
    let repaint_ctx = ctx.clone();
    Core::start(journal_dir.cloned(), move || repaint_ctx.request_repaint(), history)
}

#[derive(Clone, Copy)]
enum CapAction {
    MarketDb,
    Accounts,
    TogglePause,
}

/// One item in the toolbar capsule.
struct CapItem {
    text: String,
    color: Color32,
    strong: bool,
    dot: Option<Color32>,
    tip: &'static str,
    action: Option<CapAction>,
}

impl CapItem {
    fn new(text: String, color: Color32, strong: bool) -> Self {
        CapItem { text, color, strong, dot: None, tip: "", action: None }
    }
    fn dot(mut self, c: Color32) -> Self {
        self.dot = Some(c);
        self
    }
    fn tip(mut self, t: &'static str) -> Self {
        self.tip = t;
        self
    }
    fn action(mut self, a: CapAction) -> Self {
        self.action = Some(a);
        self
    }
}

/// A toolbar capsule (macOS 26 glass) sized to its items, with hairline dividers between them.
/// Returns the action of the item clicked.
fn toolbar_capsule(ui: &mut Ui, items: &[CapItem]) -> Option<CapAction> {
    const PAD: f32 = 16.0;
    const GAP: f32 = 12.0;
    const H: f32 = 34.0;
    let galleys: Vec<_> = items
        .iter()
        .map(|it| {
            let font = if it.strong { display(size::FOOTNOTE) } else { egui::FontId::proportional(size::FOOTNOTE) };
            ui.painter().layout_no_wrap(it.text.clone(), font, it.color)
        })
        .collect();
    let widths: Vec<f32> = items.iter().zip(&galleys).map(|(it, g)| g.size().x + if it.dot.is_some() { 14.0 } else { 0.0 }).collect();
    let n = items.len();
    let total = 2.0 * PAD + widths.iter().sum::<f32>() + (n.saturating_sub(1)) as f32 * (2.0 * GAP + 1.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(total, H), Sense::hover());
    let p = ui.painter().clone();
    p.add(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(rect, CornerRadius::same(17)));
    p.rect_filled(rect, CornerRadius::same(17), pal::CARD);
    p.rect_stroke(rect, CornerRadius::same(17), Stroke::new(1.0, Color32::from_white_alpha(16)), egui::StrokeKind::Inside);
    let mut clicked = None;
    let mut x = rect.left() + PAD;
    for (i, (it, g)) in items.iter().zip(galleys).enumerate() {
        if i > 0 {
            p.vline(x + GAP, (rect.center().y - 8.0)..=(rect.center().y + 8.0), Stroke::new(1.0, Color32::from_white_alpha(26)));
            x += 2.0 * GAP + 1.0;
        }
        let w = widths[i];
        let hit = Rect::from_min_size(egui::pos2(x - GAP / 2.0, rect.top()), Vec2::new(w + GAP, H));
        let r = ui.interact(hit, resp.id.with(i), if it.action.is_some() { Sense::click() } else { Sense::hover() });
        if it.action.is_some() && r.hovered() {
            p.rect_filled(hit.shrink2(Vec2::new(0.0, 4.0)), CornerRadius::same(8), Color32::from_white_alpha(12));
        }
        let mut tx = x;
        if let Some(d) = it.dot {
            p.circle_filled(egui::pos2(x + 4.0, rect.center().y), 4.0, d);
            tx += 14.0;
        }
        p.galley(egui::pos2(tx, rect.center().y - g.size().y / 2.0), g, it.color);
        let r = if it.tip.is_empty() { r } else { r.on_hover_text(it.tip) };
        if r.clicked() {
            clicked = it.action;
        }
        x += w;
    }
    clicked
}

/// A round glass toolbar button; its glyph is orange while what it switches is `on`.
fn toolbar_button(ui: &mut Ui, glyph: &str, tip: &str, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
    let p = ui.painter();
    p.add(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(rect, CornerRadius::same(17)));
    let fill = if resp.hovered() { pal::CARD_HI } else { pal::CARD };
    p.rect_filled(rect, CornerRadius::same(17), fill);
    p.rect_stroke(rect, CornerRadius::same(17), Stroke::new(1.0, Color32::from_white_alpha(16)), egui::StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, glyph, egui::FontId::proportional(17.0), if on { pal::ACCENT } else { pal::TEXT });
    resp.on_hover_text(tip)
}

/// The app icon: Elite orange tile with the diamond, as in the Dock.
fn paint_app_tile(p: &egui::Painter, rect: Rect, fade: f32) {
    let r = CornerRadius::same((rect.width() * 0.26) as u8);
    p.rect_filled(rect, r, pal::ACCENT.gamma_multiply(fade));
    // A lighter top half gives the tile some depth.
    let top = Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.center().y));
    p.with_clip_rect(top).rect_filled(rect, r, Color32::from_white_alpha((28.0 * fade) as u8));
    p.text(rect.center(), Align2::CENTER_CENTER, icon::DIAMOND, egui::FontId::proportional(rect.width() * 0.58), pal::ON_ACCENT.gamma_multiply(fade));
}

fn app_tile(ui: &mut Ui, side: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    paint_app_tile(ui.painter(), rect, 1.0);
}

/// The commander at the foot of the sidebar: avatar, name, ship, where, fuel. Returns true if clicked.
fn commander_card(ui: &mut Ui, gs: &nexus_model::GameState, wide: bool) -> bool {
    let rect = ui.max_rect();
    let resp = ui.interact(rect, ui.id().with("cmdr-card"), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(12), Color32::from_white_alpha(8));
    }
    let initials: String = gs.cmdr.name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase();
    let avatar = |ui: &mut Ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 18.0, Color32::from_rgb(99, 99, 102));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, &initials, display(size::CALLOUT), pal::TEXT);
    };
    let inner = rect.shrink2(Vec2::new(8.0, 8.0));
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
    if !wide {
        ui.horizontal(|ui| {
            ui.add_space(((inner.width() - 36.0) / 2.0).max(0.0));
            avatar(ui);
        });
        let ship = gs.fleet.current().map(|s| s.type_name.clone()).unwrap_or_default();
        resp.on_hover_text(format!("CMDR {}\n{}\n{}", gs.cmdr.name, ship, gs.location.system));
        return false;
    }
    let ship = gs.fleet.current().map(|s| if s.name.trim().is_empty() { s.type_name.clone() } else { format!("{} · {}", s.name, s.type_name) }).unwrap_or_default();
    ui.horizontal(|ui| {
        avatar(ui);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add(egui::Label::new(RichText::new(format!("CMDR {}", gs.cmdr.name)).font(display(size::CALLOUT)).color(pal::TEXT)).truncate());
            ui.add(egui::Label::new(RichText::new(format!("{} {}", icon::MAP_PIN, gs.location.system)).size(size::CAPTION).color(pal::DIM)).truncate());
        });
    });
    if let Some(s) = gs.fleet.current().filter(|s| s.fuel_main_capacity > 0.0) {
        let f = (gs.fleet.fuel_main / s.fuel_main_capacity) as f32;
        let col = if f < 0.25 { pal::BAD } else if f < 0.5 { pal::WARN } else { pal::CYAN };
        ui.add_space(4.0);
        widgets::bar(&mut ui, f, col, 4.0, None).on_hover_text(format!("{} {:.1} / {:.0} t fuel", icon::GAS_PUMP, gs.fleet.fuel_main, s.fuel_main_capacity));
    }
    resp.on_hover_text(format!("{ship}
{} · {}", gs.location.system, gs.location.situation())).clicked()
}

fn nav_item(ui: &mut Ui, page: Page, selected: bool, wide: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::same(9), Color32::from_white_alpha(22));
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(9), Color32::from_white_alpha(10));
    }
    if wide {
        p.text(rect.left_center() + Vec2::new(12.0, 0.0), Align2::LEFT_CENTER, page.icon(), egui::FontId::proportional(17.0), pal::ACCENT);
        p.text(rect.left_center() + Vec2::new(40.0, 0.0), Align2::LEFT_CENTER, page.label(), if selected { display(size::BODY) } else { egui::FontId::proportional(size::BODY) }, pal::TEXT);
        resp.clicked()
    } else {
        p.text(rect.center(), Align2::CENTER_CENTER, page.icon(), egui::FontId::proportional(19.0), if selected { pal::ACCENT } else { pal::DIM });
        resp.on_hover_text(page.label()).clicked()
    }
}

fn page_heading(page: Page, gs: &nexus_model::GameState) -> (String, String) {
    let sub = match page {
        Page::Dashboard => gs.location.situation(),
        Page::Exploration => format!("{} systems explored", gs.exploration.systems.len()),
        Page::Exobiology => format!("Exobiologist: {}", gs.cmdr.rank_name(nexus_data::ranks::RankKind::Exobiologist)),
        Page::Fleet => format!("{} ships", gs.fleet.ships.len()),
        Page::Missions => format!("{} active", gs.missions.active.len()),
        _ => String::new(),
    };
    (page.label().to_string(), sub)
}

impl eframe::App for NexusApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_started = Some(Instant::now());
        self.screenshot_tick(ctx);
        self.drain_alerts();
        self.assistant.poll();
        self.sync.tick(&self.core);
        // A route the assistant planned goes to the Navigation page (and the clipboard flow).
        if let Some(route) = self.ai_routes.lock().take() {
            let first = route.waypoints.first().map(|w| w.system.clone());
            self.toasts.push(Toast {
                alert: Alert::new(nexus_journal::Timestamp::now(), nexus_model::AlertKind::Navigation, Priority::Low, format!("{} route ready", route.kind))
                    .detail(first.map(|s| format!("{} · first stop {s} (see Navigation)", route.summary)).unwrap_or_else(|| route.summary.clone())),
                born: Instant::now(),
            });
            self.nav.planned = Some(route);
        }
        self.auto_copy_next();
        self.remember_deposits();
        self.follow_flight();
        let here = self.core.model.read().active().and_then(|g| g.location.star_pos);
        self.market.set_origin(here);
        self.overlay_logic(ctx);
        // egui's Ctrl +/−/0 zoom changes the factor directly; keep the setting in step with it.
        let zoom = ctx.zoom_factor();
        if self.applied_scale > 0.0 && (zoom - self.applied_scale).abs() > 1e-3 {
            self.settings.ui_scale = zoom;
            self.applied_scale = zoom;
            self.settings_dirty = Some(Instant::now());
        }
        if (self.settings.ui_scale - self.applied_scale).abs() > f32::EPSILON {
            ctx.set_zoom_factor(self.settings.ui_scale);
            self.applied_scale = self.settings.ui_scale;
        }
        if let Some(t) = self.settings_dirty
            && t.elapsed() > Duration::from_millis(600) {
                self.save_settings();
                self.settings_dirty = None;
            }
        // Keep clocks/countdowns ticking even without journal activity.
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    /// Transparent: a see-through overlay is see-through wherever the island isn't. The main window paints all of its
    /// area anyway.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
        // A frame that took long goes to nexus.log with its page, so a report of the window freezing can be traced.
        if let Some(took) = self.frame_started.take().map(|t| t.elapsed())
            && took > Duration::from_millis(250)
            && self.slow_frame_logged.is_none_or(|t| t.elapsed() > Duration::from_secs(10))
        {
            self.slow_frame_logged = Some(Instant::now());
            tracing::warn!(page = ?self.page, ms = took.as_millis() as u64, "slow frame");
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "page", &self.page);
        self.save_settings();
    }

    fn on_exit(&mut self) {
        // Close open batches; whatever the rate limits hold back stays queued on disk.
        self.sync.flush();
        // A normal exit: the next start has nothing to report.
        if self.shot.is_none() {
            crate::notices::clean_exit(&self.data_dir);
        }
    }
}

impl NexusApp {
    /// The overlay: its view from the model (read-locked for this call only; the overlay's window never locks it),
    /// and what it asks of the main window.
    fn overlay_logic(&mut self, ctx: &egui::Context) {
        let (phase_live, fresh) = {
            let status = self.core.status.read();
            (matches!(status.phase, Phase::Live), status.last_live.is_some_and(|t| t.elapsed() < Duration::from_secs(120)))
        };
        let requests = {
            let model = self.core.model.read();
            let gs = model.active();
            // The journal is followed live and the game is running (as the page header's "Live" says).
            let live = phase_live && (fresh || gs.is_some_and(|g| !g.game_closed));
            let src = crate::overlay::Sources {
                route: self.nav.planned.as_ref(),
                pins: &self.settings.market.pins,
                mining: &self.settings.mining,
                auto_copy: self.settings.auto_copy_next_system,
                high_value: self.settings.high_value_threshold,
                now: nexus_journal::Timestamp::now().0,
            };
            self.overlay.logic(ctx, gs, &src, &mut self.settings.overlay, live)
        };
        if requests.prefs_changed {
            self.settings_dirty = Some(Instant::now());
        }
        if let Some(page) = requests.open {
            self.page = page;
        }
    }

    /// The whole window: the sidebar and the page, the welcome, the banners; and the overlay's window beside it.
    fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.overlay.show(&ctx, &self.settings.overlay);
        // `--overlay <moment>`: the moment compact and expanded, for screenshots.
        if let Some(name) = self.overrides.overlay_demo.clone()
            && let Some(views) = crate::overlay::demo(&name)
        {
            use crate::overlay::draw::{stage, Anchor, Look};
            let look = Look { scale: self.settings.overlay.scale.clamp(1.0, 2.5), see_through: true, vr: false, anchor: Anchor::Centre, locked: true };
            egui::CentralPanel::default().frame(Frame::new()).show(ui, |ui| stage(ui, &name, &views, &look));
            return;
        }
        // Minimised, nothing of this window is seen: only the overlay's frames run.
        if ctx.input(|i| i.viewport().minimized == Some(true)) {
            return;
        }
        // Responsive: a full sidebar on wide windows, an icon rail on narrow ones.
        let wide = ui.ctx().content_rect().width() >= 1180.0;
        egui::Panel::left("nav")
            .exact_size(if wide { 252.0 } else { 78.0 })
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(pal::WINDOW).inner_margin(Margin { left: 10, right: 0, top: 10, bottom: 10 }))
            .show(ui, |ui| self.sidebar(ui, wide));
        let side = if wide { 32 } else { 24 };
        egui::CentralPanel::default()
            .frame(Frame::new().fill(pal::WINDOW).inner_margin(Margin { left: side, right: side, top: 20, bottom: 16 }))
            .show(ui, |ui| self.content(ui));
        if !self.settings.welcomed && self.shot.is_none() && matches!(self.core.status.read().phase, Phase::Live) {
            let files = self.core.status.read().files;
            let ctx = ui.ctx().clone();
            let model = self.core.model.read();
            let res = pages::welcome::show(&ctx, &mut self.settings, model.active(), files);
            drop(model);
            match res {
                pages::welcome::WelcomeResult::Open => {}
                pages::welcome::WelcomeResult::Done => {
                    self.settings.welcomed = true;
                    self.settings_dirty = Some(Instant::now());
                }
                pages::welcome::WelcomeResult::GoToAccounts => {
                    self.settings.welcomed = true;
                    self.settings_dirty = Some(Instant::now());
                    self.page = Page::Accounts;
                }
            }
        }
        self.toasts(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_model::context::Vehicle;

    fn cx(activity: Activity, focus: GuiFocus) -> Context {
        Context { activity, focus, danger: false, body: None }
    }

    /// The pages Follow my flight moves between, from the commander's context, EDEB's way.
    #[test]
    fn flight_from_context() {
        const KM150: f64 = 150_000.0;
        let of = |a: Activity, alt: Option<f64>| Flight::of(&cx(a, GuiFocus::None), alt, KM150);
        assert_eq!(of(Activity::Supercruise, None), Flight::System);
        assert_eq!(of(Activity::Space, None), Flight::System);
        assert_eq!(of(Activity::ChargingJump, None), Flight::Route);
        assert_eq!(of(Activity::Hyperspace, None), Flight::Route);
        assert_eq!(of(Activity::NearBody, Some(400_000.0)), Flight::System, "orbital cruise");
        assert_eq!(of(Activity::NearBody, Some(149_999.0)), Flight::Planet);
        assert_eq!(of(Activity::NearBody, None), Flight::System, "no altitude isn't under it");
        assert_eq!(of(Activity::Glide, Some(20_000.0)), Flight::Planet);
        assert_eq!(of(Activity::Landed, Some(0.0)), Flight::Planet);
        assert_eq!(of(Activity::OnFootPlanet, Some(0.0)), Flight::Planet);
        assert_eq!(of(Activity::Srv(Vehicle::Scarab), Some(0.0)), Flight::Idle, "driving changes nothing");
        assert_eq!(of(Activity::Docked, None), Flight::Idle);
        assert_eq!(of(Activity::NotInGame, None), Flight::Idle);
        assert_eq!(Flight::of(&cx(Activity::NearBody, GuiFocus::GalaxyMap), Some(1_000.0), KM150), Flight::Route, "the galaxy map wins");
        assert_eq!(Flight::of(&cx(Activity::NearBody, GuiFocus::SystemMap), Some(1_000.0), KM150), Flight::Planet, "the system map doesn't");
        assert_eq!(Flight::of(&cx(Activity::NearBody, GuiFocus::None), Some(100_000.0), 50_000.0), Flight::System, "the altitude is the setting's");
    }
}
