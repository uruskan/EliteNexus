//! HUD & cockpit. HUD colours: the game's own colour matrix (`GraphicsConfigurationOverride.xml`)
//! with presets, simple controls, a live preview and the honest limits of what the matrix can
//! change, plus "Keep hostile red, friendly green and shield blue" through Elite Nexus's graphics
//! hook (`nexus_hud::dimmer`). Glare and exposure (glare.rs): the tone-mapping values in the same
//! file. Cockpit lights (cockpit.rs): the dimmer, which installs the hook.

use crate::settings::HudTheme;
use crate::theme::{display, pal, size};
use crate::widgets::{self, Group, card, card_with, footnote, group, section};
use egui::text::LayoutJob;
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, TextFormat, Ui, Vec2, pos2, vec2};
use egui_phosphor::regular as icon;
use nexus_hud::dimmer::{self, HookConfig, Protect, Status};
use nexus_hud::install::GameInstall;
use nexus_hud::{Adjust, HudMatrix, OverrideFile, palette, presets};
use std::cell::Cell;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Simple,
    Matrix,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Yours,
    Standard,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Colours,
    Glare,
    Cockpit,
}

pub struct HudUi {
    tab: Tab,
    glare: super::glare::GlareUi,
    cockpit: super::cockpit::CockpitUi,
    /// The override file as last read or written.
    file: Option<Result<OverrideFile, String>>,
    /// The colours the game will use from the file: `Ok(None)` = standard.
    current: Result<Option<HudMatrix>, String>,
    /// Game folders found, to warn when EDHM (which needs the standard matrix) is installed.
    installs: Vec<GameInstall>,
    /// The theme being edited is `base` with `adjust` on top.
    base: HudMatrix,
    adjust: Adjust,
    mode: Mode,
    view: View,
    status: Option<(bool, String)>,
    paste: String,
    theme_name: String,
    more: bool,
    /// `--tab <preset>` (screenshots): start from this preset instead of the file.
    start_preset: Option<String>,
    /// `--tab hue:<degrees>` (screenshots): the Colour slider, set on top of the theme started from.
    start_hue: Option<f32>,
    /// The game folder and Elite Nexus's graphics hook in it (the Cockpit lights tab installs it),
    /// which "Keep hostile red, friendly green and shield blue" needs.
    hook_folder: Option<PathBuf>,
    hook: Option<Status>,
    /// The last calibration found the HUD shaders (their fixes exist).
    hook_hud_ready: bool,
    /// The protection as shown; saved to the game's config as soon as a change settles.
    protect: Protect,
    protect_options: bool,
    protect_status: Option<(bool, String)>,
    /// The game folder chosen in Settings when the game folders were last looked for.
    game_dir_seen: Option<PathBuf>,
    /// Choosing the game folder when it wasn't found, and the folder chosen (for the settings).
    picker: crate::folders::Picker,
    game_dir_picked: Option<Option<PathBuf>>,
}

impl Default for HudUi {
    fn default() -> Self {
        HudUi {
            tab: Tab::Colours,
            glare: Default::default(),
            cockpit: Default::default(),
            file: None,
            current: Ok(None),
            installs: Vec::new(),
            base: HudMatrix::IDENTITY,
            adjust: Adjust::default(),
            mode: Mode::Simple,
            view: View::Yours,
            status: None,
            paste: String::new(),
            theme_name: String::new(),
            more: false,
            start_preset: None,
            start_hue: None,
            hook_folder: None,
            hook: None,
            hook_hud_ready: false,
            protect: Protect::default(),
            protect_options: false,
            protect_status: None,
            game_dir_seen: None,
            picker: Default::default(),
            game_dir_picked: None,
        }
    }
}

impl HudUi {
    /// Opens the Cockpit lights tab (from the notice about an old dimmer install).
    pub fn show_cockpit(&mut self) {
        self.tab = Tab::Cockpit;
    }

