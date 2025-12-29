//! Symbol tree for fast symbol lookup.
//!
//! The SymbolTree provides O(1) case-insensitive lookup of symbols (methods and variables)
//! in a module. It's built from ItemTree and cached in the database.
//!
//! ## Architecture
//!
//! ```text
//! ItemTree → SymbolTree → Resolver → IDE Features
//!     ↓           ↓
//! (Iteration 5) (NEW)
//! ```
//!
//! ## Design
//!
//! - **Arena storage**: Methods and variables stored in Arena for stable indices
//! - **HashMap index**: Case-insensitive lookup using lowercase keys
//! - **BSL-specific**: Handles both Cyrillic and Latin identifiers
//!
//! ## Reference
//!
//! Inspired by bsl-language-server's SymbolTree.java, but adapted to Rust patterns.

use crate::item_tree::{Annotation, ItemTree, ModItem, Param};
use crate::{MethodId, ModuleId, Name, VariableId};
use la_arena::{Arena, Idx};
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use text_size::TextRange;

/// Symbol tree for a module.
///
/// Provides fast O(1) lookup of methods and variables by name (case-insensitive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolTree {
    /// Arena of method symbols (procedures + functions).
    methods: Arena<MethodSymbol>,

    /// Arena of variable symbols.
    variables: Arena<VariableSymbol>,

    /// Fast lookup: lowercase name → method indices.
    ///
    /// Multiple methods can have the same name (shouldn't happen in valid BSL,
    /// but we handle it for error recovery).
    methods_by_name: FxHashMap<SmolStr, Vec<Idx<MethodSymbol>>>,

    /// Fast lookup: lowercase name → variable indices.
    variables_by_name: FxHashMap<SmolStr, Vec<Idx<VariableSymbol>>>,
}

/// A method symbol (procedure or function).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSymbol {
    /// Method ID for cross-references.
    pub id: MethodId,

    /// Method name (original case preserved).
    pub name: Name,

    /// Is this a function (vs procedure)?
    pub is_function: bool,

    /// Is this exported?
    pub is_export: bool,

    /// Parameters.
    pub params: Vec<ParamSymbol>,

    /// Annotations (&НаКлиенте, etc.).
    pub annotations: Vec<Annotation>,

    /// Return type (for functions).
    ///
    /// For Iteration 8, this is always Unknown (full type inference in Iteration 12+).
    /// Procedures have return_type = Ty::Undefined.
    pub return_type: crate::Ty,

    /// Source location for navigation.
    pub source_range: TextRange,
}

/// A module-level variable symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableSymbol {
    /// Variable ID for cross-references.
    pub id: VariableId,

    /// Variable name (original case preserved).
    pub name: Name,

    /// Is exported?
    pub is_export: bool,

    /// Source location for navigation.
    pub source_range: TextRange,
}

/// Parameter symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSymbol {
    /// Parameter name.
    pub name: Name,

    /// Is this a value parameter (`Знач` modifier)?
    pub is_val: bool,

    /// Has default value?
    pub has_default: bool,

    /// Parameter type.
    ///
    /// For Iteration 8, this is always Unknown (full type inference in Iteration 12+).
    pub ty: crate::Ty,
}

impl SymbolTree {
    /// Build SymbolTree from ItemTree.
    ///
    /// This is the main entry point for constructing a SymbolTree.
    pub fn from_item_tree(item_tree: &ItemTree, module_id: ModuleId) -> Self {
        let mut builder = SymbolTreeBuilder::new(module_id);

        for (idx, item) in item_tree.top_level_items().iter().enumerate() {
            let local_id = idx as u32;

            match item {
                ModItem::Procedure(proc_idx) => {
                    let proc = item_tree.procedure(*proc_idx);
                    builder.add_procedure(local_id, proc);
                }
                ModItem::Function(func_idx) => {
                    let func = item_tree.function(*func_idx);
                    builder.add_function(local_id, func);
                }
                ModItem::Variable(var_idx) => {
                    let var = item_tree.variable(*var_idx);
                    builder.add_variable(local_id, var);
                }
            }
        }

        builder.build()
    }

    /// Find method by name (case-insensitive).
    ///
    /// Returns the first method with the given name, or None if not found.
    pub fn find_method(&self, name: &Name) -> Option<&MethodSymbol> {
        let key = name.as_str().to_lowercase();
        let indices = self.methods_by_name.get(key.as_str())?;
        indices.first().map(|&idx| &self.methods[idx])
    }

