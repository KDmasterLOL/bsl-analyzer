//! Method body representation and source mapping.
//!
//! This module contains:
//! - `Body` - HIR representation of a method body
//! - `BodySourceMap` - bidirectional mapping between HIR and AST
//!
//! ## Architecture
//!
//! ```text
//! AST (Method body)
//!       │
//!       ▼ lower()
//! ┌─────────────────────────────────────┐
//! │           Body                      │
//! │  ┌─────────────────────────────┐   │
//! │  │ exprs: Arena<Expr>          │   │  HIR expressions
//! │  │ stmts: Arena<Stmt>          │   │  HIR statements
//! │  │ bindings: Arena<Binding>    │   │  Local variables/params
//! │  │ params: Box<[BindingId]>    │   │  Parameter IDs
//! │  │ body_stmts: Box<[StmtId]>   │   │  Top-level statements
//! │  └─────────────────────────────┘   │
//! │           +                         │
//! │  ┌─────────────────────────────┐   │
//! │  │ BodySourceMap               │   │  HIR ↔ AST mapping
//! │  │ expr_map: ExprId → AstPtr   │   │  For diagnostics
//! │  │ stmt_map: StmtId → AstPtr   │   │
//! │  └─────────────────────────────┘   │
//! └─────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! Body is created by lowering an AST method definition and is cached by Salsa.
//! Diagnostics are collected during lowering and returned alongside the Body.

pub mod lower;

use la_arena::Arena;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::SyntaxNode;
use text_size::TextRange;

use crate::hir::{Binding, BindingIdx, Expr, ExprIdx, Stmt, StmtIdx};
use crate::Name;

// Opaque ID types for public API
use cfg_types::{BindingId, ExprId, IdConversion, StmtId};

/// HIR representation of a method body.
///
/// Contains all expressions, statements, and bindings in arena-allocated form.
/// This allows efficient storage and stable IDs for referencing HIR nodes.
///
/// **NOTE**: Internal fields use typed Idx<T> for type safety during lowering.
/// Public API methods return opaque IDs (cfg_types) for external consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    /// All expressions in this body (internal arena with typed indices).
    pub(crate) exprs: Arena<Expr>,
    /// All statements in this body (internal arena with typed indices).
    pub(crate) stmts: Arena<Stmt>,
    /// All local bindings (variables and parameters, internal arena).
    pub(crate) bindings: Arena<Binding>,
    /// Parameter binding IDs (in declaration order, typed for internal use).
    pub(crate) params: Box<[BindingIdx]>,
    /// Top-level statements in the method body (typed for internal use).
    pub(crate) body_stmts: Box<[StmtIdx]>,

    /// SDBL queries found in this method body (typed ExprIdx for internal use).
    /// Maps ExprIdx (Expr::Literal with SDBL string) to parsed SDBL query info.
    pub(crate) sdbl_exprs: Vec<(ExprIdx, syntax::SdblQueryInfo)>,

    /// Expressions lowered from syntactic `ERROR` recovery nodes.
    ///
    /// BSL rejects bare member access like `obj.` or `obj.field` as a
    /// statement (only assign/call are valid). The parser emits a well-formed
    /// `FIELD_EXPR` subtree inside a `NodeKind::Error` wrapper; HIR lowering
    /// unwraps that subtree as a best-effort `Stmt::Expr` so completion /
    /// hover / inference can still reason about the receiver. These ExprIds
    /// are marked "recovered" so inference-layer diagnostics and the CFG can
    /// opt out (see `crates/hir-ty/src/infer.rs` and `crates/cfg`).
    pub(crate) recovered_exprs: FxHashSet<ExprIdx>,
}

impl Default for Body {
    fn default() -> Self {
        Self::new()
    }
}

impl Body {
    /// Create a new empty body.
    pub fn new() -> Self {
        Self {
            exprs: Arena::new(),
            stmts: Arena::new(),
            bindings: Arena::new(),
            params: Box::new([]),
            body_stmts: Box::new([]),
            sdbl_exprs: Vec::new(),
            recovered_exprs: FxHashSet::default(),
        }
    }

    /// Allocate an expression and return its opaque [`ExprId`].
    ///
    /// **Test / programmatic-construction helper, not part of the
    /// regular lowering surface.** Lowering populates a `Body` via
    /// direct crate-internal arena access (and also records source-map
    /// entries, top-level body stmts, etc.); this method only forwards
    /// to the arena and does NOT update `body_stmts`, the source map,
    /// or any other side-table. It is intended for downstream crates
    /// that need to hand-roll a tiny `Body` for a unit test (e.g. the
    /// `dataflow::path_terminates` tests build minimal `Stmt::Return` /
    /// `Stmt::Raise` bodies wired into a hand-built CFG to exercise the
    /// lattice transfer in isolation from the parser+lowering stack).
    /// Production callers should not use this — go through lowering.
    #[doc(hidden)]
    pub fn alloc_expr(&mut self, expr: Expr) -> ExprId {
        ExprId::from_idx(self.exprs.alloc(expr))
    }

    /// Allocate a statement and return its opaque [`StmtId`]. See
    /// [`Body::alloc_expr`] for the rationale and the same caveats —
    /// this does NOT update `body_stmts` or the source map.
    #[doc(hidden)]
    pub fn alloc_stmt(&mut self, stmt: Stmt) -> StmtId {
        StmtId::from_idx(self.stmts.alloc(stmt))
    }

    /// Get an expression by ID (opaque → typed conversion).
    pub fn expr(&self, id: ExprId) -> &Expr {
        let typed_id: ExprIdx = id.to_idx();
        &self.exprs[typed_id]
    }

    /// Get a statement by ID (opaque → typed conversion).
    pub fn stmt(&self, id: StmtId) -> &Stmt {
        let typed_id: StmtIdx = id.to_idx();
        &self.stmts[typed_id]
    }

    /// Get a binding by ID (opaque → typed conversion).
    pub fn binding(&self, id: BindingId) -> &Binding {
        let typed_id: BindingIdx = id.to_idx();
        &self.bindings[typed_id]
    }

    /// Get expression by typed index (used during lowering and cfg building).
    pub fn expr_idx(&self, id: ExprIdx) -> &Expr {
        &self.exprs[id]
    }

    /// Get statement by typed index (used during lowering and cfg building).
    pub fn stmt_idx(&self, id: StmtIdx) -> &Stmt {
        &self.stmts[id]
    }

    /// Get binding by typed index (used during lowering and cfg building).
    pub fn binding_idx(&self, id: BindingIdx) -> &Binding {
        &self.bindings[id]
    }

    /// Get the number of expressions.
    pub fn expr_count(&self) -> usize {
        self.exprs.len()
    }

    /// Get the number of statements.
    pub fn stmt_count(&self) -> usize {
        self.stmts.len()
    }

    /// Get the number of bindings.
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    /// Get top-level statements as typed indices (for dataflow analysis).
    pub fn body_stmts_typed(&self) -> &[StmtIdx] {
        &self.body_stmts
    }

