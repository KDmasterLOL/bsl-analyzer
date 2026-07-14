use hir_def::execution_env::{self, EnvFlags, EnvOptions};
use hir_def::MethodId;

use crate::db::HirDatabase;

pub(crate) fn effective_method_env(
    db: &dyn HirDatabase,
    method: MethodId,
    options: &EnvOptions,
) -> EnvFlags {
    let metadata = db.module_metadata(method.module);
    let item_tree = db.item_tree(method.module.file_id);
    let mut environment =
        execution_env::method_env(&item_tree, method.local_id, &metadata, options);
    if !environment.is_empty() && item_tree.has_module_preproc() {
        if let Some(range) = execution_env::method_source_range(&item_tree, method.local_id) {
            let conditionals = db.conditional_tree(method.module.file_id);
            if !conditionals.is_empty() {
                environment = environment & execution_env::conditional_env(&conditionals, range);
            }
        }
    }
    environment
}
