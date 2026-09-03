use std::sync::Arc;

use rustc_hash::FxHashMap;
use vfs::{FileId, Vfs, VfsPath};

pub mod cfe;
pub mod synthetic;
pub use cfe::{CfeFixture, CfeFixtureBuilder};
pub use synthetic::{SyntheticMethod, SyntheticModule, SyntheticModuleSpec};

#[derive(Debug, Default)]
pub struct Fixture {
    pub files: FxHashMap<FileId, FixtureFile>,
    pub vfs: Vfs,
}

#[derive(Debug, Clone)]
pub struct FixtureFile {
    pub path: VfsPath,
    pub content: Arc<str>,
}

impl Fixture {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(input: &str) -> Self {
        let mut fixture = Self::new();
        let mut current_path: Option<String> = None;
        let mut current_content = String::new();

        let mut first_line_for_file = false;
        for line in input.lines() {
            if let Some(path) = line.strip_prefix("//- ") {
                if let Some(path) = current_path.take() {
                    fixture.add_file(&path, &current_content);
                    current_content.clear();
                }
                current_path = Some(path.to_string());
                first_line_for_file = true;
            } else if current_path.is_some() {
                if !first_line_for_file {
                    current_content.push('\n');
                }
                first_line_for_file = false;
                current_content.push_str(line);
            }
        }

        if let Some(path) = current_path {
            fixture.add_file(&path, &current_content);
        }

        fixture
    }

    pub fn add_file(&mut self, path: &str, content: &str) {
        let vfs_path = VfsPath::new(path);
        let file_id = self.vfs.alloc_file_id(vfs_path.clone());
        let content: Arc<str> = Arc::from(content);
        self.vfs.set_file_contents(vfs_path.clone(), Some(content.clone()));
        self.files.insert(file_id, FixtureFile { path: vfs_path, content });
    }

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
