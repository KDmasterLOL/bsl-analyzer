# Streaming / CLI mode

Streaming-режим (`crates/ide/src/streaming/`,
`crates/ide-db/src/streaming/`) — самостоятельный пайплайн для
batch-анализа (CLI и SonarQube-интеграция). Архитектурно отделён
от LSP-интерактива: общий `AnalysisProvider` traits, разные движки
и стратегии кэширования.

## Текущее состояние

- `AnalysisOrchestrator` — координатор фаз (init → workers → sink).
- `GlobalContext` — shared workspace-данные (configuration,
  symbol_trees per file, module_index).
- `StreamingProvider` — реализация `AnalysisProvider` без Salsa,
  свой per-file кэш через `SharedState`/`ParsedFile` с lazy-`OnceLock`.
- Пиковая память на крупной ERP-конфигурации: ~2.8 GB
  (configuration + symbol_trees + module_index + per-worker
  short-lived структуры). Bounded — не растёт с прогрессом обхода,
  потому что worker'ы отпускают per-file данные сразу после
  обработки.
- JSONL-вывод для SonarQube-интеграции.

## Слепые зоны

Streaming-режим **намеренно жертвует** полнотой ради bounded
memory и batch-производительности. Это осознанный компромисс,
оформленный явно в `StreamingProvider`. Из 186 handlers диагностик
~9 (≈5%) полностью или частично не работают.

### Полностью отключены

**Type-dependent (`infer` возвращает `InferenceResult::default()`):**

- `TypeMismatch`
- `UnresolvedMethodCall`
- `UnresolvedField`
- `ReadOnlyProperty`
- `MismatchedArgCount`
- `MissingCommonModuleMethod`
- `TryNumber`

**Cross-module (`file_external_refs` пустой `Vec`):**

- `PrivilegedModuleMethodCall`

**Module-level (`module_level_liveness_analysis` возвращает `None`):**

- `unused_local_variable` для module-level переменных
  (внутри-методные переменные считаются корректно).

### Деградируют без полного отключения

- `CognitiveComplexity` — `method_effect_summary` возвращает
  `EffectSummary::EMPTY` через trait default. Бонус за рекурсию не
  учитывается, метрика занижена на рекурсивных функциях. Базовая
  часть считается корректно, диагностика триггерит как обычно.

### Что работает идентично LSP-режиму

~95% диагностик:

- Все стилистические правила.
- Region/structure-диагностики.
- CFG/dataflow внутри метода (`unreachable_code`,
  `misplaced_loop_control`, `all_function_path_must_have_return`).
- Локальные `unused_local_variable` / `unused_parameters` /
  `unused_local_method`.
- SDBL-диагностики.
- Метрики (`cyclomatic_complexity`, `method_size`).

## Развитие streaming-режима

### Документировать слепые зоны явно

Перед расширением функциональности — добавить публичный документ
«Что доступно в streaming-режиме» (в `crates/ide/src/streaming/mod.rs`
или `docs/contributing/`). Цель: пользователь не должен удивляться,
что правило срабатывает в IDE и не срабатывает в Sonar-отчёте.
Кандидат на `[degraded]`/`[disabled]` маркер в JSONL-выходе для
diagnostic'ов, которые в streaming работают неполноценно.

### Восстановление дешёвых диагностик

Часть слепых зон можно закрыть без полного включения типизации
в streaming:

- **`PrivilegedModuleMethodCall`** — `file_external_refs` собираемые
  eagerly в Phase 1 (наряду с `symbol_trees`). Стоимость — ещё один
  параллельный pass, оценочно +5-10% к initialization-времени.
- **`module_level_liveness_analysis`** — переиспользовать готовый
  `dataflow::liveness::ModuleLiveness` API на module-level CFG.
  Технически тот же путь, что и для методов, просто другой entry.

Обе доработки имеют смысл, только если эти диагностики реально
нужны в Sonar-отчётах команд.

### Опциональный type-mode (долгосрочно)

Streaming можно научить опциональному прогону `infer`. Это `O(N)`
extra работа на старте, +память на arena типов, но открывает все
7 type-dependent диагностик. Применимо только когда:

1. Type-inference дозреет до уровня, где false-positive
   приемлемы для CI (см. type-inference.md Tier 1).
2. Появится конкретный запрос от пользователей SonarQube-интеграции
   на типовые проверки в отчёте.

До тех пор — оставлять `infer` пустым: лучше silent skip, чем шум
false-positive в CI-пайплайне.

### Что НЕ менять

- **Не сливать streaming-движок с LSP-движком.** Это два разных
  use case с разными гарантиями кэширования и памяти. Общим должен
  оставаться только `AnalysisProvider` traits.
- **Не вводить cross-file mutable state в streaming-провайдер.**
  Worker'ы должны оставаться шардируемыми по файлам с
  изолированными per-file локальными кэшами.
- **Не «улучшать» eagerness Phase 1.** Текущий объём
  (configuration + symbol_trees + module_index) обоснован
  cross-module диагностиками. Расширение eager-перечня требует
  явного обоснования по диагностикам.
