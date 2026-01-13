# Streaming Analyze Architecture

## Проблема

Текущий analyze mode через Salsa потребляет слишком много памяти:
- up_erp (25K файлов, 605K методов): **18.8 GB** базовые структуры, **26.6 GB** полный запуск
- Даже с batch mode (synthetic_write): **4.2 GB**

Причина: Salsa держит все закэшированные данные в памяти.

## Цель

Streaming analyze mode с потреблением **~335 MB** для любого размера проекта.

## Ключевое наблюдение

Анализ измерения памяти (up_erp):

| Компонент | Память | Нужен глобально? |
|-----------|--------|------------------|
| Configuration (metadata) | 31 MB | ✅ Да |
| SymbolTrees (все файлы) | 292 MB | ✅ Да (cross-module) |
| WorkspaceSymbols (индекс) | 5 MB | ✅ Да (qualified names) |
| ModuleIndex | 5 MB | ✅ Да (resolve refs) |
| VFS text | 1.5 GB | ❌ Нет |
| parse + item_tree | 4.5 GB | ❌ Нет |
| module_bodies (HIR) | 10.2 GB | ❌ Нет |
| CFG + Dataflow | 694 MB | ❌ Нет |
| sdbl_hir | 1.7 GB | ❌ Нет |

**Постоянно нужно: ~335 MB** (global context)
**Освобождается после анализа файла: ~17 GB**

## Архитектура

### Dual-mode через AnalysisProvider

```
                    ┌─────────────────────┐
                    │   AnalysisProvider  │  ← trait (абстракция)
                    └──────────┬──────────┘
                               │
           ┌───────────────────┴───────────────────┐
           │                                       │
           ▼                                       ▼
┌─────────────────────┐               ┌─────────────────────┐
│  SalsaProvider      │               │  StreamingProvider  │
│  (для LSP mode)     │               │  (для analyze mode) │
│                     │               │                     │
│  - RootDatabase     │               │  - GlobalContext    │
│  - Full caching     │               │  - Per-file compute │
│  - IDE features     │               │  - Immediate free   │
└─────────────────────┘               └─────────────────────┘
```

**Детали:** [ANALYSIS_PROVIDER_IMPLEMENTATION.md](./ANALYSIS_PROVIDER_IMPLEMENTATION.md)

### GlobalContext (постоянно в памяти)

| Компонент | Размер (ERP) | Назначение |
|-----------|--------------|------------|
| Configuration | 31 MB | Metadata 1C (CommonModules, Registers, etc.) |
| SymbolTrees | 292 MB | Символы всех модулей (cross-module validation) |
| WorkspaceSymbols | 5 MB | Индекс для qualified names (`ОбщегоНазначения.Метод()`) |
| ModuleIndex | 5 MB | Resolve external references |
| FileSet | 2 MB | Path ↔ FileId mapping |
| **Итого** | **~335 MB** | |

### Per-file данные (освобождаются сразу)

| Компонент | Размер | Назначение |
|-----------|--------|------------|
| AST | ~70 KB | Syntax tree |
| HIR Bodies | ~160 KB | Lowered code + diagnostics |
| CFG | ~30 KB | Control flow graph |
| Dataflow | ~50 KB | Reaching defs, liveness |
| **Per worker** | **~310 KB** | |

**Peak (8 workers): ~338 MB**

## Pipeline

```
┌───────────────────────────────────────────────────────────────────┐
│                     INITIALIZATION PHASE (~30 sec)                │
│  1. Load Configuration metadata (~31 MB)                          │
│  2. Scan all files, build FileSet                                 │
│  3. Build SymbolTrees for ALL files (~292 MB)                    │
│  4. Build WorkspaceSymbols index (~5 MB)                         │
│  5. Build ModuleIndex (~5 MB)                                     │
└───────────────────────────────────────────────────────────────────┘
                              │
                              ▼ GlobalContext ready (~335 MB)
┌───────────────────────────────────────────────────────────────────┐
│                      ANALYSIS PHASE (parallel)                    │
│                                                                   │
│  Workers (N CPUs) process files in parallel:                      │
│                                                                   │
│    For each file:                                                 │
│    ├─ Phase 1: Build SymbolTree (no deps) → PUBLISH              │
│    ├─ Phase 2: Parse → HIR → CFG → Dataflow → Diagnostics        │
│    └─ Phase 3: DROP all per-file data, send results              │
│                                                                   │
│  Cyclic dependencies handled via early SymbolTree publish         │
└───────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌───────────────────────────────────────────────────────────────────┐
│                      OUTPUT PHASE                                 │
│  - Aggregate diagnostics                                          │
│  - Generate SARIF/JSON for SonarQube                             │
└───────────────────────────────────────────────────────────────────┘
```

**Детали worker pool:** [STREAMING_WORKER_ARCHITECTURE.md](./STREAMING_WORKER_ARCHITECTURE.md)

## Ожидаемые результаты

| Метрика | Salsa (текущий) | Batch mode | Streaming (цель) |
|---------|-----------------|------------|------------------|
| Память (ERP) | 26.6 GB | 4.2 GB | **~335 MB** |
| Время (ERP) | 10 min | 22 min | ~12-15 min |

**Улучшение памяти: ~80x** (26.6 GB → 335 MB)

## Совместимость

- **LSP mode**: продолжает использовать Salsa через SalsaProvider
- **Analyze mode**: использует StreamingProvider
- **Все диагностики работают**: включая CFG/Dataflow (для SonarQube)
- **Общие алгоритмы**: parser, HIR lowering, diagnostics handlers
- **Нет дублирования кода**: только разные стратегии хранения

## Этапы реализации

| Этап | Описание | Срок |
|------|----------|------|
| 1 | AnalysisProvider trait + SalsaProvider | 1-2 дня |
| 2 | Обновить DiagnosticsContext | 1 день |
| 3 | Мигрировать diagnostic handlers | 2-3 дня |
| 4 | Выделить pure algorithms (без Salsa) | 2 дня |
| 5 | StreamingProvider + GlobalContext | 2-3 дня |
| 6 | Worker pool + Pipeline | 3-4 дня |
| 7 | CLI интеграция + профилирование | 2 дня |

**Всего: ~2 недели**

**Детальный план:** [ANALYSIS_PROVIDER_IMPLEMENTATION.md](./ANALYSIS_PROVIDER_IMPLEMENTATION.md)

## Риски и митигация

| Риск | Митигация |
|------|-----------|
| Cross-module диагностики ломаются | SymbolTrees + WorkspaceSymbols держатся глобально |
| Медленнее из-за re-parse | Параллелизм компенсирует |
| Сложность двух путей | Единый trait AnalysisProvider |
| Циклические зависимости | Early SymbolTree publish (см. worker architecture) |

## Зависимости

```toml
# bsl-analyzer/Cargo.toml
dashmap = "6.1.0"
parking_lot = "0.12.5"
crossbeam-channel = "0.5.15"
```

## Связанные документы

- [ANALYSIS_PROVIDER_IMPLEMENTATION.md](./ANALYSIS_PROVIDER_IMPLEMENTATION.md) — детали AnalysisProvider trait
- [STREAMING_WORKER_ARCHITECTURE.md](./STREAMING_WORKER_ARCHITECTURE.md) — worker pool и синхронизация
