//! Single source of truth for lowering raw HBK type-strings to [`Ty`].
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
use hir_def::ty::Ty;
use hir_def::Name;

/// Lower a raw HBK parameter-type string to a [`Ty`].
///
/// 1. Empty / whitespace-only string → `Ty::Unknown` (no claim).
/// 2. Any segment is the `Произвольный` placeholder → `Ty::Unknown`
///    (any-value collapses every union).
/// 3. Single segment → [`Ty::from_type_name`]: primitives become their
///    canonical variant, anything else stays `Ty::Unknown` so gradual
///    typing accepts any actual at the call site (the asymmetry).
/// 4. Multi-segment with every segment validated → `Ty::union(...)` of
///    [`lower_platform_type_name`] per segment.
/// 5. Multi-segment with any segment failing validation (prose-with-commas
///    or scraper garbage like `"Ссылка на объект, либо"`) → `Ty::Unknown`.
pub fn lower_param_type_string(raw: &str) -> Ty {
    let segments = split_type_alternatives(raw);
    if segments.is_empty() {
        return Ty::Unknown;
    }
    if segments.iter().any(|s| is_arbitrary_type_name(s)) {
        return Ty::Unknown;
    }
    if segments.len() == 1 {
        return Ty::from_type_name(segments[0]);
    }
    if segments.iter().all(|s| segment_is_valid_type(s)) {
        Ty::union(segments.iter().copied().map(lower_platform_type_name).collect())
    } else {
        Ty::Unknown
    }
}

/// Lower a raw HBK return-type string to a [`Ty`].
///
/// Mirrors [`lower_param_type_string`] but with three differences anchored
/// in the param/return asymmetry:
///
/// 1. Single segment is routed through [`lower_platform_type_name`], so
///    an unrecognised name lifts to `Ty::PlatformObject(name)` rather
///    than `Ty::Unknown` — chained receivers stay typed.
/// 2. Multi-segment with every segment validated → `Ty::union(...)` of
///    [`lower_platform_type_name`] per segment (same as the param path).
/// 3. Multi-segment with any segment failing validation → fallback to
///    `lower_platform_type_name(raw)` so the whole prose-with-commas
///    becomes a single `Ty::PlatformObject(<full raw>)`. This matches
///    the legacy `resolve_platform_type_union` fallback.
pub fn lower_return_type_string(raw: &str) -> Ty {
    let segments = split_type_alternatives(raw);
    if segments.is_empty() {
        return Ty::Unknown;
    }
    if segments.iter().any(|s| is_arbitrary_type_name(s)) {
        return Ty::Unknown;
    }
    if segments.iter().all(|s| segment_is_valid_type(s)) {
        Ty::union(segments.iter().copied().map(lower_platform_type_name).collect())
    } else {
        lower_platform_type_name(raw)
    }
}

/// Lower a single bare type-name token to a [`Ty`].
///
/// Primitives / collections (`"Число"`, `"Массив"`) take their canonical
/// variant via [`Ty::from_type_name`]; anything else lifts to
/// `Ty::PlatformObject(name)` so chained dispatch on platform objects
/// (`Запрос.Выполнить().Выбрать()`) keeps resolving.
///
/// `"Произвольный"` / `"Arbitrary"` collapses to [`Ty::Unknown`] — the
/// BSL "any value" placeholder must satisfy gradual typing in any typed
/// slot, and lifting it to `PlatformObject("Произвольный")` would let
/// structural equality false-fire `TypeMismatch` (see
/// [`crate::subtype::is_assignable`]).
pub fn lower_platform_type_name(name: &str) -> Ty {
    if is_arbitrary_type_name(name) {
        return Ty::Unknown;
    }
    let ty = Ty::from_type_name(name);
    if ty.is_unknown() {
        Ty::PlatformObject(Name::new(name))
    } else {
        ty
    }
}

