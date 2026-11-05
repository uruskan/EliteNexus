//! Cockpit lights (a tab of HUD & cockpit): installs, calibrates, sets up and removes the
//! cockpit-light dimmer, 3Dmigoto plus a fix for the game's box-light shader (`nexus_hud::dimmer`).
//! The same install is the graphics hook the HUD colours tab needs to keep hostile red, friendly green
//! and shield blue.

use crate::jobs::Slot;
use crate::theme::{display, pal, size};
use crate::widgets::{self, card, footnote, group, section};
use egui::{RichText, Ui};
use egui_phosphor::regular as icon;
use nexus_binds::{Bindings, Input};
use nexus_hud::dimmer::{self, Calibration, Dimmer, HookConfig, Removed, Status};
use nexus_hud::install::GameInstall;
use std::path::{Path, PathBuf};

/// The game folder the HUD & cockpit page works on: the live (Odyssey) client, else the first found.
pub fn game_folder(installs: &[GameInstall]) -> Option<PathBuf> {
    installs.iter().find(|i| i.is_live()).or(installs.first()).map(|i| i.folder.clone())
}

type Files = Vec<(&'static str, Vec<u8>)>;

#[derive(Default)]
pub struct CockpitUi {
    loaded: bool,
    /// The game folder (the one holding `EliteDangerous64.exe`) and what's in it.
    folder: Option<PathBuf>,
    status: Option<Status>,
    /// Settings being edited; Save writes them to the game's config.
    edit: Dimmer,
    /// Install was clicked: the page explains what happens before anything is downloaded.
    confirm_install: bool,
    confirm_remove: bool,
    download: Slot<Result<Files, String>>,
    /// Bytes of capture files left in the game folder, measured in the background.
    leftovers: Slot<u64>,
    cleanup: Slot<Result<Removed, String>>,
    /// The dimmer's keys in the installed config, with what each does (the current ones until it's read).
    keys: Vec<(String, &'static str)>,
    /// The installed config still has an earlier version's keys (the light kept on F10, the game's screenshot key).
    keys_outdated: bool,
    /// The dimmer's keys that the game uses too.
    clashes: Vec<Clash>,
    /// Why the game's controls couldn't be read for that check.
    clash_error: Option<String>,
    message: Option<(bool, String)>,
    /// The last calibration, and pixel shaders recorded so far while one runs.
    calibration: Option<Calibration>,
    recorded: u32,
    /// The config still has Elite Nexus 0.3.0's in-game patterns (they crashed the game near the newer
    /// fleet carriers).
    old_patterns: bool,
    /// 3Dmigoto's d3d11.dll isn't there (renamed or deleted): the hook is off.
    hook_missing: bool,
    /// The game was running last frame (a calibration finishes when it closes).
    was_playing: bool,
    /// Choosing the game folder when it wasn't found, and the folder chosen (for the settings).
    picker: crate::folders::Picker,
    pub game_dir_picked: Option<Option<PathBuf>>,
}

impl CockpitUi {
    /// Read the game folder again the next time the tab is shown (another tab may have changed it).
    pub fn reload(&mut self) {
        self.loaded = false;
    }

    fn refresh(&mut self, ctx: &egui::Context, installs: &[GameInstall]) {
        self.loaded = true;
        // Settings changed here but not saved yet survive a reload.
        let unsaved = matches!(&self.status, Some(Status::Installed { config }) if config.dimmer != self.edit);
        self.folder = game_folder(installs);
        self.status = self.folder.as_deref().map(dimmer::status);
        if let Some(Status::Installed { config } | Status::Unmanaged { config }) = &self.status
            && !unsaved
        {
            self.edit = config.dimmer;
        }
        self.leftovers = Slot::default();
        self.keys = current_keys();
        self.keys_outdated = false;
        self.clashes.clear();
        self.clash_error = None;
        self.calibration = None;
        self.recorded = 0;
        self.old_patterns = false;
        self.hook_missing = false;
        if let (Some(folder), Some(Status::Installed { .. })) = (self.folder.clone(), &self.status) {
            // The keys the game has now, which may be an earlier version's until the settings are saved.
            if let Some(bound) = dimmer::bound_keys(&folder) {
                self.keys = dimmer::KEYS.iter().zip(bound).map(|((key, what), b)| (b.unwrap_or_else(|| key.to_string()), *what)).collect();
            }
            self.keys_outdated = dimmer::keys_outdated(&folder);
            let keys: Vec<String> = self.keys.iter().map(|(k, _)| k.clone()).collect();
            (self.clashes, self.clash_error) = key_clashes(&folder, &keys);
            self.calibration = dimmer::read_calibration(&folder);
            self.recorded = dimmer::recorded_shaders(&folder);
            self.old_patterns = dimmer::uses_patterns(&folder);
            self.hook_missing = dimmer::hook_missing(&folder);
            self.leftovers.run(ctx, move || dimmer::leftovers(&folder));
        }
    }

    fn recording(&self) -> bool {
        matches!(&self.status, Some(Status::Installed { config }) if config.recording)
    }
}

/// The dimmer's keys as this version writes them, with what each does.
fn current_keys() -> Vec<(String, &'static str)> {
    dimmer::KEYS.iter().map(|(k, what)| (k.to_string(), *what)).collect()
}

/// "F9, F7 and F8": the keys this version writes.
fn key_names() -> String {
    let keys: Vec<&str> = dimmer::KEYS.iter().map(|(k, _)| *k).collect();
    match keys.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => keys.join(""),
    }
}

/// A dimmer key something else uses too: the game itself (outside its controls), and the actions the commander's
/// controls bind to it on its own.
#[derive(Debug, Clone, PartialEq)]
struct Clash {
    key: String,
    game: Option<&'static str>,
    actions: String,
}

/// The dimmer's `keys` that the game takes for itself or the commander's active control presets bind too. Only the
/// presets the game uses count: when they can't be read, the error says why (no guess from another preset), and the
/// game's own keys are still checked.
fn key_clashes(game: &Path, keys: &[String]) -> (Vec<Clash>, Option<String>) {
    let bindings = || -> Result<Bindings, String> {
        let dir = nexus_binds::default_bindings_dir().ok_or("the game's Options folder wasn't found")?;
        let presets = nexus_binds::find_presets(&dir).map_err(|e| e.to_string())?;
        // Built-in presets (KeyboardMouseOnly, ...) live in the game's ControlSchemes folder.
        let schemes = game.join("ControlSchemes");
        presets.load_effective(&[schemes.as_path()]).map_err(|e| e.to_string())
    };
    match bindings() {
        Ok(b) => (clashes(Some(&b), keys), None),
        Err(e) => (clashes(None, keys), Some(e)),
    }
}

fn clashes(b: Option<&Bindings>, keys: &[String]) -> Vec<Clash> {
    let mut out = Vec::new();
    for key in keys {
        let game = dimmer::GAME_KEYS.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, what)| *what);
        let name = format!("Key_{key}");
        let mut actions: Vec<String> = Vec::new();
        for a in b.iter().flat_map(|b| b.actions.iter()).filter(|a| a.inputs().any(|(_, i)| fires_with(i, &name))) {
            let info = a.info();
            let what = format!("{} ({})", info.name, info.context.label());
            if !actions.contains(&what) {
                actions.push(what);
            }
        }
        if game.is_some() || !actions.is_empty() {
            out.push(Clash { key: key.clone(), game, actions: actions.join(", ") });
        }
    }
    out
}

