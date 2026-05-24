//! Integration tests for M4 Task 6.6 — `narrow_query` + `Semantics::type_of_expr`.
//!
//! These tests exercise the Salsa-wired narrowing pipeline end-to-end through
//! `RootDatabaseImpl`. They cover the observable behaviours that Task 6.6
//! is supposed to deliver:
//!
//! 1. `HirDatabase::narrow(file_id, owner)` returns `Some` for a body that
//!    contains a recognised guard shape — the query plumbing is wired.
//! 2. `narrowed_type_at` on a Path inside the then-block returns the narrowed
//!    `Ty` (ADR-01 Q4 narrowed case — True-edge overlay propagates into the
//!    successor BasicBlock).
//! 3. `narrowed_type_at` on the guard's own receiver returns the *pre-narrow*
//!    overlay at the Conditional vertex's IN state (ADR-01 Q4 pre-narrow
//!    case). Note: `narrow`'s state tracks all reaching assignments, not just
//!    guard narrowings, so "pre-narrow" is whatever the reaching-def overlay
//!    holds at that point — which equals the base type if no intervening
//!    assignment changed it.
//! 4. One-sided narrowings drop at merge points (Task 6.4 intersection join)
//!    when the narrowed variable has no overlay entry on the opposite branch.
//! 5. `narrow_query` is deterministic across calls on the same DB state.
//!
//! `narrow_query` follows the same plain-function pattern as `infer_query`
//! (no `#[salsa::tracked]` attribute); the determinism test checks content
//! equality rather than `Arc::ptr_eq`, which is the strongest invariant we
//! can exercise without changing the caching strategy elsewhere in the crate.

use hir::{
    narrow_query, narrowed_type_at, ty_bridge, DefDatabase, DefWithBodyId, ExprId, IdConversion,
    ModuleId, Name, Semantics, Ty, Type,
};
use ide_db::base_db::{RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxNode, TextRange};
use test_fixture::Fixture;
use vfs::FileId;

fn setup(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = vfs::FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, SourceRootId(0));
        db.set_file_text(*file_id, &file.content);
    }
    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    (db, test_file)
}

/// Pick the single method body in the fixture as `(owner, method_local_id)`.
///
/// All fixtures in this file declare exactly one `Процедура`, so we can skip
/// the disambiguation that `Semantics::type_of_expr` performs via `BodySourceMap`.
fn first_method_owner(db: &RootDatabaseImpl, file_id: FileId) -> DefWithBodyId {
    let module_bodies = db.module_bodies(ModuleId::new(file_id));
    let (local_id, _body, _source_map) =
        module_bodies.method_bodies().next().expect("fixture declares a method body");
    DefWithBodyId::Method(local_id)
}

/// Look up the `ExprId` that `BodySourceMap` associates with the given
/// text range in the (single) method body of the test fixture.
fn expr_id_at_range(db: &RootDatabaseImpl, file_id: FileId, range: TextRange) -> ExprId {
    let module_bodies = db.module_bodies(ModuleId::new(file_id));
    let found = module_bodies
        .method_bodies()
        .find_map(|(_local_id, _body, source_map)| source_map.expr_at_range(range));
    found.unwrap_or_else(|| panic!("BodySourceMap has no expression at range {:?}", range))
}

