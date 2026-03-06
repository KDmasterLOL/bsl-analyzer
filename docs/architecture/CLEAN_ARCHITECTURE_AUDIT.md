# Clean Architecture Audit — BSL Analyzer

> Дата: 2026-03-07 (обновление)
> Предыдущий аудит: 2026-02-17
> Методология: Robert C. Martin (Clean Architecture, SOLID) + анализ дублирования кода
> Объём: 31 crate, ~170k строк Rust

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

### C1. [ИСПРАВЛЕНО] Нарушение LSP: `Lattice::bottom()` паникует в 2 из 3 реализаций

**Статус:** [x] Исправлено (коммит ce7a244)

### C2. [ИСПРАВЛЕНО] Нарушение OCP: `metadata_registry.rs` — 4352 строки, match на 220 веток

**Статус:** [x] Исправлено (коммит ce7a244)

### C3. [ИСПРАВЛЕНО] Shotgun Surgery: добавление диагностики требует правок в 5–6 местах

**Статус:** [x] Исправлено (коммит ce7a244)

### C4. [НОВАЯ] `UnusedLocalVariable` эмитится из двух путей, дедупликация отсутствует

| Файл | Строка |
|---|---|
| `crates/ide-diagnostics/src/lib.rs` | 133 |
| `crates/ide-diagnostics/src/runner.rs` | 1087 |
| `crates/ide-diagnostics/src/hir_dispatch.rs` | 74, 144-146 |

`UnusedLocalVariable` зарегистрирована в **двух** коллекторах: `HIR_DIAGNOSTICS` (через `BodyDiagnostic::UnusedVariable` → `from_hir()`) и `DATAFLOW_DIAGNOSTICS` (через liveness analysis → `check()`). Обе могут обнаружить одну и ту же неиспользуемую переменную.

Функция `deduplicate_diagnostics` обрабатывает **только** `UnreachableCode`:
```rust
let dedupe_codes = [DiagnosticCode::UnreachableCode];
```

Комментарий в тесте (`runner.rs:1085-1086`) утверждает, что обе дуально-зарегистрированные диагностики "deduplicated at runtime by `deduplicate_diagnostics` in lib.rs" — это **ложь** для `UnusedLocalVariable`. В результате пользователь получает **дубликаты диагностик**.

`IncorrectUseOfStrTemplate` безопасна — HIR-путь обрабатывает литералы, dataflow-путь обрабатывает переменные (непересекающиеся множества). Но безопасность зависит от implementation detail, а не от архитектурной гарантии.

**Решение:** Отключить HIR-путь для `UnusedLocalVariable` (аналогично `UnreachableCode` — return `None` в `hir_dispatch.rs`), т.к. dataflow-анализ строго точнее. Удалить `BodyDiagnostic::UnusedVariable` из lowering.

---

## HIGH

### H3. [ОТЛОЖЕНО] Нарушение ISP: `RootDatabase` → дублирование `AnalysisProvider`

**Файлы:** `crates/ide-db/src/lib.rs`, `crates/ide-diagnostics/src/context.rs`

`DiagnosticsContext` содержит 20+ helper-методов с идентичным шаблоном:
```rust
if let Some(provider) = self.provider { return provider.X(); }
self.db.X()
```

При добавлении Salsa-запроса — правка в 3 местах: `RootDatabase`, `AnalysisProvider`, `DiagnosticsContext`.

**Статус:** [ ] Отложено — рефакторинг Salsa-трейта слишком рискован

### H4-H6, H8-H9. [ИСПРАВЛЕНО] Дублирование common_module, forbidden_names, test setup, hir facade

**Статус:** [x] Исправлено

### H7, H10. [ЗАКРЫТО] By design

### NEW-H1. `resolve_local_to_definition` возвращает неправильный MethodId

| Файл | Строка |
|---|---|
| `crates/hir/src/lib.rs` | 431-445, 458-472 |

