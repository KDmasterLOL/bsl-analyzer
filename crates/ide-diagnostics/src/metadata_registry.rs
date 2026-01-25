//! Central registry for diagnostic metadata.
//!
//! Provides const metadata definitions for all diagnostics.
//! Progress: 149/149 diagnostics defined (100%)
//! - 11 DISABLED_BY_DEFAULT diagnostics
//! - 39 Tier 1 diagnostics (syntax-only)
//! - 56 Tier 2 diagnostics (semantic analysis)
//! - 43 Tier 3 + SDBL + Additional diagnostics (metadata-based + queries + special cases)

use crate::metadata::*;
use crate::DiagnosticCode;

/// Get metadata for a diagnostic code.
///
/// Returns `None` if metadata is not yet defined for this diagnostic.
pub fn get_metadata(code: DiagnosticCode) -> Option<&'static DiagnosticMetadata> {
    match code {
        // DISABLED_BY_DEFAULT diagnostics (11 total)
        DiagnosticCode::BadWords => Some(&BAD_WORDS),
        DiagnosticCode::CodeAfterAsyncCall => Some(&CODE_AFTER_ASYNC_CALL),
        DiagnosticCode::DenyIncompleteValues => Some(&DENY_INCOMPLETE_VALUES),
        DiagnosticCode::FieldsFromJoinsWithoutIsNull => Some(&FIELDS_FROM_JOINS_WITHOUT_IS_NULL),
        DiagnosticCode::FileSystemAccess => Some(&FILE_SYSTEM_ACCESS),
        DiagnosticCode::FunctionNameStartsWithGet => Some(&FUNCTION_NAME_STARTS_WITH_GET),
        DiagnosticCode::FunctionOutParameter => Some(&FUNCTION_OUT_PARAMETER),
        DiagnosticCode::InternetAccess => Some(&INTERNET_ACCESS),
        DiagnosticCode::MissingTempStorageDeletion => Some(&MISSING_TEMP_STORAGE_DELETION),
        DiagnosticCode::TernaryOperatorUsage => Some(&TERNARY_OPERATOR_USAGE),
        DiagnosticCode::TooManyReturns => Some(&TOO_MANY_RETURNS),

        // Tier 1 diagnostics (syntax-only) - 39 total
        DiagnosticCode::ParseError => Some(&PARSE_ERROR),
        DiagnosticCode::CanonicalSpellingKeywords => Some(&CANONICAL_SPELLING_KEYWORDS),
        DiagnosticCode::ConsecutiveEmptyLines => Some(&CONSECUTIVE_EMPTY_LINES),
        DiagnosticCode::LineLength => Some(&LINE_LENGTH),
        DiagnosticCode::MissingSpace => Some(&MISSING_SPACE),
        DiagnosticCode::OneStatementPerLine => Some(&ONE_STATEMENT_PER_LINE),
        DiagnosticCode::SemicolonPresence => Some(&SEMICOLON_PRESENCE),
        DiagnosticCode::SpaceAtStartComment => Some(&SPACE_AT_START_COMMENT),
        DiagnosticCode::IncorrectLineBreak => Some(&INCORRECT_LINE_BREAK),
        DiagnosticCode::IncorrectUseOfStrTemplate => Some(&INCORRECT_USE_OF_STR_TEMPLATE),
        DiagnosticCode::ExtraCommas => Some(&EXTRA_COMMAS),
        DiagnosticCode::CommentedCode => Some(&COMMENTED_CODE),
        DiagnosticCode::EmptyCodeBlock => Some(&EMPTY_CODE_BLOCK),
        DiagnosticCode::EmptyRegion => Some(&EMPTY_REGION),
        DiagnosticCode::EmptyStatement => Some(&EMPTY_STATEMENT),
        DiagnosticCode::UnreachableCode => Some(&UNREACHABLE_CODE),
        DiagnosticCode::CodeBlockBeforeSub => Some(&CODE_BLOCK_BEFORE_SUB),
        DiagnosticCode::CodeOutOfRegion => Some(&CODE_OUT_OF_REGION),
        DiagnosticCode::MagicNumber => Some(&MAGIC_NUMBER),
        DiagnosticCode::MagicDate => Some(&MAGIC_DATE),
        DiagnosticCode::YoLetterUsage => Some(&YO_LETTER_USAGE),
        DiagnosticCode::LatinAndCyrillicSymbolInWord => Some(&LATIN_AND_CYRILLIC_SYMBOL_IN_WORD),
        DiagnosticCode::InvalidCharacterInFile => Some(&INVALID_CHARACTER_IN_FILE),
        DiagnosticCode::DoubleNegatives => Some(&DOUBLE_NEGATIVES),
        DiagnosticCode::NestedTernaryOperator => Some(&NESTED_TERNARY_OPERATOR),
        DiagnosticCode::NonExportMethodsInApiRegion => Some(&NON_EXPORT_METHODS_IN_API_REGION),
        DiagnosticCode::UnaryPlusInConcatenation => Some(&UNARY_PLUS_IN_CONCATENATION),
        DiagnosticCode::UselessTernaryOperator => Some(&USELESS_TERNARY_OPERATOR),
        DiagnosticCode::DuplicateStringLiteral => Some(&DUPLICATE_STRING_LITERAL),
        DiagnosticCode::DuplicateRegion => Some(&DUPLICATE_REGION),
        DiagnosticCode::NonStandardRegion => Some(&NON_STANDARD_REGION),
        DiagnosticCode::DuplicatedInsertionIntoCollection => {
            Some(&DUPLICATED_INSERTION_INTO_COLLECTION)
        }
        DiagnosticCode::ExcessiveAutoTestCheck => Some(&EXCESSIVE_AUTO_TEST_CHECK),
        DiagnosticCode::IdenticalExpressions => Some(&IDENTICAL_EXPRESSIONS),
        DiagnosticCode::IfElseDuplicatedCodeBlock => Some(&IF_ELSE_DUPLICATED_CODE_BLOCK),
        DiagnosticCode::IfElseDuplicatedCondition => Some(&IF_ELSE_DUPLICATED_CONDITION),
        DiagnosticCode::IfElseIfEndsWithElse => Some(&IF_ELSE_IF_ENDS_WITH_ELSE),
        DiagnosticCode::MultilingualStringHasAllDeclaredLanguages => {
            Some(&MULTILINGUAL_STRING_HAS_ALL_DECLARED_LANGUAGES)
        }
        DiagnosticCode::MultilingualStringUsingWithTemplate => {
            Some(&MULTILINGUAL_STRING_USING_WITH_TEMPLATE)
        }
        DiagnosticCode::NestedConstructorsInStructureDeclaration => {
            Some(&NESTED_CONSTRUCTORS_IN_STRUCTURE_DECLARATION)
        }
        DiagnosticCode::NestedFunctionInParameters => Some(&NESTED_FUNCTION_IN_PARAMETERS),

        // Tier 2 diagnostics (semantic analysis) - 52 total
        DiagnosticCode::AllFunctionPathMustHaveReturn => Some(&ALL_FUNCTION_PATH_MUST_HAVE_RETURN),
        DiagnosticCode::FunctionShouldHaveReturn => Some(&FUNCTION_SHOULD_HAVE_RETURN),
        DiagnosticCode::ProcedureReturnsValue => Some(&PROCEDURE_RETURNS_VALUE),
        DiagnosticCode::FunctionReturnsSamePrimitive => Some(&FUNCTION_RETURNS_SAME_PRIMITIVE),
        DiagnosticCode::NumberOfParams => Some(&NUMBER_OF_PARAMS),
        DiagnosticCode::NumberOfOptionalParams => Some(&NUMBER_OF_OPTIONAL_PARAMS),
        DiagnosticCode::NumberOfValuesInStructureConstructor => {
            Some(&NUMBER_OF_VALUES_IN_STRUCTURE_CONSTRUCTOR)
        }
        DiagnosticCode::OrderOfParams => Some(&ORDER_OF_PARAMS),
        DiagnosticCode::MissedRequiredParameter => Some(&MISSED_REQUIRED_PARAMETER),
        DiagnosticCode::UnusedParameters => Some(&UNUSED_PARAMETERS),
        DiagnosticCode::MissingParameterDescription => Some(&MISSING_PARAMETER_DESCRIPTION),
        DiagnosticCode::MissingReturnedValueDescription => {
            Some(&MISSING_RETURNED_VALUE_DESCRIPTION)
        }
        DiagnosticCode::ReservedParameterNames => Some(&RESERVED_PARAMETER_NAMES),
        DiagnosticCode::RewriteMethodParameter => Some(&REWRITE_METHOD_PARAMETER),
        DiagnosticCode::UnusedLocalMethod => Some(&UNUSED_LOCAL_METHOD),
        DiagnosticCode::ExportVariables => Some(&EXPORT_VARIABLES),
        DiagnosticCode::MissingVariablesDescription => Some(&MISSING_VARIABLES_DESCRIPTION),
        DiagnosticCode::SelfAssign => Some(&SELF_ASSIGN),
        DiagnosticCode::ThisObjectAssign => Some(&THIS_OBJECT_ASSIGN),
        DiagnosticCode::CyclomaticComplexity => Some(&CYCLOMATIC_COMPLEXITY),
        DiagnosticCode::CognitiveComplexity => Some(&COGNITIVE_COMPLEXITY),
        DiagnosticCode::NestedStatements => Some(&NESTED_STATEMENTS),
        DiagnosticCode::MethodSize => Some(&METHOD_SIZE),
        DiagnosticCode::IfConditionComplexity => Some(&IF_CONDITION_COMPLEXITY),
        DiagnosticCode::MissingCodeTryCatchEx => Some(&MISSING_CODE_TRY_CATCH_EX),
        DiagnosticCode::MissingTemporaryFileDeletion => Some(&MISSING_TEMPORARY_FILE_DELETION),
        DiagnosticCode::UseLessForEach => Some(&USE_LESS_FOR_EACH),
        DiagnosticCode::UsingGoto => Some(&USING_GOTO),
        DiagnosticCode::BeginTransactionBeforeTryCatch => Some(&BEGIN_TRANSACTION_BEFORE_TRY_CATCH),
        DiagnosticCode::CommitTransactionOutsideTryCatch => {
            Some(&COMMIT_TRANSACTION_OUTSIDE_TRY_CATCH)
        }
        DiagnosticCode::CompilationDirectiveLost => Some(&COMPILATION_DIRECTIVE_LOST),
        DiagnosticCode::CompilationDirectiveNeedLess => Some(&COMPILATION_DIRECTIVE_NEED_LESS),
        DiagnosticCode::CreateQueryInCycle => Some(&CREATE_QUERY_IN_CYCLE),
        DiagnosticCode::DeletingCollectionItem => Some(&DELETING_COLLECTION_ITEM),
        DiagnosticCode::SelfInsertion => Some(&SELF_INSERTION),
        DiagnosticCode::SeveralCompilerDirectives => Some(&SEVERAL_COMPILER_DIRECTIVES),
        DiagnosticCode::StyleElementConstructors => Some(&STYLE_ELEMENT_CONSTRUCTORS),
        DiagnosticCode::DeprecatedCurrentDate => Some(&DEPRECATED_CURRENT_DATE),
        DiagnosticCode::DeprecatedFind => Some(&DEPRECATED_FIND),
        DiagnosticCode::DeprecatedMessage => Some(&DEPRECATED_MESSAGE),
        DiagnosticCode::DeprecatedTypeManagedForm => Some(&DEPRECATED_TYPE_MANAGED_FORM),
        DiagnosticCode::DeprecatedMethods8310 => Some(&DEPRECATED_METHODS_8310),
        DiagnosticCode::DeprecatedMethods8317 => Some(&DEPRECATED_METHODS_8317),
        DiagnosticCode::DeprecatedAttributes8312 => Some(&DEPRECATED_ATTRIBUTES_8312),
        DiagnosticCode::DeprecatedMethodCall => Some(&DEPRECATED_METHOD_CALL),
        DiagnosticCode::DisableSafeMode => Some(&DISABLE_SAFE_MODE),
        DiagnosticCode::ExternalAppStarting => Some(&EXTERNAL_APP_STARTING),
        DiagnosticCode::OSUsersMethod => Some(&OS_USERS_METHOD),
        DiagnosticCode::TempFilesDir => Some(&TEMP_FILES_DIR),
        DiagnosticCode::FormDataToValue => Some(&FORM_DATA_TO_VALUE),
        DiagnosticCode::GetFormMethod => Some(&GET_FORM_METHOD),
        DiagnosticCode::GlobalContextMethodCollision8312 => {
            Some(&GLOBAL_CONTEXT_METHOD_COLLISION_8312)
        }
        DiagnosticCode::IsInRoleMethod => Some(&IS_IN_ROLE_METHOD),
        DiagnosticCode::PairingBrokenTransaction => Some(&PAIRING_BROKEN_TRANSACTION),
        DiagnosticCode::WrongUseOfRollbackTransactionMethod => {
            Some(&WRONG_USE_OF_ROLLBACK_TRANSACTION_METHOD)
        }

        // Tier 3 + SDBL diagnostics (35 total)
        DiagnosticCode::AssignAliasFieldsInQuery => Some(&ASSIGN_ALIAS_FIELDS_IN_QUERY),
        DiagnosticCode::CachedPublic => Some(&CACHED_PUBLIC),
        DiagnosticCode::CommandModuleExportMethods => Some(&COMMAND_MODULE_EXPORT_METHODS),
        DiagnosticCode::CommonModuleAssign => Some(&COMMON_MODULE_ASSIGN),
        DiagnosticCode::CommonModuleInvalidType => Some(&COMMON_MODULE_INVALID_TYPE),
        DiagnosticCode::CommonModuleMissingAPI => Some(&COMMON_MODULE_MISSING_API),
        DiagnosticCode::CommonModuleNameCached => Some(&COMMON_MODULE_NAME_CACHED),
        DiagnosticCode::CommonModuleNameClient => Some(&COMMON_MODULE_NAME_CLIENT),
        DiagnosticCode::CommonModuleNameClientServer => Some(&COMMON_MODULE_NAME_CLIENT_SERVER),
        DiagnosticCode::CommonModuleNameFullAccess => Some(&COMMON_MODULE_NAME_FULL_ACCESS),
        DiagnosticCode::CommonModuleNameGlobal => Some(&COMMON_MODULE_NAME_GLOBAL),
        DiagnosticCode::CommonModuleNameGlobalClient => Some(&COMMON_MODULE_NAME_GLOBAL_CLIENT),
        DiagnosticCode::CommonModuleNameServerCall => Some(&COMMON_MODULE_NAME_SERVER_CALL),
        DiagnosticCode::CommonModuleNameWords => Some(&COMMON_MODULE_NAME_WORDS),
        DiagnosticCode::FullOuterJoinQuery => Some(&FULL_OUTER_JOIN_QUERY),
        DiagnosticCode::JoinWithSubQuery => Some(&JOIN_WITH_SUB_QUERY),
        DiagnosticCode::LogicalOrInJoinQuerySection => Some(&LOGICAL_OR_IN_JOIN_QUERY_SECTION),
        DiagnosticCode::LogicalOrInTheWhereSectionOfQuery => {
            Some(&LOGICAL_OR_IN_THE_WHERE_SECTION_OF_QUERY)
        }
        DiagnosticCode::MetadataObjectNameLength => Some(&METADATA_OBJECT_NAME_LENGTH),
        DiagnosticCode::MissingCommonModuleMethod => Some(&MISSING_COMMON_MODULE_METHOD),
        DiagnosticCode::MissingEventSubscriptionHandler => {
            Some(&MISSING_EVENT_SUBSCRIPTION_HANDLER)
        }
        DiagnosticCode::MultilineStringInQuery => Some(&MULTILINE_STRING_IN_QUERY),
        DiagnosticCode::OrdinaryAppSupport => Some(&ORDINARY_APP_SUPPORT),
        DiagnosticCode::PrivilegedModuleMethodCall => Some(&PRIVILEGED_MODULE_METHOD_CALL),
        DiagnosticCode::ProtectedModule => Some(&PROTECTED_MODULE),
        DiagnosticCode::PublicMethodsDescription => Some(&PUBLIC_METHODS_DESCRIPTION),
        DiagnosticCode::QueryNestedFieldsByDot => Some(&QUERY_NESTED_FIELDS_BY_DOT),
        DiagnosticCode::QueryParseError => Some(&QUERY_PARSE_ERROR),
        DiagnosticCode::QueryToMissingMetadata => Some(&QUERY_TO_MISSING_METADATA),
        DiagnosticCode::RefOveruse => Some(&REF_OVERUSE),
        DiagnosticCode::UnionAll => Some(&UNION_ALL),
        DiagnosticCode::UsingLikeInQuery => Some(&USING_LIKE_IN_QUERY),
        DiagnosticCode::VirtualTableCallWithoutParameters => {
            Some(&VIRTUAL_TABLE_CALL_WITHOUT_PARAMETERS)
        }
        DiagnosticCode::ScheduledJobHandler => Some(&SCHEDULED_JOB_HANDLER),
        DiagnosticCode::ServerCallsInFormEvents => Some(&SERVER_CALLS_IN_FORM_EVENTS),
        DiagnosticCode::ServerSideExportFormMethod => Some(&SERVER_SIDE_EXPORT_FORM_METHOD),
        DiagnosticCode::SetPermissionsForNewObjects => Some(&SET_PERMISSIONS_FOR_NEW_OBJECTS),
        DiagnosticCode::SetPrivilegedMode => Some(&SET_PRIVILEGED_MODE),
        DiagnosticCode::TransferringParametersBetweenClientAndServer => {
            Some(&TRANSFERRING_PARAMETERS_BETWEEN_CLIENT_AND_SERVER)
        }
        DiagnosticCode::UnsafeFindByCode => Some(&UNSAFE_FIND_BY_CODE),

        // Additional diagnostics
        DiagnosticCode::DataExchangeLoading => Some(&DATA_EXCHANGE_LOADING),
        DiagnosticCode::ExecuteExternalCode => Some(&EXECUTE_EXTERNAL_CODE),
        DiagnosticCode::ExecuteExternalCodeInCommonModule => {
            Some(&EXECUTE_EXTERNAL_CODE_IN_COMMON_MODULE)
        }
        DiagnosticCode::RedundantAccessToObject => Some(&REDUNDANT_ACCESS_TO_OBJECT),
        DiagnosticCode::SameMetadataObjectAndChildNames => {
            Some(&SAME_METADATA_OBJECT_AND_CHILD_NAMES)
        }
        DiagnosticCode::UnusedLocalVariable => Some(&UNUSED_LOCAL_VARIABLE),
        DiagnosticCode::TimeoutsInExternalResources => Some(&TIMEOUTS_IN_EXTERNAL_RESOURCES),
        DiagnosticCode::TryNumber => Some(&TRY_NUMBER),
        DiagnosticCode::Typo => Some(&TYPO),
        DiagnosticCode::UnknownPreprocessorSymbol => Some(&UNKNOWN_PREPROCESSOR_SYMBOL),
        DiagnosticCode::UnsafeSafeModeMethodCall => Some(&UNSAFE_SAFE_MODE_METHOD_CALL),
        DiagnosticCode::UsageWriteLogEvent => Some(&USAGE_WRITE_LOG_EVENT),
        DiagnosticCode::UseSystemInformation => Some(&USE_SYSTEM_INFORMATION),
        DiagnosticCode::UsingCancelParameter => Some(&USING_CANCEL_PARAMETER),
        DiagnosticCode::UsingExternalCodeTools => Some(&USING_EXTERNAL_CODE_TOOLS),
        DiagnosticCode::UsingFindElementByString => Some(&USING_FIND_ELEMENT_BY_STRING),
        DiagnosticCode::UsingHardcodeNetworkAddress => Some(&USING_HARDCODE_NETWORK_ADDRESS),
        DiagnosticCode::UsingHardcodePath => Some(&USING_HARDCODE_PATH),
        DiagnosticCode::UsingHardcodeSecretInformation => Some(&USING_HARDCODE_SECRET_INFORMATION),
        DiagnosticCode::UsingModalWindows => Some(&USING_MODAL_WINDOWS),
        DiagnosticCode::UsingObjectNotAvailableUnix => Some(&USING_OBJECT_NOT_AVAILABLE_UNIX),
        DiagnosticCode::UsingServiceTag => Some(&USING_SERVICE_TAG),
        DiagnosticCode::UsingThisForm => Some(&USING_THIS_FORM),
        DiagnosticCode::WrongDataPathForFormElements => Some(&WRONG_DATA_PATH_FOR_FORM_ELEMENTS),
        DiagnosticCode::WrongHttpServiceHandler => Some(&WRONG_HTTP_SERVICE_HANDLER),
        DiagnosticCode::WrongWebServiceHandler => Some(&WRONG_WEB_SERVICE_HANDLER),
        DiagnosticCode::WrongUseFunctionProceedWithCall => {
            Some(&WRONG_USE_FUNCTION_PROCEED_WITH_CALL)
        }
    }
}

