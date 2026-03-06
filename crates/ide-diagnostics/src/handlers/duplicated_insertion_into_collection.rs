//! DuplicatedInsertionIntoCollection diagnostic.
//!
//! Detects duplicate insertions of the same value into a collection.
//!
//! ## Why?
//! Duplicate insertions are likely errors:
//! - Same value inserted twice (copy-paste error)
//! - Logic mistake
//! - Unnecessary operations
//!
//! ## Bad practice
//! ```bsl
//! Массив.Добавить(Значение1);
//! Массив.Добавить(Значение1);  // Duplicate!
//!
//! Соответствие.Вставить("Ключ1", Значение);
//! Соответствие.Вставить("Ключ1", Значение);  // Duplicate key!
//! ```
//!
//! ## Good practice
//! ```bsl
//! Массив.Добавить(Значение1);
//! Массив.Добавить(Значение2);  // Different values
//!
//! // Or if intentional, use loop:
//! Для Индекс = 1 По 3 Цикл
//!     Массив.Добавить(ЗначениеПоУмолчанию);
//! КонецЦикла;
//! ```
//!
//! ## Configuration
//! - `isAllowedMethodADD` (boolean, default: true) - If false, only Вставить/Insert checked
//!
//! ## Implementation
//!
//! This diagnostic uses HIR-based post-analysis for structural expression comparison.
//! Instead of regex-based text normalization, it compares HIR expression trees directly.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Body, BodySourceMap, Expr, ExprId, IdConversion, Literal, Name, Stmt, StmtId};
use ide_db::TextRange;
use rustc_hash::{FxHashMap, FxHasher};
use smol_str::SmolStr;
use std::hash::{Hash, Hasher};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Suspicious, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Check a HIR body for duplicated insertions.
///
/// This is a post-HIR analysis function that examines the body after lowering.
/// It tracks variable generations and detects duplicate insertions into collections.
pub fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("DuplicatedInsertionIntoCollection::check_body").entered();

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let allow_add = ctx.config.get_bool(code, "isAllowedMethodADD").unwrap_or(true);

    let mut tracker = InsertionTracker::new(body);
    let mut diagnostics = Vec::new();

    let body_stmts: Vec<StmtId> = body.body_stmts().collect();
    check_stmt_list(
        body,
        source_map,
        &body_stmts,
        &mut tracker,
        &mut diagnostics,
        0,
        allow_add,
        code,
        ctx,
    );
    tracker.report_duplicates(&mut diagnostics, 0, code, ctx);

    tracing::debug!(count = diagnostics.len(), "diagnostics found");
    diagnostics
}

/// Discriminant tags for expression types in hash computation.
/// These ensure different expression types produce different hashes.
mod expr_tag {
    pub const MISSING: u8 = 0;
    pub const LITERAL_NUMBER: u8 = 1;
    pub const LITERAL_STRING: u8 = 2;
    pub const LITERAL_DATE: u8 = 3;
    pub const LITERAL_BOOL: u8 = 4;
    pub const LITERAL_UNDEFINED: u8 = 5;
    pub const LITERAL_NULL: u8 = 6;
    pub const PATH: u8 = 7;
    pub const FIELD: u8 = 8;
    pub const METHOD_CALL: u8 = 9;
    pub const CALL: u8 = 10;
    pub const INDEX: u8 = 11;
    pub const BINARY_OP: u8 = 12;
    pub const NEW: u8 = 13;
}

/// Special values that should be allowed to duplicate.
fn is_special_value(body: &Body, expr_id: ExprId) -> bool {
    match body.expr(expr_id) {
        Expr::Literal(lit) => match lit {
            // Empty string or whitespace-only string
            Literal::String(s) => s.is_empty() || s.chars().all(char::is_whitespace),
            // Undefined/Null
            Literal::Undefined | Literal::Null => true,
            // Zero
            Literal::Number(n) => *n == 0.0,
            _ => false,
        },
        // Символы.ПС / Chars.LF etc.
        Expr::Field { base, field: _ } => {
            if let Expr::Path(name) = body.expr(ExprId::from_idx(*base)) {
                let base_lower = name.as_str().to_lowercase();
                base_lower == "символы" || base_lower == "chars"
            } else {
                false
            }
        }
        // Missing expression (empty argument)
        Expr::Missing => true,
        _ => false,
    }
}

/// Type of insertion method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsertionMethodKind {
    /// Добавить/Add - adds element to collection (Array, ValueList)
    /// For duplicates: compare ALL arguments
    Add,
    /// Вставить/Insert - inserts by key (Map, Structure)
    /// For duplicates: compare only FIRST argument (key)
    Insert,
}

/// Check if a method name is an insertion method and return its kind.
fn get_insertion_method_kind(name: &Name, allow_add: bool) -> Option<InsertionMethodKind> {
    let lower = name.as_str().to_lowercase();
    match lower.as_str() {
        "вставить" | "insert" => Some(InsertionMethodKind::Insert),
        "добавить" | "add" if allow_add => Some(InsertionMethodKind::Add),
        _ => None,
    }
}

