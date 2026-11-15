//! Putting a new Elite Nexus in place. While the old one runs, the new program waits beside it as
//! `EliteNexus.new.exe` (the name tools/build-release.ps1 and Start-EliteNexus.cmd use for the same thing); once the
//! old one has closed, it takes its name. Only the program and its documents change: the data folder is never
//! touched.

use crate::archive::File;
use std::io;
use std::path::{Path, PathBuf};

/// Why this copy of Elite Nexus doesn't update itself. The app then says so and offers the release page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocked {
    /// Built from source: a debug build, or one in a cargo `target` folder.
    Source,
    /// Started from inside its zip, which Windows unpacks to a new temporary folder every time.
    InsideZip,
    /// Its folder can't be written to (Program Files, /usr/bin, a package manager's folder).
    ReadOnly(PathBuf),
    /// A Mac: replacing the app bundle hasn't been tried on a Mac yet.
    Mac,
    /// No release archive is made for this computer (only Windows and Linux on x64 update themselves).
    Platform(String),
}

impl Blocked {
    /// In plain words, for the banner and Settings.
    pub fn why(&self) -> String {
        match self {
            Blocked::Source => "This copy was built from source: update it as you built it.".into(),
            Blocked::InsideZip => "Elite Nexus is running from inside its zip file, so it can't replace itself: unzip it into a folder of its own first.".into(),
            Blocked::ReadOnly(dir) => format!("Elite Nexus can't write to its own folder ({}), so it can't replace itself: update it the way you installed it.", dir.display()),
            Blocked::Mac => "On a Mac, Elite Nexus doesn't replace its app yet: download the new one and drag it to Applications, over the old one. Your data stays where it is.".into(),
            Blocked::Platform(p) => format!("Releases have no archive for this computer ({p}), so it can't update itself."),
        }
    }
}

/// Whether the program at `exe` can be replaced by a release archive, and if not, why.
pub fn installable(exe: &Path) -> Result<(), Blocked> {
    installable_as(exe, cfg!(debug_assertions), &crate::manifest::this_platform())
}

fn installable_as(exe: &Path, debug: bool, platform: &str) -> Result<(), Blocked> {
    if platform.starts_with("macos") {
        return Err(Blocked::Mac);
    }
    if platform != "windows-x64" && platform != "linux-x64" {
        return Err(Blocked::Platform(platform.to_string()));
    }
    let dir = exe.parent().ok_or(Blocked::Source)?;
    let in_target = dir.components().any(|c| {
        let c = c.as_os_str().to_string_lossy().to_ascii_lowercase();
        c == "target" || c.starts_with("target-")
    });
    if debug || in_target {
        return Err(Blocked::Source);
    }
    if inside_zip(dir) {
        return Err(Blocked::InsideZip);
    }
    if !writable(dir) {
        return Err(Blocked::ReadOnly(dir.to_path_buf()));
    }
    Ok(())
}

/// Explorer runs a program opened from inside a zip from a temporary `Temp1_EliteNexus-….zip` folder.
fn inside_zip(dir: &Path) -> bool {
    dir.components().any(|c| {
        let name = c.as_os_str().to_string_lossy().to_ascii_lowercase();
        name.starts_with("temp") && name.ends_with(".zip")
    })
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".elite-nexus-update-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Where the new program waits while the old one runs: `EliteNexus.new.exe` beside `EliteNexus.exe`.
pub fn staged_path(exe: &Path) -> PathBuf {
    with_suffix(exe, ".new")
}

/// Where the old program goes when the new one takes its place (Windows, where a running program can be renamed but
/// not deleted): `EliteNexus.old.exe`, deleted at the next start.
pub fn old_path(exe: &Path) -> PathBuf {
    with_suffix(exe, ".old")
}

/// `EliteNexus.exe` with `suffix` before its extension.
fn with_suffix(exe: &Path, suffix: &str) -> PathBuf {
    let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "EliteNexus".into());
    let ext = exe.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    exe.with_file_name(format!("{stem}{suffix}{ext}"))
}

/// What [`stage`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    /// The staged program's SHA-256.
    pub sha256: String,
    /// Documents that couldn't be replaced (open in an editor, say), with why: they stay as they were.
    pub kept: Vec<String>,
}

