//! Structural assignability on interned [`TypeId`]s.
//!
//! Single source of truth for the M4 Task 7 subtype / assignability
//! algorithm. Hosted in `hir-ty` (not `hir`) so that emitters inside
//! [`crate::infer`] can call it without dragging the IDE-facing `hir`
//! facade into the dependency graph — `hir` depends on `hir-ty`, so
//! any reverse reference would cycle.
//!
//! Phase 3 §4.E: the algorithm matches on [`TypeKind`] directly via
//! `db.lookup_type(id)` rather than on legacy [`hir_def::ty::Ty`].
//! Callers that still hold raw `Ty` bridge through
//! [`crate::ty_bridge::ty_to_typeid`] at the call site; the public
//! entry points take `TypeId`. Structural equality collapses to
//! `TypeId` equality because interning canonicalises every value.

use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{MetadataKind, TypeId, TypeKind};

/// Structural assignability: can a value of type `from` be used where
/// a value of type `to` is expected?
///
/// Rules:
///
/// - Reflexivity: `A ≤ A` — collapses to `TypeId` equality because
///   interning canonicalises structurally-equal types to the same id.
/// - Gradual top/bottom: `A ≤ Unknown` and `Unknown ≤ A`. Neither
///   side constrains the check when one of the types is
///   [`TypeKind::Unknown`] — failed inference on either side must not
///   create false diagnostics.
/// - Universal top: `A ≤ Any` and `Any ≤ A`. `Произвольный` is BSL's
///   universal type — assignable to and from every type.
/// - Bottom: `Never ≤ A` for all `A` (proven-unreachable values fill
///   any slot). The reverse (`A ≤ Never`) stays false except for the
///   reflexive `Never ≤ Never`.
/// - `Null ≤ ref-type` for any `MetadataKind::*Ref` variant.
/// - `A ≤ Union(…, X, …)` iff `A ≤ X` for some `X`.
/// - `Union(A, B) ≤ T` iff `A ≤ T ∧ B ≤ T` (distributes on the left).
/// - `ThisObject{(k, n)} ≤ MetadataRef{*Object matching k, n}` as a
///   **one-way** coercion. The reverse is rejected so
///   [`TypeKind::ThisObject`]'s provenance signal stays meaningful.
/// - Function subtyping: arities match, params contravariant, return
///   covariant (TAPL §15).
/// - Everything else: structural equality (`TypeId` ==).
pub fn is_assignable(db: &dyn TypeKernelDb, from: TypeId, to: TypeId) -> bool {
    // Reflexivity fast path: canonical ids compare by value.
    if from == to {
        return true;
    }

    let from_kind = db.lookup_type(from);
    let to_kind = db.lookup_type(to);

    // GRADUAL / UNIVERSAL: `Unknown` (analysis-incomplete) and `Any`
    // (`Произвольный`, the universal type) are permissive in BOTH
    // directions. The strict rule is `A ≤ Unknown` (Unknown as top);
    // we also accept `Unknown ≤ A` so a failed / partial inference on
    // the from-side does not fire a `TypeMismatch`. `Произвольный`
    // behaves the same — a value annotated `Any` is assignment-
    // compatible everywhere, and any value fits an `Any` slot.
    //
    // These kernel sentinels (`Any`, and `Never` below) are not
    // produced by the current `Ty`-bridged callers — legacy `Ty` has
    // no `Any`/`Never` — so the arms are inert today and exist for the
    // §4.F native callers, keeping the kernel-native API total.
    if matches!(from_kind, TypeKind::Unknown | TypeKind::Any)
        || matches!(to_kind, TypeKind::Unknown | TypeKind::Any)
    {
        return true;
    }

    // BOTTOM: `Never ≤ A` for every `A` — a proven-unreachable value
    // can fill any slot. The reverse `A ≤ Never` stays false (only
    // `Never ≤ Never`, already caught by the reflexivity fast-path).
    if matches!(from_kind, TypeKind::Never) {
        return true;
    }

    // Union left: distributes — `A | B ≤ T` iff every component is
    // assignable to `T`. Evaluated before union-right so a union-to-
    // union check unfolds left first.
    if let TypeKind::Union(parts) = from_kind {
        return parts.iter().all(|p| is_assignable(db, *p, to));
    }
    // Union right: `A ≤ Union(…, X, …)` iff `A ≤ X` for some `X`.
    if let TypeKind::Union(parts) = to_kind {
        return parts.iter().any(|p| is_assignable(db, from, *p));
    }

    // `Null ≤ ref-type` — assigning `Null` to a catalog / document
    // reference (etc.) is how BSL clears a ref. No corresponding
    // `Undefined ≤ ref` rule: the plan only mentions `Null`.
    if matches!(from_kind, TypeKind::Null) && is_ref_kind(to_kind) {
        return true;
    }

    // TabularSectionRow ↔ `PlatformObject("Строка табличной части")`
    // bridge (bidirectional). Both denote the same BSL value: the row
    // receiver carries a `(parent, "Parent.Section")` payload so
    // attribute access can find the right XML attributes, but the
    // platform's own type system describes "any tabular-section row"
    // with the bare type name. Subtyping must accept conversion in
    // either direction.
    if is_tabular_row_bridge(from_kind, to_kind) {
        return true;
    }

    // Array ↔ TypedArray bridge (bidirectional) + covariant element.
    //
    // `Ty::Array` lowers to `Array { element: None }`; `Ty::TypedArray`
    // to `Array { element: Some(_) }`. The two forms denote the same
    // BSL value — a runtime array — but differ in whether an element
    // witness is recoverable. They must cross the slot boundary freely:
    // dropping the witness is always safe, accepting a witness-less
    // array where a typed one is expected is the gradual stance, and
    // `TypedArray(A) ≤ TypedArray(B)` is covariant on the element.
    if is_array_bridge(db, from_kind, to_kind) {
        return true;
    }

    // ThisObject / ThisManager → MetadataRef / ObjectManager coercion
    // (one direction only): `ЭтотОбъект` is accepted where the explicit
    // `CatalogObject.Товары` is expected. The reverse is rejected so
    // the provenance signal used by `RedundantAccessToObject` stays
    // meaningful.
    //
    // §4.E.4a: native kernel coercion — `ThisObject` / `ThisManager`
    // owners carry resolved configs, so coerce + compare directly (no Ty
    // round-trip; `config_id` preserved). Gated on the discriminant so
    // the common non-`This*` path pays no extra `lookup_type`.
    if matches!(from_kind, TypeKind::ThisObject { .. } | TypeKind::ThisManager { .. }) {
        if let Some(coerced) = crate::this_object::coerce_to_metadata_ref_id(db, from) {
            if coerced == to {
                return true;
            }
        }
    }

    // Function subtyping (`Fn(A) → R ≤ Fn(B) → S ↔ B ≤ A ∧ R ≤ S`).
    // Arity must match — a shorter signature is never a subtype of a
    // longer one. Params are **contravariant**, return is **covariant**.
    if let (TypeKind::Function(from_fn), TypeKind::Function(to_fn)) = (from_kind, to_kind) {
        if from_fn.params.len() != to_fn.params.len() {
            return false;
        }
        let params_ok = from_fn
            .params
            .iter()
            .zip(to_fn.params.iter())
            .all(|(from_p, to_p)| is_assignable(db, to_p.ty, from_p.ty));
        return params_ok && is_assignable(db, from_fn.returns, to_fn.returns);
    }

    // Reflexivity already handled by the `from == to` fast path; any
    // remaining distinct-id pair is not assignable.
    false
}

