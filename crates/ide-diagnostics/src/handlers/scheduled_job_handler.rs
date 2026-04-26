//! Validates metadata-backed scheduled job handlers from `SessionModule`.

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

/// Main entry point for ScheduledJobHandler diagnostic.
///
/// Validates scheduled job handlers in SessionModule.
///
/// ## Algorithm
///
/// 1. Early return if disabled or not SessionModule
/// 2. Load Configuration metadata via Salsa (cached!)
/// 3. Iterate all scheduled jobs
/// 4. For each job, perform validation checks
/// 5. Check for duplicate handlers
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ScheduledJobHandler;

    // 1. Check if disabled
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // 2. SessionModule-only scope
    if !is_session_module(ctx) {
        return Vec::new();
    }

    // 3. Load configuration metadata
    let configuration = match ctx.load_configuration() {
        Some(config) => config,
        None => return Vec::new(),
    };

    // 4. Process all scheduled jobs
    let mut diagnostics = Vec::new();
    let mut handler_usage: FxHashMap<String, Vec<String>> = FxHashMap::default();

    for job in configuration.scheduled_jobs() {
        check_scheduled_job(ctx, job, &configuration, code, &mut diagnostics, &mut handler_usage);
    }

    // 5. Check for duplicate handlers
    check_duplicate_handlers(ctx, &handler_usage, code, &mut diagnostics);

    diagnostics
}

/// Check if current file is SessionModule
fn is_session_module(ctx: &DiagnosticsContext) -> bool {
    // Get file path using ctx.file_path() (CRITICAL: bypasses Salsa for performance)
    let file_path = match ctx.file_path() {
        Some(path) => path,
        None => return false,
    };

    // SessionModule is at Configuration/Ext/SessionModule.bsl
    file_path.ends_with("/Ext/SessionModule.bsl") || file_path.ends_with("\\Ext\\SessionModule.bsl")
}

