use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Expr, ExprId, IdConversion, Literal, MethodId, ModuleId, Stmt, StmtId};
use ide_db::TextRange;
use stdx::case::CaseExt;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Suspicious, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

fn has_str_template_calls(text: &str) -> bool {
    const PATTERNS: &[&str] = &["стршаблон", "strtemplate"];

    let text_lower = text.fold_lower();
    for pattern in PATTERNS {
        if text_lower.contains(pattern) {
            return true;
        }
    }
    false
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::IncorrectUseOfStrTemplate;

    if ctx.is_disabled_with_metadata(code) {
        return vec![];
    }

    let text = ctx.file_text();
    if !has_str_template_calls(&text) {
        return vec![];
    }

    let mut diagnostics = Vec::new();
    let module_id = ModuleId { file_id: ctx.file_id };

    let module_bodies = ctx.module_bodies();

    for (local_id, body, source_map) in module_bodies.method_bodies() {
        let mut candidates: Vec<(StmtId, ExprId, usize)> = Vec::new();

        for (stmt_id, stmt) in body.stmts_iter() {
            let expr_id = match stmt {
                Stmt::Expr(id) => Some(*id),
                Stmt::Assign { value, .. } => Some(*value),
                _ => None,
            };

            if let Some(expr_id) = expr_id {
                let expr = body.expr(ExprId::from_idx(expr_id));

                let (method_name, args) = match expr {
                    Expr::Call { callee, args } => {
                        if let Expr::Path(name) = body.expr(ExprId::from_idx(*callee)) {
                            (name.as_str().fold_lower(), args)
                        } else {
                            continue;
                        }
                    }
                    Expr::MethodCall { method, args, .. } => (method.as_str().fold_lower(), args),
                    _ => continue,
                };

                if !matches!(method_name.as_str(), "strtemplate" | "стршаблон") {
                    continue;
                }

                if args.is_empty() {
                    continue;
                }

                let template_expr_id = ExprId::from_idx(args[0]);
                let param_count = args.len() - 1;

                if matches!(body.expr(template_expr_id), Expr::Literal(Literal::String(_))) {
                    continue;
                }

                candidates.push((stmt_id, template_expr_id, param_count));
            }
        }

        if candidates.is_empty() {
            continue;
        }

        let method_id = MethodId { module: module_id, local_id };
        let reaching_defs = match ctx.reaching_definitions(method_id) {
            Some(defs) => defs,
            None => continue,
        };

        for (stmt_id, template_expr_id, param_count) in candidates {
            if let Some(template_string) =
                resolve_expr_to_string(template_expr_id, body, &reaching_defs, stmt_id)
            {
                if is_wrong_str_template(&template_string, param_count) {
                    if let Some(range) = source_map.expr_range(template_expr_id) {
                        diagnostics.push(Diagnostic {
                            code,
                            message: format!(
                                "Template '{}' requires {} parameters but {} provided",
                                template_string.chars().take(50).collect::<String>(),
                                count_required_params(&template_string),
                                param_count
                            ),
                            severity: ctx.severity(code),
                            range,
                            tags: ctx.tags(code),
                            fixes: vec![],
                        });
                    }
                }
            }
        }
    }

    diagnostics
}

fn resolve_expr_to_string(
    expr_id: ExprId,
    body: &hir::Body,
    reaching_defs: &hir::dataflow::reaching_defs::ReachingDefsResult,
    stmt_id: StmtId,
) -> Option<String> {
    resolve_expr_to_string_impl(expr_id, body, reaching_defs, stmt_id, 0)
}

fn resolve_expr_to_string_impl(
    expr_id: ExprId,
    body: &hir::Body,
    reaching_defs: &hir::dataflow::reaching_defs::ReachingDefsResult,
    stmt_id: StmtId,
    depth: u32,
) -> Option<String> {
    const MAX_DEPTH: u32 = 10;

    if depth > MAX_DEPTH {
        return None;
    }

    match body.expr(expr_id) {
        Expr::Literal(Literal::String(s)) => Some(s.to_string()),

        Expr::Path(var_name) => {
            let defs = reaching_defs.defs_for_var_at_stmt(var_name.as_str(), stmt_id)?;

            let mut resolved_values = std::collections::HashSet::new();

            for def in defs {
                if let Some(value) =
                    resolve_definition(&def, body, reaching_defs, stmt_id, depth + 1)
                {
                    resolved_values.insert(value);
                }
            }

            if resolved_values.len() == 1 {
                resolved_values.into_iter().next()
            } else {
                None
            }
        }

        _ => None,
    }
}

