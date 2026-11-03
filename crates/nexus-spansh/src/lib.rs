//! Spansh API client (<https://spansh.co.uk>): route planners and galaxy search.
//!
//! * **Route planners** — neutron, galaxy (exact) with journal-`Loadout` ship parameters, fleet
//!   carrier, Road to Riches (+ ammonia / Earth-like / rocky-HMC), exobiology, trade, tourist,
//!   colonisation and engineer plotters. Each request type implements [`RoutePlanner`]; jobs are
//!   submitted, polled with exponential backoff (resubmitted once if they expire) and decoded
//!   into typed routes ([`Plotted`]). Every route also converts to the common [`Waypoint`] list
//!   ([`RouteWaypoints`]). Cancel with a [`CancellationToken`] or by dropping the future.
//! * **Search & lookup** — autocomplete, system by id64/name, full system dumps, nearest system,
//!   stations/bodies/systems search with typed helpers for commodities, outfitting (several
//!   modules and ships at once), services and ring hotspots, the global search box, and
//!   [`Spansh::search_raw`] as an escape hatch.
//!
//! All response types are `#[serde(default)]` and lenient (Spansh echoes numbers as strings,
//! uses `null` freely and adds fields over time); ids are `u64` throughout.
//!
//! Be polite: one [`Spansh`] per app (it limits itself to 2 requests in flight), debounce
//! autocomplete, cache dumps, and credit "Route data © Spansh".

mod client;
mod de;
pub mod dump;
mod error;
mod job;
mod lookup;
pub mod records;
pub mod routes;
pub mod search;
pub mod ship;
mod util;

#[cfg(test)]
mod tests;

pub use client::{DEFAULT_BASE_URL, Spansh, SpanshBuilder, USER_AGENT};
pub use error::{Error, Result};
pub use job::{
    DEFAULT_JOB_TIMEOUT, JobEnvelope, JobOptions, JobProgress, JobState, PlannerBody, Plotted,
    ProgressFn, RoutePlanner,
};
pub use records::{
    BodyParent, BodyRecord, FactionPresence, FieldValues, Landmark, MarketEntry, NameCount,
    NameShare, OutfittingEntry, PowerProgress, RingRecord, SearchableField, ShipyardEntry,
    SmartSearchHit, SmartSearchResponse, StationRecord, SynthesisRecipe, SystemBody, SystemNames,
    SystemPoint, SystemRecord, SystemStation,
};
pub use routes::carrier::{CarrierFuel, CarrierJump, CarrierRoute, CarrierRouteRequest};
pub use routes::colonisation::{ColonisationJump, ColonisationRoute, ColonisationRouteRequest};
pub use routes::engineer::{
    EngineerBlueprint, EngineerRoute, EngineerRouteRequest, EngineerStop, EngineerVocabulary,
};
pub use routes::galaxy::{GalaxyAlgorithm, GalaxyJump, GalaxyRoute, GalaxyRouteRequest};
pub use routes::neutron::{NeutronRoute, NeutronRouteRequest, SystemJump};
pub use routes::riches::{
    ExobiologyRoute, ExobiologyRouteRequest, RichesRoute, RichesRouteRequest, ScanBody, ScanRoute,
    ScanSystem, Species,
};
pub use routes::tourist::{TouristRoute, TouristRouteRequest};
pub use routes::trade::{
    TradeCommodity, TradeHop, TradePrice, TradeRoute, TradeRouteRequest, TradeStop,
};
pub use routes::{RouteWaypoints, SystemRef, Waypoint};
pub use search::{
    HotspotQuery, MarketBounds, MarketQuery, MarketSide, MarketSort, MaterialTrader, ModuleFilter,
    OutfittingQuery, Reference, SearchKind, SearchRequest, SearchResponse, ServiceQuery, SortDir,
    StationService, TechnologyBroker, filter, search_url, sort,
};
pub use ship::{FsdSpec, LoadoutError, ShipProfile, fsd_spec, guardian_booster_range};
pub use tokio_util::sync::CancellationToken;
pub use util::{Coords, WEBSITE, body_url, parse_timestamp, station_url, system_url};
