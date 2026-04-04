//! AST to HIR lowering.
//!
//! This module converts AST method bodies into HIR representation.
//! Diagnostics are collected as a byproduct of lowering.
//!
//! ## Lowering strategy
//!
//! The lowering process walks the AST once and:
//! 1. Allocates HIR nodes in arenas
//! 2. Records source mappings for diagnostics
//! 3. Emits diagnostics when issues are detected
//!
//! This is more efficient than having separate diagnostic passes
//! because we traverse the AST only once.
//!
//! ## Module structure
//!
//! - `stmt` - Statement lowering
//! - `expr` - Expression lowering
//! - `preproc` - Preprocessor directive handling
//! - `control_flow` - Return path and unreachable code analysis
//! - `diagnostics` - Diagnostic helpers (async calls, transactions, deprecated methods)
//! - `utils` - Utility functions

mod control_flow;
mod diagnostics;
mod expr;

mod platform_helpers;
mod preproc;
mod stmt;
mod utils;

#[cfg(test)]
mod tests;

use line_index::LineIndex;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use syntax::{SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::body::{Body, BodyDiagnostic, BodySourceMap, LowerResult};
use crate::hir::{Binding, BindingIdx, Expr, ExprIdx, Stmt, StmtIdx};
use crate::Name;

/// Lowering context.
///
/// Holds state during AST → HIR conversion.
pub(crate) struct LoweringCtx {
    pub(crate) body: Body,
    pub(crate) source_map: BodySourceMap,
    pub(crate) diagnostics: Vec<BodyDiagnostic>,
    /// Whether we're lowering a function (vs procedure).
    /// Used for diagnostics like FunctionShouldHaveReturn.
    pub(crate) is_function: bool,

    /// Local variables (lowercase name -> (original name, declaration range)).
    /// Used to distinguish local vars from module names in qualified call checks.
    pub(crate) local_vars: FxHashMap<String, (Name, TextRange)>,

    /// Parameter names (lowercase).
    /// Used to distinguish params from module names in qualified call checks.
    pub(crate) param_names: FxHashSet<String>,

    /// By-reference parameter names (lowercase) - parameters without "Знач" keyword.
    /// Used for FunctionOutParameter diagnostic.
    pub(crate) by_ref_param_names: FxHashSet<String>,

    /// By-value parameter mapping: lowercase name -> BindingIdx (typed).
    /// Used for RewriteMethodParameter diagnostic to detect overwrites of byValue params.
    pub(crate) by_value_params: FxHashMap<String, BindingIdx>,

    /// Cancel parameter names (lowercase) - parameters named "Отказ" or "Cancel".
    /// Used for UsingCancelParameter diagnostic.
    pub(crate) cancel_params: FxHashSet<String>,

    /// Pending SDBL queries (before ExprIdx allocation).
    /// Stores (literal_range, query_info) to match by TextRange instead of String comparison.
    pub(crate) pending_sdbl: Vec<(syntax::TextRange, syntax::SdblQueryInfo)>,

    /// Loop nesting depth (0 = not in loop, 1+ = inside loop).
    /// Used for CreateQueryInCycle diagnostic.
    pub(crate) loop_depth: usize,

    /// Query-like variables: lowercase name -> VarType.
    /// Tracks Query, QueryBuilder, ReportBuilder variables for CreateQueryInCycle diagnostic.
    pub(crate) query_vars: FxHashMap<String, QueryVarType>,

    /// ForEach collection stack: (collection_expr_idx, collection_text) tuples.
    /// Tracks the collection being iterated for DeletingCollectionItem diagnostic.
    /// Stack handles nested ForEach loops.
    pub(crate) foreach_collections: Vec<(ExprIdx, String)>,

    /// Whether current method is client-only (&НаКлиенте annotation ONLY).
    /// Used for ExecuteExternalCode diagnostic - Execute/Eval is allowed only in client-only context.
    pub(crate) is_client_only: bool,

    /// Whether current method has "БезКонтекста" (NoContext) annotation.
    /// Used for FormDataToValue diagnostic - call is allowed in БезКонтекста methods.
    /// Checks for @НаСервереБезКонтекста or @НаКлиентеНаСервереБезКонтекста.
    pub(crate) has_no_context_annotation: bool,

    /// External module references collected during lowering.
    /// Used to build module dependency graph for lazy loading.
    pub(crate) external_refs: Vec<crate::body::ExternalRef>,

    /// Line index for OneStatementPerLine diagnostic.
    /// If set, statements are tracked by their starting line.
    pub(crate) line_index: Option<Arc<LineIndex>>,

    /// Statements grouped by line number for OneStatementPerLine diagnostic.
    /// Key: 0-based line number, Value: list of statement TextRanges starting on that line.
    pub(crate) statements_by_line: FxHashMap<u32, Vec<TextRange>>,

    /// Current method name for method-scoped diagnostics.
    /// Set at the beginning of method lowering.
    pub(crate) current_method_name: Option<String>,

    /// Return statements in current method for TooManyReturns diagnostic.
    /// Stores TextRange of each return statement.
    pub(crate) return_statements: Vec<TextRange>,

    /// Whether current method has &Вместо/&Instead annotation.
    /// Used for WrongUseFunctionProceedWithCall diagnostic - ПродолжитьВызов is only allowed
    /// in methods with &Вместо annotation.
    pub(crate) is_instead_method: bool,

    /// Whether current method has server annotation (&НаСервере or &НаСервереБезКонтекста).
    /// Used for UsingSynchronousCalls diagnostic - synchronous calls are skipped in server context.
    pub(crate) is_server_method: bool,

    /// Current nesting depth for control flow statements (IF, WHILE, FOR, TRY).
    /// Used for NestedStatements diagnostic.
    pub(crate) nesting_depth: u32,

    /// Flag indicating if any child was a nesting statement.
    /// Used for NestedStatements diagnostic to identify leaf statements.
    pub(crate) had_nested_child: bool,

    /// Whether we're inside an IF statement that has a platform type guard.
    /// Used for UsingObjectNotAvailableUnix diagnostic to skip COMObject/Mail checks.
    pub(crate) in_platform_guard: bool,

    /// Whether we're inside an EXCEPT_CLAUSE of a try statement.
    /// Used for UsageWriteLogEvent diagnostic validation.
    pub(crate) in_except_block: bool,

    /// Whether current except block contains a Raise statement.
    /// Used for UsageWriteLogEvent diagnostic validation.
    pub(crate) except_has_raise: bool,
}

/// Type of query-like variable for CreateQueryInCycle diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum QueryVarType {
    Query,
    QueryBuilder,
    ReportBuilder,
    Undefined,
}

