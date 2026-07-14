use hir_def::{MethodId, MethodIdInput};

use bsl_types::kind::TypeId;

use crate::{db::HirDatabase, method_lookup::MethodInfo, proc_signature::proc_signature_query};

mod candidates;

pub fn resolve_workspace_method(db: &dyn HirDatabase, method_id: MethodId) -> MethodInfo {
    let method_input = MethodIdInput::new(db, method_id);
    let signature = proc_signature_query(db, method_input);
    let candidates = candidates::for_workspace_method(db, method_id, signature.return_ty);
    MethodInfo {
        return_ty: signature.return_ty,
        candidates,
        env: hir_def::execution_env::EnvFlags::ALL,
    }
}

pub fn resolve_workspace_return_ty(db: &dyn HirDatabase, method_id: MethodId) -> TypeId {
    let method_input = MethodIdInput::new(db, method_id);
    let signature = proc_signature_query(db, method_input);
    signature.return_ty
}
