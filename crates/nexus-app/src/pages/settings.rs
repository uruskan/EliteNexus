//! Settings, laid out like System Settings: grouped rows with the control on the right.

use crate::core::{CoreStatus, Phase};
use crate::folders::{self, FolderField};
use crate::jobs::Job;
use crate::overlay::draw::Anchor;
use crate::settings::Settings;
use crate::theme::{pal, size};
use crate::tts::{EngineKind, VoiceInfo};
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
    /// Windows: the voices of the speech engine in use.
    pub voices: Vec<crate::tts::VoiceInfo>,
    /// Out: list the engine's voices again (the page has just opened, or its voice list), for a voice installed
    /// while Elite Nexus runs.
    pub refresh_voices: bool,
    /// The overlay is see-through this run (its switch applies after a restart).
    pub overlay_see_through: bool,
    /// What happened to the last screenshot saved as PNG this run.
    pub last_screenshot: Option<crate::screenshots::Outcome>,
    /// AI agents over MCP: listening, and the calls answered this run.
    pub agents: crate::mcp::Status,
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
    /// Where the PNG screenshots go.
    shots_folder: FolderField,
    /// The game's screenshot folder, and the journal folder it was found for.
    shots_dir: Option<(Option<PathBuf>, Option<PathBuf>)>,
    shots_msg: Option<String>,
    /// The pass the page was last drawn in: a gap means it has just been opened.
    shown_pass: Option<u64>,
}

/// A slider with its value shown on the right, sized for a settings row.
fn slider(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, decimals: usize, suffix: &str) -> bool {
    ui.add_sized([64.0, 20.0], egui::Label::new(RichText::new(format!("{:.*}{suffix}", decimals, *value)).color(pal::DIM)));
    ui.add(egui::Slider::new(value, range).show_value(false)).changed()
}

const UI_SCALES: [(f32, &str); 6] = [(0.9, "90 %"), (1.0, "100 %"), (1.1, "110 %"), (1.25, "125 %"), (1.5, "150 %"), (1.75, "175 %")];

