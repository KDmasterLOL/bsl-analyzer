use bsl_types::kind::TypeId;
use hir_def::MethodId;

use crate::{call_resolution::CallCandidateSet, db::HirDatabase};

pub(super) fn for_workspace_method(
    db: &dyn HirDatabase,
    method_id: MethodId,
    return_ty: TypeId,
) -> CallCandidateSet {
    let symbol_tree = db.symbol_tree(method_id.module);
    let method = symbol_tree.find_method_by_id(method_id).expect(
        "method_id supplied to resolve_workspace_method must exist in its module symbol tree",
    );
    crate::user_call_candidates::for_resolved_method(db, &method.name, method_id, return_ty)
        .expect("one workspace method must produce one unique candidate")
}