impl LoweringCtx {
    /// Create a new lowering context.
    pub(crate) fn new(is_function: bool) -> Self {
        Self {
            body: Body::new(),
            source_map: BodySourceMap::new(),
            diagnostics: Vec::new(),
            is_function,
            local_vars: FxHashMap::default(),
            param_names: FxHashSet::default(),
            by_ref_param_names: FxHashSet::default(),
            by_value_params: FxHashMap::default(),
            cancel_params: FxHashSet::default(),
            pending_sdbl: Vec::new(),
            loop_depth: 0,
            query_vars: FxHashMap::default(),
            foreach_collections: Vec::new(),
            is_client_only: false, // Will be set in lower_method_with_externals
            has_no_context_annotation: false, // Will be set in lower_method_with_externals
            external_refs: Vec::new(),
            line_index: None, // Will be set in lower_method_with_externals_and_line_index
            statements_by_line: FxHashMap::default(),
            current_method_name: None, // Will be set in lower_method_with_externals_and_line_index
            return_statements: Vec::new(),
            is_instead_method: false, // Will be set in lower_method_with_externals_and_line_index
            is_server_method: false,  // Will be set in lower_method_with_externals
            nesting_depth: 0,
            had_nested_child: false,
            in_platform_guard: false,
            in_except_block: false,
            except_has_raise: false,
        }
    }

    /// Set the line index for OneStatementPerLine diagnostic.
    pub(crate) fn set_line_index(&mut self, line_index: Arc<LineIndex>) {
        self.line_index = Some(line_index);
    }

    /// Track a statement by its starting line for OneStatementPerLine diagnostic.
    /// Returns true if this is NOT the first statement on the line (i.e., should emit diagnostic).
    pub(crate) fn track_statement_line(&mut self, range: TextRange) -> bool {
        let Some(ref line_index) = self.line_index else {
            return false;
        };

        let line = line_index.line_col(range.start()).line;
        let statements = self.statements_by_line.entry(line).or_default();
        statements.push(range);

        // Return true if this is not the first statement on the line
        statements.len() > 1
    }

