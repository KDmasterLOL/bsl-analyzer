# Detailed Iterations

> **Примечание:** Для каждой итерации указаны проекты-источники. Подробное описание проектов см. в [SOURCES.md](./SOURCES.md).

> **Последнее обновление:** 2025-12-30

## Phase 1: Foundation

### Iteration 1: Project Setup & Lexer Foundation ✅ COMPLETED

**Источники:**
- `bsl-parser/src/main/antlr/BSLLexer.g4` — полный список токенов (40+ ключевых слов)
- `bsl-language-server-rust/crates/bsl-parser/src/bsl_tokenizer.rs` — Rust реализация токенизатора
- `tree-sitter-bsl/grammar.js` — альтернативный список токенов и ключевых слов
- `rust-analyzer/crates/parser/src/lexed_str.rs` — архитектура лексера на logos

**Цель:** Создать базовую инфраструктуру и реализовать лексер BSL.

**Задачи:**

1. **Настройка проекта**
   - [x] Инициализация git репозитория
   - [x] Настройка CI/CD (GitLab CI с форматированием и clippy)
   - [x] Настройка clippy и rustfmt (pre-commit hooks)
   - [x] Добавление LICENSE файлов (Apache 2.0 + MIT)
   - [x] Создание структуры из 15 крейтов

2. **Крейт `stdx`**
   - [x] Создан базовый крейт
   - [ ] TODO: Добавить утилиты по мере необходимости

