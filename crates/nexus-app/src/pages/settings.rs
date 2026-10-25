//! Settings, laid out like System Settings: grouped rows with the control on the right.

use crate::core::{CoreStatus, Phase};
use crate::folders::{self, FolderField};
use crate::jobs::Job;
use crate::overlay::draw::Anchor;
use crate::settings::Settings;
use crate::theme::{pal, size};
use crate::widgets::{self, cards, footnote, group, section};
use egui::{Color32, RichText, Ui};
use egui_phosphor::regular as icon;
use nexus_journal::paths::JournalFolderCheck;
use std::path::PathBuf;

/// The owner's GitHub Sponsors page (the README and the website link it too).
const SPONSOR_URL: &str = "https://github.com/sponsors/uruskan";

pub struct SettingsCtx<'a> {
    pub settings: &'a mut Settings,
    pub ui: &'a mut SettingsUi,
    pub status: &'a CoreStatus,
    pub data_dir: &'a std::path::Path,
    pub changed: bool,
    pub test_voice: bool,
    pub restart_core: bool,
    /// What happened the last time Elite Nexus spoke (shown under Test voice).
    pub voice: crate::tts::VoiceStatus,
    /// The overlay is see-through this run (its switch applies after a restart).
    pub overlay_see_through: bool,
}

/// What the Settings page keeps between frames: the folder fields and what checking a folder found.
#[derive(Default)]
pub struct SettingsUi {
    journal: FolderField,
    /// A journal folder just entered, being looked at off the UI thread (a network folder can be slow).
    journal_check: Option<Job<(PathBuf, JournalFolderCheck)>>,
    journal_msg: Option<(Color32, String)>,
    game: FolderField,
    /// The game folders in use, and the folder chosen when they were looked for (it may change on another page).
    game_found: Option<(Option<PathBuf>, Vec<nexus_hud::install::GameInstall>)>,
    game_msg: Option<(Color32, String)>,
}

/// A slider with its value shown on the right, sized for a settings row.
fn slider(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, decimals: usize, suffix: &str) -> bool {
    ui.add_sized([64.0, 20.0], egui::Label::new(RichText::new(format!("{:.*}{suffix}", decimals, *value)).color(pal::DIM)));
    ui.add(egui::Slider::new(value, range).show_value(false)).changed()
}

const UI_SCALES: [(f32, &str); 6] = [(0.9, "90 %"), (1.0, "100 %"), (1.1, "110 %"), (1.25, "125 %"), (1.5, "150 %"), (1.75, "175 %")];

