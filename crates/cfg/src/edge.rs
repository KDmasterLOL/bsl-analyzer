//! CFG Edge types
//!
//! Ported from BSL Language Server (Java) via bsl-language-server-rust:
//! - CfgEdge.java
//! - CfgEdgeType.java

/// Type of edge in the control flow graph
///
/// Maps to CfgEdgeType enum in Java
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CfgEdgeType {
    /// Direct/sequential control flow
    /// Default edge type for sequential statements
    #[default]
    Direct,

    /// True branch of a conditional (if/elsif)
    /// Edge taken when condition evaluates to true
    TrueBranch,

    /// False branch of a conditional (if/elsif)
    /// Edge taken when condition evaluates to false
    FalseBranch,

    /// Loop iteration edge
    /// Back edge from loop body to loop header
    LoopIteration,

    /// Adjacent dead code
    /// Code that follows after unconditional jump (goto/return/break)
    AdjacentCode,
}

impl CfgEdgeType {
    /// Check if this edge represents a conditional branch
    pub fn is_conditional_branch(&self) -> bool {
        matches!(self, CfgEdgeType::TrueBranch | CfgEdgeType::FalseBranch)
    }

    /// Check if this edge represents a loop back-edge
    pub fn is_loop_back_edge(&self) -> bool {
        matches!(self, CfgEdgeType::LoopIteration)
    }

    /// Check if this edge leads to dead code
    pub fn is_dead_code_edge(&self) -> bool {
        matches!(self, CfgEdgeType::AdjacentCode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_edge_type() {
        assert_eq!(CfgEdgeType::default(), CfgEdgeType::Direct);
    }

    #[test]
    fn test_is_conditional_branch() {
        assert!(CfgEdgeType::TrueBranch.is_conditional_branch());
        assert!(CfgEdgeType::FalseBranch.is_conditional_branch());
        assert!(!CfgEdgeType::Direct.is_conditional_branch());
        assert!(!CfgEdgeType::LoopIteration.is_conditional_branch());
        assert!(!CfgEdgeType::AdjacentCode.is_conditional_branch());
    }

    #[test]
    fn test_is_loop_back_edge() {
        assert!(CfgEdgeType::LoopIteration.is_loop_back_edge());
        assert!(!CfgEdgeType::Direct.is_loop_back_edge());
        assert!(!CfgEdgeType::TrueBranch.is_loop_back_edge());
    }

    #[test]
    fn test_is_dead_code_edge() {
        assert!(CfgEdgeType::AdjacentCode.is_dead_code_edge());
        assert!(!CfgEdgeType::Direct.is_dead_code_edge());
        assert!(!CfgEdgeType::TrueBranch.is_dead_code_edge());
    }
}
