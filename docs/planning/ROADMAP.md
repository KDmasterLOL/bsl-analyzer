# BSL Analyzer - Roadmap

## Цели проекта

**Основная цель:** Создать высокопроизводительный Language Server для BSL (1C:Enterprise) на Rust с полной обратной совместимостью с bsl-language-server (Java).

**Целевые метрики:**
- Время анализа проекта уровня 1C ERP: < 15 минут (сейчас ~60 минут)
- Потребление памяти: < 2.5 GB (сейчас ~10 GB)
- Совместимость диагностик: 100% (181 диагностика)

## Проекты-источники

Подробное описание см. в [SOURCES.md](./SOURCES.md).

| Проект | Путь | Роль |
|--------|------|------|
| **rust-analyzer** | `/Users/kiriller/src/lsp/rust-analyzer/` | Образец архитектуры (Rowan, Salsa, event-based parser) |
| **bsl-language-server** | `/Users/kiriller/src/lsp/bsl-language-server/` | Целевая совместимость (181 диагностика, .bslls.json) |
| **bsl-parser** | `/Users/kiriller/src/lsp/bsl-parser/` | Грамматика BSL/SDBL (ANTLR4 .g4 файлы) |
| **tree-sitter-bsl** | `/Users/kiriller/src/lsp/tree-sitter-bsl/` | Альтернативная грамматика BSL (приоритеты операторов, тесты) |
| **bsl-language-server-rust** | `/Users/kiriller/src/lsp/bsl-language-server-rust/` | Готовые Rust компоненты (183 диагностики, tree-sitter) |

## Приоритеты

| Приоритет | Направление | Обоснование |
|-----------|-------------|-------------|
| P0 | Парсинг и AST | Фундамент для всего функционала |
| P0 | Базовая инфраструктура (VFS, DB) | Необходим для инкрементального анализа |
| P1 | Диагностики (критические) | Основной функционал для SonarQube |
| P1 | LSP Server базовый | Интеграция с IDE |
| P2 | Диагностики (все остальные) | Полное покрытие функционала |
| P2 | Navigation (Go to Definition) | IDE функциональность |
| P3 | Assists и Code Actions | Улучшение UX |
| P3 | Formatting | Опциональный функционал |

## Верхнеуровневая декомпозиция

### Phase 1: Foundation (Фундамент)
**Срок: Итерации 1-5**

1. **Lexer** - лексический анализатор BSL
2. **Parser** - парсер BSL с error recovery
3. **Syntax** - CST/AST на базе Rowan
4. **Base-DB** - инкрементальная база данных на Salsa
5. **VFS** - виртуальная файловая система

### Phase 2: Semantic Analysis (Семантический анализ)
**Срок: Итерации 6-10**

1. **HIR** - High-level IR
2. **HIR-Def** - определения и разрешение имён
3. **Symbol Table** - таблица символов
4. **IDE-DB** - база данных для IDE

### Phase 3: Diagnostics (Диагностики)
**Срок: Итерации 11-25**

1. **IDE-Diagnostics** - инфраструктура диагностик
2. Миграция 181 диагностики из bsl-language-server
3. Тесты с полным покрытием

### Phase 4: LSP Integration (LSP интеграция)
**Срок: Итерации 26-30**

1. **bsl-analyzer** crate - LSP сервер
2. Поддержка всех LSP capabilities
3. CLI интерфейс

### Phase 5: Advanced Features (Продвинутые функции)
**Срок: Итерации 31+**

1. **IDE-Assists** - Code Actions
2. Форматирование
3. Семантические токены

## Детальная декомпозиция по итерациям

### Iteration 1: Project Setup & Lexer Foundation
**Источники:** bsl-parser (BSLLexer.g4), bsl-language-server-rust (bsl_tokenizer.rs), tree-sitter-bsl (grammar.js)

- [x] Создать структуру проекта
- [ ] Настроить CI/CD (GitHub Actions)
- [ ] Реализовать базовый лексер BSL
  - Референс: `bsl-parser/src/main/antlr/BSLLexer.g4` (40+ токенов)
  - Rust пример: `bsl-language-server-rust/crates/bsl-parser/src/bsl_tokenizer.rs`
- [ ] Покрыть лексер тестами
- [ ] Документация по токенам BSL

### Iteration 2: Parser Foundation
**Источники:** bsl-parser (BSLParser.g4), rust-analyzer (parser/grammar/), tree-sitter-bsl (приоритеты операторов, тесты)

- [ ] Реализовать грамматику BSL (top-level)
  - Референс: `bsl-parser/src/main/antlr/BSLParser.g4` (правила file, subs, procedure, function)
  - Архитектура: `rust-analyzer/crates/parser/src/grammar/`
- [ ] Expressions parsing
  - Референс: BSLParser.g4 (expression, member, operation)
