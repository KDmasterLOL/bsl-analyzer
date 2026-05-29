use crate::edge::CfgEdgeType;
use crate::graph::ControlFlowGraph;
use crate::vertex::{BasicBlockVertex, CfgVertex};
use cfg_types::{BindingId, ExprId, IdConversion, StmtId};
use hir_def::hir::StmtIdx;
use hir_def::{Body, BodySourceMap, Name, Stmt};
use petgraph::graph::NodeIndex;
use rustc_hash::FxHashMap;

struct LoopFrame {
    header: NodeIndex,
    exit: NodeIndex,
}

pub struct CfgBuilder {
    cfg: ControlFlowGraph,
    current_block: Option<NodeIndex>,

    produce_loop_iterations: bool,

    except_stack: Vec<NodeIndex>,

    loop_stack: Vec<LoopFrame>,

    label_table: FxHashMap<Name, NodeIndex>,

    pending_gotos: Vec<(NodeIndex, Name)>,
}

impl CfgBuilder {
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

    pub fn produce_loop_iterations(&mut self, value: bool) {
        self.produce_loop_iterations = value;
    }

    pub fn build_graph_from_hir(
        mut self,
        body_stmts: &[StmtIdx],
        body: &Body,
        _source_map: Option<&BodySourceMap>,
    ) -> ControlFlowGraph {
        let entry = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        self.cfg.set_entry_point(entry);
        self.current_block = Some(entry);

        for &stmt_id in body_stmts {
            self.walk_statement_hir(stmt_id, body);
        }

        if let Some(block_idx) = self.current_block {
            let exit = self.cfg.exit_point();
            let ends_with_terminator =
                if let Some(CfgVertex::BasicBlock(bb)) = self.cfg.vertex(block_idx) {
                    bb.statements().last().is_some_and(|&stmt_id| {
                        matches!(body.stmt(stmt_id), Stmt::Return { .. } | Stmt::Raise { .. })
                    })
                } else {
                    false
                };

            if !ends_with_terminator {
                if self.block_has_live_incoming(block_idx) {
                    let _ = self.cfg.add_edge(block_idx, exit, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(block_idx, exit, CfgEdgeType::AdjacentCode);
                }
            }
        }

        self.cfg
    }

    fn walk_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        let stmt = body.stmt_idx(stmt_id);
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
                self.add_to_current_block_hir(stmt_id);
            }
        }
    }

    fn add_to_current_block_hir(&mut self, stmt_id: StmtIdx) {
        if let Some(block_idx) = self.current_block {
            if let Some(CfgVertex::BasicBlock(block)) = self.cfg.vertex_mut(block_idx) {
                block.add_statement(StmtId::from_idx(stmt_id));
            }
        }
    }

    fn walk_return_statement_hir(&mut self, stmt_id: StmtIdx, _body: &Body) {
        self.add_to_current_block_hir(stmt_id);

        if let Some(block_idx) = self.current_block {
            let exit = self.cfg.exit_point();

            let _ = self.cfg.add_edge(block_idx, exit, CfgEdgeType::Direct);

            let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);

            self.current_block = Some(dead_block);
        }
    }

    fn walk_raise_statement_hir(&mut self, stmt_id: StmtIdx, _body: &Body) {
        self.add_to_current_block_hir(stmt_id);

        if let Some(block_idx) = self.current_block {
            let target = if let Some(&except_block) = self.except_stack.last() {
                except_block
            } else {
                self.cfg.exit_point()
            };

            let _ = self.cfg.add_edge(block_idx, target, CfgEdgeType::Direct);

            let dead_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            let _ = self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::AdjacentCode);

            self.current_block = Some(dead_block);
        }
    }

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

    fn walk_label_statement_hir(&mut self, _stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::LabelVertex;

        if let Stmt::Label(name) = body.stmt_idx(_stmt_id) {
            let label_vertex =
                self.cfg.add_vertex(CfgVertex::Label(LabelVertex::new(name.clone())));

            if let Some(current) = self.current_block {
                let _ = self.cfg.add_edge(current, label_vertex, CfgEdgeType::Direct);
            }

            let pending = std::mem::take(&mut self.pending_gotos);
            let (matched, leftover): (Vec<_>, Vec<_>) =
                pending.into_iter().partition(|(_, n)| n == name);
            for (source, _) in matched {
                let _ = self.cfg.add_edge(source, label_vertex, CfgEdgeType::Direct);
            }
            self.pending_gotos = leftover;

            self.label_table.insert(name.clone(), label_vertex);

            let after_label = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(label_vertex, after_label, CfgEdgeType::Direct);

            self.current_block = Some(after_label);
        }
    }

    fn is_block_reachable(&self, block: NodeIndex) -> bool {
        let has_incoming = self.cfg.incoming_edges(block).next().is_some();
        let is_entry = self.cfg.entry_point() == Some(block);
        has_incoming || is_entry
    }

    fn block_has_live_incoming(&self, block: NodeIndex) -> bool {
        if self.cfg.entry_point() == Some(block) {
            return true;
        }
        self.cfg.incoming_edges(block).any(|(_, edge_type)| !edge_type.is_dead_code_edge())
    }

    fn walk_if_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::ConditionalVertex;

        if let Stmt::If(if_stmt) = body.stmt_idx(stmt_id) {
            let cond_vertex = self.cfg.add_vertex(CfgVertex::Conditional(ConditionalVertex::new(
                ExprId::from_idx(if_stmt.condition),
            )));

            if let Some(current) = self.current_block {
                let _ = self.cfg.add_edge(current, cond_vertex, CfgEdgeType::Direct);
            }

            let merge_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            let then_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(cond_vertex, then_block, CfgEdgeType::TrueBranch);
            self.current_block = Some(then_block);

            for &then_stmt_id in if_stmt.then_branch.iter() {
                self.walk_statement_hir(then_stmt_id, body);
            }

            let then_exit = self.current_block;
            if let Some(exit) = then_exit {
                if self.block_has_live_incoming(exit) {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::AdjacentCode);
                }
            }

            let mut current_cond = cond_vertex;

            for (elsif_condition, elsif_body) in if_stmt.elsif_branches.iter() {
                let elsif_cond = self.cfg.add_vertex(CfgVertex::Conditional(
                    ConditionalVertex::new(ExprId::from_idx(*elsif_condition)),
                ));

                let _ = self.cfg.add_edge(current_cond, elsif_cond, CfgEdgeType::FalseBranch);

                let elsif_block =
                    self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
                let _ = self.cfg.add_edge(elsif_cond, elsif_block, CfgEdgeType::TrueBranch);
                self.current_block = Some(elsif_block);

                for &elsif_stmt_id in elsif_body.iter() {
                    self.walk_statement_hir(elsif_stmt_id, body);
                }

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

            if let Some(ref else_stmts) = if_stmt.else_branch {
                let else_block =
                    self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
                let _ = self.cfg.add_edge(current_cond, else_block, CfgEdgeType::FalseBranch);
                self.current_block = Some(else_block);

                for &else_stmt_id in else_stmts.iter() {
                    self.walk_statement_hir(else_stmt_id, body);
                }

                let else_exit = self.current_block;
                if let Some(exit) = else_exit {
                    if self.block_has_live_incoming(exit) {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::Direct);
                    } else {
                        let _ = self.cfg.add_edge(exit, merge_block, CfgEdgeType::AdjacentCode);
                    }
                }
            } else {
                let _ = self.cfg.add_edge(current_cond, merge_block, CfgEdgeType::FalseBranch);
            }

            self.current_block = Some(merge_block);
        }
    }

    fn walk_preproc_if_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::PreprocConditionVertex;
        use hir_def::hir::HirPreBranchKind;

        if let Stmt::PreprocIf(preproc_if) = body.stmt_idx(stmt_id) {
            let cond_vertex = self.cfg.add_vertex(CfgVertex::PreprocCondition(
                PreprocConditionVertex::with_ranges(
                    preproc_if.condition_range,
                    preproc_if.directive_range,
                    preproc_if.full_range,
                ),
            ));

            if let Some(current) = self.current_block {
                let _ = self.cfg.add_edge(current, cond_vertex, CfgEdgeType::Direct);
            }

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
                        let elsif_cond = self.cfg.add_vertex(CfgVertex::PreprocCondition(
                            PreprocConditionVertex::with_directive_range(
                                branch.condition_range.expect("elsif branch has condition range"),
                                branch.directive_range.expect("elsif branch has directive range"),
                            ),
                        ));

                        let _ =
                            self.cfg.add_edge(current_cond, elsif_cond, CfgEdgeType::FalseBranch);

                        let elsif_block =
                            self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
                        let _ = self.cfg.add_edge(elsif_cond, elsif_block, CfgEdgeType::TrueBranch);
                        self.current_block = Some(elsif_block);

                        current_cond = elsif_cond;
                    }
                    HirPreBranchKind::Else => {
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
                let _ = self.cfg.add_edge(current_cond, merge_block, CfgEdgeType::FalseBranch);
            }

            self.current_block = Some(merge_block);
        }
    }

    fn walk_while_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::WhileLoopVertex;

        if let Stmt::While { condition, body: loop_body } = body.stmt_idx(stmt_id) {
            let loop_vertex = self.cfg.add_vertex(CfgVertex::WhileLoop(WhileLoopVertex::new(
                ExprId::from_idx(*condition),
            )));

            if let Some(current) = self.current_block {
                if self.block_has_live_incoming(current) {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::AdjacentCode);
                }
            }

            let body_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            let _ = self.cfg.add_edge(loop_vertex, body_block, CfgEdgeType::TrueBranch);

            let after_loop = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            self.loop_stack.push(LoopFrame { header: loop_vertex, exit: after_loop });

            self.current_block = Some(body_block);

            for &loop_stmt_id in loop_body.iter() {
                self.walk_statement_hir(loop_stmt_id, body);
            }

            self.loop_stack.pop();

            let body_exit = self.current_block;

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

    fn walk_for_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::ForLoopVertex;

        if let Stmt::For { var, from, to, body: loop_body } = body.stmt_idx(stmt_id) {
            let loop_vertex = self.cfg.add_vertex(CfgVertex::ForLoop(ForLoopVertex::with_stmt_id(
                BindingId::from_idx(*var),
                ExprId::from_idx(*from),
                ExprId::from_idx(*to),
                StmtId::from_idx(stmt_id),
            )));

            if let Some(current) = self.current_block {
                if self.block_has_live_incoming(current) {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::AdjacentCode);
                }
            }

            let body_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            let _ = self.cfg.add_edge(loop_vertex, body_block, CfgEdgeType::TrueBranch);

            let after_loop = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            self.loop_stack.push(LoopFrame { header: loop_vertex, exit: after_loop });

            self.current_block = Some(body_block);

            for &loop_stmt_id in loop_body.iter() {
                self.walk_statement_hir(loop_stmt_id, body);
            }

            self.loop_stack.pop();

            let body_exit = self.current_block;

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

    fn walk_foreach_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::ForEachLoopVertex;

        if let Stmt::ForEach { var, collection, body: loop_body } = body.stmt_idx(stmt_id) {
            let loop_vertex =
                self.cfg.add_vertex(CfgVertex::ForEachLoop(ForEachLoopVertex::with_stmt_id(
                    BindingId::from_idx(*var),
                    ExprId::from_idx(*collection),
                    StmtId::from_idx(stmt_id),
                )));

            if let Some(current) = self.current_block {
                if self.block_has_live_incoming(current) {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::Direct);
                } else {
                    let _ = self.cfg.add_edge(current, loop_vertex, CfgEdgeType::AdjacentCode);
                }
            }

            let body_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

            let _ = self.cfg.add_edge(loop_vertex, body_block, CfgEdgeType::TrueBranch);

            let after_loop = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            self.loop_stack.push(LoopFrame { header: loop_vertex, exit: after_loop });

            self.current_block = Some(body_block);

            for &loop_stmt_id in loop_body.iter() {
                self.walk_statement_hir(loop_stmt_id, body);
            }

            self.loop_stack.pop();

            let body_exit = self.current_block;

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

    fn walk_try_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use crate::vertex::TryExceptVertex;

        if let Stmt::Try { body: try_body, except } = body.stmt_idx(stmt_id) {
            let try_vertex = self.cfg.add_vertex(CfgVertex::TryExcept(TryExceptVertex::new()));

            if let Some(current) = self.current_block {
                let _ = self.cfg.add_edge(current, try_vertex, CfgEdgeType::Direct);
            }

            let try_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(try_vertex, try_block, CfgEdgeType::TrueBranch);

            let except_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let _ = self.cfg.add_edge(try_vertex, except_block, CfgEdgeType::FalseBranch);

            self.except_stack.push(except_block);

            self.current_block = Some(try_block);
            for &try_stmt_id in try_body.iter() {
                self.walk_statement_hir(try_stmt_id, body);
            }

            let try_exit = self.current_block;

            self.except_stack.pop();

            self.current_block = Some(except_block);
            for &except_stmt_id in except.iter() {
                self.walk_statement_hir(except_stmt_id, body);
            }

            let except_exit = self.current_block;

            let merge_block = self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));

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

        let mut body = Body::default();

        let var_a = body.bindings_mut().alloc(Binding::var(hir_def::Name::new("А")));

        let lit_42 =
            body.exprs_mut().alloc(Expr::Literal(Literal::Number(NotNan::new(42.0).unwrap())));
        let path_a = body.exprs_mut().alloc(Expr::Path(hir_def::Name::new("А")));

        let var_decl = body.stmts_mut().alloc(Stmt::VarDecl { bindings: vec![var_a].into() });
        let assign = body.stmts_mut().alloc(Stmt::Assign { target: path_a, value: lit_42 });
        let return_stmt = body.stmts_mut().alloc(Stmt::Return { value: Some(path_a) });

        body.set_body_stmts(vec![var_decl, assign, return_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

        assert!(cfg.entry_point().is_some(), "CFG should have entry point");
        assert!(cfg.exit_point() != cfg.entry_point().unwrap(), "Exit should differ from entry");

        let vertex_count = cfg.graph().node_count();
        assert!(
            vertex_count >= 2,
            "Should have at least entry and exit vertices, got {}",
            vertex_count
        );

        let exit = cfg.exit_point();
        let incoming_to_exit: Vec<_> = cfg.incoming_edges(exit).collect();
        assert!(!incoming_to_exit.is_empty(), "Exit should have incoming edges");
    }

    #[test]
    fn test_hir_based_cfg_if_statement() {
        use hir_def::{Body, Expr, Literal, Stmt};
        use ordered_float::NotNan;

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

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

        let vertex_count = cfg.graph().node_count();
        assert!(
            vertex_count >= 5,
            "If-else CFG should have multiple vertices, got {}",
            vertex_count
        );

        let has_conditional = cfg.graph().node_indices().any(|idx| {
            if let Some(vertex) = cfg.vertex(idx) {
                matches!(vertex, CfgVertex::Conditional(_))
            } else {
                false
            }
        });
        assert!(has_conditional, "CFG should contain conditional vertex for if statement");
    }

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
        use hir_def::{Body, Expr, Literal, Stmt};

        let mut body = Body::default();
        let true_lit = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
        let break_stmt = body.stmts_mut().alloc(Stmt::Break);
        let while_stmt = body
            .stmts_mut()
            .alloc(Stmt::While { condition: true_lit, body: vec![break_stmt].into() });
        body.set_body_stmts(vec![while_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

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
        assert!(
            direct.len() >= 2,
            "Backward `Перейти` must add a Direct edge to the existing label, got {direct:?}",
        );
    }

    #[test]
    fn goto_forward_resolves_when_label_arrives() {
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
        use hir_def::{Body, Name, Stmt};

        let mut body = Body::default();
        let goto = body.stmts_mut().alloc(Stmt::Goto(Name::new("Нет")));
        body.set_body_stmts(vec![goto].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body, None);

        let label_vertex_exists = cfg
            .graph()
            .node_indices()
            .any(|idx| vertex_is(&cfg, idx, |v| matches!(v, CfgVertex::Label(_))));
        assert!(!label_vertex_exists, "Unresolved goto must NOT fabricate a Label vertex");
    }
}