/// Whether pressing `key` on its own also triggers the binding `i`. 3Dmigoto's keys fire only with
/// no Shift, Ctrl, Alt or Windows key held, so a binding that needs one of those doesn't clash.
fn fires_with(i: &Input, key: &str) -> bool {
    let keyboard = |d: &str| d.eq_ignore_ascii_case("Keyboard");
    let held = i.modifiers.iter().any(|(d, k)| keyboard(d) && ["Shift", "Control", "Alt", "Win"].iter().any(|m| k.ends_with(m)));
    (keyboard(&i.device) && i.key == key && !held) || i.modifiers.iter().any(|(d, k)| keyboard(d) && k == key)
}

fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1u64 << 20) as f64;
    if mb >= 10.0 { format!("{mb:.0} MB") } else { format!("{mb:.1} MB") }
}

/// Downloads the official 3Dmigoto release and unpacks the two DLLs (checked against pinned
/// checksums).
async fn fetch() -> Result<Files, String> {
    let http = reqwest::Client::builder().user_agent(concat!("EliteNexus/", env!("CARGO_PKG_VERSION"))).build().map_err(|e| e.to_string())?;
    let resp = http.get(dimmer::MIGOTO_URL).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || dimmer::extract(&bytes).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

/// The game the commander last played, from the journal: (version, build).
pub type GameBuild<'a> = (&'a str, &'a str);

pub fn show(ui: &mut Ui, st: &mut CockpitUi, installs: &[GameInstall], playing: bool, game: GameBuild, rt: &tokio::runtime::Handle) {
    if !st.loaded {
        st.refresh(ui.ctx(), installs);
    }
    if st.download.tick() {
        match (st.download.last.take(), st.folder.clone()) {
            (Some(Ok(files)), Some(folder)) => {
                let config = HookConfig { dimmer: st.edit, ..HookConfig::default() };
                // A fresh install starts with a calibration: the fixes are made from this game.
                st.message = Some(match dimmer::install(&folder, &files, &config).and_then(|()| dimmer::start_recording(&folder)) {
                    Ok(()) => (true, "Installed. One step left: calibrate (below), so Elite Nexus can find your game's light shader.".into()),
                    Err(e) => (false, e.to_string()),
                });
                st.confirm_install = false;
                st.refresh(ui.ctx(), installs);
            }
            (Some(Err(e)), _) => st.message = Some((false, format!("The download didn't work: {e}"))),
            _ => {}
        }
    }
    // The game closed while a calibration recorded: finish it.
    if st.was_playing && !playing && st.recording() {
        st.refresh(ui.ctx(), installs);
        if st.recorded > 0 {
            finish(st, ui.ctx(), installs, game);
        }
    }
    st.was_playing = playing;
    st.leftovers.tick();
    if st.cleanup.tick() {
        let bytes = st.leftovers.last.unwrap_or(0);
        st.message = match st.cleanup.last.take() {
            Some(Ok(done)) => Some((true, format!("Deleted {} of capture files: {}.", megabytes(bytes), done.removed.join(", ")))),
            Some(Err(e)) => Some((false, format!("Not all capture files could be deleted: {e}"))),
            None => None,
        };
        st.refresh(ui.ctx(), installs);
    }
    widgets::split(ui, 0.5, 400.0, |ui| controls(ui, st, installs, playing, game, rt), explain);
}

/// Finishes a calibration from what the game exported.
fn finish(st: &mut CockpitUi, ctx: &egui::Context, installs: &[GameInstall], (version, build): GameBuild) {
    let Some(folder) = st.folder.clone() else { return };
    st.message = Some(match dimmer::finish_calibration(&folder, version, build) {
        Ok(cal) => {
            let hud = match cal.hud.len() {
                0 => String::new(),
                1 => " and 1 HUD shader".to_string(),
                n => format!(" and {n} HUD shaders"),
            };
            (true, format!("Calibrated: found the cockpit's light shader{hud} among the {} your game loaded. The dimmer works from the next game start.", cal.shaders))
        }
        Err(e) => (false, format!("Not calibrated yet: {e}.")),
    });
    st.refresh(ctx, installs);
}

fn controls(ui: &mut Ui, st: &mut CockpitUi, installs: &[GameInstall], playing: bool, game: GameBuild, rt: &tokio::runtime::Handle) {
    card(ui, "Cockpit-light dimmer", |ui| {
        let Some(folder) = st.folder.clone() else {
            let mut dir = nexus_hud::install::game_dir();
            let why = if dir.is_some() { "The dimmer goes into Elite Dangerous's folder.".to_string() } else { format!("The dimmer goes into Elite Dangerous's folder. {}", crate::folders::game_not_found()) };
            ui.add(egui::Label::new(RichText::new(why).color(pal::DIM)).wrap());
            if crate::folders::choose_game_folder(ui, &mut st.picker, &mut dir) {
                st.game_dir_picked = Some(dir);
            }
            return;
        };
        match st.status.clone() {
            Some(Status::Installed { .. }) => installed(ui, st, &folder, installs, playing, game, false),
            Some(Status::Unmanaged { .. }) => installed(ui, st, &folder, installs, playing, game, true),
            Some(Status::Foreign { edhm }) => {
                let what = if edhm { "EDHM is installed in the game folder." } else { "Another 3Dmigoto mod is installed in the game folder." };
                ui.label(RichText::new(what).font(display(size::BODY)).color(pal::TEXT));
                ui.add(egui::Label::new(RichText::new("Elite Nexus leaves it alone: two mods can't share the game's d3d11.dll. To use Elite Nexus's dimmer instead, remove that mod first.").color(pal::DIM)).wrap());
            }
            Some(Status::None) | None => not_installed(ui, st, playing, rt),
        }
        if let Some((ok, msg)) = &st.message {
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(msg).size(size::FOOTNOTE).color(if *ok { pal::GOOD } else { pal::BAD })).wrap());
        }
        ui.add_space(4.0);
        let path = folder.display().to_string();
        ui.add(egui::Label::new(RichText::new(&path).size(size::FOOTNOTE).color(pal::DIM)).truncate()).on_hover_text(path);
    });
}

