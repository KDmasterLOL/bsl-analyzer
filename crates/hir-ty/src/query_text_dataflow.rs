//! Variable-state refinement for SDBL queries (Phase D).
//!
//! Reaching-definitions–driven recovery of the projection for receivers
//! shaped like
//!
//! ```bsl
//! Зап = Новый Запрос;
//! Зап.Текст = "ВЫБРАТЬ Имя ИЗ Справочник.Товары";
//! Зап.Выполнить().Выбрать().Имя
//! ```
//!
//! Phase B synthesises a projection when the SDBL literal is the
//! constructor argument; this module covers the variable-assignment
//! idiom by tracing `<var>.Текст` definitions reaching the dispatch
//! site, validating that every reaching write is a static SDBL string
//! literal, and reusing the [`crate::sdbl_bridge`] lowering pipeline to
//! turn that literal back into an [`SdblProjection`].
//!
//! ## Hook surface
//!
//! [`refine_query_at_use_site`] is invoked from two callers:
//!
//! 1. [`crate::method_lookup::apply_sdbl_chain_rewrite`] — Phase D
//!    chain-dispatch refinement (`Зап.Выполнить()` and friends).
//! 2. [`crate::infer::InferCtx::infer_path_name`] — Phase F lift: bare
//!    `Expr::Path("Зап")` uses (e.g. `Возврат Зап;` from a helper
//!    body) refine to the same projection so cross-method propagation
//!    via `method_return_type_query` carries it to callers.
//!
//! Both callers gate on `receiver_needs_refinement` (see
//! `method_lookup.rs`) so this helper is only reached when:
//!
//! 1. The receiver type carries no projection — either
//!    `Ty::PlatformObject("Запрос")` (legacy) or
//!    `Ty::Query { projections: [None] }` (Phase B synth produced None
//!    because the constructor arg was not a static literal).
//! 2. The receiver/binding name is known (callers extract it from the
//!    `Expr::Path` or pass it directly).
//!
//! Failure modes (multiple writes with divergent text, dynamic text,
//! loop-carried append, non-literal RHS, owner is module-level code)
//! all collapse to `None`, leaving the chain rewrite to produce the
//! existing `Ty::QueryResult{None}` shape — the user still gets
//! methods, just without projection enrichment.

use std::sync::Arc;

use base_db::FileIdInput;
use dataflow::reaching_defs::DefSite;
use hir_def::body::Body;
use hir_def::hir::{Expr, Literal, Stmt};
use hir_def::ty::SdblProjection;
use hir_def::{
    sdbl_hir_for_file_query, DefWithBodyId, ExprId, IdConversion, Name, SdblExprId, StmtId,
};
use vfs::FileId;

use crate::db::HirDatabase;
use crate::sdbl_bridge::query_to_projection;

