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
//!                    ├── ConditionalTree (preprocessor conditionals hierarchy)
//!                    ├── Body (method bodies, expressions/statements)
//!                    └── SourceMap (HIR ↔ AST mapping for diagnostics)
//! ```
//!
//! ## Key components
//!
//! - **ItemTree**: Module-level definitions (procedures, functions, variables)
//! - **RegionTree**: Hierarchical structure of preprocessor regions
//! - **ConditionalTree**: Hierarchical structure of preprocessor conditionals (#Если/#If)
//! - **Body**: HIR representation of method bodies
//! - **hir**: Expression and statement types (Expr, Stmt, Literal)
//! - **BodySourceMap**: Bidirectional mapping between HIR and AST

pub mod body;
// pub mod cfg_builder; // TODO: Reimplement with updated cfg crate API
pub mod cognitive_complexity;
pub mod conditional_tree;
pub mod cyclomatic_complexity;
pub mod docs;
pub mod hir;
pub mod item_tree;
pub mod name;
pub mod path;
pub mod queries;
pub mod region_tree;
pub mod resolver;
pub mod scope;
pub mod symbol_tree;
pub mod ty;
pub mod workspace;

use std::sync::Arc;

use vfs::FileId;

pub use body::{
    lower_method, lower_module_code, Body, BodyDiagnostic, BodySourceMap, CommonModuleMethodError,
    LowerResult,
};
pub use hir::{BinaryOp, Binding, BindingId, Expr, ExprId, Literal, Stmt, StmtId, UnaryOp};

// ModuleBodies, ModuleMetadata, ExecutionContext are defined in this file, not in modules
pub use conditional_tree::{ConditionalData, ConditionalIdx, ConditionalKind, ConditionalTree};
pub use item_tree::ItemTree;
pub use name::Name;
pub use path::{PathResolution, QualifiedName};
pub use region_tree::{RegionData, RegionIdx, RegionTree};
pub use symbol_tree::{MethodSymbol, ParamSymbol, SymbolTree, VariableSymbol};
pub use ty::infer::{InferenceContext, InferenceResult};
pub use ty::{FunctionSignature, Ty};
pub use workspace::{CommonModuleInfo, WorkspaceSymbols};

// Re-export all Salsa query functions from the queries module
pub use queries::{
    conditional_tree_query, infer_types_query, item_tree_query, module_bodies_query,
    module_data_query, module_metadata_query, region_tree_query, symbol_tree_query,
    workspace_symbols_query,
};

/// HIR definition layer - lowering from AST to HIR.
///
/// # Query Group Organization
///
/// This trait defines the HIR-level queries that transform BSL syntax trees (AST)
/// into semantic representations (HIR) and extract metadata for analysis.
///
/// **Dependencies:** base_db::RootQueryDb (parsing)
/// **Used by:** ide_db::RootDatabase (IDE queries, dataflow, SDBL)
///
/// # Query Categories
///
/// ## Invalidation Barrier Queries (AST → HIR metadata)
///
/// These queries extract signatures and structure WITHOUT analyzing method bodies.
/// They form an "invalidation barrier" - changes to method bodies don't invalidate consumers.
///
/// - [`item_tree`](Self::item_tree) - Method/variable signatures (LRU: 512)
/// - [`region_tree`](Self::region_tree) - Preprocessor region hierarchy (LRU: 256)
/// - [`conditional_tree`](Self::conditional_tree) - Preprocessor conditional hierarchy (LRU: 256)
///
/// ## Derived Queries (depend on ItemTree)
///
/// - [`symbol_tree`](Self::symbol_tree) - Case-insensitive symbol lookup (LRU: 512)
/// - [`module_data`](Self::module_data) - Module-level data (LRU: 512)
///
/// ## Type Inference
///
/// - [`infer_types`](Self::infer_types) - Type inference for module (LRU: 256)
///
/// ## HIR Lowering (AST → HIR bodies, produces diagnostics)
///
/// - [`module_bodies`](Self::module_bodies) - Lower method bodies + diagnostics (LRU: 128)
///
/// ## Metadata
///
/// - [`module_metadata`](Self::module_metadata) - Module type and execution context (LRU: 128)
///
/// # Implementation Pattern
///
/// Implementations delegate to tracked query functions in the `queries` module:
///
/// ```ignore
/// impl DefDatabase for MyDatabase {
///     fn item_tree(&self, file_id: FileId) -> Arc<ItemTree> {
///         let file_id_input = base_db::FileIdInput::new(self, file_id);
///         item_tree_query(self, file_id_input)
///     }
/// }
/// ```
#[salsa::db]
pub trait DefDatabase: base_db::RootQueryDb {
    /// Get ItemTree for a file.
    ///
    /// ItemTree is the "invalidation barrier" - it contains only method/variable signatures,
    /// NOT method bodies. Changes to procedure/function bodies don't invalidate ItemTree,
    /// so consumers (like symbol_tree, module_data) aren't re-computed unnecessarily.
    ///
    /// # Performance
    /// - **LRU cache:** 512 files (frequently accessed)
    /// - **Depends on:** [`parse`](base_db::RootQueryDb::parse)
    /// - **Typical time:** 2-5ms for medium files (after parsing)
    ///
    /// # Implementation
    /// Should delegate to [`item_tree_query`].
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree>;

