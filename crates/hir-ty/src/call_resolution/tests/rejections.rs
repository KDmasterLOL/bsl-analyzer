#[test]
fn rejects_wrong_variadic_element() {
    use super::{
        evaluate_applicability, ArgumentApplicability, CandidateApplicability,
        CandidateEvaluation,
    };
    use support::{positional, signature, variadic, AritySpec};

    // Given a number followed by a boolean variadic tail.
    let db = InMemoryDb::new();
    let number = db.number(None, None);
    let boolean = db.boolean();
    let candidate = signature(&db, vec![
        positional("first", number, false),
        variadic("rest", boolean),
    ], AritySpec {
        required: 1,
        maximum: None,
    });

    // When a concrete number reaches the variadic boolean element position.
    let evaluation = evaluate_applicability(&db, &candidate, &[number, boolean, number]);

    // Then the candidate is incompatible at that tail argument.
    let CandidateEvaluation::ArityCompatible {
        applicability: CandidateApplicability::Incompatible { arguments },
        ..
    } = evaluation
    else {
        panic!("wrong variadic element must reject the candidate");
    };
    assert_eq!(arguments[2].applicability, ArgumentApplicability::Incompatible);
}

#[test]
fn rejects_cross_product_signature() {
    use super::{evaluate_applicability, CandidateApplicability, CandidateEvaluation};
    use support::{positional, signature, AritySpec};

    // Given the independent signatures (Number, Date) and (Boolean, Structure).
    let db = InMemoryDb::new();
    let number = db.number(None, None);
    let date = db.date(bsl_types::facet::DateComponent::DateTime);
    let boolean = db.boolean();
    let structure = db.structure(None);
    let first = signature(&db, vec![
        positional("left", number, false),
        positional("right", date, false),
    ], AritySpec {
        required: 2,
        maximum: Some(2),
    });
    let second = signature(&db, vec![
        positional("left", boolean, false),
        positional("right", structure, false),
    ], AritySpec {
        required: 2,
        maximum: Some(2),
    });

    // When arguments cross the first position of one signature with the second of the other.
    let arguments = [number, structure];
    let evaluations = [
        evaluate_applicability(&db, &first, &arguments),
        evaluate_applicability(&db, &second, &arguments),
    ];

    // Then neither complete signature is accepted.
    assert!(evaluations.iter().all(|evaluation| matches!(
        evaluation,
        CandidateEvaluation::ArityCompatible {
            applicability: CandidateApplicability::Incompatible { .. },
            ..
        }
    )));
}

#[test]
fn rejects_unknown_plus_concrete_incompatibility() {
    use super::{
        evaluate_applicability, ArgumentApplicability, CandidateApplicability,
        CandidateEvaluation,
    };
    use support::{positional, signature, AritySpec};

    // Given one unknown comparison followed by a concrete boolean parameter.
    let db = InMemoryDb::new();
    let unknown = db.unknown();
    let number = db.number(None, None);
    let boolean = db.boolean();
    let candidate = signature(&db, vec![
        positional("uncertain", unknown, false),
        positional("concrete", boolean, false),
    ], AritySpec {
        required: 2,
        maximum: Some(2),
    });

    // When the second argument is concretely incompatible.
    let evaluation = evaluate_applicability(&db, &candidate, &[unknown, number]);

    // Then the concrete contradiction rejects the whole candidate.
    let CandidateEvaluation::ArityCompatible {
        applicability: CandidateApplicability::Incompatible { arguments },
        ..
    } = evaluation
    else {
        panic!("a concrete contradiction must dominate unknown evidence");
    };
    assert!(matches!(arguments[0].applicability, ArgumentApplicability::Indeterminate(_)));
    assert_eq!(arguments[1].applicability, ArgumentApplicability::Incompatible);
}
