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
//! turn that literal back into an [`Projection`].
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
use bsl_types::kind::Projection;
use dataflow::reaching_defs::DefSite;
use hir_def::body::Body;
use hir_def::hir::{Expr, Literal, Stmt};
use hir_def::{
    sdbl_hir_for_file_query, DefWithBodyId, ExprId, IdConversion, Name, SdblExprId, StmtId,
};
use vfs::FileId;

use crate::db::HirDatabase;
use crate::sdbl_bridge::package_to_projections;

/// Try to recover the per-sub-query [`Projection`] vector for a
/// `<name>` use site by walking reaching `<name>.Текст = "..."` writes.
///
/// Returns `Some(projections)` only when **every** reaching definition
/// for `<name>.Текст` is a static SDBL string literal whose bridge
/// lowering produces structurally equal projection vectors. The vector
/// shape mirrors [`crate::sdbl_bridge::package_to_projections`] — one
/// entry per sub-query in the SDBL package, in source order — so the
/// chain-rewrite consumers (`projection_of_query_receiver`,
/// `projections_of_query_receiver`) read the same shape regardless of
/// whether refinement came from Phase B (constructor literal) or Phase
/// D/F (variable-state reaching defs).
///
/// Any ambiguity (loop-carried append, dynamic text, divergent
/// literals across branches, missing dataflow result) collapses to
/// `None`. Multi-statement SDBL packages (`ВЫБРАТЬ A; ВЫБРАТЬ B`) are
/// supported — the vector carries every sub-query's projection so
/// `.Выполнить()` can read the last (runtime semantics) and
/// `.ВыполнитьПакет()[i]` can index by position.
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
) -> Option<Arc<[Option<Arc<Projection>>]>> {
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
    // produce structurally equal projection vectors. The first
    // successful resolution becomes the candidate; later resolutions
    // must match it byte-for-byte (divergent literals across branches
    // ship as None — a future phase may upgrade to per-branch
    // projection union).
    let mut candidate: Option<Arc<[Option<Arc<Projection>>]>> = None;
    for def in defs {
        let DefSite::Assignment(stmt_raw) = def.def_site else {
            return None;
        };
        let assign_stmt_id = StmtId::from_raw(stmt_raw);
        let projections =
            projections_from_text_assignment(db, file_id, owner, body, assign_stmt_id)?;
        match &candidate {
            None => candidate = Some(projections),
            Some(prev) if projections_eq_ignoring_provenance(prev, &projections) => (),
            Some(_) => return None,
        }
    }
    candidate
}

/// Structural equality of two per-sub-query projection vectors that
/// **ignores provenance** (`ProjectionOrigin` / `ProjectionFieldSource`).
///
/// Branch convergence must hinge on the observable shape — field order,
/// names, interned `TypeId`s, and the SDBL display shadow — never on
/// provenance hints. The kernel strips provenance at intern time, so two
/// reaching writes that produce the same projected columns must converge
/// even if a future bridge tags fields with finer `Column`/`Cast`/… sources.
fn projections_eq_ignoring_provenance(
    a: &[Option<Arc<Projection>>],
    b: &[Option<Arc<Projection>>],
) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| match (x, y) {
            (None, None) => true,
            (Some(x), Some(y)) => {
                x.fields.len() == y.fields.len()
                    && x.raw_sdbl_types == y.raw_sdbl_types
                    && x.fields
                        .iter()
                        .zip(y.fields.iter())
                        .all(|(fx, fy)| fx.name == fy.name && fx.ty == fy.ty)
            }
            _ => false,
        })
}

/// Resolve a single `<var>.Текст = "<literal>"` assignment to its
/// per-sub-query projection vector.
///
/// Returns `None` for any shape that isn't a static SDBL string
/// literal — `Зап.Текст = Зап.Текст + "..."` (append idiom),
/// `Зап.Текст = ПолучитьТекст()` (dynamic), or assignments where the
/// literal is not picked up by the lowerer's `looks_like_sdbl` gate.
/// Multi-statement packages (`ВЫБРАТЬ A; ВЫБРАТЬ B`) project every
/// sub-query in source order — the chain-rewrite layer picks the
/// runtime-relevant entry (`last` for `.Выполнить()`, position-indexed
/// for `.ВыполнитьПакет()[i]`).
///
/// An empty package (parser produced no queries) collapses to `None`
/// so the upstream gate sees nothing to attach.
fn projections_from_text_assignment(
    db: &dyn HirDatabase,
    file_id: FileId,
    owner: DefWithBodyId,
    body: &Body,
    assign_stmt_id: StmtId,
) -> Option<Arc<[Option<Arc<Projection>>]>> {
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

    if pkg.queries().is_empty() {
        return None;
    }
    Some(package_to_projections(db, pkg).into())
}
