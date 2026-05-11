//! Common test utilities for diagnostic tests.
//!
//! This module provides helper functions for testing diagnostics,
//! including position calculation and assertion helpers.

use crate::{Diagnostic, DiagnosticCode, Severity};
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
    let provider = ide_db::SalsaProvider::new(&db, None);
    let ctx = crate::DiagnosticsContext::new(&config, file_id, &provider);
    check_fn(&ctx)
}

/// Run ALL diagnostics on test code.
pub fn check_hir_diagnostic(code: &str) -> Vec<Diagnostic> {
    check_ast_diagnostic(code, crate::diagnostics)
}

/// Deterministic snapshot format for diagnostic lists.
/// Per-diag block:
///   <DiagnosticCode> @ <l>:<c>..<l>:<c>
///     message: <text>
///     severity: <Severity>
/// Sorted by (start_line, start_col, end_line, end_col, code as u16,
/// severity, message). Range coords 1-based.
pub fn format_diags(source: &str, diags: &[Diagnostic]) -> String {
    let mut entries = diags
        .iter()
        .map(|diag| {
            let (start_line, start_col, end_line, end_col) = range_to_line_col(source, diag.range);
            FormattedDiag {
                diag,
                start_line: start_line + 1,
                start_col: start_col + 1,
                end_line: end_line + 1,
                end_col: end_col + 1,
            }
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| {
        (
            left.start_line,
            left.start_col,
            left.end_line,
            left.end_col,
            left.diag.code as u16,
            severity_sort_key(left.diag.severity),
            left.diag.message.as_str(),
        )
            .cmp(&(
                right.start_line,
                right.start_col,
                right.end_line,
                right.end_col,
                right.diag.code as u16,
                severity_sort_key(right.diag.severity),
                right.diag.message.as_str(),
            ))
    });

    let mut output = String::new();
    for (idx, entry) in entries.iter().enumerate() {
        if idx > 0 {
            output.push('\n');
        }
        use std::fmt::Write as _;
        writeln!(
            output,
            "{} @ {}:{}..{}:{}",
            entry.diag.code.as_str(),
            entry.start_line,
            entry.start_col,
            entry.end_line,
            entry.end_col
        )
        .expect("writing to String should not fail");
        writeln!(output, "  message: {}", entry.diag.message)
            .expect("writing to String should not fail");
        write!(output, "  severity: {}", entry.diag.severity.as_str())
            .expect("writing to String should not fail");
    }
    output
}

/// Snapshot all diagnostics for `source`.
pub fn check_diagnostics_snapshot(source: &str, expected: expect_test::Expect) {
    let diagnostics = check_hir_diagnostic(source);
    expected.assert_eq(&format_diags(source, &diagnostics));
}

/// Snapshot diagnostics filtered to a single DiagnosticCode.
pub fn check_diagnostics_snapshot_for(
    source: &str,
    code_filter: DiagnosticCode,
    expected: expect_test::Expect,
) {
    let diagnostics = check_hir_diagnostic(source);
    expected.assert_eq(&format_diags_for(source, &diagnostics, code_filter));
}

struct FormattedDiag<'a> {
    diag: &'a Diagnostic,
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
}

fn format_diags_for(source: &str, diags: &[Diagnostic], code_filter: DiagnosticCode) -> String {
    let filtered =
        diags.iter().filter(|diag| diag.code == code_filter).cloned().collect::<Vec<_>>();
    format_diags(source, &filtered)
}

fn severity_sort_key(severity: Severity) -> u8 {
    match severity {
        Severity::Blocker => 0,
        Severity::Critical => 1,
        Severity::Major => 2,
        Severity::Error => 3,
        Severity::Warning => 4,
        Severity::Information => 5,
        Severity::Hint => 6,
    }
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
    let provider = ide_db::SalsaProvider::new(&db, None);
    let ctx = crate::DiagnosticsContext::new(&config, test_file, &provider);

    crate::diagnostics(&ctx)
}