/// Whether `kind` is one of the MDO reference variants — the set for
/// which [`is_assignable`] accepts `Null ≤ ref-type`.
fn is_ref_kind(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::MetadataRef(facet) if matches!(
            facet.kind,
            MetadataKind::CatalogRef
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
        )
    )
}

/// Whether `ty` is one of the MDO reference variants. Kept public so
/// the [`crate::field_lookup`] / [`crate::method_lookup`] adapters can
/// reuse the same predicate.
pub fn is_ref_ty(db: &dyn TypeKernelDb, ty: TypeId) -> bool {
    is_ref_kind(db.lookup_type(ty))
}

/// Symmetric bridge between the concrete row receiver
/// `MetadataRef { TabularSectionRow { _ }, _ }` and the generic
/// platform-object form `PlatformObject("Строка табличной части")`
/// (and its English alias `"Line of a tabular section"`).
fn is_tabular_row_bridge(a: &TypeKind, b: &TypeKind) -> bool {
    fn is_row_metadata_ref(ty: &TypeKind) -> bool {
        matches!(
            ty,
            TypeKind::MetadataRef(facet)
                if matches!(facet.kind, MetadataKind::TabularSectionRow { .. })
        )
    }
    fn is_row_platform_object(ty: &TypeKind) -> bool {
        matches!(ty, TypeKind::PlatformObject(facet)
            if facet.name.eq_ignore_ascii_case("Line of a tabular section")
                || facet.name.to_lowercase() == "строка табличной части")
    }
    (is_row_metadata_ref(a) && is_row_platform_object(b))
        || (is_row_platform_object(a) && is_row_metadata_ref(b))
}

