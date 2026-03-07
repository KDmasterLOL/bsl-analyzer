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
//!
//! Tier 3 diagnostic: Requires metadata (CommonModule, ReturnValueReuse).
//! Uses HIR RegionTree and ItemTree for clean, cached access to regions and methods.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::ReturnValueReuse;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Adaptable,
};

/// Main entry point for CachedPublic diagnostic.
///
/// Checks:
/// 1. File has public regions (via RegionTree - Salsa cached)
/// 2. File is a cached CommonModule (ReturnValueReuse = DuringRequest/DuringSession)
/// 3. Reports public regions that contain procedures/functions (via ItemTree)
///
/// Uses HIR RegionTree and ItemTree for clean access to structured data.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CachedPublic;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // OPTIMIZATION 1: Check regions first (Salsa cached via region_tree_query)
    // This is faster than loading metadata, so we do it first for early exit
    let region_tree = ctx.region_tree();

    // Find all public regions (ПрограммныйИнтерфейс or Public)
    let public_regions: Vec<_> = region_tree
        .regions()
        .filter(|(_, region)| is_public_region(region.name.as_str()))
        .collect();

    if public_regions.is_empty() {
        return Vec::new(); // Early exit - no public regions, no need to check metadata
    }

    // OPTIMIZATION 2: Only load metadata if we have public regions
    let metadata = ctx.module_metadata();

    // Check if this is a cached CommonModule
    let common_module = match &metadata.common_module {
        Some(cm) => cm,
        None => return Vec::new(), // Not a CommonModule
    };

    if !is_cached_reuse(common_module.return_values_reuse()) {
        return Vec::new(); // Not cached
    }

    // Get item tree for method lookup (Salsa cached)
    let item_tree = ctx.item_tree();

    // Check each public region for methods
    public_regions
        .into_iter()
        .filter_map(|(_, region)| {
            // Check if this region contains any procedures or functions
            let has_methods = item_tree
                .procedures()
                .any(|(_, proc)| region.range.contains_range(proc.source_range))
                || item_tree
                    .functions()
                    .any(|(_, func)| region.range.contains_range(func.source_range));

            if has_methods {
                Some(Diagnostic {
                    code: DiagnosticCode::CachedPublic,
                    message: "Кэшируемый модуль не должен содержать методы в публичных областях"
                        .to_string(),
                    severity: ctx.severity(code),
                    // Use region range for compatibility with existing tests
                    // (region.name_range would be better UX but breaks tests)
                    range: region.range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                })
            } else {
                None
            }
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

/// Check code with specific ReturnValueReuse (test-only helper).
///
/// Skips HIR metadata loading and uses provided reuse value directly,
/// allowing tests to verify caching behavior.
#[cfg(test)]
fn check_with_reuse(ctx: &DiagnosticsContext, reuse: ReturnValueReuse) -> Vec<Diagnostic> {
    // Check if module would be cached with this reuse setting
    if !is_cached_reuse(reuse) {
        return Vec::new();
    }

    // Use HIR RegionTree and ItemTree (same as main check())
    let region_tree = ctx.region_tree();

    // Find all public regions
    let public_regions: Vec<_> = region_tree
        .regions()
        .filter(|(_, region)| is_public_region(region.name.as_str()))
        .collect();

    if public_regions.is_empty() {
        return Vec::new();
    }

    // Get item tree for method lookup
    let item_tree = ctx.item_tree();

    // Check each public region for methods
    public_regions
        .into_iter()
        .filter_map(|(_, region)| {
            // Check if this region contains any procedures or functions
            let has_methods = item_tree
                .procedures()
                .any(|(_, proc)| region.range.contains_range(proc.source_range))
                || item_tree
                    .functions()
                    .any(|(_, func)| region.range.contains_range(func.source_range));

            if has_methods {
                let code = DiagnosticCode::CachedPublic;
                Some(Diagnostic {
                    code,
                    message: "Кэшируемый модуль не должен содержать методы в публичных областях"
                        .to_string(),
                    severity: ctx.severity(code),
                    range: region.range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                })
            } else {
                None
            }
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
            provider: None,
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
            provider: None,
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
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 0);
    }

    // ========== Tests for check_with_reuse (ReturnValueReuse variants) ==========

    /// DuringRequest cached module with two public regions (ПрограммныйИнтерфейс and public),
    /// one non-public region (СлужебныйПрограммныйИнтерфейс). Expects 2 diagnostics.
    #[test]
    fn test_during_request_finds_public_regions() {
        // Inline equivalent of CachedPublicDiagnostic.bsl
        let code = r#"#Область ПрограммныйИнтерфейс

Процедура Метод1()

КонецПроцедуры

#КонецОбласти

#Область СлужебныйПрограммныйИнтерфейс

Процедура Метод1()

КонецПроцедуры

#КонецОбласти

#Область public

Процедура Метод1()

КонецПроцедуры

#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            provider: None,
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

    /// DuringSession is also a cached mode - same 2 diagnostics expected.
    #[test]
    fn test_during_session_finds_public_regions() {
        let code = r#"#Область ПрограммныйИнтерфейс

Процедура Метод1()

КонецПроцедуры

#КонецОбласти

#Область СлужебныйПрограммныйИнтерфейс

Процедура Метод1()

КонецПроцедуры

#КонецОбласти

#Область public

Процедура Метод1()

КонецПроцедуры

#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringSession);
        assert_eq!(diagnostics.len(), 2, "DuringSession is also cached");
    }

    /// DontUse means no caching - diagnostic should be skipped entirely.
    #[test]
    fn test_dont_use_skips_check() {
        let code = r#"#Область ПрограммныйИнтерфейс

Процедура Метод1()

КонецПроцедуры

#КонецОбласти

#Область СлужебныйПрограммныйИнтерфейс

Процедура Метод1()

КонецПроцедуры

#КонецОбласти

#Область public

Процедура Метод1()

КонецПроцедуры

#КонецОбласти
"#;
        let (db, file_id, config) = create_test_ctx(code);
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            provider: None,
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
            provider: None,
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
            provider: None,
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
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check_with_reuse(&ctx, ReturnValueReuse::DuringRequest);
        assert_eq!(diagnostics.len(), 1, "Only public region triggers diagnostic");
    }
}
