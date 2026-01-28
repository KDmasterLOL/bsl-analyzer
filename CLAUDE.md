# CLAUDE.md

Guidance for Claude Code when working with bsl-analyzer codebase.

## Project Overview

**BSL Analyzer** - High-performance LSP for BSL (1C:Enterprise) in Rust. Target: drop-in replacement for bsl-language-server with 5x speed, 2.7x less memory.

**Performance (doc3: 121 MB, 6,540 files):** 11.2s vs 58.9s Java (5.3x faster), 1.4 GB vs 3.8 GB memory (2.7x less)

See `docs/planning/PERFORMANCE_REAL_DATA.md` for benchmarks.

## Quick Reference

```bash
# Build & Test
cargo build --release
cargo test --all
cargo clippy --all-targets --all-features -- -D warnings

# Development
./scripts/setup-hooks.sh          # Install pre-commit hooks (fmt + clippy)
UPDATE_EXPECT=1 cargo test        # Update snapshot tests

# Debugging
BSL_LOG=debug cargo run           # Enable debug logs
BSL_PROFILE=* cargo run           # Enable profiling
```

## Architecture

```
bsl-analyzer (LSP Server)
  └── ide (High-level API)
      ├── ide-diagnostics (180/181 diagnostics, 99%)
      ├── ide-assists (Code actions)
      └── ide-db (Salsa database)
          └── hir / hir-def / hir-ty (Semantic analysis)
              ├── cfg + dataflow (CFG, reaching defs)
              ├── sdbl-hir (Query language HIR)
              └── syntax (Rowan CST/AST)
                  └── parser (Event-based) → lexer (logos)
```

### Key Components

**Salsa 0.25.2** - Incremental computation, automatic cache invalidation, LRU eviction (128-512 files)

**Rowan** - Immutable CST, full-fidelity parsing, typed AST wrappers

**DiagnosticMetadata** - Zero-cost metadata system (180 const definitions, 100% coverage):
- Compile-time const metadata + runtime JSON overrides
- `ctx.severity(code)`, `ctx.tags(code)` instead of hardcoded values
- Automatic LSP severity mapping from type+severity
- 100% compatible with Java `@DiagnosticMetadata`

**HIR Diagnostics** - Collected during HIR lowering, cached by Salsa, see `docs/architecture/ARCHITECTURE.md`

**Metadata** - `bsl-metadata` crate: Configuration, CommonModule, XML parsing, Salsa integration

**Dataflow** - `cfg` + `dataflow` crates: CFG construction, reaching definitions, liveness analysis

### Crate Structure

| Layer | Crates | Purpose |
|-------|--------|---------|
| **Analysis** | lexer, parser, syntax | Tokenization (80+ BSL, 150+ SDBL), Rowan CST |
| **Semantic** | hir-def, hir-ty, hir | ItemTree, SymbolTree, type inference |
| **IDE** | ide-db, ide-diagnostics, ide-assists, ide | Database, 180 diagnostics, code actions |
| **SDBL** | sdbl-hir | Query language HIR + type inference |
| **Dataflow** | cfg, dataflow | CFG, reaching defs, liveness |
| **Metadata** | bsl-metadata, bsl-platform | Configuration, platform types |
| **Infra** | base-db, vfs, project-model | Salsa, VFS, config |

## Development Rules

### 1. Use Library Docs First
Before using external crates: `resolve-library-id` → `query-docs` (Context7 MCP). Key libs: rowan, salsa, logos, lsp-types.

### 2. LSP for Navigation
**Use:** hover (types), goToDefinition, findReferences, documentSymbol, workspaceSymbol, call hierarchy
**Don't use:** when you know exact path (use Read), text search (Grep), patterns (Glob)

### 3. Reference Sources
**Primary reference:**
- `~/src/lsp/rust-analyzer/` - Architecture patterns, diagnostics infrastructure, call hierarchy, search/usages