    /// `tab` (screenshots): `glare` opens Glare and exposure, `cockpit` Cockpit lights; a preset name
    /// such as `mocha` or `royalpurple` starts the colours from that theme; `hue:<degrees>` (0 to 360)
    /// sets the Colour slider on top of it; `options` opens the protection's options. Combine them with
    /// commas: `empireblue,options`, `mocha,hue:120`. Nothing is written to the game.
    pub fn new(tab: Option<&str>) -> Self {
        let mut st = HudUi::default();
        for t in tab.unwrap_or_default().split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let lower = t.to_ascii_lowercase();
            if let Some(hue) = lower.strip_prefix("hue:") {
                // A hue that isn't a number is left out, not taken for a theme's name.
                st.start_hue = hue.trim().parse::<f32>().ok().filter(|h| h.is_finite()).map(|h| h.clamp(0.0, 360.0));
                continue;
            }
            match lower.as_str() {
                "glare" => st.tab = Tab::Glare,
                "cockpit" => st.tab = Tab::Cockpit,
                "options" => st.protect_options = true,
                _ => st.start_preset = Some(t.to_owned()),
            }
        }
        st
    }

    /// Screenshots: the theme asked for with `--tab`, then the hue on top of it (starting from a theme resets the
    /// simple controls, so the hue comes second).
    fn start_as_asked(&mut self) {
        let wanted = self.start_preset.take().map(|t| t.replace([' ', '-', '_'], "").to_ascii_lowercase());
        if let Some(p) = wanted.and_then(|t| presets::ALL.iter().find(|p| p.name.replace(' ', "").eq_ignore_ascii_case(&t))) {
            self.start_from(p.matrix);
        }
        if let Some(hue) = self.start_hue.take() {
            self.adjust.hue = hue;
        }
    }

    /// Reads what's in the game folder: is the graphics hook there, and how is the protection set.
    fn read_hook(&mut self) {
        self.hook_folder = super::cockpit::game_folder(&self.installs);
        self.hook = self.hook_folder.as_deref().map(dimmer::status);
        self.hook_hud_ready = self.hook_folder.as_deref().and_then(dimmer::read_calibration).is_some_and(|c| !c.hud.is_empty());
        if let Some(Status::Installed { config }) = &self.hook {
            self.protect = config.protect;
        }
    }

    /// The protection as the game will apply it: `None` while it's off or can't run.
    fn protection(&self) -> Option<Protect> {
        (matches!(self.hook, Some(Status::Installed { .. })) && self.protect.on).then_some(self.protect)
    }

    fn saved_protect(&self) -> Option<Protect> {
        match &self.hook {
            Some(Status::Installed { config }) => Some(config.protect),
            _ => None,
        }
    }

    /// Writes the protection into the game's 3Dmigoto config, keeping the dimmer's settings there.
    fn save_protect(&mut self) {
        let Some(folder) = self.hook_folder.clone() else { return };
        let before = self.saved_protect();
        let result = match dimmer::status(&folder) {
            Status::Installed { config } => dimmer::save_config(&folder, &HookConfig { protect: self.protect, ..config }),
            _ => Err(dimmer::DimmerError::NotOurs),
        };
        self.protect_status = Some(match result {
            Ok(()) => (
                true,
                match (before.is_some_and(|b| b.on), self.protect.on) {
                    (false, true) => "Saved. From its next start the game keeps hostile red, friendly green and shield blue, and your theme colours only the HUD's orange.",
                    (true, false) => "Saved. From its next start the game themes every HUD colour again.",
                    _ => "Saved. The game uses these shares from its next start.",
                }
                .into(),
            ),
            Err(e) => (false, e.to_string()),
        });
        // Show what's really in the file now (the old settings, if saving failed).
        self.read_hook();
    }

    fn edit(&self) -> HudMatrix {
        self.base.then(&HudMatrix::from_adjust(&self.adjust))
    }

    fn start_from(&mut self, m: HudMatrix) {
        self.base = m;
        self.adjust = Adjust::default();
    }

    /// Reads the override file and starts editing what it holds; looks for the game's folders.
    fn load(&mut self) {
        let file = nexus_hud::default_graphics_dir().ok_or_else(|| nexus_hud::HudError::NoOptionsDir.to_string()).and_then(|d| OverrideFile::load(&d).map_err(|e| e.to_string()));
        self.current = match &file {
            Ok(f) => f.matrix().map_err(|e| e.to_string()),
            Err(e) => Err(e.clone()),
        };
        self.start_from(self.current.clone().ok().flatten().unwrap_or(HudMatrix::IDENTITY));
        self.file = Some(file);
        self.find_game();
        self.start_as_asked();
    }

    /// Looks for the game folders (the one chosen in Settings, else every one found) and what's in them.
    fn find_game(&mut self) {
        self.game_dir_seen = nexus_hud::install::game_dir();
        self.installs = nexus_hud::install::installs();
        self.read_hook();
        self.glare.reload(self.file.as_ref(), &self.installs);
        self.cockpit.reload();
    }

    fn apply(&mut self) {
        let m = self.edit();
        let Some(Ok(file)) = self.file.as_mut() else { return };
        // The standard matrix is written as "no override", which leaves the file as the game made it.
        let target = (!m.is_identity()).then_some(m);
        self.status = Some(match file.save(target.as_ref()) {
            Ok(()) => {
                self.current = Ok(target);
                (true, if target.is_some() { "Saved. Elite Dangerous shows the new colours the next time it starts.".into() } else { "Back to the standard orange the next time Elite Dangerous starts.".into() })
            }
            Err(e) => (false, e.to_string()),
        });
    }

    /// The game folder with EDHM switched on, if any.
    fn edhm(&self) -> Option<&GameInstall> {
        self.installs.iter().find(|i| i.shader_mod.is_some_and(|m| m.edhm && m.enabled))
    }
}

/// Returns true when the saved themes or the game folder changed (settings to save).
pub fn show(ui: &mut Ui, st: &mut HudUi, themes: &mut Vec<HudTheme>, game_dir: &mut Option<PathBuf>, playing: bool, game: super::cockpit::GameBuild, rt: &tokio::runtime::Handle) -> bool {
    if st.file.is_none() {
        st.load();
    } else if st.game_dir_seen != nexus_hud::install::game_dir() {
        // Chosen in Settings or on one of these tabs.
        st.find_game();
    }
    if let Some(t) = widgets::tabs(ui, st.tab, &[(Tab::Colours, "HUD colours"), (Tab::Glare, "Glare and exposure"), (Tab::Cockpit, "Cockpit lights")]) {
        if t != st.tab {
            // HUD colours and Cockpit lights share the graphics hook: each shows what the other did.
            st.read_hook();
            st.cockpit.reload();
        }
        st.tab = t;
    }
    ui.add_space(widgets::GAP);
    let mut changed = match st.tab {
        Tab::Colours => colours(ui, st, themes, playing),
        Tab::Glare => {
            super::glare::show(ui, &mut st.glare, &mut st.file, playing);
            false
        }
        Tab::Cockpit => {
            super::cockpit::show(ui, &mut st.cockpit, &st.installs, playing, game, rt);
            false
        }
    };
    for picked in [st.game_dir_picked.take(), st.cockpit.game_dir_picked.take()].into_iter().flatten() {
        *game_dir = picked;
        changed = true;
    }
    changed
}

fn colours(ui: &mut Ui, st: &mut HudUi, themes: &mut Vec<HudTheme>, playing: bool) -> bool {
    let mut changed = false;
    let edit = st.edit();
    let protect = st.protection();
    let view = Cell::new(st.view);
    widgets::split(
        ui,
        0.56,
        400.0,
        |ui| {
            card_with(
                ui,
                "Preview",
                |ui| {
                    let mut v = view.get();
                    widgets::segmented(ui, &mut v, &[(View::Yours, "Your colours"), (View::Standard, "Standard")]);
                    view.set(v);
                },
                |ui| {
                    let shown = if view.get() == View::Standard { HudMatrix::IDENTITY } else { edit };
                    preview(ui, &shown, protect.as_ref());
                    ui.add_space(8.0);
                    swatches(ui, &shown, protect.as_ref());
                    let worries = palette::concerns(&edit, protect.as_ref());
                    if !worries.is_empty() {
                        ui.add_space(6.0);
                        for w in worries {
                            ui.add(egui::Label::new(RichText::new(format!("{}  {w}", icon::WARNING)).size(size::FOOTNOTE).color(pal::WARN)).wrap());
                        }
                    }
                },
            );
            footnote(ui, "An impression, not a screenshot: the game draws its HUD with glow and transparency, and Odyssey renders some colours (reds especially) a little differently.");
            ui.add_space(widgets::GAP);
            limits_card(ui);
        },
        |ui| {
            changed |= controls(ui, st, themes, playing, &edit);
        },
    );
    st.view = view.get();
    changed
}

