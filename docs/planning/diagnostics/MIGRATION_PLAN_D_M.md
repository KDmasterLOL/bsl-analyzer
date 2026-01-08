# Diagnostics Migration Plan: D-M (Part 2/4)

## Overview

This document contains the migration plan for diagnostics starting with letters **D through M** (45 diagnostics).

Continuing alphabetical migration with infrastructure improvements as needed.

---

## Diagnostics: D-M

### 31. DataExchangeLoading

**Code:** `DataExchangeLoading`
**Russian:** Отсутствует проверка ОбменДанными.Загрузка
**Type:** `ERROR`
**Severity:** `CRITICAL`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes
**Minutes to fix:** 5
**Tags:** `STANDARD`, `BADPRACTICE`, `UNPREDICTABLE`
**Scope:** `BSL`

**Sources:**

- **Java:** `~/src/lsp/bsl-language-server/src/main/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/DataExchangeLoadingDiagnostic.java`
- **Rust Reference:** ✅ `~/src/lsp/bsl-language-server-rust/crates/bsl-diagnostics/src/rules/data_exchange_loading.rs`
- **Target:** `~/src/lsp/bsl-analyzer/crates/ide-diagnostics/src/handlers/data_exchange_loading.rs`

**Infrastructure Requirements:**

- ✅ Parser, AST
- Event handler detection (OnWrite, BeforeWrite, etc.)
- DataExchange.Load check pattern

**Test Files:** `~/src/lsp/bsl-language-server/src/test/.../DataExchangeLoadingDiagnostic*`

**Notes:** Check event handlers for missing `DataExchange.Load` check

---

### 32. DeletingCollectionItem

**Code:** `DeletingCollectionItem`
**Russian:** Удаление элемента коллекции в цикле обхода этой коллекции
**Type:** `ERROR`
**Severity:** `MAJOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes
**Minutes to fix:** 5
**Tags:** `STANDARD`, `ERROR`
**Scope:** `ALL`

**Sources:**

- **Java:** `~/src/lsp/bsl-language-server/src/main/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/DenyIncompleteValuesDiagnostic.java`
- **Target:** `~/src/lsp/bsl-analyzer/crates/ide-diagnostics/src/handlers/deny_incomplete_values.rs`

**Infrastructure Requirements:**

- ✅ Parser, AST
- ForEach loop analysis
- Collection mutation detection (Delete, Remove)

**Test Files:** `~/src/lsp/bsl-language-server/src/test/.../DeletingCollectionItemDiagnostic*`

**Notes:** Detect `Delete`/`Remove` calls inside ForEach over same collection

---

### 33. DenyIncompleteValues

**Code:** `DenyIncompleteValues`
**Russian:** Запрет незаполненных значений измерений
**Type:** `CODE_SMELL`
**Severity:** `MAJOR`
**Enabled by default:** ❌ **No**
**Configurable:** ✅ Yes
**Minutes to fix:** 1
**Tags:** `BADPRACTICE`
**Scope:** `BSL`

**Infrastructure Requirements:**

- ⚠️ Metadata (InformationRegister dimensions)
- Dimension completeness check

**Test Files:** `~/src/lsp/bsl-language-server/src/test/.../DenyIncompleteValuesDiagnostic*`

**Notes:** **Defer to Iteration 19** (requires metadata)

---

### 34-38. Deprecated\* Diagnostics (7 diagnostics)

#### 34. DeprecatedAttributes8312

- **Severity:** `INFO`
- **Tags:** `DEPRECATED`
- **Requires:** AST + platform version context
- **Notes:** Detect usage of deprecated 8.3.12 attributes

#### 35. DeprecatedCurrentDate

- **Severity:** `MAJOR` (ERROR)
- **Tags:** `STANDARD`, `DEPRECATED`, `UNPREDICTABLE`
- **Requires:** AST (method call detection)
- **Notes:** Detect `ТекущаяДата()`/`CurrentDate()` usage in server modules

#### 36. DeprecatedFind

- **Severity:** `MINOR`
- **Tags:** `DEPRECATED`
- **Requires:** AST (method call detection)
- **Notes:** Detect deprecated `Найти()`/`Find()` method

#### 37. DeprecatedMessage

- **Severity:** `MINOR`
- **Tags:** `STANDARD`, `DEPRECATED`
- **Requires:** AST (method call detection)
- **Notes:** Detect `Сообщить()`/`Message()` usage

#### 38. DeprecatedMethodCall

- **Severity:** `MINOR`
- **Tags:** `DEPRECATED`, `DESIGN`
- **Requires:** ⚠️ **Type system** (method signature lookup)
- **Notes:** Generic deprecated method detector - **defer to Iteration 16**

#### 39. DeprecatedMethods8310

- **Severity:** `INFO`
- **Tags:** `DEPRECATED`
- **Requires:** AST + platform version
- **Notes:** Detect deprecated 8.3.10 client application methods

#### 40. DeprecatedMethods8317

- **Severity:** `INFO`
- **Tags:** `DEPRECATED`
- **Requires:** AST + platform version
- **Notes:** Detect deprecated 8.3.17 global methods

#### 41. DeprecatedTypeManagedForm

- **Severity:** `INFO`
- **Tags:** `STANDARD`, `DEPRECATED`
- **Requires:** ⚠️ Type system
- **Notes:** Detect `УправляемаяФорма`/`ManagedForm` type usage - **defer to Iteration 16**

**Implementation Strategy:**

- Implement simple method call checks first (35-37, 39-40)
- Defer type-dependent diagnostics (38, 41) to Iteration 16

---

### 42. DisableSafeMode

**Code:** `DisableSafeMode`
**Russian:** Отключение безопасного режима
**Type:** `VULNERABILITY`
**Severity:** `MAJOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes
**Minutes to fix:** 15
**Tags:** `SUSPICIOUS`
**Scope:** `BSL`

