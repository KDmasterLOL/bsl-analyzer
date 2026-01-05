//! Integration tests for module graph.

use crate::*;
use base_db::{RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use vfs::file_set::FileSet;
use vfs::{FileId, VfsPath};

// Test database implementation
#[salsa::db]
#[derive(Clone, Default)]
struct TestDatabase {
    storage: salsa::Storage<Self>,
    files: base_db::Files,
}

#[salsa::db]
impl salsa::Database for TestDatabase {}

#[salsa::db]
impl SourceDatabase for TestDatabase {
    fn file_text_input(&self, file_id: FileId) -> base_db::FileTextInput {
        self.files.file_text(file_id)
    }

    fn source_root_input(&self, source_root_id: SourceRootId) -> base_db::SourceRootInput {
        self.files.source_root(source_root_id)
    }

    fn file_source_root_input(&self, file_id: FileId) -> base_db::FileSourceRootInput {
        self.files.file_source_root(file_id)
    }

    fn set_file_text(&mut self, file_id: FileId, text: &str) {
        let files = self.files.clone();
        files.set_file_text(self, file_id, text);
    }

    fn set_file_source_root(&mut self, file_id: FileId, source_root_id: SourceRootId) {
        let files = self.files.clone();
        files.set_file_source_root(self, file_id, source_root_id);
    }

    fn set_source_root(&mut self, source_root_id: SourceRootId, source_root: SourceRoot) {
        let files = self.files.clone();
        files.set_source_root(self, source_root_id, source_root);
    }

    fn resolve_vfs_path(
        &self,
        source_root_id: SourceRootId,
        vfs_path: &vfs::VfsPath,
    ) -> Option<FileId> {
        let source_root_input = self.source_root_input(source_root_id);
        let vfs_path_str = vfs_path.as_path().to_string_lossy().to_string();
        base_db::resolve_vfs_path_query(self, source_root_input, vfs_path_str)
    }
}

#[salsa::db]
impl RootQueryDb for TestDatabase {
    fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode> {
        let input = self.file_text_input(file_id);
        base_db::parse_query(self, input)
    }

    fn sdbl_queries(&self, file_id: FileId) -> std::sync::Arc<Vec<syntax::SdblQueryInfo>> {
        let input = self.file_text_input(file_id);
        base_db::sdbl_queries_in_file(self, input)
    }

    fn method_regions(
        &self,
        file_id: FileId,
    ) -> std::sync::Arc<std::collections::HashMap<syntax::TextRange, String>> {
        let input = self.file_text_input(file_id);
        base_db::method_regions(self, input)
    }

    fn module_level_regions(&self, file_id: FileId) -> std::sync::Arc<Vec<base_db::RegionInfo>> {
        let input = self.file_text_input(file_id);
        base_db::module_level_regions(self, input)
    }
}

#[test]
fn test_build_simple_graph() {
    let mut builder = ModuleGraphBuilder::new();

    let m1 = builder.add_module(FileId(0), "Module1".to_string(), ModuleKind::CommonModule);
    let m2 = builder.add_module(FileId(1), "Module2".to_string(), ModuleKind::CommonModule);
    let m3 = builder.add_module(FileId(2), "Module3".to_string(), ModuleKind::CommonModule);

    // Build graph: m1 → m2 → m3
    builder.add_dependency(m1, m2, DependencyKind::DirectCall).unwrap();
    builder.add_dependency(m2, m3, DependencyKind::Import).unwrap();

    let graph = builder.build();

    assert_eq!(graph.len(), 3);

    // Check dependencies
    let m1_id = graph.by_file(FileId(0)).unwrap();
    let m2_id = graph.by_file(FileId(1)).unwrap();
    let m3_id = graph.by_file(FileId(2)).unwrap();

    assert_eq!(graph.dependencies(m1_id).len(), 1);
    assert_eq!(graph.dependencies(m1_id)[0].target, m2_id);
    assert_eq!(graph.dependencies(m1_id)[0].kind, DependencyKind::DirectCall);

    assert_eq!(graph.dependencies(m2_id).len(), 1);
    assert_eq!(graph.dependencies(m2_id)[0].target, m3_id);
    assert_eq!(graph.dependencies(m2_id)[0].kind, DependencyKind::Import);

    assert_eq!(graph.dependencies(m3_id).len(), 0);

    // Check reverse dependencies
    assert!(graph.reverse_dependencies(m1_id).is_empty());
    assert_eq!(graph.reverse_dependencies(m2_id), &[m1_id]);
    assert_eq!(graph.reverse_dependencies(m3_id), &[m2_id]);
}

#[test]
fn test_case_insensitive_lookup() {
    let mut builder = ModuleGraphBuilder::new();

    builder.add_module(FileId(0), "ОбщегоНазначения".to_string(), ModuleKind::CommonModule);

    let graph = builder.build();

    let id = graph.by_file(FileId(0)).unwrap();

    // All these lookups should find the same module
    assert_eq!(graph.by_name("ОбщегоНазначения"), Some(id));
    assert_eq!(graph.by_name("общегоназначения"), Some(id));
    assert_eq!(graph.by_name("ОБЩЕГОНАЗНАЧЕНИЯ"), Some(id));
    assert_eq!(graph.by_name("ОбЩеГоНаЗнАчЕнИя"), Some(id));
}

#[test]
fn test_diamond_dependency() {
    let mut builder = ModuleGraphBuilder::new();

    let m1 = builder.add_module(FileId(0), "Module1".to_string(), ModuleKind::CommonModule);
    let m2 = builder.add_module(FileId(1), "Module2".to_string(), ModuleKind::CommonModule);
    let m3 = builder.add_module(FileId(2), "Module3".to_string(), ModuleKind::CommonModule);
    let m4 = builder.add_module(FileId(3), "Module4".to_string(), ModuleKind::CommonModule);

    // Build diamond: m1 → m2 → m4
    //                    → m3 → m4
    builder.add_dependency(m1, m2, DependencyKind::DirectCall).unwrap();
    builder.add_dependency(m1, m3, DependencyKind::DirectCall).unwrap();
    builder.add_dependency(m2, m4, DependencyKind::DirectCall).unwrap();
    builder.add_dependency(m3, m4, DependencyKind::DirectCall).unwrap();

    let graph = builder.build();

    let m4_id = graph.by_file(FileId(3)).unwrap();

    // m4 should have 2 reverse dependencies (m2 and m3)
    assert_eq!(graph.reverse_dependencies(m4_id).len(), 2);
}

#[test]
fn test_all_modules_iterator() {
    let mut builder = ModuleGraphBuilder::new();

    for i in 0..10 {
        builder.add_module(FileId(i), format!("Module{}", i), ModuleKind::CommonModule);
    }

    let graph = builder.build();

    let count = graph.all_modules().count();
    assert_eq!(count, 10);

    // Check that we can collect names
    let names: Vec<String> = graph.iter().map(|(_, data)| data.name.clone()).collect();

    assert_eq!(names.len(), 10);
    assert!(names.contains(&"Module0".to_string()));
    assert!(names.contains(&"Module9".to_string()));
}

#[test]
fn test_build_graph_from_database() {
    // Create a test database with 3 BSL modules
    let mut db = TestDatabase::default();

    let file1 = FileId(0);
    let file2 = FileId(1);
    let file3 = FileId(2);

    // Module1.bsl - calls Module2
    let code1 = r#"
Процедура Тест()
    Модуль2.ВыполнитьДействие();
КонецПроцедуры
"#;

    // Module2.bsl - calls Module3
    let code2 = r#"
Процедура ВыполнитьДействие()
    Module3.Сохранить();
КонецПроцедуры
"#;

    // Module3.bsl - no dependencies
    let code3 = r#"
Процедура Сохранить()
    Сообщить("Сохранено");
КонецПроцедуры
"#;

    // Set up file set with paths
    let mut file_set = FileSet::new();
    file_set.insert(file1, VfsPath::new("CommonModules/Module1.bsl"));
    file_set.insert(file2, VfsPath::new("CommonModules/Модуль2.bsl"));
    file_set.insert(file3, VfsPath::new("CommonModules/Module3.bsl"));

    let source_root = SourceRoot::new_local(file_set);
    let source_root_id = SourceRootId(0);

    db.set_source_root(source_root_id, source_root.clone());
    db.set_file_source_root(file1, source_root_id);
    db.set_file_source_root(file2, source_root_id);
    db.set_file_source_root(file3, source_root_id);

    db.set_file_text(file1, code1);
    db.set_file_text(file2, code2);
    db.set_file_text(file3, code3);

    // Build the graph
    let graph = build_module_graph(&db, &source_root);

    // Verify graph structure
    assert_eq!(graph.len(), 3);

    // Check module names
    let module1_id = graph.by_name("Module1").expect("Module1 should exist");
    let module2_id = graph.by_name("Модуль2").expect("Модуль2 should exist");
    let module3_id = graph.by_name("Module3").expect("Module3 should exist");

    // Check dependencies: Module1 → Module2 → Module3
    let deps1 = graph.dependencies(module1_id);
    assert_eq!(deps1.len(), 1);
    assert_eq!(deps1[0].target, module2_id);
    assert_eq!(deps1[0].kind, DependencyKind::DirectCall);

    let deps2 = graph.dependencies(module2_id);
    assert_eq!(deps2.len(), 1);
    assert_eq!(deps2[0].target, module3_id);
    assert_eq!(deps2[0].kind, DependencyKind::DirectCall);

    let deps3 = graph.dependencies(module3_id);
    assert_eq!(deps3.len(), 0);

    // Check reverse dependencies
    assert_eq!(graph.reverse_dependencies(module1_id).len(), 0);
    assert_eq!(graph.reverse_dependencies(module2_id), &[module1_id]);
    assert_eq!(graph.reverse_dependencies(module3_id), &[module2_id]);
}

#[test]
fn test_build_graph_with_external_reference() {
    // Test that references to non-existent modules are ignored
    let mut db = TestDatabase::default();

    let file1 = FileId(0);

    // Module1.bsl - references ExternalModule that doesn't exist
    let code1 = r#"
Процедура Тест()
    ExternalModule.DoSomething();
КонецПроцедуры
"#;

    let mut file_set = FileSet::new();
    file_set.insert(file1, VfsPath::new("CommonModules/Module1.bsl"));

    let source_root = SourceRoot::new_local(file_set);
    let source_root_id = SourceRootId(0);

    db.set_source_root(source_root_id, source_root.clone());
    db.set_file_source_root(file1, source_root_id);
    db.set_file_text(file1, code1);

    // Build the graph
    let graph = build_module_graph(&db, &source_root);

    // Should have 1 module with 0 dependencies
    assert_eq!(graph.len(), 1);

    let module1_id = graph.by_name("Module1").unwrap();
    assert_eq!(graph.dependencies(module1_id).len(), 0);
}
