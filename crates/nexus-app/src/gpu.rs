//! The graphics device: which one draws the window, and what happens when Windows (or the driver) takes it away.
//!
//! A lost device (a driver reset, the computer waking from sleep, the GPU stopping under load) can't be drawn on again,
//! and egui-wgpu 0.36/0.37 panics on the next frame ("Failed to create staging buffer", emilk/egui#8450) instead of
//! letting the window rebuild its renderer. So the loss is logged, and the panic hook starts a new copy (`--gpu-reset`)
//! that says what happened, once a minute at most so a device that keeps failing can't loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The command-line flag of a copy started after the last one lost its graphics device.
pub const RESTART_FLAG: &str = "--gpu-reset";

/// A run shorter than this doesn't restart itself: the device failing again at once would only loop.
const MIN_RUN: Duration = Duration::from_secs(60);

static LOST: AtomicBool = AtomicBool::new(false);

/// Logs which adapter draws the window and watches its device for loss (`cc.wgpu_render_state`).
pub fn watch(rs: Option<&eframe::egui_wgpu::RenderState>) {
    let Some(rs) = rs else { return };
    let info = rs.adapter.get_info();
    tracing::info!(adapter = %info.name, backend = ?info.backend, driver = %info.driver_info, "graphics");
    rs.device.set_device_lost_callback(|reason, message| {
        // `Destroyed` is the device being let go of on purpose (the window closing): nothing went wrong.
        if reason != eframe::wgpu::DeviceLostReason::Destroyed {
            LOST.store(true, Ordering::SeqCst);
            tracing::error!(?reason, %message, "graphics device lost: Windows or the driver reset the GPU");
        }
    });
}

/// Whether a panic comes from the graphics device being lost: the callback fired, or egui-wgpu's own message for it
/// (the callback can come a frame after the failed write that panics).
pub fn is_loss(panic_message: &str) -> bool {
    LOST.load(Ordering::SeqCst) || panic_message.contains("Failed to create staging buffer")
}

/// Starts a new copy after a lost device, when this one has run long enough. True when it started.
pub fn restart(started: Instant) -> bool {
    if started.elapsed() < MIN_RUN {
        tracing::warn!("graphics device lost {}s after start: not restarting", started.elapsed().as_secs());
        return false;
    }
    let launched = std::env::current_exe().and_then(|exe| std::process::Command::new(exe).arg(RESTART_FLAG).spawn());
    match launched {
        Ok(_) => {
            tracing::info!("graphics device lost: started a new copy");
            true
        }
        Err(e) => {
            tracing::error!("graphics device lost; couldn't start a new copy: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egui_wgpus_panic_counts_as_a_loss() {
        assert!(is_loss("Failed to create staging buffer for index data. Index count: 38631."));
        assert!(!LOST.load(Ordering::SeqCst) && !is_loss("index out of bounds"));
    }

    #[test]
    fn a_short_run_doesnt_restart() {
        assert!(!restart(Instant::now()));
    }
}
