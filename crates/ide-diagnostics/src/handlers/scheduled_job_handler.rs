use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::{ScheduledJob, ScheduledJobHandler};
use hir::{ModuleId, Name};
use ide_db::TextRange;
use rustc_hash::FxHashMap;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: crate::metadata::DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: true,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ScheduledJobHandler;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    if !is_session_module(ctx) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let mut handler_usage: FxHashMap<String, Vec<String>> = FxHashMap::default();

    for job in ctx.main_scheduled_jobs() {
        check_scheduled_job(ctx, &job, code, &mut diagnostics, &mut handler_usage);
    }

    check_duplicate_handlers(ctx, &handler_usage, code, &mut diagnostics);

    diagnostics
}

fn is_session_module(ctx: &DiagnosticsContext) -> bool {
    let file_path = match ctx.file_path() {
        Some(path) => path,
        None => return false,
    };

    file_path.ends_with("/Ext/SessionModule.bsl") || file_path.ends_with("\\Ext\\SessionModule.bsl")
}

fn check_scheduled_job(
    ctx: &DiagnosticsContext,
    job: &ScheduledJob,
    code: DiagnosticCode,
    diagnostics: &mut Vec<Diagnostic>,
    handler_usage: &mut FxHashMap<String, Vec<String>>,
) {
    if job.method_name().is_empty() {
        diagnostics.push(create_diagnostic(
            ctx,
            DiagnosticType::EmptyHandler,
            job.name(),
            "",
            code,
        ));
        return;
    }

    let handler = match job.parse_handler() {
        Some(h) if h.method_name.is_empty() => {
            diagnostics.push(create_diagnostic(
                ctx,
                DiagnosticType::MissingMethod,
                job.name(),
                job.method_name(),
                code,
            ));
            return;
        }
        Some(h) => h,
        None => return,
    };

    let full_handler_name = format!("{}.{}", handler.module_name, handler.method_name);

    handler_usage.entry(full_handler_name.clone()).or_default().push(job.name().to_string());

    let common_module = match ctx.resolve_common_module(&handler.module_name) {
        Some(found) => found,
        None => {
            diagnostics.push(create_diagnostic(
                ctx,
                DiagnosticType::MissingModule,
                job.name(),
                &handler.module_name,
                code,
            ));
            return;
        }
    };

    if !common_module.is_server() {
        diagnostics.push(create_diagnostic(
            ctx,
            DiagnosticType::NonServerModule,
            job.name(),
            &handler.module_name,
            code,
        ));
        return;
    }

    check_method(ctx, job, &handler, &full_handler_name, code, diagnostics);
}

fn check_method(
    ctx: &DiagnosticsContext,
    job: &ScheduledJob,
    handler: &ScheduledJobHandler,
    full_handler_name: &str,
    code: DiagnosticCode,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let module_files = ctx.common_module_body_files(&handler.module_name);
    if module_files.is_empty() {
        return;
    }

    let method_name_obj = Name::new(&handler.method_name);

    struct Resolved {
        module_id: ModuleId,
        local_id: u32,
        is_export: bool,
        params_empty: bool,
    }
    let mut resolved: Option<Resolved> = None;

    for module_file_id in module_files {
        let module_id = ModuleId::new(module_file_id);
        let symbol_tree = ctx.symbol_tree_for(module_id);
        let Some(method) = symbol_tree.find_method(&method_name_obj) else {
            continue;
        };
        let candidate = Resolved {
            module_id,
            local_id: method.id.local_id,
            is_export: method.is_export,
            params_empty: method.params.is_empty(),
        };
        if candidate.is_export {
            resolved = Some(candidate);
            break;
        }
        if resolved.is_none() {
            resolved = Some(candidate);
        }
    }

    let Some(resolved) = resolved else {
        diagnostics.push(create_diagnostic(
            ctx,
            DiagnosticType::MissingMethod,
            job.name(),
            full_handler_name,
            code,
        ));
        return;
    };

    if !resolved.is_export {
        diagnostics.push(create_diagnostic(
            ctx,
            DiagnosticType::NonExportMethod,
            job.name(),
            full_handler_name,
            code,
        ));
    }

    if job.is_predefined() && !resolved.params_empty {
        diagnostics.push(create_diagnostic(
            ctx,
            DiagnosticType::MethodWithParameters,
            job.name(),
            full_handler_name,
            code,
        ));
    }

    if is_empty_method(ctx, resolved.module_id, resolved.local_id) {
        diagnostics.push(create_diagnostic(
            ctx,
            DiagnosticType::EmptyMethod,
            job.name(),
            full_handler_name,
            code,
        ));
    }
}

