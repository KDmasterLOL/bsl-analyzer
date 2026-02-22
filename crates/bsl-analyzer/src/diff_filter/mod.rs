//! Diff-based filtering for diagnostics.
//!
//! This module provides filtering of analysis to only include diagnostics
//! within changed lines (hunks) as reported by a diff tool.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Input JSON format from rtools diff-report.
#[derive(Debug, Deserialize)]
pub struct DiffFilterInput {
    pub base_ref: String,
    pub head_ref: String,
    pub files: HashMap<String, FileDiff>,
}

/// Per-file diff information.
#[derive(Debug, Deserialize)]
pub struct FileDiff {
    /// Changed line ranges (1-based, inclusive).
    /// None means new file - all lines are relevant.
    /// Empty vec means no changes - file should be skipped.
    pub hunks: Option<Vec<[u32; 2]>>,
}

/// A range of changed lines (1-based, inclusive).
#[derive(Debug, Clone)]
pub struct Hunk {
    /// Start line (1-based).
    pub start: u32,
    /// End line (1-based, inclusive).
    pub end: u32,
}

impl Hunk {
    /// Check if this hunk overlaps with a 0-based line range.
    ///
    /// # Arguments
    /// * `start_0based` - Start line (0-based).
    /// * `end_0based` - End line (0-based, inclusive).
    pub fn overlaps(&self, start_0based: u32, end_0based: u32) -> bool {
        // Convert 0-based to 1-based
        let start_1based = start_0based + 1;
        let end_1based = end_0based + 1;

        // No overlap if diagnostic ends before hunk starts or starts after hunk ends
        !(end_1based < self.start || start_1based > self.end)
    }
}

/// Runtime filter for diff-based diagnostics filtering.
#[derive(Debug, Clone)]
pub struct DiffFilter {
    pub base_ref: String,
    pub head_ref: String,
    /// Maps normalized paths to hunks.
    /// None means new file (all lines relevant).
    /// Some(empty vec) means no changes (skip file).
    files: HashMap<PathBuf, Option<Vec<Hunk>>>,
    /// Index by filename for O(1) lookup.
    /// Maps filename -> list of full paths with that filename.
    filename_index: HashMap<String, Vec<PathBuf>>,
}

impl DiffFilter {
    /// Load diff filter from a JSON file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let input: DiffFilterInput = serde_json::from_str(&content)?;

        let files: HashMap<PathBuf, Option<Vec<Hunk>>> = input
            .files
            .into_iter()
            .map(|(path_str, diff)| {
                let path = normalize_path(&path_str);
                let hunks = diff.hunks.map(|ranges| {
                    ranges.into_iter().map(|[start, end]| Hunk { start, end }).collect()
                });
                (path, hunks)
            })
            .collect();