// ============================================================================
// DISABLED_BY_DEFAULT diagnostics (11 total)
// ============================================================================

/// BadWords diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 1,
///   activatedByDefault = false,
///   tags = { DESIGN }
/// )
const BAD_WORDS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CodeAfterAsyncCall diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 10,
///   tags = { SUSPICIOUS },
///   activatedByDefault = false (default)
/// )
const CODE_AFTER_ASYNC_CALL: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// DenyIncompleteValues diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   activatedByDefault = false,
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 1,
///   tags = { BADPRACTICE },
///   scope = BSL,
///   canLocateOnProject = true
/// )
const DENY_INCOMPLETE_VALUES: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: true,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// FieldsFromJoinsWithoutIsNull diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   minutesToFix = 2,
///   activatedByDefault = false,
///   tags = { SQL, SUSPICIOUS, UNPREDICTABLE }
/// )
const FIELDS_FROM_JOINS_WITHOUT_IS_NULL: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Suspicious, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// FileSystemAccess diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = VULNERABILITY,
///   severity = MAJOR,
///   minutesToFix = 3,
///   tags = { SUSPICIOUS },
///   scope = BSL,
///   activatedByDefault = false
/// )
const FILE_SYSTEM_ACCESS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// FunctionNameStartsWithGet diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = INFO,
///   minutesToFix = 3,
///   activatedByDefault = false,
///   tags = { STANDARD }
/// )
const FUNCTION_NAME_STARTS_WITH_GET: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// FunctionOutParameter diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 10,
///   activatedByDefault = false,
///   tags = { DESIGN }
/// )
const FUNCTION_OUT_PARAMETER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// InternetAccess diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = VULNERABILITY,
///   severity = MAJOR,
///   minutesToFix = 60,
///   tags = { SUSPICIOUS },
///   activatedByDefault = false
/// )
const INTERNET_ACCESS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 60,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MissingTempStorageDeletion diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = CRITICAL,
///   minutesToFix = 3,
///   tags = { STANDARD, PERFORMANCE, BADPRACTICE },
///   activatedByDefault = false
/// )
const MISSING_TEMP_STORAGE_DELETION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Performance, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// TernaryOperatorUsage diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   minutesToFix = 3,
///   activatedByDefault = false,
///   tags = { BRAINOVERLOAD }
/// )
const TERNARY_OPERATOR_USAGE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// TooManyReturns diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   minutesToFix = 20,
///   activatedByDefault = false,
///   tags = { BRAINOVERLOAD }
/// )
const TOO_MANY_RETURNS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 20,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

