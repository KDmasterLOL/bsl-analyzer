# BSL Analyzer - Architecture

## Обзор

BSL Analyzer построен по образцу rust-analyzer с адаптацией под специфику BSL/1C.

```
┌─────────────────────────────────────────────────────────────┐
│                    bsl-analyzer (LSP Server)                 │
│  - JSON-RPC handling                                         │
│  - LSP protocol implementation                               │
│  - CLI interface                                             │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                         ide                                  │
│  - High-level API for IDE features                          │
│  - Coordinates all subsystems                               │
└─────────────────────────────────────────────────────────────┘
        │                     │                     │
        ▼                     ▼                     ▼
┌───────────────┐    ┌───────────────┐    ┌───────────────┐
│ ide-diagnostics│    │  ide-assists  │    │    ide-db     │
│ - 181 diagnostics│  │ - Code actions│    │ - RootDatabase│
│ - Quick fixes   │  │ - Refactorings│    │ - Queries     │
└───────────────┘    └───────────────┘    └───────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                          hir                                 │
│  - High-level Intermediate Representation                   │
│  - OOP-style API for semantic information                   │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        hir-def                               │
│  - Definitions (modules, methods, variables)                │
│  - Name resolution                                          │
│  - Symbol table                                             │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        syntax                                │
│  - CST (Concrete Syntax Tree) based on Rowan               │
│  - AST typed wrappers                                       │
│  - SyntaxNode, SyntaxToken                                  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        parser                                │
│  - BSL grammar implementation                               │
│  - Error recovery                                           │
│  - SDBL (query language) support                            │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        lexer                                 │
│  - Tokenization of BSL source code                          │
│  - Based on logos crate                                     │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                   Supporting Crates                          │
├─────────────────────────────────────────────────────────────┤
│ base-db      │ Source database, Salsa integration           │
│ vfs          │ Virtual file system                          │
│ project-model│ Project structure (configurations, etc.)     │
│ intern       │ String/ID interning                          │
│ stdx         │ Standard library extensions                  │
│ profile      │ Profiling utilities                          │
│ test-fixture │ Test fixtures                                │
│ test-utils   │ Test utilities                               │
└─────────────────────────────────────────────────────────────┘
```

## Ключевые архитектурные решения

### 1. Incremental Computation (Salsa)

Используем Salsa для инкрементального пересчёта:

```rust
#[salsa::input]
pub struct SourceFile {
    #[return_ref]
    pub text: String,
}

#[salsa::tracked]
pub fn parse(db: &dyn SourceDatabase, file: SourceFile) -> Parse<SourceFile> {
    let text = file.text(db);
    parser::parse(&text)
}
```

### 2. Rowan для Syntax Trees

Red-green trees для эффективного представления синтаксиса:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntaxKind {
    // Tokens
    L_PAREN,
    R_PAREN,
    IDENT,
    NUMBER,
    STRING,
    // ...

    // Nodes
    SOURCE_FILE,
    FUNCTION_DEF,
    PROCEDURE_DEF,
    STATEMENT,
    EXPRESSION,
    // ...
}

pub type SyntaxNode = rowan::SyntaxNode<BslLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<BslLanguage>;
```

### 3. Diagnostic Infrastructure

Каждая диагностика - отдельный модуль с единообразным интерфейсом:

```rust
pub trait Diagnostic: Send + Sync {
    fn code(&self) -> DiagnosticCode;
    fn message(&self) -> String;
    fn severity(&self) -> Severity;
    fn range(&self) -> TextRange;
    fn fixes(&self) -> Option<Vec<Assist>>;
}

