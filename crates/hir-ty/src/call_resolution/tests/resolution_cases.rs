use bsl_types::builders::Builders;
use bsl_types::kind::TypeId;
use bsl_types::testing::InMemoryDb;

use super::super::{CallSignature, CandidateScore};
use super::support::{candidate, positional, variadic, AritySpec, CandidateSpec};

#[derive(Debug)]
pub(super) enum ExpectedSelection {
    Unique(usize),
    Ambiguous(Vec<usize>),
    TypeRejected,
    ArityRejected(usize),
}

pub(super) struct Case {
    pub name: &'static str,
    pub candidates: Vec<CallSignature>,
    pub arguments: Vec<TypeId>,
    pub expected: ExpectedSelection,
    pub expected_return: TypeId,
    pub expected_scores: Vec<(usize, CandidateScore)>,
}

fn unary_candidate(
    ordinal: usize,
    param: super::super::CallParam,
    return_ty: TypeId,
) -> CallSignature {
    candidate(CandidateSpec {
        ordinal,
        params: vec![param],
        arity: AritySpec { required: 1, maximum: Some(1) },
        return_ty,
    })
}

const fn score(rank: (usize, usize, usize, usize, usize)) -> CandidateScore {
    let (indeterminate_count, coercion_count, assignable_count, defaults_used, variadic_used) =
        rank;
    CandidateScore {
        indeterminate_count,
        coercion_count,
        assignable_count,
        defaults_used,
        variadic_used,
    }
}

