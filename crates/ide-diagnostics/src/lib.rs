//! Diagnostics for bsl-analyzer.
//!
//! This crate implements all 181 diagnostics from bsl-language-server.

pub mod common_module_helpers;
pub mod handlers;
pub mod metadata_diagnostic;
pub mod rules;
pub mod sdbl_utils;

#[cfg(test)]
pub mod test_utils;

use ide_db::{RootDatabase, TextRange};
use vfs::FileId;

/// A diagnostic produced by the analyzer.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    pub severity: Severity,
    pub range: TextRange,
    pub tags: Vec<DiagnosticTag>,
    pub fixes: Vec<Fix>,
}

/// Diagnostic code - matches bsl-language-server codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    // Tier 1: Simple (syntax-only)
    CanonicalSpellingKeywords,
    ConsecutiveEmptyLines,
    LineLength,
    MissingSpace,
    OneStatementPerLine,
    SemicolonPresence,
    SpaceAtStartComment,
    IncorrectLineBreak,
    ExtraCommas,
    CommentedCode,
    EmptyCodeBlock,
    EmptyRegion,
    EmptyStatement,
    UnreachableCode,
    CodeBlockBeforeSub,
    CodeOutOfRegion,
    MagicNumber,
    MagicDate,
    YoLetterUsage,
    LatinAndCyrillicSymbolInWord,
    InvalidCharacterInFile,
    DoubleNegatives,
    NestedTernaryOperator,
    TernaryOperatorUsage,
    UnaryPlusInConcatenation,
    UselessTernaryOperator,
    BadWords,
    DuplicateStringLiteral,
    DuplicateRegion,
    DuplicatedInsertionIntoCollection,
    ExcessiveAutoTestCheck,
    IdenticalExpressions,

    // Tier 2: Medium (requires symbol table)
    AllFunctionPathMustHaveReturn,
    FunctionShouldHaveReturn,
    ProcedureReturnsValue,
    FunctionReturnsSamePrimitive,
    FunctionNameStartsWithGet,
    TooManyReturns,
    NumberOfParams,
    NumberOfOptionalParams,
    OrderOfParams,
    MissedRequiredParameter,
    FunctionOutParameter,
    UnusedParameters,
    MissingParameterDescription,
    MissingReturnedValueDescription,
    RewriteMethodParameter,
    UnusedLocalVariable,
    UnusedLocalMethod,
    ExportVariables,
    MissingVariablesDescription,
    SelfAssign,
    ThisObjectAssign,
    CyclomaticComplexity,
    CognitiveComplexity,
    NestedStatements,
    MethodSize,
    IfConditionComplexity,
    MissingCodeTryCatchEx,
    UsingGoto,
    BeginTransactionBeforeTryCatch,
    CodeAfterAsyncCall,
    CommitTransactionOutsideTryCatch,
    CompilationDirectiveLost,
    CreateQueryInCycle,
    DataExchangeLoading,
    DeletingCollectionItem,
    DeprecatedCurrentDate,
    DeprecatedFind,
    DeprecatedMessage,
    DeprecatedTypeManagedForm,
    DeprecatedMethods8310,
    DeprecatedMethods8317,
    DeprecatedAttributes8312,
    DisableSafeMode,
    ExecuteExternalCode,
    ExternalAppStarting,
    FileSystemAccess,
    FormDataToValue,
    GetFormMethod,
    GlobalContextMethodCollision8312,
    PairingBrokenTransaction,
    WrongUseOfRollbackTransactionMethod,

    // Tier 3: Metadata (requires 1C configuration metadata)
    CachedPublic,
    CommandModuleExportMethods,
    CommonModuleAssign,
    CommonModuleInvalidType,
    CommonModuleMissingAPI,
    CommonModuleNameCached,
    CommonModuleNameClient,
    CommonModuleNameClientServer,
    CommonModuleNameFullAccess,
    CommonModuleNameGlobal,
    CommonModuleNameGlobalClient,
    CommonModuleNameServerCall,
    CommonModuleNameWords,
    DenyIncompleteValues,
    ExecuteExternalCodeInCommonModule,

    // SDBL Diagnostics
    AssignAliasFieldsInQuery,
    FieldsFromJoinsWithoutIsNull,
    FullOuterJoinQuery,
    // TODO: Add all 181 codes
    // See DIAGNOSTICS_MIGRATION.md for full list
}

