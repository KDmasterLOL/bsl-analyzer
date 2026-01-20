//! Diagnostic runner helpers.
//!
//! This module provides helper functions for running diagnostics.
//! Each diagnostic type has a dedicated collector function for clean separation.
//!
//! ## Diagnostic Types
//!
//! | Type | Collector | Description |
//! |------|-----------|-------------|
//! | Text-based | `collect_text_diagnostics` | Line/formatting checks |
//! | Syntax (Tier 1) | `collect_syntax_diagnostics` | Syntactic patterns |
//! | Semantic (Tier 2) | `collect_semantic_diagnostics` | Semantic analysis |
//! | Metadata (Tier 3) | `collect_metadata_ast_diagnostics` | AST-based metadata checks |
//! | SDBL | `collect_sdbl_diagnostics` | Query language diagnostics |
//! | Dataflow | `collect_dataflow_diagnostics` | CFG + liveness analysis |
//! | HIR | `hir_dispatch::collect_hir_diagnostics` | HIR lowering byproducts |
//! | Metadata HIR | `metadata_dispatch::collect_metadata_diagnostics` | ModuleMetadata checks |

use crate::{handlers, Diagnostic, DiagnosticsContext};

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
        tracing::warn!(
            diagnostic = name,
            elapsed_ms = elapsed.as_millis(),
            count = result.len(),
            "Slow diagnostic"
        );
    }

    result
}

/// Collect text-based diagnostics in a single AST pass.
///
/// This function performs ONE traversal of the syntax tree and calls all text-based
/// diagnostics on each node. This is much faster than calling each diagnostic separately.
///
/// Pattern from rust-analyzer: crates/ide-diagnostics/src/lib.rs:336-352
pub fn collect_text_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    diagnostics.extend(handlers::parse_error::check(ctx));
    diagnostics.extend(handlers::consecutive_empty_lines::check(ctx));
    diagnostics.extend(handlers::line_length::check(ctx));
    diagnostics.extend(handlers::commented_code::check(ctx));

    for node in root.descendants() {
        handlers::bad_words::check_node(&node, &mut diagnostics, ctx);
    }

    diagnostics
}

/// Collect Tier 1 syntax diagnostics.
///
/// Syntactic pattern checks that don't require semantic analysis.
pub fn collect_syntax_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    diagnostics.extend(run_diagnostic("DoubleNegatives", ctx, handlers::double_negatives::check));
    diagnostics.extend(run_diagnostic(
        "DuplicatedInsertionIntoCollection",
        ctx,
        handlers::duplicated_insertion_into_collection::check,
    ));
    diagnostics.extend(run_diagnostic(
        "ExcessiveAutoTestCheck",
        ctx,
        handlers::excessive_auto_test_check::check,
    ));
    diagnostics.extend(run_diagnostic(
        "IdenticalExpressions",
        ctx,
        handlers::identical_expressions::check,
    ));
    diagnostics.extend(run_diagnostic(
        "IncorrectUseOfStrTemplate",
        ctx,
        handlers::incorrect_use_of_str_template::check,
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
        "NestedConstructorsInStructureDeclaration",
        ctx,
        handlers::nested_constructors_in_structure_declaration::check,
    ));
    diagnostics.extend(run_diagnostic(
        "NestedFunctionInParameters",
        ctx,
        handlers::nested_function_in_parameters::check,
    ));
    diagnostics.extend(run_diagnostic(
        "NonExportMethodsInApiRegion",
        ctx,
        handlers::non_export_methods_in_api_region::check,
    ));
    diagnostics.extend(run_diagnostic(
        "LatinAndCyrillicSymbolInWord",
        ctx,
        handlers::latin_and_cyrillic_symbol_in_word::check,
    ));

    diagnostics
}

