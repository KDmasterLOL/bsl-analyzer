//! Diagnostic runner helpers.
//!
//! This module provides helper functions for running diagnostics.
//! Diagnostics are organized by data source for clarity and proper caching.
//!
//! ## Architecture
//!
//! | Category | Collector | Data Source | Caching |
//! |----------|-----------|-------------|---------|
//! | **HIR Body emit** | `hir_dispatch::collect_hir_diagnostics` | Lowering | Salsa |
//! | **ItemTree** | `collect_item_tree_diagnostics` | ItemTree + Metadata | Salsa |
//! | **ModuleBodies** | `collect_module_bodies_diagnostics` | HIR bodies | Salsa |
//! | **Configuration** | `collect_configuration_diagnostics` | Configuration XML | Salsa |
//! | **Single-pass AST** | `collect_syntax_single_pass` | One traversal | None |
//! | **Line/Text** | `collect_line_diagnostics` | Raw text | None |
//! | **SDBL** | `collect_sdbl_hir_diagnostics` | SDBL HIR | Salsa |
//! | **Dataflow** | `collect_dataflow_diagnostics` | CFG + liveness | Salsa |
//! | **Metadata HIR** | `metadata_dispatch::collect_metadata_diagnostics` | ModuleMetadata | Salsa |
//!
//! ## Caching Strategy
//!
//! 1. **Salsa-cached (optimal):** HIR Body emit, ItemTree, ModuleBodies, Configuration, SDBL, Dataflow, Metadata
//! 2. **Not cached (keep minimal):** AST traversal, Single-pass AST, Line/Text
//!
//! Migration goal: Maximize Salsa-cached diagnostics (~85%), minimize AST traversal (~15%)
//!
//! ## Single-Pass Architecture
//!
//! The single-pass collector (`collect_syntax_single_pass`) traverses the AST once and calls
//! all migrated handlers via `check_node()` or `check_token()` API. This provides:
//! - **Performance:** O(n) instead of O(n × handlers)
//! - **Cache locality:** Better CPU cache utilization
//! - **Reduced latency:** Faster diagnostics, less UI flicker
//!
//! ### Migrated handlers (9 total):
//! - **Node-based:** useless_ternary_operator, double_negatives, unknown_preprocessor_symbol,
//!   bad_words, typo, nested_ternary_operator
//! - **Token-based:** yo_letter_usage, magic_date
//!
//! Handlers not yet migrated remain in `collect_syntax_diagnostics` (legacy).

use crate::single_pass::collect_syntax_single_pass;
use crate::{handlers, Diagnostic, DiagnosticCode, DiagnosticsContext};

// ============================================================================
// Diagnostic code sets for early exit optimization
// ============================================================================

/// Diagnostics in collect_line_diagnostics
const LINE_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::ParseError,
    DiagnosticCode::ConsecutiveEmptyLines,
    DiagnosticCode::LineLength,
    DiagnosticCode::CommentedCode,
    DiagnosticCode::UsingServiceTag,
    DiagnosticCode::CanonicalSpellingKeywords,
    DiagnosticCode::IncorrectLineBreak,
    DiagnosticCode::InvalidCharacterInFile,
    DiagnosticCode::MissingSpace,
    DiagnosticCode::SpaceAtStartComment,
];

/// Diagnostics in collect_syntax_single_pass
pub(crate) const SINGLE_PASS_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::UselessTernaryOperator,
    DiagnosticCode::DoubleNegatives,
    DiagnosticCode::UnknownPreprocessorSymbol,
    DiagnosticCode::BadWords,
    DiagnosticCode::Typo,
    DiagnosticCode::NestedTernaryOperator,
    DiagnosticCode::YoLetterUsage,
    DiagnosticCode::MagicDate,
    DiagnosticCode::UsingHardcodePath,
];

