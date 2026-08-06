use std::error::Error;

pub(super) fn build_workspace_code(
    source_path: &std::path::Path,
) -> Result<(usize, Vec<bsl_search::IndexedDocument>), Box<dyn Error + Send + Sync>> {
    use bsl_search::SearchEngine;

    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("baseline-sync.db");
    let mut engine = SearchEngine::fts_only(&db_path)?;
    let indexed_files = engine.index_directory_fts(source_path)?;
    let documents = engine.load_indexed_documents(Some("code"))?;
    Ok((indexed_files, documents))
}

pub(super) fn build_reference(
) -> Result<(usize, Vec<bsl_search::IndexedDocument>), Box<dyn Error + Send + Sync>> {
    use bsl_search::SearchEngine;

    let documents = build_reference_source_documents();

    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("reference-baseline-sync.db");
    let mut engine = SearchEngine::fts_only(&db_path)?;
    let indexed_files = engine.index_documents(
        "platform",
        "platform://docs",
        env!("CARGO_PKG_VERSION").as_bytes(),
        &documents,
        None,
    )?;
    let indexed_documents = engine.load_indexed_documents(Some("platform"))?;
    Ok((indexed_files, indexed_documents))
}

pub(super) fn build_reference_source_documents() -> Vec<bsl_search::Document> {
    use bsl_platform::PlatformDataInner;
    use bsl_search::Document;

    let platform = PlatformDataInner::instance();
    let mut documents = Vec::new();

    for ty in platform.all_types() {
        let methods = platform.get_type_methods(&ty.name);
        let method_list: String = methods
            .iter()
            .map(|method| format!("{} / {}", method.name, method.english_name))
            .collect::<Vec<_>>()
            .join(", ");

        documents.push(Document {
            title: format!("{} / {}", ty.name, ty.english_name),
            body: format!("Тип: {} / {}\nМетоды: {method_list}", ty.name, ty.english_name),
            kind: "type".to_owned(),
        });
    }

    for method in platform.all_methods() {
        let mut body = format!(
            "Тип: {}\nМетод: {} / {}\n",
            method.type_name, method.name, method.english_name
        );
        if let Some(ref ret) = method.return_type {
            body.push_str(&format!("Возвращает: {ret}\n"));
        }
        if let Some(docs) = platform.get_method_docs(method.id) {
            if !docs.syntax.is_empty() {
                body.push_str(&format!("Синтаксис: {}\n", docs.syntax));
            }
            if !docs.description.is_empty() {
                body.push_str(&format!("Описание: {}\n", docs.description));
            }
            for param in &docs.params {
                body.push_str(&format!("Параметр {}: {}\n", param.name, param.description));
            }
            for example in &docs.examples {
                body.push_str(&format!("Пример: {}\n", example.code));
            }
        }
        documents.push(Document {
            title: format!(
                "{}.{} / {}.{}",
                method.type_name, method.name, method.type_name, method.english_name
            ),
            body,
            kind: "method".to_owned(),
        });
    }

    for func in platform.all_global_functions() {
        let mut body = format!("Глобальная функция: {} / {}\n", func.name, func.english_name);
        if let Some(ref ret) = func.return_type {
            body.push_str(&format!("Возвращает: {ret}\n"));
        }
        if let Some(docs) = platform.get_global_function_docs(func.id) {
            if !docs.syntax.is_empty() {
                body.push_str(&format!("Синтаксис: {}\n", docs.syntax));
            }
            if !docs.description.is_empty() {
                body.push_str(&format!("Описание: {}\n", docs.description));
            }
            for param in &docs.params {
                body.push_str(&format!("Параметр {}: {}\n", param.name, param.description));
            }
        }
        documents.push(Document {
            title: format!("{} / {}", func.name, func.english_name),
            body,
            kind: "global_function".to_owned(),
        });
    }

    documents
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    type Keys = BTreeSet<(String, String)>;

    fn write(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn module(name: &str) -> String {
        format!("Процедура {name}()\n    Сообщить(\"{name}\");\nКонецПроцедуры\n")
    }

    /// The keys the published corpus carries, one entry per file however many chunks it holds.
    fn published_keys(root: &Path) -> Keys {
        let (_, documents) = build_workspace_code(root).unwrap();
        documents.into_iter().map(|d| (d.root_id, d.path)).collect()
    }

    /// The keys the CONSUMER derives from the same tree: the daemon's own attribution
    /// (`WorkspaceRoots::root_of`) over the shared walk, deduplicated exactly as
    /// `workspace_overlay::scanned_files_from` does.
    fn consumed_keys(root: &Path) -> Keys {
        let (roots, _) = bsl_search::WorkspaceRoots::build(root, root, &[]);
        project_model::SourceSet::scan(std::slice::from_ref(&root.to_path_buf()))
            .files
            .iter()
            .filter(|file| file.role == project_model::FileRole::Source)
            .filter_map(|file| roots.root_of(&file.walked, &file.canonical))
            .map(|key| (key.root_id, key.path))
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn a_file_behind_a_directory_link_is_published() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("configuration");
        write(&root.join("CommonModules/Прямой/Ext/Module.bsl"), &module("Прямой"));
        let outside = dir.path().join("outside");
        write(&outside.join("Ссылочный.bsl"), &module("Ссылочный"));
        std::os::unix::fs::symlink(&outside, root.join("Связанные")).unwrap();

        let published = published_keys(&root);

        assert!(
            published.contains(&(String::new(), "Связанные/Ссылочный.bsl".to_owned())),
            "a module reachable only through a directory link belongs in the corpus, \
             because the consumer's walk sees it; published: {published:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_bsl_named_link_to_a_foreign_target_is_not_published() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("configuration");
        write(&root.join("CommonModules/Прямой/Ext/Module.bsl"), &module("Прямой"));
        write(&root.join("Заметки.txt"), &module("НеИсходник"));
        std::os::unix::fs::symlink(root.join("Заметки.txt"), root.join("Призрак.bsl")).unwrap();

        let published = published_keys(&root);

        assert!(
            !published.contains(&(String::new(), "Призрак.bsl".to_owned())),
            "a name that claims to be BSL over a target that is not gets no row: the key \
             would name a file no walk of the target's root can rebuild; published: {published:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_published_keys_are_the_keys_the_consumer_derives() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("configuration");
        write(&root.join("Настоящий/Module.bsl"), &module("Настоящий"));
        // An alias INSIDE the root: both spellings canonicalise under it, so the consumer
        // collapses them into one key.
        std::os::unix::fs::symlink(root.join("Настоящий"), root.join("Псевдоним")).unwrap();
        // An alias OUT of the root: the canonical path leaves it, so attribution falls back
        // to the walked spelling and the file keeps a key of its own.
        let outside = dir.path().join("outside");
        write(&outside.join("Внешний.bsl"), &module("Внешний"));
        std::os::unix::fs::symlink(&outside, root.join("Наружу")).unwrap();

        assert_eq!(
            published_keys(&root),
            consumed_keys(&root),
            "the writer of the baseline and its reader must agree on what a file is called; \
             a key only one of them can produce is a row the other can never match, update or drop"
        );
    }

    #[test]
    fn a_root_that_is_itself_a_file_publishes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let standalone = dir.path().join("Одиночный.bsl");
        write(&standalone, &module("Одиночный"));

        assert!(
            published_keys(&standalone).is_empty(),
            "attribution rejects a path equal to its own root, so a file root has no key the \
             consumer could ever resolve; publishing one row under the empty path puts a record \
             into the baseline that its only reader cannot reach"
        );
    }
}
