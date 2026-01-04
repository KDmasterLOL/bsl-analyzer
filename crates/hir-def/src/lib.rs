//! HIR definitions for bsl-analyzer.
//!
//! This crate contains definitions and data structures for the
//! High-level Intermediate Representation.
//!
//! ## Architecture
//!
//! ```text
//! AST (syntax) → HIR (hir-def) → Diagnostics + Type inference
//!                    │
//!                    ├── ItemTree (signatures only, invalidation barrier)
//!                    ├── Body (method bodies, expressions/statements)
//!                    └── SourceMap (HIR ↔ AST mapping for diagnostics)
//! ```
//!
//! ## Key components
//!
//! - **ItemTree**: Module-level definitions (procedures, functions, variables)
//! - **Body**: HIR representation of method bodies
//! - **hir**: Expression and statement types (Expr, Stmt, Literal)
//! - **BodySourceMap**: Bidirectional mapping between HIR and AST

pub mod body;
pub mod hir;
pub mod item_tree;
pub mod name;
pub mod resolver;
pub mod scope;
pub mod symbol_tree;
pub mod ty;

use std::sync::Arc;

use vfs::FileId;

pub use body::{lower_method, lower_module_code, Body, BodyDiagnostic, BodySourceMap, LowerResult};
pub use hir::{BinaryOp, Binding, BindingId, Expr, ExprId, Literal, Stmt, StmtId, UnaryOp};

// ModuleBodies is defined in this file, not in body module
pub use item_tree::ItemTree;
pub use name::Name;
pub use symbol_tree::{MethodSymbol, ParamSymbol, SymbolTree, VariableSymbol};
pub use ty::infer::{FunctionSignature, InferenceContext, InferenceResult};
pub use ty::Ty;

/// Database trait for HIR queries.
///
/// This trait extends base_db::RootQueryDb with queries for ItemTree and module-level data.
pub trait DefDatabase: base_db::RootQueryDb {
    /// Get ItemTree for a file (main query).
    ///
    /// ItemTree is the "invalidation barrier" - it only changes when signatures change,
    /// not when procedure bodies are edited.
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree>;

    /// Get module data for a module (derived query).
    ///
    /// In BSL, 1 file = 1 module, so ModuleId contains FileId.
    fn module_data(&self, module_id: ModuleId) -> Arc<ModuleData>;

    /// Get symbol tree for a module (derived from ItemTree).
    ///
    /// SymbolTree provides fast O(1) case-insensitive lookup of methods and variables.
    /// Built from ItemTree and cached.
    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree>;

    /// Infer types for a module.
    ///
    /// Performs type inference for all expressions, variables, and methods in a module.
    /// Results are cached and only re-computed when the module's ItemTree changes.
    ///
    /// ## Performance
    /// - Initial inference: ~10-20ms for a typical 1000-line module
    /// - Cached access: < 1ms (via Salsa caching when fully integrated)
    /// - Invalidation: Only when ItemTree changes (signature changes, not body edits)
    ///
    /// ## Phase 1 Support
    /// - Literals: `42`, `"text"`, `True`
    /// - Binary operations: `5 + 3`, `"a" + "b"`, `x > 5`
    ///
    /// Future phases will add support for function calls, method calls, and variables.
    fn infer_types(&self, module_id: ModuleId) -> Arc<InferenceResult>;

    /// Get all method bodies for a module with their diagnostics.
    ///
    /// Returns lowered HIR bodies for all procedures and functions in the module.
    /// Diagnostics are collected during lowering as a byproduct of semantic analysis.
    ///
    /// ## Performance
    /// - Cached per module
    /// - Invalidated when file content changes
    /// - O(n) where n is number of statements in methods
    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies>;
}

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

/// Module-level variable declaration for tracking usage.
#[derive(Debug, Clone)]
pub struct ModuleVarDecl {
    /// Variable name (original case).
    pub name: String,
    /// Source range of the declaration.
    pub range: text_size::TextRange,
    /// Whether the variable is exported.
    pub is_export: bool,
}

/// All method bodies for a module with their diagnostics.
///
/// This structure contains the lowered HIR bodies for all procedures and functions
/// in a module, along with diagnostics collected during lowering.
#[derive(Debug)]
pub struct ModuleBodies {
    /// Bodies indexed by MethodId.local_id
    bodies: rustc_hash::FxHashMap<u32, body::LowerResult>,
    /// All diagnostics from all methods
    all_diagnostics: Vec<(MethodId, BodyDiagnostic)>,
    /// Module-level variable declarations
    module_vars: Vec<ModuleVarDecl>,
    /// Module-level code body (statements outside procedures)
    module_code: Option<body::LowerResult>,
}

impl ModuleBodies {
    /// Create empty ModuleBodies.
    pub fn new() -> Self {
        Self {
            bodies: rustc_hash::FxHashMap::default(),
            all_diagnostics: Vec::new(),
            module_vars: Vec::new(),
            module_code: None,
        }
    }

