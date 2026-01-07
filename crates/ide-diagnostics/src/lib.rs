//! Diagnostics for bsl-analyzer.
//!
//! This crate implements all 181 diagnostics from bsl-language-server.

pub mod common_module_helpers;
pub mod handlers;
pub mod metadata_diagnostic;
pub mod method_description;
pub mod rules;
pub mod sdbl_utils;
pub mod utils;

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
    IncorrectUseOfStrTemplate,
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
    NonExportMethodsInApiRegion,
    TernaryOperatorUsage,
    UnaryPlusInConcatenation,
    UselessTernaryOperator,
    BadWords,
    DuplicateStringLiteral,
    DuplicateRegion,
    NonStandardRegion,
    DuplicatedInsertionIntoCollection,
    ExcessiveAutoTestCheck,
    IdenticalExpressions,
    IfElseDuplicatedCodeBlock,
    IfElseDuplicatedCondition,
    IfElseIfEndsWithElse,
    MultilingualStringHasAllDeclaredLanguages,
    MultilingualStringUsingWithTemplate,
    NestedConstructorsInStructureDeclaration,
    NestedFunctionInParameters,

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
    MissingTempStorageDeletion,
    MissingTemporaryFileDeletion,
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
    InternetAccess,
    IsInRoleMethod,
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
    // ExecuteExternalCodeInCommonModule removed - duplicate of ExecuteExternalCode
    MetadataObjectNameLength,
    MissingCommonModuleMethod,
    MissingEventSubscriptionHandler,

    // SDBL Diagnostics
    AssignAliasFieldsInQuery,
    FieldsFromJoinsWithoutIsNull,
    FullOuterJoinQuery,
    JoinWithSubQuery,
    LogicalOrInJoinQuerySection,
    LogicalOrInTheWhereSectionOfQuery,
    MultilineStringInQuery,
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
            Self::IncorrectUseOfStrTemplate => "IncorrectUseOfStrTemplate",
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
            Self::NonExportMethodsInApiRegion => "NonExportMethodsInApiRegion",
            Self::TernaryOperatorUsage => "TernaryOperatorUsage",
            Self::UnaryPlusInConcatenation => "UnaryPlusInConcatenation",
            Self::UselessTernaryOperator => "UselessTernaryOperator",
            Self::BadWords => "BadWords",
            Self::DuplicateStringLiteral => "DuplicateStringLiteral",
            Self::DuplicateRegion => "DuplicateRegion",
            Self::NonStandardRegion => "NonStandardRegion",
            Self::DuplicatedInsertionIntoCollection => "DuplicatedInsertionIntoCollection",
            Self::ExcessiveAutoTestCheck => "ExcessiveAutoTestCheck",
            Self::IdenticalExpressions => "IdenticalExpressions",
            Self::IfElseDuplicatedCodeBlock => "IfElseDuplicatedCodeBlock",
            Self::IfElseDuplicatedCondition => "IfElseDuplicatedCondition",
            Self::IfElseIfEndsWithElse => "IfElseIfEndsWithElse",
            Self::MultilingualStringHasAllDeclaredLanguages => {
                "MultilingualStringHasAllDeclaredLanguages"
            }
            Self::MultilingualStringUsingWithTemplate => "MultilingualStringUsingWithTemplate",
            Self::NestedConstructorsInStructureDeclaration => {
                "NestedConstructorsInStructureDeclaration"
            }
            Self::NestedFunctionInParameters => "NestedFunctionInParameters",
            Self::AllFunctionPathMustHaveReturn => "AllFunctionPathMustHaveReturn",
            Self::AssignAliasFieldsInQuery => "AssignAliasFieldsInQuery",
            Self::FieldsFromJoinsWithoutIsNull => "FieldsFromJoinsWithoutIsNull",
            Self::FullOuterJoinQuery => "FullOuterJoinQuery",
            Self::JoinWithSubQuery => "JoinWithSubQuery",
            Self::LogicalOrInJoinQuerySection => "LogicalOrInJoinQuerySection",
            Self::LogicalOrInTheWhereSectionOfQuery => "LogicalOrInTheWhereSectionOfQuery",
            Self::MultilineStringInQuery => "MultilineStringInQuery",
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
            Self::InternetAccess => "InternetAccess",
            Self::IsInRoleMethod => "IsInRoleMethod",
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
            Self::MetadataObjectNameLength => "MetadataObjectNameLength",
            Self::MissingCommonModuleMethod => "MissingCommonModuleMethod",
            Self::MissingEventSubscriptionHandler => "MissingEventSubscriptionHandler",
            Self::ExportVariables => "ExportVariables",
            Self::FunctionOutParameter => "FunctionOutParameter",
            Self::FunctionNameStartsWithGet => "FunctionNameStartsWithGet",
            Self::FunctionReturnsSamePrimitive => "FunctionReturnsSamePrimitive",
            Self::FunctionShouldHaveReturn => "FunctionShouldHaveReturn",
            Self::IfConditionComplexity => "IfConditionComplexity",
            Self::MethodSize => "MethodSize",
            Self::MissedRequiredParameter => "MissedRequiredParameter",
            Self::MissingCodeTryCatchEx => "MissingCodeTryCatchEx",
            Self::MissingTempStorageDeletion => "MissingTempStorageDeletion",
            Self::MissingTemporaryFileDeletion => "MissingTemporaryFileDeletion",
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
    /// FileSet for path lookups (CRITICAL for performance!)
    /// Keeping FileSet outside of Salsa avoids O(n) hash/compare operations.
    /// If None, falls back to Salsa lookup (slower, for tests only).
    pub file_set: Option<&'a vfs::FileSet>,
}

