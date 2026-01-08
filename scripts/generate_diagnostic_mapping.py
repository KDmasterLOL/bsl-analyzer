#!/usr/bin/env python3
"""Generate complete diagnostic mapping: Java → Rust implementations"""

import os
import re

RUST_DIR = "~/src/lsp/bsl-language-server-rust/crates/bsl-diagnostics/src/rules"

# Get all Rust diagnostic files
rust_files = {}
for f in os.listdir(RUST_DIR):
    if f.endswith('.rs') and f not in ['mod.rs', 'test_helpers.rs']:
        name = f[:-3]  # Remove .rs
        rust_files[name] = f

# All Java diagnostics in alphabetical order
java_diagnostics = [
    "AllFunctionPathMustHaveReturn",
    "AssignAliasFieldsInQuery",
    "BadWords",
    "BeginTransactionBeforeTryCatch",
    "CachedPublic",
    "CanonicalSpellingKeywords",
    "CodeAfterAsyncCall",
    "CodeBlockBeforeSub",
    "CodeOutOfRegion",
    "CognitiveComplexity",
    "CommandModuleExportMethods",
    "CommentedCode",
    "CommitTransactionOutsideTryCatch",
    "CommonModuleAssign",
    "CommonModuleInvalidType",
    "CommonModuleMissingAPI",
    "CommonModuleNameCached",
    "CommonModuleNameClient",
    "CommonModuleNameClientServer",
    "CommonModuleNameFullAccess",
    "CommonModuleNameGlobal",
    "CommonModuleNameGlobalClient",
    "CommonModuleNameServerCall",
    "CommonModuleNameWords",
    "CompilationDirectiveLost",
    "CompilationDirectiveNeedLess",
    "ConsecutiveEmptyLines",
    "CrazyMultilineString",
    "CreateQueryInCycle",
    "CyclomaticComplexity",
    "DataExchangeLoading",
    "DeletingCollectionItem",
    "DenyIncompleteValues",
    "DeprecatedAttributes8312",
    "DeprecatedCurrentDate",
    "DeprecatedFind",
    "DeprecatedMessage",
    "DeprecatedMethodCall",
    "DeprecatedMethods8310",
    "DeprecatedMethods8317",
    "DeprecatedTypeManagedForm",
    "DisableSafeMode",
    "DoubleNegatives",
    "DuplicateRegion",
    "DuplicateStringLiteral",
    "DuplicatedInsertionIntoCollection",
    "EmptyCodeBlock",
    "EmptyRegion",
    "EmptyStatement",
    "ExcessiveAutoTestCheck",
    "ExecuteExternalCode",
    "ExecuteExternalCodeInCommonModule",
    "ExportVariables",
    "ExternalAppStarting",
    "ExtraCommas",
    "FieldsFromJoinsWithoutIsNull",
    "FileSystemAccess",
    "ForbiddenMetadataName",
    "FormDataToValue",
    "FullOuterJoinQuery",
    "FunctionNameStartsWithGet",
    "FunctionOutParameter",
    "FunctionReturnsSamePrimitive",
    "FunctionShouldHaveReturn",
    "GetFormMethod",
    "GlobalContextMethodCollision8312",
    "IdenticalExpressions",
    "IfConditionComplexity",
    "IfElseDuplicatedCodeBlock",
    "IfElseDuplicatedCondition",
    "IfElseIfEndsWithElse",
    "IncorrectLineBreak",
    "IncorrectUseLikeInQuery",
    "IncorrectUseOfStrTemplate",
    "InternetAccess",
    "InvalidCharacterInFile",
    "IsInRoleMethod",
    "JoinWithSubQuery",
    "JoinWithVirtualTable",
    "LatinAndCyrillicSymbolInWord",
    "LineLength",
    "LogicalOrInJoinQuerySection",
    "LogicalOrInTheWhereSectionOfQuery",
    "MagicDate",
    "MagicNumber",
    "MetadataObjectNameLength",
    "MethodSize",
    "MissedRequiredParameter",
    "MissingCodeTryCatchEx",
    "MissingCommonModuleMethod",
    "MissingEventSubscriptionHandler",
    "MissingParameterDescription",
    "MissingReturnedValueDescription",
    "MissingSpace",
    "MissingTemporaryFileDeletion",
    "MissingTempStorageDeletion",
    "MissingVariablesDescription",
    "MultilineStringInQuery",
    "MultilingualStringHasAllDeclaredLanguages",
    "MultilingualStringUsingWithTemplate",
    "NestedConstructorsInStructureDeclaration",
    "NestedFunctionInParameters",
    "NestedStatements",
    "NestedTernaryOperator",
    "NonExportMethodsInApiRegion",
    "NonStandardRegion",
    "NumberOfOptionalParams",
    "NumberOfParams",
    "NumberOfValuesInStructureConstructor",
    "OneStatementPerLine",
    "OrderOfParams",
    "OrdinaryAppSupport",
    "OSUsersMethod",
    "PairingBrokenTransaction",
    "ParseError",
    "PrivilegedModuleMethodCall",
    "ProcedureReturnsValue",
    "ProtectedModule",
    "PublicMethodsDescription",
    "QueryNestedFieldsByDot",
    "QueryParseError",
    "QueryToMissingMetadata",
    "RedundantAccessToObject",
    "RefOveruse",
    "ReservedParameterNames",
    "RewriteMethodParameter",
    "SameMetadataObjectAndChildNames",
    "ScheduledJobHandler",
    "SelectTopWithoutOrderBy",
    "SelfAssign",
    "SelfInsertion",
    "SemicolonPresence",
    "ServerCallsInFormEvents",
    "ServerSideExportFormMethod",
    "SetPermissionsForNewObjects",
    "SetPrivilegedMode",
    "SeveralCompilerDirectives",
    "SpaceAtStartComment",
    "StyleElementConstructors",
    "TempFilesDir",
    "TernaryOperatorUsage",
    "ThisObjectAssign",
    "TimeoutsInExternalResources",
    "TooManyReturns",
    "TransferringParametersBetweenClientAndServer",
    "TryNumber",
    "Typo",
    "UnaryPlusInConcatenation",
    "UnionAll",
    "UnknownPreprocessorSymbol",
    "UnreachableCode",
    "UnsafeFindByCode",
    "UnsafeSafeModeMethodCall",
    "UnusedLocalMethod",
    "UnusedLocalVariable",
    "UnusedParameters",
    "UsageWriteLogEvent",
    "UseLessForEach",
    "UselessTernaryOperator",
    "UseSystemInformation",
    "UsingCancelParameter",
    "UsingExternalCodeTools",
    "UsingFindElementByString",
    "UsingGoto",
    "UsingHardcodeNetworkAddress",
    "UsingHardcodePath",
    "UsingHardcodeSecretInformation",
    "UsingLikeInQuery",
    "UsingModalWindows",
    "UsingObjectNotAvailableUnix",
    "UsingServiceTag",
    "UsingSynchronousCalls",
    "UsingThisForm",
    "VirtualTableCallWithoutParameters",
    "WrongDataPathForFormElements",
    "WrongHttpServiceHandler",
    "WrongUseFunctionProceedWithCall",
    "WrongUseOfRollbackTransactionMethod",
    "WrongWebServiceHandler",
    "YoLetterUsage",
]

def to_snake_case(name):
    """Convert PascalCase to snake_case"""
    s1 = re.sub('(.)([A-Z][a-z]+)', r'\1_\2', name)
    return re.sub('([a-z0-9])([A-Z])', r'\1_\2', s1).lower()

# Generate mapping
print("# Diagnostic Mapping: Java → Rust")
print()
print("| # | Java Diagnostic | Rust File | Status |")
print("|---|-----------------|-----------|--------|")

found = 0
missing = 0

for idx, java_diag in enumerate(java_diagnostics, 1):
    snake = to_snake_case(java_diag)

    if snake in rust_files:
        print(f"| {idx} | {java_diag} | [`{snake}.rs`]({RUST_DIR}/{snake}.rs) | ✅ **Exists** |")
        found += 1
    else:
        print(f"| {idx} | {java_diag} | - | ❌ **Missing** |")
        missing += 1

print()
print(f"**Summary:**")
print(f"- ✅ **Found:** {found} ({found/len(java_diagnostics)*100:.1f}%)")
print(f"- ❌ **Missing:** {missing} ({missing/len(java_diagnostics)*100:.1f}%)")
print(f"- Total Rust diagnostic files: {len(rust_files)}")