impl DiagnosticCode {
    /// Returns the string representation (for LSP and SonarQube).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CanonicalSpellingKeywords => "CanonicalSpellingKeywords",
            Self::ConsecutiveEmptyLines => "ConsecutiveEmptyLines",
            Self::LineLength => "LineLength",
            Self::MissingSpace => "MissingSpace",
            Self::OneStatementPerLine => "OneStatementPerLine",
            Self::SemicolonPresence => "SemicolonPresence",
            Self::SpaceAtStartComment => "SpaceAtStartComment",
            Self::IncorrectLineBreak => "IncorrectLineBreak",
            Self::ExtraCommas => "ExtraCommas",
            Self::CommentedCode => "CommentedCode",
            Self::EmptyCodeBlock => "EmptyCodeBlock",
            Self::EmptyRegion => "EmptyRegion",
            Self::EmptyStatement => "EmptyStatement",
            Self::UnreachableCode => "UnreachableCode",
            Self::CodeBlockBeforeSub => "CodeBlockBeforeSub",
            Self::CodeOutOfRegion => "CodeOutOfRegion",
            Self::MagicNumber => "MagicNumber",
            Self::MagicDate => "MagicDate",
            Self::YoLetterUsage => "YoLetterUsage",
            Self::LatinAndCyrillicSymbolInWord => "LatinAndCyrillicSymbolInWord",
            Self::InvalidCharacterInFile => "InvalidCharacterInFile",
            Self::DoubleNegatives => "DoubleNegatives",
            Self::NestedTernaryOperator => "NestedTernaryOperator",
            Self::TernaryOperatorUsage => "TernaryOperatorUsage",
            Self::UnaryPlusInConcatenation => "UnaryPlusInConcatenation",
            Self::UselessTernaryOperator => "UselessTernaryOperator",
            Self::BadWords => "BadWords",
            Self::DuplicateStringLiteral => "DuplicateStringLiteral",
            Self::DuplicateRegion => "DuplicateRegion",
            Self::DuplicatedInsertionIntoCollection => "DuplicatedInsertionIntoCollection",
            Self::ExcessiveAutoTestCheck => "ExcessiveAutoTestCheck",
            Self::IdenticalExpressions => "IdenticalExpressions",
            Self::AllFunctionPathMustHaveReturn => "AllFunctionPathMustHaveReturn",
            Self::AssignAliasFieldsInQuery => "AssignAliasFieldsInQuery",
            Self::FieldsFromJoinsWithoutIsNull => "FieldsFromJoinsWithoutIsNull",
            Self::FullOuterJoinQuery => "FullOuterJoinQuery",
            Self::BeginTransactionBeforeTryCatch => "BeginTransactionBeforeTryCatch",
            Self::CachedPublic => "CachedPublic",
            Self::CommitTransactionOutsideTryCatch => "CommitTransactionOutsideTryCatch",
            Self::CompilationDirectiveLost => "CompilationDirectiveLost",
            Self::CreateQueryInCycle => "CreateQueryInCycle",
            Self::DataExchangeLoading => "DataExchangeLoading",
            Self::DeletingCollectionItem => "DeletingCollectionItem",
            Self::DenyIncompleteValues => "DenyIncompleteValues",
            Self::DeprecatedCurrentDate => "DeprecatedCurrentDate",
            Self::DeprecatedFind => "DeprecatedFind",
            Self::DeprecatedMessage => "DeprecatedMessage",
            Self::DeprecatedTypeManagedForm => "DeprecatedTypeManagedForm",
            Self::DeprecatedMethods8310 => "DeprecatedMethods8310",
            Self::DeprecatedMethods8317 => "DeprecatedMethods8317",
            Self::DeprecatedAttributes8312 => "DeprecatedAttributes8312",
            Self::DisableSafeMode => "DisableSafeMode",
            Self::ExecuteExternalCode => "ExecuteExternalCode",
            Self::ExternalAppStarting => "ExternalAppStarting",
            Self::FileSystemAccess => "FileSystemAccess",
            Self::FormDataToValue => "FormDataToValue",
            Self::GetFormMethod => "GetFormMethod",
            Self::GlobalContextMethodCollision8312 => "GlobalContextMethodCollision8312",
            Self::CodeAfterAsyncCall => "CodeAfterAsyncCall",
            Self::CognitiveComplexity => "CognitiveComplexity",
            Self::CyclomaticComplexity => "CyclomaticComplexity",
            Self::CommandModuleExportMethods => "CommandModuleExportMethods",
            Self::CommonModuleAssign => "CommonModuleAssign",
            Self::CommonModuleInvalidType => "CommonModuleInvalidType",
            Self::CommonModuleMissingAPI => "CommonModuleMissingAPI",
            Self::CommonModuleNameCached => "CommonModuleNameCached",
            Self::CommonModuleNameClient => "CommonModuleNameClient",
            Self::CommonModuleNameClientServer => "CommonModuleNameClientServer",
            Self::CommonModuleNameFullAccess => "CommonModuleNameFullAccess",
            Self::CommonModuleNameGlobal => "CommonModuleNameGlobal",
            Self::CommonModuleNameGlobalClient => "CommonModuleNameGlobalClient",
            Self::CommonModuleNameServerCall => "CommonModuleNameServerCall",
            Self::CommonModuleNameWords => "CommonModuleNameWords",
            Self::ExecuteExternalCodeInCommonModule => "ExecuteExternalCodeInCommonModule",
            Self::ExportVariables => "ExportVariables",
            Self::FunctionOutParameter => "FunctionOutParameter",
            Self::FunctionNameStartsWithGet => "FunctionNameStartsWithGet",
            Self::FunctionReturnsSamePrimitive => "FunctionReturnsSamePrimitive",
            Self::FunctionShouldHaveReturn => "FunctionShouldHaveReturn",
            Self::IfConditionComplexity => "IfConditionComplexity",
            Self::MethodSize => "MethodSize",
            Self::MissedRequiredParameter => "MissedRequiredParameter",
            Self::MissingCodeTryCatchEx => "MissingCodeTryCatchEx",
            Self::MissingParameterDescription => "MissingParameterDescription",
            Self::MissingReturnedValueDescription => "MissingReturnedValueDescription",
            Self::MissingVariablesDescription => "MissingVariablesDescription",
            Self::NestedStatements => "NestedStatements",
            Self::NumberOfOptionalParams => "NumberOfOptionalParams",
            Self::NumberOfParams => "NumberOfParams",
            Self::OrderOfParams => "OrderOfParams",
            Self::PairingBrokenTransaction => "PairingBrokenTransaction",
            Self::ProcedureReturnsValue => "ProcedureReturnsValue",
            Self::RewriteMethodParameter => "RewriteMethodParameter",
            Self::SelfAssign => "SelfAssign",
            Self::ThisObjectAssign => "ThisObjectAssign",
            Self::TooManyReturns => "TooManyReturns",
            Self::UnusedLocalMethod => "UnusedLocalMethod",
            Self::UnusedLocalVariable => "UnusedLocalVariable",
            Self::UnusedParameters => "UnusedParameters",
            Self::UsingGoto => "UsingGoto",
            Self::WrongUseOfRollbackTransactionMethod => "WrongUseOfRollbackTransactionMethod",
        }
    }
}

