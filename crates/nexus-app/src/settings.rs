//! Persistent user settings (JSON in the data directory).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Override for the journal folder (None = auto-detect Saved Games).
    pub journal_dir: Option<PathBuf>,
    /// The game's folder (None = found automatically): HUD & cockpit and Bindings' built-in presets use it.
    pub game_dir: Option<PathBuf>,
    /// Speak alerts out loud.
    pub tts_enabled: bool,
    /// 0 = low .. 3 = critical: minimum priority that gets spoken.
    pub tts_min_priority: u8,
    pub tts_rate: f32,
    pub tts_volume: f32,
    pub toast_seconds: f32,
    pub ui_scale: f32,
    /// Exploration: bodies worth at least this (mapped) are highlighted.
    pub high_value_threshold: i64,
    /// Copy the next route system to the clipboard after each jump.
    pub auto_copy_next_system: bool,
    /// Switch pages by itself as the commander flies: Exploration while flying a system, Exobiology near a
    /// planet with life still to analyse, Navigation in the galaxy map, while a jump charges or when the
    /// system is done. Only while on one of those three pages.
    pub auto_page: bool,
    /// Altitude (km) below which Follow my flight treats the commander as exploring the planet rather than
    /// the system: EDEB's default of 150 km.
    pub auto_page_altitude_km: f32,
    pub accounts: Accounts,
    /// The first-run welcome has been completed.
    pub welcomed: bool,
    /// Engineering wishlist (blueprint planner).
    pub wishlist: Vec<WishItem>,
    /// Market price alerts, checked against every live market update.
    pub price_watches: Vec<PriceWatch>,
    /// The "Ask" assistant's model service.
    pub ai: AiSettings,
    /// HUD colour themes the commander saved (HUD colours page).
    pub hud_themes: Vec<HudTheme>,
    /// The Market page's last searches and filters.
    pub market: crate::pages::market::MarketPrefs,
    /// The carrier the Fleet carrier page and the carrier planner show, for commanders with both
    /// their own and a squadron carrier.
    pub carrier: nexus_model::carrier::CarrierKind,
    /// Colonisation: hidden depots, which groups are open.
    pub colonisation: crate::pages::colonisation::ColonisationPrefs,
    /// BGS & Powerplay: the period shown and the tick hour.
    pub bgs: crate::pages::bgs::BgsPrefs,
    /// Mining: the deposits the Rhino refined at (positions exist only live), notes on mining locations.
    pub mining: crate::pages::mining::MiningPrefs,
}

/// A named HUD colour matrix.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HudTheme {
    pub name: String,
    pub matrix: nexus_hud::HudMatrix,
}

/// Which model answers "Ask" questions. The key is DPAPI-sealed (see secrets.rs).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub provider: nexus_ai::ProviderKind,
    pub api_key: String,
    /// Empty = automatic (the cheapest suitable model the service offers).
    pub model: String,
    /// Empty = the provider's default address (needed only for "Other").
    pub base_url: String,
}

impl AiSettings {
    /// The provider configuration with the key unsealed (for one request; never logged).
    pub fn provider(&self) -> nexus_ai::ProviderConfig {
        nexus_ai::ProviderConfig { kind: self.provider, api_key: crate::secrets::open(&self.api_key), model: self.model.trim().to_string(), base_url: self.base_url.trim().to_string() }
    }