/// Diagnostics in collect_syntax_diagnostics (excluding single-pass)
const SYNTAX_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::CodeBlockBeforeSub,
    DiagnosticCode::CodeOutOfRegion,
    DiagnosticCode::DuplicateRegion,
    DiagnosticCode::DuplicateStringLiteral,
    DiagnosticCode::ExcessiveAutoTestCheck,
    DiagnosticCode::MultilingualStringHasAllDeclaredLanguages,
    DiagnosticCode::MultilingualStringUsingWithTemplate,
    DiagnosticCode::LatinAndCyrillicSymbolInWord,
    DiagnosticCode::NonStandardRegion,
];

/// Diagnostics in collect_item_tree_diagnostics
const ITEM_TREE_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::OrderOfParams,
    DiagnosticCode::ReservedParameterNames,
    DiagnosticCode::SeveralCompilerDirectives,
    DiagnosticCode::NonExportMethodsInApiRegion,
    DiagnosticCode::CachedPublic,
    DiagnosticCode::CompilationDirectiveLost,
    DiagnosticCode::CompilationDirectiveNeedLess,
    DiagnosticCode::CommandModuleExportMethods,
    DiagnosticCode::ServerSideExportFormMethod,
    DiagnosticCode::OrdinaryAppSupport,
    DiagnosticCode::MissingReturnedValueDescription,
    DiagnosticCode::MissingParameterDescription,
    DiagnosticCode::PublicMethodsDescription,
    DiagnosticCode::MissingVariablesDescription,
    DiagnosticCode::ExecuteExternalCodeInCommonModule,
    DiagnosticCode::CommonModuleMissingAPI,
    DiagnosticCode::PrivilegedModuleMethodCall,
    DiagnosticCode::SetPermissionsForNewObjects,
];

/// Diagnostics in collect_module_bodies_diagnostics
const MODULE_BODIES_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::InternetAccess,
    DiagnosticCode::IsInRoleMethod,
    DiagnosticCode::PairingBrokenTransaction,
    DiagnosticCode::ServerCallsInFormEvents,
    DiagnosticCode::TimeoutsInExternalResources,
    DiagnosticCode::UsingHardcodeSecretInformation,
    DiagnosticCode::DataExchangeLoading,
    DiagnosticCode::TransferringParametersBetweenClientAndServer,
    DiagnosticCode::UnusedLocalMethod,
    DiagnosticCode::IdenticalExpressions,
    DiagnosticCode::DuplicatedInsertionIntoCollection,
    DiagnosticCode::IncorrectUseOfStrTemplate,
    DiagnosticCode::NumberOfValuesInStructureConstructor,
    DiagnosticCode::NestedConstructorsInStructureDeclaration,
    DiagnosticCode::NestedFunctionInParameters,
    DiagnosticCode::MissingCodeTryCatchEx,
    DiagnosticCode::UsingHardcodeNetworkAddress,
];

/// Diagnostics in collect_configuration_diagnostics
const CONFIGURATION_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::ProtectedModule,
    DiagnosticCode::MissingEventSubscriptionHandler,
    DiagnosticCode::ScheduledJobHandler,
];

/// Diagnostics in collect_sdbl_hir_diagnostics
const SDBL_HIR_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::QueryParseError,
    DiagnosticCode::AssignAliasFieldsInQuery,
    DiagnosticCode::FieldsFromJoinsWithoutIsNull,
    DiagnosticCode::FullOuterJoinQuery,
    DiagnosticCode::IncorrectUseLikeInQuery,
    DiagnosticCode::JoinWithSubQuery,
    DiagnosticCode::JoinWithVirtualTable,
    DiagnosticCode::LogicalOrInJoinQuerySection,
    DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
    DiagnosticCode::MultilineStringInQuery,
    DiagnosticCode::QueryNestedFieldsByDot,
    DiagnosticCode::QueryToMissingMetadata,
    DiagnosticCode::RefOveruse,
    DiagnosticCode::SelectTopWithoutOrderBy,
    DiagnosticCode::UnionAll,
    DiagnosticCode::UsingLikeInQuery,
    DiagnosticCode::VirtualTableCallWithoutParameters,
];

