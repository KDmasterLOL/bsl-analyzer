# Diagnostics Migration Plan: T-Z (Part 4/4)

## Overview

This document contains the migration plan for diagnostics starting with letters **T through Z** (56 diagnostics - final part).

---

## Diagnostics: T-Z

### 152-162. Unary/Union/Unknown/Unreachable/Unsafe/Unused/Usage/UseLess/Useless/UseSystem

#### 152. UnaryPlusInConcatenation
**Code:** `UnaryPlusInConcatenation`
**Russian:** Унарный плюс в конкатенации строк
**Type:** `ERROR` | **Severity:** `BLOCKER`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `SUSPICIOUS`, `BRAINOVERLOAD`
**Scope:** `ALL`

**Requires:** AST (unary operator in string context)
**Notes:** Detect `"A" + +B` instead of `"A" + B` - **good early diagnostic**

---

#### 153. UnionAll
**Code:** `UnionAll`
**Type:** `CODE_SMELL` | **Severity:** `MINOR`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `STANDARD`, `SQL`, `PERFORMANCE`
**Scope:** `BSL`

**Requires:** ⚠️ SDBL AST
**Notes:** Suggest UNION ALL instead of UNION - **defer until SDBL**

---

#### 154. UnknownPreprocessorSymbol
**Code:** `UnknownPreprocessorSymbol`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `STANDARD`, `ERROR`
**Scope:** `ALL`

**Requires:** Preprocessor symbol tracking
**Notes:** Check `#Если` symbols against defined symbols - **good early diagnostic**

---

#### 155. UnreachableCode
**Code:** `UnreachableCode`
**Russian:** Недостижимый код
**Type:** `ERROR` | **Severity:** `MINOR`
**Enabled:** ✅ Yes | **Minutes:** 10
**Tags:** `DESIGN`, `SUSPICIOUS`
**Scope:** `ALL`

**Requires:** ⚠️ CFG (dead code detection)
**Notes:** Detect code after Return/Break/Continue - **defer until CFG**

---

#### 156. UnsafeFindByCode
**Code:** `UnsafeFindByCode`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `DESIGN`, `SUSPICIOUS`
**Scope:** `BSL`

**Requires:** AST (НайтиПоКоду/FindByCode call detection)
**Notes:** Suggest safer alternatives to FindByCode

---

#### 157. UnsafeSafeModeMethodCall
**Code:** `UnsafeSafeModeMethodCall`
**Type:** `ERROR` | **Severity:** `BLOCKER`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `DEPRECATED`, `ERROR`
**Scope:** `BSL`

**Requires:** AST (БезопасныйРежим/SafeMode call validation)
**Notes:** Check for unsafe SafeMode usage

---

#### 158. UnusedLocalMethod
**Code:** `UnusedLocalMethod`
**Russian:** Неиспользуемый локальный метод
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`, `SUSPICIOUS`, `UNUSED`
**Scope:** `ALL`

**Requires:** ⚠️ Symbol table + usage tracking
**Notes:** Find non-export methods never called - **defer to Iteration 16**

---

#### 159. UnusedLocalVariable
**Code:** `UnusedLocalVariable`
**Russian:** Неиспользуемая локальная переменная
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `BRAINOVERLOAD`, `BADPRACTICE`, `UNUSED`
**Scope:** `ALL`

**Requires:** ⚠️ Symbol table + usage tracking
**Notes:** Find unused local variables - **defer to Iteration 16**

---

#### 160. UnusedParameters
**Code:** `UnusedParameters`
**Russian:** Неиспользуемые параметры
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `DESIGN`, `UNUSED`
**Scope:** `OS` (OneScript only)

**Requires:** ⚠️ Symbol table + usage tracking
**Notes:** OneScript-specific - **defer to Iteration 16**

---

#### 161. UsageWriteLogEvent
**Code:** `UsageWriteLogEvent`
**Type:** `CODE_SMELL` | **Severity:** `INFO`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`, `BADPRACTICE`
**Scope:** `ALL`

**Requires:** AST (ЗаписьЖурналаРегистрации call validation)
**Notes:** Check for incorrect event log usage

---

#### 162. UseLessForEach
**Code:** `UseLessForEach`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 2
**Tags:** `CLUMSY`
**Scope:** `ALL`

