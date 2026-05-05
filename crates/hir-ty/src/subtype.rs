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

    // TabularSectionRow ↔ `Ty::PlatformObject("Строка табличной части")`
    // bridge (bidirectional). The TS method bridge in `method_lookup`
    // rebinds the platform return `"Строка табличной части"` to a
    // concrete `Ty::MetadataRef { TabularSectionRow { parent }, name }`
    // so chained `.<row attribute>` resolves through `field_lookup`.
    // Without this rule, structural equality on `MetadataRef` would
    // reject every legitimate transfer between the concrete row and
    // the generic platform-object form — including:
    //   - user-procedure JSDoc params typed `Строка табличной части`,
    //     which lower to `Ty::PlatformObject("Строка табличной части")`;
    //   - row values stored in fields / collections that erase the
    //     `(parent, name)` payload back to the platform name.
    // The two forms denote the same BSL value, so the subtype
    // relation is symmetric.
    if is_tabular_row_bridge(from, to) {
        return true;
    }

    // Array ↔ TypedArray bridge (bidirectional) + covariant element.
    //
    // `Ty::Array` and `Ty::TypedArray(_)` denote the same BSL value
    // — a runtime array — but differ in whether an element witness
    // is recoverable. `Новый Массив(...)` literals always lower to
    // unparameterised `Ty::Array`; JSDoc `Массив из X` and the
    // form-control refinement (`Элементы.X.ВыделенныеСтроки`,
    // Phase 5) produce `Ty::TypedArray(X)`. The two forms must
    // cross the slot boundary freely:
    //
    // - `TypedArray(X) ≤ Array` — drop the element witness at the
    //   slot. Always safe; the runtime value is still an array and
    //   the receiver of a bare `Массив` does not consult elements.
    // - `Array ≤ TypedArray(X)` — accept a witness-less array where
    //   a typed array is expected. BSL has no element-typed runtime,
    //   so a bare `Новый Массив` may legally hold any element; the
    //   gradual stance (same rationale as `Unknown ≤ A` above) keeps
    //   the diagnostic permissive on under-annotated code. A future
    //   strict mode would flip this to `false`.
    // - `TypedArray(A) ≤ TypedArray(B)` — covariant on the element
    //   type. Matches the function-return-covariance rule: an
    //   "array of A" satisfies "array of B" iff every `A ≤ B`. Pure
    //   invariance would reject `TypedArray(Number) ≤
    //   TypedArray(Number | String)`, which is the exact JSDoc-
    //   widening pattern this variant was introduced to support.
    //
    // Without this branch, lowering a JSDoc `Массив из Строка`
    // through `Ty::TypedArray(String)` and passing the result to an
    // unannotated callee (whose param Ty stays `Ty::Array` at M4)
    // would fire a spurious `TypeMismatch` — Phase 0 must not
    // regress assignability for already-working code.
    if is_array_typed_array_bridge(from, to) {
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
        Ty::Function { params: from_params, ret: from_ret, .. },
        Ty::Function { params: to_params, ret: to_ret, .. },
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

/// Symmetric bridge between the concrete row receiver
/// `Ty::MetadataRef { TabularSectionRow { _ }, _ }` and the generic
/// platform-object form `Ty::PlatformObject("Строка табличной части")`
/// (and its English alias `"Line of a tabular section"`).
///
/// Both denote the same BSL value: the row receiver carries a
/// `(parent, "Parent.Section")` payload so attribute access can find
/// the right XML attributes, but the platform's own type system
/// describes "any tabular-section row" with the bare type name.
/// Subtyping must accept conversion in either direction so that:
///
/// - a row from `ТЧ.Добавить()` can be passed where a JSDoc-typed
///   parameter declared `Строка табличной части` is expected
///   (`MetadataRef → PlatformObject`);
/// - a value typed `Строка табличной части` (e.g. coming back through
///   a field-eraser) can be passed where a concrete row is expected
///   (`PlatformObject → MetadataRef`).
fn is_tabular_row_bridge(a: &Ty, b: &Ty) -> bool {
    fn is_row_metadata_ref(ty: &Ty) -> bool {
        matches!(ty, Ty::MetadataRef { kind: MetadataKind::TabularSectionRow { .. }, .. })
    }
    fn is_row_platform_object(ty: &Ty) -> bool {
        matches!(ty, Ty::PlatformObject(name)
            if name.as_str().eq_ignore_ascii_case("Line of a tabular section")
                || name.as_str().to_lowercase() == "строка табличной части")
    }
    (is_row_metadata_ref(a) && is_row_platform_object(b))
        || (is_row_platform_object(a) && is_row_metadata_ref(b))
}

/// Bidirectional bridge between [`Ty::Array`] and [`Ty::TypedArray`]
/// + covariant element comparison for `TypedArray → TypedArray`.
///
/// See the call site in [`is_assignable`] for the full rationale.
/// Recurses through [`is_assignable`] on the element pair so that
/// `Ty::union` / `Ty::Unknown` rules apply uniformly.
fn is_array_typed_array_bridge(from: &Ty, to: &Ty) -> bool {
    match (from, to) {
        (Ty::TypedArray(_), Ty::Array) | (Ty::Array, Ty::TypedArray(_)) => true,
        (Ty::TypedArray(a), Ty::TypedArray(b)) => is_assignable(a, b),
        _ => false,
    }
}

/// Argument-position coercion check.
///
/// Equivalent to [`is_assignable`] except that **any** value coerces
/// to a bare [`Ty::String`] target. BSL implicitly stringifies values
/// when a String slot is expected (`СтрШаблон`, `Сообщить`, log /
/// trace writers, exception messages, …), so firing `TypeMismatch`
/// on `Number → String` or `Union(Date, Number) → String` produces
/// noise on real code without representing a runtime bug.
///
/// **Scope is intentionally narrow.** Call only from the call-site
/// argument-mismatch emitter ([`crate::arg_diagnostics`]). All other
/// subtype users — overload selection in [`crate::infer`], function
/// variance, the public `hir::Type::is_assignable_to` facade — must
/// keep using [`is_assignable`] so that:
///
/// - Overload picking in `infer_query` does **not** start preferring
///   a String-accepting overload for non-String actuals (that would
///   change inferred return types and cascade into hover / completion
///   / downstream type flow).
/// - `Fn(String)` does not silently start satisfying `Fn(Number)`
///   slots through param contravariance.
/// - The reverse direction (`String → Number`, `String → Date`, …)
///   stays a real diagnostic — strings flowing into typed sinks is
///   still a bug worth surfacing.
///
/// The rule fires only when `to` is bare [`Ty::String`] at the top
/// level. A `Union(String, X)` slot keeps the existing union-right
/// distribution; extend here with a recursive walk if real-code
/// signal shows that's also too strict.
pub fn is_coercible_to(from: &Ty, to: &Ty) -> bool {
    if matches!(to, Ty::String) {
        return true;
    }
    is_assignable(from, to)
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
        // Tests in this module focus on params/ret variance only; defaults
        // and max_args do not participate in the function subtype rule
        // (`is_assignable` ignores them).
        let max_args = Some(params.len() as u32);
        let defaults = vec![false; params.len()].into_boxed_slice();
        Ty::Function { params: params.into_boxed_slice(), defaults, max_args, ret: Box::new(ret) }
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
    fn tabular_row_metadata_ref_assignable_to_platform_object() {
        // The TS method bridge produces a concrete row receiver
        // `MetadataRef { TabularSectionRow { Catalog }, "X.Y" }` from
        // `ТЧ.Добавить()`. Passing it where a JSDoc-declared
        // `Строка табличной части` (lowered to `Ty::PlatformObject`)
        // is expected must NOT fire `TypeMismatch` — both denote the
        // same BSL value.
        let row = Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Catalog },
            name: hir_def::Name::new("X.Y"),
        };
        let generic_ru = Ty::PlatformObject(hir_def::Name::new("Строка табличной части"));
        let generic_en = Ty::PlatformObject(hir_def::Name::new("Line of a tabular section"));
        assert!(is_assignable(&row, &generic_ru));
        assert!(is_assignable(&row, &generic_en));
    }

    #[test]
    fn tabular_row_platform_object_assignable_to_metadata_ref() {
        // Reverse direction: a `Ty::PlatformObject("Строка табличной
        // части")` value (e.g. from a field-eraser path) flows into a
        // slot expecting a concrete row receiver. Symmetric bridge
        // accepts both directions because BSL has no observable
        // distinction between them.
        let row = Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Document },
            name: hir_def::Name::new("X.Y"),
        };
        let generic = Ty::PlatformObject(hir_def::Name::new("Строка табличной части"));
        assert!(is_assignable(&generic, &row));
    }

    #[test]
    fn tabular_row_bridge_does_not_open_unrelated_platform_objects() {
        // The bridge is intentionally narrow: only the two row
        // platform-name spellings activate. Other `PlatformObject`
        // values must NOT silently flow into a row receiver — that
        // would erase real type errors.
        let row = Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Catalog },
            name: hir_def::Name::new("X.Y"),
        };
        let unrelated = Ty::PlatformObject(hir_def::Name::new("ТаблицаЗначений"));
        assert!(!is_assignable(&row, &unrelated));
        assert!(!is_assignable(&unrelated, &row));
    }

    #[test]
    fn coercible_anything_to_string() {
        // BSL implicitly stringifies any value that lands in a String
        // slot. Pin both the simple primitive case and the Union case
        // — the latter is the actual production trigger
        // (`СтрШаблон(value)` where `value`'s inferred type is a
        // disjunction of branch results) and would otherwise have to
        // satisfy the strict union-left rule (every component ≤ String).
        assert!(is_coercible_to(&Ty::Number, &Ty::String));
        assert!(is_coercible_to(&Ty::Date, &Ty::String));
        assert!(is_coercible_to(&Ty::Boolean, &Ty::String));
        assert!(is_coercible_to(&Ty::Null, &Ty::String));
        // `Undefined → String` matters because BSL's `СтрШаблон("…%1…",
        // Undefined)` renders to "Undefined" without erroring; pinning
        // it explicitly makes the bare `Ty::String` short-circuit's
        // intent unambiguous instead of relying on the catch-all.
        assert!(is_coercible_to(&Ty::Undefined, &Ty::String));
        assert!(is_coercible_to(&Ty::union(vec![Ty::Number, Ty::Date]), &Ty::String));
    }

    #[test]
    fn coercion_does_not_open_reverse_direction() {
        // Regression guard: the rule is one-way. `String → Number` /
        // `String → Date` must stay rejected — a string flowing into
        // a typed sink is still a real bug worth surfacing, and the
        // structural-equality fallthrough at the bottom of
        // `is_assignable` is what catches it.
        assert!(!is_coercible_to(&Ty::String, &Ty::Number));
        assert!(!is_coercible_to(&Ty::String, &Ty::Date));
        assert!(!is_coercible_to(&Ty::String, &Ty::Boolean));
    }

    #[test]
    fn coercion_does_not_leak_into_is_assignable() {
        // Critical isolation check: the coercion rule must NOT bleed
        // into `is_assignable`. Overload selection in `infer_query`
        // and the public `hir::Type::is_assignable_to` facade both
        // depend on `is_assignable` rejecting `Number → String` —
        // letting it through would change which overload wins for
        // non-String actuals and silently shift inferred return types
        // through hover / completion / downstream type flow. This
        // test fires first if a future refactor accidentally folds
        // `is_coercible_to` back into `is_assignable`.
        assert!(!is_assignable(&Ty::Number, &Ty::String));
        assert!(!is_assignable(&Ty::Date, &Ty::String));
        assert!(!is_assignable(&Ty::union(vec![Ty::Number, Ty::Date]), &Ty::String));
    }

    #[test]
    fn typed_array_assignable_to_unparameterised_array() {
        // Drop-the-witness direction: a `TypedArray(X)` value must
        // satisfy a bare `Massiv` slot. Otherwise a JSDoc-typed
        // `Массив из Строка` returned from one helper would fire
        // `TypeMismatch` when passed to an unannotated callee whose
        // param Ty stays `Ty::Array`.
        let typed = Ty::TypedArray(Box::new(Ty::String));
        assert!(is_assignable(&typed, &Ty::Array));
    }

    #[test]
    fn unparameterised_array_assignable_to_typed_array_gradual() {
        // Gradual direction: `Новый Массив` (Ty::Array, no element
        // witness) must satisfy a `TypedArray(X)` slot — BSL arrays
        // are heterogeneous at runtime, and rejecting this would
        // fire false positives across every untyped helper that
        // builds an array literal and hands it to a JSDoc-annotated
        // callee. Same gradual stance as `Unknown ≤ A`.
        assert!(is_assignable(&Ty::Array, &Ty::TypedArray(Box::new(Ty::Number))));
    }

    #[test]
    fn typed_array_covariant_on_element() {
        // Element covariance: `TypedArray(Number)` satisfies
        // `TypedArray(Number | String)` because every Number is a
        // valid member of the wider union. Matches the
        // function-return covariance rule and is the exact
        // JSDoc-widening pattern this variant was introduced to
        // support.
        let narrow = Ty::TypedArray(Box::new(Ty::Number));
        let wide = Ty::TypedArray(Box::new(Ty::union(vec![Ty::Number, Ty::String])));
        assert!(is_assignable(&narrow, &wide), "TypedArray covariant: Number ≤ Number|String");
        assert!(
            !is_assignable(&wide, &narrow),
            "TypedArray covariant: Number|String ≰ Number — String leg cannot satisfy Number callers"
        );
    }

    #[test]
    fn typed_array_unrelated_elements_rejected() {
        // Element rule must reject genuinely unrelated types. A
        // permissive bridge that always accepts would erase real
        // type errors (`TypedArray(String) → TypedArray(Number)`
        // is a bug worth surfacing).
        let str_arr = Ty::TypedArray(Box::new(Ty::String));
        let num_arr = Ty::TypedArray(Box::new(Ty::Number));
        assert!(!is_assignable(&str_arr, &num_arr));
        assert!(!is_assignable(&num_arr, &str_arr));
    }

    #[test]
    fn typed_array_reflexivity_holds() {
        // Sanity: identical TypedArray instances must still be
        // assignable. The new bridge recurses through `is_assignable`
        // for the element pair, which catches reflexivity at the
        // bottom of the recursive call.
        let ta = Ty::TypedArray(Box::new(Ty::String));
        assert!(is_assignable(&ta, &ta));
    }

    #[test]
    fn typed_array_bridge_does_not_open_unrelated_ty_pairs() {
        // The bridge is intentionally narrow: only `Array` /
        // `TypedArray` cells activate. Non-array `Ty` pairs must
        // not start passing assignability through this branch.
        assert!(!is_assignable(&Ty::String, &Ty::Array));
        assert!(!is_assignable(&Ty::Array, &Ty::String));
        assert!(!is_assignable(&Ty::Number, &Ty::TypedArray(Box::new(Ty::Number))));
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
