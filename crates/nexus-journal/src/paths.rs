//! Locating the game's folders (journals, Options, Steam libraries) and enumerating journal files in order.
//!
//! On Linux the game runs in a Wine prefix (Steam's Proton, Lutris, Heroic), and on macOS in one too (NotProton's
//! Steam Play, which keeps Proton's `compatdata` layout; CrossOver; Whisky). Its Windows user folder is
//! `<prefix>/drive_c/users/<name>`, and the journals and Options sit inside it just as on Windows.

use crate::time::Timestamp;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Elite Dangerous' Steam app id: Proton keeps the game's prefix in `steamapps/compatdata/359320/pfx`.
pub const STEAM_APP_ID: &str = "359320";
/// The journal folder inside a Windows user folder.
const JOURNAL_IN_PROFILE: [&str; 3] = ["Saved Games", "Frontier Developments", "Elite Dangerous"];
/// The game's Options folder inside `%LOCALAPPDATA%`.
const OPTIONS_IN_LOCAL: [&str; 3] = ["Frontier Developments", "Elite Dangerous", "Options"];
/// The game's screenshot folder (the journal's `\ED_Pictures\`) inside the Windows user's Pictures folder.
const SCREENSHOTS_IN_PICTURES: [&str; 2] = ["Frontier Developments", "Elite Dangerous"];

/// The journal folder the app reads, when it has said so (see [`set_journal_dir_in_use`]).
static JOURNAL_DIR_IN_USE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// `%USERPROFILE%\Saved Games\Frontier Developments\Elite Dangerous` (honouring folder redirection).
///
/// On Linux, the same folder in the game's Wine prefix: Proton's in every Steam library (native, Flatpak and
/// Snap Steam), Lutris's and Heroic's usual folders, `~/.wine`. On macOS: the prefix in every Steam library
/// (NotProton), CrossOver's bottles, `~/.wine`, and only when none of those holds a journal folder, Whisky's
/// bottles (they sit in Whisky's sandbox, and looking there makes macOS ask for access to another app's data).
/// With several, the one written to last.
pub fn default_journal_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(p) = known_folders::get_known_folder_path(known_folders::KnownFolder::SavedGames) {
            return Some(p.join("Frontier Developments").join("Elite Dangerous"));
        }
        let home = std::env::var_os("USERPROFILE")?;
        Some(JOURNAL_IN_PROFILE.iter().fold(PathBuf::from(home), |p, c| p.join(c)))
    }
    #[cfg(not(windows))]
    {
        let newest = |prefixes: Vec<PathBuf>| {
            let found = prefixes.iter().flat_map(|p| journal_dirs_in_prefix(p)).collect::<Vec<_>>();
            found.into_iter().max_by_key(|d| std::fs::metadata(d).and_then(|m| m.modified()).ok())
        };
        newest(wine_prefixes()).or_else(|| newest(sandboxed_wine_prefixes()))
    }
}

/// Tells [`options_dir`] which journal folder the app reads: a custom one may sit in a Wine prefix of its own.
pub fn set_journal_dir_in_use(dir: Option<&Path>) {
    if let Ok(mut d) = JOURNAL_DIR_IN_USE.lock() {
        *d = dir.map(Path::to_path_buf);
    }
}

/// The game's Options folder (control bindings, graphics overrides):
/// `%LOCALAPPDATA%\Frontier Developments\Elite Dangerous\Options`.
///
/// On Linux, the same folder in the Wine prefix that holds the journal folder in use, else in the prefix
/// [`default_journal_dir`] finds.
pub fn options_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(OPTIONS_IN_LOCAL.iter().fold(PathBuf::from(base), |p, c| p.join(c)))
    }
    #[cfg(not(windows))]
    {
        wine_journal_dir().and_then(|d| options_dir_beside(&d))
    }
}

/// Linux and macOS: the `drive_c` folder of the game's Wine prefix (the one holding the journal folder in use, else
/// the one found), where a Frontier or Epic launcher run under Wine installs the game. Windows: `None`.
pub fn wine_drive_c() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    // <drive_c>/users/<user>/Saved Games/Frontier Developments/Elite Dangerous
    let journals = wine_journal_dir()?;
    let drive_c = journals.ancestors().nth(5)?;
    drive_c.file_name().is_some_and(|n| n.eq_ignore_ascii_case("drive_c")).then(|| drive_c.to_path_buf())
}