    /// Emit OneStatementPerLine diagnostics for all lines with multiple statements.
    pub(crate) fn emit_one_statement_per_line_diagnostics(&mut self) {
        if self.line_index.is_none() {
            return;
        }

        for (_line, ranges) in std::mem::take(&mut self.statements_by_line) {
            if ranges.len() > 1 {
                // Skip the first statement, emit diagnostic for each subsequent one
                for range in ranges.into_iter().skip(1) {
                    self.emit(BodyDiagnostic::OneStatementPerLine { range });
                }
            }
        }
    }

    /// Emit TooManyReturns diagnostic if method has too many return statements.
    /// Returns are collected during lowering, but threshold check happens in from_hir().
    /// We emit diagnostic if there are more than 2 returns (minimum reasonable threshold).
    pub(crate) fn emit_too_many_returns_diagnostic(
        &mut self,
        method_name: String,
        method_name_range: TextRange,
    ) {
        const MIN_THRESHOLD: usize = 2;

        let returns = std::mem::take(&mut self.return_statements);
        if returns.len() > MIN_THRESHOLD {
            self.emit(BodyDiagnostic::TooManyReturns { method_name, method_name_range, returns });
        }
    }

    /// Register a parameter name.
    /// Used to distinguish params from module names in qualified call checks.
    pub(crate) fn register_param(&mut self, name: &str) {
        self.param_names.insert(name.to_lowercase());
    }

    /// Register a local variable.
    /// Used to distinguish local vars from module names in qualified call checks.
    pub(crate) fn register_local_var(&mut self, name: Name, range: TextRange) {
        let key = name.as_str().to_lowercase();
        self.local_vars.insert(key, (name, range));
    }

    /// Allocate an expression and record its source range.
    /// Returns typed index for internal use in lowering.
    pub(crate) fn alloc_expr(&mut self, expr: Expr, range: TextRange) -> ExprIdx {
        let id = self.body.exprs.alloc(expr);
        self.source_map.record_expr(id, range);
        id
    }

    /// Allocate a statement and record its source range.
    /// Returns typed index for internal use in lowering.
    pub(crate) fn alloc_stmt(&mut self, stmt: Stmt, range: TextRange) -> StmtIdx {
        let id = self.body.stmts.alloc(stmt);
        self.source_map.record_stmt(id, range);
        id
    }

    /// Allocate a binding and record its source range.
    /// Returns typed index for internal use in lowering.
    pub(crate) fn alloc_binding(&mut self, binding: Binding, range: TextRange) -> BindingIdx {
        let id = self.body.bindings.alloc(binding);
        self.source_map.record_binding(id, range);
        id
    }

    /// Allocate a missing expression (for error recovery).
    /// Returns typed index for internal use in lowering.
    pub(crate) fn missing_expr(&mut self) -> ExprIdx {
        self.body.exprs.alloc(Expr::Missing)
    }

    /// Enter a loop (increment loop depth).
    pub(crate) fn enter_loop(&mut self) {
        self.loop_depth += 1;
    }

    /// Leave a loop (decrement loop depth).
    pub(crate) fn leave_loop(&mut self) {
        if self.loop_depth > 0 {
            self.loop_depth -= 1;
        }
    }

    /// Check if currently inside a loop.
    pub(crate) fn in_loop(&self) -> bool {
        self.loop_depth > 0
    }

    /// Register a query-like variable (Query, QueryBuilder, ReportBuilder).
    pub(crate) fn register_query_var(&mut self, name: String, var_type: QueryVarType) {
        self.query_vars.insert(name.to_lowercase(), var_type);
    }

    /// Get query variable type by name (case-insensitive).
    pub(crate) fn get_query_var_type(&self, name: &str) -> Option<QueryVarType> {
        self.query_vars.get(&name.to_lowercase()).copied()
    }

    /// Check if a variable is a query-like type.
    pub(crate) fn is_query_var(&self, name: &str) -> bool {
        matches!(
            self.get_query_var_type(name),
            Some(QueryVarType::Query | QueryVarType::QueryBuilder | QueryVarType::ReportBuilder)
        )
    }

