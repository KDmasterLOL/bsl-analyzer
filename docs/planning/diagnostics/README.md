# Diagnostics Migration Plan - Index

## Overview

This directory contains the detailed migration plan for all **180 diagnostics** from bsl-language-server to bsl-analyzer.

## Migration Principles

1. **Alphabetical Order:** Migrate diagnostics strictly A→Z, one by one
2. **Infrastructure First:** Implement required infrastructure **immediately** when discovered
3. **100% Compatibility:** Match bsl-language-server exactly (line/column numbers, ranges, messages)
4. **Test-Driven:** Port all test cases from Java and verify identical results

## Plan Structure

### 📋 Main Migration Plan

**[ALPHABETICAL_MIGRATION_PLAN.md](./ALPHABETICAL_MIGRATION_PLAN.md)** - ⭐ **СТРОГО АЛФАВИТНЫЙ ПЛАН**

Полный список всех 180 диагностик в алфавитном порядке БЕЗ ПРОПУСКОВ.
Для каждой диагностики указано:
- Путь к Java файлу
- Путь к существующему Rust файлу (если есть - 149 диагностик уже реализованы!)
- Статус: ✅ Портировать / ❌ Реализовать с нуля
- Приоритет
- Зависимости

**[DIAGNOSTIC_MAPPING.md](./DIAGNOSTIC_MAPPING.md)** - Таблица сопоставления Java ↔ Rust

---

### Детальные планы по категориям

The plan is split into 4 files (~45 diagnostics each):

### [Part 1: A-C](./MIGRATION_PLAN_A_C.md) - 30 diagnostics

**A-C diagnostics** including:
- Simple: CanonicalSpellingKeywords, ConsecutiveEmptyLines, CodeBlockBeforeSub
- Medium: BeginTransactionBeforeTryCatch, CommitTransactionOutsideTryCatch
- Complex: AllFunctionPathMustHaveReturn (CFG), CognitiveComplexity, CyclomaticComplexity
- Metadata: All CommonModule* diagnostics (11), CachedPublic
- SDBL: AssignAliasFieldsInQuery

**Key Infrastructure Needs:**
- CFG (Control Flow Graph) for AllFunctionPathMustHaveReturn
- Complexity calculators for CognitiveComplexity, CyclomaticComplexity
- Metadata integration for CommonModule* diagnostics (defer to Iteration 19)

---

### [Part 2: D-M](./MIGRATION_PLAN_D_M.md) - 48 diagnostics

**D-M diagnostics** including:
- Simple: EmptyCodeBlock, EmptyRegion, EmptyStatement, DoubleNegatives, IdenticalExpressions
- Simple: LineLength, MagicNumber, MagicDate, MethodSize, MissingSpace
- Medium: DeprecatedCurrentDate, DeprecatedFind, DisableSafeMode
- Medium: IfConditionComplexity, If-ElseIf checks
- Metadata: DenyIncompleteValues, ForbiddenMetadataName, MetadataObjectNameLength
- SDBL: FieldsFromJoinsWithoutIsNull, FullOuterJoinQuery, JoinWithSubQuery

**Key Infrastructure Needs:**
- AST expression comparison for IfElseDuplicatedCodeBlock
- Code heuristics for CommentedCode
- SDBL AST for query diagnostics (defer until SDBL complete)

---

### [Part 3: N-S](./MIGRATION_PLAN_N_S.md) - 45 diagnostics

**N-S diagnostics** including:
- Simple: NumberOfParams, OrderOfParams, OneStatementPerLine, ProcedureReturnsValue
- Simple: ReservedParameterNames, SelfAssign, SelfInsertion, SemicolonPresence
- Simple: SpaceAtStartComment, ThisObjectAssign
- Medium: OSUsersMethod, PublicMethodsDescription, SetPrivilegedMode
- Metadata: PrivilegedModuleMethodCall, ScheduledJobHandler, ServerCallsInFormEvents
- SDBL: QueryNestedFieldsByDot, SelectTopWithoutOrderBy
- Special: ParseError (wrap existing), Typo (spell checker)

**Key Infrastructure Needs:**
- ParseError wrapper for existing parser errors
- CFG for PairingBrokenTransaction
- Data flow for RewriteMethodParameter (defer to Iteration 17)

---

### [Part 4: T-Z](./MIGRATION_PLAN_T_Z.md) - 56 diagnostics

**T-Z diagnostics** including:
- Simple: UnaryPlusInConcatenation, UnknownPreprocessorSymbol, UseLessForEach
- Simple: UselessTernaryOperator, UsingGoto, YoLetterUsage
- Medium: UnsafeSafeModeMethodCall, UsingCancelParameter, UsingExternalCodeTools
- Security: UsingHardcodeNetworkAddress, UsingHardcodePath, UsingHardcodeSecretInformation
- Metadata: WrongDataPathForFormElements, WrongHttpServiceHandler, WrongWebServiceHandler
- SDBL: UnionAll, UsingLikeInQuery, VirtualTableCallWithoutParameters

**Key Infrastructure Needs:**
- Security pattern detection (hardcoded secrets, IPs)
- Service tag parsing for UsingServiceTag
- CFG for UnreachableCode

---

## Quick Statistics

### Total Diagnostics: 180

| Category | Count | % |
|----------|-------|---|
| **Tier 1: Syntax-only** | ~60 | 33% |
| **Tier 2: Symbol Table** | ~60 | 33% |
| **Tier 3: Metadata** | ~40 | 22% |
| **Tier 4: SDBL** | ~16 | 9% |
| **Special** | ~4 | 2% |

### By Severity

