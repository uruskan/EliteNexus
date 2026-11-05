//! Uploads to EDDN / EDSM / Inara / EDAstro: forwards the core's live events to `nexus-uplink`.
//!
//! Only live events ever reach this (the history load never does), and every service stays off
//! until the user enables it in Accounts & sync.

use crate::core::{Core, EventContext, LiveMsg, Phase};
use crate::secrets;
use crate::settings::Accounts;
use crossbeam_channel::Receiver;
use nexus_uplink::{EdsmCredentials, InaraCredentials, Uplink, UplinkConfig, UplinkStatus, UploadContext};
use nexus_journal::{LineSource, RawEvent};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub struct SyncService {
    /// None for test runs: the uplink is never started, so its persisted queues are not touched.
    uplink: Option<Arc<Uplink>>,
    applied: UplinkConfig,
    /// Send the current session's profile to Inara once the journal history is loaded.
    inara_due: bool,
}

impl SyncService {
    pub fn start(core: &Core, acc: &Accounts, data_dir: &Path, rt: &tokio::runtime::Handle) -> SyncService {
        let cfg = config(acc);
        let uplink = Arc::new(Uplink::new(cfg.clone(), data_dir, rt));
        let inara_due = cfg.inara_enabled();
        let sync = SyncService { uplink: Some(uplink), applied: cfg, inara_due };
        sync.attach(core);
        sync
    }

    /// No uploads at all (screenshot / replay runs).
    pub fn disabled() -> SyncService {
        SyncService { uplink: None, applied: UplinkConfig::default(), inara_due: false }
    }

    /// Call every frame: brings the Inara profile up to date when it is due (at start-up and
    /// when sync is switched on or the key changes) and the journal history has been read.
    pub fn tick(&mut self, core: &Core) {
        if !self.inara_due || !matches!(core.status.read().phase, Phase::Live) {
            return;
        }
        self.inara_due = false;
        let Some(u) = &self.uplink else { return };
        let events = current_session(core);
        if events.is_empty() {
            return;
        }
        let ctx = upload_context(&EventContext::capture(&core.model.read()));
        u.sync_inara_profile(events, &ctx);
    }

    /// Asks Inara again who the key belongs to.
    pub fn check_inara(&self) {
        if let Some(u) = &self.uplink {
            u.check_inara();
        }
    }

    /// Follows a (re)started core. The previous forwarder ends when its core stops.
    pub fn attach(&self, core: &Core) {
        if let Some(u) = &self.uplink {
            forward(core.subscribe(), u.clone());
        }
    }

    /// Call after the account settings changed (decrypts the keys, so not every frame).
    pub fn apply(&mut self, acc: &Accounts) {
        let Some(u) = &self.uplink else { return };
        let cfg = config(acc);
        if cfg != self.applied {
            // Switched on, a new key, or resumed after a pause: the profile is brought up to date.
            let inara_key = |c: &UplinkConfig| c.inara_credentials().map(|i| i.api_key.clone());
            if cfg.inara_enabled() && (!self.applied.inara_enabled() || inara_key(&cfg) != inara_key(&self.applied)) {
                self.inara_due = true;
            }
            u.set_config(cfg.clone());
            self.applied = cfg;
        }
    }

    pub fn status(&self) -> UplinkStatus {
        self.uplink.as_ref().map(|u| u.status()).unwrap_or_default()
    }

    pub fn any_enabled(&self) -> bool {
        self.applied.any_enabled()
    }

    /// Sends what is batched now (rate limits still apply; the rest stays queued on disk).
    pub fn flush(&self) {
        if let Some(u) = &self.uplink {
            u.flush();
        }
    }
}