    /// Get RegionTree for a file.
    ///
    /// RegionTree provides hierarchical structure of preprocessor regions (#Область/#Region).
    /// Used for diagnostics (CodeOutOfRegion, NonStandardRegion, etc.) and IDE features.
    ///
    /// # Performance
    /// - **LRU cache:** 256 files (region extraction is inexpensive)
    /// - **Depends on:** [`parse`](base_db::RootQueryDb::parse)
    /// - **Typical time:** ~1ms (shallow tree walk)
    ///
    /// # Implementation
    /// Should delegate to [`region_tree_query`].
    fn region_tree(&self, file_id: FileId) -> Arc<RegionTree>;

    /// Get ConditionalTree for a file.
    ///
    /// ConditionalTree provides hierarchical structure of preprocessor conditionals
    /// (#Если/#If, #ИначеЕсли/#ElsIf, #Иначе/#Else). Stores condition TEXT only (not evaluated).
    ///
    /// Used for conditional context diagnostics (grammatical construct splits, platform checks, etc.).
    ///
    /// # Performance
    /// - **LRU cache:** 256 files (conditional extraction is inexpensive)
    /// - **Depends on:** [`parse`](base_db::RootQueryDb::parse)
    /// - **Typical time:** ~1ms (shallow tree walk)
    ///
    /// # Implementation
    /// Should delegate to [`conditional_tree_query`].
    fn conditional_tree(&self, file_id: FileId) -> Arc<ConditionalTree>;

    /// Get module data for a module (derived from ItemTree).
    ///
    /// ModuleData is a simplified view of ItemTree containing lists of procedures,
    /// functions, and variables with their IDs. In BSL: 1 file = 1 module.
    ///
    /// # Performance
    /// - **LRU cache:** 512 (derived query, cheap to compute)
    /// - **Depends on:** [`item_tree`](Self::item_tree)
    /// - **Typical time:** ~1ms (extracts data from ItemTree)
    ///
    /// # Implementation
    /// Should delegate to [`module_data_query`].
    fn module_data(&self, module_id: ModuleId) -> Arc<ModuleData>;

    /// Get symbol tree for a module (derived from ItemTree).
    ///
    /// SymbolTree provides fast O(1) case-insensitive lookup of methods and variables.
    /// Built from ItemTree and cached.
    ///
    /// # Performance
    /// - **LRU cache:** 512 (frequently accessed by completion/hover)
    /// - **Depends on:** [`item_tree`](Self::item_tree)
    /// - **Typical time:** ~1-2ms (builds lookup maps from ItemTree)
    ///
    /// # Implementation
    /// Should delegate to [`symbol_tree_query`].
    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree>;

