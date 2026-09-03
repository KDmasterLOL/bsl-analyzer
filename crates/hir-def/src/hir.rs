use cfg_types::LocalRange;
use la_arena::Idx;
use ordered_float::NotNan;

use crate::Name;

pub type ExprIdx = Idx<Expr>;
pub type StmtIdx = Idx<Stmt>;
pub type BindingIdx = Idx<Binding>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Literal {
    Number(NotNan<f64>),
    String(String),
    Date(String),
    Bool(bool),
    Undefined,
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,

    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,

    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Neg,
    Not,
    Plus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Missing,

    Literal(Literal),

    Path(Name),

    BinaryOp { lhs: ExprIdx, rhs: ExprIdx, op: BinaryOp },

    UnaryOp { expr: ExprIdx, op: UnaryOp },

    Ternary { condition: ExprIdx, then_expr: ExprIdx, else_expr: ExprIdx },

    Call { callee: ExprIdx, args: Box<[ExprIdx]> },

    MethodCall { receiver: ExprIdx, method: Name, args: Box<[ExprIdx]> },

    Index { base: ExprIdx, index: ExprIdx },

    Field { base: ExprIdx, field: Name },

    New { type_name: Option<Name>, args: Box<[ExprIdx]> },

    Array(Box<[ExprIdx]>),

    Await { expr: ExprIdx },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfStmt {
    pub condition: ExprIdx,
    pub then_branch: Box<[StmtIdx]>,
    pub elsif_branches: Box<[(ExprIdx, Box<[StmtIdx]>)]>,
    pub else_branch: Option<Box<[StmtIdx]>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::type_complexity)]
pub struct PreprocIfStmt {
    pub condition_range: LocalRange,
    pub directive_range: LocalRange,
    pub full_range: LocalRange,
    /// Parsed `#Если` condition; [`PreprocCondition::Unknown`] when malformed.
    ///
    /// [`PreprocCondition::Unknown`]: crate::preproc_condition::PreprocCondition::Unknown
    pub condition: crate::preproc_condition::PreprocCondition,
    pub then_branch: Box<[StmtIdx]>,
    pub elsif_branches: Box<[(LocalRange, LocalRange, Box<[StmtIdx]>)]>,
    /// Parsed `#ИначеЕсли` conditions, aligned with `elsif_branches`.
    pub elsif_conditions: Box<[crate::preproc_condition::PreprocCondition]>,
    pub else_branch: Option<Box<[StmtIdx]>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirPreBranchKind {
    Then,
    ElsIf(usize),
    Else,
}

#[derive(Debug, Clone)]
pub struct HirPreBranch<'a> {
    pub kind: HirPreBranchKind,
    pub condition_range: Option<LocalRange>,
    pub directive_range: Option<LocalRange>,
    pub stmts: &'a [StmtIdx],
}