    pub fn is_ready(&self) -> bool {
        !self.provider.needs_key() || !self.api_key.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WishItem {
    pub label: String,
    pub blueprint: String,
    pub module: String,
    pub from: u8,
    pub to: u8,
    pub rolls: u32,
    pub experimental: Option<String>,
}

/// "Tell me when …": a market rule checked against every live market update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PriceWatch {
    /// Display name or symbol (`Tritium`, `lowtemperaturediamond`).
    pub commodity: String,
    /// true: a station pays at least `price` (for selling); false: sells for at most `price`.
    pub sell: bool,
    pub price: i64,
    /// From the current position; 0 = anywhere.
    pub max_ly: f64,
    pub min_pad: Option<char>,
    pub carriers: bool,
    /// Minimum demand (selling) or stock (buying).
    pub min_qty: i64,
    pub enabled: bool,
}

impl Default for PriceWatch {
    fn default() -> Self {
        PriceWatch { commodity: "Tritium".into(), sell: true, price: 50_000, max_ly: 50.0, min_pad: Some('L'), carriers: false, min_qty: 100, enabled: true }
    }
}

/// Website/service connections. Secrets are DPAPI-sealed (see secrets.rs).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Accounts {
    /// Contribute market/outfitting/shipyard/exploration data to EDDN (opt-in).
    pub eddn_upload: bool,
    /// Send to EDDN's /test schemas only (for verifying the setup).
    pub eddn_test_mode: bool,
    /// Receive the live EDDN market feed into the local galaxy database.
    pub eddn_listen: bool,
    pub edsm_enabled: bool,
    pub edsm_commander: String,
    pub edsm_api_key: String,
    pub inara_enabled: bool,
    pub inara_api_key: String,
    /// Sent as `isBeingDeveloped`. Inara approved "Elite Nexus" on 2026-10-13; on by default for the first weeks after.
    pub inara_dev_mode: bool,
    pub edastro_enabled: bool,
    /// EDDN sharing paused by the commander (the header's Pause button).
    pub eddn_paused: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            journal_dir: None,
            game_dir: None,
            tts_enabled: true,
            tts_min_priority: 1,
            tts_rate: 1.0,
            tts_volume: 0.9,
            toast_seconds: 7.0,
            ui_scale: 1.0,
            high_value_threshold: 1_000_000,
            auto_copy_next_system: true,
            auto_page: false,
            auto_page_altitude_km: 150.0,
            accounts: Accounts { eddn_listen: true, inara_dev_mode: true, ..Default::default() },
            welcomed: false,
            wishlist: Vec::new(),
            price_watches: Vec::new(),
            ai: AiSettings::default(),
            hud_themes: Vec::new(),
            market: Default::default(),
            carrier: Default::default(),
            colonisation: Default::default(),
            bgs: Default::default(),
            mining: Default::default(),
        }
    }
}

/// What was wrong with settings.json at start (see [`Settings::load`]), for the banner that says so.
#[derive(Debug, Clone, Default)]
pub struct LoadProblem {
    /// What went wrong and what was kept, in plain words.
    pub what: String,
    /// Off when the file couldn't be read, or couldn't be copied aside: nothing is saved this run, so the file stays
    /// as it is.
    pub can_save: bool,
}

