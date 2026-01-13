# Streaming Analyze Architecture

## Проблема

Текущий analyze mode через Salsa потребляет слишком много памяти:
- up_erp (25K файлов, 605K методов): **18.8 GB** базовые структуры, **26.6 GB** полный запуск
- Даже с batch mode (synthetic_write): **4.2 GB**

Причина: Salsa держит все закэшированные данные в памяти.

## Цель

Streaming analyze mode с потреблением **~500 MB - 1 GB** для любого размера проекта.

## Ключевое наблюдение

Анализ измерения памяти (up_erp):
| Компонент | Память | Нужен глобально? |
|-----------|--------|------------------|
| metadata | 31 MB | ✅ Да |
| symbol_tree | 292 MB | ✅ Да (cross-module) |
| VFS text | 1.5 GB | ❌ Нет |
| parse + item_tree | 4.5 GB | ❌ Нет |
| module_bodies (HIR) | 10.2 GB | ❌ Нет |
| CFG | 694 MB | ❌ Нет |
| sdbl_hir | 1.7 GB | ❌ Нет |

**Постоянно нужно: ~320 MB** (metadata + symbol_tree)
**Можно освобождать после анализа файла: ~17 GB**

## Архитектура

### Текущая (Salsa-based)

```
┌─────────────────────────────────────────────────────────┐
│                    RootDatabase (Salsa)                 │
│  ┌─────────────────────────────────────────────────┐    │
│  │ Cached queries (все файлы в памяти)             │    │
│  │  - parse_query (AST)                            │    │
│  │  - item_tree_query                              │    │
│  │  - symbol_tree_query                            │    │
│  │  - module_bodies_query (HIR)                    │    │
│  │  - module_cfgs_query (CFG)                      │    │
│  │  - ...                                          │    │
│  └─────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
              ┌───────────────────────┐
              │ DiagnosticsContext    │
              │   db: &RootDatabase   │
              └───────────────────────┘
                          │
                          ▼
              ┌───────────────────────┐
              │ diagnostics(ctx)      │
              └───────────────────────┘
```

### Предлагаемая (Dual-mode)

```
                    ┌─────────────────────┐
                    │   AnalysisProvider  │  ← trait
                    │   (абстракция)      │
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
│  - Full caching     │               │    - metadata       │
│  - IDE features     │               │    - symbol_trees   │
│                     │               │  - Per-file compute │
│                     │               │  - Immediate free   │
└─────────────────────┘               └─────────────────────┘
```

## AnalysisProvider Trait

```rust
/// Абстракция над источником данных для диагностик.
///
/// Две реализации:
/// - SalsaProvider: использует RootDatabase с полным кэшированием (LSP mode)
/// - StreamingProvider: вычисляет на лету, освобождает после use (analyze mode)
pub trait AnalysisProvider: Send + Sync {
    // === Глобальные данные (держатся в памяти) ===

    /// Metadata конфигурации 1C
    fn configuration(&self) -> Option<Arc<Configuration>>;

    /// Symbol tree для модуля (нужен для cross-module resolution)
    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree>;

    /// Metadata модуля (тип, execution context)
    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata>;

    /// Resolve VFS path to FileId
    fn resolve_vfs_path(&self, path: &VfsPath) -> Option<FileId>;

    // === Per-file данные (могут быть временными) ===

    /// Parse file to AST
    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode>;

    /// Get file text
    fn file_text(&self, file_id: FileId) -> Arc<str>;

    /// Build ItemTree (signatures)
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree>;

    /// Lower to HIR bodies
    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies>;

    /// Build CFGs for module
    fn module_cfgs(&self, file_id: FileId) -> Arc<ModuleCfgs>;

    /// Region tree
    fn region_tree(&self, file_id: FileId) -> Arc<RegionTree>;

    /// Module-level regions
    fn module_level_regions(&self, file_id: FileId) -> Arc<Vec<RegionInfo>>;

    /// SDBL HIR
    fn sdbl_hir_in_file(&self, file_id: FileId) -> SdblHirEntries;

    /// All SDBL queries in file
    fn all_sdbl_in_file(&self, file_id: FileId) -> Arc<Vec<SdblQueryInfo>>;

    /// Reaching definitions for module
    fn module_reaching_definitions(&self, file_id: FileId) -> Arc<ModuleReachingDefs>;

    /// Liveness analysis for module
    fn module_liveness_analysis(&self, file_id: FileId) -> Arc<ModuleLiveness>;

    /// Line index for file
    fn line_index(&self, file_id: FileId) -> Arc<LineIndex>;
}
```

## StreamingProvider Implementation

```rust
pub struct StreamingProvider {
    /// Глобальный контекст (держится в памяти всю сессию)
    global: Arc<GlobalContext>,
}

struct GlobalContext {
    /// Metadata конфигурации (~31 MB для ERP)
    configuration: Option<Arc<Configuration>>,

    /// Symbol trees для всех модулей (~292 MB для ERP)
    /// Построены заранее в фазе инициализации
    symbol_trees: DashMap<ModuleId, Arc<SymbolTree>>,

    /// Module metadata (type, execution context)
    module_metadata: DashMap<ModuleId, Arc<ModuleMetadata>>,

    /// FileSet для resolve путей
    file_set: Arc<FileSet>,

    /// Источник текста файлов (читаем с диска по требованию)
    file_reader: FileReader,
}

impl AnalysisProvider for StreamingProvider {
    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree> {
        // Возвращаем из глобального кэша (всегда в памяти)
        self.global.symbol_trees.get(&module_id)
            .map(|r| r.clone())
            .unwrap_or_else(|| Arc::new(SymbolTree::empty()))
    }

    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode> {
        // Читаем текст с диска и парсим
        // НЕ кэшируем - caller отвечает за lifetime
        let text = self.global.file_reader.read(file_id);
        parser::parse(&text)
    }

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies> {
        // Вычисляем на лету
        let file_id = module_id.file_id;
        let text = self.global.file_reader.read(file_id);
        let parse = parser::parse(&text);
        let item_tree = Arc::new(item_tree::lower_file(&parse));

        // Lower to HIR (uses pure algorithms from hir-def)
        Arc::new(hir_def::lower_module_bodies_pure(&item_tree, &parse))
    }

    // ... остальные методы аналогично
}
```

