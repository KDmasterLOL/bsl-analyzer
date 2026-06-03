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
    fn extension_lists_have_no_overlap() {
        for src in SOURCE_EXTENSIONS {
            assert!(
                !METADATA_WATCHED_EXTENSIONS.iter().any(|m| m.eq_ignore_ascii_case(src)),
                "{src} appears in both source and metadata extension lists",
            );
        }
    }
}