pub fn show(ui: &mut Ui, cx: &mut SettingsCtx<'_>) {
    cards(ui, 2, 420.0, |c| {
        c.add(|ui| {
            let s = &mut *cx.settings;
            section(ui, "Voice and alerts");
            group(ui, |g| {
                cx.changed |= g.switch(&mut s.tts_enabled, "Speak alerts", "Text-to-speech for jumps, scans, missions and price alerts.");
                g.row("Speak from priority", "", |ui| {
                    egui::ComboBox::from_id_salt("ttsprio").selected_text(["Low", "Normal", "High", "Critical"][s.tts_min_priority.min(3) as usize]).show_ui(ui, |ui| {
                        for (i, n) in ["Low", "Normal", "High", "Critical"].iter().enumerate() {
                            cx.changed |= ui.selectable_value(&mut s.tts_min_priority, i as u8, *n).changed();
                        }
                    });
                });
                g.row("Speech rate", "", |ui| cx.changed |= slider(ui, &mut s.tts_rate, 0.5..=2.0, 2, "×"));
                g.row("Volume", "", |ui| {
                    let mut pct = s.tts_volume * 100.0;
                    if slider(ui, &mut pct, 0.0..=100.0, 0, " %") {
                        s.tts_volume = pct / 100.0;
                        cx.changed = true;
                    }
                });
                g.row("Banners stay for", "", |ui| cx.changed |= slider(ui, &mut s.toast_seconds, 2.0..=30.0, 0, " s"));
                g.content(|ui| {
                    if widgets::button(ui, "Test voice").clicked() {
                        cx.test_voice = true;
                    }
                    if let Some((color, text)) = voice_line(&cx.voice) {
                        ui.add(egui::Label::new(RichText::new(text).size(size::FOOTNOTE).color(color)).wrap());
                    }
                });
            });
            section(ui, "Exploration");
            group(ui, |g| {
                g.row("High-value body", "Bodies worth at least this much are highlighted and announced.", |ui| {
                    let mut hv = s.high_value_threshold as f32 / 1e6;
                    if slider(ui, &mut hv, 0.1..=10.0, 1, " M Cr") {
                        s.high_value_threshold = (hv * 1e6) as i64;
                        cx.changed = true;
                    }
                });
                cx.changed |= g.switch(&mut s.auto_page, "Follow my flight", "Exploration while you fly a system, Exobiology near a planet with life, Navigation in the galaxy map or when a jump charges or the system is done. Only while you're on one of those pages.");
                g.row("Planet view below", "Closer to a planet than this, Exobiology opens instead of Exploration.", |ui| {
                    cx.changed |= slider(ui, &mut s.auto_page_altitude_km, 10.0..=500.0, 0, " km");
                });
            });
            section(ui, "Display");
            group(ui, |g| {
                // Fixed steps, not a slider: zooming while dragging rescales the slider under the
                // pointer, which runs the value off to either end.
                g.content(|ui| {
                    ui.label(RichText::new("Text and controls size").color(pal::TEXT));
                    let mut scale = s.ui_scale;
                    if widgets::segmented(ui, &mut scale, &UI_SCALES) {
                        s.ui_scale = scale;
                        cx.changed = true;
                    }
                });
            });
            footnote(ui, "Ctrl + and Ctrl − also change the size; Ctrl 0 goes back to 100 %.");
            overlay(ui, cx);
        });
        c.add(|ui| {
            let st = cx.status;
            section(ui, "Journal");
            group(ui, |g| {
                let phase = match &st.phase {
                    Phase::Starting => "Starting".to_string(),
                    Phase::Loading { done, total } => format!("Loading {done} of {total}"),
                    Phase::Live => "Live".to_string(),
                    Phase::Error(e) => format!("Error: {e}"),
                };
                g.value("State", phase);
                g.row("Folder", "", |ui| {
                    let folder = st.journal_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Not found".into());
                    ui.add(egui::Label::new(RichText::new(folder.clone()).color(pal::DIM)).truncate()).on_hover_text(folder);
                });
                g.value("Journal files", format!("{} · {:.1} MB", st.files, st.bytes as f64 / 1e6));
                g.value("History load time", format!("{:.0} ms", st.load_ms));
                g.row("Following", "", |ui| {
                    ui.add(egui::Label::new(RichText::new(st.current_file.clone().unwrap_or_default()).color(pal::DIM)).truncate());
                });
                g.value("Live events this run", st.live_events.to_string());
                g.content(|ui| journal_setup(ui, cx));
                if !st.errors.is_empty() {
                    g.content(|ui| {
                        for e in st.errors.iter().rev().take(5) {
                            ui.add(egui::Label::new(RichText::new(e).size(size::FOOTNOTE).color(pal::BAD)).wrap());
                        }
                    });
                }
            });
            section(ui, "Game");
            game_folder(ui, cx);
            section(ui, "About");
            group(ui, |g| {
                g.value("Version", format!("Elite Nexus {}", env!("CARGO_PKG_VERSION")));
                g.row("Support Elite Nexus", "It's free, with no ads. If it helps you, GitHub Sponsors keeps it going.", |ui| {
                    ui.hyperlink_to(format!("{} Sponsor {}", icon::HEART, icon::ARROW_SQUARE_OUT), SPONSOR_URL).on_hover_text(SPONSOR_URL);
                });
                g.row("Data folder", "Settings, the market database, nexus.log and, after a crash, crash.log.", |ui| {
                    if widgets::button(ui, "Open").clicked() {
                        folders::reveal(cx.data_dir);
                    }
                    let d = cx.data_dir.display().to_string();
                    ui.add(egui::Label::new(RichText::new(d.clone()).color(pal::DIM)).truncate()).on_hover_text(d);
                });
                #[cfg(target_os = "linux")]
                app_menu(g);
            });
            footnote(ui, "Elite Nexus is free software under the GNU GPL, version 3 or later, and comes with no warranty. Source: github.com/uruskan/EliteNexus. Elite Dangerous is a trademark of Frontier Developments plc; Elite Nexus is an unofficial community tool.");
        });
    });
}

