// Licence: GPL-3.0-or-later, with additional terms under its section 7: see LICENSE-ADDITIONAL-TERMS.md at the repository root.
//! Elite Dangerous player journal access for Elite Nexus.
//!
//! * [`paths`] — finding the journal and Options folders (on Linux in the game's Wine prefix), ordering journal files.
//! * [`raw`] — allocation-free pre-scan of a line's timestamp and event name.
//! * [`events`] — typed events (`Event::parse` only parses events Elite Nexus models).
//! * [`companion`] — Status.json, Market.json, Cargo.json, NavRoute.json, …
//! * [`reader`] — parallel bulk loading of historical journals.
//! * [`watcher`] — live tailing with ~100 ms latency.

pub mod companion;
pub mod events;
pub mod index;
pub mod paths;
pub mod raw;
pub mod reader;
pub mod time;
pub mod watcher;

pub use companion::{Companion, CompanionKind, Status};
pub use events::Event;
pub use index::{IndexEntry, JournalIndex};
pub use paths::{default_bindings_dir, default_journal_dir, list_journal_files, options_dir, set_journal_dir_in_use, steam_libraries, JournalFile};
pub use raw::{LineSource, RawEvent};
pub use reader::{load_history, FileEvents, LoadedEvent};
pub use time::Timestamp;
pub use watcher::{LiveEvent, WatchMsg, Watcher, WatcherConfig};

/// Context passed alongside a typed event when it is applied to the model.
#[derive(Debug, Clone, Copy)]
pub struct EventCtx<'a> {
    pub ts: Timestamp,
    pub name: &'a str,
    pub line: &'a str,
    /// True for events observed live; false during the historical load. Alerts and uploads must
    /// only ever be produced for live events.
    pub live: bool,
}
