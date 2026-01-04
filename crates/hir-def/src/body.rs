//! Method body representation and source mapping.
//!
//! This module contains:
//! - `Body` - HIR representation of a method body
//! - `BodySourceMap` - bidirectional mapping between HIR and AST
//!
//! ## Architecture
//!
//! ```text
//! AST (Method body)
//!       │
//!       ▼ lower()
//! ┌─────────────────────────────────────┐
//! │           Body                      │
//! │  ┌─────────────────────────────┐   │
//! │  │ exprs: Arena<Expr>          │   │  HIR expressions
//! │  │ stmts: Arena<Stmt>          │   │  HIR statements
//! │  │ bindings: Arena<Binding>    │   │  Local variables/params
//! │  │ params: Box<[BindingId]>    │   │  Parameter IDs
//! │  │ body_stmts: Box<[StmtId]>   │   │  Top-level statements
//! │  └─────────────────────────────┘   │
//! │           +                         │
//! │  ┌─────────────────────────────┐   │
//! │  │ BodySourceMap               │   │  HIR ↔ AST mapping
//! │  │ expr_map: ExprId → AstPtr   │   │  For diagnostics
//! │  │ stmt_map: StmtId → AstPtr   │   │
//! │  └─────────────────────────────┘   │
//! └─────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! Body is created by lowering an AST method definition and is cached by Salsa.
//! Diagnostics are collected during lowering and returned alongside the Body.

pub mod lower;

use la_arena::Arena;
use rustc_hash::FxHashMap;
use syntax::SyntaxNode;
use text_size::TextRange;

use crate::hir::{Binding, BindingId, Expr, ExprId, Stmt, StmtId};

/// HIR representation of a method body.
///
/// Contains all expressions, statements, and bindings in arena-allocated form.
/// This allows efficient storage and stable IDs for referencing HIR nodes.
#[derive(Debug)]
pub struct Body {
    /// All expressions in this body.
    pub exprs: Arena<Expr>,
    /// All statements in this body.
    pub stmts: Arena<Stmt>,
    /// All local bindings (variables and parameters).
    pub bindings: Arena<Binding>,
    /// Parameter binding IDs (in declaration order).
    pub params: Box<[BindingId]>,
    /// Top-level statements in the method body.
    pub body_stmts: Box<[StmtId]>,
}

impl Default for Body {
    fn default() -> Self {
        Self::new()
    }
}

impl Body {
    /// Create a new empty body.
    pub fn new() -> Self {
        Self {
            exprs: Arena::new(),
            stmts: Arena::new(),
            bindings: Arena::new(),
            params: Box::new([]),
            body_stmts: Box::new([]),
        }
    }

    /// Get an expression by ID.
    pub fn expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id]
    }

    /// Get a statement by ID.
    pub fn stmt(&self, id: StmtId) -> &Stmt {
        &self.stmts[id]
    }

    /// Get a binding by ID.
    pub fn binding(&self, id: BindingId) -> &Binding {
        &self.bindings[id]
    }

    /// Get the number of expressions.
    pub fn expr_count(&self) -> usize {
        self.exprs.len()
    }

    /// Get the number of statements.
    pub fn stmt_count(&self) -> usize {
        self.stmts.len()
    }

    /// Get the number of bindings.
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    /// Iterate over all expressions.
    pub fn exprs_iter(&self) -> impl Iterator<Item = (ExprId, &Expr)> {
        self.exprs.iter()
    }

    /// Iterate over all statements.
    pub fn stmts_iter(&self) -> impl Iterator<Item = (StmtId, &Stmt)> {
        self.stmts.iter()
    }

    /// Iterate over all bindings.
    pub fn bindings_iter(&self) -> impl Iterator<Item = (BindingId, &Binding)> {
        self.bindings.iter()
    }
}