        // Build filename index for fast lookup
        let mut filename_index: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for path in files.keys() {
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                filename_index.entry(filename.to_string()).or_default().push(path.clone());
            }
        }

        Ok(Self { base_ref: input.base_ref, head_ref: input.head_ref, files, filename_index })
    }

    /// Check if a file should be analyzed.
    ///
    /// Returns true if:
    /// - The file is in the diff filter (matched by suffix)
    /// - AND has changes (hunks is None for new file, or non-empty vec)
    pub fn should_analyze(&self, path: &Path) -> bool {
        match self.find_file(path) {
            Some(Some(hunks)) => !hunks.is_empty(), // Has changes
            Some(None) => true,                     // New file
            None => false,                          // Not in diff
        }
    }

    /// Check if a diagnostic is within changed lines.
    ///
    /// # Arguments
    /// * `path` - File path.
    /// * `start_line_0based` - Diagnostic start line (0-based).
    /// * `end_line_0based` - Diagnostic end line (0-based, inclusive).
    pub fn diagnostic_in_diff(
        &self,
        path: &Path,
        start_line_0based: u32,
        end_line_0based: u32,
    ) -> bool {
        match self.find_file(path) {
            Some(Some(hunks)) => {
                // Check if any hunk overlaps with the diagnostic range
                hunks.iter().any(|h| h.overlaps(start_line_0based, end_line_0based))
            }
            Some(None) => true, // New file - all diagnostics relevant
            None => false,      // Not in diff
        }
    }

    /// Find file in the filter by suffix matching.
    ///
    /// Uses filename index for O(1) average lookup instead of O(n) iteration.
    ///
    /// This handles cases where:
    /// - diff paths are relative to repo root: `src/cf/CommonModules/Foo/Module.bsl`
    /// - file paths are relative to source_dir: `CommonModules/Foo/Module.bsl`
    fn find_file(&self, path: &Path) -> Option<&Option<Vec<Hunk>>> {
        let normalized = normalize_path(&path.to_string_lossy());

        // First try exact match
        if let Some(hunks) = self.files.get(&normalized) {
            return Some(hunks);
        }

        // Use filename index for fast lookup
        let filename = path.file_name()?.to_str()?;
        let candidates = self.filename_index.get(filename)?;

        let normalized_str = normalized.to_string_lossy();

        // Check only candidates with matching filename
        for diff_path in candidates {
            let diff_str = diff_path.to_string_lossy();

            // Check if diff path ends with normalized path
            if diff_str.ends_with(&*normalized_str) {
                let prefix_len = diff_str.len() - normalized_str.len();
                if prefix_len == 0 || diff_str.as_bytes()[prefix_len - 1] == b'/' {
                    return self.files.get(diff_path);
                }
            }

            // Check reverse: normalized path ends with diff path
            if normalized_str.ends_with(&*diff_str) {
                let prefix_len = normalized_str.len() - diff_str.len();
                if prefix_len == 0 || normalized_str.as_bytes()[prefix_len - 1] == b'/' {
                    return self.files.get(diff_path);
                }
            }
        }

        None
    }
}