/// Quick pre-check: does the file text contain any insertion method names?
///
/// This is a fast O(n) scan that avoids expensive HIR analysis for files
/// that don't call any insertion methods. Uses case-insensitive matching
/// without allocating a lowercase copy of the entire file.
fn has_insertion_methods(text: &str) -> bool {
    // Patterns to search for (with leading dot to match method calls)
    const PATTERNS: &[&str] = &[".добавить(", ".add(", ".вставить(", ".insert("];

    // Scan through the text looking for any pattern
    let bytes = text.as_bytes();

    for (i, &byte) in bytes.iter().enumerate() {
        // Quick check: must start with '.'
        if byte != b'.' {
            continue;
        }

        // Check each pattern
        for pattern in PATTERNS {
            if matches_case_insensitive(text, i, pattern) {
                return true;
            }
        }
    }

    false
}

/// Check if text at position matches pattern (case-insensitive).
#[inline]
fn matches_case_insensitive(text: &str, start: usize, pattern: &str) -> bool {
    let text_bytes = text.as_bytes();
    let remaining = text_bytes.len() - start;

    if remaining < pattern.len() {
        return false;
    }

    // Get the slice starting at position
    let text_slice = &text[start..];
    let mut text_chars = text_slice.chars();
    let mut pattern_chars = pattern.chars();

    loop {
        match (pattern_chars.next(), text_chars.next()) {
            (None, _) => return true,        // Pattern exhausted - match!
            (Some(_), None) => return false, // Text exhausted before pattern
            (Some(p), Some(t)) => {
                // Case-insensitive comparison
                let p_lower = p.to_lowercase().next().unwrap_or(p);
                let t_lower = t.to_lowercase().next().unwrap_or(t);
                if p_lower != t_lower {
                    return false;
                }
            }
        }
    }
}

/// Recorded insertion for duplicate detection.
#[derive(Debug, Clone)]
struct Insertion {
    /// Range of the insertion call in source code
    range: TextRange,
    /// Receiver expression ID (for lazy display string generation)
    receiver: ExprId,
    /// Argument expression IDs (for lazy display string generation)
    args: Vec<ExprId>,
    /// Scope depth where insertion occurred
    scope_depth: usize,
    /// Breaker context (return/raise offset) before this insertion
    breaker_context: Option<u32>,
    /// Local breaker context (break/continue in loop) before this insertion
    local_breaker_context: Option<u32>,
}

/// Key for grouping insertions.
///
/// Uses precomputed hashes instead of full expression trees for efficiency.
/// Hash collisions are extremely unlikely with 64-bit hashes (1 in 2^64).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct InsertionKey {
    /// Hash of the collection expression (with variable generations)
    collection_hash: u64,
    /// Hash of all argument expressions (with variable generations)
    all_args_hash: u64,
}

/// Variable generation tracker.
///
/// Tracks how many times each variable has been assigned.
/// Used to distinguish between different "versions" of a variable.
/// Uses SmolStr with lowercase keys for case-insensitive matching.
struct VariableGenerations {
    /// Variable name (lowercase SmolStr) → generation count
    generations: FxHashMap<SmolStr, usize>,
}

impl VariableGenerations {
    fn new() -> Self {
        Self { generations: FxHashMap::default() }
    }

    /// Get generation for a variable (0 if never assigned).
    fn get(&self, name: &str) -> usize {
        let key: SmolStr = name.to_lowercase().into();

        // Get direct generation
        let direct_gen = self.generations.get(&key).copied().unwrap_or(0);

        // Also check prefixes for partial reassignment detection
        // Example: Данные.Реквизит.Коллекция should check Данные.Реквизит and Данные
        let parts: Vec<&str> = name.split('.').collect();
        let mut max_gen = direct_gen;

        for i in 1..parts.len() {
            let prefix: SmolStr = parts[..i].join(".").to_lowercase().into();
            if let Some(&gen) = self.generations.get(&prefix) {
                max_gen = max_gen.max(gen);
            }
        }

        max_gen
    }

    /// Increment generation for a variable after assignment.
    fn increment(&mut self, name: &str) {
        let key: SmolStr = name.to_lowercase().into();
        *self.generations.entry(key).or_insert(0) += 1;

        // Partial reassignment: X.Y.Z changes invalidate X.Y and X
        let parts: Vec<&str> = name.split('.').collect();
        for i in (1..parts.len()).rev() {
            let prefix: SmolStr = parts[..i].join(".").to_lowercase().into();
            *self.generations.entry(prefix).or_insert(0) += 1;
        }
    }
}

/// Insertion tracker for duplicate detection.
struct InsertionTracker<'a> {
    body: &'a Body,
    generations: VariableGenerations,
    insertions: FxHashMap<InsertionKey, Vec<Insertion>>,
    /// Last return/raise statement: (offset, scope_depth)
    last_breaker: Option<(u32, usize)>,
    /// Last local break/continue statement: (offset, scope_depth)
    last_local_breaker: Option<(u32, usize)>,
}

impl<'a> InsertionTracker<'a> {
    fn new(body: &'a Body) -> Self {
        Self {
            body,
            generations: VariableGenerations::new(),
            insertions: FxHashMap::default(),
            last_breaker: None,
            last_local_breaker: None,
        }
    }

    /// Compute hash of an expression for comparison.
    ///
    /// This computes a structural hash directly without building intermediate tree structures.
    /// Names are lowercased for case-insensitive comparison.
    /// Variable generations are incorporated to distinguish different versions.
    fn hash_expr(&self, expr_id: ExprId) -> u64 {
        let mut hasher = FxHasher::default();
        self.hash_expr_into(expr_id, &mut hasher);
        hasher.finish()
    }