impl Settings {
    /// Reads settings.json; a missing file gives the defaults. A file that can't be used as it is never means a
    /// silent start from scratch (settings are saved every half minute, which would overwrite it): it's copied
    /// aside first, every setting that still reads is kept (from a file that isn't valid JSON at all, the API keys),
    /// and the problem says what happened. A file that can't even be read isn't saved over this run.
    pub fn load(dir: &Path) -> (Self, Option<LoadProblem>) {
        let path = dir.join("settings.json");
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), None),
            Err(e) => {
                tracing::warn!("settings.json can't be read: {e}");
                let what = format!("Elite Nexus couldn't read settings.json ({e}). It started with the default settings and won't save over that file this time; restart it to try again.");
                // Not a first start: no welcome.
                return (Settings { welcomed: true, ..Settings::default() }, Some(LoadProblem { what, ..Default::default() }));
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        let utf8 = matches!(text, std::borrow::Cow::Borrowed(_));
        // (settings, what happened, advice for the end of the message)
        let (mut settings, mut what, advice) = match serde_json::from_str::<Settings>(&text) {
            Ok(s) if utf8 => return (s, None),
            Ok(s) => (s, "settings.json isn't UTF-8 text (an editor saved it in another encoding), so letters outside English in it may have been replaced.".to_string(), ""),
            Err(e) => match serde_json::from_str::<serde_json::Value>(&text) {
                // Valid JSON, but some settings have the wrong type: keep all the others.
                Ok(v) => {
                    let (s, dropped) = salvage(v);
                    let names: Vec<&str> = dropped.iter().map(String::as_str).collect();
                    let what = match names.len() {
                        0 => format!("settings.json couldn't be read ({e}), so Elite Nexus started with the default settings."),
                        n => format!("Some settings in settings.json couldn't be read ({e}): {} {} back at the default. Every other setting was kept.", and_list(&names), if n == 1 { "is" } else { "are" }),
                    };
                    (s, what, "")
                }
                // Not JSON at all, typically a folder typed in by hand with single backslashes.
                Err(_) => {
                    let (s, recovered) = recover_keys(&text);
                    let mut what = format!("settings.json isn't valid JSON ({e}), so Elite Nexus started with the default settings.");
                    if !recovered.is_empty() {
                        let keys = if recovered.len() == 1 { "key was" } else { "keys were" };
                        what += &format!(" Your {} {keys} recovered from it; uploads stay off until you switch them on again under Accounts & sync.", and_list(&recovered));
                    }
                    let advice = r" If you edited it by hand: JSON needs every backslash written twice, so \\PC\Elite Dangerous is written \\\\PC\\Elite Dangerous. Settings › Journal › Browse… sets folders without editing the file.";
                    (s, what, advice)
                }
            },
        };
        if !utf8 && !what.contains("UTF-8") {
            what += " It isn't UTF-8 text either (an editor saved it in another encoding), so letters outside English in it may have been replaced.";
        }
        let copy = dir.join(format!("settings-unreadable-{}.json", nexus_journal::Timestamp::now().to_local_string("%Y%m%d-%H%M%S")));
        let can_save = match write_private(&copy, &bytes) {
            Ok(()) => {
                what += &format!(" Your file as it was is kept as {} in the data folder.", copy.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
                true
            }
            Err(e) => {
                what += &format!(" Elite Nexus couldn't keep a copy of it ({e}), so it won't save over it this time: changes made now aren't kept.");
                false
            }
        };
        what += advice;
        // A settings file existed, so this isn't a first start: no welcome over the banner.
        settings.welcomed = true;
        tracing::warn!(%what, "settings.json: started without some settings");
        (settings, Some(LoadProblem { what, can_save }))
    }

    pub fn save(&self, dir: &Path) {
        let saved = serde_json::to_string_pretty(self).map_err(std::io::Error::other).and_then(|s| {
            let tmp = dir.join("settings.json.tmp");
            write_private(&tmp, s.as_bytes())?;
            std::fs::rename(&tmp, dir.join("settings.json"))
        });
        if let Err(e) = saved {
            tracing::warn!("settings.json not saved: {e}");
        }
    }
}

/// Writes a settings file only this user account may read: Linux and macOS keep API keys unencrypted (no DPAPI).
/// Created that way, and a file left from before is made so too.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    // The mode only applies to a new file: set it on one left from before too, before anything is written.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)
}