/// Pick the N-th *distinct-range* occurrence of an identifier, in source
/// order.
///
/// BSL's parser wraps every expression in a synthetic `EXPR` node
/// (`NodeKind::Expr → SK::EXPR`), and the Pratt-style precedence climb
/// can leave several EXPR wrappers covering the *same* text range for a
/// single identifier — e.g., a bare `Х` argument ends up with
/// `EXPR(EXPR(EXPR(IDENT(Х))))` under an `ARG_LIST`. All those wrappers
/// share a text range, and `BodySourceMap::expr_at_range` keys on exactly
/// that range, so for testing purposes we care about *distinct positions*
/// in the source — not distinct nodes. The `HashSet<TextRange>` dedupes
/// the nested wrappers, leaving one representative per position.
fn nth_ident_expr_at_distinct_position(root: &SyntaxNode, ident: &str, nth: usize) -> SyntaxNode {
    let mut seen: HashSet<TextRange> = HashSet::new();
    root.descendants()
        .filter(|n| n.kind() == SyntaxKind::EXPR && n.text() == ident)
        .filter(|n| seen.insert(n.text_range()))
        .nth(nth)
        .unwrap_or_else(|| {
            panic!("fixture missing EXPR(IDENT({ident})) at distinct position index {nth}")
        })
}

// -----------------------------------------------------------------------------
// Test 1: Salsa plumbing — narrow_query returns Some for a well-formed body.
// -----------------------------------------------------------------------------

#[test]
fn narrow_query_returns_some_for_body_with_guard() {
    // A body with a recognised `ТипЗнч(Х) = Тип("Строка")` guard must
    // cause `narrow_query` to converge and yield a non-empty CFG. If this
    // fails, either the query wiring is broken or the solver didn't
    // converge — both are regressions.
    let fixture = r#"
//- /test.bsl
Процедура П()
    Х = 42;
    Если ТипЗнч(Х) = Тип("Строка") Тогда
        А = Х;
    КонецЕсли;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let owner = first_method_owner(&db, file_id);

    let result = narrow_query(&db, file_id, owner).expect("narrow_query must converge");
    assert!(
        result.cfg().vertices().count() > 0,
        "CFG built for a non-empty body must have at least one vertex"
    );
}

// -----------------------------------------------------------------------------
// Test 2: narrowed_type_at on the then-body Path returns the narrowed Ty.
// -----------------------------------------------------------------------------

#[test]
fn narrowed_type_at_then_body_returns_narrowed_ty() {
    // The `Х` on the RHS of `А = Х;` inside the Тогда-branch lives in a
    // successor BasicBlock whose IN carries the True-edge overlay
    // (`Х → Ty::String`). This is the ADR-01 Q4 narrowed case.
    let fixture = r#"
//- /test.bsl
Процедура П()
    Х = 42;
    Если ТипЗнч(Х) = Тип("Строка") Тогда
        А = Х;
    КонецЕсли;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let owner = first_method_owner(&db, file_id);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    // Distinct `Х` positions in source: 0 = guard argument (ТипЗнч(Х)),
    // 1 = then-body RHS (А = Х). The LHS of `Х = 42` is a bare IDENT
    // token without an EXPR wrapper, so it does not count here.
    let then_rhs = nth_ident_expr_at_distinct_position(&root, "Х", 1);
    let expr_id = expr_id_at_range(&db, file_id, then_rhs.text_range());

    let result = narrow_query(&db, file_id, owner).expect("narrow_query must converge");
    assert_eq!(
        narrowed_type_at(&db, &result, expr_id.to_idx(), &Name::new("Х"))
            .map(|id| ty_bridge::typeid_to_ty(&db, id)),
        Some(Ty::String),
        "then-body `Х` must observe the narrowed Ty::String (True-edge overlay)"
    );

    // Semantics::type_of_expr must match — the merge picks the narrowed
    // overlay entry over the inferred base type when it is non-Unknown.
    let sema = Semantics::new(&db);
    assert_eq!(
        ty_bridge::typeid_to_ty(&db, sema.type_of_expr(file_id, &then_rhs)),
        Ty::String,
        "Semantics::type_of_expr merges the narrowed overlay onto the base"
    );
}

// -----------------------------------------------------------------------------
// Test 3: guard receiver — narrowed_type_at returns the pre-narrow reaching type.
// -----------------------------------------------------------------------------

