//! Incremental analysis algorithms.
//!
//! This module provides algorithms for determining which modules need to be re-analyzed
//! when files change, enabling efficient incremental CI/CD workflows.

use std::collections::VecDeque;

use rustc_hash::FxHashSet;
use tracing::{debug, info};
use vfs::FileId;

use crate::{ModuleGraph, ModuleGraphId};

impl ModuleGraph {
    /// Computes the set of modules affected by changes to specific files.
    ///
    /// This is the core algorithm for incremental CI/CD analysis.
    ///
    /// # Algorithm
    ///
    /// 1. **Direct impact**: Add all changed modules to the affected set
    /// 2. **Reverse propagation**: BFS through reverse dependencies (modules that depend on changed modules)
    /// 3. **Context inclusion**: Add direct dependencies of affected modules (for diagnostic context)
    ///
    /// # Performance
    ///
    /// For a typical change affecting 1-5 modules in a project with 25,000 modules:
    /// - Without incremental: analyze all 25,000 modules (~10-15 seconds)
    /// - With incremental: analyze 10-50 modules (~0.5-2 seconds, **5x-30x faster**)
    ///
    /// # Example
    ///
    /// ```ignore
    /// // User edits CommonModules/Utils.bsl
    /// let changed_files = vec![FileId(42)];
    /// let affected = graph.affected_modules(&changed_files);
    ///
    /// // affected includes:
    /// // - Utils (changed)
    /// // - All modules that call Utils.Something()
    /// // - All modules called by Utils (for context)
    /// ```
    pub fn affected_modules(&self, changed_files: &[FileId]) -> Vec<ModuleGraphId> {
        let _span =
            tracing::info_span!("affected_modules", changed_count = changed_files.len()).entered();

        let mut affected = FxHashSet::default();
        let mut queue = VecDeque::new();

        // Step 1: Add directly changed modules
        for &file_id in changed_files {
            if let Some(module_id) = self.by_file(file_id) {
                debug!(file_id = ?file_id, module = ?self.get(module_id).name, "Changed module");
                affected.insert(module_id);
                queue.push_back(module_id);
            }
        }

        let direct_count = affected.len();

        // Step 2: BFS through reverse dependencies
        // If module A depends on module B, and B changed, then A needs re-analysis
        while let Some(module_id) = queue.pop_front() {
            for &dependent in self.reverse_dependencies(module_id) {
                if affected.insert(dependent) {
                    debug!(
                        dependent = ?self.get(dependent).name,
                        changed = ?self.get(module_id).name,
                        "Module affected by change"
                    );
                    queue.push_back(dependent);
                }
            }
        }

        let propagated_count = affected.len() - direct_count;

        // Step 3: Add direct dependencies for context
        // If we're analyzing module A, we need its dependencies loaded for context
        let mut with_context = affected.clone();
        for &module_id in &affected {
            for dep in self.dependencies(module_id) {
                if with_context.insert(dep.target) {
                    debug!(
                        dependency = ?self.get(dep.target).name,
                        module = ?self.get(module_id).name,
                        "Added dependency for context"
                    );
                }
            }
        }

        let context_count = with_context.len() - affected.len();

        info!(
            total_modules = self.len(),
            changed = direct_count,
            propagated = propagated_count,
            context = context_count,
            affected = with_context.len(),
            reduction_factor = format!("{:.1}x", self.len() as f64 / with_context.len() as f64),
            "Computed affected modules"
        );

        with_context.into_iter().collect()
    }

    /// Computes all modules that this module depends on (transitively).
    ///
    /// Uses DFS to traverse the dependency graph.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Module A → B → C
    /// //         → D
    /// let deps = graph.transitive_dependencies(module_a);
    /// // deps = [B, C, D]
    /// ```
    pub fn transitive_dependencies(&self, module_id: ModuleGraphId) -> Vec<ModuleGraphId> {
        let mut visited = FxHashSet::default();
        let mut result = Vec::new();

        self.dfs_dependencies(module_id, &mut visited, &mut result);

        result
    }

    /// Computes all modules that depend on this module (transitively).
    ///
    /// Uses DFS to traverse reverse dependencies.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Module C is used by B, B is used by A
    /// let dependents = graph.transitive_reverse_dependencies(module_c);
    /// // dependents = [B, A]
    /// ```
    pub fn transitive_reverse_dependencies(&self, module_id: ModuleGraphId) -> Vec<ModuleGraphId> {
        let mut visited = FxHashSet::default();
        let mut result = Vec::new();

        self.dfs_reverse_dependencies(module_id, &mut visited, &mut result);

        result
    }

