//! Bulk (historical) journal loading.
//!
//! Files are read and parsed in parallel with rayon, then handed to the caller strictly in
//! chronological order, a batch at a time, so memory stays bounded no matter how many years of
//! journals a commander has. Each [`FileEvents`] keeps the file text so callers can slice raw
//! lines and event names out of it without extra allocations.

use crate::events::Event;
use crate::paths::JournalFile;
use crate::raw::{parse_event_lenient, scan_line};
use crate::time::Timestamp;
use rayon::prelude::*;
use std::path::Path;

/// One complete journal line from a bulk load.
#[derive(Debug, Clone)]
pub struct LoadedEvent {
    pub ts: Timestamp,
    /// Byte offset of the line within the file.
    pub offset: u32,
    /// Byte length of the line (without the newline).
    pub len: u32,
    name_off: u32,
    name_len: u16,
    pub event: Event,
}

/// The parsed contents of one journal file.
#[derive(Debug)]
pub struct FileEvents {
    /// Index of the file in the list passed to [`load_history`].
    pub file: u32,
    pub text: String,
    pub events: Vec<LoadedEvent>,
    /// Bytes consumed: everything up to and including the last newline. A partially written
    /// final line (the game is still writing it) is left for the live tailer.
    pub consumed: u64,
    pub parse_errors: u32,
}

impl FileEvents {
    #[inline]
    pub fn line(&self, e: &LoadedEvent) -> &str {
        &self.text[e.offset as usize..(e.offset + e.len) as usize]
    }
    #[inline]
    pub fn name(&self, e: &LoadedEvent) -> &str {
        &self.text[e.name_off as usize..e.name_off as usize + e.name_len as usize]
    }
}

/// Parses the complete lines of one journal file.
pub fn read_journal_file(path: &Path, file: u32) -> std::io::Result<FileEvents> {
    let bytes = std::fs::read(path)?;
    let text = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    };
    Ok(parse_journal_text(text, file))
}

/// Parses journal text (exposed for tests and replays).
pub fn parse_journal_text(text: String, file: u32) -> FileEvents {
    let consumed = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let mut events = Vec::with_capacity(consumed / 300 + 1);
    let mut parse_errors = 0u32;
    let mut pos = 0usize;
    let base = text.as_ptr() as usize;
    while pos < consumed {
        let end = text[pos..consumed].find('\n').map(|i| pos + i).unwrap_or(consumed);
        let line = text[pos..end].trim_end_matches('\r');
        if !line.trim().is_empty() {
            if let Some((ts, name)) = scan_line(line) {
                match parse_event_lenient(name, line) {
                    Ok(event) => events.push(LoadedEvent {
                        ts,
                        offset: pos as u32,
                        len: line.len() as u32,
                        name_off: (name.as_ptr() as usize - base) as u32,
                        name_len: name.len().min(u16::MAX as usize) as u16,
                        event,
                    }),
                    Err(err) => {
                        parse_errors += 1;
                        tracing::debug!(%err, event = name, "journal line failed typed parse");
                        events.push(LoadedEvent {
                            ts,
                            offset: pos as u32,
                            len: line.len() as u32,
                            name_off: (name.as_ptr() as usize - base) as u32,
                            name_len: name.len().min(u16::MAX as usize) as u16,
                            event: Event::Other,
                        });
                    }
                }
            } else {
                parse_errors += 1;
            }
        }
        pos = end + 1;
    }
    FileEvents { file, text, events, consumed: consumed as u64, parse_errors }
}

/// Loads every file in `files` (oldest first), parsing `batch` files concurrently and calling
/// `sink` once per file in chronological order.
pub fn load_history(files: &[JournalFile], batch: usize, mut sink: impl FnMut(&JournalFile, FileEvents)) {
    let batch = batch.max(1);
    for (chunk_idx, chunk) in files.chunks(batch).enumerate() {
        let parsed: Vec<std::io::Result<FileEvents>> = chunk
            .par_iter()
            .enumerate()
            .map(|(i, f)| read_journal_file(&f.path, (chunk_idx * batch + i) as u32))
            .collect();
        for (f, res) in chunk.iter().zip(parsed) {
            match res {
                Ok(fe) => sink(f, fe),
                Err(err) => tracing::warn!(file = %f.path.display(), %err, "failed to read journal file"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_partial_last_line() {
        let text = concat!(
            r#"{ "timestamp":"2026-09-05T00:13:29Z", "event":"Commander", "FID":"F1", "Name":"Jameson" }"#,
            "\n",
            r#"{ "timestamp":"2026-09-05T00:13:30Z", "event":"Music", "MusicTrack":"NoTrack" }"#,
            "\r\n",
            r#"{ "timestamp":"2026-09-05T00:13:31Z", "event":"Fileh"#
        )
        .to_string();
        let fe = parse_journal_text(text, 0);
        assert_eq!(fe.events.len(), 2);
        assert_eq!(fe.name(&fe.events[0]), "Commander");
        assert_eq!(fe.name(&fe.events[1]), "Music");
        assert!(fe.line(&fe.events[1]).ends_with('}'));
        assert!(matches!(&fe.events[1].event, Event::Music(m) if m.music_track == "NoTrack"));
        let Event::Commander(c) = &fe.events[0].event else { panic!() };
        assert_eq!(c.name, "Jameson");
        assert!(fe.consumed < fe.text.len() as u64);
    }
}
