# Diagnostics Migration Plan: N-S (Part 3/4)

## Overview

This document contains the migration plan for diagnostics starting with letters **N through S** (45 diagnostics).

---

## Diagnostics: N-S

### 107-110. Non*/Number/One/Order

#### 107. NonExportMethodsInApiRegion
**Code:** `NonExportMethodsInApiRegion`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`
**Scope:** `ALL`

**Requires:** AST (region + export method detection)
**Notes:** Check for non-export methods in `#Область ПрограммныйИнтерфейс`
**Good early diagnostic**

---

#### 108. NonStandardRegion
**Code:** `NonStandardRegion`
**Type:** `CODE_SMELL` | **Severity:** `INFO`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`
**Scope:** `BSL`

**Requires:** AST (region name validation)
**Notes:** Check region names against standard list
**Good early diagnostic**

---

#### 109. NumberOfOptionalParams
**Code:** `NumberOfOptionalParams`
**Type:** `CODE_SMELL` | **Severity:** `MINOR`
**Enabled:** ✅ Yes | **Minutes:** 30
**Tags:** `STANDARD`, `BRAINOVERLOAD`
**Scope:** `ALL`

**Configuration:**
```json
{"NumberOfOptionalParams": {"maxOptionalParamsCount": 3}}
```

**Requires:** AST (parameter counting)
**Notes:** Count optional parameters with default values
**Good early diagnostic**

---

#### 110. NumberOfParams
**Code:** `NumberOfParams`
**Type:** `CODE_SMELL` | **Severity:** `MINOR`
**Enabled:** ✅ Yes | **Minutes:** 30
**Tags:** `STANDARD`, `BRAINOVERLOAD`
**Scope:** `ALL`

**Configuration:**
```json
{"NumberOfParams": {"maxParamsCount": 7}}
```

**Requires:** AST (parameter counting)
**Notes:** Count total method parameters
**Good early diagnostic**

---

#### 111. NumberOfValuesInStructureConstructor
**Code:** `NumberOfValuesInStructureConstructor`
**Type:** `CODE_SMELL` | **Severity:** `MINOR`
**Enabled:** ✅ Yes | **Minutes:** 10
**Tags:** `STANDARD`, `BRAINOVERLOAD`
**Scope:** `ALL`

**Configuration:**
```json
{"NumberOfValuesInStructureConstructor": {"maxValuesCount": 3}}
```

**Requires:** AST (New Structure argument counting)
**Notes:** Count Structure constructor values
**Good early diagnostic**

---

#### 112. OneStatementPerLine
**Code:** `OneStatementPerLine`
**Type:** `CODE_SMELL` | **Severity:** `MINOR`
**Enabled:** ✅ Yes | **Minutes:** 2
**Tags:** `STANDARD`, `DESIGN`
**Scope:** `ALL`

**Requires:** Token + line tracking
**Notes:** Detect multiple statements on one line (`A = 1; B = 2;`)
**Good early diagnostic**

---

#### 113. OrderOfParams
**Code:** `OrderOfParams`
**Russian:** Порядок параметров метода
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 30
**Tags:** `STANDARD`, `DESIGN`
**Scope:** `ALL`

**Requires:** AST (parameter order validation)
**Notes:** Required params must come before optional
**Good early diagnostic**

---

### 114-115. Ordinary/OS

#### 114. OrdinaryAppSupport
**Code:** `OrdinaryAppSupport`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`, `UNPREDICTABLE`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata (configuration OrdinaryApplication flag)
**Notes:** **Defer to Iteration 19**

---

#### 115. OSUsersMethod
**Code:** `OSUsersMethod`
**Type:** `SECURITY_HOTSPOT` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 15
**Tags:** `SUSPICIOUS`
**Scope:** `BSL`

**Requires:** AST (ПользователиОС/OSUsers call detection)
**Notes:** Security diagnostic - flag OS user access

---

### 116-117. Pairing/Parse

#### 116. PairingBrokenTransaction
**Code:** `PairingBrokenTransaction`
**Russian:** Нарушение парности транзакций
**Type:** `ERROR` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 15
**Tags:** `STANDARD`
**Scope:** `ALL`

**Requires:** ⚠️ CFG (transaction pairing analysis)
**Notes:** Check BeginTransaction/CommitTransaction/RollbackTransaction pairing
**Defer until CFG available**

---

#### 117. ParseError
**Code:** `ParseError`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `ERROR`
**Scope:** `ALL`

**Requires:** ✅ Parser (already reports errors)
**Notes:** **Already implemented** - wrap parser errors as diagnostic

---