fn controls(ui: &mut Ui, st: &mut HudUi, themes: &mut Vec<HudTheme>, playing: bool, edit: &HudMatrix) -> bool {
    let mut changed = false;
    card(ui, "In the game", |ui| {
        match &st.current {
            Ok(None) => {
                ui.horizontal(|ui| {
                    dot(ui, colour(&HudMatrix::IDENTITY, palette::ORANGE.rgb), 7.0);
                    ui.label(RichText::new("Standard orange").font(display(size::BODY)).color(pal::TEXT));
                });
            }
            Ok(Some(m)) => {
                ui.horizontal(|ui| {
                    dot(ui, colour(m, palette::ORANGE.rgb), 7.0);
                    let name = presets::ALL.iter().find(|p| p.matrix.approx_eq(m)).map(|p| p.name).or_else(|| themes.iter().find(|t| t.matrix.approx_eq(m)).map(|t| t.name.as_str())).unwrap_or("Your own colours");
                    ui.label(RichText::new(name).font(display(size::BODY)).color(pal::TEXT));
                });
            }
            Err(e) => {
                ui.add(egui::Label::new(RichText::new(format!("{}  {e}", icon::WARNING)).color(pal::BAD)).wrap());
                if matches!(st.file, Some(Ok(_))) {
                    ui.add(egui::Label::new(RichText::new("The game most likely ignores a file it can't read, too. Elite Nexus won't change it: fix or delete it, then reload.").size(size::FOOTNOTE).color(pal::DIM)).wrap());
                }
            }
        }
        if let Some(Ok(f)) = &st.file {
            let path = f.path.display().to_string();
            ui.add(egui::Label::new(RichText::new(&path).size(size::FOOTNOTE).color(pal::DIM)).truncate()).on_hover_text(path);
        }
        if let Some(g) = st.edhm() {
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "{}  EDHM is installed ({} game folder). It colours the HUD with its own shaders and needs this setting left at the standard orange: use EDHM's own colour settings instead, and expect EDHM-UI to reset what you apply here.",
                        icon::WARNING,
                        g.store
                    ))
                    .size(size::FOOTNOTE)
                    .color(pal::WARN),
                )
                .wrap(),
            )
            .on_hover_text(g.folder.display().to_string());
        }
        let unsaved = st.current.as_ref().ok().is_some_and(|c| !c.unwrap_or(HudMatrix::IDENTITY).approx_eq(edit));
        ui.add_space(4.0);
        let can_save = matches!(st.file, Some(Ok(_))) && st.current.is_ok();
        ui.horizontal_wrapped(|ui| {
            if unsaved && can_save {
                if widgets::primary_button(ui, &format!("{} Apply to Elite Dangerous", icon::CHECK)).clicked() {
                    st.apply();
                }
                if widgets::button(ui, "Undo changes").clicked() {
                    st.start_from(st.current.clone().ok().flatten().unwrap_or(HudMatrix::IDENTITY));
                    st.status = None;
                }
            } else if can_save {
                ui.label(RichText::new("Pick a theme or adjust the colours, then apply them here.").size(size::FOOTNOTE).color(pal::DIM));
            }
            if widgets::icon_button(ui, icon::ARROW_CLOCKWISE, "Read the file again").clicked() {
                st.load();
                st.status = None;
            }
        });
        if let Some((ok, msg)) = &st.status {
            ui.add(egui::Label::new(RichText::new(msg).size(size::FOOTNOTE).color(if *ok { pal::GOOD } else { pal::BAD })).wrap());
        } else if unsaved && can_save {
            ui.label(RichText::new("Not applied yet.").size(size::FOOTNOTE).color(pal::WARN));
        }
        let restart = if playing { "The game is running: it reads these colours only when it starts, so they appear after you quit to the desktop and start it again." } else { "The game reads these colours when it starts." };
        ui.add(egui::Label::new(RichText::new(restart).size(size::FOOTNOTE).color(pal::DIM)).wrap());
        if let Some(Ok(f)) = &st.file
            && f.backup_path().exists() {
                let b = f.backup_path().display().to_string();
                ui.add(egui::Label::new(RichText::new("Your file as it was before Elite Nexus first changed it is kept next to it.").size(size::FOOTNOTE).color(pal::DIM)).wrap()).on_hover_text(b);
            }
        ui.add_space(8.0);
        protect_controls(ui, st);
    });
    ui.add_space(widgets::GAP);
    card(ui, "Start from", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for p in presets::ALL {
                let selected = st.base.approx_eq(&p.matrix) && st.adjust == Adjust::default();
                if chip(ui, p.name, colour(&p.matrix, palette::ORANGE.rgb), selected).on_hover_text(p.note).clicked() {
                    st.start_from(p.matrix);
                }
            }
            let mut remove = None;
            for (i, t) in themes.iter().enumerate() {
                let selected = st.base.approx_eq(&t.matrix) && st.adjust == Adjust::default();
                let r = chip(ui, &t.name, colour(&t.matrix, palette::ORANGE.rgb), selected).on_hover_text("Your theme. Right-click to delete it.");
                if r.clicked() {
                    st.start_from(t.matrix);
                }
                r.context_menu(|ui| {
                    if ui.button(format!("{} Delete \"{}\"", icon::TRASH, t.name)).clicked() {
                        remove = Some(i);
                        ui.close();
                    }
                });
            }
            if let Some(i) = remove {
                themes.remove(i);
                changed = true;
            }
        });
    });
    ui.add_space(widgets::GAP);
    let shown_mode = st.mode;
    let picked = Cell::new(None);
    card_with(
        ui,
        "Adjust",
        |ui| {
            let mut mode = shown_mode;
            if widgets::segmented(ui, &mut mode, &[(Mode::Simple, "Simple"), (Mode::Matrix, "Matrix")]) {
                picked.set(Some(mode));
            }
        },
        |ui| {
            if let Some(mode) = picked.take() {
                if mode == Mode::Matrix {
                    // The matrix view edits the combined result directly.
                    st.start_from(st.edit());
                }
                st.mode = mode;
            }
            match st.mode {
                Mode::Simple => simple(ui, st),
                Mode::Matrix => matrix_grid(ui, st),
            }
        },
    );
    ui.add_space(widgets::GAP);
    card(ui, "Share and keep", |ui| {
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 150.0).max(120.0);
            ui.add(egui::TextEdit::singleline(&mut st.theme_name).hint_text("Name this theme").desired_width(w).margin(egui::Margin::symmetric(10, 6)));
            let name = st.theme_name.trim().to_string();
            if ui.add_enabled(!name.is_empty(), egui::Button::new("Save theme").corner_radius(CornerRadius::same(255)).min_size(vec2(0.0, 30.0))).clicked() {
                match themes.iter_mut().find(|t| t.name == name) {
                    Some(t) => t.matrix = *edit,
                    None => themes.push(HudTheme { name, matrix: *edit }),
                }
                st.theme_name.clear();
                changed = true;
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if widgets::button(ui, &format!("{} Copy as XML", icon::COPY)).on_hover_text("The three <Matrix…> lines commanders share on forums").clicked() {
                ui.ctx().copy_text(edit.to_snippet());
                st.status = Some((true, "Copied the three matrix lines to the clipboard.".into()));
            }
        });
        widgets::disclosure(ui, &mut st.more, "Use a theme someone shared");
        if st.more {
            ui.add(egui::TextEdit::multiline(&mut st.paste).hint_text("Paste the <MatrixRed>, <MatrixGreen> and <MatrixBlue> lines").desired_rows(3).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
            ui.horizontal(|ui| {
                let parsed = HudMatrix::from_snippet(&st.paste);
                if ui.add_enabled(parsed.is_some(), egui::Button::new("Use it").corner_radius(CornerRadius::same(255)).min_size(vec2(0.0, 30.0))).clicked()
                    && let Some(m) = parsed {
                        st.start_from(m);
                        st.mode = Mode::Matrix;
                        st.paste.clear();
                    }
                if !st.paste.trim().is_empty() && parsed.is_none() {
                    ui.label(RichText::new("No <MatrixRed>-style lines with three numbers found.").size(size::FOOTNOTE).color(pal::WARN));
                }
            });
        }
    });
    changed
}