/// Bidirectional mapping between HIR and AST.
///
/// Used for:
/// - Diagnostics: HIR node → source location
/// - Go-to-definition: source location → HIR node
#[derive(Debug, Default)]
pub struct BodySourceMap {
    /// Expression ID → source range.
    expr_ranges: FxHashMap<ExprId, TextRange>,
    /// Statement ID → source range.
    stmt_ranges: FxHashMap<StmtId, TextRange>,
    /// Binding ID → source range.
    binding_ranges: FxHashMap<BindingId, TextRange>,

    /// Source range → Expression ID (for reverse lookup).
    range_to_expr: FxHashMap<TextRange, ExprId>,
    /// Source range → Statement ID (for reverse lookup).
    range_to_stmt: FxHashMap<TextRange, StmtId>,
}

impl BodySourceMap {
    /// Create a new empty source map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record expression source range.
    pub fn record_expr(&mut self, id: ExprId, range: TextRange) {
        self.expr_ranges.insert(id, range);
        self.range_to_expr.insert(range, id);
    }

    /// Record statement source range.
    pub fn record_stmt(&mut self, id: StmtId, range: TextRange) {
        self.stmt_ranges.insert(id, range);
        self.range_to_stmt.insert(range, id);
    }

    /// Record binding source range.
    pub fn record_binding(&mut self, id: BindingId, range: TextRange) {
        self.binding_ranges.insert(id, range);
    }

    /// Get source range for an expression.
    pub fn expr_range(&self, id: ExprId) -> Option<TextRange> {
        self.expr_ranges.get(&id).copied()
    }

    /// Get source range for a statement.
    pub fn stmt_range(&self, id: StmtId) -> Option<TextRange> {
        self.stmt_ranges.get(&id).copied()
    }

    /// Get source range for a binding.
    pub fn binding_range(&self, id: BindingId) -> Option<TextRange> {
        self.binding_ranges.get(&id).copied()
    }

    /// Find expression at a given range.
    pub fn expr_at_range(&self, range: TextRange) -> Option<ExprId> {
        self.range_to_expr.get(&range).copied()
    }

    /// Find statement at a given range.
    pub fn stmt_at_range(&self, range: TextRange) -> Option<StmtId> {
        self.range_to_stmt.get(&range).copied()
    }
}

/// Result of body lowering.
///
/// Contains the lowered body, source map, and any diagnostics collected during lowering.
#[derive(Debug)]
pub struct LowerResult {
    /// The lowered HIR body.
    pub body: Body,
    /// Mapping between HIR and AST.
    pub source_map: BodySourceMap,
    /// Diagnostics collected during lowering.
    pub diagnostics: Vec<BodyDiagnostic>,
    /// Variables referenced but not locally declared (potential module-level variables).
    /// Lowercase names for case-insensitive comparison.
    pub referenced_externals: rustc_hash::FxHashSet<String>,
}

/// Diagnostic collected during body lowering.
///
/// These diagnostics are emitted as a byproduct of lowering AST to HIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyDiagnostic {
    /// Missing return statement in function.
    MissingReturn { range: TextRange },

    /// Unreachable code after return/raise/break/continue.
    UnreachableCode { range: TextRange },

    /// Empty code block (if/while/for/try with empty body).
    EmptyCodeBlock { range: TextRange },

    /// Deprecated method call.
    DeprecatedMethod { name: String, range: TextRange },

    /// Magic number literal (hardcoded number that should be a constant).
    /// Value is stored as string to allow Eq derivation.
    MagicNumber { value: String, range: TextRange },

    /// Self-assignment (a = a).
    SelfAssign { range: TextRange },

    /// Unused local variable.
    UnusedVariable { name: String, range: TextRange },

    /// Function should have return.
    FunctionShouldHaveReturn { range: TextRange },
}

