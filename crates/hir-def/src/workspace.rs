//! Workspace-wide symbol indexing for fast cross-module lookup.
//!
//! This module provides a global index of all CommonModules in the workspace,
//! enabling O(1) lookup for qualified name resolution like `ОбщийМодуль.Метод()`.

use std::path::Path;

use rustc_hash::FxHashMap;
use tracing::debug;
use vfs::{FileId, FileSet};

use crate::{DefDatabase, MethodSymbol, ModuleId, Name};

/// Returns `true` when `path` has the `.bsl` extension (case-insensitive).
///
/// XML metadata files, OneScript `.os` modules, and other non-BSL entries
/// scanned into VFS are never valid inputs for the BSL parser. Every
/// workspace-wide scan that calls into `db.parse` / `db.item_tree` must
/// filter through this predicate, and `process_changes` uses the same
/// predicate to decide whether to push the file's text through the Salsa
/// `FileTextInput` (non-BSL files keep their FileSet entry but never enter
/// Salsa storage — see `bsl_metadata::loader` for the disk-side reader).
pub fn is_bsl_source_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("bsl"))
}

/// FileSet-aware companion of [`is_bsl_source_path`]: resolves `file_id`
/// through `file_set` and applies the same `.bsl` check. Returns `false`
/// for unknown ids, matching the safe-default expected by every caller.
pub fn is_bsl_source(file_set: &FileSet, file_id: FileId) -> bool {
    let Some(vfs_path) = file_set.path_for_file(&file_id) else {
        return false;
    };
    is_bsl_source_path(vfs_path.as_path())
}

/// Information about a CommonModule in the workspace.
///
/// Contains the module's location and all its exported methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonModuleInfo {
    /// File ID of the module source file.
    pub file_id: FileId,

    /// Module ID for HIR queries.
    pub module_id: ModuleId,

    /// All methods (procedures and functions) exported by this module.
    pub methods: Vec<MethodSymbol>,
}

/// Global symbol index for the workspace.
///
/// This structure is cached by Salsa and provides O(1) lookup for CommonModule methods.
/// Automatically invalidated when any file in the workspace changes.
///
/// # Example
///
/// ```ignore
/// let symbols = db.workspace_symbols(&all_files);
/// if let Some(module_info) = symbols.common_modules.get(&Name::new("ОбщегоНазначения")) {
///     // Found CommonModule "ОбщегоНазначения"
///     for method in &module_info.methods {
///         println!("Method: {}", method.name);
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceSymbols {
    /// Map from CommonModule name to module information.
    ///
    /// Case-insensitive lookup is handled by Name type.
    pub common_modules: FxHashMap<Name, CommonModuleInfo>,
}

/// Build workspace symbol index from all files in the database.
///
/// This function scans all provided files, identifies CommonModules by path,
/// and extracts their method signatures into a searchable index.
///
/// # Performance
///
/// - **Time complexity:** O(n×m) where n = files, m = average methods per file
/// - **Memory:** ~1-5 KB per module (signatures only, not bodies)
/// - **Caching:** Results are cached by Salsa and recomputed only when files change
///
/// # File Path Recognition
///
/// CommonModules are identified by path patterns:
/// - English: `CommonModules/ModuleName.bsl`
/// - Russian: `ОбщиеМодули/ИмяМодуля.bsl`
///
/// # Module Naming
///
/// This function uses a heuristic to extract module names from file paths.
/// If VFS path extraction fails (requires proper integration in ide-db),
/// files are indexed by FileId for MVP compatibility.
pub fn workspace_symbols(db: &dyn DefDatabase, files: &[FileId]) -> WorkspaceSymbols {
    let mut common_modules = FxHashMap::default();

    debug!(file_count = files.len(), "Building workspace symbol index");

    for &file_id in files {
        let module_id = ModuleId::new(file_id);
        let symbol_tree = db.symbol_tree(module_id);

        // Only index modules that have methods (likely code modules)
        let methods: Vec<_> = symbol_tree.methods().cloned().collect();
        if !methods.is_empty() {
            // Try to extract module name from file path
            // This is a best-effort approach for MVP
            let module_name = extract_module_name_from_file(db, file_id).unwrap_or_else(|| {
                // Fallback: use FileId for modules without clear path structure
                debug!(
                    file_id = ?file_id,
                    "Could not extract module name, using FileId"
                );
                Name::new(&format!("Module{}", file_id.0))
            });

            debug!(
                file_id = ?file_id,
                module_name = %module_name,
                method_count = methods.len(),
                "Indexed module"
            );

            common_modules.insert(module_name, CommonModuleInfo { file_id, module_id, methods });
        }
    }

    debug!(common_module_count = common_modules.len(), "Workspace symbol index built");

    WorkspaceSymbols { common_modules }
}