fn is_empty_method(ctx: &DiagnosticsContext, module_id: ModuleId, local_id: u32) -> bool {
    let bodies = ctx.module_bodies_for(module_id);
    let Some(body) = bodies.body(local_id) else {
        return false;
    };

    body.binding_count() == 0 && body.stmt_count() == 0
}

fn check_duplicate_handlers(
    ctx: &DiagnosticsContext,
    handler_usage: &FxHashMap<String, Vec<String>>,
    code: DiagnosticCode,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (handler_name, job_names) in handler_usage {
        if job_names.len() > 1 {
            let mut sorted_names = job_names.clone();
            sorted_names.sort();
            let jobs_list = sorted_names.join(", ");
            diagnostics.push(create_diagnostic(
                ctx,
                DiagnosticType::DuplicateHandler,
                &jobs_list,
                handler_name,
                code,
            ));
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum DiagnosticType {
    EmptyHandler,
    MissingModule,
    NonServerModule,
    MissingMethod,
    NonExportMethod,
    MethodWithParameters,
    EmptyMethod,
    DuplicateHandler,
}

fn create_diagnostic(
    ctx: &DiagnosticsContext,
    diagnostic_type: DiagnosticType,
    job_name: &str,
    detail: &str,
    code: DiagnosticCode,
) -> Diagnostic {
    let message = match diagnostic_type {
        DiagnosticType::EmptyHandler => {
            format!(
                "Укажите существующий обработчик вместо несуществующего \"\" у регламентного задания \"{}\"",
                job_name
            )
        }
        DiagnosticType::MissingModule => {
            format!(
                "Создайте общий модуль \"{}\" или исправьте некорректный обработчик регламентного задания \"{}\"",
                detail, job_name
            )
        }
        DiagnosticType::NonServerModule => {
            format!(
                "Установите флаг \"Сервер\" общему модулю \"{}\" или исправьте некорректный обработчик регламентного задания \"{}\"",
                detail, job_name
            )
        }
        DiagnosticType::MissingMethod => {
            format!(
                "Укажите существующий обработчик вместо несуществующего \"{}\" у регламентного задания \"{}\"",
                detail, job_name
            )
        }
        DiagnosticType::NonExportMethod => {
            format!(
                "Добавьте \"Экспорт\" методу \"{}\" или исправьте некорректный обработчик регламентного задания \"{}\"",
                detail, job_name
            )
        }
        DiagnosticType::MethodWithParameters => {
            format!(
                "Исправьте некорректный обработчик \"{}\" предопределенного регламентного задания \"{}\" - у метода не должно быть параметров",
                detail, job_name
            )
        }
        DiagnosticType::EmptyMethod => {
            format!(
                "Добавьте код в тело обработчика \"{}\" регламентного задания \"{}\"",
                detail, job_name
            )
        }
        DiagnosticType::DuplicateHandler => {
            format!(
                "Исправьте дубли использования одного обработчика \"{}\" в разных регламентных заданиях. Задания: \"{}\"",
                detail, job_name
            )
        }
    };

    let file_text = ctx.file_text();
    let file_len = file_text.len();

    let end_offset = std::cmp::min(9, file_len);
    let range = TextRange::new(0.into(), (end_offset as u32).into());

    Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::format_diags;
    use crate::DiagnosticsConfig;
    use expect_test::expect;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::path::PathBuf;
    use vfs::{FileId, FileSet, VfsPath};
    fn check_diagnostic(code: &str, fixtures_dir: &str) -> (Vec<Diagnostic>, String) {
        let mut db = RootDatabaseImpl::new();

        let workspace_root = PathBuf::from(fixtures_dir);

        let mut file_set = FileSet::default();

        let file_id = FileId(0);
        let session_module_path = VfsPath::new(format!("{}/Ext/SessionModule.bsl", fixtures_dir));
        file_set.insert(file_id, session_module_path);

        let common_module_file_id = FileId(1);
        let common_module_path = VfsPath::new(format!(
            "{}/CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl",
            fixtures_dir
        ));
        file_set.insert(common_module_file_id, common_module_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_source_root(common_module_file_id, source_root_id);
        db.set_file_text(file_id, code);

        let common_module_code = std::fs::read_to_string(format!(
            "{}/CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl",
            fixtures_dir
        ))
        .unwrap_or_default();
        db.set_file_text(common_module_file_id, &common_module_code);

        let configuration_path_input = ide_db::metadata::ConfigurationPathInput::new(
            &db,
            workspace_root.to_string_lossy().to_string(),
            0,
        );

        let provider = ide_db::SalsaProvider::new(&db, Some(configuration_path_input));
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        (diagnostics, code.to_string())
    }

    #[test]
    fn test_scheduled_job_handler() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let code = "Procedure Test()\nEndProcedure";
        let (diagnostics, file_content) = check_diagnostic(code, fixtures_dir);

        expect![[r#"
            ScheduledJobHandler @ 1:1..1:10
              message: Добавьте "Экспорт" методу "ПервыйОбщийМодуль.Тест" или исправьте некорректный обработчик регламентного задания "РегламентноеЗаданиеПриватныйМетод"
              severity: Critical
            ScheduledJobHandler @ 1:1..1:10
              message: Добавьте код в тело обработчика "ПервыйОбщийМодуль.НеУстаревшаяПроцедура" регламентного задания "РегламентноеЗадание1"
              severity: Critical
            ScheduledJobHandler @ 1:1..1:10
              message: Добавьте код в тело обработчика "ПервыйОбщийМодуль.НеУстаревшаяПроцедура" регламентного задания "РегламентноеЗадание2"
              severity: Critical
            ScheduledJobHandler @ 1:1..1:10
              message: Исправьте дубли использования одного обработчика "ПервыйОбщийМодуль.НеУстаревшаяПроцедура" в разных регламентных заданиях. Задания: "РегламентноеЗадание1, РегламентноеЗадание2"
              severity: Critical
            ScheduledJobHandler @ 1:1..1:10
              message: Исправьте некорректный обработчик "ПервыйОбщийМодуль.ВерсионированиеПриЗаписи" предопределенного регламентного задания "РегламентноеЗаданиеПредопределенноеНесколькоПараметров" - у метода не должно быть параметров
              severity: Critical
            ScheduledJobHandler @ 1:1..1:10
              message: Укажите существующий обработчик вместо несуществующего "ПервыйОбщийМодуль.НесуществующийМетод" у регламентного задания "РегламентноеЗаданиеНесуществующийМетод"
              severity: Critical"#]].assert_eq(&format_diags(&file_content, &diagnostics));
    }

    #[test]
    fn test_scheduled_job_handler_uses_main_scheduled_jobs_surface() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let mut db = RootDatabaseImpl::new();

        let vfs_path = VfsPath::new(format!("{}/Ext/SessionModule.bsl", fixtures_dir));

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        file_set.insert(file_id, vfs_path.clone());

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, "Процедура Тест()\nКонецПроцедуры");

        let provider = ide_db::SalsaProvider::new(&db, None);
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let job_names: Vec<_> =
            ctx.main_scheduled_jobs().into_iter().map(|job| job.name().to_string()).collect();

        assert!(
            job_names.contains(&"РегламентноеЗадание1".to_string()),
            "main_scheduled_jobs must enumerate scheduled jobs from the main config"
        );
    }

    #[test]
    fn test_not_session_module() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let mut db = RootDatabaseImpl::new();

        let vfs_path = VfsPath::new(format!("{}/CommonModules/Test/Ext/Module.bsl", fixtures_dir));

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        file_set.insert(file_id, vfs_path.clone());

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, "Процедура Тест()\nКонецПроцедуры");

        let provider = ide_db::SalsaProvider::new(&db, None);
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);

        expect![[r#""#]].assert_eq(&format_diags("Процедура Тест()\nКонецПроцедуры", &diagnostics));
    }
}
