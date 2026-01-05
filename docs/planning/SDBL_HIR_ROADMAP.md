# SDBL HIR Implementation Roadmap

## Ключевые инсайты (читать первым!)

### 🎯 Lenient Parser + Strict HIR = Лучший UX

**Открытие из диалога:**
```
SDBL Parser (lenient - для IDE):
  SELECT Name N    ← Принимает (implicit alias)
  SELECT Name      ← Принимает (no alias)

Платформа 1С (strict):
  SELECT Name N    ← ❌ Ошибка компиляции!
  SELECT Name AS N ← ✅ Валидно
  SELECT Name      ← ✅ Валидно (no alias в основном запросе)
```

**Почему parser lenient?**
- LSP должен работать **во время набора текста**
- Completion, hover, diagnostics нужны **до завершения ввода**
- Error recovery позволяет продолжить анализ

**Почему HIR strict?**
- HIR = "семантически корректный код"
- Гарантия: если HIR создан → синтаксис валиден
- Диагностики работают только на валидном коде

### 🚀 Incremental Validation Workflow

```
Пользователь пишет:
  "SELECT Name" (неполный)
      ↓
  Parser: ✅ AST создан (lenient)
      ↓
  LSP features: ✅ Работают (completion, hover)
      ↓
  HIR lowering: ❌ Fails (incomplete/invalid)
      ↓
  Diagnostics: ⏸️ Не запускаются

Пользователь дописывает:
  "SELECT Name AS N FROM Table" (валидный)
      ↓
  Parser: ✅ AST обновлен
      ↓
  HIR lowering: ✅ Success!
      ↓
  Salsa: 🔥 Invalidates только ЭТОТ запрос (не все 10!)
      ↓
  Diagnostics: ✅ Запускаются только на новом HIR
```

**Salsa преимущество:**
- Изменился 1 запрос → recompute только его HIR
- Остальные 9 запросов → cache hit
- Диагностики перезапускаются только на changed queries

### 📐 Граница ответственности: AST vs HIR

| Уровень | Что содержит | Для чего |
|---------|-------------|----------|
| **Parser** | Всё (даже ошибки) | IDE features во время набора |
| **AST** | Структура (lenient) | Syntax highlighting, folding |
| **HIR** | Только валидное | Semantic analysis, diagnostics |
| **Diagnostics** | Читают HIR | Type checking, metadata validation |

**Важно:**
- AST может содержать `SdblAlias { as_keyword: None }` (невалидно в 1С)
- HIR lowering ОТВЕРГАЕТ это → diagnostic автоматически
- Не нужно дублировать проверки в parser и diagnostics!

## Мотивация

SDBL (язык запросов 1С) — это встроенный DSL внутри BSL кода. Сейчас SDBL обрабатывается отдельно от BSL HIR, что приводит к:

1. **Дублированию обходов AST** - BSL HIR lowering и SDBL extraction делают отдельные обходы
2. **Отсутствию семантической связи** - BSL HIR не знает что строка это SDBL запрос
3. **Ограниченным возможностям LSP** - нет completion, type checking, go-to-definition для SDBL

**Цель:** Создать полноценный SDBL HIR с интеграцией в BSL HIR для:
- Ускорения диагностик (1 обход AST вместо 2+)
- LSP features (completion, hover, go-to-definition)
- Type checking в запросах
- Metadata integration
- **Incremental validation** через Salsa (1 запрос изменился → 1 recompute)

## Текущая архитектура (что есть)

### Обработка SDBL сейчас

```
┌─────────────────────────────────────────────────────────────────┐
│  Путь 1: BSL HIR (для BSL диагностик)                           │
│                                                                  │
│  parse(file_id) → module_bodies(file_id)                        │
│       ↓                    ↓                                     │
│   BSL AST    →  Lowering → BSL HIR (Body)                       │
│                   ОБХОД #1 ← обходим AST для BSL                │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│  Путь 2: SDBL queries (для SDBL диагностик)                     │
│                                                                  │
│  parse(file_id) → sdbl_queries(file_id)                         │
│       ↓                    ↓                                     │
│   BSL AST    →   extract_and_parse_sdbl_queries()               │
│                   ОБХОД #2 ← обходим AST для SDBL отдельно!     │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Текущая реализация

**File:** `crates/base-db/src/lib.rs`

```rust
#[salsa::tracked(lru = 256)]
pub fn sdbl_queries_in_file(
    db: &dyn Database,
    input: FileTextInput,
) -> Arc<Vec<SdblQueryInfo>> {
    let parse = parse_query(db, input);
    let root = parse.syntax_node();

    // ❌ Отдельный обход BSL AST
    let queries = extract_and_parse_sdbl_queries(&root);

    Arc::new(queries)
}

