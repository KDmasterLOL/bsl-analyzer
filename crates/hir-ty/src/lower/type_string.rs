//! Single source of truth for lowering raw HBK type-strings to `TypeId`.
//!
//! The platform JSON encodes parameter, return, and property types as
//! free-form strings: `"Число"`, `"Булево, Неопределено"`,
//! `"Форма ; Элемент управления"`. Three call sites used to fan out into
//! ad-hoc copies of the same logic — `method_lookup::lower_param_type`,
//! `method_lookup::resolve_platform_type_union`, and
//! `builtin::map_type_string` / `map_return_type`. This module is the
//! consolidated pipeline shared across them; see plan §2.1–§2.3.
//!
//! # Asymmetry between params and returns
//!
//! BSL platform argument checks live on the **left** of `is_assignable`,
//! and structural equality on `Ty::PlatformObject` would false-fire
//! `TypeMismatch` against legitimately looser actuals. So a single
//! unrecognised param token stays `Ty::Unknown` (gradual typing). Returns
//! live on the **right** — chained receivers (`Запрос.Выполнить()`)
//! must keep their typed shape, so a single unrecognised return token
//! lifts to `Ty::PlatformObject(name)` instead.
//!
//! Multi-segment unions distribute across both sides identically. The
//! `Произвольный` / `Arbitrary` placeholder always collapses the whole
//! union to `Ty::Unknown` because the gradual rule must dominate
//! every typed sink.

use bsl_platform::{split_type_alternatives, PlatformData};
use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::type_ref::TypeRef;

use super::builtin_names::bare_name_to_typeid;

/// `true` when the trimmed segment is a primitive / collection / sentinel
/// recognised by [`TypeRef::from_bare_name`], or a registered platform type.
///
/// Used by both lowering entry points to gate union-versus-fallback —
/// a multi-segment string with even one invalid segment is
/// prose-with-commas, not a real type list.
pub fn segment_is_valid_type(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    TypeRef::from_bare_name(s).is_some() || PlatformData::instance().get_type(s).is_some()
}

/// `true` when the trimmed name is the BSL "any value" placeholder.
///
/// Cyrillic case folding requires `to_lowercase` — `eq_ignore_ascii_case`
/// only normalises ASCII bytes, so a future scraper-emitted
/// `"произвольный"` would slip through.
pub fn is_arbitrary_type_name(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.eq_ignore_ascii_case("Arbitrary") || trimmed.to_lowercase() == "произвольный"
}

// ── §4.A kernel-native counterparts ──────────────────────────
//
// Mint `TypeId` unions directly via [`Builders`].

/// Lower a raw HBK parameter-type string directly into a kernel [`TypeId`].
pub fn lower_param_type_string_typeid(db: &dyn TypeKernelDb, raw: &str) -> TypeId {
    let segments = split_type_alternatives(raw);
    if segments.is_empty() {
        return db.unknown();
    }
    if segments.iter().any(|s| is_arbitrary_type_name(s)) {
        return db.unknown();
    }
    if segments.len() == 1 {
        return bare_name_to_typeid(db, segments[0]);
    }
    if segments.iter().all(|s| segment_is_valid_type(s)) {
        db.union(segments.iter().map(|s| lower_platform_type_name_typeid(db, s)).collect())
    } else {
        db.unknown()
    }
}

/// Lower a raw HBK return-type string directly into a kernel [`TypeId`].
pub fn lower_return_type_string_typeid(db: &dyn TypeKernelDb, raw: &str) -> TypeId {
    let segments = split_type_alternatives(raw);
    if segments.is_empty() {
        return db.unknown();
    }
    if segments.iter().any(|s| is_arbitrary_type_name(s)) {
        return db.unknown();
    }
    if segments.iter().all(|s| segment_is_valid_type(s)) {
        db.union(segments.iter().map(|s| lower_platform_type_name_typeid(db, s)).collect())
    } else {
        lower_platform_type_name_typeid(db, raw)
    }
}

