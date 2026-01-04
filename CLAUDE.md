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
- **project-model** - Project configuration (.bslls.json support)
- **intern** / **stdx** - Utilities
- **profile** - Profiling utilities
- **test-fixture** / **test-utils** - Testing infrastructure

## Critical Development Rules

### 1. Always Check Library Documentation First

Before using any external crate, consult its current documentation using the Context7 MCP tool:
- Use `resolve-library-id` to find the library
- Use `query-docs` to get up-to-date documentation
- Key libraries: `rowan`, `salsa`, `logos`, `lsp-types`, `lsp-server`

### 2. Reference Source Projects

When implementing features, reference these source projects (see `docs/planning/SOURCES.md`):
- **rust-analyzer** (`/Users/kiriller/src/lsp/rust-analyzer/`) - Architecture patterns, Rowan/Salsa usage
- **bsl-language-server** (`/Users/kiriller/src/lsp/bsl-language-server/`) - Compatibility target (diagnostics, config, metadata)
- **bsl-parser** (`/Users/kiriller/src/lsp/bsl-parser/`) - BSL/SDBL grammar (ANTLR4)
- **tree-sitter-bsl** (`/Users/kiriller/src/lsp/tree-sitter-bsl/`) - Operator precedence, test cases
- **bsl-language-server-rust** (`/Users/kiriller/src/lsp/bsl-language-server-rust/`) - Existing Rust components (diagnostics, metadata)
- **salsa** (`/Users/kiriller/src/lsp/salsa/`) - Incremental computation framework (v0.25.2)

### 3. Logging: Use tracing, Never println!

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

### 4. Code Must Be Self-Documenting

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

### 5. Tests Are Mandatory

- All new functionality requires tests
- Never break existing tests without understanding why
- If tests fail, fix the code or update tests with justification
- Use snapshot tests (`expect-test`) for parser/AST
- Copy test fixtures from source projects into our repo (no external paths)

### 6. No Warnings Allowed

```bash
# Must pass with no warnings before commit
cargo clippy --all-targets --all-features -- -D warnings
```

Use `#[allow(...)]` only with explanation comment.

### 7. Self-Contained Project

All test files must be copied into this repository. Never use absolute paths to external projects:

**Correct:**
```rust
let input = include_str!("fixtures/Module.bsl");
```

**Wrong:**
```rust
let path = "/Users/kiriller/src/lsp/bsl-parser/...";  // ❌
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
- **docs/planning/ROADMAP.md** - Development plan with progress tracking
- **docs/planning/DIAGNOSTICS_MIGRATION.md** - Plan for 181 diagnostics migration

**Development:**
- **docs/contributing/DEVELOPMENT_RULES.md** - Development guidelines
- **docs/contributing/CONTRIBUTING.md** - Contribution process

## Compatibility Requirements

Must maintain 100% compatibility with bsl-language-server:
- Same diagnostic codes
- Same severity levels
- Same configuration format (`.bslls.json`)
- Same parameters for diagnostics