Метод ищет `ProcedureDef`/`FunctionDef` в синтаксическом дереве, затем перебирает ItemTree. Внутренний цикл возвращает **первый** найденный procedure/function, а не тот, который соответствует объемлющему синтаксическому узлу. Если в файле несколько процедур — для всех, кроме первой, будет возвращён неправильный `MethodId`.

```rust
for (idx, item) in tree.top_level_items().iter().enumerate() {
    if let hir_def::item_tree::ModItem::Procedure(_) = item {
        let method_id = MethodId { module: module_id, local_id: idx as u32 };
        return Some(...);  // ← всегда первая процедура!
    }
}
```

**Влияние:** Баг в goto definition для локальных символов (параметров, переменных).

**Решение:** Сравнивать range синтаксического узла найденного ItemTree элемента с range объемлющего `ProcedureDef`/`FunctionDef`, или матчить по имени метода.

### NEW-H2. `param_index: 0` — TODO в production-коде

| Файл | Строка |
|---|---|
| `crates/hir/src/lib.rs` | 438, 465 |

```rust
param_index: 0, // TODO: get actual index
```

Все параметры резолвятся с `param_index: 0`. Любой consumer, зависящий от `param_index` (signature help, rename), будет работать неправильно для параметров после первого.

**Решение:** Получить реальный индекс из `ExprScopes`.

### NEW-H3. `unwrap()` на URI из LSP-клиента → panic на non-file URIs

| Файл | Строка |
|---|---|
| `crates/bsl-analyzer/src/handlers/notification.rs` | 90, 171 |

```rust
let vfs_path = vfs::VfsPath::new(uri.to_file_path().unwrap());
```

`to_file_path()` возвращает `Err` для non-`file://` URIs (`untitled:`, `vscode-notebook-cell:`). Сервер упадёт при получении такого URI от клиента.

**Решение:** `uri.to_file_path().ok()` + early return.

### NEW-H4. Dead code: `BodyDiagnostic::UnreachableCode` эмитится из 5 мест, но dispatch возвращает `None`

| Файл | Строки эмиссии |
|---|---|
| `crates/hir-def/src/body/lower/stmt.rs` | 572 |
| `crates/hir-def/src/body/lower/mod.rs` | 1027, 1139 |
| `crates/hir-def/src/body/lower/preproc.rs` | 170, 279 |
| `crates/ide-diagnostics/src/hir_dispatch.rs` | 147-149 |

`UnreachableCode` диагностики вычисляются при lowering, хранятся в `LowerResult.diagnostics`, итерируются в `collect_hir_diagnostics`, и **тихо отбрасываются** (return `None`). CFG-based детекция в `unreachable_code::check()` — единственный рабочий путь.

**Влияние:** Впустую тратятся CPU-циклы на вычисление, хранение и итерацию бесполезных диагностик.

**Решение:** Удалить `BodyDiagnostic::UnreachableCode` вариант и все 5 мест эмиссии.

### NEW-H5. Двойная регистрация диагностик — вводящий в заблуждение комментарий

| Файл | Строка |
|---|---|
| `crates/ide-diagnostics/src/runner.rs` | 1085-1088 |

Комментарий в тесте:
```rust
// Codes intentionally in multiple collectors (detected via both paths,
// deduplicated at runtime by deduplicate_diagnostics in lib.rs)
let known_dual_registration: &[DiagnosticCode] =
    &[DiagnosticCode::IncorrectUseOfStrTemplate, DiagnosticCode::UnusedLocalVariable];
```

Утверждение о runtime-дедупликации **ложно** — `deduplicate_diagnostics` обрабатывает только `UnreachableCode`. Если разработчик добавит новую dual-registered диагностику, полагаясь на этот комментарий, получит дубликаты.

**Решение:** Исправить комментарий + убрать dual-registration (см. C4).

---

## MEDIUM

### M1. [ОТКРЫТО] `base-db` зависит от `parser` — нарушение слоёв

`crates/base-db/Cargo.toml:11`. `base-db` (Layer 4) зависит от `parser` (Layer 3) для Salsa-запроса `parse_query`.

