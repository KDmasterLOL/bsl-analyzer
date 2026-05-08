//! Reports invalid event subscription handlers declared in configuration metadata.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::{EventSubscription, EventSubscriptionHandler};
use hir::{ModuleId, Name};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: crate::metadata::DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::SessionModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Main entry point for event subscription handler validation.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MissingEventSubscriptionHandler;

    // 1. Check if disabled
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // 2. SessionModule-only scope
    if !is_session_module(ctx) {
        return Vec::new();
    }

    // 3. Load main configuration — `event_subscriptions()` is a main-only
    // collection (CFEs cannot declare new event subscriptions), so we
    // iterate the main config; the per-handler CommonModule lookup goes
    // through `is_common_module_anywhere` / `find_common_module_anywhere`
    // because handlers may resolve to a CommonModule defined in a CFE.
    let configuration = match ctx.main_configuration() {
        Some(config) => config,
        None => return Vec::new(),
    };

    // 4. Process all event subscriptions
    let mut diagnostics = Vec::new();
    for event_sub in configuration.event_subscriptions() {
        check_event_subscription(ctx, event_sub, code, &mut diagnostics);
    }

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

/// Validate single event subscription
fn check_event_subscription(
    ctx: &DiagnosticsContext,
    event_sub: &EventSubscription,
    code: DiagnosticCode,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // CHECK 1: Empty handler
    if event_sub.handler_string().is_empty() {
        diagnostics.push(create_diagnostic(
            ctx,
            DiagnosticType::EmptyHandler,
            event_sub.name(),
            "",
            code,
        ));
        return;
    }

    // CHECK 2: Parse handler format
    let handler = match event_sub.parse_handler() {
        Some(h) if h.method_name.is_empty() => {
            // Malformed: "CommonModule.ModuleName" (no method)
            diagnostics.push(create_diagnostic(
                ctx,
                DiagnosticType::IncorrectFormat,
                event_sub.name(),
                event_sub.handler_string(),
                code,
            ));
            return;
        }
        Some(h) => h,
        None => return, // Invalid prefix, ignore
    };

    // CHECK 3: CommonModule exists somewhere (main or CFE).
    let (_visible, common_module) = match ctx.find_common_module_anywhere(&handler.module_name) {
        Some(found) => found,
        None => {
            diagnostics.push(create_diagnostic(
                ctx,
                DiagnosticType::MissingModule,
                event_sub.name(),
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
            DiagnosticType::ShouldBeServer,
            event_sub.name(),
            &handler.module_name,
            code,
        ));
        // Continue - check method anyway (may report multiple issues)
    }

    // CHECK 5 & 6: Method exists and exported
    check_method(ctx, event_sub, &handler, code, diagnostics);
}

/// Validate method exists and is exported across CFE-unioned defining files.
///
/// 1C extension semantics treat same-name CommonModules across main + CFE
/// as one logical module whose methods are unioned across all defining
/// files. A handler resolved here may be defined in any one of them, so
/// we iterate every defining file via `find_common_module_files_anywhere`
/// and accept the first exported match. Only when **no** defining file
/// has the method (or every file has it as non-export) do we emit a
/// diagnostic — this is the same posture as
/// `missed_required_parameter::check_qualified_call`.
fn check_method(
    ctx: &DiagnosticsContext,
    event_sub: &EventSubscription,
    handler: &EventSubscriptionHandler,
    code: DiagnosticCode,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let module_files = ctx.find_common_module_files_anywhere(&handler.module_name);
    if module_files.is_empty() {
        // No source code in any visible configuration — skip method check.
        return;
    }

    let method_name_obj = Name::new(&handler.method_name);
    let mut saw_non_export = false;

    for module_file_id in module_files {
        let module_id = ModuleId::new(module_file_id);
        let symbol_tree = ctx.symbol_tree_for(module_id);
        let Some(method) = symbol_tree.find_method(&method_name_obj) else {
            continue;
        };
        if method.is_export {
            // Valid — method exists and is exported in some defining file.
            return;
        }
        saw_non_export = true;
    }

    let detail = format!("{}.{}", handler.module_name, handler.method_name);
    let dtype = if saw_non_export {
        // CHECK 6: method exists in some defining file but never as export.
        DiagnosticType::NonExportMethod
    } else {
        // CHECK 5: method does not exist in any defining file.
        DiagnosticType::MissingMethod
    };
    diagnostics.push(create_diagnostic(ctx, dtype, event_sub.name(), &detail, code));
}

/// Diagnostic type for different validation failures
#[derive(Debug, Clone, Copy)]
enum DiagnosticType {
    EmptyHandler,
    IncorrectFormat,
    MissingModule,
    ShouldBeServer,
    MissingMethod,
    NonExportMethod,
}

/// Create diagnostic with Russian error message
///
/// All diagnostics are reported at the SessionModule start (line 1, columns 1-8).
fn create_diagnostic(
    ctx: &DiagnosticsContext,
    diagnostic_type: DiagnosticType,
    event_sub_name: &str,
    detail: &str,
    code: DiagnosticCode,
) -> Diagnostic {
    let message = match diagnostic_type {
        DiagnosticType::EmptyHandler => {
            format!("Заполните обработчик подписки на событие \"{}\"", event_sub_name)
        }
        DiagnosticType::IncorrectFormat => {
            format!(
                "Исправьте некорректный обработчик \"{}\" у подписки на событие \"{}\"",
                detail, event_sub_name
            )
        }
        DiagnosticType::MissingModule => {
            format!(
                "Создайте модуль \"{}\" или исправьте некорректный обработчик подписки на событие \"{}\"",
                detail, event_sub_name
            )
        }
        DiagnosticType::ShouldBeServer => {
            format!(
                "Добавьте \"Сервер\" модулю \"{}\" или исправьте некорректный обработчик подписки на событие \"{}\"",
                detail, event_sub_name
            )
        }
        DiagnosticType::MissingMethod => {
            format!(
                "Создайте процедуру \"{}\" или исправьте некорректный обработчик подписки на событие \"{}\"",
                detail, event_sub_name
            )
        }
        DiagnosticType::NonExportMethod => {
            format!(
                "Добавьте \"Экспорт\" процедуре \"{}\"  или исправьте некорректный обработчик подписки на событие \"{}\"",
                detail, event_sub_name
            )
        }
    };

    // Get file text to determine safe range
    let file_text = ctx.file_text();
    let file_len = file_text.len();

    // Use range [0, min(14, file_len)) to avoid exceeding file bounds
    let end_offset = std::cmp::min(14, file_len);
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
    fn test_missing_event_subscription_handler() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let code = "Функция Маркер()\nКонецФункции\n";
        let (diagnostics, file_content) = check_diagnostic(code, fixtures_dir);

        // Should find 6 diagnostics
        assert_eq!(diagnostics.len(), 6, "Expected 6 diagnostics, found {}", diagnostics.len());

        // All diagnostics should be at line 1, columns 1-8 (range 0-7)
        for (i, diagnostic) in diagnostics.iter().enumerate() {
            assert_diagnostic_range(&file_content, diagnostic, 0, 0, 7);
            assert_eq!(
                diagnostic.severity,
                Severity::Blocker,
                "Diagnostic {} should be BLOCKER",
                i
            );
        }

        // Verify messages (order-independent)
        let messages: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();

        // Check for specific error messages
        assert!(
            messages
                .iter()
                .any(|m| m.contains("ОбщийПодпискиНаСобытия") && m.contains("Создайте модуль")),
            "Should have MissingModule diagnostic"
        );
        assert!(
            messages.iter().any(|m| m.contains("некорректный обработчик")),
            "Should have IncorrectFormat diagnostic"
        );
        assert!(
            messages.iter().any(|m| m.contains("Добавьте \"Сервер\"")),
            "Should have ShouldBeServer diagnostic"
        );
        assert!(
            messages.iter().any(|m| m.contains("Заполните обработчик")),
            "Should have EmptyHandler diagnostic"
        );
        assert!(
            messages.iter().any(|m| m.contains("ПодпискаНаСобытиеПриУстановкеНовогоКода")),
            "Should have MissingMethod diagnostic"
        );
        assert!(
            messages.iter().any(|m| m.contains("РегистрацияИзмененийПередУдалением")),
            "Should have NonExportMethod diagnostic"
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
