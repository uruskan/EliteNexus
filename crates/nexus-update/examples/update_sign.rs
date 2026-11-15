//! The release side of updates: makes the signing key, writes a release's `update.json` and signs it.
//!
//!   cargo run -p nexus-update --example update_sign -- keygen <folder>
//!       A new key pair: <folder>/elite-nexus-update.key (secret: the UPDATE_SIGNING_KEY secret, and a backup kept
//!       offline) and <folder>/elite-nexus-update.pub (crates/nexus-update/src/minisign.pub). A new key means every
//!       version already out refuses the releases it signs: make one only to replace a lost or leaked key.
//!   cargo run -p nexus-update --example update_sign -- manifest <folder> <tag> <date>
//!       Writes <folder>/update.json for the release archives in <folder> (EliteNexus-v<version>-<platform>.zip or
//!       .tar.gz), to download from the release tagged <tag>.
//!   cargo run -p nexus-update --example update_sign -- sign <file>
//!       Writes <file>.minisig with the secret key in the UPDATE_SIGNING_KEY environment variable (the key file's text).
//!   cargo run -p nexus-update --example update_sign -- verify <file> [<public key file>]
//!       Checks <file>.minisig against the key Elite Nexus has compiled in, or the one given, as the app will.
//!
//! .github/workflows/release.yml runs manifest, sign and verify for every release.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    let done = match (arg(0), arg(1), arg(2), arg(3)) {
        (Some("keygen"), Some(dir), None, None) => keygen(Path::new(dir)),
        (Some("manifest"), Some(dir), Some(tag), Some(date)) => manifest(Path::new(dir), tag, date),
        (Some("sign"), Some(file), None, None) => sign(Path::new(file)),
        (Some("verify"), Some(file), key, None) => verify(Path::new(file), key.map(Path::new)),
        _ => Err("usage: update_sign keygen <folder> | manifest <folder> <tag> <date> | sign <file> | verify <file> [<public key file>]".to_string()),
    };
    match done {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("update_sign: {e}");
            ExitCode::FAILURE
        }
    }
}

fn keygen(dir: &Path) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (secret, public) = (dir.join("elite-nexus-update.key"), dir.join("elite-nexus-update.pub"));
    if secret.exists() || public.exists() {
        return Err(format!("{} already holds a key: it's never overwritten", dir.display()));
    }
    let pair = minisign::KeyPair::generate_unencrypted_keypair().map_err(|e| e.to_string())?;
    let sk = pair.sk.to_box(Some("Elite Nexus update signing key: keep it secret")).map_err(|e| e.to_string())?.into_string();
    let pk = pair.pk.to_box().map_err(|e| e.to_string())?.into_string();
    std::fs::write(&secret, sk).map_err(|e| format!("{}: {e}", secret.display()))?;
    std::fs::write(&public, pk).map_err(|e| format!("{}: {e}", public.display()))?;
    Ok(format!("Wrote {} (secret) and {}", secret.display(), public.display()))
}

fn manifest(dir: &Path, tag: &str, date: &str) -> Result<String, String> {
    let m = nexus_update::Manifest::from_release_files(dir, tag, date, nexus_update::REPO).map_err(|e| format!("{}: {e}", dir.display()))?;
    if m.version().is_none() {
        return Err(format!("{tag} isn't a version tag"));
    }
    if m.assets.is_empty() {
        return Err(format!("no release archives for {tag} in {}", dir.display()));
    }
    let out = dir.join(nexus_update::MANIFEST);
    std::fs::write(&out, m.to_json()).map_err(|e| format!("{}: {e}", out.display()))?;
    let platforms: Vec<&str> = m.assets.keys().map(String::as_str).collect();
    Ok(format!("Wrote {} for {} ({})", out.display(), m.version, platforms.join(", ")))
}

fn sig_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".minisig");
    PathBuf::from(name)
}

fn sign(file: &Path) -> Result<String, String> {
    let key = std::env::var("UPDATE_SIGNING_KEY").map_err(|_| "UPDATE_SIGNING_KEY isn't set: it holds the secret key file's text".to_string())?;
    let sk = minisign::SecretKeyBox::from_string(key.trim()).and_then(|b| b.into_unencrypted_secret_key()).map_err(|e| format!("UPDATE_SIGNING_KEY: {e}"))?;
    let pk = minisign::PublicKey::from_secret_key(&sk).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let sig = minisign::sign(Some(&pk), &sk, std::io::Cursor::new(&bytes), Some(&format!("file:{name}")), Some("Elite Nexus release signature")).map_err(|e| e.to_string())?;
    let out = sig_path(file);
    std::fs::write(&out, sig.into_string()).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(format!("Wrote {}", out.display()))
}

fn verify(file: &Path, key: Option<&Path>) -> Result<String, String> {
    let key = match key {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?,
        None => nexus_update::PUBLIC_KEY.to_string(),
    };
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let sig_file = sig_path(file);
    let sig = std::fs::read_to_string(&sig_file).map_err(|e| format!("{}: {e}", sig_file.display()))?;
    let m = nexus_update::Manifest::verified(&bytes, &sig, &key).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(format!("{} is signed with the key Elite Nexus checks: {} for {}", file.display(), m.version, m.assets.keys().cloned().collect::<Vec<_>>().join(", ")))
}