### M2. [ОТКРЫТО] Две параллельные системы type inference

- `crates/hir-def/src/ty/infer.rs` — старая AST-based
- `crates/hir-ty/src/infer.rs` — новая HIR-based

Обе содержат "Phase 1" в документации. Обе существуют параллельно. Разработчик не знает, куда добавлять новую логику.

### M3. [ОТКРЫТО] Дублирование `parse_module_path` — две реализации

- `hir-def/src/module_index.rs:223` — `(ModulePathType, String)`, case-insensitive
- `ide-db/src/metadata.rs:237` — `ModulePathInfo`, case-sensitive

### M4. [ОТКРЫТО] `BodyDiagnostic` — 80+ variants, все содержат `range`

`crates/hir-def/src/body.rs`. `fn range()` — 80 однообразных match-рук.

### M5. [ОТКРЫТО] `runner.rs` — 1116 строк, registry + dispatcher + orchestrator

9 const-массивов + 8 коллекторных функций.

### M9. [ОТКРЫТО] `ide` использует утилиты из `ide-diagnostics`

`crates/ide/src/syntax_highlighting/sdbl.rs:76-182` — `SdblPositionMapper`, `extract_string_content`, `build_line_index_shared`. Принадлежат `sdbl-hir` или `ide-db`.

### M10. [ОТКРЫТО] `petgraph` — неиспользуемая зависимость в `hir-def`

`crates/hir-def/Cargo.toml:35` — `petgraph = "0.8.3"`. Ни один `.rs` файл не использует.

### NEW-M1. `catch_unwind` маскирует баги в diagnostic dispatch

| Файл | Строка |
|---|---|
| `crates/ide-diagnostics/src/hir_dispatch.rs` | 103-106 |
| `crates/ide-diagnostics/src/metadata_dispatch.rs` | 42-45 |

`catch_unwind(AssertUnwindSafe(...))` вокруг `ctx.module_bodies()` тихо проглатывает панику, возвращая пустой `Vec`. В LSP-сервере (`notification.rs`, `main.rs`) `catch_unwind` имеет смысл (Salsa cancellation). Но внутри diagnostic dispatch — это маскировка багов.

**Решение:** Проверять preconditions (наличие source_root) вместо catch_unwind.

### NEW-M2. `regex` в `hir-def` — дублирование + нарушение правил

| Файл | Строка |
|---|---|
| `crates/hir-def/src/body/lower/expr.rs` | 2082-2134 |

3 функции (`is_wrong_str_template`, `compare_template_and_params`, `various_params`) используют regex. В `ide-diagnostics/src/handlers/incorrect_use_of_str_template.rs:320-331` есть regex-free реализация. Нарушение правила CLAUDE.md: "Использовать regexp нельзя".

### NEW-M3. Дублирование `SymbolInfo`/`SymbolKind`

- `hir-def/src/workspace_index.rs:29-36` — `SymbolKind::Method, Variable`
- `ide-db/src/lib.rs:52-65` — `SymbolKind::Procedure, Function, Variable, Region`

### NEW-M4. SDBL — 16 handler'ов с идентичным ~30-строчным boilerplate

| Файлы |
|---|
| `using_like_in_query.rs`, `union_all.rs`, `full_outer_join_query.rs`, `join_with_sub_query.rs`, `virtual_table_call_without_parameters.rs`, `join_with_virtual_table.rs`, + 10 ещё |

Паттерн: disabled-check → `sdbl_hir_in_file()` → `all_sdbl_in_file()` → `build_line_index_shared` → zip-итерация → match на конкретный `SdblDiagnostic` вариант → `mapper.map_range` → push `Diagnostic`.

~350 строк чистого дублирования. `common_module_helpers.rs` уже показал, что команда умеет абстрагировать такие паттерны.

**Решение:** Helper `collect_sdbl_single_variant(ctx, code, |diag| ...)` в `sdbl_utils.rs`.

