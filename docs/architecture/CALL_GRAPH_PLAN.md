# CallGraph: План реализации (v2)

> Ревизия после tri-model review (Opus Architect + Codex + Gemini, 2026-04-04).
> Исходная версия переработана с учётом архитектурных, типовых и domain-специфичных замечаний.

## Мотивация

Две задачи требуют инфраструктуры call graph:

### 1. ServerCallsInFormEvents — точная диагностика
**Текущее состояние:** Проверяет суффикс имени метода (`ПриАктивизацииСтроки`, `НачалоВыбора`).
- Ложные срабатывания: idle handler `ПослеОжиданияТаблицаПриАктивизацииСтроки` совпадает по суффиксу
- Пропуски: обработчики с нестандартными именами не обнаруживаются
- Не трассирует вызовы через промежуточные клиентские методы

**Целевое поведение:** Точные обработчики событий из Form.XML → BFS по call chain → обнаружение серверных вызовов на любой глубине.

### 2. NotifyDescription — кросс-модульное подавление (Phase 4, deferred)
**Текущее состояние:** `UnusedParameters` подавляет только intra-module callbacks (`ЭтотОбъект`/`ThisObject`).
**Целевое поведение:** Проектный reverse index всех NotifyDescription регистраций для кросс-модульного подавления.

---

## Существующая инфраструктура

| Компонент | Что есть | Файл |
|-----------|----------|------|
| `ExternalRef` | Трекает cross-module вызовы (QualifiedCall, ManagerAccess) | `crates/hir-def/src/body.rs:938-951` |
| `LowerResult` | Per-method: body + source_map + diagnostics + external_refs | `crates/hir-def/src/body.rs:315-328` |
| `ModuleBodies` | Per-file: bodies indexed by local_id, module_code, iter_lower_results() | `crates/hir-def/src/lib.rs:601-767` |
| `Name` | Case-insensitive name with SmolStr backing | `crates/hir-def/src/name.rs:13` |
| `AnnotationKind` | AtClient, AtServer, AtClientAtServer, AtClientAtServerNoContext, AtServerNoContext, Before, After, Instead, ChangeAndValidate | `crates/hir-def/src/item_tree.rs:206-225` |
| `SymbolTree` | O(1) case-insensitive method lookup, annotations, export | `crates/hir-def/src/symbol_tree.rs:35-49` |
| `ModuleIndex` | Resolve module name → FileId, поддержка resolve(&ExternalRef) | `crates/hir-def/src/module_index.rs:24-31` |
| `WorkspaceSymbols` | FxHashMap<Name, CommonModuleInfo> для общих модулей | `crates/hir-def/src/workspace.rs:44-49` |
| `Form.XML parser` | Рекурсивно собирает handler names из `<Events>` и `<Commands>` | `crates/bsl-metadata/src/xml_parser/form.rs:136-155` |
| `SharedState` | DashMap-based concurrent state: symbol_trees, parsed_files | `crates/ide-db/src/streaming/shared_state.rs:327-632` |
| `deprecated_method_call` | Паттерн cross-module resolve: ModuleIndex → SymbolTree → method | `crates/ide-diagnostics/src/handlers/deprecated_method_call.rs:148-181` |
| `unused_local_method` | Ad-hoc intra-module сбор вызовов Expr::Call/MethodCall | `crates/ide-diagnostics/src/handlers/unused_local_method.rs:141-158` |

**Чего нет:** CallGraph, семантическая обработка `ПодключитьОбработчикОжидания`, event type в Form.XML parser.

---

## Архитектура

### Распределение по слоям (Clean Architecture)

```
┌─────────────────────────────────────────────────────────────┐
│  Frameworks/Drivers: bsl-analyzer (LSP server)              │
├─────────────────────────────────────────────────────────────┤
│  Interface Adapters: ide-db                                 │
│    - call_summary() method on AnalysisProvider              │
│    - Salsa query wiring: module_call_summary_query          │
│    - Streaming: SharedState.call_summaries DashMap           │
├─────────────────────────────────────────────────────────────┤
│  Use Cases: ide-diagnostics                                 │
│    - BFS traversal (server_calls_in_form_events::check)     │
│    - Reverse callback index lookup (unused_parameters)      │
│    - Diagnostic emission                                    │
├─────────────────────────────────────────────────────────────┤
│  Entities: hir-def                                          │
│    - ModuleCallFacts (types + extraction)                   │
│    - No cross-module resolution, no BFS                     │
│    - Pure per-module facts from code                        │
└─────────────────────────────────────────────────────────────┘
```

