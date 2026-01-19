//! Diagnostics for bsl-analyzer.
//!
//! This crate implements all 181 diagnostics from bsl-language-server.

mod code;
mod config;
mod context;
mod hir_dispatch;
mod metadata_dispatch;
mod query;
mod runner;
mod types;

pub mod common_module_helpers;
pub mod handlers;
pub mod metadata_diagnostic;
pub mod method_description;
pub mod rules;
pub mod sdbl_utils;
pub mod utils;

#[cfg(test)]
pub mod test_utils;

// Re-exports for public API
pub use code::DiagnosticCode;
pub use config::DiagnosticsConfig;
pub use context::DiagnosticsContext;
pub use query::file_diagnostics_query;
pub use types::{Diagnostic, DiagnosticOutput, DiagnosticTag, Fix, Severity, TextEdit};

use hir_dispatch::collect_hir_diagnostics;
use metadata_dispatch::collect_metadata_diagnostics;
use runner::{collect_text_diagnostics, run_diagnostic};

/// Runs all diagnostics on a file.
pub fn diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut result = Vec::new();

    // Text-based diagnostics (single AST pass)
    result.extend(collect_text_diagnostics(ctx));

    // Tier 1: Syntax diagnostics
    result.extend(run_diagnostic("DoubleNegatives", ctx, handlers::double_negatives::check));
    result.extend(run_diagnostic(
        "DuplicatedInsertionIntoCollection",
        ctx,
        handlers::duplicated_insertion_into_collection::check,
    ));
    result.extend(run_diagnostic(
        "ExcessiveAutoTestCheck",
        ctx,
        handlers::excessive_auto_test_check::check,
    ));
    result.extend(run_diagnostic(
        "IdenticalExpressions",
        ctx,
        handlers::identical_expressions::check,
    ));
    result.extend(run_diagnostic(
        "IncorrectUseOfStrTemplate",
        ctx,
        handlers::incorrect_use_of_str_template::check,
    ));
    result.extend(run_diagnostic(
        "MultilingualStringHasAllDeclaredLanguages",
        ctx,
        handlers::multilingual_string_has_all_declared_languages::check,
    ));
    result.extend(run_diagnostic(
        "MultilingualStringUsingWithTemplate",
        ctx,
        handlers::multilingual_string_using_with_template::check,
    ));
    result.extend(run_diagnostic(
        "NestedConstructorsInStructureDeclaration",
        ctx,
        handlers::nested_constructors_in_structure_declaration::check,
    ));
    result.extend(run_diagnostic(
        "NestedFunctionInParameters",
        ctx,
        handlers::nested_function_in_parameters::check,
    ));
    result.extend(run_diagnostic(
        "NonExportMethodsInApiRegion",
        ctx,
        handlers::non_export_methods_in_api_region::check,
    ));

    // Tier 2: Semantic diagnostics
    result.extend(run_diagnostic(
        "CreateQueryInCycle",
        ctx,
        handlers::create_query_in_cycle::check,
    ));
    result.extend(run_diagnostic(
        "DataExchangeLoading",
        ctx,
        handlers::data_exchange_loading::check,
    ));
    result.extend(run_diagnostic(
        "DeletingCollectionItem",
        ctx,
        handlers::deleting_collection_item::check,
    ));
    result.extend(run_diagnostic(
        "DeprecatedAttributes8312",
        ctx,
        handlers::deprecated_attributes_8312::check,
    ));
    result.extend(run_diagnostic("InternetAccess", ctx, handlers::internet_access::check));
    result.extend(run_diagnostic("IsInRoleMethod", ctx, handlers::is_in_role_method::check));
    result.extend(run_diagnostic(
        "CognitiveComplexity",
        ctx,
        handlers::cognitive_complexity::check,
    ));
    result.extend(run_diagnostic(
        "CyclomaticComplexity",
        ctx,
        handlers::cyclomatic_complexity::check,
    ));
    result.extend(run_diagnostic("MethodSize", ctx, handlers::method_size::check));
    result.extend(run_diagnostic("NestedStatements", ctx, handlers::nested_statements::check));
    result.extend(run_diagnostic(
        "NumberOfOptionalParams",
        ctx,
        handlers::number_of_optional_params::check,
    ));
    result.extend(run_diagnostic("NumberOfParams", ctx, handlers::number_of_params::check));
    result.extend(run_diagnostic("OrderOfParams", ctx, handlers::order_of_params::check));
    result.extend(run_diagnostic(
        "NumberOfValuesInStructureConstructor",
        ctx,
        handlers::number_of_values_in_structure_constructor::check,
    ));
    result.extend(run_diagnostic(
        "MissingCodeTryCatchEx",
        ctx,
        handlers::missing_code_try_catch_ex::check,
    ));
    result.extend(run_diagnostic(
        "MissingTempStorageDeletion",
        ctx,
        handlers::missing_temp_storage_deletion::check,
    ));
    result.extend(run_diagnostic(
        "MissingTemporaryFileDeletion",
        ctx,
        handlers::missing_temporary_file_deletion::check,
    ));

    // Tier 3: Metadata diagnostics
    result.extend(run_diagnostic("CachedPublic", ctx, handlers::cached_public::check));
    result.extend(run_diagnostic(
        "CommandModuleExportMethods",
        ctx,
        handlers::command_module_export_methods::check,
    ));
    result.extend(run_diagnostic(
        "CommonModuleMissingAPI",
        ctx,
        handlers::common_module_missing_api::check,
    ));
    result.extend(run_diagnostic(
        "DenyIncompleteValues",
        ctx,
        handlers::deny_incomplete_values::check,
    ));
    result.extend(run_diagnostic(
        "MetadataObjectNameLength",
        ctx,
        handlers::metadata_object_name_length::check,
    ));
    result.extend(run_diagnostic(
        "MissingReturnedValueDescription",
        ctx,
        handlers::missing_returned_value_description::check,
    ));
    result.extend(run_diagnostic("OrdinaryAppSupport", ctx, handlers::ordinary_app_support::check));

    // SDBL diagnostics
    result.extend(run_diagnostic(
        "AssignAliasFieldsInQuery",
        ctx,
        handlers::assign_alias_fields_in_query::check,
    ));
    result.extend(run_diagnostic(
        "FieldsFromJoinsWithoutIsNull",
        ctx,
        handlers::fields_from_joins_without_is_null::check,
    ));
    result.extend(run_diagnostic(
        "FullOuterJoinQuery",
        ctx,
        handlers::full_outer_join_query::check,
    ));
    result.extend(run_diagnostic("JoinWithSubQuery", ctx, handlers::join_with_sub_query::check));
    result.extend(run_diagnostic(
        "LogicalOrInJoinQuerySection",
        ctx,
        handlers::logical_or_in_join_query_section::check,
    ));
    result.extend(run_diagnostic(
        "LogicalOrInTheWhereSectionOfQuery",
        ctx,
        handlers::logical_or_in_the_where_section_of_query::check,
    ));
    result.extend(run_diagnostic(
        "MultilineStringInQuery",
        ctx,
        handlers::multiline_string_in_query::check,
    ));
    result.extend(run_diagnostic(
        "LatinAndCyrillicSymbolInWord",
        ctx,
        handlers::latin_and_cyrillic_symbol_in_word::check,
    ));

    // HIR-based diagnostics (collected during AST→HIR lowering)
    result.extend(collect_hir_diagnostics(ctx));

    // Dataflow-based diagnostics (using CFG + liveness analysis)
    result.extend(run_diagnostic(
        "UnusedLocalVariable",
        ctx,
        handlers::unused_local_variable::check,
    ));

    // Metadata-based diagnostics (Phase 2: using module_metadata from HIR)
    result.extend(collect_metadata_diagnostics(ctx));

    result
}