/// Lower a single bare type-name token directly into a kernel [`TypeId`].
pub fn lower_platform_type_name_typeid(db: &dyn TypeKernelDb, name: &str) -> TypeId {
    if is_arbitrary_type_name(name) {
        return db.unknown();
    }
    let id = bare_name_to_typeid(db, name);
    if id == db.unknown() {
        db.platform_object(name.to_string())
    } else {
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::testing::InMemoryDb;

    #[test]
    fn type_string_typeid_covers_core_branches() {
        let db = InMemoryDb::new();
        assert_eq!(lower_param_type_string_typeid(&db, ""), db.unknown());
        assert_eq!(lower_param_type_string_typeid(&db, "Произвольный"), db.unknown());
        assert_eq!(lower_param_type_string_typeid(&db, "Число"), db.number(None, None));
        assert_eq!(lower_param_type_string_typeid(&db, "Строка табличной части"), db.unknown());
        assert_eq!(
            lower_param_type_string_typeid(&db, "Число, Строка"),
            db.union(vec![db.number(None, None), db.string(None, false)])
        );
        assert_eq!(
            lower_return_type_string_typeid(&db, "Ссылка на объект, либо"),
            db.platform_object("Ссылка на объект, либо".to_string())
        );
        assert_eq!(
            lower_platform_type_name_typeid(&db, "Запрос"),
            db.platform_object("Запрос".to_string())
        );
    }

    #[test]
    fn split_handles_comma_semicolon_and_trailing_garbage() {
        assert_eq!(split_type_alternatives("Число"), vec!["Число"]);
        assert_eq!(split_type_alternatives("Число, Строка"), vec!["Число", "Строка"]);
        assert_eq!(split_type_alternatives("Форма ; Элемент"), vec!["Форма", "Элемент"]);
        // Mixed separators with a stray trailing `;`.
        assert_eq!(split_type_alternatives("Метаданные, Массив ;"), vec!["Метаданные", "Массив"],);
        // All separators / whitespace → empty after filter.
        assert!(split_type_alternatives(", ,").is_empty());
        assert!(split_type_alternatives("").is_empty());
    }

    #[test]
    fn lower_param_single_primitive() {
        let db = InMemoryDb::new();
        assert_eq!(lower_param_type_string_typeid(&db, "Число"), db.number(None, None));
        assert_eq!(lower_param_type_string_typeid(&db, "Number"), db.number(None, None));
    }

    #[test]
    fn lower_param_single_unknown_stays_unknown_for_gradual_typing() {
        // Single unrecognised name MUST stay Unknown: lifting to
        // `PlatformObject("Строка табличной части")` would let structural
        // equality false-fire `TypeMismatch` against legitimately looser
        // actuals at the call site.
        let db = InMemoryDb::new();
        assert_eq!(lower_param_type_string_typeid(&db, "Строка табличной части"), db.unknown());
    }

    #[test]
    fn lower_param_arbitrary_collapses_to_unknown() {
        let db = InMemoryDb::new();
        assert_eq!(lower_param_type_string_typeid(&db, "Произвольный"), db.unknown());
        assert_eq!(lower_param_type_string_typeid(&db, "Произвольный, Неопределено"), db.unknown());
        assert_eq!(lower_param_type_string_typeid(&db, "Arbitrary, Undefined"), db.unknown());
    }

    #[test]
    fn lower_param_multi_with_invalid_segment_collapses() {
        // Prose-with-commas (`"Ссылка на объект, либо"`) → not a real
        // union, gradual typing wins.
        let db = InMemoryDb::new();
        assert_eq!(lower_param_type_string_typeid(&db, "Ссылка на объект, либо"), db.unknown());
    }

    #[test]
    fn lower_param_multi_all_valid_lowers_to_union() {
        let db = InMemoryDb::new();
        assert_eq!(
            lower_param_type_string_typeid(&db, "Число, Строка"),
            db.union(vec![db.number(None, None), db.string(None, false)])
        );
    }

    #[test]
    fn lower_param_semicolon_separator_activates_union() {
        // Headline `;`-activation: 1С HBK uses `;` as a sibling of `,`
        // for alternatives. Before unification this whole string lowered
        // to `Ty::Unknown`; now it surfaces as a real union.
        let db = InMemoryDb::new();
        assert_eq!(
            lower_param_type_string_typeid(&db, "Число; Строка"),
            db.union(vec![db.number(None, None), db.string(None, false)])
        );
    }

    #[test]
    fn lower_param_mixed_comma_semicolon_with_trailing_garbage() {
        // `"Метаданные, Неопределено, УдалениеОбъекта, Массив ;"` is the
        // shape the HBK scraper actually emits. With `;`-aware splitting
        // and trailing-empty filtering, the four real segments survive —
        // assuming all are validated against `PlatformData`.
        let db = InMemoryDb::new();
        assert_eq!(
            lower_param_type_string_typeid(&db, "Число, Строка, Булево ;"),
            db.union(vec![db.number(None, None), db.string(None, false), db.boolean()])
        );
    }

    #[test]
    fn lower_return_single_unrecognised_lifts_to_platform_object() {
        // Asymmetry vs param: returns lift to PlatformObject so chained
        // receivers (`Запрос.Выполнить().Выбрать()`) keep typing.
        let db = InMemoryDb::new();
        assert_eq!(
            lower_return_type_string_typeid(&db, "Запрос"),
            db.platform_object("Запрос".to_string())
        );
    }

    #[test]
    fn lower_return_multi_invalid_falls_back_to_raw_platform_object() {
        // Prose-with-commas in returns → lift the WHOLE raw string as a
        // single PlatformObject, mirroring legacy
        // `resolve_platform_type_union` fallback.
        let db = InMemoryDb::new();
        assert_eq!(
            lower_return_type_string_typeid(&db, "Ссылка на объект, либо"),
            db.platform_object("Ссылка на объект, либо".to_string())
        );
    }

    #[test]
    fn lower_return_mixed_separator_with_prose_segment_falls_back() {
        // Prose with a `;` mixed in: `"ТабличныйДокумент, ТекстовыйДокумент; другой объект"`
        // splits to three segments; "другой объект" is multi-word lowercase
        // and not a registered platform type — invalid. The whole raw string
        // lifts to a single `PlatformObject`. Pins the contract for a real
        // shape platform_data emits and that the previous
        // `resolve_platform_type_union_falls_back_for_prose_commas` test
        // covered before consolidation.
        let raw = "ТабличныйДокумент, ТекстовыйДокумент; другой объект";
        let db = InMemoryDb::new();
        assert_eq!(lower_return_type_string_typeid(&db, raw), db.platform_object(raw.to_string()));
    }

    #[test]
    fn lower_param_mixed_separator_with_prose_segment_collapses() {
        // Same raw string as the return test, but param-side: the asymmetry
        // gives `Ty::Unknown` (gradual typing wins for prose).
        let raw = "ТабличныйДокумент, ТекстовыйДокумент; другой объект";
        let db = InMemoryDb::new();
        assert_eq!(lower_param_type_string_typeid(&db, raw), db.unknown());
    }

    #[test]
    fn lower_return_multi_all_valid_lowers_to_union() {
        let db = InMemoryDb::new();
        assert_eq!(
            lower_return_type_string_typeid(&db, "Булево, Неопределено"),
            db.union(vec![db.boolean(), db.undefined()])
        );
    }

    #[test]
    fn lower_return_arbitrary_collapses() {
        let db = InMemoryDb::new();
        assert_eq!(
            lower_return_type_string_typeid(&db, "Произвольный, Неопределено"),
            db.unknown()
        );
        assert_eq!(
            lower_return_type_string_typeid(&db, "  Произвольный  ,  Неопределено"),
            db.unknown()
        );
    }

    #[test]
    fn lower_platform_type_name_arbitrary_to_unknown() {
        let db = InMemoryDb::new();
        assert_eq!(lower_platform_type_name_typeid(&db, "Произвольный"), db.unknown());
        assert_eq!(lower_platform_type_name_typeid(&db, "Arbitrary"), db.unknown());
    }

    #[test]
    fn lower_platform_type_name_unknown_lifts_to_platform_object() {
        let db = InMemoryDb::new();
        assert_eq!(
            lower_platform_type_name_typeid(&db, "Запрос"),
            db.platform_object("Запрос".to_string())
        );
    }

    #[test]
    fn lower_platform_type_name_primitive_returns_canonical() {
        let db = InMemoryDb::new();
        assert_eq!(lower_platform_type_name_typeid(&db, "Число"), db.number(None, None));
    }

    /// Audit: walk every `;`-bearing `param_type` / `return_type` in the
    /// loaded platform data and confirm a non-zero fraction now lowers
    /// to a non-`Unknown` `Ty`. Plan §2.1 finalisation: the headline
    /// `;`-activation fix turns previously-`Ty::Unknown` collapses into
    /// real `Ty::Union`s for entries whose every segment is a registered
    /// platform type.
    ///
    /// The test is intentionally robust against minor platform-data
    /// drift: it asserts that **at least 1 entry** lowers to non-Unknown
    /// rather than pinning an exact count. Pre-fix this number was
    /// zero (every `;`-entry collapsed); post-fix it is positive.
    #[test]
    fn semicolon_separator_activation_audit() {
        let data = PlatformData::instance();
        if data.all_types().is_empty() {
            // Some build environments ship without platform data; the
            // unit-level tests above already pin the contract.
            return;
        }

        let mut total = 0_usize;
        let mut non_unknown = 0_usize;
        for ty in data.all_types() {
            for method in data.get_type_methods(&ty.name) {
                for param in &method.parameters {
                    if let Some(pt) = &param.param_type {
                        if pt.contains(';') {
                            total += 1;
                            let db = InMemoryDb::new();
                            if lower_param_type_string_typeid(&db, pt) != db.unknown() {
                                non_unknown += 1;
                            }
                        }
                    }
                }
                if let Some(ret) = &method.return_type {
                    if ret.contains(';') {
                        total += 1;
                        let db = InMemoryDb::new();
                        if lower_return_type_string_typeid(&db, ret) != db.unknown() {
                            non_unknown += 1;
                        }
                    }
                }
            }
        }

        assert!(
            total > 0,
            "Expected platform data to contain at least one `;`-bearing type string",
        );
        assert!(
            non_unknown > 0,
            "Expected `;`-activation to lower at least one entry to non-Unknown — got {non_unknown}/{total}",
        );
    }
}
