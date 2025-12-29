# Detailed Iterations

> **Примечание:** Для каждой итерации указаны проекты-источники. Подробное описание проектов см. в [SOURCES.md](./SOURCES.md).

## Phase 1: Foundation

### Iteration 1: Project Setup & Lexer Foundation

**Источники:**
- `bsl-parser/src/main/antlr/BSLLexer.g4` — полный список токенов (40+ ключевых слов)
- `bsl-language-server-rust/crates/bsl-parser/src/bsl_tokenizer.rs` — Rust реализация токенизатора
- `tree-sitter-bsl/grammar.js` — альтернативный список токенов и ключевых слов
- `rust-analyzer/crates/parser/src/lexed_str.rs` — архитектура лексера на logos

**Цель:** Создать базовую инфраструктуру и реализовать лексер BSL.

**Задачи:**

1. **Настройка проекта**
   - [ ] Инициализация git репозитория
   - [ ] Настройка CI/CD (GitHub Actions)
   - [ ] Настройка clippy и rustfmt
   - [ ] Добавление LICENSE файлов

2. **Крейт `stdx`**
   - [ ] Создать базовые утилиты
   - [ ] Макросы для тестирования
   - [ ] Расширения стандартных типов

3. **Крейт `lexer`**
   - [ ] Определить все токены BSL
   - [ ] Реализовать лексер на базе logos
   - [ ] Обработка строк (в том числе многострочных)
   - [ ] Обработка чисел и дат
   - [ ] Обработка комментариев
   - [ ] Обработка директив препроцессора
   - [ ] Обработка регионов
   - [ ] Поддержка русских и английских ключевых слов

4. **Тесты лексера**
   - [ ] Тесты для каждого типа токена
   - [ ] Тесты граничных случаев
   - [ ] Snapshot тесты

**Критерии готовности:**
- Лексер токенизирует любой валидный BSL код
- Покрытие тестами > 90%
- CI проходит

---

### Iteration 2: Parser Foundation

**Источники:**
- `bsl-parser/src/main/antlr/BSLParser.g4` — полная грамматика BSL (file, subs, procedure, function)
- `rust-analyzer/crates/parser/src/grammar/` — event-based подход к парсингу
- `rust-analyzer/crates/parser/src/parser.rs` — Marker pattern для error recovery
- `tree-sitter-bsl/grammar.js` — приоритеты операторов (PREC.*), структура выражений
- `tree-sitter-bsl/test/corpus/` — тестовые fixtures для верификации парсера

**Цель:** Реализовать парсер для основных конструкций BSL.

**Задачи:**

1. **Крейт `parser`**
   - [ ] Инфраструктура парсера (events, markers)
   - [ ] Грамматика верхнего уровня (module, function, procedure)
   - [ ] Грамматика выражений (expressions)
   - [ ] Грамматика операторов (statements)

2. **Грамматика items**
   - [ ] Функции и процедуры
   - [ ] Параметры
   - [ ] Аннотации (&НаКлиенте, &НаСервере)
   - [ ] Директивы компиляции
   - [ ] Описания переменных

3. **Грамматика statements**
   - [ ] Присваивание
   - [ ] Вызов процедуры
   - [ ] Условный оператор (Если)
   - [ ] Циклы (Для, Пока, Для Каждого)
   - [ ] Попытка/Исключение
   - [ ] Возврат
   - [ ] Прервать/Продолжить

4. **Грамматика expressions**
   - [ ] Литералы
   - [ ] Идентификаторы
   - [ ] Вызовы функций
   - [ ] Бинарные операции
   - [ ] Унарные операции
   - [ ] Тернарный оператор
   - [ ] Доступ к свойствам
   - [ ] Индексация

**Критерии готовности:**
- Парсер обрабатывает основные конструкции BSL
- Error recovery для некорректного кода
- Тесты грамматики

---

### Iteration 3: Complete Parser

**Источники:**
- `bsl-parser/src/main/antlr/BSLParser.g4` — preprocessor, preproc_if, regionStart/End
- `bsl-parser/src/main/antlr/BSLLexer.g4` — PREPROCESSOR_MODE, ANNOTATION_MODE
- `bsl-parser/src/main/antlr/SDBLParser.g4` — полная грамматика SDBL (SELECT, JOIN, виртуальные таблицы)
- `bsl-parser/src/main/antlr/SDBLLexer.g4` — токены SDBL
- `bsl-language-server-rust/crates/bsl-parser/src/sdbl_tokenizer.rs` — Rust реализация SDBL токенизатора
- `bsl-parser/src/test/resources/` — тестовые данные для парсера

**Цель:** Дополнить парсер оставшимися конструкциями.

**Задачи:**

1. **Препроцессор**
   - [ ] #Если / #ИначеЕсли / #Иначе / #КонецЕсли
   - [ ] #Область / #КонецОбласти
   - [ ] Символы препроцессора (Клиент, Сервер, и т.д.)