// ============================================================================
// Tier 1 diagnostics (syntax-only) - 39 total
// ============================================================================

const PARSE_ERROR: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const CANONICAL_SPELLING_KEYWORDS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const CONSECUTIVE_EMPTY_LINES: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const LINE_LENGTH: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const MISSING_SPACE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const ONE_STATEMENT_PER_LINE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const SEMICOLON_PRESENCE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const SPACE_AT_START_COMMENT: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const INCORRECT_LINE_BREAK: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const INCORRECT_USE_OF_STR_TEMPLATE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Suspicious, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const EXTRA_COMMAS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const COMMENTED_CODE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const EMPTY_CODE_BLOCK: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const EMPTY_REGION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const EMPTY_STATEMENT: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const UNREACHABLE_CODE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const CODE_BLOCK_BEFORE_SUB: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const CODE_OUT_OF_REGION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Compatibility8320,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const MAGIC_NUMBER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const MAGIC_DATE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const YO_LETTER_USAGE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const LATIN_AND_CYRILLIC_SYMBOL_IN_WORD: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const INVALID_CHARACTER_IN_FILE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Standard, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DOUBLE_NEGATIVES: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const NESTED_TERNARY_OPERATOR: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const NON_EXPORT_METHODS_IN_API_REGION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const UNARY_PLUS_IN_CONCATENATION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const USELESS_TERNARY_OPERATOR: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DUPLICATE_STRING_LITERAL: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DUPLICATE_REGION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Compatibility8320,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const NON_STANDARD_REGION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Compatibility8320,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DUPLICATED_INSERTION_INTO_COLLECTION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Suspicious, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const EXCESSIVE_AUTO_TEST_CHECK: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[
        bsl_metadata::ModuleType::FormModule,
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::RecordSetModule,
        bsl_metadata::ModuleType::CommonModule,
    ],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const IDENTICAL_EXPRESSIONS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const IF_ELSE_DUPLICATED_CODE_BLOCK: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const IF_ELSE_DUPLICATED_CONDITION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const IF_ELSE_IF_ENDS_WITH_ELSE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const MULTILINGUAL_STRING_HAS_ALL_DECLARED_LANGUAGES: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Localize],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const MULTILINGUAL_STRING_USING_WITH_TEMPLATE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Localize],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const NESTED_CONSTRUCTORS_IN_STRUCTURE_DECLARATION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const NESTED_FUNCTION_IN_PARAMETERS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Brainoverload, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

