// Licence: GPL-3.0-or-later, with additional terms under its section 7: see LICENSE-ADDITIONAL-TERMS.md at the repository root.
//! Everything Elite Nexus uploads: EDDN, EDSM journal sync, Inara and EDAstro.
//!
//! ```no_run
//! # use nexus_uplink::{Uplink, UplinkConfig, UploadContext};
//! # fn demo(rt: &tokio::runtime::Handle, ev: &nexus_journal::LiveEvent, ctx: &UploadContext) {
//! let uplink = Uplink::new(UplinkConfig { eddn: true, ..Default::default() }, std::path::Path::new("nexus-data"), rt);
//! uplink.on_event(ev, ctx); // after the model applied the live event; only enqueues
//! let status = uplink.status();
//! # let _ = status;
//! # }
//! ```
//!
//! * Only live events are ever uploaded (the history load never reaches the uplink, and events
//!   older than an hour are refused); nothing is sent until the user enables a service.
//! * [`eddn`] is a pure message builder (used by the `validate` example to replay real journals);
//!   [`service`] runs the persistent queues (`data_dir\uplink\*.jsonl`) and the senders.
//! * Every request goes through a [`Transport`]; tests use [`RecordingTransport`].

pub mod config;
pub mod context;
pub mod edastro;
pub mod eddn;
pub mod edsm;
pub mod geo;
pub mod inara;
pub mod json;
mod queue;
pub mod service;
pub mod session;
pub mod status;
pub mod transport;

pub use config::{EdsmCredentials, InaraCredentials, UplinkConfig};
pub use context::{ContextTracker, UploadContext};
pub use service::Uplink;
pub use status::{Account, ServiceStatus, UplinkStatus};
pub use transport::{HttpRequest, HttpResponse, Method, RecordingTransport, ReqwestTransport, Transport, TransportError};