    /// Find all methods by name (case-insensitive).
    ///
    /// Returns multiple if there are shadowed names (error recovery).
    pub fn find_methods(&self, name: &Name) -> Vec<&MethodSymbol> {
        let key = name.as_str().to_lowercase();
        self.methods_by_name
            .get(key.as_str())
            .map(|indices| indices.iter().map(|&idx| &self.methods[idx]).collect())
            .unwrap_or_default()
    }

    /// Find variable by name (case-insensitive).
    pub fn find_variable(&self, name: &Name) -> Option<&VariableSymbol> {
        let key = name.as_str().to_lowercase();
        let indices = self.variables_by_name.get(key.as_str())?;
        indices.first().map(|&idx| &self.variables[idx])
    }

    /// Get all methods.
    pub fn methods(&self) -> impl Iterator<Item = &MethodSymbol> {
        self.methods.iter().map(|(_, m)| m)
    }

    /// Get all exported methods.
    pub fn exported_methods(&self) -> impl Iterator<Item = &MethodSymbol> {
        self.methods().filter(|m| m.is_export)
    }

    /// Get all variables.
    pub fn variables(&self) -> impl Iterator<Item = &VariableSymbol> {
        self.variables.iter().map(|(_, v)| v)
    }

    /// Get all exported variables.
    pub fn exported_variables(&self) -> impl Iterator<Item = &VariableSymbol> {
        self.variables().filter(|v| v.is_export)
    }
}

/// Builder for constructing SymbolTree.
struct SymbolTreeBuilder {
    module_id: ModuleId,
    methods: Arena<MethodSymbol>,
    variables: Arena<VariableSymbol>,
    methods_by_name: FxHashMap<SmolStr, Vec<Idx<MethodSymbol>>>,
    variables_by_name: FxHashMap<SmolStr, Vec<Idx<VariableSymbol>>>,
}

impl SymbolTreeBuilder {
    fn new(module_id: ModuleId) -> Self {
        Self {
            module_id,
            methods: Arena::new(),
            variables: Arena::new(),
            methods_by_name: FxHashMap::default(),
            variables_by_name: FxHashMap::default(),
        }
    }

    fn add_procedure(&mut self, local_id: u32, proc: &crate::item_tree::Procedure) {
        let method_id = MethodId { module: self.module_id, local_id };

        let symbol = MethodSymbol {
            id: method_id,
            name: proc.name.clone(),
            is_function: false,
            is_export: proc.is_export,
            params: proc.params.iter().map(ParamSymbol::from).collect(),
            annotations: proc.annotations.to_vec(),
            return_type: crate::Ty::Undefined, // Procedures don't return values
            source_range: proc.source_range,
        };

        self.add_method_symbol(symbol);
    }

    fn add_function(&mut self, local_id: u32, func: &crate::item_tree::Function) {
        let method_id = MethodId { module: self.module_id, local_id };

        let symbol = MethodSymbol {
            id: method_id,
            name: func.name.clone(),
            is_function: true,
            is_export: func.is_export,
            params: func.params.iter().map(ParamSymbol::from).collect(),
            annotations: func.annotations.to_vec(),
            return_type: crate::Ty::Unknown, // TODO: Full type inference in Iteration 12+
            source_range: func.source_range,
        };

        self.add_method_symbol(symbol);
    }

    fn add_method_symbol(&mut self, symbol: MethodSymbol) {
        let key: SmolStr = symbol.name.as_str().to_lowercase().into();
        let idx = self.methods.alloc(symbol);

        self.methods_by_name.entry(key).or_default().push(idx);
    }

    fn add_variable(&mut self, local_id: u32, var: &crate::item_tree::Variable) {
        let variable_id = VariableId { module: self.module_id, local_id };

        let symbol = VariableSymbol {
            id: variable_id,
            name: var.name.clone(),
            is_export: var.is_export,
            source_range: var.source_range,
        };

        let key: SmolStr = symbol.name.as_str().to_lowercase().into();
        let idx = self.variables.alloc(symbol);

        self.variables_by_name.entry(key).or_default().push(idx);
    }

