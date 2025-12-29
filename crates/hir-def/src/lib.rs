//! HIR definitions for bsl-analyzer.
//!
//! This crate contains definitions and data structures for the
//! High-level Intermediate Representation.

pub mod item_tree;
pub mod name;
pub mod resolver;
pub mod scope;

use std::sync::Arc;

use base_db::SourceDatabase;
use vfs::FileId;

pub use item_tree::ItemTree;
pub use name::Name;

// ========== Database Traits ==========

/// Database trait for HIR queries.
///
/// This trait extends SourceDatabase with queries for ItemTree and module-level data.
pub trait DefDatabase: SourceDatabase {
    /// Get ItemTree for a file (main query).
    ///
    /// ItemTree is the "invalidation barrier" - it only changes when signatures change,
    /// not when procedure bodies are edited.
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree>;

    /// Get module data for a module (derived query).
    ///
    /// In BSL, 1 file = 1 module, so ModuleId contains FileId.
    fn module_data(&self, module_id: ModuleId) -> Arc<ModuleData>;
}

// ========== Module & Item Identifiers ==========

/// Module identifier.
///
/// In BSL: 1 file = 1 module (no nested modules like in Rust).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId {
    pub file_id: FileId,
}

impl ModuleId {
    pub fn new(file_id: FileId) -> Self {
        Self { file_id }
    }
}

/// Method identifier (procedure or function).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MethodId {
    pub module: ModuleId,
    /// Index in ItemTree.top_level_items()
    pub local_id: u32,
}

/// Variable identifier (module-level variable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariableId {
    pub module: ModuleId,
    /// Index in ItemTree.top_level_items()
    pub local_id: u32,
}

// ========== Data Structures ==========

/// Data about a module (derived from ItemTree).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleData {
    pub file_id: FileId,
    pub name: Option<Name>,
    pub procedures: Vec<MethodId>,
    pub functions: Vec<MethodId>,
    pub variables: Vec<VariableId>,
}

impl ModuleData {
    /// Convert ItemTree → ModuleData.
    pub fn from_item_tree(module_id: ModuleId, tree: Arc<ItemTree>) -> Self {
        let mut procedures = Vec::new();
        let mut functions = Vec::new();
        let mut variables = Vec::new();

        for (idx, item) in tree.top_level_items().iter().enumerate() {
            match item {
                item_tree::ModItem::Procedure(_) => {
                    procedures.push(MethodId { module: module_id, local_id: idx as u32 });
                }
                item_tree::ModItem::Function(_) => {
                    functions.push(MethodId { module: module_id, local_id: idx as u32 });
                }
                item_tree::ModItem::Variable(_) => {
                    variables.push(VariableId { module: module_id, local_id: idx as u32 });
                }
            }
        }

        ModuleData {
            file_id: module_id.file_id,
            name: None, // TODO: Extract from metadata
            procedures,
            functions,
            variables,
        }
    }
}

/// Data about a method (procedure or function).
#[derive(Debug, Clone)]
pub struct MethodData {
    pub name: String,
    pub is_function: bool,
    pub is_export: bool,
    pub parameters: Vec<ParameterData>,
}

/// Data about a parameter.
#[derive(Debug, Clone)]
pub struct ParameterData {
    pub name: String,
    pub is_val: bool,
    pub has_default: bool,
}

/// Data about a variable.
#[derive(Debug, Clone)]
pub struct VariableData {
    pub name: String,
    pub is_export: bool,
}

// TODO: Add more HIR structures
