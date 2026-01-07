//! CFG Vertex types
//!
//! Ported from BSL Language Server (Java) via bsl-language-server-rust:
//! - CfgVertex.java
//! - BasicBlockVertex.java
//! - ConditionalVertex.java
//! - BranchingVertex.java
//! - LoopVertex.java (WhileLoopVertex, ForLoopVertex, ForeachLoopVertex)
//! - TryExceptVertex.java
//! - LabelVertex.java
//! - ExitVertex.java
//!
//! ## Design Decision: HIR-based CFG
//!
//! **Migrated from SyntaxNode to HIR indices (Phase 6.1)**
//!
//! Unlike the previous AST-based approach, we now store HIR indices (StmtId, ExprId, BindingId)
//! from hir_def::Body arenas.
//!
//! **Rationale**:
//! - Enables dataflow analysis (needs HIR statement access)
//! - More compact (8-byte indices vs Arc<SyntaxNode>)
//! - Type-safe (StmtId can only reference statements)
//! - No fragile AST parsing with find() + fallbacks
//! - Direct integration with HIR-based diagnostics
//!
//! **Advantage**:
//! - Dataflow transfer functions can access Body arenas
//! - Same Body used in CFG and diagnostics (single source of truth)
//! - Structured access via pattern matching (no tree traversal)
//! - Follows rust-analyzer patterns

use hir_def::{BindingId, ExprId, Name, StmtId};

/// Vertex in the control flow graph
///
/// Maps to CfgVertex hierarchy in Java
#[derive(Debug, Clone)]
pub enum CfgVertex {
    /// Basic block - sequence of sequential statements
    /// Maps to BasicBlockVertex.java
    BasicBlock(BasicBlockVertex),

    /// Conditional branching (if/elsif)
    /// Maps to ConditionalVertex.java
    Conditional(ConditionalVertex),

    /// While loop
    /// Maps to WhileLoopVertex.java
    WhileLoop(WhileLoopVertex),

    /// For loop
    /// Maps to ForLoopVertex.java
    ForLoop(ForLoopVertex),

    /// ForEach loop
    /// Maps to ForeachLoopVertex.java
    ForEachLoop(ForEachLoopVertex),

    /// Try-Except block
    /// Maps to TryExceptVertex.java
    TryExcept(TryExceptVertex),

    /// Label (target for Goto)
    /// Maps to LabelVertex.java
    Label(LabelVertex),

    /// Exit point of the method
    /// Maps to ExitVertex.java
    Exit,
}

impl CfgVertex {
    /// Get the first statement ID from a BasicBlock vertex, if this is a BasicBlock
    ///
    /// For other vertex types, use specific accessors:
    /// - Conditional/WhileLoop: access `.condition` field directly
    /// - ForLoop/ForEachLoop: access `.loop_var` field directly
    /// - Label: access `.name` field directly
    pub fn first_stmt_id(&self) -> Option<StmtId> {
        match self {
            CfgVertex::BasicBlock(v) => v.statements().first().copied(),
            _ => None,
        }
    }

    /// Get a display name for this vertex type
    pub fn type_name(&self) -> &'static str {
        match self {
            CfgVertex::BasicBlock(_) => "BasicBlock",
            CfgVertex::Conditional(_) => "Conditional",
            CfgVertex::WhileLoop(_) => "WhileLoop",
            CfgVertex::ForLoop(_) => "ForLoop",
            CfgVertex::ForEachLoop(_) => "ForEachLoop",
            CfgVertex::TryExcept(_) => "TryExcept",
            CfgVertex::Label(_) => "Label",
            CfgVertex::Exit => "Exit",
        }
    }

    /// Check if this is a branching vertex (requires multiple outgoing edges)
    pub fn is_branching(&self) -> bool {
        matches!(
            self,
            CfgVertex::Conditional(_)
                | CfgVertex::WhileLoop(_)
                | CfgVertex::ForLoop(_)
                | CfgVertex::ForEachLoop(_)
                | CfgVertex::TryExcept(_)
        )
    }

    /// Check if this is a loop vertex
    pub fn is_loop(&self) -> bool {
        matches!(self, CfgVertex::WhileLoop(_) | CfgVertex::ForLoop(_) | CfgVertex::ForEachLoop(_))
    }
}

/// Basic block vertex - sequence of statements with no branches
///
/// Maps to BasicBlockVertex.java
#[derive(Debug, Clone)]
pub struct BasicBlockVertex {
    /// Sequential statements in this basic block
    /// Stores HIR statement indices from Body arena
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

/// Conditional vertex - if/elsif branching
///
/// Maps to ConditionalVertex.java
#[derive(Debug, Clone)]
pub struct ConditionalVertex {
    /// Condition expression (HIR index from Body)
    pub condition: ExprId,
}

impl ConditionalVertex {
    pub fn new(condition: ExprId) -> Self {
        Self { condition }
    }
}

/// While loop vertex
///
/// Maps to WhileLoopVertex.java
#[derive(Debug, Clone)]
pub struct WhileLoopVertex {
    /// Loop condition (HIR index from Body)
    pub condition: ExprId,
}

impl WhileLoopVertex {
    pub fn new(condition: ExprId) -> Self {
        Self { condition }
    }

    // TODO(Phase 6.4): Re-add is_endless() method with Body parameter
    // Check if this is an endless loop (While True / Пока Истина)
    // Requires access to Body to check if condition is Literal::Bool(true)
    //
    // pub fn is_endless(&self, body: &hir_def::Body) -> bool {
    //     matches!(body.expr(self.condition), hir_def::Expr::Literal(hir_def::Literal::Bool(true)))
    // }
}

/// For loop vertex
///
/// Maps to ForLoopVertex.java
#[derive(Debug, Clone)]
pub struct ForLoopVertex {
    /// Loop variable (HIR binding from Body)
    pub loop_var: BindingId,
}

impl ForLoopVertex {
    pub fn new(loop_var: BindingId) -> Self {
        Self { loop_var }
    }
}

/// ForEach loop vertex
///
/// Maps to ForeachLoopVertex.java
#[derive(Debug, Clone)]
pub struct ForEachLoopVertex {
    /// Loop variable (HIR binding from Body)
    pub loop_var: BindingId,
}

impl ForEachLoopVertex {
    pub fn new(loop_var: BindingId) -> Self {
        Self { loop_var }
    }
}

/// Try-Except block vertex
///
/// Maps to TryExceptVertex.java
///
/// Note: CFG doesn't need to store any data for Try-Except blocks,
/// just mark their presence in the control flow. Body statements
/// are tracked in BasicBlock vertices.
#[derive(Debug, Clone)]
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

/// Label vertex (for Goto statements)
///
/// Maps to LabelVertex.java
#[derive(Debug, Clone)]
pub struct LabelVertex {
    /// Label name (HIR Name from Body)
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
