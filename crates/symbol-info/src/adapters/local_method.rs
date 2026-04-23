//! Adapter for methods declared in the caller's own module.

use hir::{ModItem, ModuleId, Name, Resolver};
use ide_db::RootDatabase;
use smol_str::SmolStr;

use crate::adapters::params::{build_user_params, user_type_to_ref};
use crate::domain::{MethodKind, SignatureSource, SymbolSignature};

pub(super) fn build(
    db: &dyn RootDatabase,
    module_id: ModuleId,
    method: &Name,
) -> Option<SymbolSignature> {
    let resolver = Resolver::for_module(module_id);
    let method_id = resolver.resolve_module_method(db, method)?;

    let item_tree = db.item_tree(method_id.module.file_id);
    let item = item_tree.top_level_items().get(method_id.local_id as usize)?;
    let docs = db.method_docs(method_id);
    let docs_ref = docs.as_deref();

    let (kind, params, returns, name_ru, is_export) = match item {
        ModItem::Procedure(idx) => {
            let proc = item_tree.procedure(*idx);
            (
                MethodKind::Procedure,
                build_user_params(&proc.params, docs_ref),
                Vec::new(),
                proc.name.as_str().to_string(),
                proc.is_export,
            )
        }
        ModItem::Function(idx) => {
            let func = item_tree.function(*idx);
            let returns = docs_ref
                .map(|d| d.returned_value.iter().map(user_type_to_ref).collect())
                .unwrap_or_default();
            (
                MethodKind::Function,
                build_user_params(&func.params, docs_ref),
                returns,
                func.name.as_str().to_string(),
                func.is_export,
            )
        }
        ModItem::Variable(_) => return None,
    };

    Some(SymbolSignature {
        kind,
        name_russian: SmolStr::new(&name_ru),
        name_english: None,
        qualifier: None,
        prefix: None,
        params,
        returns,
        purpose: docs_ref.and_then(|d| d.purpose.clone()),
        description: docs_ref.and_then(|d| d.purpose.clone()),
        examples: Vec::new(),
        notes: None,
        deprecation: docs_ref.and_then(|d| d.deprecation.clone()),
        is_export,
        source: SignatureSource::Local,
        method_id: Some(method_id),
    })
}