/// Settings › Overlay: the island at the top of the game's screen.
fn overlay(ui: &mut Ui, cx: &mut SettingsCtx<'_>) {
    section(ui, "Overlay");
    let running = cx.overlay_see_through;
    let o = &mut cx.settings.overlay;
    let mut changed = false;
    group(ui, |g| {
        changed |= g.switch(&mut o.enabled, "Show the overlay", "A small black island at the top of the game's screen: it says what's going on in a word or two, opens up for the moments that need you and fades away when nothing does.");
        changed |= g.switch(&mut o.locked, "Locked", "Clicks go through to the game, which keeps the focus. Unlock it to drag it elsewhere; a click on it then opens its page here.");
        g.content(|ui| {
            ui.label(RichText::new("Where").color(pal::TEXT));
            let mut anchor = o.anchor;
            if widgets::segmented(ui, &mut anchor, &[(Anchor::Left, "Top left"), (Anchor::Centre, "Top centre"), (Anchor::Right, "Top right")]) {
                o.anchor = anchor;
                o.position = None;
                changed = true;
            }
            if o.position.is_some() && widgets::button(ui, "Back to the top of the game's screen").on_hover_text("Where you dragged it is forgotten").clicked() {
                o.position = None;
                changed = true;
            }
        });
        g.content(|ui| {
            ui.label(RichText::new("Size").color(pal::TEXT));
            let mut scale = o.scale;
            if widgets::segmented(ui, &mut scale, &crate::overlay::window::SCALES) {
                o.scale = scale;
                changed = true;
            }
        });
        changed |= g.switch(&mut o.keep_minimal, "Keep it when idle", "A small empty island stays at the top instead of fading away when there's nothing to say. See-through only: an empty opaque panel hides.");
        changed |= g.switch(&mut o.always_visible, "Visible over every window", "Off, it shows only while Elite Dangerous or Elite Nexus is the window in front. Wayland and macOS don't say which window that is, so there it always shows.");
        changed |= g.switch(&mut o.see_through, "See-through", "Just the island over the game, changing shape as it opens and closes. Off: an opaque black panel. Applies after a restart.");
        if o.see_through != running {
            g.content(|ui| {
                ui.add(egui::Label::new(RichText::new("Restart Elite Nexus for this to take effect.").size(size::FOOTNOTE).color(pal::WARN)).wrap());
            });
        }
        changed |= g.switch(&mut o.vr, "VR mode", "An opaque panel of fixed size that never hides, with larger text, to mirror into a headset (the user guide says how).");
        changed |= g.switch(&mut o.xsoverlay, "Alerts in XSOverlay", "Important alerts also appear as XSOverlay notifications in your headset. They're sent only to XSOverlay on this computer (port 42069).");
    });
    cx.changed |= changed;
    footnote(ui, "The game hides every overlay in exclusive fullscreen: set Options › Graphics › Display to Borderless or Windowed.");
}

/// The last speech attempt in a line, when there is something to say about it.
fn voice_line(status: &crate::tts::VoiceStatus) -> Option<(Color32, String)> {
    use crate::tts::VoiceStatus;
    match status {
        VoiceStatus::Untried => None,
        VoiceStatus::Spoke(engine) => Some((pal::DIM, format!("Speaking through {}. Nothing heard? Check the volume of the sound device it uses.", program_name(engine)))),
        VoiceStatus::Missing if cfg!(target_os = "macos") => Some((pal::WARN, "Voice uses macOS's own say (/usr/bin/say), which isn't there.".into())),
        VoiceStatus::Missing => Some((
            pal::WARN,
            "Voice needs speech-dispatcher (spd-say) or espeak-ng, and neither is installed: install the speech-dispatcher package with its espeak-ng module, then restart Elite Nexus.".into(),
        )),
        VoiceStatus::Failed(engine, why) => Some((
            pal::BAD,
            if *engine == "spd-say" {
                format!("spd-say couldn't speak: {why}. speech-dispatcher needs an output module too, for example speech-dispatcher-espeak-ng on Fedora and Nobara.")
            } else {
                format!("{} couldn't speak: {why}.", program_name(engine))
            },
        )),
    }
}

/// A speech program as the commander knows it: `say`, not `/usr/bin/say`.
fn program_name(engine: &str) -> &str {
    std::path::Path::new(engine).file_name().and_then(|n| n.to_str()).unwrap_or(engine)
}