fn extract_and_parse_sdbl_queries(root: &SyntaxNode) -> Vec<SdblQueryInfo> {
    let mut queries = Vec::new();

    // Обход всего дерева
    for node in root.descendants() {
        if node.kind() == SyntaxKind::LITERAL {
            if let Some(text) = extract_string_content(&node) {
                if looks_like_sdbl(&text) {
                    let sdbl_ast = parser::parse_sdbl(&text);
                    queries.push(SdblQueryInfo::new(
                        node.text_range(),
                        text,
                        Some(sdbl_ast),
                    ));
                }
            }
        }
    }

    queries
}
```

### Как используют диагностики

**File:** `crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs`

```rust
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // ✅ Salsa cache - первый вызов делает обход, остальные читают кеш
    let sdbl_queries = ctx.db.sdbl_queries(ctx.file_id);

    for query_info in sdbl_queries.iter() {
        if query_info.is_valid() {
            // Работаем с SDBL AST
            check_sdbl_ast(&query_info.query_ast);
        }
    }
}
```

### Проблемы

1. **2 обхода BSL AST:**
   - `module_bodies()` обходит AST для BSL HIR
   - `sdbl_queries()` обходит AST для SDBL (отдельно!)

2. **Нет связи BSL ↔ SDBL:**
   - BSL HIR не знает какие `Expr::Literal` это SDBL
   - Нельзя найти SDBL выражение по `ExprId`
   - Нет cross-language analysis

3. **Только syntax-level проверки:**
   - Нет type inference (не знаем типы полей)
   - Нет name resolution (не разрешаем алиасы)
   - Нет metadata integration (не проверяем существование таблиц)

## Категоризация SDBL диагностик

### Category 1: Syntax-only (AST-based)

**Не требуют HIR семантики, проверяют структуру запроса:**

| # | Диагностика | Статус | Что проверяет |
|---|------------|--------|---------------|
| 2 | AssignAliasFieldsInQuery | ✅ Готова | AS keyword обязателен |
| 60 | FullOuterJoinQuery | ✅ Готова | Запрещенные FULL OUTER JOIN |
| 82 | LogicalOrInJoinQuerySection | ✅ Готова | OR в JOIN условиях |
| 83 | LogicalOrInTheWhereSectionOfQuery | ✅ Готова | OR в WHERE |
| 73 | IncorrectUseLikeInQuery | ✅ Готова | Неправильное LIKE |
| - | MultilineStringInQuery | ✅ Готова | Форматирование строк |
| 78 | JoinWithSubQuery | ✅ Готова | Подзапросы в JOIN |
| 56 | FieldsFromJoinsWithoutIsNull | ✅ Готова | Поля из LEFT JOIN без IsNull |

**Итого:** 8 диагностик

**Важно:** Эти диагностики **НЕ ПОТРЕБУЮТ переписывания** при переходе на SDBL HIR!
Они будут работать быстрее (читать HIR вместо AST), но логика останется та же.

### Category 2: Semantic (требуют SDBL HIR)

**Требуют type inference, metadata, name resolution:**

| # | Диагностика | Что требует |
|---|------------|-------------|
| 122 | QueryToMissingMetadata | Проверка существования таблиц в metadata |
| 79 | JoinWithVirtualTable | Понимание виртуальных таблиц 1С |
| 174 | VirtualTableCallWithoutParameters | Metadata схема виртуальных таблиц |
| - | TypeMismatchInQuery (future) | Type inference для полей |
| - | UnknownFieldInQuery (future) | Name resolution + metadata fields |

**Итого:** ~3-5 диагностик

**Важно:** Эти диагностики **ОТЛОЖЕНЫ** до создания SDBL HIR!
Делать их сейчас = двойная работа.

---

## Поэтапный план реализации

## Этап 1: Интеграция SDBL в BSL HIR (2-3 дня)

**Цель:** Объединить обходы AST - собирать SDBL попутно при lowering BSL HIR.

**Статус:** ⚠️ TODO (можно начинать сейчас!)

### Что делаем

#### 1. Расширяем `Body` структуру

**File:** `crates/hir-def/src/body.rs`

```rust
#[derive(Debug)]
pub struct Body {
    pub exprs: Arena<Expr>,
    pub stmts: Arena<Stmt>,
    pub bindings: Arena<Binding>,
    pub params: Box<[BindingId]>,
    pub body_stmts: Box<[StmtId]>,

    /// ✅ NEW: SDBL queries found in this method body.
    /// Maps ExprId (Expr::Literal with SDBL string) to parsed SDBL query info.
    pub sdbl_exprs: Vec<(ExprId, syntax::SdblQueryInfo)>,
}

impl Body {
    pub fn new() -> Self {
        Self {
            exprs: Arena::new(),
            stmts: Arena::new(),
            bindings: Arena::new(),
            params: Box::new([]),
            body_stmts: Box::new([]),
            sdbl_exprs: Vec::new(),  // ✅ NEW
        }
    }

    /// Get all SDBL expressions in this body.
    pub fn sdbl_exprs(&self) -> &[(ExprId, syntax::SdblQueryInfo)] {
        &self.sdbl_exprs
    }

