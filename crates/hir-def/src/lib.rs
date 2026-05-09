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
pub mod call_graph;
pub mod catch_class;
pub mod conditional_tree;
pub mod configs;
pub mod docs;
pub mod hir;
pub mod item_tree;
pub mod metrics;
pub mod module_index;
pub mod name;
pub mod path;
pub mod queries;
pub mod region_tree;
pub mod resolver;
pub mod scope;
pub mod symbol_tree;
pub mod ty;
pub mod type_ref;
pub mod workspace;
pub mod workspace_index;

use std::sync::Arc;

use base_db::SourceRootId;
use vfs::FileId;

pub use body::{
    lower_method, lower_module_code, Body, BodyDiagnostic, BodySourceMap, DeprecatedKind8312,
    ExistingBindingKind, ExternalRef, LowerResult, ManagerType, RedundantAccessKind,
};
pub use hir::{BinaryOp, Binding, Expr, IfStmt, Literal, Stmt, UnaryOp};

// Re-export opaque ID types from cfg-types for backward compatibility.
// These are used by CFG and other external consumers.
pub use cfg_types::{BindingId, ExprId, IdConversion, StmtId};

// ModuleBodies, ModuleMetadata, ExecutionContext are defined in this file, not in modules
pub use conditional_tree::{ConditionalData, ConditionalIdx, ConditionalKind, ConditionalTree};
pub use configs::{ConfigsDatabase, VisibleConfig};
pub use item_tree::ItemTree;
pub use module_index::ModuleIndex;
pub use name::Name;
pub use path::{PathResolution, QualifiedName};
pub use region_tree::{RegionData, RegionIdx, RegionTree};
pub use symbol_tree::{MethodSymbol, ParamSymbol, SymbolTree, VariableSymbol};
pub use ty::{FunctionSignature, Ty};
pub use type_ref::{BuiltinTypeRef, TypeRef};
pub use workspace::{is_bsl_source, CommonModuleInfo, WorkspaceSymbols};
pub use workspace_index::{SymbolInfo, SymbolKind, WorkspaceIndex};

