//! The Elite Nexus commander model: every journal event and companion file is folded into a
//! [`GameState`] per commander. Pure logic, no I/O — the app drives it from the journal reader
//! and watcher, and the UI reads it.

pub mod alerts;
pub mod bgs;
pub mod carrier;
pub mod colonisation;
pub mod commander;
pub mod context;
pub mod exobio;
pub mod exploration;
pub mod fleet;
pub mod history;
pub mod inventory;
pub mod location;
pub mod mining;
pub mod missions;
pub mod session;
pub mod state;

pub use alerts::{Alert, AlertKind, Alerts, Priority};
pub use state::{FeedItem, GameState, Model};
