//! Core module graph structures and implementation.

use la_arena::{Arena, Idx};
use rustc_hash::FxHashMap;
use vfs::FileId;

/// Unique identifier for a module in the graph.
pub type ModuleGraphId = Idx<ModuleGraphData>;

/// Dependency graph of BSL modules.
///
/// Stores modules using Arena pattern for efficient memory layout
/// and stable IDs. Provides indices for fast lookups by file ID and name.
#[derive(Debug, Clone, Default)]
pub struct ModuleGraph {
    /// Arena storage for all modules (efficient, stable IDs).
    modules: Arena<ModuleGraphData>,

    /// Index: FileId → ModuleGraphId (1:1 mapping in BSL).
    file_to_module: FxHashMap<FileId, ModuleGraphId>,

    /// Index: normalized name (lowercase) → ModuleGraphId.
    /// Case-insensitive for BSL compatibility.
    name_to_module: FxHashMap<String, ModuleGraphId>,

    /// Reverse dependency index: ModuleGraphId → Vec<ModuleGraphId>.
    /// Maps each module to modules that depend on it.
    reverse_deps: FxHashMap<ModuleGraphId, Vec<ModuleGraphId>>,
}

/// Data for a single module in the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleGraphData {
    /// Unique ID in the graph.
    pub id: ModuleGraphId,

    /// File ID from the VFS.
    pub file_id: FileId,

    /// Module name (e.g., "CommonModule.MyModule").
    pub name: String,

    /// Direct dependencies of this module.
    pub dependencies: Vec<Dependency>,

    /// Kind of module (CommonModule, ObjectModule, etc.).
    pub kind: ModuleKind,
}

/// A dependency edge in the graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Dependency {
    /// ID of the target module.
    pub target: ModuleGraphId,

    /// Type of dependency.
    pub kind: DependencyKind,
}

/// Types of dependencies between modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DependencyKind {
    /// Direct function call (e.g., ОбщегоНазначения.Метод()).
    DirectCall,

    /// Import via #Использовать directive.
    Import,

    /// Dependency via metadata (requires Iteration 11).
    Metadata,
}

/// Types of BSL modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleKind {
    /// Common module (общий модуль).
    CommonModule,

    /// Object module (модуль объекта).
    /// Requires metadata to distinguish - for now treat as CommonModule.
    ObjectModule,

    /// Form module (модуль формы).
    /// Requires metadata to distinguish - for now treat as CommonModule.
    FormModule,

    /// Manager module (модуль менеджера).
    /// Requires metadata to distinguish - for now treat as CommonModule.
    ManagerModule,

    /// Command module (модуль команды).
    /// Requires metadata to distinguish - for now treat as CommonModule.
    CommandModule,

    /// Unknown module type (fallback).
    Unknown,
}

impl ModuleGraph {
    /// Creates an empty module graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the total number of modules in the graph.
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Returns true if the graph contains no modules.
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Gets module data by ID.
    ///
    /// # Panics
    ///
    /// Panics if the ID is invalid.
    pub fn get(&self, id: ModuleGraphId) -> &ModuleGraphData {
        &self.modules[id]
    }

    /// Looks up a module by file ID.
    ///
    /// Returns `None` if no module corresponds to this file.
    pub fn by_file(&self, file_id: FileId) -> Option<ModuleGraphId> {
        self.file_to_module.get(&file_id).copied()
    }

    /// Looks up a module by name (case-insensitive).
    ///
    /// Returns `None` if no module with this name exists.
    pub fn by_name(&self, name: &str) -> Option<ModuleGraphId> {
        self.name_to_module.get(&name.to_lowercase()).copied()
    }

    /// Returns the direct dependencies of a module.
    pub fn dependencies(&self, id: ModuleGraphId) -> &[Dependency] {
        &self.modules[id].dependencies
    }

    /// Returns modules that directly depend on this module (reverse dependencies).
    pub fn reverse_dependencies(&self, id: ModuleGraphId) -> &[ModuleGraphId] {
        self.reverse_deps.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Returns an iterator over all module IDs in the graph.
    pub fn all_modules(&self) -> impl Iterator<Item = ModuleGraphId> + '_ {
        self.modules.iter().map(|(id, _)| id)
    }

    /// Returns an iterator over all modules and their data.
    pub fn iter(&self) -> impl Iterator<Item = (ModuleGraphId, &ModuleGraphData)> + '_ {
        self.modules.iter()
    }

    // Internal methods used by ModuleGraphBuilder

    pub(crate) fn add_module(&mut self, data: ModuleGraphData) -> ModuleGraphId {
        let id = self.modules.alloc(data);

        // Update indices
        let module = &self.modules[id];
        self.file_to_module.insert(module.file_id, id);
        self.name_to_module.insert(module.name.to_lowercase(), id);

        id
    }

    pub(crate) fn build_reverse_deps(&mut self) {
        self.reverse_deps.clear();

        for (id, module) in self.modules.iter() {
            for dep in &module.dependencies {
                self.reverse_deps.entry(dep.target).or_default().push(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vfs::FileId;

    #[test]
    fn test_empty_graph() {
        let graph = ModuleGraph::new();
        assert!(graph.is_empty());
        assert_eq!(graph.len(), 0);
    }

    #[test]
    fn test_add_module() {
        let mut graph = ModuleGraph::new();

        let file_id = FileId(0);
        let module_data = ModuleGraphData {
            id: ModuleGraphId::from_raw(0.into()),
            file_id,
            name: "CommonModule.Test".to_string(),
            dependencies: vec![],
            kind: ModuleKind::CommonModule,
        };

        let id = graph.add_module(module_data);

        assert_eq!(graph.len(), 1);
        assert_eq!(graph.get(id).name, "CommonModule.Test");
        assert_eq!(graph.by_file(file_id), Some(id));
        assert_eq!(graph.by_name("CommonModule.Test"), Some(id));
        assert_eq!(graph.by_name("commonmodule.test"), Some(id)); // Case-insensitive
    }

    #[test]
    fn test_reverse_dependencies() {
        let mut graph = ModuleGraph::new();

        let file_id1 = FileId(0);
        let file_id2 = FileId(1);

        let module2_id = ModuleGraphId::from_raw(1.into());

        let module1_data = ModuleGraphData {
            id: ModuleGraphId::from_raw(0.into()),
            file_id: file_id1,
            name: "Module1".to_string(),
            dependencies: vec![Dependency { target: module2_id, kind: DependencyKind::DirectCall }],
            kind: ModuleKind::CommonModule,
        };

        let module2_data = ModuleGraphData {
            id: module2_id,
            file_id: file_id2,
            name: "Module2".to_string(),
            dependencies: vec![],
            kind: ModuleKind::CommonModule,
        };

        let id1 = graph.add_module(module1_data);
        graph.add_module(module2_data);

        graph.build_reverse_deps();

        // Module2 should have Module1 as reverse dependency
        assert_eq!(graph.reverse_dependencies(module2_id), &[id1]);
        // Module1 should have no reverse dependencies
        assert!(graph.reverse_dependencies(id1).is_empty());
    }
}