**Requires:** AST (ForEach with empty body)
**Notes:** Detect empty ForEach loops - **good early diagnostic**

---

#### 163. UselessTernaryOperator
**Code:** `UselessTernaryOperator`
**Type:** `CODE_SMELL` | **Severity:** `INFO`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `BADPRACTICE`, `SUSPICIOUS`
**Scope:** `BSL`

**Requires:** AST (ternary simplification check)
**Notes:** Detect `A ? True : False` (simplify to `A`) - **good early diagnostic**

---

#### 164. UseSystemInformation
**Code:** `UseSystemInformation`
**Type:** `SECURITY_HOTSPOT` | **Severity:** `CRITICAL`
**Enabled:** ❌ **No**
**Minutes:** 5
**Tags:** `SUSPICIOUS`
**Scope:** `ALL`

**Requires:** AST (system info access detection)
**Notes:** Security diagnostic - flag system information usage

---

### 165-177. Using* Diagnostics (13 diagnostics)

#### 165. UsingCancelParameter
**Code:** `UsingCancelParameter`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 10
**Tags:** `STANDARD`, `BADPRACTICE`
**Scope:** `ALL`

**Requires:** AST (Отказ/Cancel parameter pattern)
**Notes:** Check for improper Cancel parameter usage

---

#### 166. UsingExternalCodeTools
**Code:** `UsingExternalCodeTools`
**Type:** `SECURITY_HOTSPOT` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 15
**Tags:** `STANDARD`, `DESIGN`
**Scope:** `ALL`

**Requires:** AST (external code tool detection: Execute, Eval, etc.)
**Notes:** Security diagnostic - comprehensive external code checker

---

#### 167. UsingFindElementByString
**Code:** `UsingFindElementByString`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 2
**Tags:** `STANDARD`, `BADPRACTICE`, `PERFORMANCE`
**Scope:** `BSL`

**Requires:** AST (НайтиПоНаименованию/FindByDescription call)
**Notes:** Suggest alternatives to FindByDescription

---

#### 168. UsingGoto
**Code:** `UsingGoto`
**Type:** `CODE_SMELL` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `STANDARD`, `BADPRACTICE`
**Scope:** `ALL`

**Requires:** AST (Перейти/Goto statement detection)
**Notes:** Flag all Goto usage - **good early diagnostic**

---

#### 169. UsingHardcodeNetworkAddress
**Code:** `UsingHardcodeNetworkAddress`
**Type:** `VULNERABILITY` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 15
**Tags:** `STANDARD`
**Scope:** `ALL`

**Requires:** AST (IP address/URL literal detection)
**Notes:** Security diagnostic - detect hardcoded IPs

---

#### 170. UsingHardcodePath
**Code:** `UsingHardcodePath`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 15
**Tags:** `STANDARD`
**Scope:** `BSL`

**Requires:** AST (file path literal detection)
**Notes:** Detect hardcoded file paths

---

#### 171. UsingHardcodeSecretInformation
**Code:** `UsingHardcodeSecretInformation`
**Type:** `VULNERABILITY` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 15
**Tags:** `STANDARD`
**Scope:** `BSL`

**Requires:** AST (password/secret pattern detection)
**Notes:** Security diagnostic - detect hardcoded secrets

---

#### 172. UsingLikeInQuery
**Code:** `UsingLikeInQuery`
**Type:** `ERROR` | **Severity:** `MAJOR`
**Enabled:** ❌ **No**
**Minutes:** 10
**Tags:** `SQL`, `UNPREDICTABLE`
**Scope:** `BSL`

**Requires:** ⚠️ SDBL AST
**Notes:** **Defer until SDBL**

---

#### 173. UsingModalWindows
**Code:** `UsingModalWindows`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 15
**Tags:** `STANDARD`
**Scope:** `BSL`

**Requires:** AST (modal window method detection)
**Notes:** Detect deprecated modal dialog usage

---

#### 174. UsingObjectNotAvailableUnix
**Code:** `UsingObjectNotAvailableUnix`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 30
**Tags:** `STANDARD`, `LOCKINOS`
**Scope:** `BSL`

**Requires:** Platform object compatibility list
**Notes:** Check for Windows-only objects (COMObject, etc.)