#[test]
fn narrowed_type_at_guard_receiver_returns_pre_narrow_reaching_ty() {
    // The `Х` *inside* `ТипЗнч(Х) = Тип("Строка")` is the guard's own
    // receiver. By ADR-01 Q4 and Task 6.5 placement, its containing CFG
    // vertex is the Conditional itself — whose IN state has **not** yet
    // seen the pending-guard applied (applications happen on outgoing
    // True/False edges in `transfer_edge`). So the overlay holds the
    // reaching-def state *before* the guard fires: after `Х = 42`, that
    // reaching type is `Ty::Number`.
    //
    // If a future refactor accidentally applies the guard too early
    // (e.g., inside `transfer_block` instead of `transfer_edge`), this
    // test fails with `Some(Ty::String)` — the tell-tale sign of the
    // Task 6.5 invariant being broken.
    let fixture = r#"
//- /test.bsl
Процедура П()
    Х = 42;
    Если ТипЗнч(Х) = Тип("Строка") Тогда
        А = Х;
    КонецЕсли;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let owner = first_method_owner(&db, file_id);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    let receiver = nth_ident_expr_at_distinct_position(&root, "Х", 0); // guard arg
    let expr_id = expr_id_at_range(&db, file_id, receiver.text_range());

    let result = narrow_query(&db, file_id, owner).expect("narrow_query must converge");
    assert_eq!(
        narrowed_type_at(&db, &result, expr_id.to_idx(), &Name::new("Х"))
            .map(|id| ty_bridge::typeid_to_ty(&db, id)),
        Some(Ty::Number),
        "guard-receiver `Х` observes the pre-narrow reaching type (Number from `Х = 42`), \
         not the narrowed one (String)"
    );

    let sema = Semantics::new(&db);
    assert_eq!(
        ty_bridge::typeid_to_ty(&db, sema.type_of_expr(file_id, &receiver)),
        Ty::Number,
        "hover on guard receiver returns the pre-narrow Number"
    );
}

// -----------------------------------------------------------------------------
// Test 4: one-sided narrowing drops at merge — parameter case.
// -----------------------------------------------------------------------------

#[test]
fn narrowed_type_at_after_one_sided_if_on_parameter_drops() {
    // Task 6.4 intersection-join: overlay entries that live in only one
    // predecessor are dropped at the merge point. Here `Х` is a parameter
    // with no local assignment, so the Conditional IN state has no entry
    // for it. The True edge applies the guard and writes `Х → String`
    // into the then-branch overlay; the False edge tries
    // `ty_difference(base, String)` but cannot refine a non-Union base
    // (`None` base degrades to `Ty::Unknown`, which `insert_if_informative`
    // skips), so the False side carries no entry either. At
    // `КонецЕсли` the join intersects { Х: String } with {} → the entry
    // is dropped, and `narrowed_type_at` returns `None`. The
    // `narrow_or_base` merge in `Semantics::type_of_expr` then falls back
    // to the inferred base — `Ty::Unknown` for an untyped parameter.
    let fixture = r#"
//- /test.bsl
Процедура П(Х)
    Если ТипЗнч(Х) = Тип("Строка") Тогда
        А = Х;
    КонецЕсли;
    Б = Х;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let owner = first_method_owner(&db, file_id);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    // Distinct positions: 0 = guard arg, 1 = then-body RHS (А = Х),
    // 2 = post-if RHS (Б = Х). The LHS tokens of assignments are bare
    // IDENTs without an EXPR wrapper, so they don't count.
    let after_if = nth_ident_expr_at_distinct_position(&root, "Х", 2);
    let expr_id = expr_id_at_range(&db, file_id, after_if.text_range());

    let result = narrow_query(&db, file_id, owner).expect("narrow_query must converge");
    assert_eq!(
        narrowed_type_at(&db, &result, expr_id.to_idx(), &Name::new("Х"))
            .map(|id| ty_bridge::typeid_to_ty(&db, id)),
        None,
        "post-КонецЕсли `Х` must drop the one-sided narrowing (entry only in True branch)"
    );
}

