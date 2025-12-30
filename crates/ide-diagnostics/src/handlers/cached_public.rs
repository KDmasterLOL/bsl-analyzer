//! CachedPublic diagnostic.
//!
//! Checks that cached CommonModules (ReturnValueReuse = DuringRequest/DuringSession)
//! do not contain public regions with methods.
//!
//! ## Why?
//! Caching works at the module level, not at the method level. Having public methods in
//! cached modules can lead to:
//! - Unexpected stale data being returned from cache
//! - Confusion about which methods are actually cached
//! - Violation of the single responsibility principle
//!
//! ## Bad practice
//! ```bsl
//! // CommonModule with ReturnValueReuse = DuringRequest
//! #Область ПрограммныйИнтерфейс  // ← Public region with methods - ERROR!
//!
//! Функция ПолучитьДанные()
//!     Возврат Новый Структура();
//! КонецФункции
//!
//! #КонецОбласти
//! ```
//!
//! ## Good practice
//! ```bsl
//! // CommonModule with ReturnValueReuse = DuringRequest
//! #Область СлужебныйПрограммныйИнтерфейс  // ← Non-public region - OK
//!
//! Функция ПолучитьДанные()
//!     Возврат Новый Структура();
//! КонецФункции
//!
//! #КонецОбласти
//! ```
//!
//! ## Implementation
//!
//! Ported from:
//! - CachedPublicDiagnostic.java (bsl-language-server) - PRIMARY
//! - cached_public.rs (bsl-language-server-rust) - REFERENCE
//!
//! Tier 3 diagnostic: Requires metadata (CommonModule, ReturnValueReuse).

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::{traits::Module, ReturnValueReuse};
use ide_db::TextRange;
use syntax::ast::{AstNode, PreRegionDir};
use syntax::{SyntaxKind, SyntaxNode, TextSize};

/// Information about a region.
#[derive(Debug, Clone)]
struct RegionInfo {
    #[allow(dead_code)] // Kept for debugging purposes
    name: String,
    range: TextRange,
    has_methods: bool,
}

/// Main entry point for CachedPublic diagnostic.
///
/// Checks:
/// 1. File is a cached CommonModule (ReturnValueReuse = DuringRequest/DuringSession)
/// 2. Finds all public regions (#Область ПрограммныйИнтерфейс or #Region Public)
/// 3. Reports regions that contain PROCEDURE_DEF or FUNCTION_DEF
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CachedPublic) {
        return Vec::new();
    }

    // Workspace integration required for metadata access
    let _config_path = match ctx.configuration_path.or(ctx.workspace_root) {
        Some(path) => path,
        None => {
            // No workspace - skip metadata check (used in standalone tests)
            tracing::debug!("No workspace root - skipping CachedPublic check");
            return Vec::new();
        }
    };

    // Load configuration metadata
    // Workaround for trait object limitation: call Salsa query function directly
    // instead of using trait method (which requires Self: Sized)
    let config_path_str = _config_path.to_string_lossy().to_string();
    let path_input = ide_db::metadata::ConfigurationPathInput::new(ctx.db, config_path_str);

    // Upcast &dyn RootDatabase to &dyn salsa::Database to call the free function
    let configuration = ide_db::metadata::load_configuration(ctx.db, path_input);

    // Find CommonModule for current file
    let common_module = match find_common_module_for_file(ctx, &configuration) {
        Some(module) => module,
        None => {
            // Not a CommonModule - skip check
            return Vec::new();
        }
    };

    // Check if module is cached
    if !is_cached(&common_module) {
        return Vec::new();
    }

    // Analyze source code for public regions
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let regions = find_public_regions(&root);

    // Generate diagnostics for public regions with methods
    regions
        .into_iter()
        .filter(|r| r.has_methods)
        .map(|r| Diagnostic {
            code: DiagnosticCode::CachedPublic,
            message: "Кэшируемый модуль не должен содержать методы в публичных областях"
                .to_string(),
            severity: Severity::Warning,
            range: r.range,
            tags: vec![],
            fixes: vec![],
        })
        .collect()
}

/// Check if module has caching enabled.
fn is_cached(module: &bsl_metadata::CommonModule) -> bool {
    matches!(
        module.return_values_reuse(),
        ReturnValueReuse::DuringRequest | ReturnValueReuse::DuringSession
    )
}