---

#### 175. UsingServiceTag
**Code:** `UsingServiceTag`
**Type:** `CODE_SMELL` | **Severity:** `INFO`
**Enabled:** ✅ Yes | **Minutes:** 0
**Tags:** `BADPRACTICE`
**Scope:** `ALL`

**Requires:** Comment parsing (service tag detection)
**Notes:** Detect service tags in comments (TODO, FIXME, etc.)

---

#### 176. UsingSynchronousCalls
**Code:** `UsingSynchronousCalls`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 15
**Tags:** `STANDARD`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata (form module + synchronous call list)
**Notes:** **Defer to Iteration 19**

---

#### 177. UsingThisForm
**Code:** `UsingThisForm`
**Type:** `CODE_SMELL` | **Severity:** `MINOR`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`, `DEPRECATED`
**Scope:** `BSL`

**Requires:** AST (ЭтаФорма/ThisForm reference detection)
**Notes:** Detect deprecated ThisForm usage

---

### 178-184. Virtual/Wrong/Yo (Final 7 diagnostics)

#### 178. VirtualTableCallWithoutParameters
**Code:** `VirtualTableCallWithoutParameters`
**Type:** `ERROR` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `SQL`, `STANDARD`, `PERFORMANCE`
**Scope:** `BSL`

**Requires:** ⚠️ SDBL AST
**Notes:** Check virtual table parameters - **defer until SDBL**

---

#### 179. WrongDataPathForFormElements
**Code:** `WrongDataPathForFormElements`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `UNPREDICTABLE`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata (form structure + data path validation)
**Notes:** **Defer to Iteration 19**

---

#### 180. WrongHttpServiceHandler
**Code:** `WrongHttpServiceHandler`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 10
**Tags:** `SUSPICIOUS`, `ERROR`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata (HTTP service validation)
**Notes:** **Defer to Iteration 19**

---

#### 181. WrongUseFunctionProceedWithCall
**Code:** `WrongUseFunctionProceedWithCall`
**Type:** `ERROR` | **Severity:** `BLOCKER`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `ERROR`, `SUSPICIOUS`
**Scope:** `BSL`

**Requires:** AST (ПродолжитьВызов/ProceedWithCall validation)
**Notes:** Check async callback usage

---

#### 182. WrongUseOfRollbackTransactionMethod
**Code:** `WrongUseOfRollbackTransactionMethod`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`
**Scope:** `ALL`

**Requires:** AST (RollbackTransaction in try-catch)
**Notes:** Check rollback placement (should be in exception handler)

---

#### 183. WrongWebServiceHandler
**Code:** `WrongWebServiceHandler`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 10
**Tags:** `SUSPICIOUS`, `ERROR`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata (web service validation)
**Notes:** **Defer to Iteration 19**

---

#### 184. YoLetterUsage
**Code:** `YoLetterUsage`
**Russian:** Использование буквы "ё" в коде
**Type:** `CODE_SMELL` | **Severity:** `INFO`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `STANDARD`
**Scope:** `ALL`

**Requires:** Token/identifier text scanning
**Notes:** Detect Cyrillic "ё" in identifiers/strings - **good early diagnostic**

---

## Complete Diagnostic Summary (All 180 Diagnostics)

### By Implementation Tier

| Tier | Count | Description |
|------|-------|-------------|
| **Tier 1: Syntax-only** | ~60 | Simple AST/token checks, no semantic analysis |
| **Tier 2: Symbol Table** | ~60 | Require symbol resolution, type inference |
| **Tier 3: Metadata** | ~40 | Require 1C metadata (Configuration.xml, etc.) |
| **Tier 4: SDBL** | ~16 | Require SDBL parser and semantic analysis |
| **Special** | ~4 | CFG, spell checker, external tools |

### By Enabled Status

| Status | Count | Notes |
|--------|-------|-------|
| **Enabled by default** | ~172 | Active in default configuration |
| **Disabled by default** | ~8 | Must be explicitly enabled |

**Disabled by default:**
1. BadWords
2. CodeAfterAsyncCall
3. DenyIncompleteValues
4. FieldsFromJoinsWithoutIsNull
5. FileSystemAccess
6. FunctionNameStartsWithGet
7. FunctionOutParameter
8. InternetAccess
9. MissingTempStorageDeletion
10. TernaryOperatorUsage
11. TooManyReturns
12. UseSystemInformation
13. UsingLikeInQuery