    /// Enter a ForEach loop, tracking the collection being iterated.
    pub(crate) fn enter_foreach(&mut self, collection_expr: ExprIdx, collection_text: String) {
        self.foreach_collections.push((collection_expr, collection_text));
    }

    /// Leave a ForEach loop.
    pub(crate) fn leave_foreach(&mut self) {
        self.foreach_collections.pop();
    }

    /// Check if an expression matches any active ForEach collection (case-insensitive).
    /// Returns collection text for diagnostic message if matched.
    pub(crate) fn matches_foreach_collection(&self, expr: ExprIdx) -> Option<&str> {
        use crate::body::lower::expr::exprs_are_equal;

        for (collection_expr, collection_text) in self.foreach_collections.iter().rev() {
            if exprs_are_equal(&self.body, *collection_expr, expr) {
                return Some(collection_text.as_str());
            }
        }
        None
    }

    /// Emit a diagnostic.
    pub(crate) fn emit(&mut self, diagnostic: BodyDiagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

/// Lower a method AST node to HIR.
pub fn lower_method(method_node: &SyntaxNode, is_function: bool) -> LowerResult {
    lower_method_with_externals(method_node, is_function, None)
}

/// Check if method has ONLY &НаКлиенте annotation.
///
/// For ExecuteExternalCode diagnostic - Execute/Eval is only allowed in client-only context.
fn is_client_only_method(method_node: &SyntaxNode) -> bool {
    let annotations: Vec<_> = method_node
        .children()
        .filter(|child| {
            matches!(child.kind(), SyntaxKind::ANNOTATION | SyntaxKind::COMPILER_DIRECTIVE)
        })
        .collect();

    // Must have exactly ONE annotation
    if annotations.len() != 1 {
        return false;
    }

    // Check if it's &НаКлиенте / &AtClient
    annotations[0]
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|token| token.kind() == SyntaxKind::ANN_AT_CLIENT)
}

/// Check if method name collides with platform 8.3.12 global context methods.
///
/// Platform 8.3.12 added bitwise operation methods to global context.
/// User-defined methods with these names will conflict.
fn is_global_context_collision_8312(name: &str) -> bool {
    const COLLISION_METHODS: &[&str] = &[
        // Russian variants
        "проверитьбит",
        "проверитьпобитовоймаске",
        "установитьбит",
        "побитовоеи",
        "побитовоеили",
        "побитовоене",
        "побитовоеине",
        "побитовоеисключительноеили",
        "побитовыйсдвигвлево",
        "побитовыйсдвигвправо",
        // English variants
        "checkbit",
        "checkbybitmask",
        "setbit",
        "bitwiseand",
        "bitwiseor",
        "bitwisenot",
        "bitwiseandnot",
        "bitwisexor",
        "bitwiseshiftleft",
        "bitwiseshiftright",
    ];

    COLLISION_METHODS.contains(&name.to_lowercase().as_str())
}

/// Check if method has "БезКонтекста" (NoContext) annotation.
///
/// For FormDataToValue diagnostic - call is allowed in БезКонтекста methods.
/// Returns true if method has:
/// - @НаСервереБезКонтекста / @AtServerNoContext
/// - @НаКлиентеНаСервереБезКонтекста / @AtClientAtServerNoContext
fn has_no_context_annotation_method(method_node: &SyntaxNode) -> bool {
    let annotations: Vec<_> = method_node
        .children()
        .filter(|child| {
            matches!(child.kind(), SyntaxKind::ANNOTATION | SyntaxKind::COMPILER_DIRECTIVE)
        })
        .collect();

    // Check any annotation for БезКонтекста tokens
    annotations.iter().any(|ann| {
        ann.descendants_with_tokens().filter_map(|el| el.into_token()).any(|token| {
            matches!(
                token.kind(),
                SyntaxKind::ANN_AT_SERVER_NO_CONTEXT
                    | SyntaxKind::ANN_AT_CLIENT_AT_SERVER_NO_CONTEXT
            )
        })
    })
}

/// Check if method has &Вместо / &Around annotation.
///
/// For WrongUseFunctionProceedWithCall diagnostic - ПродолжитьВызов is only allowed
/// in methods with &Вместо annotation.
fn is_around_annotation_method(method_node: &SyntaxNode) -> bool {
    let annotations: Vec<_> = method_node
        .children()
        .filter(|child| {
            matches!(child.kind(), SyntaxKind::ANNOTATION | SyntaxKind::COMPILER_DIRECTIVE)
        })
        .collect();

    annotations.iter().any(|ann| {
        ann.descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .any(|token| token.kind() == SyntaxKind::ANN_AROUND)
    })
}

/// Check if method has server annotation (&НаСервере or &НаСервереБезКонтекста).
///
/// For UsingSynchronousCalls diagnostic - synchronous calls are allowed on server.
/// Returns true if method has:
/// - @НаСервере / @AtServer
/// - @НаСервереБезКонтекста / @AtServerNoContext
fn is_server_method(method_node: &SyntaxNode) -> bool {
    let annotations: Vec<_> = method_node
        .children()
        .filter(|child| {
            matches!(child.kind(), SyntaxKind::ANNOTATION | SyntaxKind::COMPILER_DIRECTIVE)
        })
        .collect();

    annotations.iter().any(|ann| {
        ann.descendants_with_tokens().filter_map(|el| el.into_token()).any(|token| {
            matches!(token.kind(), SyntaxKind::ANN_AT_SERVER | SyntaxKind::ANN_AT_SERVER_NO_CONTEXT)
        })
    })
}

/// Check if function always returns the same primitive value.
///
/// For FunctionReturnsSamePrimitive diagnostic.
/// Skips "Attachable" methods (names starting with "Подключаемый_" or "Attachable_").
fn check_function_returns_same_primitive(ctx: &mut LoweringCtx, method_node: &SyntaxNode) {
    use crate::hir::{Expr, Literal, Stmt};

    // Skip attachable methods
    if let Some(name_token) = method_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)
    {
        let name = name_token.text().to_lowercase();
        if name.starts_with("подключаемый_") || name.starts_with("attachable_") {
            return;
        }
    }