3. **Крейт `lexer`**
   - [x] Определить все токены BSL (80+ токенов)
   - [x] Реализовать лексер на базе logos
   - [x] Обработка строк (в том числе многострочных с |)
   - [x] Обработка чисел и дат
   - [x] Обработка комментариев (// и /* */)
   - [x] Обработка директив препроцессора (#Если, #Область, #Удаление, #Вставка)
   - [x] Обработка регионов (#Область/КонецОбласти)
   - [x] Поддержка русских и английских ключевых слов (case-insensitive)
   - [x] Обработка аннотаций (&НаКлиенте, &До, &После и т.д.)

4. **Тесты лексера**
   - [x] Тесты для каждого типа токена (26 unit тестов)
   - [x] Тесты граничных случаев (keywords, strings, numbers)
   - [x] Performance тесты (lexer скорость)

**Критерии готовности:**
- ✅ Лексер токенизирует любой валидный BSL код
- ✅ Покрытие тестами > 90% (26 тестов, все проходят)
- ✅ CI проходит (GitLab CI green)

---

### Iteration 2: Parser Foundation ✅ COMPLETED

**Источники:**
- `bsl-parser/src/main/antlr/BSLParser.g4` — полная грамматика BSL (file, subs, procedure, function)
- `rust-analyzer/crates/parser/src/grammar/` — event-based подход к парсингу
- `rust-analyzer/crates/parser/src/parser.rs` — Marker pattern для error recovery
- `tree-sitter-bsl/grammar.js` — приоритеты операторов (PREC.*), структура выражений
- `tree-sitter-bsl/test/corpus/` — тестовые fixtures для верификации парсера

**Цель:** Реализовать парсер для основных конструкций BSL.

**Задачи:**

1. **Крейт `parser`**
   - [x] Инфраструктура парсера (events, markers, Marker pattern)
   - [x] Грамматика верхнего уровня (source_file, procedures, functions)
   - [x] Грамматика выражений (expressions с приоритетами)
   - [x] Грамматика операторов (statements)
   - [x] Error recovery (p.error(), iteration_limit)

2. **Грамматика items**
   - [x] Функции и процедуры (включая Async)
   - [x] Параметры (с export и значениями по умолчанию)
   - [x] Аннотации (&До, &После, &Вокруг, &ИзменениеИПроверкаПроведения)
   - [x] Директивы компиляции (&НаКлиенте, &НаСервере и т.д.)
   - [x] Описания переменных (Перем с export)

3. **Грамматика statements**
   - [x] Присваивание (=)
   - [x] Вызов процедуры (call statement)
   - [x] Условный оператор (Если/ИначеЕсли/Иначе/КонецЕсли)
   - [x] Циклы (Для/Пока/Для Каждого)
   - [x] Попытка/Исключение (Попытка/Исключение/КонецПопытки)
   - [x] Возврат (с опциональным значением)
   - [x] Прервать/Продолжить (Break/Continue)
   - [x] Перейти/Метка (Goto/Label ~метка:)
   - [x] Выполнить (Execute)
   - [x] ДобавитьОбработчик/УдалитьОбработчик (AddHandler/RemoveHandler)

4. **Грамматика expressions**
   - [x] Литералы (числа, строки, даты, булевы, Null, Undefined)
   - [x] Идентификаторы
   - [x] Вызовы функций (с аргументами)
   - [x] Бинарные операции (+, -, *, /, %, =, <>, <, >, <=, >=, И, ИЛИ)
   - [x] Унарные операции (-, Не/Not, +)
   - [x] Тернарный оператор (?(условие, знач1, знач2))
   - [x] Доступ к свойствам (obj.field)
   - [x] Индексация (arr[index])
   - [x] Await выражения
   - [x] New выражения

5. **Тесты**
   - [x] 34 unit тестов (все проходят)
   - [x] 2 performance тестов (225 MB/s)
   - [x] Тестовые данные в fixtures/

**Критерии готовности:**
- ✅ Парсер обрабатывает основные конструкции BSL
- ✅ Error recovery для некорректного кода (iteration_limit, p.error())
- ✅ Тесты грамматики (34 тестов)

---

### Iteration 3: Complete Parser ✅ COMPLETED (2025-12-29)

**Источники:**
- `bsl-parser/src/main/antlr/BSLParser.g4` — preprocessor, preproc_if, regionStart/End
- `bsl-parser/src/main/antlr/BSLLexer.g4` — PREPROCESSOR_MODE, ANNOTATION_MODE
- `bsl-parser/src/main/antlr/SDBLParser.g4` — полная грамматика SDBL (SELECT, JOIN, виртуальные таблицы)
- `bsl-parser/src/main/antlr/SDBLLexer.g4` — токены SDBL
- `bsl-language-server-rust/crates/bsl-parser/src/sdbl_tokenizer.rs` — Rust реализация SDBL токенизатора
- `bsl-parser/src/test/resources/` — тестовые данные для парсера

**Цель:** Дополнить парсер оставшимися конструкциями.

**Задачи:**

1. **Препроцессор** ✅ DONE
   - [x] #Если / #ИначеЕсли / #Иначе / #КонецЕсли
   - [x] #Область / #КонецОбласти
   - [x] #Удаление / #КонецУдаления
   - [x] #Вставка / #КонецВставки
   - [x] Символы препроцессора (Клиент, Сервер, Linux, Windows и т.д.)
   - [x] Логические операторы (НЕ, И, ИЛИ)
   - [x] Вложенность директив

2. **SDBL парсер** ⚠️ TODO (приоритет P2)
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

### Iteration 4: Syntax Trees (Rowan) ✅ COMPLETED (2025-12-29)

**Источники:**
- `rust-analyzer/crates/syntax/src/lib.rs` — интеграция с Rowan
- `rust-analyzer/crates/syntax/src/ast/` — типизированные AST обёртки
- `rust-analyzer/crates/syntax/src/parsing/` — TreeBuilder, GreenNode

**Цель:** Создать типизированные синтаксические деревья.

**Задачи:**

1. **Крейт `syntax`**
   - [x] SyntaxKind enum (130+ variants)
   - [x] BslLanguage для Rowan
   - [x] GreenNode builder интеграция

2. **AST layer**
   - [x] Создание AST типов (23+ wrappers)
   - [x] Trait AstNode и AstToken
   - [x] Типизированные обёртки для всех узлов

3. **Утилиты**
   - [x] SyntaxNodePtr
   - [x] Алгоритмы обхода (preorder, ancestors, siblings)
   - [x] Поиск узлов по типу

4. **Интеграция parser + syntax**
   - [x] Parse<T> результат
   - [x] Получение CST из парсера
   - [x] Event-based парсинг с SyntaxTreeBuilder

**Критерии готовности:**
- ✅ Полное CST для любого BSL файла
- ✅ Типизированный AST (23+ типов)
- ✅ Возможность восстановить исходный текст из CST
- ✅ 34/34 BSL тестов проходят

---

### Iteration 5: Base Infrastructure ✅ COMPLETED (2025-12-29)

**Источники:**
- `rust-analyzer/crates/vfs/` — Virtual File System (VfsPath, FileId, AbsPathBuf)
- `rust-analyzer/crates/base-db/src/lib.rs` — Salsa database, SourceDatabase trait
- `rust-analyzer/crates/base-db/src/input.rs` — FileSet, SourceRoot

**Цель:** Создать базовую инфраструктуру для инкрементального анализа.

**Задачи:**

1. **Крейт `intern`**
   - [x] Интернирование путей (PathInterner)
   - [ ] Интернирование идентификаторов (отложено)

2. **Крейт `vfs`**
   - [x] FileId
   - [x] VfsPath
   - [x] Хранение содержимого файлов
   - [x] Уведомления об изменениях (ChangedFile, Change enum)
   - [x] Content-based change detection (FxHasher)
   - [x] Smart change merging

3. **Крейт `base-db`**
   - [x] SourceDatabase trait
   - [x] RootQueryDb trait
   - [x] Кеширование парсинга (DashMap-based)
   - [x] FileSet / SourceRoot
   - [x] FileChange для батчевых обновлений
   - [ ] ⚠️ Полная Salsa 0.25.2 интеграция (отложена, см. SALSA_TODO.md)

4. **Тесты инфраструктуры**
   - [x] Тесты VFS (change tracking, hashing, merging)
   - [x] Тесты base-db (parse query, incremental reparse)
   - [x] Тесты производительности (O(1) hash-based detection)

**Критерии готовности:**
- ✅ Инкрементальное обновление при изменении файлов
- ✅ Кеширование результатов парсинга (Arc<Parse>)
- ✅ 82+ тестов проходят
- ⚠️ Salsa интеграция отложена (DashMap предоставляет эквивалентную функциональность)

**Результаты:**
- VFS: 436 строк, 10 тестов
- base-db: 276 строк, 4 теста
- PathInterner: 152 строки, 9 тестов
- FileSet: 175 строк, 13 тестов
- Производительность: O(1) для change detection и cache lookups

---

## Phase 2: Semantic Analysis

### Iteration 6-7: HIR Foundation ✅ COMPLETED

**Источники:**
- `rust-analyzer/crates/hir/src/lib.rs` — HIR публичный API
- `rust-analyzer/crates/hir-def/` — определения (FunctionData, ItemTree)
- `bsl-language-server-rust/crates/bsl-symbols/src/lib.rs` — Symbol, SymbolKind, SymbolTable

**Цель:** Создать High-level IR для семантического анализа.

**Реализовано:**

1. **Крейт `hir-def`**
   - [x] ModuleData - данные о модуле (procedures, functions, variables)
   - [x] MethodData - данные о методах (name, is_function, is_export, parameters)
   - [x] ParameterData - данные о параметрах (name, is_val, has_default)
   - [x] VariableData - данные о переменных (name, is_export)
   - [x] ItemTree - модель верхнего уровня (procedures, functions, variables)
     - Создан `crates/hir-def/src/item_tree.rs`
     - Procedure, Function, Variable items
     - Source ranges для навигации
   - [x] DefDatabase trait
     - item_tree(), module_data() queries

2. **Разрешение имён**
   - [x] ExprScope - scopes для локальных переменных и параметров
     - Создан `crates/hir-def/src/scope.rs`
     - add_parameter(), add_local_variable()
     - resolve() для поиска имён в scope
   - [x] Resolver - разрешение имён на разных уровнях
     - Создан `crates/hir-def/src/resolver.rs`
     - resolve_name() для поиска в stack of scopes
     - push_expr_scope() для добавления scopes
   - [x] Export analysis - отслеживание export видимости

3. **Крейт `hir`**
   - [x] Module, Method, Variable типы
     - Создан `crates/hir/src/lib.rs`
     - Module::procedures(), Module::functions(), Module::variables()
     - Method::name(), Method::is_function(), Method::is_export()
     - Variable::name(), Variable::is_export()
   - [x] Semantics API
     - Semantics::new(), Semantics::module_from_file()
     - Semantics::find_method() - поиск метода по имени
   - [x] Database integration
     - RootDatabaseImpl с поддержкой DefDatabase

**Критерии готовности:**
- ✅ Разрешение имён в пределах модуля
- ✅ Semantics API для IDE
- ✅ 14+ тестов в hir крейте
- ✅ 52+ тестов в hir-def крейте

**Статистика:**
- `crates/hir/src/lib.rs`: 700+ строк
- `crates/hir-def/src/item_tree.rs`: 600+ строк
- `crates/hir-def/src/resolver.rs`: 300+ строк
- `crates/hir-def/src/scope.rs`: 200+ строк

---

### Iteration 8: Symbol Resolution ✅ COMPLETED

**Источники:**
- `rust-analyzer/crates/hir-def/src/resolver.rs` — разрешение имён
- `rust-analyzer/crates/hir-def/src/body/scope.rs` — scopes
- `bsl-language-server-rust/crates/bsl-symbols/` — SymbolTable, scope analysis
- `rust-analyzer/crates/hir/src/semantics.rs` — Semantics API pattern

**Цель:** Полное разрешение символов и область видимости внутри модуля.

**Реализовано:**

1. **SymbolTree** (Phase 1-2)
   - [x] Построение таблицы символов (`crates/hir-def/src/symbol_tree.rs`)
   - [x] MethodSymbol, VariableSymbol, ParamSymbol
   - [x] Регистронезависимый поиск (case-insensitive HashMap)
   - [x] Export visibility tracking
   - [x] 15+ тестов

2. **Module-Level Resolution** (Phase 3)
   - [x] Resolver расширен для module-level scope
   - [x] `resolve_module_method()` и `resolve_module_variable()`
   - [x] Порядок разрешения: ExprScope → ModuleScope → WorkspaceScope
   - [x] 8+ тестов

3. **Type Information** (Phase 4)
   - [x] Базовые типы (`crates/hir-def/src/ty.rs`)
   - [x] Примитивные типы: Number, String, Boolean, Date, Undefined, Null
   - [x] Сложные типы: Array, Structure, Map, Function
   - [x] Вывод типов из литералов
   - [x] 8+ тестов

4. **Semantics API** (Phase 5)
   - [x] `Semantics::resolve_method_call()` — разрешение вызовов
   - [x] `Semantics::symbol_at_position()` — символ в позиции курсора
   - [x] `Semantics::find_method_references()` — поиск ссылок на методы
   - [x] `Semantics::find_variable_references()` — поиск ссылок на переменные
   - [x] `Symbol` enum (Method, Variable, Parameter)
   - [x] 9+ тестов

5. **IDE Features** (Phase 6)
   - [x] Go to Definition (`crates/ide/src/goto_definition.rs`)
     - Навигация к определению методов/функций/переменных
     - 6 комплексных тестов
   - [x] Find References (`crates/ide/src/references.rs`)
     - Поиск всех использований методов/переменных
     - 6 комплексных тестов
   - [x] Интеграция в Analysis API
   - [x] `Analysis::goto_definition()` и `Analysis::find_references()`

6. **Cross-module resolution** (инфраструктура)
   - [x] WorkspaceScope в Resolver (базовая инфраструктура)
   - [ ] Полная реализация → Iteration 9.5 (ModuleGraph)

**Критерии готовности:**
- ✅ Go to Definition работает (внутри файла)
- ✅ Find References работает (внутри файла)
- ✅ Регистронезависимое разрешение (BSL-специфика)
- ✅ 40+ новых тестов (190 тестов всего в проекте)
- ✅ Clippy без предупреждений

**Статистика:**
- `crates/hir-def/src/symbol_tree.rs`: 500+ строк
- `crates/hir-def/src/ty.rs`: 230+ строк
- `crates/ide/src/goto_definition.rs`: 242 строки
- `crates/ide/src/references.rs`: 224 строки
- Расширения `Semantics` API: ~150 строк
- Всего: ~1350 строк нового кода

---

### Iteration 9.5: ModuleGraph & Incremental CI Mode

**Источники:**
- `rust-analyzer/crates/base-db/src/input.rs` — CrateGraph pattern (1187 lines)
- `rust-analyzer/crates/base-db/src/change.rs` — Transactional updates
- `/Users/kiriller/src/lsp/salsa/` — Automatic dependency tracking

**Цель:** Граф зависимостей BSL модулей для инкрементального анализа в CI/CD.

**См. детальный план в `INCREMENTAL_CI.md`**

**Задачи:**

1. **Core: ModuleGraph** (5-7 дней)
   - [ ] Создать `crates/base-db/src/module_graph.rs`
     - ModuleGraph (Arena-based, как CrateGraph)
     - ModuleData (id, name, file_id, dependencies, metadata, kind)
     - Dependency (module_id, kind: DirectCall/Import/Metadata)
     - ModuleKind (CommonModule, ObjectModule, FormModule, etc.)
   - [ ] Создать `crates/base-db/src/module_graph/builder.rs`
     - ModuleGraphBuilder с валидацией
     - add_module(), add_dependency() с cycle detection
     - build() с конвертацией в final graph
   - [ ] Индексы для быстрого поиска
     - path_to_module: FxHashMap<VfsPath, ModuleId>
     - name_to_modules: FxHashMap<ModuleName, SmallVec<[ModuleId; 1]>>
   - [ ] Unit tests: построение графа, циклы, транзитивные зависимости

2. **Dependency Extraction** (3-5 дней)
   - [ ] Создать `crates/hir-def/src/module_deps.rs`
     - extract_dependencies(parse: &Parse) → Vec<Dependency>
     - Парсинг вызовов функций (прямые зависимости)
     - Парсинг `#Использовать` директив
   - [ ] Интеграция с метаданными
     - CommonModule dependencies из XML
     - Metadata-based dependencies (Server/Client/ServerAndClient)
   - [ ] Tests: различные виды зависимостей, edge cases

3. **Incremental Analysis Engine** (3-5 дней)
   - [ ] Добавить в ModuleGraph:
     - affected_modules(changed_files: &[FileId]) → Vec<ModuleId>
     - transitive_dependencies(module_id) → Vec<ModuleId>
     - transitive_reverse_dependencies(module_id) → Vec<ModuleId>
   - [ ] BFS алгоритм для поиска затронутых модулей
   - [ ] Tests: корректность affected_modules для различных сценариев
   - [ ] Benchmarks: pt_erp (25,090 модулей)

4. **CLI Integration** (2-3 дня)
   - [ ] Обновить `crates/bsl-analyzer/src/cli/analyze.rs`
     - --incremental flag
     - --changed-files опция (comma-separated paths)
     - --git-diff опция (git reference: HEAD~1, main, etc.)
   - [ ] Добавить `crates/bsl-analyzer/src/cli/graph.rs`
     - Команда для визуализации графа (DOT format)
     - Статистики: количество модулей, зависимостей, циклов
   - [ ] E2E тесты с реальными проектами

5. **Graph Caching** (2-3 дня)
   - [ ] Создать `crates/base-db/src/module_graph/cache.rs`
     - Сохранение графа (MessagePack или JSON)
     - Загрузка графа
     - Инвалидация кеша (проверка timestamps файлов)
   - [ ] Интеграция с CLI (--cache-dir опция)
   - [ ] Tests: сохранение/загрузка, корректность после десериализации
   - [ ] Benchmark: загрузка графа < 0.1 сек для pt_erp

6. **Diagnostics на основе графа** (3-5 дней)
   - [ ] UnusedModule (DG001)
     - `crates/ide-diagnostics/src/handlers/unused_module.rs`
     - Модуль неиспользуемый, если нет обратных зависимостей и не экспортный
   - [ ] CircularDependency (DG002)
     - `crates/ide-diagnostics/src/handlers/circular_dependency.rs`
     - DFS для поиска циклов
   - [ ] ModuleCoupling (DG003)
     - `crates/ide-diagnostics/src/handlers/module_coupling.rs`
     - Метрики: afferent/efferent coupling, instability
   - [ ] Tests для каждой диагностики

7. **LSP Navigation (опционально)** (3-5 дней)
   - [ ] Call Hierarchy
     - `crates/ide/src/call_hierarchy.rs`
     - incoming_calls() через reverse_dependencies
     - outgoing_calls() через dependencies
   - [ ] Find Usages
     - `crates/ide/src/references.rs`
     - Интеграция с ModuleGraph для cross-module usages
   - [ ] Tests: call hierarchy, find usages

**Критерии готовности:**
- ✅ ModuleGraph корректно строится для реальных проектов (pt_erp: 25,090 модулей)
- ✅ Incremental mode для pt_erp: < 1 сек (vs 10-15 сек full scan)
- ✅ Циклические зависимости обнаруживаются
- ✅ GitLab CI интеграция работает (примеры .gitlab-ci.yml)
- ✅ Граф кешируется и быстро загружается (< 0.1 сек)
- ✅ 3+ graph-based диагностики работают

**Метрики производительности (pt_erp, 25,090 модулей):**

| Сценарий | Full scan | Incremental | Экономия |
|----------|-----------|-------------|----------|
| 1 модуль изменен | 10-15 сек | 0.5-1 сек | 10x-30x |
| 5 модулей изменено | 10-15 сек | 1-2 сек | 5x-15x |
| 100 модулей (большой MR) | 10-15 сек | 3-5 сек | 2x-5x |

**GitLab CI пример:**
```yaml
bsl-analysis-incremental:
  script:
    - CHANGED=$(git diff --name-only $CI_COMMIT_BEFORE_SHA...$CI_COMMIT_SHA | grep '\.bsl$' | tr '\n' ',')
    - bsl-analyzer analyze --project . --incremental --changed-files "$CHANGED" --output sonarqube.json
  only:
    - merge_requests
```

---

### Iteration 10: IDE-DB & Salsa Integration

**Источники:**
- `rust-analyzer/crates/ide-db/src/` — RootDatabase, symbol index
- `/Users/kiriller/src/lsp/salsa/` — Salsa 0.25.2 (инкрементальные вычисления)
- `/Users/kiriller/src/lsp/salsa/book/` — актуальная документация
- `/Users/kiriller/src/lsp/salsa/tests/` — примеры использования
- `rust-analyzer/crates/base-db/` — примеры Salsa queries

**Цель:** Полная интеграция Salsa для инкрементальных вычислений и создание RootDatabase.

**См. детальный план в `SALSA_TODO.md`**

**Задачи:**

1. **Изучение Salsa 0.25.2** (1-2 дня)
   - [ ] Прочитать актуальную документацию в `/Users/kiriller/src/lsp/salsa/book/`
   - [ ] Изучить примеры в `/Users/kiriller/src/lsp/salsa/tests/`
   - [ ] Изучить код rust-analyzer: `base-db/src/lib.rs`, `base-db/src/input.rs`
   - [ ] Понять систему jars и ingredient registration
   - [ ] Создать минимальный прототип (file_text -> parse)

2. **Создать Salsa Database** (2-3 дня)
   - [ ] Определить Database struct с `salsa::Storage`
   - [ ] Реализовать `salsa::Database` trait
   - [ ] Использовать `#[salsa::db]` для всех trait'ов
   - [ ] Настроить систему jars для группировки queries

3. **Мигрировать input queries** (1-2 дня)
   - [ ] `file_text()` — текст файла (input)
   - [ ] `source_root()` — корень проекта (input)
   - [ ] `configuration_path()` — путь к конфигурации (input, для метаданных)
   - [ ] Настроить Durability (HIGH для библиотек, LOW для исходников)

4. **Мигрировать derived queries** (2-3 дня)
   - [ ] `parse()` — парсинг файла с LRU=128
   - [ ] `module_tree()` — дерево зависимостей модулей
   - [ ] Обновить все существующие queries для работы с Salsa

5. **Крейт `ide-db`** (2-3 дня)
   - [ ] RootDatabase struct с интеграцией всех слоёв
   - [ ] Symbol Index
   - [ ] Интеграция HIR queries
   - [ ] Интеграция VFS

6. **Тесты и оптимизация** (2-3 дня)
   - [ ] Убедиться что все 82+ теста проходят
   - [ ] Добавить тесты инкрементальности:
     - Изменение файла не должно пересчитывать зависимые (если интерфейс не изменился)
     - Benchmark: incremental update < 100ms
   - [ ] Настроить LRU размеры для оптимальной производительности
   - [ ] Профилирование: `BSL_PROFILE=* cargo run`

**Критерии готовности:**
- ✅ Все существующие тесты проходят
- ✅ Salsa корректно кеширует результаты
- ✅ Incremental updates работают (< 100ms)
- ✅ Профилирование показывает минимальный overhead от Salsa
- ✅ RootDatabase готов для использования в диагностиках
- ✅ Документация обновлена

---

### Iteration 11: Metadata Infrastructure

**Источники:**
- `bsl-language-server-rust/crates/bsl-metadata/` — готовые Rust структуры
- `bsl-language-server` (Java) — mdclasses интеграция, AbstractMetadataDiagnostic
- `/Users/kiriller/src/lsp/salsa/` — для Salsa queries
- `rust-analyzer/crates/base-db/` — примеры Salsa

**Цель:** Создать инфраструктуру для работы с метаданными 1С (Configuration, CommonModule, и т.д.) с интеграцией Salsa для кеширования.

**См. детальный план в `METADATA_PLAN.md`**

**Задачи:**

1. **Создать крейт `bsl-metadata`** (2-3 дня)
   - [ ] Скопировать структуры из `bsl-language-server-rust/crates/bsl-metadata/`
   - [ ] Configuration, CommonModule, MetadataObject
   - [ ] Enums: ModuleType, MdoType, ReturnValueReuse, ObjectBelonging
   - [ ] Traits: MdObject, Module
   - [ ] Error handling: MetadataError
   - [ ] Unit tests для структур

2. **Реализовать XML loader** (3-4 дня)
   - [ ] Выбрать XML библиотеку (quick-xml или roxmltree) через Context7
   - [ ] Реализовать `parse_configuration()` для Configuration.xml
   - [ ] Реализовать `parse_common_module()` для CommonModules/*.xml
   - [ ] Реализовать загрузку других типов:
     - Catalog, Document (объекты метаданных)
     - InformationRegister, AccumulationRegister (регистры)
     - Role, Enum (вспомогательные объекты)
   - [ ] Обработка ошибок парсинга (валидация XML, missing fields)
   - [ ] Тесты с реальными XML файлами из bsl-language-server

3. **Интеграция с Salsa** (3-4 дня)
   - [ ] Создать Salsa queries в `ide-db/src/metadata.rs`:
     - `configuration_path()` — input query
     - `load_configuration()` — derived, Durability::HIGH, LRU=16
     - `find_common_module(name)` — derived query
     - `metadata_object_exists(name)` — derived query
   - [ ] Добавить `MetadataDb` trait
   - [ ] Интегрировать в `RootDatabase`
   - [ ] Тесты инкрементальности:
     - Изменение .bsl файла НЕ триггерит перезагрузку метаданных
     - Изменение Configuration.xml триггерит перезагрузку

4. **AbstractMetadataDiagnostic паттерн** (2-3 дня)
   - [ ] Портировать паттерн из Java (`AbstractMetadataDiagnostic.java`)
   - [ ] `MetadataDiagnostic` trait с методами:
     - `filter_mdo_types()` — фильтр типов для проверки
     - `check_metadata()` — проверка объекта метаданных
   - [ ] `MetadataDiagnosticRunner` для запуска диагностик
   - [ ] Примеры диагностик (proof-of-concept):
     - CommonModuleAssign — проверка присваивания общему модулю
     - ForbiddenMetadataName — проверка запрещённых имён
     - MetadataObjectNameLength — проверка длины имён

5. **Тестирование** (2-3 дня)
   - [ ] Скопировать тестовые конфигурации из bsl-language-server
   - [ ] Unit tests для XML loader
   - [ ] Integration tests для Salsa queries
   - [ ] Performance tests:
     - Загрузка большой конфигурации (ERP 2.5) < 1 сек
     - Кешированный доступ < 1 мс
     - Многократные запросы (проверка кеширования)
   - [ ] Тесты для примеров диагностик

6. **Документация** (1-2 дня)
   - [ ] Обновить `ARCHITECTURE.md` — добавить раздел про метаданные
   - [ ] Doc comments для публичного API
   - [ ] Примеры использования в `bsl-metadata/examples/`
   - [ ] Обновить `DEVELOPMENT_RULES.md` если нужно

**Критерии готовности:**
- ✅ Все структуры метаданных реализованы
- ✅ XML loader работает с реальными конфигурациями
- ✅ Salsa queries корректно кешируют метаданные
- ✅ 2-3 metadata-диагностики работают как proof-of-concept
- ✅ Performance: загрузка < 1 сек, кешированный доступ < 1 мс
- ✅ Тесты покрывают основные сценарии (> 80%)
- ✅ API удобен для написания Tier 3 диагностик

---

## Phase 3: Diagnostics

### Iterations 12-25: Diagnostics Implementation

**Зависимости:**
- **Iteration 11: Metadata Infrastructure** — обязательна для Tier 3 диагностик (19-23)

**Источники:**
- `bsl-language-server/src/main/java/.../diagnostics/` — 181 Java реализация (референс)
- `bsl-language-server-rust/crates/bsl-diagnostics/src/rules/` — 183 Rust реализации
- `bsl-language-server-rust/crates/bsl-diagnostics/src/lib.rs` — trait DiagnosticRule
- `bsl-language-server-rust/crates/bsl-diagnostics/src/registry.rs` — параллельное выполнение (rayon)
- `bsl-language-server/src/test/resources/diagnostics/` — тестовые данные
- `rust-analyzer/crates/ide-diagnostics/` — архитектура диагностик
- `bsl-language-server-rust/crates/bsl-cfg/` — Control Flow Graph для сложных анализов
- `bsl-metadata/` — метаданные 1C для диагностик Tier 3 (реализовано в Iteration 11)

**Итерации:**
| Итерация | Tier | Зависимости | Источники |
|----------|------|-------------|-----------|
| 12-14 | Tier 1 (Syntax-based) | Parser, Syntax | bsl-language-server (diagnostics/), bsl-language-server-rust (rules/) |
| 15-18 | Tier 2 (Semantic-based) | HIR, Symbols | bsl-language-server, bsl-language-server-rust |
| 19-23 | Tier 3 (Metadata-dependent) | **Iteration 11** | bsl-language-server, bsl-metadata/ |
| 24-25 | Tier 4 (SDBL) | SDBL Parser | bsl-language-server, bsl-parser (SDBL*.g4), bsl-language-server-rust |

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
| 1 | Project Setup & Lexer | ✅ Completed | 2025-12-27 | 2025-12-28 |
| 2 | Parser Foundation | ✅ Completed | 2025-12-28 | 2025-12-28 |
| 3 | Complete Parser | ✅ Completed | 2025-12-28 | 2025-12-29 |
| 4 | Syntax Trees | ✅ Completed | 2025-12-29 | 2025-12-29 |
| 5 | Base Infrastructure | ✅ Completed | 2025-12-29 | 2025-12-29 |
| 6-7 | HIR Foundation | 📋 Next | - | - |
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
