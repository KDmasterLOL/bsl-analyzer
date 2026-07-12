use hir::{
    graph_index::{project_batch_method_call_pairs, GraphIndex},
    MethodCallPair, ModuleId,
};

use super::{clear_node_caches, CallHierarchyIndexBuildError};
use crate::graph::BatchDbOpener;

#[derive(Debug)]
pub struct CallHierarchyIndexModuleProjection {
    pub module: ModuleId,
    pub layout_hash: u64,
    pub pairs: Vec<MethodCallPair>,
}

pub fn reproject_call_hierarchy_index_modules(
    graph_index: &GraphIndex,
    batch_size: usize,
    changed_modules: &[ModuleId],
    open_batch: &mut BatchDbOpener<'_>,
) -> Result<Vec<CallHierarchyIndexModuleProjection>, CallHierarchyIndexBuildError> {
    let batch_size = batch_size.max(1);
    let pool = rayon::ThreadPoolBuilder::new().build()?;
    let mut projections = Vec::with_capacity(changed_modules.len());
    for batch in changed_modules.chunks(batch_size) {
        let db = open_batch(batch);
        let pairs = project_batch_method_call_pairs(&db, graph_index, batch);
        let mut refreshed_layouts = GraphIndex::new();
        refreshed_layouts.add_batch(&pool, &db, batch);
        for &module in batch {
            let layout_hash = refreshed_layouts
                .module_layout_hash(module)
                .ok_or(CallHierarchyIndexBuildError::MissingLayoutHash(module))?;
            let pairs = pairs
                .iter()
                .filter(|(caller, _)| caller.module == module)
                .map(|&(caller, target)| MethodCallPair::new(caller, target))
                .collect();
            projections.push(CallHierarchyIndexModuleProjection { module, layout_hash, pairs });
        }
        drop(db);
        clear_node_caches(&pool);
    }

    Ok(projections)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use vfs::{file_set::FileSet, FileId, VfsPath};

    use super::*;
    use crate::RootDatabaseImpl;

    const ROOT: SourceRootId = SourceRootId(0);

    #[test]
    fn call_hierarchy_index_catch_up_minimal_graph_index() {
        // Given: a body edit to one of two modules during a build.
        let changed_module = ModuleId::new(FileId(0));
        let unchanged_module = ModuleId::new(FileId(1));
        let modules = [changed_module, unchanged_module];
        let initial_text =
            "Процедура Первый()\nВторой();\nКонецПроцедуры\n\nПроцедура Второй()\nКонецПроцедуры";
        let changed_text =
            "Процедура Первый()\nКонецПроцедуры\n\nПроцедура Второй()\nКонецПроцедуры";
        let unchanged_text = "Процедура Второй()\nКонецПроцедуры";
        let mut target_index = GraphIndex::new();
        let base = batch_database(&modules, initial_text, unchanged_text);
        for &module in &modules {
            target_index.add_module(&base, module);
        }
        let batches = RefCell::new(Vec::new());
        let mut open_batch = |batch: &[ModuleId]| {
            batches.borrow_mut().push(batch.to_vec());
            batch_database(batch, changed_text, unchanged_text)
        };

        // When: catch-up reprojects only the edited module.
        let projections = reproject_call_hierarchy_index_modules(
            &target_index,
            1,
            &[changed_module],
            &mut open_batch,
        )
        .expect("catch-up projection");

        // Then: the unchanged module is not opened to rebuild a GraphIndex.
        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].module, changed_module);
        assert!(projections[0].pairs.is_empty());
        assert_eq!(*batches.borrow(), vec![vec![changed_module]]);
    }

    fn batch_database(
        batch: &[ModuleId],
        changed_text: &str,
        unchanged_text: &str,
    ) -> RootDatabaseImpl {
        let mut db = RootDatabaseImpl::new();
        let mut file_set = FileSet::new();
        file_set.insert(FileId(0), VfsPath::new("/src/Changed.bsl"));
        file_set.insert(FileId(1), VfsPath::new("/src/Unchanged.bsl"));
        db.set_source_root(ROOT, SourceRoot::new_local(file_set));
        for module in batch {
            let text = match module.file_id {
                FileId(0) => changed_text,
                FileId(1) => unchanged_text,
                _ => unreachable!("fixture contains two modules"),
            };
            db.set_file_source_root(module.file_id, ROOT);
            db.set_file_text(module.file_id, text);
        }
        db
    }
}
