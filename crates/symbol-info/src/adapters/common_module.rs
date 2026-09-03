use hir::{ModItem, ModuleId, Name};
use ide_db::RootDatabase;
use smol_str::SmolStr;
use vfs::FileId;

use crate::adapters::params::{build_user_params, user_type_to_ref};
use crate::domain::{MethodKind, SignatureSource, SymbolSignature};

/// The body that DECLARES the method, lowest config-root rank first.
///
/// A configuration extension adopts its base module's name, so one name can have several
/// bodies and the path-derived index answers with just one of them — chosen by path order,
/// which does not track the root topology. Asking that one body alone loses the base
/// declaration outright whenever the extension's path sorts first: the method reads as
/// undeclared while it is right there in the configuration.
fn declaring_body(
    db: &dyn RootDatabase,
    module_index: &hir::ModuleIndex,
    module: &Name,
    method: &Name,
) -> Option<FileId> {
    let mut bodies = module_index.common_module_candidates(module).to_vec();
    if bodies.is_empty() {
        bodies.extend(module_index.resolve_common_module(module));
    }
    // Stable sort: bodies whose root the topology does not rank keep their path order.
    bodies.sort_by_key(|&file| {
        db.config_root_rank_and_label(file).map(|(rank, _)| rank).unwrap_or(usize::MAX)
    });
    bodies.into_iter().find(|&file| {
        db.symbol_tree(ModuleId::new(file))
            .find_method(method)
            .is_some_and(|symbol| symbol.is_export)
    })
}

pub(super) fn build(
    db: &dyn RootDatabase,
    caller_file: FileId,
    module: &Name,
    method: &Name,
) -> Option<Vec<SymbolSignature>> {
    let source_root_input = db.file_source_root_input(caller_file);
    let source_root_id = source_root_input.source_root_id(db);
    let module_index = db.module_index(source_root_id);
    let module_file_id = declaring_body(db, &module_index, module, method)?;

    let module_id = ModuleId::new(module_file_id);
    let symbol_tree = db.symbol_tree(module_id);
    let method_symbol = symbol_tree.find_method(method)?;

    let item_tree = db.item_tree(module_file_id);
    let item = item_tree.item_of(method_symbol.id.local_id)?;
    let docs = db.method_docs(method_symbol.id);
    let docs_ref = docs.as_deref();

    // The name as DECLARED, never as asked for. BSL matches names case-insensitively, so a
    // request may spell the method any way at all — and this string goes on to encode the
    // graph id, which `references` derives from the declaration itself. Echoing the request
    // here makes the two tools print different ids for one symbol on any request whose
    // spelling differs from the source.
    let (kind, params, returns, declared_name) = match item {
        ModItem::Procedure(idx) => {
            let proc = item_tree.procedure(*idx);
            (
                MethodKind::Procedure,
                build_user_params(&proc.params, docs_ref),
                Vec::new(),
                proc.name.as_str().to_string(),
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
            )
        }
        ModItem::Variable(_) => return None,
    };

    Some(vec![SymbolSignature {
        candidate_ordinal: Some(0),
        kind,
        name_russian: SmolStr::new(&declared_name),
        name_english: None,
        qualifier: Some(SmolStr::from(format!("{}.", module.as_str()))),
        prefix: None,
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
        platform_id: None,
    }])
}
