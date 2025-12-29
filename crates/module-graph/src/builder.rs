//! Module graph builder with cycle detection.

use crate::graph::{Dependency, DependencyKind, ModuleGraph, ModuleGraphData, ModuleKind};
use la_arena::{Arena, Idx};
use rustc_hash::FxHashSet;
use std::fmt;
use vfs::FileId;

/// Unique identifier for a module during graph construction.
pub type ModuleBuilderId = Idx<ModuleBuilder>;

/// Builder for constructing a module graph with validation.
///
/// Provides cycle detection to prevent invalid graphs.
/// Use [`ModuleGraphBuilder::build`] to produce the final [`ModuleGraph`].
#[derive(Default)]
pub struct ModuleGraphBuilder {
    arena: Arena<ModuleBuilder>,
}

/// Module data during graph construction.
#[derive(Debug, Clone)]
pub struct ModuleBuilder {
    pub file_id: FileId,
    pub name: String,
    pub kind: ModuleKind,
    pub dependencies: Vec<(ModuleBuilderId, DependencyKind)>,
}

/// Error returned when a cyclic dependency is detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CyclicDependencyError {
    /// The path forming the cycle (from → to → ... → from).
    pub path: Vec<ModuleBuilderId>,
}

impl fmt::Display for CyclicDependencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cyclic dependency detected: ")?;
        for (i, &module_id) in self.path.iter().enumerate() {
            if i > 0 {
                write!(f, " → ")?;
            }
            write!(f, "{:?}", module_id)?;
        }
        Ok(())
    }
}

impl std::error::Error for CyclicDependencyError {}

impl ModuleGraphBuilder {
    /// Creates a new empty module graph builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a module to the graph.
    ///
    /// Returns a builder ID that can be used to reference this module
    /// when adding dependencies.
    pub fn add_module(
        &mut self,
        file_id: FileId,
        name: String,
        kind: ModuleKind,
    ) -> ModuleBuilderId {
        self.arena.alloc(ModuleBuilder { file_id, name, kind, dependencies: Vec::new() })
    }

    /// Adds a dependency edge from one module to another.
    ///
    /// # Errors
    ///
    /// Returns [`CyclicDependencyError`] if adding this edge would create a cycle.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut builder = ModuleGraphBuilder::new();
    /// let m1 = builder.add_module(...);
    /// let m2 = builder.add_module(...);
    ///
    /// builder.add_dependency(m1, m2, DependencyKind::DirectCall)?;
    /// // This would fail with CyclicDependencyError:
    /// // builder.add_dependency(m2, m1, DependencyKind::DirectCall)?;
    /// ```
    pub fn add_dependency(
        &mut self,
        from: ModuleBuilderId,
        to: ModuleBuilderId,
        kind: DependencyKind,
    ) -> Result<(), CyclicDependencyError> {
        // Check for cycles using DFS
        if let Some(path) = self.find_path(to, from) {
            // Path exists from `to` to `from`, so adding `from` → `to` creates a cycle
            return Err(CyclicDependencyError { path });
        }

        // Safe to add edge
        self.arena[from].dependencies.push((to, kind));
        Ok(())
    }

    /// Builds the final module graph.
    ///
    /// Converts builder IDs to final graph IDs and constructs reverse dependency index.
    pub fn build(self) -> ModuleGraph {
        use crate::graph::ModuleGraphId;

        let mut graph = ModuleGraph::new();

        // Map builder IDs to final graph IDs
        let mut builder_to_graph: Vec<ModuleGraphId> = Vec::new();

        // Add all modules with their dependencies in one pass
        for (_builder_id, builder_module) in self.arena.iter() {
            // Map builder dependency IDs to graph IDs
            let dependencies: Vec<Dependency> = builder_module
                .dependencies
                .iter()
                .map(|&(target_builder_id, kind)| {
                    // For dependencies, we need to use the mapping that will be built
                    // For now, use a placeholder - we'll fix this in a moment
                    let target_idx = target_builder_id.into_raw().into_u32() as usize;
                    Dependency { target: ModuleGraphId::from_raw((target_idx as u32).into()), kind }
                })
                .collect();

            let graph_id = graph.add_module(ModuleGraphData {
                id: ModuleGraphId::from_raw((builder_to_graph.len() as u32).into()),
                file_id: builder_module.file_id,
                name: builder_module.name.clone(),
                dependencies,
                kind: builder_module.kind,
            });

            builder_to_graph.push(graph_id);
        }

        // Build reverse dependency index
        graph.build_reverse_deps();

        graph
    }