### 118-120. Privileged/Procedure/Protected

#### 118. PrivilegedModuleMethodCall
**Code:** `PrivilegedModuleMethodCall`
**Type:** `SECURITY_HOTSPOT` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 60
**Tags:** `SUSPICIOUS`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata (privileged module detection) + cross-module calls
**Notes:** **Defer to Iteration 19**

---

#### 119. ProcedureReturnsValue
**Code:** `ProcedureReturnsValue`
**Russian:** Процедура не должна возвращать значение
**Type:** `ERROR` | **Severity:** `BLOCKER`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `ERROR`
**Scope:** `ALL`

**Requires:** AST (return in procedure check)
**Notes:** **Good early diagnostic** - check for Return with value in Procedure

---

#### 120. ProtectedModule
**Code:** `ProtectedModule`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `BADPRACTICE`, `SUSPICIOUS`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata (module properties)
**Notes:** **Defer to Iteration 19**

---

### 121. PublicMethodsDescription

**Code:** `PublicMethodsDescription`
**Type:** `CODE_SMELL` | **Severity:** `INFO`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`, `BRAINOVERLOAD`, `BADPRACTICE`
**Scope:** `ALL`

**Sources:**
- **Java:** `/Users/kiriller/src/lsp/bsl-language-server/src/main/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/PublicMethodsDescriptionDiagnostic.java`
- **Rust Reference:** ✅ `/Users/kiriller/src/lsp/bsl-language-server-rust/crates/bsl-diagnostics/src/rules/public_methods_description.rs`
- **Target:** `/Users/kiriller/src/lsp/bsl-analyzer/crates/ide-diagnostics/src/handlers/public_methods_description.rs`

**Requires:** AST (doc comment parsing for export methods)
**Notes:** Check for missing doc comments on export methods

---

### 122-126. Query* Diagnostics (5 SDBL diagnostics)

#### 122. QueryNestedFieldsByDot
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Tags:** `STANDARD`, `SQL`, `PERFORMANCE`
**Requires:** ⚠️ SDBL AST
**Notes:** **Defer until SDBL**

---

#### 123. QueryParseError
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Tags:** `STANDARD`, `SQL`, `BADPRACTICE`
**Requires:** ⚠️ SDBL parser
**Notes:** Report SDBL parsing errors as diagnostic

---

#### 124. QueryToMissingMetadata
**Type:** `ERROR` | **Severity:** `BLOCKER`
**Tags:** `SUSPICIOUS`, `SQL`
**Requires:** ⚠️ SDBL + Metadata
**Notes:** **Defer to Iteration 19**

---

### 127-130. Redundant/Ref/Reserved/Rewrite

#### 127. RedundantAccessToObject
**Code:** `RedundantAccessToObject`
**Type:** `CODE_SMELL` | **Severity:** `INFO`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`, `CLUMSY`
**Scope:** `BSL`

**Requires:** AST (redundant `.Ref` access detection)
**Notes:** Detect `Object.Ref.Property` instead of `Object.Property`

---

#### 128. RefOveruse
**Code:** `RefOveruse`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `SQL`, `PERFORMANCE`
**Scope:** `BSL`

**Requires:** ⚠️ SDBL + semantic analysis
**Notes:** Detect excessive `.Ref` in queries - **defer until SDBL**

---

#### 129. ReservedParameterNames
**Code:** `ReservedParameterNames`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `STANDARD`, `BADPRACTICE`
**Scope:** `ALL`

**Requires:** AST (parameter name validation)
**Notes:** Check parameter names against reserved words list
**Good early diagnostic**

---

#### 130. RewriteMethodParameter
**Code:** `RewriteMethodParameter`
**Russian:** Перезапись параметра метода
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 2
**Tags:** `SUSPICIOUS`
**Scope:** `ALL`

**Requires:** ⚠️ Data flow (assignment tracking)
**Notes:** Detect parameter reassignment - **defer to Iteration 17**

---

### 131-135. Same/Scheduled/Select/Self/Semicolon

#### 131. SameMetadataObjectAndChildNames
**Code:** `SameMetadataObjectAndChildNames`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 30
**Tags:** `STANDARD`, `SQL`, `DESIGN`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata
**Notes:** **Defer to Iteration 19**

---

#### 132. ScheduledJobHandler
**Code:** `ScheduledJobHandler`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `ERROR`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata (scheduled job validation)
**Notes:** **Defer to Iteration 19**

---

#### 133. SelectTopWithoutOrderBy
**Code:** `SelectTopWithoutOrderBy`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `STANDARD`, `SQL`, `SUSPICIOUS`
**Scope:** `BSL`