/// Linux and macOS: what the game may need to load the hook under Proton or CrossOver. Elite Nexus hasn't been
/// tried there.
const PROTON_NOTE: &str = if cfg!(target_os = "macos") {
    "On macOS this is untested. CrossOver (which NotProton and Whisky build on) runs Direct3D 11 through D3DMetal or DXVK and may skip the d3d11.dll in the game folder: if calibrating finds no shaders, add WINEDLLOVERRIDES=\"d3d11,d3dcompiler_47=n,b\" %command% to Elite Dangerous's launch options in Steam (Properties › General). If it still finds none, the hook can't run there: Remove takes it out again."
} else {
    "On Linux this is untested. Proton brings its own d3d11.dll (DXVK) and may skip the one in the game folder: if calibrating finds no shaders, add WINEDLLOVERRIDES=\"d3d11,d3dcompiler_47=n,b\" %command% to Elite Dangerous's launch options in Steam (Properties › General)."
};

fn not_installed(ui: &mut Ui, st: &mut CockpitUi, playing: bool, rt: &tokio::runtime::Handle) {
    ui.add(egui::Label::new(RichText::new("Dims your cockpit's own lights (lamps, panel glow, indicator lights) and nothing else: stations, carriers, planets and the HUD stay as they are.").color(pal::TEXT)).wrap());
    ui.add_space(6.0);
    if !st.confirm_install {
        if widgets::primary_button(ui, &format!("{} Install the dimmer", icon::DOWNLOAD_SIMPLE)).clicked() {
            st.confirm_install = true;
            st.message = None;
        }
        return;
    }
    section(ui, "Before you install");
    let mut points = vec![
        format!("Elite Nexus downloads 3Dmigoto {} (4.7 MB) from its official GitHub release and checks it against a pinned checksum. 3Dmigoto is an open-source (GPL-3.0) graphics modding tool; Elite Nexus uses it to load its own fixes, nothing else.", dimmer::MIGOTO_VERSION),
        format!("It puts d3d11.dll, d3dcompiler_47.dll, {}, {} and a ShaderFixes folder next to EliteDangerous64.exe, in the folder below. Remove takes them out again.", dimmer::CONFIG, dimmer::MANIFEST),
        "Then you calibrate once: start the game, get into your cockpit, quit. Elite Nexus finds the light shader among the ones your game loaded and keeps a fix for exactly that one.".to_string(),
        format!(
            "While you play, {} (pressed without Shift, Ctrl or Alt) switch and set the dimmer. None of the game's built-in control presets uses them, and the game's screenshot key (F10) is left alone. The game gets those presses too, so a key your own controls use on its own does both; the tab names any it finds.",
            key_names()
        ),
        "No game file is changed: 3Dmigoto swaps the light shader in memory while the game runs. Frontier tolerates 3Dmigoto mods (commanders have used them for years) but has never approved them.".to_string(),
    ];
    if cfg!(not(windows)) {
        points.push(PROTON_NOTE.to_string());
    }
    for p in points {
        ui.add(egui::Label::new(RichText::new(format!("•  {p}")).color(pal::TEXT)).wrap());
    }
    if playing {
        ui.label(RichText::new("The game is running: calibrating starts with its next start, so quit it after installing.").size(size::FOOTNOTE).color(pal::WARN));
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if st.download.busy() {
            ui.spinner();
            ui.label(RichText::new("Downloading and checking 3Dmigoto…").color(pal::DIM));
        } else {
            if widgets::primary_button(ui, &format!("{} Download and install", icon::CHECK)).clicked() {
                st.message = None;
                st.download.run_async(ui.ctx(), rt, fetch());
            }
            if widgets::button(ui, "Cancel").clicked() {
                st.confirm_install = false;
            }
        }
    });
}