    // Collect all return statements with their literal values
    let mut return_literals: Vec<&Literal> = Vec::new();

    for (stmt_id, _) in ctx.body.stmts.iter() {
        if let Stmt::Return { value: Some(expr_id) } = &ctx.body.stmts[stmt_id] {
            // Check if expression is a literal (primitive)
            if let Expr::Literal(lit) = &ctx.body.exprs[*expr_id] {
                return_literals.push(lit);
            } else {
                // Non-primitive return found (variable, function call, etc.)
                return;
            }
        }
    }

    // Need at least 2 return statements
    if return_literals.len() < 2 {
        return;
    }

    // Compare all literals - check if all are the same
    let first = return_literals[0];
    let all_same = return_literals[1..].iter().all(|lit| literals_equal(first, lit));

    if all_same {
        // Emit diagnostic on function name
        if let Some(name_token) = method_node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::IDENT)
        {
            ctx.emit(BodyDiagnostic::FunctionReturnsSamePrimitive {
                range: name_token.text_range(),
            });
        }
    }
}

/// Emit method-scoped diagnostics at end of lowering.
///
/// Phase 4: Method metrics (complexity, size, nesting, params) are calculated
/// after body is lowered and emitted as diagnostics.
/// Threshold filtering happens in from_hir() handlers.
fn emit_method_scoped_diagnostics(
    ctx: &mut LoweringCtx,
    method_name: &str,
    name_range: TextRange,
    method_range: TextRange,
    is_function: bool,
) {
    use crate::body::BodyDiagnostic;
    use crate::cognitive_complexity;
    use crate::cyclomatic_complexity;

    // Calculate complexity metrics using existing implementations
    let cognitive = cognitive_complexity::calculate_complexity(&ctx.body);
    let cyclomatic = cyclomatic_complexity::calculate_complexity(&ctx.body);

    // Emit CognitiveComplexity candidate (always emit, filter in from_hir)
    // Only emit if complexity > 0 to reduce noise
    if cognitive > 0 {
        ctx.emit(BodyDiagnostic::CognitiveComplexity {
            method_name: method_name.to_string(),
            complexity: cognitive,
            is_function,
            range: name_range,
        });
    }

    // Emit CyclomaticComplexity candidate
    // Base complexity is 1, so only emit if > 1
    if cyclomatic > 1 {
        ctx.emit(BodyDiagnostic::CyclomaticComplexity {
            method_name: method_name.to_string(),
            complexity: cyclomatic,
            is_function,
            range: name_range,
        });
    }

    // Emit MethodSize candidate using line-based calculation
    // Algorithm: subCodeBlock.getStop().getLine() - subCodeBlock.getStart().getLine()
    // Rowan PROCEDURE_DEF spans from declaration to end keyword, so subtract 4 to match the subCodeBlock span
    if let Some(ref line_index) = ctx.line_index {
        let start_line = line_index.line_col(method_range.start()).line as usize;
        let end_line = line_index.line_col(method_range.end()).line as usize;
        let total_span = end_line.saturating_sub(start_line);
        let method_size = total_span.saturating_sub(4) as u32;

        if method_size > 0 {
            ctx.emit(BodyDiagnostic::MethodSize {
                method_name: method_name.to_string(),
                size: method_size,
                is_function,
                range: name_range,
            });
        }
    }

    // Emit NumberOfParams and NumberOfOptionalParams candidates
    let params_count = ctx.body.params.len() as u32;
    let optional_count =
        ctx.body.params.iter().filter(|&p| ctx.body.bindings[*p].default_value.is_some()).count()
            as u32;

    if params_count > 0 {
        ctx.emit(BodyDiagnostic::NumberOfParams {
            method_name: method_name.to_string(),
            count: params_count,
            is_function,
            range: name_range,
        });
    }

    if optional_count > 0 {
        ctx.emit(BodyDiagnostic::NumberOfOptionalParams {
            method_name: method_name.to_string(),
            count: optional_count,
            is_function,
            range: name_range,
        });
    }
}