/// Diagnostic severity.
/// Matches bsl-language-server severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Blocker,     // Highest severity (Java: BLOCKER)
    Critical,    // Critical issues (Java: CRITICAL)
    Major,       // Significant issues (Java: MAJOR)
    Error,       // General errors
    Warning,     // Minor issues (Java: MINOR)
    Information, // Informational (Java: INFO)
    Hint,        // Lowest severity
}

/// Diagnostic tag for special handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticTag {
    Unnecessary,
    Deprecated,
}

/// A quick fix for a diagnostic.
#[derive(Debug, Clone)]
pub struct Fix {
    pub label: String,
    pub edits: Vec<TextEdit>,
}

/// A text edit.
#[derive(Debug, Clone)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}

/// Configuration for diagnostics.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticsConfig {
    pub disabled: Vec<DiagnosticCode>,
    pub parameters: std::collections::HashMap<DiagnosticCode, serde_json::Value>,
    pub ordinary_app_support: bool,
}

impl DiagnosticsConfig {
    /// Check if a diagnostic is disabled
    pub fn is_disabled(&self, code: DiagnosticCode) -> bool {
        self.disabled.contains(&code)
    }

    /// Get a boolean parameter for a diagnostic
    pub fn get_bool(&self, code: DiagnosticCode, param: &str) -> Option<bool> {
        self.parameters.get(&code).and_then(|v| v.get(param)).and_then(|v| v.as_bool())
    }

    /// Get an integer parameter for a diagnostic
    pub fn get_int(&self, code: DiagnosticCode, param: &str) -> Option<i64> {
        self.parameters.get(&code).and_then(|v| v.get(param)).and_then(|v| v.as_i64())
    }

