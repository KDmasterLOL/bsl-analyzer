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
//!                    ├── RegionTree (preprocessor regions hierarchy)
//!                    ├── Body (method bodies, expressions/statements)
//!                    └── SourceMap (HIR ↔ AST mapping for diagnostics)
//! ```
//!
//! ## Key components
//!
//! - **ItemTree**: Module-level definitions (procedures, functions, variables)
//! - **RegionTree**: Hierarchical structure of preprocessor regions
//! - **Body**: HIR representation of method bodies
//! - **hir**: Expression and statement types (Expr, Stmt, Literal)
//! - **BodySourceMap**: Bidirectional mapping between HIR and AST

pub mod body;
pub mod cognitive_complexity;
pub mod cyclomatic_complexity;
pub mod hir;
pub mod item_tree;
pub mod name;
pub mod region_tree;
pub mod resolver;
pub mod scope;
pub mod symbol_tree;
pub mod ty;

use std::sync::Arc;

use vfs::FileId;

pub use body::{lower_method, lower_module_code, Body, BodyDiagnostic, BodySourceMap, LowerResult};
pub use hir::{BinaryOp, Binding, BindingId, Expr, ExprId, Literal, Stmt, StmtId, UnaryOp};

// ModuleBodies, ModuleMetadata, ExecutionContext are defined in this file, not in modules
pub use item_tree::ItemTree;
pub use name::Name;
pub use region_tree::{RegionData, RegionIdx, RegionTree};
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

    /// Get RegionTree for a file.
    ///
    /// RegionTree provides hierarchical structure of preprocessor regions (#Область/#Region).
    /// Used for diagnostics (code_out_of_region, non_standard_region, etc.) and IDE features.
    ///
    /// ## Performance
    /// - Cached per file
    /// - Invalidated when file content changes
    /// - O(n) construction where n is number of region directives
    fn region_tree(&self, file_id: FileId) -> Arc<RegionTree>;

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

    /// Get metadata for a module (type and execution context).
    ///
    /// Loads metadata from 1C Configuration if available. Used by metadata-based diagnostics
    /// to provide context-sensitive checks (naming rules, API requirements, etc.).
    ///
    /// ## Performance
    /// - Configuration loading: ~1 second (Salsa cached, LRU=16)
    /// - Cached per module alongside ModuleBodies
    /// - Invalidated when file content changes
    ///
    /// ## Returns
    /// - `Arc<ModuleMetadata>` containing module type, execution context, and metadata objects
    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata>;
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

/// Execution context for a CommonModule (derived from metadata).
///
/// Represents the execution context of a Common Module based on its metadata properties.
/// These contexts are mutually exclusive (a module has exactly one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionContext {
    /// Server execution only (server: true, clientManagedApplication: false, externalConnection: false)
    Server,
    /// Server call capability (serverCall: true)
    ServerCall,
    /// Client managed application (clientManagedApplication: true, server: false, externalConnection: false)
    Client,
    /// Client and Server (clientManagedApplication: true, server: true, externalConnection: false)
    ClientServer,
    /// External connection (externalConnection: true, server: false)
    ExternalConnection,
    /// Unknown/other configuration
    Unknown,
}

/// Metadata for a module including its type and execution context.
///
/// This structure is populated during HIR lowering and used by metadata-based diagnostics
/// to provide context-sensitive checks (e.g., naming rules, API requirements).
///
/// # Performance
///
/// - Cached per module alongside ModuleBodies
/// - Invalidated when file content changes
/// - Metadata load is shared with all diagnostics (single point of loading)
#[derive(Debug, Clone)]
pub struct ModuleMetadata {
    /// Type of the module (determined from file path).
    pub module_type: bsl_metadata::ModuleType,

    /// Execution context (only for CommonModules).
    /// If not a CommonModule, this will be None.
    pub execution_context: Option<ExecutionContext>,

    /// CommonModule metadata if this module is a CommonModule.
    ///
    /// Arc-wrapped for efficient sharing between diagnostics.
    pub common_module: Option<Arc<bsl_metadata::CommonModule>>,

    /// Generic metadata object if available.
    ///
    /// Used for non-CommonModule types (ObjectModule, FormModule, etc.)
    /// Arc-wrapped for efficient sharing.
    pub mdo: Option<Arc<bsl_metadata::MetadataObject>>,
}

impl ModuleMetadata {
    /// Create metadata for a module with no metadata available.
    ///
    /// Used when metadata loading fails or module is outside Designer format.
    pub fn unknown(module_type: bsl_metadata::ModuleType) -> Self {
        Self { module_type, execution_context: None, common_module: None, mdo: None }
    }
}