/// Valid JSON with some settings of the wrong type: each setting that still reads is kept, also one level into the
/// groups (`accounts`, `ai`, …). Returns the names of those that fell back to their defaults.
fn salvage(v: serde_json::Value) -> (Settings, Vec<String>) {
    use serde_json::Value;
    let reads = |v: &Value| serde_json::from_value::<Settings>(v.clone()).is_ok();
    let mut good = serde_json::to_value(Settings::default()).unwrap_or(Value::Null);
    let mut dropped = Vec::new();
    if let (Value::Object(fields), true) = (v, good.is_object()) {
        for (k, val) in fields {
            let mut trial = good.clone();
            trial[k.as_str()] = val.clone();
            if reads(&trial) {
                good = trial;
                continue;
            }
            match (val, good[k.as_str()].is_object()) {
                (Value::Object(inner), true) => {
                    for (ik, ival) in inner {
                        let mut trial = good.clone();
                        trial[k.as_str()][ik.as_str()] = ival;
                        if reads(&trial) {
                            good = trial;
                        } else {
                            dropped.push(format!("{k}.{ik}"));
                        }
                    }
                }
                _ => dropped.push(k),
            }
        }
    }
    // The Ask key only with its service: if that fell back to the default, the key would be sent to another company.
    if dropped.iter().any(|d| d == "ai.provider" || d == "ai.base_url") && good["ai"]["api_key"].as_str().is_some_and(|k| !k.is_empty()) {
        good["ai"]["api_key"] = Value::String(String::new());
        dropped.push("ai.api_key".into());
    }
    dropped.sort();
    (serde_json::from_value(good).unwrap_or_default(), dropped)
}

/// From a file that isn't JSON: the API keys (and the EDSM commander name), found by name. Saved keys are base64
/// behind `dpapi:`/`b64:`, so a broken folder path elsewhere in the file leaves them intact. Uploads stay off (they're
/// opt-in); the keys are there when the commander switches them back on.
fn recover_keys(text: &str) -> (Settings, Vec<&'static str>) {
    let mut s = Settings::default();
    let mut found = Vec::new();
    let sealed = |name: &str| string_value(text, name).filter(|v| v.starts_with("dpapi:") || v.starts_with("b64:"));
    if let Some(k) = sealed("edsm_api_key") {
        s.accounts.edsm_api_key = k;
        found.push("EDSM");
    }
    if let Some(k) = sealed("inara_api_key") {
        s.accounts.inara_api_key = k;
        found.push("Inara");
    }
    // The Ask key only with its service: paired with the default service instead, it would be sent to another company.
    let provider = string_value(text, "provider").and_then(|p| serde_json::from_value::<nexus_ai::ProviderKind>(serde_json::Value::String(p)).ok());
    let base_url = string_value(text, "base_url").unwrap_or_default();
    if let (Some(api_key), Some(provider)) = (sealed("api_key"), provider)
        && (provider != nexus_ai::ProviderKind::Custom || !base_url.is_empty())
    {
        s.ai = AiSettings { provider, api_key, model: string_value(text, "model").unwrap_or_default(), base_url };
        found.push("Ask");
    }
    if let Some(name) = string_value(text, "edsm_commander") {
        s.accounts.edsm_commander = name;
    }
    (s, found)
}

/// "A", "A and B", "A, B and C".
fn and_list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The value of the first `"name": "…"` in JSON text that holds no escapes.
fn string_value(text: &str, name: &str) -> Option<String> {
    let key = format!("\"{name}\"");
    let mut rest = text;
    while let Some(i) = rest.find(&key) {
        rest = &rest[i + key.len()..];
        let Some(value) = rest.trim_start().strip_prefix(':').map(str::trim_start).and_then(|r| r.strip_prefix('"')) else { continue };
        let v = &value[..value.find('"')?];
        if !v.is_empty() && !v.contains('\\') {
            return Some(v.to_string());
        }
    }
    None
}

