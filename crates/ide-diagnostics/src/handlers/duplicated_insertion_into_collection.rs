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

fn is_special_value(body: &Body, expr_id: ExprId) -> bool {
    match body.expr(expr_id) {
        Expr::Literal(lit) => match lit {
            Literal::String(s) => s.is_empty() || s.chars().all(char::is_whitespace),
            Literal::Undefined | Literal::Null => true,
            Literal::Number(n) => *n == 0.0,
            _ => false,
        },
        Expr::Field { base, field: _ } => {
            if let Expr::Path(name) = body.expr(ExprId::from_idx(*base)) {
                let base_lower = name.as_str().to_lowercase();
                base_lower == "символы" || base_lower == "chars"
            } else {
                false
            }
        }
        Expr::Missing => true,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsertionMethodKind {
    Add,
    Insert,
}

fn get_insertion_method_kind(name: &Name, allow_add: bool) -> Option<InsertionMethodKind> {
    let lower = name.as_str().to_lowercase();
    match lower.as_str() {
        "вставить" | "insert" => Some(InsertionMethodKind::Insert),
        "добавить" | "add" if allow_add => Some(InsertionMethodKind::Add),
        _ => None,
    }
}

fn has_insertion_methods(text: &str) -> bool {
    const PATTERNS: &[&str] = &[".добавить(", ".add(", ".вставить(", ".insert("];

    let bytes = text.as_bytes();

    for (i, &byte) in bytes.iter().enumerate() {
        if byte != b'.' {
            continue;
        }

        for pattern in PATTERNS {
            if matches_case_insensitive(text, i, pattern) {
                return true;
            }
        }
    }

    false
}

#[inline]
fn matches_case_insensitive(text: &str, start: usize, pattern: &str) -> bool {
    let text_bytes = text.as_bytes();
    let remaining = text_bytes.len() - start;

    if remaining < pattern.len() {
        return false;
    }

    let text_slice = &text[start..];
    let mut text_chars = text_slice.chars();
    let mut pattern_chars = pattern.chars();

    loop {
        match (pattern_chars.next(), text_chars.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(p), Some(t)) => {
                let p_lower = p.to_lowercase().next().unwrap_or(p);
                let t_lower = t.to_lowercase().next().unwrap_or(t);
                if p_lower != t_lower {
                    return false;
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Insertion {
    range: TextRange,
    receiver: ExprId,
    args: Vec<ExprId>,
    scope_depth: usize,
    breaker_context: Option<u32>,
    local_breaker_context: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct InsertionKey {
    collection_hash: u64,
    all_args_hash: u64,
}

struct VariableGenerations {
    generations: FxHashMap<SmolStr, usize>,
}

impl VariableGenerations {
    fn new() -> Self {
        Self { generations: FxHashMap::default() }
    }

    fn get(&self, name: &str) -> usize {
        let key: SmolStr = name.to_lowercase().into();

        let direct_gen = self.generations.get(&key).copied().unwrap_or(0);

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

    fn increment(&mut self, name: &str) {
        let key: SmolStr = name.to_lowercase().into();
        *self.generations.entry(key).or_insert(0) += 1;

        let parts: Vec<&str> = name.split('.').collect();
        for i in (1..parts.len()).rev() {
            let prefix: SmolStr = parts[..i].join(".").to_lowercase().into();
            *self.generations.entry(prefix).or_insert(0) += 1;
        }
    }
}

struct InsertionTracker<'a> {
    body: &'a Body,
    generations: VariableGenerations,
    insertions: FxHashMap<InsertionKey, Vec<Insertion>>,
    last_breaker: Option<(u32, usize)>,
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

    fn hash_expr(&self, expr_id: ExprId) -> u64 {
        let mut hasher = FxHasher::default();
        self.hash_expr_into(expr_id, &mut hasher);
        hasher.finish()
    }

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
                for c in name_str.chars() {
                    for lc in c.to_lowercase() {
                        hasher.write_u32(lc as u32);
                    }
                }
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
                for c in field.as_str().chars() {
                    for lc in c.to_lowercase() {
                        hasher.write_u32(lc as u32);
                    }
                }
            }

            Expr::MethodCall { receiver, method, args } => {
                hasher.write_u8(expr_tag::METHOD_CALL);
                self.hash_expr_into(ExprId::from_idx(*receiver), hasher);
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
                hasher.write_u8(expr_tag::MISSING);
            }
        }
    }

    fn record_assignment(&mut self, target: ExprId) {
        let name = self.extract_target_name(target);
        if let Some(name) = name {
            tracing::trace!(name = %name, "recording assignment");
            self.generations.increment(&name);
        }
    }

    fn extract_target_name(&self, expr_id: ExprId) -> Option<String> {
        match self.body.expr(expr_id) {
            Expr::Path(name) => Some(name.to_string()),
            Expr::Field { base, field } => {
                let base_name = self.extract_target_name(ExprId::from_idx(*base))?;
                Some(format!("{}.{}", base_name, field))
            }
            Expr::Index { base, .. } => self.extract_target_name(ExprId::from_idx(*base)),
            Expr::MethodCall { receiver, method, .. } => {
                let base_name = self.extract_target_name(ExprId::from_idx(*receiver))?;
                Some(format!("{}.{}()", base_name, method))
            }
            _ => None,
        }
    }

    fn record_breaker(&mut self, offset: u32, scope_depth: usize) {
        self.last_breaker = Some((offset, scope_depth));
    }

    fn record_local_breaker(&mut self, offset: u32, scope_depth: usize) {
        self.last_local_breaker = Some((offset, scope_depth));
    }

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

        if is_special_value(self.body, args[0]) {
            return;
        }

        let collection_hash = self.hash_expr(receiver);

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
                let mut grouped: FxHashMap<(Option<u32>, Option<u32>), Vec<&Insertion>> =
                    FxHashMap::default();

                for ins in scope_insertions {
                    let key = (ins.breaker_context, ins.local_breaker_context);
                    grouped.entry(key).or_default().push(ins);
                }

                for group in grouped.values() {
                    if group.len() > 1 {
                        if let Some(second_insertion) = group.get(1) {
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

        for insertions in self.insertions.values_mut() {
            insertions.retain(|ins| ins.scope_depth != scope_depth);
        }
    }
}

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
            for branch in preproc.branches() {
                let stmts: Vec<StmtId> =
                    branch.stmts.iter().map(|&idx| StmtId::from_idx(idx)).collect();
                check_stmt_list(
                    body,
                    source_map,
                    &stmts,
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

        Stmt::VarDecl { .. }
        | Stmt::Goto(_)
        | Stmt::Label(_)
        | Stmt::Execute { .. }
        | Stmt::AddHandler { .. }
        | Stmt::RemoveHandler { .. } => {}
    }
}

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

fn check_expr_for_side_effects(
    body: &Body,
    expr_id: ExprId,
    tracker: &mut InsertionTracker,
    allow_add: bool,
) {
    match body.expr(expr_id) {
        Expr::Call { callee, args } => {
            if let Expr::Field { base: _, field } = body.expr(ExprId::from_idx(*callee)) {
                if get_insertion_method_kind(field, allow_add).is_some() {
                    return;
                }
            }

            for arg in args.iter() {
                let arg_id = ExprId::from_idx(*arg);
                if let Some(name) = tracker.extract_target_name(arg_id) {
                    if matches!(body.expr(arg_id), Expr::Path(_) | Expr::Field { .. }) {
                        tracker.generations.increment(&name);
                    }
                }
            }
        }
        Expr::MethodCall { receiver: _, method, args } => {
            if get_insertion_method_kind(method, allow_add).is_some() {
                return;
            }

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

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("DuplicatedInsertionIntoCollection::check").entered();
    let code = DiagnosticCode::DuplicatedInsertionIntoCollection;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let text = ctx.file_text();
    if !has_insertion_methods(&text) {
        return Vec::new();
    }

    let module_bodies = ctx.module_bodies();

    let mut diagnostics = Vec::new();

    for (_local_id, body, source_map) in module_bodies.method_bodies() {
        diagnostics.extend(check_body(body, source_map, code, ctx));
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic, format_diags};
    use expect_test::expect;

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
        expect![[r#"
            DuplicatedInsertionIntoCollection @ 5:5..5:30
              message: Проверьте повторную вставку Значение в коллекцию Массив
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
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
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
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
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
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
        expect![[r#"
            DuplicatedInsertionIntoCollection @ 4:5..4:35
              message: Проверьте повторную вставку Значение в коллекцию Коллекция()
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_preprocessor_duplicate() {
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
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_preprocessor_intra_branch_dup() {
        let code = r#"
Процедура Тест()
    #Если Сервер Тогда
        Массив.Добавить(Значение);
        Массив.Добавить(Значение);
    #КонецЕсли
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DuplicatedInsertionIntoCollection @ 5:9..5:34
              message: Проверьте повторную вставку Значение в коллекцию Массив
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_preprocessor_mixed_intra_dup_with_cross_branch_same() {
        let code = r#"
Процедура Тест()
    #Если Сервер Тогда
        Массив.Добавить(Значение);
        Массив.Добавить(Значение);
    #Иначе
        Массив.Добавить(Значение);
    #КонецЕсли
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DuplicatedInsertionIntoCollection @ 5:9..5:34
              message: Проверьте повторную вставку Значение в коллекцию Массив
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_preprocessor_nested_intra_branch_dup() {
        let code = r#"
Процедура Тест()
    #Если Сервер Тогда
        #Если ВнешнееСоединение Тогда
            Массив.Добавить(Значение);
            Массив.Добавить(Значение);
        #КонецЕсли
    #КонецЕсли
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DuplicatedInsertionIntoCollection @ 6:13..6:38
              message: Проверьте повторную вставку Значение в коллекцию Массив
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
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
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
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
        expect![[r#"
            DuplicatedInsertionIntoCollection @ 5:5..5:50
              message: Проверьте повторную вставку "Значение" в коллекцию Данные.Метод().Коллекция
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
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
        expect![[r#"
            DuplicatedInsertionIntoCollection @ 4:5..4:78
              message: Проверьте повторную вставку Данные.Метод().ПовторнаяКоллекция в коллекцию Данные.Метод().ОбщаяКоллекция
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_insert_duplicate_key_different_value() {
        let code = r#"
Процедура Тест()
    Коллекция = Новый Структура;
    Коллекция.Вставить("Ключ1", 1);
    Коллекция.Вставить("Ключ1", 2);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DuplicatedInsertionIntoCollection @ 5:5..5:35
              message: Проверьте повторную вставку "Ключ1", 2 в коллекцию Коллекция
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_nested_field_duplicate() {
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Итог.Коллекция.Индексы.Добавить("Пользователь");
        Итог.Коллекция.Индексы.Добавить("Пользователь");
    КонецЦикла;
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DuplicatedInsertionIntoCollection @ 5:9..5:56
              message: Проверьте повторную вставку "Пользователь" в коллекцию Итог.Коллекция.Индексы
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_different_receivers_no_duplicate() {
        let code = r#"
Процедура Тест()
    Итог.ПерваяКоллекция.Индексы.Добавить("Пользователь");
    Итог.ВтораяКоллекция.Индексы.Добавить("Пользователь");
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_reinit_clears_tracking() {
        let code = r#"
Процедура Тест()
    Если Условие() Тогда
        КоллекцияА = Новый Массив;
        КоллекцияА.Добавить("Пользователь");
        ОбщаяКоллекция.Добавить(КоллекцияА);
        КоллекцияА = Новый Массив;
        КоллекцияА.Добавить("Пользователь");
        ОбщаяКоллекция.Добавить(КоллекцияА);
    КонецЕсли;
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_return_interrupts_flow() {
        let code = r#"
Функция Тест(Ссылка)
    ВидыСвойствНабора = Новый Структура;
    ВидыСвойствНабора.Вставить("ДополнительныеРеквизиты", Ложь);

    Если УсловиеВозврата() Тогда
        Возврат ВидыСвойствНабора;
    КонецЕсли;

    ВидыСвойствНабора.Вставить("ДополнительныеРеквизиты", Истина);
    Возврат ВидыСвойствНабора;
КонецФункции
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_loop_break_interrupts_flow() {
        let code = r#"
Функция Тест()
    ВидыСвойствНабора = Новый Структура;
    ВидыСвойствНабора.Вставить("ДополнительныеРеквизиты", Ложь);

    Для Каждого Элемент Из Коллекция Цикл
        Прервать;
    КонецЦикла;

    ВидыСвойствНабора.Вставить("ДополнительныеРеквизиты", Истина);
    Возврат ВидыСвойствНабора;
КонецФункции
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            DuplicatedInsertionIntoCollection @ 10:5..10:66
              message: Проверьте повторную вставку "ДополнительныеРеквизиты", Истина в коллекцию ВидыСвойствНабора
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_add_different_arg_counts_no_duplicate() {
        let code = r#"
Процедура Тест()
    Сведения2.ДобавленныеЭлементы.Добавить(ИмяКоманды, 1);
    Сведения2.ДобавленныеЭлементы.Добавить(ИмяКоманды, 9, Истина);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_insert_with_key_change_no_duplicate() {
        let code = r#"
Процедура Тест()
    Контекст.Коллекция.Вставить("ИмяПрава", "Чтение");
    ЗаполнитьСтруктуруРасчетаПрава(Результат.СтруктураРасчетаПраваЧтение, Контекст.Коллекция);
    Контекст.Коллекция.Вставить("ИмяПрава", "Изменение");
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_has_insertion_methods() {
        use super::has_insertion_methods;
        assert!(has_insertion_methods("Массив.Добавить(1)"));
        assert!(has_insertion_methods("Массив.добавить(1)"));
        assert!(has_insertion_methods("Array.Add(1)"));
        assert!(has_insertion_methods("Array.add(1)"));
        assert!(has_insertion_methods("Соответствие.Вставить(К, З)"));
        assert!(has_insertion_methods("Соответствие.вставить(К, З)"));
        assert!(has_insertion_methods("Map.Insert(K, V)"));
        assert!(has_insertion_methods("Map.insert(K, V)"));

        assert!(!has_insertion_methods("Добавить(1)"));
        assert!(!has_insertion_methods("Процедура Добавить()"));
        assert!(!has_insertion_methods("Массив.Получить(1)"));
        assert!(!has_insertion_methods("Массив.Удалить(1)"));
        assert!(!has_insertion_methods("// комментарий"));
        assert!(!has_insertion_methods(""));
    }
}