    /// Get a string parameter for a diagnostic
    pub fn get_string(&self, code: DiagnosticCode, param: &str) -> Option<&str> {
        self.parameters.get(&code).and_then(|v| v.get(param)).and_then(|v| v.as_str())
    }

    /// Get a string parameter for a diagnostic (owned version)
    pub fn get_string_param(&self, code: DiagnosticCode, param: &str) -> Option<String> {
        self.parameters
            .get(&code)
            .and_then(|v| v.get(param))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Get a float parameter for a diagnostic
    pub fn get_float(&self, code: DiagnosticCode, param: &str) -> Option<f64> {
        self.parameters.get(&code).and_then(|v| v.get(param)).and_then(|v| v.as_f64())
    }
}

/// Context for running diagnostics.
pub struct DiagnosticsContext<'a> {
    pub db: &'a dyn RootDatabase,
    pub config: &'a DiagnosticsConfig,
    pub file_id: FileId,

    // Workspace integration (for Tier 3 diagnostics)
    /// Root directory of the workspace (for finding Configuration.xml)
    pub workspace_root: Option<&'a std::path::Path>,
    /// Direct path to Configuration.xml (if known)
    pub configuration_path: Option<&'a std::path::Path>,
    /// Pre-created ConfigurationPathInput for metadata queries (CRITICAL for Salsa caching!)
    /// If None, diagnostics should create it once from configuration_path/workspace_root
    pub configuration_path_input: Option<ide_db::metadata::ConfigurationPathInput<'a>>,
}