    /// Iterate over all expressions (converts internal Idx to opaque ExprId).
    pub fn exprs_iter(
        &self,
    ) -> impl ExactSizeIterator<Item = (ExprId, &Expr)> + DoubleEndedIterator + Clone {
        self.exprs.iter().map(|(idx, expr)| (ExprId::from_idx(idx), expr))
    }

    /// Iterate over all statements (converts internal Idx to opaque StmtId).
    pub fn stmts_iter(
        &self,
    ) -> impl ExactSizeIterator<Item = (StmtId, &Stmt)> + DoubleEndedIterator + Clone {
        self.stmts.iter().map(|(idx, stmt)| (StmtId::from_idx(idx), stmt))
    }

    /// Iterate over all bindings (converts internal Idx to opaque BindingId).
    pub fn bindings_iter(
        &self,
    ) -> impl ExactSizeIterator<Item = (BindingId, &Binding)> + DoubleEndedIterator + Clone {
        self.bindings.iter().map(|(idx, binding)| (BindingId::from_idx(idx), binding))
    }

    /// Get all SDBL expressions in this body (with opaque ExprId).
    pub fn sdbl_exprs(&self) -> impl Iterator<Item = (ExprId, &syntax::SdblQueryInfo)> {
        self.sdbl_exprs.iter().map(|(idx, info)| (ExprId::from_idx(*idx), info))
    }

    /// Whether this expression was reconstructed from a parser ERROR node.
    ///
    /// Recovered expressions carry valid type information (inference looks at
    /// them like any other expression), but inference-layer diagnostics and
    /// CFG construction should usually skip them to avoid noise on code the
    /// user is still typing.
    pub fn is_recovered(&self, id: ExprId) -> bool {
        let typed_id: ExprIdx = id.to_idx();
        self.recovered_exprs.contains(&typed_id)
    }

    /// Get parameter binding IDs (opaque, in declaration order).
    pub fn params(&self) -> impl Iterator<Item = BindingId> + '_ {
        self.params.iter().map(|&idx| BindingId::from_idx(idx))
    }

    /// Get top-level statement IDs (opaque).
    pub fn body_stmts(&self) -> impl Iterator<Item = StmtId> + '_ {
        self.body_stmts.iter().map(|&idx| StmtId::from_idx(idx))
    }

    // ===== Test helpers =====
    // These methods are public to allow test code in other crates (like cfg tests)
    // to construct Body instances directly without going through the lowering pipeline.

    /// Access expression arena mutably (for tests only).
    #[doc(hidden)]
    pub fn exprs_mut(&mut self) -> &mut Arena<Expr> {
        &mut self.exprs
    }

    /// Access statement arena mutably (for tests only).
    #[doc(hidden)]
    pub fn stmts_mut(&mut self) -> &mut Arena<Stmt> {
        &mut self.stmts
    }

    /// Access binding arena mutably (for tests only).
    #[doc(hidden)]
    pub fn bindings_mut(&mut self) -> &mut Arena<Binding> {
        &mut self.bindings
    }

    /// Set body statements (for tests only).
    #[doc(hidden)]
    pub fn set_body_stmts(&mut self, stmts: Box<[StmtIdx]>) {
        self.body_stmts = stmts;
    }

    /// Set parameters (for tests only).
    #[doc(hidden)]
    pub fn set_params(&mut self, params: Box<[BindingIdx]>) {
        self.params = params;
    }
}

/// Bidirectional mapping between HIR and AST.
///
/// Used for:
/// - Diagnostics: HIR node → source location
/// - Go-to-definition: source location → HIR node
///
/// ## Memory Optimization
///
/// Uses Vec instead of HashMap for ID→Range mappings since IDs are
/// sequential arena indices. This saves ~78% memory per mapping:
/// - HashMap: ~36 bytes per entry (key + value + bucket overhead)
/// - Vec: ~8 bytes per entry (just TextRange, index is implicit)
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BodySourceMap {
    /// Expression ID → source range (index = ExprId.into_raw().into_u32()).
    expr_ranges: Vec<Option<TextRange>>,
    /// Statement ID → source range (index = StmtId.into_raw().into_u32()).
    stmt_ranges: Vec<Option<TextRange>>,
    /// Binding ID → source range (index = BindingId.into_raw().into_u32()).
    binding_ranges: Vec<Option<TextRange>>,

    /// Source range → Expression ID (for reverse lookup).
    /// Kept as HashMap since TextRange keys aren't sequential.
    range_to_expr: FxHashMap<TextRange, ExprId>,
    /// Source range → Statement ID (for reverse lookup).
    range_to_stmt: FxHashMap<TextRange, StmtId>,
    /// Source range → Binding ID (for reverse lookup).
    range_to_binding: FxHashMap<TextRange, BindingId>,
}

impl BodySourceMap {
    /// Create a new empty source map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record expression source range (accepts typed Idx during lowering).
    pub(crate) fn record_expr(&mut self, id: ExprIdx, range: TextRange) {
        let opaque_id = ExprId::from_idx(id);
        let idx = id.into_raw().into_u32() as usize;
        if idx >= self.expr_ranges.len() {
            self.expr_ranges.resize(idx + 1, None);
        }
        self.expr_ranges[idx] = Some(range);
        self.range_to_expr.insert(range, opaque_id);
    }

    /// Record statement source range (accepts typed Idx during lowering).
    pub(crate) fn record_stmt(&mut self, id: StmtIdx, range: TextRange) {
        let opaque_id = StmtId::from_idx(id);
        let idx = id.into_raw().into_u32() as usize;
        if idx >= self.stmt_ranges.len() {
            self.stmt_ranges.resize(idx + 1, None);
        }
        self.stmt_ranges[idx] = Some(range);
        self.range_to_stmt.insert(range, opaque_id);
    }

    /// Record binding source range (accepts typed Idx during lowering).
    pub(crate) fn record_binding(&mut self, id: BindingIdx, range: TextRange) {
        let opaque_id = BindingId::from_idx(id);
        let idx = id.into_raw().into_u32() as usize;
        if idx >= self.binding_ranges.len() {
            self.binding_ranges.resize(idx + 1, None);
        }
        self.binding_ranges[idx] = Some(range);
        self.range_to_binding.insert(range, opaque_id);
    }

    /// Get source range for an expression.
    pub fn expr_range(&self, id: ExprId) -> Option<TextRange> {
        let idx = id.into_raw().into_u32() as usize;
        self.expr_ranges.get(idx).copied().flatten()
    }

    /// Get source range for a statement.
    pub fn stmt_range(&self, id: StmtId) -> Option<TextRange> {
        let idx = id.into_raw().into_u32() as usize;
        self.stmt_ranges.get(idx).copied().flatten()
    }

    /// Get source range for a binding.
    pub fn binding_range(&self, id: BindingId) -> Option<TextRange> {
        let idx = id.into_raw().into_u32() as usize;
        self.binding_ranges.get(idx).copied().flatten()
    }

    /// Find expression at a given range.
    pub fn expr_at_range(&self, range: TextRange) -> Option<ExprId> {
        self.range_to_expr.get(&range).copied()
    }