    /// Get body for a method by its local_id.
    pub fn body(&self, local_id: u32) -> Option<&Body> {
        self.bodies.get(&local_id).map(|r| &r.body)
    }

    /// Get source map for a method by its local_id.
    pub fn source_map(&self, local_id: u32) -> Option<&BodySourceMap> {
        self.bodies.get(&local_id).map(|r| &r.source_map)
    }

    /// Get diagnostics for a method by its local_id.
    pub fn diagnostics(&self, local_id: u32) -> Option<&[BodyDiagnostic]> {
        self.bodies.get(&local_id).map(|r| r.diagnostics.as_slice())
    }

    /// Get all diagnostics from all methods.
    pub fn all_diagnostics(&self) -> &[(MethodId, BodyDiagnostic)] {
        &self.all_diagnostics
    }

    /// Get LowerResult for a method.
    pub fn lower_result(&self, local_id: u32) -> Option<&body::LowerResult> {
        self.bodies.get(&local_id)
    }

    /// Number of methods with bodies.
    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }
}

impl Default for ModuleBodies {
    fn default() -> Self {
        Self::new()
    }
}

/// Lower all method bodies in a module.
///
/// This function walks the AST and lowers each procedure/function body to HIR.
/// Also tracks module-level variables and emits diagnostics for unused ones.
pub fn lower_module_bodies(db: &dyn base_db::RootQueryDb, module_id: ModuleId) -> ModuleBodies {
    use rustc_hash::FxHashSet;
    use syntax::SyntaxKind;

    let parse = db.parse(module_id.file_id);
    let root = parse.syntax_node();

    let mut result = ModuleBodies::new();
    let mut method_idx = 0u32;
    let mut all_referenced_externals: FxHashSet<String> = FxHashSet::default();

    // First pass: collect module-level variable declarations
    for node in root.children() {
        if node.kind() == SyntaxKind::VAR_DEF {
            collect_module_vars(&node, &mut result.module_vars);
        }
    }

    // Second pass: lower methods and collect referenced externals
    for node in root.children() {
        match node.kind() {
            SyntaxKind::PROCEDURE_DEF => {
                let lower_result = body::lower_method(&node, false);

                // Collect diagnostics with MethodId
                let method_id = MethodId { module: module_id, local_id: method_idx };
                for diag in &lower_result.diagnostics {
                    result.all_diagnostics.push((method_id, diag.clone()));
                }

                // Collect referenced externals
                all_referenced_externals.extend(lower_result.referenced_externals.iter().cloned());

                result.bodies.insert(method_idx, lower_result);
                method_idx += 1;
            }
            SyntaxKind::FUNCTION_DEF => {
                let lower_result = body::lower_method(&node, true);

                // Collect diagnostics with MethodId
                let method_id = MethodId { module: module_id, local_id: method_idx };
                for diag in &lower_result.diagnostics {
                    result.all_diagnostics.push((method_id, diag.clone()));
                }

                // Collect referenced externals
                all_referenced_externals.extend(lower_result.referenced_externals.iter().cloned());

                result.bodies.insert(method_idx, lower_result);
                method_idx += 1;
            }
            SyntaxKind::VAR_DEF => {
                method_idx += 1;
            }
            _ => {}
        }
    }

    // Third pass: lower module-level code (statements outside procedures)
    let module_code_result = body::lower_module_code(&root);

    // Collect diagnostics from module-level code
    // Use a special MethodId with local_id = u32::MAX to indicate module-level code
    let module_method_id = MethodId { module: module_id, local_id: u32::MAX };
    for diag in &module_code_result.diagnostics {
        result.all_diagnostics.push((module_method_id, diag.clone()));
    }

    // Collect referenced externals from module code
    all_referenced_externals.extend(module_code_result.referenced_externals.iter().cloned());

    result.module_code = Some(module_code_result);

    // Fourth pass: check for unused module variables
    for var in &result.module_vars {
        // Skip exported variables (externally visible)
        if var.is_export {
            continue;
        }

        let key = var.name.to_lowercase();
        if !all_referenced_externals.contains(&key) {
            // Variable is not used anywhere
            result.all_diagnostics.push((
                module_method_id,
                BodyDiagnostic::UnusedVariable { name: var.name.clone(), range: var.range },
            ));
        }
    }

    result
}

/// Collect module-level variable declarations from a VAR_DEF node.
fn collect_module_vars(var_def: &syntax::SyntaxNode, vars: &mut Vec<ModuleVarDecl>) {
    use syntax::SyntaxKind;

    // Check if any variable in this VAR_DEF has Export
    let has_export = var_def
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::KW_EXPORT);

    // Find all IDENT tokens (variable names)
    for token in var_def.children_with_tokens().filter_map(|el| el.into_token()) {
        if token.kind() == SyntaxKind::IDENT {
            vars.push(ModuleVarDecl {
                name: token.text().to_string(),
                range: token.text_range(),
                is_export: has_export,
            });
        }
    }
}
