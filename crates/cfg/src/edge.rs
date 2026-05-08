//! CFG Edge types

/// Type of edge in the control flow graph
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
    /// Synthetic back-edge from loop body to loop header. Produced by the
    /// builder when `produce_loop_iterations` is on; models the "loop runs
    /// at least once" abstraction. Not a user-source jump — see
    /// [`Self::LoopContinue`] for that.
    LoopIteration,

    /// User-explicit `Прервать` / `Break`
    /// Live edge from the block ending in `Прервать` to the after-loop
    /// merge point (the existing `BasicBlock` produced by
    /// `walk_*_loop_statement_hir`). Not dead code — control flow really
    /// flows here at runtime.
    LoopBreak,

    /// User-explicit `Продолжить` / `Continue`
    /// Live edge from the block ending in `Продолжить` to the loop header.
    /// Distinct from [`Self::LoopIteration`]: that one is the synthetic
    /// abstraction toggled by the builder, this one comes from a real BSL
    /// statement in the source.
    LoopContinue,

    /// Adjacent dead code
    /// Edge from a block ending in an unconditional jump
    /// (`Прервать`, `Продолжить`, `Возврат`, `goto`, `Raise`) to the
    /// **statement that textually follows it** — the dead-fallthrough
    /// successor that no execution path can actually reach.
    ///
    /// This is the dead-fallthrough complement, not the live jump
    /// itself: the live jump from `Прервать` / `Продолжить` is
    /// modeled separately as `LoopBreak` / `LoopContinue` and targets
    /// the after-loop merge / loop header. So a `Прервать` block
    /// produces TWO outgoing edges: one `LoopBreak` to the loop exit
    /// (live), and one `AdjacentCode` to the unreachable next statement
    /// (dead). Reachability / return-path analyses key off
    /// [`Self::is_dead_code_edge`] to ignore the latter without losing
    /// the former.
    AdjacentCode,
}

impl CfgEdgeType {
    /// Check if this edge represents a conditional branch
    pub fn is_conditional_branch(&self) -> bool {
        matches!(self, CfgEdgeType::TrueBranch | CfgEdgeType::FalseBranch)
    }

    /// Check if this edge represents a loop back-edge
    ///
    /// Both the synthetic `LoopIteration` and the user-explicit
    /// `LoopContinue` target the loop header and behave as back-edges for
    /// reverse-postorder dataflow.
    pub fn is_loop_back_edge(&self) -> bool {
        matches!(self, CfgEdgeType::LoopIteration | CfgEdgeType::LoopContinue)
    }

    /// Check if this edge leads to dead code
    pub fn is_dead_code_edge(&self) -> bool {
        matches!(self, CfgEdgeType::AdjacentCode)
    }

    /// Check if this edge models a user-explicit jump (`Прервать` / `Продолжить`).
    ///
    /// Used by analyses that need to distinguish user control flow from
    /// the builder's synthetic edges (e.g. dead-code reachability, dataflow
    /// transfer functions for resource lattices).
    pub fn is_user_loop_jump(&self) -> bool {
        matches!(self, CfgEdgeType::LoopBreak | CfgEdgeType::LoopContinue)
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
        assert!(!CfgEdgeType::LoopBreak.is_conditional_branch());
        assert!(!CfgEdgeType::LoopContinue.is_conditional_branch());
    }

    #[test]
    fn test_is_loop_back_edge() {
        assert!(CfgEdgeType::LoopIteration.is_loop_back_edge());
        // `LoopContinue` targets the loop header — same back-edge shape
        // as the synthetic `LoopIteration` for reverse-postorder dataflow.
        assert!(CfgEdgeType::LoopContinue.is_loop_back_edge());
        assert!(!CfgEdgeType::Direct.is_loop_back_edge());
        assert!(!CfgEdgeType::TrueBranch.is_loop_back_edge());
        assert!(!CfgEdgeType::LoopBreak.is_loop_back_edge());
    }

    #[test]
    fn test_is_dead_code_edge() {
        assert!(CfgEdgeType::AdjacentCode.is_dead_code_edge());
        assert!(!CfgEdgeType::Direct.is_dead_code_edge());
        assert!(!CfgEdgeType::TrueBranch.is_dead_code_edge());
        // Live user jumps must NOT be classified as dead code — that is
        // the headline change vs the legacy `AdjacentCode` routing for
        // `Прервать` / `Продолжить`.
        assert!(!CfgEdgeType::LoopBreak.is_dead_code_edge());
        assert!(!CfgEdgeType::LoopContinue.is_dead_code_edge());
    }

    #[test]
    fn test_is_user_loop_jump() {
        assert!(CfgEdgeType::LoopBreak.is_user_loop_jump());
        assert!(CfgEdgeType::LoopContinue.is_user_loop_jump());
        // The synthetic builder edge is NOT a user jump.
        assert!(!CfgEdgeType::LoopIteration.is_user_loop_jump());
        assert!(!CfgEdgeType::Direct.is_user_loop_jump());
        assert!(!CfgEdgeType::TrueBranch.is_user_loop_jump());
        assert!(!CfgEdgeType::FalseBranch.is_user_loop_jump());
        assert!(!CfgEdgeType::AdjacentCode.is_user_loop_jump());
    }
}