/// Hue, saturation and brightness on top of the theme picked under "Start from".
fn simple(ui: &mut Ui, st: &mut HudUi) {
    ui.label(RichText::new("Colour").color(pal::TEXT));
    hue_strip(ui, st);
    ui.add_space(6.0);
    group(ui, |g| {
        g.row("Saturation", "", |ui| {
            let mut pct = st.adjust.saturation * 100.0;
            if slider(ui, &mut pct, 0.0..=150.0, " %") {
                st.adjust.saturation = pct / 100.0;
            }
        });
        g.row("Brightness", "Below 100 % dims the whole HUD", |ui| {
            let mut pct = st.adjust.brightness * 100.0;
            if slider(ui, &mut pct, 20.0..=150.0, " %") {
                st.adjust.brightness = pct / 100.0;
            }
        });
    });
    if st.adjust != Adjust::default() && widgets::button(ui, "Reset adjustments").clicked() {
        st.adjust = Adjust::default();
    }
    if st.protection().is_some() {
        footnote(ui, "Turning the colour moves the HUD's orange round the wheel; hostile red, friendly green and shield blue keep their colours while \"Keep hostile red…\" is on.");
    } else {
        footnote(ui, "Turning the colour moves every HUD colour round the wheel together, so hostile red and shield blue change too: check them in the preview.");
    }
}

/// "Keep hostile red, friendly green and shield blue": needs Elite Nexus's graphics hook; saved to
/// the game's 3Dmigoto config as soon as a change settles, and read by the game when it starts.
fn protect_controls(ui: &mut Ui, st: &mut HudUi) {
    const TITLE: &str = "Keep hostile red, friendly green and shield blue";
    let open_cockpit = group(ui, |g| match &st.hook {
        Some(Status::Installed { .. }) => {
            g.switch(&mut st.protect.on, TITLE, "Your theme then colours only the HUD's orange.");
            false
        }
        Some(Status::None) => why_not(g, TITLE, "Needs Elite Nexus's graphics hook, which the Cockpit lights tab installs. Its dimmer can be switched off there if you only want this.", true),
        Some(Status::Unmanaged { .. }) => why_not(g, TITLE, "Needs Elite Nexus to manage the graphics hook in the game folder: Cockpit lights › Let Elite Nexus manage it.", true),
        Some(Status::Foreign { edhm: true }) => why_not(g, TITLE, "Can't be used with EDHM: it needs Elite Nexus's graphics hook, and EDHM already holds the game's d3d11.dll.", false),
        Some(Status::Foreign { edhm: false }) => why_not(g, TITLE, "Can't be used with the other 3Dmigoto mod in the game folder: it needs Elite Nexus's graphics hook, and the two can't share the game's d3d11.dll.", false),
        None if nexus_hud::install::game_dir().is_some() => why_not(g, TITLE, "Needs Elite Dangerous's folder.", false),
        None => why_not(g, TITLE, &format!("Needs Elite Dangerous's folder. {}", crate::folders::game_not_found()), false),
    });
    if open_cockpit {
        st.tab = Tab::Cockpit;
        st.cockpit.reload();
    }
    if st.hook.is_none() {
        let mut dir = nexus_hud::install::game_dir();
        if crate::folders::choose_game_folder(ui, &mut st.picker, &mut dir) {
            st.game_dir_picked = Some(dir);
        }
        return;
    }
    if !matches!(st.hook, Some(Status::Installed { .. })) {
        return;
    }
    if st.protect.on && !st.hook_hud_ready {
        ui.add(egui::Label::new(RichText::new(format!("{} Not calibrated yet: until the Cockpit lights tab has calibrated the graphics hook, the game has no HUD fix to load and your theme still colours every HUD colour.", egui_phosphor::regular::WARNING)).size(size::FOOTNOTE).color(pal::WARN)).wrap());
    }
    let mut busy = false;
    widgets::disclosure(ui, &mut st.protect_options, "Options");
    if st.protect_options {
        let p = &mut st.protect;
        group(ui, |g| {
            busy |= share(g, "Most green", "In the HUD's orange, as a share of its red. Yellower colours keep theirs.", &mut p.max_gr);
            busy |= share(g, "Most blue", "As a share of red. Bluer colours, such as pinks, keep theirs.", &mut p.max_br);
            busy |= share(g, "Least green", "As a share of red. Redder colours, such as hostile red, keep theirs.", &mut p.min_gr);
        });
        footnote(ui, "Frontier documents none of this: adjust if hostile contacts still change colour or parts of the HUD stay orange.");
        let defaults = Protect { on: st.protect.on, ..Protect::default() };
        if st.protect != defaults && widgets::button(ui, "Back to the defaults").clicked() {
            st.protect = defaults;
        }
    }
    // Saved once a change settles, not while a value is being dragged or typed.
    if !busy && st.saved_protect() != Some(st.protect) {
        st.save_protect();
    }
    if let Some((ok, msg)) = &st.protect_status {
        ui.add(egui::Label::new(RichText::new(msg).size(size::FOOTNOTE).color(if *ok { pal::GOOD } else { pal::BAD })).wrap());
    }
}

