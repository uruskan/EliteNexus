//! EDDN live subscriber: ZeroMQ SUB on the public relay -> galaxy database.
//!
//! * Subscribes to `""` on `tcp://eddn.edcd.io:9500` (the relay is an XPUB that filters on the
//!   *compressed* bytes, so any other topic matches nothing).
//! * The socket task only receives frames and pushes them onto a bounded channel; a decoder thread
//!   inflates (zlib), parses, drops `/test` schemas, de-duplicates (10 min window), updates
//!   [`EddnStats`] and hands [`nexus_galaxy::Ingest`] items to the database writer (never blocking on I/O).
//! * Watchdog: no message for 2 minutes (normal traffic is several per second) -> drop the socket and
//!   reconnect with exponential backoff (5 s -> 60 s).
//!
//! ```no_run
//! let rt = tokio::runtime::Runtime::new().unwrap();
//! let db = nexus_galaxy::GalaxyDb::open("galaxy.db").unwrap();
//! let eddn = nexus_eddn::EddnSubscriber::spawn(rt.handle(), db);
//! println!("{} msgs/min", eddn.stats().messages_per_minute());
//! // dropping `eddn` stops the subscriber
//! ```

mod decode;
mod dedupe;
mod stats;

pub use stats::{EddnStats, EddnStatsSnapshot, Headline};

use crossbeam_channel::TrySendError;
use nexus_galaxy::GalaxyDb;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use zeromq::{Socket, SocketOptions, SocketRecv, SubSocket};

/// The public live relay.
pub const EDDN_RELAY: &str = "tcp://eddn.edcd.io:9500";

#[derive(Debug, Clone)]
pub struct EddnConfig {
    pub endpoint: String,
    /// Reconnect when no message arrived for this long.
    pub watchdog: Duration,
    pub connect_timeout: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    /// Identical messages within this window are dropped.
    pub dedupe_window: Duration,
    /// Raw frames buffered between the socket and the decoder (dropped beyond that).
    pub queue_capacity: usize,
    /// Size of the headline ring buffer.
    pub headlines: usize,
}

impl Default for EddnConfig {
    fn default() -> Self {
        EddnConfig {
            endpoint: EDDN_RELAY.to_string(),
            watchdog: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(20),
            backoff_min: Duration::from_secs(5),
            backoff_max: Duration::from_secs(60),
            dedupe_window: Duration::from_secs(600),
            queue_capacity: 20_000,
            headlines: 100,
        }
    }
}

/// Entry point; see [`EddnSubscriber::spawn`].
pub struct EddnSubscriber;

impl EddnSubscriber {
    /// Starts the subscriber on `rt` with the default configuration.
    pub fn spawn(rt: &tokio::runtime::Handle, db: GalaxyDb) -> EddnHandle {
        EddnSubscriber::spawn_with(rt, db, EddnConfig::default())
    }

    pub fn spawn_with(rt: &tokio::runtime::Handle, db: GalaxyDb, cfg: EddnConfig) -> EddnHandle {
        let stats = Arc::new(EddnStats::new(cfg.headlines));
        let (frame_tx, frame_rx) =
            crossbeam_channel::bounded::<Vec<u8>>(cfg.queue_capacity.max(16));
        let (stop_tx, stop_rx) = watch::channel(false);

        let decoder = {
            let stats = stats.clone();
            let window = cfg.dedupe_window;
            std::thread::Builder::new()
                .name("eddn-decoder".into())
                .spawn(move || {
                    // The writer counts what it kept: queued isn't stored (most scanned bodies aren't kept).
                    let stored = stats.stored_counter();
                    let mut dec = decode::Decoder::new(stats, window);
                    // Ends when the socket task drops its sender.
                    while let Ok(frame) = frame_rx.recv() {
                        let items = dec.handle_frame(&frame);
                        if !items.is_empty() {
                            db.ingest_counted(items, stored.clone());
                        }
                    }
                    tracing::debug!("EDDN decoder stopped");
                })
                .map_err(|e| tracing::error!(error = %e, "could not start the EDDN decoder thread"))
                .ok()
        };

        let task = rt.spawn(socket_loop(cfg, frame_tx, stats.clone(), stop_rx));
        EddnHandle {
            stats,
            stop: Some(stop_tx),
            task: Some(task),
            _decoder: decoder,
        }
    }
}

/// Running subscriber. Stops when dropped (or with [`EddnHandle::stop`]).
pub struct EddnHandle {
    stats: Arc<EddnStats>,
    stop: Option<watch::Sender<bool>>,
    task: Option<tokio::task::JoinHandle<()>>,
    _decoder: Option<std::thread::JoinHandle<()>>,
}