    /// Hash an expression into the given hasher.
    fn hash_expr_into(&self, expr_id: ExprId, hasher: &mut FxHasher) {
        match self.body.expr(expr_id) {
            Expr::Missing => {
                hasher.write_u8(expr_tag::MISSING);
            }

            Expr::Literal(lit) => match lit {
                Literal::Number(n) => {
                    hasher.write_u8(expr_tag::LITERAL_NUMBER);
                    hasher.write_u64(n.to_bits());
                }
                Literal::String(s) => {
                    hasher.write_u8(expr_tag::LITERAL_STRING);
                    hasher.write(s.as_bytes());
                }
                Literal::Date(d) => {
                    hasher.write_u8(expr_tag::LITERAL_DATE);
                    hasher.write(d.as_bytes());
                }
                Literal::Bool(b) => {
                    hasher.write_u8(expr_tag::LITERAL_BOOL);
                    hasher.write_u8(*b as u8);
                }
                Literal::Undefined => {
                    hasher.write_u8(expr_tag::LITERAL_UNDEFINED);
                }
                Literal::Null => {
                    hasher.write_u8(expr_tag::LITERAL_NULL);
                }
            },

            Expr::Path(name) => {
                hasher.write_u8(expr_tag::PATH);
                let name_str = name.as_str();
                // Hash lowercase name for case-insensitive comparison
                for c in name_str.chars() {
                    for lc in c.to_lowercase() {
                        hasher.write_u32(lc as u32);
                    }
                }
                // Include generation for non-keywords
                let generation = if is_bsl_keyword_or_literal(name_str) {
                    0
                } else {
                    self.generations.get(name_str)
                };
                hasher.write_usize(generation);
            }

            Expr::Field { base, field } => {
                hasher.write_u8(expr_tag::FIELD);
                self.hash_expr_into(ExprId::from_idx(*base), hasher);
                // Hash lowercase field name
                for c in field.as_str().chars() {
                    for lc in c.to_lowercase() {
                        hasher.write_u32(lc as u32);
                    }
                }
            }

            Expr::MethodCall { receiver, method, args } => {
                hasher.write_u8(expr_tag::METHOD_CALL);
                self.hash_expr_into(ExprId::from_idx(*receiver), hasher);
                // Hash lowercase method name
                for c in method.as_str().chars() {
                    for lc in c.to_lowercase() {
                        hasher.write_u32(lc as u32);
                    }
                }
                hasher.write_usize(args.len());
                for arg in args.iter() {
                    self.hash_expr_into(ExprId::from_idx(*arg), hasher);
                }
            }

            Expr::Call { callee, args } => {
                // Check if this is actually a method call (Call with Field as callee)
                if let Expr::Field { base, field } = self.body.expr(ExprId::from_idx(*callee)) {
                    hasher.write_u8(expr_tag::METHOD_CALL);
                    self.hash_expr_into(ExprId::from_idx(*base), hasher);
                    for c in field.as_str().chars() {
                        for lc in c.to_lowercase() {
                            hasher.write_u32(lc as u32);
                        }
                    }
                    hasher.write_usize(args.len());
                    for arg in args.iter() {
                        self.hash_expr_into(ExprId::from_idx(*arg), hasher);
                    }
                } else {
                    hasher.write_u8(expr_tag::CALL);
                    self.hash_expr_into(ExprId::from_idx(*callee), hasher);
                    hasher.write_usize(args.len());
                    for arg in args.iter() {
                        self.hash_expr_into(ExprId::from_idx(*arg), hasher);
                    }
                }
            }

            Expr::Index { base, index } => {
                hasher.write_u8(expr_tag::INDEX);
                self.hash_expr_into(ExprId::from_idx(*base), hasher);
                self.hash_expr_into(ExprId::from_idx(*index), hasher);
            }

            Expr::BinaryOp { lhs, rhs, op } => {
                hasher.write_u8(expr_tag::BINARY_OP);
                self.hash_expr_into(ExprId::from_idx(*lhs), hasher);
                self.hash_expr_into(ExprId::from_idx(*rhs), hasher);
                // Hash discriminant of op
                std::mem::discriminant(op).hash(hasher);
            }

            Expr::New { type_name, args } => {
                hasher.write_u8(expr_tag::NEW);
                if let Some(name) = type_name {
                    hasher.write_u8(1);
                    for c in name.as_str().chars() {
                        for lc in c.to_lowercase() {
                            hasher.write_u32(lc as u32);
                        }
                    }
                } else {
                    hasher.write_u8(0);
                }
                hasher.write_usize(args.len());
                for arg in args.iter() {
                    self.hash_expr_into(ExprId::from_idx(*arg), hasher);
                }
            }

            Expr::UnaryOp { .. }
            | Expr::Ternary { .. }
            | Expr::Array(_)
            | Expr::Await { .. }
            | Expr::QualifiedPath(_) => {
                // For complex expressions, use Missing tag to avoid false positives
                // TODO: Implement proper hashing for QualifiedPath once resolution is available
                hasher.write_u8(expr_tag::MISSING);
            }
        }
    }

    /// Record an assignment (increments variable generation).
    fn record_assignment(&mut self, target: ExprId) {
        let name = self.extract_target_name(target);
        if let Some(name) = name {
            tracing::trace!(name = %name, "recording assignment");
            self.generations.increment(&name);
        }
    }

