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
pub mod compilation_directive_lost;
pub mod consecutive_empty_lines;
pub mod create_query_in_cycle;
pub mod cyclomatic_complexity;
pub mod data_exchange_loading;
#[cfg(test)]
mod debug_test;
pub mod deleting_collection_item;
pub mod deny_incomplete_values;
pub mod deprecated_attributes_8312;
pub mod deprecated_current_date;
pub mod deprecated_find;
pub mod deprecated_message;
pub mod deprecated_methods_8310;
pub mod deprecated_methods_8317;
pub mod deprecated_type_managed_form;
pub mod disable_safe_mode;
pub mod double_negatives;
pub mod duplicate_region;
pub mod duplicate_string_literal;
pub mod duplicated_insertion_into_collection;
pub mod empty_code_block;
pub mod empty_region;
pub mod empty_statement;
pub mod excessive_auto_test_check;
pub mod execute_external_code;
pub mod execute_external_code_in_common_module;
pub mod export_variables;
pub mod external_app_starting;
pub mod extra_commas;
// TODO: Add all 181 handlers