impl EddnHandle {
    pub fn stats(&self) -> &Arc<EddnStats> {
        &self.stats
    }

    /// Stops the subscriber; already received messages are still decoded and ingested.
    pub fn stop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(true);
        }
        if let Some(t) = self.task.take() {
            t.abort();
        }
        // The aborted task may not get to clear it itself.
        self.stats.set_connected(false);
    }

    pub fn is_running(&self) -> bool {
        self.task.as_ref().is_some_and(|t| !t.is_finished())
    }
}

impl Drop for EddnHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn sleep_or_stop(d: Duration, stop: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => false,
        _ = stop.changed() => true,
    }
}

async fn connect(cfg: &EddnConfig) -> zeromq::ZmqResult<SubSocket> {
    let mut opts = SocketOptions::default();
    opts.connect_timeout(cfg.connect_timeout);
    let mut socket = SubSocket::with_options(opts);
    socket.connect(&cfg.endpoint).await?;
    // Must be empty: the relay filters on the zlib-compressed bytes.
    socket.subscribe("").await?;
    Ok(socket)
}

async fn socket_loop(
    cfg: EddnConfig,
    frames: crossbeam_channel::Sender<Vec<u8>>,
    stats: Arc<EddnStats>,
    mut stop: watch::Receiver<bool>,
) {
    let mut backoff = cfg.backoff_min;
    loop {
        if *stop.borrow() {
            break;
        }
        tracing::info!(endpoint = cfg.endpoint, "connecting to EDDN");
        let attempt = tokio::select! {
            r = tokio::time::timeout(cfg.connect_timeout + Duration::from_secs(5), connect(&cfg)) => r,
            _ = stop.changed() => break,
        };
        let mut socket = match attempt {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, retry_in = ?backoff, "EDDN connect failed");
                stats.set_error(format!("connect: {e}"));
                if sleep_or_stop(backoff, &mut stop).await {
                    break;
                }
                backoff = (backoff * 2).min(cfg.backoff_max);
                continue;
            }
            Err(_) => {
                tracing::warn!(retry_in = ?backoff, "EDDN connect timed out");
                stats.set_error("connect: timed out");
                if sleep_or_stop(backoff, &mut stop).await {
                    break;
                }
                backoff = (backoff * 2).min(cfg.backoff_max);
                continue;
            }
        };
        stats.set_connected(true);
        tracing::info!("EDDN connected");
        let mut received = false;
        loop {
            let next = tokio::select! {
                r = tokio::time::timeout(cfg.watchdog, socket.recv()) => r,
                _ = stop.changed() => {
                    stats.set_connected(false);
                    return;
                }
            };
            match next {
                Ok(Ok(msg)) => {
                    received = true;
                    for frame in msg.iter() {
                        match frames.try_send(frame.to_vec()) {
                            Ok(()) => {}
                            Err(TrySendError::Full(_)) => stats.backlog_drop(),
                            Err(TrySendError::Disconnected(_)) => {
                                stats.set_connected(false);
                                return;
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "EDDN receive failed; reconnecting");
                    stats.set_error(format!("receive: {e}"));
                    break;
                }
                Err(_) => {
                    tracing::warn!(after = ?cfg.watchdog, "no EDDN traffic; reconnecting");
                    stats.set_error("watchdog: no traffic");
                    break;
                }
            }
        }
        stats.set_connected(false);
        drop(socket);
        if received {
            backoff = cfg.backoff_min;
        }
        if sleep_or_stop(backoff, &mut stop).await {
            break;
        }
        if !received {
            backoff = (backoff * 2).min(cfg.backoff_max);
        }
    }
    stats.set_connected(false);
    tracing::info!("EDDN subscriber stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_cleanly_without_network() {
        // An unroutable endpoint: the subscriber keeps retrying until dropped.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("rt");
        let dir = std::env::temp_dir().join(format!("nexus-eddn-test-{}", std::process::id()));
        let db = GalaxyDb::open(dir.join("g.db")).expect("db");
        let cfg = EddnConfig {
            endpoint: "tcp://127.0.0.1:1".into(),
            connect_timeout: Duration::from_millis(200),
            backoff_min: Duration::from_millis(50),
            ..EddnConfig::default()
        };
        let mut h = EddnSubscriber::spawn_with(rt.handle(), db.clone(), cfg);
        std::thread::sleep(Duration::from_millis(400));
        assert!(!h.stats().is_connected());
        assert!(h.stats().snapshot().last_error.is_some());
        h.stop();
        assert!(!h.is_running());
        drop(h);
        drop(db);
        drop(rt);
        let _ = std::fs::remove_dir_all(dir);
    }
}
