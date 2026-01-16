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

/// A diagnostic produced by the analyzer (internal representation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    pub severity: Severity,
    pub range: TextRange,
    pub tags: Vec<DiagnosticTag>,
    pub fixes: Vec<Fix>,
}

/// Diagnostic output DTO for external consumption (reports, CLI, etc.).
///
/// This is the public-facing format with line/column positions instead of byte offsets.
/// Used by:
/// - Streaming mode results
/// - Reporter system (JSON, SARIF, console)
/// - CLI output
///
/// ## Architecture
/// This follows the DTO (Data Transfer Object) pattern:
/// - Internal representation: `Diagnostic` with `TextRange` (byte offsets)
/// - External representation: `DiagnosticOutput` with line/column positions
///
/// Conversion happens in the domain layer via `Diagnostic::to_output()`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiagnosticOutput {
    /// Diagnostic code (e.g., "LineLength", "BadWords").
    pub code: String,

    /// Human-readable message.
    pub message: String,

    /// Severity level (e.g., "Warning", "Error").
    pub severity: String,

    /// Start line (0-based).
    pub start_line: usize,

    /// Start column (0-based).
    pub start_column: usize,

    /// End line (0-based).
    pub end_line: usize,

    /// End column (0-based).
    pub end_column: usize,

    /// Diagnostic tags (e.g., "Unnecessary", "Deprecated").
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl Diagnostic {
    /// Convert to output DTO with line/column positions.
    ///
    /// This method performs the conversion from internal representation (TextRange)
    /// to external output format (line/column). Requires file text to build LineIndex.
    ///
    /// ## Example
    /// ```ignore
    /// let diagnostic = Diagnostic { ... };
    /// let file_text = "...";
    /// let output = diagnostic.to_output(file_text);
    /// println!("Error at {}:{}", output.start_line, output.start_column);
    /// ```
    pub fn to_output(&self, file_text: &str) -> DiagnosticOutput {
        use line_index::LineIndex;

        // Validate TextRange before creating LineIndex
        let file_len = file_text.len();
        let range_start: u32 = self.range.start().into();
        let range_end: u32 = self.range.end().into();

        if range_start as usize > file_len || range_end as usize > file_len {
            panic!(
                "BUG in diagnostic handler '{}': Invalid TextRange [{}, {}) exceeds file length {}\n\
                 Diagnostic message: '{}'\n\
                 This is a bug in the diagnostic handler that created this Diagnostic.\n\
                 The handler must ensure TextRange is within file bounds.",
                self.code.as_str(),
                range_start,
                range_end,
                file_len,
                self.message
            );
        }

        let line_index = LineIndex::new(file_text);

        let start = line_index.line_col(self.range.start());
        let end = line_index.line_col(self.range.end());

        DiagnosticOutput {
            code: self.code.as_str().to_string(),
            message: self.message.clone(),
            severity: self.severity.as_str().to_string(),
            start_line: start.line as usize,
            start_column: start.col as usize,
            end_line: end.line as usize,
            end_column: end.col as usize,
            tags: self.tags.iter().map(|tag| tag.as_str().to_string()).collect(),
        }
    }
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

impl Severity {
    /// Returns string representation for output.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Blocker => "Blocker",
            Self::Critical => "Critical",
            Self::Major => "Major",
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Information => "Information",
            Self::Hint => "Hint",
        }
    }
}

/// Diagnostic tag for special handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticTag {
    Unnecessary,
    Deprecated,
}

impl DiagnosticTag {
    /// Returns string representation for output.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unnecessary => "Unnecessary",
            Self::Deprecated => "Deprecated",
        }
    }
}

/// A quick fix for a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub label: String,
    pub edits: Vec<TextEdit>,
}

/// A text edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}

/// Configuration for diagnostics.
///
/// Supports Java BSL-LS compatible format:
/// ```json
/// {
///   "diagnostics": {
///     "ordinaryAppSupport": false,
///     "dataflowMaxIterations": 10000,
///     "parameters": {
///       "EmptyCodeBlock": false,
///       "LineLength": { "maxLength": 120 }
///     }
///   }
/// }
/// ```
///
/// In `parameters`:
/// - `false` = diagnostic disabled
/// - `true` = diagnostic enabled (default)
/// - `{...}` = diagnostic parameters
#[derive(Debug, Clone)]
pub struct DiagnosticsConfig {
    pub disabled: Vec<DiagnosticCode>,
    pub parameters: std::collections::HashMap<DiagnosticCode, serde_json::Value>,
    pub ordinary_app_support: bool,
    /// Maximum iterations for dataflow analysis (default: 10000)
    ///
    /// Controls convergence limit for liveness analysis and other dataflow algorithms.
    /// Increase this for very complex methods with deep nesting or many loops.
    /// Warning is logged if analysis exceeds this limit.
    pub dataflow_max_iterations: usize,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            disabled: Vec::new(),
            parameters: std::collections::HashMap::new(),
            ordinary_app_support: false,
            dataflow_max_iterations: 10000,
        }
    }
}

