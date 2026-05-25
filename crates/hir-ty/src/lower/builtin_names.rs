//! Builtin type name tables, consolidated from the previously-duplicated
//! lookups in `hir_def::ty::from_type_name` and `hir_def::ty::doc_types::parse_type_name`.
//!
//! The tables are intentionally kept internal to `hir-ty::lower`: outside
//! consumers go through [`super::TyLoweringContext`] so adding a new primitive
//! means touching one file.

use bsl_types::builders::Builders;
use bsl_types::facet::{DateComponent, TableSource};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::type_ref::{BuiltinTypeRef, TypeRef};

/// Lower a resolved [`BuiltinTypeRef`] directly into a kernel [`TypeId`].
///
/// Mints the `TypeId` directly via [`Builders`] — no legacy `Ty` intermediate.
pub(super) fn builtin_to_typeid(db: &dyn TypeKernelDb, b: BuiltinTypeRef) -> TypeId {
    match b {
        BuiltinTypeRef::Number => db.number(None, None),
        BuiltinTypeRef::String => db.string(None, false),
        BuiltinTypeRef::Boolean => db.boolean(),
        BuiltinTypeRef::Date => db.date(DateComponent::DateTime),
        BuiltinTypeRef::Undefined => db.undefined(),
        BuiltinTypeRef::Null => db.null(),
        BuiltinTypeRef::Structure => db.structure(None),
        BuiltinTypeRef::ValueTable => db.value_table(None, TableSource::Unknown),
        BuiltinTypeRef::ValueList => db.value_list(None),
        BuiltinTypeRef::Type => db.type_descriptor(),
    }
}

/// Map a bare type-name token directly into a kernel [`TypeId`].
///
/// Mints the `TypeId` directly via [`Builders`] (Array → `db.array(None)`,
/// Map → `db.map(None, None)`, Builtin → [`builtin_to_typeid`], otherwise →
/// `db.unknown()`).
pub fn bare_name_to_typeid(db: &dyn TypeKernelDb, name: &str) -> TypeId {
    match TypeRef::from_bare_name(name) {
        Some(TypeRef::Array(_)) => db.array(None),
        Some(TypeRef::Map(_)) => db.map(None, None),
        Some(TypeRef::Builtin(b)) => builtin_to_typeid(db, b),
        Some(_) | None => db.unknown(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::testing::InMemoryDb;

    #[test]
    fn builtin_typeid_lowers_to_expected_kernel_type() {
        let db = InMemoryDb::new();
        for (b, expected) in [
            (BuiltinTypeRef::Number, db.number(None, None)),
            (BuiltinTypeRef::String, db.string(None, false)),
            (BuiltinTypeRef::Boolean, db.boolean()),
            (BuiltinTypeRef::Date, db.date(DateComponent::DateTime)),
            (BuiltinTypeRef::Undefined, db.undefined()),
            (BuiltinTypeRef::Null, db.null()),
            (BuiltinTypeRef::Structure, db.structure(None)),
            (BuiltinTypeRef::ValueTable, db.value_table(None, TableSource::Unknown)),
            (BuiltinTypeRef::ValueList, db.value_list(None)),
            (BuiltinTypeRef::Type, db.type_descriptor()),
        ] {
            assert_eq!(builtin_to_typeid(&db, b), expected);
        }
    }

    #[test]
    fn bare_name_typeid_lowers_to_expected_kernel_type() {
        let db = InMemoryDb::new();
        for (name, expected) in [
            ("Число", db.number(None, None)),
            ("Строка", db.string(None, false)),
            ("Массив", db.array(None)),
            ("Соответствие", db.map(None, None)),
            ("ТаблицаЗначений", db.value_table(None, TableSource::Unknown)),
            ("Запрос", db.unknown()),
            ("", db.unknown()),
        ] {
            assert_eq!(bare_name_to_typeid(&db, name), expected);
        }
    }
}
