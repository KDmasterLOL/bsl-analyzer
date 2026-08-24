//! Search tool execution paths and stable public entrypoints.

mod acquire;
mod docs;
mod gating;
mod hybrid;
mod lexical;
mod render;
mod semantic;
mod status;
#[cfg(test)]
mod test_support;
mod types;

pub use docs::{find_docs, search_docs};
#[allow(unused_imports)]
pub use hybrid::hybrid_code_fenced;
pub(crate) use status::baseline_warming_not_ready;
pub use status::search_status;