**Infrastructure Requirements:**

- ✅ Parser, AST
- Method call detection (`ОтключитьБезопасныйРежим`/`DisableSafeMode`)

**Test Files:** `~/src/lsp/bsl-language-server/src/test/.../DisableSafeModeDiagnostic*`

**Notes:** Security diagnostic - flag any DisableSafeMode calls

---

### 43. DoubleNegatives

**Code:** `DoubleNegatives`
**Russian:** Двойные отрицания
**Type:** `CODE_SMELL`
**Severity:** `MAJOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes
**Minutes to fix:** 3
**Tags:** `BRAINOVERLOAD`, `BADPRACTICE`
**Scope:** `ALL`

**Infrastructure Requirements:**

- ✅ Parser, AST
- Expression analysis (detect `Not Not`, `<> False`, etc.)

**Test Files:** `~/src/lsp/bsl-language-server/src/test/.../DoubleNegativesDiagnostic*`

**Notes:** Detect double negation patterns: `Not Not X`, `X <> False`, `Not (X = False)`

---

### 44-46. Duplicate\* Diagnostics

#### 44. DuplicateRegion

- **Severity:** `INFO`
- **Tags:** `STANDARD`
- **Requires:** Region name tracking
- **Notes:** Detect duplicate `#Область` names in module

#### 45. DuplicateStringLiteral

- **Severity:** `MINOR`
- **Tags:** `BADPRACTICE`
- **Configuration:** `minLength`, `threshold` parameters
- **Requires:** Cross-file string literal collection
- **Notes:** Find repeated string literals (suggest constants)

#### 46. DuplicatedInsertionIntoCollection

- **Severity:** `MAJOR`
- **Tags:** `BRAINOVERLOAD`, `SUSPICIOUS`, `BADPRACTICE`
- **Requires:** ⚠️ Data flow analysis
- **Notes:** Detect same value inserted multiple times - **defer to Iteration 17**

