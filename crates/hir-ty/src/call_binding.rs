use std::sync::Arc;

use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;

use crate::call_resolution::{
    resolve_candidates, CallCandidateSet, CallRejection, CallResolution, CallSelection,
    CandidateDisposition, CandidateId,
};
use crate::infer::{CandidateCallBinding, ParamsShape};

pub(crate) struct BindingProjection {
    pub semantic: CandidateCallBinding,
    pub params: ParamsShape,
    pub params_from_doc_comment: bool,
    pub arity: Option<ArityDiagnosticInput>,
}

pub(crate) struct ArityDiagnosticInput {
    pub required_count: usize,
    pub total_count: usize,
}

impl CallResolution {
    pub fn unique_candidate(&self) -> Option<CandidateId> {
        match self.selection {
            CallSelection::Unique { candidate } => Some(candidate),
            CallSelection::Ambiguous { .. } | CallSelection::Rejected(_) => None,
        }
    }

    pub fn is_survivor(&self, candidate: CandidateId) -> bool {
        self.candidates
            .iter()
            .any(|fact| fact.id == candidate && fact.disposition == CandidateDisposition::Survivor)
    }
}

pub(crate) fn resolve_binding(
    db: &dyn TypeKernelDb,
    candidates: CallCandidateSet,
    argument_types: &[TypeId],
) -> BindingProjection {
    let resolution = resolve_candidates(db, &candidates, argument_types);
    let params = compatibility_params(&candidates, &resolution);
    let params_from_doc_comment =
        candidates.as_slice().iter().all(|candidate| candidate.from_doc_comment);
    let arity = arity_diagnostic(&candidates, &resolution);
    BindingProjection {
        semantic: CandidateCallBinding { candidates, resolution },
        params,
        params_from_doc_comment,
        arity,
    }
}

fn compatibility_params(candidates: &CallCandidateSet, resolution: &CallResolution) -> ParamsShape {
    let signatures = candidates.as_slice();
    if let [signature] = signatures {
        return ParamsShape::Single(signature.params.iter().map(|param| param.ty).collect());
    }
    let selected = match resolution.selection {
        CallSelection::Unique { candidate } => Some(candidate),
        CallSelection::Ambiguous { .. } | CallSelection::Rejected(_) => None,
    };
    let flat = selected
        .and_then(|id| signature_by_id(candidates, id))
        .map(|signature| signature.params.iter().map(|param| param.ty).collect())
        .unwrap_or_else(|| Arc::from([]));
    let overloads = signatures
        .iter()
        .map(|signature| signature.params.iter().map(|param| param.ty).collect())
        .collect::<Vec<Arc<[TypeId]>>>()
        .into();
    ParamsShape::Overloaded { flat, overloads }
}

fn arity_diagnostic(
    candidates: &CallCandidateSet,
    resolution: &CallResolution,
) -> Option<ArityDiagnosticInput> {
    let CallSelection::Rejected(CallRejection::Arity { fallback }) = resolution.selection else {
        return None;
    };
    let signature = signature_by_id(candidates, fallback.candidate)?;
    Some(ArityDiagnosticInput {
        required_count: signature.required_args,
        total_count: signature.params.len(),
    })
}

fn signature_by_id(
    candidates: &CallCandidateSet,
    id: CandidateId,
) -> Option<&crate::call_resolution::CallSignature> {
    candidates.as_slice().iter().find(|candidate| candidate.id == id)
}

#[cfg(test)]
mod tests {
    use bsl_types::testing::InMemoryDb;

    use super::{resolve_binding, CallCandidateSet};
    use crate::builtin::builtin_functions;

    #[test]
    fn capped_builtin_uses_declared_params_for_compatibility_arity() {
        let db = InMemoryDb::new();
        let builtins = builtin_functions();
        let callable = builtins.callable_id("StrTemplate").expect("StrTemplate must be registered");
        let candidates = CallCandidateSet::try_from(
            builtins
                .get("StrTemplate")
                .expect("StrTemplate signature must be registered")
                .iter()
                .enumerate()
                .map(|(ordinal, signature)| signature.to_call_signature(&db, callable, ordinal))
                .collect::<Vec<_>>(),
        )
        .expect("builtin signatures must have unique identities");

        assert_eq!(candidates.as_slice().len(), 1);
        assert_eq!(candidates.as_slice()[0].max_args, Some(11));
        assert_eq!(candidates.as_slice()[0].params.len(), 2);

        let projection = resolve_binding(&db, candidates, &[]);
        let arity = projection.arity.expect("zero arguments must be rejected");
        assert_eq!((arity.required_count, arity.total_count), (1, 2));
    }
}