2. **SDBL парсер**
   - [ ] SELECT
   - [ ] FROM
   - [ ] WHERE
   - [ ] JOIN (все виды)
   - [ ] GROUP BY / HAVING
   - [ ] ORDER BY
   - [ ] UNION

3. **Специфика BSL**
   - [ ] Новый оператор
   - [ ] Обращение к метаданным
   - [ ] Встроенные функции

4. **Дополнительные тесты**
   - [ ] Парсинг реальных файлов из bsl-language-server тестов
   - [ ] Benchmark парсинга

**Критерии готовности:**
- Парсер полностью совместим с BSL
- SDBL парсится корректно
- Benchmark: > 50 MB/s

---

### Iteration 4: Syntax Trees (Rowan)

**Источники:**
- `rust-analyzer/crates/syntax/src/lib.rs` — интеграция с Rowan
- `rust-analyzer/crates/syntax/src/ast/` — типизированные AST обёртки
- `rust-analyzer/crates/syntax/src/parsing/` — TreeBuilder, GreenNode

**Цель:** Создать типизированные синтаксические деревья.

**Задачи:**

1. **Крейт `syntax`**
   - [ ] SyntaxKind enum
   - [ ] BslLanguage для Rowan
   - [ ] GreenNode builder интеграция

2. **AST layer**
   - [ ] Генерация или ручное создание AST типов
   - [ ] Trait AstNode
   - [ ] Типизированные обёртки для всех узлов

3. **Утилиты**
   - [ ] SyntaxNodePtr
   - [ ] Алгоритмы обхода (preorder, ancestors, siblings)
   - [ ] Поиск узлов по типу

4. **Интеграция parser + syntax**
   - [ ] Parse<T> результат
   - [ ] Получение CST из парсера

**Критерии готовности:**
- Полное CST для любого BSL файла
- Типизированный AST
- Возможность восстановить исходный текст из CST

---

### Iteration 5: Base Infrastructure

**Источники:**
- `rust-analyzer/crates/vfs/` — Virtual File System (VfsPath, FileId, AbsPathBuf)
- `rust-analyzer/crates/base-db/src/lib.rs` — Salsa database, SourceDatabase trait
- `rust-analyzer/crates/base-db/src/input.rs` — FileSet, SourceRoot

**Цель:** Создать базовую инфраструктуру для инкрементального анализа.

**Задачи:**

1. **Крейт `intern`**
   - [ ] Интернирование строк
   - [ ] Интернирование идентификаторов

2. **Крейт `vfs`**
   - [ ] FileId
   - [ ] VfsPath
   - [ ] Хранение содержимого файлов
   - [ ] Уведомления об изменениях

3. **Крейт `base-db`**
   - [ ] Интеграция с Salsa
   - [ ] SourceDatabase trait
   - [ ] Кеширование парсинга
   - [ ] FileSet / SourceRoot

4. **Тесты инфраструктуры**
   - [ ] Тесты инвалидации кеша
   - [ ] Тесты производительности

**Критерии готовности:**
- Инкрементальное обновление при изменении файлов
- Кеширование результатов парсинга
- Тесты Salsa интеграции

---

## Phase 2: Semantic Analysis

### Iteration 6-7: HIR Foundation

**Источники:**
- `rust-analyzer/crates/hir/src/lib.rs` — HIR публичный API
- `rust-analyzer/crates/hir-def/` — определения (FunctionData, ItemTree)
- `bsl-language-server-rust/crates/bsl-symbols/src/lib.rs` — Symbol, SymbolKind, SymbolTable

**Цель:** Создать High-level IR для семантического анализа.

**Задачи:**

1. **Крейт `hir-def`**
   - [ ] ModuleData
   - [ ] FunctionData / ProcedureData
   - [ ] VariableData
   - [ ] ItemTree (список определений в файле)

2. **Разрешение имён**
   - [ ] Scopes
   - [ ] Resolver
   - [ ] ExportImport analysis

3. **Крейт `hir`**
   - [ ] Module, Function, Procedure типы
   - [ ] Semantics API
   - [ ] SourceAnalyzer

**Критерии готовности:**
- Разрешение имён в пределах модуля
- Semantics API для IDE

---

### Iteration 8-9: Symbol Resolution

**Источники:**
- `rust-analyzer/crates/hir-def/src/resolver.rs` — разрешение имён
- `rust-analyzer/crates/hir-def/src/body/scope.rs` — scopes
- `bsl-language-server-rust/crates/bsl-symbols/` — SymbolTable, scope analysis

**Цель:** Полное разрешение символов и область видимости.

**Задачи:**

1. **SymbolTree**
   - [ ] Построение таблицы символов
   - [ ] ModuleSymbol, MethodSymbol, VariableSymbol

2. **Cross-module resolution**
   - [ ] Разрешение вызовов общих модулей
   - [ ] Экспортируемые методы

3. **Type information** (базовое)
   - [ ] Примитивные типы
   - [ ] Inferred types