/// The journal folder setting with, on Linux and macOS, where Elite Nexus looks by itself. Also shown under a journal
/// problem on every page, so the folder can be set before any commander is loaded.
pub fn journal_setup(ui: &mut Ui, cx: &mut SettingsCtx<'_>) {
    journal_folder(ui, cx);
    if cfg!(not(windows)) {
        let hint = if cfg!(target_os = "macos") {
            "macOS: Elite Nexus looks in the game's prefix in every Steam library, where NotProton (Steam Play for macOS) keeps it (~/Library/Application Support/Steam/steamapps/compatdata/359320/pfx/drive_c/users/steamuser/Saved Games/Frontier Developments/Elite Dangerous), then in CrossOver's bottles, ~/.wine and Whisky's bottles. For any other Wine prefix, enter its folder here: Bindings and HUD colours then use the Options folder in that same prefix."
        } else {
            "Linux: Elite Nexus looks in the game's Proton prefix in every Steam library (steamapps/compatdata/359320/pfx/drive_c/users/steamuser/Saved Games/Frontier Developments/Elite Dangerous), then in Lutris's and Heroic's usual folders. For any other Wine prefix, enter its folder here: Bindings and HUD colours then use the Options folder in that same prefix."
        };
        ui.add(egui::Label::new(RichText::new(hint).size(size::FOOTNOTE).color(pal::DIM)).wrap());
    }
}

/// The custom journal folder: typed, pasted or picked; checked for journals when entered, and read from at once
/// when it holds them.
fn journal_folder(ui: &mut Ui, cx: &mut SettingsCtx<'_>) {
    let st = &mut *cx.ui;
    ui.label(RichText::new("Custom journal folder").size(size::FOOTNOTE).color(pal::DIM));
    let current = cx.settings.journal_dir.clone();
    let mut entered = st.journal.field(ui, "journal", &current, "Empty: found automatically");
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if let Some(dir) = st.journal.browse(ui, "Elite Dangerous's journal folder", &current) {
            entered = Some(dir);
        }
        if widgets::button(ui, "Reload journals").on_hover_text("Reads every journal again, from the folder set here").clicked() {
            cx.restart_core = true;
            st.journal_msg = None;
        }
    });
    if let Some(dir) = entered {
        cx.settings.journal_dir = dir.clone();
        cx.changed = true;
        st.journal_msg = None;
        match dir {
            // Back to the folder found automatically: read it now.
            None => {
                cx.restart_core = true;
                st.journal_check = None;
                st.journal_msg = Some((pal::GOOD, "Back to the folder Elite Nexus finds by itself.".into()));
            }
            Some(dir) => st.journal_check = Some(Job::spawn(ui.ctx(), move || (dir.clone(), nexus_journal::paths::check_journal_folder(&dir)))),
        }
    }
    if let Some((dir, check)) = st.journal_check.as_mut().and_then(Job::poll) {
        st.journal_check = None;
        // Only if the folder is still the one entered.
        if cx.settings.journal_dir.as_ref() == Some(&dir) {
            let reading = cx.status.journal_dir.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "no folder".into());
            st.journal_msg = Some(match check {
                JournalFolderCheck::Journals { dir: found, files } => {
                    cx.restart_core = true;
                    let files = if files == 1 { "1 journal file".to_string() } else { format!("{files} journal files") };
                    if found == dir {
                        (pal::GOOD, format!("Reading {files} from this folder."))
                    } else {
                        let msg = format!("The journals are in {}, inside that folder: reading {files} from there.", found.display());
                        cx.settings.journal_dir = Some(found);
                        (pal::GOOD, msg)
                    }
                }
                JournalFolderCheck::Empty => (pal::WARN, format!("No journal files (Journal.….log) in this folder, so Elite Nexus keeps reading {reading} for now. The folder is saved: Reload journals, or the next start, switches to it.")),
                JournalFolderCheck::Unreadable(e) => (
                    pal::BAD,
                    format!("Elite Nexus can't open this folder ({e}), so it keeps reading {reading} for now. The folder is saved: Reload journals, or the next start, tries it again. A folder on another computer needs that computer on and the folder shared."),
                ),
            });
        }
    }
    if st.journal_check.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("Looking for journals in that folder…").size(size::FOOTNOTE).color(pal::DIM));
        });
    }
    if let Some((color, msg)) = &st.journal_msg {
        ui.add(egui::Label::new(RichText::new(msg).size(size::FOOTNOTE).color(*color)).wrap());
    }
}