/// A setting that can't be switched on yet: its title and why, full width, and (with `button`) the
/// way to the Cockpit lights tab. Returns true when that button is clicked.
fn why_not(g: &mut Group, title: &str, detail: &str, button: bool) -> bool {
    g.content(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add(egui::Label::new(RichText::new(title).size(size::BODY).color(pal::TEXT)).wrap());
        ui.add(egui::Label::new(RichText::new(detail).size(size::FOOTNOTE).color(pal::DIM)).wrap());
        if !button {
            return false;
        }
        ui.add_space(6.0);
        widgets::button(ui, &format!("Go to Cockpit lights {}", icon::ARROW_RIGHT)).clicked()
    })
}

/// A share of red, edited as a percentage. Returns true while it's being dragged or typed.
fn share(g: &mut Group, title: &str, detail: &str, value: &mut f32) -> bool {
    g.row(title, detail, |ui| {
        let mut pct = *value * 100.0;
        let r = ui.add(egui::DragValue::new(&mut pct).range(0.0..=100.0).speed(0.1).fixed_decimals(1).suffix(" %"));
        if r.changed() {
            *value = (pct * 10.0).round() / 1000.0;
        }
        r.dragged() || r.has_focus()
    })
}

/// A strip showing what the HUD orange becomes at each hue; click or drag to pick.
fn hue_strip(ui: &mut Ui, st: &mut HudUi) {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click_and_drag());
    // Inset by the knob's radius so the knob stays inside at either end.
    let bar = rect.shrink2(vec2(12.0, 5.0));
    if let Some(p) = resp.interact_pointer_pos() {
        st.adjust.hue = ((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0) * 360.0;
    }
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    let n = 72;
    let at = |hue: f32| st.base.then(&HudMatrix::from_adjust(&Adjust { hue, ..st.adjust })).apply_srgb(palette::ORANGE.rgb);
    let clip = p.with_clip_rect(bar);
    // Rounded ends: paint the segments, then mask the corners with the card colour.
    for i in 0..n {
        let x0 = bar.left() + bar.width() * i as f32 / n as f32;
        let x1 = bar.left() + bar.width() * (i + 1) as f32 / n as f32 + 0.5;
        clip.rect_filled(Rect::from_x_y_ranges(x0..=x1, bar.y_range()), 0.0, rgb(at(360.0 * (i as f32 + 0.5) / n as f32)));
    }
    p.rect_stroke(bar.expand(0.5), CornerRadius::same(8), Stroke::new(2.0, pal::CARD), egui::StrokeKind::Outside);
    let x = bar.left() + bar.width() * st.adjust.hue / 360.0;
    let knob = pos2(x, rect.center().y);
    p.circle_filled(knob, 10.0, rgb(at(st.adjust.hue)));
    p.circle_stroke(knob, 10.0, Stroke::new(2.5, Color32::WHITE));
    resp.on_hover_text(format!("{:.0}°", st.adjust.hue));
}

/// The nine numbers, for exact themes.
fn matrix_grid(ui: &mut Ui, st: &mut HudUi) {
    ui.add(egui::Label::new(RichText::new("Each row says where one colour of the standard HUD goes: how much of it is shown as red, green and blue.").size(size::FOOTNOTE).color(pal::DIM)).wrap());
    ui.add_space(4.0);
    let mut m = st.edit();
    let mut touched = false;
    egui::Grid::new("hud-matrix").num_columns(4).spacing([14.0, 8.0]).show(ui, |ui| {
        ui.label("");
        for h in ["Red", "Green", "Blue"] {
            widgets::th(ui, &format!("→ {h}"));
        }
        ui.end_row();
        for (i, name) in ["Red", "Green", "Blue"].iter().enumerate() {
            ui.label(RichText::new(format!("{name} {}", HudMatrix::ROW_NAMES[i])).color(pal::DIM)).on_hover_text(format!("<{}> in the file", HudMatrix::ROW_NAMES[i]));
            for v in m.row_mut(i).iter_mut() {
                touched |= ui.add(egui::DragValue::new(v).range(-2.0..=2.0).speed(0.005).fixed_decimals(2)).changed();
            }
            ui.end_row();
        }
    });
    if touched {
        st.start_from(m);
    }
}

