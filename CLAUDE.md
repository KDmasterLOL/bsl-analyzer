# CLAUDE.md

Guidance for Claude Code working in the bsl-analyzer codebase.

## Project

LSP server for BSL (1C:Enterprise), written in Rust.

## Quick reference

```bash
cargo build --release
cargo test --workspace
cargo clippy --all-targets --all-features -- -D warnings

./scripts/setup-hooks.sh           # install pre-commit hooks (fmt + clippy + tests)
UPDATE_EXPECT=1 cargo test         # accept new snapshot baselines

# Run the LSP server locally
cargo run -p bsl-analyzer --bin bsl-analyzer-app -- lsp
BSL_LOG=debug cargo run -p bsl-analyzer --bin bsl-analyzer-app -- lsp
BSL_PROFILE='*' cargo run -p bsl-analyzer --bin bsl-analyzer-app -- lsp
```

## Architecture

```
bsl-analyzer (LSP server / CLI binary)
  └── ide                     — high-level API (hover, completion, refs, …)
      ├── ide-diagnostics     — diagnostic registry (HIR + AST + dataflow)
      ├── ide-assists         — code actions
      └── ide-db              — Salsa database
          └── hir / hir-def / hir-ty   — semantics (ItemTree, SymbolTree, infer)
              ├── cfg + dataflow       — CFG, reaching defs, liveness
              ├── sdbl-hir             — query language HIR
              └── syntax (Rowan)       — full-fidelity AST/CST
                  └── parser → lexer
```

| Layer    | Crates                                            |
|----------|---------------------------------------------------|
| Analysis | `lexer`, `parser`, `syntax`                       |
| Semantic | `hir-def`, `hir-ty`, `hir`                        |
| IDE      | `ide-db`, `ide-diagnostics`, `ide-assists`, `ide` |
| SDBL     | `sdbl-hir`                                        |
| Dataflow | `cfg`, `dataflow`                                 |
| Metadata | `bsl-metadata`, `bsl-platform`                    |
| Infra    | `base-db`, `vfs`, `project-model`                 |

- **Salsa 0.26.x** drives incremental computation (auto-invalidate on input change, LRU eviction).
- **Rowan** for the syntax tree (immutable, full-fidelity, typed AST wrappers).
- **`bsl-platform`** is a process-wide singleton seeded from HBK dumps (`shcntx_ru.hbk`, `shlang_ru.hbk`). The generated `crates/bsl-platform/data/platform_data.json` is checked in; regeneration steps are in `crates/bsl-platform/data/PROVENANCE.md` and `docs/contributing/DEVELOPMENT_RULES.md`.
- **`DiagnosticMetadata`** — compile-time const metadata per diagnostic; never hardcode severity / tags inline, always `ctx.severity(code)` / `ctx.tags(code)`.

Detailed reference: `docs/architecture/ARCHITECTURE.md`, `docs/contributing/DEVELOPMENT_RULES.md`.

## Development rules

0. **Commit format — Conventional Commits**. `feat:` / `fix:` / `chore:` / `test:` / `docs:` / `refactor:`, scope in parens (`feat(ide-diagnostics): …`). Full convention in `CONTRIBUTING.md`.

1. **Library docs first**. Before reaching for an unfamiliar external crate, use Context7 (`resolve-library-id` → `query-docs`). Key crates worth re-checking: `rowan`, `salsa`, `logos`, `lsp-types`.

2. **LSP for navigation**. Hover / goto-definition / find-references / document-symbol / call-hierarchy beat grep when LSP can answer. Use Read for known paths and Grep only for true text search.

3. **Logging via `tracing` only**. `println!`, `eprintln!`, `dbg!` are forbidden in library crates (CLI binaries are the exception). Use spans for hot paths:
   ```rust
   let _span = tracing::info_span!("parse_file", len = input.len()).entered();
   ```

4. **Self-documenting code**. Comments explain WHY, not WHAT. Doc-comments (`///`) for public API. No commented-out code, no obvious-restate-of-code comments.

5. **Tests are mandatory** for new functionality. Use `expect-test` snapshots for parser/AST output. Fixtures live in the repo (`include_str!("fixtures/...")`) — no absolute paths, no per-machine references. **New diagnostic** = handler module + `DiagnosticMetadata` registration + test fixture + `crates/ide-diagnostics/docs/{en,ru}/<Code>.md`; full route in `CONTRIBUTING.md`.

6. **No warnings, no hook bypass**:
   - `cargo clippy --all-targets --all-features -- -D warnings` must pass.
   - `git commit --no-verify` / `git push --no-verify` are forbidden — fix the hook failure.
   - `#[allow(...)]` requires a written rationale right next to it.

## BSL language

- **Bilingual** identifiers, case-insensitive: `Процедура` ≡ `Procedure`.
- **Preprocessor**: `#Если`, `#Область`, `#Использовать`.
- **Annotations**: `&НаКлиенте`, `&НаСервере`, `&До`, `&После`, `&Вместо`.

## Общие правила

- Менять код только с согласия пользователя.
- Запрашивать установку пакетов — не искать альтернативы самостоятельно.
- **Слойная архитектура (Martin clean)**. Каждое решение живёт в одном слое из диаграммы выше — никакого дублирования логики между слоями.
  Где что лежит:
  - синтаксис → `lexer` / `parser` / `syntax`;
  - семантика, резолюция имён, инференс типов → `hir-ty` / `hir-def`;
  - эмиссия диагностик и форматирование сообщений → `ide-diagnostics`;
  - IDE-фичи (hover / completion / refs / actions) → `ide` / `ide-assists`.
  Перед написанием кода ответь себе: "в каком слое это решение и почему?". Если ответ "в нескольких" — это запах: либо логика принадлежит одному из них, либо нужен helper в общем нижнем слое.
- **Lowering работает без `db`**. `hir-def/body/lower` принимает **только синтаксические** решения. Любое решение, которое требует типа receiver'а, резолвера или конфигурации, живёт в `hir-ty` (см. cascade gate в `infer.rs::dispatch_bare_ident_field_call` как образец). Адаптеры (`ide-diagnostics`, `ide-completion`, …) — тонкие проекции, без собственной бизнес-логики.
- Удалять неиспользуемый код, не оставлять заглушки.
- Не использовать regex для парсинга / семантики BSL — есть AST / HIR / SDBL API. Regex допустим только для инфраструктурных утилит (поиск по тексту, форматтеры вывода).
- Push только на `origin` (не на `github` mirror).
