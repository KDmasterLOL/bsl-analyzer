//! Common test utilities for diagnostic tests.
//!
//! This module provides helper functions for testing diagnostics,
//! including position calculation and assertion helpers.

use crate::Diagnostic;
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

/// Run AST-based diagnostics on test code.
///
/// Créates a test database, parses the code, and runs the diagnostic function.
pub fn check_ast_diagnostic<F>(code: &str, check_fn: F) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    let config = crate::DiagnosticsConfig::all_enabled();
    check_ast_diagnostic_with_config(code, config, check_fn)
}

/// Run AST-based diagnostics on test code with custom configuration.
///
/// # Arguments
/// * `code` - BSL source code to test
/// * `config` - Diagnostic configuration
/// * `check_fn` - Diagnostic check function
pub fn check_ast_diagnostic_with_config<F>(
    code: &str,
    config: crate::DiagnosticsConfig,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use test_fixture::Fixture;

    // Create fixture
    let fixture_text = format!("//- /test.bsl\n{}", code);
    let fixture = Fixture::parse(&fixture_text);
    let file_id = fixture.first_file().expect("fixture should have at least one file");

    let mut db = RootDatabaseImpl::new();

    // Set up source root
    let mut file_set = vfs::FileSet::default();
    file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file content
    for (fid, file) in &fixture.files {
        db.set_file_text(*fid, &file.content);
    }

    // Run diagnostic
    let ctx = crate::DiagnosticsContext::new(&db, &config, file_id);

    check_fn(&ctx)
}

/// Run HIR-based diagnostics on test code with a custom check function.
///
/// Similar to `check_ast_diagnostic` but explicitly for HIR-based checks.
pub fn check_hir_diagnostic_with_fn<F>(code: &str, check_fn: F) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    check_ast_diagnostic(code, check_fn)
}

