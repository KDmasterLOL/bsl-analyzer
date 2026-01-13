# AnalysisProvider Trait Implementation Plan

## Overview

Этот документ описывает план реализации `AnalysisProvider` trait — абстракции над источником данных для диагностик, которая позволит использовать:
- **SalsaProvider** для LSP mode (полное кэширование, IDE features)
- **StreamingProvider** для analyze mode (минимальное потребление памяти)

**Связанные документы:**
- [STREAMING_ANALYZE_ARCHITECTURE.md](./STREAMING_ANALYZE_ARCHITECTURE.md) — верхнеуровневая архитектура
- [STREAMING_WORKER_ARCHITECTURE.md](./STREAMING_WORKER_ARCHITECTURE.md) — детали worker pool

## Текущее состояние

### DiagnosticsContext использует RootDatabase напрямую

```rust
// crates/ide-diagnostics/src/lib.rs:395-412
pub struct DiagnosticsContext<'a> {
    pub db: &'a dyn RootDatabase,  // ← Прямая зависимость от Salsa
    pub config: &'a DiagnosticsConfig,
    pub file_id: FileId,
    pub workspace_root: Option<&'a std::path::Path>,
    pub configuration_path: Option<&'a std::path::Path>,
    pub configuration_path_input: Option<ide_db::metadata::ConfigurationPathInput<'a>>,
    pub file_set: Option<&'a vfs::FileSet>,
}
```

### Методы RootDatabase используемые диагностиками

Анализ `collect_hir_diagnostics` и `collect_metadata_diagnostics`:

| Метод | Описание | Глобальный? |
|-------|----------|-------------|
| `db.parse(file_id)` | AST файла | ❌ Per-file |
| `db.module_bodies(module_id)` | HIR bodies + diagnostics | ❌ Per-file |
| `db.module_metadata(module_id)` | Тип модуля, контекст | ❌ Per-file |
| `db.file_source_root_input(file_id)` | Source root файла | ✅ Global |
| `db.source_root_input(id)` | Source root | ✅ Global |
| `db.item_tree(file_id)` | Сигнатуры методов | ❌ Per-file |
| `db.symbol_tree(module_id)` | Символы модуля | ✅ Global (cross-module) |
| `db.workspace_symbols(source_root_id)` | Все символы workspace | ✅ Global |
| `db.module_index(source_root_input)` | Индекс модулей | ✅ Global |
| `ide_db::metadata::load_configuration()` | Configuration 1C | ✅ Global |

## Ключевые различия Configuration и ModuleMetadata

### Configuration (глобальный контекст)

```rust
// bsl_metadata::Configuration (~31 MB для ERP)
struct Configuration {
    name: String,
    common_modules: Vec<CommonModule>,      // Все CommonModules
    metadata_objects: Vec<MetadataObject>,  // Catalogs, Documents, etc.
    registers: Vec<Register>,               // Information, Accumulation, etc.
    event_subscriptions: Vec<EventSubscription>,
    defined_types: Vec<DefinedType>,
    // ... caches для быстрого поиска
}
```

**Использование:**
- Глобальные запросы: найти CommonModule по имени
- Cross-module проверки: существует ли вызываемый метод
- Metadata diagnostics: EventSubscription handlers

### ModuleMetadata (per-file контекст)

```rust
// hir_def::ModuleMetadata (~1 KB per file)
struct ModuleMetadata {
    module_type: ModuleType,                    // CommonModule, ObjectModule, FormModule, etc.
    execution_context: Option<ExecutionContext>, // Server, Client, ClientServer
    common_module: Option<Arc<CommonModule>>,   // Полные данные если CommonModule
    mdo: Option<Arc<MetadataObject>>,           // Полные данные если ObjectModule
}
```

**Использование:**
- Тип модуля для диагностик
- Контекст выполнения (НаСервере, НаКлиенте)
- Флаги модуля (global, privileged, serverCall)

### Связь