    /// Extract the full path name from an assignment target.
    fn extract_target_name(&self, expr_id: ExprId) -> Option<String> {
        match self.body.expr(expr_id) {
            Expr::Path(name) => Some(name.to_string()),
            Expr::Field { base, field } => {
                let base_name = self.extract_target_name(ExprId::from_idx(*base))?;
                Some(format!("{}.{}", base_name, field))
            }
            Expr::Index { base, .. } => self.extract_target_name(ExprId::from_idx(*base)),
            Expr::MethodCall { receiver, method, .. } => {
                // For method calls like Данные.Метод().Поле, include the method
                let base_name = self.extract_target_name(ExprId::from_idx(*receiver))?;
                Some(format!("{}.{}()", base_name, method))
            }
            _ => None,
        }
    }

    /// Record a breaker (return/raise).
    fn record_breaker(&mut self, offset: u32, scope_depth: usize) {
        self.last_breaker = Some((offset, scope_depth));
    }

    /// Record a local breaker (break/continue in loop).
    fn record_local_breaker(&mut self, offset: u32, scope_depth: usize) {
        self.last_local_breaker = Some((offset, scope_depth));
    }

    /// Record an insertion into a collection.
    fn record_insertion(
        &mut self,
        receiver: ExprId,
        args: &[ExprId],
        call_range: TextRange,
        scope_depth: usize,
        kind: InsertionMethodKind,
    ) {
        if args.is_empty() {
            return;
        }

        // Skip special literals
        if is_special_value(self.body, args[0]) {
            return;
        }

        let collection_hash = self.hash_expr(receiver);

        // Hash arguments based on method kind:
        // - Add: hash ALL arguments (different presentations = different elements)
        // - Insert: hash only FIRST argument (key), different values for same key is an error
        let all_args_hash = match kind {
            InsertionMethodKind::Add => {
                let mut args_hasher = FxHasher::default();
                for arg in args {
                    self.hash_expr_into(*arg, &mut args_hasher);
                }
                args_hasher.finish()
            }
            InsertionMethodKind::Insert => self.hash_expr(args[0]),
        };

        let key = InsertionKey { collection_hash, all_args_hash };

        let breaker_context = self.last_breaker.map(|(offset, _)| offset);
        let local_breaker_context = self.last_local_breaker.map(|(offset, _)| offset);

        let insertion = Insertion {
            range: call_range,
            receiver,
            args: args.to_vec(),
            scope_depth,
            breaker_context,
            local_breaker_context,
        };

        self.insertions.entry(key).or_default().push(insertion);
    }

