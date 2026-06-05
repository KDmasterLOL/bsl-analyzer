//! Local-socket broker: one heavy backend per project, with thin auto-started
//! stdio proxies connecting to it instead of each client spawning its own ~3 GB
//! workspace server. This module is pure IPC/discovery infrastructure — no MCP
//! business logic lives here; serving reuses [`crate::serve_stream`].
//!
//! Layout: [`name`] is backend identity/naming; [`daemon`] is the shared backend
//! (accept loop + idle shutdown); [`proxy`] is the thin stdio relay that connects
//! to (or launches) a backend.

pub mod daemon;
mod name;
pub mod proxy;

pub use name::{backend_name, embedding_config_fingerprint, BackendKey};
