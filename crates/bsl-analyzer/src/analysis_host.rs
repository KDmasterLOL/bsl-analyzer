//! The analysis host now lives in the shared [`ide_host_core`] crate so the LSP
//! server, the MCP server, and the CLI drive one host with one memory model. This
//! re-export keeps the existing `crate::analysis_host::AnalysisHost` path stable.

pub use ide_host_core::AnalysisHost;
