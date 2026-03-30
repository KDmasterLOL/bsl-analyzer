//! Common test utilities for diagnostic tests.
//!
//! This module provides helper functions for testing diagnostics,
//! including position calculation and assertion helpers.

use crate::Diagnostic;
use hir::DefDatabase;
use ide_db::TextRange;

/// Convert TextRange (byte offsets) to (line, column) positions.
///
/// Handles UTF-8 properly by converting byte positions to character positions.
/// Lines and columns are 0-indexed.
///
/// # Arguments
/// * `text` - The source text
/// * `range` - The byte range to convert
///
/// # Returns
/// Tuple of (start_line, start_col, end_line, end_col)
pub fn range_to_line_col(text: &str, range: TextRange) -> (u32, u32, u32, u32) {
    let start_offset: usize = range.start().into();
    let end_offset: usize = range.end().into();

    let mut line = 0u32;
    let mut col = 0u32; // Character position in current line
    let mut byte_pos = 0usize; // Byte position in text
    let mut start_line = 0u32;
    let mut start_col = 0u32;
    let mut end_line = 0u32;
    let mut end_col = 0u32;

    for ch in text.chars() {
        // Save current position before advancing
        if byte_pos == start_offset {
            start_line = line;
            start_col = col;
        }
        if byte_pos == end_offset {
            end_line = line;
            end_col = col;
            break;
        }

        // Advance
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
        byte_pos += ch.len_utf8();
    }

    // Handle case where end_offset is at the end of the text
    if byte_pos == end_offset {
        end_line = line;
        end_col = col;
    }

    (start_line, start_col, end_line, end_col)
}