```
Configuration (загружается 1 раз)
    │
    ├── find_common_module(name) ──► CommonModule
    │                                    │
    └── file_path ───────────────────────┼──► ModuleMetadata {
                                         │        module_type,
                                         │        execution_context,
                                         └──────► common_module: Some(Arc<...>)
                                              }
```

## AnalysisProvider Trait

### Определение

```rust
// crates/ide-db/src/provider.rs (новый файл)

use std::sync::Arc;
use vfs::FileId;
use hir_def::{ModuleId, ModuleBodies, ModuleMetadata};
use bsl_metadata::Configuration;
use syntax::Parse;
use rowan::SyntaxNode;

/// Абстракция над источником данных для диагностик.
///
/// Две реализации:
/// - SalsaProvider: использует RootDatabase с полным кэшированием (LSP mode)
/// - StreamingProvider: вычисляет на лету, освобождает после use (analyze mode)
pub trait AnalysisProvider: Send + Sync {
    // === Глобальные данные (держатся в памяти) ===

    /// Metadata конфигурации 1C.
    ///
    /// Содержит все CommonModules, MetadataObjects, Registers, etc.
    /// Загружается один раз и переиспользуется для всех файлов.
    fn configuration(&self) -> Option<Arc<Configuration>>;

    /// Symbol tree для модуля (нужен для cross-module resolution).
    ///
    /// В streaming mode: построен заранее для всех файлов (~292 MB для ERP).
    /// В salsa mode: кэшируется по требованию.
    fn symbol_tree(&self, module_id: ModuleId) -> Arc<hir_def::SymbolTree>;

    /// Индекс модулей (имя → FileId).
    fn module_index(&self) -> Arc<hir_def::module_index::ModuleIndex>;

    /// Workspace symbols — индекс всех CommonModules.
    ///
    /// Используется для qualified name resolution: `ОбщегоНазначения.Метод()`.
    fn workspace_symbols(&self) -> Arc<hir_def::WorkspaceSymbols>;

    // === Per-file данные (могут быть временными) ===

    /// Metadata модуля (тип, execution context).
    ///
    /// Извлекается из Configuration + file_path.
    /// В streaming mode: вычисляется на лету.
    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata>;

    /// Parse file to AST.
    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode>;

    /// Get file text.
    fn file_text(&self, file_id: FileId) -> Arc<str>;

    /// Build ItemTree (signatures).
    fn item_tree(&self, file_id: FileId) -> Arc<hir_def::item_tree::ItemTree>;

    /// Lower to HIR bodies.
    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies>;

    /// Region tree.
    fn region_tree(&self, file_id: FileId) -> Arc<hir_def::region_tree::RegionTree>;

    /// Line index for file.
    fn line_index(&self, file_id: FileId) -> Arc<base_db::LineIndex>;

    /// File path as string (for metadata lookups).
    fn file_path(&self, file_id: FileId) -> Option<String>;

    // === Optional: CFG and dataflow (for complex diagnostics) ===

    /// Build CFGs for module.
    fn module_cfgs(&self, file_id: FileId) -> Option<Arc<cfg::ModuleCfgs>>;

    /// Reaching definitions for module.
    fn module_reaching_definitions(&self, file_id: FileId)
        -> Option<Arc<cfg::dataflow::ModuleReachingDefs>>;

    /// Liveness analysis for module.
    fn module_liveness_analysis(&self, file_id: FileId)
        -> Option<Arc<cfg::dataflow::ModuleLiveness>>;
}
```

## Реализации

### SalsaProvider (для LSP mode)