/// The install of Elite Nexus 0.3.0 (in-game patterns), or a hook switched off by renaming its DLL:
/// what's wrong and the one click that fixes it. Returns true when something was shown.
fn repairs(ui: &mut Ui, st: &mut CockpitUi, folder: &Path, installs: &[GameInstall]) -> bool {
    if !st.old_patterns && !st.hook_missing {
        return false;
    }
    if st.old_patterns {
        ui.add(
            egui::Label::new(
                RichText::new(format!(
                    "{} This install still uses the patterns of Elite Nexus 0.3.0, which the game runs on every shader it loads. They crash the game when it loads the newer fleet carriers. The update replaces them with fixes for exactly the shaders the dimmer needs, found in one calibration.",
                    icon::WARNING
                ))
                .color(pal::BAD),
            )
            .wrap(),
        );
    }
    if st.hook_missing {
        ui.add(egui::Label::new(RichText::new("3Dmigoto's d3d11.dll isn't in the game folder (renamed or deleted), so the game runs without the hook: no dimmer.").color(pal::WARN)).wrap());
    }
    let label = match (st.old_patterns, st.hook_missing) {
        (true, true) => "Update, put the hook back and calibrate",
        (true, false) => "Update and calibrate",
        _ => "Put the hook back",
    };
    if widgets::primary_button(ui, &format!("{} {label}", icon::WRENCH)).clicked() {
        // The config first: the hook never comes back with the old patterns in place.
        let result = (if st.old_patterns { dimmer::start_recording(folder) } else { Ok(()) }).and_then(|()| if st.hook_missing { dimmer::restore_dll(folder) } else { Ok(true) });
        st.message = Some(match result {
            Ok(true) if st.old_patterns => (true, "Updated. Now calibrate: start the game, get into your cockpit somewhere quiet, then quit to the desktop.".into()),
            Ok(true) => (true, "The hook is back: it runs from the next game start.".into()),
            Ok(false) => (false, "There's no official d3d11.dll to put back (it was deleted or isn't 3Dmigoto's). Remove, then install the dimmer again.".into()),
            Err(e) => (false, e.to_string()),
        });
        st.refresh(ui.ctx(), installs);
    }
    ui.add_space(6.0);
    true
}

