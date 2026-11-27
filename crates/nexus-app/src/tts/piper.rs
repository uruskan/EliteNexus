use super::audio_cache::{Cache, MAX_WAV_BYTES, valid_wav};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

#[cfg(windows)]
#[link(name = "winmm")]
unsafe extern "system" {
    fn PlaySoundW(sound: *const u16, module: *mut core::ffi::c_void, flags: u32) -> i32;
}

pub(super) struct Piper {
    folder: PathBuf,
    cache: Cache,
    worker: Option<Worker>,
    rate: f32,
    volume: f32,
    _install_lock: std::fs::File,
}

impl Piper {
    pub(super) fn open() -> Result<Self, String> {
        let root = crate::settings::data_dir();
        let lock = super::piper_install::lock_file(&root).map_err(|e| format!("{e:#}"))?;
        lock.lock_shared().map_err(|e| e.to_string())?;
        let folder = super::piper_install::installed().ok_or("Piper is not installed")?;
        let folder = std::path::absolute(folder).map_err(|e| e.to_string())?;
        let cache = Cache::new(root.join("piper-audio"), folder.join("alerts"));
        // Begin loading at selection, while cached alerts remain ready to play.
        let worker = Worker::start(&folder).ok();
        Ok(Self { folder, cache, worker, rate: 1.0, volume: 1.0, _install_lock: lock })
    }

    pub(super) fn set_rate(&mut self, rate: f32) {
        self.rate = if rate.is_finite() { rate.clamp(0.5, 2.0) } else { 1.0 };
    }

    pub(super) fn set_volume(&mut self, volume: f32) {
        self.volume = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 };
    }

    pub(super) fn speak(&mut self, text: &str) -> Result<(), String> {
        let text: String = text.chars().take(400).map(|c| if c.is_control() { ' ' } else { c }).collect();
        let text = text.trim();
        if text.is_empty() || self.volume == 0.0 {
            return Ok(());
        }
        let key = Cache::key(text, self.rate);
        if let Some(wav) = self.cache.read(&key) {
            return play(&wav, self.volume);
        }
        if self.worker.is_none() {
            self.worker = Some(Worker::start(&self.folder)?);
        }
        let outcome = self.worker.as_mut().ok_or("Piper did not start")?.generate(text, self.rate);
        let wav = match outcome {
            Ok(wav) => wav,
            Err(err) => {
                self.worker = None;
                return Err(err);
            }
        };
        // Start the current alert before spending time compressing it for future sessions.
        let result = play(&wav, self.volume);
        if let Err(err) = self.cache.store(&key, &wav) {
            tracing::debug!("Piper audio wasn't cached: {err}");
        }
        result
    }
}

struct Worker {
    process: Child,
    input: ChildStdin,
    output: ChildStdout,
}

impl Worker {
    fn start(folder: &Path) -> Result<Self, String> {
        let helper = folder.join(super::piper_install::HELPER);
        if !helper.is_file() || !folder.join("model/en_US-kristin-medium.onnx").is_file() {
            return Err("the Piper installation is incomplete".into());
        }
        let mut command = Command::new(helper);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_4000); // CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS
        }
        let mut process = command
            .current_dir(folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn().map_err(|e| e.to_string())?;
        let (Some(input), Some(output)) = (process.stdin.take(), process.stdout.take()) else {
            let _ = process.kill();
            let _ = process.wait();
            return Err("couldn't open the Piper audio pipe".into());
        };
        Ok(Self { process, input, output })
    }

    fn generate(&mut self, text: &str, rate: f32) -> Result<Vec<u8>, String> {
        writeln!(self.input, "{rate} 1 {text}").map_err(|e| e.to_string())?;
        let mut len = [0; 4];
        self.output.read_exact(&mut len).map_err(|e| format!("Piper stopped before returning audio ({e})"))?;
        let len = u32::from_le_bytes(len) as usize;
        if !(46..=MAX_WAV_BYTES).contains(&len) {
            return Err("Piper returned no valid audio".into());
        }
        let mut wav = vec![0; len];
        self.output.read_exact(&mut wav).map_err(|e| e.to_string())?;
        if !valid_wav(&wav) {
            return Err("Piper returned an invalid WAV header".into());
        }
        Ok(wav)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

#[cfg(windows)]
fn play(wav: &[u8], volume: f32) -> Result<(), String> {
    // Copy the header as is and scale PCM samples, leaving the cache independent of volume.
    let mut audio: Vec<u16> = wav.as_chunks::<2>().0.iter().copied().map(u16::from_le_bytes).collect();
    if volume < 1.0 {
        for sample in &mut audio[22..] {
            *sample = (f32::from(*sample as i16) * volume) as i16 as u16;
        }
    }
    // SAFETY: SND_MEMORY | SND_NODEFAULT, synchronous over the aligned WAV buffer above.
    if unsafe { PlaySoundW(audio.as_ptr(), std::ptr::null_mut(), 0x0006) } == 0 {
        return Err("Windows couldn't play the Piper audio".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
use super::piper_linux::play;