/// The journal folder in use if it sits in a Wine user folder, else the one found automatically.
fn wine_journal_dir() -> Option<PathBuf> {
    let in_use = JOURNAL_DIR_IN_USE.lock().ok().and_then(|d| d.clone());
    in_use.filter(|d| options_dir_beside(d).is_some()).or_else(default_journal_dir)
}

/// `<user>/AppData/Local/Frontier Developments/Elite Dangerous/Options` for a journal folder at
/// `<user>/Saved Games/Frontier Developments/Elite Dangerous`; `None` for a journal folder anywhere else.
pub fn options_dir_beside(journal_dir: &Path) -> Option<PathBuf> {
    let profile = profile_of(journal_dir)?;
    Some(OPTIONS_IN_LOCAL.iter().fold(profile.join("AppData").join("Local"), |p, c| p.join(c)))
}

/// Where the game saves screenshots (the journal's `\ED_Pictures\`): `Frontier Developments\Elite Dangerous` in the
/// Windows user's Pictures folder, wherever Windows keeps it (OneDrive, another drive). On Linux and macOS, the same
/// folder in the Wine user folder that holds `journal_dir`, the journal folder in use.
pub fn screenshots_dir(journal_dir: Option<&Path>) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let _ = journal_dir;
        let pictures = known_folders::get_known_folder_path(known_folders::KnownFolder::Pictures).or_else(|| Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join("Pictures")))?;
        Some(SCREENSHOTS_IN_PICTURES.iter().fold(pictures, |p, c| p.join(c)))
    }
    #[cfg(not(windows))]
    {
        screenshots_dir_beside(journal_dir?)
    }
}

/// `<user>/Pictures/Frontier Developments/Elite Dangerous` for a journal folder at
/// `<user>/Saved Games/Frontier Developments/Elite Dangerous` (`My Pictures` in a prefix made by an older Wine);
/// `None` for a journal folder anywhere else.
pub fn screenshots_dir_beside(journal_dir: &Path) -> Option<PathBuf> {
    let profile = profile_of(journal_dir)?;
    let pictures = ["Pictures", "My Pictures"].iter().map(|n| profile.join(n)).find(|p| p.is_dir()).unwrap_or_else(|| profile.join("Pictures"));
    Some(SCREENSHOTS_IN_PICTURES.iter().fold(pictures, |p, c| p.join(c)))
}

/// The Windows user folder of a journal folder at `<user>/Saved Games/Frontier Developments/Elite Dangerous`.
fn profile_of(journal_dir: &Path) -> Option<&Path> {
    let mut profile = journal_dir;
    for name in JOURNAL_IN_PROFILE.iter().rev() {
        if !profile.file_name()?.to_string_lossy().eq_ignore_ascii_case(name) {
            return None;
        }
        profile = profile.parent()?;
    }
    Some(profile)
}

/// `%LOCALAPPDATA%\Frontier Developments\Elite Dangerous\Options\Bindings` (on Linux and macOS, in the game's Wine
/// prefix).
pub fn default_bindings_dir() -> Option<PathBuf> {
    Some(options_dir()?.join("Bindings"))
}

/// Every Steam library folder on this PC (each holds `steamapps`), from Steam's `libraryfolders.vdf`.
/// Windows: Steam in Program Files, or wherever the registry says it is. Linux: native Steam, Flatpak Steam and
/// Snap Steam. macOS: Steam in `~/Library/Application Support` (where NotProton keeps Windows games and their
/// prefixes).
pub fn steam_libraries() -> Vec<PathBuf> {
    let mut out: Vec<(PathBuf, PathBuf)> = Vec::new();
    for root in steam_roots() {
        let listed = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")).map(|t| parse_library_folders(&t)).unwrap_or_default();
        for lib in std::iter::once(root).chain(listed) {
            if !lib.join("steamapps").is_dir() {
                continue;
            }
            // ~/.steam/steam is usually a link to ~/.local/share/Steam: compare where they lead.
            let key = std::fs::canonicalize(&lib).unwrap_or_else(|_| lib.clone());
            if !out.iter().any(|(k, _)| same_path(k, &key)) {
                out.push((key, lib));
            }
        }
    }
    out.into_iter().map(|(_, lib)| lib).collect()
}