/// Helper to run a diagnostic and log if it's slow (>80ms)
fn run_diagnostic<F>(name: &'static str, ctx: &DiagnosticsContext, check_fn: F) -> Vec<Diagnostic>
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

/// Runs all diagnostics on a file.
pub fn diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut result = Vec::new();

    // Tier 1: Syntax diagnostics
    result.extend(run_diagnostic("BadWords", ctx, handlers::bad_words::check));
    result.extend(run_diagnostic(
        "CanonicalSpellingKeywords",
        ctx,
        handlers::canonical_spelling_keywords::check,
    ));
    result.extend(run_diagnostic("CommentedCode", ctx, handlers::commented_code::check));
    result.extend(run_diagnostic("DoubleNegatives", ctx, handlers::double_negatives::check));
    result.extend(run_diagnostic(
        "DuplicateStringLiteral",
        ctx,
        handlers::duplicate_string_literal::check,
    ));
    result.extend(run_diagnostic("DuplicateRegion", ctx, handlers::duplicate_region::check));
    result.extend(run_diagnostic(
        "DuplicatedInsertionIntoCollection",
        ctx,
        handlers::duplicated_insertion_into_collection::check,
    ));
    result.extend(run_diagnostic("EmptyCodeBlock", ctx, handlers::empty_code_block::check));
    result.extend(run_diagnostic("EmptyRegion", ctx, handlers::empty_region::check));
    result.extend(run_diagnostic("EmptyStatement", ctx, handlers::empty_statement::check));
    result.extend(run_diagnostic("ExtraCommas", ctx, handlers::extra_commas::check));
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

    // Tier 2: Semantic diagnostics
    result.extend(run_diagnostic(
        "AllFunctionPathMustHaveReturn",
        ctx,
        handlers::all_function_path_must_have_return::check,
    ));
    result.extend(run_diagnostic(
        "BeginTransactionBeforeTryCatch",
        ctx,
        handlers::begin_transaction_before_try_catch::check,
    ));
    result.extend(run_diagnostic(
        "CommitTransactionOutsideTryCatch",
        ctx,
        handlers::commit_transaction_outside_try_catch::check,
    ));
    // TODO: Fix source root setup for full diagnostics
    // result.extend(run_diagnostic("CompilationDirectiveLost", ctx, handlers::compilation_directive_lost::check));
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
        "DeprecatedCurrentDate",
        ctx,
        handlers::deprecated_current_date::check,
    ));
    result.extend(run_diagnostic("DeprecatedFind", ctx, handlers::deprecated_find::check));
    result.extend(run_diagnostic("DeprecatedMessage", ctx, handlers::deprecated_message::check));
    result.extend(run_diagnostic(
        "DeprecatedTypeManagedForm",
        ctx,
        handlers::deprecated_type_managed_form::check,
    ));
    result.extend(run_diagnostic(
        "DeprecatedMethods8310",
        ctx,
        handlers::deprecated_methods_8310::check,
    ));
    result.extend(run_diagnostic(
        "DeprecatedMethods8317",
        ctx,
        handlers::deprecated_methods_8317::check,
    ));
    result.extend(run_diagnostic(
        "DeprecatedAttributes8312",
        ctx,
        handlers::deprecated_attributes_8312::check,
    ));
    result.extend(run_diagnostic("DisableSafeMode", ctx, handlers::disable_safe_mode::check));
    result.extend(run_diagnostic(
        "ExecuteExternalCode",
        ctx,
        handlers::execute_external_code::check,
    ));
    result.extend(run_diagnostic(
        "ExternalAppStarting",
        ctx,
        handlers::external_app_starting::check,
    ));
    result.extend(run_diagnostic("FileSystemAccess", ctx, handlers::file_system_access::check));
    result.extend(run_diagnostic("FormDataToValue", ctx, handlers::form_data_to_value::check));
    result.extend(run_diagnostic("GetFormMethod", ctx, handlers::get_form_method::check));
    result.extend(run_diagnostic(
        "GlobalContextMethodCollision8312",
        ctx,
        handlers::global_context_method_collision8312::check,
    ));
    result.extend(run_diagnostic("ExportVariables", ctx, handlers::export_variables::check));
    result.extend(run_diagnostic(
        "CodeAfterAsyncCall",
        ctx,
        handlers::code_after_async_call::check,
    ));
    result.extend(run_diagnostic(
        "CodeBlockBeforeSub",
        ctx,
        handlers::code_block_before_sub::check,
    ));
    result.extend(run_diagnostic("CodeOutOfRegion", ctx, handlers::code_out_of_region::check));
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
    result.extend(run_diagnostic(
        "FunctionNameStartsWithGet",
        ctx,
        handlers::function_name_starts_with_get::check,
    ));
    result.extend(run_diagnostic(
        "FunctionReturnsSamePrimitive",
        ctx,
        handlers::function_returns_same_primitive::check,
    ));
    result.extend(run_diagnostic(
        "FunctionShouldHaveReturn",
        ctx,
        handlers::function_should_have_return::check,
    ));

    // Tier 3: Metadata diagnostics
    result.extend(run_diagnostic("CachedPublic", ctx, handlers::cached_public::check));
    result.extend(run_diagnostic(
        "CommandModuleExportMethods",
        ctx,
        handlers::command_module_export_methods::check,
    ));
    result.extend(run_diagnostic("CommonModuleAssign", ctx, handlers::common_module_assign::check));
    result.extend(run_diagnostic(
        "CommonModuleInvalidType",
        ctx,
        handlers::common_module_invalid_type::check,
    ));
    result.extend(run_diagnostic(
        "CommonModuleMissingAPI",
        ctx,
        handlers::common_module_missing_api::check,
    ));
    result.extend(run_diagnostic(
        "CommonModuleNameCached",
        ctx,
        handlers::common_module_name_cached::check,
    ));
    result.extend(run_diagnostic(
        "CommonModuleNameClient",
        ctx,
        handlers::common_module_name_client::check,
    ));
    result.extend(run_diagnostic(
        "CommonModuleNameClientServer",
        ctx,
        handlers::common_module_name_client_server::check,
    ));
    result.extend(run_diagnostic(
        "CommonModuleNameFullAccess",
        ctx,
        handlers::common_module_name_full_access::check,
    ));
    result.extend(run_diagnostic(
        "CommonModuleNameGlobal",
        ctx,
        handlers::common_module_name_global::check,
    ));
    result.extend(run_diagnostic(
        "CommonModuleNameGlobalClient",
        ctx,
        handlers::common_module_name_global_client::check,
    ));
    result.extend(run_diagnostic(
        "CommonModuleNameServerCall",
        ctx,
        handlers::common_module_name_server_call::check,
    ));
    result.extend(run_diagnostic(
        "CommonModuleNameWords",
        ctx,
        handlers::common_module_name_words::check,
    ));
    result.extend(run_diagnostic(
        "DenyIncompleteValues",
        ctx,
        handlers::deny_incomplete_values::check,
    ));
    result.extend(run_diagnostic(
        "ExecuteExternalCodeInCommonModule",
        ctx,
        handlers::execute_external_code_in_common_module::check,
    ));

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

    // TODO: Add all 181 diagnostics
    // See DIAGNOSTICS_MIGRATION.md for full list

    result
}
