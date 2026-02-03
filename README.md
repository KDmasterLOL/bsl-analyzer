# BSL Analyzer

High-performance Language Server for BSL (1C:Enterprise) written in Rust.

## Goals

- **4x faster analysis** compared to bsl-language-server (Java)
- **4x lower memory usage** compared to bsl-language-server
- **100% compatibility** with bsl-language-server diagnostics and configuration
- Drop-in replacement for SonarQube integration

## Project Status

**Phase: Research & Planning**

See [docs/planning/ROADMAP.md](docs/planning/ROADMAP.md) for detailed development plan.

## Installation

### Linux

```bash
curl -fsSL https://dev.runsystems.ru/releases/bsl-analyzer/$(curl -fsSL https://dev.runsystems.ru/releases/bsl-analyzer/latest)/bsl-launcher-linux-amd64 -o ~/.local/bin/bsl-analyzer && chmod +x ~/.local/bin/bsl-analyzer
```

### Windows (PowerShell)

```powershell
$v = Invoke-RestMethod https://dev.runsystems.ru/releases/bsl-analyzer/latest
Invoke-WebRequest "https://dev.runsystems.ru/releases/bsl-analyzer/$v/bsl-launcher-windows-amd64.exe" -OutFile bsl-analyzer.exe
```

### macOS (Apple Silicon)

```bash
curl -fsSL https://dev.runsystems.ru/releases/bsl-analyzer/$(curl -fsSL https://dev.runsystems.ru/releases/bsl-analyzer/latest)/bsl-launcher-darwin-arm64 -o /usr/local/bin/bsl-analyzer && chmod +x /usr/local/bin/bsl-analyzer
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
git clone http://gitlab.runsystems.ru/proit/bsl-analyzer.git
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
- **`./scripts/ci-status.sh`** — проверка статуса GitLab CI

**Проверка CI:**
```bash
# Последний pipeline
./scripts/ci-status.sh

# Конкретный pipeline
./scripts/ci-status.sh 564
```

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
- **GitLab CI** — автоматическая проверка на каждый push

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
