use cfg::{CfgBuilder, ControlFlowGraph};
use expect_test::{expect, Expect};
use hir_def::{Body, Expr, Literal, Stmt};

fn snapshot(cfg: ControlFlowGraph, expect: Expect) {
    expect.assert_eq(&cfg::test_utils::format_cfg(&cfg));
}

fn build(body: &Body) -> ControlFlowGraph {
    CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), body)
}

#[test]
fn try_except_fallthrough_merges_try_and_except_exits() {
    let mut body = Body::default();
    let value = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
    let try_stmt_body = body.stmts_mut().alloc(Stmt::Expr(value));
    let except_stmt = body.stmts_mut().alloc(Stmt::Expr(value));
    let after_try = body.stmts_mut().alloc(Stmt::Expr(value));
    let try_stmt = body
        .stmts_mut()
        .alloc(Stmt::Try { body: vec![try_stmt_body].into(), except: vec![except_stmt].into() });
    body.set_body_stmts(vec![try_stmt, after_try].into());

    snapshot(
        build(&body),
        expect![[r#"
            blocks:
              ENTRY:EMPTY:0
              EXIT:EMPTY:3
              NORMAL:CALL_STMT:2:#6797164c
              NORMAL:CALL_STMT:2:#bf26ac9d
              NORMAL:CALL_STMT:2:#ddbea5ab
              NORMAL:TRY_STMT:1
            edges:
              ENTRY:EMPTY:0 -> NORMAL:TRY_STMT:1 [Direct]
              NORMAL:CALL_STMT:2:#6797164c -> NORMAL:CALL_STMT:2:#bf26ac9d [Direct]
              NORMAL:CALL_STMT:2:#bf26ac9d -> EXIT:EMPTY:3 [Direct]
              NORMAL:CALL_STMT:2:#ddbea5ab -> NORMAL:CALL_STMT:2:#bf26ac9d [Direct]
              NORMAL:TRY_STMT:1 -> NORMAL:CALL_STMT:2:#6797164c [FalseBranch]
              NORMAL:TRY_STMT:1 -> NORMAL:CALL_STMT:2:#ddbea5ab [TrueBranch]
        "#]],
    );
}

#[test]
fn nested_try_routes_raise_to_nearest_except() {
    let mut body = Body::default();
    let value = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
    let inner_raise = body.stmts_mut().alloc(Stmt::Raise { value: None });
    let inner_dead_stmt = body.stmts_mut().alloc(Stmt::Expr(value));
    let inner_except_stmt = body.stmts_mut().alloc(Stmt::Expr(value));
    let inner_try = body.stmts_mut().alloc(Stmt::Try {
        body: vec![inner_raise, inner_dead_stmt].into(),
        except: vec![inner_except_stmt].into(),
    });
    let outer_raise = body.stmts_mut().alloc(Stmt::Raise { value: None });
    let outer_except_stmt = body.stmts_mut().alloc(Stmt::Expr(value));
    let outer_try = body.stmts_mut().alloc(Stmt::Try {
        body: vec![inner_try, outer_raise].into(),
        except: vec![outer_except_stmt].into(),
    });
    body.set_body_stmts(vec![outer_try].into());

    snapshot(
        build(&body),
        expect![[r#"
            blocks:
              ENTRY:EMPTY:0
              EXIT:EMPTY:3
              NORMAL:CALL_STMT:2
              NORMAL:CALL_STMT:4
              NORMAL:EMPTY:2:#8cbe2a90
              NORMAL:EMPTY:2:#ddbea5ab
              NORMAL:EMPTY:5
              NORMAL:RAISE_STMT:4:#25f21145
              NORMAL:RAISE_STMT:4:#c06e657a
              NORMAL:RAISE_STMT:5
              NORMAL:TRY_STMT:1
              NORMAL:TRY_STMT:3
            edges:
              ENTRY:EMPTY:0 -> NORMAL:TRY_STMT:1 [Direct]
              NORMAL:CALL_STMT:2 -> NORMAL:EMPTY:2:#8cbe2a90 [Direct]
              NORMAL:CALL_STMT:4 -> NORMAL:RAISE_STMT:4:#c06e657a [Direct]
              NORMAL:EMPTY:2:#8cbe2a90 -> EXIT:EMPTY:3 [Direct]
              NORMAL:EMPTY:2:#ddbea5ab -> NORMAL:TRY_STMT:3 [Direct]
              NORMAL:EMPTY:5 -> NORMAL:EMPTY:2:#8cbe2a90 [AdjacentCode]
              NORMAL:RAISE_STMT:4:#25f21145 -> NORMAL:CALL_STMT:4 [Direct]
              NORMAL:RAISE_STMT:4:#25f21145 -> NORMAL:RAISE_STMT:5 [AdjacentCode]
              NORMAL:RAISE_STMT:4:#c06e657a -> NORMAL:CALL_STMT:2 [Direct]
              NORMAL:RAISE_STMT:4:#c06e657a -> NORMAL:EMPTY:5 [AdjacentCode]
              NORMAL:RAISE_STMT:5 -> NORMAL:RAISE_STMT:4:#c06e657a [AdjacentCode]
              NORMAL:TRY_STMT:1 -> NORMAL:CALL_STMT:2 [FalseBranch]
              NORMAL:TRY_STMT:1 -> NORMAL:EMPTY:2:#ddbea5ab [TrueBranch]
              NORMAL:TRY_STMT:3 -> NORMAL:CALL_STMT:4 [FalseBranch]
              NORMAL:TRY_STMT:3 -> NORMAL:RAISE_STMT:4:#25f21145 [TrueBranch]
        "#]],
    );
}
