use bsl_platform::{MethodParam, PlatformConstructor};
use bsl_types::testing::InMemoryDb;
use hir_def::execution_env::EnvFlags;

use super::constructor_candidates;
use crate::call_binding::resolve_binding;
use crate::{CallRejection, CallSelection, CandidateId, PlatformSignatureSlot};

fn constructor(id: u32, parameter_count: usize) -> PlatformConstructor {
    PlatformConstructor {
        id,
        type_name: "InvariantConstructor".into(),
        variant_name: None,
        parameters: (0..parameter_count)
            .map(|index| MethodParam {
                name: format!("Param{index}").into(),
                param_type: None,
                is_optional: false,
                is_variadic: false,
            })
            .collect(),
        min_version: None,
        context: None,
    }
}

#[test]
fn constructor_fallback_is_order_invariant_at_equal_distance() {
    let db = InMemoryDb::new();
    let one_parameter = constructor(40, 1);
    let three_parameters = constructor(20, 3);

    let forward = constructor_candidates(&db, &[&one_parameter, &three_parameters], EnvFlags::ALL)
        .expect("constructor identities must be unique");
    let reverse = constructor_candidates(&db, &[&three_parameters, &one_parameter], EnvFlags::ALL)
        .expect("constructor identities must be unique");

    let arguments = [db.unknown(), db.unknown()];
    let forward = resolve_binding(&db, forward, &arguments);
    let reverse = resolve_binding(&db, reverse, &arguments);

    assert_eq!(forward.semantic.resolution, reverse.semantic.resolution);
    assert_eq!(
        forward.semantic.resolution.selection,
        CallSelection::Rejected(CallRejection::Arity {
            fallback: crate::ArityFallback {
                candidate: CandidateId::Platform {
                    method_id: 20,
                    signature: PlatformSignatureSlot::Base,
                },
                mismatch: crate::call_resolution::ArityMismatch::TooFew { actual: 2, required: 3 },
                distance: 1,
            },
        })
    );
    let forward_arity = forward.arity.expect("all constructors reject arity");
    let reverse_arity = reverse.arity.expect("all constructors reject arity");
    assert_eq!(
        (forward_arity.required_count, forward_arity.total_count),
        (reverse_arity.required_count, reverse_arity.total_count)
    );
    assert_eq!((forward_arity.required_count, forward_arity.total_count), (3, 3));
}
