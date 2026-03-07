#!/bin/bash
#
# Profile all diagnostics on a project using streaming mode
# Usage: ./scripts/profile-diagnostics.sh /path/to/project
#
# Streaming mode provides accurate per-diagnostic timing without Salsa overhead.
#

set -e

PROJECT_DIR="${1:-$HOME/src/doc3}"
ANALYZER="$(dirname "$0")/../target/release/bsl-analyzer"
OUTPUT_FILE="diagnostic-profiling-$(date +%Y%m%d-%H%M%S).csv"

# Check if analyzer exists
if [[ ! -x "$ANALYZER" ]]; then
    echo "Error: bsl-analyzer not found at $ANALYZER"
    echo "Run: cargo build --release"
    exit 1
fi

# Check if project exists
if [[ ! -d "$PROJECT_DIR" ]]; then
    echo "Error: Project directory not found: $PROJECT_DIR"
    exit 1
fi

# List of all diagnostics
DIAGNOSTICS=(
    AllFunctionPathMustHaveReturn
    AssignAliasFieldsInQuery
    BadWords
    BeginTransactionBeforeTryCatch
    CachedPublic
    CanonicalSpellingKeywords
    CodeAfterAsyncCall
    CodeBlockBeforeSub
    CodeOutOfRegion
    CognitiveComplexity
    CommandModuleExportMethods
    CommentedCode
    CommitTransactionOutsideTryCatch
    CommonModuleAssign
    CommonModuleInvalidType
    CommonModuleMissingAPI
    CommonModuleNameCached
    CommonModuleNameClient
    CommonModuleNameClientServer
    CommonModuleNameFullAccess
    CommonModuleNameGlobal
    CommonModuleNameGlobalClient
    CommonModuleNameServerCall
    CommonModuleNameWords
    CompilationDirectiveLost
    CompilationDirectiveNeedLess
    ConsecutiveEmptyLines
    CreateQueryInCycle
    CyclomaticComplexity
    DataExchangeLoading
    DeletingCollectionItem
    DenyIncompleteValues
    DeprecatedCurrentDate
    DeprecatedFind
    DeprecatedMessage
    DeprecatedMethodCall
    DeprecatedTypeManagedForm
    DisableSafeMode
    DoubleNegatives
    DuplicatedInsertionIntoCollection
    DuplicateRegion
    DuplicateStringLiteral
    EmptyCodeBlock
    EmptyRegion
    EmptyStatement
    ExcessiveAutoTestCheck
    ExecuteExternalCode
    ExecuteExternalCodeInCommonModule
    ExportVariables
    ExternalAppStarting
    ExtraCommas
    FieldsFromJoinsWithoutIsNull
    FileSystemAccess
    ForbiddenMetadataName
    FormDataToValue
    FullOuterJoinQuery
    FunctionNameStartsWithGet
    FunctionOutParameter
    FunctionReturnsSamePrimitive
    FunctionShouldHaveReturn
    GetFormMethod
    IdenticalExpressions
    IfConditionComplexity
    IfElseDuplicatedCodeBlock
    IfElseDuplicatedCondition
    IfElseIfEndsWithElse
    IncorrectLineBreak
    IncorrectUseLikeInQuery
    IncorrectUseOfStrTemplate
    InternetAccess
    InvalidCharacterInFile
    IsInRoleMethod
    JoinWithSubQuery
    JoinWithVirtualTable
    LatinAndCyrillicSymbolInWord
    LineLength
    LogicalOrInJoinQuerySection
    LogicalOrInTheWhereSectionOfQuery
    MagicDate
    MagicNumber
    MetadataObjectNameLength
    MethodSize
    MissedRequiredParameter
    MissingCodeTryCatchEx
    MissingCommonModuleMethod
    MissingEventSubscriptionHandler
    MissingParameterDescription
    MissingReturnedValueDescription
    MissingSpace
    MissingTemporaryFileDeletion
    MissingTempStorageDeletion
    MissingVariablesDescription
    MultilineStringInQuery
    MultilingualStringHasAllDeclaredLanguages
    MultilingualStringUsingWithTemplate
    NestedConstructorsInStructureDeclaration
    NestedFunctionInParameters
    NestedStatements
    NestedTernaryOperator
    NonExportMethodsInApiRegion
    NonStandardRegion
    NumberOfOptionalParams
    NumberOfParams
    NumberOfValuesInStructureConstructor
    OneStatementPerLine
    OrderOfParams
    OrdinaryAppSupport
    OSUsersMethod
    PairingBrokenTransaction
    ParseError
    PrivilegedModuleMethodCall
    ProcedureReturnsValue
    ProtectedModule
    PublicMethodsDescription
    QueryNestedFieldsByDot
    QueryParseError
    QueryToMissingMetadata
    RedundantAccessToObject
    RefOveruse
    ReservedParameterNames
    RewriteMethodParameter
    SameMetadataObjectAndChildNames
    ScheduledJobHandler
    SelectTopWithoutOrderBy
    SelfAssign
    SelfInsertion
    SemicolonPresence
    ServerCallsInFormEvents
    ServerSideExportFormMethod
    SetPermissionsForNewObjects
    SetPrivilegedMode
    SeveralCompilerDirectives
    SpaceAtStartComment
    StyleElementConstructors
    TempFilesDir
    TernaryOperatorUsage
    ThisObjectAssign
    TimeoutsInExternalResources
    TooManyReturns
    TransferringParametersBetweenClientAndServer
    TryNumber
    Typo
    UnaryPlusInConcatenation
    UnionAll
    UnknownPreprocessorSymbol
    UnreachableCode
    UnsafeFindByCode
    UnsafeSafeModeMethodCall
    UnusedLocalMethod
    UnusedLocalVariable
    UnusedParameters
    UsageWriteLogEvent
    UseLessForEach
    UselessTernaryOperator
    UseSystemInformation
    UsingCancelParameter
    UsingExternalCodeTools
    UsingFindElementByString
    UsingGoto
    UsingHardcodeNetworkAddress
    UsingHardcodePath
    UsingHardcodeSecretInformation
    UsingLikeInQuery
    UsingModalWindows
    UsingObjectNotAvailableUnix
    UsingServiceTag
    UsingSynchronousCalls
    UsingThisForm
    VirtualTableCallWithoutParameters
    WrongDataPathForFormElements
    WrongHttpServiceHandler
    WrongUseFunctionProceedWithCall
    WrongUseOfRollbackTransactionMethod
    WrongWebServiceHandler
    YoLetterUsage
)