/// `true` when the trimmed segment is a primitive / collection / sentinel
/// recognised by [`Ty::from_type_name`], or a registered platform type.
///
/// Used by both lowering entry points to gate union-versus-fallback —
/// a multi-segment string with even one invalid segment is
/// prose-with-commas, not a real type list.
pub fn segment_is_valid_type(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let ty = Ty::from_type_name(s);
    !ty.is_unknown() || PlatformData::instance().get_type(s).is_some()
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(lower_param_type_string("Число"), Ty::Number);
        assert_eq!(lower_param_type_string("Number"), Ty::Number);
    }

    #[test]
    fn lower_param_single_unknown_stays_unknown_for_gradual_typing() {
        // Single unrecognised name MUST stay Unknown: lifting to
        // `PlatformObject("Строка табличной части")` would let structural
        // equality false-fire `TypeMismatch` against legitimately looser
        // actuals at the call site.
        assert_eq!(lower_param_type_string("Строка табличной части"), Ty::Unknown);
    }

    #[test]
    fn lower_param_arbitrary_collapses_to_unknown() {
        assert_eq!(lower_param_type_string("Произвольный"), Ty::Unknown);
        assert_eq!(lower_param_type_string("Произвольный, Неопределено"), Ty::Unknown);
        assert_eq!(lower_param_type_string("Arbitrary, Undefined"), Ty::Unknown);
    }

    #[test]
    fn lower_param_multi_with_invalid_segment_collapses() {
        // Prose-with-commas (`"Ссылка на объект, либо"`) → not a real
        // union, gradual typing wins.
        assert_eq!(lower_param_type_string("Ссылка на объект, либо"), Ty::Unknown);
    }

    #[test]
    fn lower_param_multi_all_valid_lowers_to_union() {
        let ty = lower_param_type_string("Число, Строка");
        assert_eq!(ty, Ty::union(vec![Ty::Number, Ty::String]));
    }

    #[test]
    fn lower_param_semicolon_separator_activates_union() {
        // Headline `;`-activation: 1С HBK uses `;` as a sibling of `,`
        // for alternatives. Before unification this whole string lowered
        // to `Ty::Unknown`; now it surfaces as a real union.
        let ty = lower_param_type_string("Число; Строка");
        assert_eq!(ty, Ty::union(vec![Ty::Number, Ty::String]));
    }

    #[test]
    fn lower_param_mixed_comma_semicolon_with_trailing_garbage() {
        // `"Метаданные, Неопределено, УдалениеОбъекта, Массив ;"` is the
        // shape the HBK scraper actually emits. With `;`-aware splitting
        // and trailing-empty filtering, the four real segments survive —
        // assuming all are validated against `PlatformData`.
        let ty = lower_param_type_string("Число, Строка, Булево ;");
        assert_eq!(ty, Ty::union(vec![Ty::Number, Ty::String, Ty::Boolean]));
    }

    #[test]
    fn lower_return_single_unrecognised_lifts_to_platform_object() {
        // Asymmetry vs param: returns lift to PlatformObject so chained
        // receivers (`Запрос.Выполнить().Выбрать()`) keep typing.
        assert_eq!(lower_return_type_string("Запрос"), Ty::PlatformObject(Name::new("Запрос")),);
    }

    #[test]
    fn lower_return_multi_invalid_falls_back_to_raw_platform_object() {
        // Prose-with-commas in returns → lift the WHOLE raw string as a
        // single PlatformObject, mirroring legacy
        // `resolve_platform_type_union` fallback.
        let ty = lower_return_type_string("Ссылка на объект, либо");
        assert_eq!(ty, Ty::PlatformObject(Name::new("Ссылка на объект, либо")));
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
        assert_eq!(lower_return_type_string(raw), Ty::PlatformObject(Name::new(raw)));
    }

    #[test]
    fn lower_param_mixed_separator_with_prose_segment_collapses() {
        // Same raw string as the return test, but param-side: the asymmetry
        // gives `Ty::Unknown` (gradual typing wins for prose).
        let raw = "ТабличныйДокумент, ТекстовыйДокумент; другой объект";
        assert_eq!(lower_param_type_string(raw), Ty::Unknown);
    }

    #[test]
    fn lower_return_multi_all_valid_lowers_to_union() {
        let ty = lower_return_type_string("Булево, Неопределено");
        assert_eq!(ty, Ty::union(vec![Ty::Boolean, Ty::Undefined]));
    }

    #[test]
    fn lower_return_arbitrary_collapses() {
        assert_eq!(lower_return_type_string("Произвольный, Неопределено"), Ty::Unknown);
        assert_eq!(lower_return_type_string("  Произвольный  ,  Неопределено"), Ty::Unknown);
    }

    #[test]
    fn lower_platform_type_name_arbitrary_to_unknown() {
        assert_eq!(lower_platform_type_name("Произвольный"), Ty::Unknown);
        assert_eq!(lower_platform_type_name("Arbitrary"), Ty::Unknown);
    }

    #[test]
    fn lower_platform_type_name_unknown_lifts_to_platform_object() {
        assert_eq!(lower_platform_type_name("Запрос"), Ty::PlatformObject(Name::new("Запрос")),);
    }

    #[test]
    fn lower_platform_type_name_primitive_returns_canonical() {
        assert_eq!(lower_platform_type_name("Число"), Ty::Number);
    }
}
