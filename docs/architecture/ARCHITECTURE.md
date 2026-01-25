# BSL Analyzer - Architecture

## Обзор

BSL Analyzer построен по образцу rust-analyzer с адаптацией под BSL/1C.

```
┌─────────────────────────────────────────────────────────────┐
│                    bsl-analyzer (LSP Server)                 │
│  - JSON-RPC, LSP protocol, CLI                              │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                         ide                                  │
│  - High-level API (hover, completion, goto, diagnostics)    │
└─────────────────────────────────────────────────────────────┘
        │                     │                     │
        ▼                     ▼                     ▼
┌───────────────┐    ┌───────────────┐    ┌───────────────┐
│ide-diagnostics│    │  ide-assists  │    │    ide-db     │
│ 171 diagnostics│   │ Code actions  │    │ RootDatabase  │
└───────────────┘    └───────────────┘    └───────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    hir / hir-def / hir-ty                    │
│  - ItemTree, SymbolTree, ModuleBodies                       │
│  - Type inference, Name resolution                          │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                   syntax / parser / lexer                    │
│  - Rowan CST (120+ nodes), typed AST wrappers               │
│  - Event-based parser, logos tokenizer                      │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                   Supporting Crates                          │
├─────────────────────────────────────────────────────────────┤
│ base-db      │ Source database, Salsa 0.25.2                │
│ vfs          │ Virtual file system                          │
│ bsl-metadata │ 1C metadata (Configuration, CommonModule)    │
│ bsl-platform │ Platform types (Строка, Число, Массив)       │
│ sdbl-hir     │ SDBL HIR + type inference                    │
│ cfg          │ Control Flow Graph                           │
│ dataflow     │ Reaching definitions, liveness               │
│ project-model│ Project config (.bsl-analyzer.json)          │
└─────────────────────────────────────────────────────────────┘
```

## Структура крейтов

| Слой | Крейты | Назначение |
|------|--------|------------|
| **Анализ** | lexer, parser, syntax | Tokenization (80+ BSL, 150+ SDBL), Rowan CST |
| **Семантика** | hir-def, hir-ty, hir | ItemTree, SymbolTree, type inference |
| **IDE** | ide-db, ide-diagnostics, ide-assists, ide | 171 диагностика, code actions, LSP API |
| **SDBL** | sdbl-hir | Query language HIR + type inference |
| **Dataflow** | cfg, cfg-types, dataflow | CFG, reaching definitions, liveness |
| **Metadata** | bsl-metadata, bsl-platform | 1C configuration, platform types |
| **Infra** | base-db, vfs, vfs-notify, project-model | Salsa, VFS, file watching |
| **Utils** | line-index, intern, stdx, paths, profile | Helpers |

## Database Hierarchy (Salsa)

```
salsa::Database
    ↓
SourceDatabase (base-db)
    - file_text(), source_root(), file_source_root()
    ↓
DefDatabase (hir-def)
    - Invalidation Barriers:
      • item_tree_query (LRU: 512) - method signatures
      • region_tree_query (LRU: 128) - preprocessor regions
      • conditional_tree_query (LRU: 128)
    - Derived:
      • symbol_tree_query (LRU: 128) - case-insensitive lookup
      • module_data_query (LRU: 512)
      • module_bodies_query (LRU: 256) - HIR bodies + diagnostics
    - Type inference:
      • infer_types_query (LRU: 16)
    - Workspace:
      • module_index_query (LRU: 512)
      • file_dependencies_query (LRU: 16)
    ↓
RootDatabase (ide-db)
    - Metadata:
      • load_configuration (LRU: 16, Durability::HIGH)
      • module_metadata_query (LRU: 128)
    - SDBL:
      • all_sdbl_in_file_query (LRU: 128)
      • sdbl_hir_in_file_query (LRU: 64)
    - Dataflow:
      • method_cfg_query (LRU: 256)
      • reaching_definitions_query (LRU: 256)
      • liveness_analysis_query (LRU: 256)
    - Utils:
      • line_index_query (LRU: 256)
```