/// Normalize a path string to a consistent format.
/// - Convert Windows separators to Unix
/// - Remove leading ./ or ./
fn normalize_path(path: &str) -> PathBuf {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.strip_prefix("./").unwrap_or(&normalized);
    PathBuf::from(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hunk_overlaps() {
        // Hunk: lines 10-20 (1-based)
        let hunk = Hunk { start: 10, end: 20 };

        // Diagnostic fully inside hunk (0-based 9-15 = 1-based 10-16)
        assert!(hunk.overlaps(9, 15));

        // Diagnostic at start of hunk
        assert!(hunk.overlaps(9, 9));

        // Diagnostic at end of hunk
        assert!(hunk.overlaps(19, 19));

        // Diagnostic spanning hunk start
        assert!(hunk.overlaps(5, 12));

        // Diagnostic spanning hunk end
        assert!(hunk.overlaps(15, 25));

        // Diagnostic completely containing hunk
        assert!(hunk.overlaps(5, 25));

        // Diagnostic before hunk (0-based 0-7 = 1-based 1-8)
        assert!(!hunk.overlaps(0, 7));

        // Diagnostic after hunk (0-based 20-25 = 1-based 21-26)
        assert!(!hunk.overlaps(20, 25));

        // Edge case: diagnostic ends exactly before hunk
        assert!(!hunk.overlaps(0, 8)); // 1-based 1-9, hunk starts at 10

        // Edge case: diagnostic starts exactly after hunk
        assert!(!hunk.overlaps(20, 30)); // 1-based 21-31, hunk ends at 20
    }

    #[test]
    fn test_parse_diff_filter() {
        let json = r#"{
            "base_ref": "vendor",
            "head_ref": "develop",
            "files": {
                "Module.bsl": { "hunks": [[10, 25], [40, 50]] },
                "NewFile.bsl": { "hunks": null },
                "NoChanges.bsl": { "hunks": [] }
            }
        }"#;

        let input: DiffFilterInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.base_ref, "vendor");
        assert_eq!(input.head_ref, "develop");
        assert_eq!(input.files.len(), 3);

        // File with hunks
        let module = input.files.get("Module.bsl").unwrap();
        let hunks = module.hunks.as_ref().unwrap();
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0], [10, 25]);
        assert_eq!(hunks[1], [40, 50]);

        // New file (null hunks)
        let new_file = input.files.get("NewFile.bsl").unwrap();
        assert!(new_file.hunks.is_none());

        // No changes (empty hunks)
        let no_changes = input.files.get("NoChanges.bsl").unwrap();
        assert!(no_changes.hunks.as_ref().unwrap().is_empty());
    }

    /// Helper to create DiffFilter from JSON in tests
    fn filter_from_json(json: &str) -> DiffFilter {
        let input: DiffFilterInput = serde_json::from_str(json).unwrap();
        let files: HashMap<PathBuf, Option<Vec<Hunk>>> = input
            .files
            .into_iter()
            .map(|(p, d)| {
                let hunks = d
                    .hunks
                    .map(|h| h.into_iter().map(|[s, e]| Hunk { start: s, end: e }).collect());
                (PathBuf::from(p), hunks)
            })
            .collect();

        let mut filename_index: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for path in files.keys() {
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                filename_index.entry(filename.to_string()).or_default().push(path.clone());
            }
        }

        DiffFilter { base_ref: input.base_ref, head_ref: input.head_ref, files, filename_index }
    }

    #[test]
    fn test_should_analyze() {
        let json = r#"{
            "base_ref": "vendor",
            "head_ref": "develop",
            "files": {
                "Module.bsl": { "hunks": [[10, 25]] },
                "NewFile.bsl": { "hunks": null },
                "NoChanges.bsl": { "hunks": [] }
            }
        }"#;

        let filter = filter_from_json(json);

        // File with changes
        assert!(filter.should_analyze(Path::new("Module.bsl")));

        // New file
        assert!(filter.should_analyze(Path::new("NewFile.bsl")));

        // No changes
        assert!(!filter.should_analyze(Path::new("NoChanges.bsl")));

        // Not in diff
        assert!(!filter.should_analyze(Path::new("Other.bsl")));
    }

    #[test]
    fn test_diagnostic_in_diff() {
        let json = r#"{
            "base_ref": "vendor",
            "head_ref": "develop",
            "files": {
                "Module.bsl": { "hunks": [[10, 20], [30, 40]] },
                "NewFile.bsl": { "hunks": null }
            }
        }"#;

        let filter = filter_from_json(json);

        // Diagnostic in first hunk (0-based 9-15 = 1-based 10-16)
        assert!(filter.diagnostic_in_diff(Path::new("Module.bsl"), 9, 15));

        // Diagnostic in second hunk
        assert!(filter.diagnostic_in_diff(Path::new("Module.bsl"), 29, 35));

        // Diagnostic between hunks
        assert!(!filter.diagnostic_in_diff(Path::new("Module.bsl"), 21, 28));

        // Diagnostic before first hunk
        assert!(!filter.diagnostic_in_diff(Path::new("Module.bsl"), 0, 5));

        // New file - all diagnostics relevant
        assert!(filter.diagnostic_in_diff(Path::new("NewFile.bsl"), 0, 100));

        // Not in diff
        assert!(!filter.diagnostic_in_diff(Path::new("Other.bsl"), 0, 100));
    }

    #[test]
    fn test_normalize_path() {
        // Windows separators
        assert_eq!(normalize_path("foo\\bar\\baz.bsl"), PathBuf::from("foo/bar/baz.bsl"));

        // Leading ./
        assert_eq!(normalize_path("./foo/bar.bsl"), PathBuf::from("foo/bar.bsl"));

        // Mixed
        assert_eq!(normalize_path(".\\foo\\bar.bsl"), PathBuf::from("foo/bar.bsl"));

        // Already normalized
        assert_eq!(normalize_path("foo/bar.bsl"), PathBuf::from("foo/bar.bsl"));
    }
}
