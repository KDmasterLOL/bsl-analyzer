# CallGraph: План реализации

## Мотивация

Две задачи требуют инфраструктуры call graph:

### 1. ServerCallsInFormEvents — точная диагностика
**Текущее состояние:** Проверяет суффикс имени метода (`ПриАктивизацииСтроки`, `НачалоВыбора`).
- Ложные срабатывания: idle handler `ПослеОжиданияТаблицаПриАктивизацииСтроки` совпадает по суффиксу
- Пропуски: обработчики с нестандартными именами не обнаруживаются
- Не трассирует вызовы через промежуточные клиентские методы

**Целевое поведение:** Точные обработчики событий из Form.XML → BFS по call chain → обнаружение серверных вызовов на любой глубине.

### 2. NotifyDescription — кросс-модульное подавление
**Текущее состояние:** `UnusedParameters` подавляет только intra-module callbacks (`ЭтотОбъект`/`ThisObject`).
**Целевое поведение:** Проектный reverse index всех NotifyDescription регистраций для кросс-модульного подавления.

---

## Существующая инфраструктура

| Компонент | Что есть | Файл |
|-----------|----------|------|
| `ExternalRef` | Трекает cross-module вызовы (QualifiedCall, ManagerAccess) | `crates/hir-def/src/body.rs:938-951` |
| `LowerResult.external_refs` | Per-method external refs, caller восстановим из bodies[local_id] | `crates/hir-def/src/body.rs:315-328` |
| `SymbolTree` | O(1) case-insensitive method lookup, annotations, export | `crates/hir-def/src/symbol_tree.rs:35-49` |
| `ModuleIndex` | Resolve module name → FileId, поддержка resolve(&ExternalRef) | `crates/hir-def/src/module_index.rs:24-31` |
| `WorkspaceSymbols` | FxHashMap<Name, CommonModuleInfo> для общих модулей | `crates/hir-def/src/workspace.rs:44-49` |
| `Form.XML parser` | Рекурсивно собирает handler names из `<Events>` и `<Commands>` | `crates/bsl-metadata/src/xml_parser/form.rs:136-155` |
| `deprecated_method_call` | Паттерн cross-module resolve: ModuleIndex → SymbolTree → method | `crates/ide-diagnostics/src/handlers/deprecated_method_call.rs:148-181` |
| `unused_local_method` | Ad-hoc intra-module сбор вызовов Expr::Call/MethodCall | `crates/ide-diagnostics/src/handlers/unused_local_method.rs:141-158` |
| `cfg` + `dataflow` | Строго intra-procedural, не расширяемы для inter-procedural | `crates/cfg/`, `crates/dataflow/` |

**Чего нет:** CallGraph, семантическая обработка `ПодключитьОбработчикОжидания`, event type в Form.XML parser.

---

## Архитектура

```
Form XML Metadata (exact event bindings: event_type → handler_name)
          │
          ▼
HIR lowering / ModuleBodies(file)
          │
          ▼
module_call_summary(file)  ← per-module, Salsa LRU=256
  ├─ methods: Vec<MethodSummary>        (local_id, name, dispatch)
  ├─ edges: Vec<CallEdge>              (caller → callee, kind, range)
  ├─ notify_regs: Vec<NotifyReg>       (callback_name, target_module)
  ├─ idle_handler_regs: Vec<IdleReg>   (handler_name, one_shot)
  └─ form_entries: Vec<FormEventEntry>  (event_type, handler_local_id)
          │
    ┌─────┴──────┐
    ▼            ▼
Salsa/LSP    Streaming/CI
demand BFS   final reducer
sidecar idx  batch reverse idx
```

### Типы рёбер (EdgeKind)

```rust
enum EdgeKind {
    /// Прямой вызов локального метода: Метод()
    DirectLocal,
    /// Qualified вызов: ОбщийМодуль.Метод()
    DirectQualifiedModule,
    /// NotifyDescription регистрация: Новый ОписаниеОповещения("Name", Target)
    NotifyDescriptionCallback,
    /// Idle handler: ПодключитьОбработчикОжидания("Name", ...)
    IdleHandlerAttach,
    /// Synthetic: Form.XML event → handler method
    FormEventEntry,
}
```