/// Run diagnostics on multi-file fixtures with custom module metadata for the test file.
pub fn check_metadata_diagnostic_with_fixtures<F>(
    metadata: hir::ModuleMetadata,
    fixture_text: &str,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&hir::ModuleMetadata, &crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    use std::rc::Rc;
    use std::sync::Arc;

    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use test_fixture::Fixture;

    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();

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

    let test_file = *fixture.files.keys().last().expect("Fixture should have at least one file");
    let metadata_arc = Arc::new(metadata);
    let config_rc = Rc::new(crate::DiagnosticsConfig::all_enabled());
    let provider_impl =
        MetadataTestProvider { db, metadata: Arc::clone(&metadata_arc), configuration: None };
    let ctx = crate::DiagnosticsContext::new(
        &config_rc,
        test_file,
        &provider_impl as &dyn ide_db::provider::AnalysisProvider,
    );

    check_fn(&metadata_arc, &ctx)
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

/// Run a single dataflow-based handler on test code.
///
/// Track 2 §1.6 Group C: alias of [`check_ast_diagnostic`] used by the
/// `set_privileged_mode` / `disable_safe_mode` handlers, which consume
/// the §1.2 saturating-counter lattice through `module_security_state`.
/// Because `SalsaProvider` already implements that accessor, the test
/// setup is identical to the AST path — the alias documents the
/// dataflow dependency at the call site.
pub fn check_dataflow_diagnostic<F>(code: &str, check_fn: F) -> Vec<Diagnostic>
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
///
/// Track 2 §1.7-A: optionally carries a [`bsl_metadata::Configuration`]
/// so handlers that consult `visible_configurations()` (notably
/// `PrivilegedModuleMethodCall`) can be exercised end-to-end. When
/// `configuration` is `Some`, both `configuration()` and
/// `visible_configurations()` surface it; otherwise both return the
/// default empty values.
struct MetadataTestProvider {
    db: ide_db::RootDatabaseImpl,
    metadata: std::sync::Arc<hir::ModuleMetadata>,
    configuration: Option<std::sync::Arc<bsl_metadata::Configuration>>,
}

impl ide_db::provider::AnalysisProvider for MetadataTestProvider {
    fn configuration(&self) -> Option<std::sync::Arc<bsl_metadata::Configuration>> {
        self.configuration.clone()
    }

    fn visible_configurations(
        &self,
        _file_id: vfs::FileId,
    ) -> Vec<ide_db::provider::VisibleConfig> {
        match &self.configuration {
            Some(cfg) => vec![ide_db::provider::VisibleConfig {
                name: None,
                root: std::path::PathBuf::from("/test"),
                configuration: std::sync::Arc::clone(cfg),
            }],
            None => Vec::new(),
        }
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

    fn call_summary(&self, module_id: hir::ModuleId) -> std::sync::Arc<hir::ModuleCallSummary> {
        use hir::DefDatabase;

        let file_id = module_id.file_id;
        let item_tree = self.db.item_tree(file_id);
        let module_bodies = self.db.module_bodies(module_id);
        let form_handlers: &[bsl_metadata::FormEventHandler] =
            self.metadata.form.as_ref().map(|form| form.event_handlers.as_slice()).unwrap_or(&[]);

        std::sync::Arc::new(hir::call_graph::extract_call_summary(
            &item_tree,
            &module_bodies,
            form_handlers,
        ))
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

    fn variable_docs(
        &self,
        variable_id: hir::VariableId,
    ) -> Option<std::sync::Arc<hir::VariableDocs>> {
        self.db.variable_docs(variable_id)
    }

    fn module_cfgs(&self, file_id: vfs::FileId) -> std::sync::Arc<hir::cfg::ModuleCfgs> {
        use ide_db::base_db::FileIdInput;
        use ide_db::RootDatabase;
        let input = FileIdInput::new(&self.db, file_id);
        self.db.module_cfgs(input)
    }

    fn module_path_terminates(
        &self,
        file_id: vfs::FileId,
    ) -> std::sync::Arc<hir::dataflow::path_terminates::ModulePathTerminates> {
        use ide_db::base_db::FileIdInput;
        use ide_db::RootDatabase;
        let input = FileIdInput::new(&self.db, file_id);
        self.db.module_path_terminates(input)
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

    // ========================================================================
    // Track 2 §1.4c — Security/effect Salsa accessors
    // ========================================================================

    fn method_effect_summary(
        &self,
        method: hir::MethodId,
    ) -> std::sync::Arc<hir::dataflow::effect_summary::EffectSummary> {
        let method_input = hir::MethodIdInput::new(&self.db, method);
        ide_db::effects::method_effect_summary_query(&self.db, method_input)
    }

    fn module_security_state(
        &self,
        file_id: vfs::FileId,
    ) -> std::sync::Arc<ide_db::effects::ModuleSecurityState> {
        use ide_db::base_db::FileIdInput;
        let input = FileIdInput::new(&self.db, file_id);
        ide_db::effects::module_security_state_query(&self.db, input)
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

    let provider_impl =
        MetadataTestProvider { db, metadata: Arc::clone(&metadata_arc), configuration: None };

    let ctx = crate::DiagnosticsContext::new(
        &config_rc,
        file_id,
        &provider_impl as &dyn ide_db::provider::AnalysisProvider,
    );

    check_fn(&metadata_arc, &ctx)
}

/// Track 2 §1.7-A — run a multi-file diagnostic with a synthetic
/// [`bsl_metadata::Configuration`] containing privileged `CommonModule`s.
/// End-to-end harness for handlers that consult both
/// `ctx.visible_configurations()` (the `is_privileged()` flag) AND
/// `ctx.resolve_qualified_path(...)` (which needs the privileged
/// module's body file in the VFS so workspace symbol resolution finds
/// the called method).
///
/// `caller_code` is the file under test, mounted at
/// `/CommonModules/Caller/Module.bsl`. Each `(name, body)` entry in
/// `privileged_modules` is mounted at `/CommonModules/<name>/Module.bsl`
/// AND added to the synthetic configuration with `privileged=true,
/// server=true`. Diagnostics are returned for the caller file only.
pub fn check_diagnostic_with_privileged_modules<F>(
    caller_code: &str,
    privileged_modules: &[(&str, &str)],
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: Fn(&crate::DiagnosticsContext) -> Vec<Diagnostic>,
{
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use std::sync::Arc;
    use vfs::{FileId, FileSet, VfsPath};

    let mut configuration = bsl_metadata::Configuration::new("TestConfig");
    for &(name, _) in privileged_modules {
        let module =
            bsl_metadata::CommonModule::builder().name(name).privileged(true).server(true).build();
        configuration.add_common_module(module);
    }

    let mut db = RootDatabaseImpl::new();
    let mut file_set = FileSet::default();

    // Mount caller file at id 0.
    let caller_file_id = FileId(0);
    file_set.insert(caller_file_id, VfsPath::new("/CommonModules/Caller/Module.bsl"));

    // Mount each privileged module body at /CommonModules/<name>/Module.bsl.
    let mut privileged_file_ids = Vec::with_capacity(privileged_modules.len());
    for (idx, (name, _body)) in privileged_modules.iter().enumerate() {
        let fid = FileId((idx + 1) as u32);
        file_set.insert(fid, VfsPath::new(format!("/CommonModules/{}/Module.bsl", name)));
        privileged_file_ids.push(fid);
    }

    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(caller_file_id, SourceRootId(0));
    db.set_file_text(caller_file_id, caller_code);
    for (fid, (_, body)) in privileged_file_ids.iter().zip(privileged_modules.iter()) {
        db.set_file_source_root(*fid, SourceRootId(0));
        db.set_file_text(*fid, body);
    }

    let metadata =
        make_common_module_metadata(bsl_metadata::CommonModule::builder().name("Caller").build());
    let metadata_arc = Arc::new(metadata);
    let config_rc = Rc::new(crate::DiagnosticsConfig::all_enabled());
    let provider_impl = MetadataTestProvider {
        db,
        metadata: Arc::clone(&metadata_arc),
        configuration: Some(Arc::new(configuration)),
    };
    let ctx = crate::DiagnosticsContext::new(
        &config_rc,
        caller_file_id,
        &provider_impl as &dyn ide_db::provider::AnalysisProvider,
    );

    check_fn(&ctx)
}

/// Run all diagnostics on `source` with a Configuration.xml-backed CommonModule layout.
///
/// `config_xml` is written as the project's `Configuration.xml`. Each provided
/// `(name, source)` CommonModule declared by `<CommonModule>Name</CommonModule>`
/// is mounted as `CommonModules/<Name>/Ext/Module.bsl`; when `config_xml` is a
/// skeleton with no CommonModule declarations, all provided modules are mounted.
pub fn check_with_config_xml(
    source: &str,
    config_xml: &str,
    common_modules: &[(&str, &str)],
) -> Vec<Diagnostic> {
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::metadata::intern_configuration_path;
    use ide_db::RootDatabaseImpl;
    use vfs::{FileId, FileSet, VfsPath};

    let project = ConfigXmlFixtureProject::new(config_xml, common_modules);
    let registered_modules = project.registered_modules();

    let mut db = RootDatabaseImpl::new();
    db.set_all_config_paths(vec![(None, project.root().to_path_buf())]);

    let mut file_set = FileSet::default();
    let caller_file_id = FileId(0);
    let caller_path = project.root().join("CommonModules/Caller/Ext/Module.bsl");
    file_set.insert(caller_file_id, VfsPath::new(caller_path.to_string_lossy().into_owned()));

    let mut module_file_ids = Vec::with_capacity(registered_modules.len());
    for (idx, (name, _body)) in registered_modules.iter().enumerate() {
        let fid = FileId((idx + 1) as u32);
        let path = project.root().join(format!("CommonModules/{}/Ext/Module.bsl", name));
        file_set.insert(fid, VfsPath::new(path.to_string_lossy().into_owned()));
        module_file_ids.push(fid);
    }

    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(caller_file_id, SourceRootId(0));
    db.set_file_text(caller_file_id, source);
    for (fid, (_, body)) in module_file_ids.iter().zip(registered_modules.iter()) {
        db.set_file_source_root(*fid, SourceRootId(0));
        db.set_file_text(*fid, body);
    }

    let config_path = project.root().to_string_lossy();
    let config_path_input =
        intern_configuration_path(&db, config_path.as_ref(), db.metadata_version());
    let provider = ide_db::SalsaProvider::new(&db, Some(config_path_input));
    let config = crate::DiagnosticsConfig::all_enabled();
    let ctx = crate::DiagnosticsContext::new(&config, caller_file_id, &provider);

    crate::diagnostics(&ctx)
}

/// Snapshot all diagnostics for a Configuration.xml-backed CommonModule layout.
pub fn check_snapshot_with_config_xml(
    source: &str,
    config_xml: &str,
    common_modules: &[(&str, &str)],
    expected: expect_test::Expect,
) {
    let diagnostics = check_with_config_xml(source, config_xml, common_modules);
    expected.assert_eq(&format_diags(source, &diagnostics));
}

/// Run all diagnostics on `source` with a base Configuration.xml plus visible CFE extensions.
///
/// The [`test_fixture::CfeFixtureBuilder`] output owns the real CFE tree.
/// This wrapper extends the Phase B
/// Configuration.xml project setup by registering `set_all_config_paths`
/// with the base config first and each extension after it.
pub fn check_with_cfe(source: &str, fixture: test_fixture::CfeFixture) -> Vec<Diagnostic> {
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::metadata::intern_configuration_path;
    use ide_db::RootDatabaseImpl;
    use vfs::{FileId, FileSet, VfsPath};

    materialize_cfe_loader_compat(&fixture);

    let mut db = RootDatabaseImpl::new();
    db.set_all_config_paths(fixture.config_paths());

    let mut file_set = FileSet::default();
    let caller_file_id = FileId(0);
    let caller_path = fixture.root().join("CommonModules/Caller/Ext/Module.bsl");
    file_set.insert(caller_file_id, VfsPath::new(caller_path.to_string_lossy().into_owned()));

    let mut module_files = Vec::new();
    for extension in fixture.extensions() {
        for module in extension.modules() {
            let fid = FileId((module_files.len() + 1) as u32);
            let path =
                extension.root().join(format!("CommonModules/{}/Ext/Module.bsl", module.name()));
            file_set.insert(fid, VfsPath::new(path.to_string_lossy().into_owned()));
            module_files.push((fid, module.source()));
        }
    }

    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(caller_file_id, SourceRootId(0));
    db.set_file_text(caller_file_id, source);
    for (fid, body) in module_files {
        db.set_file_source_root(fid, SourceRootId(0));
        db.set_file_text(fid, body);
    }

    let config_path = fixture.root().to_string_lossy();
    let config_path_input =
        intern_configuration_path(&db, config_path.as_ref(), db.metadata_version());
    let provider = ide_db::SalsaProvider::new(&db, Some(config_path_input));
    let config = crate::DiagnosticsConfig::all_enabled();
    let ctx = crate::DiagnosticsContext::new(&config, caller_file_id, &provider);

    crate::diagnostics(&ctx)
}

/// Snapshot all diagnostics for a CFE-backed CommonModule layout.
pub fn check_snapshot_with_cfe(
    source: &str,
    fixture: test_fixture::CfeFixture,
    expected: expect_test::Expect,
) {
    let diagnostics = check_with_cfe(source, fixture);
    expected.assert_eq(&format_diags(source, &diagnostics));
}

struct ConfigXmlFixtureProject {
    root: std::path::PathBuf,
    registered_modules: Vec<(String, String)>,
}

impl ConfigXmlFixtureProject {
    fn new(config_xml: &str, common_modules: &[(&str, &str)]) -> Self {
        let root = next_config_xml_fixture_root();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create config XML fixture root");
        let config_body = if config_xml.trim().is_empty() {
            minimal_configuration_xml()
        } else {
            config_xml.to_string()
        };
        std::fs::write(root.join("Configuration.xml"), config_body)
            .expect("write Configuration.xml");

        let declared_modules = declared_common_module_names(config_xml);
        let registered_modules = common_modules
            .iter()
            .filter(|(name, _)| {
                declared_modules.is_empty()
                    || declared_modules
                        .iter()
                        .any(|declared| declared.to_lowercase() == name.to_lowercase())
            })
            .map(|(name, body)| ((*name).to_string(), (*body).to_string()))
            .collect::<Vec<_>>();

        for (idx, (name, body)) in registered_modules.iter().enumerate() {
            let module_dir = root.join(format!("CommonModules/{name}"));
            let ext_dir = module_dir.join("Ext");
            std::fs::create_dir_all(&ext_dir).expect("create CommonModule Ext directory");
            std::fs::write(ext_dir.join("Module.bsl"), body).expect("write CommonModule body");
            std::fs::write(
                root.join(format!("CommonModules/{name}.xml")),
                common_module_xml(name, idx),
            )
            .expect("write CommonModule XML");
        }

        Self { root, registered_modules }
    }

    fn root(&self) -> &std::path::Path {
        &self.root
    }

    fn registered_modules(&self) -> Vec<(&str, &str)> {
        self.registered_modules.iter().map(|(name, body)| (name.as_str(), body.as_str())).collect()
    }
}

impl Drop for ConfigXmlFixtureProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn next_config_xml_fixture_root() -> std::path::PathBuf {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("bsl_analyzer_config_xml_{}_{}", std::process::id(), id))
}

fn minimal_configuration_xml() -> String {
    r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Configuration uuid="00000000-0000-0000-0000-000000000000">
        <Properties>
            <Name>TestConfiguration</Name>
        </Properties>
    </Configuration>
</MetaDataObject>"#
        .to_string()
}

fn declared_common_module_names(config_xml: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = config_xml;
    const OPEN: &str = "<CommonModule";
    const CLOSE: &str = "</CommonModule>";

    while let Some(start) = rest.find(OPEN) {
        rest = &rest[start + OPEN.len()..];
        let Some(next) = rest.chars().next() else { break };
        if !matches!(next, '>' | '/' | ' ' | '\t' | '\n' | '\r') {
            rest = &rest[next.len_utf8()..];
            continue;
        }

        let Some(open_end) = rest.find('>') else { break };
        let tag_tail = &rest[..open_end];
        let after_open = &rest[open_end + 1..];
        if tag_tail.trim_end().ends_with('/') {
            rest = after_open;
            continue;
        }

        let Some(close_start) = after_open.find(CLOSE) else { break };
        let name = after_open[..close_start].trim();
        if !name.is_empty() && !name.contains('<') {
            names.push(name.to_string());
        }
        rest = &after_open[close_start + CLOSE.len()..];
    }

    names
}

fn common_module_xml(name: &str, idx: usize) -> String {
    common_module_xml_with_privileged(name, idx, false)
}

fn common_module_xml_with_privileged(name: &str, idx: usize, privileged: bool) -> String {
    let uuid = format!("00000000-0000-0000-0000-{:012}", idx + 1);
    format!(
        r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <CommonModule uuid="{uuid}">
        <Properties>
            <Name>{}</Name>
            <Global>false</Global>
            <ClientManagedApplication>false</ClientManagedApplication>
            <ClientOrdinaryApplication>false</ClientOrdinaryApplication>
            <Server>true</Server>
            <ExternalConnection>false</ExternalConnection>
            <ServerCall>false</ServerCall>
            <Privileged>{}</Privileged>
            <ReturnValuesReuse>DontUse</ReturnValuesReuse>
        </Properties>
    </CommonModule>
</MetaDataObject>"#,
        escape_xml_text(name),
        privileged
    )
}

fn materialize_cfe_loader_compat(fixture: &test_fixture::CfeFixture) {
    let mut idx = 0usize;
    for extension in fixture.extensions() {
        for module in extension.modules() {
            let common_modules_dir = extension.root().join("CommonModules");
            let ext_dir = common_modules_dir.join(module.name()).join("Ext");
            std::fs::create_dir_all(&ext_dir).expect("create CFE CommonModule Ext directory");
            std::fs::write(ext_dir.join("Module.bsl"), module.source())
                .expect("write CFE CommonModule Ext body");

            let privileged = extension_config_marks_module_privileged(extension.config_xml());
            std::fs::write(
                common_modules_dir.join(format!("{}.xml", module.name())),
                common_module_xml_with_privileged(module.name(), idx, privileged),
            )
            .expect("write CFE CommonModule XML");
            idx += 1;
        }
    }
}

fn extension_config_marks_module_privileged(config_xml: &str) -> bool {
    config_xml.contains("<Privileged>true</Privileged>")
}

fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
mod format_diags_tests {
    use super::*;

    fn diag(
        code: DiagnosticCode,
        message: &str,
        severity: Severity,
        start: u32,
        end: u32,
    ) -> Diagnostic {
        Diagnostic {
            code,
            message: message.to_string(),
            severity,
            range: TextRange::new(start.into(), end.into()),
            tags: vec![],
            fixes: vec![],
        }
    }

    #[test]
    fn format_diags_empty_list() {
        assert_eq!(format_diags("", &[]), "");
    }

    #[test]
    fn format_diags_sort_stable_across_input_permutations() {
        let source = "abc\ndef\nghi";
        let first = diag(DiagnosticCode::LineLength, "third", Severity::Warning, 8, 9);
        let second = diag(DiagnosticCode::EmptyCodeBlock, "first", Severity::Major, 0, 1);
        let third = diag(DiagnosticCode::BadWords, "second", Severity::Critical, 4, 6);
        let ordered = vec![first.clone(), second.clone(), third.clone()];
        let shuffled = vec![third, first, second];

        assert_eq!(format_diags(source, &ordered), format_diags(source, &shuffled));
    }

    #[test]
    fn format_diags_ties_break_on_end_position() {
        let source = "abcdef";
        let longer = diag(DiagnosticCode::LineLength, "longer", Severity::Warning, 0, 4);
        let shorter = diag(DiagnosticCode::LineLength, "shorter", Severity::Warning, 0, 2);

        assert_eq!(
            format_diags(source, &[longer, shorter]),
            "LineLength @ 1:1..1:3\n  message: shorter\n  severity: Warning\nLineLength @ 1:1..1:5\n  message: longer\n  severity: Warning"
        );
    }

    #[test]
    fn check_diagnostics_snapshot_for_filters_by_code() {
        let source = "abcdef";
        let matching = diag(DiagnosticCode::LineLength, "kept", Severity::Warning, 0, 2);
        let non_matching = diag(DiagnosticCode::BadWords, "dropped", Severity::Major, 3, 4);

        expect_test::expect![[r#"
            LineLength @ 1:1..1:3
              message: kept
              severity: Warning"#]]
        .assert_eq(&format_diags_for(
            source,
            &[matching, non_matching],
            DiagnosticCode::LineLength,
        ));
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

    #[test]
    fn check_snapshot_with_config_xml_resolves_declared_common_module() {
        let config_xml = r#"
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Configuration uuid="00000000-0000-0000-0000-000000000000">
        <Properties>
            <Name>TestConfiguration</Name>
        </Properties>
        <ChildObjects>
            <CommonModule>ОбщийМодульТест</CommonModule>
        </ChildObjects>
    </Configuration>
</MetaDataObject>
"#;
        let common_module = r#"
Процедура Метод() Экспорт
КонецПроцедуры
"#;
        let source = r#"
#Область ПрограммныйИнтерфейс
// Тестирует вызов общего модуля.
Процедура Тест() Экспорт
    ОбщийМодульТест.Метод();
КонецПроцедуры
#КонецОбласти
"#;

        check_snapshot_with_config_xml(
            source,
            config_xml,
            &[("ОбщийМодульТест", common_module)],
            expect_test::expect![""],
        );
    }
}
