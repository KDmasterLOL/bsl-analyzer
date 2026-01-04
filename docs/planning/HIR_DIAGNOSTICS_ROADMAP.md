# HIR-based Diagnostics Roadmap

## Цель

Перейти от архитектуры bsl-language-server (90+ отдельных AST traversals) к архитектуре rust-analyzer (диагностики как побочный продукт семантического анализа).

**Текущее состояние:** ~27s на 6.5K файлов (90× O(n) traversals)
**Целевое состояние:** ~10-15s (1× traversal + Salsa caching)

## Архитектурное сравнение

```
bsl-language-server (текущий подход):
┌─────────────────────────────────────────────────────┐
│  Parse (cached)                                      │
│       ↓                                              │
│  Diagnostic 1: traverse AST → find issues            │
│  Diagnostic 2: traverse AST → find issues            │
│  ...                                                 │
│  Diagnostic 90: traverse AST → find issues           │
│       ↓                                              │
│  Collect all diagnostics                             │
└─────────────────────────────────────────────────────┘
   = 90 × O(n) traversals

rust-analyzer (целевой подход):
┌─────────────────────────────────────────────────────┐
│  Parse (cached)                                      │
│       ↓                                              │
│  Lower to ItemTree (signatures) → emit diagnostics   │
│       ↓                                              │
│  Lower to Body (expressions) → emit diagnostics      │
│       ↓                                              │
│  Type inference → emit diagnostics                   │
│       ↓                                              │
│  Body validation → emit diagnostics                  │
│       ↓                                              │
│  All diagnostics collected as byproduct              │
└─────────────────────────────────────────────────────┘
   = 1 × O(n) traversal, all cached by Salsa
```

## Текущий HIR (что уже есть)

```
crates/hir-def/src/
├── item_tree.rs      ✅ ItemTree (signatures)
├── symbol_tree.rs    ✅ SymbolTree (fast lookup)
├── scope.rs          ✅ Scope handling
├── resolver.rs       ✅ Name resolution (partial)
├── ty.rs             ✅ Type definitions
├── ty/infer.rs       ✅ Type inference (Phase 1)
├── body.rs           ✅ Body + BodySourceMap + BodyDiagnostic (Phase 1-3)
├── body/lower.rs     ✅ AST → Body lowering (Phase 1)
└── hir.rs            ✅ Expr, Stmt, Binding enums (Phase 1)

crates/hir/src/
└── lib.rs            ✅ High-level API (Module, Method, Variable)

crates/ide-diagnostics/src/
├── lib.rs            ✅ collect_hir_diagnostics() + dispatch_hir_diagnostic()
└── handlers/
    ├── function_should_have_return.rs ✅ from_hir()
    ├── empty_code_block.rs            ✅ from_hir()
    ├── magic_number.rs                ✅ from_hir()
    └── self_assign.rs                 ✅ from_hir()
```

## Статус реализации

| Phase | Описание | Статус |
|-------|----------|--------|
| Phase 1 | Body Lowering | ✅ Завершена |
| Phase 2 | SourceMap | ✅ Завершена |
| Phase 3 | Diagnostics Infrastructure | ✅ Завершена |
| Phase 4 | Migrate Tier 1 Diagnostics | ✅ Частично (FunctionShouldHaveReturn, EmptyCodeBlock, MagicNumber, SelfAssign) |
| Phase 5 | Cleanup + Архитектурный рефакторинг | ✅ Завершена |
| Phase 6 | UnreachableCode | ⏳ Следующий |
| Phase 7 | CFG из Body | ⏳ Планируется |
| Phase 8 | Body Validation Pass | ⏳ Планируется |

### Архитектурный рефакторинг (Phase 5)

Выполнено:
1. Удален файл `hir_diagnostics.rs` (временный прототип с диагностиками в одном файле)
2. Каждая HIR диагностика теперь в своём отдельном файле с функцией `from_hir()`
3. Dispatch логика добавлена в `lib.rs` (`collect_hir_diagnostics` + `dispatch_hir_diagnostic`)
4. Создан тестовый helper `check_hir_diagnostic()` в `test_utils.rs`

Это соответствует архитектуре rust-analyzer, где каждая диагностика имеет свой handler file.

### Мигрированные диагностики (через HIR)