| Severity | Count | % |
|----------|-------|---|
| **BLOCKER** | 12 | 7% |
| **CRITICAL** | 30 | 17% |
| **MAJOR** | 90 | 50% |
| **MINOR** | 30 | 17% |
| **INFO** | 18 | 10% |

### By Type

| Type | Count | % |
|------|-------|---|
| **CODE_SMELL** | 109 | 61% |
| **ERROR** | 56 | 31% |
| **VULNERABILITY** | 7 | 4% |
| **SECURITY_HOTSPOT** | 8 | 4% |

### By Default Status

| Status | Count | % |
|--------|-------|---|
| **Enabled** | ~172 | 96% |
| **Disabled** | ~8 | 4% |

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

---

## Implementation Phases

### Phase 1: Quick Wins (Iterations 12-13)

**~40 simple diagnostics** - validate infrastructure

**Examples:** CanonicalSpellingKeywords, LineLength, EmptyCodeBlock, SelfAssign, UsingGoto

**Dependencies:** Parser, AST, Token access

---

### Phase 2: Symbol Table (Iterations 14-18)

**~50 diagnostics** - semantic analysis

**Examples:** UnusedLocalVariable, FunctionShouldHaveReturn, DeprecatedCurrentDate

**Dependencies:** Symbol table, type inference, CFG

---

### Phase 3: Metadata (Iterations 19-23)

**~40 diagnostics** - 1C metadata integration

**Examples:** CommonModule* (11), ForbiddenMetadataName, ScheduledJobHandler

**Dependencies:** Metadata loader (Iteration 11), Configuration.xml parsing

---

### Phase 4: SDBL (Iterations 24-25)

**~16 diagnostics** - query language

**Examples:** AssignAliasFieldsInQuery, JoinWithSubQuery, SelectTopWithoutOrderBy

**Dependencies:** SDBL AST completion, semantic analysis

---

### Phase 5: Advanced (Later)

**~10 diagnostics** - special infrastructure

**Examples:** CommentedCode (heuristics), Typo (spell checker), DuplicateStringLiteral (cross-file)

**Dependencies:** Custom algorithms, external tools

---

## Configuration

All diagnostics are configurable via `.bsl-language-server.json`.

See [CONFIGURATION.md](./CONFIGURATION.md) for complete schema documentation.

**Quick example:**
```json
{
  "diagnostics": {
    "parameters": {
      "LineLength": {
        "maxLineLength": 140
      },
      "MethodSize": false
    }
  }
}
```

---

## Progress Tracking

**Current Status:** Planning phase complete

| Phase | Total | Completed | % |
|-------|-------|-----------|---|
| Phase 1 (Syntax) | 40 | 0 | 0% |
| Phase 2 (Semantic) | 50 | 0 | 0% |
| Phase 3 (Metadata) | 40 | 0 | 0% |
| Phase 4 (SDBL) | 16 | 0 | 0% |
| Phase 5 (Advanced) | 10 | 0 | 0% |
| **Total** | **156** | **0** | **0%** |

*Note: 24 diagnostics deferred to later iterations (metadata, data flow)*

---

## Quick Reference

### Good "First Diagnostics" to Implement

Start with these simple, well-defined diagnostics to validate infrastructure:

1. **CanonicalSpellingKeywords** - simple token check
2. **LineLength** - token + line tracking
3. **EmptyCodeBlock** - simple AST check
4. **CodeBlockBeforeSub** - simple AST check
5. **SemicolonPresence** - token check
6. **MagicNumber** - AST literal check
7. **SelfAssign** - AST assignment check
8. **UsingGoto** - AST statement check
9. **YoLetterUsage** - identifier text scan
10. **UnaryPlusInConcatenation** - AST expression check

### Most Critical Diagnostics (BLOCKER/CRITICAL)

These are high-severity errors that should be prioritized:

1. **CodeBlockBeforeSub** (BLOCKER) - syntax error
2. **CommonModuleAssign** (BLOCKER) - metadata error
3. **ForbiddenMetadataName** (BLOCKER) - metadata error
4. **GlobalContextMethodCollision8312** (BLOCKER) - name collision
5. **IncorrectUseOfStrTemplate** (BLOCKER) - runtime error
6. **ProcedureReturnsValue** (BLOCKER) - syntax error
7. **ThisObjectAssign** (BLOCKER) - syntax error
8. **UnaryPlusInConcatenation** (BLOCKER) - logic error
9. **UnsafeSafeModeMethodCall** (BLOCKER) - security error
10. **WrongUseFunctionProceedWithCall** (BLOCKER) - async error

---

## Source References

All diagnostics are ported from:

**Java Project:** `/Users/kiriller/src/lsp/bsl-language-server/`

**Diagnostics:** `src/main/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/`

**Tests:** `src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/`

**Test Data:** `src/test/resources/diagnostics/`

**Documentation:** https://1c-syntax.github.io/bsl-language-server/diagnostics/

---

## Related Documentation

- [ARCHITECTURE.md](../../architecture/ARCHITECTURE.md) - System architecture
- [ROADMAP.md](../ROADMAP.md) - 30-iteration development plan
- [ITERATIONS.md](../ITERATIONS.md) - Detailed iteration breakdown
- [METADATA_PLAN.md](../METADATA_PLAN.md) - Metadata infrastructure (Iteration 11)
- [SALSA_TODO.md](../SALSA_TODO.md) - Salsa integration (Iteration 10)
- [INCREMENTAL_CI.md](../INCREMENTAL_CI.md) - ModuleGraph for CI/CD (Iteration 9.5)

---

**Last Updated:** 2025-12-30

**Status:** ✅ Planning Complete - Ready for Implementation