// ============================================================================
// Tier 2 diagnostics (semantic analysis) - 52 total
// ============================================================================

/// AllFunctionPathMustHaveReturn diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 1,
///   tags = { UNPREDICTABLE, BADPRACTICE, SUSPICIOUS }
/// )
const ALL_FUNCTION_PATH_MUST_HAVE_RETURN: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Unpredictable, MetadataTag::Badpractice, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// FunctionShouldHaveReturn diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 10,
///   tags = { SUSPICIOUS, UNPREDICTABLE }
/// )
const FUNCTION_SHOULD_HAVE_RETURN: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// ProcedureReturnsValue diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = BLOCKER,
///   minutesToFix = 5,
///   tags = { ERROR }
/// )
const PROCEDURE_RETURNS_VALUE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// FunctionReturnsSamePrimitive diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 5,
///   tags = { DESIGN, BADPRACTICE }
/// )
const FUNCTION_RETURNS_SAME_PRIMITIVE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// NumberOfParams diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   minutesToFix = 30,
///   tags = { STANDARD, BRAINOVERLOAD }
/// )
const NUMBER_OF_PARAMS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// NumberOfOptionalParams diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   minutesToFix = 30,
///   tags = { STANDARD, BRAINOVERLOAD }
/// )
const NUMBER_OF_OPTIONAL_PARAMS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// NumberOfValuesInStructureConstructor diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   scope = ALL,
///   minutesToFix = 10,
///   tags = { STANDARD, BRAINOVERLOAD }
/// )
const NUMBER_OF_VALUES_IN_STRUCTURE_CONSTRUCTOR: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// OrderOfParams diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 30,
///   tags = { STANDARD, DESIGN }
/// )
const ORDER_OF_PARAMS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MissedRequiredParameter diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 1,
///   tags = { ERROR }
/// )
const MISSED_REQUIRED_PARAMETER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UnusedParameters diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = OS,
///   minutesToFix = 5,
///   tags = { DESIGN, UNUSED }
/// )
const UNUSED_PARAMETERS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Os,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design, MetadataTag::Unused],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MissingParameterDescription diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 5,
///   tags = { STANDARD, BADPRACTICE }
/// )
const MISSING_PARAMETER_DESCRIPTION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MissingReturnedValueDescription diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 5,
///   tags = { STANDARD, BADPRACTICE }
/// )
const MISSING_RETURNED_VALUE_DESCRIPTION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// ReservedParameterNames diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 5,
///   tags = { STANDARD, BADPRACTICE }
/// )
const RESERVED_PARAMETER_NAMES: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// RewriteMethodParameter diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 2,
///   tags = { SUSPICIOUS }
/// )
const REWRITE_METHOD_PARAMETER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UnusedLocalMethod diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   modules = { CommonModule, ObjectModule },
///   minutesToFix = 1,
///   tags = { STANDARD, SUSPICIOUS, UNUSED }
/// )
const UNUSED_LOCAL_METHOD: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[bsl_metadata::ModuleType::CommonModule, bsl_metadata::ModuleType::ObjectModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Suspicious, MetadataTag::Unused],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// ExportVariables diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 5,
///   scope = ALL,
///   tags = { STANDARD, DESIGN, UNPREDICTABLE }
/// )
const EXPORT_VARIABLES: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MissingVariablesDescription diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   minutesToFix = 1,
///   tags = { STANDARD }
/// )
const MISSING_VARIABLES_DESCRIPTION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// SelfAssign diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 10,
///   tags = { SUSPICIOUS }
/// )
const SELF_ASSIGN: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// ThisObjectAssign diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = BLOCKER,
///   scope = BSL,
///   modules = { CommonModule, FormModule },
///   minutesToFix = 1,
///   compatibilityMode = COMPATIBILITY_MODE_8_3_3,
///   tags = { ERROR }
/// )
const THIS_OBJECT_ASSIGN: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule, bsl_metadata::ModuleType::FormModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_3,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CyclomaticComplexity diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = CRITICAL,
///   minutesToFix = 25,
///   tags = { BRAINOVERLOAD },
///   extraMinForComplexity = 1
/// )
const CYCLOMATIC_COMPLEXITY: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 25,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 1.0,
    lsp_severity_override: "",
};

/// CognitiveComplexity diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = CRITICAL,
///   minutesToFix = 15,
///   tags = { BRAINOVERLOAD },
///   extraMinForComplexity = 1
/// )
const COGNITIVE_COMPLEXITY: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 1.0,
    lsp_severity_override: "",
};

/// NestedStatements diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = CRITICAL,
///   scope = ALL,
///   minutesToFix = 30,
///   tags = { BADPRACTICE, BRAINOVERLOAD }
/// )
const NESTED_STATEMENTS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MethodSize diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 30,
///   tags = { BADPRACTICE }
/// )
const METHOD_SIZE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// IfConditionComplexity diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   minutesToFix = 5,
///   tags = { BRAINOVERLOAD }
/// )
const IF_CONDITION_COMPLEXITY: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MissingCodeTryCatchEx diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 15,
///   tags = { STANDARD, BADPRACTICE }
/// )
const MISSING_CODE_TRY_CATCH_EX: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MissingTemporaryFileDeletion diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 5,
///   tags = { BADPRACTICE, STANDARD }
/// )
const MISSING_TEMPORARY_FILE_DELETION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UseLessForEach diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   minutesToFix = 2,
///   tags = { CLUMSY }
/// )
const USE_LESS_FOR_EACH: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Clumsy],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UsingGoto diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = CRITICAL,
///   minutesToFix = 5,
///   tags = { STANDARD, BADPRACTICE }
/// )
const USING_GOTO: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// BeginTransactionBeforeTryCatch diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 10,
///   tags = { STANDARD }
/// )
const BEGIN_TRANSACTION_BEFORE_TRY_CATCH: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommitTransactionOutsideTryCatch diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 10,
///   tags = { STANDARD }
/// )
const COMMIT_TRANSACTION_OUTSIDE_TRY_CATCH: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CompilationDirectiveLost diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = BSL,
///   modules = { FormModule, CommandModule },
///   minutesToFix = 1,
///   tags = { STANDARD, UNPREDICTABLE }
/// )
const COMPILATION_DIRECTIVE_LOST: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::FormModule, bsl_metadata::ModuleType::CommandModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CompilationDirectiveNeedLess diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = BSL,
///   modules = { ApplicationModule, CommonModule, ExternalConnectionModule,
///               ManagedApplicationModule, ManagerModule, ObjectModule,
///               OrdinaryApplicationModule, RecordSetModule, SessionModule,
///               ValueManagerModule },
///   minutesToFix = 1,
///   tags = { CLUMSY, STANDARD, UNPREDICTABLE }
/// )
const COMPILATION_DIRECTIVE_NEED_LESS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::ApplicationModule,
        bsl_metadata::ModuleType::CommonModule,
        bsl_metadata::ModuleType::ExternalConnectionModule,
        bsl_metadata::ModuleType::ManagedApplicationModule,
        bsl_metadata::ModuleType::ManagerModule,
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::OrdinaryApplicationModule,
        bsl_metadata::ModuleType::RecordSetModule,
        bsl_metadata::ModuleType::SessionModule,
        bsl_metadata::ModuleType::ValueManagerModule,
    ],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Clumsy, MetadataTag::Standard, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CreateQueryInCycle diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   minutesToFix = 20,