### By Severity

| Severity | Count | Type Distribution |
|----------|-------|-------------------|
| **BLOCKER** | ~12 | Mostly ERROR |
| **CRITICAL** | ~30 | ERROR, VULNERABILITY, SECURITY_HOTSPOT |
| **MAJOR** | ~90 | ERROR, CODE_SMELL, VULNERABILITY |
| **MINOR** | ~30 | CODE_SMELL |
| **INFO** | ~18 | CODE_SMELL |

### By Type

| Type | Count | Examples |
|------|-------|----------|
| **CODE_SMELL** | ~109 | Style, bad practices, complexity |
| **ERROR** | ~56 | Syntax errors, logic bugs |
| **VULNERABILITY** | ~7 | Security issues (Execute, hardcoded secrets) |
| **SECURITY_HOTSPOT** | ~8 | Potential security concerns |

---

## Implementation Roadmap

### Phase 1: Quick Wins (Iterations 12-13) - ~30 diagnostics

**Simple syntax checks, good for validating infrastructure:**

1. CanonicalSpellingKeywords
2. ConsecutiveEmptyLines
3. LineLength
4. MissingSpace
5. OneStatementPerLine
6. SemicolonPresence
7. SpaceAtStartComment
8. CodeBlockBeforeSub
9. CodeOutOfRegion
10. EmptyCodeBlock
11. EmptyRegion
12. EmptyStatement
13. ExtraCommas
14. IncorrectLineBreak
15. InvalidCharacterInFile (wrap parser)
16. LatinAndCyrillicSymbolInWord
17. MagicNumber
18. MagicDate
19. MethodSize
20. NestedStatements
21. NestedTernaryOperator
22. NonStandardRegion
23. NumberOfOptionalParams
24. NumberOfParams
25. OrderOfParams
26. ParseError (wrap parser)
27. ProcedureReturnsValue
28. ReservedParameterNames
29. SelfAssign
30. SelfInsertion
31. SemicolonPresence
32. SpaceAtStartComment
33. ThisObjectAssign
34. UnaryPlusInConcatenation
35. UnknownPreprocessorSymbol
36. UseLessForEach
37. UselessTernaryOperator
38. UsingGoto
39. YoLetterUsage

### Phase 2: Symbol Table (Iterations 14-18) - ~50 diagnostics

**Require basic symbol resolution:**

1. BeginTransactionBeforeTryCatch
2. CommitTransactionOutsideTryCatch
3. CreateQueryInCycle
4. DeprecatedCurrentDate
5. DeprecatedFind
6. DeprecatedMessage
7. DisableSafeMode
8. DoubleNegatives
9. ExecuteExternalCode
10. ExportVariables
11. ExternalAppStarting
12. FunctionReturnsSamePrimitive
13. FunctionShouldHaveReturn
14. IdenticalExpressions
15. IfConditionComplexity
16. IfElseDuplicatedCodeBlock
17. IfElseDuplicatedCondition
18. IfElseIfEndsWithElse
19. AllFunctionPathMustHaveReturn (requires CFG)
20. CognitiveComplexity
21. CyclomaticComplexity
22. PairingBrokenTransaction (requires CFG)
23. UnreachableCode (requires CFG)
24. UnusedLocalMethod
25. UnusedLocalVariable
26. UnusedParameters
27. And ~25 more symbol/expression-based diagnostics

### Phase 3: Metadata (Iteration 19-23) - ~40 diagnostics

**All CommonModule*, metadata validation, etc.:**

1. All CommonModule* diagnostics (11)
2. CachedPublic
3. CommandModuleExportMethods
4. DenyIncompleteValues
5. ExecuteExternalCodeInCommonModule
6. ForbiddenMetadataName
7. MetadataObjectNameLength
8. MissingCommonModuleMethod
9. MissingEventSubscriptionHandler
10. OrdinaryAppSupport
11. PrivilegedModuleMethodCall
12. ProtectedModule
13. QueryToMissingMetadata
14. SameMetadataObjectAndChildNames
15. ScheduledJobHandler
16. ServerCallsInFormEvents
17. ServerSideExportFormMethod
18. SetPermissionsForNewObjects
19. UsingSynchronousCalls
20. WrongDataPathForFormElements
21. WrongHttpServiceHandler
22. WrongWebServiceHandler
23. And ~18 more metadata-dependent diagnostics

