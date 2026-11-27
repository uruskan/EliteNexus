//! Install the pinned Piper runtime and Kristin voice into the user's data directory.

use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(windows)]
const INSTALL_DIR: &str = "piper-kristin-sherpa-1.12.20";
#[cfg(target_os = "linux")]
const INSTALL_DIR: &str = "piper-kristin-sherpa-1.12.20-linux";
const STAGING_DIR: &str = "piper-download";
const MODEL: &str = "model/en_US-kristin-medium.onnx";
#[cfg(windows)]
pub(super) const HELPER: &str = "nexus-piper.exe";
#[cfg(target_os = "linux")]
pub(super) const HELPER: &str = "nexus-piper";
#[cfg(windows)]
const LIBRARIES: &[&str] = &[
    "sherpa-onnx-c-api.dll",
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
];
#[cfg(target_os = "linux")]
const LIBRARIES: &[&str] = &["libsherpa-onnx-c-api.so", "libonnxruntime.so"];

pub(super) fn installed() -> Option<PathBuf> {
    // Keep existing portable Piper packages and their seeded audio working.
    portable_folder().filter(|path| complete(path)).or_else(|| {
        let local = crate::settings::data_dir().join(INSTALL_DIR);
        complete(&local).then_some(local)
    })
}

fn portable_folder() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("piper"))
}

fn complete(folder: &Path) -> bool {
    folder.join(HELPER).is_file()
        && folder.join(MODEL).is_file()
        && folder.join("model/tokens.txt").is_file()
        && folder.join("model/espeak-ng-data").is_dir()
        && LIBRARIES
            .iter()
            .all(|library| folder.join(library).is_file())
}

pub(super) fn lock_file(root: &Path) -> Result<fs::File> {
    fs::create_dir_all(root).context("creating the Elite Nexus data folder")?;
    fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("piper-install.lock"))
        .context("opening the Piper installation lock")
}

pub(super) fn remove() -> Result<()> {
    let root = crate::settings::data_dir();
    let lock = lock_file(&root)?;
    lock.try_lock()
        .context("Piper is in use. Close other Elite Nexus instances and try again")?;
    for folder in [
        Some(root.join(INSTALL_DIR)),
        Some(root.join(STAGING_DIR)),
        portable_folder().filter(|path| complete(path)),
        Some(root.join("piper-audio")),
    ]
    .into_iter()
    .flatten()
    {
        remove_folder(&folder)?;
    }
    Ok(())
}

fn remove_folder(folder: &Path) -> Result<()> {
    match fs::remove_dir_all(folder) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", folder.display())),
    }
}

#[cfg(all(
    target_arch = "x86_64",
    any(
        all(windows, target_env = "msvc"),
        all(target_os = "linux", target_env = "gnu")
    )
))]
pub(super) fn install() -> Result<()> {
    let root = std::path::absolute(crate::settings::data_dir())?;
    let lock = lock_file(&root)?;
    let locked = lock.try_lock();
    if installed().is_some() {
        return Ok(());
    }
    locked.context("Piper is in use or another Elite Nexus instance is installing it")?;
    let staging = root.join(STAGING_DIR);
    // A process that was closed during a download may have left partial files here.
    remove_folder(&staging)?;
    fs::create_dir(&staging)?;
    let result = install_staged(&staging, &root.join(INSTALL_DIR));
    let cleanup = remove_folder(&staging);
    result.and(cleanup)
}

#[cfg(not(all(
    target_arch = "x86_64",
    any(
        all(windows, target_env = "msvc"),
        all(target_os = "linux", target_env = "gnu")
    )
)))]
pub(super) fn install() -> Result<()> {
    anyhow::bail!("Automatic Piper installation requires a supported Windows or Linux x64 build")
}

