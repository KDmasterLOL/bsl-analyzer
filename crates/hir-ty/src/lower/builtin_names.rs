//! Builtin type name tables, consolidated from the previously-duplicated
//! lookups in `hir_def::ty::from_type_name` and `hir_def::ty::doc_types::parse_type_name`.
//!
//! The tables are intentionally kept internal to `hir-ty::lower`: outside
//! consumers go through [`super::TyLoweringContext`] so adding a new primitive
//! means touching one file.

use hir_def::ty::Ty;
use hir_def::type_ref::BuiltinTypeRef;

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