### Phase 4: SDBL (Iterations 24-25) - ~16 diagnostics

**Query language analysis:**

1. AssignAliasFieldsInQuery
2. FieldsFromJoinsWithoutIsNull
3. FullOuterJoinQuery
4. IncorrectUseLikeInQuery
5. JoinWithSubQuery
6. JoinWithVirtualTable
7. LogicalOrInJoinQuerySection
8. LogicalOrInTheWhereSectionOfQuery
9. MultilineStringInQuery
10. QueryNestedFieldsByDot
11. QueryParseError
12. RefOveruse
13. SelectTopWithoutOrderBy
14. UnionAll
15. UsingLikeInQuery
16. VirtualTableCallWithoutParameters

### Phase 5: Advanced (Later) - ~10 diagnostics

**Special infrastructure:**

1. CommentedCode (code heuristics)
2. DuplicateStringLiteral (cross-file analysis)
3. Typo (spell checker integration)
4. MultilingualString* (2 diagnostics)
5. Data flow diagnostics (DuplicatedInsertionIntoCollection, FunctionOutParameter, MissingTemp*, RewriteMethodParameter)

---

## Configuration Schema

All diagnostics support configuration via `.bsl-language-server.json`:

```json
{
  "diagnostics": {
    "mode": "on",           // "on", "off", "only", "except"
    "parameters": {
      // Disable specific diagnostic
      "DiagnosticKey": false,

      // Configure diagnostic parameters
      "LineLength": {
        "maxLineLength": 140
      },
      "CyclomaticComplexity": {
        "complexityThreshold": 20,
        "checkModuleBody": true
      },
      "BadWords": {
        "words": "хрень,дурацкий"
      }
    }
  }
}
```

**Configuration modes:**
- `"on"` (default): All enabled diagnostics run
- `"off"`: No diagnostics run
- `"only"`: Run only listed diagnostics
- `"except"`: Run all except listed

---

## Testing Strategy

For each diagnostic:

1. **Copy test fixtures** from Java project to Rust:
   - `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/XxxDiagnostic.bsl`
   - Copy to: `crates/ide-diagnostics/test_data/xxx_diagnostic/`

2. **Port test cases** from Java to Rust:
   - `/Users/kiriller/src/lsp/bsl-language-server/src/test/java/.../XxxDiagnosticTest.java`
   - Port to: `crates/ide-diagnostics/src/diagnostics/xxx_diagnostic.rs`

3. **Verify identical results:**
   - Same line numbers
   - Same column numbers
   - Same diagnostic ranges
   - Same diagnostic messages

4. **Use snapshot testing:**
   ```rust
   #[test]
   fn test_canonical_spelling_keywords() {
       check_diagnostics(r#"
   Процедура Тест()
       если Истина тогда  // CanonicalSpellingKeywords: expected "Если"
       КонецЕсли;
   КонецПроцедуры
   "#);
   }
   ```

---

## Final Notes

**Total Diagnostics:** 180 (excluding BSLDiagnostic base class)

**Migration Strategy:**
1. ✅ Move **strictly alphabetically** (A → Z)
2. ✅ Implement infrastructure **immediately** when needed
3. ✅ Ensure **100% test compatibility** with Java version
4. ✅ Skip metadata-dependent diagnostics until Iteration 11 complete
5. ✅ Skip SDBL diagnostics until SDBL AST complete

**Key Infrastructure Requirements:**
- ✅ **Salsa integration** (Iteration 10) - for incremental computation
- ✅ **Metadata loader** (Iteration 11) - for ~40 diagnostics
- ✅ **CFG construction** - for control flow diagnostics
- ✅ **SDBL AST completion** - for 16 query diagnostics
- ✅ **Symbol table** (Iterations 6-9) - for 50+ diagnostics

---

**Previous:** [MIGRATION_PLAN_N_S.md](./MIGRATION_PLAN_N_S.md)

**Index:** [README.md](./README.md) - Diagnostics Migration Plan Index
