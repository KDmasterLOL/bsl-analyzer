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
bsl-analyzer
```

### Analysis Mode (for SonarQube)

```bash
bsl-analyzer analyze --project ./my-project --output report.json
```

## Configuration

Compatible with `.bslls.json` format from bsl-language-server.

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

### Running tests

```bash
cargo test --all
```

### Project structure

- `crates/bsl-analyzer` - Main LSP server binary
- `crates/lexer` - Lexical analysis
- `crates/parser` - Parsing
- `crates/syntax` - Syntax trees (Rowan-based)
- `crates/hir` - High-level IR
- `crates/ide` - IDE features
- `crates/ide-diagnostics` - All 181 diagnostics
- `docs/` - Documentation

## License

MIT OR Apache-2.0