impl BodyDiagnostic {
    /// Get the source range of this diagnostic.
    pub fn range(&self) -> TextRange {
        match self {
            BodyDiagnostic::MissingReturn { range } => *range,
            BodyDiagnostic::UnreachableCode { range } => *range,
            BodyDiagnostic::EmptyCodeBlock { range } => *range,
            BodyDiagnostic::DeprecatedMethod { range, .. } => *range,
            BodyDiagnostic::MagicNumber { range, .. } => *range,
            BodyDiagnostic::SelfAssign { range } => *range,
            BodyDiagnostic::UnusedVariable { range, .. } => *range,
            BodyDiagnostic::FunctionShouldHaveReturn { range } => *range,
        }
    }
}

/// Lower a method AST node to HIR Body.
///
/// This is the main entry point for body lowering.
pub fn lower_method(method_node: &SyntaxNode, is_function: bool) -> LowerResult {
    lower::lower_method(method_node, is_function)
}

/// Lower module-level code (statements outside procedures/functions).
///
/// This handles initialization code that runs when the module is loaded.
pub fn lower_module_code(root: &SyntaxNode) -> LowerResult {
    lower::lower_module_code(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::{Binding, Expr, Literal, Stmt};
    use crate::Name;

    #[test]
    fn test_body_creation() {
        let body = Body::new();
        assert_eq!(body.expr_count(), 0);
        assert_eq!(body.stmt_count(), 0);
        assert_eq!(body.binding_count(), 0);
        assert_eq!(body.params.len(), 0);
        assert_eq!(body.body_stmts.len(), 0);
    }

    #[test]
    fn test_body_with_expressions() {
        let mut body = Body::new();

        let expr_id = body.exprs.alloc(Expr::Literal(Literal::Number(42.0)));
        assert_eq!(body.expr_count(), 1);

        let expr = body.expr(expr_id);
        assert!(matches!(expr, Expr::Literal(Literal::Number(n)) if *n == 42.0));
    }

    #[test]
    fn test_body_with_statements() {
        let mut body = Body::new();

        let expr_id = body.exprs.alloc(Expr::Literal(Literal::Number(1.0)));
        let stmt_id = body.stmts.alloc(Stmt::Expr(expr_id));

        assert_eq!(body.stmt_count(), 1);
        let stmt = body.stmt(stmt_id);
        assert!(matches!(stmt, Stmt::Expr(id) if *id == expr_id));
    }

    #[test]
    fn test_body_with_bindings() {
        let mut body = Body::new();

        let binding_id = body.bindings.alloc(Binding::var(Name::new("Переменная")));
        assert_eq!(body.binding_count(), 1);

        let binding = body.binding(binding_id);
        assert_eq!(binding.name.as_str(), "Переменная");
        assert!(!binding.is_val);
    }

    #[test]
    fn test_source_map() {
        let mut body = Body::new();
        let mut source_map = BodySourceMap::new();

        let expr_id = body.exprs.alloc(Expr::Literal(Literal::Number(42.0)));
        let range = TextRange::new(0.into(), 2.into());

        source_map.record_expr(expr_id, range);

        assert_eq!(source_map.expr_range(expr_id), Some(range));
        assert_eq!(source_map.expr_at_range(range), Some(expr_id));
    }

    #[test]
    fn test_body_diagnostic_range() {
        let range = TextRange::new(10.into(), 20.into());

        let diagnostics = vec![
            BodyDiagnostic::MissingReturn { range },
            BodyDiagnostic::UnreachableCode { range },
            BodyDiagnostic::EmptyCodeBlock { range },
            BodyDiagnostic::DeprecatedMethod { name: "Test".to_string(), range },
            BodyDiagnostic::MagicNumber { value: "42".to_string(), range },
            BodyDiagnostic::SelfAssign { range },
            BodyDiagnostic::UnusedVariable { name: "x".to_string(), range },
            BodyDiagnostic::FunctionShouldHaveReturn { range },
        ];

        for diag in diagnostics {
            assert_eq!(diag.range(), range);
        }
    }
}
