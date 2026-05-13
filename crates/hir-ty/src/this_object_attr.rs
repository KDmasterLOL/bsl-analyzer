//! Object-module implicit `ЭтотОбъект` member resolution.
//!
//! A bare identifier inside `ObjectModule.bsl` can name a member of the
//! owning metadata object: `Настройки` is resolved as an implicit
//! `ЭтотОбъект.Настройки` access. The resolver gate keeps this limited to
//! ObjectModules whose MDO kind has an `*Object` companion; field lookup
//! handles user attributes, standard attributes, and tabular sections.

use hir_def::resolver::Resolver;
use hir_def::ty::Ty;
use hir_def::Name;

use crate::db::HirDatabase;

pub(crate) fn resolve_this_object_member(
    db: &dyn HirDatabase,
    resolver: &Resolver,
    name: &Name,
) -> Option<Ty> {
    let (mdo_type, mdo_name) = resolver.resolve_this_object(db)?;
    let kind = hir_def::ty::MetadataKind::object_kind_for(mdo_type)?;
    let receiver = Ty::MetadataRef { kind, name: mdo_name };
    let module_id = resolver.module_id()?;
    let configs = db.configurations(module_id.file_id);
    crate::field_lookup::lookup_field(&configs, &receiver, name).map(|info| info.ty)
}
