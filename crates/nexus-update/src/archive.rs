//! What Elite Nexus takes from a release archive: the program and the documents beside it, by name, and nothing
//! else. Entries are read into memory, never written where the archive says, so a path in the archive can't reach
//! anywhere on the computer.
//!
//! The release workflow packs the Windows zip with the files at its root and the Linux tar.gz with them in one folder
//! (`EliteNexus-v0.8.3-linux-x64/`); both are read.

use std::io::Read;

/// The documents every release archive carries beside the program (release.yml › Package).
pub const DOCUMENTS: [&str; 5] = ["LICENSE", "README.md", "NOTICE.md", "USER_GUIDE.md", "Inter-LICENSE.txt"];

/// The largest program accepted (today's is about 50 MB).
const MAX_PROGRAM: u64 = 400 * 1024 * 1024;
/// The largest document accepted.
const MAX_DOCUMENT: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zip,
    TarGz,
}

impl Kind {
    /// From the archive's file name.
    pub fn of(name: &str) -> Option<Kind> {
        let name = name.to_ascii_lowercase();
        if name.ends_with(".zip") {
            Some(Kind::Zip)
        } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Some(Kind::TarGz)
        } else {
            None
        }
    }
}

/// A file taken from the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Its name, without a folder.
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("the archive can't be read ({0})")]
    Read(String),
    #[error("{0} isn't in the archive")]
    Missing(String),
    #[error("{0} in the archive is larger than Elite Nexus could be")]
    TooLarge(String),
}

