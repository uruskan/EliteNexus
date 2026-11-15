//! The release manifest, `update.json`: the version and, for each platform, the archive's name, address, size and
//! SHA-256. Signed (`update.json.minisig`) with the key whose public half is compiled in, so the archive's checksum
//! comes from Elite Nexus' releases and not just from the server that sends the archive.
//!
//! Every version that has shipped reads this format: fields are only ever added (unknown ones are ignored), and a
//! format this version can't read (`schema` above [`SCHEMA`]) is reported, not guessed at.

use crate::version::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// The manifest format this version reads and writes.
pub const SCHEMA: u32 = 1;

/// The most a manifest or its signature may weigh: anything larger isn't one.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    /// The format, [`SCHEMA`].
    pub schema: u32,
    /// The release's version, without the `v` (`0.8.3`).
    pub version: String,
    /// Its tag (`v0.8.3`).
    pub tag: String,
    /// The day it was released (YYYY-MM-DD).
    pub date: String,
    /// The archive for each platform (`windows-x64`, `linux-x64`, `macos-arm64`), as the release page offers them.
    /// A platform the release has no archive for is missing (a release goes out without the Mac zip if its build
    /// fails).
    pub assets: BTreeMap<String, Asset>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Asset {
    /// The file's name on the release page (`EliteNexus-v0.8.3-windows-x64.zip`).
    pub name: String,
    /// Where it downloads from.
    pub url: String,
    /// Its size in bytes.
    pub size: u64,
    /// Its SHA-256, lowercase hex.
    pub sha256: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("it doesn't match its signature: it was changed or damaged on the way")]
    BadSignature,
    #[error("it's signed with another key than the one Elite Nexus releases are signed with")]
    WrongKey,
    #[error("its signature can't be read ({0})")]
    Signature(String),
    #[error("the key compiled into Elite Nexus can't be read ({0})")]
    Key(String),
    #[error("it can't be read ({0})")]
    Format(String),
    #[error("it's in a newer format ({0}) than this version reads")]
    NewerFormat(u32),
    #[error("it names no version Elite Nexus can read")]
    NoVersion,
}

impl Manifest {
    /// Checks `bytes` against `signature` (the text of `update.json.minisig`) with `public_key` (a minisign public key
    /// file, or its base64 line), then reads them. Nothing is read from a manifest whose signature doesn't hold.
    pub fn verified(bytes: &[u8], signature: &str, public_key: &str) -> Result<Manifest, ManifestError> {
        let key = public_key.trim();
        let key = if key.lines().count() > 1 { minisign_verify::PublicKey::decode(key) } else { minisign_verify::PublicKey::from_base64(key) };
        let key = key.map_err(|e| ManifestError::Key(e.to_string()))?;
        let signature = minisign_verify::Signature::decode(signature.trim()).map_err(|e| ManifestError::Signature(e.to_string()))?;
        // Legacy (unhashed) signatures are refused: every release is signed by update_sign, which prehashes.
        key.verify(bytes, &signature, false).map_err(|e| match e {
            minisign_verify::Error::InvalidSignature => ManifestError::BadSignature,
            minisign_verify::Error::UnexpectedKeyId => ManifestError::WrongKey,
            e => ManifestError::Signature(e.to_string()),
        })?;
        Manifest::read(bytes)
    }

    /// Reads a manifest without checking a signature (the signer, tests).
    pub fn read(bytes: &[u8]) -> Result<Manifest, ManifestError> {
        let m: Manifest = serde_json::from_slice(bytes).map_err(|e| ManifestError::Format(e.to_string()))?;
        if m.schema > SCHEMA {
            return Err(ManifestError::NewerFormat(m.schema));
        }
        if m.version().is_none() {
            return Err(ManifestError::NoVersion);
        }
        Ok(m)
    }

    pub fn version(&self) -> Option<Version> {
        Version::parse(&self.version)
    }

    /// The archive for `platform`, if the release has one with what it takes to download and check it.
    pub fn asset(&self, platform: &str) -> Option<&Asset> {
        self.assets.get(platform).filter(|a| !a.name.is_empty() && a.url.starts_with("https://") && a.size > 0 && a.sha256.len() == 64)
    }

