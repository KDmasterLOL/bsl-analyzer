use bsl_types::builders::Builders;
use bsl_types::kind::TypeId;
use bsl_types::testing::RootConfigCtx;
use hir_def::resolver::Resolver;
use hir_def::Name;

use crate::db::HirDatabase;

pub(crate) fn resolve_this_object_member(
    db: &dyn HirDatabase,
    resolver: &Resolver,
    name: &Name,
) -> Option<TypeId> {
    let (mdo_type, mdo_name) = crate::this_object::resolve_this_object_owner(db, resolver)?;
    let kind = hir_def::ty::MetadataKind::object_kind_for(mdo_type)?;
    let module_id = resolver.module_id()?;
    let obj_resolver = crate::object_resolver::DbObjectResolver::new(db, module_id.file_id);
    let receiver_id = db.metadata_ref(kind, mdo_name.as_str().to_string(), &RootConfigCtx);
    crate::field_lookup::lookup_field(db, &obj_resolver, receiver_id, name).map(|info| info.ty)
}

pub(crate) fn resolve_this_record_set_member(
    db: &dyn HirDatabase,
    resolver: &Resolver,
    name: &Name,
) -> Option<TypeId> {
    let (mdo_type, mdo_name) = crate::this_object::resolve_this_record_set_owner(db, resolver)?;
    let kind = hir_def::ty::MetadataKind::record_set_kind_for(mdo_type)?;
    let module_id = resolver.module_id()?;
    let obj_resolver = crate::object_resolver::DbObjectResolver::new(db, module_id.file_id);
    let receiver_id = db.metadata_ref(kind, mdo_name.as_str().to_string(), &RootConfigCtx);
    crate::field_lookup::lookup_field(db, &obj_resolver, receiver_id, name).map(|info| info.ty)
}