/// Diagnostics in collect_dataflow_diagnostics
const DATAFLOW_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::UnreachableCode,
    DiagnosticCode::UnusedLocalVariable,
    DiagnosticCode::UnusedParameters,
    DiagnosticCode::MissingTemporaryFileDeletion,
    DiagnosticCode::MissingTempStorageDeletion,
    // Track 2 §1.6 Group C: lattice-driven (security_state).
    DiagnosticCode::SetPrivilegedMode,
    DiagnosticCode::DisableSafeMode,
];

/// Helper to run a diagnostic and log if it's slow (>80ms).
pub fn run_diagnostic<F>(
    name: &'static str,
    ctx: &DiagnosticsContext,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: FnOnce(&DiagnosticsContext) -> Vec<Diagnostic>,
{
    let start = std::time::Instant::now();
    let _span = tracing::debug_span!("diagnostic", name = name).entered();

    let result = check_fn(ctx);

    let elapsed = start.elapsed();
    if elapsed.as_millis() > 80 {
        tracing::debug!(
            diagnostic = name,
            elapsed_ms = elapsed.as_millis(),
            count = result.len(),
            "Slow diagnostic"
        );
    }

    result
}

/// Collect line-based diagnostics.
///
/// These diagnostics work on lines/text level, not AST nodes.
/// Examples: line_length, consecutive_empty_lines, parse_error, etc.
///
pub fn collect_line_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Early exit: skip if none of our diagnostics are enabled
    if !ctx.config.any_enabled(LINE_DIAGNOSTICS) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    diagnostics.extend(handlers::parse_error::check(ctx));
    diagnostics.extend(handlers::consecutive_empty_lines::check(ctx));
    diagnostics.extend(handlers::line_length::check(ctx));
    diagnostics.extend(handlers::commented_code::check(ctx));
    diagnostics.extend(handlers::using_service_tag::check(ctx));
    diagnostics.extend(handlers::canonical_spelling_keywords::check(ctx));
    diagnostics.extend(handlers::incorrect_line_break::check(ctx));
    diagnostics.extend(handlers::invalid_character_in_file::check(ctx));
    diagnostics.extend(handlers::missing_space::check(ctx));
    diagnostics.extend(handlers::space_at_start_comment::check(ctx));

    // Node-based handlers moved to collect_syntax_single_pass():
    // - bad_words, typo, nested_ternary_operator

    diagnostics
}

/// Collect Tier 1 syntax diagnostics.
///
/// Contains:
/// - Single-pass AST traversal (optimized)
/// - Pure AST diagnostics (parse + descendants)
pub fn collect_syntax_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Early exit: skip if none of our diagnostics are enabled
    // Check both single-pass and syntax-specific diagnostics
    if !ctx.config.any_enabled(SINGLE_PASS_DIAGNOSTICS)
        && !ctx.config.any_enabled(SYNTAX_DIAGNOSTICS)
    {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    // Single-pass handlers (optimized - one traversal for all)
    diagnostics.extend(collect_syntax_single_pass(ctx));

    // Pure AST handlers (parse + descendants)
    diagnostics.extend(run_diagnostic(
        "CodeBlockBeforeSub",
        ctx,
        handlers::code_block_before_sub::check,
    ));
    diagnostics.extend(run_diagnostic("CodeOutOfRegion", ctx, handlers::code_out_of_region::check));
    diagnostics.extend(run_diagnostic("DuplicateRegion", ctx, handlers::duplicate_region::check));
    diagnostics.extend(run_diagnostic(
        "DuplicateStringLiteral",
        ctx,
        handlers::duplicate_string_literal::check,
    ));
    diagnostics.extend(run_diagnostic(
        "ExcessiveAutoTestCheck",
        ctx,
        handlers::excessive_auto_test_check::check,
    ));
    diagnostics.extend(run_diagnostic(
        "MultilingualStringHasAllDeclaredLanguages",
        ctx,
        handlers::multilingual_string_has_all_declared_languages::check,
    ));
    diagnostics.extend(run_diagnostic(
        "MultilingualStringUsingWithTemplate",
        ctx,
        handlers::multilingual_string_using_with_template::check,
    ));
    diagnostics.extend(run_diagnostic(
        "LatinAndCyrillicSymbolInWord",
        ctx,
        handlers::latin_and_cyrillic_symbol_in_word::check,
    ));
    diagnostics.extend(run_diagnostic(
        "NonStandardRegion",
        ctx,
        handlers::non_standard_region::check,
    ));

    diagnostics
}