```rust
// crates/ide-db/src/salsa_provider.rs (новый файл)

use crate::{RootDatabase, provider::AnalysisProvider};
use std::sync::Arc;

/// Provider backed by Salsa RootDatabase.
///
/// Все методы делегируют к Salsa queries с полным кэшированием.
/// Используется в LSP mode для максимальной производительности
/// при редактировании файлов.
pub struct SalsaProvider<'db> {
    db: &'db dyn RootDatabase,
    configuration_path_input: Option<ide_db::metadata::ConfigurationPathInput<'db>>,
}

impl<'db> SalsaProvider<'db> {
    pub fn new(
        db: &'db dyn RootDatabase,
        configuration_path_input: Option<ide_db::metadata::ConfigurationPathInput<'db>>,
    ) -> Self {
        Self { db, configuration_path_input }
    }
}

impl<'db> AnalysisProvider for SalsaProvider<'db> {
    fn configuration(&self) -> Option<Arc<Configuration>> {
        let path_input = self.configuration_path_input?;
        Some(ide_db::metadata::load_configuration(self.db, path_input))
    }

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies> {
        self.db.module_bodies(module_id)
    }

    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata> {
        self.db.module_metadata(module_id)
    }

    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode> {
        self.db.parse(file_id)
    }

    // ... остальные методы делегируют к db
}
```

### StreamingProvider (для analyze mode)

```rust
// crates/ide-db/src/streaming.rs (новый файл)

use crate::provider::AnalysisProvider;
use dashmap::DashMap;
use std::sync::Arc;

/// Global context that stays in memory for the entire analysis.
///
/// Total memory: ~335 MB for ERP (25K files)
pub struct GlobalContext {
    /// Metadata конфигурации (~31 MB для ERP).
    pub configuration: Option<Arc<Configuration>>,

    /// Symbol trees для всех модулей (~292 MB для ERP).
    /// Построены заранее в фазе инициализации.
    /// Ключевая структура для cross-module validation.
    pub symbol_trees: DashMap<ModuleId, Arc<SymbolTree>>,

    /// Workspace symbols — индекс над SymbolTrees (~5 MB).
    /// Map: имя_CommonModule → CommonModuleInfo.
    /// Для qualified name resolution: `ОбщегоНазначения.Метод()`.
    pub workspace_symbols: Arc<WorkspaceSymbols>,

    /// Module index (name → FileId) (~5 MB).
    /// Для resolve external references.
    pub module_index: Arc<ModuleIndex>,

    /// FileSet для resolve путей.
    pub file_set: Arc<FileSet>,

    /// Источник текста файлов (читаем с диска по требованию).
    pub file_reader: FileReader,
}

/// Provider for streaming analyze mode.
///
/// - Глобальные данные (configuration, symbol_trees) держатся в памяти
/// - Per-file данные (AST, HIR) вычисляются на лету и НЕ кэшируются
/// - Caller отвечает за освобождение памяти после обработки файла
pub struct StreamingProvider {
    global: Arc<GlobalContext>,
}

impl AnalysisProvider for StreamingProvider {
    fn configuration(&self) -> Option<Arc<Configuration>> {
        self.global.configuration.clone()
    }

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree> {
        self.global.symbol_trees
            .get(&module_id)
            .map(|r| r.clone())
            .unwrap_or_else(|| Arc::new(SymbolTree::empty()))
    }

    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata> {
        // Вычисляем на лету из configuration + file_path
        let file_path = self.file_path(module_id.file_id)?;
        let module_type = get_module_type_from_uri(&file_path)?;

        if module_type == ModuleType::CommonModule {
            let config = self.global.configuration.as_ref()?;
            let name = extract_common_module_name(&file_path)?;
            let common_module = config.find_common_module(&name)?.clone();
            let execution_context = determine_execution_context(&common_module);

            Arc::new(ModuleMetadata {
                module_type,
                execution_context: Some(execution_context),
                common_module: Some(Arc::new(common_module)),
                mdo: None,
            })
        } else {
            Arc::new(ModuleMetadata::unknown(module_type))
        }
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

        // Lower to HIR (pure algorithm, no Salsa dependency)
        Arc::new(hir_def::lower_module_bodies_pure(&item_tree, &parse))
    }

    // ... остальные методы
}
```

## Миграция DiagnosticsContext

### Шаг 1: Добавить provider в DiagnosticsContext