///   tags = { PERFORMANCE }
/// )
const CREATE_QUERY_IN_CYCLE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 20,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// DeletingCollectionItem diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 5,
///   tags = { STANDARD, ERROR }
/// )
const DELETING_COLLECTION_ITEM: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// SelfInsertion diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 10,
///   tags = { STANDARD, UNPREDICTABLE, PERFORMANCE }
/// )
const SELF_INSERTION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Unpredictable, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// SeveralCompilerDirectives diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   minutesToFix = 5,
///   tags = { UNPREDICTABLE, ERROR }
/// )
const SEVERAL_COMPILER_DIRECTIVES: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Unpredictable, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// StyleElementConstructors diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MINOR,
///   scope = BSL,
///   minutesToFix = 5,
///   tags = { STANDARD, BADPRACTICE }
/// )
const STYLE_ELEMENT_CONSTRUCTORS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// DeprecatedCurrentDate diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   scope = BSL,
///   minutesToFix = 5,
///   tags = { STANDARD, DEPRECATED, UNPREDICTABLE }
/// )
const DEPRECATED_CURRENT_DATE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// DeprecatedFind diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   scope = BSL,
///   minutesToFix = 2,
///   compatibilityMode = COMPATIBILITY_MODE_8_3_6,
///   tags = { DEPRECATED }
/// )
const DEPRECATED_FIND: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_6,
    tags: &[MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// DeprecatedMessage diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   scope = BSL,
///   minutesToFix = 2,
///   tags = { STANDARD, DEPRECATED }
/// )
const DEPRECATED_MESSAGE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// DeprecatedTypeManagedForm diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = INFO,
///   scope = BSL,
///   compatibilityMode = COMPATIBILITY_MODE_8_3_14,
///   minutesToFix = 1,
///   tags = { STANDARD, DEPRECATED }
/// )
const DEPRECATED_TYPE_MANAGED_FORM: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_14,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// DeprecatedMethods8310 diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = INFO,
///   minutesToFix = 1,
///   scope = BSL,
///   compatibilityMode = COMPATIBILITY_MODE_8_3_10,
///   tags = { DEPRECATED }
/// )
const DEPRECATED_METHODS_8310: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_10,
    tags: &[MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// DeprecatedMethods8317 diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = INFO,
///   compatibilityMode = COMPATIBILITY_MODE_8_3_17,
///   scope = BSL,
///   minutesToFix = 5,
///   tags = { DEPRECATED }
/// )
const DEPRECATED_METHODS_8317: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_17,
    tags: &[MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// DeprecatedAttributes8312 diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = INFO,
///   scope = BSL,
///   compatibilityMode = COMPATIBILITY_MODE_8_3_12,
///   minutesToFix = 1,
///   tags = { DEPRECATED }
/// )
const DEPRECATED_ATTRIBUTES_8312: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_12,
    tags: &[MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// DeprecatedMethodCall diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   minutesToFix = 3,
///   tags = { DEPRECATED, DESIGN }
/// )
const DEPRECATED_METHOD_CALL: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Deprecated, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// DisableSafeMode diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = VULNERABILITY,
///   severity = MAJOR,
///   minutesToFix = 15,
///   tags = { SUSPICIOUS },
///   scope = BSL
/// )
const DISABLE_SAFE_MODE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// ExternalAppStarting diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = SECURITY_HOTSPOT,
///   severity = MAJOR,
///   minutesToFix = 5,
///   tags = { SUSPICIOUS },
///   scope = BSL
/// )
const EXTERNAL_APP_STARTING: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// OSUsersMethod diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = SECURITY_HOTSPOT,
///   severity = CRITICAL,
///   minutesToFix = 15,
///   scope = BSL,
///   tags = { SUSPICIOUS }
/// )
const OS_USERS_METHOD: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// TempFilesDir diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 5,
///   scope = BSL,
///   tags = { STANDARD, BADPRACTICE }
/// )
const TEMP_FILES_DIR: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// FormDataToValue diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = INFO,
///   scope = BSL,
///   minutesToFix = 5,
///   tags = { BADPRACTICE }
/// )
const FORM_DATA_TO_VALUE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// GetFormMethod diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 15,
///   scope = BSL,
///   tags = { ERROR }
/// )
const GET_FORM_METHOD: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// GlobalContextMethodCollision8312 diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = BLOCKER,
///   minutesToFix = 10,
///   tags = { ERROR, UNPREDICTABLE },
///   compatibilityMode = COMPATIBILITY_MODE_8_3_12
/// )
const GLOBAL_CONTEXT_METHOD_COLLISION_8312: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_12,
    tags: &[MetadataTag::Error, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// IsInRoleMethod diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = BSL,
///   minutesToFix = 5,
///   tags = { ERROR }
/// )
const IS_IN_ROLE_METHOD: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// PairingBrokenTransaction diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 15,
///   tags = { STANDARD }
/// )
const PAIRING_BROKEN_TRANSACTION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// WrongUseOfRollbackTransactionMethod diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   minutesToFix = 1,
///   scope = BSL,
///   tags = { STANDARD }
/// )
const WRONG_USE_OF_ROLLBACK_TRANSACTION_METHOD: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// TimeoutsInExternalResources diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   minutesToFix = 5,
///   tags = { UNPREDICTABLE, STANDARD }
/// )
const TIMEOUTS_IN_EXTERNAL_RESOURCES: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Unpredictable, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// TryNumber diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 2,
///   tags = { STANDARD }
/// )
const TRY_NUMBER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UnknownPreprocessorSymbol diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   minutesToFix = 5,
///   tags = { STANDARD, ERROR }
/// )
const UNKNOWN_PREPROCESSOR_SYMBOL: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const UNSAFE_SAFE_MODE_METHOD_CALL: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_1,
    tags: &[MetadataTag::Deprecated, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

// ============================================================================
// Tier 3 + SDBL diagnostics (36 total)
// ============================================================================

/// WrongDataPathForFormElements diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   scope = BSL,
///   modules = { FormModule, ManagedApplicationModule },
///   minutesToFix = 5,
///   tags = { UNPREDICTABLE }
/// )
const WRONG_DATA_PATH_FOR_FORM_ELEMENTS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::FormModule,
        bsl_metadata::ModuleType::ManagedApplicationModule,
    ],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// WrongHttpServiceHandler diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   minutesToFix = 10,
///   tags = { SUSPICIOUS, ERROR }
/// )
const WRONG_HTTP_SERVICE_HANDLER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::HTTPServiceModule],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// WrongWebServiceHandler diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   minutesToFix = 10,
///   tags = { SUSPICIOUS, ERROR }
/// )
const WRONG_WEB_SERVICE_HANDLER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::WebServiceModule],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// WrongUseFunctionProceedWithCall diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = BLOCKER,
///   minutesToFix = 1,
///   scope = BSL,
///   tags = { ERROR, SUSPICIOUS }
/// )
const WRONG_USE_FUNCTION_PROCEED_WITH_CALL: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// AssignAliasFieldsInQuery diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 1,
///   scope = BSL,
///   tags = { STANDARD, SQL, BADPRACTICE }
/// )
const ASSIGN_ALIAS_FIELDS_IN_QUERY: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CachedPublic diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = BSL,
///   modules = { CommonModule },
///   minutesToFix = 5,
///   tags = { STANDARD, DESIGN }
/// )
const CACHED_PUBLIC: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommandModuleExportMethods diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = INFO,
///   scope = BSL,
///   modules = { CommandModule },
///   minutesToFix = 1,
///   tags = { STANDARD, CLUMSY }
/// )
const COMMAND_MODULE_EXPORT_METHODS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommandModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Clumsy],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommonModuleAssign diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = BLOCKER,
///   minutesToFix = 2,
///   tags = { ERROR }
/// )
const COMMON_MODULE_ASSIGN: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommonModuleInvalidType diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   scope = BSL,
///   modules = { CommonModule },
///   minutesToFix = 5,
///   tags = { STANDARD, UNPREDICTABLE, DESIGN }
/// )
const COMMON_MODULE_INVALID_TYPE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Unpredictable, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommonModuleMissingAPI diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   scope = BSL,
///   modules = { CommonModule },
///   minutesToFix = 1,
///   tags = { BRAINOVERLOAD, SUSPICIOUS }
/// )
const COMMON_MODULE_MISSING_API: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommonModuleNameCached diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = BSL,
///   modules = { CommonModule },
///   minutesToFix = 5,
///   tags = { STANDARD, BADPRACTICE, UNPREDICTABLE }
/// )
const COMMON_MODULE_NAME_CACHED: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommonModuleNameClient diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   scope = BSL,
///   modules = { CommonModule },
///   minutesToFix = 5,
///   tags = { STANDARD, BADPRACTICE, UNPREDICTABLE }
/// )
const COMMON_MODULE_NAME_CLIENT: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommonModuleNameClientServer diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = BSL,
///   modules = { CommonModule },
///   minutesToFix = 5,
///   tags = { STANDARD, BADPRACTICE, UNPREDICTABLE }
/// )
const COMMON_MODULE_NAME_CLIENT_SERVER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommonModuleNameFullAccess diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = SECURITY_HOTSPOT,
///   severity = MAJOR,
///   scope = BSL,
///   modules = { CommonModule },
///   minutesToFix = 5,
///   tags = { STANDARD, BADPRACTICE, UNPREDICTABLE }
/// )
const COMMON_MODULE_NAME_FULL_ACCESS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommonModuleNameGlobal diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = BSL,
///   modules = { CommonModule },
///   minutesToFix = 5,
///   tags = { STANDARD, BADPRACTICE, BRAINOVERLOAD }
/// )
const COMMON_MODULE_NAME_GLOBAL: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommonModuleNameGlobalClient diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = BSL,
///   modules = { CommonModule },
///   minutesToFix = 5,
///   tags = { STANDARD }
/// )
const COMMON_MODULE_NAME_GLOBAL_CLIENT: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommonModuleNameServerCall diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   scope = BSL,
///   modules = { CommonModule },
///   minutesToFix = 5,
///   tags = { STANDARD, BADPRACTICE, UNPREDICTABLE }
/// )
const COMMON_MODULE_NAME_SERVER_CALL: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// CommonModuleNameWords diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = INFO,
///   scope = BSL,
///   modules = { CommonModule },
///   minutesToFix = 5,
///   tags = { STANDARD }
/// )
const COMMON_MODULE_NAME_WORDS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// FullOuterJoinQuery diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 10,
///   tags = { SQL, STANDARD, PERFORMANCE },
///   scope = BSL
/// )
const FULL_OUTER_JOIN_QUERY: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Standard, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// JoinWithSubQuery diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 10,
///   tags = { SQL, STANDARD, PERFORMANCE },
///   scope = BSL
/// )
const JOIN_WITH_SUB_QUERY: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Standard, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// LogicalOrInJoinQuerySection diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 15,
///   tags = { SQL, PERFORMANCE, UNPREDICTABLE }
/// )
const LOGICAL_OR_IN_JOIN_QUERY_SECTION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Performance, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// LogicalOrInTheWhereSectionOfQuery diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 15,
///   tags = { SQL, PERFORMANCE, STANDARD },
///   scope = BSL
/// )
const LOGICAL_OR_IN_THE_WHERE_SECTION_OF_QUERY: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Performance, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MetadataObjectNameLength diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 10,
///   scope = BSL,
///   tags = { STANDARD },
///   canLocateOnProject = true
/// )
const METADATA_OBJECT_NAME_LENGTH: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: true,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MissingCommonModuleMethod diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = BLOCKER,
///   scope = BSL,
///   minutesToFix = 5,
///   tags = { ERROR }
/// )
const MISSING_COMMON_MODULE_METHOD: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MissingEventSubscriptionHandler diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = BLOCKER,
///   minutesToFix = 5,
///   tags = { ERROR },
///   scope = BSL,
///   modules = { SessionModule }
/// )
const MISSING_EVENT_SUBSCRIPTION_HANDLER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::SessionModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// MultilineStringInQuery diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   minutesToFix = 1,
///   tags = { BADPRACTICE, SUSPICIOUS, UNPREDICTABLE },
///   scope = BSL
/// )
const MULTILINE_STRING_IN_QUERY: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Suspicious, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// OrdinaryAppSupport diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = BSL,
///   modules = { SessionModule },
///   minutesToFix = 1,
///   tags = { STANDARD, UNPREDICTABLE }
/// )
const ORDINARY_APP_SUPPORT: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::SessionModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// PrivilegedModuleMethodCall diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = SECURITY_HOTSPOT,
///   severity = MAJOR,
///   minutesToFix = 60,
///   tags = { SUSPICIOUS },
///   scope = BSL
/// )
const PRIVILEGED_MODULE_METHOD_CALL: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 60,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// ProtectedModule diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 5,
///   tags = { BADPRACTICE, SUSPICIOUS },
///   modules = { SessionModule },
///   scope = BSL,
///   canLocateOnProject = true
/// )
const PROTECTED_MODULE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::SessionModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Suspicious],
    can_locate_on_project: true,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// PublicMethodsDescription diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = INFO,
///   minutesToFix = 1,
///   tags = { STANDARD, BRAINOVERLOAD, BADPRACTICE }
/// )
const PUBLIC_METHODS_DESCRIPTION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Brainoverload, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// QueryNestedFieldsByDot diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 10,
///   tags = { STANDARD, SQL, PERFORMANCE }
/// )
const QUERY_NESTED_FIELDS_BY_DOT: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// QueryParseError diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 5,
///   tags = { STANDARD, SQL, BADPRACTICE },
///   scope = BSL
/// )
const QUERY_PARSE_ERROR: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// QueryToMissingMetadata diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = BLOCKER,
///   scope = BSL,
///   minutesToFix = 5,
///   tags = { SUSPICIOUS, SQL }
/// )
const QUERY_TO_MISSING_METADATA: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Sql],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// RefOveruse diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = BSL,
///   minutesToFix = 5,
///   tags = { SQL, PERFORMANCE }
/// )
const REF_OVERUSE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UnionAll diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   minutesToFix = 5,
///   tags = { STANDARD, SQL, PERFORMANCE },
///   scope = BSL
/// )
const UNION_ALL: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UsingLikeInQuery diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 10,
///   tags = { SQL, UNPREDICTABLE },
///   scope = BSL,
///   activatedByDefault = false
/// )
const USING_LIKE_IN_QUERY: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// VirtualTableCallWithoutParameters diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = MAJOR,
///   minutesToFix = 5,
///   tags = { SQL, STANDARD, PERFORMANCE },
///   scope = BSL
/// )
const VIRTUAL_TABLE_CALL_WITHOUT_PARAMETERS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Standard, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// ScheduledJobHandler diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   minutesToFix = 5,
///   tags = { ERROR },
///   scope = BSL,
///   canLocateOnProject = true
/// )
const SCHEDULED_JOB_HANDLER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: true,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// ServerCallsInFormEvents diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = CRITICAL,
///   scope = BSL,
///   modules = { FormModule },
///   minutesToFix = 15,
///   tags = { DESIGN }
/// )
const SERVER_CALLS_IN_FORM_EVENTS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::FormModule],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// ServerSideExportFormMethod diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = ERROR,
///   severity = BLOCKER,
///   minutesToFix = 5,
///   tags = { ERROR, UNPREDICTABLE, SUSPICIOUS },
///   scope = BSL,
///   modules = { FormModule }
/// )
const SERVER_SIDE_EXPORT_FORM_METHOD: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::FormModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Unpredictable, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// SetPermissionsForNewObjects diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = VULNERABILITY,
///   severity = CRITICAL,
///   scope = BSL,
///   modules = { ManagedApplicationModule },
///   minutesToFix = 1,
///   tags = { STANDARD, BADPRACTICE, DESIGN }
/// )
const SET_PERMISSIONS_FOR_NEW_OBJECTS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::ManagedApplicationModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// SetPrivilegedMode diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = SECURITY_HOTSPOT,
///   severity = MAJOR,
///   minutesToFix = 1,
///   tags = { SUSPICIOUS },
///   scope = BSL
/// )
const SET_PRIVILEGED_MODE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// TransferringParametersBetweenClientAndServer diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 2,
///   tags = { BADPRACTICE, PERFORMANCE, STANDARD },
///   scope = BSL
/// )
const TRANSFERRING_PARAMETERS_BETWEEN_CLIENT_AND_SERVER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Performance, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