    /// Find statement at a given range.
    pub fn stmt_at_range(&self, range: TextRange) -> Option<StmtId> {
        self.range_to_stmt.get(&range).copied()
    }

    /// Find binding at a given range.
    ///
    /// Used by hover/goto on declaration-site identifiers (loop variable
    /// in `Для Каждого X Из …`, classic `Для X = … По …`, `Перем X`,
    /// procedure parameters) where no `Expr::Path` is created and the
    /// range maps directly to the freshly allocated binding.
    pub fn binding_at_range(&self, range: TextRange) -> Option<BindingId> {
        self.range_to_binding.get(&range).copied()
    }
}

/// Result of body lowering.
///
/// Contains the lowered body, source map, and any diagnostics collected during lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerResult {
    /// The lowered HIR body.
    pub body: Body,
    /// Mapping between HIR and AST.
    pub source_map: BodySourceMap,
    /// Diagnostics collected during lowering.
    pub diagnostics: Vec<BodyDiagnostic>,
    /// Variables referenced but not locally declared (potential module-level variables).
    /// Lowercase names for case-insensitive comparison.
    pub referenced_externals: rustc_hash::FxHashSet<String>,
    /// External module references collected during lowering.
    /// Used to build module dependency graph for lazy loading.
    pub external_refs: Vec<ExternalRef>,
}

/// Pre-existing binding kind captured by lowering for an assignment
/// target whose name shadows a configuration-scope binding (e.g. a
/// CommonModule).
///
/// Recorded **before** the implicit `register_local_var` runs so the
/// downstream diagnostic handler (`CommonModuleAssign`,
/// `ThisObjectAssign` future work) can fast-path-skip when a real
/// local / param shadows the configuration name without rebuilding a
/// `Resolver`. The enum stays intentionally narrow — extending it
/// later (`ModuleVariable`, `ImportedAlias`) is non-breaking; the
/// `Option<ExistingBindingKind>` payload uses `None` for "no
/// shadowing tracked" rather than a placeholder variant, so absence
/// has a single, unambiguous meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingBindingKind {
    /// Identifier is already a local `Перем` declared earlier in the
    /// body.
    Local,
    /// Identifier is a procedure / function parameter name.
    Param,
}

