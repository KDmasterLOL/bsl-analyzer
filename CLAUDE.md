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
        ├── ide-diagnostics (181 diagnostics)
        ├── ide-assists (Code actions)
        └── ide-db (Database)
            └── hir (Semantic analysis)
                └── syntax (AST with Rowan)
                    └── parser (Event-based)
                        └── lexer (logos-based)
```

### Key Architectural Components

**Incremental Computation (Salsa 0.25.2):**
- **Repository:** `/Users/kiriller/src/lsp/salsa/`
- **Status:** Full integration planned for Iteration 10 (see `docs/planning/SALSA_TODO.md`)
- Uses Salsa framework for incremental computation
- Minimizes recomputation on file changes
- All queries are cached and invalidated automatically
- **Critical for performance:** Without Salsa, the project would be 10-100x slower
- **Key features:**
  - Automatic cache invalidation based on dependencies
  - LRU eviction (only 128-512 most recent files in memory)
  - Durability levels: HIGH for metadata (rarely change), LOW for source code
  - Thread-safe parallel computation with Rayon
- **Example:** User edits a comment → Salsa checks "did interface change?" → NO → returns cached result (20ms instead of 500ms)

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
- Each diagnostic is a separate module
- Uniform interface via `DiagnosticContext`
- Full compatibility with bsl-language-server codes
- **3 Tiers:**
  - Tier 1 (Syntax): ~60 diagnostics, fast AST checks
  - Tier 2 (Semantic): ~60 diagnostics, require HIR/symbols
  - Tier 3 (Metadata): ~40 diagnostics, require 1C metadata

**Metadata Infrastructure (Iteration 11):**
- **Status:** Planned (see `docs/planning/METADATA_PLAN.md`)
- **Critical for:** ~40 Tier 3 diagnostics, Navigation, SDBL analysis
- **What it is:** 1C:Enterprise metadata (Configuration, CommonModules, Catalogs, Documents, Registers)
- **Integration with Salsa:** Metadata loaded once (~1 sec), cached with `Durability::HIGH`, accessed in < 1ms
- **Key components:**
  - XML loader (parses Configuration.xml and other metadata files)
  - Metadata structures (Configuration, CommonModule, MetadataObject)
  - Salsa queries for efficient caching
  - AbstractMetadataDiagnostic pattern (ported from bsl-language-server)
- **Performance:** Metadata loading < 1 sec, cached access < 1ms (critical for large projects)

**ModuleGraph & Incremental CI (Iteration 9.5):**
- **Status:** Planned (see `docs/planning/INCREMENTAL_CI.md`)
- **Critical for:** CI/CD incremental analysis (5x-30x speedup), cross-module diagnostics, LSP navigation
- **What it is:** Dependency graph of BSL modules (CommonModules, ObjectModules, FormModules)
- **Key use cases:**
  - **GitLab CI incremental mode:** Analyze only changed modules + dependencies (pt_erp: 10-15 sec → 0.5-1 sec for typical commit)
  - **Graph-based diagnostics:** UnusedModule, CircularDependency, ModuleCoupling metrics
  - **LSP navigation:** Call Hierarchy, Find Usages across modules
- **Key components:**
  - ModuleGraph (Arena-based, like rust-analyzer's CrateGraph)
  - ModuleGraphBuilder with cycle detection
  - Dependency extraction from AST (function calls, #Использовать, metadata)
  - CLI: `--incremental --changed-files` or `--git-diff HEAD~1`
- **Performance (pt_erp, 25,090 modules):**
  - Full scan: 10-15 seconds
  - Incremental (1 module changed): 0.5-1 second (10x-30x faster)
  - Incremental (5 modules): 1-2 seconds (5x-15x faster)
- **Relation to Salsa:** Salsa provides incremental computation INSIDE analysis, ModuleGraph provides INPUT FILTERING for CI/CD

### Crate Structure

- **bsl-analyzer** - Main LSP server binary
- **lexer** - Tokenization using logos (80+ tokens, bilingual RU/EN keywords)
- **parser** - BSL grammar implementation with error recovery
- **syntax** - CST/AST using Rowan (120+ SyntaxKind variants, 23+ AST wrappers)
- **hir** / **hir-def** - High-level IR and semantic analysis
- **ide** - High-level API coordinating all subsystems
- **ide-db** - RootDatabase with Salsa integration
- **ide-diagnostics** - 181 diagnostics from bsl-language-server
- **ide-assists** - Code actions and refactorings
- **base-db** - Source database with Salsa
- **vfs** - Virtual file system
- **bsl-metadata** - 1C metadata (Configuration, CommonModule, etc.) with XML loader and Salsa integration
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

**Completed (Iterations 1-5):**
- ✅ Lexer with 80+ BSL tokens + 150+ SDBL tokens (49 tests passing)
- ✅ Parser for BSL (expressions, statements, preprocessor)
- ✅ SDBL infrastructure (tokens, parser entry point, SyntaxKind nodes)
- ✅ Syntax trees (Rowan integration with 23+ AST wrappers)
- ✅ **Base Infrastructure (VFS, SourceDatabase)** - Iteration 5 complete
  - VFS with change tracking (PathInterner, FileSet)
  - SourceDatabase traits (Files helper with DashMap caching)
  - Parse query with caching (82+ tests passing)
  - ⚠️ Full Salsa integration deferred (see `docs/planning/SALSA_TODO.md`)
- ✅ Performance: 225 MB/s parsing speed (4.5x faster than goal!)
- ✅ Tracing infrastructure (BSL_LOG, BSL_PROFILE, BSL_LOG_FILE)
- ✅ CI/CD with GitLab

**Next Steps:**
- **Iteration 6-9:** HIR Foundation & Symbol Resolution
- **Iteration 9.5:** ModuleGraph & Incremental CI Mode (dependency graph for 5x-30x CI speedup)
- **Iteration 10:** IDE-DB & Full Salsa 0.25.2 Integration
- **Iteration 11:** Metadata Infrastructure (Configuration, CommonModule, XML loader)
- **Iterations 12-25:** Diagnostics migration (181 diagnostics)
  - Tier 1 (Syntax): 12-14
  - Tier 2 (Semantic): 15-18
  - Tier 3 (Metadata): 19-23 ← **requires Iteration 11**
  - SDBL: 24-25
- **Iterations 26-30:** LSP Server integration

See `docs/planning/ROADMAP.md` for full 30-iteration plan.

## Important Files

**Architecture & Planning:**
- **docs/architecture/ARCHITECTURE.md** - Detailed architecture documentation (Salsa, Rowan, Metadata)
- **docs/planning/ROADMAP.md** - 30-iteration development plan with progress tracking
- **docs/planning/SOURCES.md** - Source projects and their roles (rust-analyzer, bsl-language-server, salsa)
- **docs/planning/ITERATIONS.md** - Detailed breakdown of each iteration
- **docs/planning/SALSA_TODO.md** - Plan for Salsa 0.25.2 integration (Iteration 10)
- **docs/planning/METADATA_PLAN.md** - Plan for 1C metadata infrastructure (Iteration 11)
- **docs/planning/INCREMENTAL_CI.md** - ✅ **NEW:** ModuleGraph for incremental CI/CD (Iteration 9.5, 5x-30x speedup)
- **docs/planning/PERFORMANCE_REAL_DATA.md** - ✅ **Real measurements** from pt_erp project (121 MB, 1 hour → 10-15 sec)
- **docs/planning/PERFORMANCE_ESTIMATES.md** - Performance extrapolations for projects up to 4GB
- **docs/planning/DIAGNOSTICS_MIGRATION.md** - Plan for 181 diagnostics migration

**Development:**
- **docs/contributing/DEVELOPMENT_RULES.md** - Comprehensive development guidelines
- **docs/contributing/CONTRIBUTING.md** - Contribution process
- **docs/contributing/VERSIONING.md** - Versioning policy

## Compatibility Requirements

Must maintain 100% compatibility with bsl-language-server:
- Same diagnostic codes
- Same severity levels
- Same configuration format (`.bslls.json`)
- Same parameters for diagnostics