### Целевой узел (CallTarget)

```rust
enum CallTarget {
    /// Локальный метод текущего модуля
    Local { callee_local_id: u32 },
    /// Qualified cross-module: ОбщийМодуль.Метод
    QualifiedModule { module_name: String, method_name: String, resolved_file: Option<FileId> },
    /// Только имя метода (для idle handler, notify с ЭтотОбъект)
    MethodNameOnly { method_name: String },
    /// Не удалось резолвить
    Unknown,
}
```

### Dispatch классификация методов

```rust
enum MethodDispatch {
    ClientOnly,          // &НаКлиенте
    ServerOnly,          // &НаСервере
    ServerNoContext,     // &НаСервереБезКонтекста
    ClientServer,        // &НаКлиентеНаСервереБезКонтекста
    Unknown,             // нет аннотации
}
```

### BFS для ServerCallsInFormEvents

- Старт: точные form event handlers из Form.XML (OnActivateRow, OnStartChoice)
- Следуем: только `DirectLocal` и `DirectQualifiedModule` (синхронные вызовы)
- НЕ следуем: `IdleHandlerAttach`, `NotifyDescriptionCallback` (асинхронные)
- Стоп: метод с `MethodDispatch::ServerOnly` или `ServerNoContext` → диагностика
- Caps: max depth 64, max visited 10000
- Результат: shortest path для сообщения диагностики

---

## Фазы реализации

### Фаза 1: Фундамент — типы и metadata (low risk)

**Файлы:**
- `crates/hir-def/src/call_graph.rs` — NEW: `ModuleCallSummary`, `CallEdge`, `EdgeKind`, `CallTarget`, `MethodSummary`, `MethodDispatch`
- `crates/bsl-metadata/src/xml_parser/form.rs` — FIX: `collect_events_recursive` собирает `(event_type, handler_name)` вместо только `handler_name`
- `crates/bsl-metadata/src/form.rs` — FIX: `FormEventHandler { event_type: String, handler_name: String }` вместо `String`

**Задачи:**
1. Определить типы данных CallGraph в `hir-def`
2. Расширить Form.XML parser: собирать event type (атрибут `name` из `<Event name="OnActivateRow">`)
3. Обновить `Form` struct: `event_handlers: Vec<FormEventHandler>`
4. Тесты: парсинг Form.XML с event types
5. Обратная совместимость: `Form::event_handler_names()` → vec of names (для UnusedParameters)

**Оценка:** ~300 строк кода, ~100 строк тестов

### Фаза 2: Extraction — ModuleCallSummary (medium risk)

**Файлы:**
- `crates/hir-def/src/call_graph.rs` — ADD: `extract_call_summary(item_tree, module_bodies, module_metadata) → ModuleCallSummary`
- `crates/hir-def/src/queries.rs` — ADD: `module_call_summary_query` (Salsa tracked, LRU=256)
- `crates/ide-db/src/provider.rs` — ADD: `call_summary()` method on `AnalysisProvider`
- `crates/ide-db/src/streaming/global_context.rs` — ADD: `call_summaries: DashMap<FileId, Arc<ModuleCallSummary>>`

**Задачи:**
1. Implement `extract_call_summary`:
   - Iterate `module_bodies.iter_bodies()` + `module_code()`
   - Для каждого метода: собрать `MethodSummary` (dispatch из annotations в item_tree)
   - Для каждого `Expr::Call` с `Expr::Path`: `DirectLocal` edge
   - Для каждого `Expr::Call` с `Expr::QualifiedPath`: `DirectQualifiedModule` edge
   - Для каждого `ExternalRef` в `lower_result.external_refs`: дополнительные cross-module edges
   - Для `Новый ОписаниеОповещения(...)`: `NotifyDescriptionCallback` / `NotifyReg`
   - Для `ПодключитьОбработчикОжидания(...)`: `IdleHandlerAttach` / `IdleReg`
2. Salsa query + AnalysisProvider integration
3. Streaming: publish summary в DashMap после Phase 2 per-file
4. Тесты: extraction для модулей с разными типами вызовов

