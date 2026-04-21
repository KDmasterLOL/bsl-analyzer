//! Adapter for user-defined methods inside common modules.

use hir::{ModItem, ModuleId, Name};
use ide_db::RootDatabase;
use smol_str::SmolStr;
use vfs::FileId;

use crate::adapters::params::{build_user_params, user_type_to_ref};
use crate::domain::{MethodKind, SignatureSource, SymbolSignature};

pub(super) fn build(
    db: &dyn RootDatabase,
    caller_file: FileId,
    module: &Name,
    method: &Name,
) -> Option<SymbolSignature> {
    let source_root_input = db.file_source_root_input(caller_file);
    let source_root_id = source_root_input.source_root_id(db);
    let module_index = db.module_index(source_root_id);
    let module_file_id = module_index.resolve_common_module(module)?;

    let module_id = ModuleId::new(module_file_id);
    let symbol_tree = db.symbol_tree(module_id);
    let method_symbol = symbol_tree.find_method(method)?;
    if !method_symbol.is_export {
        return None;
    }

    let item_tree = db.item_tree(module_file_id);
    let item = item_tree.top_level_items().get(method_symbol.id.local_id as usize)?;
    let docs = db.method_docs(method_symbol.id);
    let docs_ref = docs.as_deref();

    let (kind, params, returns) = match item {
        ModItem::Procedure(idx) => {
            let proc = item_tree.procedure(*idx);
            (MethodKind::Procedure, build_user_params(&proc.params, docs_ref), Vec::new())
        }
        ModItem::Function(idx) => {
            let func = item_tree.function(*idx);
            let returns = docs_ref
                .map(|d| d.returned_value.iter().map(user_type_to_ref).collect())
                .unwrap_or_default();
            (MethodKind::Function, build_user_params(&func.params, docs_ref), returns)
        }
        ModItem::Variable(_) => return None,
    };

    Some(SymbolSignature {
        kind,
        name_russian: SmolStr::new(method.as_str()),
        name_english: None,
        qualifier: Some(SmolStr::from(format!("{}.", module.as_str()))),
        params,
        returns,
        purpose: docs_ref.and_then(|d| d.purpose.clone()),
        description: docs_ref.and_then(|d| d.purpose.clone()),
        examples: Vec::new(),
        notes: None,
        deprecation: docs_ref.and_then(|d| d.deprecation.clone()),
        is_export: method_symbol.is_export,
        source: SignatureSource::CommonModule,
        method_id: Some(method_symbol.id),
    })
}