```rust
// crates/ide-diagnostics/src/lib.rs

pub struct DiagnosticsContext<'a> {
    // Новый унифицированный интерфейс
    pub provider: &'a dyn AnalysisProvider,

    // Конфигурация диагностик
    pub config: &'a DiagnosticsConfig,
    pub file_id: FileId,

    // Deprecated: будет удалено после миграции
    #[deprecated(note = "Use provider instead")]
    pub db: Option<&'a dyn RootDatabase>,
}

impl<'a> DiagnosticsContext<'a> {
    /// Create context with provider (new API).
    pub fn new(
        provider: &'a dyn AnalysisProvider,
        config: &'a DiagnosticsConfig,
        file_id: FileId,
    ) -> Self {
        Self {
            provider,
            config,
            file_id,
            db: None,
        }
    }

    // Helper methods that delegate to provider
    pub fn parse(&self) -> Parse<SyntaxNode> {
        self.provider.parse(self.file_id)
    }

    pub fn module_bodies(&self) -> Arc<ModuleBodies> {
        let module_id = ModuleId::new(self.file_id);
        self.provider.module_bodies(module_id)
    }

    pub fn module_metadata(&self) -> Arc<ModuleMetadata> {
        let module_id = ModuleId::new(self.file_id);
        self.provider.module_metadata(module_id)
    }

    pub fn configuration(&self) -> Option<Arc<Configuration>> {
        self.provider.configuration()
    }
}
```

### Шаг 2: Мигрировать handlers по одному

```rust
// Пример миграции: common_module_name_cached.rs

// До:
pub fn from_metadata(
    metadata: &ModuleMetadata,
    config: &DiagnosticsConfig,
) -> Vec<Diagnostic> {
    // ...
}

// После:
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let metadata = ctx.module_metadata();
    // ... остальной код без изменений
}
```

## Этапы реализации

### Этап 1: Создать AnalysisProvider trait (1-2 дня)

**Файлы:**
- `crates/ide-db/src/provider.rs` — trait definition
- `crates/ide-db/src/salsa_provider.rs` — SalsaProvider implementation
- `crates/ide-db/src/lib.rs` — exports

**Задачи:**
1. Определить trait AnalysisProvider со всеми методами
2. Реализовать SalsaProvider (обёртка над RootDatabase)
3. Добавить unit tests

### Этап 2: Обновить DiagnosticsContext (1 день)

**Файлы:**
- `crates/ide-diagnostics/src/lib.rs`

**Задачи:**
1. Добавить `provider: &dyn AnalysisProvider` в DiagnosticsContext
2. Добавить helper методы (parse, module_bodies, etc.)
3. Сохранить обратную совместимость через deprecated `db` field

### Этап 3: Мигрировать diagnostic handlers (2-3 дня)

**Задачи:**
1. Заменить `ctx.db.method()` на `ctx.provider.method()` или `ctx.method()`
2. Проверить что все тесты проходят
3. Удалить deprecated `db` field

### Этап 4: Выделить чистые алгоритмы (2 дня)

**Файлы:**
- `crates/hir-def/src/pure.rs` (новый)
- `crates/parser/src/lib.rs`

**Задачи:**
1. Создать `lower_module_bodies_pure()` без Salsa зависимостей
2. Убедиться что parser::parse() не зависит от Salsa
3. Добавить benchmarks

### Этап 5: Реализовать StreamingProvider (2-3 дня)

**Файлы:**
- `crates/ide-db/src/streaming.rs` (новый)
- `crates/ide-db/src/global_context.rs` (новый)

**Задачи:**
1. Реализовать GlobalContext (configuration, symbol_trees)
2. Реализовать FileReader (чтение с диска)
3. Реализовать все методы AnalysisProvider
4. Integration tests

### Этап 6: Интеграция с analyze pipeline (3-4 дня)

**Файлы:**
- `crates/bsl-analyzer/src/analyze/mod.rs`
- `crates/bsl-analyzer/src/analyze/pipeline.rs`
- `crates/bsl-analyzer/src/analyze/worker.rs`

