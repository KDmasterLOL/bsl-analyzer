# Diagnostics Migration Plan: A-C (Part 1/4)

## Overview

This document contains the migration plan for diagnostics starting with letters **A through C** (45 diagnostics).

**Migration Strategy:**
- Transfer diagnostics **one by one** in alphabetical order
- Implement infrastructure improvements **immediately** when discovered during diagnostic implementation
- Ensure **100% compatibility** with bsl-language-server (same line/column numbers and ranges)
- Copy test cases from Java project and verify identical results

---

## Configuration

Diagnostics can be configured via `.bsl-language-server.json`:

```json
{
  "diagnostics": {
    "parameters": {
      "DiagnosticKey": false,           // Disable diagnostic
      "DiagnosticKey": {                // Configure parameters
        "parameterName": "value"
      }
    }
  }
}
```

---

## Diagnostics: A-C

### 1. AllFunctionPathMustHaveReturn

**Code:** `AllFunctionPathMustHaveReturn`
**Russian:** Все возможные пути выполнения функции должны содержать оператор Возврат
**Type:** `CODE_SMELL`
**Severity:** `MAJOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 1
**Tags:** `UNPREDICTABLE`, `BADPRACTICE`, `SUSPICIOUS`
**Scope:** `ALL` (BSL + OneScript)

**Sources:**
- **Java:** `/Users/kiriller/src/lsp/bsl-language-server/src/main/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/AllFunctionPathMustHaveReturnDiagnostic.java`
- **Rust Reference:** ✅ `/Users/kiriller/src/lsp/bsl-language-server-rust/crates/bsl-diagnostics/src/rules/all_function_path_must_have_return.rs`
- **Target:** `/Users/kiriller/src/lsp/bsl-analyzer/crates/ide-diagnostics/src/handlers/all_function_path_must_have_return.rs`

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ AST (already available)
- ⚠️ **Control Flow Graph (CFG)** - needs implementation
- Path analysis for all function branches

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/AllFunctionPathMustHaveReturnDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/AllFunctionPathMustHaveReturnDiagnostic.bsl`

**Implementation Notes:**
- ✅ **Rust reference exists** - can study implementation approach
- Requires CFG construction (add to HIR infrastructure)
- Must analyze all code paths through function
- Skip procedures (only check functions)
- Handle early returns, nested conditions, loops

---

### 2. AssignAliasFieldsInQuery

**Code:** `AssignAliasFieldsInQuery`
**Russian:** Назначение псевдонимов выбранным полям в запросе
**Type:** `CODE_SMELL`
**Severity:** `MAJOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 1
**Tags:** `STANDARD`, `SQL`, `BADPRACTICE`
**Scope:** `BSL`

**Sources:**
- **Java:** `/Users/kiriller/src/lsp/bsl-language-server/src/main/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/AssignAliasFieldsInQueryDiagnostic.java`
- **Rust Reference:** ✅ `/Users/kiriller/src/lsp/bsl-language-server-rust/crates/bsl-diagnostics/src/rules/assign_alias_fields_in_query.rs`
- **Target:** `/Users/kiriller/src/lsp/bsl-analyzer/crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs`

**Infrastructure Requirements:**
- ✅ SDBL lexer (already available)
- ✅ SDBL parser entry point (already available)
- ⚠️ **SDBL AST** - needs completion (SyntaxKind nodes exist, need AST wrappers)
- Query field alias detection

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/AssignAliasFieldsInQueryDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/AssignAliasFieldsInQueryDiagnostic.bsl`

**Implementation Notes:**
- ✅ **Rust reference exists** - can study SDBL parsing approach
- Parse SDBL queries in string literals
- Check SELECT field list for missing aliases
- Skip system fields and aggregates

---

### 3. BadWords

**Code:** `BadWords`
**Russian:** Запрещенные слова
**Type:** `CODE_SMELL`
**Severity:** `MAJOR`
**Enabled by default:** ❌ **No** (must be enabled via config)
**Configurable:** ✅ Yes (requires word list configuration)
**Minutes to fix:** 1
**Tags:** `DESIGN`
**Scope:** `ALL`

**Sources:**
- **Java:** `/Users/kiriller/src/lsp/bsl-language-server/src/main/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/BadWordsDiagnostic.java`
- **Rust Reference:** ✅ `/Users/kiriller/src/lsp/bsl-language-server-rust/crates/bsl-diagnostics/src/rules/bad_words.rs`
- **Target:** `/Users/kiriller/src/lsp/bsl-analyzer/crates/ide-diagnostics/src/handlers/bad_words.rs`

