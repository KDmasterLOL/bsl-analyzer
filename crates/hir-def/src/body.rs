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
use rustc_hash::FxHashMap;
use syntax::SyntaxNode;
use text_size::TextRange;

use crate::hir::{Binding, BindingId, Expr, ExprId, Stmt, StmtId};

/// HIR representation of a method body.
///
/// Contains all expressions, statements, and bindings in arena-allocated form.
/// This allows efficient storage and stable IDs for referencing HIR nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    /// All expressions in this body.
    pub exprs: Arena<Expr>,
    /// All statements in this body.
    pub stmts: Arena<Stmt>,
    /// All local bindings (variables and parameters).
    pub bindings: Arena<Binding>,
    /// Parameter binding IDs (in declaration order).
    pub params: Box<[BindingId]>,
    /// Top-level statements in the method body.
    pub body_stmts: Box<[StmtId]>,

    /// SDBL queries found in this method body.
    /// Maps ExprId (Expr::Literal with SDBL string) to parsed SDBL query info.
    pub sdbl_exprs: Vec<(ExprId, syntax::SdblQueryInfo)>,
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
        }
    }

    /// Get an expression by ID.
    pub fn expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id]
    }

    /// Get a statement by ID.
    pub fn stmt(&self, id: StmtId) -> &Stmt {
        &self.stmts[id]
    }

    /// Get a binding by ID.
    pub fn binding(&self, id: BindingId) -> &Binding {
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

    /// Iterate over all expressions.
    pub fn exprs_iter(&self) -> impl Iterator<Item = (ExprId, &Expr)> {
        self.exprs.iter()
    }

    /// Iterate over all statements.
    pub fn stmts_iter(&self) -> impl Iterator<Item = (StmtId, &Stmt)> {
        self.stmts.iter()
    }

    /// Iterate over all bindings.
    pub fn bindings_iter(&self) -> impl Iterator<Item = (BindingId, &Binding)> {
        self.bindings.iter()
    }

    /// Get all SDBL expressions in this body.
    pub fn sdbl_exprs(&self) -> &[(ExprId, syntax::SdblQueryInfo)] {
        &self.sdbl_exprs
    }
}

/// Bidirectional mapping between HIR and AST.
///
/// Used for:
/// - Diagnostics: HIR node → source location
/// - Go-to-definition: source location → HIR node
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BodySourceMap {
    /// Expression ID → source range.
    expr_ranges: FxHashMap<ExprId, TextRange>,
    /// Statement ID → source range.
    stmt_ranges: FxHashMap<StmtId, TextRange>,
    /// Binding ID → source range.
    binding_ranges: FxHashMap<BindingId, TextRange>,

    /// Source range → Expression ID (for reverse lookup).
    range_to_expr: FxHashMap<TextRange, ExprId>,
    /// Source range → Statement ID (for reverse lookup).
    range_to_stmt: FxHashMap<TextRange, StmtId>,
}

impl BodySourceMap {
    /// Create a new empty source map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record expression source range.
    pub fn record_expr(&mut self, id: ExprId, range: TextRange) {
        self.expr_ranges.insert(id, range);
        self.range_to_expr.insert(range, id);
    }

    /// Record statement source range.
    pub fn record_stmt(&mut self, id: StmtId, range: TextRange) {
        self.stmt_ranges.insert(id, range);
        self.range_to_stmt.insert(range, id);
    }

    /// Record binding source range.
    pub fn record_binding(&mut self, id: BindingId, range: TextRange) {
        self.binding_ranges.insert(id, range);
    }

    /// Get source range for an expression.
    pub fn expr_range(&self, id: ExprId) -> Option<TextRange> {
        self.expr_ranges.get(&id).copied()
    }

    /// Get source range for a statement.
    pub fn stmt_range(&self, id: StmtId) -> Option<TextRange> {
        self.stmt_ranges.get(&id).copied()
    }

    /// Get source range for a binding.
    pub fn binding_range(&self, id: BindingId) -> Option<TextRange> {
        self.binding_ranges.get(&id).copied()
    }

    /// Find expression at a given range.
    pub fn expr_at_range(&self, range: TextRange) -> Option<ExprId> {
        self.range_to_expr.get(&range).copied()
    }

