//! CFG Builder - constructs Control Flow Graph from BSL functions
//!
//! Constructs CFG from HIR Body (StmtId, ExprId, BindingId) via `build_graph_from_hir()`.
//! Direct pattern matching on Stmt enum enables dataflow analysis.

use crate::edge::CfgEdgeType;
use crate::graph::ControlFlowGraph;
use crate::vertex::{BasicBlockVertex, CfgVertex};
use cfg_types::{BindingId, ExprId, IdConversion, StmtId};
use hir_def::hir::StmtIdx;
use hir_def::{Body, BodySourceMap, Stmt};
use petgraph::graph::NodeIndex;

/// CFG Builder for BSL functions/procedures
///
/// Constructs a Control Flow Graph by walking HIR of a function body.
pub struct CfgBuilder {
    /// The CFG being constructed
    cfg: ControlFlowGraph,

    /// Current basic block being built
    current_block: Option<NodeIndex>,

    /// Whether to produce loop iteration edges (back edges)
    /// Used for configuration: loopsExecutedAtLeastOnce
    produce_loop_iterations: bool,

    /// Stack of except block indices for modeling exception flow.
    /// When Raise is encountered inside a Try block, control transfers
    /// to the corresponding except block instead of the function exit.
    except_stack: Vec<NodeIndex>,
}

impl CfgBuilder {
    /// Create a new CFG builder
    pub fn new() -> Self {
        Self {
            cfg: ControlFlowGraph::new(),
            current_block: None,
            produce_loop_iterations: true,
            except_stack: Vec::new(),
        }
    }

    /// Set whether to produce loop iteration edges
    ///
    /// When true (default), adds back edges from loop body to loop condition.
    /// This affects diagnostics that check for missing returns - if loops
    /// are assumed to execute at least once, paths through loops are considered.
    pub fn produce_loop_iterations(&mut self, value: bool) {
        self.produce_loop_iterations = value;
    }