/// What the matrix can't do, and the tool that can.
fn limits_card(ui: &mut Ui) {
    card(ui, "What this setting changes, and what it can't", |ui| {
        let lines = [
            "One colour matrix for the whole interface: cockpit HUD, panels, scanner contacts, shields, station menus, the on-foot HUD and the portraits on mission boards (strong themes give them colour bands). It can't give one element, say the scanner, a colour of its own.",
            "Friend and foe colours move with everything else, which is why the preview warns when hostile and friendly contacts end up alike. \"Keep hostile red, friendly green and shield blue\" changes that, with Elite Nexus's graphics hook.",
            "The main menu and the store's ship previews stay in the standard colours.",
            "It doesn't touch the cockpit's own lighting: dashboard lights and cabin glow keep their colour and brightness.",
            "The game reads it only at launch; there is no way to reload it while playing.",
        ];
        for l in lines {
            ui.add(egui::Label::new(RichText::new(format!("•  {l}")).color(pal::TEXT)).wrap());
        }
        section(ui, "Elsewhere on this page");
        let more = [
            "Cockpit lights: the game has no setting for them, so the Cockpit lights tab dims them with Elite Nexus's own dimmer.",
            "Glare and exposure: how bright lights glare and how the picture is exposed, in the same file as these colours.",
            "Colours for single HUD elements: the game's setting can't do them. Elite Nexus's graphics hook can keep hostile red, friendly green and shield blue out of a theme (the switch under In the game), but can't yet give one element, such as the scanner, a colour of its own.",
        ];
        for l in more {
            ui.add(egui::Label::new(RichText::new(format!("•  {l}")).color(pal::DIM)).wrap());
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Painting

fn rgb(c: nexus_hud::Rgb) -> Color32 {
    Color32::from_rgb((c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8)
}

fn colour(m: &HudMatrix, c: nexus_hud::Rgb) -> Color32 {
    rgb(m.apply_srgb(c))
}

fn dot(ui: &mut Ui, c: Color32, r: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(r * 2.0 + 2.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), r, c);
}

/// A selectable pill with a colour dot.
fn chip(ui: &mut Ui, text: &str, c: Color32, selected: bool) -> egui::Response {
    let mut job = LayoutJob::default();
    job.append("●  ", 0.0, TextFormat { font_id: FontId::proportional(size::CALLOUT), color: c, ..Default::default() });
    let font = if selected { display(size::CALLOUT) } else { FontId::proportional(size::CALLOUT) };
    job.append(text, 0.0, TextFormat { font_id: font, color: pal::TEXT, ..Default::default() });
    let fill = if selected { pal::CONTROL } else { pal::CARD_HI };
    let stroke = if selected { Stroke::new(1.5, c) } else { Stroke::NONE };
    ui.add(egui::Button::new(job).fill(fill).stroke(stroke).corner_radius(CornerRadius::same(255)).min_size(vec2(0.0, 30.0)))
}

/// "standard → yours" for each standard colour, with what it's used for.
fn swatches(ui: &mut Ui, m: &HudMatrix, protect: Option<&Protect>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(16.0, 6.0);
        for s in palette::ALL {
            let label = ui.painter().layout_no_wrap(s.name.to_string(), FontId::proportional(size::FOOTNOTE), pal::DIM);
            let size = vec2(14.0 + 18.0 + 14.0 + 8.0 + label.size().x, 18.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
            let p = ui.painter();
            let y = rect.center().y;
            p.circle_filled(pos2(rect.left() + 7.0, y), 7.0, rgb(s.rgb));
            p.text(pos2(rect.left() + 14.0 + 9.0, y), Align2::CENTER_CENTER, icon::ARROW_RIGHT, FontId::proportional(size::CAPTION), pal::FAINT);
            p.circle_filled(pos2(rect.left() + 14.0 + 18.0 + 7.0, y), 7.0, rgb(m.apply_srgb_protected(s.rgb, protect)));
            p.galley(pos2(rect.left() + 46.0 + 8.0, y - label.size().y / 2.0), label, pal::DIM);
            resp.on_hover_text(s.used_for);
        }
    });
}

/// A cockpit view with the HUD drawn in the matrix's colours (only its orange while `protect` is on).
fn preview(ui: &mut Ui, m: &HudMatrix, protect: Option<&Protect>) {
    let w = ui.available_width();
    let h = (w * 0.56).clamp(240.0, 440.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_hud(&ui.painter_at(rect), rect, m, protect);
    }
}

fn paint_hud(p: &egui::Painter, r: Rect, m: &HudMatrix, protect: Option<&Protect>) {
    let c = |s: palette::Swatch, a: f32| rgb(m.apply_srgb_protected(s.rgb, protect)).gamma_multiply(a);
    let o = |a: f32| c(palette::ORANGE, a);
    let x = |f: f32| r.left() + f * r.width();
    let y = |f: f32| r.top() + f * r.height();
    let u = r.height() / 100.0;
    let font = |pt: f32| FontId::proportional(pt.max(size::CAPTION));

    // The world outside: the colour setting doesn't touch it.
    p.rect_filled(r, CornerRadius::same(12), Color32::from_rgb(4, 6, 10));
    let mut seed = 0x9e37_79b9_u32;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 10_000) as f32 / 10_000.0
    };
    for _ in 0..90 {
        let pos = pos2(x(rnd()), y(rnd() * 0.6));
        let a = 40 + (rnd() * 150.0) as u8;
        p.circle_filled(pos, 0.5 + rnd(), Color32::from_white_alpha(a));
    }
    // A planet's limb low on the right, unaffected by the HUD colours.
    p.circle_filled(pos2(x(0.86), y(0.05)), 13.0 * u, Color32::from_rgb(36, 58, 92));
    p.circle_filled(pos2(x(0.875), y(0.03)), 12.0 * u, Color32::from_rgb(4, 6, 10).gamma_multiply(0.75));
    // The dashboard the HUD sits on.
    let dash = vec![pos2(r.left(), y(0.66)), pos2(x(0.2), y(0.6)), pos2(x(0.8), y(0.6)), pos2(r.right(), y(0.66)), pos2(r.right(), r.bottom()), pos2(r.left(), r.bottom())];
    p.add(Shape::convex_polygon(dash, Color32::from_rgb(15, 16, 20), Stroke::NONE));
    p.line_segment([pos2(x(0.2), y(0.6)), pos2(x(0.8), y(0.6))], Stroke::new(1.0, Color32::from_rgb(34, 36, 42)));

    // Reticle.
    let ret = pos2(x(0.5), y(0.34));
    p.circle_stroke(ret, 6.0 * u, Stroke::new(1.3, o(0.9)));
    for d in [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)] {
        p.line_segment([ret + d * 4.0 * u, ret + d * 8.5 * u], Stroke::new(1.3, o(0.9)));
    }
    // A hostile target ahead, a friendly one to the left.
    bracket(p, pos2(x(0.63), y(0.2)), 3.2 * u, c(palette::RED, 1.0));
    p.text(pos2(x(0.63) + 4.5 * u, y(0.2)), Align2::LEFT_CENTER, "WANTED", font(2.6 * u), c(palette::RED, 1.0));
    bracket(p, pos2(x(0.36), y(0.27)), 2.6 * u, c(palette::GREEN, 1.0));

    // Navigation panel, top left.
    let panel = Rect::from_min_max(pos2(x(0.035), y(0.07)), pos2(x(0.3), y(0.47)));
    p.rect_filled(panel, CornerRadius::same(2), o(0.06));
    p.rect_stroke(panel, CornerRadius::same(2), Stroke::new(1.0, o(0.45)), egui::StrokeKind::Inside);
    let line_h = panel.height() / 5.5;
    p.text(panel.left_top() + vec2(1.5 * u, line_h * 0.55), Align2::LEFT_CENTER, "NAVIGATION", font(2.8 * u), o(1.0));
    // Short names: at small sizes the text stays at the 12 pt minimum while the panel shrinks.
    for (i, (name, ly)) in [("Deciat", "12.4"), ("Sol", "48.0"), ("Colonia", "22k")].iter().enumerate() {
        let row = Rect::from_min_size(pos2(panel.left() + 1.0 * u, panel.top() + line_h * (1.2 + i as f32)), vec2(panel.width() - 2.0 * u, line_h * 0.86));
        let (fill, text) = if i == 0 { (o(0.9), Color32::from_rgb(10, 8, 6)) } else { (Color32::TRANSPARENT, o(0.85)) };
        p.rect_filled(row, CornerRadius::same(1), fill);
        p.text(row.left_center() + vec2(1.0 * u, 0.0), Align2::LEFT_CENTER, *name, font(2.5 * u), text);
        p.text(row.right_center() - vec2(1.0 * u, 0.0), Align2::RIGHT_CENTER, format!("{ly} ly"), font(2.5 * u), text);
    }

    // Scanner.
    let sc = pos2(x(0.5), y(0.83));
    let rad = vec2(r.width() * 0.15, 9.5 * u);
    p.add(Shape::ellipse_filled(sc, rad, o(0.07)));
    p.add(Shape::ellipse_stroke(sc, rad, Stroke::new(1.4, o(0.85))));
    p.add(Shape::ellipse_stroke(sc, rad * 0.55, Stroke::new(1.0, o(0.4))));
    p.line_segment([sc - vec2(rad.x, 0.0), sc + vec2(rad.x, 0.0)], Stroke::new(1.0, o(0.3)));
    for s in [-1.0, 1.0] {
        p.line_segment([sc, sc + vec2(s * rad.x * 0.62, -rad.y * 0.78)], Stroke::new(1.0, o(0.35)));
    }
    let contacts = [(-0.5, -0.35, 5.0, palette::RED, true), (0.42, -0.5, -3.5, palette::GREEN, false), (0.62, 0.25, 3.0, palette::ORANGE, true), (-0.2, 0.45, 2.5, palette::BLUE, false), (0.1, -0.2, 6.0, palette::ORANGE, false)];
    for (dx, dy, hgt, s, square) in contacts {
        let base = sc + vec2(dx * rad.x, dy * rad.y);
        let top = base - vec2(0.0, hgt * u);
        let col = c(s, 1.0);
        p.line_segment([base, top], Stroke::new(1.0, col.gamma_multiply(0.7)));
        if square {
            p.rect_filled(Rect::from_center_size(top, Vec2::splat(1.6 * u)), 0.0, col);
        } else {
            p.add(Shape::convex_polygon(vec![top + vec2(0.0, -1.2 * u), top + vec2(1.1 * u, 0.9 * u), top + vec2(-1.1 * u, 0.9 * u)], col, Stroke::NONE));
        }
    }
    p.text(sc - vec2(0.0, rad.y + 3.2 * u), Align2::CENTER_BOTTOM, "MASS LOCKED", font(2.4 * u), o(0.9));

    // Your ship and its shields (left), the target and its shields (right).
    for (cx, s) in [(0.3, palette::ORANGE), (0.7, palette::RED)] {
        let at = pos2(x(cx), y(0.8));
        for (i, a) in [0.95, 0.65, 0.4].iter().enumerate() {
            let rr = (6.0 + 1.6 * i as f32) * u;
            arc(p, at, rr, Stroke::new(1.6, c(palette::BLUE, *a)));
        }
        let ship = vec![at + vec2(0.0, -4.0 * u), at + vec2(3.0 * u, 3.5 * u), at + vec2(0.0, 2.0 * u), at + vec2(-3.0 * u, 3.5 * u)];
        p.add(Shape::closed_line(ship, Stroke::new(1.3, c(s, 1.0))));
    }
    // Power distributor pips under your ship.
    for (col, n) in [2, 4, 3].iter().enumerate() {
        for k in 0..4 {
            let pip = Rect::from_center_size(pos2(x(0.3) + (col as f32 - 1.0) * 4.5 * u, y(0.95) - k as f32 * 1.7 * u), vec2(3.4 * u, 1.1 * u));
            p.rect_filled(pip, 0.0, if k < *n { o(0.9) } else { o(0.18) });
        }
    }
    // Target hull under the target.
    let hull = Rect::from_min_size(pos2(x(0.7) - 6.0 * u, y(0.93)), vec2(12.0 * u, 1.2 * u));
    p.rect_filled(hull, 0.0, o(0.18));
    p.rect_filled(Rect::from_min_size(hull.min, vec2(hull.width() * 0.62, hull.height())), 0.0, o(0.9));
    p.text(hull.center_top() - vec2(0.0, 0.6 * u), Align2::CENTER_BOTTOM, "62%", font(2.3 * u), o(0.9));

    // Throttle with its blue zone (left) and fuel / heat (right).
    let thr = Rect::from_min_max(pos2(x(0.15), y(0.66)), pos2(x(0.15) + 2.0 * u, y(0.95)));
    p.rect_stroke(thr, 0.0, Stroke::new(1.0, o(0.6)), egui::StrokeKind::Inside);
    let lvl = |f: f32| thr.bottom() - thr.height() * f;
    p.rect_filled(Rect::from_min_max(pos2(thr.left(), lvl(0.55)), thr.max), 0.0, o(0.8));
    p.rect_filled(Rect::from_min_max(pos2(thr.right() + 0.6 * u, lvl(0.6)), pos2(thr.right() + 1.6 * u, lvl(0.42))), 0.0, c(palette::BLUE, 1.0));
    for (i, f) in [0.8, 0.3].iter().enumerate() {
        let bar = Rect::from_min_max(pos2(x(0.84) + i as f32 * 3.0 * u, y(0.66)), pos2(x(0.84) + i as f32 * 3.0 * u + 1.6 * u, y(0.95)));
        p.rect_stroke(bar, 0.0, Stroke::new(1.0, o(0.5)), egui::StrokeKind::Inside);
        p.rect_filled(Rect::from_min_max(pos2(bar.left(), bar.bottom() - bar.height() * f), bar.max), 0.0, o(0.8));
    }
    // A highlighted line of text.
    p.text(pos2(x(0.5), y(0.06)), Align2::CENTER_TOP, "DOCKING GRANTED  ·  PAD 07", font(2.6 * u), c(palette::WHITE, 0.9));
}

