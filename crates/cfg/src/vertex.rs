use cfg_types::LocalRange;
use cfg_types::{BindingId, ExprId, StmtId};
use hir_def::Name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfgVertex {
    BasicBlock(BasicBlockVertex),

    Conditional(ConditionalVertex),

    WhileLoop(WhileLoopVertex),

    ForLoop(ForLoopVertex),

    ForEachLoop(ForEachLoopVertex),

    TryExcept(TryExceptVertex),

    Label(LabelVertex),

    PreprocCondition(PreprocConditionVertex),

    Exit,
}

impl CfgVertex {
    pub fn first_stmt_id(&self) -> Option<StmtId> {
        match self {
            CfgVertex::BasicBlock(v) => v.statements().first().copied(),
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            CfgVertex::BasicBlock(_) => "BasicBlock",
            CfgVertex::Conditional(_) => "Conditional",
            CfgVertex::WhileLoop(_) => "WhileLoop",
            CfgVertex::ForLoop(_) => "ForLoop",
            CfgVertex::ForEachLoop(_) => "ForEachLoop",
            CfgVertex::TryExcept(_) => "TryExcept",
            CfgVertex::Label(_) => "Label",
            CfgVertex::PreprocCondition(_) => "PreprocCondition",
            CfgVertex::Exit => "Exit",
        }
    }

    pub fn is_branching(&self) -> bool {
        matches!(
            self,
            CfgVertex::Conditional(_)
                | CfgVertex::WhileLoop(_)
                | CfgVertex::ForLoop(_)
                | CfgVertex::ForEachLoop(_)
                | CfgVertex::TryExcept(_)
                | CfgVertex::PreprocCondition(_)
        )
    }

    pub fn is_loop(&self) -> bool {
        matches!(self, CfgVertex::WhileLoop(_) | CfgVertex::ForLoop(_) | CfgVertex::ForEachLoop(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicBlockVertex {
    statements: Vec<StmtId>,
}

impl BasicBlockVertex {
    pub fn new() -> Self {
        Self { statements: Vec::new() }
    }

    pub fn add_statement(&mut self, stmt: StmtId) {
        self.statements.push(stmt);
    }

    pub fn statements(&self) -> &[StmtId] {
        &self.statements
    }

    pub fn first_statement(&self) -> Option<StmtId> {
        self.statements.first().copied()
    }

    pub fn last_statement(&self) -> Option<StmtId> {
        self.statements.last().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }

    pub fn len(&self) -> usize {
        self.statements.len()
    }
}

impl Default for BasicBlockVertex {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalVertex {
    pub condition: ExprId,
}

impl ConditionalVertex {
    pub fn new(condition: ExprId) -> Self {
        Self { condition }
    }
}

/// Ranges are those of the lowered body: relative to the method root, so the
/// graph of a method is the same value wherever the method sits in its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocConditionVertex {
    pub condition_range: LocalRange,
    pub directive_range: Option<LocalRange>,
    pub full_range: Option<LocalRange>,
}

impl PreprocConditionVertex {
    pub fn new(condition_range: LocalRange) -> Self {
        Self { condition_range, directive_range: None, full_range: None }
    }

    pub fn with_directive_range(condition_range: LocalRange, directive_range: LocalRange) -> Self {
        Self { condition_range, directive_range: Some(directive_range), full_range: None }
    }

    pub fn with_ranges(
        condition_range: LocalRange,
        directive_range: LocalRange,
        full_range: LocalRange,
    ) -> Self {
        Self {
            condition_range,
            directive_range: Some(directive_range),
            full_range: Some(full_range),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhileLoopVertex {
    pub condition: ExprId,
}

impl WhileLoopVertex {
    pub fn new(condition: ExprId) -> Self {
        Self { condition }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForLoopVertex {
    pub loop_var: BindingId,
    pub from: ExprId,
    pub to: ExprId,
    pub stmt_id: Option<StmtId>,
}

impl ForLoopVertex {
    pub fn new(loop_var: BindingId, from: ExprId, to: ExprId) -> Self {
        Self { loop_var, from, to, stmt_id: None }
    }

    pub fn with_stmt_id(loop_var: BindingId, from: ExprId, to: ExprId, stmt_id: StmtId) -> Self {
        Self { loop_var, from, to, stmt_id: Some(stmt_id) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForEachLoopVertex {
    pub loop_var: BindingId,
    pub collection: ExprId,
    pub stmt_id: Option<StmtId>,
}

impl ForEachLoopVertex {
    pub fn new(loop_var: BindingId, collection: ExprId) -> Self {
        Self { loop_var, collection, stmt_id: None }
    }

    pub fn with_stmt_id(loop_var: BindingId, collection: ExprId, stmt_id: StmtId) -> Self {
        Self { loop_var, collection, stmt_id: Some(stmt_id) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TryExceptVertex;

impl TryExceptVertex {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TryExceptVertex {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelVertex {
    pub name: Name,
}

impl LabelVertex {
    pub fn new(name: Name) -> Self {
        Self { name }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_block_empty() {
        let block = BasicBlockVertex::new();
        assert!(block.is_empty());
        assert_eq!(block.len(), 0);
    }

    #[test]
    fn test_vertex_type_names() {
        let exit = CfgVertex::Exit;
        assert_eq!(exit.type_name(), "Exit");
        assert!(!exit.is_branching());
        assert!(!exit.is_loop());
    }

    #[test]
    fn test_branching_vertices() {
        let block = CfgVertex::BasicBlock(BasicBlockVertex::new());
        assert!(!block.is_branching());
        assert!(!block.is_loop());
    }
}
