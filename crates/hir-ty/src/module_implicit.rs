//! Bare-identifier fields visible through the current module's implicit self.
//!
//! Object and record-set modules expose their owner attributes as unqualified
//! names. Managed form modules expose form attributes the same way.

use bsl_metadata::{MdoType, ModuleType};
use hir_def::ty::{MetadataKind, Ty};
use hir_def::{ModuleId, Name};
use vfs::FileId;

use crate::db::HirDatabase;
use crate::field_enum::{enumerate_fields, FieldInfo, FieldOrigin};
use crate::form_attr::lower_form_attribute_to_ty;

/// Symbols visible as bare-ident in the current module.
pub fn module_implicit_fields(db: &dyn HirDatabase, file_id: FileId) -> Vec<FieldInfo> {
    let module_id = ModuleId::new(file_id);
    let metadata = db.module_metadata(module_id);
    let configs = db.configurations(file_id);

    match metadata.module_type {
        ModuleType::ObjectModule => {
            let Some(mdo) = metadata.mdo.as_ref() else { return Vec::new() };
            let Some(kind) = MetadataKind::object_kind_for(mdo.mdo_type) else {
                return Vec::new();
            };
            let receiver = Ty::MetadataRef { kind, name: Name::new(&mdo.name) };
            enumerate_fields(&configs, &receiver)
        }
        ModuleType::RecordSetModule => {
            let Some((mdo_type, name)) = module_owner_mdo(&metadata) else {
                return Vec::new();
            };
            let Some(kind) = MetadataKind::record_set_kind_for(mdo_type) else {
                return Vec::new();
            };
            let receiver = Ty::MetadataRef { kind, name };
            enumerate_fields(&configs, &receiver)
        }
        ModuleType::ManagerModule => Vec::new(),
        ModuleType::FormModule => {
            let Some(form) = metadata.form.as_ref().filter(|form| form.is_managed()) else {
                return Vec::new();
            };
            form.attributes()
                .iter()
                .map(|attr| FieldInfo {
                    name: Name::new(&attr.name),
                    name_en: None,
                    ty: lower_form_attribute_to_ty(attr, &configs),
                    value_ty: None,
                    is_readonly: false,
                    origin: if attr.is_main {
                        FieldOrigin::MainFormAttribute
                    } else {
                        FieldOrigin::FormAttribute
                    },
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn module_owner_mdo(metadata: &hir_def::ModuleMetadata) -> Option<(MdoType, Name)> {
    match (metadata.mdo.as_ref(), metadata.register.as_ref()) {
        (Some(mdo), _) => Some((mdo.mdo_type, Name::new(&mdo.name))),
        (None, Some(register)) => Some((register.mdo_type(), Name::new(register.name()))),
        (None, None) => None,
    }
}