    /// Add SDBL expression (called during lowering).
    pub(crate) fn add_sdbl_expr(&mut self, expr_id: ExprId, query_info: syntax::SdblQueryInfo) {
        self.sdbl_exprs.push((expr_id, query_info));
    }
}
```

#### 2. Модифицируем lowering для сбора SDBL

**File:** `crates/hir-def/src/body/lower.rs`

```rust
impl ExprCollector<'_> {
    fn lower_expr(&mut self, expr: &ast::Expr) -> ExprId {
        let result = match expr {
            ast::Expr::Literal(lit) => {
                let hir_lit = self.lower_literal(lit);

                // ✅ NEW: Check if this is SDBL query
                if let Literal::String(ref s) = hir_lit {
                    if self.looks_like_sdbl(s) {
                        // Parse SDBL
                        let sdbl_ast = parser::parse_sdbl(s);

                        if !sdbl_ast.has_errors() {
                            // Create SdblQueryInfo
                            let query_info = syntax::SdblQueryInfo::new(
                                lit.syntax().text_range(),
                                s.clone(),
                                Some(sdbl_ast),
                            );

                            // Store in body (will add ExprId after allocation)
                            self.pending_sdbl.push((s.clone(), query_info));
                        }
                    }
                }

                Expr::Literal(hir_lit)
            }
            // ... other cases
        };

        let expr_id = self.alloc_expr(result, expr.syntax());

        // ✅ NEW: Associate SDBL with ExprId
        if let Some(idx) = self.pending_sdbl.iter().position(|(s, _)| {
            // Match by string content (hack, but works)
            if let Expr::Literal(Literal::String(ref expr_s)) = self.body.exprs[expr_id] {
                s == expr_s
            } else {
                false
            }
        }) {
            let (_, query_info) = self.pending_sdbl.remove(idx);
            self.body.add_sdbl_expr(expr_id, query_info);
        }

        expr_id
    }

    /// Check if string looks like SDBL query.
    fn looks_like_sdbl(&self, s: &str) -> bool {
        if s.len() < 15 {
            return false;
        }
        let upper = s.to_uppercase();
        upper.contains("SELECT") || upper.contains("ВЫБРАТЬ")
    }
}

struct ExprCollector<'a> {
    // ... existing fields

    /// ✅ NEW: Pending SDBL queries (before ExprId allocation)
    pending_sdbl: Vec<(String, syntax::SdblQueryInfo)>,
}
```

#### 3. Добавляем query для всех SDBL в файле

**File:** `crates/ide-db/src/lib.rs`

```rust
impl RootDatabase for RootDatabaseImpl {
    // ... existing queries

    /// Get all SDBL queries in a file with their ExprId in BSL HIR.
    ///
    /// This query reuses BSL HIR lowering - no separate AST traversal!
    ///
    /// ## Performance
    /// - ✅ Reuses `module_bodies()` lowering (no extra AST walk)
    /// - ✅ Salsa cached (LRU 256)
    /// - ✅ Auto-invalidated when file changes
    ///
    /// ## Usage
    /// ```ignore
    /// let sdbl_queries = ctx.db.all_sdbl_in_file(ctx.file_id);
    /// for (expr_id, query_info) in sdbl_queries.iter() {
    ///     // expr_id - ID in BSL HIR
    ///     // query_info - parsed SDBL AST
    ///     check_sdbl(&query_info.query_ast);
    /// }
    /// ```
    fn all_sdbl_in_file(&self, file_id: FileId) -> Arc<Vec<(ExprId, syntax::SdblQueryInfo)>> {
        all_sdbl_in_file_query(self, file_id)
    }
}

#[salsa::tracked(lru = 256)]
fn all_sdbl_in_file_query(
    db: &dyn salsa::Database,
    file_id: FileId,
) -> Arc<Vec<(ExprId, syntax::SdblQueryInfo)>> {
    let _span = tracing::info_span!("all_sdbl_in_file").entered();

    // ✅ Reuse BSL HIR lowering (already cached!)
    let item_tree = hir_def::item_tree::ItemTree::file_item_tree(db, file_id);

    let mut result = Vec::new();

    // Collect SDBL from all methods
    for method_id in item_tree.all_methods() {
        let body = hir_def::body::Body::body_query(db, method_id);

        // ✅ Read SDBL expressions from Body
        for (expr_id, query_info) in body.sdbl_exprs() {
            result.push((*expr_id, query_info.clone()));
        }
    }

    tracing::debug!(count = result.len(), "Collected SDBL queries from BSL HIR");

    Arc::new(result)
}
```

#### 4. Добавляем RootDatabase trait method

**File:** `crates/base-db/src/lib.rs`

```rust
pub trait SourceDatabase: salsa::Database {
    // ... existing methods

