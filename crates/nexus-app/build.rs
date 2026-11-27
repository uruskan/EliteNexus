//! Windows: embeds the app icon (the same procedural diamond as the window icon, no asset files)
//! and the version information into EliteNexus.exe, so Explorer, the taskbar and installers show them.

use std::path::PathBuf;

// The icon's design and renderer, shared with the app.
#[allow(dead_code)]
#[path = "src/icon.rs"]
mod icon;
use icon::pixel;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").expect("target OS");
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64")
        && matches!((os.as_str(), env.as_str()), ("windows", "msvc") | ("linux", "gnu"))
    {
        build_piper_worker(&out, os == "windows");
    }
    if os != "windows" {
        return;
    }
    let ico = out.join("app.ico");
    std::fs::write(&ico, make_ico(&[16, 20, 24, 32, 48, 64, 128, 256])).expect("write app.ico");

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut parts = version.split(['.', '-', '+']).map(|p| p.parse::<u16>().unwrap_or(0));
    let (major, minor, patch) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    let rc = format!(
        r#"1 ICON "{ico}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Elite Nexus"
      VALUE "FileDescription", "Elite Nexus - Elite Dangerous companion"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "EliteNexus"
      VALUE "LegalCopyright", "CMDR Alka Uruskan"
      VALUE "OriginalFilename", "EliteNexus.exe"
      VALUE "ProductName", "Elite Nexus"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = ico.display().to_string().replace('\\', "\\\\"),
    );
    let rc_path = out.join("app.rc");
    std::fs::write(&rc_path, rc).expect("write app.rc");
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional().expect("embed Windows resources");
}

fn build_piper_worker(out: &std::path::Path, windows: bool) {
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/dev");
    let source = dev.join("piper-worker.cpp");
    let vendor = dev.join("piper-vendor");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed={}", vendor.display());
    let mut cmd = cc::Build::new().cpp(true).get_compiler().to_command();
    cmd.current_dir(out);
    if windows {
        cmd.args(["/nologo", "/std:c++17", "/O2", "/MT", "/EHsc"])
            .arg(format!("/I{}", vendor.display()))
            .args(["/Fopiper-worker.obj", "/Fenexus-piper.exe"])
            .arg(source);
    } else {
        cmd.args(["-std=c++17", "-O2", "-I"]).arg(vendor).arg(source)
            .args(["-o", "nexus-piper", "-static-libstdc++", "-static-libgcc", "-ldl"]);
    }
    let status = cmd.status().expect("run the Piper worker compiler");
    assert!(status.success(), "compile the Piper worker");
}

/// A 32-bit DIB as stored in .ico files: header with doubled height, BGRA rows bottom-up, AND mask.
fn dib(n: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(40 + (n * n * 4) as usize);
    v.extend(40u32.to_le_bytes());
    v.extend((n as i32).to_le_bytes());
    v.extend((2 * n as i32).to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(32u16.to_le_bytes());
    v.extend(0u32.to_le_bytes()); // BI_RGB
    v.extend((n * n * 4).to_le_bytes());
    v.extend([0u8; 16]); // resolution + palette counts
    for y in (0..n).rev() {
        for x in 0..n {
            let [r, g, b, a] = pixel(n, x, y);
            v.extend([b, g, r, a]);
        }
    }
    // AND mask: all zero (transparency comes from the alpha channel), rows padded to 32 bits.
    let row = n.div_ceil(32) * 4;
    v.resize(v.len() + (row * n) as usize, 0);
    v
}

fn make_ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&n| dib(n)).collect();
    let mut out = Vec::new();
    out.extend(0u16.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // icon
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&n, img) in sizes.iter().zip(&images) {
        let dim = if n >= 256 { 0 } else { n as u8 };
        out.extend([dim, dim, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((img.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend(img);
    }
    out
}
