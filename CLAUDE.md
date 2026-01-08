# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

BSL Analyzer is a high-performance Language Server for BSL (1C:Enterprise) written in Rust. The project follows rust-analyzer architecture and aims for 4x faster analysis and 4x lower memory usage compared to bsl-language-server (Java) while maintaining 100% compatibility.

**Target:** Drop-in replacement for bsl-language-server with SonarQube integration.

**Performance (Real Data):**

**✅ Real project doc3 (121 MB, 6,540 BSL files):**

- **Full analysis**: **11.2 seconds** (vs 58.9 seconds in Java) — **5.3x faster** ⚡
- **CPU efficiency**: 59.3s user time (vs 337.1s) — **5.7x less CPU** 🚀
- **I/O efficiency**: 2.8s system time (vs 28.8s) — **10.3x less I/O** 💾
- **Throughput**: 585 files/sec (vs 111 files/sec) — **5.3x higher** 📈
- **Peak memory**: **1,426 MB** (vs 3,822 MB) — **2.7x less** 💪

**Extrapolation for 4GB project (~33x larger):**

- **Full analysis**: **~6 minutes** (vs ~32 minutes in Java) — **5-6x faster**
- **Peak memory**: **~46 GB** (vs ~123 GB) — **2.7x less**

See `docs/planning/PERFORMANCE_REAL_DATA.md` for detailed benchmark data and methodology.

**Benchmark project:** `~/src/doc3` (121 MB, 6,540 BSL files)

## Common Commands

### Building and Testing

```bash
# Build the project
cargo build

# Build release version
cargo build --release

# Run all tests
cargo test --all

# Run tests for a specific crate
cargo test -p parser
cargo test -p lexer
cargo test -p syntax

# Update snapshot tests (after reviewing changes)
UPDATE_EXPECT=1 cargo test
```

### Code Quality

```bash
# Format code (required before commit)
cargo fmt --all

# Check formatting without applying
cargo fmt --all -- --check

# Run clippy (must pass with no warnings)
cargo clippy --all-targets --all-features -- -D warnings

# Quick validation
cargo check
```

### Development Workflow

```bash
# Install pre-commit hooks (runs fmt and clippy automatically)
./scripts/setup-hooks.sh

# Check GitLab CI status
./scripts/ci-status.sh
```

### Logging and Profiling

The project uses the `tracing` ecosystem for structured logging and profiling:

```bash
# Enable debug logs for specific module
BSL_LOG=parser=debug cargo run

# Enable all debug logs
BSL_LOG=debug cargo run

# Enable trace-level logs
BSL_LOG=trace cargo run

# Enable profiling for all operations
BSL_PROFILE=* cargo run

# Profile specific operations with filters
BSL_PROFILE=parse|analyze cargo run

# Write logs to file
BSL_LOG_FILE=/tmp/bsl.log BSL_LOG=debug cargo run

# Combine options
BSL_LOG=debug BSL_PROFILE=* BSL_LOG_FILE=/tmp/bsl.log cargo run
```

## Architecture Overview

The project uses a layered architecture inspired by rust-analyzer:

```
bsl-analyzer (LSP Server)
    └── ide (High-level API)
        ├── ide-diagnostics (~90 diagnostics implemented)
        ├── ide-assists (Code actions)
        └── ide-db (Database + Salsa)
            └── hir (Semantic analysis)
                ├── cfg (Control Flow Graph)
                └── syntax (AST with Rowan)
                    └── parser (Event-based)
                        └── lexer (logos-based)
```

### Key Architectural Components

**Incremental Computation (Salsa 0.25.2):**

- **Status:** ✅ Integrated
- Uses Salsa framework for incremental computation
- All queries are cached and invalidated automatically
- **Key features:**
  - Automatic cache invalidation based on dependencies
  - LRU eviction (only 128-512 most recent files in memory)
  - Durability levels: HIGH for metadata (rarely change), LOW for source code
  - Thread-safe parallel computation with Rayon

**Red-Green Trees (Rowan):**

- Immutable CST (Concrete Syntax Tree) representation
- Full-fidelity parsing (preserves all tokens including whitespace)
- Efficient memory sharing between versions
- Typed AST wrappers over untyped CST

**Event-Based Parser:**

- Parser generates events, not AST directly
- Enables error recovery
- Events are consumed by SyntaxTreeBuilder to create Rowan tree

**Diagnostic System:**

- Each diagnostic is a separate module in `ide-diagnostics/src/handlers/`
- Uniform interface via `DiagnosticContext`
- Full compatibility with bsl-language-server codes
- **~90 diagnostics implemented** (of 181 total planned)

**HIR-based Diagnostics (rust-analyzer pattern):**