// -----------------------------------------------------------------------------
// Test 5: determinism — two narrow_query calls produce content-equal results.
// -----------------------------------------------------------------------------

#[test]
fn narrow_query_is_deterministic() {
    // `narrow_query` follows the same plain-function pattern as
    // `infer_query` (no `#[salsa::tracked]` attribute), so Arc pointer
    // identity is not guaranteed across calls. What must hold is content
    // determinism: given the same database state, two calls produce
    // byte-equal `DataflowResult`s. `DataflowResult<L>: PartialEq`
    // (derived from its block_in / block_out maps), so we can check
    // this structurally.
    let fixture = r#"
//- /test.bsl
Процедура П()
    Х = 42;
    Если ТипЗнч(Х) = Тип("Строка") Тогда
        А = Х;
    КонецЕсли;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let owner = first_method_owner(&db, file_id);

    let r1 = narrow_query(&db, file_id, owner).expect("first call must converge");
    let r2 = narrow_query(&db, file_id, owner).expect("second call must converge");

    assert_eq!(
        *r1, *r2,
        "narrow_query is a pure function of the database state — two calls must agree"
    );
}

// -----------------------------------------------------------------------------
// Test 6: case-insensitivity regression — mixed-case Х / х hit the same overlay.
// -----------------------------------------------------------------------------

#[test]
fn narrow_is_case_insensitive_across_guard_and_hover() {
    // BSL is case-insensitive: `Х` and `х` refer to the same variable.
    // `Name`'s derived `Hash` / `Eq` are case-*sensitive* (it's a
    // `SmolStr` wrapper), so without normalisation, a guard written
    // with a lowercase receiver would stash its overlay under
    // `Name("х")` while the later hover, lowered from an uppercase
    // reference, would miss on `Name("Х")` and fall back to the base
    // type. This would silently violate ADR-01 Q4 ("hover on then-body
    // reference sees the narrowed type") for any author who mixed
    // cases across a guard and its consumers.
    //
    // The fix routes every overlay write and every explicit lookup in
    // `narrow.rs` through `fold_name`, which lowercases before hashing.
    // This regression fires if that normalisation ever slips — either
    // at a new write site that forgets to fold, or at a refactor that
    // moves `NarrowState::get` back to a raw `.get()`.
    let fixture = r#"
//- /test.bsl
Процедура П()
    Х = 42;
    Если ТипЗнч(х) = Тип("Строка") Тогда
        А = Х;
    КонецЕсли;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let owner = first_method_owner(&db, file_id);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    // The then-body `Х` is the second distinct-range occurrence overall
    // (0 = guard receiver `х` — different spelling, so it's in its own
    // bucket by text match; but `EXPR`-text filter picks `Х` positions
    // only, so in this fixture `Х` appears just once as an EXPR node
    // at the then-body RHS).
    let then_rhs = nth_ident_expr_at_distinct_position(&root, "Х", 0);
    let expr_id = expr_id_at_range(&db, file_id, then_rhs.text_range());

    let result = narrow_query(&db, file_id, owner).expect("narrow_query must converge");
    // Lookup under the uppercase `Х` must hit the overlay written by
    // the lowercase-receiver guard. If case-folding regresses this
    // returns `None` and `Semantics::type_of_expr` falls back to base.
    assert_eq!(
        narrowed_type_at(&db, &result, expr_id.to_idx(), &Name::new("Х"))
            .map(|id| ty_bridge::typeid_to_ty(&db, id)),
        Some(Ty::String),
        "mixed-case guard receiver (`х`) must narrow the uppercase reference (`Х`)"
    );

    let sema = Semantics::new(&db);
    assert_eq!(
        ty_bridge::typeid_to_ty(&db, sema.type_of_expr(file_id, &then_rhs)),
        Ty::String,
        "hover under Semantics::type_of_expr must see the case-folded narrowing"
    );
}