/// Assert that a diagnostic with specific message exists at a specific line.
pub fn assert_diagnostic_message_at_line(
    code: &str,
    diagnostics: &[&Diagnostic],
    expected_line: u32,
    expected_message_part: &str,
) {
    let matching = diagnostics.iter().find(|d| {
        let start: u32 = d.range.start().into();
        let line = code[..start as usize].matches('\n').count() as u32;
        line == expected_line && d.message.contains(expected_message_part)
    });

    assert!(
        matching.is_some(),
        "No diagnostic with message containing '{}' at line {}.\nDiagnostics at that line: {:?}",
        expected_message_part,
        expected_line,
        diagnostics
            .iter()
            .filter(|d| {
                let start: u32 = d.range.start().into();
                code[..start as usize].matches('\n').count() as u32 == expected_line
            })
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Assert that a diagnostic is at a specific position (single-line range).
///
/// # Arguments
/// * `code` - The source code being tested
/// * `diagnostic` - The diagnostic to check
/// * `expected_line` - Expected line number (0-indexed)
/// * `expected_start_col` - Expected start column (0-indexed, character position)
/// * `expected_end_col` - Expected end column (0-indexed, character position)
pub fn assert_diagnostic_range(
    code: &str,
    diagnostic: &Diagnostic,
    expected_line: u32,
    expected_start_col: u32,
    expected_end_col: u32,
) {
    let (start_line, start_col, end_line, end_col) = range_to_line_col(code, diagnostic.range);

    assert_eq!(
        start_line, expected_line,
        "Diagnostic at wrong line. Expected line {}, got line {}.\nMessage: {}",
        expected_line, start_line, diagnostic.message
    );

    assert_eq!(
        end_line, expected_line,
        "Diagnostic spans multiple lines unexpectedly. Expected single line {}.\nMessage: {}",
        expected_line, diagnostic.message
    );

    assert_eq!(
        start_col, expected_start_col,
        "Diagnostic start column mismatch on line {}. Expected col {}, got col {}.\nMessage: {}",
        expected_line, expected_start_col, start_col, diagnostic.message
    );

    assert_eq!(
        end_col, expected_end_col,
        "Diagnostic end column mismatch on line {}. Expected col {}, got col {}.\nMessage: {}",
        expected_line, expected_end_col, end_col, diagnostic.message
    );
}

/// Assert that a diagnostic is at a specific multi-line range.
///
/// # Arguments
/// * `code` - The source code being tested
/// * `diagnostic` - The diagnostic to check
/// * `expected_start_line` - Expected start line (0-indexed)
/// * `expected_start_col` - Expected start column (0-indexed, character position)
/// * `expected_end_line` - Expected end line (0-indexed)
/// * `expected_end_col` - Expected end column (0-indexed, character position)
pub fn assert_diagnostic_range_multiline(
    code: &str,
    diagnostic: &Diagnostic,
    expected_start_line: u32,
    expected_start_col: u32,
    expected_end_line: u32,
    expected_end_col: u32,
) {
    let (start_line, start_col, end_line, end_col) = range_to_line_col(code, diagnostic.range);

    assert_eq!(
        start_line, expected_start_line,
        "Diagnostic start line mismatch. Expected line {}, got line {}.\nMessage: {}",
        expected_start_line, start_line, diagnostic.message
    );

    assert_eq!(
        start_col, expected_start_col,
        "Diagnostic start column mismatch on line {}. Expected col {}, got col {}.\nMessage: {}",
        expected_start_line, expected_start_col, start_col, diagnostic.message
    );

    assert_eq!(
        end_line, expected_end_line,
        "Diagnostic end line mismatch. Expected line {}, got line {}.\nMessage: {}",
        expected_end_line, end_line, diagnostic.message
    );

    assert_eq!(
        end_col, expected_end_col,
        "Diagnostic end column mismatch on line {}. Expected col {}, got col {}.\nMessage: {}",
        expected_end_line, expected_end_col, end_col, diagnostic.message
    );
}

/// Create a test database from BSL source code.
///
/// Returns the database and file ID for the test file.
fn create_test_db(code: &str) -> (ide_db::RootDatabaseImpl, vfs::FileId) {
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use test_fixture::Fixture;

    let fixture_text = format!("//- /test.bsl\n{}", code);
    let fixture = Fixture::parse(&fixture_text);
    let file_id = fixture.first_file().expect("fixture should have at least one file");

    let mut db = RootDatabaseImpl::new();

    let mut file_set = vfs::FileSet::default();
    file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    for (fid, file) in &fixture.files {
        db.set_file_text(*fid, &file.content);
    }

    (db, file_id)
}

/// Run AST-based diagnostics on test code.
pub fn check_ast_diagnostic<F>(code: &str, check_fn: F) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    let config = crate::DiagnosticsConfig::all_enabled();
    check_ast_diagnostic_with_config(code, config, check_fn)
}

/// Run AST-based diagnostics on test code with custom configuration.
pub fn check_ast_diagnostic_with_config<F>(
    code: &str,
    config: crate::DiagnosticsConfig,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    let (db, file_id) = create_test_db(code);
    let ctx = crate::DiagnosticsContext::new(&db, &config, file_id);
    check_fn(&ctx)
}

/// Run ALL diagnostics on test code.
pub fn check_hir_diagnostic(code: &str) -> Vec<Diagnostic> {
    check_ast_diagnostic(code, crate::diagnostics)
}

/// Run diagnostics on test code with CommonModule fixtures.
///
/// Allows testing diagnostics that require CommonModule resolution by creating
/// proper file structure with CommonModules/ directory.
///
/// # Arguments
/// * `fixture_text` - Multi-file fixture with CommonModules structure
///
/// # Returns
/// All diagnostics found in the last file (assumed to be the test file)
///
/// # Example
/// ```ignore
/// let fixture = r#"
/// //- /CommonModules/ПервыйОбщийМодуль/Module.bsl
/// Процедура Метод() Экспорт
/// КонецПроцедуры
///
/// //- /test.bsl
/// Процедура Тест()
///     ПервыйОбщийМодуль.Метод();
/// КонецПроцедуры
/// "#;
/// let diagnostics = check_hir_diagnostic_with_fixtures(fixture);
/// ```
pub fn check_hir_diagnostic_with_fixtures(fixture_text: &str) -> Vec<Diagnostic> {
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use test_fixture::Fixture;

    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();

    // Set up source root
    let mut file_set = vfs::FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
        db.set_file_text(*file_id, &file.content);
    }

    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);

    for file_id in fixture.files.keys() {
        db.set_file_source_root(*file_id, SourceRootId(0));
    }

    // Get last file as test file (convention: test file is last)
    let test_file = *fixture.files.keys().last().expect("Fixture should have at least one file");

    let config = crate::DiagnosticsConfig::all_enabled();
    let ctx = crate::DiagnosticsContext::new(&db, &config, test_file);

    crate::diagnostics(&ctx)
}