/// The last calibration was made for another game build.
fn outdated(st: &CockpitUi, (_, build): GameBuild) -> bool {
    st.calibration.as_ref().is_some_and(|c| !build.trim().is_empty() && !c.game_build.is_empty() && c.game_build != build.trim())
}

/// The calibration: what was found and for which game build, the steps while one records, and the
/// button that starts one.
fn calibration(ui: &mut Ui, st: &mut CockpitUi, folder: &Path, installs: &[GameInstall], playing: bool, game: GameBuild) {
    section(ui, "Calibration");
    if st.recording() {
        let steps = [
            "Start the game (if it's running, quit it first: it reads the setting when it starts).",
            "Get into your cockpit somewhere quiet: docked, or in open space. Stay away from the newer fleet carriers while calibrating.",
            "Quit to the desktop. Elite Nexus finishes the calibration when the game closes (or click Finish).",
        ];
        for (i, s) in steps.iter().enumerate() {
            ui.add(egui::Label::new(RichText::new(format!("{}.  {s}", i + 1)).color(pal::TEXT)).wrap());
        }
        ui.label(RichText::new(format!("The dimmer is off while calibrating. Shaders recorded so far: {}.", st.recorded)).size(size::FOOTNOTE).color(pal::DIM));
        ui.horizontal_wrapped(|ui| {
            let finish_button = ui.add_enabled_ui(st.recorded > 0, |ui| widgets::primary_button(ui, &format!("{} Finish calibration", icon::CHECK))).inner;
            let tip = if playing { "Works while the game runs too, once you have been in your cockpit; the fix takes effect from the next game start." } else { "Uses the shaders the game recorded." };
            if finish_button.on_hover_text(tip).on_disabled_hover_text("Start the game and get into your cockpit first.").clicked() {
                finish(st, ui.ctx(), installs, game);
            }
            if widgets::button(ui, "Cancel").on_hover_text("Stop calibrating; the previous fixes (if any) come back").clicked() {
                st.message = Some(match dimmer::cancel_recording(folder) {
                    Ok(()) => (true, "Calibration cancelled.".into()),
                    Err(e) => (false, e.to_string()),
                });
                st.refresh(ui.ctx(), installs);
            }
        });
        return;
    }
    let (version, build) = game;
    let outdated = outdated(st, game);
    match &st.calibration {
        Some(c) => {
            let when = nexus_journal::Timestamp(c.at).to_local_string("%d %b %Y");
            let hud = match c.hud.len() {
                0 => String::new(),
                1 => ", 1 HUD shader".to_string(),
                n => format!(", {n} HUD shaders"),
            };
            ui.label(RichText::new(format!("Calibrated on {when} for game {} ({}): light shader found{}{hud}.", c.game_version, c.game_build, if c.lights.len() > 1 { format!(" ({} versions)", c.lights.len()) } else { String::new() })).color(pal::DIM));
            if outdated {
                ui.add(
                    egui::Label::new(RichText::new(format!("{} The game has been updated since ({version}, {}). If the update changed the light shader, the dimmer is off until you calibrate again; the game itself is fine either way.", icon::WARNING, build.trim())).color(pal::WARN))
                        .wrap(),
                );
            }
        }
        None => {
            ui.add(egui::Label::new(RichText::new("Not calibrated yet: the dimmer (and the HUD colour protection) stay off until Elite Nexus has found your game's shaders.").color(pal::WARN)).wrap());
        }
    }
    let label = if st.calibration.is_some() { "Calibrate again" } else { "Calibrate" };
    let clicked = if outdated || st.calibration.is_none() { widgets::primary_button(ui, &format!("{} {label}", icon::CROSSHAIR)) } else { widgets::button(ui, &format!("{} {label}", icon::CROSSHAIR)) }
        .on_hover_text("For one game session the game records the shaders it loads; Elite Nexus then finds the ones it needs")
        .clicked();
    if clicked {
        st.message = Some(match dimmer::start_recording(folder) {
            Ok(()) => (true, "Calibrating: follow the steps above.".into()),
            Err(e) => (false, e.to_string()),
        });
        st.refresh(ui.ctx(), installs);
    }
}