// -----------------------------------------------------------------------------
// Test 7: module-level code path — narrow_query resolves DefWithBodyId::ModuleCode.
// -----------------------------------------------------------------------------

#[test]
fn narrow_query_handles_module_code_body() {
    // `narrow_query` has two owner branches: `Method(local_id)` for
    // regular procedures / functions, and `ModuleCode` for statements
    // outside any `Процедура` / `Функция`. The previous tests all ran
    // under `Method(...)` because they declared a `Процедура П(...)`.
    // Without this test, `DefWithBodyId::ModuleCode` would be
    // completely unexercised — and a refactor that dropped the
    // `ModuleCode` branch in `narrow_query`'s owner-match would go
    // unnoticed until an IDE user triggered a hover on a variable in a
    // module with top-level code.
    let fixture = r#"
//- /test.bsl
Х = 42;
Если ТипЗнч(Х) = Тип("Строка") Тогда
    А = Х;
КонецЕсли;
"#;
    let (db, file_id) = setup(fixture);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    let then_rhs = nth_ident_expr_at_distinct_position(&root, "Х", 1); // then-body RHS
    let module_bodies = db.module_bodies(ModuleId::new(file_id));
    let module_code = module_bodies.module_code_result().expect("module-level body lowered");
    let expr_id = module_code
        .source_map
        .expr_at_range(then_rhs.text_range())
        .expect("module-level BodySourceMap must locate the then-body Х");

    let result = narrow_query(&db, file_id, DefWithBodyId::ModuleCode)
        .expect("narrow_query must converge for ModuleCode");
    assert_eq!(
        narrowed_type_at(&db, &result, expr_id.to_idx(), &Name::new("Х"))
            .map(|id| ty_bridge::typeid_to_ty(&db, id)),
        Some(Ty::String),
        "module-code narrowing must reach the then-body Х"
    );

    // End-to-end: Semantics::type_of_expr auto-detects the
    // ModuleCode owner through module_code_result().source_map — so
    // the hover for top-level code must also return the narrowed type.
    let sema = Semantics::new(&db);
    assert_eq!(
        ty_bridge::typeid_to_ty(&db, sema.type_of_expr(file_id, &then_rhs)),
        Ty::String,
        "Semantics::type_of_expr on module-level then-body must merge the ModuleCode narrowing"
    );
}

// -----------------------------------------------------------------------------
// Test 8: else-body Path inherits the Conditional IN state (ADR-01 Q2 edge).
// -----------------------------------------------------------------------------

#[test]
fn narrowed_type_at_else_body_inherits_reaching_when_complement_degrades() {
    // Task 6.3 / ADR-01 Q2 edge case. When the pre-narrow base is a
    // singleton (non-Union) like `Ty::Number`, `ty_difference(Number,
    // String)` cannot refine further and returns `Ty::Unknown`; by the
    // overlay invariant (`insert_if_informative` skips Unknown), the
    // False-edge transfer is a no-op on the overlay. That leaves the
    // else body inheriting the Conditional vertex's IN state verbatim —
    // which in this fixture is `{Х → Number}` (the reaching type from
    // `Х = 42`).
    //
    // This test pins the invariant: `Semantics::type_of_expr` on an
    // else-body reference returns the pre-narrow reaching type when
    // the complement can't narrow further. Without this coverage, a
    // future refactor that clobbers the overlay on non-refinable
    // false-edges (e.g. overwriting with `Ty::Unknown` and thus
    // hiding reaching data under an overlay hole) would go unnoticed
    // at the IDE layer.
    let fixture = r#"
//- /test.bsl
Процедура П()
    Х = 42;
    Если ТипЗнч(Х) = Тип("Строка") Тогда
        А = Х;
    Иначе
        Б = Х;
    КонецЕсли;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let owner = first_method_owner(&db, file_id);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    // Distinct Х positions: 0 = guard arg, 1 = then-body (А = Х),
    // 2 = else-body (Б = Х). LHS tokens are bare IDENTs, not EXPR nodes.
    let else_rhs = nth_ident_expr_at_distinct_position(&root, "Х", 2);
    let expr_id = expr_id_at_range(&db, file_id, else_rhs.text_range());

    let result = narrow_query(&db, file_id, owner).expect("narrow_query must converge");
    assert_eq!(
        narrowed_type_at(&db, &result, expr_id.to_idx(), &Name::new("Х"))
            .map(|id| ty_bridge::typeid_to_ty(&db, id)),
        Some(Ty::Number),
        "else-body Х inherits the Conditional-IN reaching type when the complement is Unknown"
    );

    // End-to-end sema assertion — this is the SHOULD-FIX "else-body
    // hover at IDE layer" regression coverage that was previously
    // missing from the suite.
    let sema = Semantics::new(&db);
    assert_eq!(
        ty_bridge::typeid_to_ty(&db, sema.type_of_expr(file_id, &else_rhs)),
        Ty::Number,
        "Semantics::type_of_expr on else-body must merge the else-IN overlay (Number)"
    );
}

