# Clean Architecture Audit — BSL Analyzer

> Дата: 2026-02-17
> Методология: Robert C. Martin (Clean Architecture, SOLID) + анализ дублирования кода
> Объём: 31 crate, ~100k+ строк Rust

## Иерархия слоёв (текущая)

```
Layer 12: bsl-analyzer (LSP server binary)
Layer 11: ide
Layer 10: ide-diagnostics, ide-assists
Layer  9: ide-db
Layer  8: test-fixture (test support)
Layer  7: hir
Layer  6: hir-ty, cfg, dataflow
Layer  5: hir-def, sdbl-hir
Layer  4: base-db
Layer  3: parser
Layer  2: syntax, lexer
Layer  1: vfs, bsl-metadata, bsl-platform, project-model
Layer  0: paths, stdx, intern, line-index, cfg-types, profile, test-utils
```

---

## CRITICAL

### C1. Нарушение LSP: `Lattice::bottom()` паникует в 2 из 3 реализаций

| Файл | Строка |
|---|---|
| `crates/dataflow/src/reaching_defs.rs` | 536 |
| `crates/dataflow/src/liveness.rs` | 332 |

Трейт `Lattice` требует `fn bottom() -> Self`, но `ReachingDefs` и `Liveness` паникуют при вызове. `DataflowSolver` вызывает `L::bottom()` в `or_insert_with` (строка 351) — латентный баг при определённых путях выполнения. Обходной путь через `set_bottom_factory()` не гарантирует безопасность.

**Решение:** Изменить трейт на `fn bottom(ctx: &BottomContext) -> Self` или убрать `bottom()` из трейта и использовать фабрику.

**Статус:** [x] Исправлено (коммит ce7a244) — `bottom()` удалён из трейта, `set_bottom_factory()` обязателен

### C2. Нарушение OCP: `metadata_registry.rs` — 4352 строки, match на 220 веток

| Файл | Строка |
|---|---|
| `crates/ide-diagnostics/src/metadata_registry.rs` | 94–316 |

Каждая новая диагностика = ещё одна ветка match в `get_metadata()` + const определение. Файл уже 4352 строки. Нарушение Open/Closed Principle — модификация вместо расширения.

**Решение:** Inventory/linkme или `#[diagnostic_metadata]` proc-macro для автоматической регистрации.

**Статус:** [x] Исправлено (коммит ce7a244) — `define_metadata!` macro, const METADATA в каждом handler, metadata_registry.rs сокращён до ~560 строк

### C3. Shotgun Surgery: добавление диагностики требует правок в 5–6 местах

Для каждой новой диагностики нужно изменить:
1. `code.rs` — новый вариант enum
2. `metadata_registry.rs` — match arm + const metadata (4352 строки)
3. `runner.rs` — добавить в нужный const-массив + dispatch-функцию
4. `handlers/` — новый файл
5. `handlers.rs` — `pub mod`
6. `hir_dispatch.rs` — если HIR-диагностика (+70 match arms)

При 180 диагностиках — это 1000+ мест ручной синхронизации.

**Решение:** Единая точка регистрации с автогенерацией dispatch-кода.

**Статус:** [x] Исправлено (коммит ce7a244) — metadata co-located в handler файлах, metadata_registry только dispatch

---

## HIGH

### H1. Нарушение SRP: `sdbl-hir/src/lib.rs` — 3206 строк, 6 ответственностей

Смешаны: типы, строковые утилиты, определение позиции, completion-контекст, парсинг-хелперы, тесты. Должно быть минимум 4 модуля.

**Статус:** [x] Исправлено — lib.rs 3207→223 строк, извлечены `literal.rs`, `position_detector.rs`, `context_detector.rs`

### H2. Нарушение SRP: `ide-db/src/lib.rs` — 1755 строк

Database struct + 6 trait impl'ов + бизнес-логика (`build_module_metadata`, `find_common_module_by_uri`) + re-exports + filesystem-хелперы.

**Статус:** [x] Исправлено — lib.rs 1755→690 строк, бизнес-логика в `metadata.rs`, VFS-хелперы в `vfs_helpers.rs`, тесты в `database_impl_tests.rs`

### H3. Нарушение ISP: `RootDatabase` — 16 методов + `as_any()` escape hatch

`crates/ide-db/src/lib.rs:69-303`. Трейт невозможно реализовать без Salsa — `StreamingProvider` создал параллельный `AnalysisProvider` (15+ методов) вместо реализации `RootDatabase`. `as_any()` — прямой индикатор нарушения ISP.