- Diagnostics are collected as a byproduct of HIR lowering
- `BodyDiagnostic` enum in `hir-def/body.rs` — collected during AST→HIR transformation
- Each diagnostic stays in its own handler file with `from_hir()` function
- Salsa caching via `module_bodies()` query — diagnostics recomputed only when file changes
- See `docs/architecture/ARCHITECTURE.md` for detailed architecture

**Metadata Infrastructure:**

- **Status:** ✅ Implemented (`bsl-metadata` crate)
- Configuration, CommonModule, Register, EventSubscription structures
- XML loader for Designer format
- Salsa integration for caching

**ModuleGraph:**

- **Status:** ✅ Implemented (`module-graph` crate)
- Dependency graph for BSL modules
- Cycle detection and incremental CI support

**Control Flow Graph (CFG):**

- **Status:** ✅ Implemented (`cfg` crate)
- CFG construction from Rowan AST
- Used for flow-sensitive diagnostics

### Crate Structure

- **bsl-analyzer** - Main LSP server binary
- **lexer** - Tokenization using logos (80+ tokens, bilingual RU/EN keywords)
- **parser** - BSL grammar implementation with error recovery
- **syntax** - CST/AST using Rowan (120+ SyntaxKind variants, 23+ AST wrappers)
- **hir** / **hir-def** - High-level IR and semantic analysis
- **ide** - High-level API coordinating all subsystems
- **ide-db** - RootDatabase with Salsa integration
- **ide-diagnostics** - ~90 diagnostics implemented (of 181 planned)
- **ide-assists** - Code actions and refactorings
- **base-db** - Source database with Salsa
- **vfs** - Virtual file system
- **bsl-metadata** - 1C metadata (Configuration, CommonModule, etc.)
- **module-graph** - Module dependency graph for incremental CI
- **cfg** - Control Flow Graph for flow-sensitive analysis
- **project-model** - Project configuration (.bsl-analyzer.json support)
- **intern** / **stdx** - Utilities
- **profile** - Profiling utilities
- **test-fixture** / **test-utils** - Testing infrastructure

## Critical Development Rules

### 1. Always Check Library Documentation First

Before using any external crate, consult its current documentation using the Context7 MCP tool:

- Use `resolve-library-id` to find the library
- Use `query-docs` to get up-to-date documentation
- Key libraries: `rowan`, `salsa`, `logos`, `lsp-types`, `lsp-server`

### 2. Use LSP for Navigation and Code Intelligence

The `LSP` tool provides Rust language server integration for efficient code navigation. Use it when:

**When to use LSP:**

- **Exploring unfamiliar code** - Use `hover` to understand types, traits, and function signatures
- **Finding definitions** - Use `goToDefinition` to jump to symbol declarations instead of manual searching
- **Understanding API usage** - Use `findReferences` to see how a type or function is used across the codebase
- **Getting code structure** - Use `documentSymbol` to see all functions, structs, enums in a file
- **Finding symbols by name** - Use `workspaceSymbol` to locate types/functions across the entire project
- **Understanding call chains** - Use `prepareCallHierarchy`, `incomingCalls`, `outgoingCalls` to trace function calls

**When NOT to use LSP:**

- When you already know the exact file path and line number (use `Read` instead)
- For simple text search (use `Grep` instead)
- For file pattern matching (use `Glob` instead)
- When exploring multiple files in parallel (LSP is better for focused navigation)

**Examples:**

```
# Check what a function returns
LSP: operation=hover, line=378, character=10

# Find where a struct is defined
LSP: operation=goToDefinition, line=37, character=20

# See all usages of a function
LSP: operation=findReferences, line=100, character=15

# Get overview of file structure
LSP: operation=documentSymbol, line=1, character=1
```

### 3. Reference Source Projects

When implementing features, reference these source projects (see `docs/planning/SOURCES.md`):

- **rust-analyzer** (`~/src/lsp/rust-analyzer/`) - Architecture patterns, Rowan/Salsa usage
- **bsl-language-server** (`~/src/lsp/bsl-language-server/`) - Compatibility target (diagnostics, config, metadata)
- **bsl-parser** (`~/src/lsp/bsl-parser/`) - BSL/SDBL grammar (ANTLR4)
- **tree-sitter-bsl** (`~/src/lsp/tree-sitter-bsl/`) - Operator precedence, test cases
- **bsl-language-server-rust** (`~/src/lsp/bsl-language-server-rust/`) - Existing Rust components (diagnostics, metadata)
- **salsa** (`~/src/lsp/salsa/`) - Incremental computation framework (v0.25.2)

### 4. Logging: Use tracing, Never println

**Required:**

```rust
use tracing::{trace, debug, info, warn, error};

// Structured logging with fields
info!("parsing started");
debug!(file_id = ?file_id, "parsing file");

// Spans for profiling
pub fn parse_file(input: &str) -> Parse {
    let _span = tracing::info_span!("parse_file", len = input.len()).entered();
    // ... logic
}
```

**Forbidden:**