    /// Get all SDBL queries in a file.
    ///
    /// **NEW API:** Replaces `sdbl_queries()` - uses BSL HIR instead of separate AST walk.
    ///
    /// ## Migration from old API
    ///
    /// Old (separate AST walk):
    /// ```ignore
    /// let queries = ctx.db.sdbl_queries(file_id);  // Arc<Vec<SdblQueryInfo>>
    /// ```
    ///
    /// New (from BSL HIR):
    /// ```ignore
    /// let queries = ctx.db.all_sdbl_in_file(file_id);  // Arc<Vec<(ExprId, SdblQueryInfo)>>
    /// for (expr_id, query_info) in queries.iter() {
    ///     // same as before: query_info.query_ast
    /// }
    /// ```
    fn all_sdbl_in_file(&self, file_id: FileId) -> Arc<Vec<(hir_def::ExprId, syntax::SdblQueryInfo)>>;
}
```

#### 5. Мигрируем диагностики на новый API

**File:** `crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs`

```rust
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // OLD:
    // let sdbl_queries = ctx.db.sdbl_queries(ctx.file_id);

    // ✅ NEW: Use BSL HIR-integrated API
    let sdbl_queries = ctx.db.all_sdbl_in_file(ctx.file_id);

    let mut diagnostics = Vec::new();

    // Rest is the same (query_info structure unchanged)
    for (_expr_id, query_info) in sdbl_queries.iter() {
        if !query_info.is_valid() {
            continue;
        }

        // Same as before
        check_sdbl_query(&query_info, &mut diagnostics);
    }

    diagnostics
}
```

### Performance улучшение

**До (2 обхода AST):**
```
File with 10 methods, 5 SDBL queries:

BSL diagnostics:
  parse() → module_bodies()
              ↓
          ОБХОД BSL AST #1 (10 methods lowering)

SDBL diagnostics:
  parse() → sdbl_queries()
              ↓
          ОБХОД BSL AST #2 (find 5 SDBL strings)

Total: 2× AST traversals
```

**После (1 обход AST):**
```
File with 10 methods, 5 SDBL queries:

parse() → module_bodies()
            ↓
        ОБХОД BSL AST (10 methods lowering + find 5 SDBL)
            ↓
    ┌───────┴────────┐
    ↓                ↓
BSL HIR        SDBL queries
               (in Body.sdbl_exprs)

Total: 1× AST traversal
```

**Ожидаемое улучшение:** ~30-50% для файлов с SDBL запросами.

### Deliverables

- [ ] `Body.sdbl_exprs` field added
- [ ] `lower_expr()` collects SDBL during lowering
- [ ] `all_sdbl_in_file()` query implemented
- [ ] All 8 SDBL diagnostics migrated to new API
- [ ] Old `sdbl_queries()` marked as deprecated
- [ ] Tests updated
- [ ] Documentation updated

---

## Этап 2: Syntax-only диагностики (current work)

**Цель:** Завершить все syntax-level SDBL диагностики используя новый API.

**Статус:** ✅ 6/8 готовы, 2 в процессе

### Оставшиеся диагностики

- [x] AssignAliasFieldsInQuery (готова)
- [x] FullOuterJoinQuery (готова)
- [x] LogicalOrInJoinQuerySection (готова)
- [x] LogicalOrInTheWhereSectionOfQuery (готова)
- [x] IncorrectUseLikeInQuery (готова)
- [x] MultilineStringInQuery (готова)
- [x] JoinWithSubQuery (готова)
- [x] FieldsFromJoinsWithoutIsNull (готова)

### Важно

**Эти диагностики НЕ будут переписаны при создании SDBL HIR!**
Они будут работать с SDBL AST (как сейчас), но через новый API:

```rust
// Логика остается та же
for (_expr_id, query_info) in ctx.db.all_sdbl_in_file(file_id).iter() {
    // Work with query_info.query_ast (SDBL AST)
    check_sdbl_patterns(&query_info.query_ast);
}
```

---

## Этап 3: SDBL HIR (будущее, 3-4 недели)

**Цель:** Создать полноценный SDBL HIR для semantic analysis и LSP features.

**Статус:** ⚠️ DEFERRED (после завершения всех 181 диагностики)

### Архитектура SDBL HIR

#### Новый crate: `crates/sdbl-hir/`

```rust
//! SDBL HIR - semantic representation of SDBL queries
//!
//! Provides:
//! - Type inference (field types, expression types)
//! - Name resolution (tables, fields, aliases)
//! - Metadata integration (1C catalogs, documents, registers)
//! - Semantic diagnostics collection

use hir_def::ExprId;
use bsl_metadata::MetadataRef;

/// HIR representation of SDBL query.
///
/// Created by lowering SDBL AST with metadata context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblHir {
    /// SELECT clause with typed fields
    pub select: SelectHir,

    /// FROM clause with resolved tables
    pub from: Vec<TableRef>,

    /// JOIN clauses
    pub joins: Vec<JoinHir>,

    /// WHERE clause
    pub where_clause: Option<ExprHir>,

    /// Semantic diagnostics collected during lowering
    pub diagnostics: Vec<SdblDiagnostic>,
}

