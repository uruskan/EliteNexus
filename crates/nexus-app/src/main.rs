//! Elite Nexus — the native Elite Dangerous companion.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai_tools;
mod app;
mod assistant;
mod core;
#[cfg(target_os = "linux")]
mod desktop_entry;
mod export;
mod finder;
mod folders;
mod icon;
mod jobs;
mod market;
mod notices;
mod overlay;
mod pages;
mod runplan;
mod secrets;
mod settings;
mod sync;
#[cfg(test)]
mod testutil;
mod theme;
mod tours;
mod tts;
mod untouched;
mod widgets;

fn app_icon() -> egui::IconData {
    // macOS: an app bundle's own icon (Contents/Resources/EliteNexus.icns) is the one in the Dock. An empty icon
    // keeps eframe from replacing it with this small square one at start.
    if cfg!(target_os = "macos") && settings::runs_from_app_bundle() {
        return egui::IconData::default();
    }
    egui::IconData { rgba: icon::rgba(64), width: 64, height: 64 }
}

/// Holds an exclusive lock on instance.lock for the life of the process. A second instance would
/// tail the same journal and double-upload to EDDN/EDSM/Inara, so it must not start.
fn single_instance(data_dir: &std::path::Path) -> Option<std::fs::File> {
    let path = data_dir.join("instance.lock");
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).share_mode(0).open(path).ok()
    }
    #[cfg(not(windows))]
    {
        // An advisory lock (flock), released when the process ends.
        let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path).ok()?;
        file.try_lock().ok()?;
        Some(file)
    }
}

fn already_running_message() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
        let text: Vec<u16> = "Elite Nexus is already running.\0".encode_utf16().collect();
        let title: Vec<u16> = "Elite Nexus\0".encode_utf16().collect();
        // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call.
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION);
        }
    }
    #[cfg(not(windows))]
    eprintln!("Elite Nexus is already running.");
}

/// Linux: `--install-desktop` / `--uninstall-desktop` put Elite Nexus in the app menu, or take it out, from a
/// terminal. Returns the exit code when one of them was given.
#[cfg(target_os = "linux")]
fn desktop_entry_cli() -> Option<i32> {
    let (install, uninstall) = (std::env::args().any(|a| a == "--install-desktop"), std::env::args().any(|a| a == "--uninstall-desktop"));
    if !install && !uninstall {
        return None;
    }
    let Some(home) = settings::user_data_base() else {
        eprintln!("HOME is not set, so the app menu's folder can't be found.");
        return Some(1);
    };
    let done = if install {
        std::env::current_exe().and_then(|exe| desktop_entry::install(&home, &exe)).map(|()| format!("Added Elite Nexus to the app menu: {}", desktop_entry::entry_path(&home).display()))
    } else {
        desktop_entry::uninstall(&home).map(|()| "Removed Elite Nexus from the app menu.".to_string())
    };
    match done {
        Ok(msg) => {
            println!("{msg}");
            Some(0)
        }
        Err(e) => {
            eprintln!("Couldn't change the app menu: {e}");
            Some(1)
        }
    }
}

