//! Updates: Elite Nexus asks GitHub whether a newer version is out (a minute after start, then every 12 hours; Settings
//! › About switches it off), says so in a banner above every page, and on a click downloads it, checks it against the
//! release's signed manifest and puts it in place once Elite Nexus closes: at once with Restart now, else whenever the
//! commander closes it. Copies that can't replace themselves (Program Files, inside the zip, a Mac, a build from
//! source) say why and offer the release page.
//!
//! Checks are downloads: nothing about the commander is sent. Never in a test run (`--screenshot`, `--journal-dir`),
//! and the data folder is never touched. The release side, the checks and the swap are in the nexus-update crate.

use crate::jobs::Job;
use crate::theme::{display, pal, size};
use crate::widgets;
use egui::{Align, CornerRadius, Frame, Layout, Margin, RichText, Ui, Vec2};
use egui_phosphor::regular as icon;
use nexus_update::{archive, install, notes, Asset, Manifest, Version};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The changelog as this version shipped it: "What's new" after an update, without the network.
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// The first check, after start-up and the journals.
const FIRST_CHECK: Duration = Duration::from_secs(60);
/// Then one every 12 hours while Elite Nexus runs.
const EVERY: Duration = Duration::from_secs(12 * 3600);
/// After a check that didn't get through, the next comes sooner.
const RETRY: Duration = Duration::from_secs(3600);
/// The release archive's program.
const PROGRAM: &str = if cfg!(windows) { "EliteNexus.exe" } else { "EliteNexus" };
/// main.rs writes why the swap at exit failed here; the next start says so.
const SWAP_ERROR: &str = "update-error.txt";

/// Settings › About › Updates (settings.json `updates`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatePrefs {
    /// Ask GitHub a minute after start and every 12 hours whether a newer version is out. On by default: it's a
    /// download of the release's manifest, and nothing about the commander is sent.
    pub check: bool,
    /// A version the commander chose to skip: its banner doesn't come back; a newer version's does.
    pub skipped: String,
    /// The version that ran last: a newer one at start says what's new.
    pub last_version: String,
    /// A version downloaded and waiting beside the program, so a start after a crash still puts it in place when
    /// Elite Nexus closes.
    pub staged: Option<StagedPrefs>,
}

