use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DiffFilterInput {
    pub base_ref: String,
    pub head_ref: String,
    pub files: HashMap<String, FileDiff>,
}

#[derive(Debug, Deserialize)]
pub struct FileDiff {
    pub hunks: Option<Vec<[u32; 2]>>,
}

#[derive(Debug, Clone)]
pub struct Hunk {
    pub start: u32,
    pub end: u32,
}

impl Hunk {
    pub fn overlaps(&self, start_0based: u32, end_0based: u32) -> bool {
        let start_1based = start_0based + 1;
        let end_1based = end_0based + 1;

        !(end_1based < self.start || start_1based > self.end)
    }
}

#[derive(Debug, Clone)]
pub struct DiffFilter {
    pub base_ref: String,
    pub head_ref: String,
    files: HashMap<PathBuf, Option<Vec<Hunk>>>,
    filename_index: HashMap<String, Vec<PathBuf>>,
}

impl DiffFilter {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let input: DiffFilterInput = serde_json::from_str(&content)?;

        Ok(Self::from_input(input))
    }

    fn from_input(input: DiffFilterInput) -> Self {
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

        let mut filename_index: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for path in files.keys() {
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                filename_index.entry(filename.to_string()).or_default().push(path.clone());
            }
        }

        Self { base_ref: input.base_ref, head_ref: input.head_ref, files, filename_index }
    }

    pub fn should_analyze(&self, path: &Path) -> bool {
        match self.find_file(path) {
            Some(Some(hunks)) => !hunks.is_empty(),
            Some(None) => true,
            None => false,
        }
    }

    pub fn diagnostic_in_diff(
        &self,
        path: &Path,
        start_line_0based: u32,
        end_line_0based: u32,
    ) -> bool {
        match self.find_file(path) {
            Some(Some(hunks)) => {
                hunks.iter().any(|hunk| hunk.overlaps(start_line_0based, end_line_0based))
            }
            Some(None) => true,
            None => false,
        }
    }

    fn find_file(&self, path: &Path) -> Option<&Option<Vec<Hunk>>> {
        let normalized = normalize_path(&path.to_string_lossy());

        if let Some(hunks) = self.files.get(&normalized) {
            return Some(hunks);
        }

        let filename = path.file_name()?.to_str()?;
        let candidates = self.filename_index.get(filename)?;
        let normalized_str = normalized.to_string_lossy();

        for diff_path in candidates {
            let diff_str = diff_path.to_string_lossy();

            if diff_str.ends_with(&*normalized_str) {
                let prefix_len = diff_str.len() - normalized_str.len();
                if prefix_len == 0 || diff_str.as_bytes()[prefix_len - 1] == b'/' {
                    return self.files.get(diff_path);
                }
            }

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
        let hunk = Hunk { start: 10, end: 20 };

        for (start, end) in [(9, 15), (9, 9), (19, 19), (5, 12), (15, 25), (5, 25)] {
            assert!(hunk.overlaps(start, end));
        }

        for (start, end) in [(0, 7), (20, 25), (0, 8), (20, 30)] {
            assert!(!hunk.overlaps(start, end));
        }
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

        let module = input.files.get("Module.bsl").unwrap();
        let hunks = module.hunks.as_ref().unwrap();
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0], [10, 25]);
        assert_eq!(hunks[1], [40, 50]);

        let new_file = input.files.get("NewFile.bsl").unwrap();
        assert!(new_file.hunks.is_none());

        let no_changes = input.files.get("NoChanges.bsl").unwrap();
        assert!(no_changes.hunks.as_ref().unwrap().is_empty());
    }

    fn filter_from_json(json: &str) -> DiffFilter {
        let input: DiffFilterInput = serde_json::from_str(json).unwrap();
        DiffFilter::from_input(input)
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

        assert!(filter.should_analyze(Path::new("Module.bsl")));
        assert!(filter.should_analyze(Path::new("NewFile.bsl")));
        assert!(!filter.should_analyze(Path::new("NoChanges.bsl")));
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

        assert!(filter.diagnostic_in_diff(Path::new("Module.bsl"), 9, 15));
        assert!(filter.diagnostic_in_diff(Path::new("Module.bsl"), 29, 35));
        assert!(!filter.diagnostic_in_diff(Path::new("Module.bsl"), 21, 28));
        assert!(!filter.diagnostic_in_diff(Path::new("Module.bsl"), 0, 5));
        assert!(filter.diagnostic_in_diff(Path::new("NewFile.bsl"), 0, 100));
        assert!(!filter.diagnostic_in_diff(Path::new("Other.bsl"), 0, 100));
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("foo\\bar\\baz.bsl"), PathBuf::from("foo/bar/baz.bsl"));
        assert_eq!(normalize_path("./foo/bar.bsl"), PathBuf::from("foo/bar.bsl"));
        assert_eq!(normalize_path(".\\foo\\bar.bsl"), PathBuf::from("foo/bar.bsl"));
        assert_eq!(normalize_path("foo/bar.bsl"), PathBuf::from("foo/bar.bsl"));
    }
}