```rust
println!("Debug: {:?}", value);  // ❌ Never use for debugging
eprintln!("Error: {}", error);   // ❌ Never use
dbg!(value);                     // ❌ Never use
```

**Exception:** `println!` is acceptable only for CLI output in binary crates, not for debugging.

### 5. Code Must Be Self-Documenting

Minimize comments. Use expressive names, extract functions, and use type system instead.

**Allowed comments:**

- Explaining non-obvious logic (BSL language quirks, compatibility notes)
- SAFETY comments for unsafe code
- References to issues/specs
- Doc comments (`///`) for public API

**Forbidden comments:**

- Duplicating what code does
- Obvious statements
- Commented-out code
- Decorative separators

### 6. Tests Are Mandatory

- All new functionality requires tests
- Never break existing tests without understanding why
- If tests fail, fix the code or update tests with justification
- Use snapshot tests (`expect-test`) for parser/AST
- Copy test fixtures from source projects into our repo (no external paths)

**Testing diagnostics with Java fixtures:**

When porting diagnostics from bsl-language-server (Java), use the same test fixtures and verify that diagnostic positions match exactly:

```rust
// ✅ CORRECT: use helper methods with line/column positions
assert_diagnostic_range_multiline(&code, &diagnostics[0], 3, 0, 5, 13);
assert_diagnostic_range(&code, &diagnostics[0], 5, 1, 6);  // single line

// ❌ FORBIDDEN: magic numbers for TextRange (byte offsets)
assert_eq!(diagnostics[0].range, TextRange::new(42.into(), 156.into()));
```

**Available helpers** (`crates/ide-diagnostics/src/test_utils.rs`):

- `assert_diagnostic_range_multiline(code, diag, start_line, start_col, end_line, end_col)`
- `assert_diagnostic_range(code, diag, line, start_col, end_col)` — single line
- `check_hir_diagnostic(code)` — run HIR diagnostics on test code

**Why:** Java tests specify line/column positions. Using helpers ensures we match Java behavior exactly and makes tests readable.

### 7. No Warnings Allowed

```bash
# Must pass with no warnings before commit
cargo clippy --all-targets --all-features -- -D warnings
```

Use `#[allow(...)]` only with explanation comment.

### 8. Self-Contained Project

All test files must be copied into this repository. Never use absolute paths to external projects:

**Correct:**

```rust
let input = include_str!("fixtures/Module.bsl");
```

**Wrong:**

```rust
let path = "~/src/lsp/bsl-parser/...";  // ❌
```

## BSL Language Specifics

**Bilingual Keywords:**

- BSL supports both Russian and English keywords
- Example: `Процедура` = `Procedure`, `Функция` = `Function`
- Case-insensitive: `ПРОЦЕДУРА`, `процедура`, `Процедура` are all valid
- Preprocessor symbols are also case-insensitive

**Preprocessor Directives:**

- `#Если`, `#ИначеЕсли`, `#Иначе`, `#КонецЕсли`
- `#Область` / `#КонецОбласти` (regions)
- `#Использовать` (imports)

**Annotations:**

- Method annotations: `&НаКлиенте`, `&НаСервере`, `&НаКлиентеНаСервере`
- Compiler directives: `&До`, `&После`, `&Вместо`

## Current Development Status

**Completed:**

- ✅ Lexer with 80+ BSL tokens + 150+ SDBL tokens
- ✅ Parser for BSL (expressions, statements, preprocessor)
- ✅ SDBL infrastructure (tokens, parser, SyntaxKind nodes)
- ✅ Syntax trees (Rowan integration)
- ✅ Base Infrastructure (VFS, SourceDatabase with Salsa)
- ✅ HIR / hir-def (ItemTree, SymbolTree, type inference)
- ✅ Metadata Infrastructure (`bsl-metadata` crate)
- ✅ ModuleGraph (`module-graph` crate)
- ✅ Control Flow Graph (`cfg` crate)
- ✅ ~90 diagnostics implemented
- ✅ Tracing infrastructure (BSL_LOG, BSL_PROFILE, BSL_LOG_FILE)
- ✅ CI/CD with GitLab

**Next Steps:**

- Remaining ~91 diagnostics (of 181 total)
- LSP Server integration
- IDE features (hover, completion, etc.)

See `docs/planning/ROADMAP.md` for details.

## Important Files

**Architecture & Planning:**

- **docs/architecture/ARCHITECTURE.md** - Detailed architecture documentation
- **docs/architecture/SOURCES.md** - Source projects reference

**Development:**

- **docs/contributing/DEVELOPMENT_RULES.md** - Development guidelines
- **docs/contributing/CONTRIBUTING.md** - Contribution process

## Compatibility Requirements

Must maintain 100% compatibility with bsl-language-server:

- Same diagnostic codes
- Same severity levels
- Same configuration format (`.bsl-analyzer.json`, also supports `.bsl-language-server.json` for compatibility)
- Same parameters for diagnostics