/// Target brackets: four corners around `at`.
fn bracket(p: &egui::Painter, at: Pos2, half: f32, c: Color32) {
    let k = half * 0.45;
    let s = Stroke::new(1.4, c);
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let corner = at + vec2(sx * half, sy * half);
        p.line_segment([corner, corner - vec2(sx * k, 0.0)], s);
        p.line_segment([corner, corner - vec2(0.0, sy * k)], s);
    }
    p.circle_filled(at, half * 0.22, c);
}

/// The upper half-ring of a shield, as the ship display draws them.
fn arc(p: &egui::Painter, at: Pos2, r: f32, s: Stroke) {
    let pts: Vec<Pos2> = (0..=24).map(|i| {
        let a = std::f32::consts::PI * (1.1 + 0.8 * i as f32 / 24.0);
        at + vec2(a.cos() * r, a.sin() * r)
    }).collect();
    p.add(Shape::line(pts, s));
}

/// A settings-row slider with its value on the left.
fn slider(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) -> bool {
    ui.add_sized([56.0, 20.0], egui::Label::new(RichText::new(format!("{:.0}{suffix}", *value)).color(pal::DIM)));
    ui.add(egui::Slider::new(value, range).show_value(false)).changed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_hud::dimmer::Dimmer;

    #[test]
    fn the_switch_saves_the_protection_and_keeps_the_dimmer() {
        let game = std::env::temp_dir().join(format!("nexus-app-protect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&game);
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("EliteDangerous64.exe"), b"game").unwrap();
        let mut st = HudUi { installs: vec![GameInstall { folder: game.clone(), store: "custom", shader_mod: None }], ..Default::default() };
        // No hook yet: nothing to switch.
        st.read_hook();
        assert_eq!(st.hook, Some(Status::None));
        assert!(st.protection().is_none());

        let dimmer = Dimmer { on: true, strength: 0.15, reach: 8.0 };
        let fake = vec![("d3d11.dll", b"fake".to_vec()), ("d3dcompiler_47.dll", b"fake".to_vec())];
        dimmer::install(&game, &fake, &HookConfig { dimmer, ..HookConfig::default() }).unwrap();
        st.read_hook();
        assert!(st.protection().is_none());
        st.protect.on = true;
        st.protect.min_gr = 0.05;
        st.save_protect();
        assert!(st.protect_status.as_ref().is_some_and(|(ok, msg)| *ok && msg.contains("keeps hostile red")), "{:?}", st.protect_status);
        let want = Protect { on: true, min_gr: 0.05, ..Protect::default() };
        assert_eq!(dimmer::status(&game), Status::Installed { config: HookConfig { dimmer, protect: want, recording: false } });
        assert_eq!(st.protection(), Some(want));

        // Removed from the Cockpit lights tab: the protection can't run, whatever was saved.
        dimmer::uninstall(&game).unwrap();
        st.read_hook();
        assert!(st.protection().is_none());
        let _ = std::fs::remove_dir_all(&game);
    }

    #[test]
    fn screenshot_tabs_combine() {
        let st = HudUi::new(Some("empireblue,options"));
        assert!(st.tab == Tab::Colours && st.protect_options && st.start_preset.as_deref() == Some("empireblue"));
        assert!(HudUi::new(Some("cockpit")).tab == Tab::Cockpit);
        assert!(HudUi::new(None).start_preset.is_none());
    }

    #[test]
    fn screenshot_hue_goes_on_top_of_the_theme() {
        let mut st = HudUi::new(Some("mocha, hue:120"));
        assert_eq!((st.start_preset.as_deref(), st.start_hue), (Some("mocha"), Some(120.0)));
        // Starting from the theme resets the simple controls: the hue is set after it.
        st.start_as_asked();
        let mocha = presets::ALL.iter().find(|p| p.name == "Mocha").map(|p| p.matrix);
        assert_eq!((Some(st.base), st.adjust.hue), (mocha, 120.0));
        assert!(st.start_preset.is_none() && st.start_hue.is_none());
        // Without a theme, the hue goes on top of what the game's file holds.
        let mut alone = HudUi::new(Some("HUE:45.5"));
        alone.start_as_asked();
        assert_eq!((alone.base, alone.adjust.hue), (HudMatrix::IDENTITY, 45.5));
        // Out of range is brought into the slider's 0 to 360; a hue that isn't a number is no theme either.
        assert_eq!(HudUi::new(Some("hue:400")).start_hue, Some(360.0));
        assert_eq!(HudUi::new(Some("hue:-5")).start_hue, Some(0.0));
        let bad = HudUi::new(Some("hue:red,cockpit"));
        assert!(bad.start_hue.is_none() && bad.start_preset.is_none() && bad.tab == Tab::Cockpit);
    }
}