/// SELECT field with inferred type
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldHir {
    /// Field expression (column ref, literal, function call, etc.)
    pub expr: ExprHir,

    /// Field alias (if specified with AS)
    pub alias: Option<Name>,

    /// ✅ Inferred type from metadata!
    pub ty: SdblType,

    /// Source range in BSL file
    pub range: TextRange,
}

/// Table reference with metadata link
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    /// Table name (e.g., "Справочник.Валюты")
    pub name: Name,

    /// Table alias (e.g., "AS T1")
    pub alias: Option<Name>,

    /// ✅ Link to 1C metadata!
    pub metadata: Option<MetadataObjectRef>,

    /// ✅ Available fields from metadata (for completion!)
    pub fields: Vec<FieldDef>,

    /// Source range
    pub range: TextRange,
}

/// SDBL type system
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SdblType {
    /// Boolean (ПометкаУдаления, etc.)
    Boolean,

    /// String (Код, Наименование, etc.)
    String,

    /// Number (Количество, Сумма, etc.)
    Number,

    /// Date (Дата, ДатаДокумента, etc.)
    Date,

    /// Reference to metadata object (Ссылка)
    Ref(MetadataObjectKind),

    /// Unknown/inference failed
    Unknown,
}

/// Metadata object reference
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataObjectRef {
    Catalog(Name),              // Справочник.Валюты
    Document(Name),             // Документ.ПоступлениеТоваров
    InformationRegister(Name),  // РегистрСведений.Цены
    AccumulationRegister(Name), // РегистрНакопления.Остатки
    // ... other types
}

/// Semantic diagnostics collected during SDBL HIR lowering
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdblDiagnostic {
    /// Table doesn't exist in metadata
    QueryToMissingMetadata {
        table_name: String,
        range: TextRange,
    },

    /// JOIN with virtual table
    JoinWithVirtualTable {
        table_name: String,
        range: TextRange,
    },

    /// Type mismatch in expression
    TypeMismatch {
        expected: SdblType,
        actual: SdblType,
        range: TextRange,
    },

    /// Unknown field in table
    UnknownField {
        table: String,
        field: String,
        range: TextRange,
    },
}

/// SDBL expression HIR
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprHir {
    /// Column reference (Таблица.Поле or Поле)
    ColumnRef {
        table: Option<Name>,
        column: Name,
        ty: SdblType,  // ✅ Inferred!
    },

    /// Literal value
    Literal {
        value: LiteralValue,
        ty: SdblType,
    },

    /// Binary operation (field = value, field + 10, etc.)
    BinaryOp {
        lhs: Box<ExprHir>,
        op: BinaryOp,
        rhs: Box<ExprHir>,
        ty: SdblType,  // ✅ Result type!
    },

    /// Function call (ВЫРАЗИТЬ(), ДАТАВРЕМЯ(), etc.)
    FunctionCall {
        function: Name,
        args: Vec<ExprHir>,
        ty: SdblType,  // ✅ Return type!
    },
}
```

#### Lowering: SDBL AST → SDBL HIR

**File:** `crates/sdbl-hir/src/lower.rs`

```rust
/// Lower SDBL AST to HIR with metadata context.
///
/// This performs:
/// 1. Name resolution (tables, fields, aliases)
/// 2. Type inference (from metadata field types)
/// 3. Semantic validation (collect diagnostics)
pub fn lower_sdbl_to_hir(
    db: &dyn RootDatabase,
    sdbl_ast: &Parse<SyntaxNode>,
    bsl_expr_id: ExprId,  // Link to BSL HIR
) -> SdblHir {
    let metadata = db.metadata();

    let mut ctx = LoweringContext::new(db, metadata);

    // Lower AST → HIR
    let select = ctx.lower_select(&sdbl_ast);
    let from = ctx.lower_from(&sdbl_ast);
    let joins = ctx.lower_joins(&sdbl_ast);
    let where_clause = ctx.lower_where(&sdbl_ast);

    // Collect diagnostics
    let diagnostics = ctx.diagnostics;

    SdblHir {
        select,
        from,
        joins,
        where_clause,
        diagnostics,
    }
}

struct LoweringContext<'a> {
    db: &'a dyn RootDatabase,
    metadata: Arc<Configuration>,

    /// Current scope (tables available for name resolution)
    scope: Scope,

    /// Collected semantic diagnostics
    diagnostics: Vec<SdblDiagnostic>,
}