**Configuration Parameters:**
```json
{
  "diagnostics": {
    "parameters": {
      "BadWords": {
        "words": "хрень,дурацкий,тупой"  // Custom word list
      }
    }
  }
}
```

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ Token stream access
- Configuration parameter support
- Case-insensitive word matching

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/BadWordsDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/BadWordsDiagnostic.bsl`

**Implementation Notes:**
- ✅ **Rust reference exists** - can study implementation approach
- Check identifiers, comments, string literals
- Support bilingual (RU/EN) keywords
- Configurable word list via parameters

---

### 4. BeginTransactionBeforeTryCatch

**Code:** `BeginTransactionBeforeTryCatch`
**Russian:** НачатьТранзакцию вне блока Попытка-Исключение
**Type:** `ERROR`
**Severity:** `MAJOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 10
**Tags:** `STANDARD`
**Scope:** `ALL`

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ AST (already available)
- Method call detection (`BeginTransaction`, `НачатьТранзакцию`)
- Try-catch block context analysis

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/BeginTransactionBeforeTryCatchDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/BeginTransactionBeforeTryCatchDiagnostic.bsl`

**Implementation Notes:**
- Find `BeginTransaction`/`НачатьТранзакцию` calls
- Check if call is inside `Попытка...Исключение` block
- Must handle both Russian and English method names

---

### 5. CachedPublic

**Code:** `CachedPublic`
**Russian:** Кеширование программного интерфейса
**Type:** `CODE_SMELL`
**Severity:** `MAJOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 5
**Tags:** `STANDARD`, `DESIGN`
**Scope:** `BSL`

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ⚠️ **Module context** (module type detection) - needs metadata integration
- Export method detection
- Common module type checking (Cached, Global, etc.)

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CachedPublicDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CachedPublicDiagnostic.bsl`

**Implementation Notes:**
- Requires metadata context (CommonModule properties)
- Check export procedures/functions in cached modules
- **Deferred until Metadata Infrastructure (Iteration 11)**

---

### 6. CanonicalSpellingKeywords

**Code:** `CanonicalSpellingKeywords`
**Russian:** Каноническое написание ключевых слов
**Type:** `CODE_SMELL`
**Severity:** `INFO`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 1
**Tags:** `STANDARD`
**Scope:** `ALL`

**Infrastructure Requirements:**
- ✅ Lexer (already available)
- ✅ Token access with original text
- Keyword canonical form mapping (Если vs если vs ЕСЛИ)

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CanonicalSpellingKeywordsDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CanonicalSpellingKeywordsDiagnostic.bsl`

**Implementation Notes:**
- Simple token-based diagnostic
- Compare token text with canonical spelling
- Support both RU/EN keywords
- **Good first diagnostic to implement**

---

### 7. CodeAfterAsyncCall

**Code:** `CodeAfterAsyncCall`
**Russian:** Код после асинхронного вызова
**Type:** `CODE_SMELL`
**Severity:** `MAJOR`
**Enabled by default:** ❌ **No** (must be enabled via config)
**Configurable:** ✅ Yes
**Minutes to fix:** 10
**Tags:** `SUSPICIOUS`
**Scope:** `BSL`

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ AST (already available)
- Async method call detection
- Statement sequence analysis

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CodeAfterAsyncCallDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CodeAfterAsyncCallDiagnostic.bsl`

**Implementation Notes:**
- Detect async calls (methods with callback parameters)
- Check for statements after async call in same block
- Warn about unreachable code after async

---

### 8. CodeBlockBeforeSub

**Code:** `CodeBlockBeforeSub`
**Russian:** Блок кода перед определением метода
**Type:** `ERROR`
**Severity:** `BLOCKER`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 5
**Tags:** `ERROR`
**Scope:** `ALL`

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ AST (already available)
- Module-level statement detection

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CodeBlockBeforeSubDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CodeBlockBeforeSubDiagnostic.bsl`

**Implementation Notes:**
- Check module structure for executable code before first Procedure/Function
- Allow variable declarations, regions, preprocessor
- **Good early diagnostic** (simple AST traversal)

---

### 9. CodeOutOfRegion

