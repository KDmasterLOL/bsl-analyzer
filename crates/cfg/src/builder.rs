//! CFG Builder - constructs Control Flow Graph from BSL functions
//!
//! Constructs CFG from HIR Body (StmtId, ExprId, BindingId) via `build_graph_from_hir()`.
//! Direct pattern matching on Stmt enum enables dataflow analysis.

use crate::edge::CfgEdgeType;
use crate::graph::ControlFlowGraph;
use crate::vertex::{BasicBlockVertex, CfgVertex};
use cfg_types::{BindingId, ExprId, IdConversion, StmtId};
use hir_def::hir::StmtIdx;
use hir_def::{Body, BodySourceMap, Name, Stmt};
use petgraph::graph::NodeIndex;
use rustc_hash::FxHashMap;

/// One frame of the loop-context stack used to wire `Прервать` and
/// `Продолжить` to live targets.
///
/// `header` is the loop's condition vertex (the `WhileLoop` /
/// `ForLoop` / `ForEachLoop`); `exit` is the merge `BasicBlock` that
/// `walk_*_statement_hir` creates to receive the false branch from
/// the header. Nested loops push frames in source order — the
/// innermost frame is `loop_stack.last()`.
struct LoopFrame {
    header: NodeIndex,
    exit: NodeIndex,
}

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

    /// Stack of `(loop_header, after_loop)` pairs for the enclosing
    /// loops. `walk_break_statement_hir` reads `last().exit` to point
    /// the live `LoopBreak` edge at the after-loop merge; the matching
    /// `walk_continue_statement_hir` reads `last().header` for the
    /// `LoopContinue` back-edge. Empty outside any loop — `Прервать` /
    /// `Продолжить` outside a loop is a parser/lowering error and is
    /// not the CFG's responsibility to diagnose.
    loop_stack: Vec<LoopFrame>,

    /// Resolved labels visited so far. `walk_goto_statement_hir`
    /// consults this map for backward jumps; forward jumps that haven't
    /// seen the matching `walk_label_statement_hir` yet land in
    /// `pending_gotos` and get patched when the label arrives.
    label_table: FxHashMap<Name, NodeIndex>,

    /// Forward `goto`s waiting for their label to be declared. Each
    /// entry is `(source_block, label_name)`. `walk_label_statement_hir`
    /// drains every entry that names the new label and adds a `Direct`
    /// edge from `source_block` to the freshly-created label vertex.
    /// Entries that are still pending after the body finishes name
    /// labels that never appear — Track 6 owns the diagnostic; the
    /// CFG simply leaves those source blocks with no outgoing
    /// `Direct` edge for the goto.
    pending_gotos: Vec<(NodeIndex, Name)>,
}

impl CfgBuilder {
    /// Create a new CFG builder
    pub fn new() -> Self {
        Self {
            cfg: ControlFlowGraph::new(),
            current_block: None,
            produce_loop_iterations: true,
            except_stack: Vec::new(),
            loop_stack: Vec::new(),
            label_table: FxHashMap::default(),
            pending_gotos: Vec::new(),
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
        // Recovered `Stmt::Expr`s come from parser ERROR recovery (see
        // `hir-def::body::lower::stmt::try_lower_recovered_expr_stmt`). They
        // carry usable type info for completion/hover but represent code the
        // user is still typing — including them in the CFG risks feeding
        // dataflow / reachability checks junk and flickering diagnostics at
        // the user. Skip them here.
        if let Stmt::Expr(expr_idx) = stmt {
            if body.is_recovered(ExprId::from_idx(*expr_idx)) {
                return;
            }
        }
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

    /// Walk a HIR `Прервать` statement.
    ///
    /// Wires two outgoing edges from the block ending in `Прервать`:
    /// - **Live**: `LoopBreak` to the innermost loop's after-loop merge
    ///   (`loop_stack.last().exit`). Skipped when the stack is empty —
    ///   `Прервать` outside a loop is a parser/lowering error, not the
    ///   CFG's concern.
    /// - **Dead-fallthrough**: `AdjacentCode` to a fresh dead block, so
    ///   any source code that textually follows still has a successor
    ///   that reachability analysis can flag as unreachable. The doc on
    ///   [`crate::edge::CfgEdgeType::AdjacentCode`] explains the dual
    ///   semantics.
    fn walk_break_statement_hir(&mut self, stmt_id: StmtIdx) {
        self.add_to_current_block_hir(stmt_id);

        if let Some(block_idx) = self.current_block {
            if let Some(frame) = self.loop_stack.last() {
                let _ = self.cfg.add_edge(block_idx, frame.exit, CfgEdgeType::LoopBreak);
            }
        }

        let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        if let Some(block_idx) = self.current_block {
            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);
        }
        self.current_block = Some(dead_block);
    }

    /// Walk a HIR `Продолжить` statement.
    ///
    /// Symmetrical to [`Self::walk_break_statement_hir`]: live
    /// `LoopContinue` edge back to the innermost loop's header
    /// (`loop_stack.last().header`) plus an `AdjacentCode`
    /// dead-fallthrough for any code that textually follows.
    fn walk_continue_statement_hir(&mut self, stmt_id: StmtIdx) {
        self.add_to_current_block_hir(stmt_id);

        if let Some(block_idx) = self.current_block {
            if let Some(frame) = self.loop_stack.last() {
                let _ = self.cfg.add_edge(block_idx, frame.header, CfgEdgeType::LoopContinue);
            }
        }

        let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        if let Some(block_idx) = self.current_block {
            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);
        }
        self.current_block = Some(dead_block);
    }