**BSL specifics:**
- `~/src/lsp/bsl-parser/` - BSL/SDBL grammar (ANTLR4)
- `~/src/lsp/bsl-language-server/` - Diagnostic compatibility (codes, messages, config format only)

**Priority:** rust-analyzer > bsl-parser > bsl-language-server

See `docs/architecture/SOURCES.md` for details

### 4. Logging: tracing only
```rust
use tracing::{debug, info, warn, error};
let _span = tracing::info_span!("parse_file", len = input.len()).entered();
```
**Forbidden:** `println!`, `eprintln!`, `dbg!` (except CLI output in binaries)

### 5. Self-Documenting Code
**Allowed comments:** non-obvious logic, SAFETY, issue refs, doc comments (`///`)
**Forbidden:** duplicating code, obvious statements, commented-out code

### 6. Tests Mandatory
- All new functionality requires tests
- Use snapshot tests (`expect-test`) for parser/AST
- Copy fixtures into repo (no external paths)

**Test diagnostics with helpers:**
```rust
assert_diagnostic_range_multiline(&code, &diagnostics[0], 3, 0, 5, 13);
assert_diagnostic_range(&code, &diagnostics[0], 5, 1, 6);  // single line
check_hir_diagnostic(code)  // run HIR diagnostics
```

### 7. No Warnings, No Bypassing Hooks
```bash
cargo clippy --all-targets --all-features -- -D warnings  # Must pass
```
Use `#[allow(...)]` only with explanation.

**FORBIDDEN:** `git commit --no-verify` or `git push --no-verify`
- Pre-commit hooks are protection against bad code/text in git
- If hooks fail, FIX THE ISSUES, don't bypass them
- Bypassing hooks is NEVER acceptable

### 8. Self-Contained
All test files in repo. Never use absolute paths: `include_str!("fixtures/Module.bsl")` ✅

## BSL Language Specifics

- **Bilingual:** `Процедура` = `Procedure`, case-insensitive
- **Preprocessor:** `#Если`, `#Область`, `#Использовать`
- **Annotations:** `&НаКлиенте`, `&НаСервере`, `&До`, `&После`, `&Вместо`

## Status & Files

**Completed:** Lexer, Parser, Syntax, HIR, SDBL, Metadata, Dataflow, 180/181 diagnostics (99%), DiagnosticMetadata architecture, LSP server

**Next:** CrazyMultilineString diagnostic, SonarQube integration, IDE features (formatting, refactoring)

**Key Files:**
- `docs/architecture/ARCHITECTURE.md` - Architecture details
- `docs/architecture/SOURCES.md` - Source projects
- `docs/contributing/DEVELOPMENT_RULES.md` - Guidelines
- `docs/METADATA_COMPATIBILITY.md` - Metadata compatibility

## Compatibility & Architecture

**User-facing compatibility** with bsl-language-server (Java):
- 100% compatible diagnostic codes, messages, severity levels
- Same config format (`.bsl-analyzer.json` or `.bsl-language-server.json`)
- Same diagnostic parameters and metadata

**Architecture advantages** over Java implementation:
- ✅ **DiagnosticMetadata:** Compile-time const + runtime JSON (vs Java annotations only)
- ✅ **HIR-based diagnostics:** Collected during lowering, Salsa-cached (vs separate AST passes)
- ✅ **Text-based single pass:** One traversal for all text diagnostics (vs N separate passes)
- ✅ **Dataflow analysis:** CFG + liveness for intra-procedural checks (Java has limited CFG)
- 🚧 **CallGraph (planned):** Inter-procedural analysis infrastructure inspired by rust-analyzer

**Reference architecture:** rust-analyzer (not Java) for all new infrastructure

## Общие правила

- Менять код только с согласия пользователя
- Запрашивать установку пакетов, не искать альтернативы самостоятельно
- Чистая архитектура, единая точка истины, не дублировать код
- Удалять неиспользуемый код
