//! DuplicatedInsertionIntoCollection diagnostic.
//!
//! Detects duplicate insertions of the same value into a collection.
//!
//! ## Why?
//! Duplicate insertions are likely errors:
//! - Same value inserted twice (copy-paste error)
//! - Logic mistake
//! - Unnecessary operations
//!
//! ## Bad practice
//! ```bsl
//! Массив.Добавить(Значение1);
//! Массив.Добавить(Значение1);  // Duplicate!
//!
//! Соответствие.Вставить("Ключ1", Значение);
//! Соответствие.Вставить("Ключ1", Значение);  // Duplicate key!
//! ```
//!
//! ## Good practice
//! ```bsl
//! Массив.Добавить(Значение1);
//! Массив.Добавить(Значение2);  // Different values
//!
//! // Or if intentional, use loop:
//! Для Индекс = 1 По 3 Цикл
//!     Массив.Добавить(ЗначениеПоУмолчанию);
//! КонецЦикла;
//! ```
//!
//! ## Configuration
//! - `isAllowedMethodADD` (boolean, default: true) - If false, only Вставить/Insert checked
//!
//! ## Implementation
//!
//! This diagnostic uses HIR-based post-analysis for structural expression comparison.
//! Instead of regex-based text normalization, it compares HIR expression trees directly.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsConfig, DiagnosticsContext, Severity};
use hir::{Body, BodySourceMap};
use hir_def::hir::{Expr, ExprId, Literal, Stmt, StmtId};
use hir_def::Name;
use ide_db::TextRange;
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use unicase::UniCase;

/// Check a HIR body for duplicated insertions.
///
/// This is a post-HIR analysis function that examines the body after lowering.
/// It tracks variable generations and detects duplicate insertions into collections.
pub fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    config: &DiagnosticsConfig,
) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("DuplicatedInsertionIntoCollection::check_body").entered();

    if config.is_disabled(DiagnosticCode::DuplicatedInsertionIntoCollection) {
        return Vec::new();
    }

    let allow_add = config
        .get_bool(DiagnosticCode::DuplicatedInsertionIntoCollection, "isAllowedMethodADD")
        .unwrap_or(true);

    let mut tracker = InsertionTracker::new(body);
    let mut diagnostics = Vec::new();

    check_stmt_list(
        body,
        source_map,
        &body.body_stmts,
        &mut tracker,
        &mut diagnostics,
        0,
        allow_add,
    );
    tracker.report_duplicates(&mut diagnostics, 0);

    tracing::debug!(count = diagnostics.len(), "diagnostics found");
    diagnostics
}

/// Normalized expression for structural comparison.
///
/// This replaces the regex-based text normalization with proper structural comparison.
/// Expressions are normalized by incorporating variable generations into Path references.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum NormalizedExpr {
    /// Literal value (string, number, etc.)
    Literal(NormalizedLiteral),
    /// Variable reference with generation number
    Path { name: UniCase<String>, generation: usize },
    /// Field access: base.field
    Field { base: Box<NormalizedExpr>, field: UniCase<String> },
    /// Method call: receiver.method(args)
    MethodCall { receiver: Box<NormalizedExpr>, method: UniCase<String>, args: Vec<NormalizedExpr> },
    /// Function call: func(args)
    Call { callee: Box<NormalizedExpr>, args: Vec<NormalizedExpr> },
    /// Index access: base[index]
    Index { base: Box<NormalizedExpr>, index: Box<NormalizedExpr> },
    /// Binary operation: lhs op rhs
    BinaryOp { lhs: Box<NormalizedExpr>, rhs: Box<NormalizedExpr>, op: String },
    /// New expression: Новый Type(args)
    New { type_name: Option<UniCase<String>>, args: Vec<NormalizedExpr> },
    /// Missing or unknown expression
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum NormalizedLiteral {
    Number(OrderedF64),
    String(String),
    Date(String),
    Bool(bool),
    Undefined,
    Null,
}

/// Wrapper for f64 that implements Eq and Hash for use in HashMap keys.
#[derive(Debug, Clone, Copy)]
struct OrderedF64(f64);