/// Run diagnostics on SDBL test code.
///
/// Convenience function for SDBL-based diagnostics.
pub fn check_sdbl_diagnostic<F>(code: &str, check_fn: F) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    check_ast_diagnostic(code, check_fn)
}

/// Run diagnostics on test code with custom configuration.
pub fn check_hir_diagnostic_with_config<F>(
    code: &str,
    config: crate::DiagnosticsConfig,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    check_ast_diagnostic_with_config(code, config, check_fn)
}

/// AnalysisProvider that returns custom metadata for testing metadata-based diagnostics.
struct MetadataTestProvider {
    db: ide_db::RootDatabaseImpl,
    metadata: std::sync::Arc<hir::ModuleMetadata>,
}

impl ide_db::provider::AnalysisProvider for MetadataTestProvider {
    fn configuration(&self) -> Option<std::sync::Arc<bsl_metadata::Configuration>> {
        None
    }

    fn workspace_symbols(
        &self,
        source_root_id: ide_db::base_db::SourceRootId,
    ) -> std::sync::Arc<hir::WorkspaceSymbols> {
        self.db.workspace_symbols(source_root_id)
    }

    fn module_index(
        &self,
        source_root_id: ide_db::base_db::SourceRootId,
    ) -> std::sync::Arc<hir::ModuleIndex> {
        self.db.module_index(source_root_id)
    }

    fn parse(&self, file_id: vfs::FileId) -> syntax::Parse<syntax::SyntaxNode> {
        use ide_db::base_db::RootQueryDb;
        self.db.parse(file_id)
    }

    fn file_text(&self, file_id: vfs::FileId) -> String {
        use ide_db::base_db::SourceDatabase;
        let input = self.db.file_text_input(file_id);
        input.text(&self.db).to_string()
    }

    fn item_tree(&self, file_id: vfs::FileId) -> std::sync::Arc<hir::ItemTree> {
        use hir::DefDatabase;
        self.db.item_tree(file_id)
    }

    fn symbol_tree(&self, module_id: hir::ModuleId) -> std::sync::Arc<hir::SymbolTree> {
        use hir::DefDatabase;
        self.db.symbol_tree(module_id)
    }

    fn region_tree(&self, file_id: vfs::FileId) -> std::sync::Arc<hir::RegionTree> {
        use hir::DefDatabase;
        self.db.region_tree(file_id)
    }

    fn module_bodies(&self, module_id: hir::ModuleId) -> std::sync::Arc<hir::ModuleBodies> {
        use hir::DefDatabase;
        self.db.module_bodies(module_id)
    }

    fn module_metadata(&self, _module_id: hir::ModuleId) -> std::sync::Arc<hir::ModuleMetadata> {
        std::sync::Arc::clone(&self.metadata)
    }

    fn module_liveness_analysis(
        &self,
        file_id: vfs::FileId,
    ) -> std::sync::Arc<hir::dataflow::liveness::ModuleLiveness> {
        use ide_db::base_db::FileIdInput;
        use ide_db::RootDatabase;
        let input = FileIdInput::new(&self.db, file_id);
        self.db.module_liveness_analysis(input)
    }