/// The library folders listed in Steam's `libraryfolders.vdf`: `"path"  "D:\\SteamLibrary"` since 2021, and
/// `"1"  "D:\\SteamLibrary"` in the older format (whose numbered values are the libraries).
pub fn parse_library_folders(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|l| {
            let mut quoted = l.split('"').skip(1).step_by(2);
            let (key, value) = (quoted.next()?, quoted.next()?);
            // An app's size in a library's "apps" block is numbered too ("359320"  "71241906176"): not a path.
            let old_format = !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) && value.contains(['\\', '/']);
            (key == "path" || old_format).then(|| PathBuf::from(value.replace("\\\\", "\\")))
        })
        .collect()
}

/// Windows: Steam in Program Files, then wherever the registry says Steam is (Steam installed on another drive).
#[cfg(windows)]
fn steam_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["ProgramFiles(x86)", "ProgramFiles"].iter().filter_map(std::env::var_os).map(|pf| PathBuf::from(pf).join("Steam")).collect();
    roots.extend(registry::steam_paths());
    roots
}

/// Folders where Windows' list of installed programs says Elite Dangerous is: Steam's "Steam App 359320" entry
/// (`…\steamapps\common\Elite Dangerous`) and the Frontier launcher's. Each holds the game's `Products` folder.
/// Linux: none (the game's Wine prefix is searched instead).
pub fn registered_game_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        registry::game_dirs()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// A folder as the registry holds it: Steam writes `d:/program files (x86)/steam`, installers sometimes quote it
/// or end it with a backslash. `None` for an empty value.
pub fn clean_registry_path(value: &str) -> Option<PathBuf> {
    let v = value.trim().trim_matches('"').trim();
    if v.is_empty() {
        return None;
    }
    let mut v = if cfg!(windows) { v.replace('/', "\\") } else { v.to_string() };
    // Keep the root of a drive ("D:\") whole.
    while v.len() > 3 && v.ends_with(['\\', '/']) {
        v.pop();
    }
    Some(PathBuf::from(v))
}

/// Whether an entry of Windows' list of installed programs (a subkey of `…\CurrentVersion\Uninstall`) may be the
/// game: Steam's `Steam App 359320`, or any whose name starts "Elite Dangerous" (the Frontier launcher's). Other
/// programs with the game in their name (Elite Dangerous Market Connector) lead to folders without the game, which
/// the callers skip.
pub fn is_game_entry(key: &str, display_name: &str) -> bool {
    key.eq_ignore_ascii_case(&format!("Steam App {STEAM_APP_ID}")) || display_name.trim().to_ascii_lowercase().starts_with("elite dangerous")
}