impl PartialEq for OrderedF64 {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for OrderedF64 {}

impl Hash for OrderedF64 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

/// Special values that should be allowed to duplicate.
fn is_special_value(body: &Body, expr_id: ExprId) -> bool {
    match body.expr(expr_id) {
        Expr::Literal(lit) => match lit {
            // Empty string or whitespace-only string
            Literal::String(s) => s.is_empty() || s.chars().all(char::is_whitespace),
            // Undefined/Null
            Literal::Undefined | Literal::Null => true,
            // Zero
            Literal::Number(n) => *n == 0.0,
            _ => false,
        },
        // Символы.ПС / Chars.LF etc.
        Expr::Field { base, field: _ } => {
            if let Expr::Path(name) = body.expr(*base) {
                let base_lower = name.as_str().to_lowercase();
                base_lower == "символы" || base_lower == "chars"
            } else {
                false
            }
        }
        // Missing expression (empty argument)
        Expr::Missing => true,
        _ => false,
    }
}

/// Check if a method name is an insertion method.
fn is_insertion_method(name: &Name, allow_add: bool) -> bool {
    let lower = name.as_str().to_lowercase();
    if allow_add {
        matches!(lower.as_str(), "добавить" | "add" | "вставить" | "insert")
    } else {
        matches!(lower.as_str(), "вставить" | "insert")
    }
}

/// Recorded insertion for duplicate detection.
#[derive(Debug, Clone)]
struct Insertion {
    /// Range of the insertion call in source code
    range: TextRange,
    /// Display string for collection
    collection_display: String,
    /// Display string for arguments
    args_display: String,
    /// Scope depth where insertion occurred
    scope_depth: usize,
    /// Breaker context (return/raise offset) before this insertion
    breaker_context: Option<u32>,
    /// Local breaker context (break/continue in loop) before this insertion
    local_breaker_context: Option<u32>,
}

/// Key for grouping insertions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InsertionKey {
    /// Normalized collection expression
    collection: NormalizedExpr,
    /// Normalized first argument (key for Insert/Вставить)
    first_arg: NormalizedExpr,
}

/// Variable generation tracker.
///
/// Tracks how many times each variable has been assigned.
/// Used to distinguish between different "versions" of a variable.
struct VariableGenerations {
    /// Variable name (lowercase) → generation count
    generations: FxHashMap<UniCase<String>, usize>,
}

impl VariableGenerations {
    fn new() -> Self {
        Self { generations: FxHashMap::default() }
    }

    /// Get generation for a variable (0 if never assigned).
    fn get(&self, name: &str) -> usize {
        let key = UniCase::new(name.to_string());

        // Get direct generation
        let direct_gen = self.generations.get(&key).copied().unwrap_or(0);

        // Also check prefixes for partial reassignment detection
        // Example: Данные.Реквизит.Коллекция should check Данные.Реквизит and Данные
        let parts: Vec<&str> = name.split('.').collect();
        let mut max_gen = direct_gen;

        for i in 1..parts.len() {
            let prefix = parts[..i].join(".");
            if let Some(&gen) = self.generations.get(&UniCase::new(prefix)) {
                max_gen = max_gen.max(gen);
            }
        }

        max_gen
    }

    /// Increment generation for a variable after assignment.
    fn increment(&mut self, name: &str) {
        let key = UniCase::new(name.to_string());
        *self.generations.entry(key).or_insert(0) += 1;

        // Partial reassignment: X.Y.Z changes invalidate X.Y and X
        let parts: Vec<&str> = name.split('.').collect();
        for i in (1..parts.len()).rev() {
            let prefix = parts[..i].join(".");
            *self.generations.entry(UniCase::new(prefix)).or_insert(0) += 1;
        }
    }
}

/// Insertion tracker for duplicate detection.
struct InsertionTracker<'a> {
    body: &'a Body,
    generations: VariableGenerations,
    insertions: FxHashMap<InsertionKey, Vec<Insertion>>,
    /// Last return/raise statement: (offset, scope_depth)
    last_breaker: Option<(u32, usize)>,
    /// Last local break/continue statement: (offset, scope_depth)
    last_local_breaker: Option<(u32, usize)>,
}

impl<'a> InsertionTracker<'a> {
    fn new(body: &'a Body) -> Self {
        Self {
            body,
            generations: VariableGenerations::new(),
            insertions: FxHashMap::default(),
            last_breaker: None,
            last_local_breaker: None,
        }
    }