/// Run ALL diagnostics on test code (backward compatibility).
///
/// This function runs all registered diagnostics and returns all found issues.
/// Useful for integration tests that want to verify multiple diagnostics at once.
pub fn check_hir_diagnostic(code: &str) -> Vec<Diagnostic> {
    check_ast_diagnostic(code, crate::diagnostics)
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
///
/// Similar to `check_ast_diagnostic` but allows passing custom DiagnosticsConfig.
pub fn check_hir_diagnostic_with_config<F>(
    code: &str,
    config: crate::DiagnosticsConfig,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use test_fixture::Fixture;

    let fixture_text = format!("//- /test.bsl\n{}", code);
    let fixture = Fixture::parse(&fixture_text);
    let file_id = fixture.first_file().unwrap();

    let mut db = RootDatabaseImpl::new();

    // Set up source root for module_bodies to work
    let mut file_set = vfs::FileSet::default();
    file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file content
    for (fid, file) in &fixture.files {
        db.set_file_text(*fid, &file.content);
    }

    // Run diagnostic
    let ctx = crate::DiagnosticsContext::new(&db, &config, file_id);

    check_fn(&ctx)
}

/// Run diagnostics on test code with custom configuration.
///
/// Returns only diagnostics for the specified diagnostic code.
pub fn check_ast_diagnostic_filtered<F>(
    code: &str,
    diagnostic_code: crate::DiagnosticCode,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    let diagnostics = check_ast_diagnostic(code, check_fn);
    diagnostics.into_iter().filter(|d| d.code == diagnostic_code).collect()
}

/// Test helper for module-level metadata diagnostics.
///
/// Creates a minimal test environment with custom metadata and runs the check function.
///
/// # Arguments
/// * `metadata` - Module metadata to test with
/// * `file_text` - Source code content
/// * `check_fn` - Diagnostic check function that takes (&ModuleMetadata, &DiagnosticsContext)
pub fn check_metadata_diagnostic<F>(
    metadata: hir_def::ModuleMetadata,
    file_text: &str,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&hir_def::ModuleMetadata, &crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    use hir_def::{DefDatabase, ModuleMetadata};
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use std::sync::Arc;
    use test_fixture::Fixture;

    // Create fixture
    let fixture_text = format!("//- /test.bsl\n{}", file_text);
    let fixture = Fixture::parse(&fixture_text);
    let file_id = fixture.first_file().expect("fixture should have at least one file");

    let mut db = RootDatabaseImpl::new();

    // Set up source root
    let mut file_set = vfs::FileSet::default();
    file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file content
    for (fid, file) in &fixture.files {
        db.set_file_text(*fid, &file.content);
    }

    let metadata_arc = Arc::new(metadata);
    let config = Rc::new(crate::DiagnosticsConfig::all_enabled());

    // Create custom provider that returns our metadata
    struct MetadataTestProvider {
        db: RootDatabaseImpl,
        metadata: Arc<ModuleMetadata>,
    }

    impl ide_db::provider::AnalysisProvider for MetadataTestProvider {
        fn configuration(&self) -> Option<Arc<bsl_metadata::Configuration>> {
            None
        }

        fn workspace_symbols(
            &self,
            source_root_id: SourceRootId,
        ) -> Arc<hir_def::WorkspaceSymbols> {
            self.db.workspace_symbols(source_root_id)
        }

        fn module_index(&self, source_root_id: SourceRootId) -> Arc<hir_def::ModuleIndex> {
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

        fn item_tree(&self, file_id: vfs::FileId) -> Arc<hir_def::ItemTree> {
            use hir_def::DefDatabase;
            self.db.item_tree(file_id)
        }

        fn symbol_tree(&self, module_id: hir_def::ModuleId) -> Arc<hir_def::SymbolTree> {
            use hir_def::DefDatabase;
            self.db.symbol_tree(module_id)
        }

        fn region_tree(&self, file_id: vfs::FileId) -> Arc<hir_def::RegionTree> {
            use hir_def::DefDatabase;
            self.db.region_tree(file_id)
        }

        fn module_bodies(&self, module_id: hir_def::ModuleId) -> Arc<hir_def::ModuleBodies> {
            use hir_def::DefDatabase;
            self.db.module_bodies(module_id)
        }

        fn module_metadata(&self, _module_id: hir_def::ModuleId) -> Arc<ModuleMetadata> {
            Arc::clone(&self.metadata)
        }

        fn module_liveness_analysis(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<dataflow::liveness::ModuleLiveness> {
            use ide_db::base_db::FileIdInput;
            use ide_db::RootDatabase;
            let input = FileIdInput::new(&self.db, file_id);
            self.db.module_liveness_analysis(input)
        }

        fn module_reaching_definitions(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<dataflow::reaching_defs::ModuleReachingDefs> {
            use ide_db::base_db::FileIdInput;
            use ide_db::RootDatabase;
            let input = FileIdInput::new(&self.db, file_id);
            self.db.module_reaching_definitions(input)
        }

        fn reaching_definitions(
            &self,
            method_id: hir_def::MethodId,
        ) -> Option<Arc<dataflow::reaching_defs::ReachingDefsResult>> {
            use ide_db::RootDatabase;
            self.db.reaching_definitions(method_id)
        }

        fn line_index(&self, file_id: vfs::FileId) -> Arc<line_index::LineIndex> {
            use ide_db::base_db::SourceDatabase;
            let input = self.db.file_text_input(file_id);
            Arc::new(line_index::LineIndex::new(&input.text(&self.db)))
        }

        fn file_path(&self, _file_id: vfs::FileId) -> Option<String> {
            // For tests, returning None is fine
            None
        }

        fn file_source_root_id(&self, _file_id: vfs::FileId) -> SourceRootId {
            // For tests, return default source root
            SourceRootId(0)
        }

        fn module_level_regions(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<Vec<ide_db::base_db::RegionInfo>> {
            use base_db::RootQueryDb;
            self.db.module_level_regions(file_id)
        }

        fn sdbl_hir_in_file(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<Vec<(hir_def::SdblExprId, Arc<sdbl_hir::SdblPackage>)>> {
            use ide_db::RootDatabase;
            self.db.sdbl_hir_in_file(file_id)
        }

        fn all_sdbl_in_file(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<Vec<(hir_def::SdblExprId, syntax::SdblQueryInfo)>> {
            use ide_db::RootDatabase;
            self.db.all_sdbl_in_file(file_id)
        }

        fn module_data(&self, module_id: hir_def::ModuleId) -> Arc<hir_def::ModuleData> {
            use hir_def::DefDatabase;
            self.db.module_data(module_id)
        }

        fn method_docs(
            &self,
            method_id: hir_def::MethodId,
        ) -> Option<Arc<hir_def::docs::MethodDocs>> {
            self.db.method_docs(method_id)
        }

        fn module_cfgs(&self, file_id: vfs::FileId) -> Arc<cfg::ModuleCfgs> {
            use ide_db::base_db::FileIdInput;
            use ide_db::RootDatabase;
            let input = FileIdInput::new(&self.db, file_id);
            self.db.module_cfgs(input)
        }

        fn resolve_vfs_path(
            &self,
            source_root_id: SourceRootId,
            path: &vfs::VfsPath,
        ) -> Option<vfs::FileId> {
            use ide_db::base_db::SourceDatabase;
            self.db.resolve_vfs_path(source_root_id, path)
        }
    }

    let provider_impl = MetadataTestProvider { db, metadata: Arc::clone(&metadata_arc) };

    // Need to use with_provider to pass custom metadata provider
    // But we can't because db is moved into provider_impl
    // So we create the context with a reference to the inner db
    let ctx = crate::DiagnosticsContext::with_provider(
        &provider_impl.db,
        &config,
        file_id,
        &provider_impl as &dyn ide_db::provider::AnalysisProvider,
    );

    check_fn(&metadata_arc, &ctx)
}

/// Test helper for module-level metadata diagnostics with custom config.
///
/// Similar to check_metadata_diagnostic but allows passing custom DiagnosticsConfig.
pub fn check_metadata_diagnostic_with_config<F>(
    metadata: hir_def::ModuleMetadata,
    file_text: &str,
    config: crate::DiagnosticsConfig,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&hir_def::ModuleMetadata, &crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    use hir_def::{DefDatabase, ModuleMetadata};
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use std::sync::Arc;
    use test_fixture::Fixture;

    // Create fixture
    let fixture_text = format!("//- /test.bsl\n{}", file_text);
    let fixture = Fixture::parse(&fixture_text);
    let file_id = fixture.first_file().expect("fixture should have at least one file");

    let mut db = RootDatabaseImpl::new();

    // Set up source root
    let mut file_set = vfs::FileSet::default();
    file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file content
    for (fid, file) in &fixture.files {
        db.set_file_text(*fid, &file.content);
    }

    let metadata_arc = Arc::new(metadata);
    let config_rc = Rc::new(config);

    // Create custom provider that returns our metadata
    struct MetadataTestProvider {
        db: RootDatabaseImpl,
        metadata: Arc<ModuleMetadata>,
    }

    impl ide_db::provider::AnalysisProvider for MetadataTestProvider {
        fn configuration(&self) -> Option<Arc<bsl_metadata::Configuration>> {
            None
        }

        fn workspace_symbols(
            &self,
            source_root_id: SourceRootId,
        ) -> Arc<hir_def::WorkspaceSymbols> {
            self.db.workspace_symbols(source_root_id)
        }

        fn module_index(&self, source_root_id: SourceRootId) -> Arc<hir_def::ModuleIndex> {
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

        fn item_tree(&self, file_id: vfs::FileId) -> Arc<hir_def::ItemTree> {
            use hir_def::DefDatabase;
            self.db.item_tree(file_id)
        }

        fn symbol_tree(&self, module_id: hir_def::ModuleId) -> Arc<hir_def::SymbolTree> {
            use hir_def::DefDatabase;
            self.db.symbol_tree(module_id)
        }

        fn region_tree(&self, file_id: vfs::FileId) -> Arc<hir_def::RegionTree> {
            use hir_def::DefDatabase;
            self.db.region_tree(file_id)
        }

        fn module_bodies(&self, module_id: hir_def::ModuleId) -> Arc<hir_def::ModuleBodies> {
            use hir_def::DefDatabase;
            self.db.module_bodies(module_id)
        }

        fn module_metadata(&self, _module_id: hir_def::ModuleId) -> Arc<ModuleMetadata> {
            Arc::clone(&self.metadata)
        }

        fn module_liveness_analysis(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<dataflow::liveness::ModuleLiveness> {
            use ide_db::base_db::FileIdInput;
            use ide_db::RootDatabase;
            let input = FileIdInput::new(&self.db, file_id);
            self.db.module_liveness_analysis(input)
        }

        fn module_reaching_definitions(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<dataflow::reaching_defs::ModuleReachingDefs> {
            use ide_db::base_db::FileIdInput;
            use ide_db::RootDatabase;
            let input = FileIdInput::new(&self.db, file_id);
            self.db.module_reaching_definitions(input)
        }

        fn reaching_definitions(
            &self,
            method_id: hir_def::MethodId,
        ) -> Option<Arc<dataflow::reaching_defs::ReachingDefsResult>> {
            use ide_db::RootDatabase;
            self.db.reaching_definitions(method_id)
        }

        fn line_index(&self, file_id: vfs::FileId) -> Arc<line_index::LineIndex> {
            use ide_db::base_db::SourceDatabase;
            let input = self.db.file_text_input(file_id);
            Arc::new(line_index::LineIndex::new(&input.text(&self.db)))
        }

        fn file_path(&self, _file_id: vfs::FileId) -> Option<String> {
            None
        }

        fn file_source_root_id(&self, _file_id: vfs::FileId) -> SourceRootId {
            SourceRootId(0)
        }

        fn module_level_regions(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<Vec<ide_db::base_db::RegionInfo>> {
            use base_db::RootQueryDb;
            self.db.module_level_regions(file_id)
        }

        fn sdbl_hir_in_file(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<Vec<(hir_def::SdblExprId, Arc<sdbl_hir::SdblPackage>)>> {
            use ide_db::RootDatabase;
            self.db.sdbl_hir_in_file(file_id)
        }

        fn all_sdbl_in_file(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<Vec<(hir_def::SdblExprId, syntax::SdblQueryInfo)>> {
            use ide_db::RootDatabase;
            self.db.all_sdbl_in_file(file_id)
        }

        fn module_data(&self, module_id: hir_def::ModuleId) -> Arc<hir_def::ModuleData> {
            use hir_def::DefDatabase;
            self.db.module_data(module_id)
        }

        fn method_docs(
            &self,
            method_id: hir_def::MethodId,
        ) -> Option<Arc<hir_def::docs::MethodDocs>> {
            self.db.method_docs(method_id)
        }

        fn module_cfgs(&self, file_id: vfs::FileId) -> Arc<cfg::ModuleCfgs> {
            use ide_db::base_db::FileIdInput;
            use ide_db::RootDatabase;
            let input = FileIdInput::new(&self.db, file_id);
            self.db.module_cfgs(input)
        }

        fn resolve_vfs_path(
            &self,
            source_root_id: SourceRootId,
            path: &vfs::VfsPath,
        ) -> Option<vfs::FileId> {
            use ide_db::base_db::SourceDatabase;
            self.db.resolve_vfs_path(source_root_id, path)
        }
    }

    let provider_impl = MetadataTestProvider { db, metadata: Arc::clone(&metadata_arc) };

    let ctx = crate::DiagnosticsContext::with_provider(
        &provider_impl.db,
        &config_rc,
        file_id,
        &provider_impl as &dyn ide_db::provider::AnalysisProvider,
    );

    check_fn(&metadata_arc, &ctx)
}

// TODO: Temporarily disabled until FormTestProvider is updated to match AnalysisProvider trait
#[cfg(feature = "disabled-form-test-helper")]
/// Check diagnostics in a form module with specific form type.
///
/// This helper sets up the correct form metadata and module path.
pub fn check_diagnostics_in_form(
    form_type: bsl_metadata::FormType,
    code: &str,
    expected_positions: &[(u32, u32, u32)],
) {
    use bsl_metadata::Form;
    use hir_def::{DefDatabase, ModuleMetadata};
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use std::sync::Arc;
    use test_fixture::Fixture;
    use uuid::Uuid;
    use vfs::VfsPath;

    // Create fixture
    let fixture_text = format!("//- /test.bsl\n{}", code);
    let fixture = Fixture::parse(&fixture_text);
    let file_id = fixture.first_file().expect("fixture should have at least one file");

    let mut db = RootDatabaseImpl::new();

    // Set up source root with form module path
    let mut file_set = vfs::FileSet::default();
    file_set
        .insert(file_id, VfsPath::new("/Catalogs/Catalog1/Forms/FormElement/Ext/Form/Module.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    for (fid, file) in &fixture.files {
        db.set_file_text(*fid, &file.content);
    }

    // Create form metadata
    let form = Form::new("FormElement".to_string(), form_type, Uuid::nil());

    // Create module metadata
    let metadata = Arc::new(ModuleMetadata {
        module_type: bsl_metadata::ModuleType::FormModule,
        execution_context: None,
        common_module: None,
        mdo: None,
        register: None,
        form: Some(Arc::new(form)),
    });

    let config = Rc::new(crate::DiagnosticsConfig::default());

    // TODO: Temporarily disabled - FormTestProvider needs to be updated to match AnalysisProvider trait
    #[allow(unexpected_cfgs)]
    #[cfg(feature = "disabled-form-test-helper")]
    // Create provider that returns our custom metadata
    struct FormTestProvider {
        db: RootDatabaseImpl,
        metadata: Arc<ModuleMetadata>,
    }

    impl ide_db::provider::AnalysisProvider for FormTestProvider {
        fn configuration(&self) -> Option<Arc<bsl_metadata::Configuration>> {
            None
        }

        fn workspace_symbols(
            &self,
            source_root_id: SourceRootId,
        ) -> Arc<hir_def::WorkspaceSymbols> {
            self.db.workspace_symbols(source_root_id)
        }

        fn module_index(&self, source_root_id: SourceRootId) -> Arc<hir_def::ModuleIndex> {
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

        fn item_tree(&self, file_id: vfs::FileId) -> Arc<hir_def::ItemTree> {
            use hir_def::DefDatabase;
            self.db.item_tree(file_id)
        }

        fn symbol_tree(&self, module_id: hir_def::ModuleId) -> Arc<hir_def::SymbolTree> {
            use hir_def::DefDatabase;
            self.db.symbol_tree(module_id)
        }

        fn region_tree(&self, file_id: vfs::FileId) -> Arc<hir_def::RegionTree> {
            use hir_def::DefDatabase;
            self.db.region_tree(file_id)
        }

        fn module_bodies(&self, module_id: hir_def::ModuleId) -> Arc<hir_def::ModuleBodies> {
            use hir_def::DefDatabase;
            self.db.module_bodies(module_id)
        }

        fn module_metadata(&self, _module_id: hir_def::ModuleId) -> Arc<ModuleMetadata> {
            Arc::clone(&self.metadata)
        }

        fn module_liveness_analysis(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<dataflow::liveness::ModuleLiveness> {
            use ide_db::base_db::FileIdInput;
            use ide_db::RootDatabase;
            let input = FileIdInput::new(&self.db, file_id);
            self.db.module_liveness_analysis(input)
        }

        fn module_reaching_definitions(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<dataflow::reaching_defs::ModuleReachingDefs> {
            use ide_db::base_db::FileIdInput;
            use ide_db::RootDatabase;
            let input = FileIdInput::new(&self.db, file_id);
            self.db.module_reaching_definitions(input)
        }

        fn reaching_definitions(
            &self,
            file_id: vfs::FileId,
        ) -> Arc<dataflow::reaching_defs::ReachingDefinitions> {
            use ide_db::RootDatabase;
            self.db.reaching_definitions(file_id)
        }

        fn file_dependencies(&self, file_id: vfs::FileId) -> Arc<dataflow::FileDependencies> {
            use ide_db::RootDatabase;
            self.db.file_dependencies(file_id)
        }

        fn line_index(&self, file_id: vfs::FileId) -> Arc<line_index::LineIndex> {
            use ide_db::base_db::SourceDatabase;
            let input = self.db.file_text_input(file_id);
            input.line_index(&self.db)
        }
    }

    // Temporarily simplified version until FormTestProvider is fixed
    let ctx = crate::DiagnosticsContext::new(&db, &config, file_id);
    let diagnostics = crate::handlers::server_side_export_form_method::check(&ctx);

    // Check expected positions
    assert_eq!(
        diagnostics.len(),
        expected_positions.len(),
        "Expected {} diagnostics, got {}",
        expected_positions.len(),
        diagnostics.len()
    );

    for (i, (line, start_col, end_col)) in expected_positions.iter().enumerate() {
        assert_diagnostic_range(code, &diagnostics[i], *line, *start_col, *end_col);
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
        let text = "Привет мир"; // Cyrillic
        let range = TextRange::new(3.into(), 15.into());
        let (start_line, start_col, end_line, end_col) = range_to_line_col(text, range);
        assert_eq!(start_line, 0);
        assert_eq!(start_col, 3); // Character position
        assert_eq!(end_line, 0);
        assert_eq!(end_col, 9); // Character position (3 + 6)
    }
}