/// Where Elite Nexus keeps its data.
///
/// Portable by default: a `nexus-data` folder next to the executable (keeps everything on the
/// drive the app lives on). Falls back to `%LOCALAPPDATA%\EliteNexus` if that isn't writable (on Linux
/// `$XDG_DATA_HOME/EliteNexus`, usually `~/.local/share/EliteNexus`). `ELITE_NEXUS_HOME` overrides both.
///
/// macOS: the app inside `Elite Nexus.app` keeps its data in `~/Library/Application Support/EliteNexus`, never in
/// the bundle (a signed bundle must not change, and an update replaces it whole). A bare `EliteNexus` binary
/// outside a bundle stays portable, as on Linux.
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("ELITE_NEXUS_HOME") {
        let p = PathBuf::from(p);
        let _ = std::fs::create_dir_all(&p);
        return p;
    }
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf))
        && !(cfg!(target_os = "macos") && in_app_bundle(&exe_dir))
    {
        let p = exe_dir.join("nexus-data");
        if std::fs::create_dir_all(&p).is_ok() && is_writable(&p) {
            return p;
        }
    }
    let p = user_data_base().unwrap_or_else(|| PathBuf::from(".")).join("EliteNexus");
    let _ = std::fs::create_dir_all(&p);
    p
}

/// macOS: this program is the one inside an app bundle (`Elite Nexus.app`), not a bare binary.
pub fn runs_from_app_bundle() -> bool {
    std::env::current_exe().ok().and_then(|e| e.parent().map(in_app_bundle)).unwrap_or(false)
}

/// True for the folder of an executable inside a macOS app bundle: `<Name>.app/Contents/MacOS`.
fn in_app_bundle(exe_dir: &Path) -> bool {
    let named = |p: Option<&Path>, name: &str| p.and_then(Path::file_name).is_some_and(|n| n == name);
    let contents = exe_dir.parent();
    let bundle = contents.and_then(Path::parent);
    named(Some(exe_dir), "MacOS") && named(contents, "Contents") && bundle.and_then(Path::extension).is_some_and(|e| e.eq_ignore_ascii_case("app"))
}

#[cfg(windows)]
pub(crate) fn user_data_base() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// `~/Library/Application Support`.
#[cfg(target_os = "macos")]
pub(crate) fn user_data_base() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Application Support"))
}

/// `$XDG_DATA_HOME`, else `~/.local/share`.
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn user_data_base() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute());
    xdg.or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
}