---

### 47-49. Empty\* Diagnostics (3 simple diagnostics)

#### 47. EmptyCodeBlock

- **Severity:** `MAJOR`
- **Tags:** `BADPRACTICE`, `SUSPICIOUS`
- **Requires:** AST (detect empty if/for/while bodies)
- **Notes:** **Good early diagnostic**

#### 48. EmptyRegion

- **Severity:** `INFO`
- **Tags:** `STANDARD`
- **Requires:** Region content checking
- **Notes:** **Good early diagnostic**

#### 49. EmptyStatement

- **Severity:** `INFO`
- **Tags:** `BADPRACTICE`
- **Requires:** AST (detect lone semicolons)
- **Notes:** **Good early diagnostic**

**Implementation:** All three are simple AST checks - implement early

---

### 50-52. Execute/Export/External

#### 50. ExcessiveAutoTestCheck

- **Severity:** `MINOR`
- **Tags:** `STANDARD`, `DEPRECATED`
- **Requires:** AST (parameter check pattern)
- **Notes:** Detect excessive `АвтоТест` parameter checks

#### 51. ExecuteExternalCode

- **Severity:** `CRITICAL` (VULNERABILITY)
- **Tags:** `ERROR`, `STANDARD`
- **Requires:** AST (Execute/Eval call detection)
- **Notes:** Security diagnostic - detect `Execute()`, `Eval()` calls

#### 52. ExecuteExternalCodeInCommonModule

- **Severity:** `CRITICAL` (SECURITY_HOTSPOT)
- **Tags:** `BADPRACTICE`, `STANDARD`
- **Requires:** ⚠️ Metadata (module type) + Execute detection
- **Notes:** **Defer to Iteration 19**

#### 53. ExportVariables

- **Severity:** `MAJOR`
- **Tags:** `STANDARD`, `DESIGN`, `UNPREDICTABLE`
- **Requires:** AST (module-level export variable detection)
- **Notes:** Check for `Экспорт`/`Export` variables in global modules

#### 54. ExternalAppStarting

- **Severity:** `MAJOR` (SECURITY_HOTSPOT)
- **Tags:** `SUSPICIOUS`
- **Requires:** AST (RunApp/Shell call detection)
- **Notes:** Security diagnostic

#### 55. ExtraCommas

- **Severity:** `MAJOR`
- **Tags:** `STANDARD`, `BADPRACTICE`
- **Requires:** AST (method call argument analysis)
- **Notes:** Detect missing arguments between commas (`Func(A, , B)`)

---

### 56-58. Field/File/Forbidden

#### 56. FieldsFromJoinsWithoutIsNull

- **Severity:** `CRITICAL` (ERROR)
- **Enabled by default:** ❌ **No**
- **Tags:** `SQL`, `SUSPICIOUS`, `UNPREDICTABLE`
- **Requires:** ⚠️ SDBL AST + semantic analysis
- **Notes:** **Defer until SDBL semantic analysis**

#### 57. FileSystemAccess

- **Severity:** `MAJOR` (VULNERABILITY)
- **Enabled by default:** ❌ **No**
- **Tags:** `SUSPICIOUS`
- **Requires:** AST (file operation detection)
- **Notes:** Security diagnostic - detect file I/O calls

#### 58. ForbiddenMetadataName

- **Severity:** `BLOCKER` (ERROR)
- **Tags:** `STANDARD`, `SQL`, `DESIGN`
- **Requires:** ⚠️ Metadata (object name validation)
- **Notes:** **Defer to Iteration 19**

---

### 59-64. Form/Function Diagnostics

#### 59. FormDataToValue

- **Severity:** `INFO`
- **Tags:** `BADPRACTICE`
- **Requires:** AST (method call detection)
- **Notes:** Detect `ДанныеФормыВЗначение()` usage

#### 60. FullOuterJoinQuery