    /// Finds a path from `from` to `to` using DFS.
    ///
    /// Returns `None` if no path exists.
    /// Returns `Some(path)` where path includes both endpoints.
    fn find_path(
        &self,
        from: ModuleBuilderId,
        to: ModuleBuilderId,
    ) -> Option<Vec<ModuleBuilderId>> {
        let mut visited = FxHashSet::default();
        let mut path = Vec::new();

        if self.dfs(from, to, &mut visited, &mut path) {
            path.push(from); // Add starting point
            path.reverse(); // Reverse to get from → ... → to order
            Some(path)
        } else {
            None
        }
    }

    /// DFS helper for path finding.
    ///
    /// Returns true if a path from `current` to `target` exists.
    /// Populates `path` with the nodes along the path (in reverse order).
    fn dfs(
        &self,
        current: ModuleBuilderId,
        target: ModuleBuilderId,
        visited: &mut FxHashSet<ModuleBuilderId>,
        path: &mut Vec<ModuleBuilderId>,
    ) -> bool {
        if current == target {
            return true;
        }

        if !visited.insert(current) {
            return false; // Already visited
        }

        for &(dep_id, _) in &self.arena[current].dependencies {
            if self.dfs(dep_id, target, visited, path) {
                path.push(dep_id);
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_builder() {
        let builder = ModuleGraphBuilder::new();
        let graph = builder.build();
        assert!(graph.is_empty());
    }

    #[test]
    fn test_add_modules() {
        let mut builder = ModuleGraphBuilder::new();

        let m1 = builder.add_module(FileId(0), "Module1".to_string(), ModuleKind::CommonModule);
        let m2 = builder.add_module(FileId(1), "Module2".to_string(), ModuleKind::CommonModule);

        builder.add_dependency(m1, m2, DependencyKind::DirectCall).unwrap();

        let graph = builder.build();

        assert_eq!(graph.len(), 2);
        assert_eq!(graph.dependencies(graph.by_file(FileId(0)).unwrap()).len(), 1);
    }

    #[test]
    fn test_cycle_detection_simple() {
        let mut builder = ModuleGraphBuilder::new();

        let m1 = builder.add_module(FileId(0), "Module1".to_string(), ModuleKind::CommonModule);
        let m2 = builder.add_module(FileId(1), "Module2".to_string(), ModuleKind::CommonModule);

        // Add m1 → m2
        builder.add_dependency(m1, m2, DependencyKind::DirectCall).unwrap();

        // Try to add m2 → m1 (would create cycle)
        let result = builder.add_dependency(m2, m1, DependencyKind::DirectCall);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.path.len(), 2); // m2 → m1 → m2
    }

    #[test]
    fn test_cycle_detection_transitive() {
        let mut builder = ModuleGraphBuilder::new();

        let m1 = builder.add_module(FileId(0), "Module1".to_string(), ModuleKind::CommonModule);
        let m2 = builder.add_module(FileId(1), "Module2".to_string(), ModuleKind::CommonModule);
        let m3 = builder.add_module(FileId(2), "Module3".to_string(), ModuleKind::CommonModule);

        // Add m1 → m2 → m3
        builder.add_dependency(m1, m2, DependencyKind::DirectCall).unwrap();
        builder.add_dependency(m2, m3, DependencyKind::DirectCall).unwrap();

        // Try to add m3 → m1 (would create cycle: m1 → m2 → m3 → m1)
        let result = builder.add_dependency(m3, m1, DependencyKind::DirectCall);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.path.len(), 3); // m3 → m1 → m2 → m3
    }

    #[test]
    fn test_no_false_positive_cycles() {
        let mut builder = ModuleGraphBuilder::new();

        let m1 = builder.add_module(FileId(0), "Module1".to_string(), ModuleKind::CommonModule);
        let m2 = builder.add_module(FileId(1), "Module2".to_string(), ModuleKind::CommonModule);
        let m3 = builder.add_module(FileId(2), "Module3".to_string(), ModuleKind::CommonModule);

        // Add m1 → m2 and m1 → m3 (no cycle)
        builder.add_dependency(m1, m2, DependencyKind::DirectCall).unwrap();
        builder.add_dependency(m1, m3, DependencyKind::DirectCall).unwrap();

        // This should succeed (no cycle)
        let result = builder.add_dependency(m2, m3, DependencyKind::DirectCall);

        assert!(result.is_ok());
    }
}