**Requires:** ⚠️ SDBL AST
**Notes:** **Defer until SDBL**

---

#### 134. SelfAssign
**Code:** `SelfAssign`
**Russian:** Присваивание переменной самой себе
**Type:** `ERROR` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 10
**Tags:** `SUSPICIOUS`
**Scope:** `ALL`

**Requires:** AST (assignment analysis)
**Notes:** Detect `A = A` patterns - **good early diagnostic**

---

#### 135. SelfInsertion
**Code:** `SelfInsertion`
**Type:** `ERROR` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 10
**Tags:** `STANDARD`, `UNPREDICTABLE`, `PERFORMANCE`
**Scope:** `ALL`

**Requires:** AST (collection method analysis)
**Notes:** Detect `Collection.Add(Collection)` - **good early diagnostic**

---

#### 136. SemicolonPresence
**Code:** `SemicolonPresence`
**Type:** `CODE_SMELL` | **Severity:** `MINOR`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`, `BADPRACTICE`
**Scope:** `ALL`

**Requires:** Token (semicolon presence check)
**Notes:** Check for missing semicolons - **good early diagnostic**

---

### 137-141. Server/Set/Several

#### 137. ServerCallsInFormEvents
**Code:** `ServerCallsInFormEvents`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 15
**Tags:** `DESIGN`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata (form module + event handler detection)
**Notes:** **Defer to Iteration 19**

---

#### 138. ServerSideExportFormMethod
**Code:** `ServerSideExportFormMethod`
**Type:** `ERROR` | **Severity:** `BLOCKER`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `ERROR`, `UNPREDICTABLE`, `SUSPICIOUS`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata (form module + compilation directive)
**Notes:** **Defer to Iteration 19**

---

#### 139. SetPermissionsForNewObjects
**Code:** `SetPermissionsForNewObjects`
**Type:** `VULNERABILITY` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`, `BADPRACTICE`, `DESIGN`
**Scope:** `BSL`

**Requires:** ⚠️ Metadata (configuration properties)
**Notes:** **Defer to Iteration 19**

---

#### 140. SetPrivilegedMode
**Code:** `SetPrivilegedMode`
**Type:** `SECURITY_HOTSPOT` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `SUSPICIOUS`
**Scope:** `BSL`

**Requires:** AST (УстановитьПривилегированныйРежим call)
**Notes:** Security diagnostic - flag privileged mode usage

---

#### 141. SeveralCompilerDirectives
**Code:** `SeveralCompilerDirectives`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `UNPREDICTABLE`, `ERROR`
**Scope:** `ALL`

**Requires:** AST (annotation counting)
**Notes:** Detect multiple conflicting compilation directives on method

---

### 142-145. Space/Style/Temp/Ternary

#### 142. SpaceAtStartComment
**Code:** `SpaceAtStartComment`
**Type:** `CODE_SMELL` | **Severity:** `INFO`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `STANDARD`
**Scope:** `ALL`

**Requires:** Token (comment formatting check)
**Notes:** Check for space after `//` - **good early diagnostic**

---

#### 143. StyleElementConstructors
**Code:** `StyleElementConstructors`
**Type:** `ERROR` | **Severity:** `MINOR`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `STANDARD`, `BADPRACTICE`
**Scope:** `BSL`

**Requires:** AST (Style/Font constructor detection)
**Notes:** Detect deprecated style constructors

---

#### 144. TempFilesDir
**Code:** `TempFilesDir`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `STANDARD`, `BADPRACTICE`
**Scope:** `BSL`

**Requires:** AST (КаталогВременныхФайлов call)
**Notes:** Detect temp directory usage without cleanup

---

#### 145. TernaryOperatorUsage
**Code:** `TernaryOperatorUsage`
**Type:** `CODE_SMELL` | **Severity:** `MINOR`
**Enabled:** ❌ **No**
**Minutes:** 3
**Tags:** `BRAINOVERLOAD`
**Scope:** `ALL`

**Requires:** AST (ternary operator detection)
**Notes:** Flag all ternary operator usage (style preference)

---

### 146-151. This/Timeouts/TooMany/Transferring/Try/Typo

#### 146. ThisObjectAssign
**Code:** `ThisObjectAssign`
**Type:** `ERROR` | **Severity:** `BLOCKER`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `ERROR`
**Scope:** `BSL`

**Requires:** AST (ЭтотОбъект assignment detection)
**Notes:** Detect `ThisObject = ...` - **good early diagnostic**

---

