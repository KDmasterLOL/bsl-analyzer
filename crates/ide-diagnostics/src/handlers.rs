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
pub mod consecutive_empty_lines;
#[cfg(test)]
mod debug_test;
// TODO: Add all 181 handlers
