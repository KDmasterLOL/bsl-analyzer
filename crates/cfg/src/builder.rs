//! CFG Builder - constructs Control Flow Graph from BSL functions
//!
//! Ported from BSL Language Server (Java) via bsl-language-server-rust:
//! - CfgBuilder.java
//!
//! ## Key Differences from bsl-language-server-rust
//!
//! **Node Storage:**
//! - bsl-language-server-rust: Uses NodePosition (byte offsets) to avoid tree-sitter lifetime issues
//! - bsl-analyzer: Uses SyntaxNode directly (Rowan Arc-based, no lifetime issues)
//!
//! **Traversal:**
//! - bsl-language-server-rust: tree-sitter cursor navigation (cursor.goto_first_child())
//! - bsl-analyzer: Rowan iterators (node.children(), node.kind() matching)

use crate::edge::CfgEdgeType;
use crate::graph::ControlFlowGraph;
use crate::vertex::{BasicBlockVertex, CfgVertex};
use petgraph::graph::NodeIndex;
use syntax::{SyntaxKind, SyntaxNode};

/// CFG Builder for BSL functions/procedures
///
/// Constructs a Control Flow Graph by walking the AST of a function body.
///
/// Maps to CfgBuilder in Java and bsl-language-server-rust.
pub struct CfgBuilder {
    /// The CFG being constructed
    cfg: ControlFlowGraph,

    /// Current basic block being built
    current_block: Option<NodeIndex>,

    /// Whether to produce loop iteration edges (back edges)
    /// Used for configuration: loopsExecutedAtLeastOnce
    produce_loop_iterations: bool,
}

impl CfgBuilder {
    /// Create a new CFG builder
    pub fn new() -> Self {
        Self { cfg: ControlFlowGraph::new(), current_block: None, produce_loop_iterations: true }
    }

    /// Set whether to produce loop iteration edges
    ///
    /// When true (default), adds back edges from loop body to loop condition.
    /// This affects diagnostics that check for missing returns - if loops
    /// are assumed to execute at least once, paths through loops are considered.
    pub fn produce_loop_iterations(&mut self, value: bool) {
        self.produce_loop_iterations = value;
    }

    /// Build CFG from a function/procedure body
    ///
    /// Maps to build() in Java
    pub fn build_graph(mut self, body: &SyntaxNode) -> ControlFlowGraph {
        // Create entry block
        let entry = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        self.cfg.set_entry_point(entry);
        self.current_block = Some(entry);

        // Walk the body
        // If body is already a STMT_LIST, walk it directly
        // Otherwise, find STMT_LIST children and process them
        if body.kind() == SyntaxKind::STMT_LIST {
            self.walk_stmt_list(body);
        } else {
            for child in body.children() {
                if child.kind() == SyntaxKind::STMT_LIST {
                    self.walk_stmt_list(&child);
                }
            }
        }

        // Connect last block to exit if it exists and doesn't already connect
        if let Some(block) = self.current_block {
            let exit = self.cfg.exit_point();
            // Only add edge if this block doesn't already end with return/raise
            if let Some(CfgVertex::BasicBlock(bb)) = self.cfg.vertex(block) {
                let ends_with_exit = bb.statements().last().is_some_and(|stmt| {
                    matches!(stmt.kind(), SyntaxKind::RETURN_STMT | SyntaxKind::RAISE_STMT)
                });
                if !ends_with_exit {
                    let _ = self.cfg.add_edge(block, exit, CfgEdgeType::Direct);
                }
            }
        }

        self.cfg
    }

    /// Walk a statement list
    fn walk_stmt_list(&mut self, stmt_list: &SyntaxNode) {
        for stmt in stmt_list.children() {
            // Skip non-statement nodes (whitespace, comments, etc.)
            if !Self::is_statement(&stmt) {
                continue;
            }
            self.walk_statement(&stmt);
        }
    }

    /// Check if a node is a statement
    fn is_statement(node: &SyntaxNode) -> bool {
        matches!(
            node.kind(),
            SyntaxKind::RETURN_STMT
                | SyntaxKind::IF_STMT
                | SyntaxKind::WHILE_STMT
                | SyntaxKind::FOR_STMT
                | SyntaxKind::FOR_EACH_STMT
                | SyntaxKind::TRY_STMT
                | SyntaxKind::BREAK_STMT
                | SyntaxKind::CONTINUE_STMT
                | SyntaxKind::RAISE_STMT
                | SyntaxKind::ASSIGN_STMT
                | SyntaxKind::CALL_STMT
                | SyntaxKind::EXECUTE_STMT
                | SyntaxKind::GOTO_STMT
                | SyntaxKind::LABEL_STMT
        )
    }