| Диагностика | Статус | Примечание |
|-------------|--------|------------|
| FunctionShouldHaveReturn | ✅ Полностью | AST версия удалена |
| EmptyCodeBlock | ✅ В lowering | Проверяется при lowering |
| MagicNumber | ✅ В lowering | Проверяется при lowering литералов |
| SelfAssign | ✅ В lowering | Проверяется при lowering assignment |
| UnreachableCode | ⏳ Enum готов | Требует CFG |
| MissingReturn | ⏳ Enum готов | Требует CFG |
| UnusedVariable | ⏳ Enum готов | Требует usage tracking |
| DeprecatedMethod | ⏳ Enum готов | Требует metadata |

## Что нужно добавить

### Phase 1: Body Lowering (2-3 дня)

**Цель:** Преобразовать тела методов в HIR-выражения

```rust
// Новая структура: Body
pub struct Body {
    pub exprs: Arena<Expr>,
    pub stmts: Arena<Stmt>,
    pub params: Vec<ParamId>,
    pub body_expr: ExprId,  // корневой statement list
}

// HIR выражения (не AST!)
pub enum Expr {
    Literal(Literal),
    Path(Path),
    BinaryOp { lhs: ExprId, rhs: ExprId, op: BinaryOp },
    UnaryOp { expr: ExprId, op: UnaryOp },
    Call { callee: ExprId, args: Vec<ExprId> },
    MethodCall { receiver: ExprId, method: Name, args: Vec<ExprId> },
    Index { base: ExprId, index: ExprId },
    Field { base: ExprId, field: Name },
    If { condition: ExprId, then_branch: ExprId, else_branch: Option<ExprId> },
    // ...
}

pub enum Stmt {
    Expr(ExprId),
    Assign { target: ExprId, value: ExprId },
    Return { value: Option<ExprId> },
    // ...
}
```

**Файлы:**
- `crates/hir-def/src/body.rs` — Body + Arena structures
- `crates/hir-def/src/body/lower.rs` — AST → Body lowering

### Phase 2: SourceMap (1 день)

**Цель:** Связать HIR с исходным AST для диагностик

```rust
// Маппинг HIR ↔ AST
pub struct BodySourceMap {
    expr_map: FxHashMap<ExprId, AstPtr<ast::Expr>>,
    stmt_map: FxHashMap<StmtId, AstPtr<ast::Stmt>>,
    // Обратный маппинг для go-to-definition
    expr_map_back: FxHashMap<AstPtr<ast::Expr>, ExprId>,
}
```

### Phase 3: Diagnostics Infrastructure (2 дня)

**Цель:** Инфраструктура для сбора диагностик

```rust
// Диагностики уровня определений
#[derive(Debug, Clone)]
pub enum DefDiagnostic {
    DuplicateMethod { name: Name, first: TextRange, second: TextRange },
    UnresolvedImport { path: Path },
    // ...
}

// Диагностики уровня тела
#[derive(Debug, Clone)]
pub enum BodyDiagnostic {
    UnresolvedVariable { name: Name, range: TextRange },
    TypeMismatch { expected: Ty, actual: Ty, range: TextRange },
    MissingReturn { range: TextRange },
    DeprecatedMethod { name: Name, range: TextRange },
    // ...
}

// Salsa query с диагностиками
#[salsa::tracked]
pub fn body_with_diagnostics(
    db: &dyn DefDatabase,
    method: MethodId,
) -> (Arc<Body>, Arc<Vec<BodyDiagnostic>>);
```

### Phase 4: Migrate Tier 1 Diagnostics (3-4 дня)

**Диагностики для миграции (простые, syntax-level):**

| Диагностика | Текущий подход | HIR подход |
|-------------|----------------|------------|
| `FunctionShouldHaveReturn` | traverse + find RETURN_STMT | Body lowering: check all paths |
| `UnreachableCode` | CFG analysis | Body + CFG |
| `MethodSize` | line counting | Body.stmt_count() |
| `EmptyCodeBlock` | find IF/WHILE/FOR | Body lowering: emit if empty |
| `NestedStatements` | recursive traverse | Body: track depth during lowering |
| `MagicNumber` | find literals | Body: check Expr::Literal |
| `DeprecatedMethods` | find CALL_STMT | Body: check Expr::Call/MethodCall |

**Подход миграции:**
1. Добавить эмит диагностики в lowering
2. Удалить старый handler
3. Проверить тесты

### Phase 5: Migrate Tier 2 Diagnostics (4-5 дней)

