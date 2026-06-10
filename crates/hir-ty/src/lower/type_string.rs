use bsl_platform::{split_type_alternatives, PlatformData};
use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::type_ref::TypeRef;

use super::builtin_names::bare_name_to_typeid;

pub fn segment_is_valid_type(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    TypeRef::from_bare_name(s).is_some() || PlatformData::instance().get_type(s).is_some()
}

pub fn is_arbitrary_type_name(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.eq_ignore_ascii_case("Arbitrary") || trimmed.to_lowercase() == "произвольный"
}

pub fn lower_param_type_string_typeid(db: &dyn TypeKernelDb, raw: &str) -> TypeId {
    let segments = split_type_alternatives(raw);
    if segments.is_empty() {
        return db.unknown();
    }
    if segments.iter().any(|s| is_arbitrary_type_name(s)) {
        return db.any();
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

pub fn lower_return_type_string_typeid(db: &dyn TypeKernelDb, raw: &str) -> TypeId {
    let segments = split_type_alternatives(raw);
    if segments.is_empty() {
        return db.unknown();
    }
    // A documented «Произвольный» is a contract, not missing information:
    // it lowers to the sticky Any so signature enrichment never replaces it
    // with a body-inferred type.
    if segments.iter().any(|s| is_arbitrary_type_name(s)) {
        return db.any();
    }
    if segments.iter().all(|s| segment_is_valid_type(s)) {
        db.union(segments.iter().map(|s| lower_platform_type_name_typeid(db, s)).collect())
    } else {
        lower_platform_type_name_typeid(db, raw)
    }
}

pub fn lower_platform_type_name_typeid(db: &dyn TypeKernelDb, name: &str) -> TypeId {
    if is_arbitrary_type_name(name) {
        return db.any();
    }
    let id = bare_name_to_typeid(db, name);
    if id != db.unknown() {
        return id;
    }
    // A PlatformObject participates in nominal comparisons, so its name must be
    // a real type: doc comments and platform dumps put free prose in the type
    // position, and a prose-named phantom later contradicts the genuinely
    // inferred type in every comparison. Identifier shape keeps the common case
    // independent of registry seeding; the registry lookup admits legitimate
    // space-bearing platform names («Виртуальный каталог», «Внешний модуль»);
    // the tabular-row phrase is allowlisted separately because the row rewrite
    // in method lookup and the subtype bridge key on it while the registry does
    // not list it as a type.
    if is_type_name_shaped(name)
        || is_tabular_row_type_name(name)
        || PlatformData::instance().get_type(name).is_some()
    {
        db.platform_object(name.to_string())
    } else {
        db.unknown()
    }
}

pub(crate) fn is_tabular_row_type_name(name: &str) -> bool {
    let lc = name.trim().to_lowercase();
    lc == "строка табличной части" || lc == "line of a tabular section"
}

fn is_type_name_shaped(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_alphabetic() || first == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::testing::InMemoryDb;

    #[test]
    fn type_string_typeid_covers_core_branches() {
        let db = InMemoryDb::new();
        assert_eq!(lower_param_type_string_typeid(&db, ""), db.unknown());
        assert_eq!(lower_param_type_string_typeid(&db, "Произвольный"), db.any());
        assert_eq!(lower_param_type_string_typeid(&db, "Число"), db.number(None, None));
        assert_eq!(lower_param_type_string_typeid(&db, "Строка табличной части"), db.unknown());
        assert_eq!(
            lower_param_type_string_typeid(&db, "Число, Строка"),
            db.union(vec![db.number(None, None), db.string(None, false)])
        );
        assert_eq!(lower_return_type_string_typeid(&db, "Ссылка на объект, либо"), db.unknown());
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
        assert_eq!(split_type_alternatives("Метаданные, Массив ;"), vec!["Метаданные", "Массив"],);
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
        let db = InMemoryDb::new();
        assert_eq!(lower_param_type_string_typeid(&db, "Строка табличной части"), db.unknown());
    }

    #[test]
    fn lower_param_arbitrary_collapses_to_any() {
        let db = InMemoryDb::new();
        assert_eq!(lower_param_type_string_typeid(&db, "Произвольный"), db.any());
        assert_eq!(lower_param_type_string_typeid(&db, "Произвольный, Неопределено"), db.any());
        assert_eq!(lower_param_type_string_typeid(&db, "Arbitrary, Undefined"), db.any());
    }

    #[test]
    fn lower_param_multi_with_invalid_segment_collapses() {
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
        let db = InMemoryDb::new();
        assert_eq!(
            lower_param_type_string_typeid(&db, "Число; Строка"),
            db.union(vec![db.number(None, None), db.string(None, false)])
        );
    }

    #[test]
    fn lower_param_mixed_comma_semicolon_with_trailing_garbage() {
        let db = InMemoryDb::new();
        assert_eq!(
            lower_param_type_string_typeid(&db, "Число, Строка, Булево ;"),
            db.union(vec![db.number(None, None), db.string(None, false), db.boolean()])
        );
    }

    #[test]
    fn lower_return_single_unrecognised_lifts_to_platform_object() {
        let db = InMemoryDb::new();
        assert_eq!(
            lower_return_type_string_typeid(&db, "Запрос"),
            db.platform_object("Запрос".to_string())
        );
    }

    #[test]
    fn lower_return_multi_invalid_collapses_to_unknown() {
        let db = InMemoryDb::new();
        assert_eq!(lower_return_type_string_typeid(&db, "Ссылка на объект, либо"), db.unknown());
    }

    #[test]
    fn lower_return_mixed_separator_with_prose_segment_collapses_to_unknown() {
        let raw = "ТабличныйДокумент, ТекстовыйДокумент; другой объект";
        let db = InMemoryDb::new();
        assert_eq!(lower_return_type_string_typeid(&db, raw), db.unknown());
    }

    #[test]
    fn lower_param_mixed_separator_with_prose_segment_collapses() {
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
    fn lower_return_arbitrary_collapses_to_any() {
        let db = InMemoryDb::new();
        assert_eq!(lower_return_type_string_typeid(&db, "Произвольный, Неопределено"), db.any());
        assert_eq!(
            lower_return_type_string_typeid(&db, "  Произвольный  ,  Неопределено"),
            db.any()
        );
    }

    #[test]
    fn lower_platform_type_name_arbitrary_to_any() {
        let db = InMemoryDb::new();
        assert_eq!(lower_platform_type_name_typeid(&db, "Произвольный"), db.any());
        assert_eq!(lower_platform_type_name_typeid(&db, "Arbitrary"), db.any());
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
    fn lower_platform_type_name_prose_collapses_to_unknown() {
        let db = InMemoryDb::new();
        assert_eq!(
            lower_platform_type_name_typeid(&db, "имя редактируемого регистра"),
            db.unknown()
        );
        assert_eq!(
            lower_platform_type_name_typeid(&db, "Справочник1 (Необязательный)"),
            db.unknown()
        );
        assert_eq!(
            lower_platform_type_name_typeid(&db, "Форма клиентского приложения"),
            db.unknown()
        );
    }

    #[test]
    fn lower_platform_type_name_spaced_registry_name_survives() {
        let data = PlatformData::instance();
        if data.get_type("Виртуальный каталог").is_none() {
            return;
        }
        let db = InMemoryDb::new();
        assert_eq!(
            lower_platform_type_name_typeid(&db, "Виртуальный каталог"),
            db.platform_object("Виртуальный каталог".to_string()),
            "a space-bearing name listed in the platform registry must stay representable"
        );
    }

    #[test]
    fn lower_platform_type_name_primitive_returns_canonical() {
        let db = InMemoryDb::new();
        assert_eq!(lower_platform_type_name_typeid(&db, "Число"), db.number(None, None));
    }

    #[test]
    fn semicolon_separator_activation_audit() {
        let data = PlatformData::instance();
        if data.all_types().is_empty() {
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
