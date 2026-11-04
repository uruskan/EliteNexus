//! Screenshots as PNG (Settings › Screenshots; off until the commander switches it on). The game saves every screenshot
//! (F10, and Alt+F10 at high resolution) as an uncompressed BMP in `Pictures\Frontier Developments\Elite Dangerous`
//! and logs a `Screenshot` event with the file, the system and the nearest body. Each one logged while Elite Nexus
//! runs is saved again as a PNG named after where it was taken; its BMP is deleted only when the commander asked.
//!
//! Only live events reach this, so the screenshots already in the folder stay as they are, and test runs
//! (`--journal-dir`, `--screenshot`) never start it: a replayed journal can't touch the Pictures folder.

use crate::core::{Core, CoreStatus, LiveMsg};
use crossbeam_channel::Receiver;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, ImageFormat, RgbImage};
use nexus_journal::Timestamp;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a screenshot may take to appear and be readable: the game writes a high-resolution one for a while.
const WAIT: Duration = Duration::from_secs(30);

/// The commander's choices (Settings › Screenshots).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenshotPrefs {
    /// Save each new screenshot again as a PNG.
    pub convert: bool,
    /// Where the PNGs go; `None`: next to the game's BMPs.
    pub folder: Option<PathBuf>,
    /// Delete the BMP once its PNG is saved.
    pub delete_bmp: bool,
}

/// What happened to the last screenshot, for Settings.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Being saved as PNG: the BMP's name.
    Working(String),
    /// Saved: the PNG, and what to say about it.
    Saved(PathBuf, String),
    /// Not saved, or saved but its BMP not deleted: what happened, in plain words.
    Failed(String),
}

/// Follows the core's live events and saves each new screenshot as a PNG.
pub struct ScreenshotService {
    /// `None` for test runs: no screenshot is ever touched.
    shared: Option<Arc<Shared>>,
}

struct Shared {
    prefs: RwLock<ScreenshotPrefs>,
    last: Mutex<Option<Outcome>>,
    repaint: egui::Context,
}

impl ScreenshotService {
    pub fn start(core: &Core, prefs: &ScreenshotPrefs, ctx: &egui::Context) -> ScreenshotService {
        let shared = Arc::new(Shared { prefs: RwLock::new(prefs.clone()), last: Mutex::new(None), repaint: ctx.clone() });
        let service = ScreenshotService { shared: Some(shared) };
        service.attach(core);
        service
    }

    /// Test runs (screenshots of the app, replayed journals).
    pub fn disabled() -> ScreenshotService {
        ScreenshotService { shared: None }
    }

    /// Follows a (re)started core. The previous thread ends when its core stops.
    pub fn attach(&self, core: &Core) {
        if let Some(s) = &self.shared {
            follow(core.subscribe(), core.status.clone(), s.clone());
        }
    }

    /// Call after the settings changed.
    pub fn apply(&self, prefs: &ScreenshotPrefs) {
        if let Some(s) = &self.shared
            && *s.prefs.read() != *prefs
        {
            *s.prefs.write() = prefs.clone();
        }
    }

    /// What happened to the last screenshot this run.
    pub fn last(&self) -> Option<Outcome> {
        self.shared.as_ref().and_then(|s| s.last.lock().clone())
    }
}

fn follow(rx: Receiver<LiveMsg>, status: Arc<RwLock<CoreStatus>>, shared: Arc<Shared>) {
    let spawned = std::thread::Builder::new().name("nexus-screenshots".into()).spawn(move || {
        // Ends when the core stops (its senders drop). One screenshot at a time: a high-resolution one takes a few
        // hundred megabytes of memory while it's converted.
        for msg in rx {
            let LiveMsg::Event(e, _) = msg else { continue };
            if &*e.raw.name != "Screenshot" {
                continue;
            }
            let prefs = shared.prefs.read().clone();
            if !prefs.convert {
                continue;
            }
            let Ok(shot) = serde_json::from_str::<ShotEvent>(&e.raw.line) else { continue };
            // Its own statement: the core's status stays locked only this long, not for the whole conversion.
            let game_dir = nexus_journal::paths::screenshots_dir(status.read().journal_dir.as_deref());
            let outcome = match game_dir {
                Some(dir) => {
                    set(&shared, Outcome::Working(shot.file_name().to_string()));
                    convert(&shot, e.raw.ts, &dir, &prefs, WAIT)
                }
                None => Outcome::Failed("Elite Nexus can't tell where the game saves screenshots: the journal folder in use isn't inside a Windows user folder.".into()),
            };
            match &outcome {
                Outcome::Saved(png, _) => tracing::info!(png = %png.display(), "screenshot saved as PNG"),
                Outcome::Failed(why) => tracing::warn!("screenshot: {why}"),
                Outcome::Working(_) => {}
            }
            set(&shared, outcome);
        }
    });
    if let Err(e) = spawned {
        tracing::error!("screenshots: cannot start the thread: {e}");
    }
}