/// Reading Steam's and the game's install folders from the registry.
#[cfg(windows)]
mod registry {
    use std::path::PathBuf;
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, FILETIME};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY, RRF_SUBKEY_WOW6464KEY, RegCloseKey, RegEnumKeyExW,
        RegGetValueW, RegOpenKeyExW,
    };

    const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A text value (`REG_SZ`, or `REG_EXPAND_SZ` expanded); `wow` picks the 32- or 64-bit view of HKLM.
    fn read_string(root: HKEY, subkey: &str, name: &str, wow: u32) -> Option<String> {
        let (subkey, name) = (wide(subkey), wide(name));
        let mut bytes: u32 = 0;
        // SAFETY: null-terminated UTF-16 names; the first call only asks for the size.
        let size = unsafe { RegGetValueW(root, subkey.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ | wow, std::ptr::null_mut(), std::ptr::null_mut(), &mut bytes) };
        if size != ERROR_SUCCESS || bytes < 2 {
            return None;
        }
        let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
        // SAFETY: `buf` holds `bytes` bytes, as the call is told.
        let read = unsafe { RegGetValueW(root, subkey.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ | wow, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut bytes) };
        if read != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    /// The names of a key's subkeys.
    fn subkeys(root: HKEY, path: &str, view: u32) -> Vec<String> {
        let path = wide(path);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: a null-terminated name and a place for the handle, closed below.
        if unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ | view, &mut key) } != ERROR_SUCCESS {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut name = [0u16; 256];
        for index in 0.. {
            let mut len = name.len() as u32;
            let mut written = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
            // SAFETY: `name` holds `len` characters; the optional class arguments are null.
            let r = unsafe { RegEnumKeyExW(key, index, name.as_mut_ptr(), &mut len, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), &mut written) };
            if r != ERROR_SUCCESS {
                // ERROR_NO_MORE_ITEMS at the end; a name longer than 255 characters isn't a registry key name.
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        // SAFETY: opened above.
        unsafe { RegCloseKey(key) };
        out
    }

    /// Steam's own folder: `SteamPath`, which Steam writes each time it runs, and its installer's `InstallPath`.
    pub fn steam_paths() -> Vec<PathBuf> {
        [
            read_string(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath", 0),
            read_string(HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath", RRF_SUBKEY_WOW6432KEY),
            read_string(HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath", RRF_SUBKEY_WOW6464KEY),
        ]
        .into_iter()
        .flatten()
        .filter_map(|v| super::clean_registry_path(&v))
        .collect()
    }

    /// The game's folders in the list of installed programs, both registry views of HKLM and the user's own.
    pub fn game_dirs() -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for (root, view, wow) in [(HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY, RRF_SUBKEY_WOW6464KEY), (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY, RRF_SUBKEY_WOW6432KEY), (HKEY_CURRENT_USER, 0, 0)] {
            for key in subkeys(root, UNINSTALL, view) {
                let entry = format!(r"{UNINSTALL}\{key}");
                let name = read_string(root, &entry, "DisplayName", wow).unwrap_or_default();
                if !super::is_game_entry(&key, &name) {
                    continue;
                }
                let Some(dir) = read_string(root, &entry, "InstallLocation", wow).and_then(|loc| super::clean_registry_path(&loc)) else { continue };
                if !out.iter().any(|d| super::same_path(d, &dir)) {
                    out.push(dir);
                }
            }
        }
        out
    }
}

#[cfg(not(windows))]
fn steam_roots() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
    let mut roots: Vec<PathBuf> = std::env::var_os("XDG_DATA_HOME").map(|x| PathBuf::from(x).join("Steam")).into_iter().collect();
    for rel in [".local/share/Steam", ".steam/steam", ".steam/root", ".var/app/com.valvesoftware.Steam/.local/share/Steam", ".var/app/com.valvesoftware.Steam/data/Steam", "snap/steam/common/.local/share/Steam"] {
        roots.push(home.join(rel));
    }
    if cfg!(target_os = "macos") {
        roots.push(home.join(MACOS_STEAM));
    }
    roots
}

/// The macOS Steam client's folder under `$HOME`, with its own `steamapps` (the default library).
#[cfg(not(windows))]
const MACOS_STEAM: &str = "Library/Application Support/Steam";
/// CrossOver's bottles under `$HOME` (each bottle is a Wine prefix).
#[cfg(not(windows))]
const CROSSOVER_BOTTLES: &str = "Library/Application Support/CrossOver/Bottles";
/// Whisky's bottles under `$HOME`, inside Whisky's sandbox container.
#[cfg(not(windows))]
const WHISKY_BOTTLES: &str = "Library/Containers/com.isaacmarovitz.Whisky/Bottles";

fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) { a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()) } else { a == b }
}

/// Wine prefixes that may hold the game: Proton's in every Steam library (NotProton's too, on macOS), Lutris's
/// (`~/Games/<game>`), Heroic's (`~/Games/Heroic/Prefixes/default/<game>`), `~/.wine` and, on macOS, CrossOver's
/// bottles.
#[cfg(not(windows))]
fn wine_prefixes() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = steam_libraries().iter().map(|l| l.join("steamapps").join("compatdata").join(STEAM_APP_ID).join("pfx")).collect();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        out.push(home.join(".wine"));
        let mut parents = vec![home.join("Games"), home.join("Games/Heroic/Prefixes/default"), home.join("Games/Heroic/Prefixes")];
        if cfg!(target_os = "macos") {
            parents.push(home.join(CROSSOVER_BOTTLES));
        }
        for parent in parents {
            out.extend(subfolders(&parent));
        }
    }
    out
}