// ============================================================================
// ItemTree-based diagnostics (method signatures, cached by Salsa)
// ============================================================================

/// Collect diagnostics based on ItemTree (method signatures).
///
/// These diagnostics use Salsa-cached data:
/// - `ctx.item_tree()` - method signatures, parameters, annotations
/// - `ctx.module_metadata()` - module type (FormModule, CommonModule, etc.)
/// - `ctx.region_tree()` - code regions (ПрограммныйИнтерфейс, etc.)
/// - `ctx.method_docs()` - method documentation comments
/// - `ctx.module_data()` - module-level data
///
/// All data sources are cached by Salsa, making these diagnostics efficient.
pub fn collect_item_tree_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Early exit: skip if none of our diagnostics are enabled
    if !ctx.config.any_enabled(ITEM_TREE_DIAGNOSTICS) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    // === Pure ItemTree checks ===

    // Parameter checks
    diagnostics.extend(run_diagnostic("OrderOfParams", ctx, handlers::order_of_params::check));
    diagnostics.extend(run_diagnostic(
        "ReservedParameterNames",
        ctx,
        handlers::reserved_parameter_names::check,
    ));

    // Compiler directives
    diagnostics.extend(run_diagnostic(
        "SeveralCompilerDirectives",
        ctx,
        handlers::several_compiler_directives::check,
    ));

    // === ItemTree + RegionTree checks ===

    // Region/export checks
    diagnostics.extend(run_diagnostic(
        "NonExportMethodsInApiRegion",
        ctx,
        handlers::non_export_methods_in_api_region::check,
    ));

    // Cached module checks (RegionTree + ItemTree + ModuleMetadata)
    diagnostics.extend(run_diagnostic("CachedPublic", ctx, handlers::cached_public::check));

    // === ItemTree + ModuleMetadata checks ===

    // Compilation directive checks (require ModuleMetadata for module type)
    diagnostics.extend(run_diagnostic(
        "CompilationDirectiveLost",
        ctx,
        handlers::compilation_directive_lost::check,
    ));
    diagnostics.extend(run_diagnostic(
        "CompilationDirectiveNeedLess",
        ctx,
        handlers::compilation_directive_need_less::check,
    ));
    diagnostics.extend(run_diagnostic(
        "CommandModuleExportMethods",
        ctx,
        handlers::command_module_export_methods::check,
    ));
    diagnostics.extend(run_diagnostic(
        "ServerSideExportFormMethod",
        ctx,
        handlers::server_side_export_form_method::check,
    ));
    diagnostics.extend(run_diagnostic(
        "OrdinaryAppSupport",
        ctx,
        handlers::ordinary_app_support::check,
    ));

    // === ItemTree + MethodDocs checks ===

    // Documentation checks (require MethodDocs)
    diagnostics.extend(run_diagnostic(
        "MissingReturnedValueDescription",
        ctx,
        handlers::missing_returned_value_description::check,
    ));
    diagnostics.extend(run_diagnostic(
        "MissingParameterDescription",
        ctx,
        handlers::missing_parameter_description::check,
    ));
    diagnostics.extend(run_diagnostic(
        "PublicMethodsDescription",
        ctx,
        handlers::public_methods_description::check,
    ));
    diagnostics.extend(run_diagnostic(
        "MissingVariablesDescription",
        ctx,
        handlers::missing_variables_description::check,
    ));

    // === ItemTree + CommonModule metadata checks ===

    diagnostics.extend(run_diagnostic(
        "ExecuteExternalCodeInCommonModule",
        ctx,
        handlers::execute_external_code_in_common_module::check,
    ));
    diagnostics.extend(run_diagnostic(
        "CommonModuleMissingAPI",
        ctx,
        handlers::common_module_missing_api::check,
    ));
    diagnostics.extend(run_diagnostic(
        "PrivilegedModuleMethodCall",
        ctx,
        handlers::privileged_module_method_call::check,
    ));
    diagnostics.extend(run_diagnostic(
        "SetPermissionsForNewObjects",
        ctx,
        handlers::set_permissions_for_new_objects::check,
    ));

    diagnostics
}