/// Takes `program` (`EliteNexus.exe`, `EliteNexus`) and the [`DOCUMENTS`] from `archive`, from its root or from one
/// folder at its root; every other entry is skipped. The program must be there, the documents may be missing.
pub fn extract(archive: &[u8], kind: Kind, program: &str) -> Result<Vec<File>, ArchiveError> {
    let wanted = |path: &str| -> Option<String> {
        let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect();
        let name = *parts.last()?;
        (parts.len() <= 2 && !parts.contains(&"..") && (name == program || DOCUMENTS.contains(&name))).then(|| name.to_string())
    };
    let mut out: Vec<File> = Vec::new();
    let mut take = |name: String, reader: &mut dyn Read, size: u64| -> Result<(), ArchiveError> {
        if out.iter().any(|f| f.name == name) {
            return Ok(());
        }
        let limit = if name == program { MAX_PROGRAM } else { MAX_DOCUMENT };
        if size > limit {
            return Err(ArchiveError::TooLarge(name));
        }
        let mut bytes = Vec::with_capacity(size as usize);
        // A size the archive misstates still can't make it read more than the limit.
        reader.take(limit + 1).read_to_end(&mut bytes).map_err(|e| ArchiveError::Read(format!("{name}: {e}")))?;
        if bytes.len() as u64 > limit {
            return Err(ArchiveError::TooLarge(name));
        }
        out.push(File { name, bytes });
        Ok(())
    };
    match kind {
        Kind::Zip => {
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(|e| ArchiveError::Read(e.to_string()))?;
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i).map_err(|e| ArchiveError::Read(e.to_string()))?;
                if !entry.is_file() {
                    continue;
                }
                let Some(name) = entry.name().ok().and_then(|n| wanted(&n)) else { continue };
                let size = entry.size();
                take(name, &mut entry, size)?;
            }
        }
        Kind::TarGz => {
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
            for entry in tar.entries().map_err(|e| ArchiveError::Read(e.to_string()))? {
                let mut entry = entry.map_err(|e| ArchiveError::Read(e.to_string()))?;
                if !entry.header().entry_type().is_file() {
                    continue;
                }
                let Some(name) = entry.path().ok().and_then(|p| wanted(&p.to_string_lossy())) else { continue };
                let size = entry.size();
                take(name, &mut entry, size)?;
            }
        }
    }
    if !out.iter().any(|f| f.name == program) {
        return Err(ArchiveError::Missing(program.to_string()));
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A zip of `files` (path, body), deflated as Compress-Archive does.
    pub(crate) fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (path, body) in files {
            w.start_file(*path, opts).expect("entry");
            w.write_all(body).expect("body");
        }
        w.finish().expect("zip").into_inner()
    }

    /// A tar.gz of `files` (path, body).
    pub(crate) fn tar_gz_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut b = tar::Builder::new(gz);
        for (path, body) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o755);
            h.set_entry_type(tar::EntryType::Regular);
            h.set_cksum();
            b.append_data(&mut h, path, *body).expect("entry");
        }
        b.into_inner().expect("tar").finish().expect("gz")
    }

    fn names(files: &[File]) -> Vec<&str> {
        files.iter().map(|f| f.name.as_str()).collect()
    }

    #[test]
    fn the_windows_zip_has_its_files_at_the_root() {
        // As release.yml packs it: Compress-Archive of the folder's contents.
        let zip = zip_of(&[("EliteNexus.exe", b"MZ new program"), ("LICENSE", b"GPL"), ("README.md", b"readme"), ("NOTICE.md", b"notice"), ("USER_GUIDE.md", b"guide"), ("Inter-LICENSE.txt", b"OFL")]);
        let files = extract(&zip, Kind::Zip, "EliteNexus.exe").expect("extract");
        assert_eq!(names(&files), ["EliteNexus.exe", "LICENSE", "README.md", "NOTICE.md", "USER_GUIDE.md", "Inter-LICENSE.txt"]);
        assert_eq!(files[0].bytes, b"MZ new program");
    }

    #[test]
    fn the_linux_tar_has_them_in_one_folder() {
        let tar = tar_gz_of(&[("EliteNexus-v0.8.4-linux-x64/EliteNexus", b"\x7fELF new"), ("EliteNexus-v0.8.4-linux-x64/README.md", b"readme")]);
        let files = extract(&tar, Kind::TarGz, "EliteNexus").expect("extract");
        assert_eq!(names(&files), ["EliteNexus", "README.md"]);
        assert_eq!(files[0].bytes, b"\x7fELF new");
    }

    #[test]
    fn only_known_files_near_the_root_are_taken() {
        let zip = zip_of(&[
            ("deep/er/EliteNexus.exe", b"too deep"),
            ("nexus-data/settings.json", b"{}"),
            ("other.dll", b"not ours"),
            ("EliteNexus\\EliteNexus.exe", b"one folder, backslash"),
            ("EliteNexus.exe", b"a second copy"),
        ]);
        let files = extract(&zip, Kind::Zip, "EliteNexus.exe").expect("extract");
        assert_eq!(names(&files), ["EliteNexus.exe"]);
        assert_eq!(files[0].bytes, b"one folder, backslash", "the first one counts");
        // No program: an error, whatever else is there.
        let zip = zip_of(&[("README.md", b"readme")]);
        assert!(matches!(extract(&zip, Kind::Zip, "EliteNexus.exe"), Err(ArchiveError::Missing(_))));
        // The Linux program isn't the Windows one.
        let tar = tar_gz_of(&[("EliteNexus.exe", b"MZ")]);
        assert!(matches!(extract(&tar, Kind::TarGz, "EliteNexus"), Err(ArchiveError::Missing(_))));
        // Not an archive.
        assert!(matches!(extract(b"not a zip", Kind::Zip, "EliteNexus.exe"), Err(ArchiveError::Read(_))));
        assert!(matches!(extract(b"not a tar", Kind::TarGz, "EliteNexus"), Err(ArchiveError::Read(_))));
    }

    #[test]
    fn kinds_come_from_the_name() {
        assert_eq!(Kind::of("EliteNexus-v0.8.3-windows-x64.zip"), Some(Kind::Zip));
        assert_eq!(Kind::of("EliteNexus-v0.8.3-linux-x64.tar.gz"), Some(Kind::TarGz));
        assert_eq!(Kind::of("EliteNexus.dmg"), None);
    }
}
