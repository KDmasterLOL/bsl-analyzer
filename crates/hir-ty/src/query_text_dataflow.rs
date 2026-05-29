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

pub(crate) fn refine_query_at_use_site(
    db: &dyn HirDatabase,
    file_id: FileId,
    owner: DefWithBodyId,
    use_expr_id: ExprId,
    name: &Name,
    body: &Body,
) -> Option<Arc<[Option<Arc<Projection>>]>> {
    let DefWithBodyId::Method(local_id) = owner else {
        return None;
    };

    let receiver_lower = name.as_str().to_lowercase();
    let composite_ru = format!("{receiver_lower}.текст");
    let composite_en = format!("{receiver_lower}.text");

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