fn installed(ui: &mut Ui, st: &mut CockpitUi, folder: &Path, installs: &[GameInstall], playing: bool, game: GameBuild, unmanaged: bool) {
    if unmanaged {
        ui.label(RichText::new("The test version you put in by hand is installed.").font(display(size::BODY)).color(pal::TEXT));
        ui.add(
            egui::Label::new(
                RichText::new("Let Elite Nexus manage it: it replaces the test's copy of the game's shader with a calibrated fix, keeps your settings and can remove it again. Nothing is downloaded.")
                    .color(pal::DIM),
            )
            .wrap(),
        );
        if widgets::primary_button(ui, &format!("{} Let Elite Nexus manage it", icon::CHECK)).clicked() {
            let protect = match &st.status {
                Some(Status::Unmanaged { config }) => config.protect,
                _ => Default::default(),
            };
            st.message = Some(match dimmer::install(folder, &[], &HookConfig { dimmer: st.edit, protect, recording: false }).and_then(|()| dimmer::start_recording(folder)) {
                Ok(()) => (true, "Done: Elite Nexus manages the dimmer now. One step left: calibrate (below).".into()),
                Err(e) => (false, e.to_string()),
            });
            st.refresh(ui.ctx(), installs);
        }
        return;
    }
    let on = st.edit.on;
    ui.horizontal(|ui| {
        ui.label(RichText::new(if on { icon::LIGHTBULB_FILAMENT } else { icon::LIGHTBULB }).size(18.0).color(if on { pal::ACCENT } else { pal::DIM }));
        ui.label(RichText::new("Installed").font(display(size::BODY)).color(pal::TEXT));
    });
    ui.add_space(4.0);
    if repairs(ui, st, folder, installs) {
        return;
    }
    // A calibration to do (or under way) comes first; a done one goes below the settings.
    let urgent = st.recording() || st.calibration.is_none() || outdated(st, game);
    if urgent {
        calibration(ui, st, folder, installs, playing, game);
        ui.add_space(6.0);
    }
    // An earlier version's keys count as unsaved too: saving writes the current ones.
    let saved = matches!(&st.status, Some(Status::Installed { config }) if config.dimmer == st.edit) && !st.keys_outdated;
    // HUD colours' "Keep hostile red..." runs on the same install.
    let protecting = matches!(&st.status, Some(Status::Installed { config }) if config.protect.on);
    group(ui, |g| {
        g.switch(&mut st.edit.on, "Dim the cockpit lights", "Off keeps 3Dmigoto installed but changes nothing.");
        g.row("Light kept", "0 % switches the cockpit lights off.", |ui| {
            let mut pct = st.edit.strength * 100.0;
            if slider(ui, &mut pct, 0.0..=100.0, 0, " %") {
                st.edit.strength = (pct / 100.0).clamp(0.0, 1.0);
            }
        });
        g.row("Reach", "Lights closer to your eyes than this are dimmed. About 5 m covers most cockpits; big ships need more.", |ui| {
            if slider(ui, &mut st.edit.reach, 2.0..=15.0, 1, " m") {
                st.edit.reach = (st.edit.reach * 2.0).round() / 2.0;
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if !saved && widgets::primary_button(ui, &format!("{} Save", icon::CHECK)).clicked() {
            // Read what's there now, so the HUD colours' settings in the same file are kept.
            let result = match dimmer::status(folder) {
                Status::Installed { config } => dimmer::save_config(folder, &HookConfig { dimmer: st.edit, ..config }),
                _ => Err(dimmer::DimmerError::NotOurs),
            };
            st.message = Some(match result {
                Ok(()) => (true, format!("Saved. The game uses it the next time it starts; {} change it while you play.", key_names())),
                Err(e) => (false, e.to_string()),
            });
            st.refresh(ui.ctx(), installs);
        }
        if !st.confirm_remove {
            if widgets::button(ui, &format!("{} Remove", icon::TRASH)).clicked() {
                st.confirm_remove = true;
            }
        } else {
            let question = if protecting {
                "Remove 3Dmigoto from the game folder? The dimmer goes, and HUD colours stops keeping hostile red, friendly green and shield blue."
            } else {
                "Remove 3Dmigoto and the dimmer from the game folder?"
            };
            ui.add(egui::Label::new(RichText::new(question).color(pal::WARN)).wrap());
            if widgets::button(ui, "Remove").clicked() {
                st.message = Some(match dimmer::uninstall(folder) {
                    Ok(done) if done.kept.is_empty() && protecting => (true, "Removed. From the next game start the cockpit lights are back to normal and your HUD theme colours every HUD colour again.".into()),
                    Ok(done) if done.kept.is_empty() => (true, "Removed. The cockpit lights are back to normal the next time the game starts.".into()),
                    Ok(done) => (false, format!("Removed, except {}: changed since Elite Nexus installed it, so it was left in place.", done.kept.join(", "))),
                    Err(e) if playing => (false, format!("{e}. Quit the game first: it keeps these files open.")),
                    Err(e) => (false, e.to_string()),
                });
                st.confirm_remove = false;
                st.refresh(ui.ctx(), installs);
            }
            if widgets::button(ui, "Cancel").clicked() {
                st.confirm_remove = false;
            }
        }
    });
    if !saved {
        ui.label(RichText::new("Not saved yet.").size(size::FOOTNOTE).color(pal::WARN));
    }
    if !urgent {
        calibration(ui, st, folder, installs, playing, game);
    }
    let leftover = st.leftovers.last.unwrap_or(0);
    if leftover > 0 && !st.cleanup.busy() && !st.recording() {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("Capture files from the setup are still in the game folder ({}).", megabytes(leftover))).color(pal::DIM));
            let tip = "Frame captures (FrameAnalysis-…) and the game's shaders as 3Dmigoto exported them (ShaderCache), written while the dimmer was being worked out. The dimmer doesn't use them.";
            if widgets::button(ui, &format!("{} Delete them", icon::TRASH)).on_hover_text(tip).clicked() {
                let folder = folder.to_path_buf();
                st.message = None;
                st.cleanup.run(ui.ctx(), move || dimmer::remove_leftovers(&folder).map_err(|e| e.to_string()));
            }
        });
    } else if st.cleanup.busy() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("Deleting the capture files…").color(pal::DIM));
        });
    }
    section(ui, "Keys in the game");
    for (key, what) in &st.keys {
        let clash = st.clashes.iter().find(|c| c.key == *key);
        ui.horizontal(|ui| {
            widgets::badge(ui, key, if clash.is_some() { pal::WARN } else { pal::ACCENT });
            ui.label(RichText::new(*what).color(pal::DIM));
        });
        let warn = |ui: &mut Ui, text: String| {
            ui.add(egui::Label::new(RichText::new(format!("{}  {text}", icon::WARNING)).size(size::FOOTNOTE).color(pal::WARN)).wrap());
        };
        if let Some(game) = clash.and_then(|c| c.game) {
            warn(ui, format!("{key} is also {game}, which isn't among its controls: pressing it does both."));
        }
        if let Some(c) = clash.filter(|c| !c.actions.is_empty()) {
            warn(ui, format!("{key} is bound in your game controls too: {}. Pressing it does both, so give the game's action another key (Options › Controls in the game).", c.actions));
        }
    }
    if st.keys_outdated {
        ui.add(
            egui::Label::new(
                RichText::new(format!(
                    "{}  These are an earlier version's keys: the light kept was on F10, the game's screenshot key, so every screenshot changed it too. Save to move the keys to {}: F7 steps the light kept, F8 the reach, F9 switches it on and off.",
                    icon::WARNING,
                    key_names()
                ))
                .size(size::FOOTNOTE)
                .color(pal::WARN),
            )
            .wrap(),
        );
    }
    if let Some(e) = &st.clash_error {
        ui.add(egui::Label::new(RichText::new(format!("{}  Couldn't check these keys against the control presets the game uses ({e}).", icon::WARNING)).size(size::FOOTNOTE).color(pal::WARN)).wrap());
    }
    footnote(ui, "The keys work while the game window is in front, without Shift, Ctrl or Alt held; what they set lasts until the game closes. Elite Nexus can't keep a key from the game: the game gets every press too. It never changes your controls.");
}

