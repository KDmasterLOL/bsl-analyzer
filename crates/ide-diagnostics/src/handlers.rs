//! Diagnostic handlers.
//!
//! Each diagnostic has its own handler module.

pub mod all_function_path_must_have_return;
pub mod assign_alias_fields_in_query;
pub mod bad_words;
pub mod begin_transaction_before_try_catch;
pub mod cached_public;
pub mod canonical_spelling_keywords;
pub mod code_after_async_call;
pub mod code_block_before_sub;
pub mod code_out_of_region;
pub mod cognitive_complexity;
pub mod command_module_export_methods;
pub mod commented_code;
pub mod commit_transaction_outside_try_catch;
pub mod common_module_assign;
pub mod common_module_invalid_type;
pub mod common_module_missing_api;
pub mod common_module_name_cached;
pub mod common_module_name_client;
pub mod common_module_name_client_server;
pub mod common_module_name_full_access;
pub mod common_module_name_global;
pub mod common_module_name_global_client;
pub mod common_module_name_server_call;
pub mod common_module_name_words;
pub mod consecutive_empty_lines;
#[cfg(test)]
mod debug_test;
// TODO: Add all 181 handlers
