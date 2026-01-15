//! HIR expressions and statements.
//!
//! This module defines the High-level Intermediate Representation (HIR) for BSL code.
//! HIR is a simplified, semantically-meaningful representation of code that:
//! - Is easier to analyze than AST
//! - Supports efficient diagnostics collection during lowering
//! - Enables Salsa caching for incremental computation
//!
//! ## Architecture
//!
//! ```text
//! AST (syntax) → HIR (hir-def) → Diagnostics + Type inference
//!     │              │
//!     │              └── Simplified, semantic representation
//!     └── Full-fidelity, syntactic representation
//! ```
//!
//! ## Key differences from AST
//!
//! - HIR uses arena-allocated IDs instead of tree pointers
//! - HIR normalizes equivalent constructs (e.g., `a + b` and `a.Add(b)`)
//! - HIR drops syntactic sugar and preserves only semantic information
//! - Diagnostics are collected during AST → HIR lowering

use la_arena::Idx;
use ordered_float::NotNan;

use crate::Name;

// Typed arena indices for internal use (lowering, cfg building).
// For opaque IDs in public APIs, use cfg_types::{ExprId, StmtId, BindingId}.
pub type ExprIdx = Idx<Expr>;
pub type StmtIdx = Idx<Stmt>;
pub type BindingIdx = Idx<Binding>;

/// Literal value in BSL.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Literal {
    /// Numeric literal (integer or float).
    /// BSL doesn't distinguish between int and float at syntax level.
    /// Uses NotNan<f64> to enable Eq/Hash traits required by Salsa.
    Number(NotNan<f64>),
    /// String literal.
    String(String),
    /// Date literal ('YYYYMMDD' or 'YYYYMMDDHHmmss').
    Date(String),
    /// Boolean literal (Истина/True or Ложь/False).
    Bool(bool),
    /// Undefined value (Неопределено/Undefined).
    Undefined,
    /// Null value (Null).
    Null,
}

/// Binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,

    // Comparison
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,

    // Logical
    And,
    Or,
}

/// Unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// Negation (-)
    Neg,
    /// Logical NOT (Не/Not)
    Not,
    /// Unary plus (+)
    Plus,
}

/// HIR expression.
///
/// Represents a value-producing construct in BSL code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Placeholder for parse errors or missing expressions.
    Missing,

    /// Literal value (number, string, date, boolean, undefined, null).
    Literal(Literal),

    /// Variable or identifier reference.
    Path(Name),

    /// Qualified name (multi-segment path like Module.Method or Documents.PKO.Create).
    ///
    /// Used for:
    /// - CommonModule calls: `ОбщийМодуль.Метод()`
    /// - Manager module calls: `Документы.ПКО.Создать()`
    /// - Chained field access that requires resolution
    ///
    /// Note: Boxed to reduce Expr enum size from 64 to 48 bytes.
    QualifiedPath(Box<crate::path::QualifiedName>),

    /// Binary operation (a + b, a И b, etc.).
    BinaryOp { lhs: ExprIdx, rhs: ExprIdx, op: BinaryOp },

    /// Unary operation (-a, Не a).
    UnaryOp { expr: ExprIdx, op: UnaryOp },

    /// Ternary conditional expression (?(condition, then, else)).
    Ternary { condition: ExprIdx, then_expr: ExprIdx, else_expr: ExprIdx },

    /// Function/procedure call (Func(args)).
    Call { callee: ExprIdx, args: Box<[ExprIdx]> },

    /// Method call (obj.Method(args)).
    MethodCall { receiver: ExprIdx, method: Name, args: Box<[ExprIdx]> },

    /// Index access (array[index]).
    Index { base: ExprIdx, index: ExprIdx },

    /// Field access (obj.field).
    Field { base: ExprIdx, field: Name },

    /// New expression (Новый Type(args) or New Type(args)).
    New { type_name: Option<Name>, args: Box<[ExprIdx]> },

    /// Array literal ([a, b, c]).
    /// Note: BSL doesn't have array literals in syntax, but we may need this for analysis.
    Array(Box<[ExprIdx]>),

    /// Await expression (Ждать expr).
    Await { expr: ExprIdx },
}

/// If statement data.
///
/// Boxed in `Stmt::If` to reduce enum size from 56 to 32 bytes,
/// saving ~313 MB for large projects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfStmt {
    pub condition: ExprIdx,
    pub then_branch: Box<[StmtIdx]>,
    pub elsif_branches: Box<[(ExprIdx, Box<[StmtIdx]>)]>,
    pub else_branch: Option<Box<[StmtIdx]>>,
}

