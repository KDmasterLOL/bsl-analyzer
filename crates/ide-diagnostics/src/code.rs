//! Diagnostic codes matching bsl-language-server.

use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// Diagnostic code - matches bsl-language-server codes.
///
/// Uses strum for automatic as_str/from_str generation and iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, Display, IntoStaticStr, EnumIter)]
pub enum DiagnosticCode {
    // Tier 1: Simple (syntax-only)
    ParseError,
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
    Typo,

    // Tier 2: Medium (requires symbol table)
    AllFunctionPathMustHaveReturn,
    FunctionShouldHaveReturn,
    ProcedureReturnsValue,
    FunctionReturnsSamePrimitive,
    FunctionNameStartsWithGet,
    TooManyReturns,
    NumberOfParams,
    NumberOfOptionalParams,
    NumberOfValuesInStructureConstructor,
    OrderOfParams,
    MissedRequiredParameter,
    FunctionOutParameter,
    UnusedParameters,
    MissingParameterDescription,
    MissingReturnedValueDescription,
    ReservedParameterNames,
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
    UseLessForEach,
    UsingGoto,
    BeginTransactionBeforeTryCatch,
    CodeAfterAsyncCall,
    CommitTransactionOutsideTryCatch,
    CompilationDirectiveLost,
    CompilationDirectiveNeedLess,
    CreateQueryInCycle,
    DataExchangeLoading,
    DeletingCollectionItem,
    SelfInsertion,
    SeveralCompilerDirectives,
    StyleElementConstructors,
    DeprecatedCurrentDate,
    DeprecatedFind,
    DeprecatedMessage,
    DeprecatedTypeManagedForm,
    DeprecatedMethods8310,
    DeprecatedMethods8317,
    DeprecatedAttributes8312,
    DeprecatedMethodCall,
    DisableSafeMode,
    ExecuteExternalCode,
    ExecuteExternalCodeInCommonModule,
    ExternalAppStarting,
    FileSystemAccess,
    OSUsersMethod,
    TempFilesDir,
    FormDataToValue,
    GetFormMethod,
    GlobalContextMethodCollision8312,
    InternetAccess,
    IsInRoleMethod,
    PairingBrokenTransaction,
    WrongUseOfRollbackTransactionMethod,
    TimeoutsInExternalResources,
    TryNumber,
    UnknownPreprocessorSymbol,
    UnsafeSafeModeMethodCall,
    UsageWriteLogEvent,
    UseSystemInformation,
    UsingCancelParameter,
    UsingExternalCodeTools,
    UsingFindElementByString,
    UsingHardcodeNetworkAddress,
    UsingHardcodePath,
    UsingHardcodeSecretInformation,
    UsingModalWindows,
    UsingObjectNotAvailableUnix,
    UsingServiceTag,
    UsingThisForm,
    WrongUseFunctionProceedWithCall,

    // Tier 3: Metadata (requires 1C configuration metadata)
    WrongHttpServiceHandler,
    WrongWebServiceHandler,
    WrongDataPathForFormElements,
    PublicMethodsDescription,
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
    MetadataObjectNameLength,
    MissingCommonModuleMethod,
    MissingEventSubscriptionHandler,
    OrdinaryAppSupport,
    PrivilegedModuleMethodCall,
    ProtectedModule,
    RedundantAccessToObject,
    SameMetadataObjectAndChildNames,
    ScheduledJobHandler,
    ServerCallsInFormEvents,
    ServerSideExportFormMethod,
    SetPermissionsForNewObjects,
    SetPrivilegedMode,
    TransferringParametersBetweenClientAndServer,
    UnsafeFindByCode,

    // SDBL Diagnostics
    AssignAliasFieldsInQuery,
    FieldsFromJoinsWithoutIsNull,
    FullOuterJoinQuery,
    JoinWithSubQuery,
    LogicalOrInJoinQuerySection,
    LogicalOrInTheWhereSectionOfQuery,
    MultilineStringInQuery,
    QueryNestedFieldsByDot,
    QueryParseError,
    QueryToMissingMetadata,
    RefOveruse,
    UnionAll,
    UsingLikeInQuery,
    VirtualTableCallWithoutParameters,
}

impl DiagnosticCode {
    /// Returns the string representation (for LSP and SonarQube).
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_as_str() {
        assert_eq!(DiagnosticCode::LineLength.as_str(), "LineLength");
        assert_eq!(DiagnosticCode::EmptyCodeBlock.as_str(), "EmptyCodeBlock");
    }

    #[test]
    fn test_from_str() {
        assert_eq!(DiagnosticCode::from_str("LineLength"), Ok(DiagnosticCode::LineLength));
        assert_eq!(DiagnosticCode::from_str("EmptyCodeBlock"), Ok(DiagnosticCode::EmptyCodeBlock));
        assert!(DiagnosticCode::from_str("UnknownCode").is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", DiagnosticCode::LineLength), "LineLength");
    }
}
