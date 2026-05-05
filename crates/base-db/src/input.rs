//! Input types for the database - SourceRoot and related structures.
//!
//! A SourceRoot represents a logical grouping of files, typically corresponding
//! to a directory on the filesystem (like a project or a library).

use vfs::file_set::FileSet;
use vfs::FileId;

/// Unique identifier for a source root.
///
/// Source roots partition the set of all files into logical groups.
/// Each file belongs to exactly one source root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceRootId(pub u32);

/// A source root is a directory on the filesystem that contains related files.
///
/// Source roots are typically used to distinguish between:
/// - Local code (your project)
/// - Library code (external dependencies)
///
/// This distinction is important for setting appropriate Salsa durability levels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRoot {
    /// Whether this is a library (external dependencies vs local code).
    ///
    /// Library code is assumed to change rarely (HIGH durability in Salsa),
    /// while local code changes frequently (LOW durability).
    pub is_library: bool,

    /// The set of files in this source root.
    file_set: FileSet,
}

impl SourceRoot {
    /// Create a new source root for local (project) code.
    pub fn new_local(file_set: FileSet) -> Self {
        SourceRoot { is_library: false, file_set }
    }

    /// Create a new source root for library (external) code.
    pub fn new_library(file_set: FileSet) -> Self {
        SourceRoot { is_library: true, file_set }
    }

    /// Get the file set for this source root.
    pub fn file_set(&self) -> &FileSet {
        &self.file_set
    }

    /// Iterate over all file IDs in this source root.
    pub fn iter(&self) -> impl Iterator<Item = FileId> + '_ {
        self.file_set.iter()
    }

    /// Get Salsa durability level for this source root.
    ///
    /// Libraries (is_library = true) use HIGH durability - rarely change.
    /// User code (is_library = false) uses LOW durability - changes frequently.
    pub fn durability(&self) -> salsa::Durability {
        if self.is_library {
            salsa::Durability::HIGH
        } else {
            salsa::Durability::LOW
        }
    }
}

/// Salsa input for file text content.
///
/// This represents mutable base input that can be changed via setters.
/// When file text changes, Salsa automatically invalidates dependent queries.
#[salsa::input(debug)]
pub struct FileTextInput {
    /// The file text content (stored as String, Salsa handles efficiently)
    pub text: String,
}

/// Salsa input for source root data.
///
/// This represents the logical grouping of files into source roots.
/// Note: We store the SourceRoot directly, Salsa will intern it.
#[salsa::input(debug)]
pub struct SourceRootInput {
    /// The source root data
    pub root: SourceRoot,
}

/// Salsa input for file-to-source-root mapping.
///
/// This tracks which source root each file belongs to.
#[salsa::input(debug)]
pub struct FileSourceRootInput {
    /// The source root ID this file belongs to
    pub source_root_id: SourceRootId,
}

/// Salsa interned FileId for HIR queries.
///
/// This wrapper makes FileId compatible with Salsa tracked functions.
/// Salsa 0.25 requires parameters to tracked functions to be Salsa types,
/// so we wrap the raw FileId in a Salsa interned struct.
///
/// ## Usage
///
/// ```ignore
/// // In HIR queries:
/// #[salsa::tracked(lru = 512)]
/// pub fn item_tree_query(
///     db: &dyn salsa::Database,
///     file_id_input: FileIdInput,
/// ) -> Arc<ItemTree> {
///     let file_id = file_id_input.file_id(db);
///     // ... use file_id
/// }
/// ```
#[salsa::interned(debug)]
pub struct FileIdInput {
    /// The raw FileId value
    pub file_id: vfs::FileId,
}

