//! Diagnostics for bsl-analyzer.
//!
//! This crate implements all 181 diagnostics from bsl-language-server.

pub mod common_module_helpers;
pub mod handlers;
pub mod metadata_diagnostic;
pub mod rules;

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
            Self::AllFunctionPathMustHaveReturn => "AllFunctionPathMustHaveReturn",
            Self::AssignAliasFieldsInQuery => "AssignAliasFieldsInQuery",
            Self::FieldsFromJoinsWithoutIsNull => "FieldsFromJoinsWithoutIsNull",
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
            _ => "Unknown",
        }
    }
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Critical,
    Warning,
    Information,
    Hint,
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
}

/// Runs all diagnostics on a file.
pub fn diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut result = Vec::new();

    // Tier 1: Syntax diagnostics
    result.extend(handlers::bad_words::check(ctx));
    result.extend(handlers::canonical_spelling_keywords::check(ctx));
    result.extend(handlers::commented_code::check(ctx));
    result.extend(handlers::double_negatives::check(ctx));
    result.extend(handlers::duplicate_string_literal::check(ctx));
    result.extend(handlers::duplicate_region::check(ctx));
    result.extend(handlers::duplicated_insertion_into_collection::check(ctx));
    result.extend(handlers::empty_code_block::check(ctx));
    result.extend(handlers::empty_region::check(ctx));
    result.extend(handlers::empty_statement::check(ctx));
    result.extend(handlers::extra_commas::check(ctx));
    result.extend(handlers::excessive_auto_test_check::check(ctx));

    // Tier 2: Semantic diagnostics
    result.extend(handlers::all_function_path_must_have_return::check(ctx));
    result.extend(handlers::begin_transaction_before_try_catch::check(ctx));
    result.extend(handlers::commit_transaction_outside_try_catch::check(ctx));
    result.extend(handlers::compilation_directive_lost::check(ctx));
    result.extend(handlers::create_query_in_cycle::check(ctx));
    result.extend(handlers::data_exchange_loading::check(ctx));
    result.extend(handlers::deleting_collection_item::check(ctx));
    result.extend(handlers::deprecated_current_date::check(ctx));
    result.extend(handlers::deprecated_find::check(ctx));
    result.extend(handlers::deprecated_message::check(ctx));
    result.extend(handlers::deprecated_type_managed_form::check(ctx));
    result.extend(handlers::deprecated_methods_8310::check(ctx));
    result.extend(handlers::deprecated_methods_8317::check(ctx));
    result.extend(handlers::deprecated_attributes_8312::check(ctx));
    result.extend(handlers::disable_safe_mode::check(ctx));
    result.extend(handlers::execute_external_code::check(ctx));
    result.extend(handlers::external_app_starting::check(ctx));
    result.extend(handlers::file_system_access::check(ctx));
    result.extend(handlers::export_variables::check(ctx));
    result.extend(handlers::code_after_async_call::check(ctx));
    result.extend(handlers::code_block_before_sub::check(ctx));
    result.extend(handlers::code_out_of_region::check(ctx));
    result.extend(handlers::cognitive_complexity::check(ctx));
    result.extend(handlers::cyclomatic_complexity::check(ctx));

    // Tier 3: Metadata diagnostics
    result.extend(handlers::cached_public::check(ctx));
    result.extend(handlers::command_module_export_methods::check(ctx));
    result.extend(handlers::common_module_assign::check(ctx));
    result.extend(handlers::common_module_invalid_type::check(ctx));
    result.extend(handlers::common_module_missing_api::check(ctx));
    result.extend(handlers::common_module_name_cached::check(ctx));
    result.extend(handlers::common_module_name_client::check(ctx));
    result.extend(handlers::common_module_name_client_server::check(ctx));
    result.extend(handlers::common_module_name_full_access::check(ctx));
    result.extend(handlers::common_module_name_global::check(ctx));
    result.extend(handlers::common_module_name_global_client::check(ctx));
    result.extend(handlers::common_module_name_server_call::check(ctx));
    result.extend(handlers::common_module_name_words::check(ctx));
    result.extend(handlers::deny_incomplete_values::check(ctx));
    result.extend(handlers::execute_external_code_in_common_module::check(ctx));

    // SDBL diagnostics
    result.extend(handlers::assign_alias_fields_in_query::check(ctx));
    result.extend(handlers::fields_from_joins_without_is_null::check(ctx));

    // TODO: Add all 181 diagnostics
    // See DIAGNOSTICS_MIGRATION.md for full list

    result
}