    /// Normalize an expression for comparison.
    fn normalize_expr(&self, expr_id: ExprId) -> NormalizedExpr {
        match self.body.expr(expr_id) {
            Expr::Missing => NormalizedExpr::Missing,

            Expr::Literal(lit) => {
                let normalized = match lit {
                    Literal::Number(n) => NormalizedLiteral::Number(OrderedF64(*n)),
                    Literal::String(s) => NormalizedLiteral::String(s.clone()),
                    Literal::Date(d) => NormalizedLiteral::Date(d.clone()),
                    Literal::Bool(b) => NormalizedLiteral::Bool(*b),
                    Literal::Undefined => NormalizedLiteral::Undefined,
                    Literal::Null => NormalizedLiteral::Null,
                };
                NormalizedExpr::Literal(normalized)
            }

            Expr::Path(name) => {
                let name_str = name.as_str();
                // Check if it's a BSL keyword/literal that doesn't need generation tracking
                if is_bsl_keyword_or_literal(name_str) {
                    NormalizedExpr::Path { name: UniCase::new(name_str.to_string()), generation: 0 }
                } else {
                    NormalizedExpr::Path {
                        name: UniCase::new(name_str.to_string()),
                        generation: self.generations.get(name_str),
                    }
                }
            }

            Expr::Field { base, field } => NormalizedExpr::Field {
                base: Box::new(self.normalize_expr(*base)),
                field: UniCase::new(field.to_string()),
            },

            Expr::MethodCall { receiver, method, args } => NormalizedExpr::MethodCall {
                receiver: Box::new(self.normalize_expr(*receiver)),
                method: UniCase::new(method.to_string()),
                args: args.iter().map(|a| self.normalize_expr(*a)).collect(),
            },

            Expr::Call { callee, args } => {
                // Check if this is actually a method call (Call with Field as callee)
                if let Expr::Field { base, field } = self.body.expr(*callee) {
                    NormalizedExpr::MethodCall {
                        receiver: Box::new(self.normalize_expr(*base)),
                        method: UniCase::new(field.to_string()),
                        args: args.iter().map(|a| self.normalize_expr(*a)).collect(),
                    }
                } else {
                    NormalizedExpr::Call {
                        callee: Box::new(self.normalize_expr(*callee)),
                        args: args.iter().map(|a| self.normalize_expr(*a)).collect(),
                    }
                }
            }

            Expr::Index { base, index } => NormalizedExpr::Index {
                base: Box::new(self.normalize_expr(*base)),
                index: Box::new(self.normalize_expr(*index)),
            },

            Expr::BinaryOp { lhs, rhs, op } => NormalizedExpr::BinaryOp {
                lhs: Box::new(self.normalize_expr(*lhs)),
                rhs: Box::new(self.normalize_expr(*rhs)),
                op: format!("{:?}", op),
            },

            Expr::New { type_name, args } => NormalizedExpr::New {
                type_name: type_name.as_ref().map(|n| UniCase::new(n.to_string())),
                args: args.iter().map(|a| self.normalize_expr(*a)).collect(),
            },

            Expr::UnaryOp { .. } | Expr::Ternary { .. } | Expr::Array(_) | Expr::Await { .. } => {
                // For complex expressions, use Missing to avoid false positives
                NormalizedExpr::Missing
            }
        }
    }

    /// Record an assignment (increments variable generation).
    fn record_assignment(&mut self, target: ExprId) {
        let name = self.extract_target_name(target);
        if let Some(name) = name {
            tracing::trace!(name = %name, "recording assignment");
            self.generations.increment(&name);
        }
    }

    /// Extract the full path name from an assignment target.
    fn extract_target_name(&self, expr_id: ExprId) -> Option<String> {
        match self.body.expr(expr_id) {
            Expr::Path(name) => Some(name.to_string()),
            Expr::Field { base, field } => {
                let base_name = self.extract_target_name(*base)?;
                Some(format!("{}.{}", base_name, field))
            }
            Expr::Index { base, .. } => self.extract_target_name(*base),
            Expr::MethodCall { receiver, method, .. } => {
                // For method calls like Данные.Метод().Поле, include the method
                let base_name = self.extract_target_name(*receiver)?;
                Some(format!("{}.{}()", base_name, method))
            }
            _ => None,
        }
    }