/// Bidirectional bridge between unparameterised arrays
/// (`Array { element: None }`) and element-bearing arrays
/// (`Array { element: Some(_) }`), plus covariant element comparison
/// for the element-bearing → element-bearing case.
fn is_array_bridge(db: &dyn TypeKernelDb, from: &TypeKind, to: &TypeKind) -> bool {
    match (from, to) {
        (TypeKind::Array(a), TypeKind::Array(b)) => match (a.element, b.element) {
            // Drop / gradually-accept the element witness — always safe.
            (Some(_), None) | (None, Some(_)) => true,
            // Covariant on the element.
            (Some(ae), Some(be)) => is_assignable(db, ae, be),
            // (None, None) is reflexive — handled by the caller's
            // `from == to` fast path, never reaches here for distinct ids.
            (None, None) => false,
        },
        _ => false,
    }
}

/// Argument-position coercion check.
///
/// Equivalent to [`is_assignable`] except that **any** value coerces
/// to a bare [`TypeKind::String`] target. BSL implicitly stringifies
/// values when a String slot is expected (`СтрШаблон`, `Сообщить`, log
/// writers, exception messages, …), so firing `TypeMismatch` on
/// `Number → String` or `Union(Date, Number) → String` produces noise
/// on real code without representing a runtime bug.
///
/// **Scope is intentionally narrow.** Call only from the call-site
/// argument-mismatch emitter ([`crate::arg_diagnostics`]). All other
/// subtype users must keep using [`is_assignable`].
pub fn is_coercible_to(db: &dyn TypeKernelDb, from: TypeId, to: TypeId) -> bool {
    if matches!(db.lookup_type(to), TypeKind::String(_)) {
        return true;
    }
    is_assignable(db, from, to)
}

#[cfg(test)]
mod tests {
    //! Focused variance tests for `Function`. Reflexivity / union /
    //! Null / ThisObject coverage lives in `hir::type_facade::tests`
    //! where the API surface is `hir::Type::is_assignable_to`; this
    //! module pins the variance rule on the kernel algorithm so
    //! regressions surface here first.
    //!
    //! Phase 3 §4.E: tests build legacy `Ty` values (the readable
    //! fixture surface) and bridge them to `TypeId` via `id(&db, …)`
    //! before invoking the kernel-native `is_assignable`.
    use super::*;
    use bsl_types::builders::Builders;
    use bsl_types::facet::{ArgArity, FunctionFacet, FunctionOrigin, ParamPassing, ParamSpec};
    use bsl_types::testing::InMemoryDb;
    use std::sync::Arc;

    fn fn_ty(db: &dyn TypeKernelDb, params: Vec<TypeId>, ret: TypeId) -> TypeId {
        let params: Arc<[ParamSpec]> = params
            .into_iter()
            .enumerate()
            .map(|(idx, ty)| {
                ParamSpec::new(format!("p{}", idx + 1), ty, ParamPassing::ByRef, false)
            })
            .collect();
        db.function(FunctionFacet::new(
            params.clone(),
            vec![None; params.len()].into(),
            params.len() as u16,
            ArgArity::Fixed(params.len() as u16),
            ret,
            FunctionOrigin::Unknown,
        ))
    }

    fn metadata_ref_id(db: &dyn TypeKernelDb, kind: MetadataKind, name: &str) -> TypeId {
        db.metadata_ref(kind, name.to_string(), &bsl_types::testing::RootConfigCtx)
    }

    #[test]
    fn function_reflexive() {
        let db = InMemoryDb::new();
        let f = fn_ty(&db, vec![db.number(None, None), db.string(None, false)], db.boolean());
        assert!(is_assignable(&db, f, f));
    }

    #[test]
    fn function_arity_mismatch_is_rejected() {
        let db = InMemoryDb::new();
        let one = fn_ty(&db, vec![db.number(None, None)], db.boolean());
        let two = fn_ty(&db, vec![db.number(None, None), db.string(None, false)], db.boolean());
        assert!(!is_assignable(&db, one, two));
        assert!(!is_assignable(&db, two, one));
    }