// ============================================================================
// Additional diagnostics (5 total)
// ============================================================================

/// DataExchangeLoading diagnostic metadata.
///
/// Ported from Java: DataExchangeLoadingDiagnostic.java
const DATA_EXCHANGE_LOADING: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::RecordSetModule,
        bsl_metadata::ModuleType::ValueManagerModule,
    ],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// ExecuteExternalCode diagnostic metadata.
///
/// Ported from Java: ExecuteExternalCodeDiagnostic.java
/// Note: HTTPServiceModule not available in Rust implementation
const EXECUTE_EXTERNAL_CODE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::CommandModule,
        bsl_metadata::ModuleType::ExternalConnectionModule,
        bsl_metadata::ModuleType::FormModule,
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::OrdinaryApplicationModule,
    ],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// ExecuteExternalCodeInCommonModule diagnostic metadata.
///
/// Ported from Java: ExecuteExternalCodeInCommonModuleDiagnostic.java
const EXECUTE_EXTERNAL_CODE_IN_COMMON_MODULE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// RedundantAccessToObject diagnostic metadata.
///
/// Ported from Java: RedundantAccessToObjectDiagnostic.java
const REDUNDANT_ACCESS_TO_OBJECT: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::CommonModule,
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::ManagerModule,
        bsl_metadata::ModuleType::FormModule,
        bsl_metadata::ModuleType::RecordSetModule,
    ],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Clumsy],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// SameMetadataObjectAndChildNames diagnostic metadata.