// ============================================================================
// ModuleBodies-based diagnostics (HIR bodies, cached by Salsa)
// ============================================================================

/// Collect diagnostics based on ModuleBodies (HIR method bodies).
///
/// These diagnostics use `ctx.module_bodies()` which is cached by Salsa.
/// They analyze HIR expressions and statements, not raw AST.
pub fn collect_module_bodies_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Early exit: skip if none of our diagnostics are enabled
    if !ctx.config.any_enabled(MODULE_BODIES_DIAGNOSTICS) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    // Security checks
    diagnostics.extend(run_diagnostic("InternetAccess", ctx, handlers::internet_access::check));
    diagnostics.extend(run_diagnostic("IsInRoleMethod", ctx, handlers::is_in_role_method::check));

    // Transaction checks
    diagnostics.extend(run_diagnostic(
        "PairingBrokenTransaction",
        ctx,
        handlers::pairing_broken_transaction::check,
    ));

    // Resource management
    diagnostics.extend(run_diagnostic(
        "TimeoutsInExternalResources",
        ctx,
        handlers::timeouts_in_external_resources::check,
    ));
    diagnostics.extend(run_diagnostic(
        "UsingHardcodeSecretInformation",
        ctx,
        handlers::using_hardcode_secret_information::check,
    ));

    // Module-level checks
    diagnostics.extend(run_diagnostic(
        "DataExchangeLoading",
        ctx,
        handlers::data_exchange_loading::check,
    ));
    diagnostics.extend(run_diagnostic(
        "TransferringParametersBetweenClientAndServer",
        ctx,
        handlers::transferring_parameters_between_client_and_server::check,
    ));
    diagnostics.extend(run_diagnostic(
        "UnusedLocalMethod",
        ctx,
        handlers::unused_local_method::check,
    ));
    diagnostics.extend(run_diagnostic(
        "ServerCallsInFormEvents",
        ctx,
        handlers::server_calls_in_form_events::check,
    ));

    // Expression analysis
    diagnostics.extend(run_diagnostic(
        "IdenticalExpressions",
        ctx,
        handlers::identical_expressions::check,
    ));
    diagnostics.extend(run_diagnostic(
        "DuplicatedInsertionIntoCollection",
        ctx,
        handlers::duplicated_insertion_into_collection::check,
    ));
    diagnostics.extend(run_diagnostic(
        "IncorrectUseOfStrTemplate",
        ctx,
        handlers::incorrect_use_of_str_template::check,
    ));

    // Constructor checks
    diagnostics.extend(run_diagnostic(
        "NumberOfValuesInStructureConstructor",
        ctx,
        handlers::number_of_values_in_structure_constructor::check,
    ));
    diagnostics.extend(run_diagnostic(
        "NestedConstructorsInStructureDeclaration",
        ctx,
        handlers::nested_constructors_in_structure_declaration::check,
    ));
    diagnostics.extend(run_diagnostic(
        "NestedFunctionInParameters",
        ctx,
        handlers::nested_function_in_parameters::check,
    ));

    // Try-catch checks (uses module_bodies)
    diagnostics.extend(run_diagnostic(
        "MissingCodeTryCatchEx",
        ctx,
        handlers::missing_code_try_catch_ex::check,
    ));

    // Hardcoded values (uses module_bodies for string literal iteration)
    diagnostics.extend(run_diagnostic(
        "UsingHardcodeNetworkAddress",
        ctx,
        handlers::using_hardcode_network_address::check,
    ));

    diagnostics
}

