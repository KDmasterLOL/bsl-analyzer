//! Graph building from database.
//!
//! This module provides functionality to build a ModuleGraph from a SourceDatabase.

use rustc_hash::FxHashMap;
use tracing::{debug, info};

use crate::{deps::DependencyExtractor, DependencyKind, ModuleGraphBuilder, ModuleKind};

/// Extracts module name from file path.
///
/// Examples:
/// - `CommonModules/MyModule.bsl` → `"MyModule"`
/// - `CommonModules/ОбщегоНазначения.bsl` → `"ОбщегоНазначения"`
/// - `Documents/Invoice/ObjectModule.bsl` → `"Invoice"` (for future metadata support)
///
/// For now, we extract the base filename without extension.
///
/// # TODO: Iteration 11 - Metadata Support (CRITICAL LIMITATION)
///
/// **Current implementation DOES NOT WORK for real 1C projects!**
///
/// Real 1C configuration dump structure (from Configurator):
/// ```text
/// src/cf/CommonModules/АвтономнаяРабота/Ext/Module.bsl
/// src/cf/Catalogs/Номенклатура/ManagerModule.bsl
/// src/cf/Documents/ПриходТовара/ObjectModule.bsl
/// ```
///
/// What we extract NOW:
/// - `CommonModules/АвтономнаяРабота/Ext/Module.bsl` → `"Module"` ❌
/// - `Catalogs/Номенклатура/ManagerModule.bsl` → `"ManagerModule"` ❌
///
/// What we NEED (requires Configuration.xml parsing):
/// - Extract from path: `CommonModules/АвтономнаяРабота/...` → `"АвтономнаяРабота"` ✅
/// - Map metadata class: `Справочники.Номенклатура` → `Catalogs/Номенклатура/ManagerModule.bsl` ✅
/// - Handle Russian/English names: `Справочники` ↔ `Catalogs`, `Документы` ↔ `Documents`
///
/// **Iteration 11 will implement:**
/// 1. Configuration.xml parser (metadata structure)
/// 2. Metadata-to-path mapping (Номенклатура → ManagerModule.bsl)
/// 3. Dependency pattern recognition (`<Class>.<Object>.<Method>()`)
/// 4. Proper module name resolution from directory structure
///
/// **For now (Iteration 9.5):** ModuleGraph works ONLY for simple test cases,
/// NOT for real 1C projects. This is a known limitation.
pub fn extract_module_name_from_path(path: &str) -> String {
    // Get the filename from the path
    let filename = path.rsplit('/').next().unwrap_or(path);

    // Remove .bsl extension
    let name = filename.strip_suffix(".bsl").unwrap_or(filename);

    // For now, just return the filename
    // In the future (Iteration 11), we'll parse metadata to get proper module names
    name.to_string()
}

/// Builds a module graph from a source database and source root.
///
/// # Algorithm
///
/// 1. Build name index: path → module name → FileId (case-insensitive)
/// 2. Add all modules to the builder
/// 3. Extract dependencies from each module's AST
/// 4. Add dependency edges to the builder
/// 5. Build and return the final graph
///
/// # Cycle Handling
///
/// If a cyclic dependency is detected, a warning is logged and the edge is skipped.
/// The graph continues to be built without that edge.
pub fn build_module_graph(
    db: &dyn base_db::RootQueryDb,
    source_root: &base_db::SourceRoot,
) -> crate::ModuleGraph {
    let _span = tracing::info_span!("build_module_graph").entered();

    let mut builder = ModuleGraphBuilder::new();

    // Step 1: Build name index (path → name, name → FileId)
    let mut name_to_file = FxHashMap::default();
    let mut file_to_name = FxHashMap::default();

    for file_id in source_root.iter() {
        // Get path from FileSet
        if let Some(path) = source_root.file_set().path_for_file(&file_id) {
            // Convert path to string (use to_string_lossy for non-UTF8 paths)
            let path_str = path.as_path().to_string_lossy();
            let module_name = extract_module_name_from_path(&path_str);

            // Store mappings
            name_to_file.insert(module_name.to_lowercase(), file_id);
            file_to_name.insert(file_id, module_name);
        }
    }

    debug!(modules_count = file_to_name.len(), "Indexed module names");

    // Step 2: Add all modules to the builder
    let mut file_to_builder_id = FxHashMap::default();

    for (file_id, module_name) in &file_to_name {
        let builder_id =
            builder.add_module(*file_id, module_name.clone(), ModuleKind::CommonModule);
        file_to_builder_id.insert(*file_id, builder_id);
    }

    info!(modules_added = file_to_builder_id.len(), "Added modules to builder");

    // Step 3: Extract and add dependencies
    let mut total_deps = 0;
    let mut skipped_cycles = 0;

    for file_id in source_root.iter() {
        // Parse the file
        let parse = db.parse(file_id);

        // Extract dependencies
        let deps = DependencyExtractor::extract(&parse.syntax_node());

        debug!(file_id = ?file_id, deps_count = deps.len(), "Extracted dependencies");

        // Add dependency edges
        for dep_name in deps {
            // Resolve dependency name to FileId (case-insensitive)
            if let Some(&target_file_id) = name_to_file.get(&dep_name.to_lowercase()) {
                let from = file_to_builder_id[&file_id];
                let to = file_to_builder_id[&target_file_id];

                // Try to add dependency (may fail due to cycle)
                match builder.add_dependency(from, to, DependencyKind::DirectCall) {
                    Ok(_) => {
                        total_deps += 1;
                    }
                    Err(err) => {
                        debug!(
                            from = ?file_to_name[&file_id],
                            to = ?dep_name,
                            error = ?err,
                            "Skipped cyclic dependency"
                        );
                        skipped_cycles += 1;
                    }
                }
            } else {
                debug!(
                    from = ?file_to_name[&file_id],
                    to = ?dep_name,
                    "Dependency target not found (external module or typo)"
                );
            }
        }
    }

    info!(
        total_dependencies = total_deps,
        skipped_cycles = skipped_cycles,
        "Finished building dependency graph"
    );

    // Step 4: Build final graph
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_module_name_simple() {
        assert_eq!(extract_module_name_from_path("CommonModules/MyModule.bsl"), "MyModule");
    }

    #[test]
    fn test_extract_module_name_russian() {
        assert_eq!(
            extract_module_name_from_path("CommonModules/ОбщегоНазначения.bsl"),
            "ОбщегоНазначения"
        );
    }

    #[test]
    fn test_extract_module_name_nested() {
        assert_eq!(
            extract_module_name_from_path("Documents/Invoice/ObjectModule.bsl"),
            "ObjectModule"
        );
    }

    #[test]
    fn test_extract_module_name_no_extension() {
        assert_eq!(extract_module_name_from_path("CommonModules/MyModule"), "MyModule");
    }

    #[test]
    fn test_extract_module_name_just_filename() {
        assert_eq!(extract_module_name_from_path("MyModule.bsl"), "MyModule");
    }
}