// -----------------------------------------------------------------------------
// Test 9: narrow_query returns None for a DefWithBodyId not in the file.
// -----------------------------------------------------------------------------

#[test]
fn narrow_query_returns_none_for_unknown_owner() {
    // If the caller hands us a `Method(local_id)` that the file's
    // `ModuleBodies` has never heard of, `narrow_query` must return
    // `None` instead of panicking. This is what lets
    // `Semantics::type_of_expr` fall through to the base type when it
    // can't locate the body that covers the expression.
    let fixture = r#"
//- /test.bsl
Процедура П()
    Х = 1;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    // local_id `999` is chosen to exceed any real method count for this
    // trivial fixture — the `lower_result(999)` lookup returns None,
    // which `narrow_query` must propagate as its own None.
    let owner = DefWithBodyId::Method(999);

    assert!(
        narrow_query(&db, file_id, owner).is_none(),
        "narrow_query must return None when the owner does not resolve to a body in the file"
    );
}

// -----------------------------------------------------------------------------
// Task 6.7 — `type_narrowing` feature flag.
// -----------------------------------------------------------------------------

#[test]
fn type_narrowing_enabled_by_default() {
    // The Salsa `FeaturesInput` is eagerly initialised in
    // `RootDatabaseImpl::new()` with defaults matching
    // `FeaturesConfig::default()` (all flags on). A fresh database must
    // therefore report `type_narrowing_enabled() == true` and keep
    // `Semantics::type_of_expr` running the overlay merge — otherwise
    // every consumer would need to call the setter before expecting any
    // narrowing to happen, which contradicts the "opt-out, not opt-in"
    // decision from the Task 6.7 plan.
    let fixture = r#"
//- /test.bsl
Процедура П()
    Х = 42;
    Если ТипЗнч(Х) = Тип("Строка") Тогда
        А = Х;
    КонецЕсли;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(db.type_narrowing_enabled(), "fresh database must default `type_narrowing = true`");

    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let then_rhs = nth_ident_expr_at_distinct_position(&root, "Х", 1);

    let sema = Semantics::new(&db);
    assert_eq!(
        ty_bridge::typeid_to_ty(&db, sema.type_of_expr(file_id, &then_rhs)),
        Ty::String,
        "with default flags the narrowing overlay is applied — then-body `Х` sees `Ty::String`"
    );
}