    /// Record a breaker (return/raise).
    fn record_breaker(&mut self, offset: u32, scope_depth: usize) {
        self.last_breaker = Some((offset, scope_depth));
    }

    /// Record a local breaker (break/continue in loop).
    fn record_local_breaker(&mut self, offset: u32, scope_depth: usize) {
        self.last_local_breaker = Some((offset, scope_depth));
    }

    /// Record an insertion into a collection.
    fn record_insertion(
        &mut self,
        _source_map: &BodySourceMap,
        receiver: ExprId,
        args: &[ExprId],
        call_range: TextRange,
        scope_depth: usize,
    ) {
        if args.is_empty() {
            return;
        }

        // Skip special literals
        if is_special_value(self.body, args[0]) {
            return;
        }

        let collection = self.normalize_expr(receiver);
        let first_arg = self.normalize_expr(args[0]);

        let key = InsertionKey { collection, first_arg };

        let collection_display = self.expr_to_display_string(receiver);
        let args_display =
            args.iter().map(|a| self.expr_to_display_string(*a)).collect::<Vec<_>>().join(", ");

        let breaker_context = self.last_breaker.map(|(offset, _)| offset);
        let local_breaker_context = self.last_local_breaker.map(|(offset, _)| offset);

        let insertion = Insertion {
            range: call_range,
            collection_display,
            args_display,
            scope_depth,
            breaker_context,
            local_breaker_context,
        };

        self.insertions.entry(key).or_default().push(insertion);
    }

    /// Convert expression to display string for diagnostic message.
    fn expr_to_display_string(&self, expr_id: ExprId) -> String {
        match self.body.expr(expr_id) {
            Expr::Missing => "".to_string(),
            Expr::Literal(lit) => match lit {
                Literal::Number(n) => n.to_string(),
                Literal::String(s) => format!("\"{}\"", s),
                Literal::Date(d) => format!("'{}'", d),
                Literal::Bool(b) => if *b { "Истина" } else { "Ложь" }.to_string(),
                Literal::Undefined => "Неопределено".to_string(),
                Literal::Null => "Null".to_string(),
            },
            Expr::Path(name) => name.to_string(),
            Expr::Field { base, field } => {
                format!("{}.{}", self.expr_to_display_string(*base), field)
            }
            Expr::MethodCall { receiver, method, args } => {
                let args_str = args
                    .iter()
                    .map(|a| self.expr_to_display_string(*a))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}.{}({})", self.expr_to_display_string(*receiver), method, args_str)
            }
            Expr::Call { callee, args } => {
                let args_str = args
                    .iter()
                    .map(|a| self.expr_to_display_string(*a))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", self.expr_to_display_string(*callee), args_str)
            }
            Expr::Index { base, index } => {
                format!(
                    "{}[{}]",
                    self.expr_to_display_string(*base),
                    self.expr_to_display_string(*index)
                )
            }
            _ => "...".to_string(),
        }
    }

    /// Report duplicates for a given scope depth.
    fn report_duplicates(&mut self, diagnostics: &mut Vec<Diagnostic>, scope_depth: usize) {
        for insertions in self.insertions.values() {
            let scope_insertions: Vec<_> =
                insertions.iter().filter(|ins| ins.scope_depth == scope_depth).collect();

            if scope_insertions.len() > 1 {
                // Group by (breaker_context, local_breaker_context)
                let mut grouped: FxHashMap<(Option<u32>, Option<u32>), Vec<&Insertion>> =
                    FxHashMap::default();

                for ins in scope_insertions {
                    let key = (ins.breaker_context, ins.local_breaker_context);
                    grouped.entry(key).or_default().push(ins);
                }

                for group in grouped.values() {
                    if group.len() > 1 {
                        // Report only SECOND insertion (Java compatibility)
                        if let Some(second_insertion) = group.get(1) {
                            diagnostics.push(Diagnostic {
                                code: DiagnosticCode::DuplicatedInsertionIntoCollection,
                                message: format!(
                                    "Проверьте повторную вставку {} в коллекцию {}",
                                    second_insertion.args_display,
                                    second_insertion.collection_display
                                ),
                                severity: Severity::Warning,
                                range: second_insertion.range,
                                tags: vec![],
                                fixes: vec![],
                            });
                        }
                    }
                }
            }
        }

        // Remove processed insertions
        for insertions in self.insertions.values_mut() {
            insertions.retain(|ins| ins.scope_depth != scope_depth);
        }
    }
}