/// Extract module name from file path.
///
/// Attempts to extract CommonModule name from file path using heuristics:
/// 1. Look for "CommonModules" or "ОбщиеМодули" directory
/// 2. Extract the directory name following it (module name in 1C format)
/// 3. Fallback: use filename without extension
///
/// # Examples
///
/// - `CommonModules/MyModule/Ext/Module.bsl` → "MyModule"
/// - `ОбщиеМодули/ОбщегоНазначения/Ext/Module.bsl` → "ОбщегоНазначения"
/// - `src/MyFile.bsl` → "MyFile"
///
/// # Note
///
/// This is a best-effort heuristic for MVP. Full implementation would require
/// proper VFS integration in ide-db where file paths are accessible.
fn extract_module_name_from_file(db: &dyn DefDatabase, file_id: FileId) -> Option<Name> {
    // Try to get VFS path through source root
    // This is a workaround since we don't have direct VFS access in hir-def
    let file_source_root_input = db.file_source_root_input(file_id);
    let source_root_id = file_source_root_input.source_root_id(db);
    let source_root_input = db.source_root_input(source_root_id);
    let source_root = source_root_input.root(db);
    let file_set = source_root.file_set();

    // Get path from FileSet
    let vfs_path = file_set.path_for_file(&file_id)?;
    let path_str = vfs_path.as_path().to_string_lossy();

    debug!(path = %path_str, "Extracting module name from path");

    // Look for CommonModules directory (English or Russian)
    if let Some(pos) = path_str.find("CommonModules/").or_else(|| path_str.find("ОбщиеМодули/"))
    {
        let after_common = if path_str[pos..].starts_with("CommonModules/") {
            &path_str[pos + "CommonModules/".len()..]
        } else {
            &path_str[pos + "ОбщиеМодули/".len()..]
        };

        // Extract module name (first path component after CommonModules)
        if let Some(slash_pos) = after_common.find('/') {
            let module_name = &after_common[..slash_pos];
            debug!(module_name, "Extracted from CommonModules path");
            return Some(Name::new(module_name));
        }
    }

    // Fallback: use filename without extension
    if let Some(filename) = path_str.rsplit('/').next() {
        if let Some(name_without_ext) = filename.strip_suffix(".bsl") {
            debug!(module_name = name_without_ext, "Extracted from filename");
            return Some(Name::new(name_without_ext));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use vfs::{FileSet, VfsPath};

    #[test]
    fn test_common_module_info_creation() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);
        let methods = vec![];

        let info = CommonModuleInfo { file_id, module_id, methods };

        assert_eq!(info.file_id, file_id);
        assert_eq!(info.module_id, module_id);
        assert_eq!(info.methods.len(), 0);
    }

    fn make_file_set(entries: &[(u32, &str)]) -> FileSet {
        let mut fs = FileSet::new();
        for (id, path) in entries {
            fs.insert(FileId(*id), VfsPath::new(std::path::PathBuf::from(path)));
        }
        fs
    }

    #[test]
    fn is_bsl_source_accepts_bsl_extension() {
        let fs = make_file_set(&[
            (1, "/ws/CommonModules/MyModule/Ext/Module.bsl"),
            (2, "/ws/Documents/X/Ext/ObjectModule.bsl"),
        ]);
        assert!(is_bsl_source(&fs, FileId(1)));
        assert!(is_bsl_source(&fs, FileId(2)));
    }

    #[test]
    fn is_bsl_source_rejects_non_bsl_files() {
        // Regression: workspace_symbols/_index used to feed these to the BSL
        // parser, which triggered the iteration guard on large XML.
        let fs = make_file_set(&[
            (10, "/ws/Roles/R/Ext/Rights.xml"),
            (11, "/ws/InformationRegisters/X/Templates/T/Ext/Template.xml"),
            (12, "/ws/Configuration.xml"),
            (13, "/ws/README.md"),
            (14, "/ws/notes.txt"),
            (15, "/ws/NoExtensionAtAll"),
        ]);
        for id in 10..=15 {
            assert!(!is_bsl_source(&fs, FileId(id)), "FileId({id}) should be filtered out");
        }
    }

    #[test]
    fn is_bsl_source_is_case_insensitive() {
        // 1C Designer ships files with `.bsl` consistently, but code on
        // case-insensitive filesystems (Windows, macOS default) can still
        // surface `.BSL` / `.Bsl`. These must be treated as BSL.
        let fs = make_file_set(&[
            (20, "/ws/CommonModules/A/Ext/Module.BSL"),
            (21, "/ws/CommonModules/B/Ext/Module.Bsl"),
        ]);
        assert!(is_bsl_source(&fs, FileId(20)));
        assert!(is_bsl_source(&fs, FileId(21)));
    }

    #[test]
    fn is_bsl_source_rejects_unknown_file_id() {
        let fs = make_file_set(&[(1, "/ws/a.bsl")]);
        assert!(!is_bsl_source(&fs, FileId(999)));
    }
}