**Code:** `CodeOutOfRegion`
**Russian:** Код вне областей
**Type:** `CODE_SMELL`
**Severity:** `INFO`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 1
**Tags:** `STANDARD`
**Scope:** `BSL`

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ AST (already available)
- Region detection (`#Область` / `#Region`)

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CodeOutOfRegionDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CodeOutOfRegionDiagnostic.bsl`

**Implementation Notes:**
- Find procedures/functions outside `#Область...#КонецОбласти` blocks
- Track region nesting
- Allow module-level variables outside regions

---

### 10. CognitiveComplexity

**Code:** `CognitiveComplexity`
**Russian:** Когнитивная сложность
**Type:** `CODE_SMELL`
**Severity:** `CRITICAL`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (threshold configurable)
**Minutes to fix:** 15
**Tags:** `BRAINOVERLOAD`
**Scope:** `ALL`

**Configuration Parameters:**
```json
{
  "diagnostics": {
    "parameters": {
      "CognitiveComplexity": {
        "complexityThreshold": 15  // Default threshold
      }
    }
  }
}
```

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ AST (already available)
- ⚠️ **Cognitive Complexity Calculator** - needs implementation
- Nesting level tracking
- Control flow penalty calculation

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CognitiveComplexityDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CognitiveComplexityDiagnostic.bsl`

**Implementation Notes:**
- Algorithm defined in SonarSource spec
- Increment for if/for/while/foreach
- Additional penalty for nesting
- Different from cyclomatic complexity

---

### 11. CommandModuleExportMethods

**Code:** `CommandModuleExportMethods`
**Russian:** Экспортные методы в модулях команд
**Type:** `CODE_SMELL`
**Severity:** `INFO`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 1
**Tags:** `STANDARD`, `CLUMSY`
**Scope:** `BSL`

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ⚠️ **Module type detection** (CommandModule) - needs metadata
- Export method detection

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CommandModuleExportMethodsDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CommandModuleExportMethodsDiagnostic.bsl`

**Implementation Notes:**
- Requires metadata context (module type)
- **Deferred until Metadata Infrastructure (Iteration 11)**

---

### 12. CommentedCode

**Code:** `CommentedCode`
**Russian:** Закомментированный фрагмент кода
**Type:** `CODE_SMELL`
**Severity:** `MINOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (threshold configurable)
**Minutes to fix:** 1
**Tags:** `STANDARD`, `BADPRACTICE`
**Scope:** `ALL`

**Configuration Parameters:**
```json
{
  "diagnostics": {
    "parameters": {
      "CommentedCode": {
        "threshold": 0.9  // Default 90% code confidence
      }
    }
  }
}
```

**Infrastructure Requirements:**
- ✅ Lexer (already available)
- Comment token access
- ⚠️ **Heuristic analyzer** (detect if comment looks like code)
- Keyword/operator density check

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CommentedCodeDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CommentedCodeDiagnostic.bsl`

**Implementation Notes:**
- Parse comment content for BSL keywords, operators
- Calculate "code likelihood" score
- Threshold-based detection

---

### 13. CommitTransactionOutsideTryCatch

**Code:** `CommitTransactionOutsideTryCatch`
**Russian:** ЗафиксироватьТранзакцию вне блока Попытка-Исключение
**Type:** `ERROR`
**Severity:** `MAJOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 10
**Tags:** `STANDARD`
**Scope:** `ALL`

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ AST (already available)
- Method call detection (`CommitTransaction`, `ЗафиксироватьТранзакцию`)
- Try-catch block context analysis

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CommitTransactionOutsideTryCatchDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CommitTransactionOutsideTryCatchDiagnostic.bsl`

**Implementation Notes:**
- Similar to BeginTransactionBeforeTryCatch
- Check if `CommitTransaction` is in try-finally or try-catch

---

### 14-24. CommonModule* Diagnostics (11 diagnostics)

All `CommonModule*` diagnostics require **Metadata Infrastructure** and will be deferred to **Iteration 19** (Tier 3):

#### 14. CommonModuleAssign
- **Severity:** `BLOCKER`
- **Tags:** `ERROR`
- **Requires:** Metadata (module type detection)

#### 15. CommonModuleInvalidType
- **Severity:** `MAJOR`
- **Tags:** `STANDARD`, `UNPREDICTABLE`, `DESIGN`
- **Requires:** Metadata (module properties: Server, Client, etc.)

#### 16. CommonModuleMissingAPI
- **Severity:** `MINOR`
- **Tags:** `BRAINOVERLOAD`, `SUSPICIOUS`
- **Requires:** Metadata + region parsing