    /// Walk a HIR `Перейти ~Label` statement.
    ///
    /// Backward jumps (target already declared) get a live `Direct`
    /// edge straight to the existing label vertex. Forward jumps
    /// (target not yet seen) park in `pending_gotos` and the matching
    /// label walker drains them when it arrives. Either way the source
    /// block also gets an `AdjacentCode` dead-fallthrough successor.
    /// Unresolved labels at body end leave the source block without a
    /// live outgoing `Direct` for the goto — Track 6 owns the
    /// `UnresolvedLabel` diagnostic.
    fn walk_goto_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        self.add_to_current_block_hir(stmt_id);

        if let (Some(block_idx), Stmt::Goto(name)) = (self.current_block, body.stmt_idx(stmt_id)) {
            if let Some(&label_vertex) = self.label_table.get(name) {
                let _ = self.cfg.add_edge(block_idx, label_vertex, CfgEdgeType::Direct);
            } else {
                self.pending_gotos.push((block_idx, name.clone()));
            }
        }

        let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        if let Some(block_idx) = self.current_block {
            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);
        }
        self.current_block = Some(dead_block);
    }

    /// Walk a HIR `~Label:` statement.
    ///
    /// In addition to creating the label vertex and the post-label
    /// `BasicBlock`, the walker now also (a) registers the label in
    /// `label_table` so subsequent backward jumps resolve through
    /// [`Self::walk_goto_statement_hir`], and (b) drains
    /// `pending_gotos` for any forward jumps that named this label,
    /// patching them with the live `Direct` edge they were waiting for.
    fn walk_label_statement_hir(&mut self, _stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::LabelVertex;

        if let Stmt::Label(name) = body.stmt_idx(_stmt_id) {
            let label_vertex =
                self.cfg.add_vertex(CfgVertex::Label(LabelVertex::new(name.clone())));

            // Connect current block to label
            if let Some(current) = self.current_block {
                let _ = self.cfg.add_edge(current, label_vertex, CfgEdgeType::Direct);
            }

            // Patch forward jumps that named this label.
            let pending = std::mem::take(&mut self.pending_gotos);
            let (matched, leftover): (Vec<_>, Vec<_>) =
                pending.into_iter().partition(|(_, n)| n == name);
            for (source, _) in matched {
                let _ = self.cfg.add_edge(source, label_vertex, CfgEdgeType::Direct);
            }
            self.pending_gotos = leftover;

            // Register for backward jumps.
            self.label_table.insert(name.clone(), label_vertex);

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
    /// 3. Walk all branches through PreprocIfStmt::branches()
    /// 4. For each #ИначеЕсли: create another PreprocConditionVertex, chain it
    /// 5. Walk #Иначе branch if present
    /// 6. Create merge block
    /// 7. Connect all branch exits → merge
    fn walk_preproc_if_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::PreprocConditionVertex;
        use hir_def::hir::HirPreBranchKind;

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

            let mut current_cond = cond_vertex;
            let mut saw_else = false;

            for branch in preproc_if.branches() {
                match branch.kind {
                    HirPreBranchKind::Then => {
                        let then_block =
                            self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
                        let _ = self.cfg.add_edge(cond_vertex, then_block, CfgEdgeType::TrueBranch);
                        self.current_block = Some(then_block);
                    }
                    HirPreBranchKind::ElsIf(_) => {
                        // Create elsif preprocessor conditional
                        let elsif_cond = self.cfg.add_vertex(CfgVertex::PreprocCondition(
                            PreprocConditionVertex::with_directive_range(
                                branch.condition_range.expect("elsif branch has condition range"),
                                branch.directive_range.expect("elsif branch has directive range"),
                            ),
                        ));

                        // Connect previous cond FALSE → this elsif cond
                        let _ =
                            self.cfg.add_edge(current_cond, elsif_cond, CfgEdgeType::FalseBranch);

                        // Walk elsif body
                        let elsif_block =
                            self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
                        let _ = self.cfg.add_edge(elsif_cond, elsif_block, CfgEdgeType::TrueBranch);
                        self.current_block = Some(elsif_block);

                        current_cond = elsif_cond;
                    }
                    HirPreBranchKind::Else => {
                        // Create else block
                        let else_block =
                            self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
                        let _ =
                            self.cfg.add_edge(current_cond, else_block, CfgEdgeType::FalseBranch);
                        self.current_block = Some(else_block);
                        saw_else = true;
                    }
                }

                for &branch_stmt_id in branch.stmts.iter() {
                    self.walk_statement_hir(branch_stmt_id, body);
                }

                // Connect branch to merge:
                // - Direct if reachable, AdjacentCode (phantom) if terminated
                let branch_exit = self.current_block;
                if let Some(exit) = branch_exit {
                    if self.block_has_live_incoming(exit) {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                    } else {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::AdjacentCode);
                    }
                }
            }

            if !saw_else {
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

            // After-loop merge block created up-front so `Прервать` inside
            // the body can target it through the `loop_stack` frame.
            let after_loop = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            self.loop_stack.push(LoopFrame { header: loop_vertex, exit: after_loop });

            // Set current block to body
            self.current_block = Some(body_block);

            // Walk loop body (loop_body is Box<[StmtId]> - no searching!)
            for &loop_stmt_id in loop_body.iter() {
                self.walk_statement_hir(loop_stmt_id, body);
            }

            // Pop the loop frame: nested-loop breaks/continues from
            // unrelated outer loops must not target this one.
            self.loop_stack.pop();

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

            // Connect loop → after_loop (false branch - exit loop)
            let _ = self.cfg.add_edge(loop_vertex, after_loop, CfgEdgeType::FalseBranch);

            // Continue with after-loop block
            self.current_block = Some(after_loop);
        }
    }

    /// Walk a HIR for statement (Phase 6.2)
    fn walk_for_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::ForLoopVertex;

        if let Stmt::For { var, from, to, body: loop_body } = body.stmt_idx(stmt_id) {
            // Create for loop vertex - convert typed to opaque
            let loop_vertex = self.cfg.add_vertex(CfgVertex::ForLoop(ForLoopVertex::with_stmt_id(
                BindingId::from_idx(*var),
                ExprId::from_idx(*from),
                ExprId::from_idx(*to),
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

            // After-loop merge block created up-front so `Прервать` inside
            // the body can target it through the `loop_stack` frame.
            let after_loop = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            self.loop_stack.push(LoopFrame { header: loop_vertex, exit: after_loop });

            self.current_block = Some(body_block);

            // Walk loop body (loop_body is Box<[StmtId]> - no searching!)
            for &loop_stmt_id in loop_body.iter() {
                self.walk_statement_hir(loop_stmt_id, body);
            }

            self.loop_stack.pop();

            let body_exit = self.current_block;

            // Add back edge if enabled
            if self.produce_loop_iterations {
                if let Some(exit) = body_exit {
                    if self.is_block_reachable(exit) {
                        let _ = self.cfg.add_edge(exit, loop_vertex, CfgEdgeType::LoopIteration);
                    }
                }
            }

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

            // After-loop merge block created up-front so `Прервать` inside
            // the body can target it through the `loop_stack` frame.
            let after_loop = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            self.loop_stack.push(LoopFrame { header: loop_vertex, exit: after_loop });

            self.current_block = Some(body_block);

            // Walk loop body (loop_body is Box<[StmtId]> - no searching!)
            for &loop_stmt_id in loop_body.iter() {
                self.walk_statement_hir(loop_stmt_id, body);
            }

            self.loop_stack.pop();

            let body_exit = self.current_block;

            // Add back edge if enabled
            if self.produce_loop_iterations {
                if let Some(exit) = body_exit {
                    if self.is_block_reachable(exit) {
                        let _ = self.cfg.add_edge(exit, loop_vertex, CfgEdgeType::LoopIteration);
                    }
                }
            }

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

            // Connect both exits to merge:
            // - Direct if reachable, AdjacentCode (phantom) if terminated
            if let Some(exit) = try_exit {
                if self.block_has_live_incoming(exit) {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::AdjacentCode);
                }
            }
            if let Some(exit) = except_exit {
                if self.block_has_live_incoming(exit) {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::AdjacentCode);
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

    /// Walk the graph for any edge of the given kind and return all
    /// `(source, target, source_vertex_kind, target_vertex_kind)` tuples.
    /// Used by the loop-context tests below to assert wiring without
    /// having to keep node indices around — pattern-matching on vertex
    /// shapes (`WhileLoop`, `Label`, `BasicBlock`) is more robust than
    /// recording specific allocation order.
    fn edges_of_kind(cfg: &ControlFlowGraph, kind: CfgEdgeType) -> Vec<(NodeIndex, NodeIndex)> {
        cfg.graph()
            .edge_indices()
            .filter_map(|e| {
                let (src, dst) = cfg.graph().edge_endpoints(e)?;
                let edge_kind = *cfg.graph().edge_weight(e)?;
                (edge_kind == kind).then_some((src, dst))
            })
            .collect()
    }

    fn vertex_is(
        cfg: &ControlFlowGraph,
        idx: NodeIndex,
        predicate: impl Fn(&CfgVertex) -> bool,
    ) -> bool {
        cfg.vertex(idx).is_some_and(predicate)
    }

    #[test]
    fn break_in_while_wires_live_loop_break_edge_to_after_loop() {
        // Пока Истина Цикл
        //     Прервать;
        // КонецЦикла;
        use hir_def::{Body, Expr, Literal, Stmt};

        let mut body = Body::default();
        let true_lit = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
        let break_stmt = body.stmts_mut().alloc(Stmt::Break);
        let while_stmt = body
            .stmts_mut()
            .alloc(Stmt::While { condition: true_lit, body: vec![break_stmt].into() });
        body.set_body_stmts(vec![while_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

        // The WhileLoop vertex must exist and have exactly one
        // FalseBranch successor — the after-loop merge block. The
        // LoopBreak edge from break-source must land on that same
        // merge block, not on a fresh dead block.
        let while_vertex = cfg
            .graph()
            .node_indices()
            .find(|&idx| vertex_is(&cfg, idx, |v| matches!(v, CfgVertex::WhileLoop(_))))
            .expect("WhileLoop vertex must exist");
        let after_loop = cfg
            .outgoing_edges(while_vertex)
            .find(|(_, e)| **e == CfgEdgeType::FalseBranch)
            .map(|(target, _)| target)
            .expect("WhileLoop must have a FalseBranch successor");

        let breaks = edges_of_kind(&cfg, CfgEdgeType::LoopBreak);
        assert!(!breaks.is_empty(), "LoopBreak edge missing for `Прервать`");
        assert!(
            breaks.iter().any(|(_, dst)| *dst == after_loop),
            "LoopBreak must target the after-loop merge block, got {breaks:?}",
        );
    }

    #[test]
    fn continue_in_while_wires_live_loop_continue_edge_to_header() {
        // Пока Истина Цикл
        //     Продолжить;
        // КонецЦикла;
        use hir_def::{Body, Expr, Literal, Stmt};

        let mut body = Body::default();
        let true_lit = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
        let cont_stmt = body.stmts_mut().alloc(Stmt::Continue);
        let while_stmt = body
            .stmts_mut()
            .alloc(Stmt::While { condition: true_lit, body: vec![cont_stmt].into() });
        body.set_body_stmts(vec![while_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

        let while_vertex = cfg
            .graph()
            .node_indices()
            .find(|&idx| vertex_is(&cfg, idx, |v| matches!(v, CfgVertex::WhileLoop(_))))
            .expect("WhileLoop vertex must exist");

        let continues = edges_of_kind(&cfg, CfgEdgeType::LoopContinue);
        assert!(!continues.is_empty(), "LoopContinue edge missing for `Продолжить`");
        assert!(
            continues.iter().any(|(_, dst)| *dst == while_vertex),
            "LoopContinue must target the loop header, got {continues:?}",
        );
    }

    #[test]
    fn break_outside_loop_emits_no_loop_break_edge() {
        // Прервать; — at module level. Parser/lowering territory; the
        // CFG must NOT fabricate a LoopBreak edge that targets nothing.
        use hir_def::{Body, Stmt};

        let mut body = Body::default();
        let break_stmt = body.stmts_mut().alloc(Stmt::Break);
        body.set_body_stmts(vec![break_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

        assert!(
            edges_of_kind(&cfg, CfgEdgeType::LoopBreak).is_empty(),
            "Bare `Прервать` outside a loop must not produce a LoopBreak edge",
        );
    }

    #[test]
    fn goto_backward_resolves_to_existing_label() {
        // ~М: Перейти ~М;
        use hir_def::{Body, Name, Stmt};

        let mut body = Body::default();
        let label = body.stmts_mut().alloc(Stmt::Label(Name::new("М")));
        let goto = body.stmts_mut().alloc(Stmt::Goto(Name::new("М")));
        body.set_body_stmts(vec![label, goto].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

        let label_vertex = cfg
            .graph()
            .node_indices()
            .find(|&idx| vertex_is(&cfg, idx, |v| matches!(v, CfgVertex::Label(_))))
            .expect("Label vertex must exist");
        let direct: Vec<_> = edges_of_kind(&cfg, CfgEdgeType::Direct)
            .into_iter()
            .filter(|(_, dst)| *dst == label_vertex)
            .collect();
        // Two Direct edges into the label vertex: one from the entry
        // block (sequential fall-in) and one from the goto-source.
        assert!(
            direct.len() >= 2,
            "Backward `Перейти` must add a Direct edge to the existing label, got {direct:?}",
        );
    }

    #[test]
    fn goto_forward_resolves_when_label_arrives() {
        // Перейти ~М; ~М:
        use hir_def::{Body, Name, Stmt};

        let mut body = Body::default();
        let goto = body.stmts_mut().alloc(Stmt::Goto(Name::new("М")));
        let label = body.stmts_mut().alloc(Stmt::Label(Name::new("М")));
        body.set_body_stmts(vec![goto, label].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

        let label_vertex = cfg
            .graph()
            .node_indices()
            .find(|&idx| vertex_is(&cfg, idx, |v| matches!(v, CfgVertex::Label(_))))
            .expect("Label vertex must exist");
        let direct_into_label: Vec<_> = edges_of_kind(&cfg, CfgEdgeType::Direct)
            .into_iter()
            .filter(|(_, dst)| *dst == label_vertex)
            .collect();
        assert!(
            !direct_into_label.is_empty(),
            "Forward `Перейти` must be patched with a Direct edge once the label arrives",
        );
    }

    #[test]
    fn unresolved_goto_leaves_no_live_edge_to_label() {
        // Перейти ~Нет;  — no matching label.
        use hir_def::{Body, Name, Stmt};

        let mut body = Body::default();
        let goto = body.stmts_mut().alloc(Stmt::Goto(Name::new("Нет")));
        body.set_body_stmts(vec![goto].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

        // No Label vertex was ever created — ergo no Direct edge can
        // target one. The goto block keeps only its dead-fallthrough
        // `AdjacentCode` successor; Track 6 owns the
        // `UnresolvedLabel` diagnostic.
        let label_vertex_exists = cfg
            .graph()
            .node_indices()
            .any(|idx| vertex_is(&cfg, idx, |v| matches!(v, CfgVertex::Label(_))));
        assert!(!label_vertex_exists, "Unresolved goto must NOT fabricate a Label vertex");
    }
}
