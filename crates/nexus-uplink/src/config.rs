//! User-facing uplink configuration. Everything defaults to OFF: nothing is uploaded until the
//! user opts in to a service.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The EDDN `softwareName` / Inara `appName` / EDSM `fromSoftware`. Must stay constant: EDDN and
/// EDSM identify (and can block) misbehaving software by this name. Not `EDNexus` (taken).
pub const DEFAULT_SOFTWARE_NAME: &str = "Elite Nexus";

/// EDSM account credentials (https://www.edsm.net/en/settings/api).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EdsmCredentials {
    /// The commander name *as registered on EDSM* (may differ from the in-game name).
    pub commander_name: String,
    pub api_key: String,
}

impl fmt::Debug for EdsmCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EdsmCredentials")
            .field("commander_name", &self.commander_name)
            .field("api_key", &redact(&self.api_key))
            .finish()
    }
}

/// Inara personal API key (https://inara.cz/elite/cmdr-settings-api/).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InaraCredentials {
    pub api_key: String,
    /// Sent as `isBeingDeveloped`: Inara then ignores global events (community goals) so test
    /// data cannot pollute shared data. Inara whitelisted "Elite Nexus" on 2026-10-13; the app
    /// keeps this on by default for the first weeks after.
    pub is_being_developed: bool,
}

impl fmt::Debug for InaraCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InaraCredentials")
            .field("api_key", &redact(&self.api_key))
            .field("is_being_developed", &self.is_being_developed)
            .finish()
    }
}

fn redact(key: &str) -> &'static str {
    if key.is_empty() { "<empty>" } else { "<redacted>" }
}

/// Which services Elite Nexus uploads to. `Default` = everything off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UplinkConfig {
    /// `"Elite Nexus"`.
    pub software_name: String,
    /// Semantic version of the app. A pre-release version (`0.2.0-dev`) always uploads to the
    /// EDDN `/test` schemas, as EDDN requires for development builds.
    pub software_version: String,
    /// Upload to EDDN.
    pub eddn: bool,
    /// Use the EDDN `/test` schemas (always on for beta game clients).
    pub eddn_test_mode: bool,
    /// Temporarily stop sharing on EDDN (the commander's pause button). Nothing is sent or kept
    /// while paused: market data of the station you are docked at is dropped too.
    pub eddn_paused: bool,
    /// EDSM journal sync (None = off).
    pub edsm: Option<EdsmCredentials>,
    /// The same pause for EDSM: nothing is sent, and what happens meanwhile is dropped (the queue too).
    pub edsm_paused: bool,
    /// Inara sync (None = off).
    pub inara: Option<InaraCredentials>,
    /// The same pause for Inara (no key check either).
    pub inara_paused: bool,
    /// Send the events EDAstro accepts.
    pub edastro: bool,
    /// The same pause for EDAstro.
    pub edastro_paused: bool,
}

impl Default for UplinkConfig {
    fn default() -> Self {
        UplinkConfig {
            software_name: DEFAULT_SOFTWARE_NAME.to_string(),
            software_version: env!("CARGO_PKG_VERSION").to_string(),
            eddn: false,
            eddn_test_mode: false,
            eddn_paused: false,
            edsm: None,
            edsm_paused: false,
            inara: None,
            inara_paused: false,
            edastro: false,
            edastro_paused: false,
        }
    }
}

impl UplinkConfig {
    /// The software name actually sent (never empty).
    pub fn software(&self) -> &str {
        let s = self.software_name.trim();
        if s.is_empty() { DEFAULT_SOFTWARE_NAME } else { s }
    }

    /// The software version actually sent (never empty).
    pub fn version(&self) -> &str {
        let s = self.software_version.trim();
        if s.is_empty() { env!("CARGO_PKG_VERSION") } else { s }
    }

    /// EDSM credentials, if EDSM is enabled and both fields are filled in (paused or not).
    pub fn edsm_credentials(&self) -> Option<&EdsmCredentials> {
        self.edsm.as_ref().filter(|c| !c.commander_name.trim().is_empty() && !c.api_key.trim().is_empty())
    }

    /// The EDSM credentials to send with now: EDSM is on, with both fields filled in, and not paused.
    pub fn edsm_sending(&self) -> Option<&EdsmCredentials> {
        self.edsm_credentials().filter(|_| !self.edsm_paused)
    }

    /// Inara credentials, if Inara is enabled with a key (paused or not).
    pub fn inara_credentials(&self) -> Option<&InaraCredentials> {
        self.inara.as_ref().filter(|c| !c.api_key.trim().is_empty())
    }

