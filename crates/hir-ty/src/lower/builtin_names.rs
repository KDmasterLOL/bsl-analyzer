//! Builtin type name tables, consolidated from the previously-duplicated
//! lookups in `hir_def::ty::from_type_name` and `hir_def::ty::doc_types::parse_type_name`.
//!
//! The tables are intentionally kept internal to `hir-ty::lower`: outside
//! consumers go through [`super::TyLoweringContext`] so adding a new primitive
//! means touching one file.

use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::ty::Ty;
use hir_def::type_ref::{BuiltinTypeRef, TypeRef};

use crate::ty_bridge::ty_to_typeid;

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
/// Bridges through the §4.A `Ty` → `TypeId` translator. §4.D-§4.E will
/// rewrite this to construct `TypeKind` directly when consumers stop
/// reading the legacy `Ty` enum.
#[allow(dead_code, reason = "Phase 3 §4.B producer — callers migrate in 4.C-4.E")]
pub(super) fn builtin_to_typeid(db: &dyn TypeKernelDb, b: BuiltinTypeRef) -> TypeId {
    ty_to_typeid(db, &builtin_to_ty(b))
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
pub fn bare_name_to_typeid(db: &dyn TypeKernelDb, name: &str) -> TypeId {
    ty_to_typeid(db, &ty_from_bare_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty_bridge::typeid_to_ty;
    use bsl_types::testing::InMemoryDb;

    /// §4.B drift-detector: if the kernel-native shim ever stops mirroring
    /// the Ty path, this fails — surfacing the divergence at §4.D-§4.E.
    #[test]
    fn builtin_typeid_round_trips_via_ty() {
        let db = InMemoryDb::new();
        for b in [BuiltinTypeRef::Number, BuiltinTypeRef::Boolean, BuiltinTypeRef::ValueTable] {
            let via_ty = builtin_to_ty(b);
            let via_typeid = builtin_to_typeid(&db, b);
            assert_eq!(typeid_to_ty(&db, via_typeid), via_ty);
        }
    }
}