// ============================================================================
// Configuration-based diagnostics (require Configuration XML, SessionModule only)
// ============================================================================

/// Collect diagnostics that require Configuration XML metadata.
///
/// These diagnostics use `ctx.main_configuration()` (and CFE-aware
/// helpers like `find_common_module_anywhere` / `is_common_module_anywhere`)
/// to resolve metadata. They only run for SessionModule files.
///
/// Data source: Configuration XML (ScheduledJobs, EventSubscriptions, CommonModules)
pub fn collect_configuration_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Early exit: skip if none of our diagnostics are enabled
    if !ctx.config.any_enabled(CONFIGURATION_DIAGNOSTICS) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    // Protected module check (Configuration + CommonModules)
    diagnostics.extend(run_diagnostic("ProtectedModule", ctx, handlers::protected_module::check));

    // Handler validation (Configuration + ScheduledJobs/EventSubscriptions)
    diagnostics.extend(run_diagnostic(
        "MissingEventSubscriptionHandler",
        ctx,
        handlers::missing_event_subscription_handler::check,
    ));
    diagnostics.extend(run_diagnostic(
        "ScheduledJobHandler",
        ctx,
        handlers::scheduled_job_handler::check,
    ));

    diagnostics
}

/// Collect SDBL HIR-based diagnostics.
///
/// Diagnostics for BSL's SQL-like query language that use SDBL HIR lowering.
/// Diagnostics are collected during SDBL AST→HIR transformation.
///
/// Uses single-pass architecture: shared data (SDBL HIR, file text, line index)
/// is computed once, then all diagnostics are dispatched in a single iteration.
/// This eliminates 16× redundant line index builds and iterations that caused
/// the straggler bottleneck on large files (~26s → ~2s for a 7.8MB file).
pub fn collect_sdbl_hir_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Early exit: skip if none of our diagnostics are enabled
    if !ctx.config.any_enabled(SDBL_HIR_DIAGNOSTICS) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    // QueryParseError is AST-only (no SDBL HIR needed), runs separately
    diagnostics.extend(run_diagnostic("QueryParseError", ctx, handlers::query_parse_error::check));

    // All remaining 16 handlers share the same data — single-pass dispatch
    diagnostics.extend(collect_sdbl_hir_single_pass(ctx));

    diagnostics
}

/// Dispatch table: maps each SDBL diagnostic code to its handler's dispatch function.
///
/// Each handler encapsulates its own matching logic (Strategy pattern / OCP).
/// To add a new SDBL diagnostic, add one line here + a `dispatch` fn in the handler.
const SDBL_DISPATCH: &[(DiagnosticCode, crate::sdbl_utils::SdblDispatchFn)] = &[
    (DiagnosticCode::AssignAliasFieldsInQuery, handlers::assign_alias_fields_in_query::dispatch),
    (
        DiagnosticCode::FieldsFromJoinsWithoutIsNull,
        handlers::fields_from_joins_without_is_null::dispatch,
    ),
    (DiagnosticCode::FullOuterJoinQuery, handlers::full_outer_join_query::dispatch),
    (DiagnosticCode::IncorrectUseLikeInQuery, handlers::incorrect_use_like_in_query::dispatch),
    (DiagnosticCode::JoinWithSubQuery, handlers::join_with_sub_query::dispatch),
    (DiagnosticCode::JoinWithVirtualTable, handlers::join_with_virtual_table::dispatch),
    (
        DiagnosticCode::LogicalOrInJoinQuerySection,
        handlers::logical_or_in_join_query_section::dispatch,
    ),
    (
        DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
        handlers::logical_or_in_the_where_section_of_query::dispatch,
    ),
    (DiagnosticCode::MultilineStringInQuery, handlers::multiline_string_in_query::dispatch),
    (DiagnosticCode::QueryNestedFieldsByDot, handlers::query_nested_fields_by_dot::dispatch),
    (DiagnosticCode::QueryToMissingMetadata, handlers::query_to_missing_metadata::dispatch),
    (DiagnosticCode::RefOveruse, handlers::ref_overuse::dispatch),
    (DiagnosticCode::SelectTopWithoutOrderBy, handlers::select_top_without_order_by::dispatch),
    (DiagnosticCode::UnionAll, handlers::union_all::dispatch),
    (DiagnosticCode::UsingLikeInQuery, handlers::using_like_in_query::dispatch),
    (
        DiagnosticCode::VirtualTableCallWithoutParameters,
        handlers::virtual_table_call_without_parameters::dispatch,
    ),
];

