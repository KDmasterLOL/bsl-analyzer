# Diagnostics Quality & Performance Roadmap

Верхнеуровневый план работ по улучшению качества и производительности 185
диагностик. Построен на основе аудита `docs/diagnostics-audit/`. Каждое
поднаправление привязано к слою-владельцу из слойной архитектуры
(`CLAUDE.md` → раздел «Архитектура»).

Дата: 2026-05-07.
Статус: согласован (cross-check Codex), не детализирован.

---

## Track 1 — Семантический фундамент

| Поднаправление | Слой | Карточки |
|---|---|---|
| CFG: `break`/`continue`/`goto`/loop-context, общее `path terminates` | **`cfg`** (граф) + **`dataflow`** (path-terminates property) | `AllFunctionPathMustHaveReturn`, `UnreachableCode`, `PairingBrokenTransaction`, `MissingTempStorageDeletion`, `MissingTemporaryFileDeletion`, `CommitTransactionOutsideTryCatch`, `WrongUseOfRollbackTransactionMethod` |
| Type inference: param/return lowering, narrowing, `ManagerModule`/`FormModule` `ЭтотОбъект`, `;`-union'ы в `platform_data` | **`hir-ty`** (infer + ty lowering); **`bsl-platform`** для парсинга `platform_data` separators | `TypeMismatch`, `UnresolvedField`, `UnresolvedMethodCall`, `RedundantAccessToObject`, `MissedRequiredParameter`, `MismatchedArgCount`, `ReadOnlyPropertyAssignment`, `DeprecatedMethodCall` |
| `visible_configurations()` вместо `load_configuration()` (CFE / multi-config) | **`bsl-metadata`** (API) + **`ide-db/salsa_provider`** (query); `ide-diagnostics` — тонкая проекция | `CommonModuleAssign` (явно), остальной `CommonModule*` кластер — пересверить per-card перед имплементацией |
| Resolver/shadowing унификация (locals/params vs metadata) | **`hir-def/resolver`** (name-only resolution; typed dispatch — отдельно в `hir-ty`) | `CommonModuleAssign`, `ThisObjectAssign`, `SelfAssign`, `RewriteMethodParameter` |

## Track 2 — Семейственные переделки

- **Security hotspot infrastructure** (с разделёнными подконтекстами):
  - registry security API → **`bsl-platform`** / **`bsl-metadata`**
  - arg/context analysis → **`hir-ty`** + **`dataflow`**
  - privilege lifetime → **`dataflow`**
  - call-graph effects / trust boundaries → **`dataflow`** (поверх `hir`)
  - правила: `FileSystemAccess`, `InternetAccess`, `ExternalAppStarting`, `OSUsersMethod`, `ExecuteExternalCode*`, `SetPrivilegedMode`, `DisableSafeMode`, `PrivilegedModuleMethodCall`, `UnsafeSafeModeMethodCall`
- **Transactions dataflow** → **`dataflow`** (тонкий handler в `ide-diagnostics`):
  `PairingBrokenTransaction`, `BeginTransactionBeforeTryCatch`,
  `CommitTransactionOutsideTryCatch`, `WrongUseOfRollbackTransactionMethod`,
  `MissingCodeTryCatchEx`
- **Module structure**:
  - lexical surface (`#Region`, module-level элементы) → **`syntax`**
  - semantic element classification (export/private, region kind, module-level position) → **`hir-def`**
  - правила: `CodeOutOfRegion`, `NonStandardRegion`, `NonExportMethodsInApiRegion`, `EmptyRegion`, `DuplicateRegion`, `CodeBlockBeforeSub`
- **SDBL** (~15 правил) — общий `SdblPredicateContext` → **`sdbl-hir`**
- **Doc-comments** — единый парсер:
  - lexical surface → **`syntax`**
  - semantic doc-model (parameter linking, return) → **`hir-def`**
  - правила: `Missing*Description`, `PublicMethodsDescription`, `MissingParameterDescription`
