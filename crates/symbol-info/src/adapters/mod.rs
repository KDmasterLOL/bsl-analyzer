use ide_db::RootDatabase;
use vfs::FileId;

use crate::domain::{CalleeKind, SymbolSignature};

mod common_module;
pub mod global_function;
mod local_method;
mod manager_module;
mod mdo_naming;
mod params;
mod platform_constructor;
mod platform_manager;
pub mod platform_method;

pub use global_function::from_global_function;
pub use platform_method::from_platform_method;

pub fn build_signature(
    db: &dyn RootDatabase,
    file_id: FileId,
    callee: &CalleeKind,
) -> Option<SymbolSignature> {
    match callee {
        CalleeKind::PlatformMethod { type_name, method_name } => {
            platform_method::build(db, type_name, method_name)
        }
        CalleeKind::GlobalFunction { name } => global_function::build(db, name),
        CalleeKind::CommonModuleMethod { module, method } => {
            common_module::build(db, file_id, module, method)
        }
        CalleeKind::ManagerModuleMethod { mdo_type, object, method } => {
            manager_module::build(db, file_id, *mdo_type, object, method)
        }
        CalleeKind::PlatformManagerMethod { mdo_type, method } => {
            platform_manager::build(*mdo_type, method)
        }
        CalleeKind::LocalMethod { module_id, method } => {
            local_method::build(db, *module_id, method)
        }
        CalleeKind::PlatformConstructor { type_name } => platform_constructor::build(db, type_name),
    }
}
