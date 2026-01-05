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
//! Uses HIR module_metadata() for clean access to CommonModule info.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::ReturnValueReuse;
use ide_db::hir_def::ModuleId;
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
///
/// Uses HIR module_metadata() for clean access to CommonModule metadata.
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

    // Get metadata through HIR (cached, single point of access)
    let module_id = ModuleId::new(ctx.file_id);
    let metadata = ctx.db.module_metadata(module_id);

    // Check if this is a cached CommonModule
    let common_module = match &metadata.common_module {
        Some(cm) => cm,
        None => {
            // Not a CommonModule - skip check
            return Vec::new();
        }
    };

    // Check if module is cached
    if !is_cached_reuse(common_module.return_values_reuse()) {
        return Vec::new();
    }

    // OPTIMIZATION 2: Reuse already parsed tree (we already have 'root')
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

/// Check if ReturnValueReuse indicates caching is enabled.
fn is_cached_reuse(reuse: ReturnValueReuse) -> bool {
    matches!(reuse, ReturnValueReuse::DuringRequest | ReturnValueReuse::DuringSession)
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

/// Check code with specific ReturnValueReuse (test-only helper).
///
/// This function mimics Java's spy pattern - it skips HIR metadata loading
/// and uses provided reuse value directly, allowing tests to verify caching behavior.
#[cfg(test)]
fn check_with_reuse(ctx: &DiagnosticsContext, reuse: ReturnValueReuse) -> Vec<Diagnostic> {
    // Check if module would be cached with this reuse setting
    if !is_cached_reuse(reuse) {
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
    use crate::DiagnosticsConfig;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;
    use vfs::file_set::FileSet;
    use vfs::VfsPath;

    /// Helper to create DiagnosticsContext for testing
    fn create_test_ctx(code: &str) -> (Rc<dyn RootDatabase>, vfs::FileId, DiagnosticsConfig) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        // Set up source root (required for module_metadata)
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, code);
        let db = Rc::new(db) as Rc<dyn RootDatabase>;

        (db, file_id, DiagnosticsConfig::default())
    }

    // ========== Tests for main check() via HIR ==========

    #[test]
    fn test_no_common_module_metadata() {
        // Without CommonModule metadata from HIR, diagnostic returns empty
        let code = r#"
#Область ПрограммныйИнтерфейс
Процедура Метод1()
КонецПроцедуры
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 0, "Should skip when no CommonModule metadata");
    }

    #[test]
    fn test_non_public_region_ignored() {
        // Non-public regions should not trigger diagnostics
        let code = r#"
#Область СлужебныйПрограммныйИнтерфейс
Процедура Метод1()
КонецПроцедуры
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_empty_public_region() {
        // Empty public region - no methods, no diagnostics
        let code = r#"
#Область ПрограммныйИнтерфейс
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 0);
    }

    // ========== Tests for check_with_reuse (ReturnValueReuse variants) ==========

    #[test]
    fn test_during_request_finds_public_regions() {
        // DuringRequest = cached, should find public regions with methods
        let code = include_str!("../../test_data/CachedPublicDiagnostic.bsl");
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringRequest);

        // Expected: 2 diagnostics (ПрограммныйИнтерфейс at line 0, public at line 16)
        assert_eq!(diagnostics.len(), 2, "Should find 2 public regions with methods");

        let (first_line, _, _, _) =
            crate::test_utils::range_to_line_col(code, diagnostics[0].range);
        assert_eq!(first_line, 0, "First diagnostic at line 0");

        let (second_line, _, _, _) =
            crate::test_utils::range_to_line_col(code, diagnostics[1].range);
        assert_eq!(second_line, 16, "Second diagnostic at line 16");
    }

    #[test]
    fn test_during_session_finds_public_regions() {
        // DuringSession = also cached
        let code = include_str!("../../test_data/CachedPublicDiagnostic.bsl");
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringSession);
        assert_eq!(diagnostics.len(), 2, "DuringSession is also cached");
    }

    #[test]
    fn test_dont_use_skips_check() {
        // DontUse = not cached, should skip
        let code = include_str!("../../test_data/CachedPublicDiagnostic.bsl");
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DontUse);
        assert_eq!(diagnostics.len(), 0, "DontUse means not cached");
    }

    // ========== Tests for is_cached_reuse logic ==========

    #[test]
    fn test_is_cached_reuse() {
        assert!(is_cached_reuse(ReturnValueReuse::DuringRequest));
        assert!(is_cached_reuse(ReturnValueReuse::DuringSession));
        assert!(!is_cached_reuse(ReturnValueReuse::DontUse));
    }

    // ========== Tests for is_public_region ==========

    #[test]
    fn test_is_public_region_russian() {
        assert!(is_public_region("ПрограммныйИнтерфейс"));
        assert!(is_public_region("программныйинтерфейс"));
        assert!(is_public_region("ПРОГРАММНЫЙИНТЕРФЕЙС"));
    }

    #[test]
    fn test_is_public_region_english() {
        assert!(is_public_region("Public"));
        assert!(is_public_region("public"));
        assert!(is_public_region("PUBLIC"));
    }

    #[test]
    fn test_is_not_public_region() {
        assert!(!is_public_region("СлужебныйПрограммныйИнтерфейс"));
        assert!(!is_public_region("Private"));
        assert!(!is_public_region("Internal"));
        assert!(!is_public_region(""));
    }

    // ========== Edge case tests ==========

    #[test]
    fn test_public_region_with_function() {
        // Function (not procedure) should also be detected
        let code = r#"#Область ПрограммныйИнтерфейс
Функция ПолучитьДанные()
    Возврат 1;
КонецФункции
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringRequest);
        assert_eq!(diagnostics.len(), 1, "Function should trigger diagnostic");
    }

    #[test]
    fn test_multiple_methods_in_public_region() {
        // Multiple methods in one public region = 1 diagnostic (on region)
        let code = r#"#Область ПрограммныйИнтерфейс
Процедура Первая()
КонецПроцедуры
Функция Вторая()
    Возврат 1;
КонецФункции
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringRequest);
        assert_eq!(diagnostics.len(), 1, "One region = one diagnostic");
    }

    #[test]
    fn test_nested_regions() {
        // Non-public region inside code should not affect detection
        let code = r#"#Область ПрограммныйИнтерфейс
Процедура Метод1()
КонецПроцедуры
#КонецОбласти

#Область СлужебныйПрограммныйИнтерфейс
Процедура Метод2()
КонецПроцедуры
#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringRequest);
        assert_eq!(diagnostics.len(), 1, "Only public region triggers diagnostic");
    }
}