**Задачи:**
1. Реализовать initialization phase (build GlobalContext)
2. Реализовать worker pool с StreamingProvider
3. Интегрировать с CLI
4. Профилирование на ERP

## Риски и митигация

| Риск | Митигация |
|------|-----------|
| Некоторые диагностики неявно зависят от Salsa | Постепенная миграция с тестами |
| Breaking changes в public API | Deprecated warnings, version bump |
| Производительность StreamingProvider | Профилирование, prefetch |
| Сложность поддержки двух путей | Единый trait, максимум переиспользования |

## Метрики успеха

1. **Все существующие тесты проходят** после миграции на AnalysisProvider
2. **LSP mode работает** с SalsaProvider без регрессий
3. **Analyze mode** с StreamingProvider потребляет < 500 MB памяти на ERP
4. **Время анализа** не увеличивается более чем на 20%

## Решённые вопросы

### CFG и Dataflow — НУЖНЫ

Используются для важных диагностик:
- `UnusedLocalVariable` — liveness analysis
- `MissingTempStorageDeletion` — reaching definitions
- `RewriteMethodParameter` — dataflow

**Решение:** Вычислять на лету в StreamingProvider (не кэшировать).

### Все диагностики обязательны

Проект используется для SonarQube — нужна полная картина.
Пропуск диагностик для экономии памяти НЕ ВАРИАНТ.

### SymbolTree vs WorkspaceSymbols

**SymbolTree** (per-module, ~292 MB total):
- Символы ОДНОГО модуля (методы + переменные)
- Нужен для cross-module validation
- Строится в initialization phase

**WorkspaceSymbols** (~5 MB):
- Денормализованный индекс над SymbolTrees
- Map: имя_CommonModule → {file_id, methods}
- Для qualified name resolution: `ОбщегоНазначения.Метод()`

**Решение для streaming mode:**
1. SymbolTrees строятся в initialization (~292 MB, ~30 sec для ERP)
2. WorkspaceSymbols строится как индекс над готовыми SymbolTrees (~5 MB)
3. Обе структуры живут всю сессию анализа

```
┌───────────────────────────────────────────────────────────────────┐
│                     INITIALIZATION PHASE                          │
│  1. Load metadata (~31 MB)                                        │
│  2. Scan all files, build FileSet                                 │
│  3. Build SymbolTrees for ALL files (~292 MB, ~30 sec for ERP)   │
│  4. Build WorkspaceSymbols index over SymbolTrees (~5 MB)        │
│  5. Build ModuleIndex (name → FileId)                             │
└───────────────────────────────────────────────────────────────────┘
                              │
                              ▼ GlobalContext ready (~330 MB)
```

### Source root

**Решение:** Один synthetic source root для всех файлов.
- StreamingProvider не использует Salsa source roots
- FileSet и ModuleIndex предоставляют нужный функционал

## Appendix: Список методов RootDatabase

Полный список методов используемых в diagnostic handlers:

```rust
// Parsing
db.parse(file_id) -> Parse<SyntaxNode>
db.file_text(file_id) -> Arc<str>

// HIR
db.item_tree(file_id) -> Arc<ItemTree>
db.symbol_tree(module_id) -> Arc<SymbolTree>
db.module_bodies(module_id) -> Arc<ModuleBodies>
db.module_metadata(module_id) -> Arc<ModuleMetadata>

// Indexes
db.module_index(source_root_input) -> Arc<ModuleIndex>
db.workspace_symbols(source_root_id) -> Arc<WorkspaceSymbols>

// Source management
db.file_source_root_input(file_id) -> FileSourceRootInput
db.source_root_input(id) -> SourceRootInput

// Metadata
load_configuration(db, path_input) -> Arc<Configuration>

// CFG/Dataflow (optional)
db.module_cfgs(file_id) -> Arc<ModuleCfgs>
db.module_reaching_definitions(file_id) -> Arc<ModuleReachingDefs>
db.module_liveness_analysis(file_id) -> Arc<ModuleLiveness>

// Line index
db.line_index(file_id) -> Arc<LineIndex>
```
