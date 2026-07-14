use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{TypeId, TypeKind};
use hir_def::execution_env::EnvFlags;
use hir_def::Name;

use crate::call_resolution::CallCandidateSet;

use super::{lookup_method_inner, MethodInfo, RefineCtx};

pub(super) fn lookup(
    db: &dyn TypeKernelDb,
    members: &[TypeId],
    method_name: &Name,
    refine_ctx: Option<&RefineCtx<'_>>,
) -> Option<MethodInfo> {
    let live: Vec<TypeId> = members
        .iter()
        .copied()
        .filter(|id| !matches!(db.lookup_type(*id), TypeKind::Undefined | TypeKind::Null))
        .collect();
    let mut returns: Vec<TypeId> = Vec::with_capacity(live.len());
    let mut signatures = Vec::new();
    let mut hit_any = false;
    // A union receiver is one concrete arm at runtime; the member counts as
    // available wherever ANY arm provides it, so arm envs are united.
    let mut env = EnvFlags::EMPTY;
    for member in live {
        if let Some(info) = lookup_method_inner(db, member, method_name, refine_ctx) {
            hit_any = true;
            returns.push(info.return_ty);
            env = env | info.env;
            signatures.extend(info.candidates.as_slice().iter().cloned());
        }
    }
    let candidates = CallCandidateSet::merge_by_id(db, signatures).ok()?;
    hit_any.then(|| MethodInfo { return_ty: db.union(returns), candidates, env })
}
