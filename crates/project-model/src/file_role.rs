//! File classification by role: source vs metadata vs ignored.
//!
//! This module is the single source of truth for "what kind of file is this"
//! decisions inside the analyzer. The vfs-notify loader uses it to populate
//! its `Directories.rules` (watch-only for XML, content-load for BSL); the
//! semantic layer uses it to filter Salsa file iterations down to BSL
//! sources; the LSP handlers use it to short-circuit `didOpen`/`didChange`
//! for non-BSL paths.
//!
//! Keeping the rules here lets the workspace, vfs-notify, hir-def and the
//! LSP entry points agree on the same classification — without each layer
//! re-deriving the rule set from a hardcoded extension list.

use std::path::Path;

/// Single source of truth: extensions that load as BSL source code into
/// Salsa. Used by `Directories.extensions` for content loading, by
/// `is_bsl_source_path`, and by the `Source` arm of [`file_role`].
pub const SOURCE_EXTENSIONS: &[&str] = &["bsl"];

/// Single source of truth: extensions that get registered for change-tracking
/// (watcher events) but never have their bytes copied into Salsa. The
/// `bsl-metadata` loader re-reads them from disk on demand. Used by
/// `Directories.rules` for the watch-only `FileRule`, by `is_metadata_path`,
/// and by the `MetadataWatched` arm of [`file_role`].
pub const METADATA_WATCHED_EXTENSIONS: &[&str] = &["xml"];

/// Role of a file in the analyzer's view of the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRole {
    /// BSL source code: lex/parse/HIR/diagnostics. Contents are loaded into
    /// the VFS as `Arc<str>` and ingested by Salsa via `FileTextInput`.
    Source,
    /// Configuration metadata (XML in 1C:Enterprise): tracked by file id
    /// for change events, but its bytes are never resident in the VFS —
    /// the metadata loader re-reads them straight from disk on demand.
    /// On ERP-scale workspaces this is the dominant memory saving over
    /// the legacy "load everything as `Arc<str>`" path.
    MetadataWatched,
    /// File is not relevant to the analyzer (binary, build artefact, an
    /// unknown extension). Not registered with the VFS at all.
    Ignored,
}

/// Classify `path` by role.
///
/// Pure path-based: no disk I/O. Case-insensitive on the extension to
/// match 1C:Enterprise's own behaviour on Windows / macOS filesystems
/// (`.BSL` and `.bsl` are the same file).
pub fn file_role(path: &Path) -> FileRole {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return FileRole::Ignored;
    };
    if SOURCE_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)) {
        FileRole::Source
    } else if METADATA_WATCHED_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)) {
        FileRole::MetadataWatched
    } else {
        FileRole::Ignored
    }
}

/// Returns `true` when `path` is a BSL source file (`FileRole::Source`).
///
/// This is the path-only predicate every workspace-wide scan that calls
/// `db.parse` / `db.item_tree` must use to filter inputs — non-BSL files
/// are scanned into VFS for change tracking but must never reach the BSL
/// parser.
pub fn is_bsl_source_path(path: &Path) -> bool {
    matches!(file_role(path), FileRole::Source)
}

/// Returns `true` when `path` is a watched metadata file
/// (`FileRole::MetadataWatched`).
pub fn is_metadata_path(path: &Path) -> bool {
    matches!(file_role(path), FileRole::MetadataWatched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_bsl_as_source() {
        assert_eq!(file_role(Path::new("/ws/CommonModules/X/Ext/Module.bsl")), FileRole::Source);
        assert!(is_bsl_source_path(Path::new("/ws/a.bsl")));
    }

    #[test]
    fn classifies_xml_as_metadata_watched() {
        assert_eq!(file_role(Path::new("/ws/Configuration.xml")), FileRole::MetadataWatched,);
        assert!(is_metadata_path(Path::new("/ws/Roles/R/Ext/Rights.xml")));
        assert!(!is_bsl_source_path(Path::new("/ws/Roles/R/Ext/Rights.xml")));
    }

    #[test]
    fn classifies_other_extensions_as_ignored() {
        // Regression: workspace_symbols/_index used to feed these to the BSL
        // parser, which triggered the iteration guard on large XML.
        for path in &[
            "/ws/README.md",
            "/ws/notes.txt",
            "/ws/scripts/build.sh",
            "/ws/.bsl-analyzer.json",
            "/ws/binary.dat",
        ] {
            assert_eq!(file_role(Path::new(path)), FileRole::Ignored, "{path}");
            assert!(!is_bsl_source_path(Path::new(path)), "{path}");
            assert!(!is_metadata_path(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn classifies_paths_without_extension_as_ignored() {
        assert_eq!(file_role(Path::new("/ws/NoExtensionAtAll")), FileRole::Ignored);
        assert!(!is_bsl_source_path(Path::new("/ws/NoExtensionAtAll")));
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        // 1C Designer ships files with `.bsl` consistently, but code on
        // case-insensitive filesystems (Windows, macOS default) can still
        // surface `.BSL` / `.Bsl` and `.XML` / `.Xml`. These must classify
        // identically to their lowercase counterparts.
        assert_eq!(file_role(Path::new("/ws/M.BSL")), FileRole::Source);
        assert_eq!(file_role(Path::new("/ws/M.Bsl")), FileRole::Source);
        assert_eq!(file_role(Path::new("/ws/Form.XML")), FileRole::MetadataWatched);
        assert_eq!(file_role(Path::new("/ws/Form.Xml")), FileRole::MetadataWatched);
    }

    #[test]
    fn extension_lists_have_no_overlap() {
        // A file extension belonging to *both* `SOURCE_EXTENSIONS` and
        // `METADATA_WATCHED_EXTENSIONS` would make `file_role` resolution
        // order-dependent. Keep the sets disjoint.
        for src in SOURCE_EXTENSIONS {
            assert!(
                !METADATA_WATCHED_EXTENSIONS.iter().any(|m| m.eq_ignore_ascii_case(src)),
                "{src} appears in both source and metadata extension lists",
            );
        }
    }
}