**Dependency Rule:** зависимости только внутрь. `ide-diagnostics` → `ide-db` → `hir-def`. BFS живёт в `ide-diagnostics`, не в `hir-def`.

### Обзорная диаграмма

```
Form XML Metadata (exact event bindings: event_type → handler_name)
          │
          ▼
HIR lowering / ModuleBodies(file)
          │
          ▼
module_call_summary(file)  ← per-module, Salsa LRU=256
  ├─ methods: Vec<MethodSummary>       (local_id, name, dispatch)
  ├─ call_edges: Vec<CallEdge>         (caller → callee, kind, range)
  ├─ notify_regs: Vec<NotifyReg>       (callback_name, target_module)
  ├─ idle_handler_regs: Vec<IdleReg>   (handler_name, one_shot)
  └─ form_entries: Vec<FormEventEntry> (event_type, handler_local_id)
          │
    ┌─────┴──────┐
    ▼            ▼
Salsa/LSP    Streaming/CI
demand BFS   publish to SharedState
(ide-diag)   batch reverse idx
```

### Типы рёбер (EdgeKind)

```rust
/// Синхронные call edges — BFS следует по ним.
/// Асинхронные регистрации (NotifyDescription, idle handlers) хранятся
/// отдельно в notify_regs/idle_handler_regs, а не как edges.
enum EdgeKind {
    /// Прямой вызов локального метода: Метод()
    DirectLocal,
    /// Qualified вызов: ОбщийМодуль.Метод()
    DirectQualifiedModule,
}
```

**Решение:** `NotifyDescriptionCallback`, `IdleHandlerAttach`, `FormEventEntry` убраны из EdgeKind. Async-регистрации — отдельные коллекции. Form entries — отдельный список. Нет дублирования.

### Целевой узел (CallTarget)

```rust
/// Используем Name (не String) для case-insensitive семантики BSL.
/// resolved_file НЕ хранится — resolve ленивый, при BFS через AnalysisProvider.
enum CallTarget {
    /// Локальный метод текущего модуля
    Local { callee_local_id: u32 },
    /// Qualified cross-module: ОбщийМодуль.Метод
    QualifiedModule { module_name: Name, method_name: Name },
    /// Manager access: Документы.ИмяОбъекта.Метод()
    ManagerAccess { manager_type: ManagerType, object_name: Name, method_name: Option<Name> },
    /// Только имя метода (для idle handler с ЭтотОбъект)
    ThisObjectMethod { method_name: Name },
    /// Не удалось резолвить
    Unresolved,
}
```

**Изменения vs v1:** `Name` вместо `String`, убран `resolved_file`, добавлен `ManagerAccess`, `MethodNameOnly` → `ThisObjectMethod` (точнее семантика), `Unknown` → `Unresolved`.

### Идентификация caller

```rust
/// Кто вызывает — метод или код модуля.
/// Не используем sentinel u32::MAX.
enum CallerId {
    Method(u32),   // local_id метода
    ModuleCode,    // код модуля вне методов
}
```

### Dispatch классификация методов

```rust
/// Вычисляется из AnnotationKind, а не дублирует его.
/// AnnotationKind содержит также Before/After/Instead/ChangeAndValidate,
/// которые не нужны для call graph dispatch.
struct MethodDispatch {
    can_run_on_client: bool,
    can_run_on_server: bool,
    no_context: bool,
}

impl MethodDispatch {
    fn from_annotation(kind: Option<&AnnotationKind>) -> Self { ... }

    fn is_server_only(&self) -> bool {
        self.can_run_on_server && !self.can_run_on_client
    }
}
```

**Маппинг:**
| AnnotationKind | can_client | can_server | no_context |
|---|---|---|---|
| AtClient | true | false | false |
| AtServer | false | true | false |
| AtServerNoContext | false | true | true |
| AtClientAtServer | true | true | false |
| AtClientAtServerNoContext | true | true | true |
| None (нет аннотации) | true | false | false |

### BFS для ServerCallsInFormEvents