**Диагностики требующие семантики:**

| Диагностика | Требует |
|-------------|---------|
| `UnusedLocalVariable` | Scope + usage tracking |
| `SelfAssign` | Expression equality |
| `CreateQueryInCycle` | Loop detection + call analysis |
| `BeginTransactionBeforeTryCatch` | Control flow |

### Phase 6: Body Validation (2-3 дня)

**Отдельный проход валидации после lowering:**

```rust
pub fn validate_body(
    db: &dyn DefDatabase,
    body: &Body,
    source_map: &BodySourceMap,
) -> Vec<BodyDiagnostic> {
    let mut diagnostics = Vec::new();

    // Check all return paths
    check_return_paths(body, &mut diagnostics);

    // Check unused variables
    check_unused_variables(body, &mut diagnostics);

    // Check deprecated calls
    check_deprecated_calls(body, &mut diagnostics);

    diagnostics
}
```

## Этапы реализации

### Iteration 1: Foundation (1 неделя) ✅
- [x] Body + Arena structures
- [x] Basic lowering (literals, binary ops, calls)
- [x] SourceMap
- [x] Salsa integration (module_bodies query)

### Iteration 2: Core Lowering (1 неделя) ✅
- [x] All statement types
- [x] All expression types
- [x] Control flow (if/while/for/try)
- [x] Method calls

### Iteration 3: Diagnostics Migration (2 недели) ⏳
- [x] BodyDiagnostic infrastructure
- [x] FunctionShouldHaveReturn (мигрирована на HIR, AST версия удалена)
- [x] EmptyCodeBlock
- [x] MagicNumber
- [x] SelfAssign
- [ ] UnreachableCode (требует CFG)
- [ ] MissingReturn/AllFunctionPathMustHaveReturn (требует CFG)
- [ ] UnusedVariable (требует usage tracking)
- [ ] Migrate remaining diagnostics

### Iteration 4: Optimization (1 неделя)
- [ ] CFG из Body (Phase 7)
- [ ] Body Validation Pass (Phase 8)
- [ ] Benchmark comparisons
- [ ] Memory profiling
- [ ] Remove old diagnostic handlers

## Ожидаемые результаты

| Метрика | До | После |
|---------|-----|-------|
| Время анализа (6.5K файлов) | ~27s | ~10-15s |
| AST traversals per file | ~90 | 1 |
| Salsa cache hit rate | Low | High |
| Incremental re-analysis | Full | Targeted |

## Риски и митигация

1. **Сложность lowering** — BSL проще Rust, нет макросов/generics
2. **Совместимость диагностик** — строгое тестирование, сравнение с Java
3. **Время разработки** — итеративный подход, каждая фаза deployable

## Первый шаг: Body + Lowering (детальный план)

### Файловая структура

```
crates/hir-def/src/
├── body.rs              # Body, BodySourceMap
├── body/
│   ├── lower.rs         # AST → Body lowering
│   └── scope.rs         # Local scope tracking
├── hir.rs               # Expr, Stmt enums
└── diagnostics.rs       # DefDiagnostic, BodyDiagnostic
```

### Шаг 1.1: HIR Expressions (hir.rs)

```rust
use la_arena::{Arena, Idx};

pub type ExprId = Idx<Expr>;
pub type StmtId = Idx<Stmt>;
pub type BindingId = Idx<Binding>;

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Number(f64),
    String(String),
    Date(String),
    Bool(bool),
    Undefined,
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Missing,  // Placeholder for parse errors
    Literal(Literal),
    Path(Name),
    BinaryOp { lhs: ExprId, rhs: ExprId, op: BinaryOp },
    UnaryOp { expr: ExprId, op: UnaryOp },
    Ternary { condition: ExprId, then_expr: ExprId, else_expr: ExprId },
    Call { callee: ExprId, args: Box<[ExprId]> },
    MethodCall { receiver: ExprId, method: Name, args: Box<[ExprId]> },
    Index { base: ExprId, index: ExprId },
    Field { base: ExprId, field: Name },
    New { type_name: Name, args: Box<[ExprId]> },
    Array(Box<[ExprId]>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Expr(ExprId),
    Assign { target: ExprId, value: ExprId },
    VarDecl { bindings: Box<[BindingId]> },
    If { condition: ExprId, then_branch: Box<[StmtId]>, else_branch: Option<Box<[StmtId]>> },
    While { condition: ExprId, body: Box<[StmtId]> },
    For { var: BindingId, from: ExprId, to: ExprId, body: Box<[StmtId]> },
    ForEach { var: BindingId, collection: ExprId, body: Box<[StmtId]> },
    Try { body: Box<[StmtId]>, except: Box<[StmtId]> },
    Return { value: Option<ExprId> },
    Raise { value: Option<ExprId> },
    Break,
    Continue,
    Goto(Name),
    Label(Name),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub name: Name,
    pub is_val: bool,  // Знач параметр
}
```