**Оценка:** ~500 строк кода, ~200 строк тестов

### Фаза 3: ServerCallsInFormEvents → BFS (medium risk)

**Файлы:**
- `crates/hir-def/src/call_graph.rs` — ADD: `find_server_calls_from_entries(summary, provider) → Vec<ServerCallPath>`
- `crates/ide-diagnostics/src/handlers/server_calls_in_form_events.rs` — REWRITE: `from_hir` → `check` function using call graph BFS
- `crates/hir-def/src/body/lower/expr.rs` — REMOVE: emission of `BodyDiagnostic::ServerCallsInFormEvents` (suffix-based)
- `crates/hir-def/src/body/lower/diagnostics.rs` — REMOVE: `FORBIDDEN_EVENT_SUFFIXES`, `is_forbidden_form_event`

**Задачи:**
1. BFS traversal: от `FormEventEntry` по `DirectLocal` + `DirectQualifiedModule` edges
2. При достижении метода с `ServerOnly`/`ServerNoContext` → диагностика с path
3. Новый `check()` вместо `from_hir()` — диагностика больше не из BodyDiagnostic
4. Удалить suffix-based код из lowering
5. Тесты:
   - Прямой серверный вызов в обработчике → ERROR
   - Вызов через промежуточный клиентский метод → ERROR
   - Idle handler с серверным вызовом → НЕТ (async, не следуем)
   - Обработчик события OnChange (не OnActivateRow) → НЕТ
   - Не-обработчик с суффиксом ПриАктивизацииСтроки → НЕТ (регрессия fix)

**Оценка:** ~400 строк кода, ~300 строк тестов

### Фаза 4: NotifyDescription cross-module (lower priority)

**Файлы:**
- `crates/ide-diagnostics/src/handlers/unused_parameters.rs` — REFACTOR: заменить intra-module scan на CallGraph-based suppression
- `crates/ide-db/src/streaming/global_context.rs` — ADD: reverse callback index
- `crates/hir-def/src/queries.rs` — ADD: `notify_registrations_query`

**Задачи:**
1. Reverse index: для каждого `NotifyReg` → target_module + callback_name
2. `UnusedParameters`: check if method is callback target in any module's registrations
3. Streaming: reducer pass после всех workers
4. Тесты: cross-module callback suppression

**Оценка:** ~300 строк кода, ~150 строк тестов

---

## Бюджет производительности

| Метрика | Цель | Обоснование |
|---------|------|-------------|
| Summary extraction per file | <2ms типичный, <15ms большой | Flat scan exprs_iter(), минимум аллокаций |
| Phase 2 overhead (batch) | ≤15% wall clock | Summary extraction параллельна с diagnostics |
| CallGraph memory (12K files) | <80MB | Flat structs, pre-folded names, no AST retention |
| LSP incremental (edit 1 file) | <20ms summary, <150ms diagnostics p95 | Per-file Salsa invalidation, demand BFS |

## Salsa Query Dependencies

```
parse(file) → item_tree(file) → module_bodies(file) → module_call_summary(file)
                                                              │
form_metadata(file) ─────────────────────→ form_event_entries(file)
                                                              │
module_index(root) + symbol_tree(target) → resolved_edges(file)
                                                              │
                                          server_reachability(file, local_id)
```

Важно: `module_call_summary(A)` НЕ зависит от тел методов других модулей. Изменение тела в модуле B не инвалидирует summary модуля A.

## Миграция

- Фаза 1-2: без изменений поведения диагностик, только добавление инфраструктуры
- Фаза 3: ServerCallsInFormEvents переключается на CallGraph, suffix-based код удаляется
- Фаза 4: UnusedParameters переключается на reverse index, intra-module scan остаётся как fallback

## Риски

| Риск | Вероятность | Митигация |
|------|-------------|-----------|
| Overhead >15% в batch | Низкая | Extraction O(n) по expressions, lazy в Salsa |
| Salsa invalidation cascade | Средняя | Per-file queries, summary не зависит от target bodies |
| Streaming ordering | Низкая | Summary публикуется per-file, BFS demand-driven |
| Form.XML без event type | Нулевая | Уже парсим XML, добавить атрибут тривиально |