impl<'a> LoweringContext<'a> {
    fn lower_table_ref(&mut self, ast: &ast::SdblTableRef) -> TableRef {
        let name = ast.name();
        let alias = ast.alias();

        // ✅ Resolve in metadata
        let metadata_obj = self.metadata.find_object(&name);

        if metadata_obj.is_none() {
            // ✅ Collect diagnostic during lowering!
            self.diagnostics.push(SdblDiagnostic::QueryToMissingMetadata {
                table_name: name.to_string(),
                range: ast.syntax().text_range(),
            });
        }

        // ✅ Get available fields from metadata
        let fields = metadata_obj
            .map(|obj| obj.fields())
            .unwrap_or_default();

        TableRef {
            name,
            alias,
            metadata: metadata_obj.map(|obj| obj.to_ref()),
            fields,
            range: ast.syntax().text_range(),
        }
    }

    fn lower_column_ref(&mut self, ast: &ast::SdblColumnRef) -> ExprHir {
        let table = ast.table();
        let column = ast.column();

        // ✅ Resolve column type from scope
        let ty = self.scope.resolve_column_type(&table, &column)
            .unwrap_or(SdblType::Unknown);

        if ty == SdblType::Unknown {
            // Unknown field - collect diagnostic
            self.diagnostics.push(SdblDiagnostic::UnknownField {
                table: table.unwrap_or_default(),
                field: column.to_string(),
                range: ast.syntax().text_range(),
            });
        }

        ExprHir::ColumnRef { table, column, ty }
    }
}
```

#### Salsa integration

**File:** `crates/ide-db/src/lib.rs`

```rust
impl RootDatabase for RootDatabaseImpl {
    /// Get SDBL HIR for all queries in a file.
    ///
    /// This performs semantic analysis:
    /// - Type inference from metadata
    /// - Name resolution (tables, fields, aliases)
    /// - Semantic diagnostics collection
    fn sdbl_hir_in_file(&self, file_id: FileId) -> Arc<Vec<(ExprId, Arc<SdblHir>)>> {
        sdbl_hir_in_file_query(self, file_id)
    }
}