- **Complexity metrics** — общий фреймворк, разделённый по природе метрики:
  - HIR-структурные (по lowered body): `CognitiveComplexity`, `IfConditionComplexity`, `NestedStatements`, `MethodSize`, `NumberOfParams`, `NumberOfOptionalParams` → **`hir-def`**
  - Graph-based (по CFG): `CyclomaticComplexity` → **`cfg`**

## Track 3 — Тестовая инфраструктура

**Closed 2026-05-11.** See `TRACK_3_CLOSURE.md` for slice-by-slice
breakdown and acceptance gate evidence.

- helper для diagnostics-with-configuration (фикстуры `Configuration.xml` + `CommonModules/`) → **`ide-diagnostics`** test infra + фикстуры на уровне **`bsl-metadata`**
- BSL/EN bilingual parity matrix → **`ide-diagnostics`** snapshot harness
- CFE / visible-configurations test harness → **`bsl-metadata`** + **`project-model`** test infra
- Тесты CFG-properties (циклы, break/continue/goto) → **`cfg`** сам (свои тесты)
- Тесты препроцессорной модели → слой самой модели (см. Track 6)
- Snapshot-тесты диагностик → **`ide-diagnostics`**

## Track 4 — Пользовательское качество

- quick-fix infrastructure (rename, replace, delete, wrap) → **`ide-assists`**; связь с диагностикой через `ide-diagnostics`
- token/text quick-fixes без resolver/metadata зависимостей → **`ide-assists`** (оппортунистически после Track 3)
- message hints (canonical name, source URI metadata, hover preview) → **`ide-diagnostics`** (формат сообщения); данные приходят из `bsl-metadata`/`hir-def`
- per-family review severity/tags → **`ide-diagnostics`** (`DiagnosticMetadata`)
- консолидация дубликатов:
  - общий предметный факт (например, «функция возвращает ли значение на каждом пути») → анализирующий слой (`cfg`/`dataflow`/`hir-ty` per case)
  - два `DiagnosticCode` поверх общего факта → **`ide-diagnostics`**
  - примеры: `FunctionShouldHaveReturn` + `AllFunctionPathMustHaveReturn`
- RU/EN docs parity → **`crates/ide-diagnostics/docs/{ru,en}/`** (карточки `FileSystemAccess`, `CodeOutOfRegion`, `MissingParameterDescription` флагуют расхождения)

## Track 5 — Производительность

**Параллельно с Track 1:**

- baseline профилирование (`BSL_PROFILE='*'`) → CLI **`bsl-analyzer-app`** instrumentation
- gating candidate emission (skip правил-без-конфигурации, skip отключённых) → **`ide-diagnostics::dispatch`** (lowering трогать нельзя — правило `db`)
- regex cleanup (замена на HIR/AST API) → **`ide-diagnostics`** handlers per-handler

**После Track 2:**

- расширение кэширования (`RegionTree`, `DocCommentModel`, `SdblPredicateContext`) → **`ide-db`** (Salsa queries)
- Salsa LRU policy review → **`ide-db`**

## Track 6 — Парсер, препроцессор, cascade suppression