    /// The manifest of the release archives in `dir` (`EliteNexus-v<version>-<platform>.zip` or `.tar.gz`, as the
    /// release workflow names them), to download from `<repository>/releases/download/<tag>/`.
    pub fn from_release_files(dir: &Path, tag: &str, date: &str, repo: &str) -> std::io::Result<Manifest> {
        let version = tag.strip_prefix(['v', 'V']).unwrap_or(tag).to_string();
        let mut assets = BTreeMap::new();
        let mut names: Vec<String> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for name in names {
            let Some(platform) = platform_of(&name, &version).map(str::to_string) else { continue };
            let bytes = std::fs::read(dir.join(&name))?;
            let asset = Asset { url: format!("https://github.com/{repo}/releases/download/{tag}/{name}"), size: bytes.len() as u64, sha256: crate::sha256_hex(&bytes), name };
            assets.insert(platform, asset);
        }
        Ok(Manifest { schema: SCHEMA, version, tag: tag.to_string(), date: date.to_string(), assets })
    }

    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_default();
        s.push('\n');
        s
    }
}

/// The platform a release archive is for, from its name: `EliteNexus-v0.8.3-windows-x64.zip` gives `windows-x64`.
/// Checksum files and anything else give None.
pub fn platform_of<'a>(file_name: &'a str, version: &str) -> Option<&'a str> {
    let rest = file_name.strip_prefix("EliteNexus-v")?.strip_prefix(version)?.strip_prefix('-')?;
    let platform = rest.strip_suffix(".zip").or_else(|| rest.strip_suffix(".tar.gz"))?;
    (!platform.is_empty() && platform.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')).then_some(platform)
}