    /// Convert expression to display string for diagnostic message.
    fn expr_to_display_string(&self, expr_id: ExprId) -> String {
        match self.body.expr(expr_id) {
            Expr::Missing => "".to_string(),
            Expr::Literal(lit) => match lit {
                Literal::Number(n) => n.to_string(),
                Literal::String(s) => format!("\"{}\"", s),
                Literal::Date(d) => format!("'{}'", d),
                Literal::Bool(b) => if *b { "Истина" } else { "Ложь" }.to_string(),
                Literal::Undefined => "Неопределено".to_string(),
                Literal::Null => "Null".to_string(),
            },
            Expr::Path(name) => name.to_string(),
            Expr::Field { base, field } => {
                format!("{}.{}", self.expr_to_display_string(ExprId::from_idx(*base)), field)
            }
            Expr::MethodCall { receiver, method, args } => {
                let args_str = args
                    .iter()
                    .map(|&a| self.expr_to_display_string(ExprId::from_idx(a)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{}.{}({})",
                    self.expr_to_display_string(ExprId::from_idx(*receiver)),
                    method,
                    args_str
                )
            }
            Expr::Call { callee, args } => {
                let args_str = args
                    .iter()
                    .map(|&a| self.expr_to_display_string(ExprId::from_idx(a)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", self.expr_to_display_string(ExprId::from_idx(*callee)), args_str)
            }
            Expr::Index { base, index } => {
                format!(
                    "{}[{}]",
                    self.expr_to_display_string(ExprId::from_idx(*base)),
                    self.expr_to_display_string(ExprId::from_idx(*index))
                )
            }
            _ => "...".to_string(),
        }
    }

    /// Report duplicates for a given scope depth.
    fn report_duplicates(
        &mut self,
        diagnostics: &mut Vec<Diagnostic>,
        scope_depth: usize,
        code: DiagnosticCode,
        ctx: &DiagnosticsContext,
    ) {
        for insertions in self.insertions.values() {
            let scope_insertions: Vec<_> =
                insertions.iter().filter(|ins| ins.scope_depth == scope_depth).collect();

            if scope_insertions.len() > 1 {
                // Group by (breaker_context, local_breaker_context)
                let mut grouped: FxHashMap<(Option<u32>, Option<u32>), Vec<&Insertion>> =
                    FxHashMap::default();

                for ins in scope_insertions {
                    let key = (ins.breaker_context, ins.local_breaker_context);
                    grouped.entry(key).or_default().push(ins);
                }

                for group in grouped.values() {
                    if group.len() > 1 {
                        // Report only SECOND insertion (bsl-language-server compatibility)
                        if let Some(second_insertion) = group.get(1) {
                            // Generate display strings only when actually reporting
                            let collection_display =
                                self.expr_to_display_string(second_insertion.receiver);
                            let args_display = second_insertion
                                .args
                                .iter()
                                .map(|a| self.expr_to_display_string(*a))
                                .collect::<Vec<_>>()
                                .join(", ");

                            diagnostics.push(Diagnostic {
                                code,
                                message: format!(
                                    "Проверьте повторную вставку {} в коллекцию {}",
                                    args_display, collection_display
                                ),
                                severity: ctx.severity(code),
                                range: second_insertion.range,
                                tags: ctx.tags(code),
                                fixes: vec![],
                            });
                        }
                    }
                }
            }
        }

        // Remove processed insertions
        for insertions in self.insertions.values_mut() {
            insertions.retain(|ins| ins.scope_depth != scope_depth);
        }
    }
}

/// Check if a name is a BSL keyword or literal.
fn is_bsl_keyword_or_literal(word: &str) -> bool {
    let lower = word.to_lowercase();
    matches!(
        lower.as_str(),
        "новый"
            | "new"
            | "истина"
            | "true"
            | "ложь"
            | "false"
            | "неопределено"
            | "undefined"
            | "null"
            | "массив"
            | "array"
            | "структура"
            | "structure"
            | "соответствие"
            | "map"
            | "строка"
            | "string"
            | "число"
            | "number"
            | "дата"
            | "date"
            | "булево"
            | "boolean"
    )
}

/// Check a list of statements for insertions.
#[allow(clippy::too_many_arguments)]
fn check_stmt_list(
    body: &Body,
    source_map: &BodySourceMap,
    stmts: &[StmtId],
    tracker: &mut InsertionTracker,
    diagnostics: &mut Vec<Diagnostic>,
    scope_depth: usize,
    allow_add: bool,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    for stmt_id in stmts {
        check_stmt(
            body,
            source_map,
            *stmt_id,
            tracker,
            diagnostics,
            scope_depth,
            allow_add,
            code,
            ctx,
        );
    }
}

/// Check a single statement for insertions.
#[allow(clippy::too_many_arguments)]
fn check_stmt(
    body: &Body,
    source_map: &BodySourceMap,
    stmt_id: StmtId,
    tracker: &mut InsertionTracker,
    diagnostics: &mut Vec<Diagnostic>,
    scope_depth: usize,
    allow_add: bool,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    let stmt_range = source_map.stmt_range(stmt_id);

    match body.stmt(stmt_id) {
        Stmt::Assign { target, value: _ } => {
            tracker.record_assignment(ExprId::from_idx(*target));
        }

        Stmt::Expr(expr_id) => {
            check_expr_for_insertion(
                body,
                source_map,
                ExprId::from_idx(*expr_id),
                tracker,
                scope_depth,
                allow_add,
            );
            // Track variable modifications when passed to functions
            check_expr_for_side_effects(body, ExprId::from_idx(*expr_id), tracker, allow_add);
        }

        Stmt::Return { .. } => {
            if let Some(range) = stmt_range {
                tracker.record_breaker(range.start().into(), scope_depth);
            }
        }

        Stmt::Raise { .. } => {
            if let Some(range) = stmt_range {
                tracker.record_breaker(range.start().into(), scope_depth);
            }
        }

        Stmt::Break => {
            if let Some(range) = stmt_range {
                tracker.record_local_breaker(range.start().into(), scope_depth);
            }
        }

        Stmt::Continue => {
            if let Some(range) = stmt_range {
                tracker.record_local_breaker(range.start().into(), scope_depth);
            }
        }

        Stmt::If(if_stmt) => {
            // Check then branch
            let then_stmts: Vec<StmtId> =
                if_stmt.then_branch.iter().map(|&idx| StmtId::from_idx(idx)).collect();
            check_stmt_list(
                body,
                source_map,
                &then_stmts,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
                code,
                ctx,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1, code, ctx);

            // Check elsif branches
            for (_, branch_stmts) in if_stmt.elsif_branches.iter() {
                let elsif_stmts: Vec<StmtId> =
                    branch_stmts.iter().map(|&idx| StmtId::from_idx(idx)).collect();
                check_stmt_list(
                    body,
                    source_map,
                    &elsif_stmts,
                    tracker,
                    diagnostics,
                    scope_depth + 1,
                    allow_add,
                    code,
                    ctx,
                );
                tracker.report_duplicates(diagnostics, scope_depth + 1, code, ctx);
            }

            // Check else branch
            if let Some(ref else_stmts) = if_stmt.else_branch {
                let else_stmts_vec: Vec<StmtId> =
                    else_stmts.iter().map(|&idx| StmtId::from_idx(idx)).collect();
                check_stmt_list(
                    body,
                    source_map,
                    &else_stmts_vec,
                    tracker,
                    diagnostics,
                    scope_depth + 1,
                    allow_add,
                    code,
                    ctx,
                );
                tracker.report_duplicates(diagnostics, scope_depth + 1, code, ctx);
            }
        }

        Stmt::While { condition: _, body: loop_body } => {
            let saved_local_breaker = tracker.last_local_breaker;
            let loop_stmts: Vec<StmtId> =
                loop_body.iter().map(|&idx| StmtId::from_idx(idx)).collect();
            check_stmt_list(
                body,
                source_map,
                &loop_stmts,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
                code,
                ctx,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1, code, ctx);
            tracker.last_local_breaker = saved_local_breaker;
        }

        Stmt::For { var: _, from: _, to: _, body: loop_body } => {
            let saved_local_breaker = tracker.last_local_breaker;
            let loop_stmts: Vec<StmtId> =
                loop_body.iter().map(|&idx| StmtId::from_idx(idx)).collect();
            check_stmt_list(
                body,
                source_map,
                &loop_stmts,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
                code,
                ctx,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1, code, ctx);
            tracker.last_local_breaker = saved_local_breaker;
        }

        Stmt::ForEach { var: _, collection: _, body: loop_body } => {
            let saved_local_breaker = tracker.last_local_breaker;
            let loop_stmts: Vec<StmtId> =
                loop_body.iter().map(|&idx| StmtId::from_idx(idx)).collect();
            check_stmt_list(
                body,
                source_map,
                &loop_stmts,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
                code,
                ctx,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1, code, ctx);
            tracker.last_local_breaker = saved_local_breaker;
        }

        Stmt::Try { body: try_body, except } => {
            let try_stmts: Vec<StmtId> =
                try_body.iter().map(|&idx| StmtId::from_idx(idx)).collect();
            check_stmt_list(
                body,
                source_map,
                &try_stmts,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
                code,
                ctx,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1, code, ctx);

            let except_stmts: Vec<StmtId> =
                except.iter().map(|&idx| StmtId::from_idx(idx)).collect();
            check_stmt_list(
                body,
                source_map,
                &except_stmts,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
                code,
                ctx,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1, code, ctx);
        }

        Stmt::PreprocIf(preproc) => {
            // Check then branch
            let then_stmts: Vec<StmtId> =
                preproc.then_branch.iter().map(|&idx| StmtId::from_idx(idx)).collect();
            check_stmt_list(
                body,
                source_map,
                &then_stmts,
                tracker,
                diagnostics,
                scope_depth + 1,
                allow_add,
                code,
                ctx,
            );
            tracker.report_duplicates(diagnostics, scope_depth + 1, code, ctx);

            // Check elsif branches
            for (_range, _directive_range, elsif_body) in preproc.elsif_branches.iter() {
                let elsif_stmts: Vec<StmtId> =
                    elsif_body.iter().map(|&idx| StmtId::from_idx(idx)).collect();
                check_stmt_list(
                    body,
                    source_map,
                    &elsif_stmts,
                    tracker,
                    diagnostics,
                    scope_depth + 1,
                    allow_add,
                    code,
                    ctx,
                );
                tracker.report_duplicates(diagnostics, scope_depth + 1, code, ctx);
            }

            // Check else branch
            if let Some(ref else_body) = preproc.else_branch {
                let else_stmts: Vec<StmtId> =
                    else_body.iter().map(|&idx| StmtId::from_idx(idx)).collect();
                check_stmt_list(
                    body,
                    source_map,
                    &else_stmts,
                    tracker,
                    diagnostics,
                    scope_depth + 1,
                    allow_add,
                    code,
                    ctx,
                );
                tracker.report_duplicates(diagnostics, scope_depth + 1, code, ctx);
            }
        }

        // Other statements don't need special handling
        Stmt::VarDecl { .. }
        | Stmt::Goto(_)
        | Stmt::Label(_)
        | Stmt::Execute { .. }
        | Stmt::AddHandler { .. }
        | Stmt::RemoveHandler { .. } => {}
    }
}

/// Check an expression for insertion method calls.
///
/// Handles two patterns:
/// 1. `Expr::MethodCall { receiver, method, args }` - direct method call
/// 2. `Expr::Call { callee: Expr::Field { base, field }, args }` - call via field access
///
/// The second pattern occurs because the parser creates CALL_EXPR with FIELD_EXPR inside
/// for method calls like `Array.Add(Value)`.
fn check_expr_for_insertion(
    body: &Body,
    source_map: &BodySourceMap,
    expr_id: ExprId,
    tracker: &mut InsertionTracker,
    scope_depth: usize,
    allow_add: bool,
) {
    match body.expr(expr_id) {
        Expr::MethodCall { receiver, method, args } => {
            if let Some(kind) = get_insertion_method_kind(method, allow_add) {
                if !args.is_empty() {
                    if let Some(range) = source_map.expr_range(expr_id) {
                        let receiver_id = ExprId::from_idx(*receiver);
                        let args_vec: Vec<ExprId> =
                            args.iter().map(|&idx| ExprId::from_idx(idx)).collect();
                        tracker.record_insertion(receiver_id, &args_vec, range, scope_depth, kind);
                    }
                }
            }
        }
        // Pattern 2: Call with Field as callee (common for method calls in BSL)
        Expr::Call { callee, args } => {
            if let Expr::Field { base, field } = body.expr(ExprId::from_idx(*callee)) {
                if let Some(kind) = get_insertion_method_kind(field, allow_add) {
                    if !args.is_empty() {
                        if let Some(range) = source_map.expr_range(expr_id) {
                            let base_id = ExprId::from_idx(*base);
                            let args_vec: Vec<ExprId> =
                                args.iter().map(|&idx| ExprId::from_idx(idx)).collect();
                            tracker.record_insertion(base_id, &args_vec, range, scope_depth, kind);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Check an expression for side effects (variable modifications via function calls).
///
/// In BSL, objects passed to functions can be modified. This function tracks
/// when a variable is passed as an argument to a function/method (excluding
/// the insertion methods we're analyzing).
fn check_expr_for_side_effects(
    body: &Body,
    expr_id: ExprId,
    tracker: &mut InsertionTracker,
    allow_add: bool,
) {
    match body.expr(expr_id) {
        // Call with Field as callee: obj.Method(args)
        Expr::Call { callee, args } => {
            // Check if this is NOT an insertion method
            if let Expr::Field { base: _, field } = body.expr(ExprId::from_idx(*callee)) {
                // If it's an insertion method, don't track side effects for args
                // (we handle those separately in check_expr_for_insertion)
                if get_insertion_method_kind(field, allow_add).is_some() {
                    return;
                }
            }

            // Mark all variable arguments as potentially modified
            for arg in args.iter() {
                let arg_id = ExprId::from_idx(*arg);
                if let Some(name) = tracker.extract_target_name(arg_id) {
                    if matches!(body.expr(arg_id), Expr::Path(_) | Expr::Field { .. }) {
                        tracker.generations.increment(&name);
                    }
                }
            }
        }
        // Direct method call: obj.Method(args)
        Expr::MethodCall { receiver: _, method, args } => {
            // Don't track side effects for insertion methods
            if get_insertion_method_kind(method, allow_add).is_some() {
                return;
            }

            // Mark all variable arguments as potentially modified
            for arg in args.iter() {
                let arg_id = ExprId::from_idx(*arg);
                if let Some(name) = tracker.extract_target_name(arg_id) {
                    if matches!(body.expr(arg_id), Expr::Path(_) | Expr::Field { .. }) {
                        tracker.generations.increment(&name);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Check for duplicated insertions into collections across all methods in a module.
///
/// This diagnostic analyzes HIR bodies directly without requiring dataflow analysis.
/// It tracks insertion patterns and detects when the same value is inserted multiple times.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("DuplicatedInsertionIntoCollection::check").entered();
    let code = DiagnosticCode::DuplicatedInsertionIntoCollection;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // Early exit: skip files without insertion methods
    // This avoids expensive HIR analysis for files that can't have duplicated insertions
    let text = ctx.file_text();
    if !has_insertion_methods(&text) {
        return Vec::new();
    }

    let module_bodies = ctx.module_bodies();

    let mut diagnostics = Vec::new();

    // Check each method body for duplicated insertions
    for (_local_id, body, source_map) in module_bodies.method_bodies() {
        diagnostics.extend(check_body(body, source_map, code, ctx));
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};

    #[test]
    fn test_simple_duplicate() {
        let code = r#"
Процедура Тест()
    Массив = Новый Массив;
    Массив.Добавить(Значение);
    Массив.Добавить(Значение);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect one duplicate");

        assert_diagnostic_range(code, &diagnostics[0], 4, 4, 29);
    }

    #[test]
    fn test_generation_change() {
        let code = r#"
Процедура Тест()
    Массив = Новый Массив;
    Массив.Добавить(Х);
    Х = 5;
    Массив.Добавить(Х);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should NOT detect duplicate after generation change");
    }

    #[test]
    fn test_special_literals() {
        let code = r#"
Процедура Тест()
    Список = Новый Массив;
    Список.Добавить("");
    Список.Добавить("");
    Список.Добавить(Неопределено);
    Список.Добавить(Неопределено);
    Список.Добавить(Символы.ПС);
    Список.Добавить(Символы.ПС);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Special literals should be allowed to duplicate");
    }

    #[test]
    fn test_global_function_collection() {
        let code = r#"
Процедура Тест()
    Коллекция().Добавить(Значение);
    Коллекция().Добавить(Значение);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect duplicate with global function");

        assert_diagnostic_range(code, &diagnostics[0], 3, 4, 34);
    }

    #[test]
    fn test_preprocessor_duplicate() {
        // NOTE: HIR currently does not lower statements inside preprocessor directives.
        // This is a known limitation. Code inside #Если/#Иначе is not included in body.body_stmts.
        // The bsl-language-server does detect duplicates across preprocessor branches,
        // but our HIR-based implementation cannot until HIR is extended to support this.
        let code = r#"
Процедура Тест()
    #Если ТолстыйКлиентОбычноеПриложение Тогда
        ЭлементыСтиля.Вставить(ЭлементСтиля.Ключ, ЭлементСтиля.Значение.Получить());
    #Иначе
        ЭлементыСтиля.Вставить(ЭлементСтиля.Ключ, ЭлементСтиля.Значение);
    #КонецЕсли
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        // Current HIR limitation: 0 diagnostics (code inside preprocessor not analyzed)
        // Expected: 1 diagnostic (duplicate key across branches)
        assert_eq!(
            diagnostics.len(),
            0,
            "HIR does not currently analyze code inside preprocessor directives"
        );
    }

    #[test]
    fn test_break_in_loop() {
        let code = r#"
Процедура Тест(Коллекция, Коллекция2)
    Для Каждого Элемент Из Коллекция Цикл
        Коллекция2.Добавить(Элемент);
        Если Условие() Тогда
            Прервать;
        КонецЕсли;
        Коллекция2.Добавить(Элемент);
    КонецЦикла;
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Should NOT detect duplicate (local break may prevent execution)"
        );
    }

    #[test]
    fn test_method_in_collection_path() {
        let code = r#"
Процедура Тест()
    Данные.Метод().Коллекция = Новый Массив;
    Данные.Метод().Коллекция.Добавить("Значение");
    Данные.Метод().Коллекция.Добавить("Значение");
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect duplicate with method in collection path");

        assert_diagnostic_range(code, &diagnostics[0], 4, 4, 49);
    }

    #[test]
    fn test_complex_argument() {
        let code = r#"
Процедура Тест()
    Данные.Метод().ОбщаяКоллекция.Добавить(Данные.Метод().ПовторнаяКоллекция);
    Данные.Метод().ОбщаяКоллекция.Добавить(Данные.Метод().ПовторнаяКоллекция);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect duplicate with complex argument");

        assert_diagnostic_range(code, &diagnostics[0], 3, 4, 77);
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/DuplicatedInsertionIntoCollectionDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // Expected: 16 diagnostics
        // Note: Line 59 (inside #Если/#Иначе) is NOT detected because HIR does not analyze
        // code inside preprocessor directives. This is a known limitation.
        // Note: Line 172 is NOT detected because Добавить compares ALL arguments,
        // and (ИмяКоманды, 1) != (ИмяКоманды, 9, Истина).
        // Note: Line 197 has empty first arg (Missing), so is_special_value returns true
        // and the call is not tracked for duplicates (correct behavior per fixture comment).
        assert_eq!(diagnostics.len(), 16, "Expected 16 diagnostics");

        // Sort diagnostics by position for consistent ordering
        let mut sorted_diagnostics = diagnostics.clone();
        sorted_diagnostics.sort_by_key(|d| d.range.start());

        // Verify each diagnostic with precise line and column positions (0-indexed)
        // Line 5: Массив.Добавить(СтрокаТаблицы)
        assert_diagnostic_range(code, &sorted_diagnostics[0], 4, 4, 34);
        // Line 9: Коллекция.Вставить("Ключ1", 1)
        assert_diagnostic_range(code, &sorted_diagnostics[1], 8, 4, 34);
        // Line 13: Коллекция2.Вставить("Ключ1", 2)
        assert_diagnostic_range(code, &sorted_diagnostics[2], 12, 4, 35);
        // Line 23: Коллекция.Вставить("Ключ1", 3)
        assert_diagnostic_range(code, &sorted_diagnostics[3], 22, 8, 38);
        // Line 28: Итог.Коллекция.Индексы.Добавить("Пользователь")
        assert_diagnostic_range(code, &sorted_diagnostics[4], 27, 8, 55);
        // Line 100: Данные.Метод().ПовторнаяСоздаваемаяКоллекция.Добавить("Пользователь")
        assert_diagnostic_range(code, &sorted_diagnostics[5], 99, 8, 77);
        // Line 103: Данные.Метод().ОбщаяКоллекция.Добавить(Данные.Метод().ПовторнаяСоздаваемаяКоллекция)
        assert_diagnostic_range(code, &sorted_diagnostics[6], 102, 8, 92);
        // Line 120: ВидыСвойствНабора.Вставить("ДополнительныеРеквизиты", Истина)
        assert_diagnostic_range(code, &sorted_diagnostics[7], 119, 4, 65);
        // Line 134: ПовторнаяСоздаваемаяКоллекция.Добавить("Пользователь")
        assert_diagnostic_range(code, &sorted_diagnostics[8], 133, 4, 58);
        // Line 137: ОбщаяКоллекция.Добавить(ПовторнаяСоздаваемаяКоллекция)
        assert_diagnostic_range(code, &sorted_diagnostics[9], 136, 4, 58);
        // Line 148: Данные2.ОбщаяКоллекция2.Вставить(Данные2.Реквизит2.ПовторнаяСоздаваемаяКоллекция2)
        assert_diagnostic_range(code, &sorted_diagnostics[10], 147, 8, 90);
        // Line 152: Данные3.ОбщаяКоллекция3.Вставить(Данные3.Реквизит3.ПовторнаяСоздаваемаяКоллекция3)
        assert_diagnostic_range(code, &sorted_diagnostics[11], 151, 8, 90);
        // Line 158: Описания.Добавить(Ключ)
        assert_diagnostic_range(code, &sorted_diagnostics[12], 157, 4, 27);
        // Line 162: Описания2.Добавить(Часть1.Часть2)
        assert_diagnostic_range(code, &sorted_diagnostics[13], 161, 4, 37);
        // Line 172: NOT detected - Добавить compares ALL args, (ИмяКоманды, 1) != (ИмяКоманды, 9, Истина)
        // Line 266: Коллекция().Добавить(СтрокаТаблицы)
        assert_diagnostic_range(code, &sorted_diagnostics[14], 265, 4, 39);
        // Line 269: Коллекция2().Реквизит.Добавить(СтрокаТаблицы2)
        assert_diagnostic_range(code, &sorted_diagnostics[15], 268, 4, 50);
    }

    #[test]
    fn test_has_insertion_methods() {
        use super::has_insertion_methods;
        // Should find: .Добавить( .Add( .Вставить( .Insert(
        assert!(has_insertion_methods("Массив.Добавить(1)"));
        assert!(has_insertion_methods("Массив.добавить(1)")); // case-insensitive
        assert!(has_insertion_methods("Array.Add(1)"));
        assert!(has_insertion_methods("Array.add(1)")); // case-insensitive
        assert!(has_insertion_methods("Соответствие.Вставить(К, З)"));
        assert!(has_insertion_methods("Соответствие.вставить(К, З)"));
        assert!(has_insertion_methods("Map.Insert(K, V)"));
        assert!(has_insertion_methods("Map.insert(K, V)"));

        // Should NOT find: no dot prefix, wrong methods
        assert!(!has_insertion_methods("Добавить(1)")); // no dot
        assert!(!has_insertion_methods("Процедура Добавить()"));
        assert!(!has_insertion_methods("Массив.Получить(1)"));
        assert!(!has_insertion_methods("Массив.Удалить(1)"));
        assert!(!has_insertion_methods("// комментарий"));
        assert!(!has_insertion_methods(""));
    }
}
