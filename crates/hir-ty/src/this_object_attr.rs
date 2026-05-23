//! Object-module and record-set-module implicit `ЭтотОбъект` member resolution.
//!
//! A bare identifier inside `ObjectModule.bsl` can name a member of the
//! owning metadata object: `Настройки` is resolved as an implicit
//! `ЭтотОбъект.Настройки` access. The resolver gate keeps this limited to
//! ObjectModules whose MDO kind has an `*Object` companion; field lookup
//! handles user attributes, standard attributes, and tabular sections. The
//! record-set variant follows the same path for `RecordSetModule.bsl`, using
//! the owning register's `*RecordSet` receiver.

use hir_def::resolver::Resolver;
use hir_def::ty::Ty;
use hir_def::Name;

use crate::db::HirDatabase;

pub(crate) fn resolve_this_object_member(
    db: &dyn HirDatabase,
    resolver: &Resolver,
    name: &Name,
) -> Option<Ty> {
    let (mdo_type, mdo_name) = crate::this_object::resolve_this_object_owner(db, resolver)?;
    let kind = hir_def::ty::MetadataKind::object_kind_for(mdo_type)?;
    let receiver = Ty::MetadataRef { kind, name: mdo_name };
    let module_id = resolver.module_id()?;
    let configs = db.configurations(module_id.file_id);
    let receiver_id = crate::ty_bridge::ty_to_typeid(db, &receiver);
    crate::field_lookup::lookup_field(db, &configs, receiver_id, name).map(|info| info.ty)
}

/// Implicit `ЭтотОбъект.<name>` resolver for RecordSetModule.
///
/// Sibling of `resolve_this_object_member` — same shape, only the
/// kind-mapping helper differs (`record_set_kind_for` instead of
/// `object_kind_for`). Builds the synthetic `MetadataRef{*RecordSet, name}`
/// from `this_object::resolve_this_record_set_owner` and hands it to
/// `field_lookup::lookup_field`, which handles user dimensions/
/// resources/attributes plus the `*RecordSet` platform-properties
/// cascade (`ДополнительныеСвойства`, `Отбор`, `ОбменДанными`, …).
pub(crate) fn resolve_this_record_set_member(
    db: &dyn HirDatabase,
    resolver: &Resolver,
    name: &Name,
) -> Option<Ty> {
    let (mdo_type, mdo_name) = crate::this_object::resolve_this_record_set_owner(db, resolver)?;
    let kind = hir_def::ty::MetadataKind::record_set_kind_for(mdo_type)?;
    let receiver = Ty::MetadataRef { kind, name: mdo_name };
    let module_id = resolver.module_id()?;
    let configs = db.configurations(module_id.file_id);
    let receiver_id = crate::ty_bridge::ty_to_typeid(db, &receiver);
    crate::field_lookup::lookup_field(db, &configs, receiver_id, name).map(|info| info.ty)
}