    /// Walk a single statement
    fn walk_statement(&mut self, stmt: &SyntaxNode) {
        match stmt.kind() {
            SyntaxKind::RETURN_STMT => self.walk_return_statement(stmt),
            SyntaxKind::IF_STMT => self.walk_if_statement(stmt),
            SyntaxKind::WHILE_STMT => self.walk_while_statement(stmt),
            SyntaxKind::FOR_STMT => self.walk_for_statement(stmt),
            SyntaxKind::FOR_EACH_STMT => self.walk_foreach_statement(stmt),
            SyntaxKind::TRY_STMT => self.walk_try_statement(stmt),
            SyntaxKind::BREAK_STMT => self.walk_break_statement(stmt),
            SyntaxKind::CONTINUE_STMT => self.walk_continue_statement(stmt),
            SyntaxKind::RAISE_STMT => self.walk_raise_statement(stmt),
            _ => {
                // Regular statement - add to current basic block
                self.add_to_current_block(stmt.clone());
            }
        }
    }

    /// Add a statement to the current basic block
    fn add_to_current_block(&mut self, stmt: SyntaxNode) {
        if let Some(block_idx) = self.current_block {
            if let Some(CfgVertex::BasicBlock(block)) = self.cfg.vertex_mut(block_idx) {
                block.add_statement(stmt);
            }
        }
    }