    fn module_reaching_definitions(
        &self,
        file_id: vfs::FileId,
    ) -> std::sync::Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
        use ide_db::base_db::FileIdInput;
        use ide_db::RootDatabase;
        let input = FileIdInput::new(&self.db, file_id);
        self.db.module_reaching_definitions(input)
    }

    fn reaching_definitions(
        &self,
        method_id: hir::MethodId,
    ) -> Option<std::sync::Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
        use ide_db::RootDatabase;
        self.db.reaching_definitions(method_id)
    }

    fn line_index(&self, file_id: vfs::FileId) -> std::sync::Arc<line_index::LineIndex> {
        use ide_db::base_db::SourceDatabase;
        let input = self.db.file_text_input(file_id);
        std::sync::Arc::new(line_index::LineIndex::new(&input.text(&self.db)))
    }

    fn file_path(&self, _file_id: vfs::FileId) -> Option<String> {
        None
    }

    fn file_source_root_id(&self, _file_id: vfs::FileId) -> ide_db::base_db::SourceRootId {
        ide_db::base_db::SourceRootId(0)
    }

    fn module_level_regions(
        &self,
        file_id: vfs::FileId,
    ) -> std::sync::Arc<Vec<ide_db::base_db::RegionInfo>> {
        use base_db::RootQueryDb;
        self.db.module_level_regions(file_id)
    }

    fn sdbl_hir_in_file(
        &self,
        file_id: vfs::FileId,
    ) -> std::sync::Arc<Vec<(hir::SdblExprId, std::sync::Arc<sdbl_hir::SdblPackage>)>> {
        use ide_db::RootDatabase;
        self.db.sdbl_hir_in_file(file_id)
    }

    fn all_sdbl_in_file(
        &self,
        file_id: vfs::FileId,
    ) -> std::sync::Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>> {
        use ide_db::RootDatabase;
        self.db.all_sdbl_in_file(file_id)
    }

    fn module_data(&self, module_id: hir::ModuleId) -> std::sync::Arc<hir::ModuleData> {
        use hir::DefDatabase;
        self.db.module_data(module_id)
    }

    fn method_docs(&self, method_id: hir::MethodId) -> Option<std::sync::Arc<hir::MethodDocs>> {
        self.db.method_docs(method_id)
    }

    fn module_cfgs(&self, file_id: vfs::FileId) -> std::sync::Arc<hir::cfg::ModuleCfgs> {
        use ide_db::base_db::FileIdInput;
        use ide_db::RootDatabase;
        let input = FileIdInput::new(&self.db, file_id);
        self.db.module_cfgs(input)
    }

    fn file_external_refs(
        &self,
        module_id: hir::ModuleId,
    ) -> std::sync::Arc<Vec<hir::ExternalRef>> {
        use hir::DefDatabase;
        self.db.file_external_refs(module_id)
    }

    fn module_level_liveness_analysis(
        &self,
        module_id: hir::ModuleId,
    ) -> Option<std::sync::Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>>
    {
        use ide_db::RootDatabase;
        self.db.module_level_liveness_analysis(module_id)
    }

    fn resolve_vfs_path(
        &self,
        source_root_id: ide_db::base_db::SourceRootId,
        path: &vfs::VfsPath,
    ) -> Option<vfs::FileId> {
        use ide_db::base_db::SourceDatabase;
        self.db.resolve_vfs_path(source_root_id, path)
    }

    fn resolve_module_file(&self, _relative_uri: &str) -> Option<vfs::FileId> {
        // Not supported in test provider
        None
    }
}

/// Test helper for module-level metadata diagnostics.
pub fn check_metadata_diagnostic<F>(
    metadata: hir::ModuleMetadata,
    file_text: &str,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&hir::ModuleMetadata, &crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    let config = crate::DiagnosticsConfig::all_enabled();
    check_metadata_diagnostic_with_config(metadata, file_text, config, check_fn)
}

