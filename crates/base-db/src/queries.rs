use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use syntax::{Parse, SyntaxNode, TextRange};
use vfs::{FileId, VfsPath};

use crate::input::{content_revision, FileIdInput, SourceRootInput};
use crate::SourceDatabase;

/// Read a file's text from disk verbatim — no BOM stripping, no normalization.
///
/// The single disk-read primitive shared by [`file_text_query`] and the
/// out-of-Salsa metadata bootstrap that computes content revisions. Routing both
/// through this function guarantees the bootstrap's `content_revision(read_disk_text(p))`
/// hash matches what `file_text_query` recomputes on a later disk re-read, so a
/// disk-backed file never spuriously fails [`assert_revision`].
pub fn read_disk_text(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

#[salsa::tracked(lru = 512)]
pub fn parse_query<'db>(db: &'db dyn SourceDatabase, input: FileIdInput<'db>) -> Parse<SyntaxNode> {
    let _span = tracing::info_span!("parse").entered();

    let text = file_text_query(db, input);
    parser::parse_with_shared_cache(&text)
}

/// A file's source text, keyed on its stable [`FileIdInput`] and triggered by its
/// content revision. Returns the in-memory overlay when one is registered
/// (open editor buffers, test fixtures); otherwise reads the file from disk and
/// verifies the bytes hash to the recorded revision before returning. The
/// hash-verify makes the disk read a pure function of the revision, so the memo
/// is LRU-evictable and re-derives soundly. A mismatch (the file changed under a
/// running analysis, or a deleted/unreadable file) is a hard error, never a
/// silently-mixed result.
#[salsa::tracked(lru = 512)]
pub fn file_text_query<'db>(db: &'db dyn SourceDatabase, input: FileIdInput<'db>) -> Arc<str> {
    let _span = tracing::info_span!("file_text").entered();
    let file_id = input.file_id(db);
    let want = db.file_revision_input(file_id).revision(db);

    if let Some(text_input) = db.try_file_text_input(file_id) {
        let text = text_input.text(db);
        assert_revision(file_id, &text, want);
        return Arc::from(text.as_str());
    }

    let path = disk_path(db, file_id);
    let text = read_disk_text(&path).unwrap_or_else(|err| {
        tracing::error!(?file_id, ?path, %err, "file_text: disk read failed");
        panic!("file_text: cannot read {path:?} for {file_id:?}: {err}")
    });
    assert_revision(file_id, &text, want);
    Arc::from(text)
}

fn assert_revision(file_id: FileId, text: &str, want: u64) {
    let got = content_revision(text);
    if got != want {
        panic!(
            "file_text revision mismatch for {file_id:?}: content changed under analysis \
             (recorded {want:#018x}, on-read {got:#018x})"
        );
    }
}

fn disk_path(db: &dyn SourceDatabase, file_id: FileId) -> PathBuf {
    let source_root_id = db.file_source_root_input(file_id).source_root_id(db);
    let root = db.source_root_input(source_root_id).root(db);
    let vfs_path = root.file_set().path_for_file(&file_id).unwrap_or_else(|| {
        panic!("file_text: {file_id:?} not present in its source root file set")
    });
    vfs_path.as_path().to_path_buf()
}

#[salsa::tracked(lru = 256)]
pub fn method_regions_query<'db>(
    db: &'db dyn SourceDatabase,
    input: FileIdInput<'db>,
) -> Arc<HashMap<TextRange, String>> {
    let _span = tracing::info_span!("method_regions").entered();

    let parse = parse_query(db, input);
    let root = parse.syntax_node();

    let mut map = HashMap::new();
    collect_methods_in_regions(&root, &mut Vec::new(), &mut map);

    tracing::debug!(count = map.len(), "Collected methods in API regions");

    Arc::new(map)
}

fn collect_methods_in_regions(
    node: &SyntaxNode,
    region_stack: &mut Vec<String>,
    map: &mut HashMap<TextRange, String>,
) {
    use syntax::{
        ast::{self, AstNode},
        SyntaxKind,
    };

    for child in node.children() {
        match child.kind() {
            SyntaxKind::PRE_REGION_DIR => {
                if let Some(region) = ast::PreRegionDir::cast(child.clone()) {
                    if region.is_start() {
                        if let Some(name) = region.name() {
                            region_stack.push(name);
                            collect_methods_in_regions(region.syntax(), region_stack, map);
                            region_stack.pop();
                        }
                    }
                }
            }
            SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF => {
                if let Some(root_region) = region_stack.first() {
                    if is_api_region(root_region) {
                        let range = child.text_range();
                        map.insert(range, root_region.clone());
                    }
                }
            }
            _ => {
                collect_methods_in_regions(&child, region_stack, map);
            }
        }
    }
}

fn is_api_region(name: &str) -> bool {
    const API_REGIONS: &[&str] =
        &["программныйинтерфейс", "public", "служебныйпрограммныйинтерфейс", "internal"];
    API_REGIONS.contains(&name.to_lowercase().as_str())
}

#[salsa::tracked(lru = 256)]
pub fn resolve_vfs_path_query(
    db: &dyn salsa::Database,
    source_root_input: SourceRootInput,
    vfs_path_str: String,
) -> Option<FileId> {
    let source_root = source_root_input.root(db);
    let file_set = source_root.file_set();
    let vfs_path = VfsPath::new(PathBuf::from(vfs_path_str));
    file_set.file_for_path(&vfs_path).copied()
}
