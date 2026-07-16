use crate::{item_tree::ModItem, DefDatabase, MethodId, ModuleId, Name, VariableId};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use vfs::FileId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolInfo {
    pub name: Name,
    pub kind: SymbolKind,
    pub file_id: FileId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Method,
    Variable,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceIndex {
    method_index: FxHashMap<Name, Vec<MethodId>>,

    variable_index: FxHashMap<Name, Vec<VariableId>>,

    file_symbols: FxHashMap<FileId, Vec<SymbolInfo>>,
}

impl WorkspaceIndex {
    pub fn build(db: &dyn DefDatabase, files: &[FileId]) -> Self {
        let _span =
            tracing::info_span!("WorkspaceIndex::build", file_count = files.len()).entered();

        let mut index = WorkspaceIndex::default();

        for &file_id in files {
            index.index_file(db, file_id);
        }

        tracing::info!(
            methods = index.method_index.len(),
            variables = index.variable_index.len(),
            files = index.file_symbols.len(),
            "WorkspaceIndex built"
        );

        index
    }

    fn index_file(&mut self, db: &dyn DefDatabase, file_id: FileId) {
        let item_tree = db.item_tree(file_id);
        let module_id = ModuleId::new(file_id);

        let mut file_symbols = Vec::new();

        for (idx, item) in item_tree.top_level_items().iter().enumerate() {
            match item {
                ModItem::Procedure(proc_idx) => {
                    let proc = item_tree.procedure(*proc_idx);
                    let method_id = MethodId { module: module_id, local_id: idx as u32 };

                    self.method_index.entry(proc.name.clone()).or_default().push(method_id);

                    file_symbols.push(SymbolInfo {
                        name: proc.name.clone(),
                        kind: SymbolKind::Method,
                        file_id,
                    });
                }
                ModItem::Function(func_idx) => {
                    let func = item_tree.function(*func_idx);
                    let method_id = MethodId { module: module_id, local_id: idx as u32 };

                    self.method_index.entry(func.name.clone()).or_default().push(method_id);

                    file_symbols.push(SymbolInfo {
                        name: func.name.clone(),
                        kind: SymbolKind::Method,
                        file_id,
                    });
                }
                ModItem::Variable(var_idx) => {
                    let var = item_tree.variable(*var_idx);
                    let variable_id = VariableId { module: module_id, local_id: idx as u32 };

                    self.variable_index.entry(var.name.clone()).or_default().push(variable_id);

                    file_symbols.push(SymbolInfo {
                        name: var.name.clone(),
                        kind: SymbolKind::Variable,
                        file_id,
                    });
                }
            }
        }

        if !file_symbols.is_empty() {
            self.file_symbols.insert(file_id, file_symbols);
        }
    }

    pub fn find_methods(&self, name: &Name) -> Vec<MethodId> {
        if let Some(methods) = self.method_index.get(name) {
            return methods.clone();
        }

        self.method_index
            .iter()
            .filter(|(k, _)| k.eq_ignore_case(name))
            .flat_map(|(_, v)| v.clone())
            .collect()
    }

    pub fn find_variables(&self, name: &Name) -> Vec<VariableId> {
        if let Some(variables) = self.variable_index.get(name) {
            return variables.clone();
        }

        self.variable_index
            .iter()
            .filter(|(k, _)| k.eq_ignore_case(name))
            .flat_map(|(_, v)| v.clone())
            .collect()
    }

    pub fn symbols_in_file(&self, file_id: FileId) -> Option<&[SymbolInfo]> {
        self.file_symbols.get(&file_id).map(|v| v.as_slice())
    }

    pub fn candidate_files(&self, name: &Name) -> Vec<FileId> {
        let mut files = Vec::new();

        if let Some(methods) = self.method_index.get(name) {
            files.extend(methods.iter().map(|m| m.module.file_id));
        }

        if let Some(variables) = self.variable_index.get(name) {
            files.extend(variables.iter().map(|v| v.module.file_id));
        }

        for (k, methods) in &self.method_index {
            if k.eq_ignore_case(name) {
                files.extend(methods.iter().map(|m| m.module.file_id));
            }
        }

        for (k, variables) in &self.variable_index {
            if k.eq_ignore_case(name) {
                files.extend(variables.iter().map(|v| v.module.file_id));
            }
        }

        files.sort_unstable();
        files.dedup();

        files
    }
}

/// Approximate live heap bytes for Salsa's `memory_usage` report: each map's
/// table, the owned `Name` keys, the id posting lists, and `file_symbols`'
/// per-file `SymbolInfo` names. New heap-owning fields must be added here too.
fn workspace_index_heap(v: &Arc<WorkspaceIndex>) -> usize {
    use crate::heap_estimate::{map_table_bytes, name_bytes, vec_bytes};

    let idx = &**v;
    let mut bytes = std::mem::size_of::<WorkspaceIndex>();

    bytes += map_table_bytes::<Name, Vec<MethodId>>(idx.method_index.len());
    for (name, ids) in &idx.method_index {
        bytes += name_bytes(name) + vec_bytes::<MethodId>(ids.len());
    }

    bytes += map_table_bytes::<Name, Vec<VariableId>>(idx.variable_index.len());
    for (name, ids) in &idx.variable_index {
        bytes += name_bytes(name) + vec_bytes::<VariableId>(ids.len());
    }

    bytes += map_table_bytes::<FileId, Vec<SymbolInfo>>(idx.file_symbols.len());
    for symbols in idx.file_symbols.values() {
        bytes += vec_bytes::<SymbolInfo>(symbols.len());
        for sym in symbols {
            bytes += name_bytes(&sym.name);
        }
    }

    bytes
}

#[salsa::tracked(lru = 4, heap_size = workspace_index_heap, returns(clone))]
pub fn workspace_index_query(
    db: &dyn DefDatabase,
    source_root_input: base_db::SourceRootInput,
) -> Arc<WorkspaceIndex> {
    let source_root = source_root_input.root(db);
    let file_set = source_root.file_set();

    let files: Vec<FileId> = source_root
        .iter()
        .filter(|&file_id| crate::workspace::is_bsl_source(file_set, file_id))
        .collect();

    let _span = tracing::info_span!("workspace_index_query", file_count = files.len()).entered();
    tracing::info!(file_count = files.len(), "Building WorkspaceIndex");

    let index = WorkspaceIndex::build(db, &files);

    Arc::new(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_index_empty() {
        let index = WorkspaceIndex::default();
        assert!(index.find_methods(&Name::new("Test")).is_empty());
        assert!(index.find_variables(&Name::new("Test")).is_empty());
    }

    #[test]
    fn test_candidate_files_empty() {
        let index = WorkspaceIndex::default();
        assert!(index.candidate_files(&Name::new("Test")).is_empty());
    }
}