/// Test helper for module-level metadata diagnostics with custom config.
pub fn check_metadata_diagnostic_with_config<F>(
    metadata: hir::ModuleMetadata,
    file_text: &str,
    config: crate::DiagnosticsConfig,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&hir::ModuleMetadata, &crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    use std::rc::Rc;
    use std::sync::Arc;

    let (db, file_id) = create_test_db(file_text);
    let metadata_arc = Arc::new(metadata);
    let config_rc = Rc::new(config);

    let provider_impl = MetadataTestProvider { db, metadata: Arc::clone(&metadata_arc) };

    let ctx = crate::DiagnosticsContext::with_provider(
        &provider_impl.db,
        &config_rc,
        file_id,
        &provider_impl as &dyn ide_db::provider::AnalysisProvider,
    );

    check_fn(&metadata_arc, &ctx)
}

/// Create a `ModuleMetadata` for a CommonModule (without execution context).
pub fn make_common_module_metadata(module: bsl_metadata::CommonModule) -> hir::ModuleMetadata {
    hir::ModuleMetadata {
        module_type: bsl_metadata::ModuleType::CommonModule,
        execution_context: None,
        common_module: Some(std::sync::Arc::new(module)),
        mdo: None,
        register: None,
        http_service: None,
        web_service: None,
        form: None,
    }
}

/// Create a `ModuleMetadata` for a CommonModule with execution context.
pub fn make_common_module_metadata_with_ctx(
    module: bsl_metadata::CommonModule,
    ctx: hir::ExecutionContext,
) -> hir::ModuleMetadata {
    hir::ModuleMetadata {
        module_type: bsl_metadata::ModuleType::CommonModule,
        execution_context: Some(ctx),
        common_module: Some(std::sync::Arc::new(module)),
        mdo: None,
        register: None,
        http_service: None,
        web_service: None,
        form: None,
    }
}

/// Create a `ModuleMetadata` for a non-CommonModule type (for negative tests).
pub fn make_non_common_module_metadata(
    module_type: bsl_metadata::ModuleType,
) -> hir::ModuleMetadata {
    hir::ModuleMetadata {
        module_type,
        execution_context: None,
        common_module: None,
        mdo: None,
        register: None,
        http_service: None,
        web_service: None,
        form: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_to_line_col_simple() {
        let text = "Hello World";
        let range = TextRange::new(0.into(), 5.into());
        let (start_line, start_col, end_line, end_col) = range_to_line_col(text, range);
        assert_eq!(start_line, 0);
        assert_eq!(start_col, 0);
        assert_eq!(end_line, 0);
        assert_eq!(end_col, 5);
    }

    #[test]
    fn test_range_to_line_col_multiline() {
        let text = "Line 1\nLine 2\nLine 3";
        let range = TextRange::new(7.into(), 13.into()); // "Line 2"
        let (start_line, start_col, end_line, end_col) = range_to_line_col(text, range);
        assert_eq!(start_line, 1);
        assert_eq!(start_col, 0);
        assert_eq!(end_line, 1);
        assert_eq!(end_col, 6);
    }

    #[test]
    fn test_range_to_line_col_utf8() {
        let text = "Привет мир"; // Cyrillic (2 bytes per char)
                                 // "Привет мир" = П(0-1) р(2-3) и(4-5) в(6-7) е(8-9) т(10-11) пробел(12) м(13-14) и(15-16) р(17-18)
                                 // Characters: П(0), р(1), и(2), в(3), е(4), т(5), пробел(6), м(7), и(8), р(9)
                                 // Range from char 3 ("в") to char 9 ("р") → bytes 6 to 17
        let range = TextRange::new(6.into(), 17.into());
        let (start_line, start_col, end_line, end_col) = range_to_line_col(text, range);
        assert_eq!(start_line, 0);
        assert_eq!(start_col, 3); // Character position (в)
        assert_eq!(end_line, 0);
        assert_eq!(end_col, 9); // Character position (р)
    }
}
