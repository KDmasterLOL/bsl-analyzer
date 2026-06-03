#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CfgEdgeType {
    #[default]
    Direct,

    TrueBranch,

    FalseBranch,

    LoopIteration,

    LoopBreak,

    LoopContinue,

    AdjacentCode,
}

impl CfgEdgeType {
    pub fn is_conditional_branch(&self) -> bool {
        matches!(self, CfgEdgeType::TrueBranch | CfgEdgeType::FalseBranch)
    }

    pub fn is_loop_back_edge(&self) -> bool {
        matches!(self, CfgEdgeType::LoopIteration | CfgEdgeType::LoopContinue)
    }

    pub fn is_dead_code_edge(&self) -> bool {
        matches!(self, CfgEdgeType::AdjacentCode)
    }

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
        assert!(!CfgEdgeType::LoopBreak.is_dead_code_edge());
        assert!(!CfgEdgeType::LoopContinue.is_dead_code_edge());
    }

    #[test]
    fn test_is_user_loop_jump() {
        assert!(CfgEdgeType::LoopBreak.is_user_loop_jump());
        assert!(CfgEdgeType::LoopContinue.is_user_loop_jump());
        assert!(!CfgEdgeType::LoopIteration.is_user_loop_jump());
        assert!(!CfgEdgeType::Direct.is_user_loop_jump());
        assert!(!CfgEdgeType::TrueBranch.is_user_loop_jump());
        assert!(!CfgEdgeType::FalseBranch.is_user_loop_jump());
        assert!(!CfgEdgeType::AdjacentCode.is_user_loop_jump());
    }
}