    // Internal DFS helper for dependencies
    fn dfs_dependencies(
        &self,
        module_id: ModuleGraphId,
        visited: &mut FxHashSet<ModuleGraphId>,
        result: &mut Vec<ModuleGraphId>,
    ) {
        for dep in self.dependencies(module_id) {
            if visited.insert(dep.target) {
                result.push(dep.target);
                self.dfs_dependencies(dep.target, visited, result);
            }
        }
    }

    // Internal DFS helper for reverse dependencies
    fn dfs_reverse_dependencies(
        &self,
        module_id: ModuleGraphId,
        visited: &mut FxHashSet<ModuleGraphId>,
        result: &mut Vec<ModuleGraphId>,
    ) {
        for &dependent in self.reverse_dependencies(module_id) {
            if visited.insert(dependent) {
                result.push(dependent);
                self.dfs_reverse_dependencies(dependent, visited, result);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DependencyKind, ModuleGraphBuilder, ModuleKind};

    #[test]
    fn test_affected_single_module() {
        // Single module changed, no dependencies
        let mut builder = ModuleGraphBuilder::new();
        let file1 = FileId(0);

        let _m1 = builder.add_module(file1, "Module1".to_string(), ModuleKind::CommonModule);

        let graph = builder.build();

        // Change module1
        let affected = graph.affected_modules(&[file1]);

        // Should only include the changed module
        assert_eq!(affected.len(), 1);

        let m1_id = graph.by_file(file1).unwrap();
        assert!(affected.contains(&m1_id));
    }

    #[test]
    fn test_affected_with_dependents() {
        // Module1 → Module2 (Module2 depends on Module1)
        // Change Module1, should affect Module2
        let mut builder = ModuleGraphBuilder::new();
        let file1 = FileId(0);
        let file2 = FileId(1);

        let m1 = builder.add_module(file1, "Module1".to_string(), ModuleKind::CommonModule);
        let m2 = builder.add_module(file2, "Module2".to_string(), ModuleKind::CommonModule);

        builder.add_dependency(m2, m1, DependencyKind::DirectCall).unwrap();

        let graph = builder.build();

        // Change Module1
        let affected = graph.affected_modules(&[file1]);

        let m1_id = graph.by_file(file1).unwrap();
        let m2_id = graph.by_file(file2).unwrap();

        // Should include both Module1 (changed) and Module2 (dependent)
        assert_eq!(affected.len(), 2);
        assert!(affected.contains(&m1_id));
        assert!(affected.contains(&m2_id));
    }

    #[test]
    fn test_affected_with_dependencies() {
        // Module1 → Module2 (Module1 depends on Module2)
        // Change Module1, should include Module2 for context
        let mut builder = ModuleGraphBuilder::new();
        let file1 = FileId(0);
        let file2 = FileId(1);

        let m1 = builder.add_module(file1, "Module1".to_string(), ModuleKind::CommonModule);
        let m2 = builder.add_module(file2, "Module2".to_string(), ModuleKind::CommonModule);

        builder.add_dependency(m1, m2, DependencyKind::DirectCall).unwrap();

        let graph = builder.build();

        // Change Module1
        let affected = graph.affected_modules(&[file1]);

        let m1_id = graph.by_file(file1).unwrap();
        let m2_id = graph.by_file(file2).unwrap();

        // Should include Module1 (changed) and Module2 (dependency for context)
        assert_eq!(affected.len(), 2);
        assert!(affected.contains(&m1_id));
        assert!(affected.contains(&m2_id));
    }

    #[test]
    fn test_affected_transitive() {
        // Module1 → Module2 → Module3 (chain)
        // Change Module1, should affect Module2 and Module3
        let mut builder = ModuleGraphBuilder::new();
        let file1 = FileId(0);
        let file2 = FileId(1);
        let file3 = FileId(2);

        let m1 = builder.add_module(file1, "Module1".to_string(), ModuleKind::CommonModule);
        let m2 = builder.add_module(file2, "Module2".to_string(), ModuleKind::CommonModule);
        let m3 = builder.add_module(file3, "Module3".to_string(), ModuleKind::CommonModule);

        builder.add_dependency(m2, m1, DependencyKind::DirectCall).unwrap();
        builder.add_dependency(m3, m2, DependencyKind::DirectCall).unwrap();

        let graph = builder.build();

        // Change Module1
        let affected = graph.affected_modules(&[file1]);

        let m1_id = graph.by_file(file1).unwrap();
        let m2_id = graph.by_file(file2).unwrap();
        let m3_id = graph.by_file(file3).unwrap();

        // Should include all three: Module1 (changed), Module2 and Module3 (transitively affected)
        assert_eq!(affected.len(), 3);
        assert!(affected.contains(&m1_id));
        assert!(affected.contains(&m2_id));
        assert!(affected.contains(&m3_id));
    }

    #[test]
    fn test_affected_multiple_changes() {
        // Change multiple independent modules
        let mut builder = ModuleGraphBuilder::new();
        let file1 = FileId(0);
        let file2 = FileId(1);
        let file3 = FileId(2);

        builder.add_module(file1, "Module1".to_string(), ModuleKind::CommonModule);
        builder.add_module(file2, "Module2".to_string(), ModuleKind::CommonModule);
        builder.add_module(file3, "Module3".to_string(), ModuleKind::CommonModule);

        let graph = builder.build();

        // Change Module1 and Module2
        let affected = graph.affected_modules(&[file1, file2]);

        // Should include both changed modules
        assert_eq!(affected.len(), 2);
    }

    #[test]
    fn test_transitive_dependencies() {
        // Module1 → Module2 → Module3
        let mut builder = ModuleGraphBuilder::new();
        let file1 = FileId(0);
        let file2 = FileId(1);
        let file3 = FileId(2);

        let m1 = builder.add_module(file1, "Module1".to_string(), ModuleKind::CommonModule);
        let m2 = builder.add_module(file2, "Module2".to_string(), ModuleKind::CommonModule);
        let m3 = builder.add_module(file3, "Module3".to_string(), ModuleKind::CommonModule);

        builder.add_dependency(m1, m2, DependencyKind::DirectCall).unwrap();
        builder.add_dependency(m2, m3, DependencyKind::DirectCall).unwrap();

        let graph = builder.build();

        let m1_id = graph.by_file(file1).unwrap();
        let m2_id = graph.by_file(file2).unwrap();
        let m3_id = graph.by_file(file3).unwrap();

        // Transitive dependencies of Module1
        let deps = graph.transitive_dependencies(m1_id);
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&m2_id));
        assert!(deps.contains(&m3_id));
    }