/// macOS: Whisky's bottles, looked at only when no other prefix holds the journals. They sit in another app's
/// sandbox container, and macOS asks the user before letting Elite Nexus look there. Elsewhere: none.
#[cfg(not(windows))]
fn sandboxed_wine_prefixes() -> Vec<PathBuf> {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if cfg!(target_os = "macos") => subfolders(&home.join(WHISKY_BOTTLES)),
        _ => Vec::new(),
    }
}

#[cfg(not(windows))]
fn subfolders(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect()
}

/// The journal folders in a Wine prefix: one per Windows user that has played (Proton's is `steamuser`).
#[cfg(not(windows))]
fn journal_dirs_in_prefix(prefix: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(prefix.join("drive_c").join("users"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|user| JOURNAL_IN_PROFILE.iter().fold(user.path(), |p, c| p.join(c)))
        .filter(|d| d.is_dir())
        .collect()
}

/// A journal file with the session start time and part number decoded from its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalFile {
    pub path: PathBuf,
    pub file_name: String,
    /// Session start time encoded in the file name (local time of the game client, treated as UTC).
    pub started: Timestamp,
    pub part: u32,
    pub size: u64,
}

/// Decodes `Journal.2026-10-12T073031.01.log` (current) and `Journal.170404170616.01.log` (pre-2021).
pub fn parse_journal_file_name(name: &str) -> Option<(Timestamp, u32)> {
    let rest = name.strip_prefix("Journal.")?.strip_suffix(".log")?;
    let (stamp, part) = rest.rsplit_once('.')?;
    let part: u32 = part.parse().ok()?;
    // ASCII only: the slices below are byte positions (any file in the folder is looked at).
    let ts = if stamp.len() == 17 && stamp.is_ascii() && stamp.as_bytes()[10] == b'T' {
        // 2026-10-12T073031
        let iso = format!("{}T{}:{}:{}Z", &stamp[..10], &stamp[11..13], &stamp[13..15], &stamp[15..17]);
        Timestamp::parse(&iso)?
    } else if stamp.len() == 12 && stamp.bytes().all(|b| b.is_ascii_digit()) {
        // 170404170616 -> 2017-04-04T17:06:16
        let iso = format!(
            "20{}-{}-{}T{}:{}:{}Z",
            &stamp[0..2],
            &stamp[2..4],
            &stamp[4..6],
            &stamp[6..8],
            &stamp[8..10],
            &stamp[10..12]
        );
        Timestamp::parse(&iso)?
    } else {
        return None;
    };
    Some((ts, part))
}

/// What a folder chosen as the journal folder holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalFolderCheck {
    /// `files` journals in `dir`: the folder itself, or the game's folder under it when a folder above it was chosen
    /// (`Saved Games`, `Frontier Developments` or the Windows user folder).
    Journals { dir: PathBuf, files: usize },
    /// It opens, but holds no `Journal.*.log` file.
    Empty,
    /// It can't be opened: missing, an offline network share, no permission.
    Unreadable(String),
}

/// Looks for journals in `dir`, then in the game's journal folder under it.
pub fn check_journal_folder(dir: &Path) -> JournalFolderCheck {
    let files = match list_journal_files(dir) {
        Ok(f) => f.len(),
        Err(e) => return JournalFolderCheck::Unreadable(e.to_string()),
    };
    if files > 0 {
        return JournalFolderCheck::Journals { dir: dir.to_path_buf(), files };
    }
    // Picked one level too high: <user>, <user>\Saved Games or ...\Frontier Developments.
    for depth in (1..=JOURNAL_IN_PROFILE.len()).rev() {
        let below = JOURNAL_IN_PROFILE[JOURNAL_IN_PROFILE.len() - depth..].iter().fold(dir.to_path_buf(), |p, c| p.join(c));
        if let Ok(f) = list_journal_files(&below)
            && !f.is_empty()
        {
            return JournalFolderCheck::Journals { dir: below, files: f.len() };
        }
    }
    JournalFolderCheck::Empty
}