**Статус:** [ ] Отложено — `as_any()` используется в 4 VFS-хелперах, рефакторинг Salsa-трейта слишком рискован

### H4. Дублирование: 8 файлов `common_module_name_*.rs` (~1121 строк)

8 почти идентичных обработчиков отличаются только списком ключевых слов и кодом диагностики. Дублируются: guard-блок, `is_disabled_with_metadata`, конструкция `Diagnostic`, конструкция `ModuleMetadata` в тестах (28+ раз).

**Статус:** [x] Исправлено — generic `check_common_module_name()` в `common_module_helpers.rs`, 7 handler'ов упрощены до ~15-20 строк

### H5. Дублирование: `FORBIDDEN_NAMES` — два списка

| Файл | Структура данных |
|---|---|
| `rules/forbidden_metadata_name.rs:48-120` | `const &[&str]` (original case) |
| `handlers/forbidden_metadata_name.rs:14-87` | `Lazy<FxHashSet<&str>>` (lowercase) |

**Статус:** [x] Исправлено — модуль `rules/` удалён (dead code), единственный список в `handlers/`

### H6. Дублирование: `MetadataObjectNameLength` — две реализации

| Файл | Паттерн |
|---|---|
| `rules/metadata_object_name_length.rs` | `MetadataDiagnostic` trait |
| `handlers/metadata_object_name_length.rs` | `from_metadata()` функция |

**Статус:** [x] Исправлено — модуль `rules/` удалён (dead code), единственная реализация в `handlers/`

### H7. Нарушение OCP: `hir_dispatch.rs` — 70 match arms

`crates/ide-diagnostics/src/hir_dispatch.rs:128-390`. Каждый новый `BodyDiagnostic` вариант = правка в двух crate'ах.

**Статус:** [x] Закрыто (by design) — inherent complexity, каждый variant имеет уникальные поля

### H8. Дублирование: тестовый setup DB — 100+ копий

Идентичный 8-строчный блок копипастится в 15+ файлов, 100+ тест-функций. `test-fixture` crate существует, но не используется.

**Статус:** [x] Исправлено — `create_test_db()` хелпер, `MetadataTestProvider` единое определение, делегирование между helper-функциями

### H9. Leaky Abstraction: `ide-diagnostics` handlers обходят `hir` facade

15+ обработчиков напрямую импортируют `hir_def::hir::*`, `hir_def::item_tree::*`, `hir_def::body::*`.

**Статус:** [x] Исправлено — re-exports в `hir/src/lib.rs`, ~42 handler файла обновлены на `use hir::`

### H10. Feature Envy: диагностическая логика в HIR lowering

`crates/hir-def/src/body/lower/expr.rs:1623-1940` — 9 функций проверки (`is_os_users_method`, `is_external_app_method` и др.) в HIR-lowering вместо `ide-diagnostics`.

**Статус:** [x] Закрыто (by design) — ~230 строк утилит, перенос в diagnostics потребует re-traversal AST (performance loss)

---

## MEDIUM

### M1. `base-db` зависит от `parser` — нарушение слоёв

`crates/base-db/Cargo.toml:11`. Функция `parse_query` вызывает `parser::parse_bsl()`.

**Статус:** [ ] Не исправлено

### M2. Две параллельных системы type inference

| Файл | Система |
|---|---|
| `hir-def/src/ty/infer.rs` | Старая AST-based (TODO: "Remove in Phase 2") |
| `hir-ty/src/infer.rs` | Новая HIR-based |

**Статус:** [ ] Не исправлено

### M3. Дублирование: `parse_module_path` — две реализации

| Файл | Особенности |
|---|---|
| `hir-def/src/module_index.rs:223` | case-insensitive |
| `ide-db/src/metadata.rs:229` | case-sensitive |

**Статус:** [ ] Не исправлено

### M4. `BodyDiagnostic::range()` — 80 однообразных match arms

`crates/hir-def/src/body.rs:1024-1107`. Все 80 вариантов содержат `range`.

**Статус:** [ ] Не исправлено

### M5. `runner.rs` — 1187 строк, registry + dispatcher + orchestrator

9 const-массивов + 8 collection-функций с захардкоженными списками.

**Статус:** [ ] Не исправлено

### M6. Панники в Salsa queries и XML-парсере

| Файл | Строки |
|---|---|
| `ide-db/src/metadata.rs` | 592, 617, 662 |
| `bsl-metadata/src/xml_parser/mod.rs` | 634, 755, 798 |

**Статус:** [ ] Не исправлено

### M7. Inline `DiagnosticsContext` конструкция — 50+ копий