    /// Infer types for a module.
    ///
    /// Performs type inference for all expressions, variables, and methods in a module.
    /// Results are cached and only re-computed when the module's ItemTree changes.
    ///
    /// # Performance
    /// - **LRU cache:** 256 files (type inference is moderately expensive)
    /// - **Depends on:** [`item_tree`](Self::item_tree)
    /// - **Typical time:** ~10-20ms for 1000-line module
    ///
    /// # Implementation
    /// Should delegate to [`infer_types_query`].
    fn infer_types(&self, module_id: ModuleId) -> Arc<InferenceResult>;

    /// Get all method bodies for a module with their diagnostics.
    ///
    /// Returns lowered HIR bodies for all procedures and functions in the module.
    /// Diagnostics are collected during lowering as a byproduct of semantic analysis
    /// (MissingReturn, UnreachableCode, etc.).
    ///
    /// # Performance
    /// - **LRU cache:** 128 (heavy lowering operation)
    /// - **Depends on:** [`parse`](base_db::RootQueryDb::parse), [`item_tree`](Self::item_tree)
    /// - **Typical time:** ~5-10ms for 1000-line module
    ///
    /// # Implementation
    /// Should delegate to [`module_bodies_query`].
    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies>;

    /// Get metadata for a module (type and execution context).
    ///
    /// Loads metadata from 1C Configuration if available. Used by metadata-based diagnostics
    /// to provide context-sensitive checks (naming rules, API requirements, etc.).
    ///
    /// # Performance
    /// - **LRU cache:** 128 (metadata loading is I/O intensive)
    /// - **Depends on:** VFS, Configuration loading (in ide-db)
    /// - **Typical time:** ~1s for first load (configuration parsing), < 1ms cached
    ///
    /// # Implementation
    /// Should delegate to [`module_metadata_query`].
    ///
    /// **Note:** Actual implementation is in ide-db (needs VFS access). The query
    /// in hir-def is a placeholder.
    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata>;

    /// Get parsed documentation for a method.
    ///
    /// Extracts and parses documentation comments (lines starting with //) before
    /// a procedure or function. Returns structured `MethodDocs` containing:
    /// - Purpose/description
    /// - Parameter types and descriptions
    /// - Return value types and descriptions
    /// - Examples, call options, deprecation info
    ///
    /// Returns `None` if the method has no documentation comments.
    ///
    /// # Performance
    /// - **LRU cache:** 256 methods (documentation parsing is inexpensive)
    /// - **Depends on:** [`parse`](base_db::RootQueryDb::parse), [`item_tree`](Self::item_tree)
    /// - **Typical time:** ~0.5-1ms per method (comment extraction + parsing)
    ///
    /// # Implementation
    /// Should delegate to [`method_docs_query`](docs::method_docs_query).
    ///
    /// # Usage
    /// ```ignore
    /// let docs = db.method_docs(method_id)?;
    /// println!("Purpose: {}", docs.purpose.unwrap_or_default());
    /// for param in &docs.parameters {
    ///     println!("  {}: {:?}", param.name, param.types);
    /// }
    /// ```
    fn method_docs(&self, method: MethodId) -> Option<Arc<crate::docs::MethodDocs>>;

    /// Get workspace-wide symbol index for CommonModules.
    ///
    /// Builds a global index of all CommonModules in the provided files, enabling
    /// O(1) lookup for qualified name resolution (e.g., `ОбщийМодуль.Метод()`).
    ///
    /// # Arguments
    /// * `files` - List of file IDs to index (typically all files in SourceRoot)
    ///
    /// # Performance
    /// - **Computation:** O(n×m) where n = files, m = avg methods per file (~100ms for 6,540 files)
    /// - **Memory:** ~1-5 KB per module (signatures only, not bodies)
    /// - **Caching:** Salsa-tracked, recomputed when any file in the list changes
    ///
    /// # Usage
    /// ```ignore
    /// let all_files: Vec<FileId> = source_root.files().collect();
    /// let symbols = db.workspace_symbols(&all_files);
    ///
    /// if let Some(module_info) = symbols.common_modules().get(&Name::new("ОбщегоНазначения")) {
    ///     // Found CommonModule, access its methods
    /// }
    /// ```
    ///
    /// # Implementation
    /// Should delegate to [`workspace_symbols_query`].
    ///
    /// # Note
    /// This is a workspace-level query. Invalidation happens when any file in the
    /// workspace changes. For large workspaces, consider optimizing with finer-grained tracking.
    fn workspace_symbols(&self, files: &[FileId]) -> WorkspaceSymbols;
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

/// Salsa-compatible wrapper for MethodId.
///
/// Phase 6.5: Used for Salsa queries that take MethodId as parameter.
/// Salsa requires interned types to avoid cloning large structures.
#[salsa::interned(debug)]
pub struct MethodIdInput {
    /// The raw MethodId value
    pub method_id: MethodId,
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
/// Note: `ModuleBodies` implements `Clone` to enable metadata attachment pattern
/// in Salsa queries. Use `Arc<ModuleBodies>` for sharing to avoid cloning overhead.
#[derive(Debug, Clone, PartialEq, Eq)]
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