#[test]
fn type_narrowing_disabled_skips_overlay() {
    // Toggling `type_narrowing` off makes `narrow_or_base` short-circuit
    // to the base inferred type *before* it invokes `db.narrow(...)`,
    // which is exactly what the feature flag is supposed to provide as a
    // rollback switch. The fixture is the same as
    // `narrowed_type_at_then_body_returns_narrowed_ty` — with narrowing
    // on the then-body `Х` is `Ty::String`; with it off, the overlay is
    // skipped and we fall back to the base `Ty::Number` inferred from
    // `Х = 42`.
    let fixture = r#"
//- /test.bsl
Процедура П()
    Х = 42;
    Если ТипЗнч(Х) = Тип("Строка") Тогда
        А = Х;
    КонецЕсли;
КонецПроцедуры
"#;
    let (mut db, file_id) = setup(fixture);
    db.set_type_narrowing_enabled(false);
    assert!(
        !db.type_narrowing_enabled(),
        "setter must flip the Salsa input so subsequent reads return false"
    );

    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let then_rhs = nth_ident_expr_at_distinct_position(&root, "Х", 1);

    let sema = Semantics::new(&db);
    assert_eq!(
        ty_bridge::typeid_to_ty(&db, sema.type_of_expr(file_id, &then_rhs)),
        Ty::Number,
        "with narrowing disabled, the then-body `Х` falls back to the base `Ty::Number`"
    );

    // Re-enable and confirm the flip is observable without recreating
    // the database. Salsa invalidates any cached read of the input, so
    // the next `narrow_or_base` call sees the new value.
    db.set_type_narrowing_enabled(true);
    let sema = Semantics::new(&db);
    assert_eq!(
        ty_bridge::typeid_to_ty(&db, sema.type_of_expr(file_id, &then_rhs)),
        Ty::String,
        "re-enabling the flag restores the narrowed `Ty::String` without DB rebuild"
    );
}

// -----------------------------------------------------------------------------
// Task 7 — `hir::Type::is_assignable_to` narrowing-aware end-to-end.
// -----------------------------------------------------------------------------

#[test]
fn is_assignable_to_sees_narrowed_ty_from_semantics() {
    // End-to-end: the narrowing-aware contract of
    // `hir::Type::is_assignable_to` is that callers build `hir::Type`
    // from `Semantics::type_of_expr` rather than a declared / base
    // type. The method itself stays pure on `Ty` — the narrowing
    // enters via the `type_of_expr` overlay.
    //
    // Fixture seeds `Х = 42` (base `Ty::Number`) then narrows on the
    // `Ty::String` branch. Inside the narrowed block:
    //
    // - `Type::from(Х) ≤ Type::from(Ty::String)` must hold (the
    //   narrowed type is String).
    // - `Type::from(Х) ≤ Type::from(Ty::Number)` must **not** hold —
    //   if this were true, we'd be reading the base (pre-narrow)
    //   type, not the narrowed one, i.e. `is_assignable_to` would not
    //   be narrowing-aware after all.
    let fixture = r#"
//- /test.bsl
Процедура П()
    Х = 42;
    Если ТипЗнч(Х) = Тип("Строка") Тогда
        А = Х;
    КонецЕсли;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let then_rhs = nth_ident_expr_at_distinct_position(&root, "Х", 1);

    let sema = Semantics::new(&db);
    let narrowed_ty = ty_bridge::typeid_to_ty(&db, sema.type_of_expr(file_id, &then_rhs));
    assert_eq!(
        narrowed_ty,
        Ty::String,
        "precondition: narrowing overlay must reach the then-body `Х` before we start the assignability probe",
    );

    let narrowed = Type::from_id(&db, file_id, sema.type_of_expr(file_id, &then_rhs));
    let expect_string = Type::from_id(&db, file_id, ty_bridge::ty_to_typeid(&db, &Ty::String));
    let expect_number = Type::from_id(&db, file_id, ty_bridge::ty_to_typeid(&db, &Ty::Number));

    assert!(
        narrowed.is_assignable_to(&expect_string),
        "narrowed `Х: String` must be assignable to a `String` slot"
    );
    assert!(
        !narrowed.is_assignable_to(&expect_number),
        "narrowed `Х: String` must NOT be assignable to a `Number` slot — confirms the \
         predicate consumes the narrowed overlay, not the base `Ty::Number` from `Х = 42`"
    );
}