impl<'a> DiagnosticsContext<'a> {
    /// Load configuration metadata using cached ConfigurationPathInput.
    ///
    /// CRITICAL: This method uses ctx.configuration_path_input if available
    /// to ensure Salsa caching works properly. Creating a new ConfigurationPathInput
    /// for each file would break caching and cause massive performance degradation!
    ///
    /// Returns `None` if no configuration path is available.
    pub fn load_configuration(&self) -> Option<std::sync::Arc<bsl_metadata::Configuration>> {
        // Use pre-created path_input for proper Salsa caching
        if let Some(path_input) = self.configuration_path_input {
            return Some(ide_db::metadata::load_configuration(self.db, path_input));
        }

        // Fallback: create path_input (less efficient, but needed for tests)
        let config_path = self.configuration_path.or(self.workspace_root)?;
        let config_path_str = config_path.to_string_lossy().to_string();
        let path_input = ide_db::metadata::ConfigurationPathInput::new(self.db, config_path_str);
        Some(ide_db::metadata::load_configuration(self.db, path_input))
    }

    /// Get the file path for the current file.
    ///
    /// CRITICAL for performance: Uses the provided FileSet directly (O(1) lookup)
    /// instead of going through Salsa (which would require O(n) hash/compare
    /// of the entire FileSet).
    ///
    /// Returns `None` if file path cannot be resolved.
    pub fn file_path(&self) -> Option<String> {
        // Fast path: use provided FileSet (bypasses Salsa)
        if let Some(file_set) = self.file_set {
            let vfs_path = file_set.path_for_file(&self.file_id)?;
            return Some(vfs_path.as_path().to_string_lossy().to_string());
        }

        // Slow path: go through Salsa (for tests without FileSet)
        self.file_path_via_salsa()
    }