/// The game folder: found automatically, or chosen here (HUD & cockpit and Bindings use it).
fn game_folder(ui: &mut Ui, cx: &mut SettingsCtx<'_>) {
    let st = &mut *cx.ui;
    let chosen = nexus_hud::install::game_dir();
    if st.game_found.as_ref().is_none_or(|(c, _)| *c != chosen) {
        st.game_found = Some((chosen, nexus_hud::install::installs()));
    }
    let found = st.game_found.as_ref().map(|(_, f)| f.as_slice()).unwrap_or_default();
    let in_use = folders::game_folder_text(found);
    let mut entered = None;
    group(ui, |g| {
        g.row("Game folder", "", |ui| {
            let text = in_use.clone().unwrap_or_else(|| "Not found".into());
            ui.add(egui::Label::new(RichText::new(text.clone()).color(if in_use.is_some() { pal::DIM } else { pal::WARN })).truncate()).on_hover_text(text);
        });
        g.content(|ui| {
            ui.label(RichText::new("Custom game folder").size(size::FOOTNOTE).color(pal::DIM));
            let current = cx.settings.game_dir.clone();
            entered = st.game.field(ui, "game", &current, "Empty: found automatically");
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if let Some(dir) = st.game.browse(ui, "Elite Dangerous's folder", &current) {
                    entered = Some(dir);
                }
            });
            if let Some((color, msg)) = &st.game_msg {
                ui.add(egui::Label::new(RichText::new(msg).size(size::FOOTNOTE).color(*color)).wrap());
            } else if current.is_some() && in_use.is_none() {
                ui.add(egui::Label::new(RichText::new("No EliteDangerous64.exe in that folder or under it.").size(size::FOOTNOTE).color(pal::WARN)).wrap());
            }
        });
    });
    footnote(ui, &format!("HUD & cockpit and the game's built-in control presets (Bindings) need it. {}", folders::GAME_FOLDER_HINT));
    if let Some(dir) = entered {
        folders::set_game_dir(&mut cx.settings.game_dir, dir);
        cx.changed = true;
        let found = nexus_hud::install::installs();
        st.game_msg = Some(match (folders::game_folder_text(&found), &cx.settings.game_dir) {
            (Some(f), Some(_)) => (pal::GOOD, format!("Found the game: {f}.")),
            (Some(f), None) => (pal::GOOD, format!("Back to the folder found automatically: {f}.")),
            (None, Some(_)) => (pal::WARN, "No EliteDangerous64.exe in that folder or under it, so HUD & cockpit and Bindings have no game folder.".into()),
            (None, None) => (pal::WARN, "Elite Nexus doesn't find the game by itself on this computer: choose its folder here.".into()),
        });
        st.game_found = Some((cx.settings.game_dir.clone(), found));
    }
}

/// Linux: Elite Nexus in the desktop's app menu (desktop_entry.rs), written only on a click here.
#[cfg(target_os = "linux")]
fn app_menu(g: &mut widgets::Group) {
    use crate::desktop_entry::{self, State};
    let (Some(home), Ok(exe)) = (crate::settings::user_data_base(), std::env::current_exe()) else { return };
    let (state_id, msg_id) = (egui::Id::new("settings-app-menu"), egui::Id::new("settings-app-menu-msg"));
    let ctx = g.ui.ctx().clone();
    let state = match ctx.data(|d| d.get_temp::<State>(state_id)) {
        Some(s) => s,
        None => {
            let s = desktop_entry::state(&home, &exe);
            ctx.data_mut(|d| d.insert_temp(state_id, s.clone()));
            s
        }
    };
    let (detail, action) = match &state {
        State::Missing => (format!("Adds Elite Nexus and its icon to your desktop's app menu ({}).", desktop_entry::entry_path(&home).display()), Some("Add")),
        State::Current => ("Elite Nexus is in your desktop's app menu, with its icon.".to_string(), None),
        State::Elsewhere(p) => (format!("Your app menu starts {p}, not this copy."), Some("Point it here")),
    };
    // Some(true): add or update; Some(false): remove.
    let clicked = g.row("App menu", &detail, |ui| {
        let mut act = None;
        if let Some(label) = action
            && widgets::button(ui, label).clicked()
        {
            act = Some(true);
        }
        if state != State::Missing && widgets::button(ui, "Remove").clicked() {
            act = Some(false);
        }
        act
    });
    if let Some(add) = clicked {
        let done = if add { desktop_entry::install(&home, &exe) } else { desktop_entry::uninstall(&home) };
        let msg = done.err().map(|e| format!("Couldn't change the app menu: {e}"));
        ctx.data_mut(|d| {
            d.insert_temp(state_id, desktop_entry::state(&home, &exe));
            d.insert_temp(msg_id, msg);
        });
    }
    if let Some(Some(msg)) = ctx.data(|d| d.get_temp::<Option<String>>(msg_id)) {
        g.content(|ui| ui.add(egui::Label::new(RichText::new(msg).size(size::FOOTNOTE).color(pal::BAD)).wrap()));
    }
}