fn explain(ui: &mut Ui) {
    card(ui, "What the dimmer does, and what it can't", |ui| {
        let mut lines = vec![
            "The game has no setting for its cockpit lights. 3Dmigoto, a Direct3D 11 hook that sits next to the game, lets Elite Nexus change one step of how the game lights the cockpit: lights within the reach of your eyes keep only part of their light.",
            "Only the cockpit's own lights change. Stations, carriers, planets, the HUD and everything else stay exactly as the game draws them.",
            "Calibration: for one session the game records the shaders it loads; Elite Nexus finds the light shader among them by what its code looks like and keeps a fix for exactly that shader, by its fingerprint. In the game, 3Dmigoto only swaps that one shader: every other shader, including ones the game adds later (new ships, new carriers), is left alone.",
            "After a game update that changes the light shader, the fingerprint no longer matches: the dimmer simply stays off until you calibrate again, and Elite Nexus tells you when the game has been updated. The game is never affected.",
            "The fixes are made on your PC from your game and never leave it.",
            "Settings saved here apply the next time the game starts; F9, F7 and F8 change them while you play, and the game gets those presses too.",
            "The same install is the graphics hook HUD colours needs to keep hostile red, friendly green and shield blue out of a theme. If you only want that, switch the dimmer off here.",
            "It can't run alongside EDHM or another 3Dmigoto mod: they all need the game's d3d11.dll.",
            "Frontier tolerates mods like this but has never approved them.",
        ];
        if cfg!(not(windows)) {
            lines.push(PROTON_NOTE);
        }
        for l in lines {
            ui.add(egui::Label::new(RichText::new(format!("•  {l}")).color(pal::TEXT)).wrap());
        }
        ui.add_space(4.0);
        ui.hyperlink_to(format!("{} 3Dmigoto on GitHub", icon::ARROW_SQUARE_OUT), dimmer::MIGOTO_SOURCE);
    });
}