### Durability

```rust
SourceRoot::durability() → Durability {
    HIGH: library files (is_library = true)  // не пересчитываются при изменении user code
    LOW:  user code (is_library = false)     // пересчитываются при каждом изменении
}
```

## Ключевые компоненты

### Rowan (Syntax Trees)

Red-green trees для full-fidelity parsing:
- Immutable CST с сохранением whitespace
- Эффективное sharing памяти между версиями
- Typed AST wrappers (`ast::Function`, `ast::Statement`)

### HIR-based Diagnostics

Диагностики собираются при HIR lowering, не отдельными traversals:

```
AST → [module_bodies_query] → ModuleBodies { diagnostics: Vec<BodyDiagnostic> }
                                    ↓
ide-diagnostics/lib.rs → match BodyDiagnostic { ... } → handlers/*
```

Преимущества: 1 traversal вместо N, автоматическое кеширование Salsa.

### DiagnosticMetadata Architecture

Metadata-driven система для всех 144 диагностик (100% coverage):
- **Zero-cost abstraction**: compile-time const metadata + runtime config merging
- **Центральный registry**: все severity/tags/minutesToFix в `metadata_registry.rs`
- **Runtime overrides**: JSON config может переопределить severity/type/tags
- **Автоматический LSP mapping**: DiagnosticType + SeverityLevel → LSP Severity
- **Совместимость с Java**: 1:1 соответствие с @DiagnosticMetadata annotations

Вместо hardcoded значений handlers используют `ctx.severity(code)` и `ctx.tags(code)`.

### Metadata (bsl-metadata)

1C configuration loading с Salsa кешированием:
- Configuration, CommonModule, Register, EventSubscription
- Designer format XML parsing
- `load_configuration` query (LRU: 16, Durability::HIGH)

### SDBL (sdbl-hir)

Query language analysis:
- SDBL → HIR lowering
- Type inference по metadata (таблицы, поля)
- Name resolution (aliases, subqueries)

### CFG + Dataflow

Control Flow Graph для flow-sensitive анализа:
- `cfg` crate: CFG construction из Rowan AST
- `dataflow` crate: reaching definitions, liveness analysis
- Используется для: unreachable_code, missing_return, unused variables

## Потоки данных

### Parsing
```
Source Text → Lexer → Tokens → Parser → GreenNode → SyntaxNode → AST
```

### Diagnostics
```
file_text → parse → item_tree → module_bodies → [BodyDiagnostic]
                                     ↓
                        ide-diagnostics → [Diagnostic] → LSP
```

### Incremental Update
```
File Change → VFS → Salsa Invalidation → Recompute affected queries only
                            │
                            ├─ .bsl changed → parse, HIR (metadata NOT invalidated)
                            └─ Configuration.xml → load_configuration + dependents
```

## Производительность

**Проект doc3 (121 MB, 6,540 файлов):**

| Метрика | Java (bsl-ls) | Rust (bsl-analyzer) | Улучшение |
|---------|---------------|---------------------|-----------|
| Full analysis | 58.9s | **11.2s** | **5.3x** |
| CPU time | 337.1s | 59.3s | **5.7x** |
| I/O time | 28.8s | 2.8s | **10.3x** |
| Peak memory | 3,822 MB | **1,426 MB** | **2.7x** |

## Логирование

```bash
BSL_LOG=debug cargo run          # debug logs
BSL_LOG=parser=trace cargo run   # trace только для parser
BSL_PROFILE=* cargo run          # hierarchical profiling
BSL_LOG_FILE=/tmp/bsl.log        # write to file
```

## Совместимость

100% совместимость с bsl-language-server:
- Те же коды диагностик и severity
- Поддержка `.bsl-analyzer.json` и `.bsl-language-server.json`