    #[test]
    fn test_transitive_reverse_dependencies() {
        // Module1 → Module2 → Module3
        let mut builder = ModuleGraphBuilder::new();
        let file1 = FileId(0);
        let file2 = FileId(1);
        let file3 = FileId(2);

        let m1 = builder.add_module(file1, "Module1".to_string(), ModuleKind::CommonModule);
        let m2 = builder.add_module(file2, "Module2".to_string(), ModuleKind::CommonModule);
        let m3 = builder.add_module(file3, "Module3".to_string(), ModuleKind::CommonModule);

        builder.add_dependency(m1, m2, DependencyKind::DirectCall).unwrap();
        builder.add_dependency(m2, m3, DependencyKind::DirectCall).unwrap();

        let graph = builder.build();

        let m1_id = graph.by_file(file1).unwrap();
        let m2_id = graph.by_file(file2).unwrap();
        let m3_id = graph.by_file(file3).unwrap();

        // Transitive reverse dependencies of Module3
        let deps = graph.transitive_reverse_dependencies(m3_id);
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&m1_id));
        assert!(deps.contains(&m2_id));
    }

    #[test]
    fn test_affected_diamond() {
        // Diamond dependency:
        //     Module1
        //     /     \
        // Module2  Module3
        //     \     /
        //     Module4
        //
        // Change Module4, should affect all modules
        let mut builder = ModuleGraphBuilder::new();
        let file1 = FileId(0);
        let file2 = FileId(1);
        let file3 = FileId(2);
        let file4 = FileId(3);

        let m1 = builder.add_module(file1, "Module1".to_string(), ModuleKind::CommonModule);
        let m2 = builder.add_module(file2, "Module2".to_string(), ModuleKind::CommonModule);
        let m3 = builder.add_module(file3, "Module3".to_string(), ModuleKind::CommonModule);
        let m4 = builder.add_module(file4, "Module4".to_string(), ModuleKind::CommonModule);

        builder.add_dependency(m2, m4, DependencyKind::DirectCall).unwrap();
        builder.add_dependency(m3, m4, DependencyKind::DirectCall).unwrap();
        builder.add_dependency(m1, m2, DependencyKind::DirectCall).unwrap();
        builder.add_dependency(m1, m3, DependencyKind::DirectCall).unwrap();

        let graph = builder.build();

        // Change Module4
        let affected = graph.affected_modules(&[file4]);

        // Should affect all 4 modules
        assert_eq!(affected.len(), 4);
    }
}