/// Collect Tier 2 semantic diagnostics.
///
/// Semantic analysis checks that may use HIR/CFG but are triggered via check().
pub fn collect_semantic_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    diagnostics.extend(run_diagnostic(
        "CreateQueryInCycle",
        ctx,
        handlers::create_query_in_cycle::check,
    ));
    diagnostics.extend(run_diagnostic(
        "DataExchangeLoading",
        ctx,
        handlers::data_exchange_loading::check,
    ));
    diagnostics.extend(run_diagnostic(
        "DeletingCollectionItem",
        ctx,
        handlers::deleting_collection_item::check,
    ));
    diagnostics.extend(run_diagnostic(
        "DeprecatedAttributes8312",
        ctx,
        handlers::deprecated_attributes_8312::check,
    ));
    diagnostics.extend(run_diagnostic("InternetAccess", ctx, handlers::internet_access::check));
    diagnostics.extend(run_diagnostic("IsInRoleMethod", ctx, handlers::is_in_role_method::check));
    diagnostics.extend(run_diagnostic(
        "CognitiveComplexity",
        ctx,
        handlers::cognitive_complexity::check,
    ));
    diagnostics.extend(run_diagnostic(
        "CyclomaticComplexity",
        ctx,
        handlers::cyclomatic_complexity::check,
    ));
    diagnostics.extend(run_diagnostic("MethodSize", ctx, handlers::method_size::check));
    diagnostics.extend(run_diagnostic("NestedStatements", ctx, handlers::nested_statements::check));
    diagnostics.extend(run_diagnostic(
        "NumberOfOptionalParams",
        ctx,
        handlers::number_of_optional_params::check,
    ));
    diagnostics.extend(run_diagnostic("NumberOfParams", ctx, handlers::number_of_params::check));
    diagnostics.extend(run_diagnostic("OrderOfParams", ctx, handlers::order_of_params::check));
    diagnostics.extend(run_diagnostic(
        "NumberOfValuesInStructureConstructor",
        ctx,
        handlers::number_of_values_in_structure_constructor::check,
    ));
    diagnostics.extend(run_diagnostic(
        "MissingCodeTryCatchEx",
        ctx,
        handlers::missing_code_try_catch_ex::check,
    ));
    diagnostics.extend(run_diagnostic(
        "MissingTempStorageDeletion",
        ctx,
        handlers::missing_temp_storage_deletion::check,
    ));
    diagnostics.extend(run_diagnostic(
        "MissingTemporaryFileDeletion",
        ctx,
        handlers::missing_temporary_file_deletion::check,
    ));
    diagnostics.extend(run_diagnostic(
        "PairingBrokenTransaction",
        ctx,
        handlers::pairing_broken_transaction::check,
    ));

    diagnostics
}

/// Collect Tier 3 metadata-related diagnostics (AST-based).
///
/// These diagnostics check metadata properties via AST analysis.
/// Different from metadata_dispatch which uses ModuleMetadata from HIR.
pub fn collect_metadata_ast_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    diagnostics.extend(run_diagnostic("CachedPublic", ctx, handlers::cached_public::check));
    diagnostics.extend(run_diagnostic(
        "CommandModuleExportMethods",
        ctx,
        handlers::command_module_export_methods::check,
    ));
    diagnostics.extend(run_diagnostic(
        "CommonModuleMissingAPI",
        ctx,
        handlers::common_module_missing_api::check,
    ));
    diagnostics.extend(run_diagnostic(
        "DenyIncompleteValues",
        ctx,
        handlers::deny_incomplete_values::check,
    ));
    diagnostics.extend(run_diagnostic(
        "MetadataObjectNameLength",
        ctx,
        handlers::metadata_object_name_length::check,
    ));
    diagnostics.extend(run_diagnostic(
        "MissingReturnedValueDescription",
        ctx,
        handlers::missing_returned_value_description::check,
    ));
    diagnostics.extend(run_diagnostic(
        "PublicMethodsDescription",
        ctx,
        handlers::public_methods_description::check,
    ));
    diagnostics.extend(run_diagnostic(
        "OrdinaryAppSupport",
        ctx,
        handlers::ordinary_app_support::check,
    ));
    diagnostics.extend(run_diagnostic(
        "PrivilegedModuleMethodCall",
        ctx,
        handlers::privileged_module_method_call::check,
    ));
    diagnostics.extend(run_diagnostic("ProtectedModule", ctx, handlers::protected_module::check));

    diagnostics
}

/// Collect SDBL HIR-based diagnostics.
///
/// Diagnostics for BSL's SQL-like query language that use SDBL HIR lowering.
/// Diagnostics are collected during SDBL AST→HIR transformation.
pub fn collect_sdbl_hir_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
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
        "JoinWithSubQuery",
        ctx,
        handlers::join_with_sub_query::check,
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

    diagnostics
}

/// Collect dataflow-based diagnostics.
///
/// Diagnostics that use CFG + dataflow analysis (liveness, reaching definitions).
pub fn collect_dataflow_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    diagnostics.extend(run_diagnostic(
        "UnusedLocalVariable",
        ctx,
        handlers::unused_local_variable::check,
    ));

    diagnostics
}
