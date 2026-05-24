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
use hir_def::ty::Ty;
use hir_def::type_ref::{BuiltinTypeRef, TypeRef};

/// Lower a resolved [`BuiltinTypeRef`] into its semantic [`Ty`].
///
/// Infallible — every primitive has a fixed `Ty` counterpart. The mapping
/// deliberately collapses `Builtin::Null` into `Ty::Null` (not `Ty::Unknown`)
/// so downstream equality behaves the same as the legacy `from_type_name`
/// path.
pub(super) fn builtin_to_ty(b: BuiltinTypeRef) -> Ty {
    match b {
        BuiltinTypeRef::Number => Ty::Number,
        BuiltinTypeRef::String => Ty::String,
        BuiltinTypeRef::Boolean => Ty::Boolean,
        BuiltinTypeRef::Date => Ty::Date,
        BuiltinTypeRef::Undefined => Ty::Undefined,
        BuiltinTypeRef::Null => Ty::Null,
        BuiltinTypeRef::Structure => Ty::Structure,
        BuiltinTypeRef::ValueTable => Ty::ValueTable { projection: None },
        BuiltinTypeRef::ValueList => Ty::ValueList,
        BuiltinTypeRef::Type => Ty::Type,
    }
}

/// Kernel-native counterpart of [`builtin_to_ty`].
///
/// Mints the `TypeId` directly via [`Builders`] — no `Ty` intermediate. Each
/// arm is byte-identical to `ty_to_typeid(db, &builtin_to_ty(b))` (asserted by
/// the drift-detector test below), which is what lets §4.A.4 delete the `Ty`
/// path without changing any interned id.
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

/// Map a bare type-name token to its canonical [`Ty`], or `Ty::Unknown` for
/// names outside the primitive / collection / sentinel set.
///
/// Routes through [`TypeRef::from_bare_name`] + [`builtin_to_ty`] so the
/// case-insensitive RU/EN lookup table lives in exactly one place
/// (`hir_def::type_ref::BuiltinTypeRef::from_name`). Replaces the legacy
/// `Ty::from_type_name` inherent function that previously duplicated the
/// table in `hir-def`. Routing every consumer through this helper keeps the
/// platform-type-union lowering pipeline (param / return / single-name) on
/// a single path inside `hir-ty/src/lower/`, which is what plan §6.5
/// «Один path lowering» asks for.
pub fn ty_from_bare_name(name: &str) -> Ty {
    match TypeRef::from_bare_name(name) {
        Some(TypeRef::Array(_)) => Ty::Array,
        Some(TypeRef::Map(_)) => Ty::Map,
        Some(TypeRef::Builtin(b)) => builtin_to_ty(b),
        // `from_bare_name` only returns Array / Map / Builtin for `Some`,
        // but the catch-all keeps the function total against any future
        // TypeRef variant `from_bare_name` learns to construct.
        Some(_) | None => Ty::Unknown,
    }
}

/// Kernel-native counterpart of [`ty_from_bare_name`].
///
/// Mints the `TypeId` directly via [`Builders`], mirroring `ty_from_bare_name`
/// arm-for-arm (Array → `db.array(None)`, Map → `db.map(None, None)`, Builtin →
/// [`builtin_to_typeid`], otherwise → `db.unknown()`).
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
    use crate::ty_bridge::ty_to_typeid;
    use bsl_types::testing::InMemoryDb;

    /// §4.A drift-detector: native minting must produce the *same interned id*
    /// as bridging the legacy `Ty` path. This is the guard that lets §4.A.4
    /// delete `builtin_to_ty` / `ty_from_bare_name` without moving any id.
    #[test]
    fn builtin_typeid_matches_bridge() {
        let db = InMemoryDb::new();
        for b in [
            BuiltinTypeRef::Number,
            BuiltinTypeRef::String,
            BuiltinTypeRef::Boolean,
            BuiltinTypeRef::Date,
            BuiltinTypeRef::Undefined,
            BuiltinTypeRef::Null,
            BuiltinTypeRef::Structure,
            BuiltinTypeRef::ValueTable,
            BuiltinTypeRef::ValueList,
            BuiltinTypeRef::Type,
        ] {
            assert_eq!(builtin_to_typeid(&db, b), ty_to_typeid(&db, &builtin_to_ty(b)));
        }
    }

    #[test]
    fn bare_name_typeid_matches_bridge() {
        let db = InMemoryDb::new();
        for name in ["Число", "Строка", "Массив", "Соответствие", "ТаблицаЗначений", "Запрос", ""]
        {
            assert_eq!(bare_name_to_typeid(&db, name), ty_to_typeid(&db, &ty_from_bare_name(name)));
        }
    }
}
