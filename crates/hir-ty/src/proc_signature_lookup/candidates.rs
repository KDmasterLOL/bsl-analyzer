use bsl_types::kind::TypeId;
use hir_def::MethodId;

use crate::{call_resolution::CallCandidateSet, db::HirDatabase};

pub(super) fn for_workspace_method(
    db: &dyn HirDatabase,
    method_id: MethodId,
    return_ty: TypeId,
) -> CallCandidateSet {
    crate::user_call_candidates::for_resolved_method(db, method_id, return_ty)
        .expect("one workspace method must produce one unique candidate")
}