/// Compare two literals for equality (case-insensitive for strings by default).
///
/// Strings are compared case-insensitively unless configured otherwise.
/// TODO: add caseSensitiveForString parameter when needed.
fn literals_equal(a: &crate::hir::Literal, b: &crate::hir::Literal) -> bool {
    use crate::hir::Literal;

    match (a, b) {
        (Literal::Number(a), Literal::Number(b)) => (a - b).abs() < f64::EPSILON,
        (Literal::String(a), Literal::String(b)) => {
            // Case-insensitive comparison (default behavior)
            a.to_uppercase() == b.to_uppercase()
        }
        (Literal::Date(a), Literal::Date(b)) => a == b,
        (Literal::Bool(a), Literal::Bool(b)) => a == b,
        (Literal::Undefined, Literal::Undefined) => true,
        (Literal::Null, Literal::Null) => true,
        _ => false,
    }
}

/// Lower a method AST node to HIR.
///
/// When `line_index` is provided, additional diagnostics are emitted:
/// OneStatementPerLine, TooManyReturns, MethodSize, and method-scoped metrics.
pub fn lower_method_with_externals(
    method_node: &SyntaxNode,
    is_function: bool,
    line_index: Option<Arc<LineIndex>>,
) -> LowerResult {
    let mut ctx = LoweringCtx::new(is_function);

    if let Some(li) = line_index {
        ctx.set_line_index(li);
    }

    // Check method annotations
    ctx.is_client_only = is_client_only_method(method_node);
    ctx.has_no_context_annotation = has_no_context_annotation_method(method_node);
    ctx.is_instead_method = is_around_annotation_method(method_node);
    ctx.is_server_method = is_server_method(method_node);

    // Extract method name once for all checks (accept keywords for error recovery)
    let name_token =
        method_node.children_with_tokens().filter_map(|el| el.into_token()).find(|tok| {
            let k = tok.kind();
            if k == SyntaxKind::IDENT {
                return true;
            }
            // Accept non-structural keywords as names (error recovery)
            k.is_keyword()
                && !matches!(
                    k,
                    SyntaxKind::KW_PROCEDURE
                        | SyntaxKind::KW_END_PROCEDURE
                        | SyntaxKind::KW_FUNCTION
                        | SyntaxKind::KW_END_FUNCTION
                        | SyntaxKind::KW_ASYNC
                        | SyntaxKind::KW_EXPORT
                )
        });

    // Set current method name for method-scoped diagnostics
    if let Some(ref token) = name_token {
        ctx.current_method_name = Some(token.text().to_string());
    }

    // Check for ReservedWordAsMethodName diagnostic
    if let Some(ref token) = name_token {
        if token.kind().is_keyword() {
            ctx.emit(BodyDiagnostic::ReservedWordAsMethodName {
                name: token.text().to_string(),
                range: token.text_range(),
            });
        }
    }

    // Check for FunctionNameStartsWithGet and GlobalContextMethodCollision8312 diagnostics
    if let Some(ref token) = name_token {
        let name_text = token.text();

        if is_function && name_text.to_lowercase().starts_with("получить") {
            ctx.emit(BodyDiagnostic::FunctionNameStartsWithGet {
                name: name_text.to_string(),
                range: token.text_range(),
            });
        }

        if is_global_context_collision_8312(name_text) {
            ctx.emit(BodyDiagnostic::GlobalContextMethodCollision8312 {
                method_name: name_text.to_string(),
                range: token.text_range(),
            });
        }
    }

    // Lower parameters
    if let Some(param_list) = method_node.children().find(|n| n.kind() == SyntaxKind::PARAM_LIST) {
        let params = stmt::lower_params(&mut ctx, &param_list);
        ctx.body.params = params.into_boxed_slice();
    }

    // Lower body statements
    if let Some(stmt_list) = method_node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST) {
        let stmts = stmt::lower_stmt_list(&mut ctx, &stmt_list);
        ctx.body.body_stmts = stmts.into_boxed_slice();

        // Control flow checks (combined single-pass analysis)
        let cf_analysis = control_flow::analyze_control_flow(&stmt_list);

        if is_function {
            let name_range = name_token
                .as_ref()
                .map(|t| t.text_range())
                .unwrap_or_else(|| method_node.text_range());

            // Check for FunctionShouldHaveReturn (no return statement at all)
            if !cf_analysis.has_return {
                ctx.emit(BodyDiagnostic::FunctionShouldHaveReturn { range: name_range });
            }

            // Check for MissingReturn (some paths don't return)
            // NOTE: CFG-based analysis is performed in ide-diagnostics handler
            if cf_analysis.has_return {
                ctx.emit(BodyDiagnostic::MissingReturn { range: name_range });
            }
        }

        // Check for code after async calls
        diagnostics::check_code_after_async_call(&mut ctx, &cf_analysis.call_stmts[..]);
    }

    // Check for FunctionReturnsSamePrimitive
    if is_function {
        check_function_returns_same_primitive(&mut ctx, method_node);
    }

    // Emit OneStatementPerLine diagnostics (no-op without line_index)
    ctx.emit_one_statement_per_line_diagnostics();

    // Emit TooManyReturns diagnostic
    if let Some(ref token) = name_token {
        ctx.emit_too_many_returns_diagnostic(token.text().to_string(), token.text_range());
    }

    // Emit method-scoped diagnostics (complexity, size, params)
    if let Some(ref token) = name_token {
        emit_method_scoped_diagnostics(
            &mut ctx,
            token.text(),
            token.text_range(),
            method_node.text_range(),
            is_function,
        );
    }

    // Collect referenced externals (variables used but not declared locally)
    let referenced_externals = collect_referenced_externals(&ctx.body);

    LowerResult {
        body: ctx.body,
        source_map: ctx.source_map,
        diagnostics: ctx.diagnostics,
        referenced_externals,
        external_refs: ctx.external_refs,
    }
}

