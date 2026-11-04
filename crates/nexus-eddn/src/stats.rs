//! Live subscriber statistics, shared with the UI through an `Arc`.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

/// One line for a UI ticker. Never contains uploader names or IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Headline {
    pub unix_ms: i64,
    /// Short schema name, e.g. `commodity/3`, `journal/1`.
    pub schema: String,
    pub text: String,
}

/// Counters of the EDDN subscriber. All methods are cheap and lock-free except the ones returning
/// collections.
#[derive(Debug)]
pub struct EddnStats {
    connected: AtomicBool,
    started_unix_ms: i64,
    total: AtomicU64,
    duplicates: AtomicU64,
    test_dropped: AtomicU64,
    decode_errors: AtomicU64,
    dropped_backlog: AtomicU64,
    ingested: AtomicU64,
    /// Filled by the database writer as it commits what the feed sent (see [`EddnStatsSnapshot::stored`]).
    stored: Arc<AtomicU64>,
    connects: AtomicU64,
    last_message_unix_ms: AtomicI64,
    headline_capacity: usize,
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    per_schema: BTreeMap<String, u64>,
    /// (unix second, messages in that second), newest at the back; covers the last minute.
    per_second: VecDeque<(i64, u32)>,
    headlines: VecDeque<Headline>,
    last_error: Option<String>,
}