#### 17. CommonModuleNameCached
- **Severity:** `MAJOR`
- **Tags:** `STANDARD`, `BADPRACTICE`, `UNPREDICTABLE`
- **Requires:** Metadata (Cached flag check)

#### 18. CommonModuleNameClient
- **Severity:** `MINOR`
- **Tags:** `STANDARD`, `BADPRACTICE`, `UNPREDICTABLE`
- **Requires:** Metadata (Client flag check)

#### 19. CommonModuleNameClientServer
- **Severity:** `MAJOR`
- **Tags:** `STANDARD`, `BADPRACTICE`, `UNPREDICTABLE`
- **Requires:** Metadata (ClientServer flag check)

#### 20. CommonModuleNameFullAccess
- **Severity:** `MAJOR` (`SECURITY_HOTSPOT`)
- **Tags:** `STANDARD`, `BADPRACTICE`, `UNPREDICTABLE`
- **Requires:** Metadata (PrivilegedMode flag check)

#### 21. CommonModuleNameGlobal
- **Severity:** `MAJOR`
- **Tags:** `STANDARD`, `BADPRACTICE`, `BRAINOVERLOAD`
- **Requires:** Metadata (Global flag check)

#### 22. CommonModuleNameGlobalClient
- **Severity:** `MAJOR`
- **Tags:** `STANDARD`
- **Requires:** Metadata (Global + Client flag check)

#### 23. CommonModuleNameServerCall
- **Severity:** `MINOR`
- **Tags:** `STANDARD`, `BADPRACTICE`, `UNPREDICTABLE`
- **Requires:** Metadata (ServerCall flag check)

#### 24. CommonModuleNameWords
- **Severity:** `INFO`
- **Tags:** `STANDARD`
- **Requires:** Metadata (module name parsing)

**Test Files (Java):**
- All have corresponding test files in `/Users/kiriller/src/lsp/bsl-language-server/src/test/`

**Implementation Strategy:**
- **Defer all CommonModule diagnostics to Iteration 19**
- Implement after Metadata Infrastructure (Iteration 11) is complete
- Share common base class `AbstractCommonModuleNameDiagnostic` (port from Java)

---

### 25. CompilationDirectiveLost

**Code:** `CompilationDirectiveLost`
**Russian:** Пропущенная директива компиляции метода
**Type:** `CODE_SMELL`
**Severity:** `MAJOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 1
**Tags:** `STANDARD`, `UNPREDICTABLE`
**Scope:** `BSL`

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ AST (already available)
- Compilation directive detection (`&НаКлиенте`, `&НаСервере`, etc.)
- Method annotation tracking

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CompilationDirectiveLostDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CompilationDirectiveLostDiagnostic.bsl`

**Implementation Notes:**
- Check methods for missing compilation directives in form/managed modules
- Requires module type context

---

### 26. CompilationDirectiveNeedLess

**Code:** `CompilationDirectiveNeedLess`
**Russian:** Излишняя директива компиляции метода
**Type:** `CODE_SMELL`
**Severity:** `MAJOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 1
**Tags:** `CLUMSY`, `STANDARD`, `UNPREDICTABLE`
**Scope:** `BSL`

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ AST (already available)
- Compilation directive detection
- Module type context

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CompilationDirectiveNeedLessDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CompilationDirectiveNeedLessDiagnostic.bsl`

**Implementation Notes:**
- Opposite of CompilationDirectiveLost
- Check for unnecessary directives in common/server modules

---

### 27. ConsecutiveEmptyLines

**Code:** `ConsecutiveEmptyLines`
**Russian:** Подряд идущие пустые строки
**Type:** `CODE_SMELL`
**Severity:** `INFO`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 1
**Tags:** `BADPRACTICE`
**Scope:** `ALL`

**Configuration Parameters:**
```json
{
  "diagnostics": {
    "parameters": {
      "ConsecutiveEmptyLines": {
        "allowedEmptyLinesCount": 1  // Max consecutive empty lines
      }
    }
  }
}
```

**Infrastructure Requirements:**
- ✅ Lexer (already available)
- Token line tracking
- Empty line detection

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/ConsecutiveEmptyLinesDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/ConsecutiveEmptyLinesDiagnostic.bsl`

**Implementation Notes:**
- Count consecutive newline tokens
- **Good early diagnostic** (simple token stream)

---

### 28. CrazyMultilineString

**Code:** `CrazyMultilineString`
**Russian:** Безумные многострочные литералы
**Type:** `CODE_SMELL`
**Severity:** `MAJOR`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 1
**Tags:** `BADPRACTICE`, `SUSPICIOUS`, `UNPREDICTABLE`
**Scope:** `ALL`

**Infrastructure Requirements:**
- ✅ Lexer (already available)
- String literal token detection
- Multiline string analysis (count lines)

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CrazyMultilineStringDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CrazyMultilineStringDiagnostic.bsl`

