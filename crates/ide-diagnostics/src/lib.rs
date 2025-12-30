//! Diagnostics for bsl-analyzer.
//!
//! This crate implements all 181 diagnostics from bsl-language-server.

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
    PairingBrokenTransaction,
    WrongUseOfRollbackTransactionMethod,

    // Tier 3: Metadata (requires 1C configuration metadata)
    CachedPublic,

    // SDBL Diagnostics
    AssignAliasFieldsInQuery,
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
            Self::AllFunctionPathMustHaveReturn => "AllFunctionPathMustHaveReturn",
            Self::AssignAliasFieldsInQuery => "AssignAliasFieldsInQuery",
            Self::BeginTransactionBeforeTryCatch => "BeginTransactionBeforeTryCatch",
            Self::CachedPublic => "CachedPublic",
            Self::CodeAfterAsyncCall => "CodeAfterAsyncCall",
            _ => "Unknown",
        }
    }
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
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

    // Tier 2: Semantic diagnostics
    result.extend(handlers::all_function_path_must_have_return::check(ctx));
    result.extend(handlers::begin_transaction_before_try_catch::check(ctx));
    result.extend(handlers::code_after_async_call::check(ctx));

    // Tier 3: Metadata diagnostics
    result.extend(handlers::cached_public::check(ctx));

    // SDBL diagnostics
    result.extend(handlers::assign_alias_fields_in_query::check(ctx));

    // TODO: Add all 181 diagnostics
    // See DIAGNOSTICS_MIGRATION.md for full list

    result
}