/// Check if a name is a BSL keyword or literal.
fn is_bsl_keyword_or_literal(word: &str) -> bool {
    let lower = word.to_lowercase();
    matches!(
        lower.as_str(),
        "новый"
            | "new"
            | "истина"
            | "true"
            | "ложь"
            | "false"
            | "неопределено"
            | "undefined"
            | "null"
            | "массив"
            | "array"
            | "структура"
            | "structure"
            | "соответствие"
            | "map"
            | "строка"
            | "string"
            | "число"
            | "number"
            | "дата"
            | "date"
            | "булево"
            | "boolean"
    )
}

/// Check a list of statements for insertions.
fn check_stmt_list(
    body: &Body,
    source_map: &BodySourceMap,
    stmts: &[StmtId],
    tracker: &mut InsertionTracker,
    diagnostics: &mut Vec<Diagnostic>,
    scope_depth: usize,
    allow_add: bool,
) {
    for stmt_id in stmts {
        check_stmt(body, source_map, *stmt_id, tracker, diagnostics, scope_depth, allow_add);
    }
}

/// Check a single statement for insertions.
fn check_stmt(
    body: &Body,
    source_map: &BodySourceMap,
    stmt_id: StmtId,
    tracker: &mut InsertionTracker,
    diagnostics: &mut Vec<Diagnostic>,
    scope_depth: usize,
    allow_add: bool,
) {
    let stmt_range = source_map.stmt_range(stmt_id);

    match body.stmt(stmt_id) {
        Stmt::Assign { target, value: _ } => {
            tracker.record_assignment(*target);
        }

        Stmt::Expr(expr_id) => {
            check_expr_for_insertion(body, source_map, *expr_id, tracker, scope_depth, allow_add);
            // Track variable modifications when passed to functions
            check_expr_for_side_effects(body, *expr_id, tracker, allow_add);
        }

        Stmt::Return { .. } => {
            if let Some(range) = stmt_range {
                tracker.record_breaker(range.start().into(), scope_depth);
            }
        }

        Stmt::Raise { .. } => {
            if let Some(range) = stmt_range {
                tracker.record_breaker(range.start().into(), scope_depth);
            }
        }

        Stmt::Break => {
            if let Some(range) = stmt_range {
                tracker.record_local_breaker(range.start().into(), scope_depth);
            }
        }

        Stmt::Continue => {
            if let Some(range) = stmt_range {
                tracker.record_local_breaker(range.start().into(), scope_depth);
            }
        }

        Stmt::If { condition: _, then_branch, elsif_branches, else_branch } => {
            // Check then branch
            check_stmt_list(
                body,
                source_map,
                then_branch,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1);

            // Check elsif branches
            for (_, branch_stmts) in elsif_branches.iter() {
                check_stmt_list(
                    body,
                    source_map,
                    branch_stmts,
                    tracker,
                    diagnostics,
                    scope_depth + 1,
                    allow_add,
                );
                tracker.report_duplicates(diagnostics, scope_depth + 1);
            }

            // Check else branch
            if let Some(else_stmts) = else_branch {
                check_stmt_list(
                    body,
                    source_map,
                    else_stmts,
                    tracker,
                    diagnostics,
                    scope_depth + 1,
                    allow_add,
                );
                tracker.report_duplicates(diagnostics, scope_depth + 1);
            }
        }

        Stmt::While { condition: _, body: loop_body } => {
            let saved_local_breaker = tracker.last_local_breaker;
            check_stmt_list(
                body,
                source_map,
                loop_body,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1);
            tracker.last_local_breaker = saved_local_breaker;
        }

        Stmt::For { var: _, from: _, to: _, body: loop_body } => {
            let saved_local_breaker = tracker.last_local_breaker;
            check_stmt_list(
                body,
                source_map,
                loop_body,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1);
            tracker.last_local_breaker = saved_local_breaker;
        }

        Stmt::ForEach { var: _, collection: _, body: loop_body } => {
            let saved_local_breaker = tracker.last_local_breaker;
            check_stmt_list(
                body,
                source_map,
                loop_body,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1);
            tracker.last_local_breaker = saved_local_breaker;
        }

        Stmt::Try { body: try_body, except } => {
            check_stmt_list(
                body,
                source_map,
                try_body,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1);

            check_stmt_list(
                body,
                source_map,
                except,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1);
        }

        // Other statements don't need special handling
        Stmt::VarDecl { .. }
        | Stmt::Goto(_)
        | Stmt::Label(_)
        | Stmt::Execute { .. }
        | Stmt::AddHandler { .. }
        | Stmt::RemoveHandler { .. } => {}
    }
}

