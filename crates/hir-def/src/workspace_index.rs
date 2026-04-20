//! Workspace-wide symbol index for fast cross-file lookups.
//!
//! ## Problem
//!
//! Naive find references: O(N×M) where N=files, M=tokens/file
//! - For 6,540 files: ~30 seconds
//!
//! ## Solution
//!
//! WorkspaceIndex: O(C×M) where C=candidate files (~10-100 files)
//! - For 6,540 files: ~3 seconds (10-30x speedup)
//!
//! ## Architecture
//!
//! ```text
//! WorkspaceIndex (per SourceRoot, Salsa-cached)
//!   ├─ method_index: Name → Vec<MethodId>
//!   ├─ variable_index: Name → Vec<VariableId>
//!   └─ file_symbols: FileId → Vec<SymbolInfo>
//! ```

use crate::{item_tree::ModItem, DefDatabase, MethodId, ModuleId, Name, VariableId};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use vfs::FileId;

/// Symbol information for indexing.
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

/// Workspace-wide symbol index.
///
/// Built per SourceRoot and cached by Salsa.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceIndex {
    /// method_name → Vec<MethodId>
    method_index: FxHashMap<Name, Vec<MethodId>>,

    /// variable_name → Vec<VariableId>
    variable_index: FxHashMap<Name, Vec<VariableId>>,

    /// file_id → symbols in that file
    file_symbols: FxHashMap<FileId, Vec<SymbolInfo>>,
}

impl WorkspaceIndex {
    /// Build index for all files in a source root.
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

    /// Index a single file.
    fn index_file(&mut self, db: &dyn DefDatabase, file_id: FileId) {
        let item_tree = db.item_tree(file_id);
        let module_id = ModuleId::new(file_id);

        let mut file_symbols = Vec::new();

        for (idx, item) in item_tree.top_level_items().iter().enumerate() {
            match item {
                ModItem::Procedure(proc_idx) => {
                    let proc = item_tree.procedure(*proc_idx);
                    let method_id = MethodId { module: module_id, local_id: idx as u32 };

                    // Add to method index
                    self.method_index.entry(proc.name.clone()).or_default().push(method_id);

                    // Add to file symbols
                    file_symbols.push(SymbolInfo {
                        name: proc.name.clone(),
                        kind: SymbolKind::Method,
                        file_id,
                    });
                }
                ModItem::Function(func_idx) => {
                    let func = item_tree.function(*func_idx);
                    let method_id = MethodId { module: module_id, local_id: idx as u32 };

                    // Add to method index
                    self.method_index.entry(func.name.clone()).or_default().push(method_id);

                    // Add to file symbols
                    file_symbols.push(SymbolInfo {
                        name: func.name.clone(),
                        kind: SymbolKind::Method,
                        file_id,
                    });
                }
                ModItem::Variable(var_idx) => {
                    let var = item_tree.variable(*var_idx);
                    let variable_id = VariableId { module: module_id, local_id: idx as u32 };

                    // Add to variable index
                    self.variable_index.entry(var.name.clone()).or_default().push(variable_id);

                    // Add to file symbols
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

    /// Find all methods with the given name (case-insensitive).
    pub fn find_methods(&self, name: &Name) -> Vec<MethodId> {
        // Try exact match first
        if let Some(methods) = self.method_index.get(name) {
            return methods.clone();
        }

        // Fallback: case-insensitive search
        self.method_index
            .iter()
            .filter(|(k, _)| k.eq_ignore_case(name))
            .flat_map(|(_, v)| v.clone())
            .collect()
    }

    /// Find all variables with the given name (case-insensitive).
    pub fn find_variables(&self, name: &Name) -> Vec<VariableId> {
        // Try exact match first
        if let Some(variables) = self.variable_index.get(name) {
            return variables.clone();
        }

        // Fallback: case-insensitive search
        self.variable_index
            .iter()
            .filter(|(k, _)| k.eq_ignore_case(name))
            .flat_map(|(_, v)| v.clone())
            .collect()
    }

    /// Get all symbols in a file.
    pub fn symbols_in_file(&self, file_id: FileId) -> Option<&[SymbolInfo]> {
        self.file_symbols.get(&file_id).map(|v| v.as_slice())
    }

    /// Get candidate files for a given symbol name.
    ///
    /// Returns files that might contain references to this symbol.
    pub fn candidate_files(&self, name: &Name) -> Vec<FileId> {
        let mut files = Vec::new();

        // Files with methods matching the name
        if let Some(methods) = self.method_index.get(name) {
            files.extend(methods.iter().map(|m| m.module.file_id));
        }

        // Files with variables matching the name
        if let Some(variables) = self.variable_index.get(name) {
            files.extend(variables.iter().map(|v| v.module.file_id));
        }

        // Case-insensitive fallback
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

        // Deduplicate
        files.sort_unstable();
        files.dedup();

        files
    }
}

/// Salsa query for WorkspaceIndex.
///
/// Builds and caches workspace-wide symbol index per SourceRoot.
///
/// ## Salsa caching
/// - LRU: 4 (one per source root, typically 1-2 in most projects)
/// - Invalidation: Automatic when any file in the source root changes
/// - Durability: Inherits from SourceRoot (LOW for local code, HIGH for libraries)
///
/// ## Performance
/// - Build time: ~50-100ms for 6,540 files (doc3 project)
/// - Cached access: < 1ms
/// - Memory: ~100-500 KB per 1000 files
///
/// ## Usage
/// ```ignore
/// // In DefDatabase implementation:
/// fn workspace_index(&self, source_root_id: SourceRootId) -> Arc<WorkspaceIndex> {
///     hir_def::workspace_index_query(self, source_root_id)
/// }
/// ```
#[salsa::tracked(lru = 4)]
pub fn workspace_index_query(
    db: &dyn DefDatabase,
    source_root_input: base_db::SourceRootInput,
) -> Arc<WorkspaceIndex> {
    let source_root = source_root_input.root(db);
    let file_set = source_root.file_set();

    // Only BSL source files are valid input for item_tree/parse. SourceRoot
    // also holds XML/MD/TXT for metadata, which would otherwise be parsed
    // as BSL and burn CPU (or trigger the parser iteration guard).
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