20+ файлов обработчиков, 50+ повторений конструкции с 8 полями.

**Статус:** [ ] Не исправлено

### M8. Дублирование `SymbolInfo`/`SymbolKind` между crate'ами

| Файл | Варианты |
|---|---|
| `hir-def/src/workspace_index.rs:29` | `Method, Variable` |
| `ide-db/src/lib.rs:49` | `Procedure, Function, Variable, Region` |

**Статус:** [ ] Не исправлено

### M9. `ide` использует утилиты из `ide-diagnostics::sdbl_utils`

`syntax_highlighting.rs` и `sdbl.rs` импортируют `SdblPositionMapper`, `build_line_index_shared` из `ide-diagnostics`. Принадлежат `sdbl-hir` или `ide-db`.

**Статус:** [ ] Не исправлено

### M10. `petgraph` — неиспользуемая зависимость в `hir-def`

`hir-def/Cargo.toml` объявляет `petgraph = "0.8.3"`, ни один файл не импортирует.

**Статус:** [ ] Не исправлено

---

## LOW

### L1. `cfg-types-research` — забытый scaffold crate

Содержит только `pub fn add()`. Никем не используется.

**Статус:** [ ] Не исправлено

### L2. Дублирование `eq_ignore_case`

`hir-def/src/name.rs:30` (метод) и `ide-diagnostics/src/utils/standard_regions.rs:121` (свободная функция).

**Статус:** [ ] Не исправлено

### L3. `TextRange::empty(0.into())` — 27 копий

Должна быть `const MODULE_RANGE` или хелпер `Diagnostic::at_module_start()`.

**Статус:** [ ] Не исправлено

### L4. `#[allow(dead_code)]` на 6 функциях `message_en()`

Мёртвые английские сообщения в 6 обработчиках.

**Статус:** [ ] Не исправлено

### L5. `println!`/`eprintln!` в тестах

`hir-def/src/hir.rs:358-360`, `parser/src/sink.rs:165-256`.

**Статус:** [ ] Не исправлено

### L6. `regex` в `hir-def` при наличии решения без regex

`hir-def/src/body/lower/expr.rs:2025-2075` vs regex-free `ide-diagnostics/src/handlers/incorrect_use_of_str_template.rs:320-331`.

**Статус:** [ ] Не исправлено

### L7. Три разных конвенции сигнатур diagnostic handlers

`check()`, `from_hir()`, `check_node()`, `check_token()`, `from_metadata()`. Некоторые экспортируют оба.

**Статус:** [ ] Не исправлено

### L8. `cfg` re-exports `petgraph::graph::NodeIndex`

`crates/cfg/src/lib.rs:54` — утечка implementation detail.

**Статус:** [ ] Не исправлено

### L9. Commented-out code в `hir-def/src/lib.rs:28`

`// pub mod cfg_builder;`

**Статус:** [ ] Не исправлено

### L10. Dev-dependencies нарушают layering

`hir-def`, `hir`, `dataflow` используют `ide-db` в `[dev-dependencies]`.

**Статус:** [ ] Не исправлено

---

## Сводка

| Severity | Кол-во | Основная тема |
|---|---|---|
| **CRITICAL** | 3 | Паникующий трейт, 4352-строчный match, shotgun surgery |
| **HIGH** | 10 | God-файлы, ISP, массовое дублирование |
| **MEDIUM** | 10 | Нарушения слоёв, две inference, дублирование |
| **LOW** | 10 | Dead code, regex, стилистика |

**Главный системный паттерн:** параллельные инфраструктуры (`rules/` vs `handlers/`, старый vs новый inference, `MetadataDiagnostic` vs `from_metadata()`). Консолидация — самый эффективный рефакторинг.

---

## Прогресс

> Обновлено: 2026-02-17

| Severity | Всего | Исправлено | Закрыто (by design) | Отложено | Открыто |
|---|---|---|---|---|---|
| **CRITICAL** | 3 | 3 | 0 | 0 | 0 |
| **HIGH** | 10 | 7 | 2 | 1 | 0 |
| **MEDIUM** | 10 | 0 | 0 | 0 | 10 |
| **LOW** | 10 | 0 | 0 | 0 | 10 |

**CRITICAL (3/3):** C1 (Lattice::bottom), C2 (metadata_registry 4352→~560), C3 (shotgun surgery → co-located metadata)
**HIGH (9/10):** H1-H2 (god-файлы split), H4-H6 (дублирование удалено), H8 (test_utils), H9 (hir facade), H7+H10 (by design), H3 (отложено)
