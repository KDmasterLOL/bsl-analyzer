//! High-level Intermediate Representation for bsl-analyzer.
//!
//! This crate provides a high-level API for semantic analysis.

pub use hir_def::{MethodData, MethodId, ModuleData, ModuleId, ParameterData, VariableData, VariableId};

/// A module in the HIR.
#[derive(Debug)]
pub struct Module {
    id: ModuleId,
}

impl Module {
    pub fn new(id: ModuleId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> ModuleId {
        self.id
    }
}

/// A method (procedure or function) in the HIR.
#[derive(Debug)]
pub struct Method {
    id: MethodId,
}

impl Method {
    pub fn new(id: MethodId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> MethodId {
        self.id
    }
}

/// A variable in the HIR.
#[derive(Debug)]
pub struct Variable {
    id: VariableId,
}

impl Variable {
    pub fn new(id: VariableId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> VariableId {
        self.id
    }
}

/// Semantics API for IDE features.
#[derive(Debug)]
pub struct Semantics<'db, DB> {
    #[allow(dead_code)] // TODO: будет использоваться в semantic analysis
    db: &'db DB,
}

impl<'db, DB> Semantics<'db, DB> {
    pub fn new(db: &'db DB) -> Self {
        Self { db }
    }
}

// TODO: Implement semantic analysis