fn resolve_definition(
    def: &hir::dataflow::reaching_defs::Definition,
    body: &hir::Body,
    reaching_defs: &hir::dataflow::reaching_defs::ReachingDefsResult,
    _current_stmt: StmtId,
    depth: u32,
) -> Option<String> {
    match def.def_site {
        hir::dataflow::reaching_defs::DefSite::Assignment(assign_raw_idx) => {
            let assign_stmt_id = StmtId::from_raw(assign_raw_idx);

            if let Stmt::Assign { value, .. } = body.stmt(assign_stmt_id) {
                resolve_expr_to_string_impl(
                    ExprId::from_idx(*value),
                    body,
                    reaching_defs,
                    assign_stmt_id,
                    depth,
                )
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_wrong_str_template(template_string: &str, used_params_count: usize) -> bool {
    let is_wrong_call = compare_template_and_params(template_string, used_params_count);
    if !is_wrong_call {
        return false;
    }

    let cleaned = remove_double_percent(template_string);
    compare_template_and_params(&cleaned, used_params_count)
}

fn remove_double_percent(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'%' && bytes[i + 1] == b'%' {
            i += 2;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

fn parse_placeholder(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    if pos >= bytes.len() || bytes[pos] != b'%' {
        return None;
    }

    let start = pos + 1;
    if start >= bytes.len() {
        return None;
    }

    if bytes[start] == b'(' {
        let num_start = start + 1;
        let mut num_end = num_start;
        while num_end < bytes.len() && bytes[num_end].is_ascii_digit() {
            num_end += 1;
        }
        if num_end > num_start && num_end < bytes.len() && bytes[num_end] == b')' {
            let num_str = std::str::from_utf8(&bytes[num_start..num_end]).ok()?;
            let num: usize = num_str.parse().ok()?;
            return Some((num, num_end - pos + 1));
        }
        return None;
    }

    let mut num_end = start;
    while num_end < bytes.len() && bytes[num_end].is_ascii_digit() {
        num_end += 1;
    }
    if num_end > start {
        let num_str = std::str::from_utf8(&bytes[start..num_end]).ok()?;
        let num: usize = num_str.parse().ok()?;
        return Some((num, num_end - pos));
    }

    None
}

fn is_valid_placeholder(num: usize) -> bool {
    (1..=10).contains(&num)
}

#[allow(clippy::nonminimal_bool)]
fn compare_template_and_params(template_string: &str, used_params_count: usize) -> bool {
    let bytes = template_string.as_bytes();
    let have_params = used_params_count > 0;

    let mut has_valid_placeholder = false;
    let mut has_wrong_number = false;
    let mut used_placeholders = [false; 11];

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'%' {
                i += 2;
                continue;
            }

            if let Some((num, len)) = parse_placeholder(bytes, i) {
                if is_valid_placeholder(num) {
                    has_valid_placeholder = true;
                    used_placeholders[num] = true;
                    if num > used_params_count {
                        return true;
                    }
                } else {
                    has_wrong_number = true;
                }
                i += len;
                continue;
            }
        }
        i += 1;
    }

    if has_wrong_number {
        return true;
    }
    if has_valid_placeholder && !have_params {
        return true;
    }
    if !has_valid_placeholder && have_params {
        return true;
    }

    if has_valid_placeholder {
        for &used in used_placeholders.iter().take(used_params_count + 1).skip(1) {
            if !used {
                return true;
            }
        }
    }

    false
}

fn count_required_params(template_string: &str) -> usize {
    let bytes = template_string.as_bytes();
    let mut max_param = 0;

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'%' {
                i += 2;
                continue;
            }

            if let Some((num, len)) = parse_placeholder(bytes, i) {
                if is_valid_placeholder(num) {
                    max_param = max_param.max(num);
                }
                i += len;
                continue;
            }
        }
        i += 1;
    }

    max_param
}

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::IncorrectUseOfStrTemplate;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Некорректное использование СтрШаблон".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_hir_diagnostic, format_diags};
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_correct_usage() {
        let code = r#"
Процедура Тест()
    Г = СтрШаблон("Наименование (версия %1)", Версия());
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let filtered: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &filtered));
    }

    #[test]
    fn test_missing_parameter() {
        let code = r#"
Процедура Тест()
    А = СтрШаблон("Наименование (версия %1)");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let filtered: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        expect![[r#"
            IncorrectUseOfStrTemplate @ 3:9..3:46
              message: Некорректное использование СтрШаблон
              severity: Blocker"#]]
        .assert_eq(&format_diags(code, &filtered));
    }

    #[test]
    fn test_insufficient_arguments() {
        let code = r#"
Процедура Тест()
    Б = СтрШаблон("%1 (версия %2)", Наименование);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let filtered: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        expect![[r#"
            IncorrectUseOfStrTemplate @ 3:9..3:50
              message: Некорректное использование СтрШаблон
              severity: Blocker"#]]
        .assert_eq(&format_diags(code, &filtered));
    }

    #[test]
    fn test_comprehensive() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"Процедура Метод()

    А = СтрШаблон("Наименование (версия %1)"); // ошибка

    Б = СтрШаблон("%1 (версия %2)", Наименование); // ошибка

    К = СтрШаблон("Наименование %11", Наименование); // ошибка

    К = СтрШаблон("Наименование %0", Наименование); // ошибка

    Ж = СтрШаблон("Наименование %2 (версия %%3)", Наименование, Версия); // ошибка

    //здесь ошибочно не закрыта скобка для НСтр
    В = СтрШаблон(НСтр("ru='Наименование (версия %1)'", Версия())); // ошибка

    НовыйШаблон = "123";
    Н = СтрШаблон(НовыйШаблон, Наименование); // ошибка

    НовыйШаблон1 = "123";
    ДругаяСтрока = "5487";
    Н = СтрШаблон(НовыйШаблон1, Наименование); // ошибка

    //НовыйШаблон2 = НСтр("ru='Наименование (версия)'";
    НовыйШаблон2 = "5487";
    Н = СтрШаблон(НовыйШаблон2, Наименование); // ошибка

    // ошибка
    С24 = СтрШаблон("%1, %2, %3, %4, %5, %6, %7, %8, %9, %10, %11", "ф", "ф", "ф", "ф", "ф", "ф", "ф", "ф", "Ф", "");

    Л = СтрШаблон("Наименование %(1)"); // ошибка

    Г = СтрШаблон(НСтр("ru='Наименование (версия %1)'"), Версия());

    Д = СтрШаблон("Наименование (версия)");

    Е = СтрШаблон("Наименование (версия %1)", Наименование);

    Е = СтрШаблон("Наименование %1 (версия %2)", Наименование, Версия);

    З = СтрШаблон("Наименование %%1 (версия %%2)");
    Ий = СтрШаблон("Наименование %1 (версия %%2)", Наименование);

    Л = СтрШаблон("Наименование %(1)1", Наименование); // в СП разрешен такой вариант

    М = СтрШаблон(ШаблонНаименования, Наименование);
    М = СтрШаблон("123" + ШаблонНаименования, Наименование);

    НовыйШаблон3 = "%1";
    Н = СтрШаблон(НовыйШаблон3, Наименование);

    А = СтрШаблон("%(1)%(2)", "Первая", 2);

    Б = СтрШаблон("%%%1%%", "Первая");

    Объект.НовыйШаблон4 = "%1"; // новый код
    Н = СтрШаблон(Объект.НовыйШаблон4, Наименование);

    Объект.НовыйШаблон5Ошибка = "%1 %2"; // падает на этой строчке, что верно
    Н = СтрШаблон(Объект.НовыйШаблон5Ошибка, Наименование);
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = crate::DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = crate::diagnostics(&ctx);

        let filtered: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();

        expect![[r#"
            IncorrectUseOfStrTemplate @ 3:9..3:46
              message: Некорректное использование СтрШаблон
              severity: Blocker
            IncorrectUseOfStrTemplate @ 5:9..5:50
              message: Некорректное использование СтрШаблон
              severity: Blocker
            IncorrectUseOfStrTemplate @ 7:9..7:52
              message: Некорректное использование СтрШаблон
              severity: Blocker
            IncorrectUseOfStrTemplate @ 9:9..9:51
              message: Некорректное использование СтрШаблон
              severity: Blocker
            IncorrectUseOfStrTemplate @ 11:9..11:72
              message: Некорректное использование СтрШаблон
              severity: Blocker
            IncorrectUseOfStrTemplate @ 14:9..14:67
              message: Некорректное использование СтрШаблон
              severity: Blocker
            IncorrectUseOfStrTemplate @ 17:19..17:30
              message: Template '123' requires 0 parameters but 1 provided
              severity: Blocker
            IncorrectUseOfStrTemplate @ 21:19..21:31
              message: Template '123' requires 0 parameters but 1 provided
              severity: Blocker
            IncorrectUseOfStrTemplate @ 25:19..25:31
              message: Template '5487' requires 0 parameters but 1 provided
              severity: Blocker
            IncorrectUseOfStrTemplate @ 28:11..28:117
              message: Некорректное использование СтрШаблон
              severity: Blocker
            IncorrectUseOfStrTemplate @ 30:9..30:39
              message: Некорректное использование СтрШаблон
              severity: Blocker
            IncorrectUseOfStrTemplate @ 46:9..46:60
              message: Некорректное использование СтрШаблон
              severity: Blocker"#]]
        .assert_eq(&format_diags(code, &filtered));
    }

    #[test]
    fn test_variable_resolution_simple() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    НовыйШаблон = "123";
    А = СтрШаблон(НовыйШаблон, Наименование); // ошибка: "123" не содержит %1
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = crate::DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        expect![[r#"
            IncorrectUseOfStrTemplate @ 4:19..4:30
              message: Template '123' requires 0 parameters but 1 provided
              severity: Blocker"#]]
        .assert_eq(&format_diags(code, &filtered));
    }

    #[test]
    fn test_variable_resolution_with_template() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    НовыйШаблон = "%1";
    А = СтрШаблон(НовыйШаблон, Наименование); // OK
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = crate::DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &filtered));
    }

    #[test]
    fn test_variable_resolution_conditional() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    Если Условие Тогда
        Шаблон = "123";
    Иначе
        Шаблон = "%1";
    КонецЕсли;
    А = СтрШаблон(Шаблон, Наименование);
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = crate::DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &filtered));
    }

    #[test]
    fn test_transitive_assignment() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    Шаблон1 = "template %1";
    Шаблон2 = Шаблон1;
    А = СтрШаблон(Шаблон2, Наименование); // OK - resolves through transitive assignment
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = crate::DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &filtered));
    }

    #[test]
    fn test_transitive_assignment_error() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    Шаблон1 = "no placeholders";
    Шаблон2 = Шаблон1;
    А = СтрШаблон(Шаблон2, Наименование); // Error - resolves to "no placeholders"
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = crate::DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        expect![[r#"
            IncorrectUseOfStrTemplate @ 5:19..5:26
              message: Template 'no placeholders' requires 0 parameters but 1 provided
              severity: Blocker"#]]
        .assert_eq(&format_diags(code, &filtered));
    }

    #[test]
    fn test_deep_transitive_chain() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    Ш1 = "template %1";
    Ш2 = Ш1;
    Ш3 = Ш2;
    Ш4 = Ш3;
    А = СтрШаблон(Ш4, Наименование); // OK - resolves through chain
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = crate::DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &filtered));
    }

    #[test]
    fn test_multiple_defs_same_value() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;
        let code = r#"
Процедура Тест()
    Если Условие Тогда
        Шаблон = "template %1";
    Иначе
        Шаблон = "template %1";  // Same value as then branch
    КонецЕсли;
    А = СтрШаблон(Шаблон, Наименование); // OK - both branches give same value
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let provider = ide_db::SalsaProvider::new(db.as_ref(), None);
        let ctx = crate::DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &filtered));
    }
}