- **Severity:** `MAJOR`
- **Tags:** `SQL`, `STANDARD`, `PERFORMANCE`
- **Requires:** ⚠️ SDBL AST
- **Notes:** Detect FULL OUTER JOIN - **defer until SDBL AST complete**

#### 61. FunctionNameStartsWithGet

- **Severity:** `INFO`
- **Enabled by default:** ❌ **No**
- **Tags:** `STANDARD`
- **Requires:** AST (function name pattern)
- **Notes:** Check function names starting with "Получить"/"Get"

#### 62. FunctionOutParameter

- **Severity:** `MAJOR`
- **Enabled by default:** ❌ **No**
- **Tags:** `DESIGN`
- **Requires:** ⚠️ Data flow (output parameter detection)
- **Notes:** **Defer to Iteration 17**

#### 63. FunctionReturnsSamePrimitive

- **Severity:** `MAJOR` (ERROR)
- **Tags:** `DESIGN`, `BADPRACTICE`
- **Requires:** AST (return statement analysis)
- **Notes:** Detect function always returning same literal

#### 64. FunctionShouldHaveReturn

- **Severity:** `MAJOR` (ERROR)
- **Tags:** `SUSPICIOUS`, `UNPREDICTABLE`
- **Requires:** AST (return statement check)
- **Notes:** **Good early diagnostic**

---

### 65-66. Get/Global

#### 65. GetFormMethod

- **Severity:** `MAJOR` (ERROR)
- **Tags:** `ERROR`
- **Requires:** AST (method call detection)
- **Notes:** Detect deprecated `ПолучитьФорму()` calls

#### 66. GlobalContextMethodCollision8312

- **Severity:** `BLOCKER` (ERROR)
- **Tags:** `ERROR`, `UNPREDICTABLE`
- **Requires:** Global context method list + symbol table
- **Notes:** Check for method names colliding with platform 8.3.12

---

### 67-71. Identical/If/Incorrect

#### 67. IdenticalExpressions

- **Severity:** `MAJOR` (ERROR)
- **Tags:** `SUSPICIOUS`
- **Requires:** AST (expression comparison)
- **Notes:** Detect `X == X`, `A And A`, etc. - **good early diagnostic**

#### 68. IfConditionComplexity

- **Severity:** `MINOR`
- **Tags:** `BRAINOVERLOAD`
- **Configuration:** `maxIfConditionComplexity` parameter
- **Requires:** AST (condition complexity counter)
- **Notes:** Count boolean operators in if condition

#### 69. IfElseDuplicatedCodeBlock

- **Severity:** `MINOR`
- **Tags:** `SUSPICIOUS`
- **Requires:** AST (code block comparison)
- **Notes:** Detect identical if/else branches

#### 70. IfElseDuplicatedCondition

- **Severity:** `MAJOR`
- **Tags:** `SUSPICIOUS`
- **Requires:** AST (condition comparison)
- **Notes:** Detect duplicate conditions in if/elseif chain

#### 71. IfElseIfEndsWithElse

- **Severity:** `MAJOR`
- **Tags:** `BADPRACTICE`
- **Requires:** AST (if/elseif/else structure check)
- **Notes:** Enforce else clause in if-elseif chains

#### 72. IncorrectLineBreak

- **Severity:** `INFO`
- **Tags:** `STANDARD`, `BADPRACTICE`
- **Requires:** Token + line position tracking
- **Notes:** Check for incorrect operator/keyword line breaks

#### 73. IncorrectUseLikeInQuery

- **Severity:** `MAJOR` (ERROR)
- **Tags:** `STANDARD`, `SQL`, `UNPREDICTABLE`
- **Requires:** ⚠️ SDBL AST
- **Notes:** Detect incorrect LIKE patterns - **defer until SDBL**

#### 74. IncorrectUseOfStrTemplate