    /// Find statement at a given range.
    pub fn stmt_at_range(&self, range: TextRange) -> Option<StmtId> {
        self.range_to_stmt.get(&range).copied()
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
}

/// Diagnostic collected during body lowering.
///
/// These diagnostics are emitted as a byproduct of lowering AST to HIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyDiagnostic {
    /// Missing return statement in function.
    MissingReturn { range: TextRange },

    /// Unreachable code after return/raise/break/continue.
    UnreachableCode { range: TextRange },

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

    /// Unsafe call to УстановитьБезопасныйРежим / SetSafeMode or
    /// УстановитьОтключениеБезопасногоРежима / SetSafeModeDisabled.
    /// Detected when safe mode methods are called with unsafe arguments.
    DisableSafeMode { method_name: String, range: TextRange },

    /// Magic number literal (hardcoded number that should be a constant).
    /// Value is stored as string to allow Eq derivation.
    MagicNumber { value: String, range: TextRange },

    /// Self-assignment (a = a).
    SelfAssign { range: TextRange },

    /// Unused local variable.
    UnusedVariable { name: String, range: TextRange },

    /// Function should have return.
    FunctionShouldHaveReturn { range: TextRange },

    // NOTE: MissingCommonModuleMethod removed (Phase 4)
    // Now collected via AST-based check() with path resolution instead of during lowering.
    // This provides more accurate diagnostics using workspace symbols.
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
    CommonModuleAssign { variable_name: String, range: TextRange },

    /// Missing or non-export method call in CommonModule.
    ///
    /// Collected during HIR lowering when qualified path resolution fails
    /// or method is not exported.
    ///
    /// Examples:
    /// - `CommonModule.NonExistentMethod()` → MethodNotFound
    /// - `CommonModule.PrivateMethod()` → NonExportMethod
    ///
    /// This diagnostic is created by the resolver during path resolution and
    /// represents all cases where a CommonModule method call cannot succeed.
    MissingCommonModuleMethod {
        module: String,
        method: String,
        reason: CommonModuleMethodError,
        range: TextRange,
    },

    /// Overwrite of byValue parameter without prior use.
    /// Emitted during lowering when a parameter marked with Знач/ByValue is assigned to.
    /// Validation using reaching definitions happens in from_hir() to check if parameter
    /// was used before the assignment.
    RewriteMethodParameter {
        param_id: BindingId, // Parameter being overwritten
        stmt_id: StmtId,     // Assignment statement for CFG analysis
        range: TextRange,    // Range of the assignment for diagnostic
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

    /// Overly complex if condition with too many boolean operations.
    /// Detected when if/elsif condition has more boolean operations (AND/OR) than maxComplexity.
    /// Complexity = number of boolean operations + 1 (default max: 3).
    IfConditionComplexity { complexity: usize, max_complexity: usize, range: TextRange },

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
}

/// Error types for MissingCommonModuleMethod diagnostic.
///
/// Represents different reasons why a CommonModule method call cannot succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommonModuleMethodError {
    /// Method does not exist in the referenced CommonModule.
    ///
    /// Example: `CommonModule.NonExistentMethod()`
    MethodNotFound,

    /// Method exists but lacks `Экспорт` (Export) keyword.
    ///
    /// Example: `CommonModule.PrivateMethod()` where PrivateMethod exists but is not exported
    NonExportMethod,

