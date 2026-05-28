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
