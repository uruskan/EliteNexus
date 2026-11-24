//! Notices above every page: what the commander should know from the start (the settings file, how the last run
//! ended, running from the zip, an old cockpit-light dimmer), until closed.

use crate::theme::pal;
use crate::widgets;
use egui::{RichText, Ui};
use egui_phosphor::regular as icon;
use std::path::Path;

pub struct Notice {
    pub title: &'static str,
    pub text: String,
    /// Off while it still applies (settings that aren't being saved).
    pub closable: bool,
    pub action: Option<Action>,
}

/// The button a notice offers besides Close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    DataFolder,
    CockpitLights,
}

/// Draws the notices. Returns the action the commander followed, other than opening the data folder (done here);
/// following a notice closes it.
pub fn show(ui: &mut Ui, notices: &mut Vec<Notice>, data_dir: &Path) -> Option<Action> {
    let (mut close, mut followed) = (None, None);
    for (i, n) in notices.iter().enumerate() {
        widgets::card_tinted(ui, icon::WARNING, n.title, pal::WARN, |ui| {
            ui.add(egui::Label::new(RichText::new(&n.text).color(pal::TEXT)).wrap());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if n.action == Some(Action::DataFolder) && widgets::button(ui, "Open data folder").on_hover_text(data_dir.display().to_string()).clicked() {
                    crate::folders::reveal(data_dir);
                }
                if n.action == Some(Action::CockpitLights) && widgets::button(ui, "Open Cockpit lights").clicked() {
                    followed = Some(Action::CockpitLights);
                }
                if n.closable && widgets::button(ui, "Close").clicked() {
                    close = Some(i);
                }
            });
        });
        ui.add_space(12.0);
    }
    if let Some(i) = close {
        notices.remove(i);
    }
    if let Some(a) = followed {
        // The page it leads to shows the state from there on.
        notices.retain(|n| n.action != Some(a));
    }
    followed
}

/// A cockpit-light dimmer from Elite Nexus 0.3.0 with its hook in place: the game runs its patterns on every shader it
/// loads, which crashes it near the newer fleet carriers. Only reads the game folders.
pub fn old_dimmer_installed() -> bool {
    nexus_hud::install::installs().iter().any(|i| nexus_hud::dimmer::uses_patterns(&i.folder) && !nexus_hud::dimmer::hook_missing(&i.folder))
}

pub fn old_dimmer() -> Notice {
    Notice {
        title: "Your cockpit-light dimmer can crash the game",
        text: "It's still the install from Elite Nexus 0.3.0, which runs a pattern on every shader the game loads and crashes it near the newer fleet carriers. Update on the Cockpit lights tab replaces it; Remove takes it out.".into(),
        closable: true,
        action: Some(Action::CockpitLights),
    }
}

/// Written at start, removed on a normal exit: still there at the next start, the last run ended some other way.
const RUNNING_MARKER: &str = "running";

/// The notice when the last run didn't end normally (and the marker for this one). `gpu_reset`: it lost its graphics
/// device and started this run itself (gpu.rs).
pub fn last_run(data_dir: &Path, gpu_reset: bool) -> Option<Notice> {
    let marker = data_dir.join(RUNNING_MARKER);
    let started = std::fs::metadata(&marker).and_then(|m| m.modified()).ok();
    let _ = std::fs::write(&marker, b"");
    if gpu_reset {
        return Some(Notice {
            title: "Elite Nexus restarted after the graphics card reset",
            text: "Windows or the graphics driver reset the graphics card (after sleep, a driver update, or the card stopping under load), and the window couldn't draw on it any more, so Elite Nexus started again by itself. If it happens often, a newer graphics driver usually helps; crash.log in the data folder has the details.".into(),
            closable: true,
            action: Some(Action::DataFolder),
        });
    }
    let started = started?;
    let crashed = std::fs::metadata(data_dir.join("crash.log")).and_then(|m| m.modified()).is_ok_and(|t| t >= started);
    let text = if crashed {
        "It probably crashed: crash.log in the data folder has what went wrong during that run. Sending that file (a GitHub issue or the Frontier Forums thread) helps fix it; Elite Nexus sends nothing by itself."
    } else {
        "It may have crashed, or it was ended without closing (the computer shut down, or it was force-quit). nexus.log in the data folder shows what it was doing; Elite Nexus sends nothing by itself."
    };
    Some(Notice { title: "Elite Nexus didn't close normally last time", text: text.into(), closable: true, action: Some(Action::DataFolder) })
}

/// A normal exit: the next start has nothing to report.
pub fn clean_exit(data_dir: &Path) {
    let _ = std::fs::remove_file(data_dir.join(RUNNING_MARKER));
}

/// Started from inside its zip: Explorer unpacks it to a new temporary folder every time, data folder included.
pub fn running_from_zip() -> Option<Notice> {
    let exe = std::env::current_exe().ok()?;
    crate::settings::inside_zip(&exe).then(|| Notice {
        title: "Elite Nexus is running from inside its zip file",
        text: "Windows unpacks it to a temporary folder each time it starts, so your settings and the market database don't last, and a second copy can start alongside this one. Unzip it into a folder of its own (Extract All…) and start it from there.".into(),
        closable: true,
        action: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn last_run_reports_an_unclean_exit() {
        let dir = std::env::temp_dir().join(format!("nexus-last-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        // First start: nothing to report, and the marker is down.
        assert!(last_run(&dir, false).is_none());
        assert!(dir.join(RUNNING_MARKER).exists());
        // Not removed (no normal exit): reported, without blaming a crash it can't see.
        let n = last_run(&dir, false).expect("notice");
        assert!(n.text.starts_with("It may have crashed"), "{}", n.text);
        // A crash.log written during that run: it says so.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("crash.log"), "panicked").expect("crash.log");
        assert!(last_run(&dir, false).expect("notice").text.starts_with("It probably crashed"));
        // A normal exit removes the marker.
        clean_exit(&dir);
        assert!(last_run(&dir, false).is_none());
        // Started after the last run lost its graphics device: it says that, not "didn't close normally".
        clean_exit(&dir);
        assert!(last_run(&dir, true).expect("notice").title.contains("graphics card reset"));
        clean_exit(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