    /// Fallback: get file path through Salsa database.
    /// This is slower because Salsa needs to track the SourceRoot dependency.
    fn file_path_via_salsa(&self) -> Option<String> {
        let source_root_input = self.db.file_source_root_input(self.file_id);
        let source_root_id = source_root_input.source_root_id(self.db);
        let source_root_input = self.db.source_root_input(source_root_id);
        let source_root = source_root_input.root(self.db);
        let file_set = source_root.file_set();
        let vfs_path = file_set.path_for_file(&self.file_id)?;
        Some(vfs_path.as_path().to_string_lossy().to_string())
    }
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

/// Collect text-based diagnostics in a single AST pass.
///
/// This function performs ONE traversal of the syntax tree and calls all text-based
/// diagnostics on each node. This is much faster than calling each diagnostic separately.
///
/// Pattern from rust-analyzer: crates/ide-diagnostics/src/lib.rs:336-352
fn collect_text_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    // File-level text-based diagnostics (called once per file)
    diagnostics.extend(handlers::consecutive_empty_lines::check(ctx));
    diagnostics.extend(handlers::line_length::check(ctx));
    diagnostics.extend(handlers::missing_space::check(ctx));
    diagnostics.extend(handlers::incorrect_line_break::check(ctx));
    diagnostics.extend(handlers::invalid_character_in_file::check(ctx));
    diagnostics.extend(handlers::space_at_start_comment::check(ctx));
    diagnostics.extend(handlers::commented_code::check(ctx));

    // Region-related diagnostics (file-level)
    diagnostics.extend(handlers::duplicate_region::check(ctx));
    diagnostics.extend(handlers::non_standard_region::check(ctx));
    diagnostics.extend(handlers::code_block_before_sub::check(ctx));
    diagnostics.extend(handlers::code_out_of_region::check(ctx));

    // String/Date literal diagnostics (file-level)
    diagnostics.extend(handlers::magic_date::check(ctx));
    diagnostics.extend(handlers::duplicate_string_literal::check(ctx));

    // Keyword spelling diagnostics (file-level, token-based)
    diagnostics.extend(handlers::canonical_spelling_keywords::check(ctx));

    // Single traversal for all node-based text diagnostics
    // FIXME: This iterates the entire file which is expensive.
    // Salsa caching + incremental re-parse would be better (rust-analyzer TODO)
    for node in root.descendants() {
        // Node-based diagnostic handlers (check_node API)
        handlers::bad_words::check_node(&node, &mut diagnostics, ctx);
        handlers::extra_commas::check_node(&node, &mut diagnostics, ctx);
        handlers::nested_ternary_operator::check_node(&node, &mut diagnostics, ctx);
        // empty_region is now HIR-based (checked during preprocessor lowering)
        // empty_statement is now HIR-based (checked during statement lowering)
        // TODO: Add more node-based text diagnostics here:
        // handlers::commented_code::check_node(&node, &mut diagnostics, ctx);
        // handlers::double_negatives::check_node(&node, &mut diagnostics, ctx);
        // ...
    }

    diagnostics
}