impl Default for UpdatePrefs {
    fn default() -> Self {
        UpdatePrefs { check: true, skipped: String::new(), last_version: String::new(), staged: None }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StagedPrefs {
    pub version: String,
    /// The waiting program's SHA-256: a file something else left there isn't installed.
    pub sha256: String,
}

// ---------------------------------------------------------------------------------------------
// At exit (main.rs)

/// What happens once the window has closed: the waiting program takes the old one's place, and starts if the
/// commander asked for a restart.
#[derive(Debug, Clone)]
pub struct ExitPlan {
    pub exe: PathBuf,
    pub restart: bool,
    /// The version that's closing, for the next start's "Updated from".
    pub from: String,
}

static EXIT_PLAN: std::sync::Mutex<Option<ExitPlan>> = std::sync::Mutex::new(None);

fn plan_exit(plan: Option<ExitPlan>) {
    if let Ok(mut p) = EXIT_PLAN.lock() {
        *p = plan;
    }
}

/// The plan for the exit, if a new version is waiting (main.rs, after the window has closed).
pub fn take_exit_plan() -> Option<ExitPlan> {
    EXIT_PLAN.lock().ok().and_then(|mut p| p.take())
}

/// Puts the waiting program in place, once this one has closed and let go of the instance lock, and starts it on a
/// restart. If it can't, the old program stays (and starts again on a restart), and the next start says why.
pub fn finish_at_exit(plan: ExitPlan, data_dir: &Path) {
    match install::swap(&plan.exe) {
        Ok(()) => {
            tracing::info!(exe = %plan.exe.display(), from = %plan.from, "update put in place");
            if plan.restart
                && let Err(e) = install::launch(&plan.exe, &["--updated", &plan.from])
            {
                tracing::warn!("the updated Elite Nexus didn't start: {e}");
            }
        }
        Err(e) => {
            tracing::warn!("update not put in place: {e}");
            let _ = std::fs::write(data_dir.join(SWAP_ERROR), format!("The new version couldn't take the old one's place ({e})."));
            if plan.restart {
                let _ = install::launch(&plan.exe, &[]);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Where releases come from

/// GitHub, or for a test a folder or address holding `update.json`, its signature and the archives
/// (`ELITE_NEXUS_UPDATE_URL`). The signature is checked against the compiled-in key either way.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    GitHub,
    Folder(PathBuf),
    Url(String),
}

impl Source {
    fn from_env() -> Source {
        match std::env::var("ELITE_NEXUS_UPDATE_URL").map(|v| v.trim().to_string()) {
            Ok(v) if v.is_empty() => Source::GitHub,
            Ok(v) if Path::new(&v).is_dir() => Source::Folder(PathBuf::from(v)),
            Ok(v) => Source::Url(v.trim_end_matches('/').to_string()),
            Err(_) => Source::GitHub,
        }
    }
}

/// A newer release, as its signed manifest has it.
#[derive(Debug, Clone)]
struct Found {
    version: Version,
    manifest: Manifest,
}

impl Found {
    fn page(&self) -> String {
        if self.manifest.tag.is_empty() { nexus_update::RELEASES_PAGE.to_string() } else { nexus_update::release_page(&self.manifest.tag) }
    }
}

struct Download {
    job: Job<Result<install::Staged, String>>,
    version: String,
    got: Arc<AtomicU64>,
    total: u64,
    cancel: Arc<AtomicBool>,
}

enum Notes {
    None,
    Loading(Job<Result<Vec<notes::Section>, String>>),
    Ready(Vec<notes::Section>),
    Failed(String),
}

/// What the banner and Settings show, and what the buttons do.
pub struct Updater {
    current: Version,
    /// The program, as it was at start (before anything could move it).
    exe: Option<PathBuf>,
    /// Whether this copy can replace itself, and if not, why.
    installable: Result<(), install::Blocked>,
    /// Checks run by themselves: a live run of a release build, or one given a test source.
    auto: bool,
    source: Source,
    rt: tokio::runtime::Handle,
    http: Option<reqwest::Client>,
    next_check: Option<Instant>,
    check: Option<Job<Result<Found, String>>>,
    /// When the last check ended, and why it didn't get through.
    checked: Option<(Instant, Option<String>)>,
    found: Option<Found>,
    notes: Notes,
    notes_open: bool,
    download: Option<Download>,
    /// A version waiting beside the program, put in place when Elite Nexus closes.
    ready: Option<String>,
    failed: Option<String>,
    banner_closed: bool,
    /// This start follows an update, from this version.
    updated_from: Option<String>,
    playing: bool,
    /// A screenshot of the update states scrolls Settings to About's Updates row.
    scroll_to_row: bool,
}

impl Updater {
    /// At start. `live`: a run that follows the game (no test run checks or installs anything); `restarted_from`: the
    /// version a Restart now came from (`--updated`); `shot_tab`: a screenshot run's `--tab`, for the update states.
    pub fn new(live: bool, prefs: &mut UpdatePrefs, restarted_from: Option<String>, data_dir: &Path, rt: tokio::runtime::Handle, shot_tab: Option<&str>) -> Updater {
        let current = Version::current();
        let exe = std::env::current_exe().ok();
        let installable = exe.as_deref().map_or(Err(install::Blocked::Source), install::installable);
        let source = Source::from_env();
        let mut u = Updater {
            current,
            exe,
            installable,
            auto: live && prefs.check && (!cfg!(debug_assertions) || source != Source::GitHub),
            source,
            rt,
            http: reqwest::Client::builder().user_agent(concat!("EliteNexus/", env!("CARGO_PKG_VERSION"))).connect_timeout(Duration::from_secs(20)).build().ok(),
            next_check: None,
            check: None,
            checked: None,
            found: None,
            notes: Notes::None,
            notes_open: false,
            download: None,
            ready: None,
            failed: None,
            banner_closed: false,
            updated_from: restarted_from,
            playing: false,
            scroll_to_row: false,
        };
        if let Some(tab) = shot_tab {
            u.demo(tab);
            return u;
        }
        if !live {
            return u;
        }
        u.next_check = u.auto.then(|| Instant::now() + FIRST_CHECK);
        if let Some(exe) = u.exe.clone() {
            install::clean_up(&exe);
            if u.updated_from.is_some() {
                // The copy that started this one may still be closing, and its program can't go while it runs: again
                // once it has.
                let exe = exe.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(10));
                    install::clean_up(&exe);
                });
            }
            // A version downloaded before the last exit and still waiting (that run crashed): it goes in at this exit.
            match prefs.staged.take() {
                Some(s) if Version::parse(&s.version).is_some_and(|v| v > u.current) && install::staged_matches(&exe, &s.sha256) => {
                    u.ready = Some(s.version.clone());
                    plan_exit(Some(ExitPlan { exe: exe.clone(), restart: false, from: u.current.to_string() }));
                    prefs.staged = Some(s);
                }
                // In place already, or replaced since by hand: what's left of it goes.
                Some(s) if install::staged_matches(&exe, &s.sha256) => install::discard_staged(&exe),
                _ => {}
            }
        }
        if let Ok(why) = std::fs::read_to_string(data_dir.join(SWAP_ERROR)) {
            let _ = std::fs::remove_file(data_dir.join(SWAP_ERROR));
            u.failed = Some(format!("{} nexus.log in the data folder has the details.", why.trim()));
        }
        // Updated since the last run, by this or by hand: the banner says what's new.
        let last = Version::parse(&prefs.last_version);
        if u.updated_from.is_none() && last.as_ref().is_some_and(|l| *l < u.current) {
            u.updated_from = Some(prefs.last_version.clone());
        }
        prefs.last_version = u.current.to_string();
        u
    }

    /// Screenshot states (`--tab update` and its siblings), with the changelog's own sections as the notes.
    fn demo(&mut self, tab: &str) {
        let next = Version { patch: self.current.patch + 1, pre: Vec::new(), ..self.current.clone() };
        let asset = Asset { name: format!("EliteNexus-v{next}-windows-x64.zip"), url: nexus_update::release_file(&format!("v{next}"), "x.zip"), size: 21_400_000, sha256: "0".repeat(64) };
        let manifest = Manifest { schema: 1, version: next.to_string(), tag: format!("v{next}"), date: String::new(), assets: [(nexus_update::manifest::this_platform(), asset)].into_iter().collect() };
        let found = Found { version: next.clone(), manifest };
        // The changelog's Unreleased part, as the made-up version's notes.
        let upcoming = notes::between(&CHANGELOG.replacen("## Unreleased", &format!("## [{next}]"), 1), &self.current, &next);
        let tab = tab.to_ascii_lowercase();
        // A screenshot build can't update itself (it's built from source): the states show a copy that can, or one in
        // a folder it can't write to, the most common reason it can't.
        self.installable = if tab.contains("blocked") { Err(install::Blocked::ReadOnly(PathBuf::from(r"C:\Program Files\Elite Nexus"))) } else { Ok(()) };
        match tab.as_str() {
            "update" | "update-blocked" => self.found = Some(found),
            "update-notes" => {
                self.found = Some(found);
                self.notes = Notes::Ready(upcoming);
                self.notes_open = true;
            }
            "update-download" => {
                self.found = Some(found);
                // A download that never ends, 8.9 of 21.4 MB in.
                let job = Job::spawn_async(&egui::Context::default(), &self.rt, std::future::pending());
                self.download = Some(Download { job, version: next.to_string(), got: Arc::new(AtomicU64::new(8_900_000)), total: 21_400_000, cancel: Arc::default() });
            }
            "update-ready" => {
                self.found = Some(found);
                self.ready = Some(next.to_string());
            }
            "update-failed" => {
                self.found = Some(found);
                self.failed = Some("The download isn't the file the release signed, so nothing was changed.".into());
            }
            "updated" => {
                // From the version before this one, as the changelog has it.
                let from = notes::sections(CHANGELOG).iter().filter_map(|s| Version::parse(&s.version)).find(|v| *v < self.current);
                self.updated_from = Some(from.map_or_else(|| "0.8.0".to_string(), |v| v.to_string()));
            }
            _ => {}
        }
        self.checked = Some((Instant::now(), None));
        self.scroll_to_row = tab.starts_with("update");
    }

    /// Every frame: starts checks when they're due and collects what the jobs found. True when `prefs` changed.
    pub fn tick(&mut self, ctx: &egui::Context, prefs: &mut UpdatePrefs, playing: bool) -> bool {
        self.playing = playing;
        let now = Instant::now();
        let mut changed = false;
        if let Some(result) = self.check.as_mut().and_then(Job::poll) {
            self.check = None;
            match result {
                Ok(found) => {
                    self.checked = Some((now, None));
                    self.next_check = self.auto.then(|| now + EVERY);
                    if found.version > self.current {
                        if self.found.as_ref().is_none_or(|f| f.version != found.version) {
                            // A version not offered before: its banner opens again, with its own notes.
                            self.banner_closed = false;
                            self.notes = Notes::None;
                        }
                        // The waiting version is older than this one: it gives way.
                        if self.ready.as_deref().and_then(Version::parse).is_some_and(|r| r < found.version) {
                            self.give_up_staged(prefs);
                            changed = true;
                        }
                        self.found = Some(found);
                    } else {
                        self.found = None;
                    }
                }
                Err(e) => {
                    tracing::info!("update check: {e}");
                    self.checked = Some((now, Some(e)));
                    self.next_check = self.auto.then(|| now + RETRY);
                }
            }
        }
        if self.auto && prefs.check && self.check.is_none() && self.download.is_none() && self.next_check.is_some_and(|t| now >= t) {
            self.start_check(ctx);
        }
        if let Some(d) = self.download.as_mut() {
            if let Some(result) = d.job.poll() {
                let version = d.version.clone();
                let cancelled = d.cancel.load(Ordering::Relaxed);
                self.download = None;
                match result {
                    Ok(staged) => {
                        for k in &staged.kept {
                            tracing::warn!("update: a document wasn't replaced: {k}");
                        }
                        tracing::info!(%version, "update downloaded and waiting");
                        self.ready = Some(version.clone());
                        self.failed = None;
                        prefs.staged = Some(StagedPrefs { version, sha256: staged.sha256 });
                        changed = true;
                        if let Some(exe) = self.exe.clone() {
                            plan_exit(Some(ExitPlan { exe, restart: false, from: self.current.to_string() }));
                        }
                    }
                    Err(_) if cancelled => {}
                    Err(e) => {
                        tracing::warn!("update failed: {e}");
                        self.failed = Some(e);
                    }
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(150));
            }
        }
        if let Notes::Loading(job) = &mut self.notes
            && let Some(r) = job.poll()
        {
            self.notes = match r {
                Ok(s) => Notes::Ready(s),
                Err(e) => Notes::Failed(e),
            };
        }
        changed
    }

    /// The checks were switched on or off in Settings.
    fn set_auto(&mut self, on: bool, live: bool) {
        self.auto = on && live;
        self.next_check = self.auto.then(|| Instant::now() + Duration::from_secs(2));
    }

    fn start_check(&mut self, ctx: &egui::Context) {
        let Some(http) = self.http.clone() else {
            self.checked = Some((Instant::now(), Some("Elite Nexus couldn't set up its connection.".into())));
            return;
        };
        let (source, beta) = (self.source.clone(), self.current.is_pre());
        self.check = Some(Job::spawn_async(ctx, &self.rt, check(http, source, beta)));
    }

    fn start_download(&mut self, ctx: &egui::Context) {
        let (Some(found), Some(exe), Some(http)) = (self.found.clone(), self.exe.clone(), self.http.clone()) else { return };
        let Some(asset) = found.manifest.asset(&nexus_update::manifest::this_platform()).cloned() else {
            self.failed = Some("The release has no archive for this computer.".into());
            return;
        };
        let (got, cancel) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
        let total = asset.size;
        let job = Job::spawn_async(ctx, &self.rt, download_and_stage(http, self.source.clone(), asset, exe, got.clone(), cancel.clone()));
        self.failed = None;
        self.download = Some(Download { job, version: found.version.to_string(), got, total, cancel });
    }

    fn load_notes(&mut self, ctx: &egui::Context) {
        if !matches!(self.notes, Notes::None | Notes::Failed(_)) {
            return;
        }
        // The version on offer, or one still waiting from the last run (before this run's first check): its
        // changelog, from its own tag.
        let next = match (&self.found, self.ready.as_deref().and_then(Version::parse)) {
            (Some(f), _) => Some((f.version.clone(), f.manifest.tag.clone())),
            (None, Some(v)) => Some((v, String::new())),
            (None, None) => None,
        };
        if let Some((to, tag)) = next {
            let Some(http) = self.http.clone() else { return };
            let tag = if tag.is_empty() { format!("v{to}") } else { tag };
            let from = self.current.clone();
            let url = match &self.source {
                Source::GitHub => nexus_update::changelog_at(&tag),
                Source::Url(base) => format!("{base}/CHANGELOG.md"),
                Source::Folder(dir) => dir.join("CHANGELOG.md").to_string_lossy().into_owned(),
            };
            let folder = matches!(self.source, Source::Folder(_));
            self.notes = Notes::Loading(Job::spawn_async(ctx, &self.rt, async move {
                let text = if folder { std::fs::read_to_string(&url).map_err(|e| e.to_string())? } else { String::from_utf8_lossy(&get(&http, &url, 4 << 20, None, None).await?).into_owned() };
                Ok(notes::between(&text, &from, &to))
            }));
        } else if let Some(from) = &self.updated_from {
            let from = Version::parse(from).unwrap_or(Version { major: 0, minor: 0, patch: 0, pre: Vec::new() });
            self.notes = Notes::Ready(notes::between(CHANGELOG, &from, &self.current));
        }
    }

    /// Closes Elite Nexus, puts the waiting version in place and starts it.
    fn restart(&mut self, ctx: &egui::Context) {
        let Some(exe) = self.exe.clone() else { return };
        plan_exit(Some(ExitPlan { exe, restart: true, from: self.current.to_string() }));
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn give_up_staged(&mut self, prefs: &mut UpdatePrefs) {
        if let Some(exe) = &self.exe {
            install::discard_staged(exe);
        }
        plan_exit(None);
        self.ready = None;
        prefs.staged = None;
    }

    /// The newer version to offer: not skipped, unless it's downloading or waiting already.
    fn offered(&self, prefs: &UpdatePrefs) -> Option<&Found> {
        self.found.as_ref().filter(|f| prefs.skipped != f.version.to_string() || self.ready.is_some() || self.download.is_some())
    }

    // -----------------------------------------------------------------------------------------
    // The banner above every page

    /// The banner, when there's something to say.
    pub fn banner(&mut self, ui: &mut Ui, prefs: &UpdatePrefs) {
        let ctx = ui.ctx().clone();
        let cur = self.current.to_string();
        if let Some(d) = &self.download {
            let (got, total) = (d.got.load(Ordering::Relaxed), d.total.max(1));
            let detail = format!("{} of {}. It's checked against the release's signed manifest before anything changes.", megabytes(got), megabytes(total));
            let mut cancel = false;
            banner(ui, Tone::Info, &format!("Downloading Elite Nexus {}…", d.version), &detail, |ui| cancel = widgets::button(ui, "Cancel").clicked(), |ui| {
                widgets::bar(ui, (got as f32 / total as f32).clamp(0.0, 1.0), pal::BLUE, 4.0, None);
            });
            if cancel && let Some(d) = &self.download {
                d.cancel.store(true, Ordering::Relaxed);
            }
            return;
        }
        if self.banner_closed {
            return;
        }
        if let Some(e) = self.failed.clone() {
            let (mut again, mut page, mut close) = (false, false, false);
            let detail = format!("{e} Elite Nexus {cur} keeps running as it was.");
            banner(
                ui,
                Tone::Warn,
                "The update didn't work.",
                &detail,
                |ui| {
                    close = close_button(ui);
                    page = widgets::button(ui, &format!("Release page {}", icon::ARROW_SQUARE_OUT)).clicked();
                    if self.found.is_some() && self.installable.is_ok() {
                        again = widgets::button(ui, "Try again").clicked();
                    }
                },
                |_| {},
            );
            if again {
                self.start_download(&ctx);
            }
            if page {
                ctx.open_url(egui::OpenUrl::new_tab(self.found.as_ref().map_or_else(|| nexus_update::RELEASES_PAGE.to_string(), Found::page)));
            }
            if close {
                self.failed = None;
                self.banner_closed = true;
            }
            return;
        }
        if let Some(version) = self.ready.clone() {
            let detail = if self.playing {
                "It takes this one's place when you close Elite Nexus, so nothing stops while you play. Restart now does it at once, in a few seconds.".to_string()
            } else {
                "It takes this one's place when you close Elite Nexus. Restart now does it at once, in a few seconds; your settings and data stay as they are.".to_string()
            };
            let (mut restart, mut notes, mut close) = (false, false, false);
            banner(
                ui,
                Tone::Info,
                &format!("Elite Nexus {version} is ready."),
                &detail,
                |ui| {
                    close = close_button(ui);
                    notes = widgets::button(ui, "What's new").clicked();
                    restart = widgets::primary_button(ui, "Restart now").clicked();
                },
                |_| {},
            );
            if restart {
                self.restart(&ctx);
            }
            if notes {
                self.open_notes(&ctx);
            }
            self.banner_closed |= close;
            return;
        }
        if let Some(found) = self.offered(prefs).cloned() {
            let (mut update, mut notes, mut page, mut close) = (false, false, false, false);
            let has_asset = found.manifest.asset(&nexus_update::manifest::this_platform()).is_some();
            let can = self.installable.is_ok() && has_asset;
            let detail = match &self.installable {
                Ok(()) if has_asset => format!("You have {cur}. Update downloads it, checks it, and puts it in place when you close Elite Nexus or restart it. Your settings and data stay as they are."),
                Ok(()) => format!("You have {cur}. This release has no archive for this computer: the release page has what there is."),
                Err(why) => format!("You have {cur}. {}", why.why()),
            };
            banner(
                ui,
                Tone::Info,
                &format!("Elite Nexus {} is out.", found.version),
                &detail,
                |ui| {
                    close = close_button(ui);
                    notes = widgets::button(ui, "What's new").clicked();
                    if can {
                        update = widgets::primary_button(ui, "Update").clicked();
                    } else {
                        page = widgets::primary_button(ui, &format!("Download {}", icon::ARROW_SQUARE_OUT)).on_hover_text(found.page()).clicked();
                    }
                },
                |_| {},
            );
            if update {
                self.start_download(&ctx);
            }
            if page {
                ctx.open_url(egui::OpenUrl::new_tab(found.page()));
            }
            if notes {
                self.open_notes(&ctx);
            }
            self.banner_closed |= close;
            return;
        }
        if let Some(from) = self.updated_from.clone() {
            let (mut notes, mut close) = (false, false);
            banner(
                ui,
                Tone::Info,
                &format!("Updated to Elite Nexus {cur}."),
                &format!("From {from}. Your settings and data are as they were."),
                |ui| {
                    close = close_button(ui);
                    notes = widgets::button(ui, "What's new").clicked();
                },
                |_| {},
            );
            if notes {
                self.open_notes(&ctx);
            }
            if close {
                self.updated_from = None;
            }
        }
    }

    fn open_notes(&mut self, ctx: &egui::Context) {
        self.load_notes(ctx);
        self.notes_open = true;
    }

    /// "What's new", over the window while it's open. True when `prefs` changed (Skip this version).
    pub fn windows(&mut self, ctx: &egui::Context, prefs: &mut UpdatePrefs) -> bool {
        if !self.notes_open {
            return false;
        }
        let offered = self.found.clone().filter(|_| self.ready.is_none());
        let title = match (&offered, &self.updated_from) {
            (Some(f), _) => format!("What's new in {}", f.version),
            (None, _) if self.ready.is_some() => format!("What's new in {}", self.ready.clone().unwrap_or_default()),
            _ => format!("What's new in {}", self.current),
        };
        let page = offered.as_ref().or(self.found.as_ref()).map_or_else(|| nexus_update::release_page(&format!("v{}", self.current)), Found::page);
        let (mut close, mut skip) = (false, false);
        let modal = egui::Modal::new(egui::Id::new("whats-new"))
            .frame(Frame::new().fill(pal::SIDEBAR).corner_radius(CornerRadius::same(18)).inner_margin(Margin::same(24)))
            .show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 120.0).clamp(360.0, 680.0));
                ui.label(RichText::new(&title).font(display(size::TITLE3)).color(pal::TEXT));
                ui.add_space(4.0);
                ui.label(RichText::new(format!("From the changelog, for every version since {}.", self.updated_from.clone().filter(|_| offered.is_none()).unwrap_or_else(|| self.current.to_string()))).size(size::FOOTNOTE).color(pal::DIM));
                ui.add_space(10.0);
                let max_h = (ctx.content_rect().height() * 0.62).max(200.0);
                egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| match &self.notes {
                    Notes::Ready(sections) if sections.is_empty() => {
                        ui.label(RichText::new("The changelog has nothing for these versions: the release page says what changed.").color(pal::DIM));
                    }
                    Notes::Ready(sections) => draw_sections(ui, sections),
                    Notes::Loading(_) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Reading the changelog from GitHub…").color(pal::DIM));
                        });
                    }
                    Notes::Failed(e) => {
                        ui.add(egui::Label::new(RichText::new(format!("The changelog couldn't be read: {e}. The release page has the notes.")).color(pal::DIM)).wrap());
                    }
                    Notes::None => {}
                });
                ui.add_space(14.0);
                ui.separator();
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::button(ui, &format!("Release page {}", icon::ARROW_SQUARE_OUT)).on_hover_text(&page).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(&page));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        close = widgets::primary_button(ui, "Close").clicked();
                        if let Some(f) = &offered
                            && self.download.is_none()
                        {
                            skip = widgets::button(ui, &format!("Skip {}", f.version)).on_hover_text("No banner for this version; a newer one shows again. Settings › About can still update to it.").clicked();
                        }
                    });
                });
            });
        if close || modal.should_close() {
            self.notes_open = false;
        }
        if skip && let Some(f) = offered {
            prefs.skipped = f.version.to_string();
            self.notes_open = false;
            return true;
        }
        false
    }

    // -----------------------------------------------------------------------------------------
    // The sidebar and Settings

    /// Under "Version" in the sidebar: a newer version, which opens its banner again. Nothing when there's none.
    pub fn sidebar_line(&mut self, ui: &mut Ui, prefs: &UpdatePrefs) {
        let text = if let Some(v) = &self.ready {
            format!("{} {v} ready", icon::ARROW_CIRCLE_UP)
        } else if let Some(f) = self.offered(prefs) {
            format!("{} {} available", icon::ARROW_CIRCLE_UP, f.version)
        } else {
            return;
        };
        let r = ui.add(egui::Label::new(RichText::new(text).size(size::CAPTION).color(pal::ACCENT)).sense(egui::Sense::click()));
        if r.on_hover_text("Show the update banner").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.banner_closed = false;
        }
    }

    /// Settings › About: the update's state with its button, and the switch for the checks. True when `prefs`
    /// changed. `live`: a run that follows the game (checks run only then).
    pub fn about_rows(&mut self, g: &mut widgets::Group<'_>, prefs: &mut UpdatePrefs, live: bool) -> bool {
        let ctx = g.ui.ctx().clone();
        let mut changed = false;
        if std::mem::take(&mut self.scroll_to_row) {
            g.ui.scroll_to_cursor(Some(Align::Center));
        }
        let status = self.status(prefs);
        let mut action = None;
        g.row("Updates", &status, |ui| {
            if self.download.is_some() {
                if widgets::button(ui, "Cancel").clicked() {
                    action = Some("cancel");
                }
            } else if self.ready.is_some() {
                if widgets::primary_button(ui, "Restart now").clicked() {
                    action = Some("restart");
                }
            } else if let Some(f) = &self.found {
                let can = self.installable.is_ok() && f.manifest.asset(&nexus_update::manifest::this_platform()).is_some();
                if can && widgets::primary_button(ui, "Update").clicked() {
                    action = Some("update");
                }
                if !can && widgets::primary_button(ui, &format!("Download {}", icon::ARROW_SQUARE_OUT)).clicked() {
                    action = Some("page");
                }
                if widgets::button(ui, "What's new").clicked() {
                    action = Some("notes");
                }
            } else {
                let busy = self.check.is_some();
                if ui.add_enabled(!busy, egui::Button::new(RichText::new(if busy { "Checking…" } else { "Check now" }).size(size::CALLOUT).color(pal::TEXT)).corner_radius(CornerRadius::same(255)).min_size(Vec2::new(0.0, 30.0))).clicked() {
                    action = Some("check");
                }
            }
        });
        match action {
            Some("cancel") => {
                if let Some(d) = &self.download {
                    d.cancel.store(true, Ordering::Relaxed);
                }
            }
            Some("restart") => self.restart(&ctx),
            Some("update") => {
                // Skipped or not, a click on Update means it.
                self.banner_closed = false;
                self.start_download(&ctx);
            }
            Some("page") => {
                if let Some(f) = &self.found {
                    ctx.open_url(egui::OpenUrl::new_tab(f.page()));
                }
            }
            Some("notes") => self.open_notes(&ctx),
            Some("check") => self.start_check(&ctx),
            _ => {}
        }
        if g.switch(
            &mut prefs.check,
            "Check for updates",
            "A minute after start and every 12 hours, Elite Nexus asks GitHub which version is newest: a download, like opening the release page. Nothing about you, your commander or your game is sent. Updating always takes a click.",
        ) {
            self.set_auto(prefs.check && (!cfg!(debug_assertions) || self.source != Source::GitHub), live);
            changed = true;
        }
        changed
    }

    /// The Updates row's line.
    fn status(&self, prefs: &UpdatePrefs) -> String {
        if let Some(d) = &self.download {
            return format!("Downloading {}: {} of {}.", d.version, megabytes(d.got.load(Ordering::Relaxed)), megabytes(d.total));
        }
        if let Some(v) = &self.ready {
            return format!("{v} is downloaded and checked: it's put in place when you close Elite Nexus.");
        }
        if let Some(e) = &self.failed {
            return format!("The last update didn't work: {e}");
        }
        if let Some(f) = &self.found {
            let skipped = if prefs.skipped == f.version.to_string() { " (you skipped it)" } else { "" };
            let how = match &self.installable {
                Ok(()) => String::new(),
                Err(why) => format!(" {}", why.why()),
            };
            return format!("{} is out{skipped}; you have {}.{how}", f.version, self.current);
        }
        if self.check.is_some() {
            return "Asking GitHub…".into();
        }
        match &self.checked {
            Some((at, None)) => format!("Up to date: {} is the newest. Checked {}.", self.current, ago(*at)),
            Some((at, Some(e))) => format!("Couldn't check {}: {e}.", ago(*at)),
            None if !prefs.check => "Checks are off: Check now asks once.".into(),
            None if cfg!(debug_assertions) && self.source == Source::GitHub => "A build from source doesn't check by itself: Check now asks once.".into(),
            None if self.next_check.is_some() => "The first check comes a minute after start.".into(),
            None => "Not checked in this run.".into(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Network

/// Asks for the newest release: the latest stable one, or for a beta the newest of all (the releases' feed), and
/// checks its manifest's signature.
async fn check(http: reqwest::Client, source: Source, beta: bool) -> Result<Found, String> {
    let (bytes, sig) = match &source {
        Source::GitHub => {
            let tag = if beta {
                let feed = get(&http, nexus_update::FEED, 2 << 20, None, None).await?;
                nexus_update::newest_tag(&nexus_update::feed_tags(&String::from_utf8_lossy(&feed))).map(|(_, t)| t)
            } else {
                None
            };
            let url = |file: &str| tag.as_deref().map_or_else(|| nexus_update::latest_file(file), |t| nexus_update::release_file(t, file));
            let bytes = get(&http, &url(nexus_update::MANIFEST), nexus_update::manifest::MAX_MANIFEST_BYTES, None, None).await?;
            let sig = get(&http, &url(nexus_update::SIGNATURE), 4096, None, None).await?;
            (bytes, sig)
        }
        Source::Url(base) => {
            let bytes = get(&http, &format!("{base}/{}", nexus_update::MANIFEST), nexus_update::manifest::MAX_MANIFEST_BYTES, None, None).await?;
            let sig = get(&http, &format!("{base}/{}", nexus_update::SIGNATURE), 4096, None, None).await?;
            (bytes, sig)
        }
        Source::Folder(dir) => {
            let read = |f: &str| std::fs::read(dir.join(f)).map_err(|e| format!("{}: {e}", dir.join(f).display()));
            (read(nexus_update::MANIFEST)?, read(nexus_update::SIGNATURE)?)
        }
    };
    let manifest = Manifest::verified(&bytes, &String::from_utf8_lossy(&sig), nexus_update::PUBLIC_KEY).map_err(|e| format!("the release's update manifest was refused: {e}"))?;
    let version = manifest.version().ok_or("the release's update manifest names no version")?;
    Ok(Found { version, manifest })
}

/// Downloads the archive, checks it against the signed manifest, takes the program from it and puts it beside the
/// running one. Nothing changes unless every check holds.
async fn download_and_stage(http: reqwest::Client, source: Source, asset: Asset, exe: PathBuf, got: Arc<AtomicU64>, cancel: Arc<AtomicBool>) -> Result<install::Staged, String> {
    // Never more than the manifest says, and never more than any Elite Nexus could be.
    let max = (asset.size as usize).min(600 << 20);
    let bytes = match &source {
        Source::GitHub => get(&http, &asset.url, max, Some(&got), Some(&cancel)).await?,
        Source::Url(base) => get(&http, &format!("{base}/{}", asset.name), max, Some(&got), Some(&cancel)).await?,
        Source::Folder(dir) => std::fs::read(dir.join(&asset.name)).map_err(|e| format!("{}: {e}", asset.name))?,
    };
    if bytes.len() as u64 != asset.size {
        return Err("The download isn't as large as the release says, so nothing was changed.".into());
    }
    if nexus_update::sha256_hex(&bytes) != asset.sha256 {
        return Err("The download isn't the file the release signed, so nothing was changed.".into());
    }
    let kind = archive::Kind::of(&asset.name).ok_or_else(|| format!("{} isn't an archive Elite Nexus reads.", asset.name))?;
    tokio::task::spawn_blocking(move || {
        let files = archive::extract(&bytes, kind, PROGRAM).map_err(|e| format!("The release archive: {e}."))?;
        install::stage(&exe, &files, PROGRAM).map_err(|e| format!("The new version couldn't be put beside this one ({e}), so nothing was changed."))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A GET of at most `max` bytes, counting into `got` and stopping when `cancel` is set.
async fn get(http: &reqwest::Client, url: &str, max: usize, got: Option<&AtomicU64>, cancel: Option<&AtomicBool>) -> Result<Vec<u8>, String> {
    // Long enough for a slow line to download the archive; a check gives up sooner.
    let timeout = if got.is_some() { Duration::from_secs(30 * 60) } else { Duration::from_secs(45) };
    let mut resp = http.get(url).timeout(timeout).send().await.map_err(|e| net_error(&e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            404 => "GitHub has no such file for this release (404)".to_string(),
            403 | 429 => "GitHub turned the request down for now (too many from this network); it's asked again later".to_string(),
            s => format!("GitHub answered {s}"),
        });
    }
    if resp.content_length().is_some_and(|n| n > max as u64) {
        return Err("the file is larger than it should be".into());
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| net_error(&e))? {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err("cancelled".into());
        }
        out.extend_from_slice(&chunk);
        if out.len() > max {
            return Err("the file is larger than it should be".into());
        }
        if let Some(g) = got {
            g.store(out.len() as u64, Ordering::Relaxed);
        }
    }
    Ok(out)
}

fn net_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "GitHub didn't answer in time".into()
    } else if e.is_connect() {
        "no connection to GitHub (offline?)".into()
    } else {
        format!("the connection to GitHub failed ({e})")
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// The banners' tones (DESIGN.md › Colour › Meaning): information, or "check this".
#[derive(Clone, Copy)]
enum Tone {
    Info,
    Warn,
}

/// A banner above the page (DESIGN.md › Banner): what happened, what it means, its buttons at the right (added right
/// to left), and `below` under the text (a progress bar). The UI overhaul's `widgets::banner_with` replaces it once it
/// lands.
fn banner(ui: &mut Ui, tone: Tone, title: &str, detail: &str, buttons: impl FnOnce(&mut Ui), below: impl FnOnce(&mut Ui)) {
    let (colour, glyph) = match tone {
        Tone::Info => (pal::BLUE, icon::ARROW_CIRCLE_UP),
        Tone::Warn => (pal::WARN, icon::WARNING),
    };
    Frame::new().fill(colour.gamma_multiply(0.13)).corner_radius(CornerRadius::same(10)).inner_margin(Margin { left: 16, right: 12, top: 12, bottom: 12 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.label(RichText::new(glyph).size(18.0).color(colour));
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                buttons(ui);
                ui.add_space(4.0);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    let mut job = egui::text::LayoutJob::default();
                    job.append(title, 0.0, egui::TextFormat { font_id: display(size::BODY), color: pal::TEXT, ..Default::default() });
                    job.append(" ", 0.0, egui::TextFormat { font_id: egui::FontId::proportional(size::BODY), color: pal::TEXT, ..Default::default() });
                    job.append(detail, 0.0, egui::TextFormat { font_id: egui::FontId::proportional(size::BODY), color: pal::DIM, ..Default::default() });
                    ui.add_space(5.0);
                    ui.add(egui::Label::new(job).wrap());
                    below(ui);
                });
            });
        });
    });
    ui.add_space(12.0);
}

