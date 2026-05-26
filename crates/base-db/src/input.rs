//! Salsa input types for the base database.

use vfs::file_set::FileSet;
use vfs::FileId;

/// Unique identifier for a source root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceRootId(pub u32);

/// A logical group of files that share the same Salsa durability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRoot {
    /// Whether files in this root should be treated as rarely changing.
    pub is_library: bool,

    /// Files that belong to this source root.
    file_set: FileSet,
}

impl SourceRoot {
    /// Create a source root for local project code.
    pub fn new_local(file_set: FileSet) -> Self {
        SourceRoot { is_library: false, file_set }
    }

    /// Create a source root for library code.
    pub fn new_library(file_set: FileSet) -> Self {
        SourceRoot { is_library: true, file_set }
    }

    /// Return the files in this source root.
    pub fn file_set(&self) -> &FileSet {
        &self.file_set
    }

    /// Iterate over file IDs in this source root.
    pub fn iter(&self) -> impl Iterator<Item = FileId> + '_ {
        self.file_set.iter()
    }

    /// Return the durability used for inputs from this source root.
    pub fn durability(&self) -> salsa::Durability {
        if self.is_library {
            salsa::Durability::HIGH
        } else {
            salsa::Durability::LOW
        }
    }
}

/// Salsa input for file text.
#[salsa::input(debug)]
pub struct FileTextInput {
    /// File contents.
    pub text: String,
}

/// Salsa input for source root data.
#[salsa::input(debug)]
pub struct SourceRootInput {
    /// Source root data.
    pub root: SourceRoot,
}

/// Salsa input for file-to-source-root mapping.
#[salsa::input(debug)]
pub struct FileSourceRootInput {
    /// Source root that owns this file.
    pub source_root_id: SourceRootId,
}

/// Interned `FileId` wrapper for tracked Salsa queries.
#[salsa::interned(debug)]
pub struct FileIdInput {
    /// Raw VFS file identifier.
    pub file_id: vfs::FileId,
}

/// Hashable diagnostics configuration for Salsa caching.
///
/// Diagnostic codes stay as strings to keep `base-db` independent from
/// `ide-diagnostics`. Sorted vectors keep the input deterministic and hashable.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiagnosticsConfigInput {
    /// Disabled diagnostic codes.
    pub disabled: Vec<String>,

    /// Diagnostics enabled despite being disabled by default.
    pub enabled: Vec<String>,

    /// Per-diagnostic parameters as `(code, json_string)` pairs.
    pub parameters: Vec<(String, String)>,

    /// Whether ordinary application checks are enabled.
    pub ordinary_app_support: bool,

    /// Iteration limit for flow-sensitive diagnostics.
    pub dataflow_max_iterations: usize,

    /// Locale for rendered diagnostic messages.
    pub locale: crate::Locale,
}

impl DiagnosticsConfigInput {
    /// Build from raw config data and normalize it for deterministic hashing.
    pub fn from_raw(
        disabled: impl IntoIterator<Item = String>,
        enabled: impl IntoIterator<Item = String>,
        parameters: impl IntoIterator<Item = (String, String)>,
        ordinary_app_support: bool,
        dataflow_max_iterations: usize,
        locale: crate::Locale,
    ) -> Self {
        let mut disabled: Vec<String> = disabled.into_iter().collect();
        disabled.sort();
        disabled.dedup();

        let mut enabled: Vec<String> = enabled.into_iter().collect();
        enabled.sort();
        enabled.dedup();

        let mut parameters: Vec<(String, String)> = parameters.into_iter().collect();
        parameters.sort_by(|a, b| a.0.cmp(&b.0));
        parameters.dedup_by(|a, b| a.0 == b.0);

        Self {
            disabled,
            enabled,
            parameters,
            ordinary_app_support,
            dataflow_max_iterations,
            locale,
        }
    }

    /// Return whether a diagnostic code is disabled.
    pub fn is_disabled(&self, code: &str) -> bool {
        self.disabled.binary_search_by(|c| c.as_str().cmp(code)).is_ok()
    }

    /// Return parameter JSON for a diagnostic code.
    pub fn get_parameters(&self, code: &str) -> Option<&str> {
        self.parameters
            .binary_search_by(|(c, _)| c.as_str().cmp(code))
            .ok()
            .map(|idx| self.parameters[idx].1.as_str())
    }
}

/// Interned diagnostics config for tracked Salsa queries.
#[salsa::interned(debug)]
pub struct DiagnosticsConfigId {
    /// Diagnostics configuration.
    pub config: DiagnosticsConfigInput,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use vfs::VfsPath;

    #[test]
    fn test_source_root_local() {
        let mut file_set = FileSet::new();
        file_set.insert(FileId(0), VfsPath::from(PathBuf::from("/test.bsl")));

        let root = SourceRoot::new_local(file_set);
        assert!(!root.is_library);
        assert_eq!(root.file_set().len(), 1);
    }

    #[test]
    fn test_source_root_library() {
        let mut file_set = FileSet::new();
        file_set.insert(FileId(0), VfsPath::from(PathBuf::from("/lib.bsl")));

        let root = SourceRoot::new_library(file_set);
        assert!(root.is_library);
        assert_eq!(root.file_set().len(), 1);
    }

    #[test]
    fn test_source_root_iter() {
        let mut file_set = FileSet::new();
        let id1 = FileId(0);
        let id2 = FileId(1);
        file_set.insert(id1, VfsPath::from(PathBuf::from("/test1.bsl")));
        file_set.insert(id2, VfsPath::from(PathBuf::from("/test2.bsl")));

        let root = SourceRoot::new_local(file_set);
        let ids: Vec<_> = root.iter().collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_source_root_id_eq() {
        let id1 = SourceRootId(0);
        let id2 = SourceRootId(0);
        let id3 = SourceRootId(1);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }
}
