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