/// Lower module-level code (statements outside procedures/functions).
///
/// This handles initialization code that runs when the module is loaded.
/// When `line_index` is provided, OneStatementPerLine diagnostic is emitted.
pub fn lower_module_code(root: &SyntaxNode, line_index: Option<Arc<LineIndex>>) -> LowerResult {
    let mut ctx = LoweringCtx::new(false);

    if let Some(li) = line_index {
        ctx.set_line_index(li);
    }

    let mut stmts = Vec::new();

    for node in root.children() {
        if node.kind() == SyntaxKind::PRE_IF_DIR {
            if let Some(stmt) = preproc::lower_preproc_if(&mut ctx, &node) {
                let stmt_id = ctx.alloc_stmt(stmt, node.text_range());
                stmts.push(stmt_id);
            }
            continue;
        }
        if node.kind() == SyntaxKind::PRE_REGION_DIR {
            let (region_stmts, _region_terminates) = preproc::lower_region_stmts(&mut ctx, &node);
            stmts.extend(region_stmts);
            continue;
        }

        if !control_flow::is_statement_node(&node) {
            continue;
        }

        // Skip VAR_DEF - module-level Перем declarations are tracked separately
        // in lower_module_bodies via module_vars.
        if node.kind() == SyntaxKind::VAR_DEF {
            continue;
        }

        if let Some(stmt_id) = stmt::lower_stmt(&mut ctx, &node) {
            stmts.push(stmt_id);

            // Track statement line for OneStatementPerLine diagnostic (no-op without line_index)
            if !stmt::should_skip_one_statement_per_line(&node) {
                ctx.track_statement_line(node.text_range());
            }

            if !stmt::should_skip_semicolon_check(&node) && !stmt::has_trailing_semicolon(&node) {
                let range = stmt::get_last_meaningful_token_range(&node);
                ctx.emit(BodyDiagnostic::MissingSemicolon { range });
            }
        }
    }

    ctx.body.body_stmts = stmts.into_boxed_slice();

    // Emit OneStatementPerLine diagnostics (no-op without line_index)
    ctx.emit_one_statement_per_line_diagnostics();

    let referenced_externals = collect_referenced_externals(&ctx.body);

    LowerResult {
        body: ctx.body,
        source_map: ctx.source_map,
        diagnostics: ctx.diagnostics,
        referenced_externals,
        external_refs: ctx.external_refs,
    }
}

