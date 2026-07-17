pub use base_db;
pub use hir::is_bsl_source;
pub use syntax::TextRange;
pub use vfs;

pub mod database;
pub mod effects;
pub mod features;
pub mod metadata;
pub mod provider;
pub mod queries;
pub mod root_db;
pub mod salsa_events;
pub mod salsa_provider;
pub mod type_kernel;
pub mod types;
pub(crate) mod vfs_helpers;

pub use hir::SdblHirEntries;
pub use types::SymbolKind;

pub use database::{GraphConfigCache, RootDatabaseImpl};
pub use root_db::RootDatabase;

pub use provider::AnalysisProvider;
pub use salsa_provider::SalsaProvider;
pub use vfs_helpers::get_file_path;

pub use hir::{all_sdbl_in_file_query, sdbl_hir_for_file_query};
pub use queries::{
    configuration_path_for_file, effective_target, line_index_query, liveness_analysis_query,
    method_cfg_query, module_metadata_query, reaching_definitions_query, weaving_target,
};

pub use effects::{
    method_effect_summary_query, module_effect_summaries_query, module_security_state_query,
    ModuleEffectSummaries, ModuleSecurityState,
};

pub use metadata::build_module_metadata;
