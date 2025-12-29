# BSL Analyzer - Roadmap

## 🎯 Текущий статус проекта

**Дата обновления:** 2025-12-30

### ✅ Выполнено (Iterations 1-3):
- Структура проекта (15 крейтов)
- Lexer с поддержкой 80+ BSL токенов (26 тестов ✅)
- **SDBL Lexer** с поддержкой 150+ токенов (23 теста ✅)
- Parser для BSL (expressions, statements, preprocessor)
- Preprocessor directives (#Если, #Область, #Удаление, #Вставка)
- **SDBL infrastructure** (токены, parser entry point, SyntaxKind nodes)
- Performance: 225 MB/s (превышает цель в 4.5 раза)
- CI/CD с GitLab CI (форматирование, clippy)
- Документация (архитектура, правила разработки, логирование)
- **Tracing инфраструктура** (BSL_LOG, BSL_PROFILE, BSL_LOG_FILE)

### ✅ Выполнено (Iteration 4):
- **Syntax Trees (Rowan)** - интеграция CST/AST - **COMPLETE**
  - ✅ SyntaxKind enum (130+ variants including SDBL)
  - ✅ BslLanguage trait
  - ✅ SyntaxTreeBuilder
  - ✅ Базовые AST wrappers (SourceFile, ProcedureDef, FunctionDef)
  - ✅ Адаптация parser для генерации событий (34/36 тестов ✅)
  - ✅ Полный набор AST wrappers (23 типов: statements, expressions, literals)
  - ✅ SyntaxNodePtr (lightweight node references)

### ✅ Выполнено (Iteration 5):
- **Base Infrastructure** - VFS, SourceDatabase - **COMPLETE**
  - ✅ VFS с отслеживанием изменений (content-based change detection)
  - ✅ PathInterner для FileId ↔ VfsPath mapping
  - ✅ FileSet для логической группировки файлов
  - ✅ SourceRoot и SourceRootId
  - ✅ SourceDatabase и RootQueryDb traits
  - ✅ Files helper с DashMap-based кешированием
  - ✅ FileChange для батчевых обновлений
  - ✅ Parse query с кешированием (82+ тестов ✅)
  - ⚠️ **Полная Salsa интеграция отложена** (см. docs/planning/SALSA_TODO.md)

### ✅ Выполнено (Iterations 6-8):
- **HIR Foundation & Symbol Resolution** - **COMPLETE**
  - ✅ ItemTree (Iteration 6-7): модель верхнего уровня (procedures, functions, variables)
  - ✅ SymbolTree (Iteration 8): быстрая таблица символов с O(1) поиском
  - ✅ Resolver: module-level scope resolution
  - ✅ Type System: базовые типы (Number, String, Boolean, Array, Function, etc.)
  - ✅ Semantics API: высокоуровневый API для IDE (resolve_method_call, symbol_at_position)
  - ✅ **Go to Definition** (`crates/ide/src/goto_definition.rs`): навигация к определению символов
  - ✅ **Find References** (`crates/ide/src/references.rs`): поиск всех использований символов
  - ✅ Регистронезависимое разрешение (BSL case-insensitive)
  - ✅ 40+ новых тестов (190 тестов всего ✅)
  - ✅ Clippy без предупреждений

### 📋 Следующие шаги (Iteration 9+):
1. **ModuleGraph & Incremental CI** - граф зависимостей для CI/CD (Iteration 9.5)
2. **IDE-DB & Salsa** - полная интеграция Salsa 0.25.2 (Iteration 10)
3. **Metadata Infrastructure** - работа с метаданными 1С (Iteration 11)
4. **Diagnostics Migration** - 181 диагностика (Iterations 12-25)
5. **SDBL Grammar** - Full query parsing (deferred to Iterations 24-25 with diagnostics)

### 📊 Прогресс по фазам:
- Phase 1 (Foundation): **100% завершено** (Iterations 1-5 ✅)
- Phase 2 (Semantic Analysis): **50% завершено** (Iterations 6-11)
  - ✅ HIR/Symbols: Iterations 6-8 ✅
  - [ ] ModuleGraph & Incremental CI: Iteration 9.5
  - [ ] IDE-DB & Salsa: Iteration 10
  - [ ] Metadata Infrastructure: Iteration 11
- Phase 3 (Diagnostics): 0% (Iterations 12-25)
  - Tier 1 (Syntax): 12-14
  - Tier 2 (Semantic): 15-18
  - Tier 3 (Metadata): 19-23 ← требует Iteration 11
  - SDBL: 24-25
- Phase 4 (LSP Integration): 0% (Iterations 26-30)

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
| **salsa** | `/Users/kiriller/src/lsp/salsa/` | Инкрементальные вычисления (v0.25.2, критично для метаданных) |

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
**Срок: Итерации 6-11**

1. **HIR** - High-level IR
2. **HIR-Def** - определения и разрешение имён
3. **Symbol Table** - таблица символов
4. **IDE-DB** - база данных для IDE (с полной Salsa интеграцией)
5. **Metadata Infrastructure** - работа с метаданными 1С (Configuration, CommonModule, и т.д.)

### Phase 3: Diagnostics (Диагностики)
**Срок: Итерации 12-25**

1. **IDE-Diagnostics** - инфраструктура диагностик
2. Миграция 181 диагностики из bsl-language-server
   - Tier 1: Syntax-based (12-14)
   - Tier 2: Semantic-based (15-18)
   - Tier 3: Metadata-dependent (19-23) — требуют Metadata Infrastructure
3. **SDBL Diagnostics** (24-25) - диагностики запросов
4. Тесты с полным покрытием

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

### Iteration 3: Complete Parser ✅ COMPLETED (2025-12-29)
**Источники:** bsl-parser (BSLParser.g4, SDBLParser.g4)

**Статус:** Все основные задачи выполнены

- [x] Preprocessor directives
  - #Если/ИначеЕсли/Иначе/КонецЕсли
  - #Область/КонецОбласти
  - #Удаление/КонецУдаления
  - #Вставка/КонецВставки
  - Поддержка логических выражений (НЕ, И, ИЛИ)
  - Символы платформ (Клиент, Сервер, Linux, Windows и т.д.)
  - Вложенность директив
- [x] Regions (интеграция в source_file, preprocessor_if)
- [x] SDBL (Query language) infrastructure ✅ DONE
  - **Lexer:** 150+ SDBL tokens (keywords, functions, metadata types)
  - **Tokens:** Все SQL keywords, aggregate functions, date/string/math functions
  - **Metadata types:** 14+ types (Catalog, Document, Registers, etc.)
  - **Virtual tables:** Suffixes for Balance, Turnovers, SliceLast, etc.
  - **Parser entry point:** `parse_sdbl()` function implemented
  - **SyntaxKind:** 13 SDBL node types added
  - **Tests:** 23 SDBL lexer tests (all passing)
  - **Status:** Full grammar parsing deferred to future iterations (needed for SDBL diagnostics)
  - **Референсы:** `bsl-parser/SDBLParser.g4`, `bsl-parser/SDBLLexer.g4`
- [x] Полное покрытие тестами
  - 34 BSL unit tests (all passing)
  - 23 SDBL lexer tests (all passing)
  - 2 performance tests (225 MB/s in release)
  - Test data copied to fixtures/
  - Support for 1+ MB files

**Performance:**
- Debug: 41.65 MB/s (25ms for 1.04 MB file)
- Release: 225.80 MB/s (4.6ms for 1.04 MB file)
- ✅ Exceeds target (50 MB/s) by 4.5x

**SDBL Implementation Notes:**
- Full SDBL grammar (SELECT, FROM, WHERE, JOIN, GROUP BY, ORDER BY, etc.) will be implemented
  in future iterations when SDBL diagnostics are added (Iterations 24-25)
- Current implementation provides complete lexical analysis foundation
- Parse tree infrastructure in place via `parse_sdbl()` function

### Iteration 4: Syntax Trees (Rowan) ✅ COMPLETED (2025-12-29)
**Источники:** rust-analyzer (syntax/)

**Статус:** Все задачи выполнены

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
- [x] SyntaxNodePtr (pointer to syntax node)
- [x] Адаптация parser для генерации событий
  - Parser переведен на Output со событиями для SyntaxTreeBuilder
- [x] Полный набор AST wrappers
  - 23+ типов: statements, expressions, preprocessor nodes

**Тесты:** 34/34 BSL тестов проходят

### Iteration 5: Base Infrastructure ✅ COMPLETED (2025-12-29)
**Источники:** rust-analyzer (base-db/, vfs/)

**Статус:** Все основные задачи выполнены, Salsa интеграция отложена (см. SALSA_TODO.md)

- [x] VFS (Virtual File System)
  - Референс: `rust-analyzer/crates/vfs/`
  - PathInterner для FileId ↔ VfsPath mapping
  - FileSet для логической группировки файлов
  - Content-based change detection (FxHasher)
  - Smart change merging (Create+Modify=Create, Delete+Create=Modify)
- [x] SourceDatabase traits
  - Референс: `rust-analyzer/crates/base-db/src/lib.rs`
  - SourceDatabase и RootQueryDb traits
  - Files helper с DashMap для кеширования
  - FileChange для батчевых обновлений
- [x] FileId, SourceRootId
  - SourceRoot с is_library флагом
  - Bidirectional FileId ↔ VfsPath lookups
- [x] Parse query с кешированием
  - DashMap-based cache (Arc<Parse>)
  - Автоматическая инвалидация при изменении файлов
- [ ] ⚠️ **Полная Salsa 0.25.2 интеграция** — отложена
  - Причина: сложность ingredient registration API
  - Текущее решение: DashMap-based caching (эквивалентная функциональность)
  - План: см. `docs/planning/SALSA_TODO.md`

**Тесты:** 82+ тестов проходят (VFS + base-db + все предыдущие)

**Производительность:**
- VFS change detection: O(1) hash-based
- Parse caching: O(1) lookup via DashMap
- Memory: Arc-based sharing для Parse results

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

### Iteration 9.5: ModuleGraph & Incremental CI Mode
**Источники:** rust-analyzer (base-db/input.rs - CrateGraph)

**См. детальный план в `INCREMENTAL_CI.md`**

**Цель:** Граф зависимостей модулей для инкрементального анализа в CI/CD.

**Задачи:**

- [ ] Core: ModuleGraph (5-7 дней)
  - ModuleGraph, ModuleData, Dependency structures
  - ModuleGraphBuilder с валидацией циклов
  - Индексы: path → ModuleId, name → [ModuleId]
  - Референс: `rust-analyzer/crates/base-db/src/input.rs` (CrateGraph)
- [ ] Dependency Extraction (3-5 дней)
  - Извлечение зависимостей из AST (вызовы функций)
  - Парсинг `#Использовать` директив
  - Метаданные: CommonModule dependencies
- [ ] Incremental Analysis Engine (3-5 дней)
  - `affected_modules(changed_files)` - поиск затронутых модулей
  - `transitive_dependencies()`, `transitive_reverse_dependencies()`
  - Фильтрация модулей для анализа
- [ ] CLI Integration (2-3 дня)
  - `--incremental` flag
  - `--changed-files` и `--git-diff` опции
  - GitLab CI примеры в `.gitlab-ci.yml`
- [ ] Graph Caching (2-3 дня)
  - Сохранение/загрузка графа (MessagePack/JSON)
  - Инвалидация кеша при изменении файлов
- [ ] Diagnostics на основе графа (3-5 дней)
  - UnusedModule (DG001)
  - CircularDependency (DG002)
  - ModuleCoupling (DG003)
- [ ] LSP Navigation (опционально, 3-5 дней)
  - Call Hierarchy (incoming/outgoing calls)
  - Find Usages через граф

**Критерии готовности:**
- ✅ ModuleGraph корректно строится для реальных проектов
- ✅ Incremental mode для pt_erp: < 1 сек (vs 10-15 сек full)
- ✅ Циклические зависимости обнаруживаются
- ✅ GitLab CI интеграция работает
- ✅ Граф кешируется и быстро загружается (< 0.1 сек)

**Метрики производительности (pt_erp):**
- Full scan: 10-15 секунд (baseline)
- Incremental (1 модуль изменен): 0.5-1 секунда (**10x-30x**)
- Incremental (5 модулей): 1-2 секунды (**5x-15x**)
- Incremental (100 модулей): 3-5 секунд (**2x-5x**)

### Iteration 10: IDE-DB & Salsa Integration
**Источники:** rust-analyzer (ide-db/), salsa (src/, book/, tests/)

- [ ] Полная интеграция Salsa 0.25.2
  - Референс: `/Users/kiriller/src/lsp/salsa/`
  - Изучить актуальную документацию через book/
  - Изучить примеры в tests/ и examples/
  - См. детальный план в `SALSA_TODO.md`
- [ ] RootDatabase с Salsa
  - Референс: `rust-analyzer/crates/ide-db/src/`
  - Input queries: file_text, source_root
  - Derived queries: parse, module tree
- [ ] Salsa queries для базовых операций
  - parse() с LRU кешированием
  - module_tree() для зависимостей
- [ ] Настройка Durability
  - HIGH для библиотек
  - LOW для исходного кода
- [ ] Тесты инкрементальности
  - Изменение файла не должно пересчитывать зависимые модули (если интерфейс не изменился)
  - Benchmarks: incremental update < 100ms
- [ ] Профилирование
  - BSL_PROFILE=* для анализа overhead Salsa

**Критерии готовности:**
- ✅ Все существующие 82+ тестов проходят
- ✅ Salsa корректно кеширует результаты
- ✅ Incremental updates работают
- ✅ Профилирование показывает минимальный overhead

### Iteration 11: Metadata Infrastructure
**Источники:** bsl-language-server-rust (bsl-metadata/), bsl-language-server (mdclasses), salsa

**См. детальный план в `METADATA_PLAN.md`**

- [ ] Создать крейт bsl-metadata (2-3 дня)
  - Скопировать структуры из bsl-language-server-rust
  - Configuration, CommonModule, MetadataObject
  - Enums: ModuleType, MdoType, ReturnValueReuse
  - Traits: MdObject, Module
  - Unit tests
- [ ] Реализовать XML loader (3-4 дня)
  - Выбрать XML библиотеку (quick-xml или roxmltree)
  - parse_configuration(), parse_common_module()
  - Загрузка Catalog, Document, Register и др.
  - Обработка ошибок парсинга
  - Тесты с реальными XML из bsl-language-server
- [ ] Интеграция с Salsa (3-4 дня)
  - Salsa queries в ide-db/src/metadata.rs
  - configuration_path() — input query
  - load_configuration() — derived, Durability::HIGH
  - find_common_module(), metadata_object_exists()
  - MetadataDb trait
  - Тесты инкрементальности
- [ ] AbstractMetadataDiagnostic паттерн (2-3 дня)
  - Портировать из Java
  - MetadataDiagnostic trait
  - MetadataDiagnosticRunner
  - 2-3 примера диагностик (CommonModuleAssign, ForbiddenMetadataName)
- [ ] Тестирование (2-3 дня)
  - Unit tests для loader
  - Integration tests для Salsa queries
  - Performance: загрузка < 1 сек, кеш < 1 мс
- [ ] Документация (1-2 дня)
  - Обновить ARCHITECTURE.md
  - Doc comments для API
  - Примеры использования

**Референсы:**
- `bsl-language-server-rust/crates/bsl-metadata/`
- `bsl-language-server/.../.../diagnostics/AbstractMetadataDiagnostic.java`
- `/Users/kiriller/src/lsp/salsa/` — для Salsa queries
- `rust-analyzer/crates/base-db/` — примеры Salsa

**Критерии готовности:**
- ✅ XML loader работает с реальными конфигурациями
- ✅ Salsa queries корректно кешируют метаданные
- ✅ 2-3 metadata-диагностики работают как proof-of-concept
- ✅ Performance: загрузка < 1 сек, кешированный доступ < 1 мс
- ✅ Тесты покрывают основные сценарии (> 80%)

### Iterations 12-25: Diagnostics Migration
**Источники:** bsl-language-server (diagnostics/), bsl-language-server-rust (rules/)

**Зависимости:**
- Iteration 11: Metadata Infrastructure — обязательна для Tier 3 диагностик (19-23)

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