// Re-export all Salsa query functions from the queries module
pub use queries::{
    conditional_tree_query, file_dependencies_query, file_external_refs_query, item_tree_query,
    module_bodies_query, module_call_summary_query, module_data_query, module_index_query,
    region_tree_query, symbol_tree_query, workspace_index_query, workspace_symbols_query,
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

    /// Get all method bodies for a module with their diagnostics.
    ///
    /// Returns lowered HIR bodies for all procedures and functions in the module.
    /// Diagnostics are collected during lowering as a byproduct of semantic analysis
    /// (MissingReturn, MagicNumber, etc.).
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

    /// Get per-module call summary (methods, edges, notify/idle registrations, form entries).
    ///
    /// # Performance
    /// - **LRU cache:** 256
    /// - **Depends on:** [`module_bodies`](Self::module_bodies), [`item_tree`](Self::item_tree), [`module_metadata`](Self::module_metadata)
    /// - **Typical time:** <2ms
    ///
    /// # Implementation
    /// Should delegate to [`module_call_summary_query`].
    fn module_call_summary(&self, module_id: ModuleId) -> Arc<call_graph::ModuleCallSummary>;

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

    /// Get parsed documentation for a module-level variable.
    ///
    /// Pre-computed during `SymbolTree` construction and read from
    /// `VariableSymbol.docs` (cached via `symbol_tree_query` LRU=512).
    /// Returns `None` when the variable carries no description anywhere
    /// (no leading, inter-annotation, or trailing comment).
    fn variable_docs(&self, variable: VariableId) -> Option<Arc<crate::docs::VariableDocs>>;

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
    /// - **Caching:** Salsa-tracked via SourceRootInput, invalidated when source root changes
    ///
    /// # Usage
    /// ```ignore
    /// let source_root_id = db.file_source_root_input(file_id).source_root_id(db);
    /// let symbols = db.workspace_symbols(source_root_id);
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
    /// This is a workspace-level query. Invalidation happens when the source root changes.
    fn workspace_symbols(&self, source_root_id: SourceRootId) -> Arc<WorkspaceSymbols>;

    /// Get workspace-wide symbol index for fast cross-file lookups.
    ///
    /// Builds an index mapping symbol names to their definitions across all files in a source root.
    /// Enables O(C×M) find references where C = candidate files (~10-100) instead of naive O(N×M)
    /// where N = all files (~6,540).
    ///
    /// # Performance
    /// - **Build time:** ~50-100ms for 6,540 files (doc3 project)
    /// - **Cached access:** < 1ms
    /// - **Memory:** ~100-500 KB per 1000 files
    /// - **LRU cache:** 4 (one per source root, typically 1-2 in most projects)
    /// - **Speedup:** ~10-30x for find references in large projects
    ///
    /// # Invalidation
    /// Automatically invalidated when any file in the source root changes (Salsa dependency tracking).
    ///
    /// # Implementation
    /// Should delegate to [`workspace_index_query`](crate::workspace_index::workspace_index_query).
    ///
    /// # Usage
    /// ```ignore
    /// let index = db.workspace_index(source_root_id);
    /// let methods = index.find_methods(&Name::new("МояПроцедура"));
    /// let candidate_files = index.candidate_files(&Name::new("МояФункция"));
    /// ```
    fn workspace_index(
        &self,
        source_root_id: SourceRootId,
    ) -> Arc<crate::workspace_index::WorkspaceIndex>;

    /// Get external module references from a file.
    ///
    /// Extracts ExternalRef from module bodies (collected during HIR lowering).
    /// These references are used to build the module dependency graph.
    ///
    /// # Performance
    /// - **LRU cache:** 512 files
    /// - **Depends on:** [`module_bodies`](Self::module_bodies)
    /// - **Typical time:** < 1ms
    ///
    /// # Implementation
    /// Should delegate to [`file_external_refs_query`].
    fn file_external_refs(&self, module_id: ModuleId) -> Arc<Vec<ExternalRef>>;

    /// Build module index from source root.
    ///
    /// Creates a lightweight index mapping module names to FileIds based on
    /// file paths (Designer format). No parsing is required.
    ///
    /// # Performance
    /// - **LRU cache:** 16 source roots
    /// - **Typical time:** ~10ms for 6,540 files
    ///
    /// # Implementation
    /// Should delegate to [`module_index_query`].
    fn module_index(&self, source_root_id: SourceRootId) -> Arc<ModuleIndex>;

    /// Get file dependencies for a module.
    ///
    /// Resolves external references to actual FileIds using the module index.
    /// Returns the list of files that this module depends on.
    ///
    /// # Performance
    /// - **LRU cache:** 512 files
    /// - **Depends on:** [`file_external_refs`](Self::file_external_refs), [`module_index`](Self::module_index)
    /// - **Typical time:** < 1ms
    ///
    /// # Implementation
    /// Should delegate to [`file_dependencies_query`].
    fn file_dependencies(&self, module_id: ModuleId) -> Arc<Vec<FileId>>;
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

/// Owner of a Body.
///
/// Identifies which top-level definition contains a Body.
/// Used to disambiguate ExprId across different bodies in the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefWithBodyId {
    /// Procedure or function (local_id from ItemTree)
    Method(u32),
    /// Module-level code (executed on module load)
    ModuleCode,
}

/// Unique identifier for SDBL expression in a file context.
///
/// Combines the Body owner and the ExprId within that Body.
/// This is necessary because ExprId is only unique within a single Body,
/// but we need to identify SDBL expressions across all bodies in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SdblExprId {
    /// Owner of the Body containing this expression
    pub owner: DefWithBodyId,
    /// ExprId within the Body
    pub expr_id: ExprId,
}

impl SdblExprId {
    /// Create a new SdblExprId for a method body.
    pub fn from_method(local_id: u32, expr_id: ExprId) -> Self {
        Self { owner: DefWithBodyId::Method(local_id), expr_id }
    }

    /// Create a new SdblExprId for module-level code.
    pub fn from_module_code(expr_id: ExprId) -> Self {
        Self { owner: DefWithBodyId::ModuleCode, expr_id }
    }
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

    /// Register metadata if this module belongs to a register.
    ///
    /// Used for InformationRegister, AccumulationRegister, AccountingRegister, CalculationRegister.
    /// Arc-wrapped for efficient sharing.
    pub register: Option<Arc<bsl_metadata::Register>>,

    /// Form metadata if this module is a FormModule.
    ///
    /// Contains form type (Managed/Ordinary) needed for ServerSideExportFormMethod diagnostic.
    /// Arc-wrapped for efficient sharing.
    pub form: Option<Arc<bsl_metadata::Form>>,

    /// HTTP service metadata if this module is an HTTPServiceModule.
    ///
    /// Contains URL templates and methods for WrongHttpServiceHandler diagnostic.
    /// Arc-wrapped for efficient sharing.
    pub http_service: Option<Arc<bsl_metadata::HTTPService>>,

