#[test]
fn rejects_impossible_arity() {
    use super::{evaluate_applicability, ArityMismatch, CandidateEvaluation};
    use support::{positional, signature, AritySpec};

    // Given a signature requiring one argument and accepting at most two.
    let db = InMemoryDb::new();
    let number = db.number(None, None);
    let candidate = signature(&db, vec![
        positional("required", number, false),
        positional("optional", number, true),
    ], AritySpec {
        required: 1,
        maximum: Some(2),
    });

    // When calls fall below and above the accepted arity range.
    let too_few = evaluate_applicability(&db, &candidate, &[]);
    let too_many = evaluate_applicability(&db, &candidate, &[number, number, number]);

    // Then arity rejects before any argument comparison occurs.
    assert_eq!(
        too_few,
        CandidateEvaluation::ArityIncompatible(ArityMismatch::TooFew {
            actual: 0,
            required: 1,
        })
    );
    assert_eq!(
        too_many,
        CandidateEvaluation::ArityIncompatible(ArityMismatch::TooMany {
            actual: 3,
            maximum: 2,
        })
    );
}

#[test]
fn incomplete_variadic_metadata_is_indeterminate() {
    use super::{
        evaluate_applicability, ArgumentApplicability, ArgumentIndeterminateReason,
        CandidateApplicability, CandidateEvaluation,
    };
    use support::{positional, signature, AritySpec};

    // Given an unbounded signature whose variadic element metadata is absent.
    let db = InMemoryDb::new();
    let number = db.number(None, None);
    let candidate = signature(&db, vec![positional("known", number, false)], AritySpec {
        required: 1,
        maximum: None,
    });

    // When a tail argument has no declared element parameter to compare against.
    let evaluation = evaluate_applicability(&db, &candidate, &[number, number]);

    // Then the candidate remains indeterminate instead of accepting or panicking.
    let CandidateEvaluation::ArityCompatible {
        applicability: CandidateApplicability::Indeterminate { arguments },
        ..
    } = evaluation
    else {
        panic!("missing variadic metadata must remain indeterminate");
    };
    assert_eq!(
        arguments[1].applicability,
        ArgumentApplicability::Indeterminate(
            ArgumentIndeterminateReason::MissingParameterMetadata
        )
    );
}

#[test]
fn malformed_variadic_metadata_is_indeterminate() {
    use super::{
        evaluate_applicability, ArgumentApplicability, ArgumentIndeterminateReason,
        CandidateApplicability, CandidateEvaluation,
    };
    use support::{positional, signature, variadic, AritySpec};

    // Given a malformed variadic parameter followed by a positional parameter.
    let db = InMemoryDb::new();
    let number = db.number(None, None);
    let boolean = db.boolean();
    let candidate = signature(&db, vec![
        variadic("rest", boolean),
        positional("after_rest", number, false),
    ], AritySpec {
        required: 1,
        maximum: None,
    });

    // When both argument positions are evaluated.
    let evaluation = evaluate_applicability(&db, &candidate, &[boolean, number]);

    // Then malformed positional mapping yields only indeterminate evidence.
    let CandidateEvaluation::ArityCompatible {
        applicability: CandidateApplicability::Indeterminate { arguments },
        ..
    } = evaluation
    else {
        panic!("malformed variadic metadata must remain indeterminate");
    };
    assert!(arguments.iter().all(|argument| argument.applicability
        == ArgumentApplicability::Indeterminate(
            ArgumentIndeterminateReason::MissingParameterMetadata
        )));
}