impl PreprocIfStmt {
    pub fn branches(&self) -> impl Iterator<Item = HirPreBranch<'_>> + '_ {
        std::iter::once(HirPreBranch {
            kind: HirPreBranchKind::Then,
            condition_range: Some(self.condition_range),
            directive_range: Some(self.directive_range),
            stmts: self.then_branch.as_ref(),
        })
        .chain(self.elsif_branches.iter().enumerate().map(|(i, elsif)| HirPreBranch {
            kind: HirPreBranchKind::ElsIf(i),
            condition_range: Some(elsif.0),
            directive_range: Some(elsif.1),
            stmts: elsif.2.as_ref(),
        }))
        .chain(self.else_branch.iter().map(|_| HirPreBranch {
            kind: HirPreBranchKind::Else,
            condition_range: None,
            directive_range: None,
            stmts: self.else_branch.as_deref().unwrap(),
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Expr(ExprIdx),

    Assign { target: ExprIdx, value: ExprIdx },

    VarDecl { bindings: Box<[BindingIdx]> },

    If(Box<IfStmt>),

    PreprocIf(Box<PreprocIfStmt>),

    While { condition: ExprIdx, body: Box<[StmtIdx]> },

    For { var: BindingIdx, from: ExprIdx, to: ExprIdx, body: Box<[StmtIdx]> },

    ForEach { var: BindingIdx, collection: ExprIdx, body: Box<[StmtIdx]> },

    Try { body: Box<[StmtIdx]>, except: Box<[StmtIdx]> },

    Return { value: Option<ExprIdx> },

    Raise { value: Option<ExprIdx> },

    Break,

    Continue,

    Goto(Name),

    Label(Name),

    Execute { expr: ExprIdx },

    AddHandler { event: ExprIdx, handler: ExprIdx },

    RemoveHandler { event: ExprIdx, handler: ExprIdx },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub name: Name,
    pub is_val: bool,
    pub default_value: Option<ExprIdx>,
}

impl Binding {
    pub fn new(name: Name, is_val: bool) -> Self {
        Self { name, is_val, default_value: None }
    }

    pub fn with_default(name: Name, is_val: bool, default_value: ExprIdx) -> Self {
        Self { name, is_val, default_value: Some(default_value) }
    }

    pub fn var(name: Name) -> Self {
        Self::new(name, false)
    }
}

#[cfg(test)]
mod preproc_if_stmt_branches_tests {
    use la_arena::{Idx, RawIdx};

    use super::{HirPreBranchKind, PreprocIfStmt, Stmt, StmtIdx};
    use cfg_types::LocalRange;

    fn stmt_idx(raw: u32) -> StmtIdx {
        Idx::<Stmt>::from_raw(RawIdx::from_u32(raw))
    }

    fn range(start: u32, end: u32) -> LocalRange {
        LocalRange::of_detached_node(text_size::TextRange::new(start.into(), end.into()))
    }

    #[test]
    fn branches_then_only() {
        let condition_range = range(0, 1);
        let stmt = PreprocIfStmt {
            condition_range,
            directive_range: range(0, 1),
            full_range: range(0, 1),
            condition: crate::preproc_condition::PreprocCondition::Unknown,
            elsif_conditions: Box::new([]),
            then_branch: Box::new([stmt_idx(0)]),
            elsif_branches: Box::new([]),
            else_branch: None,
        };

        let branches = stmt.branches().collect::<Vec<_>>();

        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].kind, HirPreBranchKind::Then);
        assert_eq!(branches[0].stmts.len(), 1);
        assert_eq!(branches[0].condition_range, Some(condition_range));
    }

    #[test]
    fn branches_then_and_else() {
        let stmt = PreprocIfStmt {
            condition_range: range(0, 1),
            directive_range: range(0, 1),
            full_range: range(0, 1),
            condition: crate::preproc_condition::PreprocCondition::Unknown,
            elsif_conditions: Box::new([]),
            then_branch: Box::new([stmt_idx(0)]),
            elsif_branches: Box::new([]),
            else_branch: Some(Box::new([stmt_idx(1)])),
        };

        let branches = stmt.branches().collect::<Vec<_>>();

        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].kind, HirPreBranchKind::Then);
        assert_eq!(branches[1].kind, HirPreBranchKind::Else);
        assert_eq!(branches[1].condition_range, None);
    }

    #[test]
    fn branches_full_chain() {
        let elsif_1_condition = range(1, 2);
        let elsif_1_directive = range(2, 3);
        let elsif_2_condition = range(3, 4);
        let elsif_2_directive = range(4, 5);
        let stmt = PreprocIfStmt {
            condition_range: range(0, 1),
            directive_range: range(0, 1),
            full_range: range(0, 1),
            condition: crate::preproc_condition::PreprocCondition::Unknown,
            elsif_conditions: Box::new([
                crate::preproc_condition::PreprocCondition::Unknown,
                crate::preproc_condition::PreprocCondition::Unknown,
            ]),
            then_branch: Box::new([stmt_idx(0)]),
            elsif_branches: Box::new([
                (elsif_1_condition, elsif_1_directive, Box::new([stmt_idx(1)])),
                (elsif_2_condition, elsif_2_directive, Box::new([stmt_idx(2)])),
            ]),
            else_branch: Some(Box::new([stmt_idx(3)])),
        };

        let branches = stmt.branches().collect::<Vec<_>>();

        assert_eq!(branches.len(), 4);
        assert_eq!(branches[0].kind, HirPreBranchKind::Then);
        assert_eq!(branches[1].kind, HirPreBranchKind::ElsIf(0));
        assert_eq!(branches[1].condition_range, Some(elsif_1_condition));
        assert_eq!(branches[1].directive_range, Some(elsif_1_directive));
        assert_eq!(branches[2].kind, HirPreBranchKind::ElsIf(1));
        assert_eq!(branches[2].condition_range, Some(elsif_2_condition));
        assert_eq!(branches[2].directive_range, Some(elsif_2_directive));
        assert_eq!(branches[3].kind, HirPreBranchKind::Else);
    }

    #[test]
    fn branches_elsif_no_else() {
        let elsif_condition = range(1, 2);
        let elsif_directive = range(2, 3);
        let stmt = PreprocIfStmt {
            condition_range: range(0, 1),
            directive_range: range(0, 1),
            full_range: range(0, 1),
            condition: crate::preproc_condition::PreprocCondition::Unknown,
            elsif_conditions: Box::new([crate::preproc_condition::PreprocCondition::Unknown]),
            then_branch: Box::new([stmt_idx(0)]),
            elsif_branches: Box::new([(elsif_condition, elsif_directive, Box::new([stmt_idx(1)]))]),
            else_branch: None,
        };

        let branches = stmt.branches().collect::<Vec<_>>();

        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].kind, HirPreBranchKind::Then);
        assert_eq!(branches[1].kind, HirPreBranchKind::ElsIf(0));
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