- **Старт:** точные form event handlers из Form.XML (OnActivateRow, OnStartChoice)
- **Следуем:** только `DirectLocal` и `DirectQualifiedModule` (синхронные call_edges)
- **НЕ следуем:** notify_regs, idle_handler_regs (асинхронные, другой lifecycle)
- **Стоп:** метод с `is_server_only() == true` → диагностика
- **Continue:** `can_run_on_client == true` — метод может выполняться на клиенте, проверяем транзитивно
- **Caps:** max depth 64, max visited 10000, tracing::warn при срабатывании
- **Visited:** `FxHashSet<(FileId, u32)>` — пара (файл, local_id), не просто local_id
- **Результат:** shortest path для сообщения диагностики
- **Где живёт:** `ide-diagnostics/src/handlers/server_calls_in_form_events.rs` (stateless функция, без Salsa caching)
- **Resolve:** ленивый через `DiagnosticsContext` → `module_index()` + `symbol_tree_for()`

---

## Фазы реализации

### Фаза 1: Фундамент — типы и metadata (low risk)

**Pair mode:** Codex пишет, Claude ревьюит.
- Codex: 80% работы — точные Rust типы и pattern matching (его сильная сторона)
- Claude reviewer проверяет: `Name` vs `String` везде, `MethodDispatch::from_annotation` маппинг (BSL domain), обратная совместимость `Form::event_handler_names()`, корректные derive для `Arc<ModuleCallSummary>` (Clone + Eq + PartialEq)

**Файлы:**
- `crates/hir-def/src/call_graph.rs` — NEW: `ModuleCallSummary`, `CallEdge`, `EdgeKind`, `CallTarget`, `CallerId`, `MethodSummary`, `MethodDispatch`, `NotifyReg`, `IdleReg`, `FormEventEntry`
- `crates/bsl-metadata/src/xml_parser/form.rs` — FIX: `collect_events_recursive` собирает `(event_type, handler_name)` вместо только `handler_name`
- `crates/bsl-metadata/src/form.rs` — FIX: `FormEventHandler { event_type: String, handler_name: String }` вместо `String`

**Задачи:**
1. Определить типы данных CallGraph в `hir-def` (используя `Name`, `CallerId`, флаги dispatch)
2. Расширить Form.XML parser: собирать event type (атрибут `name` из `<Event name="OnActivateRow">`)
3. Обновить `Form` struct: `event_handlers: Vec<FormEventHandler>`
4. Тесты: парсинг Form.XML с event types
5. Обратная совместимость: `Form::event_handler_names()` → vec of names (для UnusedParameters)

**Оценка:** ~300 строк кода, ~100 строк тестов

### Фаза 2: Extraction — ModuleCallSummary (medium risk)

**Pair mode:** Claude пишет, Codex ревьюит.
- Claude: Salsa query wiring + AnalysisProvider + SharedState — архитектурная задача (его сильная сторона)
- Codex reviewer проверяет: lifetime/generic корректность Salsa query, exhaustiveness pattern matching в extraction, отсутствие duplicate edges, тест-матрица (все 7 кейсов), `CallerId::ModuleCode` не забыт

**Файлы:**
- `crates/hir-def/src/call_graph.rs` — ADD: `extract_call_summary(item_tree, module_bodies, form_event_handlers) → ModuleCallSummary`
- `crates/hir-def/src/queries.rs` — ADD: `module_call_summary_query` (Salsa tracked, LRU=256)
- `crates/ide-db/src/provider.rs` — ADD: `call_summary()` method on `AnalysisProvider`
- `crates/ide-db/src/streaming/shared_state.rs` — ADD: `call_summaries: DashMap<FileId, Arc<ModuleCallSummary>>`

**Задачи:**
1. Implement `extract_call_summary`:
   - Iterate `module_bodies.iter_lower_results()` (нужен LowerResult для source_map + external_refs)
   - Для каждого метода: собрать `MethodSummary` (dispatch из annotations в item_tree)
   - Для каждого `Expr::Call` с `Expr::Path`: `DirectLocal` edge (с фильтрацией: проверять что path — это реально локальный метод)
   - Для каждого `Expr::Call` с `Expr::QualifiedPath`: `DirectQualifiedModule` edge
   - Canonical source для qualified edges: HIR expressions (`Expr::Call` + `Expr::QualifiedPath`), НЕ `external_refs` (lowerer дублирует обе формы)
   - Для `Новый ОписаниеОповещения(...)`: `NotifyReg` (в notify_regs, не в edges)
   - Для `ПодключитьОбработчикОжидания(...)`: `IdleReg` (в idle_handler_regs, не в edges)
   - Module-level code через `CallerId::ModuleCode`
