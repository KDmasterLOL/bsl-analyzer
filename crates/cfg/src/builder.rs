//! CFG Builder - constructs Control Flow Graph from BSL functions
//!
//! Ported from BSL Language Server (Java) via bsl-language-server-rust:
//! - CfgBuilder.java
//!
//! ## Key Differences from bsl-language-server-rust
//!
//! **Node Storage (AST-based - LEGACY):**
//! - bsl-language-server-rust: Uses NodePosition (byte offsets) to avoid tree-sitter lifetime issues
//! - bsl-analyzer (old): Uses SyntaxNode directly (Rowan Arc-based, no lifetime issues)
//!
//! ## HIR-based CFG (Phase 6.2)
//!
//! **New Approach:**
//! - build_graph_from_hir(): Constructs CFG from HIR Body (StmtId, ExprId, BindingId)
//! - Direct pattern matching on Stmt enum (no AST traversal with find() + fallbacks)
//! - Enables dataflow analysis (transfer functions need StmtId access)
//!
//! **Legacy AST-based methods:**
//! - build_graph(): Old AST-based implementation (kept for backward compatibility)
//! - Will be removed after all diagnostics migrate to HIR-based CFG

use crate::edge::CfgEdgeType;
use crate::graph::ControlFlowGraph;
use crate::vertex::{BasicBlockVertex, CfgVertex};
use hir_def::{Body, BodySourceMap, Stmt, StmtId};
use petgraph::graph::NodeIndex;

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

    /* LEGACY AST-BASED METHOD - DISABLED IN PHASE 6.4
     *
     * Use build_graph_from_hir() instead for HIR-based CFG construction.
     * This method will be removed after all diagnostics migrate to HIR.
     *
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
    */

    /// Build CFG from HIR Body (Phase 6.2)
    ///
    /// This is the new HIR-based approach that enables dataflow analysis.
    ///
    /// # Arguments
    /// * `body_stmts` - Top-level statements from Body (module code or method body)
    /// * `body` - HIR Body for looking up statements/expressions
    /// * `_source_map` - Optional source map for diagnostics (unused for now, will be used in Phase 6.4)
    ///
    /// # Example
    /// ```ignore
    /// let cfg = CfgBuilder::new()
    ///     .produce_loop_iterations(true)
    ///     .build_graph_from_hir(&body.body_stmts, &body, &source_map);
    /// ```
    pub fn build_graph_from_hir(
        mut self,
        body_stmts: &[StmtId],
        body: &Body,
        _source_map: Option<&BodySourceMap>,
    ) -> ControlFlowGraph {
        use std::time::Instant;

        crate::CFG_BUILD_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let start = Instant::now();

        // Create entry block
        let entry = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        self.cfg.set_entry_point(entry);
        self.current_block = Some(entry);

        // Walk top-level statements
        for &stmt_id in body_stmts {
            self.walk_statement_hir(stmt_id, body);
        }

        // Connect last block to exit if it doesn't already connect
        if let Some(block_idx) = self.current_block {
            let exit = self.cfg.exit_point();
            // Check if block ends with a terminator
            let ends_with_terminator =
                if let Some(CfgVertex::BasicBlock(bb)) = self.cfg.vertex(block_idx) {
                    bb.statements().last().is_some_and(|&stmt_id| {
                        matches!(body.stmt(stmt_id), Stmt::Return { .. } | Stmt::Raise { .. })
                    })
                } else {
                    false
                };

            if !ends_with_terminator {
                let _ = self.cfg.add_edge(block_idx, exit, CfgEdgeType::Direct);
            }
        }

        crate::CFG_BUILD_TIME_NS
            .fetch_add(start.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);

        self.cfg
    }

    /* LEGACY AST-BASED METHODS - DISABLED IN PHASE 6.4
     *
     * Use walk_statement_hir() instead.
     *
    /// Walk a statement list (AST-based - LEGACY)
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

    /// Walk a single statement (AST-based - LEGACY)
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
    */

    /// Walk a single HIR statement (Phase 6.2)
    ///
    /// Pattern matches on Stmt enum - no AST traversal needed.
    fn walk_statement_hir(&mut self, stmt_id: StmtId, body: &Body) {
        crate::CFG_WALK_STMT_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let stmt = body.stmt(stmt_id);
        match stmt {
            Stmt::Return { .. } => {
                crate::CFG_WALK_RETURN_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.walk_return_statement_hir(stmt_id, body)
            }
            Stmt::Raise { .. } => {
                crate::CFG_WALK_RAISE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.walk_raise_statement_hir(stmt_id, body)
            }
            Stmt::If { .. } => {
                crate::CFG_WALK_IF_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.walk_if_statement_hir(stmt_id, body)
            }
            Stmt::While { .. } => {
                crate::CFG_WALK_WHILE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.walk_while_statement_hir(stmt_id, body)
            }
            Stmt::For { .. } => {
                crate::CFG_WALK_FOR_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.walk_for_statement_hir(stmt_id, body)
            }
            Stmt::ForEach { .. } => {
                crate::CFG_WALK_FOREACH_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.walk_foreach_statement_hir(stmt_id, body)
            }
            Stmt::Try { .. } => {
                crate::CFG_WALK_TRY_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.walk_try_statement_hir(stmt_id, body)
            }
            Stmt::Break => {
                crate::CFG_WALK_BREAK_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.walk_break_statement_hir(stmt_id)
            }
            Stmt::Continue => {
                crate::CFG_WALK_CONTINUE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.walk_continue_statement_hir(stmt_id)
            }
            Stmt::Goto(_) => {
                crate::CFG_WALK_GOTO_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.walk_goto_statement_hir(stmt_id, body)
            }
            Stmt::Label(_) => {
                crate::CFG_WALK_LABEL_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.walk_label_statement_hir(stmt_id, body)
            }
            _ => {
                // Regular statement (Assign, Expr, VarDecl, etc.) - add to current block
                crate::CFG_WALK_OTHER_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.add_to_current_block_hir(stmt_id);
            }
        }
    }

    /* LEGACY AST-BASED METHOD - DISABLED IN PHASE 6.4
     *
     * Use add_to_current_block_hir() instead.
     *
    /// Add a statement to the current basic block (AST-based - LEGACY)
    fn add_to_current_block(&mut self, stmt: SyntaxNode) {
        if let Some(block_idx) = self.current_block {
            if let Some(CfgVertex::BasicBlock(block)) = self.cfg.vertex_mut(block_idx) {
                block.add_statement(stmt);
            }
        }
    }
    */

    /// Add a HIR statement to the current basic block (Phase 6.2)
    fn add_to_current_block_hir(&mut self, stmt_id: StmtId) {
        if let Some(block_idx) = self.current_block {
            if let Some(CfgVertex::BasicBlock(block)) = self.cfg.vertex_mut(block_idx) {
                block.add_statement(stmt_id);
            }
        }
    }

    /* LEGACY AST-BASED METHODS - DISABLED IN PHASE 6.4
     *
     * Use walk_return_statement_hir() and walk_raise_statement_hir() instead.
     *
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
    */

    /// Walk a HIR return statement (Phase 6.2)
    fn walk_return_statement_hir(&mut self, stmt_id: StmtId, _body: &Body) {
        // Add return statement to current block
        self.add_to_current_block_hir(stmt_id);

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

    /// Walk a HIR raise statement (Phase 6.2)
    fn walk_raise_statement_hir(&mut self, stmt_id: StmtId, _body: &Body) {
        // Add raise statement to current block
        self.add_to_current_block_hir(stmt_id);

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

    /// Walk a HIR break statement (Phase 6.2)
    fn walk_break_statement_hir(&mut self, stmt_id: StmtId) {
        // Add break to current block
        self.add_to_current_block_hir(stmt_id);

        // TODO(Phase 6.2): Connect to loop exit point (requires loop context tracking)
        // For now, create dead block
        let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        if let Some(block_idx) = self.current_block {
            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);
        }
        self.current_block = Some(dead_block);
    }

    /// Walk a HIR continue statement (Phase 6.2)
    fn walk_continue_statement_hir(&mut self, stmt_id: StmtId) {
        // Add continue to current block
        self.add_to_current_block_hir(stmt_id);

        // TODO(Phase 6.2): Connect to loop header (requires loop context tracking)
        // For now, create dead block
        let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        if let Some(block_idx) = self.current_block {
            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);
        }
        self.current_block = Some(dead_block);
    }

    /// Walk a HIR goto statement (Phase 6.2)
    fn walk_goto_statement_hir(&mut self, stmt_id: StmtId, _body: &Body) {
        // Add goto to current block
        self.add_to_current_block_hir(stmt_id);

        // TODO(Phase 6.2): Connect to label vertex (requires label tracking)
        // For now, create dead block
        let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        if let Some(block_idx) = self.current_block {
            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);
        }
        self.current_block = Some(dead_block);
    }

    /// Walk a HIR label statement (Phase 6.2)
    fn walk_label_statement_hir(&mut self, _stmt_id: StmtId, body: &Body) {
        use crate::vertex::LabelVertex;

        // Extract label name from statement
        if let Stmt::Label(name) = body.stmt(_stmt_id) {
            // Create label vertex
            let label_vertex =
                self.cfg.add_vertex(CfgVertex::Label(LabelVertex::new(name.clone())));

            // Connect current block to label
            if let Some(current) = self.current_block {
                let _ = self.cfg.add_edge(current, label_vertex, CfgEdgeType::Direct);
            }

            // Create new block after label
            let after_label = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(label_vertex, after_label, CfgEdgeType::Direct);

            self.current_block = Some(after_label);
        }
    }

    /* LEGACY AST-BASED METHODS - DISABLED IN PHASE 6.4
     *
     * Use walk_if_statement_hir() instead.
     *
    // TODO: Implement control flow walkers

    /// Walk an if statement (AST-based - LEGACY)
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
    */

    /// Check if a block is reachable (has incoming edges or is entry point)
    fn is_block_reachable(&self, block: NodeIndex) -> bool {
        // A block is reachable if:
        // 1. It has incoming edges, OR
        // 2. It's the entry point
        let has_incoming = self.cfg.incoming_edges(block).next().is_some();
        let is_entry = self.cfg.entry_point() == Some(block);
        has_incoming || is_entry
    }

    /// Walk a HIR if statement (Phase 6.2)
    ///
    /// Direct pattern matching on Stmt::If - no find() needed!
    ///
    /// Algorithm:
    /// 1. Create ConditionalVertex for IF condition (ExprId directly available)
    /// 2. Connect current block → conditional
    /// 3. Walk THEN branch (Box<[StmtId]> directly available)
    /// 4. For each ELSIF: already structured as Vec<(ExprId, Box<[StmtId]>)>
    /// 5. Walk ELSE branch if present (Option<Box<[StmtId]>>)
    /// 6. Create merge block
    /// 7. Connect all branch exits → merge
    fn walk_if_statement_hir(&mut self, stmt_id: StmtId, body: &Body) {
        use crate::vertex::ConditionalVertex;

        if let Stmt::If { condition, then_branch, elsif_branches, else_branch } = body.stmt(stmt_id)
        {
            // Create conditional vertex (condition is ExprId - no searching!)
            let cond_vertex =
                self.cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(*condition)));

            // Connect current block to conditional
            if let Some(current) = self.current_block {
                let _ = self.cfg.add_edge(current, cond_vertex, CfgEdgeType::Direct);
            }

            // Create merge block (will be used after all branches)
            let merge_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            // Walk THEN branch (then_branch is Box<[StmtId]> - no searching!)
            let then_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(cond_vertex, then_block, CfgEdgeType::TrueBranch);
            self.current_block = Some(then_block);

            for &then_stmt_id in then_branch.iter() {
                self.walk_statement_hir(then_stmt_id, body);
            }

            // Save then exit and check if reachable
            let then_exit = self.current_block;
            if let Some(exit) = then_exit {
                if self.is_block_reachable(exit) {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                }
            }

            // Process ELSIF clauses (already structured as Vec<(ExprId, Box<[StmtId]>)>!)
            let mut current_cond = cond_vertex;

            for (elsif_condition, elsif_body) in elsif_branches.iter() {
                // Create elsif conditional (condition is ExprId - no searching!)
                let elsif_cond = self
                    .cfg
                    .add_vertex(CfgVertex::Conditional(ConditionalVertex::new(*elsif_condition)));

                // Connect previous cond FALSE → this elsif cond
                let _ = self.cfg.add_edge(current_cond, elsif_cond, CfgEdgeType::FalseBranch);

                // Walk elsif body (elsif_body is Box<[StmtId]> - no searching!)
                let elsif_block =
                    self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
                let _ = self.cfg.add_edge(elsif_cond, elsif_block, CfgEdgeType::TrueBranch);
                self.current_block = Some(elsif_block);

                for &elsif_stmt_id in elsif_body.iter() {
                    self.walk_statement_hir(elsif_stmt_id, body);
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

            // Process ELSE clause if present (else_branch is Option<Box<[StmtId]>>)
            if let Some(else_stmts) = else_branch {
                // Create else block
                let else_block =
                    self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
                let _ = self.cfg.add_edge(current_cond, else_block, CfgEdgeType::FalseBranch);
                self.current_block = Some(else_block);

                // Walk else body (else_stmts is Box<[StmtId]> - no searching!)
                for &else_stmt_id in else_stmts.iter() {
                    self.walk_statement_hir(else_stmt_id, body);
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
    }

    /* LEGACY AST-BASED METHOD - DISABLED IN PHASE 6.4
     *
     * Use walk_while_statement_hir() instead.
     *
    /// Walk a while statement (AST-based - LEGACY)
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
    */

    /// Walk a HIR while statement (Phase 6.2)
    fn walk_while_statement_hir(&mut self, stmt_id: StmtId, body: &Body) {
        use crate::vertex::WhileLoopVertex;

        if let Stmt::While { condition, body: loop_body } = body.stmt(stmt_id) {
            // Create while loop vertex (condition is ExprId - no searching!)
            let loop_vertex =
                self.cfg.add_vertex(CfgVertex::WhileLoop(WhileLoopVertex::new(*condition)));

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

            // Walk loop body (loop_body is Box<[StmtId]> - no searching!)
            for &loop_stmt_id in loop_body.iter() {
                self.walk_statement_hir(loop_stmt_id, body);
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
    }

    /// Walk a HIR for statement (Phase 6.2)
    fn walk_for_statement_hir(&mut self, stmt_id: StmtId, body: &Body) {
        use crate::vertex::ForLoopVertex;

        if let Stmt::For { var, from: _, to: _, body: loop_body } = body.stmt(stmt_id) {
            // Create for loop vertex (var is BindingId - no searching!)
            let loop_vertex = self.cfg.add_vertex(CfgVertex::ForLoop(ForLoopVertex::new(*var)));

            // Connect current block to loop
            if let Some(current) = self.current_block {
                let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::Direct);
            }

            // Create loop body block
            let body_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            // Connect loop → body (true branch)
            let _ = self.cfg.add_edge(loop_vertex, body_block, CfgEdgeType::TrueBranch);

            self.current_block = Some(body_block);

            // Walk loop body (loop_body is Box<[StmtId]> - no searching!)
            for &loop_stmt_id in loop_body.iter() {
                self.walk_statement_hir(loop_stmt_id, body);
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
    }

    /// Walk a HIR foreach statement (Phase 6.2)
    fn walk_foreach_statement_hir(&mut self, stmt_id: StmtId, body: &Body) {
        use crate::vertex::ForEachLoopVertex;

        if let Stmt::ForEach { var, collection, body: loop_body } = body.stmt(stmt_id) {
            // Create foreach loop vertex (var is BindingId - no searching!)
            let loop_vertex = self
                .cfg
                .add_vertex(CfgVertex::ForEachLoop(ForEachLoopVertex::new(*var, *collection)));

            // Connect current block to loop
            if let Some(current) = self.current_block {
                let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::Direct);
            }

            // Create loop body block
            let body_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            // Connect loop → body (true branch)
            let _ = self.cfg.add_edge(loop_vertex, body_block, CfgEdgeType::TrueBranch);

            self.current_block = Some(body_block);

            // Walk loop body (loop_body is Box<[StmtId]> - no searching!)
            for &loop_stmt_id in loop_body.iter() {
                self.walk_statement_hir(loop_stmt_id, body);
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
    }

    /// Walk a HIR try statement (Phase 6.2)
    fn walk_try_statement_hir(&mut self, stmt_id: StmtId, body: &Body) {
        use crate::vertex::TryExceptVertex;

        if let Stmt::Try { body: try_body, except } = body.stmt(stmt_id) {
            // Create try-except vertex
            let try_vertex = self.cfg.add_vertex(CfgVertex::TryExcept(TryExceptVertex::new()));

            // Connect current block to try
            if let Some(current) = self.current_block {
                let _ = self.cfg.add_edge(current, try_vertex, CfgEdgeType::Direct);
            }

            // Create try body block
            let try_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(try_vertex, try_block, CfgEdgeType::TrueBranch);
            self.current_block = Some(try_block);

            // Walk try body (try_body is Box<[StmtId]> - no searching!)
            for &try_stmt_id in try_body.iter() {
                self.walk_statement_hir(try_stmt_id, body);
            }

            let try_exit = self.current_block;

            // Create except body block
            let except_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(try_vertex, except_block, CfgEdgeType::FalseBranch);

            // Walk except body (except is Box<[StmtId]> - no searching!)
            self.current_block = Some(except_block);
            for &except_stmt_id in except.iter() {
                self.walk_statement_hir(except_stmt_id, body);
            }

            let except_exit = self.current_block;

            // Create merge block
            let merge_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            // Connect both exits to merge
            if let Some(exit) = try_exit {
                if self.is_block_reachable(exit) {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                }
            }
            if let Some(exit) = except_exit {
                if self.is_block_reachable(exit) {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                }
            }

            self.current_block = Some(merge_block);
        }
    }

    /* LEGACY AST-BASED METHODS - DISABLED IN PHASE 6.4
     *
     * Use walk_for_statement_hir(), walk_foreach_statement_hir(), walk_try_statement_hir(),
     * walk_break_statement_hir(), walk_continue_statement_hir() instead.
     *
    /// Walk a for statement (AST-based - LEGACY)
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
    */
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

    #[test]
    fn test_hir_based_cfg_simple() {
        use hir_def::{Binding, Body, Expr, Literal, Stmt};
        use ordered_float::NotNan;

        // Create a simple HIR Body:
        // Перем А;
        // А = 42;
        // Возврат А;
        let mut body = Body::default();

        // Create binding for variable А
        let var_a = body.bindings.alloc(Binding::var(hir_def::Name::new("А")));

        // Create statements
        let lit_42 = body.exprs.alloc(Expr::Literal(Literal::Number(NotNan::new(42.0).unwrap())));
        let path_a = body.exprs.alloc(Expr::Path(hir_def::Name::new("А")));

        let var_decl = body.stmts.alloc(Stmt::VarDecl { bindings: vec![var_a].into() });
        let assign = body.stmts.alloc(Stmt::Assign { target: path_a, value: lit_42 });
        let return_stmt = body.stmts.alloc(Stmt::Return { value: Some(path_a) });

        body.body_stmts = vec![var_decl, assign, return_stmt].into();

        // Build CFG from HIR
        let cfg = CfgBuilder::new().build_graph_from_hir(&body.body_stmts, &body, None);

        // Verify CFG structure
        assert!(cfg.entry_point().is_some(), "CFG should have entry point");
        assert!(cfg.exit_point() != cfg.entry_point().unwrap(), "Exit should differ from entry");

        // Verify vertex count (entry + basic block + exit)
        // Entry block contains all 3 statements, then connects to exit
        let vertex_count = cfg.graph().node_count();
        assert!(
            vertex_count >= 2,
            "Should have at least entry and exit vertices, got {}",
            vertex_count
        );

        // Verify return statement connects to exit
        let exit = cfg.exit_point();
        let incoming_to_exit: Vec<_> = cfg.incoming_edges(exit).collect();
        assert!(!incoming_to_exit.is_empty(), "Exit should have incoming edges");
    }

    #[test]
    fn test_hir_based_cfg_if_statement() {
        use hir_def::{Body, Expr, Literal, Stmt};
        use ordered_float::NotNan;

        // Create HIR Body:
        // Если Истина Тогда
        //     Возврат 1;
        // Иначе
        //     Возврат 2;
        // КонецЕсли;
        let mut body = Body::default();

        let true_lit = body.exprs.alloc(Expr::Literal(Literal::Bool(true)));
        let lit_1 = body.exprs.alloc(Expr::Literal(Literal::Number(NotNan::new(1.0).unwrap())));
        let lit_2 = body.exprs.alloc(Expr::Literal(Literal::Number(NotNan::new(2.0).unwrap())));

        let return_1 = body.stmts.alloc(Stmt::Return { value: Some(lit_1) });
        let return_2 = body.stmts.alloc(Stmt::Return { value: Some(lit_2) });

        let if_stmt = body.stmts.alloc(Stmt::If {
            condition: true_lit,
            then_branch: vec![return_1].into(),
            elsif_branches: vec![].into(),
            else_branch: Some(vec![return_2].into()),
        });

        body.body_stmts = vec![if_stmt].into();

        // Build CFG
        let cfg = CfgBuilder::new().build_graph_from_hir(&body.body_stmts, &body, None);

        // Verify CFG has conditional structure
        let vertex_count = cfg.graph().node_count();
        // Should have: entry, conditional, then block, else block, merge, exit (at least 5)
        assert!(
            vertex_count >= 5,
            "If-else CFG should have multiple vertices, got {}",
            vertex_count
        );

        // Verify conditional vertex exists
        let has_conditional = cfg.graph().node_indices().any(|idx| {
            if let Some(vertex) = cfg.vertex(idx) {
                matches!(vertex, CfgVertex::Conditional(_))
            } else {
                false
            }
        });
        assert!(has_conditional, "CFG should contain conditional vertex for if statement");
    }

    // TODO: Add more integration tests with real BSL code after parser integration
}