///
/// Ported from Java: SameMetadataObjectAndChildNamesDiagnostic.java
const SAME_METADATA_OBJECT_AND_CHILD_NAMES: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::ManagerModule,
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::SessionModule,
    ],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Design],
    can_locate_on_project: true,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UnusedLocalVariable diagnostic metadata.
///
/// Ported from Java: UnusedLocalVariableDiagnostic.java
const UNUSED_LOCAL_VARIABLE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[
        bsl_metadata::ModuleType::CommandModule,
        bsl_metadata::ModuleType::CommonModule,
        bsl_metadata::ModuleType::ManagerModule,
        bsl_metadata::ModuleType::ValueManagerModule,
        bsl_metadata::ModuleType::SessionModule,
        bsl_metadata::ModuleType::Unknown,
    ],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Badpractice, MetadataTag::Unused],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Typo diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = INFO,
///   minutesToFix = 1,
///   tags = { BADPRACTICE }
/// )
const TYPO: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UnsafeFindByCode diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   minutesToFix = 5,
///   tags = { DESIGN, SUSPICIOUS }
/// )
const UNSAFE_FIND_BY_CODE: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UsageWriteLogEvent diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = INFO,
///   minutesToFix = 1,
///   tags = { STANDARD, BADPRACTICE }
/// )
const USAGE_WRITE_LOG_EVENT: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UseSystemInformation diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = SECURITY_HOTSPOT,
///   severity = CRITICAL,
///   activatedByDefault = false,
///   minutesToFix = 5,
///   tags = { SUSPICIOUS }
/// )
const USE_SYSTEM_INFORMATION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const USING_CANCEL_PARAMETER: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UsingExternalCodeTools diagnostic metadata.
///
/// Ported from Java: UsingExternalCodeToolsDiagnostic.java
/// Detects usage of external code execution mechanisms (ExternalDataProcessors,
/// ExternalReports, ConfigurationExtensions) with Create/Connect methods.
const USING_EXTERNAL_CODE_TOOLS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const USING_FIND_ELEMENT_BY_STRING: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const USING_HARDCODE_NETWORK_ADDRESS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const USING_HARDCODE_PATH: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const USING_HARDCODE_SECRET_INFORMATION: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UsingModalWindows diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MAJOR,
///   scope = BSL,
///   minutesToFix = 15,
///   tags = { STANDARD },
///   compatibilityMode = COMPATIBILITY_MODE_8_3_3
/// )
const USING_MODAL_WINDOWS: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_3,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const USING_OBJECT_NOT_AVAILABLE_UNIX: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Lockinos],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const USING_SERVICE_TAG: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// UsingThisForm diagnostic metadata.
///
/// Java: @DiagnosticMetadata(
///   type = CODE_SMELL,
///   severity = MINOR,
///   scope = BSL,
///   modules = { FormModule },
///   minutesToFix = 1,
///   compatibilityMode = COMPATIBILITY_MODE_8_3_3,
///   tags = { STANDARD, DEPRECATED }
/// )
const USING_THIS_FORM: DiagnosticMetadata = DiagnosticMetadata {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_3,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ternary_operator_usage_metadata() {
        let meta = get_metadata(DiagnosticCode::TernaryOperatorUsage).unwrap();

        // Verify matches Java @DiagnosticMetadata
        assert_eq!(meta.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(meta.severity, DiagnosticSeverityLevel::Minor);
        assert_eq!(meta.minutes_to_fix, 3);
        assert!(!meta.activated_by_default); // Disabled by default
        assert_eq!(meta.tags, &[MetadataTag::Brainoverload]);

        // Verify severity mapping: CODE_SMELL + MINOR → Information
        assert_eq!(meta.calculate_severity(), crate::Severity::Information);
    }

    #[test]
    fn test_bad_words_metadata() {
        let meta = get_metadata(DiagnosticCode::BadWords).unwrap();

        assert_eq!(meta.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(meta.severity, DiagnosticSeverityLevel::Major);
        assert_eq!(meta.minutes_to_fix, 1);
        assert!(!meta.activated_by_default);
        assert_eq!(meta.tags, &[MetadataTag::Design]);

        // CODE_SMELL + MAJOR → Warning
        assert_eq!(meta.calculate_severity(), crate::Severity::Warning);
    }

    #[test]
    fn test_fields_from_joins_without_is_null_metadata() {
        let meta = get_metadata(DiagnosticCode::FieldsFromJoinsWithoutIsNull).unwrap();

        assert_eq!(meta.diagnostic_type, DiagnosticType::Error);
        assert_eq!(meta.severity, DiagnosticSeverityLevel::Critical);
        assert!(!meta.activated_by_default);
        assert_eq!(
            meta.tags,
            &[MetadataTag::Sql, MetadataTag::Suspicious, MetadataTag::Unpredictable]
        );

        // ERROR + CRITICAL → Critical
        assert_eq!(meta.calculate_severity(), crate::Severity::Critical);
    }

    #[test]
    fn test_file_system_access_metadata() {
        let meta = get_metadata(DiagnosticCode::FileSystemAccess).unwrap();

        assert_eq!(meta.diagnostic_type, DiagnosticType::Vulnerability);
        assert_eq!(meta.scope, DiagnosticScope::Bsl);
        assert!(!meta.activated_by_default);

        // VULNERABILITY + MAJOR → Major
        assert_eq!(meta.calculate_severity(), crate::Severity::Major);
    }

    #[test]
    fn test_function_name_starts_with_get_metadata() {
        let meta = get_metadata(DiagnosticCode::FunctionNameStartsWithGet).unwrap();

        assert_eq!(meta.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(meta.severity, DiagnosticSeverityLevel::Info);
        assert!(!meta.activated_by_default);

        // CODE_SMELL + INFO → Hint
        assert_eq!(meta.calculate_severity(), crate::Severity::Hint);
    }

    #[test]
    fn test_all_disabled_by_default_have_metadata() {
        let codes = [
            DiagnosticCode::BadWords,
            DiagnosticCode::CodeAfterAsyncCall,
            DiagnosticCode::DenyIncompleteValues,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            DiagnosticCode::FileSystemAccess,
            DiagnosticCode::FunctionNameStartsWithGet,
            DiagnosticCode::FunctionOutParameter,
            DiagnosticCode::InternetAccess,
            DiagnosticCode::MissingTempStorageDeletion,
            DiagnosticCode::TernaryOperatorUsage,
            DiagnosticCode::TooManyReturns,
        ];

        for code in codes {
            let meta = get_metadata(code).unwrap();
            assert!(!meta.activated_by_default, "{:?} should be disabled by default", code);
        }
    }

    #[test]
    fn test_deny_incomplete_values_metadata() {
        let meta = get_metadata(DiagnosticCode::DenyIncompleteValues).unwrap();

        assert_eq!(meta.scope, DiagnosticScope::Bsl);
        assert!(meta.can_locate_on_project);
        assert!(!meta.activated_by_default);
    }

    // ============================================================================
    // Comprehensive metadata test suite (Phase 5.3)
    // ============================================================================

    #[test]
    fn test_all_diagnostics_have_metadata() {
        use crate::DiagnosticCode;
        use strum::IntoEnumIterator;

        let mut missing = Vec::new();
        let mut count = 0;

        for code in DiagnosticCode::iter() {
            count += 1;
            if get_metadata(code).is_none() {
                missing.push(code);
            }
        }

        assert!(
            missing.is_empty(),
            "Found {} diagnostics without metadata (total {}): {:#?}",
            missing.len(),
            count,
            missing
        );
    }

    #[test]
    fn test_lsp_severity_mapping() {
        use crate::Severity;

        // Test ERROR + CRITICAL → Critical
        let data_exchange = get_metadata(DiagnosticCode::DataExchangeLoading).unwrap();
        assert_eq!(data_exchange.diagnostic_type, DiagnosticType::Error);
        assert_eq!(data_exchange.severity, DiagnosticSeverityLevel::Critical);
        assert_eq!(data_exchange.calculate_severity(), Severity::Critical);

        // Test VULNERABILITY + CRITICAL → Critical
        let execute_ext = get_metadata(DiagnosticCode::ExecuteExternalCode).unwrap();
        assert_eq!(execute_ext.diagnostic_type, DiagnosticType::Vulnerability);
        assert_eq!(execute_ext.severity, DiagnosticSeverityLevel::Critical);
        assert_eq!(execute_ext.calculate_severity(), Severity::Critical);

        // Test CODE_SMELL + INFO → Hint
        let redundant = get_metadata(DiagnosticCode::RedundantAccessToObject).unwrap();
        assert_eq!(redundant.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(redundant.severity, DiagnosticSeverityLevel::Info);
        assert_eq!(redundant.calculate_severity(), Severity::Hint);

        // Test CODE_SMELL + MINOR → Information
        let ternary = get_metadata(DiagnosticCode::TernaryOperatorUsage).unwrap();
        assert_eq!(ternary.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(ternary.severity, DiagnosticSeverityLevel::Minor);
        assert_eq!(ternary.calculate_severity(), Severity::Information);

        // Test CODE_SMELL + MAJOR → Warning
        let unused = get_metadata(DiagnosticCode::UnusedLocalVariable).unwrap();
        assert_eq!(unused.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(unused.severity, DiagnosticSeverityLevel::Major);
        assert_eq!(unused.calculate_severity(), Severity::Warning);

        // Test SECURITY_HOTSPOT → Warning
        let privileged = get_metadata(DiagnosticCode::SetPrivilegedMode).unwrap();
        assert_eq!(privileged.diagnostic_type, DiagnosticType::SecurityHotspot);
        assert_eq!(privileged.calculate_severity(), Severity::Warning);
    }

    #[test]
    fn test_tags_coverage() {
        // Verify key tags are used
        let bad_words = get_metadata(DiagnosticCode::BadWords).unwrap();
        assert!(bad_words.tags.contains(&MetadataTag::Design));

        let same_meta = get_metadata(DiagnosticCode::SameMetadataObjectAndChildNames).unwrap();
        assert!(same_meta.tags.contains(&MetadataTag::Standard));
        assert!(same_meta.tags.contains(&MetadataTag::Sql));
        assert!(same_meta.tags.contains(&MetadataTag::Design));

        let unused = get_metadata(DiagnosticCode::UnusedLocalVariable).unwrap();
        assert!(unused.tags.contains(&MetadataTag::Unused));
        assert!(unused.tags.contains(&MetadataTag::Brainoverload));
        assert!(unused.tags.contains(&MetadataTag::Badpractice));
    }

    #[test]
    fn test_activated_by_default_consistency() {
        // All DISABLED_BY_DEFAULT diagnostics should have activated_by_default = false
        let disabled_codes = [
            DiagnosticCode::BadWords,
            DiagnosticCode::CodeAfterAsyncCall,
            DiagnosticCode::DenyIncompleteValues,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            DiagnosticCode::FileSystemAccess,
            DiagnosticCode::FunctionNameStartsWithGet,
            DiagnosticCode::FunctionOutParameter,
            DiagnosticCode::InternetAccess,
            DiagnosticCode::MissingTempStorageDeletion,
            DiagnosticCode::TernaryOperatorUsage,
            DiagnosticCode::TooManyReturns,
        ];

        for code in disabled_codes {
            let meta = get_metadata(code).unwrap();
            assert!(!meta.activated_by_default, "{:?} should be disabled by default", code);
        }

        // Recently added diagnostics should be enabled by default
        let enabled_codes = [
            DiagnosticCode::DataExchangeLoading,
            DiagnosticCode::ExecuteExternalCode,
            DiagnosticCode::RedundantAccessToObject,
            DiagnosticCode::SameMetadataObjectAndChildNames,
            DiagnosticCode::UnusedLocalVariable,
        ];

        for code in enabled_codes {
            let meta = get_metadata(code).unwrap();
            assert!(meta.activated_by_default, "{:?} should be enabled by default", code);
        }
    }

    #[test]
    fn test_scope_consistency() {
        // Test BSL scope
        let data_exchange = get_metadata(DiagnosticCode::DataExchangeLoading).unwrap();
        assert_eq!(data_exchange.scope, DiagnosticScope::Bsl);

        // Test All scope (BSL + OneScript)
        let unused = get_metadata(DiagnosticCode::UnusedLocalVariable).unwrap();
        assert_eq!(unused.scope, DiagnosticScope::All);
    }

    #[test]
    fn test_can_locate_on_project() {
        // SameMetadataObjectAndChildNames should support project-level location
        let same_meta = get_metadata(DiagnosticCode::SameMetadataObjectAndChildNames).unwrap();
        assert!(same_meta.can_locate_on_project);

        // Most diagnostics don't support project-level location
        let unused = get_metadata(DiagnosticCode::UnusedLocalVariable).unwrap();
        assert!(!unused.can_locate_on_project);
    }

    #[test]
    fn test_minutes_to_fix_reasonable() {
        use strum::IntoEnumIterator;

        for code in DiagnosticCode::iter() {
            if let Some(meta) = get_metadata(code) {
                // minutes_to_fix should be reasonable (1-60 minutes)
                assert!(
                    meta.minutes_to_fix >= 1 && meta.minutes_to_fix <= 60,
                    "{:?} has unreasonable minutes_to_fix: {}",
                    code,
                    meta.minutes_to_fix
                );
            }
        }
    }

    #[test]
    fn test_new_diagnostics_metadata() {
        // Test DataExchangeLoading
        let data_exchange = get_metadata(DiagnosticCode::DataExchangeLoading).unwrap();
        assert_eq!(data_exchange.diagnostic_type, DiagnosticType::Error);
        assert_eq!(data_exchange.severity, DiagnosticSeverityLevel::Critical);
        assert_eq!(data_exchange.minutes_to_fix, 5);
        assert!(data_exchange.tags.contains(&MetadataTag::Standard));

        // Test ExecuteExternalCode
        let execute_ext = get_metadata(DiagnosticCode::ExecuteExternalCode).unwrap();
        assert_eq!(execute_ext.diagnostic_type, DiagnosticType::Vulnerability);
        assert_eq!(execute_ext.severity, DiagnosticSeverityLevel::Critical);
        assert!(execute_ext.tags.contains(&MetadataTag::Error));

        // Test RedundantAccessToObject
        let redundant = get_metadata(DiagnosticCode::RedundantAccessToObject).unwrap();
        assert_eq!(redundant.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(redundant.severity, DiagnosticSeverityLevel::Info);
        assert!(redundant.tags.contains(&MetadataTag::Clumsy));

        // Test SameMetadataObjectAndChildNames
        let same_meta = get_metadata(DiagnosticCode::SameMetadataObjectAndChildNames).unwrap();
        assert_eq!(same_meta.diagnostic_type, DiagnosticType::Error);
        assert_eq!(same_meta.minutes_to_fix, 30);
        assert!(same_meta.can_locate_on_project);

        // Test UnusedLocalVariable
        let unused = get_metadata(DiagnosticCode::UnusedLocalVariable).unwrap();
        assert_eq!(unused.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(unused.severity, DiagnosticSeverityLevel::Major);
        assert!(unused.tags.contains(&MetadataTag::Unused));
    }
}
