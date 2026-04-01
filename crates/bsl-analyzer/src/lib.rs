pub mod tracing {
    pub mod config;
    pub mod hprof;
    pub mod json;
    pub use config::Config;
}

pub mod analysis_host;
pub mod diagnostics_state;
pub mod diff_filter;
pub mod global_state;
pub mod handlers;
pub mod lsp;
pub mod mcp_install;
pub mod mem_docs;
pub mod reporters;
pub mod server;
pub mod task_pool;
pub mod workspace;

// Re-export main server function
pub use server::main_loop;
