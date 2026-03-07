pub mod tracing {
    pub mod config;
    pub mod hprof;
    pub mod json;
    pub use config::Config;
}

pub mod diff_filter;
pub mod global_state;
pub mod handlers;
pub mod lsp;
pub mod mem_docs;
pub mod reporters;
pub mod server;
pub mod task_pool;

// Re-export main server function
pub use server::main_loop;
