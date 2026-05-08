//! Builtin type name tables, consolidated from the previously-duplicated
//! lookups in `hir_def::ty::from_type_name` and `hir_def::ty::doc_types::parse_type_name`.
//!
//! The tables are intentionally kept internal to `hir-ty::lower`: outside
//! consumers go through [`super::TyLoweringContext`] so adding a new primitive
//! means touching one file.

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
        BuiltinTypeRef::ValueTable => Ty::ValueTable,
        BuiltinTypeRef::ValueList => Ty::ValueList,
        BuiltinTypeRef::Type => Ty::Type,
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