#[salsa::tracked(lru = 256)]
fn sdbl_hir_in_file_query(
    db: &dyn RootDatabase,
    file_id: FileId,
) -> Arc<Vec<(ExprId, Arc<SdblHir>)>> {
    let _span = tracing::info_span!("sdbl_hir_in_file").entered();

    // ✅ Get SDBL queries from BSL HIR (Etap 1 result!)
    let sdbl_queries = db.all_sdbl_in_file(file_id);

    // ✅ Get metadata (Salsa cached!)
    let metadata = db.metadata();

    let mut result = Vec::new();

    for (expr_id, query_info) in sdbl_queries.iter() {
        if let Some(ref sdbl_ast) = query_info.query_ast {
            // ✅ Lower SDBL AST → SDBL HIR with metadata
            let sdbl_hir = sdbl_hir::lower_sdbl_to_hir(
                db,
                sdbl_ast,
                *expr_id,
            );

            result.push((*expr_id, Arc::new(sdbl_hir)));
        }
    }

    tracing::debug!(count = result.len(), "Lowered SDBL queries to HIR");

    Arc::new(result)
}
```

### Semantic диагностики через SDBL HIR

**File:** `crates/ide-diagnostics/src/handlers/query_to_missing_metadata.rs`

```rust
//! QueryToMissingMetadata diagnostic.
//!
//! Collected during SDBL HIR lowering when table doesn't exist in metadata.

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::QueryToMissingMetadata) {
        return Vec::new();
    }

    // ✅ Get SDBL HIR (includes semantic diagnostics!)
    let sdbl_hirs = ctx.db.sdbl_hir_in_file(ctx.file_id);

    let mut diagnostics = Vec::new();

    for (_expr_id, sdbl_hir) in sdbl_hirs.iter() {
        // ✅ Diagnostics already collected during lowering!
        for diag in sdbl_hir.diagnostics.iter() {
            if let SdblDiagnostic::QueryToMissingMetadata { table_name, range } = diag {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::QueryToMissingMetadata,
                    message: format!(
                        "Таблица '{}' не найдена в метаданных",
                        table_name
                    ),
                    severity: Severity::Error,
                    range: *range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }

    diagnostics
}
```

### LSP Features

#### Completion

**File:** `crates/ide/src/completion/sdbl.rs`

```rust
/// Provide completions for SDBL queries.
pub fn sdbl_completion(
    db: &dyn RootDatabase,
    position: FilePosition,
) -> Vec<CompletionItem> {
    let sdbl_hirs = db.sdbl_hir_in_file(position.file_id);

    // Find SDBL query at position
    let Some(sdbl_hir) = find_sdbl_at_position(&sdbl_hirs, position.offset) else {
        return Vec::new();
    };

    let context = determine_completion_context(sdbl_hir, position.offset);

    match context {
        CompletionContext::AfterSelect => {
            // ✅ Suggest fields from tables in FROM/JOIN
            sdbl_hir.from.iter()
                .chain(sdbl_hir.joins.iter().map(|j| &j.table))
                .flat_map(|table| table.fields.iter())
                .map(|field| CompletionItem {
                    label: field.name.to_string(),
                    kind: CompletionItemKind::Field,
                    detail: Some(format!("Type: {:?}", field.ty)),
                    ..Default::default()
                })
                .collect()
        }

        CompletionContext::AfterFrom => {
            // ✅ Suggest tables from metadata
            let metadata = db.metadata();
            metadata.all_objects()
                .map(|obj| CompletionItem {
                    label: obj.full_name(),  // "Справочник.Валюты"
                    kind: CompletionItemKind::Class,
                    detail: Some(obj.kind().to_string()),
                    ..Default::default()
                })
                .collect()
        }

        // ... other contexts
    }
}
```

#### Hover

```rust
pub fn sdbl_hover(
    db: &dyn RootDatabase,
    position: FilePosition,
) -> Option<Hover> {
    let sdbl_hirs = db.sdbl_hir_in_file(position.file_id);
    let sdbl_hir = find_sdbl_at_position(&sdbl_hirs, position.offset)?;

    // Find field/table at position
    let field = find_field_at_position(sdbl_hir, position.offset)?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!(
                "**Field:** {}\n**Type:** {:?}\n**Table:** {}",
                field.alias.as_ref().unwrap_or(&field.expr.column_name()),
                field.ty,
                field.expr.table_name()?,
            ),
        }),
        range: Some(field.range),
    })
}
```

#### Go to Definition

```rust
pub fn sdbl_goto_definition(
    db: &dyn RootDatabase,
    position: FilePosition,
) -> Option<Vec<NavigationTarget>> {
    let sdbl_hirs = db.sdbl_hir_in_file(position.file_id);
    let sdbl_hir = find_sdbl_at_position(&sdbl_hirs, position.offset)?;

    // Find table reference at position
    let table_ref = find_table_at_position(sdbl_hir, position.offset)?;

    // ✅ Navigate to metadata XML!
    let metadata_obj = table_ref.metadata.as_ref()?;
    let metadata_file = db.metadata_file_for_object(metadata_obj)?;

    Some(vec![NavigationTarget {
        file_id: metadata_file.file_id,
        full_range: metadata_file.definition_range,
        focus_range: None,
        name: table_ref.name.to_string(),
        kind: SymbolKind::Class,
        container_name: None,
        description: None,
    }])
}
```

### Deliverables (Этап 3)

- [ ] New crate: `crates/sdbl-hir/`
- [ ] `SdblHir` structures defined
- [ ] Lowering: SDBL AST → SDBL HIR
- [ ] Type inference from metadata
- [ ] Name resolution (tables, fields, aliases)
- [ ] Semantic diagnostics collection
- [ ] `sdbl_hir_in_file()` Salsa query
- [ ] Migrate semantic diagnostics:
  - [ ] QueryToMissingMetadata
  - [ ] JoinWithVirtualTable
  - [ ] VirtualTableCallWithoutParameters
- [ ] LSP features:
  - [ ] Completion (tables, fields)
  - [ ] Hover (type info)
  - [ ] Go to definition (metadata)
- [ ] Documentation
- [ ] Tests

---

## Performance Benefits

### Этап 1 improvements

**Файл с 10 методами, 5 SDBL запросами:**

| Метрика | До (AST-only) | После (BSL HIR integration) | Улучшение |
|---------|--------------|----------------------------|-----------|
| BSL AST traversals | 2× (BSL HIR + SDBL extraction) | 1× (unified) | **2x меньше** |
| SDBL parsing | 5× (per query) | 5× (same) | Без изменений |
| Cache efficiency | 2 separate Salsa queries | 1 unified query | Лучше locality |
| Memory overhead | 2 caches | 1 cache | Меньше |

**Ожидаемое улучшение:** 30-50% для файлов с SDBL.

### Этап 3 improvements

**5 SDBL диагностик на файле:**

| Метрика | AST-only | С SDBL HIR | Улучшение |
|---------|----------|-----------|-----------|
| Semantic analysis | 5× (per diagnostic) | 1× (lowering) | **5x меньше** |
| Type resolution | Manual pattern matching | Inferred in HIR | Автоматически |
| Metadata queries | Repeated lookups | Cached in TableRef | Без дублирования |

---

## Migration Path

### Phase 1: Этап 1 (сейчас)

1. ✅ Implement `Body.sdbl_exprs`
2. ✅ Modify `lower_method()` to collect SDBL
3. ✅ Add `all_sdbl_in_file()` query
4. ✅ Migrate 8 syntax-only diagnostics
5. ✅ Deprecate old `sdbl_queries()` API

### Phase 2: Диагностики (текущая работа)

1. ✅ Complete all 181 BSL diagnostics
2. ✅ All syntax-only SDBL diagnostics
3. ⚠️ Skip semantic SDBL diagnostics (deferred to Этап 3)

### Phase 3: Этап 3 (после диагностик)

1. ⚠️ Create `crates/sdbl-hir/`
2. ⚠️ Implement lowering + type inference
3. ⚠️ Add semantic diagnostics (3-5 new)
4. ⚠️ Implement LSP features

---

## Testing Strategy

### Этап 1 tests

```rust
#[test]
fn test_sdbl_collected_in_hir() {
    let code = r#"
Процедура Тест()
    Запрос = "SELECT Ссылка FROM Справочник.Валюты";
    Результат = Запрос.Выполнить();
КонецПроцедуры
"#;

    let db = TestDatabase::new();
    let file_id = db.add_file("test.bsl", code);

    let sdbl_queries = db.all_sdbl_in_file(file_id);

    assert_eq!(sdbl_queries.len(), 1);

    let (expr_id, query_info) = &sdbl_queries[0];
    assert!(query_info.is_valid());
    assert!(query_info.query_text.contains("SELECT"));

    // ✅ Verify ExprId links to BSL HIR
    let bodies = db.module_bodies(file_id);
    let body = bodies.get_method("Тест").unwrap();
    assert!(body.exprs.contains(*expr_id));
}
```

### Этап 3 tests

```rust
#[test]
fn test_sdbl_hir_type_inference() {
    let code = r#"
Процедура Тест()
    Запрос = "SELECT Код, ПометкаУдаления FROM Справочник.Валюты";
КонецПроцедуры
"#;

    let db = TestDatabase::with_metadata();
    let file_id = db.add_file("test.bsl", code);

    let sdbl_hirs = db.sdbl_hir_in_file(file_id);
    let (_expr_id, sdbl_hir) = &sdbl_hirs[0];

    // ✅ Type inference from metadata
    assert_eq!(sdbl_hir.select.fields.len(), 2);

    let kod_field = &sdbl_hir.select.fields[0];
    assert_eq!(kod_field.ty, SdblType::String);

    let deletion_mark = &sdbl_hir.select.fields[1];
    assert_eq!(deletion_mark.ty, SdblType::Boolean);
}

