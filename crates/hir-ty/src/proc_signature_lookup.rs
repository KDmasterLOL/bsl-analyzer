//! Workspace-method signature adapter.
//!
//! Bridges [`crate::proc_signature::proc_signature_query`] to the
//! [`crate::method_lookup::MethodInfo`] shape that platform-method
//! adapters already produce. Consumers (manager-method dispatch, form
//! self-receiver lookup, future receiver-based workspace method
//! lookup) call [`resolve_workspace_method`] with a resolved
//! [`MethodId`] — they own the receiver→`MethodId` resolution; this
//! module owns only the conversion to `MethodInfo`.
//!
//! ## Cycle status (post Phase O.16b)
//!
//! `proc_signature_query` no longer reads `db.infer(file_id)`. The
//! docstring-less branch was dropped to a direct `Ty::Unknown` return
//! (see `proc_signature.rs` module-level doc-comment); cascade typing
//! recovers body-derived precision at the call site via
//! [`crate::method_graph::method_return_type_query`] (cycle-safe via
//! `cycle_fn` / `cycle_initial`). The
//! `proc_signature_query → infer_query → infer_method →
//! proc_signature_query` self-edge that Phase L's wrapper rewrite
//! would otherwise have closed is therefore broken by construction;
//! this adapter still needs no `cycle_fn` of its own.

use hir_def::{MethodId, MethodIdInput};

use crate::db::HirDatabase;
use crate::method_lookup::MethodInfo;
use crate::proc_signature::proc_signature_query;
use crate::Ty;

/// Lower a workspace-defined method's signature into a [`MethodInfo`]
/// in the same shape the platform-method path produces.
///
/// `overloads` is always empty — BSL does not let workspace methods
/// declare multiple `Вариант синтаксиса:` overloads, so the
/// `params` slot is the only signature consumers need.
///
/// Procedures get `return_ty = Ty::Undefined` (matching `to_method_info`'s
/// `return_type: None` shape); functions get the docstring-derived or
/// body-walk-inferred return type from
/// [`crate::proc_signature::proc_signature_query`].
pub fn resolve_workspace_method(db: &dyn HirDatabase, method_id: MethodId) -> MethodInfo {
    let method_input = MethodIdInput::new(db, method_id);
    let signature = proc_signature_query(db, method_input);
    MethodInfo {
        return_ty: signature.return_ty.clone(),
        params: signature.params.clone(),
        overloads: Vec::new(),
    }
}

/// Like [`resolve_workspace_method`] but yields `Ty::Unknown` slots when
/// the workspace method has no docstring entries — handy when callers
/// only need the return type and want to skip the per-parameter `clone`.
pub fn resolve_workspace_return_ty(db: &dyn HirDatabase, method_id: MethodId) -> Ty {
    let method_input = MethodIdInput::new(db, method_id);
    proc_signature_query(db, method_input).return_ty.clone()
}