/// The program runs from inside a zip file: Explorer runs it from a temporary `Temp1_EliteNexus-….zip` folder.
pub fn inside_zip(exe: &Path) -> bool {
    exe.parent().is_some_and(|d| {
        d.components().any(|c| {
            let name = c.as_os_str().to_string_lossy().to_ascii_lowercase();
            name.starts_with("temp") && name.ends_with(".zip")
        })
    })
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".write-test");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder in the temp directory, deleted on drop.
    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            let d = std::env::temp_dir().join(format!("nexus-settings-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).expect("temp dir");
            Dir(d)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The Frontier Forums case: a network folder typed in by hand with single backslashes, which JSON can't read.
    const HAND_EDITED: &str = r#"{
  "journal_dir": "\\192.168.1.50\Elite Dangerous",
  "tts_rate": 1.2,
  "accounts": {
    "edsm_enabled": true,
    "edsm_commander": "Chequers",
    "edsm_api_key": "dpapi:AQAAANCMnd8BFdERjHoAwE/Cl+s=",
    "inara_api_key": "dpapi:AAAA+/09"
  },
  "ai": { "provider": "Claude", "api_key": "b64:QUJD" }
}"#;

    fn kept_copies(d: &Dir) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(&d.0).expect("dir").filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.to_string_lossy().contains("settings-unreadable-")).collect();
        v.sort();
        v
    }

    #[test]
    fn unreadable_file_keeps_the_keys() {
        let d = Dir::new("keys");
        std::fs::write(d.0.join("settings.json"), HAND_EDITED).expect("write");
        let (s, problem) = Settings::load(&d.0);
        assert_eq!(s.accounts.edsm_api_key, "dpapi:AQAAANCMnd8BFdERjHoAwE/Cl+s=");
        assert_eq!(s.accounts.inara_api_key, "dpapi:AAAA+/09");
        assert_eq!(s.ai.api_key, "b64:QUJD");
        assert_eq!(s.ai.provider, nexus_ai::ProviderKind::Claude, "the key comes back with its service");
        assert_eq!(s.accounts.edsm_commander, "Chequers");
        // Uploads are opt-in: the keys come back, the switches don't.
        assert!(!s.accounts.edsm_enabled);
        assert_eq!(s.journal_dir, None);
        assert!(s.welcomed, "not a first start");
        let p = problem.expect("a problem to show");
        assert!(p.what.contains("Your EDSM, Inara and Ask keys were recovered"), "{}", p.what);
        assert!(p.what.contains("line 2"), "says where: {}", p.what);
        assert!(p.can_save);
        // The file as it was is kept, the message names the copy, and a later save doesn't touch it.
        let kept = kept_copies(&d);
        assert_eq!(kept.len(), 1);
        // It holds the keys: only this user account may read it, like settings.json.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&kept[0]).expect("copy").permissions().mode() & 0o777, 0o600);
        }
        assert!(p.what.contains(&*kept[0].file_name().expect("name").to_string_lossy()), "{}", p.what);
        s.save(&d.0);
        assert_eq!(std::fs::read_to_string(&kept[0]).expect("copy"), HAND_EDITED);
        assert!(Settings::load(&d.0).1.is_none(), "the saved file reads");
    }

    #[test]
    fn the_ask_key_only_comes_back_with_its_service() {
        let d = Dir::new("ask");
        // Not JSON, and the service can't be read: the key isn't paired with the default service.
        std::fs::write(d.0.join("settings.json"), r#"{ "journal_dir": "\\PC\Elite", "ai": { "provider": 3, "api_key": "b64:QUJD" } }"#).expect("write");
        let (s, problem) = Settings::load(&d.0);
        assert!(s.ai.api_key.is_empty());
        assert!(!problem.expect("a problem").what.contains("Ask"));
        // Valid JSON with a service that doesn't read: same.
        std::fs::write(d.0.join("settings.json"), r#"{ "ai": { "provider": "claude-3", "api_key": "b64:QUJD" }, "tts_rate": 1.5 }"#).expect("write");
        let (s, problem) = Settings::load(&d.0);
        assert!(s.ai.api_key.is_empty());
        assert_eq!(s.tts_rate, 1.5);
        assert!(problem.expect("a problem").what.contains("ai.api_key and ai.provider are back at the default"));
    }

    #[test]
    fn a_bad_setting_falls_back_alone() {
        let d = Dir::new("field");
        let text = r#"{ "journal_dir": "D:\\Journals", "tts_rate": "fast", "welcomed": true,
            "accounts": { "edsm_enabled": "yes", "edsm_api_key": "dpapi:QQ==", "eddn_listen": false } }"#;
        std::fs::write(d.0.join("settings.json"), text).expect("write");
        let (s, problem) = Settings::load(&d.0);
        assert_eq!(s.journal_dir, Some(PathBuf::from(r"D:\Journals")));
        assert!(s.welcomed);
        assert_eq!(s.tts_rate, Settings::default().tts_rate);
        assert_eq!(s.accounts.edsm_api_key, "dpapi:QQ==");
        assert!(!s.accounts.edsm_enabled);
        assert!(!s.accounts.eddn_listen);
        let p = problem.expect("a problem to show");
        assert!(p.what.contains("accounts.edsm_enabled and tts_rate are back at the default"), "{}", p.what);
        assert!(p.can_save);
        assert_eq!(kept_copies(&d).len(), 1);
    }

    #[test]
    fn not_utf8_is_kept_and_said() {
        let d = Dir::new("ansi");
        std::fs::write(d.0.join("settings.json"), b"{ \"tts_rate\": 1.5, \"edsm_commander\": \"\xdcmit\" }").expect("write");
        let (s, problem) = Settings::load(&d.0);
        assert_eq!(s.tts_rate, 1.5);
        assert!(problem.expect("a problem to show").what.contains("UTF-8"));
        assert_eq!(kept_copies(&d).len(), 1);
        // Not UTF-8 and a bad setting: both are said.
        std::fs::write(d.0.join("settings.json"), b"{ \"tts_rate\": \"fast\", \"edsm_commander\": \"\xdcmit\" }").expect("write");
        let what = Settings::load(&d.0).1.expect("a problem to show").what;
        assert!(what.contains("tts_rate is back at the default") && what.contains("UTF-8"), "{what}");
    }

    #[test]
    fn missing_file_is_a_first_start() {
        let d = Dir::new("missing");
        let (s, problem) = Settings::load(&d.0);
        assert!(problem.is_none());
        assert!(!s.welcomed);
    }

    #[test]
    fn a_file_that_cant_be_read_isnt_saved_over() {
        let d = Dir::new("locked");
        // A folder in its place can't be read on any system.
        std::fs::create_dir(d.0.join("settings.json")).expect("dir");
        let (_, problem) = Settings::load(&d.0);
        assert!(!problem.expect("a problem to show").can_save);
        assert!(kept_copies(&d).is_empty());
    }

    #[test]
    fn running_from_a_zip() {
        assert!(inside_zip(Path::new("/tmp/Temp1_EliteNexus-v0.6.0-windows-x64.zip/EliteNexus-v0.6.0-windows-x64/EliteNexus.exe")));
        assert!(!inside_zip(Path::new("/games/EliteNexus/EliteNexus.exe")));
        assert!(!inside_zip(Path::new("/games/zip/EliteNexus.zip.exe")), "only folders count");
        // Unzipped into a folder named after the zip: fine.
        assert!(!inside_zip(Path::new("/games/EliteNexus-v0.6.1-windows-x64.zip/EliteNexus.exe")));
    }

    #[test]
    fn round_trip() {
        let d = Dir::new("round");
        let mut s = Settings { journal_dir: Some(PathBuf::from(r"\\192.168.1.50\Elite Dangerous")), tts_rate: 1.25, ..Default::default() };
        s.accounts.inara_api_key = "dpapi:AAAA".into();
        s.save(&d.0);
        let (back, problem) = Settings::load(&d.0);
        assert!(problem.is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(d.0.join("settings.json")).expect("saved").permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(back.journal_dir, s.journal_dir);
        assert_eq!(back.tts_rate, 1.25);
        assert_eq!(back.accounts.inara_api_key, "dpapi:AAAA");
        assert!(kept_copies(&d).is_empty());
    }

    /// The BGS tick hour survives a restart (it used to go back to 13:00 UTC); older files without it get the default.
    #[test]
    fn bgs_tick_hour_is_kept() {
        let d = Dir::new("bgs");
        let s = Settings { bgs: crate::pages::bgs::BgsPrefs { period: crate::pages::bgs::Period::Week, tick_hour_utc: 16 }, ..Default::default() };
        s.save(&d.0);
        let (back, problem) = Settings::load(&d.0);
        assert!(problem.is_none());
        assert_eq!(back.bgs, s.bgs);
        std::fs::write(d.0.join("settings.json"), r#"{ "tts_rate": 1.2 }"#).expect("write");
        assert_eq!(Settings::load(&d.0).0.bgs, crate::pages::bgs::BgsPrefs::default());
    }

    #[test]
    fn knows_a_macos_app_bundle() {
        assert!(in_app_bundle(Path::new("/Applications/Elite Nexus.app/Contents/MacOS")));
        assert!(in_app_bundle(Path::new("/Users/cmdr/Applications/EliteNexus.APP/Contents/MacOS")));
        assert!(!in_app_bundle(Path::new("/Users/cmdr/Desktop/EliteNexus-v0.5.0-macos-arm64")));
        assert!(!in_app_bundle(Path::new("/Users/cmdr/MacOS")));
        assert!(!in_app_bundle(Path::new("/Users/cmdr/Contents/MacOS")));
    }
}
