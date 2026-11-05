//! Whether Elite Dangerous is still running when its journal didn't say it closed: a crash, or the game ended from the
//! Task Manager, writes no `Shutdown`. Its process on this computer says so where it can be seen; a journal that has been
//! quiet since before Elite Nexus started says it likely. The verdict goes into the model (`GameState::game_gone`), so
//! pages, the overlay and Ask's tools all read it.

use nexus_journal::Timestamp;
use nexus_model::{GameGone, GameState};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

/// How long the journal and Status.json must have been quiet, when nothing has been written since Elite Nexus started,
/// before a game not seen on this computer counts as gone. Docked and idle, the game writes nothing for as long as
/// you're away, so it isn't any shorter.
pub const QUIET_SECS: i64 = 15 * 60;

/// How often the process list is looked at.
const EVERY: Duration = Duration::from_secs(5);

/// Whether this system's processes can be looked at here: Windows lists them, Linux has /proc. Not a Mac.
const SUPPORTED: bool = cfg!(windows) || cfg!(target_os = "linux");

const UNKNOWN: u8 = 0;
const NOT_RUNNING: u8 = 1;
const RUNNING: u8 = 2;

/// Looks for the game's process every few seconds, on a thread of its own.
pub struct GameProcess {
    state: Arc<AtomicU8>,
    stop: Arc<AtomicBool>,
}

impl GameProcess {
    /// Starts looking where processes can be looked at; with `look` false (test runs, which read journals that aren't
    /// being written) and on a Mac it never knows.
    pub fn start(look: bool) -> GameProcess {
        let state = Arc::new(AtomicU8::new(UNKNOWN));
        let stop = Arc::new(AtomicBool::new(false));
        if look && SUPPORTED {
            let (state, stop) = (state.clone(), stop.clone());
            let spawned = std::thread::Builder::new().name("nexus-game".into()).spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let now = match running() {
                        Some(true) => RUNNING,
                        Some(false) => NOT_RUNNING,
                        None => UNKNOWN,
                    };
                    state.store(now, Ordering::Relaxed);
                    std::thread::sleep(EVERY);
                }
            });
            if let Err(e) = spawned {
                tracing::warn!("can't look for the game's process: {e}");
            }
        }
        GameProcess { state, stop }
    }

    /// Whether the game's process runs on this computer: None before the first look, or where it can't be looked at.
    pub fn running(&self) -> Option<bool> {
        match self.state.load(Ordering::Relaxed) {
            RUNNING => Some(true),
            NOT_RUNNING => Some(false),
            _ => None,
        }
    }
}

impl Drop for GameProcess {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// What the app knows besides the journal, for `gone`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Watch {
    /// Whether the game's process runs on this computer: None before the first look, or where it can't be looked at.
    pub process: Option<bool>,
    /// The process has been seen running here since the journals were read.
    pub seen_here: bool,
    /// Its absence settles it: the process list shows every program here (Windows), and the journal folder is this
    /// computer's own, the one Elite Nexus found by itself.
    pub absence_final: bool,
    /// The game has written something (a journal line, Status.json) since the journals were read.
    pub live_lines: bool,
}

/// Whether, and how, the game has ended without a `Shutdown` (a game that closed normally isn't gone: its `Shutdown`
/// says so):
/// - its process ran here and has gone: `Process`, whatever was written;
/// - it runs, or has written something since the journals were read: not gone. A game not seen here (a Mac, another
///   computer's journal folder, a Linux sandbox) then counts as running until its `Shutdown`, as there's nothing else
///   to go by;
/// - quiet since the journals were read: `Process` when its absence settles it (`Watch::absence_final`), else `Quiet`
///   once neither the journal nor Status.json has been written for `QUIET_SECS`: a game that ended before Elite Nexus
///   started, most likely.
pub fn gone(gs: &GameState, w: Watch, now: Timestamp) -> Option<GameGone> {
    if gs.game_closed {
        return None;
    }
    if w.seen_here && w.process == Some(false) {
        return Some(GameGone::Process);
    }
    if w.live_lines || w.process == Some(true) {
        return None;
    }
    if w.absence_final && w.process == Some(false) {
        return Some(GameGone::Process);
    }
    let last = gs.status.as_ref().map_or(gs.last_event, |s| s.timestamp.max(gs.last_event));
    (now.0 - last.0 > QUIET_SECS).then_some(GameGone::Quiet)
}

/// The game's program, by a process's name: `EliteDangerous64.exe`, with a folder before it or not, or the first 15
/// characters of it, which is what Linux keeps of the name Wine gives the process.
fn is_game(name: &str) -> bool {
    let name = name.trim().trim_matches('"').to_ascii_lowercase();
    let file = name.rsplit(['\\', '/']).next().unwrap_or(&name);
    file == "elitedangerous64.exe" || file == "elitedangerous6"
}

/// Whether the game's process runs here; None when the processes couldn't be listed.
fn running() -> Option<bool> {
    running_named(is_game)
}