/// All method bodies for a module with their diagnostics.
///
/// This structure contains the lowered HIR bodies for all procedures and functions
/// in a module, along with diagnostics collected during lowering.
///
/// Metadata is populated by the `module_bodies()` query in ide-db.
///
/// Note: `ModuleBodies` is not `Clone` because it contains large data structures.
/// Use `Arc<ModuleBodies>` for sharing between diagnostics.
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
    /// Module metadata (type, execution context, loaded from Configuration).
    /// Populated by module_bodies() query in ide-db, not by lower_module_bodies.
    metadata: Option<Arc<ModuleMetadata>>,
}

impl ModuleBodies {
    /// Create empty ModuleBodies.
    pub fn new() -> Self {
        Self {
            bodies: rustc_hash::FxHashMap::default(),
            all_diagnostics: Vec::new(),
            module_vars: Vec::new(),
            module_code: None,
            metadata: None,
        }
    }

    /// Set metadata for this module (used by module_bodies query).
    pub fn with_metadata(mut self, metadata: Arc<ModuleMetadata>) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Get metadata for this module if available.
    pub fn metadata(&self) -> Option<&ModuleMetadata> {
        self.metadata.as_ref().map(|m| m.as_ref())
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

    /// Iterate over all bodies with their local_ids.
    pub fn iter_bodies(&self) -> impl Iterator<Item = (u32, &Body)> {
        self.bodies.iter().map(|(local_id, lower_result)| (*local_id, &lower_result.body))
    }

    /// Iterate over all method bodies with their Body and SourceMap.
    /// This is useful for post-HIR analysis diagnostics that need source locations.
    pub fn method_bodies(&self) -> impl Iterator<Item = (u32, &Body, &BodySourceMap)> {
        self.bodies.iter().map(|(local_id, lower_result)| {
            (*local_id, &lower_result.body, &lower_result.source_map)
        })
    }

    /// Get module-level code body (statements outside procedures/functions).
    pub fn module_code(&self) -> Option<&Body> {
        self.module_code.as_ref().map(|r| &r.body)
    }

    /// Get module-level code with source map (for diagnostics).
    pub fn module_code_result(&self) -> Option<&body::LowerResult> {
        self.module_code.as_ref()
    }
}

impl Default for ModuleBodies {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute execution context for a CommonModule from its metadata properties.
///
/// Returns the execution context based on the module's metadata attributes.
/// Context determination follows the logic from bsl-language-server.
pub fn compute_execution_context(common_module: &bsl_metadata::CommonModule) -> ExecutionContext {
    // ServerCall takes precedence: serverCall=true, server=false, externalConnection=false
    if common_module.is_server_call() {
        return ExecutionContext::ServerCall;
    }

    // Check for Server/Client combinations
    let is_server = common_module.is_server();
    let is_client_managed = common_module.is_client_managed_application();
    let is_external = common_module.is_external_connection();

    // Server + Client = ClientServer
    if is_server && is_client_managed && !is_external {
        return ExecutionContext::ClientServer;
    }

    // Server only
    if is_server && !is_client_managed && !is_external {
        return ExecutionContext::Server;
    }

    // Client only (client_managed_application = true)
    if is_client_managed && !is_server && !is_external {
        return ExecutionContext::Client;
    }

    // External connection
    if is_external && !is_server {
        return ExecutionContext::ExternalConnection;
    }

    // Unknown configuration
    ExecutionContext::Unknown
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
    // Use descendants() to find variables inside preprocessor regions too
    // But skip VAR_DEFs that are inside methods (local variable declarations)
    for node in root.descendants() {
        if node.kind() == SyntaxKind::VAR_DEF {
            // Check if this VAR_DEF is inside a method by looking for ancestor PROCEDURE_DEF/FUNCTION_DEF
            let is_inside_method = node
                .ancestors()
                .any(|n| matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF));

            if !is_inside_method {
                collect_module_vars(&node, &mut result.module_vars);
            }
        }
    }

    // Deduplicate module variables (matching Java behavior)
    // Java: VariableSymbolComputer.visitModuleVarDeclaration:88-89 skips duplicate names
    // We keep only the first declaration with each name (case-insensitive)
    {
        let mut seen_names: FxHashSet<String> = FxHashSet::default();
        result.module_vars.retain(|var| {
            let key = var.name.to_lowercase();
            seen_names.insert(key)
        });
    }

    // Create set of module variable names (lowercase) for passing to method lowering
    let module_var_names: FxHashSet<String> =
        result.module_vars.iter().map(|v| v.name.to_lowercase()).collect();

    // Second pass: lower methods and collect referenced externals
    // Use descendants() to find methods inside preprocessor regions (#Область, #Если)
    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::PROCEDURE_DEF => {
                let lower_result =
                    body::lower_method_with_externals(&node, false, module_var_names.clone());

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
                let lower_result =
                    body::lower_method_with_externals(&node, true, module_var_names.clone());

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