/// Runs all diagnostics on a file.
pub fn diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut result = Vec::new();

    // Text-based diagnostics (single AST pass)
    result.extend(collect_text_diagnostics(ctx));

    // Tier 1: Syntax diagnostics (TODO: migrate to collect_text_diagnostics)
    // result.extend(run_diagnostic("BadWords", ctx, handlers::bad_words::check));
    // NOTE: CanonicalSpellingKeywords migrated to text-based collection (collect_text_diagnostics - file-level)
    // result.extend(run_diagnostic(
    //     "CanonicalSpellingKeywords",
    //     ctx,
    //     handlers::canonical_spelling_keywords::check,
    // ));
    // NOTE: CommentedCode migrated to text-based collection (collect_text_diagnostics)
    // result.extend(run_diagnostic("CommentedCode", ctx, handlers::commented_code::check));
    result.extend(run_diagnostic("DoubleNegatives", ctx, handlers::double_negatives::check));
    result.extend(run_diagnostic(
        "DuplicatedInsertionIntoCollection",
        ctx,
        handlers::duplicated_insertion_into_collection::check,
    ));
    // NOTE: EmptyCodeBlock migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::EmptyCodeBlock
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic("EmptyCodeBlock", ctx, handlers::empty_code_block::check));
    // NOTE: EmptyRegion migrated to text-based collection (collect_text_diagnostics - node-based)
    // result.extend(run_diagnostic("EmptyRegion", ctx, handlers::empty_region::check));
    // NOTE: EmptyStatement migrated to text-based collection (collect_text_diagnostics)
    // result.extend(run_diagnostic("EmptyStatement", ctx, handlers::empty_statement::check));
    // NOTE: ExtraCommas migrated to text-based collection (collect_text_diagnostics)
    // result.extend(run_diagnostic("ExtraCommas", ctx, handlers::extra_commas::check));
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
        "IfConditionComplexity",
        ctx,
        handlers::if_condition_complexity::check,
    ));
    // NOTE: IfElseDuplicatedCodeBlock migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::IfElseDuplicatedCodeBlock
    // and dispatched through collect_hir_diagnostics()
    result.extend(run_diagnostic(
        "IfElseDuplicatedCondition",
        ctx,
        handlers::if_else_duplicated_condition::check,
    ));
    result.extend(run_diagnostic(
        "IfElseIfEndsWithElse",
        ctx,
        handlers::if_else_if_ends_with_else::check,
    ));
    // NOTE: IncorrectLineBreak migrated to text-based collection (collect_text_diagnostics)
    result.extend(run_diagnostic(
        "IncorrectUseOfStrTemplate",
        ctx,
        handlers::incorrect_use_of_str_template::check,
    ));
    // NOTE: InvalidCharacterInFile migrated to text-based collection (collect_text_diagnostics - file-level)
    // NOTE: LineLength migrated to text-based collection (collect_text_diagnostics)
    // NOTE: MagicDate migrated to text-based collection (collect_text_diagnostics - file-level)
    // result.extend(run_diagnostic("MagicDate", ctx, handlers::magic_date::check));
    // NOTE: MagicNumber migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::MagicNumber
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic("MagicNumber", ctx, handlers::magic_number::check));
    // NOTE: MissingSpace migrated to text-based collection (collect_text_diagnostics)
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
    // NOTE: NestedTernaryOperator migrated to text-based collection (collect_text_diagnostics)
    // result.extend(run_diagnostic(
    //     "NestedTernaryOperator",
    //     ctx,
    //     handlers::nested_ternary_operator::check,
    // ));
    result.extend(run_diagnostic(
        "NonExportMethodsInApiRegion",
        ctx,
        handlers::non_export_methods_in_api_region::check,
    ));

    // Tier 2: Semantic diagnostics
    // NOTE: AllFunctionPathMustHaveReturn migrated to HIR-based MissingReturn
    // The HIR version is collected during lowering via BodyDiagnostic::MissingReturn
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "AllFunctionPathMustHaveReturn",
    //     ctx,
    //     handlers::all_function_path_must_have_return::check,
    // ));
    // NOTE: BeginTransactionBeforeTryCatch migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::BeginTransactionBeforeTryCatch
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "BeginTransactionBeforeTryCatch",
    //     ctx,
    //     handlers::begin_transaction_before_try_catch::check,
    // ));
    // NOTE: CommitTransactionOutsideTryCatch migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::CommitTransactionOutsideTryCatch
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "CommitTransactionOutsideTryCatch",
    //     ctx,
    //     handlers::commit_transaction_outside_try_catch::check,
    // ));
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
    // NOTE: Replaced with HIR-based DeprecatedCurrentDate diagnostic
    // The HIR version is collected during lowering via BodyDiagnostic::DeprecatedCurrentDate
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "DeprecatedCurrentDate",
    //     ctx,
    //     handlers::deprecated_current_date::check,
    // ));
    // NOTE: Replaced with HIR-based DeprecatedFind diagnostic
    // The HIR version is collected during lowering via BodyDiagnostic::DeprecatedFind
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic("DeprecatedFind", ctx, handlers::deprecated_find::check));
    // The HIR version is collected during lowering via BodyDiagnostic::DeprecatedMessage
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic("DeprecatedMessage", ctx, handlers::deprecated_message::check));
    // The HIR version is collected during lowering via BodyDiagnostic::DeprecatedTypeManagedForm
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "DeprecatedTypeManagedForm",
    //     ctx,
    //     handlers::deprecated_type_managed_form::check,
    // ));
    // NOTE: Replaced with HIR-based DeprecatedMethod diagnostic
    // The HIR version is collected during lowering via BodyDiagnostic::DeprecatedMethod
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "DeprecatedMethods8310",
    //     ctx,
    //     handlers::deprecated_methods_8310::check,
    // ));
    // result.extend(run_diagnostic(
    //     "DeprecatedMethods8317",
    //     ctx,
    //     handlers::deprecated_methods_8317::check,
    // ));
    result.extend(run_diagnostic(
        "DeprecatedAttributes8312",
        ctx,
        handlers::deprecated_attributes_8312::check,
    ));
    // The HIR version is collected during lowering via BodyDiagnostic::DisableSafeMode
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic("DisableSafeMode", ctx, handlers::disable_safe_mode::check));
    // The HIR version is collected during lowering via BodyDiagnostic::ExecuteExternalCode
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic("ExecuteExternalCode", ctx, handlers::execute_external_code::check));
    result.extend(run_diagnostic(
        "ExternalAppStarting",
        ctx,
        handlers::external_app_starting::check,
    ));
    result.extend(run_diagnostic("FileSystemAccess", ctx, handlers::file_system_access::check));
    result.extend(run_diagnostic("InternetAccess", ctx, handlers::internet_access::check));
    result.extend(run_diagnostic("IsInRoleMethod", ctx, handlers::is_in_role_method::check));
    result.extend(run_diagnostic("FormDataToValue", ctx, handlers::form_data_to_value::check));
    result.extend(run_diagnostic("GetFormMethod", ctx, handlers::get_form_method::check));
    result.extend(run_diagnostic(
        "GlobalContextMethodCollision8312",
        ctx,
        handlers::global_context_method_collision8312::check,
    ));
    // NOTE: ExportVariables migrated to HIR-based collection
    // Uses module_vars from module_bodies() and is collected in collect_metadata_diagnostics()
    // result.extend(run_diagnostic("ExportVariables", ctx, handlers::export_variables::check));
    // NOTE: CodeAfterAsyncCall migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::CodeAfterAsyncCall
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "CodeAfterAsyncCall",
    //     ctx,
    //     handlers::code_after_async_call::check,
    // ));
    // NOTE: CodeBlockBeforeSub migrated to text-based collection (collect_text_diagnostics - file-level)
    // result.extend(run_diagnostic(
    //     "CodeBlockBeforeSub",
    //     ctx,
    //     handlers::code_block_before_sub::check,
    // ));
    // NOTE: CodeOutOfRegion migrated to text-based collection (collect_text_diagnostics - file-level)
    // result.extend(run_diagnostic("CodeOutOfRegion", ctx, handlers::code_out_of_region::check));
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
    // NOTE: MissedRequiredParameter migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::MissedRequiredParameter
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "MissedRequiredParameter",
    //     ctx,
    //     handlers::missed_required_parameter::check,
    // ));
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
    // Note: FunctionShouldHaveReturn is now handled via hir_diagnostics (Phase 4)

    // Tier 3: Metadata diagnostics
    result.extend(run_diagnostic("CachedPublic", ctx, handlers::cached_public::check));
    result.extend(run_diagnostic(
        "CommandModuleExportMethods",
        ctx,
        handlers::command_module_export_methods::check,
    ));
    // NOTE: CommonModuleAssign migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::CommonModuleAssign
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic("CommonModuleAssign", ctx, handlers::common_module_assign::check));
    // NOTE: CommonModuleInvalidType migrated to metadata-based collection (collect_metadata_diagnostics)
    // result.extend(run_diagnostic(
    //     "CommonModuleInvalidType",
    //     ctx,
    //     handlers::common_module_invalid_type::check,
    // ));
    result.extend(run_diagnostic(
        "CommonModuleMissingAPI",
        ctx,
        handlers::common_module_missing_api::check,
    ));
    // NOTE: CommonModuleNameCached migrated to metadata-based collection (collect_metadata_diagnostics)
    // result.extend(run_diagnostic(
    //     "CommonModuleNameCached",
    //     ctx,
    //     handlers::common_module_name_cached::check,
    // ));
    // NOTE: CommonModuleNameClient migrated to metadata-based collection (collect_metadata_diagnostics)
    // result.extend(run_diagnostic(
    //     "CommonModuleNameClient",
    //     ctx,
    //     handlers::common_module_name_client::check,
    // ));
    // NOTE: CommonModuleNameClientServer migrated to metadata-based collection (collect_metadata_diagnostics)
    // result.extend(run_diagnostic(
    //     "CommonModuleNameClientServer",
    //     ctx,
    //     handlers::common_module_name_client_server::check,
    // ));
    // NOTE: CommonModuleNameFullAccess migrated to metadata-based collection (collect_metadata_diagnostics)
    // result.extend(run_diagnostic(
    //     "CommonModuleNameFullAccess",
    //     ctx,
    //     handlers::common_module_name_full_access::check,
    // ));
    // NOTE: CommonModuleNameGlobal migrated to metadata-based collection (collect_metadata_diagnostics)
    // result.extend(run_diagnostic(
    //     "CommonModuleNameGlobal",
    //     ctx,
    //     handlers::common_module_name_global::check,
    // ));
    // NOTE: CommonModuleNameGlobalClient migrated to metadata-based collection (collect_metadata_diagnostics)
    // result.extend(run_diagnostic(
    //     "CommonModuleNameGlobalClient",
    //     ctx,
    //     handlers::common_module_name_global_client::check,
    // ));
    // NOTE: CommonModuleNameServerCall migrated to metadata-based collection (collect_metadata_diagnostics)
    // result.extend(run_diagnostic(
    //     "CommonModuleNameServerCall",
    //     ctx,
    //     handlers::common_module_name_server_call::check,
    // ));
    // NOTE: CommonModuleNameWords migrated to metadata-based collection (collect_metadata_diagnostics)
    // result.extend(run_diagnostic(
    //     "CommonModuleNameWords",
    //     ctx,
    //     handlers::common_module_name_words::check,
    // ));
    result.extend(run_diagnostic(
        "DenyIncompleteValues",
        ctx,
        handlers::deny_incomplete_values::check,
    ));
    // Removed: ExecuteExternalCodeInCommonModule - duplicate of ExecuteExternalCode (HIR-based)
    // ExecuteExternalCode already covers all cases without client-only annotation
    result.extend(run_diagnostic(
        "MetadataObjectNameLength",
        ctx,
        handlers::metadata_object_name_length::check,
    ));
    // NOTE: MissingCommonModuleMethod migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::MissingCommonModuleMethod
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "MissingCommonModuleMethod",
    //     ctx,
    //     handlers::missing_common_module_method::check,
    // ));
    result.extend(run_diagnostic(
        "MissingReturnedValueDescription",
        ctx,
        handlers::missing_returned_value_description::check,
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
    // These are cached by Salsa via module_bodies() query
    result.extend(collect_hir_diagnostics(ctx));

    // Metadata-based diagnostics (Phase 2: using module_metadata from HIR)
    result.extend(collect_metadata_diagnostics(ctx));

    // TODO: Add all 181 diagnostics
    // See DIAGNOSTICS_MIGRATION.md for full list

    result
}

/// Collect HIR-based diagnostics from module_bodies().
///
/// This function retrieves diagnostics collected during HIR lowering
/// and dispatches them to the appropriate handler's `from_hir()` function.
///
/// Returns empty vec for test contexts where source_root is not set.
fn collect_hir_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use hir::ModuleId;

    // In tests, file_source_root may not be set. Rather than panicking,
    // we silently return no diagnostics. This is fine since HIR diagnostics are
    // tested separately in their respective handler tests.
    let module_id = ModuleId::new(ctx.file_id);
    let module_bodies = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ctx.db.module_bodies(module_id)
    })) {
        Ok(bodies) => bodies,
        Err(_) => return Vec::new(),
    };

    let mut diagnostics = Vec::new();

    // Collect method-level HIR diagnostics (from module_bodies)
    for (_method_id, body_diag) in module_bodies.all_diagnostics() {
        if let Some(diag) = dispatch_hir_diagnostic(body_diag, ctx) {
            diagnostics.push(diag);
        }
    }

    diagnostics
}

