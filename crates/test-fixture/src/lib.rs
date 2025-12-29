//! Test fixtures for bsl-analyzer.
//!
//! This crate provides utilities for creating test fixtures.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use vfs::{FileId, Vfs, VfsPath};

/// A test fixture with multiple files.
#[derive(Debug, Default)]
pub struct Fixture {
    pub files: FxHashMap<FileId, FixtureFile>,
    pub vfs: Vfs,
}

/// A single file in a fixture.
#[derive(Debug, Clone)]
pub struct FixtureFile {
    pub path: VfsPath,
    pub content: Arc<str>,
}

impl Fixture {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses a fixture from a string.
    ///
    /// Format:
    /// ```text
    /// //- /path/to/file.bsl
    /// file content here
    ///
    /// //- /path/to/another.bsl
    /// another file content
    /// ```
    pub fn parse(input: &str) -> Self {
        let mut fixture = Self::new();
        let mut current_path: Option<String> = None;
        let mut current_content = String::new();

        for line in input.lines() {
            if let Some(path) = line.strip_prefix("//- ") {
                // Save previous file if any
                if let Some(path) = current_path.take() {
                    fixture.add_file(&path, &current_content);
                    current_content.clear();
                }
                current_path = Some(path.to_string());
            } else if current_path.is_some() {
                if !current_content.is_empty() {
                    current_content.push('\n');
                }
                current_content.push_str(line);
            }
        }

        // Save last file
        if let Some(path) = current_path {
            fixture.add_file(&path, &current_content);
        }

        fixture
    }

    /// Adds a file to the fixture.
    pub fn add_file(&mut self, path: &str, content: &str) {
        let vfs_path = VfsPath::new(path);
        let file_id = self.vfs.alloc_file_id(vfs_path.clone());
        let content: Arc<str> = Arc::from(content);
        self.vfs.set_file_contents(vfs_path.clone(), Some(content.clone()));
        self.files.insert(file_id, FixtureFile { path: vfs_path, content });
    }

    /// Returns the first file ID.
    pub fn first_file(&self) -> Option<FileId> {
        self.files.keys().next().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixture_parse() {
        let fixture = Fixture::parse(
            r#"
//- /main.bsl
Процедура Тест()
КонецПроцедуры

//- /lib.bsl
Функция Foo()
    Возврат 42;
КонецФункции
"#,
        );

        assert_eq!(fixture.files.len(), 2);
    }
}
