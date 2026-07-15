use bsl_types::builders::Builders;
use bsl_types::kind::TypeId;
use bsl_types::testing::InMemoryDb;

use super::super::{
    resolve_candidates, ArgumentApplicability, ArgumentEvaluation, ArgumentParameter,
    ArityMismatch, BuiltinCallableId, CallCandidateSet, CallParamMode, CallRejection,
    CallSelection, CallSignature, CandidateDisposition, CandidateId, CandidateRejection,
    CandidateScore,
};
use super::support::{candidate, positional, AritySpec, CandidateSpec};

type ExpectedFact = (CandidateId, Option<CandidateScore>, CandidateDisposition);

struct Case {
    name: &'static str,
    candidates: Vec<CallSignature>,
    arguments: Vec<TypeId>,
    selection: CallSelection,
    return_ty: TypeId,
    facts: Vec<ExpectedFact>,
    reverse_input: bool,
}

fn candidate_id(ordinal: usize) -> CandidateId {
    CandidateId::Builtin { callable: BuiltinCallableId::Intrinsic(80), signature_ordinal: ordinal }
}

const fn score(rank: (usize, usize, usize, usize, usize)) -> CandidateScore {
    CandidateScore {
        indeterminate_count: rank.0,
        coercion_count: rank.1,
        assignable_count: rank.2,
        defaults_used: rank.3,
        variadic_used: rank.4,
    }
}

pub(super) fn assert_contracts(db: &InMemoryDb) {
    for case in cases(db) {
        let candidates = CallCandidateSet::try_from(case.candidates.clone())
            .expect("adversarial fixtures have unique candidate identities");
        let result = resolve_candidates(db, &candidates, &case.arguments);
        let facts = result
            .candidates
            .iter()
            .map(|fact| (fact.id, fact.score, fact.disposition.clone()))
            .collect::<Vec<_>>();
        assert_eq!(result.selection, case.selection, "{}", case.name);
        assert_eq!(result.return_ty, case.return_ty, "{}", case.name);
        assert_eq!(facts, case.facts, "{}", case.name);

        if case.reverse_input {
            let mut reversed = case.candidates;
            reversed.reverse();
            let reversed = CallCandidateSet::try_from(reversed)
                .expect("reversed adversarial candidates remain unique");
            assert_eq!(result, resolve_candidates(db, &reversed, &case.arguments), "{}", case.name);
        }
    }
}

fn cases(db: &InMemoryDb) -> Vec<Case> {
    let unknown = db.unknown();
    let any = db.any();
    let number = db.number(None, None);
    let string = db.string(None, false);
    let boolean = db.boolean();
    let structure = db.structure(None);
    let exact = score((0, 0, 0, 0, 0));
    let assignable_once = score((0, 0, 1, 0, 0));
    vec![
        Case {
            name: "empty candidate set",
            candidates: Vec::new(),
            arguments: Vec::new(),
            selection: CallSelection::Rejected(CallRejection::NoCandidates),
            return_ty: unknown,
            facts: Vec::new(),
            reverse_input: false,
        },
        Case {
            name: "type rejection dominates mixed arity rejection",
            candidates: vec![
                candidate(CandidateSpec {
                    ordinal: 0,
                    params: vec![positional("value", boolean, false)],
                    arity: AritySpec { required: 1, maximum: Some(1) },
                    return_ty: number,
                }),
                candidate(CandidateSpec {
                    ordinal: 1,
                    params: Vec::new(),
                    arity: AritySpec { required: 0, maximum: Some(0) },
                    return_ty: string,
                }),
            ],
            arguments: vec![structure],
            selection: CallSelection::Rejected(CallRejection::Type),
            return_ty: unknown,
            facts: vec![
                (
                    candidate_id(0),
                    None,
                    CandidateDisposition::Rejected(CandidateRejection::TypeIncompatible {
                        arguments: vec![ArgumentEvaluation {
                            index: 0,
                            argument_ty: structure,
                            parameter: ArgumentParameter::Declared {
                                index: 0,
                                mode: CallParamMode::Positional,
                                ty: boolean,
                            },
                            applicability: ArgumentApplicability::Incompatible,
                        }]
                        .into_boxed_slice(),
                    }),
                ),
                (
                    candidate_id(1),
                    None,
                    CandidateDisposition::Rejected(CandidateRejection::Arity(
                        ArityMismatch::TooMany { actual: 1, maximum: 0 },
                    )),
                ),
            ],
            reverse_input: false,
        },
        Case {
            name: "earlier score dimension dominates lower total",
            candidates: vec![
                candidate(CandidateSpec {
                    ordinal: 1,
                    params: vec![positional("left", any, false), positional("right", any, false)],
                    arity: AritySpec { required: 2, maximum: Some(2) },
                    return_ty: number,
                }),
                candidate(CandidateSpec {
                    ordinal: 0,
                    params: vec![
                        positional("left", number, false),
                        positional("right", string, false),
                    ],
                    arity: AritySpec { required: 2, maximum: Some(2) },
                    return_ty: string,
                }),
            ],
            arguments: vec![number, number],
            selection: CallSelection::Unique { candidate: candidate_id(1) },
            return_ty: number,
            facts: vec![
                (candidate_id(0), Some(score((0, 1, 0, 0, 0))), CandidateDisposition::LowerRanked),
                (candidate_id(1), Some(score((0, 0, 2, 0, 0))), CandidateDisposition::Survivor),
            ],
            reverse_input: true,
        },
        Case {
            name: "discarded Unknown return does not taint winner",
            candidates: vec![
                candidate(CandidateSpec {
                    ordinal: 0,
                    params: vec![positional("value", any, false)],
                    arity: AritySpec { required: 1, maximum: Some(1) },
                    return_ty: unknown,
                }),
                candidate(CandidateSpec {
                    ordinal: 1,
                    params: vec![positional("value", number, false)],
                    arity: AritySpec { required: 1, maximum: Some(1) },
                    return_ty: number,
                }),
            ],
            arguments: vec![number],
            selection: CallSelection::Unique { candidate: candidate_id(1) },
            return_ty: number,
            facts: vec![
                (candidate_id(0), Some(assignable_once), CandidateDisposition::LowerRanked),
                (candidate_id(1), Some(exact), CandidateDisposition::Survivor),
            ],
            reverse_input: false,
        },
    ]
}