// A settings-row slider with its value on the left.
fn slider(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, decimals: usize, suffix: &str) -> bool {
    ui.add_sized([56.0, 20.0], egui::Label::new(RichText::new(format!("{:.*}{suffix}", decimals, *value)).color(pal::DIM)));
    ui.add(egui::Slider::new(value, range).show_value(false)).changed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_dimmer_keys_the_game_uses_too() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" ?>
<Root PresetName="Test" MajorVersion="4" MinorVersion="2">
	<GalaxyMapOpen>
		<Primary Device="Keyboard" Key="Key_F10" />
		<Secondary Device="{NoDevice}" Key="" />
	</GalaxyMapOpen>
	<SystemMapOpen>
		<Primary Device="Keyboard" Key="Key_F9">
			<Modifier Device="Keyboard" Key="Key_LeftShift" />
		</Primary>
		<Secondary Device="{NoDevice}" Key="" />
	</SystemMapOpen>
	<FocusLeftPanel>
		<Primary Device="Keyboard" Key="Key_1">
			<Modifier Device="Keyboard" Key="Key_F11" />
		</Primary>
		<Secondary Device="Keyboard" Key="Key_F12" />
	</FocusLeftPanel>
</Root>
"#;
        let b = Bindings::parse(xml).unwrap();
        let keys = |k: &[&str]| k.iter().map(|k| k.to_string()).collect::<Vec<_>>();
        // An earlier version's keys: F9 needs Shift in the game, and the dimmer's keys fire only without modifiers, so no
        // clash; F10 is the game's screenshot key besides the binding.
        let old = keys(&["F9", "F10", "F11"]);
        assert_eq!(
            clashes(Some(&b), &old),
            vec![
                Clash { key: "F10".into(), game: Some("the game's screenshot key"), actions: "Open Galaxy Map (Ship)".into() },
                Clash { key: "F11".into(), game: None, actions: "Left Panel (Navigation) (Ship)".into() },
            ]
        );
        assert_eq!(clashes(None, &old).len(), 1, "the game's own key, even when the controls can't be read");
        // The keys now clash with nothing the game does by itself or binds by default.
        let now: Vec<String> = current_keys().into_iter().map(|(k, _)| k).collect();
        assert!(clashes(Some(&Bindings::default()), &now).is_empty());
        assert_eq!(key_names(), "F9, F7 and F8");
    }
}