/// Diagnostic collected during body lowering.
///
/// These diagnostics are emitted as a byproduct of lowering AST to HIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyDiagnostic {
    /// Missing return statement in function.
    MissingReturn { range: TextRange },

    /// Empty code block (if/while/for/try with empty body).
    EmptyCodeBlock { range: TextRange },

    /// Deprecated method call.
    DeprecatedMethod { name: String, range: TextRange },

    /// Deprecated ТекущаяДата() / CurrentDate() method call.
    /// Separate from DeprecatedMethod because it has different severity (Error vs Info).
    DeprecatedCurrentDate { name: String, range: TextRange },

    /// Deprecated Найти() / Find() global method call.
    /// Separate from DeprecatedMethod because it's a commonly misused global function.
    DeprecatedFind { name: String, range: TextRange },

    /// Deprecated Сообщить() / Message() global method call.
    /// Separate from DeprecatedMethod because it's a commonly misused global function.
    DeprecatedMessage { name: String, range: TextRange },

    /// Deprecated Тип("УправляемаяФорма") / Type("ManagedForm") call.
    /// Detected when Type() is called with deprecated type name string.
    DeprecatedTypeManagedForm { type_name: String, range: TextRange },

    /// Magic number literal (hardcoded number that should be a constant).
    /// Value is stored as string to allow Eq derivation.
    /// Context is used for filtering by excludedConstructors and allowMagicIndexes config.
    MagicNumber { value: String, range: TextRange, context: MagicNumberContext },

    /// Self-assignment (a = a).
    SelfAssign { range: TextRange },

    /// Function should have return.
    FunctionShouldHaveReturn { range: TextRange },

    /// BeginTransaction/НачатьТранзакцию call not immediately followed by Try statement.
    /// Detects three violation patterns:
    /// 1. Code between BeginTransaction and Try
    /// 2. BeginTransaction inside Try block
    /// 3. BeginTransaction without subsequent Try
    BeginTransactionBeforeTryCatch { range: TextRange },

    /// Method call that may have missing required parameters.
    /// Emitted during lowering for all calls - validation happens in from_hir().
    ///
    /// Fields:
    /// - `callee`: Name of the method being called
    /// - `module`: Optional module name for qualified calls (Module.Method)
    /// - `mdo_type`: Optional MDO type keyword for three-level calls (Документы.ПКО.Method)
    /// - `mdo_name`: Optional MDO name for three-level calls
    /// - `args`: Boolean array - true if argument has value, false if empty/missing
    /// - `range`: Source range for the diagnostic
    ///
    /// Call patterns:
    /// - Local: `Method()` → module=None, mdo_type=None, mdo_name=None
    /// - Two-level: `Module.Method()` → module=Some, mdo_type=None, mdo_name=None
    /// - Three-level: `Документы.ПКО.Method()` → module=None, mdo_type=Some, mdo_name=Some
    /// - ThisObject: `ЭтотОбъект.Method()` → module=Some("ЭтотОбъект"), mdo_type=None
    MissedRequiredParameter {
        callee: String,
        module: Option<String>,
        mdo_type: Option<String>,
        mdo_name: Option<String>,
        args: Vec<bool>,
        range: TextRange,
    },

    /// Duplicated code block in if/elsif/else branches.
    /// Detected when two or more branches contain structurally identical code.
    IfElseDuplicatedCodeBlock { range: TextRange },

    /// Code after asynchronous method call.
    /// Detects code that executes immediately after async calls - a common logic error
    /// because async methods return immediately without waiting for completion.
    CodeAfterAsyncCall { method_name: String, range: TextRange },

    /// CommitTransaction/ЗафиксироватьТранзакцию call not properly protected by try-catch.
    /// Detects four violation patterns:
    /// 1. Outside try-catch entirely
    /// 2. Inside exception handler (should be in try body)
    /// 3. Try without except clause
    /// 4. Code after commit in try body (commit must be last)
    CommitTransactionOutsideTryCatch { range: TextRange },

    /// Assignment to a potential CommonModule name.
    /// Emitted during lowering for simple identifier assignments.
    /// Validation against metadata happens in from_hir().
    ///
    /// `existing_binding_kind` records whether the assignment target
    /// already had a local / param binding **before** the implicit
    /// `register_local_var` ran, so the diagnostic handler can fast-
    /// path-skip on shadowing without rebuilding a `Resolver`. `None`
    /// means "no shadowing — the name introduces a fresh implicit
    /// binding (or is a re-assignment without an existing binding
    /// kind we tracked)". The enum is intentionally not exhaustive
    /// over all possible bindings (no `ModuleVariable`, no
    /// `Builtin`); those land in the handler's resolver path.
    CommonModuleAssign {
        variable_name: String,
        range: TextRange,
        existing_binding_kind: Option<ExistingBindingKind>,
    },

    /// Missing or non-export method call in CommonModule.
    ///
    /// Collected during HIR lowering when encountering qualified calls (Module.Method).
    /// The actual validation (resolution + export check) happens in from_hir() handler
    /// with access to ctx.db.
    ///
    /// Examples:
    /// - `CommonModule.NonExistentMethod()`
    /// - `CommonModule.PrivateMethod()`
    /// - `NonExistentModule.Method()`
    ///
    /// The diagnostic handler will determine the specific error type (MethodNotFound,
    /// NonExportMethod, ModuleNotFound) using workspace symbols and SymbolTree.
    MissingCommonModuleMethod { module: String, method: String, range: TextRange },

    /// Overwrite of byValue parameter without prior use.
    /// Emitted during lowering when a parameter marked with Знач/ByValue is assigned to.
    /// Validation using reaching definitions happens in from_hir() to check if parameter
    /// was used before the assignment.
    RewriteMethodParameter {
        param_id: BindingId,    // Parameter being overwritten
        stmt_id: StmtId,        // Assignment statement for CFG analysis
        stmt_range: TextRange,  // Full statement range for BodySourceMap lookup
        ident_range: TextRange, // Identifier range for diagnostic display
    },

    /// Query/QueryBuilder/ReportBuilder Execute() call inside a loop.
    /// Detects when Execute() is called on Query-like objects inside loops,
    /// which causes severe performance degradation.
    CreateQueryInCycle { range: TextRange },

    /// Deletion of collection item during ForEach iteration over that collection.
    /// Detected when Delete/Удалить method is called on the same collection being iterated.
    DeletingCollectionItem {
        collection_text: String, // Human-readable collection name for error message
        range: TextRange,        // Range of the Delete call
    },

    /// Self-insertion: collection is inserted into itself.
    /// Examples: `arr.Добавить(arr)`, `struct.Вставить("key", struct)`
    SelfInsertion { range: TextRange },

    /// Deprecated attribute/method usage (8.3.12).
    /// Detected when deprecated chart-related attributes, methods, or enums are used.
    /// Categories:
    /// - Chart/ChartPlotArea attributes (ОтображатьШкалу, ОтображатьЛегенду, etc.)
    /// - Chart methods (ПолучитьПалитру, УстановитьПалитру)
    /// - Global methods (ОчиститьЖурналРегистрации)
    /// - Enum names (ОриентацияМетокДиаграммы)
    /// - Enum values (ГруппировкаПодчиненныхЭлементовФормы.Горизонтальная)
    DeprecatedAttribute8312 {
        name: String, // Original member name for error message
        kind: DeprecatedKind8312,
        range: TextRange,
    },

    /// Execute() statement or Eval()/Вычислить() call.
    /// Detects arbitrary code execution which is forbidden on server for security reasons.
    /// Only allowed in client-only context (&НаКлиенте annotation).
    ExecuteExternalCode { range: TextRange },

    /// External application starting methods.
    /// Detects calls to КомандаСистемы/System, ЗапуститьПриложение/RunApp, etc.
    /// Security risk: allows arbitrary command execution.
    ExternalAppStarting { range: TextRange },

    /// Trailing comma in function/method call argument list.
    /// Detects commas before closing parenthesis: Method(a, b,)
    ExtraCommas { range: TextRange },

    /// File system access operations.
    /// Detects:
    /// - Constructor patterns: Новый Файл, Новый ЗаписьТекста, Новый xBase, etc.
    /// - Global methods: КопироватьФайл, УдалитьФайлы, СоздатьКаталог, etc.
    ///
    /// Security risk: creates attack vectors for data exfiltration and file manipulation.
    FileSystemAccess { range: TextRange },

    /// FormDataToValue() / ДанныеФормыВЗначение() call in method with context.
    /// Detects calls to FormDataToValue in methods WITHOUT БезКонтекста annotation.
    /// Allowed in: @НаСервереБезКонтекста, @НаКлиентеНаСервереБезКонтекста.
    /// Bad practice: creates unnecessary form context dependency.
    FormDataToValue { range: TextRange },

    /// Function name starts with "Получить" (Russian for "Get").
    /// According to 1C coding standards, function names should not use "Получить" prefix.
    /// Only applies to functions (FUNCTION_DEF), not procedures.
    FunctionNameStartsWithGet { name: String, range: TextRange },

    /// Function modifies by-reference parameter (output parameter).
    /// Detected when a function (not procedure) assigns to a parameter declared without "Знач".
    /// Only simple assignments are flagged - property/index assignments are ignored.
    /// Functions should use return values instead of output parameters.
    FunctionOutParameter { name: String, range: TextRange },

    /// Function always returns the same primitive value in all branches.
    /// Detected when a function has 2+ return statements, all returning the same primitive literal.
    /// Primitives: numbers, strings, boolean literals, Null, Undefined.
    /// Variables and function calls are not considered primitives.
    /// Attachable methods (starting with "Подключаемый_"/"Attachable_") are excluded.
    FunctionReturnsSamePrimitive { range: TextRange },

    /// Usage of deprecated ПолучитьФорму() / GetForm() method.
    /// Detected when calling GetForm/ПолучитьФорму method (case-insensitive).
    /// This is an error-prone approach that returns managed form objects (deprecated).
    /// Should be replaced with ОткрытьФорму() / OpenForm().
    GetFormMethod { method_name: String, range: TextRange },

    /// Method name collides with platform 8.3.12 global context method.
    /// Detected when function name matches bitwise operation methods added in 8.3.12:
    /// ПроверитьБит/CheckBit, ПобитовоеИ/BitwiseAnd, etc. (20 methods total).
    /// User-defined methods with these names will conflict with platform methods.
    GlobalContextMethodCollision8312 { method_name: String, range: TextRange },

    /// Empty preprocessor region (#Область/#КонецОбласти).
    /// Detected when a region contains no meaningful content (only comments/whitespace/nested empty regions).
    EmptyRegion { name: String, range: TextRange },

    /// Empty statement (standalone semicolon).
    /// Detected when EMPTY_STMT AST node is encountered without parser errors nearby.
    EmptyStatement { range: TextRange },

    /// Statement without trailing semicolon.
    /// Detected when statement AST node has no SEMICOLON token.
    MissingSemicolon { range: TextRange },

    /// Duplicated condition in if/elsif chain.
    /// Detected when an elsif condition is identical to a previous if/elsif condition.
    /// First occurrence index is 0-based (0 = if, 1+ = elsif).
    IfElseDuplicatedCondition { first_occurrence_index: usize, range: TextRange },

    /// If-elsif chain without else clause.
    /// Detected when if statement has elsif but no else (missing default case).
    /// Range points to КонецЕсли/EndIf keyword.
    IfElseIfEndsWithElse { range: TextRange },

    /// Incorrect usage of СтрШаблон/StrTemplate method.
    /// Detected when template string has mismatched parameter count, invalid placeholders, or wrong numbers.
    /// Checks: %1-%10 valid, %0/%11+ invalid, parameter count matches placeholders.
    IncorrectUseOfStrTemplate { range: TextRange },

    /// Multiple statements on one line.
    /// Detected when more than one statement starts on the same line.
    /// Exclusions: preprocessor directives, empty statements (`;`), statements with parse errors.
    OneStatementPerLine { range: TextRange },

    /// ПользователиОС() / OSUsers() call.
    /// Security risk: potential Pass-the-hash attack vulnerability.
    OSUsersMethod { range: TextRange },

    /// Return statement with value inside a procedure.
    /// Only functions can return values, procedures must use `Return;` without value.
    ProcedureReturnsValue { range: TextRange },

    /// Reserved keyword used as procedure/function name.
    /// Platform will reject such names with a compilation error.
    ReservedWordAsMethodName { name: String, range: TextRange },

    /// Redundant access to object via ЭтотОбъект/ThisObject or module name.
    ///
    /// Emitted during lowering as candidates - validation against module type/metadata
    /// happens in from_hir().
    ///
    /// Examples:
    /// - ObjectModule: `ЭтотОбъект.Контрагент` → redundant, use `Контрагент`
    /// - CommonModule: `МойМодуль.МояФункция()` → redundant, use `МояФункция()`
    /// - ManagerModule: `Справочники.Справочник1.Метод()` → redundant, use `Метод()`
    ///
    /// Exclusions:
    /// - `ЭтотОбъект["Поле"]` is NOT an error (INDEX_EXPR handled separately)
    /// - CommonModule with ReturnValueReuse != DontUse are NOT checked
    RedundantAccessToObject { kind: RedundantAccessKind, range: TextRange },

    /// Style element constructor (Цвет/Color, Шрифт/Font, Рамка/Border).
    /// Detected when New expression creates a style element type.
    /// Should be replaced with getting style element from configuration.
    StyleElementConstructors { type_name: String, range: TextRange },

    /// TempFilesDir/КаталогВременныхФайлов() method call.
    /// Detected when global TempFilesDir method is called.
    /// Should use GetTempFileName/ПолучитьИмяВременногоФайла instead.
    TempFilesDir { name: String, range: TextRange },

    /// Usage of ternary operator `?(condition, true_value, false_value)`.
    /// Disabled by default. Recommends using If-Else instead for readability.
    TernaryOperatorUsage { range: TextRange },

    /// Too many return statements in a method/function.
    /// Detected when method has more return statements than configured threshold.
    TooManyReturns { method_name: String, method_name_range: TextRange, returns: Vec<TextRange> },

    /// Unary plus in concatenation (accidental double plus: `"str" + + expr`).
    /// Detected when binary plus operator has unary plus on right operand (not a numeric literal).
    /// This is usually a typo - platform will try to convert right operand to number, causing runtime error.
    /// Examples: `"str" + + "str2"` (error), `"str" + + 5` (valid), `"str" + + variable` (error).
    UnaryPlusInConcatenation { range: TextRange },

    /// SystemInformation/СистемнаяИнформация constructor usage.
    /// Detected when New expression creates SystemInformation object.
    /// This is a security hotspot as it exposes system information.
    UseSystemInformation { range: TextRange },

    /// Invalid assignment to Cancel/Отказ parameter.
    /// Detected when Cancel parameter is assigned a value other than True or OR expression with Cancel.
    /// Valid: `Отказ = Истина;` or `Отказ = Отказ ИЛИ Выражение;` or `Отказ = Выражение ИЛИ Отказ;`
    /// Invalid: `Отказ = Ложь;`, `Отказ = Метод();`, `Отказ = Отказ И Выражение;`
    UsingCancelParameter { range: TextRange },

    /// Usage of external code execution tools (ExternalDataProcessors, ExternalReports, ConfigurationExtensions).
    /// Detected when calling Create/Connect methods on these global objects.
    /// Security risk: external code can execute arbitrary operations.
    UsingExternalCodeTools { range: TextRange },

    /// Using FindByName, FindByCode, FindByNumber with literal argument.
    /// Detected when calling НайтиПоНаименованию/FindByDescription, НайтиПоКоду/FindByCode,
    /// НайтиПоНомеру/FindByNumber with string or number literal as first argument.
    UsingFindElementByString { range: TextRange },

    /// Usage of Goto/Перейти statement.
    /// Goto is unstructured control flow that makes code less readable.
    /// Should use structured control flow instead (If, While, For, Continue, Break).
    UsingGoto { range: TextRange },

    /// Usage of modal window methods (Вопрос, Предупреждение, ОткрытьФормуМодально, etc.).
    /// Modal windows block execution and are not allowed when modality mode is disabled.
    /// Detected when global modal method is called.
    UsingModalWindows {
        /// Name of the called modal method (original case).
        method_name: String,
        /// Name of the recommended non-modal replacement.
        replacement: String,
        /// Range of the method name for the diagnostic.
        range: TextRange,
    },

    /// Usage of synchronous calls (Вопрос, КопироватьФайл, ЗапуститьПриложение, etc.).
    /// Synchronous calls are blocking and not compatible with web client.
    /// Detected when global synchronous method is called (not in server context).
    UsingSynchronousCalls {
        /// Name of the called synchronous method (original case).
        method_name: String,
        /// Name of the recommended asynchronous replacement.
        replacement: String,
        /// Range of the call expression for the diagnostic.
        range: TextRange,
    },

    /// Usage of deprecated ЭтаФорма/ThisForm property.
    /// Starting from 1C:Enterprise 8.3.3, should use ЭтотОбъект/ThisObject instead.
    /// Detected when ЭтаФорма/ThisForm is used as identifier (not as parameter).
    UsingThisForm { range: TextRange },

    /// Wrong use of ПродолжитьВызов/ProceedWithCall function.
    /// This function can only be called inside extension methods with &Вместо annotation.
    /// Calling it from methods with &До, &После or without extension annotation causes runtime error.
    WrongUseFunctionProceedWithCall { range: TextRange },

    /// Wrong use of RollbackTransaction/ОтменитьТранзакцию method.
    /// Detects two violation patterns:
    /// 1. RollbackTransaction outside exception handler
    /// 2. RollbackTransaction not as first statement in exception handler
    WrongUseOfRollbackTransactionMethod { range: TextRange },

    /// Candidate for deprecated method call check.
    ///
    /// Emitted during lowering for all method calls.
    /// The from_hir() handler resolves the callee and checks if it's deprecated.
    ///
    /// Fields:
    /// - `callee`: Name of the called method
    /// - `module`: Optional module name for qualified calls (Module.Method)
    /// - `range`: Source range of the method name being called
    DeprecatedMethodCall { callee: String, module: Option<String>, range: TextRange },

    /// Assignment to ЭтотОбъект/ThisObject property.
    /// This is a read-only property and cannot be assigned.
    /// Validated in from_hir() to check if module type is CommonModule or FormModule.
    ThisObjectAssign { range: TextRange },

    // ==========================================================================
    // Phase 4: Method-scoped diagnostics (emitted at end of method lowering)
    // ==========================================================================
    /// Method size (number of statements) exceeds threshold.
    /// Emitted at end of method lowering. Filtered by maxSize in from_hir().
    MethodSize {
        /// Method name for the diagnostic message.
        method_name: String,
        /// Calculated method size (number of statements).
        size: u32,
        /// Is this a function (vs procedure)?
        is_function: bool,
        /// Range of the method name for the diagnostic.
        range: TextRange,
    },

    /// Number of parameters exceeds threshold.
    /// Emitted at end of method lowering. Filtered by maxParamsCount in from_hir().
    NumberOfParams {
        /// Method name for the diagnostic message.
        method_name: String,
        /// Number of parameters.
        count: u32,
        /// Is this a function (vs procedure)?
        is_function: bool,
        /// Range of the method name for the diagnostic.
        range: TextRange,
    },

    /// Number of optional parameters exceeds threshold.
    /// Emitted at end of method lowering. Filtered by maxOptionalParamsCount in from_hir().
    NumberOfOptionalParams {
        /// Method name for the diagnostic message.
        method_name: String,
        /// Number of optional parameters.
        count: u32,
        /// Is this a function (vs procedure)?
        is_function: bool,
        /// Range of the method name for the diagnostic.
        range: TextRange,
    },

    /// Число()/Number() call inside try block body.
    /// Using exceptions for type casting is incorrect - use TypeDescription instead.
    TryNumber { range: TextRange },

    /// Usage of Unix-unavailable objects (COMObject, Mail) without platform guard.
    /// Detected when creating these objects outside of IF with platform type check.
    UsingObjectNotAvailableUnix { type_name: String, range: TextRange },

    /// Unsafe usage of SafeMode/БезопасныйРежим method.
    /// Detected when SafeMode() is used without explicit comparison (= True, <> False).
    UnsafeSafeModeMethodCall { range: TextRange },

    /// Unused iterator in ForEach loop.
    /// from_hir() checks symbol_tree to skip module-level variables.
    UselessForEach { iterator_name: String, range: TextRange },

    /// Potential unsafe FindByCode/НайтиПоКоду call on metadata object.
    /// from_hir() checks configuration metadata to verify code uniqueness.
    UnsafeFindByCode { manager_name: String, object_name: String, range: TextRange },

    /// WriteLogEvent / ЗаписьЖурналаРегистрации call with validation info.
    /// Validation logic is in from_hir() based on collected flags.
    UsageWriteLogEvent {
        /// Is this call inside an EXCEPT_CLAUSE?
        in_except_block: bool,
        /// Number of arguments
        arg_count: usize,
        /// Is 2nd param (log level) empty/missing?
        log_level_empty: bool,
        /// Is 5th param (comment) empty/missing?
        comment_empty: bool,
        /// Does log level contain Error value?
        has_error_log_level: bool,
        /// Does comment contain DetailErrorDescription(ErrorInfo())?
        has_detail_error_description: bool,
        /// Does except block contain Raise statement?
        except_has_raise: bool,
        /// Range for the diagnostic
        range: TextRange,
    },
}