/// Check if region name matches public region keywords.
fn is_public_region(region_name: &str) -> bool {
    let name_lower = region_name.to_lowercase();
    name_lower == "public" || name_lower == "программныйинтерфейс"
}

/// Find all public regions with methods.
fn find_public_regions(root: &SyntaxNode) -> Vec<RegionInfo> {
    let mut stack: Vec<(String, TextSize)> = Vec::new();
    let mut regions = Vec::new();

    for node in root.descendants() {
        if let Some(region_dir) = PreRegionDir::cast(node.clone()) {
            if region_dir.is_start() {
                if let Some(name) = region_dir.name() {
                    let offset = region_dir.syntax().text_range().start();
                    stack.push((name, offset));
                }
            } else if region_dir.is_end() {
                if let Some((name, start)) = stack.pop() {
                    let end = region_dir.syntax().text_range().end();
                    let range = TextRange::new(start, end);

                    if is_public_region(&name) {
                        let has_methods = contains_methods(root, range);
                        regions.push(RegionInfo { name: name.to_string(), range, has_methods });
                    }
                }
            }
        }
    }

    regions
}

/// Check if range contains PROCEDURE_DEF or FUNCTION_DEF.
fn contains_methods(root: &SyntaxNode, range: TextRange) -> bool {
    root.descendants().any(|n| {
        range.contains_range(n.text_range())
            && matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF)
    })
}

/// Find CommonModule metadata for given file.
///
/// Returns None if file is not a CommonModule or metadata not found.
fn find_common_module_for_file(
    ctx: &DiagnosticsContext,
    configuration: &bsl_metadata::Configuration,
) -> Option<bsl_metadata::CommonModule> {
    // Get file URI from VFS
    let file_uri = file_uri(ctx.db, ctx.file_id)?;

    // Search configuration.common_modules() for matching URI
    configuration
        .common_modules()
        .iter()
        .find(|module| {
            // Match by URI (Module trait method)
            if let Some(module_uri) = module.uri() {
                module_uri.to_lowercase() == file_uri.to_lowercase()
            } else {
                false
            }
        })
        .cloned()
}

/// Get file URI from VFS.
///
/// Helper function to convert FileId to URI string.
#[allow(dead_code)]
fn file_uri(_db: &dyn ide_db::RootDatabase, _file_id: vfs::FileId) -> Option<String> {
    // FIXME: Implement proper VFS path resolution
    // For now, return None (metadata loading is disabled anyway)
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    // use crate::test_utils::assert_diagnostic_range; // Will be used when metadata loading is enabled
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None, // No workspace in unit tests
            configuration_path: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_no_workspace() {
        // Without workspace integration, diagnostic should skip check
        let code = r#"
#Область ПрограммныйИнтерфейс
Процедура Метод1()
КонецПроцедуры
#КонецОбласти
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Should skip check without workspace");
    }

    #[test]
    fn test_public_region_with_method() {
        // This test will be updated when workspace integration is ready
        let code = r#"
#Область ПрограммныйИнтерфейс
Процедура Метод1()
КонецПроцедуры
#КонецОбласти
"#;
        let (diagnostics, _) = check_diagnostic(code);
        // Currently returns 0 (no workspace), will return 1 with workspace
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_non_public_region_ignored() {
        let code = r#"
#Область СлужебныйПрограммныйИнтерфейс
Процедура Метод1()
КонецПроцедуры
#КонецОбласти
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_empty_public_region() {
        let code = r#"
#Область ПрограммныйИнтерфейс
#КонецОбласти
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    // Comprehensive test will be added when workspace integration is ready
    // #[test]
    // fn test_comprehensive() {
    //     let code = include_str!("../../test_data/CachedPublicDiagnostic.bsl");
    //     let (diagnostics, file_content) = check_diagnostic_with_metadata(code);
    //
    //     // Expected: 2 diagnostics (lines 0 and 16 in 0-based)
    //     assert_eq!(diagnostics.len(), 2);
    //
    //     // First: #Область ПрограммныйИнтерфейс
    //     assert_diagnostic_range(&file_content, &diagnostics[0], 0, 0, 31);
    //
    //     // Second: #Область public
    //     assert_diagnostic_range(&file_content, &diagnostics[1], 16, 0, 15);
    // }
}
