# BSL Analyzer

High-performance Language Server for BSL (1C:Enterprise) written in Rust.

## Features

- **180+ diagnostics** for BSL code quality analysis
- **High performance** — 11s analysis of 121 MB / 6,540 files, 1.4 GB RAM
- **LSP support** — full Language Server Protocol integration
- **SonarQube integration** — SARIF reports, streaming mode for large projects
- **Compatibility** with `.bsl-language-server.json` configuration format
- **Cross-platform** — Linux, Windows, macOS (Apple Silicon)

## Project Status

**Phase: Active Development (Alpha)**

180 of 181 diagnostics implemented. See [docs/planning/ROADMAP.md](docs/planning/ROADMAP.md) for details.

## Installation

### Linux

```bash
curl -fsSL https://github.com/itrous/bsl-analyzer/releases/latest/download/bsl-launcher-linux-amd64 -o ~/.local/bin/bsl-analyzer && chmod +x ~/.local/bin/bsl-analyzer
```

### Windows (PowerShell)

```powershell
Invoke-WebRequest "https://github.com/itrous/bsl-analyzer/releases/latest/download/bsl-launcher-windows-amd64.exe" -OutFile bsl-analyzer.exe
```

### macOS (Apple Silicon)

```bash
curl -fsSL https://github.com/itrous/bsl-analyzer/releases/latest/download/bsl-launcher-darwin-arm64 -o /usr/local/bin/bsl-analyzer && chmod +x /usr/local/bin/bsl-analyzer
```

### Version Pinning (CI/CD)

```bash
# Use specific version
BSL_ANALYZER_VERSION=0.1.3 bsl-analyzer analyze -s ./src

# Or via command line
bsl-analyzer --launcher-use 0.1.3 analyze -s ./src
```

## Architecture

The project follows rust-analyzer architecture:

```
bsl-analyzer (LSP Server)
    └── ide (High-level API)
        ├── ide-diagnostics (181 diagnostics)
        ├── ide-assists (Code actions)
        └── ide-db (Database)
            └── hir (Semantic analysis)
                └── syntax (AST)
                    └── parser
                        └── lexer
```

See [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md) for details.

## Building

```bash
cargo build --release
```

## Usage

### LSP Server Mode

```bash
bsl-analyzer lsp
```

### Analysis Mode (for SonarQube)

```bash
# Console output
bsl-analyzer analyze -s ./my-project

# SARIF report
bsl-analyzer analyze -s ./my-project -r sarif -o ./reports

# Streaming mode for large projects (low memory)
bsl-analyzer analyze -s ./my-project --streaming --format=jsonl > report.jsonl
```

## Configuration

Uses `.bsl-analyzer.json` configuration format (also supports `.bsl-language-server.json` for compatibility with bsl-language-server).

```json
{
    "diagnostics": {
        "skip": ["CommentedCode"],
        "parameters": {
            "CyclomaticComplexity": {
                "complexityThreshold": 20
            }
        }
    }
}
```

## Development

### Quick Start

**Requirements:**
- Rust 1.91+ (`rustup install stable`)
- Git
- jq (для скрипта проверки CI)

**Setup:**

```bash
# Clone repository
git clone https://github.com/itrous/bsl-analyzer.git
cd bsl-analyzer

# Install pre-commit hooks (автоматический fmt и clippy)
./scripts/setup-hooks.sh

# Build
cargo build

# Run tests
cargo test --all

# Check formatting
cargo fmt --all -- --check

# Run clippy
cargo clippy --all-targets --all-features -- -D warnings
```

### Helper Scripts

- **`./scripts/setup-hooks.sh`** — установка git pre-commit hooks
- **`./scripts/ci-status.sh`** — проверка статуса CI

### Contributing

См. [CONTRIBUTING.md](CONTRIBUTING.md) для детальной информации о процессе разработки.

**Обязательно к прочтению:**
- [DEVELOPMENT_RULES.md](docs/contributing/DEVELOPMENT_RULES.md) — правила написания кода
- [VERSIONING.md](docs/contributing/VERSIONING.md) — политика версионирования
- [ROADMAP.md](docs/planning/ROADMAP.md) — план разработки (30 итераций)
- [SOURCES.md](docs/planning/SOURCES.md) — проекты-источники

### Project Structure

```
bsl-analyzer/
├── crates/
│   ├── bsl-analyzer/      # Main LSP server binary
│   ├── lexer/             # Lexical analysis
│   ├── parser/            # Parsing (event-based)
│   ├── syntax/            # Syntax trees (Rowan-based)
│   ├── hir/               # High-level IR
│   ├── hir-def/           # HIR definitions
│   ├── ide/               # IDE features API
│   ├── ide-db/            # IDE database
│   ├── ide-diagnostics/   # All 181 diagnostics
│   ├── ide-assists/       # Code actions
│   ├── base-db/           # Base database (Salsa)
│   ├── vfs/               # Virtual file system
│   ├── project-model/     # Project configuration
│   ├── profile/           # Profiling utilities
│   └── test-*/            # Testing infrastructure
├── docs/
│   ├── planning/          # Roadmap, iterations, migration plans
│   ├── architecture/      # Architecture documentation
│   └── contributing/      # Development rules, versioning
├── scripts/
│   ├── setup-hooks.sh     # Setup git hooks
│   ├── ci-status.sh       # Check GitLab CI status
│   └── pre-commit         # Pre-commit hook
└── xtask/                 # Build automation

```

### Code Quality

Проект использует:
- **rustfmt** — автоматическое форматирование
- **clippy** — линтинг
- **EditorConfig** — консистентные настройки редактора
- **Pre-commit hooks** — автоматические проверки перед коммитом
- **CI** — автоматическая проверка на каждый push

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
