//! Reports temp files from `GetTempFileName()` that are not cleaned up locally.
//!
//! Migrated to a CFG-anchored forward MAY analysis through the
//! generic `dataflow::temp_resource` lattice (Track 1 Step Q-β-2).
//! Pre-Step-Q the diagnostic ran an AST-order body-wide BFS check —
//! "any deletion call anywhere in the body whose path matches the
//! configured regex and whose arg references the variable counts as
//! cleanup". Post-migration the principled MAY analysis flags every
//! path on which a temp file is opened but never closed before exit,
//! including conditional cleanups whose else-branch leaves the file
//! open.
//!
//! ## Key resolution
//!
//! Resource keys are *lowercased variable names*. A `Get*` call is
//! recorded in the open-set only when it is the direct RHS of an
//! `Assign` whose target is a `Path` —
//! `Файл = ПолучитьИмяВременногоФайла("xml")`. Inline uses
//! (`Записать(GetTempFileName(...))`,
//! `Файл = Новый Файл(GetTempFileName(...))`) cannot be tracked by
//! the lattice — they have no variable to key on — and emit an
//! immediate diagnostic via the inline pre-pass below.
//!
//! ## Multi-event closes
//!
//! BSL's `УдалитьФайлы(path, mask)` overload deletes both files in
//! a single call. The provider's `classify_many` walks each arg
//! subtree for a reference to a tracked variable and emits one
//! `Close(var)` event per match — see
//! `dataflow::temp_resource::ResourceProvider`'s contract.
//!
//! ## Legacy parity
//!
//! `obj.field = GetTempFileName(...)` (non-`Path` Assign target) is
//! suppressed — the pre-Step-Q handler emitted nothing for this
//! shape, and the migration preserves that contract. Promoting it
//! to a leak diagnostic lands as a follow-up.

use hir::cfg::{CfgBuilder, ControlFlowGraph};
use hir::dataflow::temp_resource::{analyze_open_resources, ResourceEvent, ResourceProvider};
use hir::{Body, BodySourceMap, Expr, ExprId, ExprIdx, IdConversion, Stmt};
use regex::Regex;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Default deletion methods pattern (case-insensitive, anchored).
const DEFAULT_SEARCH_DELETE_FILE_METHOD: &str =
    "УдалитьФайлы|DeleteFiles|НачатьУдалениеФайлов|BeginDeletingFiles|ПереместитьФайл|MoveFile";

/// Diagnostic message for inline `GetTempFileName()` uses (no
/// variable name to interpolate).
const INLINE_MESSAGE: &str = "Нужно добавить удаление временного файла после использования";

#[derive(Debug, Clone)]
struct Config {
    deletion_methods: Regex,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let pattern = ctx
            .config
            .get_string(DiagnosticCode::MissingTemporaryFileDeletion, "searchDeleteFileMethod")
            .unwrap_or(DEFAULT_SEARCH_DELETE_FILE_METHOD);
        let regex_pattern = format!("(?i)^({})$", pattern);
        let deletion_methods = Regex::new(&regex_pattern).unwrap_or_else(|e| {
            tracing::warn!(
                error = %e,
                pattern = %pattern,
                "Invalid searchDeleteFileMethod regex, using default"
            );
            Regex::new(&format!("(?i)^({})$", DEFAULT_SEARCH_DELETE_FILE_METHOD))
                .expect("Default regex must be valid")
        });
        tracing::debug!(pattern = %pattern, "MissingTemporaryFileDeletion config loaded");
        Self { deletion_methods }
    }
}

/// Variable name preserved in both original case (for diagnostic
/// messages) and lower case (for the lattice's resource key).
struct AssignedGet {
    original_name: String,
    lower_name: String,
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MissingTemporaryFileDeletion;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let module_bodies = ctx.module_bodies();
    let module_cfgs = ctx.module_cfgs();
    let mut diagnostics = Vec::new();