#[cfg(all(
    target_arch = "x86_64",
    any(
        all(windows, target_env = "msvc"),
        all(target_os = "linux", target_env = "gnu")
    )
))]
fn install_staged(staging: &Path, dest: &Path) -> Result<()> {
    use anyhow::ensure;
    use std::time::Duration;

    #[cfg(windows)]
    const RUNTIME: (&str, u64, &str) = (
        "sherpa-onnx-v1.12.20-win-x64-shared",
        26_662_986,
        "2aa9c11f13d6a20308afedcaa8c04a678a1aaebc206b7609e4df7d7910582f56",
    );
    #[cfg(target_os = "linux")]
    const RUNTIME: (&str, u64, &str) = (
        "sherpa-onnx-v1.12.20-linux-x64-shared",
        26_742_838,
        "13a2c502f5ef373f514a5fc5c5ad03cc0798dcb564a6f8cbd54f8f6ce9e47462",
    );
    const VOICE: &str = "vits-piper-en_US-kristin-medium";
    const RELEASES: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(600))
        .build()?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    for (release, name, size, digest) in [
        ("v1.12.20", RUNTIME.0, RUNTIME.1, RUNTIME.2),
        (
            "tts-models",
            VOICE,
            67_259_230,
            "c2206f572df2956c50b1ae3367eebce3853c663e890cba8048cd62b1e4dbe6c7",
        ),
    ] {
        let archive = staging.join(format!("{name}.tar.bz2"));
        rt.block_on(download(
            &client,
            &format!("{RELEASES}/{release}/{name}.tar.bz2"),
            &archive,
            size,
            digest,
        ))
        .with_context(|| format!("downloading {name}"))?;
        extract(&archive, staging).with_context(|| format!("extracting {name}"))?;
        fs::remove_file(archive)?;
    }

    let ready = staging.join("ready");
    fs::create_dir(&ready)?;
    fs::rename(staging.join(VOICE), ready.join("model"))?;
    for library in LIBRARIES {
        fs::rename(
            staging.join(RUNTIME.0).join("lib").join(library),
            ready.join(library),
        )?;
    }
    #[cfg(windows)]
    let worker = include_bytes!(concat!(env!("OUT_DIR"), "/nexus-piper.exe"));
    #[cfg(target_os = "linux")]
    let worker = include_bytes!(concat!(env!("OUT_DIR"), "/nexus-piper"));
    let helper = ready.join(HELPER);
    fs::write(&helper, worker)?;
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755))?;
    }
    fs::write(
        ready.join("LICENSE-sherpa-onnx.txt"),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/dev/piper-vendor/LICENSE-sherpa-onnx.txt"
        )),
    )?;
    fs::write(
        ready.join("ONNXRuntime-LICENSE"),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/dev/piper-vendor/ONNXRuntime-LICENSE"
        )),
    )?;
    fs::write(
        ready.join("ONNXRuntime-ThirdPartyNotices.txt"),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/dev/piper-vendor/ONNXRuntime-ThirdPartyNotices.txt"
        )),
    )?;
    ensure!(complete(&ready), "the extracted Piper files are incomplete");
    remove_folder(dest)?;
    fs::rename(ready, dest).context("activating the Piper installation")?;
    Ok(())
}

#[cfg(all(windows, target_arch = "x86_64", target_env = "msvc"))]
fn extract(archive: &Path, dest: &Path) -> Result<()> {
    use std::{os::windows::process::CommandExt, process::Command};

    let system_root =
        std::env::var_os("SystemRoot").context("finding the Windows system folder")?;
    let tar = PathBuf::from(system_root).join("System32/tar.exe");
    anyhow::ensure!(
        tar.is_absolute() && tar.is_file(),
        "Windows tar.exe is unavailable"
    );
    let result = Command::new(tar)
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .creation_flags(0x0800_4000) // CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS
        .output()
        .context("running Windows tar.exe")?;
    anyhow::ensure!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
fn extract(archive: &Path, dest: &Path) -> Result<()> {
    tar::Archive::new(bzip2::read::BzDecoder::new(fs::File::open(archive)?)).unpack(dest)?;
    Ok(())
}

#[cfg(all(
    target_arch = "x86_64",
    any(
        all(windows, target_env = "msvc"),
        all(target_os = "linux", target_env = "gnu")
    )
))]
async fn download(
    client: &reqwest::Client,
    url: &str,
    path: &std::path::Path,
    size: u64,
    digest: &str,
) -> Result<()> {
    use anyhow::ensure;
    use sha2::{Digest, Sha256};
    use std::{fs::File, io::Write};

    let mut response = client.get(url).send().await?.error_for_status()?;
    let mut file = File::create_new(path)?;
    let mut sha = Sha256::new();
    let mut received = 0;
    while let Some(data) = response.chunk().await? {
        received += data.len() as u64;
        ensure!(received <= size, "archive exceeds the expected size");
        sha.update(&data);
        file.write_all(&data)?;
    }
    ensure!(
        received == size && format!("{:x}", sha.finalize()) == digest,
        "archive failed integrity verification"
    );
    file.flush()?;
    Ok(())
}
