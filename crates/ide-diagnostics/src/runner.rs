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
//! | **AST traversal** | `collect_ast_diagnostics` | `parse().descendants()` | None |
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

/// Diagnostics in collect_ast_diagnostics
const AST_DIAGNOSTICS: &[DiagnosticCode] = &[DiagnosticCode::UsingHardcodePath];

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
// Pure AST-based diagnostics (parse + descendants)
// ============================================================================

/// Collect diagnostics that traverse raw AST.
///
/// These diagnostics use `ctx.parse().syntax_node().descendants()`.
/// They should be migrated to HIR or single-pass when possible.
pub fn collect_ast_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if !ctx.config.any_enabled(AST_DIAGNOSTICS) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    diagnostics.extend(run_diagnostic(
        "UsingHardcodePath",
        ctx,
        handlers::using_hardcode_path::check,
    ));

    diagnostics
}

// ============================================================================
// Configuration-based diagnostics (require Configuration XML, SessionModule only)
// ============================================================================

/// Collect diagnostics that require Configuration XML metadata.
///
/// These diagnostics use `ctx.load_configuration()` which loads the full
/// Configuration.xml metadata. They only run for SessionModule files.
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
pub fn collect_sdbl_hir_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Early exit: skip if none of our diagnostics are enabled
    if !ctx.config.any_enabled(SDBL_HIR_DIAGNOSTICS) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    // QueryParseError runs first - detects parse errors at AST level
    diagnostics.extend(run_diagnostic("QueryParseError", ctx, handlers::query_parse_error::check));
    diagnostics.extend(run_diagnostic(
        "AssignAliasFieldsInQuery",
        ctx,
        handlers::assign_alias_fields_in_query::check,
    ));
    diagnostics.extend(run_diagnostic(
        "FieldsFromJoinsWithoutIsNull",
        ctx,
        handlers::fields_from_joins_without_is_null::check,
    ));
    diagnostics.extend(run_diagnostic(
        "FullOuterJoinQuery",
        ctx,
        handlers::full_outer_join_query::check,
    ));
    diagnostics.extend(run_diagnostic(
        "IncorrectUseLikeInQuery",
        ctx,
        handlers::incorrect_use_like_in_query::check,
    ));
    diagnostics.extend(run_diagnostic(
        "JoinWithSubQuery",
        ctx,
        handlers::join_with_sub_query::check,
    ));
    diagnostics.extend(run_diagnostic(
        "JoinWithVirtualTable",
        ctx,
        handlers::join_with_virtual_table::check,
    ));
    diagnostics.extend(run_diagnostic(
        "LogicalOrInJoinQuerySection",
        ctx,
        handlers::logical_or_in_join_query_section::check,
    ));
    diagnostics.extend(run_diagnostic(
        "LogicalOrInTheWhereSectionOfQuery",
        ctx,
        handlers::logical_or_in_the_where_section_of_query::check,
    ));
    diagnostics.extend(run_diagnostic(
        "MultilineStringInQuery",
        ctx,
        handlers::multiline_string_in_query::check,
    ));
    diagnostics.extend(run_diagnostic(
        "QueryNestedFieldsByDot",
        ctx,
        handlers::query_nested_fields_by_dot::check,
    ));
    diagnostics.extend(run_diagnostic(
        "QueryToMissingMetadata",
        ctx,
        handlers::query_to_missing_metadata::check,
    ));
    diagnostics.extend(run_diagnostic("RefOveruse", ctx, handlers::ref_overuse::check));
    diagnostics.extend(run_diagnostic(
        "SelectTopWithoutOrderBy",
        ctx,
        handlers::select_top_without_order_by::check,
    ));
    diagnostics.extend(run_diagnostic("UnionAll", ctx, handlers::union_all::check));
    diagnostics.extend(run_diagnostic(
        "UsingLikeInQuery",
        ctx,
        handlers::using_like_in_query::check,
    ));
    diagnostics.extend(run_diagnostic(
        "VirtualTableCallWithoutParameters",
        ctx,
        handlers::virtual_table_call_without_parameters::check,
    ));

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

    diagnostics
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_ast_diagnostic;
    use crate::DiagnosticCode;

    /// Invariant test: single-pass check_node() must produce same results as legacy check()
    ///
    /// This test ensures that migrated handlers produce identical diagnostics
    /// regardless of whether they're called via single-pass or legacy API.
    #[test]
    fn test_single_pass_invariant_useless_ternary() {
        let code = r#"
Процедура Тест()
    А = ?(Б = 1, Истина, Ложь);
    В = ?(Г = 0, False, True);
    Д = ?(истина, 1, 0);
КонецПроцедуры
"#;

        // Run legacy check()
        let legacy = check_ast_diagnostic(code, handlers::useless_ternary_operator::check);

        // Run single-pass check_node() manually
        let single_pass = check_ast_diagnostic(code, |ctx| {
            let parse = ctx.parse();
            let root = parse.syntax_node();
            let mut diagnostics = Vec::new();
            for node in root.descendants() {
                handlers::useless_ternary_operator::check_node(&node, &mut diagnostics, ctx);
            }
            diagnostics
        });

        // Compare results
        assert_eq!(
            legacy.len(),
            single_pass.len(),
            "Single-pass and legacy must produce same number of diagnostics"
        );

        for (l, s) in legacy.iter().zip(single_pass.iter()) {
            assert_eq!(l.code, s.code, "Diagnostic codes must match");
            assert_eq!(l.range, s.range, "Diagnostic ranges must match");
            assert_eq!(l.message, s.message, "Diagnostic messages must match");
        }
    }

    #[test]
    fn test_single_pass_invariant_double_negatives() {
        let code = r#"
Процедура Тест()
    А = Не (Не Значение);
    Б = Не (Отказ <> Ложь);
    В = Не Отказ <> Ложь;
КонецПроцедуры
"#;

        // Run legacy check()
        let legacy = check_ast_diagnostic(code, handlers::double_negatives::check);

        // Run single-pass check_node() manually
        let single_pass = check_ast_diagnostic(code, |ctx| {
            let parse = ctx.parse();
            let root = parse.syntax_node();
            let mut diagnostics = Vec::new();
            for node in root.descendants() {
                handlers::double_negatives::check_node(&node, &mut diagnostics, ctx);
            }
            diagnostics
        });

        // Compare results
        assert_eq!(
            legacy.len(),
            single_pass.len(),
            "Single-pass and legacy must produce same number of diagnostics. Legacy: {}, Single-pass: {}",
            legacy.len(),
            single_pass.len()
        );

        // Sort by range for stable comparison
        let mut legacy_sorted = legacy.clone();
        let mut single_pass_sorted = single_pass.clone();
        legacy_sorted.sort_by_key(|d| (d.range.start(), d.range.end()));
        single_pass_sorted.sort_by_key(|d| (d.range.start(), d.range.end()));

        for (l, s) in legacy_sorted.iter().zip(single_pass_sorted.iter()) {
            assert_eq!(l.code, s.code, "Diagnostic codes must match");
            assert_eq!(l.range, s.range, "Diagnostic ranges must match");
        }
    }

    #[test]
    fn test_single_pass_comprehensive() {
        // Test with real test files to ensure comprehensive coverage
        let code = r#"// Бессмысленные тернарники
А = ?(Б = 1, Истина, Ложь);// прямой, фиксится в А = Б = 1;
А = ?(Б = 0, False, True);// обратный, фиксится в А = НЕ (Б = 0);
А = ?(Б = 1, True, Истина);
А = ?(Б = 0, Ложь, False);
А = ?(истина, 1, 0);
А = ?(false, 0, 1);

// валидные: одна ветка-литерал — не бесполезный тернарник (null-guard и т.п.)
А = ?(Б = 1, True, 1);
А = ?(Б = 0, 0, False);
СтрокаПредмета.Картинка = МультипредметностьКлиентСервер.ИндексКартинкиРолиПредмета(
            СтрокаПредмета.РольПредмета, ?(СтрокаПредмета.Предмет = Неопределено, Ложь, СтрокаПредмета.Предмет.ПометкаУдаления));

// валидный: обе ветки — не булевы литералы
ОбластьМакета.Параметры.ДебетСубСчета = ОбластьМакета.Параметры.ДебетСубСчета
						+ ?(ПустаяСтрока(ОбластьМакета.Параметры.ДебетСубСчета), "", ", ")
						+ СчетДт;
"#;

        let legacy = check_ast_diagnostic(code, handlers::useless_ternary_operator::check);
        let single_pass = check_ast_diagnostic(code, |ctx| {
            let parse = ctx.parse();
            let root = parse.syntax_node();
            let mut diagnostics = Vec::new();
            for node in root.descendants() {
                handlers::useless_ternary_operator::check_node(&node, &mut diagnostics, ctx);
            }
            diagnostics
        });

        assert_eq!(
            legacy.len(),
            single_pass.len(),
            "Comprehensive test: single-pass and legacy must match"
        );

        // Filter to UselessTernaryOperator only
        let legacy_filtered: Vec<_> =
            legacy.iter().filter(|d| d.code == DiagnosticCode::UselessTernaryOperator).collect();
        let single_pass_filtered: Vec<_> = single_pass
            .iter()
            .filter(|d| d.code == DiagnosticCode::UselessTernaryOperator)
            .collect();

        assert_eq!(
            legacy_filtered.len(),
            single_pass_filtered.len(),
            "Filtered results must match"
        );
    }

    #[test]
    fn test_single_pass_invariant_yo_letter_usage() {
        let code = r#"
Перем ёжик;
Перем Ёлка;
Перем НормальнаяПеременная;
"#;

        // Run legacy check()
        let legacy = check_ast_diagnostic(code, handlers::yo_letter_usage::check);

        // Run single-pass check_token() manually
        let single_pass = check_ast_diagnostic(code, |ctx| {
            let parse = ctx.parse();
            let root = parse.syntax_node();
            let mut diagnostics = Vec::new();
            for element in root.descendants_with_tokens() {
                if let Some(token) = element.into_token() {
                    handlers::yo_letter_usage::check_token(&token, &mut diagnostics, ctx);
                }
            }
            diagnostics
        });

        assert_eq!(
            legacy.len(),
            single_pass.len(),
            "YoLetterUsage: single-pass and legacy must produce same number of diagnostics"
        );

        for (l, s) in legacy.iter().zip(single_pass.iter()) {
            assert_eq!(l.code, s.code, "Diagnostic codes must match");
            assert_eq!(l.range, s.range, "Diagnostic ranges must match");
        }
    }

    #[test]
    fn test_single_pass_invariant_magic_date() {
        let code = r#"
Процедура Тест()
    Дата1 = '20250101' + 1;
    Дата2 = '00010101';
КонецПроцедуры
"#;

        // Run legacy check()
        let legacy = check_ast_diagnostic(code, handlers::magic_date::check);

        // Run single-pass check_token() manually
        let single_pass = check_ast_diagnostic(code, |ctx| {
            let parse = ctx.parse();
            let root = parse.syntax_node();
            let mut diagnostics = Vec::new();
            for element in root.descendants_with_tokens() {
                if let Some(token) = element.into_token() {
                    handlers::magic_date::check_token(&token, &mut diagnostics, ctx);
                }
            }
            diagnostics
        });

        assert_eq!(
            legacy.len(),
            single_pass.len(),
            "MagicDate: single-pass and legacy must produce same number of diagnostics"
        );

        for (l, s) in legacy.iter().zip(single_pass.iter()) {
            assert_eq!(l.code, s.code, "Diagnostic codes must match");
            assert_eq!(l.range, s.range, "Diagnostic ranges must match");
        }
    }

    #[test]
    fn test_single_pass_invariant_unknown_preprocessor_symbol() {
        let code = r#"
#Если Сервер Тогда
#КонецЕсли

#Если НеизвестныйСимвол Тогда
#КонецЕсли
"#;

        // Run legacy check()
        let legacy = check_ast_diagnostic(code, handlers::unknown_preprocessor_symbol::check);

        // Run single-pass check_node() manually
        let single_pass = check_ast_diagnostic(code, |ctx| {
            let parse = ctx.parse();
            let root = parse.syntax_node();
            let mut diagnostics = Vec::new();
            for node in root.descendants() {
                handlers::unknown_preprocessor_symbol::check_node(&node, &mut diagnostics, ctx);
            }
            diagnostics
        });

        assert_eq!(
            legacy.len(),
            single_pass.len(),
            "UnknownPreprocessorSymbol: single-pass and legacy must produce same number of diagnostics"
        );

        for (l, s) in legacy.iter().zip(single_pass.iter()) {
            assert_eq!(l.code, s.code, "Diagnostic codes must match");
            assert_eq!(l.range, s.range, "Diagnostic ranges must match");
        }
    }

    #[test]
    fn test_single_pass_invariant_bad_words() {
        let code = r#"
Процедура Тест()
    // TODO: исправить
    Сообщить("ерунда");
КонецПроцедуры
"#;

        // Run legacy check()
        let legacy = check_ast_diagnostic(code, handlers::bad_words::check);

        // Run single-pass check_node() manually
        let single_pass = check_ast_diagnostic(code, |ctx| {
            let parse = ctx.parse();
            let root = parse.syntax_node();
            let mut diagnostics = Vec::new();
            for node in root.descendants() {
                handlers::bad_words::check_node(&node, &mut diagnostics, ctx);
            }
            diagnostics
        });

        assert_eq!(
            legacy.len(),
            single_pass.len(),
            "BadWords: single-pass and legacy must produce same number of diagnostics"
        );
    }

    #[test]
    fn test_single_pass_invariant_nested_ternary() {
        let code = r#"
Процедура Тест()
    А = ?(Условие, ?(Вложенное, 1, 2), 3);
    Если ?(Условие, Истина, Ложь) Тогда
    КонецЕсли;
КонецПроцедуры
"#;

        // Run legacy check()
        let legacy = check_ast_diagnostic(code, handlers::nested_ternary_operator::check);

        // Run single-pass check_node() manually
        let single_pass = check_ast_diagnostic(code, |ctx| {
            let parse = ctx.parse();
            let root = parse.syntax_node();
            let mut diagnostics = Vec::new();
            for node in root.descendants() {
                handlers::nested_ternary_operator::check_node(&node, &mut diagnostics, ctx);
            }
            diagnostics
        });

        assert_eq!(
            legacy.len(),
            single_pass.len(),
            "NestedTernaryOperator: single-pass and legacy must produce same number of diagnostics"
        );

        for (l, s) in legacy.iter().zip(single_pass.iter()) {
            assert_eq!(l.code, s.code, "Diagnostic codes must match");
            assert_eq!(l.range, s.range, "Diagnostic ranges must match");
        }
    }

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
            ("AST_DIAGNOSTICS", AST_DIAGNOSTICS),
            ("CONFIGURATION_DIAGNOSTICS", CONFIGURATION_DIAGNOSTICS),
            ("SDBL_HIR_DIAGNOSTICS", SDBL_HIR_DIAGNOSTICS),
            ("DATAFLOW_DIAGNOSTICS", DATAFLOW_DIAGNOSTICS),
            ("HIR_DIAGNOSTICS", crate::hir_dispatch::HIR_DIAGNOSTICS),
            ("METADATA_DIAGNOSTICS", crate::metadata_dispatch::METADATA_DIAGNOSTICS),
        ];

        // Build map: code → list of arrays it appears in
        let mut code_to_arrays: HashMap<DiagnosticCode, Vec<&str>> = HashMap::new();
        for (name, codes) in all_arrays {
            for code in *codes {
                code_to_arrays.entry(*code).or_default().push(name);
            }
        }

        // Codes intentionally in multiple collectors (non-overlapping detection paths:
        // HIR handles literals, dataflow handles variables)
        let known_dual_registration: &[DiagnosticCode] =
            &[DiagnosticCode::IncorrectUseOfStrTemplate];

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
