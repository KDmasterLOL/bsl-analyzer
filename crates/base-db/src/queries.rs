//! Salsa tracked queries for base-db.
//!
//! This module contains all Salsa queries for parsing BSL files and extracting regions.
//! These queries form the foundation of the incremental computation system.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use syntax::{Parse, SyntaxNode, TextRange, TextSize};
use vfs::{FileId, VfsPath};

use crate::input::{FileTextInput, SourceRootInput};

/// Information about a region (name and source range).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionInfo {
    pub name: String,
    pub range: TextRange,
}

/// Salsa tracked query for parsing.
///
/// This query automatically depends on the FileTextInput and is cached with LRU (512 entries).
/// When file text changes, Salsa automatically invalidates this query.
#[salsa::tracked(lru = 512)]
pub fn parse_query(db: &dyn salsa::Database, input: FileTextInput) -> Parse<SyntaxNode> {
    let _span = tracing::info_span!("parse").entered();

    let text = input.text(db);
    parser::parse(&text)
}

/// Map from method source ranges to parent API region names.
///
/// This query identifies all methods (procedures/functions) that are inside
/// API regions (ПрограммныйИнтерфейс, Public, СлужебныйПрограммныйИнтерфейс, Internal)
/// and returns a mapping from their TextRange to the root region name.
///
/// # Performance
/// - LRU cache: 256 files (region analysis is inexpensive)
/// - Depends on: parse_query (automatic invalidation via Salsa)
/// - Recomputed only when file content changes
/// - Shared by region-based diagnostics (no redundant tree walking)
///
/// # Root Region Lookup
/// For nested regions, always returns the TOP-LEVEL (root) region name,
/// not the immediate parent. This matches bsl-language-server behavior.
///
/// Example:
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

/// Recursively collect methods in API regions.
///
/// Tracks region stack to identify root (first) region for nested structures.
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
                if child.kind() != SyntaxKind::PRE_REGION_DIR {
                    collect_methods_in_regions(&child, region_stack, map);
                }
            }
        }
    }
}

/// Check if a region name is an API region.
///
/// API regions (case-insensitive):
/// - ПрограммныйИнтерфейс / Public
/// - СлужебныйПрограммныйИнтерфейс / Internal
fn is_api_region(name: &str) -> bool {
    const API_REGIONS: &[&str] =
        &["программныйинтерфейс", "public", "служебныйпрограммныйинтерфейс", "internal"];
    API_REGIONS.contains(&name.to_lowercase().as_str())
}

/// Extract all module-level region names and their text ranges.
///
/// This query collects information about all top-level regions in a BSL file.
/// Only module-level regions are collected (nested regions inside methods are excluded).
///
/// # Performance
/// - LRU cache: 256 files (region collection is inexpensive)
/// - Depends on: parse_query (automatic invalidation via Salsa)
/// - Recomputed only when file content changes
/// - Shared by ALL region-based diagnostics (NonStandardRegion, DuplicateRegion, EmptyRegion, etc.)
///
/// # Returns
/// Vector of RegionInfo containing region names and their text ranges (first line only).
/// Ranges point to the #Область/#Region directive line.
#[salsa::tracked(lru = 256)]
pub fn module_level_regions_query(
    db: &dyn salsa::Database,
    input: FileTextInput,
) -> Arc<Vec<RegionInfo>> {
    let _span = tracing::info_span!("module_level_regions").entered();

    let parse = parse_query(db, input);
    let root = parse.syntax_node();

    let regions = collect_module_level_regions(&root);

    tracing::debug!(count = regions.len(), "Collected module-level regions");

    Arc::new(regions)
}

/// Collect module-level regions (not nested inside methods).
///
/// Walks root.children() (not descendants) to get only top-level regions.
/// For each region start directive, extracts name and range of first line.
fn collect_module_level_regions(root: &SyntaxNode) -> Vec<RegionInfo> {
    use syntax::{
        ast::{self, AstNode},
        SyntaxKind,
    };

    let mut regions = Vec::new();

    for child in root.children() {
        if child.kind() == SyntaxKind::PRE_REGION_DIR {
            if let Some(region) = ast::PreRegionDir::cast(child.clone()) {
                if region.is_start() {
                    if let Some(name) = region.name() {
                        let text = child.text().to_string();
                        let first_line = text.lines().next().unwrap_or(&text);
                        let first_line_len = first_line.len();

                        let start = child.text_range().start();
                        let end = start + TextSize::from(first_line_len as u32);
                        let range = TextRange::new(start, end);

                        regions.push(RegionInfo { name, range });
                    }
                }
            }
        }
    }

    regions
}

/// Resolve a VfsPath to FileId within a SourceRoot.
///
/// Searches the FileSet of a SourceRoot for a given VfsPath.
/// Used by diagnostics to resolve metadata URIs to FileIds.
///
/// # Performance
/// - O(1) FileSet lookup (HashMap)
/// - Cached by Salsa (LRU 256)
/// - Expected: < 1ms after first call
///
/// # Parameters
/// - `source_root_input`: The SourceRoot to search in
/// - `vfs_path_str`: String representation of VfsPath (PathBuf is not Hash)
///
/// # Returns
/// - `Some(FileId)` if path exists in FileSet
/// - `None` if path not found
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