### NEW-M5. Дублирование `is_nstr_call` / `has_template_in_parents` / `Config` между двумя multilingual handlers

| Файлы |
|---|
| `multilingual_string_has_all_declared_languages.rs:75-85` |
| `multilingual_string_using_with_template.rs:73-83` |

3 функции + struct `Config` + const `DEFAULT_DECLARED_LANGUAGES` идентично скопированы.

**Решение:** Вынести в `utils/nstr.rs`.

### NEW-M6. `MethodData`/`ParameterData`/`VariableData` используют `String` вместо `Name`

| Файл | Строка |
|---|---|
| `crates/hir-def/src/lib.rs` | 502-535 |

Проект определяет тип `Name` (SmolStr, case-insensitive compare), но 4 публичных struct используют raw `String`. Каждое сравнение требует ручного `.to_lowercase()`. Риск case-sensitivity багов.

### NEW-M7. ItemTree traversal дублируется 7+ раз в `hir/src/definition.rs`

| Файл | Строка |
|---|---|
| `crates/hir/src/definition.rs` | 160-324 |

Паттерн `for (idx, item) in tree.top_level_items().iter().enumerate()` с match на `ModItem` повторяется в `name()`, `is_export()`, `source_range()`, `name_range()`, `label()`. При этом `Method::method_info()` в том же crate уже использует эффективный `.get()`.

**Решение:** Общий helper `resolve_item(tree, local_id) -> Option<&ModItem>`.

### NEW-M8. `bsl-analyzer` — skip-layer access (16 внутренних зависимостей)

| Файл | Строка |
|---|---|
| `crates/bsl-analyzer/Cargo.toml` | 15-31 |

LSP-сервер (Layer 12) напрямую зависит от `base-db` (Layer 4), `parser` (Layer 3), `cfg` (Layer 6), `dataflow` (Layer 6), `bsl-metadata` (Layer 1) — пропускает 5-8 слоёв. Должен зависеть только от `ide` + инфраструктура (`vfs`, `paths`).

---

## LOW

### L1. [ОТКРЫТО] `cfg-types-research` — забытый scaffold crate

Содержит только `pub fn add()`. Никем не используется.

### L8. [ОТКРЫТО] `cfg` re-exports `petgraph::graph::NodeIndex`

### L9. [ОТКРЫТО] Закомментированный `// pub mod cfg_builder;` в `hir-def/src/lib.rs:28`

### L10. [ОТКРЫТО] Dev-dependencies нарушают layering

`hir-def`, `hir`, `dataflow` используют `ide-db` в `[dev-dependencies]`.

### NEW-L1. 37 TODO/FIXME в production-коде

Наиболее значимые:
- `hir/src/lib.rs:438,465` — `param_index: 0` (→ поглощён NEW-H2)
- `ide-assists/src/lib.rs:40` — `// TODO: Implement assists`
- `ide/src/lib.rs:128` — `// TODO: Implement`
- `hir-ty/src/infer.rs:213,410,522` — Phase 2+ placeholders

### NEW-L2. `TextRange::empty(0.into())` — 21+ копия

Паттерн для module-level диагностик. Нужна const `MODULE_RANGE` или хелпер.

### NEW-L3. Dead legacy `check()` в 8 single-pass handlers

| Файлы |
|---|
| `useless_ternary_operator.rs`, `double_negatives.rs`, `unknown_preprocessor_symbol.rs`, `bad_words.rs`, `nested_ternary_operator.rs`, `yo_letter_usage.rs`, `magic_date.rs`, `typo.rs` |

Каждый сохраняет legacy `check()` (full AST traversal) рядом с `check_node()`/`check_token()` (single-pass). Legacy используется только в invariant-тестах. Двойная стоимость поддержки — при изменении логики нужно менять оба.

### NEW-L4. Ручной timing в 15 SDBL handlers, дублирующий `run_diagnostic`