- **6.1 Parser UX:** structured expected-token errors, ranges → **`parser`** + **`syntax`** — `ParseError`, `QueryParseError`
- **6.2 Misplaced loop control:** `Прервать` / `Продолжить` вне цикла. Сейчас ни parser, ни lowering, ни ide-diagnostics не флагают этот случай — Track 1 Step C сознательно делегирует диагностику этому слою и не порождает фиктивные CFG-рёбра. Слой: **`parser`** (синтаксический контекст) или **`hir-def`** (если решение опираться на enclosing-loop в lowering)
- **6.3 Preprocessor source-of-truth — CLOSED 2026-05-12, см. [TRACK_6_3_CLOSURE.md](TRACK_6_3_CLOSURE.md):**
  - surface (символы и ветки) → **`parser`** / **`syntax`** — typed AST wrappers (`PreSymbol`, `PreExpr`, `PreIfDir`, `PreElsIfClause`, `PreElseClause`) на месте.
  - per-branch анализ → **`hir-def`** через shared `PreprocIfStmt::branches()` iterator (`crates/hir-def/src/hir.rs`); CFG builder и DuplicatedInsertion handler refactored на этот iterator.
  - 3 priority cards закрыты: `AllFunctionPathMustHaveReturn` (CFG/dataflow validation), `DuplicatedInsertionIntoCollection` (intra-branch fixtures), `BeginTransactionBeforeTryCatch` (preproc-aware Begin/Try pattern + regtest re-enable).
  - **Active-symbol pruning evaluated and rejected**: BSL модули типа multi-context CommonModule компилируются дважды, обе ветки реально исполняются; in-process server invocation (толстый клиент в файловом режиме, мобильное приложение, внешнее соединение) делает active-symbol matrix многомерной. bsl-language-server reference impl за 5+ лет production не реализовал pruning — мы тоже не реализуем. Per-branch analysis discipline — правильная семантика.
- **6.4 Cascade suppression — CLOSED 2026-05-13:**
  - **TypeMismatch ↔ Unresolved* dedup**: уже реализован через gradual-typing gate в `crates/hir-ty/src/subtype.rs::is_assignable` (если **любая** сторона = `Ty::Unknown`, возвращает `true` → silence). Когда имя не резолвится, inference возвращает `Ty::Unknown`, и TypeMismatch автоматически не эмитится. Существующее покрытие в `crates/ide/tests/infer_type_mismatch.rs` уже включает 11+ тестов на cascade prevention, см. комментарии *"Without the Unknown ≤ A gradual rule, a single unresolved call would cascade mismatches through every downstream"*. Никакого нового кода не понадобилось.
  - **BadWords duplicate emissions**: исправлено коммитом `9ce81a45`. Root cause: `check_node()` сканировал `node.text()` каждого descendant'а через single-pass dispatcher, и текст ребёнка попадал в text родителя — то же слово эмитилось per-ancestor (≥3 дубликата на одно вхождение). Fix: `node.children_with_tokens()` filtered to tokens, каждый token принадлежит одному узлу. Regression test `integration_bad_words_no_duplicates_per_occurrence` ходит через production single-pass path.

---

## Порядок выполнения

1. **Track 1** (фундамент) + **Track 3** (тестовый харнесс параллельно) + **Track 5 hygiene-часть** (baseline / gating / regex cleanup параллельно)
2. **Track 2** (после Track 1, опирается на новый CFG/inference); **Track 6** параллельно с Track 2 (независимы по слою)
3. **Track 5 caching-часть** (после Track 2 — модели кэшируем после стабилизации)
4. **Track 4** последним; token/text quick-fixes допустимы оппортунистически после Track 3

---

## Согласование

План прошёл cross-check Codex. Из 7 предложенных правок:

- **Принято полностью (4):** complexity metrics критерий разделения, gating только в `ide-diagnostics::dispatch`, консолидация дубликатов через анализирующий слой + два кода, тесты CFG-properties в `cfg`.
- **Принято с уточнением (2):** препроцессорная активность веток — отдельная модель в `hir-def` вне `body/lower` (не `cfg`/`dataflow`); module structure — без лишнего type/config-based слоя.
- **Отклонено (1):** перенос resolver/shadowing унификации из `hir-def/resolver` в `hir-ty`. Resolver — name-only resolution; typed dispatch отдельно в `hir-ty`. Слой `hir-def/resolver` корректен.

---

## Дальнейшая детализация

Каждый трек далее раскрывается в отдельный документ
`docs/diagnostics-audit/roadmap/<track>.md` с:

- последовательностью правок
- списком затронутых карточек (точечно)
- тестовой матрицей и acceptance gate
- оценкой объёма

По правилу pair-mode для нетривиальной работы детальный план каждого трека
проходит критику Codex перед имплементацией.
