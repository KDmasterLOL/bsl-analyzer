use std::path::Path;

pub const SOURCE_EXTENSIONS: &[&str] = &["bsl"];

pub const METADATA_WATCHED_EXTENSIONS: &[&str] = &["xml"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRole {
    Source,
    MetadataWatched,
    Ignored,
}

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

pub fn is_bsl_source_path(path: &Path) -> bool {
    matches!(file_role(path), FileRole::Source)
}

pub fn is_metadata_path(path: &Path) -> bool {
    matches!(file_role(path), FileRole::MetadataWatched)
}

/// Whether `path` is a common module's body source — the
/// `…/CommonModules/<Name>/Ext/Module.bsl` layout the designer export uses.
///
/// Such a file is ordinary BSL source (so it flows through the source-change path,
/// not the metadata-XML one), yet creating or deleting it changes a common module's
/// per-MDO structure listing (its `module_file` reverse-index entry). Callers use
/// this to also refresh the metadata substrate on a common-module body add/remove.
pub fn is_common_module_body_path(path: &Path) -> bool {
    if path.file_name().and_then(|n| n.to_str()) != Some("Module.bsl") {
        return false;
    }
    let ext_dir = path.parent();
    let common_modules_dir = ext_dir.and_then(|p| p.parent()).and_then(|p| p.parent());
    ext_dir.and_then(|p| p.file_name()).and_then(|n| n.to_str()) == Some("Ext")
        && common_modules_dir.and_then(|p| p.file_name()).and_then(|n| n.to_str())
            == Some("CommonModules")
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
        assert_eq!(file_role(Path::new("/ws/M.BSL")), FileRole::Source);
        assert_eq!(file_role(Path::new("/ws/M.Bsl")), FileRole::Source);
        assert_eq!(file_role(Path::new("/ws/Form.XML")), FileRole::MetadataWatched);
        assert_eq!(file_role(Path::new("/ws/Form.Xml")), FileRole::MetadataWatched);
    }

    #[test]
    fn recognizes_common_module_body_layout() {
        assert!(is_common_module_body_path(Path::new(
            "/ws/cf/CommonModules/ОбщегоНазначения/Ext/Module.bsl"
        )));
        // Extension roots use the same layout.
        assert!(is_common_module_body_path(Path::new(
            "/ws/cfe/A/CommonModules/Расш/Ext/Module.bsl"
        )));
        // Not a common module body: object module, wrong filename, or wrong nesting.
        assert!(!is_common_module_body_path(Path::new(
            "/ws/cf/Catalogs/Товары/Ext/ManagerModule.bsl"
        )));
        assert!(!is_common_module_body_path(Path::new("/ws/cf/CommonModules/М/Ext/Form.bsl")));
        assert!(!is_common_module_body_path(Path::new("/ws/cf/CommonModules/Module.bsl")));
        assert!(!is_common_module_body_path(Path::new("/ws/cf/Reports/CommonModules.bsl")));
    }

    #[test]
    fn extension_lists_have_no_overlap() {
        for src in SOURCE_EXTENSIONS {
            assert!(
                !METADATA_WATCHED_EXTENSIONS.iter().any(|m| m.eq_ignore_ascii_case(src)),
                "{src} appears in both source and metadata extension lists",
            );
        }
    }
}