    /// The Inara credentials to send with now: Inara is on, with a key, and not paused.
    pub fn inara_sending(&self) -> Option<&InaraCredentials> {
        self.inara_credentials().filter(|_| !self.inara_paused)
    }

    /// EDDN is on and not paused.
    pub fn eddn_enabled(&self) -> bool {
        self.eddn && !self.eddn_paused
    }

    /// EDSM is on, with its credentials, and not paused.
    pub fn edsm_enabled(&self) -> bool {
        self.edsm_sending().is_some()
    }

    /// Inara is on, with a key, and not paused.
    pub fn inara_enabled(&self) -> bool {
        self.inara_sending().is_some()
    }

    /// EDAstro is on and not paused.
    pub fn edastro_enabled(&self) -> bool {
        self.edastro && !self.edastro_paused
    }

    pub fn any_enabled(&self) -> bool {
        self.eddn_enabled() || self.edsm_enabled() || self.inara_enabled() || self.edastro_enabled()
    }

    /// True when EDDN messages must go to the `/test` schemas regardless of the game client.
    pub fn eddn_force_test(&self) -> bool {
        self.eddn_test_mode || is_prerelease(self.version())
    }

    /// User-Agent for every request.
    pub fn user_agent(&self) -> String {
        format!("EliteNexus/{}", self.version())
    }
}

/// Semver pre-release (`1.2.3-dev`, `1.2.3-beta.1`). Build metadata (`+abc`) is not a pre-release.
pub fn is_prerelease(version: &str) -> bool {
    let core = version.split('+').next().unwrap_or(version);
    core.contains('-')
}

/// Stable fingerprint of a credential set, persisted with queued items instead of the secret
/// itself, so items queued for one account are never sent with another account's key.
pub(crate) fn fingerprint(parts: &[&str]) -> u64 {
    let mut buf = Vec::new();
    for p in parts {
        buf.extend_from_slice(p.trim().as_bytes());
        buf.push(0);
    }
    crate::json::fnv1a64(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off() {
        let c = UplinkConfig::default();
        assert!(!c.any_enabled());
        assert_eq!(c.software(), "Elite Nexus");
        let parsed: UplinkConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn credentials_must_be_complete() {
        let c = UplinkConfig {
            edsm: Some(EdsmCredentials { commander_name: "Jameson".into(), api_key: " ".into() }),
            inara: Some(InaraCredentials { api_key: String::new(), is_being_developed: true }),
            ..UplinkConfig::default()
        };
        assert!(!c.edsm_enabled());
        assert!(!c.inara_enabled());
        assert!(!format!("{c:?}").contains("api_key: \" \""));
    }

    /// A paused service keeps its credentials (resuming isn't a new key) but sends nothing.
    #[test]
    fn paused_services_send_nothing() {
        let c = UplinkConfig {
            eddn: true,
            eddn_paused: true,
            edsm: Some(EdsmCredentials { commander_name: "Jameson".into(), api_key: "k".into() }),
            edsm_paused: true,
            inara: Some(InaraCredentials { api_key: "k".into(), is_being_developed: false }),
            inara_paused: true,
            edastro: true,
            edastro_paused: true,
            ..UplinkConfig::default()
        };
        assert!(!c.any_enabled());
        assert!(c.edsm_credentials().is_some() && c.edsm_sending().is_none());
        assert!(c.inara_credentials().is_some() && c.inara_sending().is_none());
        let resumed = UplinkConfig { edsm_paused: false, inara_paused: false, edastro_paused: false, ..c };
        assert!(resumed.edsm_enabled() && resumed.inara_enabled() && resumed.edastro_enabled());
        assert!(!resumed.eddn_enabled(), "each service has its own pause");
    }

    #[test]
    fn prerelease_forces_test() {
        assert!(is_prerelease("0.2.0-dev"));
        assert!(!is_prerelease("0.2.0"));
        assert!(!is_prerelease("0.2.0+build.5"));
        let c = UplinkConfig { software_version: "0.1.0-dev".into(), ..UplinkConfig::default() };
        assert!(c.eddn_force_test());
    }

    #[test]
    fn debug_redacts_keys() {
        let e = EdsmCredentials { commander_name: "X".into(), api_key: "secret123".into() };
        assert!(!format!("{e:?}").contains("secret123"));
        let i = InaraCredentials { api_key: "secret456".into(), is_being_developed: false };
        assert!(!format!("{i:?}").contains("secret456"));
    }
}