## Pipeline для Analyze Mode

```
┌───────────────────────────────────────────────────────────────────┐
│                     INITIALIZATION PHASE                          │
│  1. Load metadata (~31 MB)                                        │
│  2. Scan all files, build FileSet                                 │
│  3. Build symbol_trees for ALL files (~292 MB, ~30 sec for ERP)  │
│     (необходимо для cross-module resolution)                      │
│  4. Build module_metadata for all files                           │
└───────────────────────────────────────────────────────────────────┘
                              │
                              ▼ GlobalContext ready (~320 MB)
┌───────────────────────────────────────────────────────────────────┐
│                      ANALYSIS PIPELINE                            │
│                                                                   │
│  ┌─────────────┐      bounded        ┌──────────────────────┐    │
│  │ File Reader │────channel(32)─────▶│ Worker Pool (N CPUs) │    │
│  │ (prefetch)  │                     │                      │    │
│  └─────────────┘                     │ For each file:       │    │
│                                      │ 1. Read text         │    │
│                                      │ 2. Parse (AST)       │    │
│                                      │ 3. Build HIR         │    │
│                                      │ 4. Run diagnostics   │    │
│                                      │ 5. DROP AST+HIR      │    │
│                                      │ 6. Send results      │    │
│                                      └──────────┬───────────┘    │
│                                                 │                 │
│                                      ┌──────────▼───────────┐    │
│                                      │ Result Collector     │    │
│                                      │ (aggregates diags)   │    │
│                                      └──────────────────────┘    │
└───────────────────────────────────────────────────────────────────┘
```

## Этапы реализации

### Этап 1: Создать AnalysisProvider trait
**Файлы:**
- `crates/ide-db/src/provider.rs` (новый)
- `crates/ide-db/src/lib.rs` (export)

**Задачи:**
1. Определить trait AnalysisProvider с методами
2. Реализовать SalsaProvider (обёртка над RootDatabase)
3. Обновить DiagnosticsContext использовать trait вместо &RootDatabase

### Этап 2: Выделить чистые алгоритмы
**Файлы:**
- `crates/hir-def/src/pure.rs` (новый)
- `crates/base-db/src/pure.rs` (новый)

**Задачи:**
1. Создать `lower_module_bodies_pure()` без Salsa зависимостей
2. Создать `build_cfgs_pure()`
3. Создать `build_region_tree_pure()`
4. Убедиться что все алгоритмы могут работать без db

### Этап 3: Реализовать StreamingProvider
**Файлы:**
- `crates/ide-db/src/streaming.rs` (новый)

**Задачи:**
1. Реализовать GlobalContext с metadata + symbol_trees
2. Реализовать FileReader (чтение с диска)
3. Реализовать все методы AnalysisProvider

### Этап 4: Реализовать Pipeline
**Файлы:**
- `crates/bsl-analyzer/src/analyze/mod.rs` (новый)
- `crates/bsl-analyzer/src/analyze/pipeline.rs`
- `crates/bsl-analyzer/src/analyze/worker.rs`

**Задачи:**
1. Добавить crossbeam-channel в зависимости
2. Реализовать FileReader с prefetch
3. Реализовать Worker pool
4. Реализовать Result collector
5. Интегрировать в CLI

### Этап 5: Оптимизация
**Задачи:**
1. Профилирование на ERP
2. Tune channel capacity
3. Tune prefetch buffer size
4. Добавить прогресс-бар

## Ожидаемые результаты

| Метрика | Текущий (Salsa) | Batch mode | Streaming (цель) |
|---------|-----------------|------------|------------------|
| Память (ERP) | 26.6 GB | 4.2 GB | **~500 MB** |
| Время (ERP) | 10 min | 22 min | ~15 min |

## Совместимость

- **LSP mode**: продолжает использовать Salsa через SalsaProvider
- **Analyze mode**: использует StreamingProvider
- **Общие алгоритмы**: parser, HIR lowering, diagnostics handlers
- **Нет дублирования кода**: только разные стратегии хранения данных

## Зависимости

Добавить в `bsl-analyzer/Cargo.toml`:
```toml
crossbeam-channel = "0.5"
```

## Риски и митигация

| Риск | Митигация |
|------|-----------|
| Cross-module диагностики ломаются | symbol_tree держится глобально |
| Медленнее из-за re-parse | Prefetch + параллелизм компенсируют |
| Сложность поддержки двух путей | Единый trait AnalysisProvider |
| symbol_tree устаревает | В analyze mode файлы не меняются |

## Альтернативы (рассмотрены и отвергнуты)

1. **Salsa 3.0** - не готова, неизвестные сроки
2. **Batch + synthetic_write** - всё равно 4+ GB для ERP
3. **Отказ от cross-module диагностик** - потеря важного функционала