- **Severity:** `BLOCKER` (ERROR)
- **Tags:** `BRAINOVERLOAD`, `SUSPICIOUS`, `UNPREDICTABLE`
- **Requires:** AST (StrTemplate call analysis)
- **Notes:** Validate СтрШаблон() placeholder usage

---

### 75-77. Internet/Invalid/IsInRole

#### 75. InternetAccess

- **Severity:** `MAJOR` (VULNERABILITY)
- **Enabled by default:** ❌ **No**
- **Tags:** `SUSPICIOUS`
- **Minutes to fix:** 60
- **Requires:** AST (HTTP/Internet method detection)
- **Notes:** Security diagnostic

#### 76. InvalidCharacterInFile

- **Severity:** `MAJOR` (ERROR)
- **Tags:** `ERROR`, `STANDARD`, `UNPREDICTABLE`
- **Requires:** ✅ Lexer (already handles)
- **Notes:** **Already implemented in parser** - wrap as diagnostic

#### 77. IsInRoleMethod

- **Severity:** `MAJOR`
- **Tags:** `ERROR`
- **Requires:** AST (РольДоступна call detection)
- **Notes:** Detect incorrect role check usage

---

### 78-80. Join (SDBL)

#### 78. JoinWithSubQuery

- **Severity:** `MAJOR`
- **Tags:** `SQL`, `STANDARD`, `PERFORMANCE`
- **Requires:** ⚠️ SDBL AST
- **Notes:** **Defer until SDBL**

#### 79. JoinWithVirtualTable

- **Severity:** `MAJOR`
- **Tags:** `SQL`, `STANDARD`, `PERFORMANCE`
- **Requires:** ⚠️ SDBL AST
- **Notes:** **Defer until SDBL**

---

### 81-84. Latin/Line/Logical

#### 81. LatinAndCyrillicSymbolInWord

- **Severity:** `MINOR`
- **Tags:** `BRAINOVERLOAD`, `SUSPICIOUS`
- **Requires:** ✅ Lexer (identifier text check)
- **Notes:** Detect mixed alphabets in identifiers - **good early diagnostic**

#### 82. LineLength

- **Severity:** `MINOR`
- **Tags:** `STANDARD`, `BADPRACTICE`
- **Configuration:** `maxLineLength` parameter (default 120)
- **Requires:** Token + line length tracking
- **Notes:** **Good early diagnostic**

#### 83. LogicalOrInJoinQuerySection

- **Severity:** `MAJOR`
- **Tags:** `SQL`, `PERFORMANCE`, `UNPREDICTABLE`
- **Requires:** ⚠️ SDBL AST
- **Notes:** **Defer until SDBL**

#### 84. LogicalOrInTheWhereSectionOfQuery

- **Severity:** `MAJOR`
- **Tags:** `SQL`, `PERFORMANCE`, `STANDARD`
- **Requires:** ⚠️ SDBL AST
- **Notes:** **Defer until SDBL**

---

### 85-86. Magic

#### 85. MagicDate

- **Severity:** `MINOR`
- **Tags:** `BADPRACTICE`, `BRAINOVERLOAD`
- **Configuration:** `authorizedDates` parameter
- **Requires:** AST (date literal detection)
- **Notes:** Detect hardcoded dates

#### 86. MagicNumber

- **Severity:** `MINOR`
- **Tags:** `BADPRACTICE`
- **Configuration:** `authorizedNumbers` parameter
- **Requires:** AST (number literal detection)
- **Notes:** Detect magic numbers - **good early diagnostic**

---

### 87-94. Metadata/Method/Missing

#### 87. MetadataObjectNameLength

- **Status:** ✅ **Completed - Integrated**
- **Severity:** `MAJOR` (ERROR)
- **Tags:** `STANDARD`
- **Requires:** ✅ Metadata
- **Configuration:** `maxMetadataObjectNameLength` (default: 80)
- **Files:**
  - Implementation: `crates/ide-diagnostics/src/rules/metadata_object_name_length.rs`
  - Handler: `crates/ide-diagnostics/src/handlers/metadata_object_name_length.rs`
  - Test data: `test_data/MetadataObjectNameLengthDiagnostic.bsl`