2. Salsa query + AnalysisProvider integration
3. Streaming: publish summary в SharedState после Phase 2 per-file
4. Тесты: extraction для модулей с разными типами вызовов

**Тест-матрица (зафиксирована до кода):**
- handler → local client method → server method (цепочка)
- handler → exported common module method (qualified call)
- qualified call в неэкспортный/несуществующий метод — не traversed
- один qualified call не дает duplicate edge
- `NotifyDescription` в module-level code попадает в notify_regs с `CallerId::ModuleCode`
- manager access `Документы.Имя.Метод()` → `CallTarget::ManagerAccess`
- `ПодключитьОбработчикОжидания` → `IdleReg`, не edge

**Оценка:** ~500 строк кода, ~200 строк тестов

### Фаза 3: ServerCallsInFormEvents → BFS (medium risk)

**Pair mode:** Codex пишет, Claude ревьюит.
- Codex: BFS алгоритм + копирование паттерна cross-module resolve из `deprecated_method_call.rs` (его сильная сторона)
- Claude reviewer проверяет: BSL семантика stop-условий (`ClientServer` = continue), отсутствие Salsa dependency chain в BFS, полное удаление suffix-based кода из lowering + hir_dispatch, regression test для idle handler с суффиксом

**Файлы:**
- `crates/ide-diagnostics/src/handlers/server_calls_in_form_events.rs` — REWRITE: `from_hir` → `check()` с BFS через DiagnosticsContext
- `crates/hir-def/src/body/lower/expr.rs` — REMOVE: emission of `BodyDiagnostic::ServerCallsInFormEvents` (suffix-based)
- `crates/hir-def/src/body/lower/diagnostics.rs` — REMOVE: `FORBIDDEN_EVENT_SUFFIXES`, `is_forbidden_form_event`

**Задачи:**
1. BFS traversal в `server_calls_in_form_events.rs`: от `FormEventEntry` по `DirectLocal` + `DirectQualifiedModule` edges
   - Cross-module resolve: `ctx.module_index()` + `ctx.symbol_tree_for()` (паттерн из deprecated_method_call)
   - Visited: `FxHashSet<(FileId, u32)>` — cross-module safe
   - Ленивый resolve: не Salsa query, stateless функция
2. При достижении метода с `is_server_only()` → диагностика с path
3. Новый `check()` вместо `from_hir()` — диагностика больше не из BodyDiagnostic
4. Удалить suffix-based код из lowering
5. Тесты:
   - Прямой серверный вызов в обработчике → ERROR
   - Вызов через промежуточный клиентский метод → ERROR
   - Idle handler с серверным вызовом → НЕТ (async, не в edges)
   - Обработчик события OnChange (не OnActivateRow) → НЕТ
   - Не-обработчик с суффиксом ПриАктивизацииСтроки → НЕТ (regression fix)
   - Один handler привязан к нескольким form events, BFS стартует только от target events
   - `ClientServer` метод в цепочке → continue, не stop

**Оценка:** ~400 строк кода, ~300 строк тестов

### Фаза 3.5: DRY-рефакторинг consumers (low risk)

**Pair mode:** Claude пишет, Codex ревьюит.
- Claude: архитектурный DRY рефакторинг, понимание семантических расхождений ad-hoc кода vs ModuleCallSummary
- Codex reviewer проверяет: точное совпадение семантики (ad-hoc считает и `Expr::Call` и `Expr::MethodCall`), case sensitivity (`Name` vs `to_lowercase()`), все существующие тесты зелёные

**Файлы:**
- `crates/ide-diagnostics/src/handlers/unused_local_method.rs` — REFACTOR: заменить ad-hoc `collect_method_calls` на чтение из `ModuleCallSummary.call_edges`
- `crates/ide-diagnostics/src/handlers/unused_parameters.rs` — REFACTOR: заменить ad-hoc `collect_notify_description_callbacks` на чтение из `ModuleCallSummary.notify_regs`

**Задачи:**
1. `unused_local_method`: `called_methods = summary.call_edges.iter().filter(|e| e.kind == DirectLocal).map(|e| e.target_name())`
2. `unused_parameters`: `notify_callbacks = summary.notify_regs.iter().map(|r| &r.callback_name)`
3. Тесты: все существующие тесты должны продолжать проходить (regression)

