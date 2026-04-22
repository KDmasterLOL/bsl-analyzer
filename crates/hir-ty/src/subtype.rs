//! Structural assignability on raw [`Ty`].
//!
//! Single source of truth for the M4 Task 7 subtype / assignability
//! algorithm. Hosted in `hir-ty` (not `hir`) so that emitters inside
//! [`crate::infer`] can call it without dragging the IDE-facing `hir`
//! facade into the dependency graph — `hir` depends on `hir-ty`, so
//! any reverse reference would cycle.
//!
//! The public entry point is [`is_assignable`]; [`hir::Type::is_assignable_to`]
//! is a thin wrapper over it, and callers that already have raw [`Ty`]
//! values (inference, diagnostic emitters) call this module directly.
//!
//! See [`is_assignable`] for the full rule list.

use hir_def::ty::{MetadataKind, Ty};

use crate::coerce_this_object_to_metadata_ref;

/// Structural assignability: can a value of type `from` be used where
/// a value of type `to` is expected?
///
/// Rules:
///
/// - Reflexivity: `A ≤ A`.
/// - Gradual top/bottom: `A ≤ Unknown` and `Unknown ≤ A`. Neither
///   side constrains the check when one of the types is
///   [`Ty::Unknown`] — failed inference on either side must not
///   create false diagnostics. See the `[FIXME]` inside this function
///   for the revisit point when `InferenceDiagnostic::TypeMismatch`
///   acquires a live emitter.
/// - `Null ≤ ref-type` for any `MetadataKind::*Ref` variant.
/// - `A ≤ Union(…, X, …)` iff `A ≤ X` for some `X`.
/// - `Union(A, B) ≤ T` iff `A ≤ T ∧ B ≤ T` (distributes on the left).
/// - `ThisObject{(k, n)} ≤ MetadataRef{*Object matching k, n}` as a
///   **one-way** coercion. The reverse is rejected so
///   [`Ty::ThisObject`]'s provenance signal stays meaningful.
/// - Function subtyping: `Function { params_a, ret_a } ≤ Function
///   { params_b, ret_b }` iff arities match, `params_b[i] ≤ params_a[i]`
///   (contravariant parameters) and `ret_a ≤ ret_b` (covariant
///   return). Matches the classic `Fn(A) → R ≤ Fn(B) → S` rule from
///   λ-calculus / TAPL §15.
/// - Everything else: structural equality (`==`).
pub fn is_assignable(from: &Ty, to: &Ty) -> bool {
    // GRADUAL TYPING: `Unknown` on either side short-circuits. The
    // strict rule is `A ≤ Unknown` (Unknown as top); we also accept
    // `Unknown ≤ A` so a failed / partial inference on the from-side
    // does not fire a `TypeMismatch`. This is
    // deliberately permissive because `Ty::Unknown` bubbles out of
    // `hir-ty::infer` for any expression the inferrer bailed on — the
    // common case today is "unresolved param type" in user procedures,
    // not "unreachable code."
    //
    // [FIXME] Re-evaluate once production telemetry from the live
    // `TypeMismatch` emitter arrives. The emitter is wired in all
    // four `infer.rs` call-paths (qualified, three-level, generic
    // `Ty::Function`, fluent `Expr::Field` method-call). If real-code
    // signal shows users losing diagnostics they wanted, either
    // restrict the rule to spec-strict (`A ≤ Unknown` only) or gate
    // the bottom direction behind a "strict_type_check" feature flag
    // (parallel to `type_narrowing`). Until we have that signal,
    // erring permissive keeps the diagnostic volume sane on
    // under-annotated BSL code.
    if matches!(from, Ty::Unknown) || matches!(to, Ty::Unknown) {
        return true;
    }

    // Union left: distributes — `A | B ≤ T` iff every component is
    // assignable to `T`. Evaluated before union-right so a union-to-
    // union check unfolds left first (`Union(A, B) ≤ T ↔ A ≤ T ∧ B ≤ T`).
    if let Ty::Union(parts) = from {
        return parts.iter().all(|p| is_assignable(p, to));
    }
    // Union right: `A ≤ Union(…, X, …)` iff `A ≤ X` for some `X`.
    if let Ty::Union(parts) = to {
        return parts.iter().any(|p| is_assignable(from, p));
    }

    // `Null ≤ ref-type` — assigning `Null` to a catalog / document
    // reference (etc.) is how BSL clears a ref. No corresponding
    // `Undefined ≤ ref` rule at M4: the plan only mentions `Null`;
    // raise `Undefined` in a follow-up if real diagnostic traffic
    // shows false positives.
    if matches!(from, Ty::Null) && is_ref_ty(to) {
        return true;
    }

    // ThisObject → MetadataRef{*Object} coercion (one direction only):
    // `ЭтотОбъект` is accepted where the explicit
    // `CatalogObject.Товары` is expected. Delegates to the M3 coercion
    // helper so the mapping stays single-source
    // (`hir_ty::this_object`).
    //
    // The **reverse** direction (`MetadataRef{*Object} → ThisObject`)
    // is deliberately not accepted: [`Ty::ThisObject`] exists to
    // preserve the "explicitly self-referential" provenance signal
    // used by `BodyDiagnostic::RedundantAccessToObject` and future
    // rename / refactor features. Letting an arbitrary `CatalogObject.X`
    // satisfy a `ThisObject{(Catalog, X)}` slot would erase that
    // signal.
    if let Some(coerced) = coerce_this_object_to_metadata_ref(from) {
        if &coerced == to {
            return true;
        }
    }

    // Function subtyping (`Fn(A) → R ≤ Fn(B) → S ↔ B ≤ A ∧ R ≤ S`).
    // Arity must match — a shorter signature is never a subtype of a
    // longer one because the callee can't conjure the missing slots.
    // Params are **contravariant**: the supertype may accept *wider*
    // input than the subtype and still safely fill every call. Return
    // is **covariant**: the subtype may return a *narrower* value and
    // still satisfy the supertype's callers.
    //
    // Short-circuits when either side is not a function — the
    // reflexivity fallthrough below handles the structural-equal
    // function case anyway, so we only branch when variance actually
    // changes the answer.
    if let (
        Ty::Function { params: from_params, ret: from_ret },
        Ty::Function { params: to_params, ret: to_ret },
    ) = (from, to)
    {
        if from_params.len() != to_params.len() {
            return false;
        }
        let params_ok = from_params
            .iter()
            .zip(to_params.iter())
            .all(|(from_p, to_p)| is_assignable(to_p, from_p));
        return params_ok && is_assignable(from_ret, to_ret);
    }

    // Reflexivity — covers every structurally-equal pair: primitives,
    // `MetadataRef` (same kind + name), `ObjectManager`,
    // `ManagerCollection`, `PlatformObject`, `ThisObject` with equal
    // owner. `Ty::Function` handled above.
    from == to
}