fn set(shared: &Shared, outcome: Outcome) {
    *shared.last.lock() = Some(outcome);
    shared.repaint.request_repaint();
}

/// The parts of a `Screenshot` event this needs.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ShotEvent {
    /// `\ED_Pictures\Screenshot_0003.bmp`: the game's screenshot folder, then the file.
    #[serde(rename = "Filename")]
    filename: String,
    #[serde(rename = "System")]
    system: String,
    /// The body nearest the commander.
    #[serde(rename = "Body")]
    body: String,
}

impl ShotEvent {
    /// The file's name, without the game's folder.
    fn file_name(&self) -> &str {
        self.filename.rsplit(['\\', '/']).next().unwrap_or_default()
    }
}

/// Saves one screenshot from `game_dir` as a PNG, then deletes its BMP when asked to.
fn convert(shot: &ShotEvent, ts: Timestamp, game_dir: &Path, prefs: &ScreenshotPrefs, wait: Duration) -> Outcome {
    let name = shot.file_name();
    if !name.to_ascii_lowercase().ends_with(".bmp") {
        return Outcome::Failed(format!("{name} isn't a BMP, so it's left as it is."));
    }
    let bmp = game_dir.join(name);
    let (image, bmp_bytes) = match read_bmp(&bmp, wait) {
        Ok(read) => read,
        Err(why) => return Outcome::Failed(why),
    };
    let dir = prefs.folder.clone().unwrap_or_else(|| game_dir.to_path_buf());
    let stem = file_stem(&shot.system, &shot.body, &ts.to_local_string("%Y-%m-%d %H-%M-%S"));
    let png = match write_png(&image, &dir, &stem) {
        Ok(png) => png,
        Err(e) => return Outcome::Failed(format!("{name} couldn't be saved as PNG in {}: {e}.", dir.display())),
    };
    let png_bytes = std::fs::metadata(&png).map(|m| m.len()).unwrap_or(0);
    let file = png.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut text = format!("Saved {file}: {} instead of the BMP's {}.", mb(png_bytes), mb(bmp_bytes));
    if prefs.delete_bmp {
        match std::fs::remove_file(&bmp) {
            Ok(()) => text.push_str(" The BMP is deleted."),
            Err(e) => return Outcome::Failed(format!("{text} The BMP couldn't be deleted ({e}), so it's still there.")),
        }
    }
    Outcome::Saved(png, text)
}

