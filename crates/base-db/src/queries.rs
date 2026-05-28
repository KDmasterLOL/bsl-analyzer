//! Salsa tracked queries for base-db.
//!
//! These queries sit below HIR and expose parsed source-level facts.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use syntax::{Parse, SyntaxNode, TextRange};
use vfs::{FileId, VfsPath};

use crate::input::{FileTextInput, SourceRootInput};

/// Parse file text using the parser's shared token cache.
#[salsa::tracked(lru = 512)]
pub fn parse_query(db: &dyn salsa::Database, input: FileTextInput) -> Parse<SyntaxNode> {
    let _span = tracing::info_span!("parse").entered();

    let text = input.text(db);
    parser::parse_with_shared_cache(&text)
}

/// Map method source ranges to their root API region name.
///
/// Nested regions report the top-level region, not the immediate parent:
/// ```bsl
/// #Region Public
///     #Region Internal
///         Procedure MyProc()  // Maps to "Public" (root), not "Internal"
///         EndProcedure
///     #EndRegion
/// #EndRegion
/// ```
#[salsa::tracked(lru = 256)]
pub fn method_regions_query(
    db: &dyn salsa::Database,
    input: FileTextInput,
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

/// Resolve a path string to `FileId` within a source root.
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