- [ ] Statements parsing
  - Референс: BSLParser.g4 (statement, ifStatement, whileStatement, forStatement)
- [ ] Error recovery базовый
  - Паттерн: rust-analyzer Marker pattern

### Iteration 3: Complete Parser
**Источники:** bsl-parser (BSLParser.g4, SDBLParser.g4)

- [ ] Preprocessor directives
  - Референс: BSLParser.g4 (preprocessor, preproc_if, regionStart/End)
  - Режимы: BSLLexer.g4 (PREPROCESSOR_MODE)
- [ ] Regions
- [ ] SDBL (Query language) parsing
  - Референс: `bsl-parser/src/main/antlr/SDBLParser.g4`
  - Токены: `bsl-parser/src/main/antlr/SDBLLexer.g4`
  - Rust пример: `bsl-language-server-rust/crates/bsl-parser/src/sdbl_tokenizer.rs`
- [ ] Полное покрытие тестами
  - Тестовые данные: `bsl-parser/src/test/resources/`

### Iteration 4: Syntax Trees (Rowan)
**Источники:** rust-analyzer (syntax/)

- [ ] Интеграция с Rowan
  - Референс: `rust-analyzer/crates/syntax/src/lib.rs`
- [ ] GreenNode / SyntaxNode
- [ ] AST typed wrappers
  - Референс: `rust-analyzer/crates/syntax/src/ast/`
- [ ] SyntaxNodePtr

### Iteration 5: Base Infrastructure
**Источники:** rust-analyzer (base-db/, vfs/)

- [ ] VFS (Virtual File System)
  - Референс: `rust-analyzer/crates/vfs/`
- [ ] Salsa integration
  - Референс: `rust-analyzer/crates/base-db/src/lib.rs`
- [ ] SourceDatabase
- [ ] FileId, SourceRootId

### Iteration 6-7: HIR Foundation
**Источники:** rust-analyzer (hir/, hir-def/), bsl-language-server-rust (bsl-symbols/)

- [ ] HIR basic structures
  - Референс: `rust-analyzer/crates/hir/src/lib.rs`
- [ ] Module representation
- [ ] Method/Function representation
  - Rust пример: `bsl-language-server-rust/crates/bsl-symbols/src/lib.rs` (Symbol, SymbolKind)
- [ ] Variable representation

### Iteration 8-9: Symbol Resolution
**Источники:** rust-analyzer (hir-def/), bsl-language-server-rust (bsl-symbols/)

- [ ] SymbolTree
  - Rust пример: `bsl-language-server-rust/crates/bsl-symbols/` (SymbolTable)
- [ ] Name resolution
- [ ] Scope analysis
- [ ] Export/Import handling

### Iteration 10: IDE-DB
**Источники:** rust-analyzer (ide-db/)

- [ ] RootDatabase
  - Референс: `rust-analyzer/crates/ide-db/src/`
- [ ] Cached queries
- [ ] Incremental updates
- [ ] Benchmarks

### Iterations 11-25: Diagnostics Migration
**Источники:** bsl-language-server (diagnostics/), bsl-language-server-rust (rules/)

См. [DIAGNOSTICS_MIGRATION.md](./DIAGNOSTICS_MIGRATION.md)

**Ключевые референсы:**
- Java реализации: `bsl-language-server/src/main/java/.../diagnostics/`
- Rust реализации: `bsl-language-server-rust/crates/bsl-diagnostics/src/rules/`
- Тестовые данные: `bsl-language-server/src/test/resources/diagnostics/`
- Registry pattern: `bsl-language-server-rust/crates/bsl-diagnostics/src/registry.rs`

### Iterations 26-30: LSP Server
**Источники:** rust-analyzer, bsl-language-server, bsl-language-server-rust

См. [LSP_IMPLEMENTATION.md](./LSP_IMPLEMENTATION.md)

**Ключевые референсы:**
- LSP handlers: `rust-analyzer/crates/rust-analyzer/src/handlers/`
- Backend pattern: `bsl-language-server-rust/crates/bsl-lsp-server/src/backend.rs`
- Capabilities: `bsl-language-server/src/main/java/.../BSLLanguageServer.java`

## Критерии готовности итерации

1. Все тесты проходят (unit + integration)
2. Код review пройден
3. Документация обновлена
4. Бенчмарки не деградировали
5. CI/CD green

## Метрики успеха

| Метрика | Текущее (Java) | Цель (Rust) | Улучшение |
|---------|----------------|-------------|-----------|
| Время анализа ERP | 60 мин | 15 мин | 4x |
| Память при анализе | 10 GB | 2.5 GB | 4x |
| Cold start | 10 сек | 1 сек | 10x |
| Incremental update | 500 мс | 50 мс | 10x |