    fn build(self) -> SymbolTree {
        SymbolTree {
            methods: self.methods,
            variables: self.variables,
            methods_by_name: self.methods_by_name,
            variables_by_name: self.variables_by_name,
        }
    }
}

impl From<&Param> for ParamSymbol {
    fn from(param: &Param) -> Self {
        ParamSymbol {
            name: param.name.clone(),
            is_val: param.is_val,
            has_default: param.has_default,
            ty: crate::Ty::Unknown, // TODO: Full type inference in Iteration 12+
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item_tree::{Function, Procedure, Variable};
    use crate::ModuleId;
    use text_size::TextSize;
    use vfs::FileId;

    fn make_text_range(start: u32, end: u32) -> TextRange {
        TextRange::new(TextSize::from(start), TextSize::from(end))
    }

    #[test]
    fn test_symbol_tree_basic() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        // Add procedure
        let proc_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("Первая"),
            is_export: false,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
        });
        item_tree.top_level.push(ModItem::Procedure(proc_idx));

        // Add exported function
        let func_idx = item_tree.functions.alloc(Function {
            name: Name::new("Вторая"),
            is_export: true,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(20, 30),
        });
        item_tree.top_level.push(ModItem::Function(func_idx));

        // Add module variable
        let var_idx = item_tree.variables.alloc(Variable {
            name: Name::new("МодульнаяПеременная"),
            is_export: true,
            source_range: make_text_range(40, 50),
        });
        item_tree.top_level.push(ModItem::Variable(var_idx));

        // Build SymbolTree
        let symbol_tree = SymbolTree::from_item_tree(&item_tree, module_id);

        // Verify methods
        assert_eq!(symbol_tree.methods().count(), 2);

        let first = symbol_tree.find_method(&Name::new("Первая")).unwrap();
        assert_eq!(first.name.as_str(), "Первая");
        assert!(!first.is_function);
        assert!(!first.is_export);

        let second = symbol_tree.find_method(&Name::new("Вторая")).unwrap();
        assert_eq!(second.name.as_str(), "Вторая");
        assert!(second.is_function);
        assert!(second.is_export);

        // Verify variables
        assert_eq!(symbol_tree.variables().count(), 1);