    for (local_id, body) in module_bodies.iter_bodies() {
        let Some(source_map) = module_bodies.source_map(local_id) else { continue };
        // Build a CFG on demand if Salsa hasn't materialised one for
        // this body — pre-Step-Q the legacy handler accepted a
        // missing CFG by falling back to a body-wide BFS, and
        // silently dropping the body would regress that contract.
        // The on-demand build mirrors the module-level path below.
        let cfg_arc = module_cfgs.get(local_id);
        let owned_cfg;
        let cfg: &ControlFlowGraph = if let Some(ref arc) = cfg_arc {
            arc.as_ref()
        } else {
            owned_cfg = CfgBuilder::new().build_graph_from_hir(
                body.body_stmts_typed(),
                body,
                Some(source_map),
            );
            &owned_cfg
        };
        diagnostics.extend(check_body(body, source_map, cfg, &config, code, ctx));
    }

    if let Some(module_result) = module_bodies.module_code_result() {
        let body = &module_result.body;
        let source_map = &module_result.source_map;
        let cfg =
            CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), body, Some(source_map));
        diagnostics.extend(check_body(body, source_map, &cfg, &config, code, ctx));
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    cfg: &ControlFlowGraph,
    config: &Config,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    // Pre-pass: bucket every `Get*` call into either a tracked
    // (lattice-driven) or suppressed (legacy parity, see module-doc)
    // role. Inline uses are everything else and emit immediately
    // below.
    let (tracked, suppressed) = collect_assigned_gets(body);

    let mut diagnostics = Vec::new();

    for (expr_id, expr) in body.exprs_iter() {
        let Expr::Call { callee, .. } = expr else { continue };
        let Expr::Path(name) = body.expr_idx(*callee) else { continue };
        if !is_get_temp_filename(name.as_str()) {
            continue;
        }
        let idx: ExprIdx = expr_id.to_idx();
        if tracked.contains_key(&idx) || suppressed.contains(&idx) {
            continue;
        }
        if let Some(range) = source_map.expr_range(expr_id) {
            diagnostics.push(Diagnostic {
                code,
                message: INLINE_MESSAGE.to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    if tracked.is_empty() {
        return diagnostics;
    }

    let known_vars: FxHashSet<String> = tracked.values().map(|ag| ag.lower_name.clone()).collect();
    let provider = TempFileProvider {
        deletion_methods: &config.deletion_methods,
        tracked: &tracked,
        known_vars: &known_vars,
    };
    let Some(result) = analyze_open_resources::<_, String>(body, cfg, provider) else {
        return diagnostics;
    };

    // For each `(var_lower, leaked_sites)` at exit, emit one
    // diagnostic per leaked site, with the variable's original-case
    // name in the message. The lattice's resource key (`var_lower`)
    // is unused at the emission site — `tracked.get(site)` already
    // carries the original-case name needed for the message — so
    // iterate values only.
    for sites in result.open_at_exit().values() {
        for &site in sites {
            let Some(ag) = tracked.get(&site) else { continue };
            let Some(range) = source_map.expr_range(ExprId::from_idx(site)) else { continue };
            diagnostics.push(Diagnostic {
                code,
                message: format!(
                    "Нужно добавить удаление временного файла '{}' после использования",
                    ag.original_name
                ),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    diagnostics
}

/// Walk body statements and bucket every `GetTempFileName` call
/// expression that is the direct RHS of an `Assign`:
/// - target = `Path(name)` → `tracked`, keyed on `lower(name)`.
/// - any other target shape → `suppressed` (legacy parity).
///
/// Inline Gets are derived implicitly by the caller via
/// `body.exprs_iter()`.
fn collect_assigned_gets(body: &Body) -> (FxHashMap<ExprIdx, AssignedGet>, FxHashSet<ExprIdx>) {
    let mut tracked: FxHashMap<ExprIdx, AssignedGet> = FxHashMap::default();
    let mut suppressed: FxHashSet<ExprIdx> = FxHashSet::default();

    for (_, stmt) in body.stmts_iter() {
        let Stmt::Assign { target, value } = stmt else { continue };
        if !is_get_temp_filename_call(body, *value) {
            continue;
        }
        match body.expr_idx(*target) {
            Expr::Path(name) => {
                tracked.insert(
                    *value,
                    AssignedGet {
                        original_name: name.as_str().to_string(),
                        lower_name: name.as_str().to_lowercase(),
                    },
                );
            }
            _ => {
                suppressed.insert(*value);
            }
        }
    }

    (tracked, suppressed)
}

/// Provider for the lattice's forward MAY analysis.
struct TempFileProvider<'a> {
    deletion_methods: &'a Regex,
    tracked: &'a FxHashMap<ExprIdx, AssignedGet>,
    known_vars: &'a FxHashSet<String>,
}

impl<'a> ResourceProvider<String> for TempFileProvider<'a> {
    fn classify(&self, _body: &Body, expr_idx: ExprIdx) -> Option<ResourceEvent<String>> {
        // Open events: this expr is a tracked `Get*` call. Closes
        // flow through `classify_many` because a single deletion
        // call may close several resources at once.
        self.tracked.get(&expr_idx).map(|ag| ResourceEvent::Open(ag.lower_name.clone()))
    }

    fn classify_many(&self, body: &Body, expr_idx: ExprIdx) -> Vec<ResourceEvent<String>> {
        if let Some(open) = self.classify(body, expr_idx) {
            return vec![open];
        }
        let (callee_path, args) = match body.expr_idx(expr_idx) {
            Expr::Call { callee, args } => (extract_call_path(body, *callee), args),
            Expr::MethodCall { method, args, .. } => (method.as_str().to_string(), args),
            _ => return Vec::new(),
        };
        if !self.deletion_methods.is_match(&callee_path) {
            return Vec::new();
        }
        let mut closed: FxHashSet<String> = FxHashSet::default();
        for &arg in args.iter() {
            collect_referenced_vars(body, arg, self.known_vars, &mut closed);
        }
        closed.into_iter().map(ResourceEvent::Close).collect()
    }
}

/// Build the full dotted call-path of a `Call` callee. Mirrors the
/// pre-Step-Q `extract_call_path` so the deletion regex matches
/// the same surface (`УдалитьФайлы`, `obj.УдалитьФайл`,
/// `Mod.Sub.Method`, …) it always did.
fn extract_call_path(body: &Body, callee: ExprIdx) -> String {
    match body.expr_idx(callee) {
        Expr::Path(name) => name.as_str().to_string(),
        Expr::Field { base, field } => {
            let base_path = extract_call_path(body, *base);
            if base_path.is_empty() {
                field.as_str().to_string()
            } else {
                format!("{}.{}", base_path, field.as_str())
            }
        }
        Expr::QualifiedPath(path) => {
            path.segments().iter().map(|s| s.as_str()).collect::<Vec<_>>().join(".")
        }
        _ => String::new(),
    }
}

/// Walk `expr` depth-first, inserting (lower-cased) names of every
/// `Path` whose lowered form is in `known_vars`. The set of
/// recursed-into expression shapes mirrors the pre-Step-Q
/// `expr_contains_var` walker exactly:
///
/// **Recursed:** `Path`, `Call`, `MethodCall`, `Field`, `Index`,
/// `BinaryOp`, `UnaryOp`, `New`.
///
/// **Not recursed (returns no closed vars):** `Ternary`, `Array`,
/// `Await`, `QualifiedPath`, `Literal`, `Missing`. Each of these is
/// a *non-direct wrapper* — its value is conditional, computed, or
/// disjunctive, so a textual occurrence of a tracked variable
/// inside one of them does not statically prove the deletion call
/// closes that variable. Treating
/// `УдалитьФайлы(?(Условие, Файл1, Файл2))` as closing both
/// branches would be a false negative under MAY semantics — at
/// runtime exactly one branch evaluates, leaving the other temp
/// file leaked. The legacy walker fell into its catch-all
/// `_ => false` arm for these shapes; the migration preserves
/// that conservative parity.
fn collect_referenced_vars(
    body: &Body,
    expr_idx: ExprIdx,
    known_vars: &FxHashSet<String>,
    out: &mut FxHashSet<String>,
) {
    match body.expr_idx(expr_idx) {
        Expr::Path(name) => {
            let lower = name.as_str().to_lowercase();
            if known_vars.contains(&lower) {
                out.insert(lower);
            }
        }
        Expr::Call { callee, args } => {
            collect_referenced_vars(body, *callee, known_vars, out);
            for &a in args.iter() {
                collect_referenced_vars(body, a, known_vars, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_referenced_vars(body, *receiver, known_vars, out);
            for &a in args.iter() {
                collect_referenced_vars(body, a, known_vars, out);
            }
        }
        Expr::Field { base, .. } => collect_referenced_vars(body, *base, known_vars, out),
        Expr::Index { base, index } => {
            collect_referenced_vars(body, *base, known_vars, out);
            collect_referenced_vars(body, *index, known_vars, out);
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_referenced_vars(body, *lhs, known_vars, out);
            collect_referenced_vars(body, *rhs, known_vars, out);
        }
        Expr::UnaryOp { expr, .. } => collect_referenced_vars(body, *expr, known_vars, out),
        Expr::New { args, .. } => {
            for &a in args.iter() {
                collect_referenced_vars(body, a, known_vars, out);
            }
        }
        Expr::Ternary { .. }
        | Expr::Array(_)
        | Expr::Await { .. }
        | Expr::Literal(_)
        | Expr::QualifiedPath(_)
        | Expr::Missing => {}
    }
}

/// True iff `expr_idx` is a `Call` whose callee is a `Path` matching
/// `GetTempFileName` (case-insensitive, bilingual).
fn is_get_temp_filename_call(body: &Body, expr_idx: ExprIdx) -> bool {
    let Expr::Call { callee, .. } = body.expr_idx(expr_idx) else { return false };
    let Expr::Path(name) = body.expr_idx(*callee) else { return false };
    is_get_temp_filename(name.as_str())
}

/// Bilingual case-insensitive `GetTempFileName` predicate.
fn is_get_temp_filename(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "получитьимявременногофайла" || lower == "gettempfilename"
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::*;
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;

    const FIXTURE: &str = "\nПроцедура ПроверкаДиагностики()\n\n    Путь = \"12345.xml\";\n\n    Данные = Base64Значение(\"12345\");\n    ИмяПромежуточногоФайла = ПолучитьИмяВременногоФайла(\"xml\"); // ошибка\n    Данные.Записать(ИмяПромежуточногоФайла);\n\n    ИмяПромежуточногоФайла2 = ПолучитьИмяВременногоФайла(\"xml\"); \n    Данные.Записать(ИмяПромежуточногоФайла2);\n    УдалитьФайлы(ИмяПромежуточногоФайла2);\n\n    ИмяПромежуточногоФайла3 = ПолучитьИмяВременногоФайла(\"xml\"); \n    Данные.Записать(ИмяПромежуточногоФайла3);\n    ПереместитьФайл(ИмяПромежуточногоФайла3, Путь);\n\n    // ошибка, если нет поиска\n    // РаботаСФайламиСлужебныйКлиент.УдалитьФайл\n    ИмяПромежуточногоФайла4 = ПолучитьИмяВременногоФайла(\"xml\"); // ошибка, если нет исключения\n    Данные.Записать(ИмяПромежуточногоФайла4);\n    РаботаСФайламиСлужебныйКлиент.УдалитьФайл(Неопределено, ИмяПромежуточногоФайла4);\n\n    // ошибка, если нет поиска\n    // РандомнаяПроцедураУдаленияФайла\n    ИмяПромежуточногоФайла5 = ПолучитьИмяВременногоФайла(\"xml\");\n    Данные.Записать(ИмяПромежуточногоФайла5);\n    РандомнаяПроцедураУдаленияФайла(ИмяПромежуточногоФайла5);\n\n    // ошибка, если нет \"НачатьУдалениеФайлов\"\n    ИмяПромежуточногоФайла6 = ПолучитьИмяВременногоФайла(\"xml\");\n    НачатьУдалениеФайлов(, ИмяПромежуточногоФайла6);\n\n    // ошибка, если нет \"BeginDeletingFiles\"\n    TempFile7 = GetTempFileName(\"xml\");\n    BeginDeletingFiles(, TempFile7);\n\nКонецПроцедуры\n\nПроцедура РандомнаяПроцедураУдаленияФайла(ИмяФайла)\n    УдалитьФайлы(ИмяФайла);\nКонецПроцедуры\n\nПроцедура ПроверкаДиагностикиСОбщимМодулем()\n\n    ИмяПромежуточногоФайла = ПолучитьИмяВременногоФайла(\"xml\"); // <-- Ошибки нет, ниже удаление\n    Данные.Записать(ИмяПромежуточногоФайла);\n\n\n    ИмяПромежуточногоФайла2 = ПолучитьИмяВременногоФайла(\"txt\"); // <-- Ошибка, удаления файла нет\n    Данные.Записать(ИмяПромежуточногоФайла2);\n\n    ОбщийМодуль.УдалитьВсеФайлы2(ИмяПромежуточногоФайла);\n    Обработки.ДляУдаления.УдалитьВсеФайлы(ИмяПромежуточногоФайла);\n    Статус = Справочники.ОбщийМодуль.УдалитьВсеФайлы(ИмяПромежуточногоФайла);\n\nКонецПроцедуры\n\nПроцедура ПроверкаДиагностикиСОбщимМодулем_Модуль()\n\n    ИмяПромежуточногоФайла = ПолучитьИмяВременногоФайла(); // <-- Ошибки нет, ниже удаление\n    ДвоичныеДанные = Модуль(\"РаботаСФайлами\").ДвоичныеДанныеФайла(ИмяПромежуточногоФайла);\n    УдалитьФайлы(ИмяПромежуточногоФайла);\n\n    ИмяПромежуточногоФайла3 = ПолучитьИмяВременногоФайла(); // <-- Ошибка, удаления файла нет\n    ДвоичныеДанные = Модуль(\"РаботаСФайлами\").ДвоичныеДанныеФайла(ИмяПромежуточногоФайла3);\n\nКонецПроцедуры\n\nФункция Тест()\n    Если Условие Тогда\n        ИмяФайлаНаДиске = ПолучитьИмяВременногоФайла(); // ошибка, удаления файла нет\n        ПолучитьИзВременногоХранилища(ИмяФайла).Записать(ИмяФайлаНаДиске);\n     Иначе\n        ИмяФайлаНаДиске = ИмяФайла;\n    КонецЕсли;\n\n    Возврат ТекстИзФайла;\nКонецФункции";

    #[test]
    fn test_default_config() {
        let code = FIXTURE;
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        expect![[r#"
            MissingTemporaryFileDeletion @ 7:30..7:63
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 20:31..20:64
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла4' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 26:31..26:64
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла5' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 46:30..46:63
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 50:31..50:64
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла2' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 65:31..65:59
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла3' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 72:27..72:55
              message: Нужно добавить удаление временного файла 'ИмяФайлаНаДиске' после использования
              severity: Major"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_extended_config() {
        let code = FIXTURE;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingTemporaryFileDeletion,
            serde_json::json!({
                "searchDeleteFileMethod": "УдалитьФайлы|DeleteFiles|НачатьУдалениеФайлов|BeginDeletingFiles|ПереместитьФайл|MoveFile|РаботаСФайламиСлужебныйКлиент.УдалитьФайл|Справочники.ОбщийМодуль.УдалитьВсеФайлы"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        expect![[r#"
            MissingTemporaryFileDeletion @ 7:30..7:63
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 26:31..26:64
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла5' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 50:31..50:64
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла2' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 65:31..65:59
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла3' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 72:27..72:55
              message: Нужно добавить удаление временного файла 'ИмяФайлаНаДиске' после использования
              severity: Major"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_restrictive_config() {
        let code = FIXTURE;

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingTemporaryFileDeletion,
            serde_json::json!({
                "searchDeleteFileMethod": "УдалитьФайл|DeleteFile|НачатьУдалениеФайловВсех|ОбщийМодуль.УдалитьВсеФайлы"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        expect![[r#"
            MissingTemporaryFileDeletion @ 7:30..7:63
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 10:31..10:64
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла2' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 14:31..14:64
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла3' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 20:31..20:64
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла4' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 26:31..26:64
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла5' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 31:31..31:64
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла6' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 35:17..35:39
              message: Нужно добавить удаление временного файла 'TempFile7' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 46:30..46:63
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 50:31..50:64
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла2' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 61:30..61:58
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 65:31..65:59
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла3' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 72:27..72:55
              message: Нужно добавить удаление временного файла 'ИмяФайлаНаДиске' после использования
              severity: Major"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_range_debug() {
        let code = r#"
Процедура Тест()
    ИмяПромежуточногоФайла = ПолучитьИмяВременногоФайла("xml"); // ошибка
КонецПроцедуры
        "#;
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        expect![[r#"
            MissingTemporaryFileDeletion @ 3:30..3:63
              message: Нужно добавить удаление временного файла 'ИмяПромежуточногоФайла' после использования
              severity: Major"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_inline_usage() {
        // Inline GetTempFileName usage (without assignment) is
        // always flagged via the inline pre-pass — there is no
        // variable to track through the lattice.

        let code = r#"
            Процедура Тест()
                Записать(GetTempFileName("txt"));
                ПолучитьИмяВременногоФайла("xml");  // standalone call
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);

        expect![[r#"
            MissingTemporaryFileDeletion @ 3:26..3:48
              message: Нужно добавить удаление временного файла после использования
              severity: Major
            MissingTemporaryFileDeletion @ 4:17..4:50
              message: Нужно добавить удаление временного файла после использования
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));

        let code2 = r#"
            Процедура Тест()
                Файл = Новый Файл(ПолучитьИмяВременногоФайла("xml"));
            КонецПроцедуры
        "#;
        let diagnostics2 = check_ast_diagnostic(code2, check);

        expect![[r#"
            MissingTemporaryFileDeletion @ 3:35..3:68
              message: Нужно добавить удаление временного файла после использования
              severity: Major"#]]
        .assert_eq(&format_diags(code2, &diagnostics2));
    }

    #[test]
    fn test_comprehensive_java_compatibility() {
        let code = r#"
            Процедура ТестВсехКейсов()
                // Case 1: Normal assignment with deletion - OK
                Файл1 = ПолучитьИмяВременногоФайла("xml");
                УдалитьФайлы(Файл1);

                // Case 2: Normal assignment without deletion - ERROR
                Файл2 = ПолучитьИмяВременногоФайла("xml");

                // Case 3: Inline usage in function call - ERROR
                Записать(GetTempFileName("txt"));

                // Case 4: Inline usage in expression - ERROR
                Файл3 = Новый Файл(ПолучитьИмяВременногоФайла("doc"));

                // Case 5: Standalone call - ERROR
                ПолучитьИмяВременногоФайла("tmp");

                // Case 6: Assignment with move (not deletion) - OK with default config
                Файл4 = ПолучитьИмяВременногоФайла("xml");
                ПереместитьФайл(Файл4, "новое_имя.xml");
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);

        expect![[r#"
            MissingTemporaryFileDeletion @ 8:25..8:58
              message: Нужно добавить удаление временного файла 'Файл2' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 11:26..11:48
              message: Нужно добавить удаление временного файла после использования
              severity: Major
            MissingTemporaryFileDeletion @ 14:36..14:69
              message: Нужно добавить удаление временного файла после использования
              severity: Major
            MissingTemporaryFileDeletion @ 17:17..17:50
              message: Нужно добавить удаление временного файла после использования
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_simple_cases() {
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
                УдалитьФайлы(ИмяФайла);
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));

        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTemporaryFileDeletion @ 3:28..3:61
              message: Нужно добавить удаление временного файла 'ИмяФайла' после использования
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));

        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
                ПереместитьФайл(ИмяФайла, "новое_имя.xml");
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
            Процедура Тест()
                Файл1 = ПОЛУЧИТЬИМЯВРЕМЕННОГОФАЙЛА("xml");
                Файл2 = получитьимявременногофайла("xml");
                Файл3 = ПолучитьИмяВременногоФайла("xml");
                УдалитьФайлы(Файл3);
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTemporaryFileDeletion @ 3:25..3:58
              message: Нужно добавить удаление временного файла 'Файл1' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 4:25..4:58
              message: Нужно добавить удаление временного файла 'Файл2' после использования
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
            Procedure Test()
                TempFile = GetTempFileName("xml");
            EndProcedure
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTemporaryFileDeletion @ 3:28..3:50
              message: Нужно добавить удаление временного файла 'TempFile' после использования
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));

        let code = r#"
            Procedure Test()
                TempFile = GetTempFileName("xml");
                DeleteFiles(TempFile);
            EndProcedure
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_module_qualified_calls() {
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
                РаботаСФайламиКлиент.УдалитьФайл(Неопределено, ИмяФайла);
            КонецПроцедуры
        "#;
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#"
            MissingTemporaryFileDeletion @ 3:28..3:61
              message: Нужно добавить удаление временного файла 'ИмяФайла' после использования
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingTemporaryFileDeletion,
            serde_json::json!({
                "searchDeleteFileMethod": "УдалитьФайлы|DeleteFiles|РаботаСФайламиКлиент.УдалитьФайл"
            }),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_lattice_flags_conditional_cleanup_leak() {
        // Conditional cleanup: Get on the linear path, Delete only
        // inside an `Если` — the else-fall-through path leaves the
        // file open at exit. Pre-Step-Q the body-wide BFS missed
        // this leak (deletion call existed somewhere in the body);
        // post-migration the principled MAY analysis flags it.
        let code = r#"
            Процедура Тест(Условие)
                Файл = ПолучитьИмяВременногоФайла("xml");
                Если Условие Тогда
                    УдалитьФайлы(Файл);
                КонецЕсли;
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTemporaryFileDeletion @ 3:24..3:57
              message: Нужно добавить удаление временного файла 'Файл' после использования
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_multi_arg_deletion_closes_each_file() {
        // BSL `УдалитьФайлы(path, mask)` deletes both files in a
        // single call. The provider's `classify_many` walks each
        // arg subtree for a tracked-var reference and emits one
        // `Close(var)` event per match — both files must close on
        // every path through this single call.
        let code = r#"
            Процедура Тест()
                Файл1 = ПолучитьИмяВременногоФайла("xml");
                Файл2 = ПолучитьИмяВременногоФайла("txt");
                УдалитьФайлы(Файл1, Файл2);
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_get_in_branch_delete_in_separate_branch_leaks() {
        // Get inside one `Если`, Delete inside a sibling `Если`
        // whose else-branch leaves the file open. Each `Если` is
        // a separate branch, so the lattice's MAY join correctly
        // reports the leak on the path through both else-branches.
        let code = r#"
            Процедура Тест(Условие1, Условие2)
                Если Условие1 Тогда
                    Файл = ПолучитьИмяВременногоФайла("xml");
                КонецЕсли;
                Если Условие2 Тогда
                    УдалитьФайлы(Файл);
                КонецЕсли;
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTemporaryFileDeletion @ 4:28..4:61
              message: Нужно добавить удаление временного файла 'Файл' после использования
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    #[ignore = "Move-method destination semantics: `ПереместитьФайл(src, dst)` leaves dst as a live temp file, but the lattice (matching pre-Step-Q legacy behavior) treats every variable referenced by any matching deletion-method arg as closed. Fix needs per-method arg-role metadata. Tracked as a follow-up beyond Track 1's scope; pinned here so the fix flips this test green."]
    fn test_move_method_destination_leaks() {
        // BSL `ПереместитьФайл(src, dst)` moves the file from `src`
        // to `dst`. The source path is freed (correctly closed); the
        // destination is now occupied and still needs explicit
        // cleanup. The pre-Step-Q body-wide BFS treated *both*
        // arguments as closed (any matching method × any var-ref =
        // delete); the Q-β-2 lattice preserves that legacy parity.
        let code = r#"
            Процедура Тест()
                Файл1 = ПолучитьИмяВременногоФайла("xml");
                Файл2 = ПолучитьИмяВременногоФайла("txt");
                ПереместитьФайл(Файл1, Файл2);
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "Move destination must remain open until explicit cleanup"
        );
        assert!(diagnostics[0].message.contains("Файл2"));
    }

    #[test]
    #[ignore = "Variable-generation tracking: a re-assignment to the same name overwrites the prior Get-site's runtime reachability, but on disk the prior temp file is still alive — leaked. The lattice keys resources on lowercased var name only, so the later delete clears both opening sites. Pre-Step-Q's body-wide BFS had the same blindspot; preserved as legacy parity. Fix requires reaching-definitions on the temp-name binding."]
    fn test_reassigned_variable_first_get_leaks() {
        // Two `Get*` calls assigned to the same name. At runtime the
        // first call's filename is overwritten by the second
        // assignment; the file at the first path is still on disk
        // when the procedure exits. The lattice treats the second
        // `УдалитьФайлы(Файл)` as closing both opening sites — both
        // share the lower_name `файл` resource key — and reports no
        // leak. Same blindspot as the sibling temp-storage handler.
        let code = r#"
            Процедура Тест()
                Файл = ПолучитьИмяВременногоФайла("a");
                Файл = ПолучитьИмяВременногоФайла("b");
                УдалитьФайлы(Файл);
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "First Get of a re-assigned name leaks");
    }

    #[test]
    fn test_ternary_deletion_arg_does_not_close_either_branch() {
        // Codex stop-time finding: a deletion call whose arg is a
        // Ternary (`?(cond, A, B)`) cannot statically be resolved
        // to a single closed resource — at runtime exactly one
        // branch evaluates, so under MAY semantics neither branch
        // is guaranteed to close. The pre-Step-Q `expr_contains_var`
        // walker did not recurse into Ternary at all (it fell into
        // its catch-all `_ => false` arm), so the body-wide BFS
        // already conservatively flagged both temps as leaks. This
        // test pins that legacy parity post-migration.
        let code = r#"
            Процедура Тест(Условие)
                Файл1 = ПолучитьИмяВременногоФайла("xml");
                Файл2 = ПолучитьИмяВременногоФайла("txt");
                УдалитьФайлы(?(Условие, Файл1, Файл2));
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTemporaryFileDeletion @ 3:25..3:58
              message: Нужно добавить удаление временного файла 'Файл1' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 4:25..4:58
              message: Нужно добавить удаление временного файла 'Файл2' после использования
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_inline_and_assigned_get_in_same_body_emit_separately() {
        // One inline Get and one tracked-but-unclosed Get in the
        // same body must each produce their own diagnostic with the
        // appropriate message shape.
        let code = r#"
            Процедура Тест()
                Файл = ПолучитьИмяВременногоФайла("xml");
                Записать(GetTempFileName("txt"));
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTemporaryFileDeletion @ 3:24..3:57
              message: Нужно добавить удаление временного файла 'Файл' после использования
              severity: Major
            MissingTemporaryFileDeletion @ 4:26..4:48
              message: Нужно добавить удаление временного файла после использования
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }
}