pub(super) fn cases(db: &InMemoryDb) -> Vec<Case> {
    let unknown = db.unknown();
    let any = db.any();
    let number = db.number(None, None);
    let string = db.string(None, false);
    let boolean = db.boolean();
    let structure = db.structure(None);
    let date = db.date(bsl_types::facet::DateComponent::DateTime);
    vec![
        Case {
            name: "exact beats assignable and coercion",
            candidates: vec![
                unary_candidate(0, positional("value", number, false), number),
                unary_candidate(1, positional("value", any, false), string),
                unary_candidate(2, positional("value", string, false), boolean),
            ],
            arguments: vec![number],
            expected: ExpectedSelection::Unique(0),
            expected_return: number,
            expected_scores: vec![
                (0, score((0, 0, 0, 0, 0))),
                (1, score((0, 0, 1, 0, 0))),
                (2, score((0, 1, 0, 0, 0))),
            ],
        },
        Case {
            name: "known applicability outranks indeterminate",
            candidates: vec![
                unary_candidate(0, positional("value", unknown, false), string),
                unary_candidate(1, positional("value", string, false), number),
            ],
            arguments: vec![number],
            expected: ExpectedSelection::Unique(1),
            expected_return: number,
            expected_scores: vec![(0, score((1, 0, 0, 0, 0))), (1, score((0, 1, 0, 0, 0)))],
        },
        Case {
            name: "assignable beats coercion",
            candidates: vec![
                unary_candidate(0, positional("value", any, false), number),
                unary_candidate(1, positional("value", string, false), string),
            ],
            arguments: vec![number],
            expected: ExpectedSelection::Unique(0),
            expected_return: number,
            expected_scores: vec![(0, score((0, 0, 1, 0, 0))), (1, score((0, 1, 0, 0, 0)))],
        },
        Case {
            name: "equal known returns remain exact",
            candidates: vec![
                unary_candidate(0, positional("value", number, false), number),
                unary_candidate(1, positional("value", number, false), number),
            ],
            arguments: vec![number],
            expected: ExpectedSelection::Ambiguous(vec![0, 1]),
            expected_return: number,
            expected_scores: vec![(0, score((0, 0, 0, 0, 0))), (1, score((0, 0, 0, 0, 0)))],
        },
        Case {
            name: "differing known returns form a canonical union",
            candidates: vec![
                unary_candidate(0, positional("value", number, false), number),
                unary_candidate(1, positional("value", number, false), string),
            ],
            arguments: vec![number],
            expected: ExpectedSelection::Ambiguous(vec![0, 1]),
            expected_return: db.union(vec![number, string]),
            expected_scores: Vec::new(),
        },
        Case {
            name: "an exact Unknown return taints the aggregate",
            candidates: vec![
                unary_candidate(0, positional("value", number, false), number),
                unary_candidate(1, positional("value", number, false), unknown),
            ],
            arguments: vec![number],
            expected: ExpectedSelection::Ambiguous(vec![0, 1]),
            expected_return: unknown,
            expected_scores: Vec::new(),
        },
        Case {
            name: "a union canonicalized to concrete is not tainted",
            candidates: vec![
                unary_candidate(0, positional("value", number, false), number),
                unary_candidate(
                    1,
                    positional("value", number, false),
                    db.union(vec![unknown, number]),
                ),
            ],
            arguments: vec![number],
            expected: ExpectedSelection::Ambiguous(vec![0, 1]),
            expected_return: number,
            expected_scores: Vec::new(),
        },
        Case {
            name: "defaults and variadic usage participate in the score",
            candidates: vec![
                unary_candidate(0, positional("value", number, false), number),
                candidate(CandidateSpec {
                    ordinal: 1,
                    params: vec![
                        positional("value", number, false),
                        positional("optional", string, true),
                    ],
                    arity: AritySpec { required: 1, maximum: Some(2) },
                    return_ty: number,
                }),
                candidate(CandidateSpec {
                    ordinal: 2,
                    params: vec![variadic("values", number)],
                    arity: AritySpec { required: 0, maximum: None },
                    return_ty: number,
                }),
            ],
            arguments: vec![number],
            expected: ExpectedSelection::Unique(0),
            expected_return: number,
            expected_scores: vec![
                (0, score((0, 0, 0, 0, 0))),
                (1, score((0, 0, 0, 1, 0))),
                (2, score((0, 0, 0, 0, 1))),
            ],
        },
        Case {
            name: "indeterminate-only candidates preserve ambiguity",
            candidates: vec![
                candidate(CandidateSpec {
                    ordinal: 0,
                    params: vec![
                        positional("value", unknown, false),
                        positional("optional", number, true),
                    ],
                    arity: AritySpec { required: 1, maximum: Some(2) },
                    return_ty: number,
                }),
                unary_candidate(1, positional("value", unknown, false), string),
            ],
            arguments: vec![number],
            expected: ExpectedSelection::Ambiguous(vec![0, 1]),
            expected_return: db.union(vec![number, string]),
            expected_scores: vec![(0, score((1, 0, 0, 1, 0))), (1, score((1, 0, 0, 0, 0)))],
        },
        Case {
            name: "one indeterminate candidate is not a known unique match",
            candidates: vec![unary_candidate(0, positional("value", unknown, false), number)],
            arguments: vec![number],
            expected: ExpectedSelection::Ambiguous(vec![0]),
            expected_return: number,
            expected_scores: vec![(0, score((1, 0, 0, 0, 0)))],
        },
        Case {
            name: "arity-compatible type failures have no arity fallback",
            candidates: vec![
                unary_candidate(0, positional("value", boolean, false), number),
                unary_candidate(1, positional("value", date, false), string),
            ],
            arguments: vec![structure],
            expected: ExpectedSelection::TypeRejected,
            expected_return: unknown,
            expected_scores: Vec::new(),
        },
        Case {
            name: "equal arity distance uses identity for display only",
            candidates: vec![
                candidate(CandidateSpec {
                    ordinal: 1,
                    params: vec![
                        positional("first", number, false),
                        positional("second", number, false),
                    ],
                    arity: AritySpec { required: 2, maximum: Some(2) },
                    return_ty: number,
                }),
                candidate(CandidateSpec {
                    ordinal: 0,
                    params: Vec::new(),
                    arity: AritySpec { required: 0, maximum: Some(0) },
                    return_ty: string,
                }),
            ],
            arguments: vec![number],
            expected: ExpectedSelection::ArityRejected(0),
            expected_return: unknown,
            expected_scores: Vec::new(),
        },
    ]
}