**Оценка:** ~100 строк изменений

### Сводка pair mode ротации

| Фаза | Implementor | Reviewer | Ключевой навык implementor |
|------|-------------|----------|---------------------------|
| 1 — типы + XML | **Codex** | **Claude** | Точные Rust типы, pattern matching |
| 2 — Salsa extraction | **Claude** | **Codex** | Salsa wiring, HIR navigation |
| 3 — BFS диагностика | **Codex** | **Claude** | BFS алгоритм, паттерн reuse |
| 3.5 — DRY рефакторинг | **Claude** | **Codex** | Архитектурный DRY, семантика замен |

Чередование: Codex → Claude → Codex → Claude.

### Фаза 4: NotifyDescription cross-module (deferred)

> Отложена по приоритету. Фазы 1-3 дают основной прирост качества.
> Техническая реализуемость подтверждена (SharedState DashMap pattern proven).

**Файлы:**
- `crates/ide-diagnostics/src/handlers/unused_parameters.rs` — REFACTOR: CallGraph-based suppression
- `crates/ide-db/src/streaming/shared_state.rs` — ADD: reverse callback index DashMap
- `crates/hir-def/src/queries.rs` — ADD: `notify_registrations_query`

**Задачи:**
1. Reverse index: для каждого `NotifyReg` → target_module + callback_name
2. `UnusedParameters`: check if method is callback target in any module's registrations
3. Streaming: reducer pass после всех workers (аналогично workspace_symbols)
4. Тесты: cross-module callback suppression

---

## Бюджет производительности

| Метрика | Цель | Обоснование |
|---------|------|-------------|
| Summary extraction per file | <2ms типичный, <15ms большой | Flat scan iter_lower_results(), минимум аллокаций |
| Phase 2 overhead (batch) | ≤15% wall clock | Summary extraction параллельна с diagnostics |
| CallGraph memory (12K files) | <80MB | Flat structs, Name (SmolStr), no AST retention |
| BFS per FormModule | <5ms | Обычно <10 edges глубины, ленивый resolve |
| LSP incremental (edit 1 file) | <20ms summary | Per-file Salsa invalidation, no cross-module cascade |

## Salsa Query Dependencies

```
parse(file) → item_tree(file) → module_bodies(file) → module_call_summary(file)
                                                              │
form_metadata(file) ──────────────────────────────────────────┘
```

**Ключевое свойство:** `module_call_summary(A)` зависит ТОЛЬКО от `module_bodies(A)` + `form_metadata(A)`. Изменение тела в модуле B НЕ инвалидирует summary модуля A.

**Отсутствует `resolved_edges` query:** cross-module resolve выполняется лениво в BFS через `DiagnosticsContext`, а не через Salsa. Это устраняет каскадную инвалидацию через `symbol_tree(target)` при изменении CommonModule.

## Known Limitations

### Динамический код (фундаментальное ограничение статического анализа)
- `Выполнить()` / `Execute()` — строковый код, невидим для call graph
- `Вычислить()` / `Eval()` — строковое выражение, невидимо
- Динамическая генерация имён методов через конкатенацию строк

### Запланированные расширения
- `УстановитьДействие()` / `SetAction()` — программное назначение обработчика формы. Можно трекать как дополнительный тип связи. TODO для будущих фаз.

## Миграция

- Фаза 1-2: без изменений поведения диагностик, только добавление инфраструктуры
- Фаза 3: ServerCallsInFormEvents переключается на CallGraph, suffix-based код удаляется
- Фаза 3.5: unused_local_method и unused_parameters переходят на ModuleCallSummary (DRY)
- Фаза 4 (deferred): UnusedParameters переключается на reverse index

## Риски

| Риск | Вероятность | Митигация |
|------|-------------|-----------|
| Overhead >15% в batch | Низкая | Extraction O(n) по expressions, lazy в Salsa |
| Salsa invalidation cascade | Устранён | Нет resolved_edges query, BFS lazy resolve |
| God-modules (200+ методов) | Средняя | Caps + tracing::warn при срабатывании лимитов |
| Form.XML без event type | Нулевая | Уже парсим XML, добавить атрибут тривиально |
| DirectLocal false positives | Низкая | Фильтрация: проверять что Path — реально локальный метод |
