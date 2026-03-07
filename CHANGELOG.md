# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Date-based Versioning](docs/contributing/VERSIONING.md).

## [Unreleased]

### Project Setup

- Initial project structure with 15 crates
- Basic lexer with BSL tokens (Russian/English keywords)
- Event-based parser foundation
- Syntax tree structure using Rowan
- IDE diagnostics infrastructure (85+ diagnostic codes defined)
- LSP server skeleton

### Documentation

- ROADMAP with 30 iterations plan
- SOURCES with 5 reference projects
- ARCHITECTURE overview
- DIAGNOSTICS_MIGRATION plan (181 diagnostics)
- LSP_IMPLEMENTATION plan
- Development rules and versioning policy
- CONTRIBUTING guide

### CI/CD

- GitLab CI configuration (fmt, clippy, test, build)
- CI status checker script (`scripts/ci-status.sh`)

### Development Tools

- Pre-commit hooks for automatic fmt/clippy checks
- EditorConfig for consistent code style
- Helper scripts for common tasks

---

## Future Releases

Releases will follow the `YYYY.MM.DD` format as defined in [VERSIONING.md](docs/contributing/VERSIONING.md).

### Planned Phases

- **Alpha (0.x)** - Current phase: Foundation and basic functionality
- **Beta** - 100+ diagnostics, basic LSP support
- **RC** - 181 diagnostics, full LSP compatibility
- **Stable** - Production ready

---

[Unreleased]: https://github.com/itrous/bsl-analyzer