/// All `Journal.*.log` files in `dir`, oldest first.
pub fn list_journal_files(dir: &Path) -> std::io::Result<Vec<JournalFile>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let Some((started, part)) = parse_journal_file_name(&file_name) else { continue };
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(JournalFile { path: entry.path(), file_name, started, part, size });
    }
    out.sort_by(|a, b| (a.started, a.part, &a.file_name).cmp(&(b.started, b.part, &b.file_name)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_both_file_name_styles() {
        let (t, p) = parse_journal_file_name("Journal.2026-10-12T073031.01.log").unwrap();
        assert_eq!(t.to_iso(), "2026-10-12T07:30:31Z");
        assert_eq!(p, 1);
        let (t, p) = parse_journal_file_name("Journal.170404170616.02.log").unwrap();
        assert_eq!(t.to_iso(), "2017-04-04T17:06:16Z");
        assert_eq!(p, 2);
        assert!(parse_journal_file_name("Status.json").is_none());
        // 17 bytes but not ASCII: skipped, not sliced in the middle of a letter.
        assert!(parse_journal_file_name("Journal.2026-10-12T0é303.01.log").is_none());
    }

    #[test]
    fn reads_steam_library_folders() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"label\"\t\t\"\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}";
        assert_eq!(parse_library_folders(vdf), vec![PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\SteamLibrary")]);
        let linux = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"/home/cmdr/.local/share/Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"/mnt/games/SteamLibrary\"\n\t}\n}";
        assert_eq!(parse_library_folders(linux), vec![PathBuf::from("/home/cmdr/.local/share/Steam"), PathBuf::from("/mnt/games/SteamLibrary")]);
        // A library's apps and their sizes are numbered too, but aren't folders.
        let apps = "\t\"1\"\n\t{\n\t\t\"path\"\t\t\"E:\\\\Games\\\\Steam\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"359320\"\t\t\"71241906176\"\n\t\t}\n\t}";
        assert_eq!(parse_library_folders(apps), vec![PathBuf::from(r"E:\Games\Steam")]);
        // Before 2021: numbered values directly under LibraryFolders.
        let old = "\"LibraryFolders\"\n{\n\t\"TimeNextStatsReport\"\t\t\"1634567890\"\n\t\"ContentStatsID\"\t\t\"-4485329811450862347\"\n\t\"1\"\t\t\"D:\\\\SteamLibrary\"\n\t\"2\"\t\t\"E:\\\\Steam Games\"\n}";
        assert_eq!(parse_library_folders(old), vec![PathBuf::from(r"D:\SteamLibrary"), PathBuf::from(r"E:\Steam Games")]);
    }

    #[test]
    fn cleans_registry_folders() {
        assert_eq!(clean_registry_path("  \"D:\\SteamLibrary\\steamapps\\common\\Elite Dangerous\\\" "), Some(PathBuf::from(r"D:\SteamLibrary\steamapps\common\Elite Dangerous")));
        assert_eq!(clean_registry_path(r"D:\"), Some(PathBuf::from(r"D:\")));
        assert_eq!(clean_registry_path(r"\\MAINPC\Games\Elite Dangerous\"), Some(PathBuf::from(r"\\MAINPC\Games\Elite Dangerous")));
        assert_eq!(clean_registry_path(" \"\" "), None);
        // Steam writes its own folder with forward slashes.
        #[cfg(windows)]
        assert_eq!(clean_registry_path("d:/program files (x86)/steam"), Some(PathBuf::from(r"d:\program files (x86)\steam")));
    }

    #[test]
    fn spots_the_game_in_installed_programs() {
        assert!(is_game_entry("Steam App 359320", ""));
        assert!(is_game_entry("steam app 359320", "Whatever Steam calls it"));
        assert!(is_game_entry("{696F8871-C91D-4CB1-825D-36BE18065575}_is1", "Elite Dangerous"));
        assert!(!is_game_entry("Steam App 570", "Dota 2"));
        assert!(!is_game_entry("EDDiscovery", "EDDiscovery"));
    }

    /// The registry is read without trouble (what it holds depends on the PC).
    #[cfg(windows)]
    #[test]
    fn reads_the_registry() {
        for dir in registry::steam_paths().into_iter().chain(registry::game_dirs()) {
            assert!(!dir.as_os_str().is_empty());
        }
    }

    #[test]
    fn checks_a_chosen_journal_folder() {
        let root = std::env::temp_dir().join(format!("nexus-journal-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let journals = JOURNAL_IN_PROFILE.iter().fold(root.clone(), |p, c| p.join(c));
        std::fs::create_dir_all(&journals).unwrap();
        assert_eq!(check_journal_folder(&journals), JournalFolderCheck::Empty);
        assert!(matches!(check_journal_folder(&root.join("missing")), JournalFolderCheck::Unreadable(_)));
        std::fs::write(journals.join("Journal.2026-10-12T073031.01.log"), "").unwrap();
        std::fs::write(journals.join("Status.json"), "").unwrap();
        let found = JournalFolderCheck::Journals { dir: journals.clone(), files: 1 };
        assert_eq!(check_journal_folder(&journals), found);
        // A folder above it was picked: the user folder, Saved Games, Frontier Developments.
        assert_eq!(check_journal_folder(&root), found);
        assert_eq!(check_journal_folder(&root.join("Saved Games")), found);
        assert_eq!(check_journal_folder(&root.join("Saved Games").join("Frontier Developments")), found);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn finds_options_next_to_the_journals() {
        let pfx = PathBuf::from("steamapps").join("compatdata").join(STEAM_APP_ID).join("pfx").join("drive_c").join("users").join("steamuser");
        let journals = pfx.join("Saved Games").join("Frontier Developments").join("Elite Dangerous");
        let options = pfx.join("AppData").join("Local").join("Frontier Developments").join("Elite Dangerous").join("Options");
        assert_eq!(options_dir_beside(&journals), Some(options));
        assert_eq!(options_dir_beside(&PathBuf::from("replays").join("Elite Dangerous")), None);
    }

    #[test]
    fn finds_screenshots_next_to_the_journals() {
        let root = std::env::temp_dir().join(format!("nexus-journal-shots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let user = root.join("drive_c").join("users").join("steamuser");
        let journals = JOURNAL_IN_PROFILE.iter().fold(user.clone(), |p, c| p.join(c));
        let shots = |pictures: &str| user.join(pictures).join("Frontier Developments").join("Elite Dangerous");
        // Before the game has saved one, the folder it will make.
        assert_eq!(screenshots_dir_beside(&journals), Some(shots("Pictures")));
        // An older Wine names the folder My Pictures.
        std::fs::create_dir_all(user.join("My Pictures")).unwrap();
        assert_eq!(screenshots_dir_beside(&journals), Some(shots("My Pictures")));
        std::fs::create_dir_all(user.join("Pictures")).unwrap();
        assert_eq!(screenshots_dir_beside(&journals), Some(shots("Pictures")));
        assert_eq!(screenshots_dir_beside(&PathBuf::from("replays").join("Elite Dangerous")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// NotProton (Steam Play on macOS) keeps Proton's layout inside the macOS Steam library; CrossOver bottles and
    /// prefixes made before NotProton's `steamuser` layout name the Windows user `crossover`.
    #[cfg(not(windows))]
    #[test]
    fn finds_journals_in_a_notproton_prefix() {
        let root = std::env::temp_dir().join(format!("nexus-journal-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pfx = root.join(MACOS_STEAM).join("steamapps").join("compatdata").join(STEAM_APP_ID).join("pfx");
        let journals = |user: &str| JOURNAL_IN_PROFILE.iter().fold(pfx.join("drive_c").join("users").join(user), |p, c| p.join(c));
        std::fs::create_dir_all(journals("steamuser")).unwrap();
        std::fs::create_dir_all(pfx.join("drive_c").join("users").join("Public")).unwrap();
        assert_eq!(journal_dirs_in_prefix(&pfx), vec![journals("steamuser")]);
        let options = pfx.join("drive_c/users/steamuser/AppData/Local/Frontier Developments/Elite Dangerous/Options");
        assert_eq!(options_dir_beside(&journals("steamuser")), Some(options));
        std::fs::create_dir_all(journals("crossover")).unwrap();
        let mut both = journal_dirs_in_prefix(&pfx);
        both.sort();
        assert_eq!(both, vec![journals("crossover"), journals("steamuser")]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