/// Single-pass SDBL HIR diagnostic collector.
///
/// Pre-computes shared data (SDBL HIR, file text, line index) once, then dispatches
/// each diagnostic to all enabled handlers via the `SDBL_DISPATCH` table.
///
/// Previously each of the 16 handlers independently rebuilt the line index and
/// iterated all diagnostics (16× redundant work). This single-pass approach
/// eliminated the straggler bottleneck (~26s → ~2s for a 7.8MB file).
fn collect_sdbl_hir_single_pass(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use crate::sdbl_utils::{build_line_index_shared, SdblPositionMapper};

    // Filter dispatch table to only enabled handlers
    let enabled: Vec<_> =
        SDBL_DISPATCH.iter().filter(|(code, _)| !ctx.is_disabled_with_metadata(*code)).collect();

    if enabled.is_empty() {
        return Vec::new();
    }

    // Pre-compute shared data ONCE
    let sdbl_hirs = ctx.sdbl_hir_in_file();
    let bsl_source = ctx.file_text();
    let sdbl_queries = ctx.all_sdbl_in_file();
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    // SINGLE PASS: one iteration over all queries × diagnostics
    for ((_, sdbl_package), (_, query_info)) in sdbl_hirs.iter().zip(sdbl_queries.iter()) {
        let mapper = SdblPositionMapper::from_query_info(query_info, &bsl_source, &line_starts);

        for hir_diag in sdbl_package.all_diagnostics() {
            for (_, dispatch_fn) in &enabled {
                dispatch_fn(ctx, hir_diag, &mapper, &query_info.query_text, &mut diagnostics);
            }
        }
    }

    diagnostics
}