15 SDBL handlers используют `Instant::now()` + `debug!(time_ms=...)`. При этом `run_diagnostic()` в `runner.rs` уже оборачивает каждый вызов handler'а с timing (порог >80ms). Два перекрывающихся механизма.

### NEW-L5. 12 dead-комментариев в `collect_ast_diagnostics`

`runner.rs:534-568` — функция из 14 строк, из которых 12 — комментарии о мигрированных диагностиках. Археологический мусор.

### NEW-L6. `using_hardcode_path` — последний legacy AST-диагностик

Единственная диагностика в `collect_ast_diagnostics()`. Все остальные мигрированы. Использует regex (нарушение правил). Целый коллектор существует ради одного handler'а.

---

## Сводка

| Severity | Всего | Новые | Из прошлого | Исправлено/Закрыто |
|---|---|---|---|---|
| **CRITICAL** | 4 | 1 (C4) | 3 → все исправлены | 3 |
| **HIGH** | 11 | 5 (NEW-H1..H5) | 10 → 9 исправлены, 1 отложен | 9 |
| **MEDIUM** | 18 | 8 (NEW-M1..M8) | 10 → 0 исправлены | 0 |
| **LOW** | 10 | 6 (NEW-L1..L6) | 10 → 6 открыто | 0 |

---

## Рекомендуемые приоритеты

### Наивысший приоритет (баги)

1. **C4** — `UnusedLocalVariable` дубликаты в production. Fix: return `None` для `BodyDiagnostic::UnusedVariable` в hir_dispatch.
2. **NEW-H1** — `resolve_local_to_definition` возвращает wrong MethodId. Fix: match по range/name.
3. **NEW-H3** — `unwrap()` на non-file URI → server crash. Fix: `.ok()` + early return.

### Высокий приоритет (dead code / misleading)

4. **NEW-H4** — удалить `BodyDiagnostic::UnreachableCode` + 5 emission sites (dead code).
5. **NEW-H5** — исправить ложный комментарий о дедупликации.

### Средний приоритет (архитектура, влияет на доработку)

6. **M2** — две type inference → разобраться, какая актуальна.
7. ~~**NEW-M4** — SDBL boilerplate → extract helper (~350 lines saved).~~ ✅ Исправлено
8. **NEW-M6** — `String` → `Name` в `MethodData`/`ParameterData`.
9. ~~**NEW-M7** — ItemTree lookup helper (7-way duplication).~~ ✅ Исправлено

### Быстрые wins (< 1 часа)

10. **L1** — удалить `cfg-types-research`.
11. **M10 + L9** — удалить `petgraph` из hir-def + закомментированный `cfg_builder`.
12. ~~**NEW-L2** — const `MODULE_RANGE`.~~ ✅ Исправлено
13. **NEW-L5** — удалить dead-комментарии в `collect_ast_diagnostics`.

---

## Прогресс

> Обновлено: 2026-03-07

| Severity | Всего | Исправлено | Закрыто (by design) | Отложено | Открыто |
|---|---|---|---|---|---|
| **CRITICAL** | 4 | 3 | 0 | 0 | 1 |
| **HIGH** | 11 | 7 | 2 | 1 | 5 (NEW) |
| **MEDIUM** | 18 | 4 | 0 | 0 | 14 |
| **LOW** | 10 | 2 | 0 | 0 | 8 |

### Исправлено в этом аудите

- **NEW-L2** — `TextRange::empty(0.into())` заменён на `syntax::MODULE_RANGE` (14 файлов)
- **NEW-L3** — Удалён мёртвый timing-код из 15 SDBL handlers
- **NEW-M2** — Regex удалён из `hir-def` (было исправлено ранее)
- **NEW-M4** — SDBL boilerplate → `collect_sdbl_simple()` helper (10 handlers, ~200 строк сэкономлено)
- **NEW-M5** — Дублирование NStr-утилит → общий `utils/nstr.rs` (7 функций, ~160 строк сэкономлено)
- **NEW-M7** — ItemTree traversal: 7 линейных сканов → `.get()` в `definition.rs`