fn main() -> eframe::Result {
    #[cfg(target_os = "linux")]
    if let Some(code) = desktop_entry_cli() {
        std::process::exit(code);
    }
    let data_dir = settings::data_dir();
    let screenshot_run = std::env::args().any(|a| a == "--screenshot");
    let _instance = match single_instance(&data_dir) {
        Some(lock) => Some(lock),
        None if screenshot_run => None,
        None => {
            already_running_message();
            return Ok(());
        }
    };
    let log_file = std::fs::OpenOptions::new().create(true).append(true).open(data_dir.join("nexus.log")).ok();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,wgpu=warn,naga=warn,eframe=warn".into());
    match log_file {
        Some(f) => tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false).with_writer(std::sync::Mutex::new(f)).init(),
        None => tracing_subscriber::fmt().with_env_filter(filter).init(),
    }
    tracing::info!(version = env!("CARGO_PKG_VERSION"), data = %data_dir.display(), "Elite Nexus starting");
    // A panic is written straight to crash.log (nexus.log may not be open, and a release build has no console), with
    // where it happened: the next start says so, and the commander can send the file. Nothing is uploaded.
    let crash_log = data_dir.join("crash.log");
    static REPORTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        tracing::error!("panic in {name}: {info}");
        // The first ones matter: a panic the core survives can repeat with every update of a file, and the backtraces
        // would bury the first report (and slow the thread down). The window's own thread always gets in: its panic
        // is the one that closes Elite Nexus.
        let n = REPORTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n >= 10 && name != "main" {
            return;
        }
        let more = if n == 9 { "Further panics this run are only in nexus.log.\n\n" } else { "" };
        let when = nexus_journal::Timestamp::now().to_local_string("%Y-%m-%d %H:%M:%S");
        let text = format!("{when} Elite Nexus {} panicked in thread '{name}': {info}\n{}\n\n{more}", env!("CARGO_PKG_VERSION"), std::backtrace::Backtrace::force_capture());
        // Kept to about a megabyte: the previous one moves aside.
        if std::fs::metadata(&crash_log).is_ok_and(|m| m.len() > 1_000_000) {
            let _ = std::fs::rename(&crash_log, crash_log.with_extension("old.log"));
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&crash_log) {
            use std::io::Write;
            let _ = f.write_all(text.as_bytes());
        }
    }));

    // Optional: --page <name>  --screenshot <file.png>
    let args: Vec<String> = std::env::args().collect();
    let arg = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let start_page = arg("--page").and_then(|p| {
        pages::Page::GROUPS
            .iter()
            .flat_map(|(_, ps)| ps.iter())
            .find(|pg| pg.label().replace(' ', "").eq_ignore_ascii_case(&p.replace(' ', "")) || format!("{pg:?}").eq_ignore_ascii_case(&p))
            .copied()
    });
    let delay = std::time::Duration::from_secs_f64(arg("--shot-delay").and_then(|s| s.parse().ok()).unwrap_or(0.0));
    let shot = arg("--screenshot").map(|p| app::ScreenshotJob::new(p.into(), delay));
    let screenshot_mode = shot.is_some();
    // The overlay's see-through window needs the renderer set up for it now (Settings › Overlay, after a restart).
    let see_through = !screenshot_mode && settings::Settings::overlay_see_through(&data_dir);
    let overrides = app::Overrides {
        journal_dir: arg("--journal-dir").map(Into::into),
        mute: args.iter().any(|a| a == "--mute"),
        tab: arg("--tab"),
        demo: args.iter().any(|a| a == "--demo"),
        overlay_demo: arg("--overlay"),
        overlay_see_through: see_through,
    };

    // --size 1000x700: window size for responsive-layout screenshots.
    let size = arg("--size").and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?])
    });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Elite Nexus")
        .with_app_id("elite-nexus")
        .with_inner_size(size.unwrap_or([1480.0, 920.0]))
        .with_min_inner_size([880.0, 600.0])
        .with_icon(app_icon());
    if screenshot_mode {
        // Render far outside the desktop, unfocused and without a taskbar button: UI checks must
        // not pop up over the game. On X11 a window off every screen counts as hidden and is never
        // painted, so Linux runs stay on screen: they belong on a virtual display (tools/dev/linux.sh).
        viewport = viewport.with_active(false).with_taskbar(false);
        if cfg!(windows) {
            viewport = viewport.with_position([-20_000.0, -20_000.0]);
        }
    }
    #[allow(unused_mut)] // changed on Windows only
    let mut options = eframe::NativeOptions {
        // eframe sets every window's renderer up from this one flag; the main window itself stays opaque (below).
        viewport: viewport.with_transparent(see_through),
        // Screenshot runs keep their own state and never restore a window size (eframe would
        // otherwise fall back to a default folder on C: and reuse the last geometry).
        persist_window: !screenshot_mode,
        persistence_path: Some(data_dir.join(if screenshot_mode { "screenshot-ui-state.ron" } else { "ui-state.ron" })),
        window_builder: Some(Box::new(|builder: egui::ViewportBuilder| builder.with_transparent(false))),
        ..Default::default()
    };
    // Windows: a DirectX 12 swap chain made on a window is opaque, and so is Vulkan's there; one made through
    // DirectComposition can be see-through. A computer without DirectX 12 still starts, with an opaque overlay.
    #[cfg(windows)]
    if see_through && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        use eframe::wgpu;
        setup.instance_descriptor.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
        setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters: &[wgpu::Adapter], surface: Option<&wgpu::Surface<'_>>| {
            let usable = |a: &&wgpu::Adapter| surface.is_none_or(|s| a.is_surface_supported(s));
            let rank = |a: &&wgpu::Adapter| {
                let info = a.get_info();
                (info.backend == wgpu::Backend::Dx12, info.device_type == wgpu::DeviceType::DiscreteGpu)
            };
            adapters.iter().filter(usable).max_by_key(rank).cloned().ok_or_else(|| "No graphics adapter can draw to the window".to_string())
        }));
    }
    eframe::run_native(
        "Elite Nexus",
        options,
        Box::new(move |cc| Ok(Box::new(app::NexusApp::new(cc, data_dir, start_page, shot, overrides)))),
    )
}