/// Whether `ty` is one of the MDO reference variants — the set for
/// which [`is_assignable`] accepts `Null ≤ ref-type`.
///
/// Kept separate from [`is_assignable`] so the
/// [`crate::field_lookup`] / [`crate::method_lookup`] adapters can
/// reuse the same predicate if they ever need to gate behaviour on
/// "is the receiver a reference, or an object / manager / primitive?"
/// without re-deriving the list.
pub fn is_ref_ty(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::MetadataRef {
            kind: MetadataKind::CatalogRef
                | MetadataKind::DocumentRef
                | MetadataKind::EnumRef
                | MetadataKind::TaskRef
                | MetadataKind::BusinessProcessRef
                | MetadataKind::ExchangePlanRef
                | MetadataKind::ChartOfAccountsRef
                | MetadataKind::InformationRegisterRef
                | MetadataKind::AccumulationRegisterRef
                | MetadataKind::AccountingRegisterRef
                | MetadataKind::CalculationRegisterRef,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    //! Focused variance tests for `Ty::Function`. Reflexivity / union /
    //! Null / ThisObject coverage lives in
    //! `hir::type_facade::tests` where the API surface is
    //! `hir::Type::is_assignable_to`; this module pins the variance
    //! rule on the raw-`Ty` algorithm so regressions in the branch at
    //! the top of `is_assignable` surface here first.
    use super::*;

    fn fn_ty(params: Vec<Ty>, ret: Ty) -> Ty {
        Ty::Function { params: params.into_boxed_slice(), ret: Box::new(ret) }
    }

    #[test]
    fn function_reflexive() {
        // Sanity: identical signatures must still be assignable after
        // the new variance branch — contravariance degenerates to
        // reflexivity when both sides match exactly. Pins that the
        // branch does not regress the `Fn == Fn` case.
        let f = fn_ty(vec![Ty::Number, Ty::String], Ty::Boolean);
        assert!(is_assignable(&f, &f));
    }

    #[test]
    fn function_arity_mismatch_is_rejected() {
        // No argument count conversion happens at the type level: a
        // `Fn(Number)` is never a subtype of `Fn(Number, String)` even
        // if the first positions line up. Both directions must be
        // rejected — the contravariance rule has nothing to say about
        // the extra slot.
        let one = fn_ty(vec![Ty::Number], Ty::Boolean);
        let two = fn_ty(vec![Ty::Number, Ty::String], Ty::Boolean);
        assert!(!is_assignable(&one, &two));
        assert!(!is_assignable(&two, &one));
    }

    #[test]
    fn function_covariant_return_widens() {
        // Return is covariant: a function that promises `Number` can
        // stand in where a `Number | String` return is expected —
        // every value it returns satisfies the wider contract. The
        // reverse direction must be rejected (a caller expecting
        // `Number` cannot safely receive a `String`).
        let narrow = fn_ty(vec![], Ty::Number);
        let wide = fn_ty(vec![], Ty::union(vec![Ty::Number, Ty::String]));
        assert!(is_assignable(&narrow, &wide), "Number return ≤ Union return (covariant widening)");
        assert!(
            !is_assignable(&wide, &narrow),
            "Union return ≤ Number return must fail — String leg cannot satisfy Number callers"
        );
    }

    #[test]
    fn function_contravariant_param_widens() {
        // Param is contravariant: a function that accepts `Number |
        // String` is *more* permissive than one that accepts only
        // `Number`, so it safely fills the narrower slot. The opposite
        // direction must fail — a `Fn(Number)` cannot handle String
        // callers. Mirrors the "classic" Liskov rule for function
        // positions.
        let wide_param = fn_ty(vec![Ty::union(vec![Ty::Number, Ty::String])], Ty::Boolean);
        let narrow_param = fn_ty(vec![Ty::Number], Ty::Boolean);
        assert!(
            is_assignable(&wide_param, &narrow_param),
            "Fn(Union) ≤ Fn(Number) (contravariant — wider accepting subtype)"
        );
        assert!(
            !is_assignable(&narrow_param, &wide_param),
            "Fn(Number) ≤ Fn(Union) must fail — String callers would slip through"
        );
    }

    #[test]
    fn function_mixed_variance() {
        // Combined: `from` widens params (OK) and narrows return (OK),
        // so it is a subtype of `to`. The reverse direction loses both
        // axes. Catches any regression that flips one direction in
        // isolation (e.g. swapping the zip-side inside the variance
        // branch would still pass reflexive / single-axis tests but
        // break this one).
        let from = fn_ty(vec![Ty::union(vec![Ty::Number, Ty::String])], Ty::Number);
        let to = fn_ty(vec![Ty::Number], Ty::union(vec![Ty::Number, Ty::String]));
        assert!(is_assignable(&from, &to));
        assert!(!is_assignable(&to, &from));
    }

    #[test]
    fn function_unknown_short_circuit_wins_over_variance() {
        // Gradual-typing escape hatch still wins at the top of
        // `is_assignable` — an `Unknown` on either side must short-
        // circuit before the function branch gets a chance to reject
        // on arity / param mismatch. Otherwise a bailed-out param
        // type inside a function signature would cascade false
        // positives through every first-class function value. Mirrors
        // the rationale for the `Unknown ≤ A` rule in the module-level
        // docs.
        let f = fn_ty(vec![Ty::Number], Ty::Boolean);
        assert!(is_assignable(&Ty::Unknown, &f));
        assert!(is_assignable(&f, &Ty::Unknown));
    }
}
