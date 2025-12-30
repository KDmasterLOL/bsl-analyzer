//! Integration tests for CFG infrastructure
//!
//! These tests verify the CFG components work correctly.

use cfg::{BasicBlockVertex, CfgBuilder, CfgEdgeType, CfgVertex, ControlFlowGraph};

#[test]
fn test_empty_graph() {
    let cfg = ControlFlowGraph::new();

    assert_eq!(cfg.vertex_count(), 1); // Only exit point
    assert_eq!(cfg.edge_count(), 0);
    assert!(cfg.entry_point().is_none());
}

#[test]
fn test_add_vertices() {
    let mut cfg = ControlFlowGraph::new();

    let block1 = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
    let block2 = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

    assert_eq!(cfg.vertex_count(), 3); // Exit + 2 blocks
    assert!(cfg.vertex(block1).is_some());
    assert!(cfg.vertex(block2).is_some());
}

#[test]
fn test_add_edges() {
    let mut cfg = ControlFlowGraph::new();

    let block1 = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
    let block2 = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

    let result = cfg.add_edge(block1, block2, CfgEdgeType::Direct);
    assert!(result.is_ok());
    assert_eq!(cfg.edge_count(), 1);
}

#[test]
fn test_conditional_edge_validation() {
    use cfg::ConditionalVertex;
    use syntax::{SyntaxKind, SyntaxNode};

    let mut cfg = ControlFlowGraph::new();

    // Create a mock SyntaxNode (in real code this comes from parser)
    // For testing, we create a minimal node
    let root = rowan::GreenNode::new(rowan::SyntaxKind(SyntaxKind::SOURCE_FILE as u16), vec![]);
    let node = SyntaxNode::new_root(root);

    let cond = cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(node)));
    let block = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

    // Conditional vertices should accept TrueBranch and FalseBranch
    assert!(cfg.add_edge(cond, block, CfgEdgeType::TrueBranch).is_ok());

    // Conditional vertices should reject Direct edges
    let block2 = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
    assert!(cfg.add_edge(cond, block2, CfgEdgeType::Direct).is_err());
}

#[test]
fn test_incoming_outgoing_edges() {
    let mut cfg = ControlFlowGraph::new();

    let block1 = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
    let block2 = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
    let block3 = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

    cfg.add_edge(block1, block2, CfgEdgeType::Direct).unwrap();
    cfg.add_edge(block1, block3, CfgEdgeType::Direct).unwrap();

    let outgoing: Vec<_> = cfg.outgoing_edges(block1).collect();
    assert_eq!(outgoing.len(), 2);

    let incoming: Vec<_> = cfg.incoming_edges(block2).collect();
    assert_eq!(incoming.len(), 1);
}

#[test]
fn test_basic_block_operations() {
    use syntax::{SyntaxKind, SyntaxNode};

    let mut block = BasicBlockVertex::new();
    assert!(block.is_empty());
    assert_eq!(block.len(), 0);

    // Create mock statements
    let root = rowan::GreenNode::new(rowan::SyntaxKind(SyntaxKind::RETURN_STMT as u16), vec![]);
    let stmt1 = SyntaxNode::new_root(root.clone());
    let stmt2 = SyntaxNode::new_root(root);

    block.add_statement(stmt1);
    block.add_statement(stmt2);

    assert!(!block.is_empty());
    assert_eq!(block.len(), 2);
    assert!(block.first_statement().is_some());
    assert!(block.last_statement().is_some());
}

#[test]
fn test_cfg_builder_creation() {
    let builder = CfgBuilder::new();
    // Should compile and create builder
    drop(builder);
}

#[test]
fn test_cfg_builder_produce_loop_iterations() {
    let mut builder = CfgBuilder::new();
    builder.produce_loop_iterations(false);
    // Should compile and set flag
    drop(builder);
}

#[test]
fn test_edge_type_helpers() {
    assert!(CfgEdgeType::TrueBranch.is_conditional_branch());
    assert!(CfgEdgeType::FalseBranch.is_conditional_branch());
    assert!(!CfgEdgeType::Direct.is_conditional_branch());

    assert!(CfgEdgeType::LoopIteration.is_loop_back_edge());
    assert!(!CfgEdgeType::Direct.is_loop_back_edge());

    assert!(CfgEdgeType::AdjacentCode.is_dead_code_edge());
    assert!(!CfgEdgeType::Direct.is_dead_code_edge());
}

#[test]
fn test_vertex_type_name() {
    use cfg::{ForLoopVertex, WhileLoopVertex};
    use syntax::{SyntaxKind, SyntaxNode};

    let root = rowan::GreenNode::new(rowan::SyntaxKind(SyntaxKind::IDENT as u16), vec![]);
    let node = SyntaxNode::new_root(root);

    let block = CfgVertex::BasicBlock(BasicBlockVertex::new());
    assert_eq!(block.type_name(), "BasicBlock");

    let while_loop = CfgVertex::WhileLoop(WhileLoopVertex::new(node.clone()));
    assert_eq!(while_loop.type_name(), "WhileLoop");

    let for_loop = CfgVertex::ForLoop(ForLoopVertex::new(node));
    assert_eq!(for_loop.type_name(), "ForLoop");

    let exit = CfgVertex::Exit;
    assert_eq!(exit.type_name(), "Exit");
}

#[test]
fn test_vertex_branching_checks() {
    use cfg::{ConditionalVertex, WhileLoopVertex};
    use syntax::{SyntaxKind, SyntaxNode};

    let root = rowan::GreenNode::new(rowan::SyntaxKind(SyntaxKind::IDENT as u16), vec![]);
    let node = SyntaxNode::new_root(root);

    let block = CfgVertex::BasicBlock(BasicBlockVertex::new());
    assert!(!block.is_branching());
    assert!(!block.is_loop());

    let cond = CfgVertex::Conditional(ConditionalVertex::new(node.clone()));
    assert!(cond.is_branching());
    assert!(!cond.is_loop());

    let while_loop = CfgVertex::WhileLoop(WhileLoopVertex::new(node));
    assert!(while_loop.is_branching());
    assert!(while_loop.is_loop());
}

#[test]
fn test_endless_loop_detection() {
    use cfg::WhileLoopVertex;
    use syntax::{SyntaxKind, SyntaxNode};

    // Test with TRUE keyword
    let true_token = rowan::GreenNode::new(rowan::SyntaxKind(SyntaxKind::KW_TRUE as u16), vec![]);
    let true_node = SyntaxNode::new_root(true_token);

    let endless_loop = WhileLoopVertex::new(true_node);
    assert!(endless_loop.is_endless());

    // Test with non-true condition
    let ident_token = rowan::GreenNode::new(rowan::SyntaxKind(SyntaxKind::IDENT as u16), vec![]);
    let ident_node = SyntaxNode::new_root(ident_token);

    let normal_loop = WhileLoopVertex::new(ident_node);
    assert!(!normal_loop.is_endless());
}