/// Windows: whether the process list has a program `is` knows.
#[cfg(windows)]
fn running_named(is: impl Fn(&str) -> bool) -> Option<bool> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
    // SAFETY: a snapshot of the process list, read entry by entry into a struct sized as the API asks, then closed.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return None;
        }
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        // A list that can't be read says nothing: not "not running".
        if Process32FirstW(snapshot, &mut entry) == 0 {
            CloseHandle(snapshot);
            return None;
        }
        let mut found = false;
        let mut more = true;
        while more && !found {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            found = is(&String::from_utf16_lossy(&entry.szExeFile[..len]));
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        Some(found)
    }
}

/// Linux: whether /proc has a process `is` knows, by its short name (`comm`) or an argument of its command line. Wine
/// names the game's process after its program in both.
#[cfg(not(windows))]
fn running_named(is: impl Fn(&str) -> bool) -> Option<bool> {
    let dir = std::fs::read_dir("/proc").ok()?;
    let found = dir.flatten().filter(|e| e.file_name().to_str().is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))).any(|e| {
        let read = |file: &str| std::fs::read(e.path().join(file)).unwrap_or_default();
        is(&String::from_utf8_lossy(&read("comm"))) || read("cmdline").split(|b| *b == 0).any(|arg| is(&String::from_utf8_lossy(arg)))
    });
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_game_by_its_program() {
        assert!(is_game("EliteDangerous64.exe"));
        assert!(is_game(r"C:\Program Files (x86)\Steam\steamapps\common\Elite Dangerous\Products\elite-dangerous-odyssey-64\EliteDangerous64.exe"));
        assert!(is_game("EliteDangerous6\n"), "Linux's 15-character comm");
        assert!(is_game("Z:/games/elite/EliteDangerous64.exe"));
        assert!(!is_game("EDLaunch.exe"), "the launcher isn't the game");
        assert!(!is_game("EliteNexus.exe"));
        assert!(!is_game(""));
    }

    /// The process list is really read: this test's own program is in it.
    #[test]
    fn finds_a_running_program() {
        if !SUPPORTED {
            return;
        }
        let me = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase())).expect("own name");
        // Linux keeps the first 15 characters of a process's name.
        let short: String = me.chars().take(15).collect();
        let mine = |name: &str| {
            let name = name.trim().to_ascii_lowercase();
            let file = name.rsplit(['\\', '/']).next().unwrap_or(&name);
            file == me || file == short
        };
        assert_eq!(running_named(mine), Some(true));
        assert_eq!(running_named(|_| false), Some(false));
    }

    #[test]
    fn gone_without_a_shutdown() {
        let now = Timestamp(1_800_000_000);
        let mut gs = GameState::default();
        let w = |process: Option<bool>, seen_here: bool, absence_final: bool, live_lines: bool| Watch { process, seen_here, absence_final, live_lines };
        // Seen here, then gone: a crash, however much it wrote meanwhile.
        gs.last_event = Timestamp(now.0 - 5);
        assert_eq!(gone(&gs, w(Some(false), true, false, true), now), Some(GameGone::Process));
        assert_eq!(gone(&gs, w(Some(true), true, false, true), now), None, "its process runs");
        // Written to since the journals were read, never seen here (a Mac, another computer's folder, a Linux sandbox):
        // running until its Shutdown, however long it's then quiet, and whatever another computer's clock says.
        gs.last_event = Timestamp(now.0 - 3 * 3600);
        assert_eq!(gone(&gs, w(None, false, false, true), now), None);
        assert_eq!(gone(&gs, w(Some(false), false, false, true), now), None);
        assert_eq!(gone(&gs, w(Some(false), false, true, true), now), None, "a line written now outweighs the process list");
        // Quiet since the journals were read.
        assert_eq!(gone(&gs, w(Some(false), false, true, false), now), Some(GameGone::Process), "Windows, this computer's own folder: crashed before");
        assert_eq!(gone(&gs, w(None, false, false, false), now), Some(GameGone::Quiet), "the journal of a crashed game, read the next day");
        assert_eq!(gone(&gs, w(Some(false), false, false, false), now), Some(GameGone::Quiet));
        assert_eq!(gone(&gs, w(Some(true), false, false, false), now), None, "idle, but its process runs");
        gs.last_event = Timestamp(now.0 - QUIET_SECS + 60);
        assert_eq!(gone(&gs, w(None, false, false, false), now), None, "quiet for less than a quarter of an hour");
        gs.last_event = Timestamp(now.0 - QUIET_SECS - 1);
        gs.status = Some(std::sync::Arc::new(nexus_journal::companion::Status { timestamp: Timestamp(now.0 - 30), ..Default::default() }));
        assert_eq!(gone(&gs, w(None, false, false, false), now), None, "Status.json was written half a minute ago");
        gs.game_closed = true;
        assert_eq!(gone(&gs, w(Some(false), true, true, false), now), None, "closed normally: the journal says so");
    }
}