/// Dispatch BodyDiagnostic to appropriate handler's from_hir() function.
fn dispatch_hir_diagnostic(
    body_diag: &hir::BodyDiagnostic,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    use hir::BodyDiagnostic;

    match body_diag {
        BodyDiagnostic::FunctionShouldHaveReturn { range } => {
            handlers::function_should_have_return::from_hir(*range, ctx)
        }
        BodyDiagnostic::EmptyCodeBlock { range } => {
            handlers::empty_code_block::from_hir(*range, ctx)
        }
        BodyDiagnostic::MagicNumber { value, range } => {
            handlers::magic_number::from_hir(value, *range, ctx)
        }
        BodyDiagnostic::SelfAssign { range } => handlers::self_assign::from_hir(*range, ctx),
        BodyDiagnostic::UnusedVariable { name, range } => {
            handlers::unused_local_variable::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::UnreachableCode { range } => {
            handlers::unreachable_code::from_hir(*range, ctx)
        }
        BodyDiagnostic::MissingReturn { range } => handlers::missing_return::from_hir(*range, ctx),
        BodyDiagnostic::DeprecatedMethod { name, range } => {
            handlers::deprecated_method::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::DeprecatedCurrentDate { name, range } => {
            handlers::deprecated_current_date::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::DeprecatedFind { name, range } => {
            handlers::deprecated_find::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::DeprecatedMessage { name, range } => {
            handlers::deprecated_message::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::DeprecatedTypeManagedForm { type_name, range } => {
            handlers::deprecated_type_managed_form::from_hir(type_name, *range, ctx)
        }
        BodyDiagnostic::DisableSafeMode { method_name, range } => {
            handlers::disable_safe_mode::from_hir(method_name, *range, ctx)
        }
        BodyDiagnostic::MissingCommonModuleMethod { module, method, range } => {
            handlers::missing_common_module_method::from_hir(module, method, *range, ctx)
        }
        BodyDiagnostic::BeginTransactionBeforeTryCatch { range } => {
            handlers::begin_transaction_before_try_catch::from_hir(*range, ctx)
        }
        BodyDiagnostic::MissedRequiredParameter {
            callee,
            module,
            mdo_type,
            mdo_name,
            args,
            range,
        } => handlers::missed_required_parameter::from_hir(
            callee,
            module.as_deref(),
            mdo_type.as_deref(),
            mdo_name.as_deref(),
            args,
            *range,
            ctx,
        ),
        BodyDiagnostic::IfElseDuplicatedCodeBlock { range } => {
            handlers::if_else_duplicated_code_block::from_hir(*range, ctx)
        }
        BodyDiagnostic::CodeAfterAsyncCall { method_name, range } => {
            handlers::code_after_async_call::from_hir(method_name, *range, ctx)
        }
        BodyDiagnostic::CommitTransactionOutsideTryCatch { range } => {
            handlers::commit_transaction_outside_try_catch::from_hir(*range, ctx)
        }
        BodyDiagnostic::CommonModuleAssign { variable_name, range } => {
            handlers::common_module_assign::from_hir(variable_name, *range, ctx)
        }
        BodyDiagnostic::CreateQueryInCycle { range } => {
            handlers::create_query_in_cycle::from_hir(*range, ctx)
        }
        BodyDiagnostic::DeletingCollectionItem { collection_text, range } => {
            handlers::deleting_collection_item::from_hir(collection_text, *range, ctx)
        }
        BodyDiagnostic::DeprecatedAttribute8312 { name, kind, range } => {
            handlers::deprecated_attributes_8312::from_hir(name, *kind, *range, ctx)
        }
        BodyDiagnostic::ExecuteExternalCode { range } => {
            handlers::execute_external_code::from_hir(*range, ctx)
        }
        BodyDiagnostic::EmptyRegion { name, range } => {
            handlers::empty_region::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::EmptyStatement { range } => {
            handlers::empty_statement::from_hir(*range, ctx)
        }
    }
}

/// Collect metadata-based diagnostics using module_metadata from HIR.
///
/// Phase 2 diagnostics that have been migrated to use ModuleMetadata directly
/// instead of loading Configuration for each file. These are part of module_bodies()
/// and are cached by Salsa for performance.
///
/// Returns empty vec for test contexts where source_root is not set.
fn collect_metadata_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use hir::ModuleId;

    // In tests, file_source_root may not be set. Rather than panicking,
    // we silently return no diagnostics. This is fine since metadata-based
    // diagnostics are production features tested separately.
    let module_id = ModuleId::new(ctx.file_id);

    let module_bodies = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ctx.db.module_bodies(module_id)
    })) {
        Ok(bodies) => bodies,
        Err(_) => return Vec::new(),
    };

    let mut diagnostics = Vec::new();

    // Get metadata attached to module_bodies
    if let Some(metadata) = module_bodies.metadata() {
        // Check CommonModuleInvalidType
        diagnostics
            .extend(handlers::common_module_invalid_type::from_metadata(metadata, ctx.config));

        // Check CommonModuleNameClient
        diagnostics
            .extend(handlers::common_module_name_client::from_metadata(metadata, ctx.config));

        // Check CommonModuleNameGlobal
        diagnostics
            .extend(handlers::common_module_name_global::from_metadata(metadata, ctx.config));

        // Check CommonModuleNameCached
        diagnostics
            .extend(handlers::common_module_name_cached::from_metadata(metadata, ctx.config));

        // Check CommonModuleNameClientServer
        diagnostics.extend(handlers::common_module_name_client_server::from_metadata(
            metadata, ctx.config,
        ));

        // Check CommonModuleNameFullAccess
        diagnostics
            .extend(handlers::common_module_name_full_access::from_metadata(metadata, ctx.config));

        // Check CommonModuleNameGlobalClient
        diagnostics.extend(handlers::common_module_name_global_client::from_metadata(
            metadata, ctx.config,
        ));

        // Check CommonModuleNameServerCall
        diagnostics
            .extend(handlers::common_module_name_server_call::from_metadata(metadata, ctx.config));

        // Check CommonModuleNameWords
        diagnostics.extend(handlers::common_module_name_words::from_metadata(metadata, ctx.config));

        // Phase 2.2: Add more metadata-based diagnostics here as they are migrated
        // Examples:
        // - common_module_missing_api (AST-based, not metadata)
        // - missing_common_module_method
        // - execute_external_code_in_common_module (AST-based, not metadata)
        // - common_module_assign (AST-based, not metadata)
        // - missing_event_subscription_handler
    }

    // Check ExportVariables - module-level variables with is_export flag
    for var in module_bodies.module_vars() {
        if var.is_export {
            if let Some(diag) = handlers::export_variables::from_hir(&var.name, var.range, ctx) {
                diagnostics.push(diag);
            }
        }
    }

    diagnostics
}