/// Context of a magic number for filtering by configuration.
///
/// Used by MagicNumber diagnostic to determine if a number should be excluded
/// based on `excludedConstructors` and `allowMagicIndexes` parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MagicNumberContext {
    /// Number inside a constructor: `Новый ТипОбъекта(10, 2)`
    InConstructor {
        /// Type name of the constructor (lowercase for comparison)
        type_name: String,
    },
    /// Number inside Structure.Insert() or Map.Insert() call
    InStructureInsert,
    /// Number inside structure constructor: `Новый Структура("Поле", 20)`
    InStructureConstructor,
    /// Number as default parameter value: `Функция Метод(Значение = 566)`
    InDefaultParam,
    /// Number in array index access: `Массив[20]`
    InArrayIndex,
    /// Number in property assignment: `Структура.Поле = 20`
    InPropertyAssignment,
    /// Number in simple assignment (direct value without operators): `День = 6`
    InSimpleAssignment,
    /// Number in binary expression: `СекундВЧасе = 60 * 60`
    InExpression,
    /// Number in return statement: `Возврат 12`
    InReturn,
    /// Number in method call argument: `.Добавить(2)`
    InMethodCall,
    /// Number in ternary operator branch (for exclusion check)
    InTernaryBranch,
    /// Precision argument of Round/Окр: `Окр(Значение, 2)`
    InRoundPrecision,
    /// Other context (not excluded)
    Other,
}