/// Plain copy of [`EddnStats`] for display.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EddnStatsSnapshot {
    pub connected: bool,
    pub started_unix_ms: i64,
    /// Decoded messages (excluding `/test` schemas and duplicates).
    pub total_messages: u64,
    /// Messages in the last 60 seconds.
    pub messages_per_minute: u64,
    pub duplicates: u64,
    pub test_dropped: u64,
    pub decode_errors: u64,
    /// Frames dropped because the decoder fell behind.
    pub dropped_backlog: u64,
    /// Database items queued from EDDN.
    pub ingested: u64,
    /// Of those, the ones that changed the database, once committed: a scan of a body it doesn't keep (most are
    /// bare rock), a market report older than the one it has, or a report that changes nothing isn't counted.
    pub stored: u64,
    /// Successful connections (1 = connected once, never reconnected).
    pub connects: u64,
    pub last_message_unix_ms: Option<i64>,
    /// Per schema (`commodity/3`, `journal/1`, ...), most frequent first.
    pub per_schema: Vec<(String, u64)>,
    /// Newest first.
    pub recent_headlines: Vec<Headline>,
    pub last_error: Option<String>,
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl EddnStats {
    pub(crate) fn new(headline_capacity: usize) -> EddnStats {
        EddnStats {
            connected: AtomicBool::new(false),
            started_unix_ms: now_ms(),
            total: AtomicU64::new(0),
            duplicates: AtomicU64::new(0),
            test_dropped: AtomicU64::new(0),
            decode_errors: AtomicU64::new(0),
            dropped_backlog: AtomicU64::new(0),
            ingested: AtomicU64::new(0),
            stored: Arc::new(AtomicU64::new(0)),
            connects: AtomicU64::new(0),
            last_message_unix_ms: AtomicI64::new(0),
            headline_capacity: headline_capacity.max(1),
            inner: Mutex::new(Inner::default()),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn total_messages(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// Messages received in the last 60 seconds.
    pub fn messages_per_minute(&self) -> u64 {
        let now = now_ms() / 1000;
        let inner = self.inner.lock();
        inner
            .per_second
            .iter()
            .filter(|(s, _)| *s > now - 60)
            .map(|(_, n)| u64::from(*n))
            .sum()
    }

    /// Newest first.
    pub fn headlines(&self, max: usize) -> Vec<Headline> {
        self.inner
            .lock()
            .headlines
            .iter()
            .rev()
            .take(max)
            .cloned()
            .collect()
    }

    pub fn snapshot(&self) -> EddnStatsSnapshot {
        let mpm = self.messages_per_minute();
        let inner = self.inner.lock();
        let mut per_schema: Vec<(String, u64)> = inner
            .per_schema
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        per_schema.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let last = self.last_message_unix_ms.load(Ordering::Relaxed);
        EddnStatsSnapshot {
            connected: self.is_connected(),
            started_unix_ms: self.started_unix_ms,
            total_messages: self.total.load(Ordering::Relaxed),
            messages_per_minute: mpm,
            duplicates: self.duplicates.load(Ordering::Relaxed),
            test_dropped: self.test_dropped.load(Ordering::Relaxed),
            decode_errors: self.decode_errors.load(Ordering::Relaxed),
            dropped_backlog: self.dropped_backlog.load(Ordering::Relaxed),
            ingested: self.ingested.load(Ordering::Relaxed),
            stored: self.stored.load(Ordering::Relaxed),
            connects: self.connects.load(Ordering::Relaxed),
            last_message_unix_ms: (last > 0).then_some(last),
            per_schema,
            recent_headlines: inner.headlines.iter().rev().cloned().collect(),
            last_error: inner.last_error.clone(),
        }
    }

    // --- updates (crate-internal) ---

    pub(crate) fn set_connected(&self, connected: bool) {
        if connected && !self.connected.swap(true, Ordering::Relaxed) {
            self.connects.fetch_add(1, Ordering::Relaxed);
        } else if !connected {
            self.connected.store(false, Ordering::Relaxed);
        }
    }

    pub(crate) fn set_error(&self, e: impl std::fmt::Display) {
        self.inner.lock().last_error = Some(e.to_string());
    }

    pub(crate) fn duplicate(&self) {
        self.duplicates.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn test_dropped(&self) {
        self.test_dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn decode_error(&self) {
        self.decode_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn backlog_drop(&self) {
        self.dropped_backlog.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn ingested(&self, n: usize) {
        self.ingested.fetch_add(n as u64, Ordering::Relaxed);
    }

    /// The counter the database writer adds the stored items to.
    pub(crate) fn stored_counter(&self) -> Arc<AtomicU64> {
        self.stored.clone()
    }

    /// One accepted message of `schema`.
    pub(crate) fn message(&self, schema: &str, at_ms: i64, headline: Option<String>) {
        self.total.fetch_add(1, Ordering::Relaxed);
        self.last_message_unix_ms.store(at_ms, Ordering::Relaxed);
        let sec = at_ms / 1000;
        let mut inner = self.inner.lock();
        *inner.per_schema.entry(schema.to_string()).or_insert(0) += 1;
        match inner.per_second.back_mut() {
            Some((s, n)) if *s == sec => *n += 1,
            _ => inner.per_second.push_back((sec, 1)),
        }
        while inner
            .per_second
            .front()
            .is_some_and(|(s, _)| *s <= sec - 60)
        {
            inner.per_second.pop_front();
        }
        if let Some(text) = headline {
            if inner.headlines.len() >= self.headline_capacity {
                inner.headlines.pop_front();
            }
            inner.headlines.push_back(Headline {
                unix_ms: at_ms,
                schema: schema.to_string(),
                text,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_rate_and_headlines() {
        let s = EddnStats::new(3);
        let now = now_ms();
        for i in 0..5 {
            s.message("journal/1", now - 1000 * i, Some(format!("h{i}")));
        }
        s.message("commodity/3", now - 120_000, None); // too old for the rolling rate
        assert_eq!(s.total_messages(), 6);
        // the old message evicted nothing (it arrived last but is older); count only the last minute
        assert_eq!(s.messages_per_minute(), 5);
        let snap = s.snapshot();
        assert_eq!(snap.per_schema[0], ("journal/1".to_string(), 5));
        assert_eq!(snap.recent_headlines.len(), 3);
        assert_eq!(snap.recent_headlines[0].text, "h4");
        s.set_connected(true);
        s.set_connected(true);
        s.set_connected(false);
        s.set_connected(true);
        assert_eq!(s.snapshot().connects, 2);
    }
}