/// Collect variables that are referenced but not locally declared.
///
/// Scans all expressions in the body to find Path expressions,
/// then filters out locally declared variables (parameters, VarDecl, For/ForEach).
///
/// Returns lowercase variable names for case-insensitive comparison.
fn collect_referenced_externals(body: &Body) -> FxHashSet<String> {
    let mut referenced = FxHashSet::default();
    let mut declared = FxHashSet::default();

    // Collect all declared variables
    // 1. Parameters
    for &param_id in body.params.iter() {
        let binding = &body.bindings[param_id];
        declared.insert(binding.name.as_str().to_lowercase());
    }

    // 2. VarDecl bindings, For/ForEach variables (recursively scan all statements)
    fn collect_declared(body: &Body, stmt_id: StmtIdx, declared: &mut FxHashSet<String>) {
        match body.stmt_idx(stmt_id) {
            Stmt::VarDecl { bindings } => {
                for &binding_id in bindings.iter() {
                    let binding = &body.bindings[binding_id];
                    declared.insert(binding.name.as_str().to_lowercase());
                }
            }
            Stmt::For { var, body: loop_body, .. } => {
                let binding = &body.bindings[*var];
                declared.insert(binding.name.as_str().to_lowercase());
                for &s in loop_body.iter() {
                    collect_declared(body, s, declared);
                }
            }
            Stmt::ForEach { var, body: loop_body, .. } => {
                let binding = &body.bindings[*var];
                declared.insert(binding.name.as_str().to_lowercase());
                for &s in loop_body.iter() {
                    collect_declared(body, s, declared);
                }
            }
            Stmt::If(if_stmt) => {
                for &s in if_stmt.then_branch.iter() {
                    collect_declared(body, s, declared);
                }
                for (_, branch) in if_stmt.elsif_branches.iter() {
                    for &s in branch.iter() {
                        collect_declared(body, s, declared);
                    }
                }
                if let Some(ref else_stmts) = if_stmt.else_branch {
                    for &s in else_stmts.iter() {
                        collect_declared(body, s, declared);
                    }
                }
            }
            Stmt::While { body: loop_body, .. } => {
                for &s in loop_body.iter() {
                    collect_declared(body, s, declared);
                }
            }
            Stmt::Try { body: try_body, except } => {
                for &s in try_body.iter() {
                    collect_declared(body, s, declared);
                }
                for &s in except.iter() {
                    collect_declared(body, s, declared);
                }
            }
            _ => {}
        }
    }

    for &stmt_id in body.body_stmts.iter() {
        collect_declared(body, stmt_id, &mut declared);
    }

    // Collect all Path expressions (simple approach: scan all expressions in arena)
    for (_, expr) in body.exprs.iter() {
        if let Expr::Path(name) = expr {
            referenced.insert(name.as_str().to_lowercase());
        }
    }

    // Filter out declared variables
    referenced.retain(|name| !declared.contains(name));

    referenced
}