fn close_button(ui: &mut Ui) -> bool {
    widgets::icon_button(ui, icon::X, "Close: it shows again at the next start").clicked()
}

/// The changelog's sections: the version and its day, then its groups and items.
fn draw_sections(ui: &mut Ui, sections: &[notes::Section]) {
    for (i, s) in sections.iter().enumerate() {
        if i > 0 {
            ui.add_space(14.0);
        }
        let when = if s.date.is_empty() { String::new() } else { format!("  ·  {}", s.date) };
        ui.label(RichText::new(format!("{}{when}", s.version)).font(display(size::HEADLINE)).color(pal::TEXT));
        ui.add_space(4.0);
        for b in &s.blocks {
            match b {
                notes::Block::Heading(h) => {
                    ui.add_space(6.0);
                    ui.label(RichText::new(h).font(display(size::FOOTNOTE)).color(pal::DIM));
                }
                notes::Block::Text { indent, marker, text } => {
                    ui.horizontal_top(|ui| {
                        ui.add_space(f32::from(*indent) * 18.0);
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if let Some(m) = marker {
                            ui.label(RichText::new(m).size(size::CALLOUT).color(pal::DIM));
                        }
                        ui.add(egui::Label::new(rich(text)).wrap());
                    });
                    ui.add_space(3.0);
                }
            }
        }
    }
}

