//! Module dependency graph for BSL projects.
//!
//! This crate provides infrastructure for building and analyzing
//! dependencies between BSL modules. It enables incremental CI/CD analysis
//! by identifying which modules are affected by changes.
//!
//! # Architecture
//!
//! The core of this crate is the [`ModuleGraph`], which represents
//! the dependency relationships between modules in a BSL project.
//! The graph is built using [`ModuleGraphBuilder`] which provides
//! cycle detection and validation.
//!
//! # Example
//!
//! ```ignore
//! use module_graph::{ModuleGraph, ModuleGraphBuilder, DependencyKind};
//!
//! let mut builder = ModuleGraphBuilder::default();
//! let module1 = builder.add_module(file_id1, "Module1".to_string(), ModuleKind::CommonModule);
//! let module2 = builder.add_module(file_id2, "Module2".to_string(), ModuleKind::CommonModule);
//!
//! builder.add_dependency(module1, module2, DependencyKind::DirectCall)?;
//!
//! let graph = builder.build();
//! ```

mod build;
mod builder;
mod deps;
mod graph;
mod incremental;

pub use build::build_module_graph;
pub use builder::{CyclicDependencyError, ModuleBuilderId, ModuleGraphBuilder};
pub use deps::DependencyExtractor;
pub use graph::{
    Dependency, DependencyKind, ModuleGraph, ModuleGraphData, ModuleGraphId, ModuleKind,
};

#[cfg(test)]
mod tests;
