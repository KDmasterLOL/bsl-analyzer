use std::collections::{btree_map::Entry, BTreeMap};

use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;

use super::{CallCandidateSet, CallSignature, CandidateId, DuplicateCandidateId};

impl CallCandidateSet {
    pub(crate) fn merge_by_id(
        db: &dyn TypeKernelDb,
        candidates: impl IntoIterator<Item = CallSignature>,
    ) -> Result<Self, DuplicateCandidateId> {
        let mut merged = BTreeMap::<CandidateId, CallSignature>::new();
        for candidate in candidates {
            match merged.entry(candidate.id) {
                Entry::Vacant(entry) => {
                    entry.insert(candidate);
                }
                Entry::Occupied(mut entry) => {
                    // Equal IDs identify the same source record and signature slot, so params,
                    // arity, origin, provenance, and documentation metadata are identical.
                    // Only receiver-path return refinement and environment availability may differ.
                    let existing = entry.get_mut();
                    if existing.return_ty != candidate.return_ty {
                        existing.return_ty =
                            db.union(vec![existing.return_ty, candidate.return_ty]);
                    }
                    existing.environment = existing.environment | candidate.environment;
                }
            }
        }
        Self::try_from(merged.into_values().collect::<Vec<_>>())
    }
}

impl TryFrom<Vec<CallSignature>> for CallCandidateSet {
    type Error = DuplicateCandidateId;

    fn try_from(mut candidates: Vec<CallSignature>) -> Result<Self, Self::Error> {
        candidates.sort_by_key(|candidate| candidate.id);
        if let Some(duplicate) =
            candidates.windows(2).find(|pair| pair[0].id == pair[1].id).map(|pair| pair[0].id)
        {
            return Err(DuplicateCandidateId { id: duplicate });
        }
        Ok(Self(candidates.into_boxed_slice()))
    }
}