/// A changelog line with its bold and code spans.
fn rich(text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for s in notes::spans(text) {
        let font = if s.code {
            egui::FontId::monospace(size::FOOTNOTE)
        } else if s.bold {
            display(size::CALLOUT)
        } else {
            egui::FontId::proportional(size::CALLOUT)
        };
        job.append(&s.text, 0.0, egui::TextFormat { font_id: font, color: if s.bold { pal::TEXT } else { pal::DIM }, ..Default::default() });
    }
    job
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

fn ago(at: Instant) -> String {
    let s = at.elapsed().as_secs();
    if s < 60 {
        "just now".into()
    } else if s < 3600 {
        format!("{} min ago", s / 60)
    } else {
        format!("{} h ago", s / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_check_by_default_and_read_old_files() {
        assert!(UpdatePrefs::default().check, "on by default");
        // A settings file from before updates existed has no `updates`: the defaults.
        let s: crate::settings::Settings = serde_json::from_str("{}").expect("settings");
        assert!(s.updates.check);
        let p: UpdatePrefs = serde_json::from_str(r#"{"check":false,"skipped":"0.8.4","staged":{"version":"0.8.4","sha256":"ab"},"later":1}"#).expect("prefs");
        assert!(!p.check);
        assert_eq!(p.staged, Some(StagedPrefs { version: "0.8.4".into(), sha256: "ab".into() }));
    }

    #[test]
    fn the_compiled_in_changelog_has_this_version_or_its_unreleased_part() {
        // A release build's changelog names its own version; a build between releases has "Unreleased" on top.
        let cur = Version::current().to_string();
        assert!(CHANGELOG.contains(&format!("## [{cur}]")) || CHANGELOG.contains("## Unreleased"), "CHANGELOG.md for {cur}");
    }

    #[test]
    fn the_exit_plan_is_taken_once() {
        plan_exit(Some(ExitPlan { exe: PathBuf::from("EliteNexus.exe"), restart: true, from: "0.8.3".into() }));
        assert!(take_exit_plan().is_some_and(|p| p.restart));
        assert!(take_exit_plan().is_none());
    }
}