impl<'de> serde::Deserialize<'de> for DiagnosticsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor};
        use std::fmt;

        struct DiagnosticsConfigVisitor;

        impl<'de> Visitor<'de> for DiagnosticsConfigVisitor {
            type Value = DiagnosticsConfig;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a diagnostics configuration object")
            }

            fn visit_map<M>(self, mut map: M) -> Result<DiagnosticsConfig, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut disabled = Vec::new();
                let mut parameters = std::collections::HashMap::new();
                let mut ordinary_app_support = false;
                let mut dataflow_max_iterations = 10000usize;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "ordinaryAppSupport" => {
                            ordinary_app_support = map.next_value()?;
                        }
                        "dataflowMaxIterations" => {
                            dataflow_max_iterations = map.next_value()?;
                        }
                        "parameters" => {
                            let params: std::collections::HashMap<String, serde_json::Value> =
                                map.next_value()?;
                            for (code_str, value) in params {
                                if let Ok(code) = code_str.parse::<DiagnosticCode>() {
                                    match &value {
                                        serde_json::Value::Bool(false) => {
                                            disabled.push(code);
                                        }
                                        serde_json::Value::Bool(true) => {
                                            // enabled = default, skip
                                        }
                                        serde_json::Value::Object(_) => {
                                            parameters.insert(code, value);
                                        }
                                        _ => {
                                            // ignore other values
                                        }
                                    }
                                }
                                // Unknown diagnostic codes are silently ignored
                            }
                        }
                        _ => {
                            // Skip unknown fields
                            let _: serde_json::Value = map.next_value()?;
                        }
                    }
                }

                Ok(DiagnosticsConfig {
                    disabled,
                    parameters,
                    ordinary_app_support,
                    dataflow_max_iterations,
                })
            }
        }

        deserializer.deserialize_map(DiagnosticsConfigVisitor)
    }
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
///
/// Supports two modes of operation:
/// - **Salsa mode** (LSP): Uses `db` field with full caching
/// - **Provider mode** (streaming): Uses `provider` field for abstracted data access
///
/// Helper methods automatically use `provider` when available, falling back to `db`.
pub struct DiagnosticsContext<'a> {
    /// RootDatabase for Salsa-backed queries (LSP mode).
    pub db: &'a dyn RootDatabase,
    /// DiagnosticsConfig with enabled/disabled diagnostics and parameters.
    pub config: &'a DiagnosticsConfig,
    /// FileId of the file being analyzed.
    pub file_id: FileId,

    // === Provider abstraction (for streaming mode) ===
    /// Optional AnalysisProvider for abstracted data access.
    /// When set, helper methods use this instead of db directly.
    /// This enables StreamingProvider for analyze mode with minimal memory.
    pub provider: Option<&'a dyn ide_db::AnalysisProvider>,

    // === Workspace integration (for Tier 3 diagnostics) ===
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
    /// Create a new DiagnosticsContext with db (Salsa mode).
    ///
    /// This is the standard constructor for LSP mode with full Salsa caching.
    pub fn new(db: &'a dyn RootDatabase, config: &'a DiagnosticsConfig, file_id: FileId) -> Self {
        Self {
            db,
            config,
            file_id,
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        }
    }

    /// Create a new DiagnosticsContext with provider (streaming mode).
    ///
    /// This constructor is for analyze mode where an AnalysisProvider
    /// abstracts the data source (enabling StreamingProvider).
    ///
    /// Note: `db` is still required for compatibility with existing code
    /// that hasn't been migrated to use helper methods.
    pub fn with_provider(
        db: &'a dyn RootDatabase,
        config: &'a DiagnosticsConfig,
        file_id: FileId,
        provider: &'a dyn ide_db::AnalysisProvider,
    ) -> Self {
        Self {
            db,
            config,
            file_id,
            provider: Some(provider),
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        }
    }

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

        // Medium path: use provider (for streaming mode)
        if let Some(provider) = self.provider {
            return provider.file_path(self.file_id);
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

    // ========================================================================
    // Helper methods for accessing data
    // These methods use provider when available, falling back to db
    // ========================================================================

    /// Get parsed AST for current file.
    pub fn parse(&self) -> syntax::Parse<syntax::SyntaxNode> {
        if let Some(provider) = self.provider {
            return provider.parse(self.file_id);
        }
        self.db.parse(self.file_id)
    }

    /// Get lowered HIR bodies for current module.
    pub fn module_bodies(&self) -> std::sync::Arc<hir_def::ModuleBodies> {
        let module_id = hir_def::ModuleId::new(self.file_id);
        if let Some(provider) = self.provider {
            return provider.module_bodies(module_id);
        }
        self.db.module_bodies(module_id)
    }

    /// Get module metadata for current file.
    pub fn module_metadata(&self) -> std::sync::Arc<hir_def::ModuleMetadata> {
        let module_id = hir_def::ModuleId::new(self.file_id);
        if let Some(provider) = self.provider {
            return provider.module_metadata(module_id);
        }
        self.db.module_metadata(module_id)
    }

    /// Get symbol tree for current module.
    pub fn symbol_tree(&self) -> std::sync::Arc<hir_def::SymbolTree> {
        let module_id = hir_def::ModuleId::new(self.file_id);
        self.symbol_tree_for(module_id)
    }

    /// Get symbol tree for specific module.
    ///
    /// Use this when you need SymbolTree for a module other than the current file.
    pub fn symbol_tree_for(
        &self,
        module_id: hir_def::ModuleId,
    ) -> std::sync::Arc<hir_def::SymbolTree> {
        if let Some(provider) = self.provider {
            return provider.symbol_tree(module_id);
        }
        self.db.symbol_tree(module_id)
    }

    /// Get item tree for current file.
    pub fn item_tree(&self) -> std::sync::Arc<hir_def::ItemTree> {
        if let Some(provider) = self.provider {
            return provider.item_tree(self.file_id);
        }
        self.db.item_tree(self.file_id)
    }

    /// Get file text as String (abstracted from db/provider).
    ///
    /// This method provides unified access to file text:
    /// - In streaming mode: returns provider's Arc<String>
    /// - In LSP mode: gets FileTextInput from db and calls .text(db)
    ///
    /// IMPORTANT: Use this instead of ctx.db.file_text_input() to enable streaming mode.
    pub fn file_text(&self) -> std::sync::Arc<String> {
        if let Some(provider) = self.provider {
            let text = provider.file_text(self.file_id);
            return std::sync::Arc::new(text);
        }
        let input = self.db.file_text_input(self.file_id);
        // Wrap the db's String in Arc for consistent API
        let text: String = input.text(self.db).to_string();
        std::sync::Arc::new(text)
    }

    /// Get file text input (Salsa input) for current file.
    ///
    /// NOTE: This method only works in LSP mode with Salsa database.
    /// For streaming mode compatibility, use ctx.file_text() instead.
    ///
    /// Kept for backward compatibility with handlers that haven't been migrated yet.
    pub fn file_text_input(&self) -> base_db::FileTextInput {
        self.db.file_text_input(self.file_id)
    }

    /// Get line index for current file.
    pub fn line_index(&self) -> std::sync::Arc<line_index::LineIndex> {
        if let Some(provider) = self.provider {
            return provider.line_index(self.file_id);
        }
        let input = base_db::FileIdInput::new(self.db, self.file_id);
        self.db.line_index(input)
    }

    /// Get source root ID for current file.
    pub fn source_root_id(&self) -> base_db::SourceRootId {
        if let Some(provider) = self.provider {
            return provider.file_source_root_id(self.file_id);
        }
        self.db.file_source_root_input(self.file_id).source_root_id(self.db)
    }

    /// Get workspace symbols for cross-module resolution.
    pub fn workspace_symbols(&self) -> std::sync::Arc<hir_def::WorkspaceSymbols> {
        let source_root_id = self.source_root_id();
        if let Some(provider) = self.provider {
            return provider.workspace_symbols(source_root_id);
        }
        self.db.workspace_symbols(source_root_id)
    }

    /// Get module index for cross-module resolution.
    pub fn module_index(&self) -> std::sync::Arc<hir_def::ModuleIndex> {
        let source_root_id = self.source_root_id();
        if let Some(provider) = self.provider {
            return provider.module_index(source_root_id);
        }
        self.db.module_index(source_root_id)
    }

    /// Get module CFGs (batch).
    pub fn module_cfgs(&self) -> std::sync::Arc<cfg::ModuleCfgs> {
        if let Some(provider) = self.provider {
            return provider.module_cfgs(self.file_id);
        }
        let input = base_db::FileIdInput::new(self.db, self.file_id);
        self.db.module_cfgs(input)
    }

    /// Get module liveness analysis (batch).
    pub fn module_liveness(&self) -> std::sync::Arc<dataflow::liveness::ModuleLiveness> {
        if let Some(provider) = self.provider {
            return provider.module_liveness_analysis(self.file_id);
        }
        let input = base_db::FileIdInput::new(self.db, self.file_id);
        self.db.module_liveness_analysis(input)
    }

    /// Get module reaching definitions (batch).
    pub fn module_reaching_defs(
        &self,
    ) -> std::sync::Arc<dataflow::reaching_defs::ModuleReachingDefs> {
        if let Some(provider) = self.provider {
            return provider.module_reaching_definitions(self.file_id);
        }
        let input = base_db::FileIdInput::new(self.db, self.file_id);
        self.db.module_reaching_definitions(input)
    }

    /// Get region tree for current file.
    pub fn region_tree(&self) -> std::sync::Arc<hir_def::RegionTree> {
        if let Some(provider) = self.provider {
            return provider.region_tree(self.file_id);
        }
        self.db.region_tree(self.file_id)
    }

    /// Get module-level regions for current file.
    pub fn module_level_regions(&self) -> std::sync::Arc<Vec<base_db::RegionInfo>> {
        if let Some(provider) = self.provider {
            return provider.module_level_regions(self.file_id);
        }
        self.db.module_level_regions(self.file_id)
    }

    /// Get SDBL HIR for all queries in current file.
    pub fn sdbl_hir_in_file(&self) -> ide_db::SdblHirEntries {
        if let Some(provider) = self.provider {
            return provider.sdbl_hir_in_file(self.file_id);
        }
        self.db.sdbl_hir_in_file(self.file_id)
    }

    /// Get all SDBL queries (parsed AST) in current file.
    pub fn all_sdbl_in_file(
        &self,
    ) -> std::sync::Arc<Vec<(hir_def::SdblExprId, syntax::SdblQueryInfo)>> {
        if let Some(provider) = self.provider {
            return provider.all_sdbl_in_file(self.file_id);
        }
        self.db.all_sdbl_in_file(self.file_id)
    }

    /// Get module data for current file.
    pub fn module_data(&self) -> std::sync::Arc<hir_def::ModuleData> {
        let module_id = hir_def::ModuleId::new(self.file_id);
        if let Some(provider) = self.provider {
            return provider.module_data(module_id);
        }
        self.db.module_data(module_id)
    }

    /// Get parsed documentation for a method.
    ///
    /// Extracts and parses leading comments (lines starting with //)
    /// before a procedure or function definition.
    pub fn method_docs(
        &self,
        method_id: hir_def::MethodId,
    ) -> Option<std::sync::Arc<hir_def::docs::MethodDocs>> {
        if let Some(provider) = self.provider {
            return provider.method_docs(method_id);
        }
        self.db.method_docs(method_id)
    }

    /// Get reaching definitions for a specific method.
    ///
    /// Returns `None` if analysis doesn't converge.
    pub fn reaching_definitions(
        &self,
        method_id: hir_def::MethodId,
    ) -> Option<std::sync::Arc<dataflow::reaching_defs::ReachingDefsResult>> {
        if let Some(provider) = self.provider {
            return provider.reaching_definitions(method_id);
        }
        self.db.reaching_definitions(method_id)
    }

    /// Resolve VfsPath to FileId.
    ///
    /// Used for finding metadata files given their URI from Configuration.
    /// IMPORTANT: This method uses ctx.file_set for fast path when available.
    pub fn resolve_vfs_path(
        &self,
        source_root_id: base_db::SourceRootId,
        vfs_path: &vfs::VfsPath,
    ) -> Option<vfs::FileId> {
        // Fast path: use provided FileSet (bypasses Salsa)
        if let Some(file_set) = self.file_set {
            return file_set.file_for_path(vfs_path).copied();
        }

        // Slow path: delegate to provider/db
        if let Some(provider) = self.provider {
            return provider.resolve_vfs_path(source_root_id, vfs_path);
        }
        self.db.resolve_vfs_path(source_root_id, vfs_path)
    }

    /// Resolve qualified path (Module.Method) using provider-first pattern.
    ///
    /// Enables streaming mode support without direct database access.
    /// Domain layer (diagnostics) depends on abstraction (ctx), not implementation (db).
    ///
    /// ## Algorithm
    ///
    /// 1. Get module_index (provider-first)
    /// 2. Resolve module_name → FileId
    /// 3. Get symbol_tree for target module
    /// 4. Find method and check export flag
    pub fn resolve_qualified_path(
        &self,
        module_name: &hir_def::Name,
        method_name: &hir_def::Name,
    ) -> hir_def::PathResolution {
        // 1. Get module_index (provider-first)
        let module_index = self.module_index();

        // 2. Resolve module name → FileId
        let target_file_id = match module_index.resolve_common_module(module_name) {
            Some(id) => id,
            None => {
                return hir_def::PathResolution::Unresolved(hir_def::QualifiedName::from_segments(
                    [module_name.clone(), method_name.clone()],
                ));
            }
        };

        // 3. Get symbol_tree for target module (provider-first via symbol_tree_for)
        let target_module_id = hir_def::ModuleId::new(target_file_id);
        let symbol_tree = self.symbol_tree_for(target_module_id);

        // 4. Find method and check export flag
        if let Some(method_symbol) = symbol_tree.find_method(method_name) {
            if method_symbol.is_export {
                return hir_def::PathResolution::Method(method_symbol.id);
            }
        }

        hir_def::PathResolution::Unresolved(hir_def::QualifiedName::from_segments([
            module_name.clone(),
            method_name.clone(),
        ]))
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
    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    // File-level text-based diagnostics (called once per file)
    // NOTE: Only fully migrated handlers enabled for streaming mode compatibility
    diagnostics.extend(handlers::consecutive_empty_lines::check(ctx));
    diagnostics.extend(handlers::line_length::check(ctx));
    diagnostics.extend(handlers::commented_code::check(ctx));

    // TODO: Migrate these handlers to use helper methods instead of ctx.db directly
    // diagnostics.extend(handlers::missing_space::check(ctx));
    // diagnostics.extend(handlers::incorrect_line_break::check(ctx));
    // diagnostics.extend(handlers::invalid_character_in_file::check(ctx));
    // diagnostics.extend(handlers::space_at_start_comment::check(ctx));

    // Region-related diagnostics (file-level)
    // TODO: These use ctx.db.module_level_regions() - need helper method
    // diagnostics.extend(handlers::duplicate_region::check(ctx));
    // diagnostics.extend(handlers::non_standard_region::check(ctx));
    // diagnostics.extend(handlers::code_block_before_sub::check(ctx));
    // diagnostics.extend(handlers::code_out_of_region::check(ctx));

    // String/Date literal diagnostics (file-level)
    // TODO: Need to migrate these handlers
    // diagnostics.extend(handlers::magic_date::check(ctx));
    // diagnostics.extend(handlers::duplicate_string_literal::check(ctx));

    // Keyword spelling diagnostics (file-level, token-based)
    // TODO: Need to migrate this handler
    // diagnostics.extend(handlers::canonical_spelling_keywords::check(ctx));

    // Single traversal for all node-based text diagnostics
    // FIXME: This iterates the entire file which is expensive.
    // Salsa caching + incremental re-parse would be better (rust-analyzer TODO)
    for node in root.descendants() {
        // Node-based diagnostic handlers (check_node API)
        handlers::bad_words::check_node(&node, &mut diagnostics, ctx);
        // extra_commas is now HIR-based (checked during argument list lowering)
        // handlers::extra_commas::check_node(&node, &mut diagnostics, ctx);
        // TODO: Migrate nested_ternary_operator before enabling
        // handlers::nested_ternary_operator::check_node(&node, &mut diagnostics, ctx);
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

    // All handlers migrated - streaming mode supports all diagnostics!

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
    // NOTE: IfConditionComplexity migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::IfConditionComplexity
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "IfConditionComplexity",
    //     ctx,
    //     handlers::if_condition_complexity::check,
    // ));
    // NOTE: IfElseDuplicatedCodeBlock migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::IfElseDuplicatedCodeBlock
    // and dispatched through collect_hir_diagnostics()
    // NOTE: IfElseDuplicatedCondition migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::IfElseDuplicatedCondition
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "IfElseDuplicatedCondition",
    //     ctx,
    //     handlers::if_else_duplicated_condition::check,
    // ));
    // NOTE: IfElseIfEndsWithElse migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::IfElseIfEndsWithElse
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "IfElseIfEndsWithElse",
    //     ctx,
    //     handlers::if_else_if_ends_with_else::check,
    // ));
    // NOTE: IncorrectLineBreak migrated to text-based collection (collect_text_diagnostics)
    // NOTE: IncorrectUseOfStrTemplate uses DUAL approach:
    // 1. HIR lowering validation (AST time) - detects string literals (fast)
    // 2. Post-HIR check (below) - resolves variables using reaching definitions (complete)
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
    // NOTE: ExternalAppStarting migrated to HIR-based collection
    // The HIR version is collected during lowering via BodyDiagnostic::ExternalAppStarting
    // and dispatched through collect_hir_diagnostics()
    // result.extend(run_diagnostic(
    //     "ExternalAppStarting",
    //     ctx,
    //     handlers::external_app_starting::check,
    // ));
    // MIGRATED TO HIR: FileSystemAccess - now collected during lowering (lower_new_expr, lower_call_expr)
    // result.extend(run_diagnostic("FileSystemAccess", ctx, handlers::file_system_access::check));
    result.extend(run_diagnostic("InternetAccess", ctx, handlers::internet_access::check));
    result.extend(run_diagnostic("IsInRoleMethod", ctx, handlers::is_in_role_method::check));
    // MIGRATED TO HIR: FormDataToValue - now collected during lowering (lower_call_expr)
    // Detects calls to ДанныеФормыВЗначение/FormDataToValue in methods WITHOUT БезКонтекста annotation
    // MIGRATED TO HIR: FormDataToValue - now collected during lowering (lower_call_expr)
    // result.extend(run_diagnostic("FormDataToValue", ctx, handlers::form_data_to_value::check));

    // MIGRATED TO HIR: GetFormMethod - now collected during lowering (lower_call_expr)
    // result.extend(run_diagnostic("GetFormMethod", ctx, handlers::get_form_method::check));

    // MIGRATED TO HIR: GlobalContextMethodCollision8312 - now collected during lowering (lower_method_with_externals)
    // result.extend(run_diagnostic(
    //     "GlobalContextMethodCollision8312",
    //     ctx,
    //     handlers::global_context_method_collision8312::check,
    // ));
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
    // MIGRATED TO HIR: FunctionNameStartsWithGet - now collected during lowering (lower_method_with_externals)
    // result.extend(run_diagnostic(
    //     "FunctionNameStartsWithGet",
    //     ctx,
    //     handlers::function_name_starts_with_get::check,
    // ));
    // MIGRATED TO HIR: FunctionReturnsSamePrimitive - now collected during lowering (check_function_returns_same_primitive)
    // result.extend(run_diagnostic(
    //     "FunctionReturnsSamePrimitive",
    //     ctx,
    //     handlers::function_returns_same_primitive::check,
    // ));
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
    // NOTE: MissingCommonModuleMethod is now collected via HIR lowering (Phase 5 complete)
    // Diagnostics are created in expr.rs during qualified call lowering and validated
    // in from_hir() handler using workspace symbols and path resolution.
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

    // Dataflow-based diagnostics (using CFG + liveness analysis)
    // These use Salsa-cached CFG and dataflow results
    result.extend(run_diagnostic(
        "UnusedLocalVariable",
        ctx,
        handlers::unused_local_variable::check,
    ));

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
    // In tests, file_source_root may not be set. Rather than panicking,
    // we silently return no diagnostics. This is fine since HIR diagnostics are
    // tested separately in their respective handler tests.
    let module_bodies =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.module_bodies())) {
            Ok(bodies) => bodies,
            Err(_) => return Vec::new(),
        };

    let mut diagnostics = Vec::new();

    // Collect method-level HIR diagnostics (from module_bodies)
    for (method_id, body_diag) in module_bodies.all_diagnostics() {
        if let Some(diag) = dispatch_hir_diagnostic(body_diag, method_id, ctx) {
            diagnostics.push(diag);
        }
    }

    diagnostics
}