- **Notes:**
  - Message format matches Java exactly
  - Tests with 82-character metadata object name
  - Supports configuration via .bsl-language-server.json
  - Bilingual message support prepared (EN currently, RU for future)

#### 88. MethodSize

- **Severity:** `MAJOR`
- **Tags:** `BADPRACTICE`
- **Configuration:** `maxMethodSize` parameter (default 200 lines)
- **Requires:** AST (method LOC counter)
- **Notes:** **Good early diagnostic**

#### 89. MissedRequiredParameter

- **Severity:** `MAJOR` (ERROR)
- **Tags:** `ERROR`
- **Requires:** ⚠️ Symbol table + type inference
- **Notes:** **Defer to Iteration 15**

#### 90. MissingCodeTryCatchEx

- **Severity:** `MAJOR` (ERROR)
- **Tags:** `STANDARD`, `BADPRACTICE`
- **Requires:** AST (try-catch handler check)
- **Notes:** Detect empty exception handlers

#### 91. MissingCommonModuleMethod

- **Severity:** `BLOCKER` (ERROR)
- **Tags:** `ERROR`
- **Requires:** ⚠️ Metadata + cross-module resolution
- **Notes:** **Defer to Iteration 19**

#### 92. MissingEventSubscriptionHandler

- **Severity:** `BLOCKER` (ERROR)
- **Tags:** `ERROR`
- **Requires:** ⚠️ Metadata (event subscription)
- **Notes:** **Defer to Iteration 19**

#### 93. MissingParameterDescription

- **Severity:** `MAJOR`
- **Tags:** `STANDARD`, `BADPRACTICE`
- **Requires:** AST (doc comment parsing)
- **Notes:** Check for parameter documentation

#### 94. MissingReturnedValueDescription

- **Severity:** `MAJOR`
- **Tags:** `STANDARD`, `BADPRACTICE`
- **Requires:** AST (doc comment parsing)
- **Notes:** Check for return value documentation

#### 95. MissingSpace

- **Severity:** `INFO`
- **Tags:** `BADPRACTICE`
- **Requires:** Token + whitespace tracking
- **Notes:** **Good early diagnostic**

#### 96-97. MissingTemp\*

- **96. MissingTemporaryFileDeletion:** ERROR/MAJOR - requires data flow
- **97. MissingTempStorageDeletion:** CODE_SMELL/CRITICAL (disabled) - requires data flow
- **Both defer to Iteration 17**

#### 98. MissingVariablesDescription

- **Severity:** `MINOR`
- **Tags:** `STANDARD`
- **Requires:** AST (doc comment parsing)
- **Notes:** Check for variable documentation

---

### 99-106. Multiline/Multilingual/Nested

#### 99. MultilineStringInQuery

- **Severity:** `CRITICAL` (ERROR)
- **Tags:** `BADPRACTICE`, `SUSPICIOUS`, `UNPREDICTABLE`
- **Requires:** ⚠️ SDBL context + multiline string detection
- **Notes:** **Defer until SDBL**

#### 100-101. MultilingualString\*

- **100. MultilingualStringHasAllDeclaredLanguages:** ERROR/MINOR
- **101. MultilingualStringUsingWithTemplate:** ERROR/MAJOR
- **Both require:** ⚠️ Multilingual literal parsing + project language config
- **Notes:** **Defer to Iteration 16**

#### 102. NestedConstructorsInStructureDeclaration

- **Severity:** `MINOR`
- **Tags:** `BADPRACTICE`, `BRAINOVERLOAD`
- **Requires:** AST (structure nesting analysis)
- **Notes:** Detect `New Structure("A", New Structure(...))` patterns

#### 103. NestedFunctionInParameters

