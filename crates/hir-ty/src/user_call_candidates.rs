use bsl_types::kind::TypeId;
use hir_def::{MethodId, Name};

use crate::call_resolution::{CallCandidateSet, CallSignature, DuplicateCandidateId};
use crate::db::HirDatabase;

pub(crate) fn for_resolved_method(
    db: &dyn HirDatabase,
    method_name: &Name,
    primary_method: MethodId,
    primary_return: TypeId,
) -> Result<CallCandidateSet, DuplicateCandidateId> {
    let symbols = db.symbol_tree(primary_method.module);
    let options = db.env_options();
    let candidates: Vec<CallSignature> = symbols
        .find_methods(method_name)
        .into_iter()
        .filter(|method| method.id == primary_method)
        .map(|method| {
            let signature =
                crate::method_resolution::materialise_signature_enriched(db, method.id, method);
            let environment =
                crate::method_environment::effective_method_env(db, method.id, &options);
            CallSignature::from_user_method(method, &signature, primary_return, environment)
        })
        .collect();
    CallCandidateSet::try_from(candidates)
}
