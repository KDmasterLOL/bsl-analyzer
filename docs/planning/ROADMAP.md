# BSL Analyzer - Roadmap

## 🎯 Текущий статус проекта

**Дата обновления:** 2025-12-29

### ✅ Выполнено (Iterations 1-2):
- Структура проекта (15 крейтов)
- Lexer с поддержкой 80+ токенов (26 тестов ✅)
- Parser для BSL (expressions, statements, preprocessor)
- Preprocessor directives (#Если, #Область, #Удаление, #Вставка)
- Performance: 225 MB/s (превышает цель в 4.5 раза)
- CI/CD с GitLab CI (форматирование, clippy)
- Документация (архитектура, правила разработки, логирование)
- **Tracing инфраструктура** (BSL_LOG, BSL_PROFILE, BSL_LOG_FILE)

### 🔄 В процессе (Iteration 4):
- **Syntax Trees (Rowan)** - интеграция CST/AST - **IN PROGRESS**
  - ✅ SyntaxKind enum (120+ variants)
  - ✅ BslLanguage trait
  - ✅ SyntaxTreeBuilder
  - ✅ Базовые AST wrappers
  - ✅ Адаптация parser для генерации событий (34/36 тестов ✅)
  - ⏳ Полный набор AST wrappers
  - ⏳ SyntaxNodePtr

### 📋 Следующие шаги (Iteration 4+):
1. **SDBL parsing** - для поддержки встроенных запросов (Iteration 3)
2. **Base Infrastructure** - VFS, Salsa, SourceDatabase (Iteration 5)

### 📊 Прогресс по фазам:
- Phase 1 (Foundation): **80% завершено** (Iterations 1-2 ✅, 3-4 🔄)
- Phase 2 (Semantic Analysis): 0%
- Phase 3 (Diagnostics): 0%
- Phase 4 (LSP Integration): 0%

---

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

### Iteration 1: Project Setup & Lexer Foundation ✅ COMPLETED
**Источники:** bsl-parser (BSLLexer.g4), bsl-language-server-rust (bsl_tokenizer.rs), tree-sitter-bsl (grammar.js), rust-analyzer (tracing/)

**Статус:** Все основные задачи выполнены

- [x] Создать структуру проекта (15 крейтов)
- [x] Настроить CI/CD (GitLab CI с форматированием и clippy)
- [x] Документация по логированию (tracing)
  - Архитектура описана в ARCHITECTURE.md
  - Правила использования в DEVELOPMENT_RULES.md #7
  - План внедрения в ROADMAP.md
- [x] Реализовать tracing инфраструктуру
  - Config с поддержкой фильтров и writers
  - Hierarchical profiler (hprof) с агрегацией
  - Environment переменные: BSL_LOG, BSL_PROFILE, BSL_LOG_FILE
  - Интеграция в main.rs
- [x] Реализовать базовый лексер BSL (26 тестов, все проходят)
  - Поддержка 80+ токенов (keywords, operators, literals)
  - Билингвальные ключевые слова (RU/EN)
  - Preprocessor directives
  - Annotations (&НаКлиенте, &До, и т.д.)
- [x] Покрыть лексер тестами (26 unit тестов)
- [x] Документация по токенам BSL (inline doc comments в lib.rs)

### Iteration 2: Parser Foundation ✅ COMPLETED
**Источники:** bsl-parser (BSLParser.g4), rust-analyzer (parser/grammar/), tree-sitter-bsl (приоритеты операторов, тесты)

**Статус:** Все задачи выполнены

- [x] Реализовать грамматику BSL (top-level)
  - Функции и процедуры (включая Async)
  - Параметры с export/значениями по умолчанию
  - Объявления переменных
  - Compiler directives (&НаКлиенте и т.д.)
  - Annotations (&До, &После, и т.д.)
- [x] Expressions parsing
  - Бинарные операторы (арифметика, сравнение, логика)
  - Унарные операторы (-, Не/Not)
  - Тернарный оператор (?)
  - Вызовы функций с аргументами
  - Доступ к полям и индексация
  - New выражения
  - Литералы (числа, строки, даты, булевы)
- [x] Statements parsing
  - If/ElsIf/Else
  - While/For/ForEach
  - Try/Except
  - Return/Break/Continue
  - Goto/Label
  - Execute/AddHandler/RemoveHandler
  - Присваивание и вызовы
- [x] Error recovery базовый
  - Marker pattern для восстановления
  - p.error() при неожиданных токенах
  - Iteration limit для защиты от бесконечных циклов
  - **TODO:** Добавить spans для логирования (после реализации tracing)

### Iteration 3: Complete Parser 🔄 IN PROGRESS
**Источники:** bsl-parser (BSLParser.g4, SDBLParser.g4)

**Статус:** Preprocessor реализован, SDBL остается

- [x] Preprocessor directives
  - #Если/ИначеЕсли/Иначе/КонецЕсли
  - #Область/КонецОбласти
  - #Удаление/КонецУдаления
  - #Вставка/КонецВставки
  - Поддержка логических выражений (НЕ, И, ИЛИ)
  - Символы платформ (Клиент, Сервер, Linux, Windows и т.д.)
  - Вложенность директив
- [x] Regions (интеграция в source_file, preprocessor_if)
- [ ] SDBL (Query language) parsing ⚠️ TODO
  - Референс: `bsl-parser/src/main/antlr/SDBLParser.g4`
  - Токены: `bsl-parser/src/main/antlr/SDBLLexer.g4`
  - Rust пример: `bsl-language-server-rust/crates/bsl-parser/src/sdbl_tokenizer.rs`
  - **Приоритет:** P2 (важно для query диагностик)
- [x] Полное покрытие тестами
  - 34 unit тестов (все проходят)
  - 2 performance тесты (225 MB/s в release)
  - Тестовые данные скопированы в fixtures/
  - Поддержка файлов 1+ MB

**Performance:**
- Debug: 41.65 MB/s (25ms для 1.04 MB файла)
- Release: 225.80 MB/s (4.6ms для 1.04 MB файла)
- ✅ Превышает цель (50 MB/s) в 4.5 раза

### Iteration 4: Syntax Trees (Rowan) 🔄 IN PROGRESS
**Источники:** rust-analyzer (syntax/)

**Статус:** Базовая инфраструктура реализована, осталась адаптация parser

- [x] Интеграция с Rowan 0.15.17
  - BslLanguage trait реализован
  - Референс: `rust-analyzer/crates/syntax/src/lib.rs`
- [x] SyntaxKind enum
  - 120+ variants (токены + composite nodes)
  - Все BSL keywords, operators, literals, statements, expressions
  - Preprocessor directives и annotations
- [x] GreenNode / SyntaxNode / SyntaxToken
  - SyntaxTreeBuilder для построения деревьев
  - Parse<T> result type
  - SyntaxError with text ranges
- [x] Базовые AST typed wrappers
  - AstNode и AstToken traits
  - SourceFile, ProcedureDef, FunctionDef
  - Референс: `rust-analyzer/crates/syntax/src/ast/`
- [ ] SyntaxNodePtr (pointer to syntax node)
- [ ] Адаптация parser для генерации событий
  - Нужно перевести parser на Output со событиями для SyntaxTreeBuilder
- [ ] Полный набор AST wrappers
  - Statements, expressions, preprocessor nodes

**Тесты:** 6/6 проходят

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