pub struct DiagnosticsContext<'a> {
    pub db: &'a dyn RootDatabase,
    pub config: &'a DiagnosticsConfig,
    pub file_id: FileId,
}
```

### 4. LSP Compatibility

Полная совместимость с bsl-language-server:

- Те же коды диагностик
- Те же severity levels
- Те же параметры конфигурации
- Тот же формат .bslls.json

## Структура крейтов

### bsl-analyzer
Главный бинарник, LSP сервер:
- `main.rs` - точка входа
- `config.rs` - конфигурация
- `handlers/` - обработчики LSP requests
- `dispatch.rs` - роутинг запросов

### lexer
Лексический анализатор:
- `lib.rs` - токенизация
- `token_kind.rs` - виды токенов
- `tests.rs` - тесты

### parser
Парсер BSL:
- `grammar/` - грамматика BSL
  - `expressions.rs`
  - `statements.rs`
  - `items.rs`
- `event.rs` - события парсинга
- `parser.rs` - основной парсер

### syntax
Синтаксические деревья:
- `syntax_kind.rs` - виды узлов
- `ast/` - типизированные обёртки
- `algo.rs` - алгоритмы обхода

### hir / hir-def
Семантический анализ:
- `module.rs` - представление модуля
- `symbols.rs` - таблица символов
- `resolver.rs` - разрешение имён

### ide-diagnostics
Диагностики:
- `lib.rs` - инфраструктура
- `handlers/` - 181 диагностика

## Потоки данных

### Parsing Flow
```
Source Text → Lexer → Tokens → Parser → GreenNode → SyntaxNode → AST
```

### Diagnostic Flow
```
AST → HIR → DiagnosticContext → [Diagnostics] → LSP Diagnostics
```

### Incremental Update
```
File Change → VFS Update → Salsa Invalidation → Recompute Affected Queries
```

## Паттерны кода

### Context Pattern
```rust
pub struct AnalysisContext<'db> {
    pub db: &'db dyn RootDatabase,
    pub file_id: FileId,
}

impl<'db> AnalysisContext<'db> {
    pub fn syntax_tree(&self) -> &SyntaxNode {
        self.db.parse(self.file_id).syntax()
    }
}
```

### Visitor Pattern (для диагностик)
```rust
pub trait SyntaxVisitor {
    fn visit_function(&mut self, func: &ast::Function) {}
    fn visit_procedure(&mut self, proc: &ast::Procedure) {}
    fn visit_statement(&mut self, stmt: &ast::Statement) {}
    // ...
}
```

### InFile<T> для привязки к файлу
```rust
pub struct InFile<T> {
    pub file_id: FileId,
    pub value: T,
}
```

## Сравнение с bsl-language-server

| Аспект | BSL-LS (Java) | BSL Analyzer (Rust) |
|--------|---------------|---------------------|
| Парсер | ANTLR 4 | Hand-written + Rowan |
| Инкрементальность | Нет | Salsa |
| Memory model | GC (JVM) | Manual (Rust) |
| Concurrency | ForkJoinPool | Rayon / async |
| AST | ANTLR generated | Rowan + typed wrappers |

## Логирование

### Трассировка и профилирование (tracing)

Используем **tracing ecosystem** по примеру rust-analyzer:

```toml
[workspace.dependencies]
tracing = { version = "0.1", default-features = false, features = ["std"] }
tracing-subscriber = { version = "0.3", default-features = false, features = [
    "registry",
    "fmt",
    "std",
    "tracing-log",
] }
tracing-tree = "0.4"
```

### Архитектура логирования

```
┌─────────────────────────────────────────────────────────┐
│                   Application Code                       │
│  tracing::info!("message", field = ?value)              │
│  let _span = tracing::info_span!("operation").entered() │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│                  tracing Registry                        │
│  (координирует несколько слоев)                         │
└─────────────────────────────────────────────────────────┘
            │               │               │
            ▼               ▼               ▼
┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│Format Layer │  │Profiler Layer│  │JSON Layer  │
│(stderr/file)│  │(timing tree) │  │(metrics)   │
└─────────────┘  └─────────────┘  └─────────────┘
```

### Инициализация логирования

Инициализация в `bsl-analyzer/src/bin/main.rs`:

```rust
fn setup_logging(log_file: Option<PathBuf>) -> anyhow::Result<()> {
    let writer = match log_file {
        Some(file) => BoxMakeWriter::new(Arc::new(fs::File::create(&file)?)),
        None => BoxMakeWriter::new(std::io::stderr),
    };

    bsl_analyzer::tracing::Config {
        writer,
        filter: env::var("BSL_LOG").ok().unwrap_or_else(|| "warn".to_owned()),
        profile_filter: env::var("BSL_PROFILE").ok(),
    }
    .init()?;

    Ok(())
}
```

### Environment Variables

- **BSL_LOG** - уровень логирования (default: "warn")
  - `BSL_LOG=debug` - весь debug output
  - `BSL_LOG=info` - общая информация
  - `BSL_LOG=parser=debug` - debug только для parser
  - Синтаксис: `target=level` (EnvFilter)

- **BSL_LOG_FILE** - запись логов в файл
  - `BSL_LOG_FILE=/tmp/bsl-analyzer.log`

- **BSL_PROFILE** - иерархическое профилирование
  - `BSL_PROFILE=*` - все spans
  - `BSL_PROFILE=parse|analyze` - только parse и analyze spans
  - `BSL_PROFILE=*@3>10` - глубина 3, > 10ms

### Паттерны использования

#### Базовое логирование

```rust
use tracing::{trace, debug, info, warn, error};

// Простое сообщение
info!("parsing started");

// С полями (structured logging)
debug!(file_id = ?file_id, "parsing file");

// С несколькими полями
warn!(
    line = line_number,
    column = column,
    "syntax error"
);
```

#### Spans для профилирования

```rust
// Span на время выполнения функции
pub fn parse_file(input: &str) -> Parse {
    let _span = tracing::info_span!("parse_file", len = input.len()).entered();

    // ... parsing logic ...
}

// Span для измерения производительности
pub fn run_diagnostics(&self, db: &dyn RootDatabase) {
    let _p = tracing::info_span!("run_diagnostics").entered();

    for diagnostic in &self.diagnostics {
        let _d = tracing::debug_span!("diagnostic", code = %diagnostic.code()).entered();
        diagnostic.check(db);
    }
}
```

#### Вложенные spans (иерархия)

```rust
fn analyze_project() {
    let _p = tracing::info_span!("analyze_project").entered();

    for module in modules {
        let _m = tracing::debug_span!("analyze_module", name = %module.name()).entered();

        for function in module.functions() {
            let _f = tracing::trace_span!("analyze_function").entered();
            // ...
        }
    }
}
```

Вывод при `BSL_PROFILE=*`:
```
1234ms    analyze_project
  456ms    analyze_module name="CommonModule"
    12ms    analyze_function
    23ms    analyze_function
  234ms    analyze_module name="ObjectModule"
```

### Рекомендации

1. **Использовать spans для всех значимых операций:**
   - Parsing
   - Semantic analysis
   - Diagnostics
   - LSP requests

2. **Уровни логирования:**
   - `error!` - критические ошибки
   - `warn!` - предупреждения (default level)
   - `info!` - общая информация (start/stop operations)
   - `debug!` - детальная отладочная информация
   - `trace!` - очень детальная информация (loop iterations)

3. **Structured logging:**
   - Использовать поля вместо string formatting
   - `debug!(file = ?file_id, "parsing")` вместо `debug!("parsing {:?}", file_id)`

4. **Guard pattern:**
   - `let _span = ...` для автоматического drop при выходе из scope
   - Имя переменной с `_` чтобы clippy не ругался

## Тестирование

### Unit Tests
Каждый крейт содержит тесты в `tests/` или inline `#[cfg(test)]`

### Integration Tests
Фикстуры в формате:
```
//- /main.bsl
Процедура Тест()
    // код
КонецПроцедуры
```

### Snapshot Tests
Используем `expect-test` для snapshot тестирования