    #[test]
    fn function_covariant_return_widens() {
        let db = InMemoryDb::new();
        let narrow = fn_ty(&db, vec![], db.number(None, None));
        let wide =
            fn_ty(&db, vec![], db.union(vec![db.number(None, None), db.string(None, false)]));
        assert!(
            is_assignable(&db, narrow, wide),
            "Number return ≤ Union return (covariant widening)"
        );
        assert!(
            !is_assignable(&db, wide, narrow),
            "Union return ≤ Number return must fail — String leg cannot satisfy Number callers"
        );
    }

    #[test]
    fn function_contravariant_param_widens() {
        let db = InMemoryDb::new();
        let wide_param = fn_ty(
            &db,
            vec![db.union(vec![db.number(None, None), db.string(None, false)])],
            db.boolean(),
        );
        let narrow_param = fn_ty(&db, vec![db.number(None, None)], db.boolean());
        assert!(
            is_assignable(&db, wide_param, narrow_param),
            "Fn(Union) ≤ Fn(Number) (contravariant — wider accepting subtype)"
        );
        assert!(
            !is_assignable(&db, narrow_param, wide_param),
            "Fn(Number) ≤ Fn(Union) must fail — String callers would slip through"
        );
    }

    #[test]
    fn function_mixed_variance() {
        let db = InMemoryDb::new();
        let from = fn_ty(
            &db,
            vec![db.union(vec![db.number(None, None), db.string(None, false)])],
            db.number(None, None),
        );
        let to = fn_ty(
            &db,
            vec![db.number(None, None)],
            db.union(vec![db.number(None, None), db.string(None, false)]),
        );
        assert!(is_assignable(&db, from, to));
        assert!(!is_assignable(&db, to, from));
    }