    /// Get module-level variable declarations.
    /// Includes both exported and non-exported variables.
    pub fn module_vars(&self) -> &[ModuleVarDecl] {
        &self.module_vars
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

// ============================================================================
// Profiling counters for lowering operations
// ============================================================================

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Time spent collecting module variables (first pass)
pub static LOWERING_MODULE_VARS_NS: AtomicU64 = AtomicU64::new(0);

/// Time spent deduplicating module variables
pub static LOWERING_DEDUP_VARS_NS: AtomicU64 = AtomicU64::new(0);

/// Time spent lowering all methods (second pass)
pub static LOWERING_METHODS_NS: AtomicU64 = AtomicU64::new(0);

/// Time spent lowering module-level code (third pass)
pub static LOWERING_MODULE_CODE_NS: AtomicU64 = AtomicU64::new(0);

/// Time spent checking unused variables (fourth pass)
pub static LOWERING_UNUSED_CHECK_NS: AtomicU64 = AtomicU64::new(0);

/// Number of methods lowered
pub static LOWERING_METHOD_COUNT: AtomicU64 = AtomicU64::new(0);

/// Number of modules processed
pub static LOWERING_MODULE_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn reset_lowering_counters() {
    LOWERING_MODULE_VARS_NS.store(0, Ordering::Relaxed);
    LOWERING_DEDUP_VARS_NS.store(0, Ordering::Relaxed);
    LOWERING_METHODS_NS.store(0, Ordering::Relaxed);
    LOWERING_MODULE_CODE_NS.store(0, Ordering::Relaxed);
    LOWERING_UNUSED_CHECK_NS.store(0, Ordering::Relaxed);
    LOWERING_METHOD_COUNT.store(0, Ordering::Relaxed);
    LOWERING_MODULE_COUNT.store(0, Ordering::Relaxed);
}

pub fn print_lowering_counters() {
    let module_vars_ms = LOWERING_MODULE_VARS_NS.load(Ordering::Relaxed) / 1_000_000;
    let dedup_vars_ms = LOWERING_DEDUP_VARS_NS.load(Ordering::Relaxed) / 1_000_000;
    let methods_ms = LOWERING_METHODS_NS.load(Ordering::Relaxed) / 1_000_000;
    let module_code_ms = LOWERING_MODULE_CODE_NS.load(Ordering::Relaxed) / 1_000_000;
    let unused_check_ms = LOWERING_UNUSED_CHECK_NS.load(Ordering::Relaxed) / 1_000_000;
    let method_count = LOWERING_METHOD_COUNT.load(Ordering::Relaxed);
    let module_count = LOWERING_MODULE_COUNT.load(Ordering::Relaxed);

    let total_ms = module_vars_ms + dedup_vars_ms + methods_ms + module_code_ms + unused_check_ms;

    eprintln!("\n=== Lowering Profiling ===");
    eprintln!("  modules_processed:    {:>12}", module_count);
    eprintln!("  methods_lowered:      {:>12}", method_count);
    eprintln!("  module_vars_ms:       {:>12}", module_vars_ms);
    eprintln!("  dedup_vars_ms:        {:>12}", dedup_vars_ms);
    eprintln!("  methods_ms:           {:>12}", methods_ms);
    eprintln!("  module_code_ms:       {:>12}", module_code_ms);
    eprintln!("  unused_check_ms:      {:>12}", unused_check_ms);
    eprintln!("  total_ms:             {:>12}", total_ms);

    if module_count > 0 {
        eprintln!("  avg_module_time_ms:   {:>12.2}", total_ms as f64 / module_count as f64);
    }
    if method_count > 0 {
        eprintln!(
            "  avg_method_time_us:   {:>12.1}",
            (methods_ms as f64 * 1000.0) / method_count as f64
        );
    }
}

/// Lower all method bodies in a module.
///
/// This function walks the AST and lowers each procedure/function body to HIR.
/// Also tracks module-level variables and emits diagnostics for unused ones.
pub fn lower_module_bodies(db: &dyn base_db::RootQueryDb, module_id: ModuleId) -> ModuleBodies {
    use rustc_hash::FxHashSet;
    use syntax::SyntaxKind;

    LOWERING_MODULE_COUNT.fetch_add(1, Ordering::Relaxed);

    let parse = db.parse(module_id.file_id);
    let root = parse.syntax_node();

    let mut result = ModuleBodies::new();
    let mut all_referenced_externals: FxHashSet<String> = FxHashSet::default();

    // Optimization: Single pass to collect both module variables and method nodes
    // This avoids two separate root.descendants() traversals
    let t0 = Instant::now();
    let mut method_nodes: Vec<(syntax::SyntaxNode, bool)> = Vec::new();

    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::VAR_DEF => {
                // Check if this VAR_DEF is inside a method
                let is_inside_method = node.ancestors().any(|n| {
                    matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF)
                });

                if !is_inside_method {
                    collect_module_vars(&node, &mut result.module_vars);
                }
            }
            SyntaxKind::PROCEDURE_DEF => {
                method_nodes.push((node, false)); // false = procedure
            }
            SyntaxKind::FUNCTION_DEF => {
                method_nodes.push((node, true)); // true = function
            }
            _ => {}
        }
    }
    LOWERING_MODULE_VARS_NS.fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);

    // Deduplicate module variables (matching Java behavior)
    let t1 = Instant::now();
    {
        let mut seen_names: FxHashSet<String> = FxHashSet::default();
        result.module_vars.retain(|var| {
            let key = var.name.to_lowercase();
            seen_names.insert(key)
        });
    }

    // Create set of module variable names for method lowering
    let module_var_names: FxHashSet<String> =
        result.module_vars.iter().map(|v| v.name.to_lowercase()).collect();
    LOWERING_DEDUP_VARS_NS.fetch_add(t1.elapsed().as_nanos() as u64, Ordering::Relaxed);

    // Lower all collected methods
    let t2 = Instant::now();
    for (method_idx, (node, is_function)) in method_nodes.into_iter().enumerate() {
        let method_idx = method_idx as u32;
        let lower_result =
            body::lower_method_with_externals(&node, is_function, module_var_names.clone());

        // Collect diagnostics with MethodId
        let method_id = MethodId { module: module_id, local_id: method_idx };
        for diag in &lower_result.diagnostics {
            result.all_diagnostics.push((method_id, diag.clone()));
        }

        // Collect referenced externals
        all_referenced_externals.extend(lower_result.referenced_externals.iter().cloned());

        result.bodies.insert(method_idx, lower_result);
        LOWERING_METHOD_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    LOWERING_METHODS_NS.fetch_add(t2.elapsed().as_nanos() as u64, Ordering::Relaxed);

    // Third pass: lower module-level code (statements outside procedures)
    let t3 = Instant::now();
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
    LOWERING_MODULE_CODE_NS.fetch_add(t3.elapsed().as_nanos() as u64, Ordering::Relaxed);

    // Fourth pass: check for unused module variables
    let t4 = Instant::now();
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
    LOWERING_UNUSED_CHECK_NS.fetch_add(t4.elapsed().as_nanos() as u64, Ordering::Relaxed);

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

// Note: All Salsa query implementations have been moved to the `queries` module.
// See `queries.rs` for the full list of HIR-level queries.