/// Try to recover an [`SdblProjection`] for a `<name>` use site by
/// walking reaching `<name>.Текст = "..."` writes.
///
/// Returns `Some(projection)` only when **every** reaching definition
/// for `<name>.Текст` is a static SDBL string literal whose bridge
/// lowering produces structurally equal projections. Any ambiguity
/// (loop-carried append, dynamic text, divergent literals across
/// branches, multi-statement SDBL package, missing dataflow result)
/// collapses to `None`.
///
/// `use_expr_id` is the ExprId of the use site (either an
/// `Expr::Field` chain dispatch — Phase D — or a bare `Expr::Path`
/// reference — Phase F). Used only to find the enclosing statement
/// via [`Body::enclosing_stmt`]. `name` is the binding name; callers
/// extract it from the AST and pass it directly so this helper does
/// not need to re-inspect the receiver shape.
pub(crate) fn refine_query_at_use_site(
    db: &dyn HirDatabase,
    file_id: FileId,
    owner: DefWithBodyId,
    use_expr_id: ExprId,
    name: &Name,
    body: &Body,
) -> Option<Arc<SdblProjection>> {
    // Reaching-defs analysis runs per-method (see
    // `module_reaching_definitions_query` in `ide-db`); module-level
    // code lives in a separate `DefWithBodyId::ModuleCode` body that
    // the per-method index does not cover.
    let DefWithBodyId::Method(local_id) = owner else {
        return None;
    };

    // Composite keys for the `<name>.Текст` field assignment.
    //
    // The dataflow transfer function gen/kills on `base.field`
    // strings (see `extract_var_name` in
    // `crates/dataflow/src/reaching_defs.rs`); the field tail is the
    // user-written spelling, which BSL allows in either the Russian
    // (`Текст`) or English (`Text`) form. Both keys are queried and
    // the resulting Definition sets merged — `defs_for_var`
    // lowercases internally, so the composites stay in their natural
    // case and the storage-side normaliser does the rest.
    let receiver_lower = name.as_str().to_lowercase();
    let composite_ru = format!("{receiver_lower}.текст");
    let composite_en = format!("{receiver_lower}.text");

    // The reaching-defs API asks "what definitions reach the
    // beginning of this statement?". The use site lives somewhere
    // inside a single statement; outer expressions (an `Expr::Call`
    // wrapping an `Expr::Field`, the bare path itself, etc.) all
    // resolve to the same enclosing stmt via `Body::enclosing_stmt`.
    let dispatch_stmt_id = body.enclosing_stmt(use_expr_id)?;

    let module_defs = db.module_reaching_definitions(file_id);
    let method_defs = module_defs.get(local_id)?;

    let mut defs =
        method_defs.defs_for_var_at_stmt(&composite_ru, dispatch_stmt_id).unwrap_or_default();
    if let Some(en_defs) = method_defs.defs_for_var_at_stmt(&composite_en, dispatch_stmt_id) {
        defs.extend(en_defs);
    }
    if defs.is_empty() {
        return None;
    }

    // All reaching writes must resolve to a static SDBL literal and
    // produce the structurally equal projection. The first
    // successful resolution becomes the candidate; later
    // resolutions must match it byte-for-byte (Phase D ships
    // divergent-literal as None — Phase E may upgrade to
    // `Ty::union` over per-branch projections).
    let mut candidate: Option<Arc<SdblProjection>> = None;
    for def in defs {
        let DefSite::Assignment(stmt_raw) = def.def_site else {
            return None;
        };
        let assign_stmt_id = StmtId::from_raw(stmt_raw);
        let proj = projection_from_text_assignment(db, file_id, owner, body, assign_stmt_id)?;
        match &candidate {
            None => candidate = Some(proj),
            Some(prev) if **prev == *proj => (),
            Some(_) => return None,
        }
    }
    candidate
}

/// Resolve a single `<var>.Текст = "<literal>"` assignment to its
/// projection.
///
/// Returns `None` for any shape that isn't a static SDBL string
/// literal — `Зап.Текст = Зап.Текст + "..."` (append idiom),
/// `Зап.Текст = ПолучитьТекст()` (dynamic), or assignments where the
/// literal is not picked up by the lowerer's `looks_like_sdbl` gate.
/// Multi-statement packages (`ВЫБРАТЬ A; ВЫБРАТЬ B`) also collapse to
/// `None`: the variable-state idiom is not the canonical way to
/// build batch queries and the user-visible refinement target for
/// `.Выполнить()` is a single-result projection.
fn projection_from_text_assignment(
    db: &dyn HirDatabase,
    file_id: FileId,
    owner: DefWithBodyId,
    body: &Body,
    assign_stmt_id: StmtId,
) -> Option<Arc<SdblProjection>> {
    let Stmt::Assign { value, .. } = body.stmt(assign_stmt_id) else {
        return None;
    };
    let value_id = ExprId::from_idx(*value);
    if !matches!(body.expr(value_id), Expr::Literal(Literal::String(_))) {
        return None;
    }

    let sdbl_expr_id = SdblExprId { owner, expr_id: value_id };
    let file_id_input = FileIdInput::new(db, file_id);
    let entries = sdbl_hir_for_file_query(db, file_id_input);
    let (_, pkg) = entries.iter().find(|(id, _)| *id == sdbl_expr_id)?;

    if pkg.queries().len() != 1 {
        return None;
    }
    query_to_projection(&pkg.queries()[0])
}