/// Category of deprecated attribute (8.3.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeprecatedKind8312 {
    /// Deprecated property/field
    Attribute,
    /// Deprecated method
    Method,
    /// Deprecated global method
    GlobalMethod,
    /// Deprecated enum type name
    EnumName,
    /// Deprecated enum value
    EnumValue,
}

/// Kind of redundant access pattern.
///
/// Used by RedundantAccessToObject diagnostic to distinguish between different patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedundantAccessKind {
    /// ЭтотОбъект.Field or ThisObject.Field (in ObjectModule/FormModule/RecordSetModule)
    ThisObject {
        /// The prefix used (ЭтотОбъект or ThisObject, preserving original case)
        prefix: String,
    },
    /// Module.Method() where Module is the current common module
    TwoLevel {
        /// Module name used in the call
        module: String,
    },
    /// MdoType.MdoName.Method() where this is the current manager module
    ThreeLevel {
        /// MDO type keyword (Справочники, Документы, etc.)
        mdo_type: String,
        /// MDO object name
        mdo_name: String,
    },
}

/// External reference collected during body lowering.
///
/// Used for building module dependency graph without parsing all files.
/// Collected during HIR lowering when encountering qualified calls.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExternalRef {
    /// Qualified call: Module.Method()
    /// Example: ОбщегоНазначения.СообщитьПользователю()
    QualifiedCall { receiver: Name, method: Name, range: TextRange },

    /// Manager access: Документы.ИмяОбъекта.Метод()
    /// Example: Документы.ПриходнаяНакладная.СоздатьЭлемент()
    ManagerAccess {
        manager_type: ManagerType,
        object_name: Name,
        method: Option<Name>,
        range: TextRange,
    },
}

/// Type of metadata manager (global context collection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManagerType {
    /// Документы / Documents
    Documents,
    /// Справочники / Catalogs
    Catalogs,
    /// Обработки / DataProcessors
    DataProcessors,
    /// Отчёты / Reports
    Reports,
    /// РегистрыСведений / InformationRegisters
    InformationRegisters,
    /// РегистрыНакопления / AccumulationRegisters
    AccumulationRegisters,
    /// РегистрыБухгалтерии / AccountingRegisters
    AccountingRegisters,
    /// РегистрыРасчёта / CalculationRegisters
    CalculationRegisters,
    /// ПланыВидовХарактеристик / ChartsOfCharacteristicTypes
    ChartsOfCharacteristicTypes,
    /// ПланыСчетов / ChartsOfAccounts
    ChartsOfAccounts,
    /// ПланыВидовРасчёта / ChartsOfCalculationTypes
    ChartsOfCalculationTypes,
    /// БизнесПроцессы / BusinessProcesses
    BusinessProcesses,
    /// Задачи / Tasks
    Tasks,
    /// Перечисления / Enums
    Enums,
    /// ПланыОбмена / ExchangePlans
    ExchangePlans,
    /// ВнешниеИсточникиДанных / ExternalDataSources
    ExternalDataSources,
    /// Константы / Constants
    Constants,
}