#[test]
fn test_query_to_missing_metadata_diagnostic() {
    let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM Справочник.НесуществующийСправочник";
КонецПроцедуры
"#;

    let db = TestDatabase::with_metadata();
    let file_id = db.add_file("test.bsl", code);

    let sdbl_hirs = db.sdbl_hir_in_file(file_id);
    let (_expr_id, sdbl_hir) = &sdbl_hirs[0];

    // ✅ Diagnostic collected during lowering
    assert_eq!(sdbl_hir.diagnostics.len(), 1);

    let diag = &sdbl_hir.diagnostics[0];
    assert!(matches!(diag, SdblDiagnostic::QueryToMissingMetadata { .. }));
}
```

---

## References

### Related Documents

- `docs/architecture/ARCHITECTURE.md` - Overall architecture
- `docs/planning/HIR_DIAGNOSTICS_ROADMAP.md` - BSL HIR diagnostics
- `docs/planning/DIAGNOSTICS_MIGRATION.md` - Diagnostic migration plan

### Source Projects

- `~/src/lsp/rust-analyzer/` - HIR architecture reference
- `~/src/lsp/bsl-language-server/` - SDBL diagnostics compatibility
- `~/src/lsp/salsa/` - Incremental computation framework

### Key Files

- `crates/hir-def/src/body.rs` - BSL HIR Body structure
- `crates/hir-def/src/body/lower.rs` - BSL HIR lowering
- `crates/base-db/src/lib.rs` - Current `sdbl_queries()` implementation
- `crates/syntax/src/sdbl_query.rs` - `SdblQueryInfo` structure

---

## Decision Log

### 2026-01-05: Decided on 3-stage approach

**Decision:** Split SDBL HIR into 3 stages instead of doing everything at once.

**Rationale:**
1. Этап 1 gives immediate performance benefits (1 AST walk vs 2)
2. Allows completing BSL diagnostics first (priority)
3. SDBL HIR semantic features needed only for LSP (later)
4. Avoids rewriting syntax-only diagnostics twice

**Alternatives considered:**
- ❌ Do full SDBL HIR now - blocks diagnostic work
- ❌ Keep AST-only approach - misses optimization opportunity
- ✅ Staged approach - best of both worlds

### 2026-01-05: Link SDBL to BSL HIR via ExprId

**Decision:** Store `Vec<(ExprId, SdblQueryInfo)>` in `Body`.

**Rationale:**
1. Enables cross-language analysis (BSL ↔ SDBL)
2. Can navigate from BSL HIR to SDBL query
3. Future: can use BSL scope for SDBL parameter resolution

**Alternatives considered:**
- ❌ Separate storage - no linkage
- ❌ Store only ExprId set - lose query info
- ✅ Store pairs - full bidirectional link

---

**Status:** Ready for Этап 1 implementation
**Next Action:** Implement `Body.sdbl_exprs` and modify lowering
**ETA:** 2-3 days for Этап 1
