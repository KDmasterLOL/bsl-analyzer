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
use syntax::{SyntaxKind, SyntaxNode};

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

    // OPTIMIZATION 1: Check for public regions BEFORE loading metadata
    // This is a fast O(n) scan that avoids expensive metadata loading if no public regions exist
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    // Quick check: are there any public regions at all?
    if !has_public_regions(&root) {
        return Vec::new(); // Early exit - no public regions, no need to check metadata
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

    // OPTIMIZATION 2: Only load metadata if there are public regions
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

    // OPTIMIZATION 3: Reuse already parsed tree (we already have 'root')
    let regions = find_public_regions_optimized(&root);

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

/// Fast check if file has any public regions (before metadata loading).
///
/// Returns true if at least one PRE_REGION_DIR with public name is found.
/// This is a quick O(n) scan to avoid expensive metadata loading.
fn has_public_regions(root: &SyntaxNode) -> bool {
    for node in root.descendants() {
        if node.kind() == SyntaxKind::PRE_REGION_DIR {
            if let Some(region_dir) = PreRegionDir::cast(node) {
                if let Some(name) = region_dir.name() {
                    if is_public_region(&name) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Find all public regions with methods (optimized version).
///
/// OPTIMIZATION: Sorts methods by position and uses early exit instead of checking all methods.
/// For N regions and M methods: O(N + M log M + N×M) → O(N + M log M + N×k) where k << M
fn find_public_regions_optimized(root: &SyntaxNode) -> Vec<RegionInfo> {
    let mut public_regions: Vec<(String, TextRange)> = Vec::new();
    let mut method_ranges: Vec<TextRange> = Vec::new();

    // Single pass: collect public regions and method definitions
    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::PRE_REGION_DIR => {
                if let Some(region_dir) = PreRegionDir::cast(node.clone()) {
                    if let Some(name) = region_dir.name() {
                        if is_public_region(&name) {
                            let range = region_dir.syntax().text_range();
                            public_regions.push((name.to_string(), range));
                        }
                    }
                }
            }
            SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF => {
                method_ranges.push(node.text_range());
            }
            _ => {}
        }
    }

    // OPTIMIZATION: Sort methods by start position for faster lookup
    method_ranges.sort_by_key(|r| r.start());

    // Match methods to regions with early exit
    public_regions
        .into_iter()
        .map(|(name, range)| {
            // Binary search to find first method that could be in this region
            let start_idx = method_ranges
                .binary_search_by_key(&range.start(), |r| r.start())
                .unwrap_or_else(|idx| idx);

            // Check only methods starting from start_idx until we exceed region end
            let has_methods = method_ranges[start_idx..]
                .iter()
                .take_while(|method_range| method_range.start() < range.end())
                .any(|method_range| range.contains_range(*method_range));

            RegionInfo { name, range, has_methods }
        })
        .collect()
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
/// Converts FileId to URI string by looking up the file path through SourceRoot.
///
/// Steps:
/// 1. Get SourceRootId for file
/// 2. Get SourceRoot for SourceRootId
/// 3. Get FileSet from SourceRoot
/// 4. Look up VfsPath for FileId
/// 5. Convert Path to URI string
fn file_uri(db: &dyn ide_db::RootDatabase, file_id: vfs::FileId) -> Option<String> {
    // Get source root for file
    let source_root_input = db.file_source_root_input(file_id);
    let source_root_id = source_root_input.source_root_id(db);

    // Get source root
    let source_root_input = db.source_root_input(source_root_id);
    let source_root = source_root_input.root(db);

    // Get file path from FileSet
    let file_set = source_root.file_set();
    let vfs_path = file_set.path_for_file(&file_id)?;

    // Convert VfsPath to URI string
    // Note: In 1C, URIs are typically file:// URLs, but metadata uses lowercase paths
    let path_str = vfs_path.as_path().to_string_lossy().to_string();
    Some(path_str)
}

/// Check code with specific module (test-only helper).
///
/// This function mimics Java's spy pattern - it skips metadata loading
/// and uses provided module directly, allowing tests to override ReturnValueReuse.
#[cfg(test)]
fn check_with_module(
    ctx: &DiagnosticsContext,
    module: &bsl_metadata::CommonModule,
) -> Vec<Diagnostic> {
    // Check if module is cached
    if !is_cached(module) {
        return Vec::new();
    }

    // Analyze source code for public regions
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let regions = find_public_regions_optimized(&root);

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
            configuration_path_input: None,
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

    /// Helper to create mock CommonModule (mimics Java's spy pattern)
    fn create_mock_module(
        return_values_reuse: bsl_metadata::ReturnValueReuse,
    ) -> bsl_metadata::CommonModule {
        use bsl_metadata::CommonModule;

        CommonModule::builder().name("TestModule").return_values_reuse(return_values_reuse).build()
    }

    #[test]
    fn test_comprehensive_during_request() {
        // Test with ReturnValueReuse::DuringRequest (like Java spy)
        let code = include_str!("../../test_data/CachedPublicDiagnostic.bsl");
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = test_fixture::Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();
        db.set_file_text(file_id, code);
        let db = Rc::new(db) as Rc<dyn RootDatabase>;

        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        // Create mock module with DuringRequest (mimics Java's spy)
        let module = create_mock_module(bsl_metadata::ReturnValueReuse::DuringRequest);
        let diagnostics = check_with_module(&ctx, &module);

        // Expected: 2 diagnostics (lines 0 and 16 in 0-based)
        assert_eq!(diagnostics.len(), 2, "Should find 2 public regions with methods");

        // First: #Область ПрограммныйИнтерфейс (line 0)
        let (first_line, _, _, _) =
            crate::test_utils::range_to_line_col(code, diagnostics[0].range);
        assert_eq!(first_line, 0, "First diagnostic should be at line 0");

        // Second: #Область public (line 16)
        let (second_line, _, _, _) =
            crate::test_utils::range_to_line_col(code, diagnostics[1].range);
        assert_eq!(second_line, 16, "Second diagnostic should be at line 16");
    }

    #[test]
    fn test_comprehensive_during_session() {
        // Test with ReturnValueReuse::DuringSession
        let code = include_str!("../../test_data/CachedPublicDiagnostic.bsl");
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = test_fixture::Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();
        db.set_file_text(file_id, code);
        let db = Rc::new(db) as Rc<dyn RootDatabase>;

        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        // Create mock module with DuringSession
        let module = create_mock_module(bsl_metadata::ReturnValueReuse::DuringSession);
        let diagnostics = check_with_module(&ctx, &module);

        // Should also find 2 diagnostics (DuringSession is also cached)
        assert_eq!(diagnostics.len(), 2, "Should find 2 public regions with methods");
    }

    #[test]
    fn test_comprehensive_dont_use() {
        // Test with ReturnValueReuse::DontUse (not cached - should skip)
        let code = include_str!("../../test_data/CachedPublicDiagnostic.bsl");
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = test_fixture::Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();
        db.set_file_text(file_id, code);
        let db = Rc::new(db) as Rc<dyn RootDatabase>;

        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        // Create mock module with DontUse (not cached)
        let module = create_mock_module(bsl_metadata::ReturnValueReuse::DontUse);
        let diagnostics = check_with_module(&ctx, &module);

        // Should find NO diagnostics (module is not cached)
        assert_eq!(diagnostics.len(), 0, "Should skip non-cached modules");
    }
}