impl ManagerType {
    /// Parse manager type from BSL name (Russian or English, case-insensitive).
    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        match lower.as_str() {
            "документы" | "documents" => Some(Self::Documents),
            "справочники" | "catalogs" => Some(Self::Catalogs),
            "обработки" | "dataprocessors" => Some(Self::DataProcessors),
            "отчёты" | "отчеты" | "reports" => Some(Self::Reports),
            "регистрысведений" | "informationregisters" => {
                Some(Self::InformationRegisters)
            }
            "регистрынакопления" | "accumulationregisters" => {
                Some(Self::AccumulationRegisters)
            }
            "регистрыбухгалтерии" | "accountingregisters" => {
                Some(Self::AccountingRegisters)
            }
            "регистрырасчёта" | "регистрырасчета" | "calculationregisters" => {
                Some(Self::CalculationRegisters)
            }
            "планывидовхарактеристик" | "chartsofcharacteristictypes" => {
                Some(Self::ChartsOfCharacteristicTypes)
            }
            "планысчетов" | "chartsofaccounts" => Some(Self::ChartsOfAccounts),
            "планывидоврасчёта" | "планывидоврасчета" | "chartsofcalculationtypes" => {
                Some(Self::ChartsOfCalculationTypes)
            }
            "бизнеспроцессы" | "businessprocesses" => Some(Self::BusinessProcesses),
            "задачи" | "tasks" => Some(Self::Tasks),
            "перечисления" | "enums" => Some(Self::Enums),
            "планыобмена" | "exchangeplans" => Some(Self::ExchangePlans),
            "внешниеисточникиданных" | "externaldatasources" => {
                Some(Self::ExternalDataSources)
            }
            "константы" | "constants" => Some(Self::Constants),
            _ => None,
        }
    }

    /// Convert MdoType to ManagerType.
    ///
    /// Returns None for types that don't have manager modules
    /// (Cube, DimensionTable, CommonModule).
    pub fn from_mdo_type(mdo_type: bsl_metadata::MdoType) -> Option<Self> {
        use bsl_metadata::MdoType;
        match mdo_type {
            MdoType::Document => Some(Self::Documents),
            MdoType::Catalog => Some(Self::Catalogs),
            MdoType::DataProcessor => Some(Self::DataProcessors),
            MdoType::Report => Some(Self::Reports),
            MdoType::InformationRegister => Some(Self::InformationRegisters),
            MdoType::AccumulationRegister => Some(Self::AccumulationRegisters),
            MdoType::AccountingRegister => Some(Self::AccountingRegisters),
            MdoType::CalculationRegister => Some(Self::CalculationRegisters),
            MdoType::ChartOfCharacteristicTypes => Some(Self::ChartsOfCharacteristicTypes),
            MdoType::ChartOfAccounts => Some(Self::ChartsOfAccounts),
            MdoType::ChartOfCalculationTypes => Some(Self::ChartsOfCalculationTypes),
            MdoType::BusinessProcess => Some(Self::BusinessProcesses),
            MdoType::Task => Some(Self::Tasks),
            MdoType::Enum => Some(Self::Enums),
            MdoType::ExchangePlan => Some(Self::ExchangePlans),
            MdoType::ExternalDataSource => Some(Self::ExternalDataSources),
            MdoType::Constant => Some(Self::Constants),

            // These types don't have manager modules
            MdoType::Cube | MdoType::DimensionTable | MdoType::CommonModule => None,
        }
    }
}

impl BodyDiagnostic {
    /// Get the source range of this diagnostic.
    pub fn range(&self) -> TextRange {
        match self {
            BodyDiagnostic::MissingReturn { range } => *range,
            BodyDiagnostic::EmptyCodeBlock { range } => *range,
            BodyDiagnostic::DeprecatedMethod { range, .. } => *range,
            BodyDiagnostic::DeprecatedCurrentDate { range, .. } => *range,
            BodyDiagnostic::DeprecatedFind { range, .. } => *range,
            BodyDiagnostic::DeprecatedMessage { range, .. } => *range,
            BodyDiagnostic::DeprecatedTypeManagedForm { range, .. } => *range,
            BodyDiagnostic::MagicNumber { range, .. } => *range,
            BodyDiagnostic::SelfAssign { range } => *range,
            BodyDiagnostic::FunctionShouldHaveReturn { range } => *range,
            BodyDiagnostic::BeginTransactionBeforeTryCatch { range } => *range,
            BodyDiagnostic::MissedRequiredParameter { range, .. } => *range,
            BodyDiagnostic::IfElseDuplicatedCodeBlock { range } => *range,
            BodyDiagnostic::CodeAfterAsyncCall { range, .. } => *range,
            BodyDiagnostic::CommitTransactionOutsideTryCatch { range } => *range,
            BodyDiagnostic::CommonModuleAssign { range, .. } => *range,
            BodyDiagnostic::MissingCommonModuleMethod { range, .. } => *range,
            BodyDiagnostic::RewriteMethodParameter { ident_range, .. } => *ident_range,
            BodyDiagnostic::CreateQueryInCycle { range } => *range,
            BodyDiagnostic::DeletingCollectionItem { range, .. } => *range,
            BodyDiagnostic::SelfInsertion { range } => *range,
            BodyDiagnostic::DeprecatedAttribute8312 { range, .. } => *range,
            BodyDiagnostic::ExecuteExternalCode { range } => *range,
            BodyDiagnostic::ExternalAppStarting { range } => *range,
            BodyDiagnostic::ExtraCommas { range } => *range,
            BodyDiagnostic::FileSystemAccess { range } => *range,
            BodyDiagnostic::FormDataToValue { range } => *range,
            BodyDiagnostic::FunctionNameStartsWithGet { range, .. } => *range,
            BodyDiagnostic::FunctionOutParameter { range, .. } => *range,
            BodyDiagnostic::FunctionReturnsSamePrimitive { range } => *range,
            BodyDiagnostic::GetFormMethod { range, .. } => *range,
            BodyDiagnostic::GlobalContextMethodCollision8312 { range, .. } => *range,
            BodyDiagnostic::EmptyRegion { range, .. } => *range,
            BodyDiagnostic::EmptyStatement { range } => *range,
            BodyDiagnostic::MissingSemicolon { range } => *range,
            BodyDiagnostic::IfElseDuplicatedCondition { range, .. } => *range,
            BodyDiagnostic::IfElseIfEndsWithElse { range } => *range,
            BodyDiagnostic::IncorrectUseOfStrTemplate { range } => *range,
            BodyDiagnostic::OneStatementPerLine { range } => *range,
            BodyDiagnostic::OSUsersMethod { range } => *range,
            BodyDiagnostic::ProcedureReturnsValue { range } => *range,
            BodyDiagnostic::ReservedWordAsMethodName { range, .. } => *range,
            BodyDiagnostic::RedundantAccessToObject { range, .. } => *range,
            BodyDiagnostic::StyleElementConstructors { range, .. } => *range,
            BodyDiagnostic::TempFilesDir { range, .. } => *range,
            BodyDiagnostic::TernaryOperatorUsage { range } => *range,
            BodyDiagnostic::TooManyReturns { method_name_range, .. } => *method_name_range,
            BodyDiagnostic::UnaryPlusInConcatenation { range } => *range,
            BodyDiagnostic::UseSystemInformation { range } => *range,
            BodyDiagnostic::UsingCancelParameter { range } => *range,
            BodyDiagnostic::UsingExternalCodeTools { range } => *range,
            BodyDiagnostic::UsingFindElementByString { range } => *range,
            BodyDiagnostic::UsingGoto { range } => *range,
            BodyDiagnostic::UsingModalWindows { range, .. } => *range,
            BodyDiagnostic::UsingSynchronousCalls { range, .. } => *range,
            BodyDiagnostic::UsingThisForm { range } => *range,
            BodyDiagnostic::WrongUseFunctionProceedWithCall { range } => *range,
            BodyDiagnostic::WrongUseOfRollbackTransactionMethod { range } => *range,
            BodyDiagnostic::DeprecatedMethodCall { range, .. } => *range,
            BodyDiagnostic::ThisObjectAssign { range } => *range,
            // Phase 4: Method-scoped diagnostics
            BodyDiagnostic::MethodSize { range, .. } => *range,
            BodyDiagnostic::NumberOfParams { range, .. } => *range,
            BodyDiagnostic::NumberOfOptionalParams { range, .. } => *range,
            BodyDiagnostic::TryNumber { range } => *range,
            BodyDiagnostic::UsingObjectNotAvailableUnix { range, .. } => *range,
            BodyDiagnostic::UnsafeSafeModeMethodCall { range } => *range,
            BodyDiagnostic::UselessForEach { range, .. } => *range,
            BodyDiagnostic::UnsafeFindByCode { range, .. } => *range,
            BodyDiagnostic::UsageWriteLogEvent { range, .. } => *range,
        }
    }
}