/// HIR statement.
///
/// Represents an executable construct in BSL code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// Expression statement (standalone expression like function call).
    Expr(ExprIdx),

    /// Assignment statement (target = value).
    Assign { target: ExprIdx, value: ExprIdx },

    /// Variable declaration (Перем a, b, c).
    VarDecl { bindings: Box<[BindingIdx]> },

    /// If statement (boxed to reduce enum size).
    If(Box<IfStmt>),

    /// While loop (Пока condition Цикл ... КонецЦикла).
    While { condition: ExprIdx, body: Box<[StmtIdx]> },

    /// For loop (Для var = from По to Цикл ... КонецЦикла).
    For { var: BindingIdx, from: ExprIdx, to: ExprIdx, body: Box<[StmtIdx]> },

    /// For-each loop (Для Каждого var Из collection Цикл ... КонецЦикла).
    ForEach { var: BindingIdx, collection: ExprIdx, body: Box<[StmtIdx]> },

    /// Try-except block.
    Try { body: Box<[StmtIdx]>, except: Box<[StmtIdx]> },

    /// Return statement (Возврат value).
    Return { value: Option<ExprIdx> },

    /// Raise statement (ВызватьИсключение value).
    Raise { value: Option<ExprIdx> },

    /// Break statement (Прервать).
    Break,

    /// Continue statement (Продолжить).
    Continue,

    /// Goto statement (Перейти ~Label).
    Goto(Name),

    /// Label statement (~Label:).
    Label(Name),

    /// Execute statement (Выполнить expr).
    Execute { expr: ExprIdx },

    /// AddHandler statement (ДобавитьОбработчик event, handler).
    AddHandler { event: ExprIdx, handler: ExprIdx },

    /// RemoveHandler statement (УдалитьОбработчик event, handler).
    RemoveHandler { event: ExprIdx, handler: ExprIdx },
}

/// Local binding (variable or parameter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// Variable name.
    pub name: Name,
    /// Is this a value parameter (Знач)?
    pub is_val: bool,
    /// Default value for parameter (if any).
    /// Only set for function/procedure parameters with default values.
    pub default_value: Option<ExprIdx>,
}

impl Binding {
    /// Create a new binding.
    pub fn new(name: Name, is_val: bool) -> Self {
        Self { name, is_val, default_value: None }
    }

    /// Create a new parameter binding with default value.
    pub fn with_default(name: Name, is_val: bool, default_value: ExprIdx) -> Self {
        Self { name, is_val, default_value: Some(default_value) }
    }

    /// Create a binding for a regular variable (not a value parameter).
    pub fn var(name: Name) -> Self {
        Self::new(name, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_creation() {
        let num = Literal::Number(NotNan::new(42.0).unwrap());
        let str_lit = Literal::String("test".to_string());
        let bool_lit = Literal::Bool(true);
        let undef = Literal::Undefined;
        let null = Literal::Null;

        assert_eq!(num, Literal::Number(NotNan::new(42.0).unwrap()));
        assert_eq!(str_lit, Literal::String("test".to_string()));
        assert_eq!(bool_lit, Literal::Bool(true));
        assert_eq!(undef, Literal::Undefined);
        assert_eq!(null, Literal::Null);
    }

    #[test]
    fn test_binary_op() {
        let ops = [
            BinaryOp::Add,
            BinaryOp::Sub,
            BinaryOp::Mul,
            BinaryOp::Div,
            BinaryOp::Mod,
            BinaryOp::Eq,
            BinaryOp::Neq,
            BinaryOp::Lt,
            BinaryOp::Le,
            BinaryOp::Gt,
            BinaryOp::Ge,
            BinaryOp::And,
            BinaryOp::Or,
        ];

        // All operators should be distinct
        for (i, op1) in ops.iter().enumerate() {
            for (j, op2) in ops.iter().enumerate() {
                if i == j {
                    assert_eq!(op1, op2);
                } else {
                    assert_ne!(op1, op2);
                }
            }
        }
    }

    #[test]
    fn test_unary_op() {
        let neg = UnaryOp::Neg;
        let not = UnaryOp::Not;
        let plus = UnaryOp::Plus;

        assert_ne!(neg, not);
        assert_ne!(neg, plus);
        assert_ne!(not, plus);
    }

    #[test]
    fn test_binding() {
        let var_binding = Binding::var(Name::new("Переменная"));
        assert!(!var_binding.is_val);
        assert_eq!(var_binding.name.as_str(), "Переменная");

        let val_binding = Binding::new(Name::new("Параметр"), true);
        assert!(val_binding.is_val);
    }

    #[test]
    fn test_expr_missing() {
        let expr = Expr::Missing;
        assert!(matches!(expr, Expr::Missing));
    }

    #[test]
    fn test_stmt_size() {
        let stmt_size = std::mem::size_of::<Stmt>();
        // After Box<IfStmt> optimization, Stmt should be 40 bytes (down from 56)
        // The largest variant is now Try with two Box<[StmtIdx]> = 32 bytes
        assert!(
            stmt_size <= 40,
            "Stmt size {} bytes exceeds expected 40 bytes. Consider boxing large variants.",
            stmt_size
        );
        println!("Stmt size: {} bytes", stmt_size);
        println!("IfStmt size: {} bytes", std::mem::size_of::<IfStmt>());
        println!("Expr size: {} bytes", std::mem::size_of::<Expr>());
    }
}
