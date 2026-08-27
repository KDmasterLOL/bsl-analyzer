use std::sync::Arc;

use base_db::SourceRootId;
use bsl_metadata::MdoType;
use hir::{
    extract_change_and_validate, interception_effective, interceptor_target, InterceptionKind,
    ManagerType, MethodSymbol, ModuleId, Name, VariableSymbol,
};
use stdx::case::CaseExt;
use vfs::FileId;

use crate::{effective_target, weaving_target, RootDatabase};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectiveModuleRole {
    Object,
    Manager,
    ManagedForm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportComposition {
    Base,
    Added,
    ChangeAndValidate,
    Instead,
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveMethodExport {
    /// Public target name. For extension interceptors this is the target named
    /// by the annotation, never the handler's internal source name.
    pub name: Name,
    pub method: MethodSymbol,
    pub source_extension: Option<String>,
    pub composition: ExportComposition,
    replaces_effective: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveVariableExport {
    pub variable: VariableSymbol,
    pub source_extension: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectiveModuleExports {
    pub methods: Vec<EffectiveMethodExport>,
    pub variables: Vec<EffectiveVariableExport>,
}

fn same_name(left: &Name, right: &Name) -> bool {
    left.as_str().fold_lower() == right.as_str().fold_lower()
}

fn replace_effective(methods: &mut Vec<EffectiveMethodExport>, candidate: EffectiveMethodExport) {
    methods.retain(|existing| {
        !(existing.replaces_effective && same_name(&existing.name, &candidate.name))
    });
    methods.push(candidate);
}

fn candidate_files(
    db: &dyn RootDatabase,
    source_root_id: SourceRootId,
    role: EffectiveModuleRole,
    mdo_type: MdoType,
    object_name: &Name,
    form_name: Option<&Name>,
) -> Vec<FileId> {
    let index = db.module_index(source_root_id);
    match role {
        EffectiveModuleRole::Object => {
            index.object_module_candidates(mdo_type, object_name).to_vec()
        }
        EffectiveModuleRole::Manager => ManagerType::from_mdo_type(mdo_type)
            .map(|manager| index.manager_candidates(manager, object_name).to_vec())
            .unwrap_or_default(),
        EffectiveModuleRole::ManagedForm => form_name
            .map(|form| index.form_module_candidates(Some((mdo_type, object_name)), form).to_vec())
            .unwrap_or_default(),
    }
}

/// Exported object/manager/managed-form surface composed from direct
/// `ModuleIndex` candidates in workspace topology order. All parsing and merge
/// semantics route through the existing symbol-tree, effective and weaving
/// queries; this query never scans the workspace or rereads XML.
#[salsa::tracked(returns(clone))]
pub fn effective_module_exports_query(
    db: &dyn RootDatabase,
    source_root_id: SourceRootId,
    visibility_file: Option<FileId>,
    role: EffectiveModuleRole,
    mdo_type: MdoType,
    object_name: String,
    form_name: Option<String>,
) -> Arc<EffectiveModuleExports> {
    let object_name = Name::new(&object_name);
    let form_name = form_name.as_deref().map(Name::new);
    let mut files =
        candidate_files(db, source_root_id, role, mdo_type, &object_name, form_name.as_ref());
    if let Some(visible_ranks) = visibility_file.and_then(|file| db.visible_config_root_ranks(file))
    {
        files.retain(|&file| {
            db.config_root_rank_and_label(file)
                .is_some_and(|(rank, _)| visible_ranks.contains(&rank))
        });
    }
    files.sort_by_key(|&file| {
        db.config_root_rank_and_label(file).map(|(rank, _)| rank).unwrap_or(usize::MAX)
    });

    let mut result = EffectiveModuleExports::default();
    for file in files {
        if db.file_is_unread(file) {
            continue;
        }
        let source_extension = db.config_root_rank_and_label(file).and_then(|(_, source)| source);
        let tree = db.symbol_tree(ModuleId::new(file));

        if source_extension.is_none() {
            result.methods.extend(tree.exported_methods().cloned().map(|method| {
                EffectiveMethodExport {
                    name: method.name.clone(),
                    method,
                    source_extension: None,
                    composition: ExportComposition::Base,
                    replaces_effective: true,
                }
            }));
            result.variables.extend(
                tree.exported_variables()
                    .cloned()
                    .map(|variable| EffectiveVariableExport { variable, source_extension: None }),
            );
            continue;
        }

        let effective = effective_target(db, file);
        let effective_tree = effective.map(|id| hir::symbol_tree_effective(db, id));
        let weaving = weaving_target(db, file);
        let base_tree = weaving.map(|id| db.symbol_tree(ModuleId::new(id.base_file(db))));
        let parse = db.parse_ref(file);

        for method in tree.methods() {
            let node = method.syntax_node(parse);
            if let Some(change) = node.as_ref().and_then(extract_change_and_validate) {
                let target = Name::new(&change.target);
                if effective_tree
                    .as_ref()
                    .and_then(|symbols| symbols.find_method(&target))
                    .is_some_and(|symbol| symbol.is_export)
                {
                    replace_effective(
                        &mut result.methods,
                        EffectiveMethodExport {
                            name: target,
                            method: method.clone(),
                            source_extension: source_extension.clone(),
                            composition: ExportComposition::ChangeAndValidate,
                            replaces_effective: true,
                        },
                    );
                }
                continue;
            }

            if let Some(interception) = node.as_ref().and_then(interceptor_target) {
                let target = Name::new(&interception.target);
                let applicable = base_tree
                    .as_ref()
                    .and_then(|symbols| symbols.find_method(&target))
                    .is_some_and(|base| interception_effective(interception.kind, method, base));
                if applicable {
                    let (composition, replaces_effective) = match interception.kind {
                        InterceptionKind::Around => (ExportComposition::Instead, true),
                        InterceptionKind::Before => (ExportComposition::Before, false),
                        InterceptionKind::After => (ExportComposition::After, false),
                    };
                    let candidate = EffectiveMethodExport {
                        name: target,
                        method: method.clone(),
                        source_extension: source_extension.clone(),
                        composition,
                        replaces_effective,
                    };
                    if replaces_effective {
                        replace_effective(&mut result.methods, candidate);
                    } else {
                        result.methods.push(candidate);
                    }
                }
                continue;
            }

            if method.is_export {
                result.methods.push(EffectiveMethodExport {
                    name: method.name.clone(),
                    method: method.clone(),
                    source_extension: source_extension.clone(),
                    composition: ExportComposition::Added,
                    replaces_effective: false,
                });
            }
        }
        result.variables.extend(tree.exported_variables().cloned().map(|variable| {
            EffectiveVariableExport { variable, source_extension: source_extension.clone() }
        }));
    }

    Arc::new(result)
}