### Шаг 1.2: Body Structure (body.rs)

```rust
use la_arena::Arena;
use crate::hir::{Expr, ExprId, Stmt, StmtId, Binding, BindingId};

#[derive(Debug, PartialEq, Eq)]
pub struct Body {
    pub exprs: Arena<Expr>,
    pub stmts: Arena<Stmt>,
    pub bindings: Arena<Binding>,
    pub params: Box<[BindingId]>,
    pub body_stmts: Box<[StmtId]>,
}

#[derive(Debug, Default)]
pub struct BodySourceMap {
    pub expr_map: FxHashMap<ExprId, AstPtr<ast::Expr>>,
    pub stmt_map: FxHashMap<StmtId, AstPtr<ast::Stmt>>,
    pub expr_map_back: FxHashMap<AstPtr<ast::Expr>, ExprId>,
}
```

### Шаг 1.3: Lowering (body/lower.rs)

```rust
pub struct LoweringContext<'a> {
    db: &'a dyn DefDatabase,
    body: Body,
    source_map: BodySourceMap,
    diagnostics: Vec<BodyDiagnostic>,
}

impl<'a> LoweringContext<'a> {
    pub fn lower_method(db: &'a dyn DefDatabase, method: &ast::Method)
        -> (Body, BodySourceMap, Vec<BodyDiagnostic>)
    {
        let mut ctx = LoweringContext::new(db);

        // Lower parameters
        if let Some(params) = method.param_list() {
            for param in params.params() {
                ctx.lower_param(param);
            }
        }

        // Lower body
        if let Some(stmt_list) = method.stmt_list() {
            for stmt in stmt_list.statements() {
                ctx.lower_stmt(stmt);
            }
        }

        (ctx.body, ctx.source_map, ctx.diagnostics)
    }

    fn lower_expr(&mut self, expr: ast::Expr) -> ExprId {
        let hir_expr = match &expr {
            ast::Expr::Literal(lit) => self.lower_literal(lit),
            ast::Expr::BinaryExpr(bin) => self.lower_binary(bin),
            ast::Expr::CallExpr(call) => self.lower_call(call),
            // ... other cases
            _ => Expr::Missing,
        };

        let id = self.body.exprs.alloc(hir_expr);
        self.source_map.expr_map.insert(id, AstPtr::new(&expr));
        id
    }

    fn lower_call(&mut self, call: &ast::CallExpr) -> Expr {
        let callee = call.callee().map(|e| self.lower_expr(e));

        // EMIT DIAGNOSTIC: Check for deprecated method
        if let Some(name) = self.extract_method_name(&call) {
            if is_deprecated_method(&name) {
                self.diagnostics.push(BodyDiagnostic::DeprecatedMethod {
                    name: name.clone(),
                    range: call.syntax().text_range(),
                });
            }
        }

        Expr::Call {
            callee: callee.unwrap_or_else(|| self.missing_expr()),
            args: self.lower_args(call.arg_list()),
        }
    }
}
```

### Шаг 1.4: Salsa Query

```rust
// В crates/hir-def/src/lib.rs

#[salsa::tracked]
pub fn body_with_source_map(
    db: &dyn DefDatabase,
    method: MethodId,
) -> (Arc<Body>, Arc<BodySourceMap>);

#[salsa::tracked]
pub fn body_diagnostics(
    db: &dyn DefDatabase,
    method: MethodId,
) -> Arc<Vec<BodyDiagnostic>>;
```

## Ссылки

- rust-analyzer/crates/hir-def/src/expr_store/body.rs — Body structure
- rust-analyzer/crates/hir-def/src/expr_store/lower.rs — lowering
- rust-analyzer/crates/hir-def/src/hir.rs — Expr, Stmt enums
- rust-analyzer/crates/hir/src/diagnostics.rs — diagnostic types
- la_arena crate — Arena allocator for HIR nodes