/// Hashable diagnostics configuration for Salsa caching.
///
/// Uses String codes instead of DiagnosticCode enum to avoid circular dependencies
/// between base-db and ide-diagnostics. Conversion to DiagnosticCode happens at
/// the query call site.
///
/// ## Design rationale
///
/// HashMap can't be used in Salsa because:
/// 1. Iteration order is non-deterministic (violates Salsa's same-input→same-output guarantee)
/// 2. serde_json::Value doesn't implement Hash
///
/// Instead, we use sorted Vec which is:
/// - Deterministic (same data → same hash)
/// - Hashable (all fields implement Hash)
/// - Efficient for ~10 items (typical diagnostic config size)
///
/// ## Usage
///
/// ```ignore
/// let config = DiagnosticsConfigInput {
///     disabled: vec!["LineLength".to_string(), "MissingReturn".to_string()],
///     parameters: vec![
///         ("LineLength".to_string(), r#"{"maxLength":140}"#.to_string()),
///     ],
///     ordinary_app_support: false,
///     dataflow_max_iterations: 10000,
///     locale: Locale::Ru,
/// };
/// let config_id = DiagnosticsConfigId::new(db, config);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct DiagnosticsConfigInput {
    /// Disabled diagnostic codes (sorted alphabetically).
    ///
    /// Example: `["LineLength", "MissingReturn"]`
    pub disabled: Vec<String>,

    /// Enabled diagnostic codes (sorted alphabetically).
    /// Used to enable diagnostics that are disabled by default.
    ///
    /// Example: `["TernaryOperatorUsage", "BadWords"]`
    pub enabled: Vec<String>,

    /// Per-diagnostic parameters as (code, json_string) pairs, sorted by code.
    ///
    /// JSON string is used instead of serde_json::Value because Value doesn't implement Hash.
    /// The JSON is parsed at the call site when needed.
    ///
    /// Example: `[("LineLength", r#"{"maxLength":140}"#)]`
    pub parameters: Vec<(String, String)>,

    /// Support for ordinary application mode.
    ///
    /// When true, enables checks specific to ordinary (non-managed) applications.
    pub ordinary_app_support: bool,

    /// Max iterations for dataflow analysis.
    ///
    /// Used by flow-sensitive diagnostics to limit computation.
    /// Default: 10000 (sync with dataflow::DEFAULT_MAX_ITERATIONS).
    pub dataflow_max_iterations: usize,

    /// User-facing output locale (for diagnostic message rendering).
    ///
    /// Determines which language is used for primitive type names and other
    /// localized labels in diagnostic messages. Resolved at the LSP layer
    /// from `[output] display_language` (TOML) or `InitializeParams.locale`
    /// (LSP), with `Locale::default()` (= Ru) as a fallback.
    pub locale: crate::Locale,
}

impl DiagnosticsConfigInput {
    /// Create a new DiagnosticsConfigInput with default values.
    pub fn new() -> Self {
        Self {
            disabled: Vec::new(),
            enabled: Vec::new(),
            parameters: Vec::new(),
            ordinary_app_support: false,
            dataflow_max_iterations: 10000,
            locale: crate::Locale::default(),
        }
    }

    /// Build from raw config data, normalizing for deterministic hashing.
    ///
    /// - Sorts disabled/enabled lists alphabetically
    /// - Sorts parameters by code
    /// - Removes duplicates
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

    /// Check if a diagnostic code is disabled.
    ///
    /// Uses binary search for O(log n) lookup.
    pub fn is_disabled(&self, code: &str) -> bool {
        self.disabled.binary_search_by(|c| c.as_str().cmp(code)).is_ok()
    }

    /// Get parameter JSON for a diagnostic code.
    ///
    /// Uses binary search for O(log n) lookup.
    /// Returns None if no parameters are set for this code.
    pub fn get_parameters(&self, code: &str) -> Option<&str> {
        self.parameters
            .binary_search_by(|(c, _)| c.as_str().cmp(code))
            .ok()
            .map(|idx| self.parameters[idx].1.as_str())
    }
}

/// Salsa interned diagnostics config for caching.
///
/// This wrapper makes DiagnosticsConfigInput compatible with Salsa tracked functions.
/// Salsa interns the config, so identical configs share the same ID.
///
/// ## Usage
///
/// ```ignore
/// // Create config ID
/// let config = DiagnosticsConfigInput::new();
/// let config_id = DiagnosticsConfigId::new(db, config);
///
/// // Use in tracked function
/// #[salsa::tracked(lru = 256)]
/// pub fn file_diagnostics(
///     db: &dyn RootDatabase,
///     file_id_input: FileIdInput,
///     config_id: DiagnosticsConfigId,
/// ) -> Arc<Vec<Diagnostic>> {
///     let config = config_id.config(db);
///     // ... use config
/// }
/// ```
#[salsa::interned(debug)]
pub struct DiagnosticsConfigId {
    /// The diagnostics configuration.
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