/// Check an expression for insertion method calls.
///
/// Handles two patterns:
/// 1. `Expr::MethodCall { receiver, method, args }` - direct method call
/// 2. `Expr::Call { callee: Expr::Field { base, field }, args }` - call via field access
///
/// The second pattern occurs because the parser creates CALL_EXPR with FIELD_EXPR inside
/// for method calls like `Array.Add(Value)`.
fn check_expr_for_insertion(
    body: &Body,
    source_map: &BodySourceMap,
    expr_id: ExprId,
    tracker: &mut InsertionTracker,
    scope_depth: usize,
    allow_add: bool,
) {
    match body.expr(expr_id) {
        Expr::MethodCall { receiver, method, args } => {
            if is_insertion_method(method, allow_add) && !args.is_empty() {
                if let Some(range) = source_map.expr_range(expr_id) {
                    tracker.record_insertion(source_map, *receiver, args, range, scope_depth);
                }
            }
        }
        // Pattern 2: Call with Field as callee (common for method calls in BSL)
        Expr::Call { callee, args } => {
            if let Expr::Field { base, field } = body.expr(*callee) {
                if is_insertion_method(field, allow_add) && !args.is_empty() {
                    if let Some(range) = source_map.expr_range(expr_id) {
                        tracker.record_insertion(source_map, *base, args, range, scope_depth);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Check an expression for side effects (variable modifications via function calls).
///
/// In BSL, objects passed to functions can be modified. This function tracks
/// when a variable is passed as an argument to a function/method (excluding
/// the insertion methods we're analyzing).
fn check_expr_for_side_effects(
    body: &Body,
    expr_id: ExprId,
    tracker: &mut InsertionTracker,
    allow_add: bool,
) {
    match body.expr(expr_id) {
        // Call with Field as callee: obj.Method(args)
        Expr::Call { callee, args } => {
            // Check if this is NOT an insertion method
            if let Expr::Field { base: _, field } = body.expr(*callee) {
                // If it's an insertion method, don't track side effects for args
                // (we handle those separately in check_expr_for_insertion)
                if is_insertion_method(field, allow_add) {
                    return;
                }
            }

            // Mark all variable arguments as potentially modified
            for arg in args.iter() {
                if let Some(name) = tracker.extract_target_name(*arg) {
                    if matches!(body.expr(*arg), Expr::Path(_) | Expr::Field { .. }) {
                        tracker.generations.increment(&name);
                    }
                }
            }
        }
        // Direct method call: obj.Method(args)
        Expr::MethodCall { receiver: _, method, args } => {
            // Don't track side effects for insertion methods
            if is_insertion_method(method, allow_add) {
                return;
            }

            // Mark all variable arguments as potentially modified
            for arg in args.iter() {
                if let Some(name) = tracker.extract_target_name(*arg) {
                    if matches!(body.expr(*arg), Expr::Path(_) | Expr::Field { .. }) {
                        tracker.generations.increment(&name);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Legacy AST-based check function (for fallback/compatibility).
/// This will be removed once HIR-based check is fully validated.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("DuplicatedInsertionIntoCollection::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::DuplicatedInsertionIntoCollection) {
        return Vec::new();
    }

    // Use HIR-based check via module_bodies
    use hir::ModuleId;

    let module_id = ModuleId::new(ctx.file_id);
    let module_bodies = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ctx.db.module_bodies(module_id)
    })) {
        Ok(bodies) => bodies,
        Err(_) => return Vec::new(),
    };

    let mut diagnostics = Vec::new();

    for (_method_id, body, source_map) in module_bodies.method_bodies() {
        diagnostics.extend(check_body(body, source_map, ctx.config));
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagnosticsConfig, DiagnosticsContext};
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;
    use vfs::VfsPath;

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();

        // Set up source root for module_bodies to work
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_simple_duplicate() {
        let code = r#"
Процедура Тест()
    Массив = Новый Массив;
    Массив.Добавить(Значение);
    Массив.Добавить(Значение);
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect one duplicate");

        use crate::test_utils::assert_diagnostic_range;
        assert_diagnostic_range(code, &diagnostics[0], 4, 4, 29);
    }

    #[test]
    fn test_generation_change() {
        let code = r#"
Процедура Тест()
    Массив = Новый Массив;
    Массив.Добавить(Х);
    Х = 5;
    Массив.Добавить(Х);
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Should NOT detect duplicate after generation change");
    }

    #[test]
    fn test_special_literals() {
        let code = r#"
Процедура Тест()
    Список = Новый Массив;
    Список.Добавить("");
    Список.Добавить("");
    Список.Добавить(Неопределено);
    Список.Добавить(Неопределено);
    Список.Добавить(Символы.ПС);
    Список.Добавить(Символы.ПС);
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Special literals should be allowed to duplicate");
    }

    #[test]
    fn test_global_function_collection() {
        let code = r#"
Процедура Тест()
    Коллекция().Добавить(Значение);
    Коллекция().Добавить(Значение);
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect duplicate with global function");

        use crate::test_utils::assert_diagnostic_range;
        assert_diagnostic_range(code, &diagnostics[0], 3, 4, 34);
    }

    #[test]
    fn test_preprocessor_duplicate() {
        // NOTE: HIR currently does not lower statements inside preprocessor directives.
        // This is a known limitation. Code inside #Если/#Иначе is not included in body.body_stmts.
        // The Java implementation does detect duplicates across preprocessor branches,
        // but our HIR-based implementation cannot until HIR is extended to support this.
        let code = r#"
Процедура Тест()
    #Если ТолстыйКлиентОбычноеПриложение Тогда
        ЭлементыСтиля.Вставить(ЭлементСтиля.Ключ, ЭлементСтиля.Значение.Получить());
    #Иначе
        ЭлементыСтиля.Вставить(ЭлементСтиля.Ключ, ЭлементСтиля.Значение);
    #КонецЕсли
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        // Current HIR limitation: 0 diagnostics (code inside preprocessor not analyzed)
        // Java expectation: 1 diagnostic (duplicate key across branches)
        assert_eq!(
            diagnostics.len(),
            0,
            "HIR does not currently analyze code inside preprocessor directives"
        );
    }

    #[test]
    fn test_break_in_loop() {
        let code = r#"
Процедура Тест(Коллекция, Коллекция2)
    Для Каждого Элемент Из Коллекция Цикл
        Коллекция2.Добавить(Элемент);
        Если Условие() Тогда
            Прервать;
        КонецЕсли;
        Коллекция2.Добавить(Элемент);
    КонецЦикла;
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(
            diagnostics.len(),
            0,
            "Should NOT detect duplicate (local break may prevent execution)"
        );
    }

    #[test]
    fn test_method_in_collection_path() {
        let code = r#"
Процедура Тест()
    Данные.Метод().Коллекция = Новый Массив;
    Данные.Метод().Коллекция.Добавить("Значение");
    Данные.Метод().Коллекция.Добавить("Значение");
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect duplicate with method in collection path");

        use crate::test_utils::assert_diagnostic_range;
        assert_diagnostic_range(code, &diagnostics[0], 4, 4, 49);
    }

    #[test]
    fn test_complex_argument() {
        let code = r#"
Процедура Тест()
    Данные.Метод().ОбщаяКоллекция.Добавить(Данные.Метод().ПовторнаяКоллекция);
    Данные.Метод().ОбщаяКоллекция.Добавить(Данные.Метод().ПовторнаяКоллекция);
КонецПроцедуры
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect duplicate with complex argument");

        use crate::test_utils::assert_diagnostic_range;
        assert_diagnostic_range(code, &diagnostics[0], 3, 4, 77);
    }

    #[test]
    fn test_comprehensive() {
        use crate::test_utils::assert_diagnostic_range;

        let code = include_str!("../../test_data/DuplicatedInsertionIntoCollectionDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        // Expected: 18 diagnostics (17 from Java that HIR can detect + 1 extra we find)
        // Note: Line 59 (inside #Если/#Иначе) is NOT detected because HIR does not analyze
        // code inside preprocessor directives. This is a known limitation.
        // We find Line 197 which Java doesn't report (complex Тип() call).
        assert_eq!(diagnostics.len(), 18, "Expected 18 diagnostics");

        // Sort diagnostics by position for consistent ordering
        let mut sorted_diagnostics = diagnostics.clone();
        sorted_diagnostics.sort_by_key(|d| d.range.start());

        // Verify each diagnostic with precise line and column positions (0-indexed)
        // Line 5: Массив.Добавить(СтрокаТаблицы)
        assert_diagnostic_range(code, &sorted_diagnostics[0], 4, 4, 34);
        // Line 9: Коллекция.Вставить("Ключ1", 1)
        assert_diagnostic_range(code, &sorted_diagnostics[1], 8, 4, 34);
        // Line 13: Коллекция2.Вставить("Ключ1", 2)
        assert_diagnostic_range(code, &sorted_diagnostics[2], 12, 4, 35);
        // Line 23: Коллекция.Вставить("Ключ1", 3)
        assert_diagnostic_range(code, &sorted_diagnostics[3], 22, 8, 38);
        // Line 28: Итог.Коллекция.Индексы.Добавить("Пользователь")
        assert_diagnostic_range(code, &sorted_diagnostics[4], 27, 8, 55);
        // Line 100: Данные.Метод().ПовторнаяСоздаваемаяКоллекция.Добавить("Пользователь")
        assert_diagnostic_range(code, &sorted_diagnostics[5], 99, 8, 77);
        // Line 103: Данные.Метод().ОбщаяКоллекция.Добавить(Данные.Метод().ПовторнаяСоздаваемаяКоллекция)
        assert_diagnostic_range(code, &sorted_diagnostics[6], 102, 8, 92);
        // Line 120: ВидыСвойствНабора.Вставить("ДополнительныеРеквизиты", Истина)
        assert_diagnostic_range(code, &sorted_diagnostics[7], 119, 4, 65);
        // Line 134: ПовторнаяСоздаваемаяКоллекция.Добавить("Пользователь")
        assert_diagnostic_range(code, &sorted_diagnostics[8], 133, 4, 58);
        // Line 137: ОбщаяКоллекция.Добавить(ПовторнаяСоздаваемаяКоллекция)
        assert_diagnostic_range(code, &sorted_diagnostics[9], 136, 4, 58);
        // Line 148: Данные2.ОбщаяКоллекция2.Вставить(Данные2.Реквизит2.ПовторнаяСоздаваемаяКоллекция2)
        assert_diagnostic_range(code, &sorted_diagnostics[10], 147, 8, 90);
        // Line 152: Данные3.ОбщаяКоллекция3.Вставить(Данные3.Реквизит3.ПовторнаяСоздаваемаяКоллекция3)
        assert_diagnostic_range(code, &sorted_diagnostics[11], 151, 8, 90);
        // Line 158: Описания.Добавить(Ключ)
        assert_diagnostic_range(code, &sorted_diagnostics[12], 157, 4, 27);
        // Line 162: Описания2.Добавить(Часть1.Часть2)
        assert_diagnostic_range(code, &sorted_diagnostics[13], 161, 4, 37);
        // Line 172: Сведения2.ДобавленныеЭлементы.Добавить(ИмяКоманды, 9, Истина)
        assert_diagnostic_range(code, &sorted_diagnostics[14], 171, 4, 65);
        // Line 197: Текст.Добавить(, Тип("ПереводСтрокиФорматированногоДокумента"))
        assert_diagnostic_range(code, &sorted_diagnostics[15], 196, 4, 67);
        // Line 266: Коллекция().Добавить(СтрокаТаблицы)
        assert_diagnostic_range(code, &sorted_diagnostics[16], 265, 4, 39);
        // Line 269: Коллекция2().Реквизит.Добавить(СтрокаТаблицы2)
        assert_diagnostic_range(code, &sorted_diagnostics[17], 268, 4, 50);
    }
}
