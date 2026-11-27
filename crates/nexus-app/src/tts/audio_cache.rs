use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const CACHE_BYTES: u64 = 1_000_000_000;
// Must match the seed generator. Bump on changes to the model, voice or audio format.
const PROFILE: &[u8] = b"elite-nexus-piper-en-us-kristin-medium-v1\0";
pub(super) const MAX_WAV_BYTES: usize = 4 * 1024 * 1024;

pub(super) struct Cache {
    directory: PathBuf,
    bundled: PathBuf,
    limit: u64,
}

impl Cache {
    pub(super) fn new(directory: PathBuf, bundled: PathBuf) -> Self {
        let bundled_bytes = entries(&bundled).unwrap_or_default().iter().map(|e| e.size).sum();
        Self { directory, bundled, limit: CACHE_BYTES.saturating_sub(bundled_bytes) }
    }

    pub(super) fn key(text: &str, rate: f32) -> String {
        let mut key = Sha256::new();
        key.update(PROFILE);
        key.update(rate.to_le_bytes());
        key.update(text.as_bytes());
        format!("{:x}.wav.gz", key.finalize())
    }

    pub(super) fn read(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(wav) = read_wav(&self.bundled.join(key)) {
            return Some(wav);
        }
        let path = self.directory.join(key);
        let wav = read_wav(&path)?;
        if let Ok(file) = File::options().write(true).open(path) {
            let _ = file.set_modified(SystemTime::now());
        }
        Some(wav)
    }

    pub(super) fn store(&self, key: &str, wav: &[u8]) -> io::Result<()> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(wav)?;
        let compressed = encoder.finish()?;
        let size = compressed.len() as u64;
        if size > self.limit {
            return Ok(());
        }
        fs::create_dir_all(&self.directory)?;
        // Serialize eviction and publication across instances sharing this data directory.
        let lock = File::options().create(true).truncate(false).read(true).write(true).open(self.directory.join("cache.lock"))?;
        lock.lock()?;
        let pending = self.directory.join("pending");
        remove_if_present(&pending)?;
        let path = self.directory.join(key);
        if path.exists() {
            if read_wav(&path).is_some() {
                return Ok(());
            }
            remove_if_present(&path)?;
        }
        let mut entries = entries(&self.directory)?;
        let mut total: u64 = entries.iter().map(|e| e.size).sum();
        entries.sort_unstable_by_key(|e| e.modified);
        for entry in entries {
            if total <= self.limit - size {
                break;
            }
            remove_if_present(&entry.path)?;
            total -= entry.size;
        }
        // Reserve space before writing. A failed or interrupted write is never a cache hit.
        let result = fs::write(&pending, compressed).and_then(|()| fs::rename(&pending, path));
        if result.is_err() {
            let _ = remove_if_present(&pending);
        }
        result
    }
}

struct Entry {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

fn entries(directory: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(directory)? {
        let item = item?;
        let name = item.file_name();
        let Some(hash) = name.to_str().and_then(|name| name.strip_suffix(".wav.gz")) else { continue };
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) || !item.file_type()?.is_file() {
            continue;
        }
        let info = item.metadata()?;
        entries.push(Entry {
            path: item.path(),
            size: info.len(),
            modified: info.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        });
    }
    Ok(entries)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn read_wav(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let mut wav = Vec::new();
    GzDecoder::new(file.take((MAX_WAV_BYTES + 65536) as u64))
        .take((MAX_WAV_BYTES + 1) as u64).read_to_end(&mut wav).ok()?;
    valid_wav(&wav).then_some(wav)
}

pub(super) fn valid_wav(wav: &[u8]) -> bool {
    if !(46..=MAX_WAV_BYTES).contains(&wav.len()) || !wav.len().is_multiple_of(2) {
        return false;
    }
    let rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
    &wav[..4] == b"RIFF"
        && wav[4..8] == (wav.len() as u32 - 8).to_le_bytes()
        && &wav[8..24] == b"WAVEfmt \x10\0\0\0\x01\0\x01\0"
        && (8000..=48000).contains(&rate)
        && wav[28..32] == (rate * 2).to_le_bytes()
        && &wav[32..40] == b"\x02\0\x10\0data"
        && wav[40..44] == (wav.len() as u32 - 44).to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn wav(sample: i16) -> Vec<u8> {
        let mut bytes = b"RIFF\x26\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0".to_vec();
        bytes.extend_from_slice(&24000u32.to_le_bytes());
        bytes.extend_from_slice(&48000u32.to_le_bytes());
        bytes.extend_from_slice(b"\x02\0\x10\0data\x02\0\0\0");
        bytes.extend_from_slice(&sample.to_le_bytes());
        bytes
    }

    #[test]
    fn reads_across_instances_and_evicts_old_audio_before_publishing() -> io::Result<()> {
        let dir = std::env::temp_dir().join(format!("nexus-piper-cache-test-{}", std::process::id()));
        fs::create_dir(&dir)?;
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(dir.clone());
        let make_cache = || Cache::new(dir.join("user"), dir.join("bundled"));
        let old = Cache::key("Fuel low.", 1.0);
        let new = Cache::key("Fuel low.", 1.5);
        let audio = wav(512);
        assert_ne!(old, new);
        assert!(valid_wav(&audio));
        make_cache().store(&old, &audio)?;

        let mut cache = make_cache();
        assert_eq!(cache.read(&old).as_deref(), Some(audio.as_slice()));
        let old_path = cache.directory.join(&old);
        let old_file = File::options().write(true).open(&old_path)?;
        old_file.set_modified(SystemTime::now() - Duration::from_secs(60))?;
        drop(old_file);
        cache.limit = fs::metadata(&old_path)?.len();
        cache.store(&new, &audio)?;
        assert!(cache.read(&old).is_none());
        assert_eq!(make_cache().read(&new).as_deref(), Some(audio.as_slice()));
        assert!(entries(&cache.directory)?.iter().map(|entry| entry.size).sum::<u64>() <= cache.limit);
        let new_path = cache.directory.join(&new);
        fs::write(&new_path, b"broken gzip")?;
        assert!(cache.read(&new).is_none());
        cache.store(&new, &audio)?;
        assert_eq!(cache.read(&new).as_deref(), Some(audio.as_slice()));
        Ok(())
    }
}