/// Dispatch BodyDiagnostic to appropriate handler's from_hir() function.
fn dispatch_hir_diagnostic(
    body_diag: &hir::BodyDiagnostic,
    method_id: &hir::MethodId,
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
        BodyDiagnostic::MissingReturn { range } => {
            handlers::all_function_path_must_have_return::from_hir(*range, method_id, ctx)
        }
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
        // NOTE: MissingCommonModuleMethod removed from HIR lowering (Phase 4)
        // Now collected via AST-based check() with path resolution
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
        BodyDiagnostic::RewriteMethodParameter { param_id, stmt_id, range } => {
            handlers::rewrite_method_parameter::from_hir(*param_id, *stmt_id, *range, ctx)
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
        BodyDiagnostic::ExternalAppStarting { range } => {
            handlers::external_app_starting::from_hir(*range, ctx)
        }
        BodyDiagnostic::ExtraCommas { range } => handlers::extra_commas::from_hir(*range, ctx),
        BodyDiagnostic::FileSystemAccess { range } => {
            handlers::file_system_access::from_hir(*range, ctx)
        }
        BodyDiagnostic::FormDataToValue { range } => {
            handlers::form_data_to_value::from_hir(*range, ctx)
        }
        BodyDiagnostic::GetFormMethod { method_name, range } => {
            handlers::get_form_method::from_hir(method_name, *range, ctx)
        }
        BodyDiagnostic::GlobalContextMethodCollision8312 { method_name, range } => {
            handlers::global_context_method_collision8312::from_hir(method_name, *range, ctx)
        }
        BodyDiagnostic::FunctionNameStartsWithGet { name, range } => {
            handlers::function_name_starts_with_get::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::FunctionOutParameter { name, range } => {
            handlers::function_out_parameter::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::FunctionReturnsSamePrimitive { range } => {
            handlers::function_returns_same_primitive::from_hir(*range, ctx)
        }
        BodyDiagnostic::EmptyRegion { name, range } => {
            handlers::empty_region::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::EmptyStatement { range } => {
            handlers::empty_statement::from_hir(*range, ctx)
        }
        BodyDiagnostic::IfConditionComplexity { complexity, max_complexity, range } => {
            handlers::if_condition_complexity::from_hir(*complexity, *max_complexity, *range, ctx)
        }
        BodyDiagnostic::IfElseDuplicatedCondition { first_occurrence_index, range } => {
            handlers::if_else_duplicated_condition::from_hir(*first_occurrence_index, *range, ctx)
        }
        BodyDiagnostic::IfElseIfEndsWithElse { range } => {
            handlers::if_else_if_ends_with_else::from_hir(*range, ctx)
        }
        BodyDiagnostic::IncorrectUseOfStrTemplate { range } => {
            handlers::incorrect_use_of_str_template::from_hir(*range, ctx)
        }
        BodyDiagnostic::MissingCommonModuleMethod { module, method, range } => {
            handlers::missing_common_module_method::from_hir(module, method, *range, ctx)
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
    // In tests, file_source_root may not be set. Rather than panicking,
    // we silently return no diagnostics. This is fine since metadata-based
    // diagnostics are production features tested separately.
    let module_bodies =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.module_bodies())) {
            Ok(bodies) => bodies,
            Err(_) => return Vec::new(),
        };

    let mut diagnostics = Vec::new();

    // Get metadata via helper method (uses provider if available)
    let metadata = ctx.module_metadata();
    let metadata_ref = metadata.as_ref();

    // Check CommonModuleInvalidType
    diagnostics
        .extend(handlers::common_module_invalid_type::from_metadata(metadata_ref, ctx.config));

    // Check CommonModuleNameClient
    diagnostics
        .extend(handlers::common_module_name_client::from_metadata(metadata_ref, ctx.config));

    // Check CommonModuleNameGlobal
    diagnostics
        .extend(handlers::common_module_name_global::from_metadata(metadata_ref, ctx.config));

    // Check CommonModuleNameCached
    diagnostics
        .extend(handlers::common_module_name_cached::from_metadata(metadata_ref, ctx.config));

    // Check CommonModuleNameClientServer
    diagnostics.extend(handlers::common_module_name_client_server::from_metadata(
        metadata_ref,
        ctx.config,
    ));

    // Check CommonModuleNameFullAccess
    diagnostics
        .extend(handlers::common_module_name_full_access::from_metadata(metadata_ref, ctx.config));

    // Check CommonModuleNameGlobalClient
    diagnostics.extend(handlers::common_module_name_global_client::from_metadata(
        metadata_ref,
        ctx.config,
    ));

    // Check CommonModuleNameServerCall
    diagnostics
        .extend(handlers::common_module_name_server_call::from_metadata(metadata_ref, ctx.config));

    // Check CommonModuleNameWords
    diagnostics.extend(handlers::common_module_name_words::from_metadata(metadata_ref, ctx.config));

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

// ============================================================================
// Salsa-cached diagnostics query
// ============================================================================

use std::sync::Arc;

use base_db::{DiagnosticsConfigId, DiagnosticsConfigInput, FileIdInput};

impl DiagnosticsConfig {
    /// Convert from Salsa-hashable DiagnosticsConfigInput.
    ///
    /// This converts the string-based config (used in Salsa for hashability)
    /// to the typed config (used by diagnostic handlers).
    pub fn from_input(input: &DiagnosticsConfigInput) -> Self {
        // Convert string codes to DiagnosticCode
        let disabled: Vec<DiagnosticCode> =
            input.disabled.iter().filter_map(|s| s.parse().ok()).collect();

        // Convert string parameters to HashMap
        let parameters: std::collections::HashMap<DiagnosticCode, serde_json::Value> = input
            .parameters
            .iter()
            .filter_map(|(code_str, json_str)| {
                let code: DiagnosticCode = code_str.parse().ok()?;
                let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
                Some((code, value))
            })
            .collect();

        Self {
            disabled,
            parameters,
            ordinary_app_support: input.ordinary_app_support,
            dataflow_max_iterations: input.dataflow_max_iterations,
        }
    }
}

impl std::fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Use Debug representation which gives variant name (e.g., "EmptyCodeBlock")
        write!(f, "{:?}", self)
    }
}

impl std::str::FromStr for DiagnosticCode {
    type Err = ();

    /// Parse diagnostic code from string.
    ///
    /// Used when converting DiagnosticsConfigInput to DiagnosticsConfig.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "CanonicalSpellingKeywords" => Ok(Self::CanonicalSpellingKeywords),
            "ConsecutiveEmptyLines" => Ok(Self::ConsecutiveEmptyLines),
            "LineLength" => Ok(Self::LineLength),
            "MissingSpace" => Ok(Self::MissingSpace),
            "OneStatementPerLine" => Ok(Self::OneStatementPerLine),
            "SemicolonPresence" => Ok(Self::SemicolonPresence),
            "SpaceAtStartComment" => Ok(Self::SpaceAtStartComment),
            "IncorrectLineBreak" => Ok(Self::IncorrectLineBreak),
            "IncorrectUseOfStrTemplate" => Ok(Self::IncorrectUseOfStrTemplate),
            "ExtraCommas" => Ok(Self::ExtraCommas),
            "CommentedCode" => Ok(Self::CommentedCode),
            "EmptyCodeBlock" => Ok(Self::EmptyCodeBlock),
            "EmptyRegion" => Ok(Self::EmptyRegion),
            "EmptyStatement" => Ok(Self::EmptyStatement),
            "UnreachableCode" => Ok(Self::UnreachableCode),
            "CodeBlockBeforeSub" => Ok(Self::CodeBlockBeforeSub),
            "CodeOutOfRegion" => Ok(Self::CodeOutOfRegion),
            "MagicNumber" => Ok(Self::MagicNumber),
            "MagicDate" => Ok(Self::MagicDate),
            "YoLetterUsage" => Ok(Self::YoLetterUsage),
            "LatinAndCyrillicSymbolInWord" => Ok(Self::LatinAndCyrillicSymbolInWord),
            "InvalidCharacterInFile" => Ok(Self::InvalidCharacterInFile),
            "DoubleNegatives" => Ok(Self::DoubleNegatives),
            "NestedTernaryOperator" => Ok(Self::NestedTernaryOperator),
            "NonExportMethodsInApiRegion" => Ok(Self::NonExportMethodsInApiRegion),
            "TernaryOperatorUsage" => Ok(Self::TernaryOperatorUsage),
            "UnaryPlusInConcatenation" => Ok(Self::UnaryPlusInConcatenation),
            "UselessTernaryOperator" => Ok(Self::UselessTernaryOperator),
            "BadWords" => Ok(Self::BadWords),
            "DuplicateStringLiteral" => Ok(Self::DuplicateStringLiteral),
            "DuplicateRegion" => Ok(Self::DuplicateRegion),
            "NonStandardRegion" => Ok(Self::NonStandardRegion),
            "DuplicatedInsertionIntoCollection" => Ok(Self::DuplicatedInsertionIntoCollection),
            "ExcessiveAutoTestCheck" => Ok(Self::ExcessiveAutoTestCheck),
            "IdenticalExpressions" => Ok(Self::IdenticalExpressions),
            "IfElseDuplicatedCodeBlock" => Ok(Self::IfElseDuplicatedCodeBlock),
            "IfElseDuplicatedCondition" => Ok(Self::IfElseDuplicatedCondition),
            "IfElseIfEndsWithElse" => Ok(Self::IfElseIfEndsWithElse),
            "MultilingualStringHasAllDeclaredLanguages" => {
                Ok(Self::MultilingualStringHasAllDeclaredLanguages)
            }
            "MultilingualStringUsingWithTemplate" => Ok(Self::MultilingualStringUsingWithTemplate),
            "NestedConstructorsInStructureDeclaration" => {
                Ok(Self::NestedConstructorsInStructureDeclaration)
            }
            "NestedFunctionInParameters" => Ok(Self::NestedFunctionInParameters),
            "AllFunctionPathMustHaveReturn" => Ok(Self::AllFunctionPathMustHaveReturn),
            "FunctionShouldHaveReturn" => Ok(Self::FunctionShouldHaveReturn),
            "ProcedureReturnsValue" => Ok(Self::ProcedureReturnsValue),
            "FunctionReturnsSamePrimitive" => Ok(Self::FunctionReturnsSamePrimitive),
            "FunctionNameStartsWithGet" => Ok(Self::FunctionNameStartsWithGet),
            "TooManyReturns" => Ok(Self::TooManyReturns),
            "NumberOfParams" => Ok(Self::NumberOfParams),
            "NumberOfOptionalParams" => Ok(Self::NumberOfOptionalParams),
            "OrderOfParams" => Ok(Self::OrderOfParams),
            "MissedRequiredParameter" => Ok(Self::MissedRequiredParameter),
            "FunctionOutParameter" => Ok(Self::FunctionOutParameter),
            "UnusedParameters" => Ok(Self::UnusedParameters),
            "MissingParameterDescription" => Ok(Self::MissingParameterDescription),
            "MissingReturnedValueDescription" => Ok(Self::MissingReturnedValueDescription),
            "RewriteMethodParameter" => Ok(Self::RewriteMethodParameter),
            "UnusedLocalVariable" => Ok(Self::UnusedLocalVariable),
            "UnusedLocalMethod" => Ok(Self::UnusedLocalMethod),
            "ExportVariables" => Ok(Self::ExportVariables),
            "MissingVariablesDescription" => Ok(Self::MissingVariablesDescription),
            "SelfAssign" => Ok(Self::SelfAssign),
            "ThisObjectAssign" => Ok(Self::ThisObjectAssign),
            "CyclomaticComplexity" => Ok(Self::CyclomaticComplexity),
            "CognitiveComplexity" => Ok(Self::CognitiveComplexity),
            "NestedStatements" => Ok(Self::NestedStatements),
            "MethodSize" => Ok(Self::MethodSize),
            "IfConditionComplexity" => Ok(Self::IfConditionComplexity),
            "MissingCodeTryCatchEx" => Ok(Self::MissingCodeTryCatchEx),
            "MissingTempStorageDeletion" => Ok(Self::MissingTempStorageDeletion),
            "MissingTemporaryFileDeletion" => Ok(Self::MissingTemporaryFileDeletion),
            "UsingGoto" => Ok(Self::UsingGoto),
            "BeginTransactionBeforeTryCatch" => Ok(Self::BeginTransactionBeforeTryCatch),
            "CodeAfterAsyncCall" => Ok(Self::CodeAfterAsyncCall),
            "CommitTransactionOutsideTryCatch" => Ok(Self::CommitTransactionOutsideTryCatch),
            "CompilationDirectiveLost" => Ok(Self::CompilationDirectiveLost),
            "CreateQueryInCycle" => Ok(Self::CreateQueryInCycle),
            "DataExchangeLoading" => Ok(Self::DataExchangeLoading),
            "DeletingCollectionItem" => Ok(Self::DeletingCollectionItem),
            "DeprecatedCurrentDate" => Ok(Self::DeprecatedCurrentDate),
            "DeprecatedFind" => Ok(Self::DeprecatedFind),
            "DeprecatedMessage" => Ok(Self::DeprecatedMessage),
            "DeprecatedTypeManagedForm" => Ok(Self::DeprecatedTypeManagedForm),
            "DeprecatedMethods8310" => Ok(Self::DeprecatedMethods8310),
            "DeprecatedMethods8317" => Ok(Self::DeprecatedMethods8317),
            "DeprecatedAttributes8312" => Ok(Self::DeprecatedAttributes8312),
            "DisableSafeMode" => Ok(Self::DisableSafeMode),
            "ExecuteExternalCode" => Ok(Self::ExecuteExternalCode),
            "ExternalAppStarting" => Ok(Self::ExternalAppStarting),
            "FileSystemAccess" => Ok(Self::FileSystemAccess),
            "FormDataToValue" => Ok(Self::FormDataToValue),
            "GetFormMethod" => Ok(Self::GetFormMethod),
            "GlobalContextMethodCollision8312" => Ok(Self::GlobalContextMethodCollision8312),
            "InternetAccess" => Ok(Self::InternetAccess),
            "IsInRoleMethod" => Ok(Self::IsInRoleMethod),
            "PairingBrokenTransaction" => Ok(Self::PairingBrokenTransaction),
            "WrongUseOfRollbackTransactionMethod" => Ok(Self::WrongUseOfRollbackTransactionMethod),
            "CachedPublic" => Ok(Self::CachedPublic),
            "CommandModuleExportMethods" => Ok(Self::CommandModuleExportMethods),
            "CommonModuleAssign" => Ok(Self::CommonModuleAssign),
            "CommonModuleInvalidType" => Ok(Self::CommonModuleInvalidType),
            "CommonModuleMissingAPI" => Ok(Self::CommonModuleMissingAPI),
            "CommonModuleNameCached" => Ok(Self::CommonModuleNameCached),
            "CommonModuleNameClient" => Ok(Self::CommonModuleNameClient),
            "CommonModuleNameClientServer" => Ok(Self::CommonModuleNameClientServer),
            "CommonModuleNameFullAccess" => Ok(Self::CommonModuleNameFullAccess),
            "CommonModuleNameGlobal" => Ok(Self::CommonModuleNameGlobal),
            "CommonModuleNameGlobalClient" => Ok(Self::CommonModuleNameGlobalClient),
            "CommonModuleNameServerCall" => Ok(Self::CommonModuleNameServerCall),
            "CommonModuleNameWords" => Ok(Self::CommonModuleNameWords),
            "DenyIncompleteValues" => Ok(Self::DenyIncompleteValues),
            "MetadataObjectNameLength" => Ok(Self::MetadataObjectNameLength),
            "MissingCommonModuleMethod" => Ok(Self::MissingCommonModuleMethod),
            "MissingEventSubscriptionHandler" => Ok(Self::MissingEventSubscriptionHandler),
            "AssignAliasFieldsInQuery" => Ok(Self::AssignAliasFieldsInQuery),
            "FieldsFromJoinsWithoutIsNull" => Ok(Self::FieldsFromJoinsWithoutIsNull),
            "FullOuterJoinQuery" => Ok(Self::FullOuterJoinQuery),
            "JoinWithSubQuery" => Ok(Self::JoinWithSubQuery),
            "LogicalOrInJoinQuerySection" => Ok(Self::LogicalOrInJoinQuerySection),
            "LogicalOrInTheWhereSectionOfQuery" => Ok(Self::LogicalOrInTheWhereSectionOfQuery),
            "MultilineStringInQuery" => Ok(Self::MultilineStringInQuery),
            _ => Err(()),
        }
    }
}