/// Puts the new version beside the running one: `program` from `files` as [`staged_path`], and the documents over the
/// old ones (nothing reads them while Elite Nexus runs).
pub fn stage(exe: &Path, files: &[File], program: &str) -> io::Result<Staged> {
    let dir = exe.parent().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the program has no folder"))?;
    let new = files.iter().find(|f| f.name == program).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{program} isn't in the archive")))?;
    write_replacing(&staged_path(exe), &new.bytes, true)?;
    let mut kept = Vec::new();
    for f in files.iter().filter(|f| f.name != program) {
        // A document that can't be replaced stays as it was: the program is what matters.
        if let Err(e) = write_replacing(&dir.join(&f.name), &f.bytes, false) {
            kept.push(format!("{}: {e}", f.name));
        }
    }
    Ok(Staged { sha256: crate::sha256_hex(&new.bytes), kept })
}

/// A file's half-written copy while it's being written: `README.md.part`.
fn part_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{name}.part"))
}

/// Writes `bytes` to a temporary file beside `path`, then moves it over `path`: a reader never sees half a file.
fn write_replacing(path: &Path, bytes: &[u8], program: bool) -> io::Result<()> {
    let tmp = part_path(path);
    std::fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    if program {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    let _ = program;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Whether the program waiting at [`staged_path`] is the one Elite Nexus downloaded (`sha256`), not a file something
/// else left there.
pub fn staged_matches(exe: &Path, sha256: &str) -> bool {
    std::fs::read(staged_path(exe)).is_ok_and(|b| crate::sha256_hex(&b) == sha256)
}

/// Gives up a staged program (a newer version is out, or the commander updated another way).
pub fn discard_staged(exe: &Path) {
    let _ = std::fs::remove_file(staged_path(exe));
}

/// Puts the staged program in place of `exe`, once `exe` no longer runs. Windows: `exe` moves aside to [`old_path`]
/// and the new program takes its name; if that fails, the old one moves back. Elsewhere the new program replaces it in
/// one step. On an error nothing has changed.
pub fn swap(exe: &Path) -> io::Result<()> {
    let staged = staged_path(exe);
    if !staged.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} isn't there", staged.display())));
    }
    if cfg!(windows) {
        let old = free_old_path(exe);
        rename_patiently(exe, &old)?;
        if let Err(e) = rename_patiently(&staged, exe) {
            let _ = rename_patiently(&old, exe);
            return Err(e);
        }
        Ok(())
    } else {
        rename_patiently(&staged, exe)
    }
}

/// A rename, tried again for up to three seconds while something else still holds the file (a virus scanner reading
/// the program that was just written).
fn rename_patiently(from: &Path, to: &Path) -> io::Result<()> {
    let mut tries = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(e) if tries < 10 && e.kind() != io::ErrorKind::NotFound => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            done => return done,
        }
    }
}

/// [`old_path`], or `EliteNexus.old-2.exe` and on while an older one is still in use (a copy an AI agent started).
fn free_old_path(exe: &Path) -> PathBuf {
    let first = old_path(exe);
    let _ = std::fs::remove_file(&first);
    if !first.exists() {
        return first;
    }
    for n in 2..100 {
        let p = with_suffix(exe, &format!(".old-{n}"));
        let _ = std::fs::remove_file(&p);
        if !p.exists() {
            return p;
        }
    }
    with_suffix(exe, &format!(".old-{}", std::process::id()))
}