/// The current game session (its Fileheader/LoadGame to now) as raw events. Each journal file is
/// read once and sliced by the index offsets.
fn current_session(core: &Core) -> Vec<RawEvent> {
    let idx = core.index.read();
    let (Some(load), Some(header)) = (idx.id_of("LoadGame"), idx.id_of("Fileheader")) else {
        return Vec::new();
    };
    let Some(li) = idx.entries.iter().rposition(|e| e.name == load) else {
        return Vec::new();
    };
    let start = idx.entries[..li].iter().rposition(|e| e.name == header && e.file == idx.entries[li].file).unwrap_or(li);
    let mut files: HashMap<u32, Vec<u8>> = HashMap::new();
    idx.entries[start..]
        .iter()
        .filter_map(|e| {
            let bytes = files.entry(e.file).or_insert_with(|| idx.files.get(e.file as usize).and_then(|p| std::fs::read(p).ok()).unwrap_or_default());
            let line = bytes.get(e.offset as usize..(e.offset as usize + e.len as usize))?;
            RawEvent::new(String::from_utf8_lossy(line).trim_end(), LineSource::default())
        })
        .collect()
}

fn forward(rx: Receiver<LiveMsg>, uplink: Arc<Uplink>) {
    let spawned = std::thread::Builder::new().name("nexus-sync".into()).spawn(move || {
        // Watcher order is preserved (EDSM swaps Cargo/ShipLocker events for their companion files).
        for msg in rx {
            match msg {
                LiveMsg::Event(e, c) => uplink.on_event(&e, &upload_context(&c)),
                LiveMsg::Companion(cmp, c) => uplink.on_companion(&cmp, &upload_context(&c)),
            }
        }
    });
    if let Err(e) = spawned {
        tracing::error!("sync: cannot start the forwarding thread: {e}");
    }
}

fn upload_context(c: &EventContext) -> UploadContext {
    UploadContext {
        cmdr: c.cmdr.clone(),
        fid: c.fid.clone(),
        game_version: c.game_version.clone(),
        game_build: c.game_build.clone(),
        horizons: c.horizons,
        odyssey: c.odyssey,
        system_name: c.system_name.clone(),
        system_address: c.system_address,
        star_pos: c.star_pos,
        market_id: c.market_id,
        station_name: c.station_name.clone(),
        ship_id: c.ship_id,
        is_beta: nexus_uplink::json::is_beta_version(&c.game_version),
    }
}

/// The uplink configuration for the user's choices (keys decrypted here, never logged).
pub fn config(acc: &Accounts) -> UplinkConfig {
    UplinkConfig {
        eddn: acc.eddn_upload,
        eddn_test_mode: acc.eddn_test_mode,
        eddn_paused: acc.eddn_paused,
        edsm: acc.edsm_enabled.then(|| EdsmCredentials {
            commander_name: acc.edsm_commander.trim().to_string(),
            api_key: secrets::open(&acc.edsm_api_key),
        }),
        edsm_paused: acc.edsm_paused,
        inara: acc.inara_enabled.then(|| InaraCredentials { api_key: secrets::open(&acc.inara_api_key) }),
        inara_paused: acc.inara_paused,
        edastro: acc.edastro_enabled,
        edastro_paused: acc.edastro_paused,
        ..UplinkConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_upload_nothing() {
        let s = crate::settings::Settings::default();
        assert!(!config(&s.accounts).any_enabled());
    }

    #[test]
    fn edsm_needs_name_and_key() {
        let acc = Accounts { edsm_enabled: true, edsm_commander: "Jameson".into(), ..Default::default() };
        assert!(!config(&acc).edsm_enabled(), "no key yet");
        let acc = Accounts { edsm_api_key: secrets::seal("k"), ..acc };
        assert!(config(&acc).edsm_enabled());
    }

    /// The header's button pauses every service that's on, and resumes them; one that's off isn't touched.
    #[test]
    fn pause_every_upload_that_is_on() {
        let mut acc = Accounts { eddn_upload: true, edsm_enabled: true, edsm_commander: "Jameson".into(), edsm_api_key: secrets::seal("k"), edastro_enabled: true, ..Default::default() };
        assert_eq!(acc.uploads(), [("EDDN", false), ("EDSM", false), ("EDAstro", false)], "Inara is off");
        acc.pause_uploads(true);
        assert_eq!(acc.uploads(), [("EDDN", true), ("EDSM", true), ("EDAstro", true)]);
        assert!(!acc.inara_paused);
        assert!(!config(&acc).any_enabled(), "nothing is sent");
        acc.pause_uploads(false);
        assert!(config(&acc).edsm_enabled() && config(&acc).eddn_enabled() && config(&acc).edastro_enabled());
    }
}