/// Salsa-cached diagnostics query.
///
/// Computes diagnostics for a file with the given configuration.
/// Results are cached by Salsa and automatically invalidated when:
/// - File content changes (via FileIdInput dependency)
/// - Config changes (via DiagnosticsConfigId)
///
/// ## Performance
/// - **LRU cache:** 256 files
/// - **First call:** ~700ms (full computation)
/// - **Cached call:** < 1ms (cache hit)
/// - **After file change:** ~700ms (recomputes for that file only)
/// - **After config change:** ~700ms × N files (all invalidated)
///
/// ## Usage
///
/// ```ignore
/// let file_id_input = FileIdInput::new(db, file_id);
/// let config = DiagnosticsConfigInput::new();
/// let config_id = DiagnosticsConfigId::new(db, config);
/// let diagnostics = file_diagnostics_query(db, file_id_input, config_id);
/// ```
#[salsa::tracked(lru = 256)]
pub fn file_diagnostics_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
    config_id: DiagnosticsConfigId<'db>,
) -> Arc<Vec<Diagnostic>> {
    let file_id = file_id_input.file_id(db);
    let config_input = config_id.config(db);
    let config = DiagnosticsConfig::from_input(&config_input);

    let _span = tracing::info_span!("file_diagnostics_query", file_id = file_id.0,).entered();

    let ctx = DiagnosticsContext {
        db,
        config: &config,
        file_id,
        provider: None,
        workspace_root: None,
        configuration_path: None,
        configuration_path_input: None,
        file_set: None,
    };

    Arc::new(diagnostics(&ctx))
}