**Критерии готовности:**
- Go to Definition работает
- Find References работает

---

### Iteration 10: IDE-DB

**Источники:**
- `rust-analyzer/crates/ide-db/src/` — RootDatabase, symbol index
- `rust-analyzer/crates/ide-db/src/search.rs` — поиск по символам

**Цель:** Создать базу данных для IDE функциональности.

**Задачи:**

1. **Крейт `ide-db`**
   - [ ] RootDatabase
   - [ ] Интеграция всех слоёв
   - [ ] Symbol Index

2. **Производительность**
   - [ ] Benchmark
   - [ ] Оптимизация запросов
   - [ ] LRU кеши

**Критерии готовности:**
- RootDatabase готов для использования
- Benchmark на реальных проектах

---

## Phase 3: Diagnostics

### Iterations 11-25: Diagnostics Implementation

**Источники:**
- `bsl-language-server/src/main/java/.../diagnostics/` — 181 Java реализация (референс)
- `bsl-language-server-rust/crates/bsl-diagnostics/src/rules/` — 183 Rust реализации
- `bsl-language-server-rust/crates/bsl-diagnostics/src/lib.rs` — trait DiagnosticRule
- `bsl-language-server-rust/crates/bsl-diagnostics/src/registry.rs` — параллельное выполнение (rayon)
- `bsl-language-server/src/test/resources/diagnostics/` — тестовые данные
- `rust-analyzer/crates/ide-diagnostics/` — архитектура диагностик
- `bsl-language-server-rust/crates/bsl-cfg/` — Control Flow Graph для сложных анализов
- `bsl-language-server-rust/crates/bsl-metadata/` — метаданные 1C для диагностик Tier 3

**Итерации:**
| Итерация | Tier | Источники |
|----------|------|-----------|
| 11-13 | Tier 1 (Critical) | bsl-language-server (diagnostics/), bsl-language-server-rust (rules/) |
| 14-18 | Tier 2 (Important) | bsl-language-server, bsl-language-server-rust |
| 19-23 | Tier 3 (Metadata) | bsl-language-server, bsl-language-server-rust (bsl-metadata/) |
| 24-25 | Tier 4 (SDBL) | bsl-language-server, bsl-parser (SDBL*.g4), bsl-language-server-rust |

См. [DIAGNOSTICS_MIGRATION.md](./DIAGNOSTICS_MIGRATION.md) для детального плана.

**Структура каждой итерации:**
1. Реализовать N диагностик
2. Написать тесты
3. Проверить совместимость с Java версией
4. Обновить документацию

---

## Phase 4: LSP Integration

### Iterations 26-30: LSP Server

**Источники:**
- `rust-analyzer/crates/rust-analyzer/src/handlers/` — LSP handlers
- `rust-analyzer/crates/rust-analyzer/src/main_loop.rs` — main loop архитектура
- `bsl-language-server-rust/crates/bsl-lsp-server/src/backend.rs` — Rust backend pattern
- `bsl-language-server/src/main/java/.../BSLLanguageServer.java` — capabilities
- `bsl-language-server/src/main/java/.../configuration/` — конфигурация (.bslls.json)

**Итерации:**
| Итерация | Фокус | Источники |
|----------|-------|-----------|
| 26 | LSP Core | rust-analyzer (main_loop), bsl-language-server-rust (backend.rs) |
| 27 | LSP Navigation | rust-analyzer (handlers/), bsl-language-server |
| 28 | LSP Code Actions | rust-analyzer, bsl-language-server |
| 29 | LSP Formatting | bsl-language-server, rust-analyzer |
| 30 | LSP Advanced | bsl-language-server (полная совместимость) |

См. [LSP_IMPLEMENTATION.md](./LSP_IMPLEMENTATION.md) для детального плана.

---

## Трекинг итераций

| # | Название | Статус | Начало | Завершение |
|---|----------|--------|--------|------------|
| 1 | Project Setup & Lexer | Not Started | - | - |
| 2 | Parser Foundation | Not Started | - | - |
| 3 | Complete Parser | Not Started | - | - |
| 4 | Syntax Trees | Not Started | - | - |
| 5 | Base Infrastructure | Not Started | - | - |
| 6-7 | HIR Foundation | Not Started | - | - |
| 8-9 | Symbol Resolution | Not Started | - | - |
| 10 | IDE-DB | Not Started | - | - |
| 11-13 | Tier 1 Diagnostics | Not Started | - | - |
| 14-18 | Tier 2 Diagnostics | Not Started | - | - |
| 19-23 | Tier 3 Diagnostics | Not Started | - | - |
| 24-25 | Tier 4 Diagnostics | Not Started | - | - |
| 26 | LSP Core | Not Started | - | - |
| 27 | LSP Navigation | Not Started | - | - |
| 28 | LSP Code Actions | Not Started | - | - |
| 29 | LSP Formatting | Not Started | - | - |
| 30 | LSP Advanced | Not Started | - | - |