/// Deletes what an update left beside the program: the old program(s) a swap moved aside, and half-written files.
/// Called at start; a file still in use stays until the next time.
pub fn clean_up(exe: &Path) {
    let Some(dir) = exe.parent() else { return };
    let old = old_path(exe);
    let old_name = old.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let numbered_old = name.starts_with(&format!("{stem}.old-"));
        let half_written = name.ends_with(".part") && (name.starts_with(&stem) || crate::archive::DOCUMENTS.iter().any(|d| name.starts_with(d)));
        if name == old_name || numbered_old || half_written {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Starts the program at `exe` with `args` and doesn't wait for it.
pub fn launch(exe: &Path, args: &[&str]) -> io::Result<()> {
    std::process::Command::new(exe).args(args).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nexus-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    fn exe_in(dir: &Path) -> PathBuf {
        dir.join(if cfg!(windows) { "EliteNexus.exe" } else { "EliteNexus" })
    }

    fn files(program: &str) -> Vec<File> {
        vec![File { name: program.into(), bytes: b"new program".to_vec() }, File { name: "README.md".into(), bytes: b"new readme".to_vec() }]
    }

    #[test]
    fn names_beside_the_program() {
        assert_eq!(staged_path(Path::new("C:/Games/EliteNexus/EliteNexus.exe")), Path::new("C:/Games/EliteNexus/EliteNexus.new.exe"));
        assert_eq!(old_path(Path::new("C:/Games/EliteNexus/EliteNexus.exe")), Path::new("C:/Games/EliteNexus/EliteNexus.old.exe"));
        assert_eq!(staged_path(Path::new("/home/cmdr/EliteNexus/EliteNexus")), Path::new("/home/cmdr/EliteNexus/EliteNexus.new"));
    }

    #[test]
    fn stage_then_swap_keeps_the_data_folder() {
        let dir = temp("swap");
        let exe = exe_in(&dir);
        std::fs::write(&exe, b"old program").expect("exe");
        std::fs::write(dir.join("README.md"), b"old readme").expect("readme");
        std::fs::create_dir_all(dir.join("nexus-data")).expect("data");
        std::fs::write(dir.join("nexus-data").join("settings.json"), b"{\"mine\":true}").expect("settings");
        let program = exe.file_name().expect("name").to_string_lossy().into_owned();

        // A half-written file from an update that was cut short is cleaned up too.
        std::fs::write(dir.join("NOTICE.md.part"), b"half").expect("part");
        let staged = stage(&exe, &files(&program), &program).expect("stage");
        let sha = staged.sha256;
        assert!(staged.kept.is_empty(), "{:?}", staged.kept);
        assert_eq!(sha, crate::sha256_hex(b"new program"));
        assert!(staged_matches(&exe, &sha));
        assert!(!staged_matches(&exe, &crate::sha256_hex(b"something else")));
        assert_eq!(std::fs::read(&exe).expect("exe"), b"old program", "the running program is left alone");
        assert_eq!(std::fs::read(dir.join("README.md")).expect("readme"), b"new readme");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(staged_path(&exe)).expect("staged").permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "the new program can run");
        }

        swap(&exe).expect("swap");
        assert_eq!(std::fs::read(&exe).expect("exe"), b"new program");
        assert!(!staged_path(&exe).exists());
        assert_eq!(std::fs::read(dir.join("nexus-data").join("settings.json")).expect("settings"), b"{\"mine\":true}");
        if cfg!(windows) {
            assert_eq!(std::fs::read(old_path(&exe)).expect("old"), b"old program", "moved aside, deleted at the next start");
        }
        clean_up(&exe);
        assert!(!old_path(&exe).exists());
        assert!(!dir.join("NOTICE.md.part").exists());
        assert!(exe.exists());
        assert!(dir.join("README.md").exists());
        // Nothing staged: an error, and nothing changes.
        assert!(swap(&exe).is_err());
        assert_eq!(std::fs::read(&exe).expect("exe"), b"new program");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_staged_file_can_be_given_up() {
        let dir = temp("discard");
        let exe = exe_in(&dir);
        std::fs::write(&exe, b"old").expect("exe");
        let program = exe.file_name().expect("name").to_string_lossy().into_owned();
        stage(&exe, &files(&program), &program).expect("stage");
        discard_staged(&exe);
        assert!(!staged_path(&exe).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_that_dont_update_themselves() {
        let dir = temp("installable");
        let exe = exe_in(&dir);
        assert_eq!(installable_as(&exe, false, "windows-x64"), Ok(()));
        assert_eq!(installable_as(&exe, false, "linux-x64"), Ok(()));
        assert_eq!(installable_as(&exe, true, "windows-x64"), Err(Blocked::Source), "a debug build");
        assert_eq!(installable_as(Path::new("C:/src/EliteNexus/target/release/EliteNexus.exe"), false, "windows-x64"), Err(Blocked::Source));
        assert_eq!(installable_as(Path::new("C:/src/EliteNexus/target-agents/me/release/EliteNexus.exe"), false, "windows-x64"), Err(Blocked::Source));
        assert_eq!(installable_as(Path::new("C:/Users/c/AppData/Local/Temp/Temp1_EliteNexus-v0.8.3-windows-x64.zip/EliteNexus.exe"), false, "windows-x64"), Err(Blocked::InsideZip));
        assert_eq!(installable_as(&exe, false, "macos-arm64"), Err(Blocked::Mac));
        assert_eq!(installable_as(&exe, false, "windows-arm64"), Err(Blocked::Platform("windows-arm64".into())));
        let missing = dir.join("no such folder").join("EliteNexus.exe");
        assert!(matches!(installable_as(&missing, false, "windows-x64"), Err(Blocked::ReadOnly(_))));
        for b in [Blocked::Source, Blocked::InsideZip, Blocked::ReadOnly(dir.clone()), Blocked::Mac, Blocked::Platform("x".into())] {
            assert!(b.why().ends_with('.'), "{b:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
