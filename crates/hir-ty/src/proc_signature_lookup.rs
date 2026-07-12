use hir_def::{MethodId, MethodIdInput};

use crate::db::HirDatabase;
use crate::method_lookup::MethodInfo;
use crate::proc_signature::proc_signature_query;
use bsl_types::kind::TypeId;

pub fn resolve_workspace_method(db: &dyn HirDatabase, method_id: MethodId) -> MethodInfo {
    let method_input = MethodIdInput::new(db, method_id);
    let signature = proc_signature_query(db, method_input);
    MethodInfo {
        return_ty: signature.return_ty,
        params: signature.params.clone(),
        overloads: Vec::new(),
        env: hir_def::execution_env::EnvFlags::ALL,
    }
}

pub fn resolve_workspace_return_ty(db: &dyn HirDatabase, method_id: MethodId) -> TypeId {
    let method_input = MethodIdInput::new(db, method_id);
    let signature = proc_signature_query(db, method_input);
    signature.return_ty
}
