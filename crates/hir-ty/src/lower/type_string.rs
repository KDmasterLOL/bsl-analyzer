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
    fn lower_param_arbitrary_collapses_to_unknown() {
        let db = InMemoryDb::new();
        assert_eq!(lower_param_type_string_typeid(&db, "Произвольный"), db.unknown());
        assert_eq!(lower_param_type_string_typeid(&db, "Произвольный, Неопределено"), db.unknown());
        assert_eq!(lower_param_type_string_typeid(&db, "Arbitrary, Undefined"), db.unknown());
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
    fn lower_return_multi_invalid_falls_back_to_raw_platform_object() {
        let db = InMemoryDb::new();
        assert_eq!(
            lower_return_type_string_typeid(&db, "Ссылка на объект, либо"),
            db.platform_object("Ссылка на объект, либо".to_string())
        );
    }

    #[test]
    fn lower_return_mixed_separator_with_prose_segment_falls_back() {
        let raw = "ТабличныйДокумент, ТекстовыйДокумент; другой объект";
        let db = InMemoryDb::new();
        assert_eq!(lower_return_type_string_typeid(&db, raw), db.platform_object(raw.to_string()));
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