**Implementation Notes:**
- Detect string literals with excessive line breaks
- Check for strings > 3-5 lines
- Suggest refactoring to templates or external files

---

### 29. CreateQueryInCycle

**Code:** `CreateQueryInCycle`
**Russian:** Создание запроса в цикле
**Type:** `ERROR`
**Severity:** `CRITICAL`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (can be disabled via config)
**Minutes to fix:** 20
**Tags:** `PERFORMANCE`
**Scope:** `ALL`

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ AST (already available)
- Loop detection (For, While, ForEach)
- Query creation detection (`New Query`, `Новый Запрос`)

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CreateQueryInCycleDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CreateQueryInCycleDiagnostic.bsl`

**Implementation Notes:**
- Find `New Query` inside loop bodies
- Critical performance issue
- Simple AST pattern matching

---

### 30. CyclomaticComplexity

**Code:** `CyclomaticComplexity`
**Russian:** Цикломатическая сложность
**Type:** `CODE_SMELL`
**Severity:** `CRITICAL`
**Enabled by default:** ✅ Yes
**Configurable:** ✅ Yes (threshold configurable)
**Minutes to fix:** 25
**Tags:** `BRAINOVERLOAD`
**Scope:** `ALL`

**Configuration Parameters:**
```json
{
  "diagnostics": {
    "parameters": {
      "CyclomaticComplexity": {
        "complexityThreshold": 20,       // Default threshold
        "checkModuleBody": true          // Check module-level code
      }
    }
  }
}
```

**Infrastructure Requirements:**
- ✅ Parser (already available)
- ✅ AST (already available)
- ⚠️ **Cyclomatic Complexity Calculator** - needs implementation
- Decision point counting

**Test Files (Java):**
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/CyclomaticComplexityDiagnosticTest.java`
- `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/CyclomaticComplexityDiagnostic.bsl`

**Implementation Notes:**
- Standard McCabe's cyclomatic complexity
- Count decision points: if, for, while, foreach, case, &&, ||, ?
- Complexity = decision points + 1

---

## Summary: A-C Diagnostics

| Category | Count | Notes |
|----------|-------|-------|
| **Simple (Syntax-only)** | 8 | CanonicalSpellingKeywords, CodeBlockBeforeSub, CodeOutOfRegion, ConsecutiveEmptyLines, CrazyMultilineString, CreateQueryInCycle, CommentedCode, BadWords |
| **Medium (Symbol Table)** | 5 | BeginTransactionBeforeTryCatch, CommitTransactionOutsideTryCatch, CompilationDirectiveLost, CompilationDirectiveNeedLess, CodeAfterAsyncCall |
| **Complex (Metadata)** | 12 | CachedPublic, CommandModuleExportMethods, all CommonModule* (11 diagnostics) |
| **Complex (CFG)** | 3 | AllFunctionPathMustHaveReturn, CognitiveComplexity, CyclomaticComplexity |
| **SDBL** | 1 | AssignAliasFieldsInQuery |
| **Disabled by default** | 2 | BadWords, CodeAfterAsyncCall |
| **Total** | **30** | |

**Recommended Implementation Order:**
1. **CanonicalSpellingKeywords** - simplest, good starter
2. **ConsecutiveEmptyLines** - simple token stream
3. **CodeBlockBeforeSub** - simple AST check
4. **CodeOutOfRegion** - region tracking
5. **CrazyMultilineString** - token analysis
6. **BeginTransactionBeforeTryCatch** - AST pattern
7. **CommitTransactionOutsideTryCatch** - similar to #6
8. **CreateQueryInCycle** - AST pattern
9. **CyclomaticComplexity** - requires complexity calculator
10. **CognitiveComplexity** - requires complexity calculator
11. **AllFunctionPathMustHaveReturn** - requires CFG
12. Defer all **CommonModule\*** diagnostics to Iteration 19
13. Defer **CachedPublic**, **CommandModuleExportMethods** to Iteration 19
14. Defer **AssignAliasFieldsInQuery** until SDBL AST is complete

---

**Next:** [MIGRATION_PLAN_D_M.md](./MIGRATION_PLAN_D_M.md) - Diagnostics D through M