    /// Build CFG from HIR Body
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
        body_stmts: &[StmtIdx],
        body: &Body,
        _source_map: Option<&BodySourceMap>,
    ) -> ControlFlowGraph {
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
                // Use AdjacentCode for unreachable blocks (e.g., merge block after
                // if/else where all branches return) to avoid false positives
                // in missing-return diagnostics
                if self.block_has_live_incoming(block_idx) {
                    let _ = self.cfg.add_edge(block_idx, exit, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(block_idx, exit, CfgEdgeType::AdjacentCode);
                }
            }
        }

        self.cfg
    }

    /// Walk a single HIR statement
    ///
    /// Pattern matches on Stmt enum - no AST traversal needed.
    fn walk_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        let stmt = body.stmt_idx(stmt_id);
        match stmt {
            Stmt::Return { .. } => self.walk_return_statement_hir(stmt_id, body),
            Stmt::Raise { .. } => self.walk_raise_statement_hir(stmt_id, body),
            Stmt::If(_) => self.walk_if_statement_hir(stmt_id, body),
            Stmt::PreprocIf(_) => self.walk_preproc_if_statement_hir(stmt_id, body),
            Stmt::While { .. } => self.walk_while_statement_hir(stmt_id, body),
            Stmt::For { .. } => self.walk_for_statement_hir(stmt_id, body),
            Stmt::ForEach { .. } => self.walk_foreach_statement_hir(stmt_id, body),
            Stmt::Try { .. } => self.walk_try_statement_hir(stmt_id, body),
            Stmt::Break => self.walk_break_statement_hir(stmt_id),
            Stmt::Continue => self.walk_continue_statement_hir(stmt_id),
            Stmt::Goto(_) => self.walk_goto_statement_hir(stmt_id, body),
            Stmt::Label(_) => self.walk_label_statement_hir(stmt_id, body),
            _ => {
                // Regular statement (Assign, Expr, VarDecl, etc.) - add to current block
                self.add_to_current_block_hir(stmt_id);
            }
        }
    }

    /// Add a HIR statement to the current basic block
    fn add_to_current_block_hir(&mut self, stmt_id: StmtIdx) {
        if let Some(block_idx) = self.current_block {
            if let Some(CfgVertex::BasicBlock(block)) = self.cfg.vertex_mut(block_idx) {
                // Convert typed StmtIdx to opaque StmtId for storage
                block.add_statement(StmtId::from_idx(stmt_id));
            }
        }
    }

    /// Walk a HIR return statement
    fn walk_return_statement_hir(&mut self, stmt_id: StmtIdx, _body: &Body) {
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
    ///
    /// If inside a try block, transfers control to the corresponding except block.
    /// Otherwise, transfers control to the function exit (uncaught exception).
    fn walk_raise_statement_hir(&mut self, stmt_id: StmtIdx, _body: &Body) {
        // Add raise statement to current block
        self.add_to_current_block_hir(stmt_id);

        if let Some(block_idx) = self.current_block {
            // Determine target: except block if inside try, otherwise function exit
            let target = if let Some(&except_block) = self.except_stack.last() {
                // Inside a try block - transfer to except handler
                except_block
            } else {
                // Outside try - uncaught exception terminates the function
                self.cfg.exit_point()
            };

            // Connect current block to target
            let _ = self.cfg.add_edge(block_idx, target, CfgEdgeType::Direct);

            // Create new unreachable block for any dead code after raise
            let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            // Connect via AdjacentCode edge
            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);

            self.current_block = Some(dead_block);
        }
    }

    /// Walk a HIR break statement (Phase 6.2)
    fn walk_break_statement_hir(&mut self, stmt_id: StmtIdx) {
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
    fn walk_continue_statement_hir(&mut self, stmt_id: StmtIdx) {
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
    fn walk_goto_statement_hir(&mut self, stmt_id: StmtIdx, _body: &Body) {
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
    fn walk_label_statement_hir(&mut self, _stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::LabelVertex;

        // Extract label name from statement
        if let Stmt::Label(name) = body.stmt_idx(_stmt_id) {
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

    /// Check if a block is reachable (has incoming edges or is entry point)
    fn is_block_reachable(&self, block: NodeIndex) -> bool {
        let has_incoming = self.cfg.incoming_edges(block).next().is_some();
        let is_entry = self.cfg.entry_point() == Some(block);
        has_incoming || is_entry
    }

    /// Check if a block has at least one live (non-dead-code) incoming edge.
    ///
    /// This is used to determine if control flow should continue from a branch:
    /// - After return/raise/break/continue/goto, current_block is a dead_block
    /// - dead_block only has AdjacentCode incoming edge (dead code marker)
    /// - We should NOT connect dead_block to merge block
    fn block_has_live_incoming(&self, block: NodeIndex) -> bool {
        // Entry point is always live
        if self.cfg.entry_point() == Some(block) {
            return true;
        }
        // Check if any incoming edge is live (not AdjacentCode)
        self.cfg.incoming_edges(block).any(|(_, edge_type)| !edge_type.is_dead_code_edge())
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
    fn walk_if_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::ConditionalVertex;

        if let Stmt::If(if_stmt) = body.stmt_idx(stmt_id) {
            // Create conditional vertex - convert typed to opaque
            let cond_vertex = self.cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(
                ExprId::from_idx(if_stmt.condition),
            )));

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

            for &then_stmt_id in if_stmt.then_branch.iter() {
                self.walk_statement_hir(then_stmt_id, body);
            }

            // Connect then branch to merge:
            // - Direct edge if branch is reachable (has live incoming edges)
            // - AdjacentCode (phantom) edge if branch terminates (for backward traversal in unreachable detection)
            let then_exit = self.current_block;
            if let Some(exit) = then_exit {
                if self.block_has_live_incoming(exit) {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                } else {
                    // Phantom edge for backward traversal (unreachable code detection)
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::AdjacentCode);
                }
            }

            // Process ELSIF clauses (already structured as Vec<(ExprId, Box<[StmtId]>)>!)
            let mut current_cond = cond_vertex;

            for (elsif_condition, elsif_body) in if_stmt.elsif_branches.iter() {
                // Create elsif conditional - convert typed to opaque
                let elsif_cond = self.cfg.add_vertex(CfgVertex::Conditional(
                    ConditionalVertex::new(ExprId::from_idx(*elsif_condition)),
                ));

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

                // Connect elsif branch to merge:
                // - Direct if reachable, AdjacentCode (phantom) if terminated
                let elsif_exit = self.current_block;
                if let Some(exit) = elsif_exit {
                    if self.block_has_live_incoming(exit) {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                    } else {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::AdjacentCode);
                    }
                }

                current_cond = elsif_cond;
            }

            // Process ELSE clause if present (else_branch is Option<Box<[StmtId]>>)
            if let Some(ref else_stmts) = if_stmt.else_branch {
                // Create else block
                let else_block =
                    self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
                let _ = self.cfg.add_edge(current_cond, else_block, CfgEdgeType::FalseBranch);
                self.current_block = Some(else_block);

                // Walk else body (else_stmts is Box<[StmtId]> - no searching!)
                for &else_stmt_id in else_stmts.iter() {
                    self.walk_statement_hir(else_stmt_id, body);
                }

                // Connect else exit to merge:
                // - Direct if reachable, AdjacentCode (phantom) if terminated
                let else_exit = self.current_block;
                if let Some(exit) = else_exit {
                    if self.block_has_live_incoming(exit) {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                    } else {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::AdjacentCode);
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

    /// Walk a HIR preprocessor #Если statement (Phase 6.4)
    ///
    /// Similar to walk_if_statement_hir but uses PreprocConditionVertex.
    /// Preprocessor conditions are symbolic (not runtime-evaluable), so we use TextRange.
    ///
    /// Algorithm:
    /// 1. Create PreprocConditionVertex for #Если condition
    /// 2. Connect current block → preprocessor conditional
    /// 3. Walk then_branch (Box<[StmtIdx]>)
    /// 4. For each #ИначеЕсли: create another PreprocConditionVertex, chain it
    /// 5. Walk #Иначе branch if present
    /// 6. Create merge block
    /// 7. Connect all branch exits → merge
    fn walk_preproc_if_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::PreprocConditionVertex;

        if let Stmt::PreprocIf(preproc_if) = body.stmt_idx(stmt_id) {
            // Create preprocessor conditional vertex
            let cond_vertex = self.cfg.add_vertex(CfgVertex::PreprocCondition(
                PreprocConditionVertex::with_ranges(
                    preproc_if.condition_range,
                    preproc_if.directive_range,
                    preproc_if.full_range,
                ),
            ));

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

            for &then_stmt_id in preproc_if.then_branch.iter() {
                self.walk_statement_hir(then_stmt_id, body);
            }

            // Connect then branch to merge:
            // - Direct if reachable, AdjacentCode (phantom) if terminated
            let then_exit = self.current_block;
            if let Some(exit) = then_exit {
                if self.block_has_live_incoming(exit) {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::AdjacentCode);
                }
            }

            // Process #ИначеЕсли clauses
            let mut current_cond = cond_vertex;

            for (elsif_condition_range, elsif_directive_range, elsif_body) in
                preproc_if.elsif_branches.iter()
            {
                // Create elsif preprocessor conditional
                let elsif_cond = self.cfg.add_vertex(CfgVertex::PreprocCondition(
                    PreprocConditionVertex::with_directive_range(
                        *elsif_condition_range,
                        *elsif_directive_range,
                    ),
                ));

                // Connect previous cond FALSE → this elsif cond
                let _ = self.cfg.add_edge(current_cond, elsif_cond, CfgEdgeType::FalseBranch);

                // Walk elsif body
                let elsif_block =
                    self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
                let _ = self.cfg.add_edge(elsif_cond, elsif_block, CfgEdgeType::TrueBranch);
                self.current_block = Some(elsif_block);

                for &elsif_stmt_id in elsif_body.iter() {
                    self.walk_statement_hir(elsif_stmt_id, body);
                }

                // Connect elsif branch to merge:
                // - Direct if reachable, AdjacentCode (phantom) if terminated
                let elsif_exit = self.current_block;
                if let Some(exit) = elsif_exit {
                    if self.block_has_live_incoming(exit) {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                    } else {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::AdjacentCode);
                    }
                }

                current_cond = elsif_cond;
            }

            // Process #Иначе clause if present
            if let Some(ref else_stmts) = preproc_if.else_branch {
                // Create else block
                let else_block =
                    self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
                let _ = self.cfg.add_edge(current_cond, else_block, CfgEdgeType::FalseBranch);
                self.current_block = Some(else_block);

                // Walk else body
                for &else_stmt_id in else_stmts.iter() {
                    self.walk_statement_hir(else_stmt_id, body);
                }

                // Connect else exit to merge:
                // - Direct if reachable, AdjacentCode (phantom) if terminated
                let else_exit = self.current_block;
                if let Some(exit) = else_exit {
                    if self.block_has_live_incoming(exit) {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                    } else {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::AdjacentCode);
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

    /// Walk a HIR while statement
    fn walk_while_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::WhileLoopVertex;

        if let Stmt::While { condition, body: loop_body } = body.stmt_idx(stmt_id) {
            // Create while loop vertex - convert typed to opaque
            let loop_vertex = self.cfg.add_vertex(CfgVertex::WhileLoop(WhileLoopVertex::new(
                ExprId::from_idx(*condition),
            )));

            // Connect current block to loop if it's reachable
            if let Some(current) = self.current_block {
                if self.block_has_live_incoming(current) {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::AdjacentCode);
                }
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
    fn walk_for_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::ForLoopVertex;

        if let Stmt::For { var, from: _, to: _, body: loop_body } = body.stmt_idx(stmt_id) {
            // Create for loop vertex - convert typed to opaque
            let loop_vertex = self.cfg.add_vertex(CfgVertex::ForLoop(ForLoopVertex::with_stmt_id(
                BindingId::from_idx(*var),
                StmtId::from_idx(stmt_id),
            )));

            // Connect current block to loop if it's reachable
            if let Some(current) = self.current_block {
                if self.block_has_live_incoming(current) {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::AdjacentCode);
                }
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
    fn walk_foreach_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::ForEachLoopVertex;

        if let Stmt::ForEach { var, collection, body: loop_body } = body.stmt_idx(stmt_id) {
            // Create foreach loop vertex - convert typed to opaque
            let loop_vertex =
                self.cfg.add_vertex(CfgVertex::ForEachLoop(ForEachLoopVertex::with_stmt_id(
                    BindingId::from_idx(*var),
                    ExprId::from_idx(*collection),
                    StmtId::from_idx(stmt_id),
                )));

            // Connect current block to loop if it's reachable
            // After return/raise/etc., current_block is dead - use AdjacentCode edge
            if let Some(current) = self.current_block {
                if self.block_has_live_incoming(current) {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::AdjacentCode);
                }
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
    ///
    /// Models exception flow: Raise inside try block transfers control to except block.
    fn walk_try_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::TryExceptVertex;

        if let Stmt::Try { body: try_body, except } = body.stmt_idx(stmt_id) {
            // Create try-except vertex
            let try_vertex = self.cfg.add_vertex(CfgVertex::TryExcept(TryExceptVertex::new()));

            // Connect current block to try
            if let Some(current) = self.current_block {
                let _ = self.cfg.add_edge(current, try_vertex, CfgEdgeType::Direct);
            }

            // Create try body block
            let try_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(try_vertex, try_block, CfgEdgeType::TrueBranch);

            // Create except body block BEFORE walking try body
            // (needed for exception flow modeling)
            let except_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(try_vertex, except_block, CfgEdgeType::FalseBranch);

            // Push except block to stack so Raise knows where to jump
            self.except_stack.push(except_block);

            // Walk try body
            self.current_block = Some(try_block);
            for &try_stmt_id in try_body.iter() {
                self.walk_statement_hir(try_stmt_id, body);
            }

            let try_exit = self.current_block;

            // Pop except block from stack (leaving try scope)
            self.except_stack.pop();

            // Walk except body
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
        let var_a = body.bindings_mut().alloc(Binding::var(hir_def::Name::new("А")));

        // Create statements
        let lit_42 =
            body.exprs_mut().alloc(Expr::Literal(Literal::Number(NotNan::new(42.0).unwrap())));
        let path_a = body.exprs_mut().alloc(Expr::Path(hir_def::Name::new("А")));

        let var_decl = body.stmts_mut().alloc(Stmt::VarDecl { bindings: vec![var_a].into() });
        let assign = body.stmts_mut().alloc(Stmt::Assign { target: path_a, value: lit_42 });
        let return_stmt = body.stmts_mut().alloc(Stmt::Return { value: Some(path_a) });

        body.set_body_stmts(vec![var_decl, assign, return_stmt].into());

        // Build CFG from HIR
        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

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

        let true_lit = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
        let lit_1 =
            body.exprs_mut().alloc(Expr::Literal(Literal::Number(NotNan::new(1.0).unwrap())));
        let lit_2 =
            body.exprs_mut().alloc(Expr::Literal(Literal::Number(NotNan::new(2.0).unwrap())));

        let return_1 = body.stmts_mut().alloc(Stmt::Return { value: Some(lit_1) });
        let return_2 = body.stmts_mut().alloc(Stmt::Return { value: Some(lit_2) });

        let if_stmt = body.stmts_mut().alloc(Stmt::If(Box::new(hir_def::IfStmt {
            condition: true_lit,
            then_branch: vec![return_1].into(),
            elsif_branches: vec![].into(),
            else_branch: Some(vec![return_2].into()),
        })));

        body.set_body_stmts(vec![if_stmt].into());

        // Build CFG
        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

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
