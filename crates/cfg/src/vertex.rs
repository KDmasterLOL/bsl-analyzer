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
//! ## Design Decision: Rowan SyntaxNode Storage
//!
//! Unlike bsl-language-server-rust which stores byte positions (NodePosition)
//! to avoid tree-sitter lifetime issues, we store Rowan SyntaxNode directly.
//!
//! **Rationale**:
//! - Rowan SyntaxNode uses Arc internally - cheap to clone
//! - No lifetime issues - can be stored safely in graph structures
//! - Direct access to AST nodes without needing Tree + source
//!
//! **Advantage**:
//! - Simpler API - no position conversion needed
//! - Type-safe access to AST information
//! - Follows rust-analyzer patterns

use syntax::SyntaxNode;

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
    /// Get the AST node associated with this vertex, if any
    pub fn node(&self) -> Option<&SyntaxNode> {
        match self {
            CfgVertex::BasicBlock(v) => v.first_statement(),
            CfgVertex::Conditional(v) => Some(&v.condition),
            CfgVertex::WhileLoop(v) => Some(&v.condition),
            CfgVertex::ForLoop(v) => Some(&v.loop_var),
            CfgVertex::ForEachLoop(v) => Some(&v.loop_var),
            CfgVertex::TryExcept(v) => Some(&v.try_node),
            CfgVertex::Label(v) => Some(&v.label),
            CfgVertex::Exit => None,
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
    /// Rowan SyntaxNode can be stored directly (Arc-based)
    statements: Vec<SyntaxNode>,
}

impl BasicBlockVertex {
    pub fn new() -> Self {
        Self { statements: Vec::new() }
    }

    pub fn add_statement(&mut self, stmt: SyntaxNode) {
        self.statements.push(stmt);
    }

    pub fn statements(&self) -> &[SyntaxNode] {
        &self.statements
    }

    pub fn first_statement(&self) -> Option<&SyntaxNode> {
        self.statements.first()
    }

    pub fn last_statement(&self) -> Option<&SyntaxNode> {
        self.statements.last()
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
    /// Condition expression (Rowan node)
    pub condition: SyntaxNode,
}

impl ConditionalVertex {
    pub fn new(condition: SyntaxNode) -> Self {
        Self { condition }
    }
}

/// While loop vertex
///
/// Maps to WhileLoopVertex.java
#[derive(Debug, Clone)]
pub struct WhileLoopVertex {
    /// Loop condition (Rowan node)
    pub condition: SyntaxNode,
}

impl WhileLoopVertex {
    pub fn new(condition: SyntaxNode) -> Self {
        Self { condition }
    }

    /// Check if this is an endless loop (While True / Пока Истина)
    pub fn is_endless(&self) -> bool {
        use syntax::SyntaxKind;

        // Check if condition contains a True keyword
        // Need to check tokens, not just nodes
        self.condition.descendants_with_tokens().any(|elem| elem.kind() == SyntaxKind::KW_TRUE)
    }
}

/// For loop vertex
///
/// Maps to ForLoopVertex.java
#[derive(Debug, Clone)]
pub struct ForLoopVertex {
    /// Loop variable
    pub loop_var: SyntaxNode,
}

impl ForLoopVertex {
    pub fn new(loop_var: SyntaxNode) -> Self {
        Self { loop_var }
    }
}

/// ForEach loop vertex
///
/// Maps to ForeachLoopVertex.java
#[derive(Debug, Clone)]
pub struct ForEachLoopVertex {
    /// Loop variable
    pub loop_var: SyntaxNode,
}

impl ForEachLoopVertex {
    pub fn new(loop_var: SyntaxNode) -> Self {
        Self { loop_var }
    }
}

/// Try-Except block vertex
///
/// Maps to TryExceptVertex.java
#[derive(Debug, Clone)]
pub struct TryExceptVertex {
    /// Try block start node
    pub try_node: SyntaxNode,
}

impl TryExceptVertex {
    pub fn new(try_node: SyntaxNode) -> Self {
        Self { try_node }
    }
}

/// Label vertex (for Goto statements)
///
/// Maps to LabelVertex.java
#[derive(Debug, Clone)]
pub struct LabelVertex {
    /// Label name node
    pub label: SyntaxNode,
}

impl LabelVertex {
    pub fn new(label: SyntaxNode) -> Self {
        Self { label }
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