/// Validate single scheduled job
fn check_scheduled_job(
    ctx: &DiagnosticsContext,
    job: &ScheduledJob,
    configuration: &bsl_metadata::Configuration,
    code: DiagnosticCode,
    diagnostics: &mut Vec<Diagnostic>,
    handler_usage: &mut FxHashMap<String, Vec<String>>,
) {
    // CHECK 1: Empty handler
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

    // CHECK 2: Parse handler format
    let handler = match job.parse_handler() {
        Some(h) if h.method_name.is_empty() => {
            // Malformed: "CommonModule.ModuleName" (no method)
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
        None => return, // Invalid prefix, ignore
    };

    let full_handler_name = format!("{}.{}", handler.module_name, handler.method_name);

    // Track handler usage for duplicate detection
    handler_usage.entry(full_handler_name.clone()).or_default().push(job.name().to_string());

    // CHECK 3: CommonModule exists
    let common_module = match configuration.find_common_module(&handler.module_name) {
        Some(cm) => cm,
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

    // CHECK 4: CommonModule has Server flag
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

    // CHECK 5, 6, 7: Method exists, exported, and valid for predefined
    check_method(ctx, job, &handler, &full_handler_name, common_module, code, diagnostics);
}

/// Validate method exists, is exported, and has valid parameters
fn check_method(
    ctx: &DiagnosticsContext,
    job: &ScheduledJob,
    handler: &ScheduledJobHandler,
    full_handler_name: &str,
    common_module: &bsl_metadata::CommonModule,
    code: DiagnosticCode,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Resolve CommonModule file via VFS
    let module_file_id = match ctx.find_common_module_file(common_module) {
        Some(id) => id,
        None => return, // Module has no source code, skip method check
    };

    // Build SymbolTree for CommonModule
    let module_id = ModuleId::new(module_file_id);
    let symbol_tree = ctx.symbol_tree_for(module_id);

    // Lookup method
    let method_name_obj = Name::new(&handler.method_name);
    let method = symbol_tree.find_method(&method_name_obj);

    match method {
        None => {
            // CHECK 5: Method does not exist
            diagnostics.push(create_diagnostic(
                ctx,
                DiagnosticType::MissingMethod,
                job.name(),
                full_handler_name,
                code,
            ));
        }
        Some(m) => {
            // CHECK 6: Method not exported
            if !m.is_export {
                diagnostics.push(create_diagnostic(
                    ctx,
                    DiagnosticType::NonExportMethod,
                    job.name(),
                    full_handler_name,
                    code,
                ));
            }

            // CHECK 7: Predefined job with parameters
            if job.is_predefined() && !m.params.is_empty() {
                diagnostics.push(create_diagnostic(
                    ctx,
                    DiagnosticType::MethodWithParameters,
                    job.name(),
                    full_handler_name,
                    code,
                ));
            }

            // CHECK 8: Empty method body
            if is_empty_method(ctx, module_id, m.id.local_id) {
                diagnostics.push(create_diagnostic(
                    ctx,
                    DiagnosticType::EmptyMethod,
                    job.name(),
                    full_handler_name,
                    code,
                ));
            }
        }
    }
}

/// Check if method body is empty (no variables and no statements)
fn is_empty_method(ctx: &DiagnosticsContext, module_id: ModuleId, local_id: u32) -> bool {
    // Get module bodies for the common module (not current file!)
    let bodies = ctx.module_bodies_for(module_id);
    let Some(body) = bodies.body(local_id) else {
        return false;
    };

    // Method is empty if it has no bindings and no statements
    // For scheduled job handlers, there should be at least some code
    body.binding_count() == 0 && body.stmt_count() == 0
}

/// Check for duplicate handler usage
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

/// Diagnostic type for different validation failures
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

/// Create diagnostic with Russian error message
///
/// All diagnostics are reported at the SessionModule start (line 1, columns 1-9): Range(0, 0, 9)
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
            // For duplicates: job_name contains jobs list, detail contains handler name
            format!(
                "Исправьте дубли использования одного обработчика \"{}\" в разных регламентных заданиях. Задания: \"{}\"",
                detail, job_name
            )
        }
    };

    // Get file text to determine safe range
    let file_text = ctx.file_text();
    let file_len = file_text.len();

    // Use range [0, min(9, file_len)): Range(0, 0, 9)
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
    use crate::test_utils::assert_diagnostic_range;
    use crate::{DiagnosticsConfig, Severity};
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::path::PathBuf;
    use vfs::{FileId, FileSet, VfsPath};
    fn check_diagnostic(code: &str, fixtures_dir: &str) -> (Vec<Diagnostic>, String) {
        // Setup database with VFS
        let mut db = RootDatabaseImpl::new();

        // Create VFS
        let workspace_root = PathBuf::from(fixtures_dir);

        // Create FileSet with SessionModule and required CommonModules
        let mut file_set = FileSet::default();

        // SessionModule file (file_id 0)
        let file_id = FileId(0);
        let session_module_path = VfsPath::new(format!("{}/Ext/SessionModule.bsl", fixtures_dir));
        file_set.insert(file_id, session_module_path);

        // Add ПервыйОбщийМодуль (needed for method validation tests)
        let common_module_file_id = FileId(1);
        let common_module_path = VfsPath::new(format!(
            "{}/CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl",
            fixtures_dir
        ));
        file_set.insert(common_module_file_id, common_module_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        // Set up database
        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_source_root(common_module_file_id, source_root_id);
        db.set_file_text(file_id, code);

        // Load the CommonModule code
        let common_module_code = std::fs::read_to_string(format!(
            "{}/CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl",
            fixtures_dir
        ))
        .unwrap_or_default();
        db.set_file_text(common_module_file_id, &common_module_code);

        // Set workspace root via Salsa
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

        // Dummy code for SessionModule
        // Use ASCII at start for correct byte range check (9 bytes = 9 chars for ASCII)
        let code = "Procedure Test()\nEndProcedure";
        let (diagnostics, file_content) = check_diagnostic(code, fixtures_dir);

        // Should find 6 diagnostics total:
        // 1. Missing method: НесуществующийМетод
        // 2. Non-export: Тест
        // 3. Empty body: НеУстаревшаяПроцедура (for РегламентноеЗадание1)
        // 4. Empty body: НеУстаревшаяПроцедура (for РегламентноеЗадание2)
        // 5. Duplicate handler: НеУстаревшаяПроцедура used twice
        // 6. Method with parameters: ВерсионированиеПриЗаписи for predefined job
        assert_eq!(diagnostics.len(), 6, "Expected 6 diagnostics, found {}", diagnostics.len());

        // Check that all diagnostics have correct range (0, 0, 9) - byte range
        for diagnostic in diagnostics.iter() {
            assert_diagnostic_range(&file_content, diagnostic, 0, 0, 9);
            assert_eq!(diagnostic.severity, Severity::Critical);
        }

        // Verify specific messages (order-independent)
        let messages: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();

        // Check for specific error messages
        assert!(
            messages.iter().any(|m| m.contains("НесуществующийМетод")),
            "Should have MissingMethod diagnostic for НесуществующийМетод"
        );
        assert!(
            messages.iter().any(|m| m.contains("Тест") && m.contains("Экспорт")),
            "Should have NonExportMethod diagnostic for Тест"
        );
        assert!(
            messages.iter().any(|m| m.contains("НеУстаревшаяПроцедура") && m.contains("тело")),
            "Should have EmptyMethod diagnostic"
        );
        assert!(
            messages.iter().any(|m| m.contains("дубли")),
            "Should have DuplicateHandler diagnostic"
        );
        assert!(
            messages.iter().any(|m| m.contains("параметров")),
            "Should have MethodWithParameters diagnostic"
        );
    }

    #[test]
    fn test_not_session_module() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        // Setup database with VFS
        let mut db = RootDatabaseImpl::new();

        // Create VFS for a non-SessionModule file (CommonModule)
        let vfs_path = VfsPath::new(format!("{}/CommonModules/Test/Ext/Module.bsl", fixtures_dir));

        // Create FileSet and SourceRoot
        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        file_set.insert(file_id, vfs_path.clone());

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        // Set up database
        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, "Процедура Тест()\nКонецПроцедуры");

        let provider = ide_db::SalsaProvider::new(&db, None);
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);

        // Should return empty for non-SessionModule
        assert_eq!(diagnostics.len(), 0, "Non-SessionModule should have no diagnostics");
    }
}