        let var = symbol_tree.find_variable(&Name::new("МодульнаяПеременная")).unwrap();
        assert_eq!(var.name.as_str(), "МодульнаяПеременная");
        assert!(var.is_export);
    }

    #[test]
    fn test_case_insensitive_lookup() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let proc_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("МояПроцедура"),
            is_export: false,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
        });
        item_tree.top_level.push(ModItem::Procedure(proc_idx));

        let symbol_tree = SymbolTree::from_item_tree(&item_tree, module_id);

        // Different cases should all find the same method
        assert!(symbol_tree.find_method(&Name::new("МояПроцедура")).is_some());
        assert!(symbol_tree.find_method(&Name::new("мояпроцедура")).is_some());
        assert!(symbol_tree.find_method(&Name::new("МОЯПРОЦЕДУРА")).is_some());
        assert!(symbol_tree.find_method(&Name::new("МоЯпРоЦеДуРа")).is_some());

        // All should return the same symbol
        let m1 = symbol_tree.find_method(&Name::new("МояПроцедура")).unwrap();
        let m2 = symbol_tree.find_method(&Name::new("мояпроцедура")).unwrap();
        assert_eq!(m1.id, m2.id);
    }

    #[test]
    fn test_exported_methods_filter() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        // Add private procedure
        let proc1_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("Приватная"),
            is_export: false,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
        });
        item_tree.top_level.push(ModItem::Procedure(proc1_idx));

        // Add exported procedure
        let proc2_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("Публичная"),
            is_export: true,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(20, 30),
        });
        item_tree.top_level.push(ModItem::Procedure(proc2_idx));

        let symbol_tree = SymbolTree::from_item_tree(&item_tree, module_id);

        // All methods
        assert_eq!(symbol_tree.methods().count(), 2);

        // Only exported
        let exported: Vec<_> = symbol_tree.exported_methods().collect();
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].name.as_str(), "Публичная");
    }

    #[test]
    fn test_method_with_parameters() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let proc_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("СПараметрами"),
            is_export: false,
            params: Box::new([
                Param { name: Name::new("Параметр1"), is_val: true, has_default: false },
                Param { name: Name::new("Параметр2"), is_val: false, has_default: true },
            ]),
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
        });
        item_tree.top_level.push(ModItem::Procedure(proc_idx));

        let symbol_tree = SymbolTree::from_item_tree(&item_tree, module_id);

        let method = symbol_tree.find_method(&Name::new("СПараметрами")).unwrap();
        assert_eq!(method.params.len(), 2);
        assert_eq!(method.params[0].name.as_str(), "Параметр1");
        assert!(method.params[0].is_val);
        assert!(!method.params[0].has_default);
        assert_eq!(method.params[1].name.as_str(), "Параметр2");
        assert!(!method.params[1].is_val);
        assert!(method.params[1].has_default);
    }

    #[test]
    fn test_method_with_annotations() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let proc_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("НаКлиенте"),
            is_export: true,
            params: Box::new([]),
            annotations: Box::new([Annotation {
                kind: crate::item_tree::AnnotationKind::AtClient,
            }]),
            source_range: make_text_range(0, 10),
        });
        item_tree.top_level.push(ModItem::Procedure(proc_idx));

        let symbol_tree = SymbolTree::from_item_tree(&item_tree, module_id);

        let method = symbol_tree.find_method(&Name::new("НаКлиенте")).unwrap();
        assert_eq!(method.annotations.len(), 1);
        assert_eq!(method.annotations[0].kind, crate::item_tree::AnnotationKind::AtClient);
    }

    #[test]
    fn test_not_found() {
        let item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let symbol_tree = SymbolTree::from_item_tree(&item_tree, module_id);

        assert!(symbol_tree.find_method(&Name::new("НесуществующаяПроцедура")).is_none());
        assert!(symbol_tree.find_variable(&Name::new("НесуществующаяПеременная")).is_none());
    }

    #[test]
    fn test_empty_symbol_tree() {
        let item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let symbol_tree = SymbolTree::from_item_tree(&item_tree, module_id);

        assert_eq!(symbol_tree.methods().count(), 0);
        assert_eq!(symbol_tree.variables().count(), 0);
        assert_eq!(symbol_tree.exported_methods().count(), 0);
        assert_eq!(symbol_tree.exported_variables().count(), 0);
    }

    #[test]
    fn test_variable_case_insensitive() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let var_idx = item_tree.variables.alloc(Variable {
            name: Name::new("МояПеременная"),
            is_export: false,
            source_range: make_text_range(0, 10),
        });
        item_tree.top_level.push(ModItem::Variable(var_idx));

        let symbol_tree = SymbolTree::from_item_tree(&item_tree, module_id);

        // Different cases
        assert!(symbol_tree.find_variable(&Name::new("МояПеременная")).is_some());
        assert!(symbol_tree.find_variable(&Name::new("мояпеременная")).is_some());
        assert!(symbol_tree.find_variable(&Name::new("МОЯПЕРЕМЕННАЯ")).is_some());
    }

    #[test]
    fn test_exported_variables_filter() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        // Private variable
        let var1_idx = item_tree.variables.alloc(Variable {
            name: Name::new("Приватная"),
            is_export: false,
            source_range: make_text_range(0, 10),
        });
        item_tree.top_level.push(ModItem::Variable(var1_idx));

        // Exported variable
        let var2_idx = item_tree.variables.alloc(Variable {
            name: Name::new("Публичная"),
            is_export: true,
            source_range: make_text_range(20, 30),
        });
        item_tree.top_level.push(ModItem::Variable(var2_idx));

        let symbol_tree = SymbolTree::from_item_tree(&item_tree, module_id);

        // All variables
        assert_eq!(symbol_tree.variables().count(), 2);

        // Only exported
        let exported: Vec<_> = symbol_tree.exported_variables().collect();
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].name.as_str(), "Публичная");
    }

    #[test]
    fn test_find_methods_multiple() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let proc_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("Метод"),
            is_export: false,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
        });
        item_tree.top_level.push(ModItem::Procedure(proc_idx));

        let symbol_tree = SymbolTree::from_item_tree(&item_tree, module_id);

        // find_methods returns Vec
        let methods = symbol_tree.find_methods(&Name::new("Метод"));
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "Метод");

        // Not found returns empty Vec
        let not_found = symbol_tree.find_methods(&Name::new("НеСуществует"));
        assert_eq!(not_found.len(), 0);
    }
}