    /// CommonModule not found in metadata.
    ///
    /// This can happen if:
    /// - Module name is misspelled
    /// - Metadata is not loaded
    /// - Module exists but file is not in VFS
    ModuleNotFound,
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

impl BodyDiagnostic {
    /// Get the source range of this diagnostic.
    pub fn range(&self) -> TextRange {
        match self {
            BodyDiagnostic::MissingReturn { range } => *range,
            BodyDiagnostic::UnreachableCode { range } => *range,
            BodyDiagnostic::EmptyCodeBlock { range } => *range,
            BodyDiagnostic::DeprecatedMethod { range, .. } => *range,
            BodyDiagnostic::DeprecatedCurrentDate { range, .. } => *range,
            BodyDiagnostic::DeprecatedFind { range, .. } => *range,
            BodyDiagnostic::DeprecatedMessage { range, .. } => *range,
            BodyDiagnostic::DeprecatedTypeManagedForm { range, .. } => *range,
            BodyDiagnostic::DisableSafeMode { range, .. } => *range,
            BodyDiagnostic::MagicNumber { range, .. } => *range,
            BodyDiagnostic::SelfAssign { range } => *range,
            BodyDiagnostic::UnusedVariable { range, .. } => *range,
            BodyDiagnostic::FunctionShouldHaveReturn { range } => *range,
            BodyDiagnostic::BeginTransactionBeforeTryCatch { range } => *range,
            BodyDiagnostic::MissedRequiredParameter { range, .. } => *range,
            BodyDiagnostic::IfElseDuplicatedCodeBlock { range } => *range,
            BodyDiagnostic::CodeAfterAsyncCall { range, .. } => *range,
            BodyDiagnostic::CommitTransactionOutsideTryCatch { range } => *range,
            BodyDiagnostic::CommonModuleAssign { range, .. } => *range,
            BodyDiagnostic::MissingCommonModuleMethod { range, .. } => *range,
            BodyDiagnostic::RewriteMethodParameter { range, .. } => *range,
            BodyDiagnostic::CreateQueryInCycle { range } => *range,
            BodyDiagnostic::DeletingCollectionItem { range, .. } => *range,
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
            BodyDiagnostic::IfConditionComplexity { range, .. } => *range,
            BodyDiagnostic::IfElseDuplicatedCondition { range, .. } => *range,
            BodyDiagnostic::IfElseIfEndsWithElse { range } => *range,
            BodyDiagnostic::IncorrectUseOfStrTemplate { range } => *range,
        }
    }
}

/// Lower a method AST node to HIR Body.
///
/// This is the main entry point for body lowering.
pub fn lower_method(method_node: &SyntaxNode, is_function: bool) -> LowerResult {
    lower::lower_method(method_node, is_function)
}

/// Lower a method AST node to HIR Body with known external variable names.
///
/// External variables (module-level) are passed so they're not registered
/// as implicit local variables.
pub fn lower_method_with_externals(
    method_node: &SyntaxNode,
    is_function: bool,
    known_externals: rustc_hash::FxHashSet<String>,
) -> LowerResult {
    lower::lower_method_with_externals(method_node, is_function, known_externals)
}

/// Lower module-level code (statements outside procedures/functions).
///
/// This handles initialization code that runs when the module is loaded.
pub fn lower_module_code(root: &SyntaxNode) -> LowerResult {
    lower::lower_module_code(root)
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

        let expr = body.expr(expr_id);
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
        let stmt = body.stmt(stmt_id);
        assert!(matches!(stmt, Stmt::Expr(id) if *id == expr_id));
    }

    #[test]
    fn test_body_with_bindings() {
        let mut body = Body::new();

        let binding_id = body.bindings.alloc(Binding::var(Name::new("Переменная")));
        assert_eq!(body.binding_count(), 1);

        let binding = body.binding(binding_id);
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

        assert_eq!(source_map.expr_range(expr_id), Some(range));
        assert_eq!(source_map.expr_at_range(range), Some(expr_id));
    }

    #[test]
    fn test_body_diagnostic_range() {
        let range = TextRange::new(10.into(), 20.into());

        let diagnostics = vec![
            BodyDiagnostic::MissingReturn { range },
            BodyDiagnostic::UnreachableCode { range },
            BodyDiagnostic::EmptyCodeBlock { range },
            BodyDiagnostic::DeprecatedMethod { name: "Test".to_string(), range },
            BodyDiagnostic::MagicNumber { value: "42".to_string(), range },
            BodyDiagnostic::SelfAssign { range },
            BodyDiagnostic::UnusedVariable { name: "x".to_string(), range },
            BodyDiagnostic::FunctionShouldHaveReturn { range },
            BodyDiagnostic::IfElseDuplicatedCodeBlock { range },
            BodyDiagnostic::CommitTransactionOutsideTryCatch { range },
            BodyDiagnostic::CommonModuleAssign { variable_name: "Test".to_string(), range },
        ];

        for diag in diagnostics {
            assert_eq!(diag.range(), range);
        }
    }
}