/// Reads a screenshot the game may still be writing: tries until it decodes, for up to `wait`. The alpha channel
/// goes: a 32-bit BMP's can be empty, which would make the PNG see-through.
fn read_bmp(path: &Path, wait: Duration) -> Result<(RgbImage, u64), String> {
    let started = Instant::now();
    loop {
        let why = match std::fs::read(path) {
            Ok(bytes) => match image::load_from_memory_with_format(&bytes, ImageFormat::Bmp) {
                Ok(image) => return Ok((image.into_rgb8(), bytes.len() as u64)),
                Err(e) => e.to_string(),
            },
            Err(e) => e.to_string(),
        };
        if started.elapsed() >= wait {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let dir = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
            return Err(if path.exists() { format!("{name} couldn't be read: {why}.") } else { format!("{name} isn't in {dir}.") });
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Writes the PNG under a name of its own (`stem.png`, or `stem 2.png`… when that's taken; a file is never
/// overwritten) and returns its path. A PNG that couldn't be written whole is removed.
fn write_png(image: &RgbImage, dir: &Path, stem: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let (path, file) = create_new(dir, stem)?;
    match encode(image, file) {
        Ok(()) => Ok(path),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(e)
        }
    }
}

fn encode(image: &RgbImage, file: std::fs::File) -> std::io::Result<()> {
    let mut out = std::io::BufWriter::new(file);
    PngEncoder::new_with_quality(&mut out, CompressionType::Default, FilterType::Adaptive)
        .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgb8)
        .map_err(std::io::Error::other)?;
    // On the disk before the BMP may go.
    out.into_inner().map_err(|e| e.into_error())?.sync_all()
}

/// Creates `stem.png` in `dir`, or `stem 2.png`, `stem 3.png`… when it exists.
fn create_new(dir: &Path, stem: &str) -> std::io::Result<(PathBuf, std::fs::File)> {
    for n in 1..=999 {
        let path = dir.join(if n == 1 { format!("{stem}.png") } else { format!("{stem} {n}.png") });
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("{stem}.png and 998 more like it exist already")))
}

/// The PNG's name without `.png`: the system, the body nearest the commander and the local time, as in
/// `Sol (Earth) 2026-11-04 14-32-05`. A body named after its system keeps only the part after the system's name
/// (`HIP 8396 (A 1)`), and characters Windows doesn't allow in a file name become `-`.
fn file_stem(system: &str, body: &str, when: &str) -> String {
    let body = if body == system { "" } else { body.strip_prefix(system).and_then(|b| b.strip_prefix(' ')).unwrap_or(body) };
    let place = match (system, body) {
        ("", "") => "Screenshot".to_string(),
        (s, "") | ("", s) => s.to_string(),
        (s, b) => format!("{s} ({b})"),
    };
    let place: String = place.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '-' } else { c }).take(120).collect();
    format!("{place} {when}")
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_a_png_after_the_place_and_time() {
        let t = "2026-11-04 14-32-05";
        assert_eq!(file_stem("Sol", "Earth", t), "Sol (Earth) 2026-11-04 14-32-05");
        assert_eq!(file_stem("HIP 8396", "HIP 8396 A 1", t), "HIP 8396 (A 1) 2026-11-04 14-32-05");
        // The main star shares the system's name.
        assert_eq!(file_stem("Sol", "Sol", t), "Sol 2026-11-04 14-32-05");
        // Only a whole name counts as the system's.
        assert_eq!(file_stem("Col 285 Sector AB-C d1", "Col 285 Sector AB-C d1-2 A 1", t), "Col 285 Sector AB-C d1 (Col 285 Sector AB-C d1-2 A 1) 2026-11-04 14-32-05");
        assert_eq!(file_stem("Shinrarta Dezhra", "", t), "Shinrarta Dezhra 2026-11-04 14-32-05");
        assert_eq!(file_stem("", "", t), "Screenshot 2026-11-04 14-32-05");
        assert_eq!(file_stem("Sol", "Earth: \"home\"", t), "Sol (Earth- -home-) 2026-11-04 14-32-05");
    }

    #[test]
    fn reads_the_event() {
        let line = r#"{ "timestamp":"2026-11-04T13:32:05Z", "event":"Screenshot", "Filename":"\\ED_Pictures\\Screenshot_0003.bmp", "Width":1920, "Height":1080, "System":"Sol", "Body":"Earth" }"#;
        let shot: ShotEvent = serde_json::from_str(line).unwrap();
        assert_eq!((shot.file_name(), shot.system.as_str(), shot.body.as_str()), ("Screenshot_0003.bmp", "Sol", "Earth"));
        // System and Body aren't always there.
        let shot: ShotEvent = serde_json::from_str(r#"{ "event":"Screenshot", "Filename":"\\ED_Pictures\\Screenshot_0004.bmp" }"#).unwrap();
        assert_eq!((shot.file_name(), shot.system.as_str()), ("Screenshot_0004.bmp", ""));
    }

    #[test]
    fn saves_every_pixel_and_deletes_the_bmp_only_when_asked() {
        let dir = std::env::temp_dir().join(format!("nexus-screenshots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let picture = RgbImage::from_fn(7, 5, |x, y| image::Rgb([x as u8 * 30, y as u8 * 50, 200]));
        let bmp = |name: &str| {
            picture.save_with_format(dir.join(name), ImageFormat::Bmp).unwrap();
            ShotEvent { filename: format!(r"\ED_Pictures\{name}"), system: "Sol".into(), body: "Earth".into() }
        };
        let ts = Timestamp::parse("2026-11-04T13:32:05Z").unwrap();
        let stem = file_stem("Sol", "Earth", &ts.to_local_string("%Y-%m-%d %H-%M-%S"));
        let keep = ScreenshotPrefs { convert: true, ..Default::default() };

        let Outcome::Saved(png, _) = convert(&bmp("Screenshot_0000.bmp"), ts, &dir, &keep, Duration::ZERO) else { panic!("not saved") };
        assert_eq!(png, dir.join(format!("{stem}.png")));
        assert_eq!(image::open(&png).unwrap().into_rgb8(), picture);
        assert!(dir.join("Screenshot_0000.bmp").exists());

        // Two in the same second, into a folder of the commander's; this time the BMP goes.
        let own = dir.join("PNG");
        let delete = ScreenshotPrefs { convert: true, folder: Some(own.clone()), delete_bmp: true };
        let Outcome::Saved(first, _) = convert(&bmp("Screenshot_0001.bmp"), ts, &dir, &delete, Duration::ZERO) else { panic!("not saved") };
        let Outcome::Saved(second, _) = convert(&bmp("Screenshot_0002.bmp"), ts, &dir, &delete, Duration::ZERO) else { panic!("not saved") };
        assert_eq!((first, second), (own.join(format!("{stem}.png")), own.join(format!("{stem} 2.png"))));
        assert!(!dir.join("Screenshot_0001.bmp").exists() && !dir.join("Screenshot_0002.bmp").exists());

        // A file the game never wrote is reported, not waited for forever.
        let missing = ShotEvent { filename: r"\ED_Pictures\Screenshot_0009.bmp".into(), ..Default::default() };
        assert!(matches!(convert(&missing, ts, &dir, &keep, Duration::ZERO), Outcome::Failed(why) if why.contains("isn't in")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