/// This build's platform as release archives name it: `windows-x64`, `linux-x64`, `macos-arm64` (the Mac zip is named
/// after `uname -m`, so an Intel Mac is `macos-x86_64`).
pub fn this_platform() -> String {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "macos" => format!("macos-{}", if arch == "aarch64" { "arm64" } else { arch }),
        os => format!(
            "{os}-{}",
            match arch {
                "x86_64" => "x64",
                "aarch64" => "arm64",
                a => a,
            }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key pair of the tests' own and a signature for `bytes` with it: (public key file, signature file).
    pub(crate) fn signed(bytes: &[u8]) -> (String, String) {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().expect("key pair");
        let public = pair.pk.to_box().expect("public key box").into_string();
        let sig = minisign::sign(Some(&pair.pk), &pair.sk, std::io::Cursor::new(bytes), Some("test"), None).expect("signature").into_string();
        (public, sig)
    }

    fn sample() -> Manifest {
        let mut assets = BTreeMap::new();
        assets.insert(
            "windows-x64".to_string(),
            Asset { name: "EliteNexus-v0.8.4-windows-x64.zip".into(), url: "https://github.com/uruskan/EliteNexus/releases/download/v0.8.4/EliteNexus-v0.8.4-windows-x64.zip".into(), size: 1234, sha256: "a".repeat(64) },
        );
        Manifest { schema: SCHEMA, version: "0.8.4".into(), tag: "v0.8.4".into(), date: "2026-12-01".into(), assets }
    }

    #[test]
    fn a_signed_manifest_reads_and_a_changed_one_doesnt() {
        let json = sample().to_json();
        let (public, sig) = signed(json.as_bytes());
        let m = Manifest::verified(json.as_bytes(), &sig, &public).expect("verified");
        assert_eq!(m, sample());
        assert_eq!(m.version().map(|v| v.to_string()).as_deref(), Some("0.8.4"));
        // The base64 line alone works as the key too.
        let line = public.lines().nth(1).expect("key line");
        assert!(Manifest::verified(json.as_bytes(), &sig, line).is_ok());
        // One byte changed: refused.
        let changed = json.replace("1234", "1235");
        assert!(matches!(Manifest::verified(changed.as_bytes(), &sig, &public), Err(ManifestError::BadSignature)));
        // Another key: refused.
        let (other, _) = signed(b"other");
        assert!(matches!(Manifest::verified(json.as_bytes(), &sig, &other), Err(ManifestError::WrongKey)));
        // Garbage for a signature or a key.
        assert!(matches!(Manifest::verified(json.as_bytes(), "not a signature", &public), Err(ManifestError::Signature(_))));
        assert!(matches!(Manifest::verified(json.as_bytes(), &sig, "not a key"), Err(ManifestError::Key(_))));
    }

    #[test]
    fn unknown_fields_are_ignored_and_newer_formats_reported() {
        let json = r#"{"schema":1,"version":"0.9.0-beta.1","tag":"v0.9.0-beta.1","note":"later field","assets":{"linux-x64":{"name":"a.tar.gz","url":"https://x/a.tar.gz","size":5,"sha256":"0000000000000000000000000000000000000000000000000000000000000000","extra":1}}}"#;
        let m = Manifest::read(json.as_bytes()).expect("reads");
        assert!(m.asset("linux-x64").is_some());
        assert!(m.asset("windows-x64").is_none(), "no archive for this platform");
        assert!(matches!(Manifest::read(br#"{"schema":2,"version":"1.0.0"}"#), Err(ManifestError::NewerFormat(2))));
        assert!(matches!(Manifest::read(br#"{"schema":1,"version":"soon"}"#), Err(ManifestError::NoVersion)));
        assert!(matches!(Manifest::read(b"<html>"), Err(ManifestError::Format(_))));
        // An archive without what it takes to download and check it isn't offered.
        let mut m = sample();
        m.assets.get_mut("windows-x64").expect("asset").url = "http://insecure/a.zip".into();
        assert!(m.asset("windows-x64").is_none());
    }

    #[test]
    fn release_file_names_give_their_platform() {
        assert_eq!(platform_of("EliteNexus-v0.8.3-windows-x64.zip", "0.8.3"), Some("windows-x64"));
        assert_eq!(platform_of("EliteNexus-v0.8.3-linux-x64.tar.gz", "0.8.3"), Some("linux-x64"));
        assert_eq!(platform_of("EliteNexus-v0.9.0-beta.1-macos-arm64.zip", "0.9.0-beta.1"), Some("macos-arm64"));
        assert_eq!(platform_of("EliteNexus-v0.8.3-windows-x64.zip.sha256", "0.8.3"), None);
        assert_eq!(platform_of("EliteNexus-v0.8.2-windows-x64.zip", "0.8.3"), None, "another version");
        assert_eq!(platform_of("update.json", "0.8.3"), None);
        let p = this_platform();
        assert!(p.contains('-') && !p.contains(' '), "{p}");
        if cfg!(all(windows, target_arch = "x86_64")) {
            assert_eq!(p, "windows-x64");
        }
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(p, "linux-x64");
        }
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(p, "macos-arm64");
        }
    }

    #[test]
    fn the_manifest_of_a_release_folder() {
        let dir = std::env::temp_dir().join(format!("nexus-update-manifest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        for (name, body) in [("EliteNexus-v0.8.4-windows-x64.zip", "zip"), ("EliteNexus-v0.8.4-windows-x64.zip.sha256", "sum"), ("EliteNexus-v0.8.4-linux-x64.tar.gz", "tar"), ("notes.md", "notes")] {
            std::fs::write(dir.join(name), body).expect("file");
        }
        let m = Manifest::from_release_files(&dir, "v0.8.4", "2026-12-01", "uruskan/EliteNexus").expect("manifest");
        assert_eq!(m.version, "0.8.4");
        assert_eq!(m.assets.keys().collect::<Vec<_>>(), ["linux-x64", "windows-x64"]);
        let w = &m.assets["windows-x64"];
        assert_eq!(w.size, 3);
        assert_eq!(w.sha256, crate::sha256_hex(b"zip"));
        assert_eq!(w.url, "https://github.com/uruskan/EliteNexus/releases/download/v0.8.4/EliteNexus-v0.8.4-windows-x64.zip");
        // What it writes, it reads.
        assert_eq!(Manifest::read(m.to_json().as_bytes()).expect("reads"), m);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