    #[test]
    fn tabular_row_metadata_ref_assignable_to_platform_object() {
        let db = InMemoryDb::new();
        let row = metadata_ref_id(
            &db,
            MetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Catalog },
            "X.Y",
        );
        let generic_ru = db.platform_object("Строка табличной части".to_string());
        let generic_en = db.platform_object("Line of a tabular section".to_string());
        assert!(is_assignable(&db, row, generic_ru));
        assert!(is_assignable(&db, row, generic_en));
    }

    #[test]
    fn tabular_row_platform_object_assignable_to_metadata_ref() {
        let db = InMemoryDb::new();
        let row = metadata_ref_id(
            &db,
            MetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Document },
            "X.Y",
        );
        let generic = db.platform_object("Строка табличной части".to_string());
        assert!(is_assignable(&db, generic, row));
    }

    #[test]
    fn tabular_row_bridge_does_not_open_unrelated_platform_objects() {
        let db = InMemoryDb::new();
        let row = metadata_ref_id(
            &db,
            MetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Catalog },
            "X.Y",
        );
        let unrelated = db.platform_object("ТаблицаЗначений".to_string());
        assert!(!is_assignable(&db, row, unrelated));
        assert!(!is_assignable(&db, unrelated, row));
    }

    #[test]
    fn coercible_anything_to_string() {
        let db = InMemoryDb::new();
        assert!(is_coercible_to(&db, db.number(None, None), db.string(None, false)));
        assert!(is_coercible_to(
            &db,
            db.date(bsl_types::facet::DateComponent::DateTime),
            db.string(None, false)
        ));
        assert!(is_coercible_to(&db, db.boolean(), db.string(None, false)));
        assert!(is_coercible_to(&db, db.null(), db.string(None, false)));
        assert!(is_coercible_to(&db, db.undefined(), db.string(None, false)));
        assert!(is_coercible_to(
            &db,
            db.union(vec![
                db.number(None, None),
                db.date(bsl_types::facet::DateComponent::DateTime)
            ]),
            db.string(None, false)
        ));
    }

    #[test]
    fn coercion_does_not_open_reverse_direction() {
        let db = InMemoryDb::new();
        assert!(!is_coercible_to(&db, db.string(None, false), db.number(None, None)));
        assert!(!is_coercible_to(
            &db,
            db.string(None, false),
            db.date(bsl_types::facet::DateComponent::DateTime)
        ));
        assert!(!is_coercible_to(&db, db.string(None, false), db.boolean()));
    }

    #[test]
    fn coercion_does_not_leak_into_is_assignable() {
        let db = InMemoryDb::new();
        assert!(!is_assignable(&db, db.number(None, None), db.string(None, false)));
        assert!(!is_assignable(
            &db,
            db.date(bsl_types::facet::DateComponent::DateTime),
            db.string(None, false)
        ));
        assert!(!is_assignable(
            &db,
            db.union(vec![
                db.number(None, None),
                db.date(bsl_types::facet::DateComponent::DateTime)
            ]),
            db.string(None, false)
        ));
    }

    #[test]
    fn typed_array_assignable_to_unparameterised_array() {
        let db = InMemoryDb::new();
        let typed = db.array(Some(db.string(None, false)));
        assert!(is_assignable(&db, typed, db.array(None)));
    }

    #[test]
    fn unparameterised_array_assignable_to_typed_array_gradual() {
        let db = InMemoryDb::new();
        assert!(is_assignable(&db, db.array(None), db.array(Some(db.number(None, None)))));
    }

    #[test]
    fn typed_array_covariant_on_element() {
        let db = InMemoryDb::new();
        let narrow = db.array(Some(db.number(None, None)));
        let wide = db.array(Some(db.union(vec![db.number(None, None), db.string(None, false)])));
        assert!(is_assignable(&db, narrow, wide), "TypedArray covariant: Number ≤ Number|String");
        assert!(
            !is_assignable(&db, wide, narrow),
            "TypedArray covariant: Number|String ≰ Number — String leg cannot satisfy Number callers"
        );
    }

    #[test]
    fn typed_array_unrelated_elements_rejected() {
        let db = InMemoryDb::new();
        let str_arr = db.array(Some(db.string(None, false)));
        let num_arr = db.array(Some(db.number(None, None)));
        assert!(!is_assignable(&db, str_arr, num_arr));
        assert!(!is_assignable(&db, num_arr, str_arr));
    }

    #[test]
    fn typed_array_reflexivity_holds() {
        let db = InMemoryDb::new();
        let ta = db.array(Some(db.string(None, false)));
        assert!(is_assignable(&db, ta, ta));
    }

    #[test]
    fn typed_array_bridge_does_not_open_unrelated_ty_pairs() {
        let db = InMemoryDb::new();
        assert!(!is_assignable(&db, db.string(None, false), db.array(None)));
        assert!(!is_assignable(&db, db.array(None), db.string(None, false)));
        assert!(!is_assignable(&db, db.number(None, None), db.array(Some(db.number(None, None)))));
    }

    #[test]
    fn function_unknown_short_circuit_wins_over_variance() {
        let db = InMemoryDb::new();
        let f = fn_ty(&db, vec![db.number(None, None)], db.boolean());
        assert!(is_assignable(&db, db.unknown(), f));
        assert!(is_assignable(&db, f, db.unknown()));
    }

    #[test]
    fn null_assignable_to_ref_types() {
        let db = InMemoryDb::new();
        let cat_ref = metadata_ref_id(&db, MetadataKind::CatalogRef, "Контрагенты");
        assert!(is_assignable(&db, db.null(), cat_ref));
        // Null is NOT assignable to a non-ref primitive.
        assert!(!is_assignable(&db, db.null(), db.number(None, None)));
    }

    #[test]
    fn any_is_universal_both_directions() {
        // `Произвольный` (kernel `Any`) is the universal type — these
        // arms are kernel-only (legacy `Ty` cannot express `Any`), so
        // we intern directly. Both `A ≤ Any` and `Any ≤ A` hold.
        let db = InMemoryDb::new();
        let any = db.any();
        let number = db.number(None, None);
        assert!(is_assignable(&db, number, any), "A ≤ Any (universal top)");
        assert!(is_assignable(&db, any, number), "Any ≤ A (universal, gradual)");
    }

    #[test]
    fn never_is_bottom() {
        // `Never` is the bottom / unreachable sentinel: `Never ≤ A` for
        // every `A`, but `A ≤ Never` is false unless reflexive.
        let db = InMemoryDb::new();
        let never = db.never();
        let number = db.number(None, None);
        assert!(is_assignable(&db, never, number), "Never ≤ A (bottom)");
        assert!(!is_assignable(&db, number, never), "A ≤ Never must fail (not reflexive)");
        assert!(is_assignable(&db, never, never), "Never ≤ Never (reflexive)");
    }
}