    /// Walk a return statement
    ///
    /// Algorithm:
    /// 1. Add statement to current block
    /// 2. Connect current block → exit
    /// 3. Create new dead code block (unreachable code after return)
    /// 4. Connect with AdjacentCode edge
    fn walk_return_statement(&mut self, ret_stmt: &SyntaxNode) {
        // Add return statement to current block
        self.add_to_current_block(ret_stmt.clone());

        if let Some(block_idx) = self.current_block {
            let exit = self.cfg.exit_point();

            // Connect current block to exit
            let _ = self.cfg.add_edge(block_idx, exit, CfgEdgeType::Direct);

            // Create new unreachable block for any dead code after return
            let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            // Connect via AdjacentCode edge (marks dead code)
            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);

            // Continue building in dead block (for completeness)
            self.current_block = Some(dead_block);
        }
    }

    /// Walk a raise statement
    ///
    /// Treated similarly to return - terminates execution path
    fn walk_raise_statement(&mut self, raise_stmt: &SyntaxNode) {
        // Add raise statement to current block
        self.add_to_current_block(raise_stmt.clone());

        if let Some(block_idx) = self.current_block {
            let exit = self.cfg.exit_point();

            // Connect current block to exit
            let _ = self.cfg.add_edge(block_idx, exit, CfgEdgeType::Direct);

            // Create new unreachable block for any dead code after raise
            let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            // Connect via AdjacentCode edge
            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);

            self.current_block = Some(dead_block);
        }
    }

    // ============================================================================
    // TODO: Implement control flow walkers
    // ============================================================================

    /// Walk an if statement
    ///
    /// Algorithm:
    /// 1. Create ConditionalVertex for IF condition
    /// 2. Connect current block → conditional
    /// 3. Walk THEN branch, save exit
    /// 4. For each ELSIF: create another ConditionalVertex, chain it
    /// 5. Walk ELSE branch if present
    /// 6. Create merge block
    /// 7. Connect all branch exits → merge
    fn walk_if_statement(&mut self, if_stmt: &SyntaxNode) {
        use crate::vertex::ConditionalVertex;

        // Find the condition (first expression child before THEN)
        let condition = if_stmt
            .children()
            .find(|n| {
                matches!(
                    n.kind(),
                    SyntaxKind::BINARY_EXPR
                        | SyntaxKind::UNARY_EXPR
                        | SyntaxKind::PAREN_EXPR
                        | SyntaxKind::CALL_EXPR
                        | SyntaxKind::IDENT
                )
            })
            .unwrap_or_else(|| if_stmt.clone());

        // Create conditional vertex
        let cond_vertex =
            self.cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(condition.clone())));

        // Connect current block to conditional
        if let Some(current) = self.current_block {
            let _ = self.cfg.add_edge(current, cond_vertex, CfgEdgeType::Direct);
        }

        // Create merge block (will be used after all branches)
        let merge_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

        // Walk THEN branch
        let then_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        let _ = self.cfg.add_edge(cond_vertex, then_block, CfgEdgeType::TrueBranch);
        self.current_block = Some(then_block);

        // Find and walk THEN body (statements after THEN_KEYWORD)
        if let Some(then_body) = self.find_then_body(if_stmt) {
            self.walk_stmt_list(&then_body);
        }

        // Save then exit and check if reachable
        let then_exit = self.current_block;
        if let Some(exit) = then_exit {
            if self.is_block_reachable(exit) {
                let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
            }
        }

        // Process ELSIF clauses
        let mut current_cond = cond_vertex;
        let elsif_clauses: Vec<_> =
            if_stmt.children().filter(|n| n.kind() == SyntaxKind::ELSIF_CLAUSE).collect();

        for elsif_clause in elsif_clauses {
            // Find condition in elsif
            let elsif_condition = elsif_clause
                .children()
                .find(|n| {
                    matches!(
                        n.kind(),
                        SyntaxKind::BINARY_EXPR
                            | SyntaxKind::UNARY_EXPR
                            | SyntaxKind::PAREN_EXPR
                            | SyntaxKind::CALL_EXPR
                            | SyntaxKind::IDENT
                    )
                })
                .unwrap_or_else(|| elsif_clause.clone());

            // Create elsif conditional
            let elsif_cond = self.cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(
                elsif_condition.clone(),
            )));

            // Connect previous cond FALSE → this elsif cond
            let _ = self.cfg.add_edge(current_cond, elsif_cond, CfgEdgeType::FalseBranch);

            // Walk elsif body
            let elsif_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(elsif_cond, elsif_block, CfgEdgeType::TrueBranch);
            self.current_block = Some(elsif_block);

            // Find and walk elsif body
            if let Some(elsif_body) = self.find_elsif_body(&elsif_clause) {
                self.walk_stmt_list(&elsif_body);
            }

            // Save elsif exit
            let elsif_exit = self.current_block;
            if let Some(exit) = elsif_exit {
                if self.is_block_reachable(exit) {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                }
            }

            current_cond = elsif_cond;
        }

        // Process ELSE clause if present
        let has_else = if_stmt.children().any(|n| n.kind() == SyntaxKind::ELSE_CLAUSE);

        if has_else {
            // Create else block
            let else_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(current_cond, else_block, CfgEdgeType::FalseBranch);
            self.current_block = Some(else_block);

            // Find and walk else body
            if let Some(else_clause) =
                if_stmt.children().find(|n| n.kind() == SyntaxKind::ELSE_CLAUSE)
            {
                if let Some(else_body) = self.find_else_body(&else_clause) {
                    self.walk_stmt_list(&else_body);
                }
            }

            // Connect else exit to merge
            let else_exit = self.current_block;
            if let Some(exit) = else_exit {
                if self.is_block_reachable(exit) {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                }
            }
        } else {
            // No else - false branch of last conditional goes to merge
            let _ = self.cfg.add_edge(current_cond, merge_block, CfgEdgeType::FalseBranch);
        }

        // Continue with merge block
        self.current_block = Some(merge_block);
    }

    /// Find THEN body in IF statement
    fn find_then_body(&self, if_stmt: &SyntaxNode) -> Option<SyntaxNode> {
        if_stmt.children().find(|n| n.kind() == SyntaxKind::STMT_LIST)
    }

    /// Find ELSIF body
    fn find_elsif_body(&self, elsif_clause: &SyntaxNode) -> Option<SyntaxNode> {
        elsif_clause.children().find(|n| n.kind() == SyntaxKind::STMT_LIST)
    }

    /// Find ELSE body
    fn find_else_body(&self, else_clause: &SyntaxNode) -> Option<SyntaxNode> {
        else_clause.children().find(|n| n.kind() == SyntaxKind::STMT_LIST)
    }

    /// Check if a block is reachable (has incoming edges or is entry point)
    fn is_block_reachable(&self, block: NodeIndex) -> bool {
        // A block is reachable if:
        // 1. It has incoming edges, OR
        // 2. It's the entry point
        let has_incoming = self.cfg.incoming_edges(block).next().is_some();
        let is_entry = self.cfg.entry_point() == Some(block);
        has_incoming || is_entry
    }

    /// Walk a while statement
    ///
    /// Algorithm:
    /// 1. Create WhileLoopVertex with condition
    /// 2. Connect current block → loop vertex
    /// 3. Create loop body block
    /// 4. Connect loop → body (TrueBranch)
    /// 5. Walk loop body
    /// 6. If produce_loop_iterations: Add back edge body → loop (LoopIteration)
    /// 7. Create after_loop block
    /// 8. Connect loop → after_loop (FalseBranch)
    fn walk_while_statement(&mut self, while_stmt: &SyntaxNode) {
        use crate::vertex::WhileLoopVertex;

        // Find loop condition (first expression)
        let condition = while_stmt
            .children()
            .find(|n| {
                matches!(
                    n.kind(),
                    SyntaxKind::EXPR
                        | SyntaxKind::BINARY_EXPR
                        | SyntaxKind::UNARY_EXPR
                        | SyntaxKind::PAREN_EXPR
                        | SyntaxKind::CALL_EXPR
                        | SyntaxKind::IDENT
                )
            })
            .unwrap_or_else(|| while_stmt.clone());

        // Create while loop vertex
        let loop_vertex =
            self.cfg.add_vertex(CfgVertex::WhileLoop(WhileLoopVertex::new(condition.clone())));

        // Connect current block to loop
        if let Some(current) = self.current_block {
            let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::Direct);
        }

        // Create loop body block
        let body_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

        // Connect loop → body (true branch - condition is true)
        let _ = self.cfg.add_edge(loop_vertex, body_block, CfgEdgeType::TrueBranch);

        // Set current block to body
        self.current_block = Some(body_block);

        // Find and walk loop body
        if let Some(body) = while_stmt.children().find(|n| n.kind() == SyntaxKind::STMT_LIST) {
            self.walk_stmt_list(&body);
        }

        // Get body exit block
        let body_exit = self.current_block;

        // Add back edge if enabled (loop iteration)
        if self.produce_loop_iterations {
            if let Some(exit) = body_exit {
                if self.is_block_reachable(exit) {
                    let _ = self.cfg.add_edge(exit, loop_vertex, CfgEdgeType::LoopIteration);
                }
            }
        }

        // Create after-loop block (for when condition is false)
        let after_loop = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

        // Connect loop → after_loop (false branch - exit loop)
        let _ = self.cfg.add_edge(loop_vertex, after_loop, CfgEdgeType::FalseBranch);

        // Continue with after-loop block
        self.current_block = Some(after_loop);
    }

    /// Walk a for statement
    ///
    /// Similar to while loop but uses ForLoopVertex
    fn walk_for_statement(&mut self, for_stmt: &SyntaxNode) {
        use crate::vertex::ForLoopVertex;

        // Find loop variable (first identifier)
        let loop_var = for_stmt
            .children()
            .find(|n| n.kind() == SyntaxKind::IDENT)
            .unwrap_or_else(|| for_stmt.clone());

        // Create for loop vertex
        let loop_vertex =
            self.cfg.add_vertex(CfgVertex::ForLoop(ForLoopVertex::new(loop_var.clone())));

        // Connect current block to loop
        if let Some(current) = self.current_block {
            let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::Direct);
        }

        // Create loop body block
        let body_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

        // Connect loop → body (true branch)
        let _ = self.cfg.add_edge(loop_vertex, body_block, CfgEdgeType::TrueBranch);

        self.current_block = Some(body_block);

        // Walk loop body
        if let Some(body) = for_stmt.children().find(|n| n.kind() == SyntaxKind::STMT_LIST) {
            self.walk_stmt_list(&body);
        }

        let body_exit = self.current_block;

        // Add back edge if enabled
        if self.produce_loop_iterations {
            if let Some(exit) = body_exit {
                if self.is_block_reachable(exit) {
                    let _ = self.cfg.add_edge(exit, loop_vertex, CfgEdgeType::LoopIteration);
                }
            }
        }

        // Create after-loop block
        let after_loop = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

        let _ = self.cfg.add_edge(loop_vertex, after_loop, CfgEdgeType::FalseBranch);

        self.current_block = Some(after_loop);
    }

    /// Walk a foreach statement
    ///
    /// Similar to for loop but uses ForEachLoopVertex
    fn walk_foreach_statement(&mut self, foreach_stmt: &SyntaxNode) {
        use crate::vertex::ForEachLoopVertex;

        // Find loop variable
        let loop_var = foreach_stmt
            .children()
            .find(|n| n.kind() == SyntaxKind::IDENT)
            .unwrap_or_else(|| foreach_stmt.clone());

        // Create foreach loop vertex
        let loop_vertex =
            self.cfg.add_vertex(CfgVertex::ForEachLoop(ForEachLoopVertex::new(loop_var.clone())));

        // Connect current block to loop
        if let Some(current) = self.current_block {
            let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::Direct);
        }

        // Create loop body block
        let body_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

        // Connect loop → body
        let _ = self.cfg.add_edge(loop_vertex, body_block, CfgEdgeType::TrueBranch);

        self.current_block = Some(body_block);

        // Walk loop body
        if let Some(body) = foreach_stmt.children().find(|n| n.kind() == SyntaxKind::STMT_LIST) {
            self.walk_stmt_list(&body);
        }

        let body_exit = self.current_block;

        // Add back edge if enabled
        if self.produce_loop_iterations {
            if let Some(exit) = body_exit {
                if self.is_block_reachable(exit) {
                    let _ = self.cfg.add_edge(exit, loop_vertex, CfgEdgeType::LoopIteration);
                }
            }
        }

        // Create after-loop block
        let after_loop = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

        let _ = self.cfg.add_edge(loop_vertex, after_loop, CfgEdgeType::FalseBranch);

        self.current_block = Some(after_loop);
    }

    /// Walk a try statement
    ///
    /// Algorithm:
    /// 1. Create TryExceptVertex
    /// 2. Connect current → try vertex
    /// 3. Create try body block, walk it
    /// 4. Create except body block, walk it
    /// 5. Connect try vertex → except (FalseBranch for exception path)
    /// 6. Create merge block
    /// 7. Connect both exits → merge
    fn walk_try_statement(&mut self, try_stmt: &SyntaxNode) {
        use crate::vertex::TryExceptVertex;

        // Create try-except vertex
        let try_vertex =
            self.cfg.add_vertex(CfgVertex::TryExcept(TryExceptVertex::new(try_stmt.clone())));

        // Connect current block to try
        if let Some(current) = self.current_block {
            let _ = self.cfg.add_edge(current, try_vertex, CfgEdgeType::Direct);
        }

        // Create try body block
        let try_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

        // Connect try vertex → try body (normal path)
        let _ = self.cfg.add_edge(try_vertex, try_block, CfgEdgeType::TrueBranch);

        self.current_block = Some(try_block);

        // Find and walk try body (first STMT_LIST)
        let stmt_lists: Vec<_> =
            try_stmt.children().filter(|n| n.kind() == SyntaxKind::STMT_LIST).collect();

        if let Some(try_body) = stmt_lists.first() {
            self.walk_stmt_list(try_body);
        }

        let try_exit = self.current_block;

        // Create except block
        let except_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

        // Connect try vertex → except (exception path)
        let _ = self.cfg.add_edge(try_vertex, except_block, CfgEdgeType::FalseBranch);

        self.current_block = Some(except_block);

        // Walk except body (second STMT_LIST, if exists)
        if stmt_lists.len() > 1 {
            self.walk_stmt_list(&stmt_lists[1]);
        }

        let except_exit = self.current_block;

        // Create merge block
        let merge_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

        // Connect try exit to merge (if reachable)
        if let Some(exit) = try_exit {
            if self.is_block_reachable(exit) {
                let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
            }
        }

        // Connect except exit to merge (if reachable)
        if let Some(exit) = except_exit {
            if self.is_block_reachable(exit) {
                let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
            }
        }

        self.current_block = Some(merge_block);
    }

    /// Walk a break statement
    ///
    /// Note: Full implementation would require loop stack to connect to loop exit.
    /// For now, we treat it like return - terminates current path.
    /// This is sufficient for AllFunctionPathMustHaveReturn diagnostic.
    fn walk_break_statement(&mut self, break_stmt: &SyntaxNode) {
        // Add break statement to current block
        self.add_to_current_block(break_stmt.clone());

        if let Some(block_idx) = self.current_block {
            // Create dead code block after break
            let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            // Connect via AdjacentCode edge
            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);

            self.current_block = Some(dead_block);
        }
    }

    /// Walk a continue statement
    ///
    /// Note: Full implementation would require loop stack to connect to loop condition.
    /// For now, we treat it like break - terminates current path.
    /// This is sufficient for AllFunctionPathMustHaveReturn diagnostic.
    fn walk_continue_statement(&mut self, continue_stmt: &SyntaxNode) {
        // Add continue statement to current block
        self.add_to_current_block(continue_stmt.clone());

        if let Some(block_idx) = self.current_block {
            // Create dead code block after continue
            let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            // Connect via AdjacentCode edge
            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);

            self.current_block = Some(dead_block);
        }
    }
}

impl Default for CfgBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_creation() {
        let builder = CfgBuilder::new();
        assert!(builder.current_block.is_none());
        assert!(builder.produce_loop_iterations);
    }

    #[test]
    fn test_produce_loop_iterations() {
        let mut builder = CfgBuilder::new();
        builder.produce_loop_iterations(false);
        assert!(!builder.produce_loop_iterations);
    }

    // TODO: Add integration tests with real BSL code after parser integration
}