    /// Web service (SOAP) metadata if this module is a WebServiceModule.
    ///
    /// Contains operations for WrongWebServiceHandler diagnostic.
    /// Arc-wrapped for efficient sharing.
    pub web_service: Option<Arc<bsl_metadata::WebService>>,
}

impl ModuleMetadata {
    /// Create metadata for a module with no metadata available.
    ///
    /// Used when metadata loading fails or module is outside Designer format.
    pub fn unknown(module_type: bsl_metadata::ModuleType) -> Self {
        Self {
            module_type,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            form: None,
            http_service: None,
            web_service: None,
        }
    }
}

/// All method bodies for a module with their diagnostics.
///
/// This structure contains the lowered HIR bodies for all procedures and functions
/// in a module, along with diagnostics collected during lowering.
///
/// Metadata is populated by the `module_bodies()` query in ide-db.
///
/// Note: This struct is returned from Salsa cached query. Do NOT clone it -
/// always use Arc<ModuleBodies> for sharing to benefit from Salsa's LRU cache.
/// Metadata is stored separately and accessed via module_metadata() query.
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

    /// Build ModuleBodies from a parse result (without Salsa).
    ///
    /// This is the pure version for streaming mode.
    pub fn from_parse(parse: &syntax::Parse<syntax::SyntaxNode>, module_id: ModuleId) -> Self {
        let root = parse.syntax_node();
        Self::lower_from_root(&root, module_id, None)
    }

    /// Shared lowering logic for both streaming and Salsa modes.
    ///
    /// Walks the AST once, collects module variables and method nodes,
    /// then lowers all method bodies and module-level code.
    fn lower_from_root(
        root: &syntax::SyntaxNode,
        module_id: ModuleId,
        line_index: Option<std::sync::Arc<line_index::LineIndex>>,
    ) -> Self {
        use rustc_hash::FxHashSet;
        use syntax::SyntaxKind;

        let mut result = ModuleBodies::new();
        // Track (top_level_idx, node, is_function) to match ItemTree index space
        let mut method_nodes: Vec<(u32, syntax::SyntaxNode, bool)> = Vec::new();
        let mut top_level_idx: u32 = 0;

        // Single pass to collect module variables and method nodes
        for node in root.descendants() {
            match node.kind() {
                SyntaxKind::VAR_DEF => {
                    let is_inside_method = node.ancestors().any(|n| {
                        matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF)
                    });
                    if !is_inside_method {
                        collect_module_vars(&node, &mut result.module_vars);
                        top_level_idx += 1;
                    }
                }
                SyntaxKind::PROCEDURE_DEF => {
                    method_nodes.push((top_level_idx, node, false));
                    top_level_idx += 1;
                }
                SyntaxKind::FUNCTION_DEF => {
                    method_nodes.push((top_level_idx, node, true));
                    top_level_idx += 1;
                }
                _ => {}
            }
        }

        // Deduplicate module variables
        {
            let mut seen_names: FxHashSet<String> = FxHashSet::default();
            result.module_vars.retain(|var| {
                let key = var.name.to_lowercase();
                seen_names.insert(key)
            });
        }

        // Lower all methods — use top_level_idx to match ItemTree index space
        for (item_tree_idx, node, is_function) in method_nodes.into_iter() {
            let lower_result =
                body::lower_method_with_externals(&node, is_function, line_index.clone());

            let method_id = MethodId { module: module_id, local_id: item_tree_idx };
            for diag in &lower_result.diagnostics {
                result.all_diagnostics.push((method_id, diag.clone()));
            }

            result.bodies.insert(item_tree_idx, lower_result);
        }

        // Lower module-level code
        let module_code_result = body::lower_module_code(root, line_index);
        let module_method_id = MethodId { module: module_id, local_id: u32::MAX };
        for diag in &module_code_result.diagnostics {
            result.all_diagnostics.push((module_method_id, diag.clone()));
        }
        result.module_code = Some(module_code_result);

        result
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

    /// Iterate over all LowerResults.
    /// Useful for extracting external references from all method bodies.
    pub fn iter_lower_results(&self) -> impl Iterator<Item = (u32, &body::LowerResult)> {
        self.bodies.iter().map(|(local_id, lower_result)| (*local_id, lower_result))
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
/// Context determination follows module metadata properties.
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
    let parse = db.parse(module_id.file_id);
    let root = parse.syntax_node();

    let file_text_input = db.file_text_input(module_id.file_id);
    let file_text = file_text_input.text(db);
    let line_index = std::sync::Arc::new(line_index::LineIndex::new(&file_text));

    ModuleBodies::lower_from_root(&root, module_id, Some(line_index))
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