pub fn show(ui: &mut Ui, cx: &mut SettingsCtx<'_>) {
    // Opened just now: the voices are listed again, so one installed meanwhile is offered.
    let pass = ui.ctx().cumulative_pass_nr();
    if cx.ui.shown_pass.is_none_or(|p| p + 1 < pass) {
        cx.refresh_voices = true;
    }
    cx.ui.shown_pass = Some(pass);
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
                if cfg!(windows) {
                    g.row("Speech engine", "SAPI also has the voices other programs install for it.", |ui| {
                        egui::ComboBox::from_id_salt("ttsengine").selected_text(s.tts_engine.label()).show_ui(ui, |ui| {
                            for e in EngineKind::ALL {
                                if ui.selectable_value(&mut s.tts_engine, e, e.label()).changed() {
                                    // The engines have different voices: start from the new one's default.
                                    s.tts_voice.clear();
                                    cx.changed = true;
                                }
                            }
                        });
                    });
                    g.row("Voice", "", |ui| {
                        // As wide as the voice's name, but no wider than the row leaves beside its title: a long SAPI
                        // name ("Microsoft Zira Desktop - English (United States)") ends in "…" and shows whole on hover.
                        let name = voice_name(&s.tts_voice, &cx.voices);
                        let text_w = ui.painter().layout_no_wrap(name.clone(), egui::TextStyle::Button.resolve(ui.style()), pal::TEXT).size().x;
                        let combo = egui::ComboBox::from_id_salt("ttsvoice").width((text_w + 40.0).min(ui.available_width())).truncate().selected_text(name.clone()).show_ui(ui, |ui| {
                            cx.changed |= ui.selectable_value(&mut s.tts_voice, String::new(), "Default").changed();
                            for v in &cx.voices {
                                cx.changed |= ui.selectable_value(&mut s.tts_voice, v.id.clone(), &v.name).changed();
                            }
                        });
                        // Opening the list reads the voices again: one installed a moment ago appears in it.
                        if combo.response.clicked() {
                            cx.refresh_voices = true;
                        }
                        combo.response.on_hover_text(name);
                    });
                }
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
                    if cfg!(windows) && !s.tts_voice.is_empty() && !cx.voices.is_empty() && !cx.voices.iter().any(|v| v.id == s.tts_voice) {
                        let text = format!("The chosen voice isn't installed for {}: the default voice speaks instead.", s.tts_engine.label());
                        ui.add(egui::Label::new(RichText::new(text).size(size::FOOTNOTE).color(pal::WARN)).wrap());
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
                g.row("Planet view below", "Closer than this to a planet with biology left to analyse, Exobiology opens instead of Exploration.", |ui| {
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
            screenshots(ui, cx);
            agents(ui, cx);
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

/// Settings › AI agents: AI programs on this computer (MCP clients such as Claude Code) may use the tools Ask uses,
/// read-only. Off until switched on.
fn agents(ui: &mut Ui, cx: &mut SettingsCtx<'_>) {
    section(ui, "AI agents");
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "EliteNexus".into());
    let state = agents_state(&cx.agents);
    let ai = &mut cx.settings.ai;
    group(ui, |g| {
        cx.changed |= g.switch(&mut ai.agents, "Let AI agents read Elite Nexus", "Claude Code, Claude Desktop and other MCP clients on this computer can use the tools Ask uses, to read only.");
        if ai.agents {
            g.row("State", "", |ui| {
                ui.add(egui::Label::new(RichText::new(&state.1).color(state.0)).wrap());
            });
        }
        g.row("Claude Code", "Run this once in a terminal.", |ui| copy_button(ui, "mcp-claude-code", "Copy command", &claude_code_command(&exe)));
        g.row("Claude Desktop", "Add it under mcpServers in claude_desktop_config.json, then restart Claude Desktop.", |ui| copy_button(ui, "mcp-claude-desktop", "Copy entry", &claude_desktop_entry(&exe)));
    });
    footnote(
        ui,
        "The agent starts Elite Nexus' own program with --mcp, which hands each call to this window over a pipe on this computer, never the network. An agent reads what Ask reads: your commander, ships, cargo, materials, ranks, engineers, journal, the market database and Spansh. It changes nothing in the game or in Elite Nexus and plans no routes. One call at a time, at most 30 a minute (10 of them from Spansh or EDGalaxyData).",
    );
}

/// The agents' state in a line, and its colour.
fn agents_state(s: &crate::mcp::Status) -> (Color32, String) {
    if let Some(e) = &s.error {
        return (pal::BAD, e.clone());
    }
    if !s.started {
        return (pal::DIM, "Not in a screenshot run: agents are answered only when Elite Nexus runs normally.".into());
    }
    if !s.listening {
        return (pal::DIM, "Starting…".into());
    }
    let calls = match s.last {
        Some((tool, at)) if s.answered > 0 => {
            let secs = at.elapsed().as_secs();
            let ago = if secs < 60 { format!("{secs} s") } else if secs < 3600 { format!("{} min", secs / 60) } else { format!("{} h", secs / 3600) };
            format!("{} call{} answered this run, the last ({tool}) {ago} ago", s.answered, if s.answered == 1 { "" } else { "s" })
        }
        _ => "no calls yet this run".into(),
    };
    let refused = if s.refused > 0 { format!(", {} refused over the limits", s.refused) } else { String::new() };
    (pal::GOOD, format!("Waiting for agents · {calls}{refused}"))
}

/// Claude Code's command that adds Elite Nexus for every project of this user.
fn claude_code_command(exe: &str) -> String {
    format!("claude mcp add --scope user elite-nexus -- \"{exe}\" --mcp")
}

/// Claude Desktop's `mcpServers` entry: only the entry, so it goes beside the commander's other servers.
fn claude_desktop_entry(exe: &str) -> String {
    let entry = serde_json::json!({ "command": exe, "args": ["--mcp"] });
    format!("\"elite-nexus\": {}", serde_json::to_string_pretty(&entry).unwrap_or_default())
}

/// A button that puts `text` on the clipboard and says so for a moment; the text shows on hover.
fn copy_button(ui: &mut Ui, id: &str, label: &str, text: &str) {
    let key = egui::Id::new(("copied", id));
    let now = ui.input(|i| i.time);
    let copied = ui.ctx().data(|d| d.get_temp::<f64>(key)).is_some_and(|t| now - t < 2.0);
    let shown = if copied { format!("{} Copied", icon::CHECK) } else { format!("{} {label}", icon::COPY) };
    if widgets::button(ui, &shown).on_hover_text(text).clicked() {
        ui.ctx().copy_text(text.to_string());
        ui.ctx().data_mut(|d| d.insert_temp(key, now));
    }
    if copied {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
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

/// The chosen voice as the voice list shows it. Before the engine has listed its voices, a voice can't be named yet.
fn voice_name(id: &str, voices: &[VoiceInfo]) -> String {
    if id.is_empty() {
        return "Default".into();
    }
    match voices.iter().find(|v| v.id == id) {
        Some(v) => v.name.clone(),
        None if voices.is_empty() => "…".into(),
        None => "Not installed".into(),
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

/// Settings › Screenshots: each new screenshot saved again as a PNG (screenshots.rs).
fn screenshots(ui: &mut Ui, cx: &mut SettingsCtx<'_>) {
    section(ui, "Screenshots");
    let st = &mut *cx.ui;
    let journal = cx.status.journal_dir.clone();
    if st.shots_dir.as_ref().is_none_or(|(j, _)| *j != journal) {
        st.shots_dir = Some((journal.clone(), nexus_journal::paths::screenshots_dir(journal.as_deref())));
    }
    let game_dir = st.shots_dir.as_ref().and_then(|(_, d)| d.clone());
    let p = &mut cx.settings.screenshots;
    let mut changed = false;
    group(ui, |g| {
        changed |= g.switch(
            &mut p.convert,
            "Save screenshots as PNG",
            "The game saves each screenshot (F10, or Alt+F10 at high resolution) as an uncompressed BMP of several megabytes. Elite Nexus saves each new one again as a PNG, every pixel kept, named after the system, the nearest body and the time.",
        );
        if p.convert {
            let bin = if cfg!(windows) { "the Recycle Bin" } else { "the Trash" };
            changed |= g.switch(&mut p.delete_bmp, "Delete the BMP afterwards", &format!("Once its PNG is saved. It's deleted for good, not moved to {bin}."));
        }
        g.row("Game's screenshots", "", |ui| match &game_dir {
            Some(dir) => {
                if widgets::button(ui, "Open").clicked() {
                    st.shots_msg = if dir.is_dir() {
                        folders::reveal(dir);
                        None
                    } else {
                        Some("The game hasn't saved a screenshot there yet.".into())
                    };
                }
                let d = dir.display().to_string();
                ui.add(egui::Label::new(RichText::new(d.clone()).color(pal::DIM)).truncate()).on_hover_text(d);
            }
            None => {
                ui.label(RichText::new("Not known").color(pal::WARN)).on_hover_text("The journal folder in use isn't inside a Windows user folder, so the game's Pictures folder can't be told from it.");
            }
        });
        if let Some(msg) = &st.shots_msg {
            g.content(|ui| ui.add(egui::Label::new(RichText::new(msg).size(size::FOOTNOTE).color(pal::DIM)).wrap()));
        }
        if p.convert {
            g.content(|ui| {
                ui.label(RichText::new("Save the PNGs in").size(size::FOOTNOTE).color(pal::DIM));
                let mut entered = st.shots_folder.field(ui, "screenshots", &p.folder, "Empty: next to the game's BMPs");
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if let Some(dir) = st.shots_folder.browse(ui, "Folder for the PNG screenshots", &p.folder) {
                        entered = Some(dir);
                    }
                });
                if let Some(dir) = entered {
                    p.folder = dir;
                    changed = true;
                }
                let (color, text) = match &cx.last_screenshot {
                    None => (pal::DIM, "Take a screenshot in the game: it's saved as PNG within a few seconds.".to_string()),
                    Some(crate::screenshots::Outcome::Working(bmp)) => (pal::DIM, format!("Saving {bmp} as PNG…")),
                    Some(crate::screenshots::Outcome::Saved(_, text)) => (pal::GOOD, text.clone()),
                    Some(crate::screenshots::Outcome::Failed(why)) => (pal::WARN, why.clone()),
                };
                ui.add(egui::Label::new(RichText::new(text).size(size::FOOTNOTE).color(color)).wrap());
                if let Some(crate::screenshots::Outcome::Saved(png, _)) = &cx.last_screenshot
                    && let Some(dir) = png.parent()
                    && widgets::button(ui, "Show the PNG's folder").clicked()
                {
                    folders::reveal(dir);
                }
            });
        }
    });
    cx.changed |= changed;
    footnote(ui, "Only screenshots taken while Elite Nexus is running are saved as PNG; the ones already in the folder stay as they are. A PNG is named like Sol (Earth) 2026-11-04 14-32-05.png.");
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
