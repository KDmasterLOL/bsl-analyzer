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
    // Phase 3 §4.E: the coercion table lives on legacy `Ty` (config
    // axis still flows through the `Ty ↔ TypeId` bridge). We bridge the
    // narrow `from` value through `coerce_this_object_to_metadata_ref`
    // and re-intern, so the comparison stays faithful to the canonical
    // form `to` was produced from. §4.F replaces this with a native
    // kernel coercion once `ThisObject` owners carry resolved configs.
    if matches!(from_kind, TypeKind::ThisObject { .. } | TypeKind::ThisManager { .. }) {
        let from_ty = crate::ty_bridge::typeid_to_ty(db, from);
        if let Some(coerced) = crate::coerce_this_object_to_metadata_ref(&from_ty) {
            if crate::ty_bridge::ty_to_typeid(db, &coerced) == to {
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
    use crate::ty_bridge::ty_to_typeid;
    use bsl_types::testing::InMemoryDb;
    use hir_def::ty::{MetadataKind as TyMetadataKind, Ty};

    fn id(db: &InMemoryDb, ty: &Ty) -> TypeId {
        ty_to_typeid(db, ty)
    }

    fn fn_ty(params: Vec<Ty>, ret: Ty) -> Ty {
        let max_args = Some(params.len() as u32);
        let defaults = vec![false; params.len()].into_boxed_slice();
        Ty::Function { params: params.into_boxed_slice(), defaults, max_args, ret: Box::new(ret) }
    }

    #[test]
    fn function_reflexive() {
        let db = InMemoryDb::new();
        let f = fn_ty(vec![Ty::Number, Ty::String], Ty::Boolean);
        assert!(is_assignable(&db, id(&db, &f), id(&db, &f)));
    }

    #[test]
    fn function_arity_mismatch_is_rejected() {
        let db = InMemoryDb::new();
        let one = fn_ty(vec![Ty::Number], Ty::Boolean);
        let two = fn_ty(vec![Ty::Number, Ty::String], Ty::Boolean);
        assert!(!is_assignable(&db, id(&db, &one), id(&db, &two)));
        assert!(!is_assignable(&db, id(&db, &two), id(&db, &one)));
    }

    #[test]
    fn function_covariant_return_widens() {
        let db = InMemoryDb::new();
        let narrow = fn_ty(vec![], Ty::Number);
        let wide = fn_ty(vec![], Ty::union(vec![Ty::Number, Ty::String]));
        assert!(
            is_assignable(&db, id(&db, &narrow), id(&db, &wide)),
            "Number return ≤ Union return (covariant widening)"
        );
        assert!(
            !is_assignable(&db, id(&db, &wide), id(&db, &narrow)),
            "Union return ≤ Number return must fail — String leg cannot satisfy Number callers"
        );
    }

    #[test]
    fn function_contravariant_param_widens() {
        let db = InMemoryDb::new();
        let wide_param = fn_ty(vec![Ty::union(vec![Ty::Number, Ty::String])], Ty::Boolean);
        let narrow_param = fn_ty(vec![Ty::Number], Ty::Boolean);
        assert!(
            is_assignable(&db, id(&db, &wide_param), id(&db, &narrow_param)),
            "Fn(Union) ≤ Fn(Number) (contravariant — wider accepting subtype)"
        );
        assert!(
            !is_assignable(&db, id(&db, &narrow_param), id(&db, &wide_param)),
            "Fn(Number) ≤ Fn(Union) must fail — String callers would slip through"
        );
    }

    #[test]
    fn function_mixed_variance() {
        let db = InMemoryDb::new();
        let from = fn_ty(vec![Ty::union(vec![Ty::Number, Ty::String])], Ty::Number);
        let to = fn_ty(vec![Ty::Number], Ty::union(vec![Ty::Number, Ty::String]));
        assert!(is_assignable(&db, id(&db, &from), id(&db, &to)));
        assert!(!is_assignable(&db, id(&db, &to), id(&db, &from)));
    }

    #[test]
    fn tabular_row_metadata_ref_assignable_to_platform_object() {
        let db = InMemoryDb::new();
        let row = Ty::MetadataRef {
            kind: TyMetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Catalog },
            name: hir_def::Name::new("X.Y"),
        };
        let generic_ru = Ty::PlatformObject(hir_def::Name::new("Строка табличной части"));
        let generic_en = Ty::PlatformObject(hir_def::Name::new("Line of a tabular section"));
        assert!(is_assignable(&db, id(&db, &row), id(&db, &generic_ru)));
        assert!(is_assignable(&db, id(&db, &row), id(&db, &generic_en)));
    }

    #[test]
    fn tabular_row_platform_object_assignable_to_metadata_ref() {
        let db = InMemoryDb::new();
        let row = Ty::MetadataRef {
            kind: TyMetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Document },
            name: hir_def::Name::new("X.Y"),
        };
        let generic = Ty::PlatformObject(hir_def::Name::new("Строка табличной части"));
        assert!(is_assignable(&db, id(&db, &generic), id(&db, &row)));
    }

    #[test]
    fn tabular_row_bridge_does_not_open_unrelated_platform_objects() {
        let db = InMemoryDb::new();
        let row = Ty::MetadataRef {
            kind: TyMetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Catalog },
            name: hir_def::Name::new("X.Y"),
        };
        let unrelated = Ty::PlatformObject(hir_def::Name::new("ТаблицаЗначений"));
        assert!(!is_assignable(&db, id(&db, &row), id(&db, &unrelated)));
        assert!(!is_assignable(&db, id(&db, &unrelated), id(&db, &row)));
    }

    #[test]
    fn coercible_anything_to_string() {
        let db = InMemoryDb::new();
        assert!(is_coercible_to(&db, id(&db, &Ty::Number), id(&db, &Ty::String)));
        assert!(is_coercible_to(&db, id(&db, &Ty::Date), id(&db, &Ty::String)));
        assert!(is_coercible_to(&db, id(&db, &Ty::Boolean), id(&db, &Ty::String)));
        assert!(is_coercible_to(&db, id(&db, &Ty::Null), id(&db, &Ty::String)));
        assert!(is_coercible_to(&db, id(&db, &Ty::Undefined), id(&db, &Ty::String)));
        assert!(is_coercible_to(
            &db,
            id(&db, &Ty::union(vec![Ty::Number, Ty::Date])),
            id(&db, &Ty::String)
        ));
    }

    #[test]
    fn coercion_does_not_open_reverse_direction() {
        let db = InMemoryDb::new();
        assert!(!is_coercible_to(&db, id(&db, &Ty::String), id(&db, &Ty::Number)));
        assert!(!is_coercible_to(&db, id(&db, &Ty::String), id(&db, &Ty::Date)));
        assert!(!is_coercible_to(&db, id(&db, &Ty::String), id(&db, &Ty::Boolean)));
    }

    #[test]
    fn coercion_does_not_leak_into_is_assignable() {
        let db = InMemoryDb::new();
        assert!(!is_assignable(&db, id(&db, &Ty::Number), id(&db, &Ty::String)));
        assert!(!is_assignable(&db, id(&db, &Ty::Date), id(&db, &Ty::String)));
        assert!(!is_assignable(
            &db,
            id(&db, &Ty::union(vec![Ty::Number, Ty::Date])),
            id(&db, &Ty::String)
        ));
    }

    #[test]
    fn typed_array_assignable_to_unparameterised_array() {
        let db = InMemoryDb::new();
        let typed = Ty::TypedArray(Box::new(Ty::String));
        assert!(is_assignable(&db, id(&db, &typed), id(&db, &Ty::Array)));
    }

    #[test]
    fn unparameterised_array_assignable_to_typed_array_gradual() {
        let db = InMemoryDb::new();
        assert!(is_assignable(
            &db,
            id(&db, &Ty::Array),
            id(&db, &Ty::TypedArray(Box::new(Ty::Number)))
        ));
    }

    #[test]
    fn typed_array_covariant_on_element() {
        let db = InMemoryDb::new();
        let narrow = Ty::TypedArray(Box::new(Ty::Number));
        let wide = Ty::TypedArray(Box::new(Ty::union(vec![Ty::Number, Ty::String])));
        assert!(
            is_assignable(&db, id(&db, &narrow), id(&db, &wide)),
            "TypedArray covariant: Number ≤ Number|String"
        );
        assert!(
            !is_assignable(&db, id(&db, &wide), id(&db, &narrow)),
            "TypedArray covariant: Number|String ≰ Number — String leg cannot satisfy Number callers"
        );
    }

    #[test]
    fn typed_array_unrelated_elements_rejected() {
        let db = InMemoryDb::new();
        let str_arr = Ty::TypedArray(Box::new(Ty::String));
        let num_arr = Ty::TypedArray(Box::new(Ty::Number));
        assert!(!is_assignable(&db, id(&db, &str_arr), id(&db, &num_arr)));
        assert!(!is_assignable(&db, id(&db, &num_arr), id(&db, &str_arr)));
    }

    #[test]
    fn typed_array_reflexivity_holds() {
        let db = InMemoryDb::new();
        let ta = Ty::TypedArray(Box::new(Ty::String));
        assert!(is_assignable(&db, id(&db, &ta), id(&db, &ta)));
    }

    #[test]
    fn typed_array_bridge_does_not_open_unrelated_ty_pairs() {
        let db = InMemoryDb::new();
        assert!(!is_assignable(&db, id(&db, &Ty::String), id(&db, &Ty::Array)));
        assert!(!is_assignable(&db, id(&db, &Ty::Array), id(&db, &Ty::String)));
        assert!(!is_assignable(
            &db,
            id(&db, &Ty::Number),
            id(&db, &Ty::TypedArray(Box::new(Ty::Number)))
        ));
    }

    #[test]
    fn function_unknown_short_circuit_wins_over_variance() {
        let db = InMemoryDb::new();
        let f = fn_ty(vec![Ty::Number], Ty::Boolean);
        assert!(is_assignable(&db, id(&db, &Ty::Unknown), id(&db, &f)));
        assert!(is_assignable(&db, id(&db, &f), id(&db, &Ty::Unknown)));
    }

    #[test]
    fn null_assignable_to_ref_types() {
        let db = InMemoryDb::new();
        let cat_ref = Ty::MetadataRef {
            kind: TyMetadataKind::CatalogRef,
            name: hir_def::Name::new("Контрагенты"),
        };
        assert!(is_assignable(&db, id(&db, &Ty::Null), id(&db, &cat_ref)));
        // Null is NOT assignable to a non-ref primitive.
        assert!(!is_assignable(&db, id(&db, &Ty::Null), id(&db, &Ty::Number)));
    }

    #[test]
    fn any_is_universal_both_directions() {
        // `Произвольный` (kernel `Any`) is the universal type — these
        // arms are kernel-only (legacy `Ty` cannot express `Any`), so
        // we intern directly. Both `A ≤ Any` and `Any ≤ A` hold.
        let db = InMemoryDb::new();
        let any = db.any();
        let number = id(&db, &Ty::Number);
        assert!(is_assignable(&db, number, any), "A ≤ Any (universal top)");
        assert!(is_assignable(&db, any, number), "Any ≤ A (universal, gradual)");
    }

    #[test]
    fn never_is_bottom() {
        // `Never` is the bottom / unreachable sentinel: `Never ≤ A` for
        // every `A`, but `A ≤ Never` is false unless reflexive.
        let db = InMemoryDb::new();
        let never = db.never();
        let number = id(&db, &Ty::Number);
        assert!(is_assignable(&db, never, number), "Never ≤ A (bottom)");
        assert!(!is_assignable(&db, number, never), "A ≤ Never must fail (not reflexive)");
        assert!(is_assignable(&db, never, never), "Never ≤ Never (reflexive)");
    }
}