TOTAL=${#DIAGNOSTICS[@]}
echo "Profiling $TOTAL diagnostics on: $PROJECT_DIR"
echo "Mode: streaming (accurate per-diagnostic timing)"
echo "Output: $OUTPUT_FILE"
echo ""

# CSV header
echo "Diagnostic,TotalMs,AverageMs,MaxMs,MaxFile,DiagnosticCount" > "$OUTPUT_FILE"

count=0
for diag in "${DIAGNOSTICS[@]}"; do
    count=$((count + 1))
    printf "\r[%3d/%d] %-50s" "$count" "$TOTAL" "$diag"

    # Run analyzer in streaming mode and capture output
    # Note: don't use -q so we get "Diagnostics: N" in output
    output=$("$ANALYZER" analyze -s "$PROJECT_DIR" --streaming --only-diagnostic "$diag" 2>/dev/null || true)

    # Parse profiling output
    total_ms=$(echo "$output" | grep "Total time:" | sed 's/.*: *\([0-9.]*\)ms/\1/' || echo "0")
    avg_ms=$(echo "$output" | grep "Average time:" | sed 's/.*: *\([0-9.]*\)ms/\1/' || echo "0")
    max_ms=$(echo "$output" | grep "Max time:" | sed 's/.*: *\([0-9.]*\)ms/\1/' || echo "0")
    max_file=$(echo "$output" | grep "Max file:" | sed 's/.*: *//' || echo "")
    diag_count=$(echo "$output" | grep "^Diagnostics:" | sed 's/.*: *//' || echo "0")

    # Handle empty values
    [[ -z "$total_ms" ]] && total_ms="0"
    [[ -z "$avg_ms" ]] && avg_ms="0"
    [[ -z "$max_ms" ]] && max_ms="0"
    [[ -z "$diag_count" ]] && diag_count="0"

    # Write to CSV
    echo "$diag,$total_ms,$avg_ms,$max_ms,\"$max_file\",$diag_count" >> "$OUTPUT_FILE"
done

echo ""
echo ""
echo "Done! Results saved to: $OUTPUT_FILE"
echo ""

# Show top 10 slowest by total time
echo "=== Top 10 Slowest Diagnostics (by Total Time) ==="
tail -n +2 "$OUTPUT_FILE" | sort -t',' -k2 -rn | head -10 | while IFS=',' read -r name total avg max file count; do
    printf "%-45s %10s ms  (avg: %s ms, count: %s)\n" "$name" "$total" "$avg" "$count"
done

echo ""
echo "=== Top 10 Slowest Diagnostics (by Max Time) ==="
tail -n +2 "$OUTPUT_FILE" | sort -t',' -k4 -rn | head -10 | while IFS=',' read -r name total avg max file count; do
    printf "%-45s %10s ms  file: %s\n" "$name" "$max" "$file"
done