- **Severity:** `MINOR`
- **Tags:** `STANDARD`, `BRAINOVERLOAD`, `BADPRACTICE`
- **Requires:** AST (call nesting in arguments)
- **Notes:** Detect `Func(OtherFunc())` patterns

#### 104. NestedStatements

- **Severity:** `CRITICAL`
- **Tags:** `BADPRACTICE`, `BRAINOVERLOAD`
- **Configuration:** `maxAllowedLevel` parameter (default 4)
- **Requires:** AST (nesting depth counter)
- **Notes:** **Good early diagnostic**

#### 105. NestedTernaryOperator

- **Severity:** `MAJOR`
- **Tags:** `BRAINOVERLOAD`
- **Requires:** AST (ternary nesting check)
- **Notes:** Detect `A ? B ? C : D : E` patterns

---

## Summary: D-M Diagnostics

| Category                     | Count  | Notes                                                                                                                                                                                                                                                                                                                                                |
| ---------------------------- | ------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Simple (Syntax-only)**     | 22     | EmptyCodeBlock, EmptyRegion, EmptyStatement, DoubleNegatives, IdenticalExpressions, FunctionShouldHaveReturn, LatinAndCyrillicSymbolInWord, LineLength, MagicNumber, MagicDate, MethodSize, MissingSpace, NestedStatements, NestedTernaryOperator, NestedConstructorsInStructureDeclaration, NestedFunctionInParameters, IfConditionComplexity, etc. |
| **Medium (Symbol Table)**    | 8      | DeprecatedCurrentDate, DeprecatedFind, DeprecatedMessage, DisableSafeMode, ExecuteExternalCode, ExportVariables, ExternalAppStarting, etc.                                                                                                                                                                                                           |
| **Complex (Metadata)**       | 6      | DenyIncompleteValues, ExecuteExternalCodeInCommonModule, ForbiddenMetadataName, MetadataObjectNameLength, MissingCommonModuleMethod, MissingEventSubscriptionHandler                                                                                                                                                                                 |
| **Complex (Type/Data Flow)** | 5      | DeprecatedMethodCall, DuplicatedInsertionIntoCollection, FunctionOutParameter, MissedRequiredParameter, MissingTemp\* (2)                                                                                                                                                                                                                            |
| **SDBL**                     | 7      | FieldsFromJoinsWithoutIsNull, FullOuterJoinQuery, IncorrectUseLikeInQuery, JoinWithSubQuery, JoinWithVirtualTable, LogicalOrInJoinQuerySection, LogicalOrInTheWhereSectionOfQuery, MultilineStringInQuery                                                                                                                                            |
| **Disabled by default**      | 4      | DenyIncompleteValues, FieldsFromJoinsWithoutIsNull, FileSystemAccess, InternetAccess, FunctionNameStartsWithGet, FunctionOutParameter                                                                                                                                                                                                                |
| **Total**                    | **48** | (31-106 in list, some duplicates/overlaps)                                                                                                                                                                                                                                                                                                           |

**Recommended Implementation Order:**

1. **Simple diagnostics first:** EmptyCodeBlock, EmptyRegion, EmptyStatement, LineLength, MagicNumber, MethodSize, MissingSpace, NestedStatements
2. **Method call checks:** DeprecatedCurrentDate, DeprecatedFind, DeprecatedMessage, DisableSafeMode
3. **Expression analysis:** DoubleNegatives, IdenticalExpressions, NestedTernaryOperator
4. **If/condition checks:** IfConditionComplexity, IfElseDuplicatedCodeBlock, IfElseDuplicatedCondition, IfElseIfEndsWithElse
5. Defer **SDBL diagnostics** until SDBL AST complete
6. Defer **Metadata diagnostics** to Iteration 19
7. Defer **Data flow diagnostics** to Iteration 17

---

**Next:** [MIGRATION_PLAN_N_S.md](./MIGRATION_PLAN_N_S.md) - Diagnostics N through S
