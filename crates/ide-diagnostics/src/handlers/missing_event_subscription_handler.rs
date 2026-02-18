//! MissingEventSubscriptionHandler diagnostic.
//!
//! Validates event subscription handlers in 1C SessionModule files.
//!
//! ## What it checks
//!
//! This diagnostic validates that all event subscriptions in the configuration have valid handlers:
//!
//! 1. **Handler must not be empty** - Each subscription must have a handler defined
//! 2. **Handler format** - Must be "CommonModule.ModuleName.MethodName" with method name present
//! 3. **CommonModule exists** - The referenced common module must exist in configuration
//! 4. **Module is server-side** - Common module must have Server flag set to true
//! 5. **Method exists** - The method must be defined in the common module
//! 6. **Method is exported** - The method must have Экспорт (Export) keyword
//!
//! ## Why?
//!
//! Event subscriptions bind system events (OnWrite, BeforeWrite, etc.) to handler procedures.
//! Invalid handlers lead to runtime errors when events are triggered.
//!
//! ## Example (bad)
//!
//! ```bsl
//! // In EventSubscription metadata:
//! // Handler: CommonModule.МодульОтсутствует.МетодОбработки
//! // ERROR: Module doesn't exist
//! ```
//!
//! ## Example (good)
//!
//! ```bsl
//! // In CommonModule.ПодпискиНаСобытия:
//! Процедура ПриЗаписиДокумента(Источник, Отказ) Экспорт
//!     // Implementation
//! КонецПроцедуры
//!
//! // In EventSubscription metadata:
//! // Handler: CommonModule.ПодпискиНаСобытия.ПриЗаписиДокумента
//! // OK: Module exists, is server-side, method exists and exported
//! ```
//!
//! ## Configuration
//!
//! - **Enabled by default:** Yes
//! - **Severity:** BLOCKER (ERROR)
//! - **Tags:** ERROR
//! - **Minutes to fix:** 5
//! - **No configurable parameters** (strict validator)
//!
//! ## Scope
//!
//! This diagnostic only runs for **SessionModule** files (Configuration/SessionModule.bsl).
//! All diagnostics are reported at the beginning of the SessionModule (line 1, columns 1-8).
//!
//! ## Reference
//!
//! Ported from:
//! - MissingEventSubscriptionHandlerDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::traits::MdObject;
use bsl_metadata::{EventSubscription, EventSubscriptionHandler};
use hir::{ModuleId, Name};
use ide_db::TextRange;
use vfs::FileId;

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

/// Main entry point for MissingEventSubscriptionHandler diagnostic.
///
/// Validates event subscription handlers in SessionModule.
///
/// ## Algorithm
///
/// 1. Early return if disabled or not SessionModule
/// 2. Load Configuration metadata via Salsa (cached!)
/// 3. Iterate all event subscriptions
/// 4. For each subscription, perform 6 validation checks:
///    - Handler not empty
///    - Handler format correct (has method name)
///    - CommonModule exists
///    - CommonModule has Server flag
///    - Method exists in module
///    - Method is exported
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

    // 3. Load configuration metadata
    let configuration = match ctx.load_configuration() {
        Some(config) => config,
        None => return Vec::new(),
    };

    // 4. Process all event subscriptions
    let mut diagnostics = Vec::new();
    for event_sub in configuration.event_subscriptions() {
        check_event_subscription(ctx, event_sub, &configuration, code, &mut diagnostics);
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
    configuration: &bsl_metadata::Configuration,
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

    // CHECK 3: CommonModule exists
    let common_module = match configuration.find_common_module(&handler.module_name) {
        Some(cm) => cm,
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
    check_method(ctx, event_sub, &handler, common_module, code, diagnostics);
}

/// Validate method exists and is exported
fn check_method(
    ctx: &DiagnosticsContext,
    event_sub: &EventSubscription,
    handler: &EventSubscriptionHandler,
    common_module: &bsl_metadata::CommonModule,
    code: DiagnosticCode,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Resolve CommonModule file via VFS
    let module_file_id = match find_common_module_file(ctx, common_module) {
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
                event_sub.name(),
                &format!("{}.{}", handler.module_name, handler.method_name),
                code,
            ));
        }
        Some(m) if !m.is_export => {
            // CHECK 6: Method not exported
            diagnostics.push(create_diagnostic(
                ctx,
                DiagnosticType::NonExportMethod,
                event_sub.name(),
                &format!("{}.{}", handler.module_name, handler.method_name),
                code,
            ));
        }
        Some(_) => {
            // Valid: method exists and is exported
        }
    }
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
/// All diagnostics are reported at the SessionModule start (line 1, columns 1-8)
/// to match bsl-language-server behavior.
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
    // Java implementation uses (0, 0, 0, 14) but we need to be safe for small files
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

/// Find CommonModule BSL file via VFS resolution
///
/// This function resolves the CommonModule URI to a FileId using the VFS.
/// Similar to MissingCommonModuleMethod::find_common_module_file.
fn find_common_module_file(
    ctx: &DiagnosticsContext,
    common_module: &bsl_metadata::CommonModule,
) -> Option<FileId> {
    use bsl_metadata::traits::Module;

    // Get module URI (e.g., "CommonModules/ModuleName/Ext/Module.bsl")
    let uri = common_module.uri()?;

    // Get workspace root (prefer configuration_path for proper path resolution)
    let workspace_root = ctx.configuration_path.or(ctx.workspace_root)?;

    // Build absolute path
    let full_path = workspace_root.join(uri);

    // Convert to VfsPath
    let vfs_path = vfs::VfsPath::new(full_path.to_string_lossy().into_owned());

    // CRITICAL: Use ctx.file_set directly to bypass Salsa for performance
    let file_id = if let Some(file_set) = ctx.file_set {
        file_set.file_for_path(&vfs_path).copied()
    } else {
        // Fallback: Use provider/db (slower, for tests)
        let source_root_id = ctx.source_root_id();
        ctx.resolve_vfs_path(source_root_id, &vfs_path)
    };

    if file_id.is_none() {
        tracing::warn!(
            module = %common_module.name(),
            uri = %uri,
            "CommonModule file not found in VFS"
        );
    }

    file_id
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
        );

        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            provider: None,
            workspace_root: Some(&workspace_root),
            configuration_path: Some(&workspace_root),
            configuration_path_input: Some(configuration_path_input),
            file_set: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, code.to_string())
    }

    #[test]
    fn test_missing_event_subscription_handler() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let code = include_str!("../../test_data/MissingEventSubscriptionHandlerDiagnostic.bsl");
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
        let workspace_root = PathBuf::from(fixtures_dir);
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

        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            provider: None,
            workspace_root: Some(&workspace_root),
            configuration_path: Some(&workspace_root),
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);

        // Should return empty for non-SessionModule
        assert_eq!(diagnostics.len(), 0, "Non-SessionModule should have no diagnostics");
    }
}