/// Lower a method AST node to HIR Body.
///
/// This is the main entry point for body lowering.
pub fn lower_method(method_node: &SyntaxNode, is_function: bool) -> LowerResult {
    lower::lower_method(method_node, is_function)
}

/// Lower a method AST node to HIR Body with line index for additional diagnostics.
///
/// When `line_index` is provided, additional diagnostics are emitted:
/// OneStatementPerLine, TooManyReturns, MethodSize, and method-scoped metrics.
pub fn lower_method_with_externals(
    method_node: &SyntaxNode,
    is_function: bool,
    line_index: Option<std::sync::Arc<line_index::LineIndex>>,
) -> LowerResult {
    lower::lower_method_with_externals(method_node, is_function, line_index)
}

/// Lower module-level code (statements outside procedures/functions).
///
/// This handles initialization code that runs when the module is loaded.
/// When `line_index` is provided, OneStatementPerLine diagnostic is emitted.
pub fn lower_module_code(
    root: &SyntaxNode,
    line_index: Option<std::sync::Arc<line_index::LineIndex>>,
) -> LowerResult {
    lower::lower_module_code(root, line_index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::{Binding, Expr, Literal, Stmt};
    use crate::Name;
    use ordered_float::NotNan;

    #[test]
    fn test_body_creation() {
        let body = Body::new();
        assert_eq!(body.expr_count(), 0);
        assert_eq!(body.stmt_count(), 0);
        assert_eq!(body.binding_count(), 0);
        assert_eq!(body.params.len(), 0);
        assert_eq!(body.body_stmts.len(), 0);
    }

    #[test]
    fn test_body_with_expressions() {
        let mut body = Body::new();

        let expr_id = body.exprs.alloc(Expr::Literal(Literal::Number(NotNan::new(42.0).unwrap())));
        assert_eq!(body.expr_count(), 1);

        let expr = body.expr(ExprId::from_idx(expr_id));
        assert!(
            matches!(expr, Expr::Literal(Literal::Number(n)) if *n == NotNan::new(42.0).unwrap())
        );
    }

    #[test]
    fn test_body_with_statements() {
        let mut body = Body::new();

        let expr_id = body.exprs.alloc(Expr::Literal(Literal::Number(NotNan::new(1.0).unwrap())));
        let stmt_id = body.stmts.alloc(Stmt::Expr(expr_id));

        assert_eq!(body.stmt_count(), 1);
        let stmt = body.stmt(StmtId::from_idx(stmt_id));
        assert!(matches!(stmt, Stmt::Expr(id) if *id == expr_id));
    }

    #[test]
    fn test_body_with_bindings() {
        let mut body = Body::new();

        let binding_id = body.bindings.alloc(Binding::var(Name::new("Переменная")));
        assert_eq!(body.binding_count(), 1);

        let binding = body.binding(BindingId::from_idx(binding_id));
        assert_eq!(binding.name.as_str(), "Переменная");
        assert!(!binding.is_val);
    }

    #[test]
    fn test_source_map() {
        let mut body = Body::new();
        let mut source_map = BodySourceMap::new();

        let expr_id = body.exprs.alloc(Expr::Literal(Literal::Number(NotNan::new(42.0).unwrap())));
        let range = TextRange::new(0.into(), 2.into());

        source_map.record_expr(expr_id, range);

        assert_eq!(source_map.expr_range(ExprId::from_idx(expr_id)), Some(range));
        assert_eq!(source_map.expr_at_range(range), Some(ExprId::from_idx(expr_id)));
    }

    #[test]
    fn test_source_map_binding_at_range() {
        let mut body = Body::new();
        let mut source_map = BodySourceMap::new();

        let binding_id = body.bindings.alloc(Binding::var(Name::new("Запись")));
        let range = TextRange::new(10.into(), 16.into());
        source_map.record_binding(binding_id, range);

        assert_eq!(source_map.binding_range(BindingId::from_idx(binding_id)), Some(range));
        assert_eq!(source_map.binding_at_range(range), Some(BindingId::from_idx(binding_id)));
        // Unknown range yields None.
        let other = TextRange::new(20.into(), 24.into());
        assert_eq!(source_map.binding_at_range(other), None);
    }

    #[test]
    fn test_body_diagnostic_range() {
        let range = TextRange::new(10.into(), 20.into());

        let diagnostics = vec![
            BodyDiagnostic::MissingReturn { range },
            BodyDiagnostic::EmptyCodeBlock { range },
            BodyDiagnostic::DeprecatedMethod { name: "Test".to_string(), range },
            BodyDiagnostic::MagicNumber {
                value: "42".to_string(),
                range,
                context: MagicNumberContext::Other,
            },
            BodyDiagnostic::SelfAssign { range },
            BodyDiagnostic::FunctionShouldHaveReturn { range },
            BodyDiagnostic::IfElseDuplicatedCodeBlock { range },
            BodyDiagnostic::CommitTransactionOutsideTryCatch { range },
            BodyDiagnostic::CommonModuleAssign {
                variable_name: "Test".to_string(),
                range,
                existing_binding_kind: None,
            },
        ];

        for diag in diagnostics {
            assert_eq!(diag.range(), range);
        }
    }
}