#### 147. TimeoutsInExternalResources
**Code:** `TimeoutsInExternalResources`
**Type:** `ERROR` | **Severity:** `CRITICAL`
**Enabled:** ✅ Yes | **Minutes:** 5
**Tags:** `UNPREDICTABLE`, `STANDARD`
**Scope:** `ALL`

**Requires:** ⚠️ AST + connection object analysis
**Notes:** Check HTTP/FTP/SOAP timeout configuration

---

#### 148. TooManyReturns
**Code:** `TooManyReturns`
**Type:** `CODE_SMELL` | **Severity:** `MINOR`
**Enabled:** ❌ **No**
**Minutes:** 20
**Tags:** `BRAINOVERLOAD`
**Scope:** `ALL`

**Configuration:**
```json
{"TooManyReturns": {"maxReturnsCount": 3}}
```

**Requires:** AST (return statement counting)
**Notes:** Count return statements in method

---

#### 149. TransferringParametersBetweenClientAndServer
**Code:** `TransferringParametersBetweenClientAndServer`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 2
**Tags:** `BADPRACTICE`, `PERFORMANCE`, `STANDARD`
**Scope:** `ALL`

**Requires:** ⚠️ Compilation directive + call analysis
**Notes:** Check for large data transfer between client/server

---

#### 150. TryNumber
**Code:** `TryNumber`
**Type:** `CODE_SMELL` | **Severity:** `MAJOR`
**Enabled:** ✅ Yes | **Minutes:** 2
**Tags:** `STANDARD`
**Scope:** `ALL`

**Requires:** AST (Число/Number call in try-catch)
**Notes:** Detect number conversion in try-catch (suggest alternative)

---

#### 151. Typo
**Code:** `Typo`
**Type:** `CODE_SMELL` | **Severity:** `INFO`
**Enabled:** ✅ Yes | **Minutes:** 1
**Tags:** `BADPRACTICE`
**Scope:** `ALL`

**Requires:** ⚠️ Spell checker integration (hunspell/aspell)
**Notes:** Typo detection in comments/strings - **defer to later iteration**

---

## Summary: N-S Diagnostics

| Category | Count | Notes |
|----------|-------|-------|
| **Simple (Syntax-only)** | 20 | NonExportMethodsInApiRegion, NonStandardRegion, NumberOfOptionalParams, NumberOfParams, NumberOfValuesInStructureConstructor, OneStatementPerLine, OrderOfParams, ProcedureReturnsValue, ReservedParameterNames, SelfAssign, SelfInsertion, SemicolonPresence, SetPrivilegedMode, SeveralCompilerDirectives, SpaceAtStartComment, ThisObjectAssign, TooManyReturns, etc. |
| **Medium (Symbol Table)** | 8 | OSUsersMethod, PublicMethodsDescription, RedundantAccessToObject, StyleElementConstructors, TempFilesDir, TernaryOperatorUsage, TimeoutsInExternalResources, TryNumber |
| **Complex (Metadata)** | 9 | OrdinaryAppSupport, PrivilegedModuleMethodCall, ProtectedModule, SameMetadataObjectAndChildNames, ScheduledJobHandler, ServerCallsInFormEvents, ServerSideExportFormMethod, SetPermissionsForNewObjects |
| **Complex (CFG/Data Flow)** | 2 | PairingBrokenTransaction, RewriteMethodParameter |
| **SDBL** | 4 | QueryNestedFieldsByDot, QueryParseError, SelectTopWithoutOrderBy, RefOveruse, QueryToMissingMetadata |
| **Special** | 2 | ParseError (already impl), Typo (spell checker) |
| **Disabled by default** | 2 | TernaryOperatorUsage, TooManyReturns |
| **Total** | **45** | |

**Recommended Implementation Order:**
1. **Simple diagnostics:** NonExportMethodsInApiRegion, NonStandardRegion, NumberOfOptionalParams, NumberOfParams, OrderOfParams, SelfAssign, SelfInsertion, SemicolonPresence, SpaceAtStartComment, ThisObjectAssign
2. **Method/region checks:** PublicMethodsDescription, ReservedParameterNames
3. **Security:** OSUsersMethod, SetPrivilegedMode
4. **Special cases:** ParseError (wrap existing), SeveralCompilerDirectives
5. Defer **SDBL** until SDBL AST complete
6. Defer **Metadata** to Iteration 19
7. Defer **CFG/Data flow** to Iteration 17
8. Defer **Typo** to later iteration (spell checker integration)

---

**Next:** [MIGRATION_PLAN_T_Z.md](./MIGRATION_PLAN_T_Z.md) - Diagnostics T through Z
