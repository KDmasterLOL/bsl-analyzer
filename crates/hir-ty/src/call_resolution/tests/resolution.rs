fn resolution_candidate_id(ordinal: usize) -> CandidateId {
    CandidateId::Builtin {
        callable: BuiltinCallableId::Intrinsic(80),
        signature_ordinal: ordinal,
    }
}

#[test]
fn resolution() {
    use super::{
        resolve_candidates, CallRejection, CallSelection, CandidateDisposition, CandidateRejection,
    };
    use resolution_cases::{cases, ExpectedSelection};

    let db = InMemoryDb::new();

    // Given candidate sets spanning every ranking and aggregation class.
    for case in cases(&db) {
        // When the pure candidate-set resolver evaluates one call.
        let candidates = super::CallCandidateSet::try_from(case.candidates)
            .expect("resolution fixtures have unique candidate identities");
        let result = resolve_candidates(&db, &candidates, &case.arguments);

        // Then selection, survivors, scores, return aggregation, and rejection stay structured.
        assert_eq!(result.return_ty, case.expected_return, "{}", case.name);
        for (ordinal, expected_score) in case.expected_scores {
            let fact = result
                .candidates
                .iter()
                .find(|fact| fact.id == resolution_candidate_id(ordinal))
                .expect("expected candidate fact");
            assert_eq!(fact.score, Some(expected_score), "{}", case.name);
        }
        match case.expected {
            ExpectedSelection::Unique(ordinal) => {
                assert_eq!(
                    result.selection,
                    CallSelection::Unique { candidate: resolution_candidate_id(ordinal) },
                    "{}",
                    case.name
                );
            }
            ExpectedSelection::Ambiguous(ordinals) => {
                let candidates = ordinals
                    .into_iter()
                    .map(resolution_candidate_id)
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                assert_eq!(
                    result.selection,
                    CallSelection::Ambiguous { candidates },
                    "{}",
                    case.name
                );
            }
            ExpectedSelection::TypeRejected => {
                assert_eq!(result.selection, CallSelection::Rejected(CallRejection::Type));
                assert!(result.candidates.iter().all(|fact| matches!(
                    fact.disposition,
                    CandidateDisposition::Rejected(CandidateRejection::TypeIncompatible { .. })
                )));
            }
            ExpectedSelection::ArityRejected(ordinal) => {
                let CallSelection::Rejected(CallRejection::Arity { fallback }) = result.selection
                else {
                    panic!("{}: expected arity rejection", case.name);
                };
                assert_eq!(fallback.candidate, resolution_candidate_id(ordinal));
                assert_eq!(fallback.distance, 1);
                assert!(result.candidates.iter().all(|fact| matches!(
                    fact.disposition,
                    CandidateDisposition::Rejected(CandidateRejection::Arity(_))
                )));
            }
        }
    }
    resolution_adversarial::assert_contracts(&db);
}

#[test]
fn preserves_equal_score_ambiguity() {
    use super::{resolve_candidates, CallSelection};
    use support::{positional, AritySpec};

    // Given equal-score known candidates with different returns in both input orders.
    let db = InMemoryDb::new();
    let number = db.number(None, None);
    let string = db.string(None, false);
    let arity = AritySpec { required: 1, maximum: Some(1) };
    let first = support::candidate(support::CandidateSpec {
        ordinal: 0,
        params: vec![positional("value", number, false)],
        arity,
        return_ty: number,
    });
    let second = support::candidate(support::CandidateSpec {
        ordinal: 1,
        params: vec![positional("value", number, false)],
        arity,
        return_ty: string,
    });

    // When both orders are resolved.
    let forward_candidates = super::CallCandidateSet::try_from(vec![first.clone(), second.clone()])
        .expect("forward candidates are unique");
    let reverse_candidates = super::CallCandidateSet::try_from(vec![second, first])
        .expect("reverse candidates are unique");
    let forward = resolve_candidates(&db, &forward_candidates, &[number]);
    let reverse = resolve_candidates(&db, &reverse_candidates, &[number]);

    // Then stable output ordering does not become semantic first-candidate selection.
    assert_eq!(forward, reverse);
    assert_eq!(
        forward.selection,
        CallSelection::Ambiguous {
            candidates: vec![resolution_candidate_id(0), resolution_candidate_id(1)]
                .into_boxed_slice(),
        }
    );
    assert_eq!(forward.return_ty, db.union(vec![number, string]));
}