/// Collect dataflow-based diagnostics.
///
/// Diagnostics that use CFG + dataflow analysis (liveness, reaching definitions).
pub fn collect_dataflow_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Early exit: skip if none of our diagnostics are enabled
    if !ctx.config.any_enabled(DATAFLOW_DIAGNOSTICS) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    diagnostics.extend(run_diagnostic("UnreachableCode", ctx, handlers::unreachable_code::check));
    diagnostics.extend(run_diagnostic(
        "UnusedLocalVariable",
        ctx,
        handlers::unused_local_variable::check,
    ));
    diagnostics.extend(run_diagnostic("UnusedParameters", ctx, handlers::unused_parameters::check));
    diagnostics.extend(run_diagnostic(
        "MissingTemporaryFileDeletion",
        ctx,
        handlers::missing_temporary_file_deletion::check,
    ));
    diagnostics.extend(run_diagnostic(
        "MissingTempStorageDeletion",
        ctx,
        handlers::missing_temp_storage_deletion::check,
    ));
    // Track 2 §1.6 Group C — lattice-driven security-mode handlers
    // (read `module_security_state` through `open_events`).
    diagnostics.extend(run_diagnostic(
        "SetPrivilegedMode",
        ctx,
        handlers::set_privileged_mode::check,
    ));
    diagnostics.extend(run_diagnostic("DisableSafeMode", ctx, handlers::disable_safe_mode::check));

    diagnostics
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticCode;

    /// Every DiagnosticCode must be registered in exactly one collector array.
    ///
    /// This catches forgotten registrations when adding new diagnostics.
    #[test]
    fn test_all_diagnostic_codes_in_exactly_one_collector() {
        use std::collections::HashMap;
        use strum::IntoEnumIterator;

        let all_arrays: &[(&str, &[DiagnosticCode])] = &[
            ("LINE_DIAGNOSTICS", LINE_DIAGNOSTICS),
            ("SINGLE_PASS_DIAGNOSTICS", SINGLE_PASS_DIAGNOSTICS),
            ("SYNTAX_DIAGNOSTICS", SYNTAX_DIAGNOSTICS),
            ("ITEM_TREE_DIAGNOSTICS", ITEM_TREE_DIAGNOSTICS),
            ("MODULE_BODIES_DIAGNOSTICS", MODULE_BODIES_DIAGNOSTICS),
            ("CONFIGURATION_DIAGNOSTICS", CONFIGURATION_DIAGNOSTICS),
            ("SDBL_HIR_DIAGNOSTICS", SDBL_HIR_DIAGNOSTICS),
            ("DATAFLOW_DIAGNOSTICS", DATAFLOW_DIAGNOSTICS),
            ("HIR_DIAGNOSTICS", crate::hir_dispatch::HIR_DIAGNOSTICS),
            ("METADATA_DIAGNOSTICS", crate::metadata_dispatch::METADATA_DIAGNOSTICS),
            ("INFERENCE_DIAGNOSTICS", crate::hir_inference_dispatch::INFERENCE_DIAGNOSTICS),
        ];

        // Build map: code → list of arrays it appears in
        let mut code_to_arrays: HashMap<DiagnosticCode, Vec<&str>> = HashMap::new();
        for (name, codes) in all_arrays {
            for code in *codes {
                code_to_arrays.entry(*code).or_default().push(name);
            }
        }

        // Codes intentionally in multiple collectors (non-overlapping detection paths:
        // HIR handles literals, dataflow handles variables).
        //
        // RedundantAccessToObject and MissedRequiredParameter are dual-registered
        // because their detection is split across two channels with non-overlapping
        // responsibilities:
        //   - HIR_DIAGNOSTICS  — three-level (`Документы.ПКО.Method`) and
        //                        ЭтотОбъект/local-call shapes, classified at body
        //                        lowering time via the positive `MdoType::from_plural`
        //                        gate.
        //   - INFERENCE_DIAGNOSTICS — two-level CommonModule shape, classified at
        //                        inference time via `Resolver::user_common_module_exists`
        //                        (clean-architecture lift — lowering can no longer
        //                        decide "this is a CommonModule call" without the
        //                        receiver type, see `dispatch_bare_ident_field_call`).
        let known_dual_registration: &[DiagnosticCode] = &[
            DiagnosticCode::IncorrectUseOfStrTemplate,
            DiagnosticCode::RedundantAccessToObject,
            DiagnosticCode::MissedRequiredParameter,
        ];

        // Check for unexpected duplicates
        let mut duplicates = Vec::new();
        for (code, arrays) in &code_to_arrays {
            if arrays.len() > 1 && !known_dual_registration.contains(code) {
                duplicates.push(format!("{:?} in: {}", code, arrays.join(", ")));
            }
        }
        assert!(
            duplicates.is_empty(),
            "Diagnostic codes registered in multiple collectors (not in known_dual_registration):\n{}",
            duplicates.join("\n")
        );

        // Check for missing codes
        let mut missing = Vec::new();
        for code in DiagnosticCode::iter() {
            if !code_to_arrays.contains_key(&code) {
                missing.push(format!("{:?}", code));
            }
        }
        assert!(
            missing.is_empty(),
            "Diagnostic codes not registered in any collector:\n{}",
            missing.join("\n")
        );
    }
}
