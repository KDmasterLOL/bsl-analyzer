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
/// Rules (M4_PLAN.md §Task 7):
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
/// - Everything else: structural equality (`==`).
///
/// Function types (`Ty::Function { params, ret }`) currently fall
/// through to structural equality — variance (contravariant params,
/// covariant return) is a follow-up once TypeMismatch emission shows
/// real pressure.
pub fn is_assignable(from: &Ty, to: &Ty) -> bool {
    // GRADUAL TYPING: `Unknown` on either side short-circuits. The
    // M4_PLAN spec only guarantees `A ≤ Unknown` (Unknown as top); we
    // also accept `Unknown ≤ A` so a failed / partial inference on
    // the from-side does not fire a `TypeMismatch`. This is
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
    // union check unfolds left first, which matches the rule in
    // M4_PLAN.md ("Union(A, B) ≤ T ↔ A ≤ T ∧ B ≤ T").
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

    // Reflexivity — covers every structurally-equal pair: primitives,
    // `MetadataRef` (same kind + name), `ObjectManager`,
    // `ManagerCollection`, `Function`, `PlatformObject`, `ThisObject`
    // with equal owner.
    //
    // [TODO] Task-7-followup: `Ty::Function { params, ret }` falls
    // through to structural equality here. Variance (contravariant
    // params, covariant return) will matter once first-class function
    // values feed `TypeMismatch` emission in `hir-ty::infer`; today
    // function values are rare enough in real BSL that strict equality
    // is adequate for the first-pass predicate.
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
