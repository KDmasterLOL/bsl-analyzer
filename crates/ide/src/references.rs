use hir::{
    normalize_match_name, normalize_usage_name, Name, ReferenceScope, SemanticSymbol, Semantics,
};
use ide_db::RootDatabase;
use syntax::TextSize;
use vfs::FileId;

use crate::Location;

pub fn find_references<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Vec<Location> {
    let _span = tracing::info_span!("find_references", ?file_id).entered();

    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    let token = match root.token_at_offset(offset).right_biased() {
        Some(t) if t.kind().is_name_token() => t,
        _ => return Vec::new(),
    };

    let sema = Semantics::new(db);
    let symbol = match sema.symbol_for_token(file_id, &token) {
        Some(symbol) => symbol,
        None => return Vec::new(),
    };

    let scope = symbol.reference_scope(db);
    tracing::debug!(
        ?scope,
        target_name = %symbol.name.as_str(),
        "Reference scope determined"
    );

    let files_to_search: Vec<FileId> = match scope {
        ReferenceScope::FileLocal => vec![file_id],
        ReferenceScope::Unknown => return Vec::new(),
        ReferenceScope::ModuleSymbolWorkspace => {
            workspace_candidate_files(db, file_id, &symbol.name)
        }
    };

    let mut all_references = Vec::new();
    for &search_file_id in &files_to_search {
        db.unwind_if_revision_cancelled();
        let references = find_references_in_file(db, search_file_id, &symbol);
        all_references.extend(references);
    }

    tracing::info!(
        total_references = all_references.len(),
        files_searched = files_to_search.len(),
        "Find references completed"
    );

    all_references
}

fn workspace_candidate_files<DB: RootDatabase>(
    db: &DB,
    current_file: FileId,
    target_name: &Name,
) -> Vec<FileId> {
    let source_root_input = db.file_source_root_input(current_file);
    let source_root_id = source_root_input.source_root_id(db);
    let aggregator = db.name_usage_index(source_root_id);
    let normalized = normalize_usage_name(target_name);
    aggregator.files_with(&normalized).to_vec()
}

pub(crate) fn find_references_in_file<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    target_symbol: &SemanticSymbol,
) -> Vec<Location> {
    let _span = tracing::debug_span!("find_references_in_file", ?file_id).entered();

    // Popular names (standard event handlers) yield hundreds of candidate
    // files; the memoised per-file offsets replace a full token walk per
    // request with a lookup, so only the actual occurrences pay resolution.
    let normalized = normalize_match_name(&target_symbol.name);
    let occurrences = db.file_name_offsets(file_id);
    let offsets = occurrences.offsets(&normalized);
    if offsets.is_empty() {
        return Vec::new();
    }

    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let sema = Semantics::new(db);

    let mut references = Vec::new();

    for &offset in offsets {
        let Some(token) = root.token_at_offset(TextSize::from(offset)).right_biased() else {
            continue;
        };
        if !token.kind().is_name_token()
            || u32::from(token.text_range().start()) != offset
            || !Name::new(token.text()).eq_ignore_case(&target_symbol.name)
        {
            continue;
        }

        let Some(candidate_symbol) = sema.symbol_for_token(file_id, &token) else {
            continue;
        };

        if candidate_symbol.key == target_symbol.key {
            let range = token.text_range();
            references.push(Location { file_id, range });
        }
    }

    tracing::debug!(
        count = references.len(),
        target_name = %target_symbol.name.as_str(),
        "Found references"
    );

    references
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{file_set::FileSet, VfsPath};

    fn create_db_with_file(source: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::default();
        let file_id = FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, source);

        (db, file_id)
    }

    #[test]
    fn test_find_method_references() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция Тест()
    МояПроцедура();
    МояПроцедура();
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);

        let def_offset = source.find("МояПроцедура").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file_id, offset);

        assert!(
            references.len() >= 3,
            "Expected at least 3 references, found {}",
            references.len()
        );

        for loc in &references {
            assert_eq!(loc.file_id, file_id);
            assert!(!loc.range.is_empty());
        }
    }

    #[test]
    fn test_find_variable_references() {
        let source = r#"
Перем МодульнаяПеременная;

Процедура Тест()
    МодульнаяПеременная = 1;
    Результат = МодульнаяПеременная + 2;
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let decl_offset = source.find("МодульнаяПеременная").unwrap();
        let offset = TextSize::from(decl_offset as u32);

        let references = find_references(&db, file_id, offset);

        assert!(
            references.len() >= 3,
            "Expected at least 3 references, found {}",
            references.len()
        );

        for loc in &references {
            assert_eq!(loc.file_id, file_id);
        }
    }

    #[test]
    fn test_find_references_case_insensitive() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция Тест()
    мояпроцедура();
    МОЯПРОЦЕДУРА();
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);

        let call_offset = source.find("мояпроцедура").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let references = find_references(&db, file_id, offset);

        assert!(
            references.len() >= 3,
            "Expected at least 3 references, found {}",
            references.len()
        );
    }

    #[test]
    fn find_references_survive_edit_that_shifts_offsets() {
        let source = "Процедура МояПроцедура()\nКонецПроцедуры\n\nПроцедура Тест()\n    МояПроцедура();\nКонецПроцедуры\n";
        let (mut db, file_id) = create_db_with_file(source);

        let def_offset = source.find("МояПроцедура").unwrap();
        let before = find_references(&db, file_id, TextSize::from(def_offset as u32));
        assert_eq!(before.len(), 2);

        // Prepend a comment so every occurrence moves; memoised per-file
        // offsets must be recomputed, not replayed at the stale positions.
        let shifted = format!("// сдвиг\n{source}");
        db.set_file_text(file_id, &shifted);

        let def_offset = shifted.find("МояПроцедура").unwrap();
        let after = find_references(&db, file_id, TextSize::from(def_offset as u32));
        assert_eq!(after.len(), 2);
        for loc in &after {
            let start = usize::from(loc.range.start());
            assert_eq!(&shifted[start..start + "МояПроцедура".len()], "МояПроцедура");
        }
    }

    #[test]
    fn find_references_final_sigma_keeps_token_walk_semantics() {
        // Final-sigma pair: `eq_ignore_case`-equal tokens whose contextual
        // `to_lowercase` keys differ. Local-symbol keys normalise via
        // `fold_lower` (`SemanticSymbolKey::BodyLocal`), so the usage never
        // matches the declaration's key: exactly one reference, same as the
        // pre-offsets token walk. The offsets bucket uses the per-char fold
        // (`normalize_match_name`) so its candidate set equals the old
        // `eq_ignore_case` prefilter; this pins that neither a missed token
        // nor a new false match appears for such identifiers.
        let source = "Процедура Тест(ΟΔΟΣ)\n    Рез = οδοσ + 1;\nКонецПроцедуры\n";
        let (db, file_id) = create_db_with_file(source);

        let decl_offset = source.find("ΟΔΟΣ").unwrap();
        let references = find_references(&db, file_id, TextSize::from(decl_offset as u32));
        assert_eq!(references.len(), 1, "declaration only, got {references:?}");
        assert_eq!(usize::from(references[0].range.start()), decl_offset);
    }

    #[test]
    fn test_find_references_not_found() {
        let source = r#"
Процедура Тест()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let offset = source.find("Процедура").unwrap();
        let offset = TextSize::from(offset as u32);

        let references = find_references(&db, file_id, offset);
        assert!(references.is_empty());
    }

    #[test]
    fn test_find_references_from_usage() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция Тест()
    МояПроцедура();
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);

        let call_offset = source.rfind("МояПроцедура").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let references = find_references(&db, file_id, offset);

        assert!(
            references.len() >= 2,
            "Expected at least 2 references, found {}",
            references.len()
        );
    }

    #[test]
    fn test_find_references_function() {
        let source = r#"
Функция МояФункция()
    Возврат 1;
КонецФункции

Процедура Тест()
    Результат = МояФункция();
    Другой = МояФункция();
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let def_offset = source.find("МояФункция").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file_id, offset);

        assert!(
            references.len() >= 3,
            "Expected at least 3 references, found {}",
            references.len()
        );
    }

    #[test]
    fn test_find_parameter_references() {
        let source = r#"
Процедура Тест(МойПараметр)
    Если МойПараметр > 0 Тогда
        Результат = МойПараметр + 1;
    КонецЕсли;
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let param_offset = source.find("МойПараметр").unwrap();
        let offset = TextSize::from(param_offset as u32);

        let references = find_references(&db, file_id, offset);

        println!("Found {} parameter references", references.len());

        assert_eq!(
            references.len(),
            3,
            "Expected exactly 3 references (declaration + 2 usages), found {}",
            references.len()
        );
    }

    #[test]
    fn test_find_implicit_local_references_do_not_cross_methods() {
        let source = r#"
Процедура Первый()
    НаборЗаписей = 1;
    Сообщить(НаборЗаписей);
КонецПроцедуры

Процедура Второй()
    НаборЗаписей = 2;
    Сообщить(НаборЗаписей);
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let offset = TextSize::from(source.find("НаборЗаписей").unwrap() as u32);

        let references = find_references(&db, file_id, offset);

        assert_eq!(references.len(), 2, "implicit locals must be scoped to their body");
        for location in references {
            let start: u32 = location.range.start().into();
            assert!(
                start < source.find("Процедура Второй").unwrap() as u32,
                "reference from the second method leaked into the first method result"
            );
        }
    }

    #[test]
    fn test_find_implicit_local_references_split_by_inferred_type() {
        let source = r#"
Процедура Тест()
    НаборЗаписей = 1;
    Сообщить(НаборЗаписей);

    НаборЗаписей = "строка";
    Сообщить(НаборЗаписей);
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let second_assignment = source.find("НаборЗаписей = \"строка\"").unwrap() as u32;

        let first_offset = TextSize::from(source.find("НаборЗаписей").unwrap() as u32);
        let first_refs = find_references(&db, file_id, first_offset);
        assert_eq!(first_refs.len(), 2, "number-typed implicit local should stay separate");
        assert!(
            first_refs.iter().all(|loc| u32::from(loc.range.start()) < second_assignment),
            "string-typed occurrences leaked into number-typed references: {first_refs:?}"
        );

        let second_offset = TextSize::from(second_assignment);
        let second_refs = find_references(&db, file_id, second_offset);
        assert_eq!(second_refs.len(), 2, "string-typed implicit local should stay separate");
        assert!(
            second_refs.iter().all(|loc| u32::from(loc.range.start()) >= second_assignment),
            "number-typed occurrences leaked into string-typed references: {second_refs:?}"
        );
    }

    #[test]
    fn test_find_local_variable_references() {
        let source = r#"
Процедура Тест()
    Перем МояПеременная;

    МояПеременная = 10;
    Результат = МояПеременная * 2;
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let var_offset = source.find("МояПеременная").unwrap();
        let offset = TextSize::from(var_offset as u32);

        let references = find_references(&db, file_id, offset);

        println!("Found {} local variable references", references.len());

        assert_eq!(
            references.len(),
            3,
            "Expected exactly 3 references (declaration + 2 usages), found {}",
            references.len()
        );
    }

    #[test]
    fn test_find_references_no_false_positives() {
        let source = r#"
Перем Значение;

Процедура Тест1()
    Перем Значение;  // Local variable with same name

    Значение = 1;
КонецПроцедуры

Процедура Тест2()
    Значение = 2;  // Module variable
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let module_var_offset = source.find("Значение").unwrap();
        let offset = TextSize::from(module_var_offset as u32);

        let references = find_references(&db, file_id, offset);

        println!("Found {} module variable references", references.len());
        for (i, loc) in references.iter().enumerate() {
            let start: u32 = loc.range.start().into();
            let end: u32 = loc.range.end().into();
            let text = &source[start as usize..end as usize];
            println!("  Ref {}: offset={}, text={:?}", i, start, text);
        }

        assert!(
            references.len() >= 2,
            "Expected at least 2 references (module var), found {}",
            references.len()
        );
    }

    #[test]
    fn test_find_module_method_multiple_files() {
        let mut db = RootDatabaseImpl::default();

        let file1_id = FileId(0);
        let file1_source = r#"
Процедура МояПроцедура() Экспорт
    // Определение
КонецПроцедуры

Функция Тест1()
    МояПроцедура();  // Вызов в том же модуле
КонецФункции
        "#;

        let file2_id = FileId(1);
        let file2_source = r#"
Процедура МояПроцедура()
    // Другая процедура с тем же именем
КонецПроцедуры

Функция Тест2()
    МояПроцедура();  // Вызов локального метода
КонецПроцедуры
        "#;

        let mut file_set = FileSet::new();
        file_set.insert(file1_id, VfsPath::new("/module1.bsl"));
        file_set.insert(file2_id, VfsPath::new("/module2.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file1_id, SourceRootId(0));
        db.set_file_source_root(file2_id, SourceRootId(0));

        db.set_file_text(file1_id, file1_source);
        db.set_file_text(file2_id, file2_source);

        let def_offset = file1_source.find("МояПроцедура").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file1_id, offset);

        println!("Found {} references for file1 method", references.len());
        for (i, loc) in references.iter().enumerate() {
            println!("  Ref {}: file={:?}, range={:?}", i, loc.file_id, loc.range);
        }

        assert_eq!(references.len(), 2, "Expected 2 references in file1 only");

        for loc in &references {
            assert_eq!(loc.file_id, file1_id, "All references should be in file1");
        }
    }

    #[test]
    fn test_find_module_variable_multiple_files() {
        let mut db = RootDatabaseImpl::default();

        let file1_id = FileId(0);
        let file1_source = r#"
Перем МояПеременная Экспорт;

Процедура Тест1()
    МояПеременная = 10;
    Сообщить(МояПеременная);
КонецПроцедуры
        "#;

        let file2_id = FileId(1);
        let file2_source = r#"
Перем МояПеременная;

Процедура Тест2()
    МояПеременная = 20;
КонецПроцедуры
        "#;

        let mut file_set = FileSet::new();
        file_set.insert(file1_id, VfsPath::new("/module1.bsl"));
        file_set.insert(file2_id, VfsPath::new("/module2.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file1_id, SourceRootId(0));
        db.set_file_source_root(file2_id, SourceRootId(0));

        db.set_file_text(file1_id, file1_source);
        db.set_file_text(file2_id, file2_source);

        let def_offset = file1_source.find("МояПеременная").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file1_id, offset);

        println!("Found {} variable references for file1", references.len());

        assert_eq!(references.len(), 3, "Expected 3 references in file1 only");

        for loc in &references {
            assert_eq!(loc.file_id, file1_id, "All references should be in file1");
        }
    }

    #[test]
    fn test_local_symbols_only_in_current_file() {
        let mut db = RootDatabaseImpl::default();

        let file1_id = FileId(0);
        let file1_source = r#"
Процедура Метод1()
    Перем ЛокальнаяПеременная;
    ЛокальнаяПеременная = 1;
    Сообщить(ЛокальнаяПеременная);
КонецПроцедуры
        "#;

        let file2_id = FileId(1);
        let file2_source = r#"
Процедура Метод2()
    Перем ЛокальнаяПеременная;  // Same name, different scope
    ЛокальнаяПеременная = 2;
КонецПроцедуры
        "#;

        let mut file_set = FileSet::new();
        file_set.insert(file1_id, VfsPath::new("/local1.bsl"));
        file_set.insert(file2_id, VfsPath::new("/local2.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file1_id, SourceRootId(0));
        db.set_file_source_root(file2_id, SourceRootId(0));

        db.set_file_text(file1_id, file1_source);
        db.set_file_text(file2_id, file2_source);

        let def_offset = file1_source.find("ЛокальнаяПеременная").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file1_id, offset);

        println!("Found {} local variable references", references.len());

        assert_eq!(references.len(), 3, "Expected exactly 3 references in same file");

        for loc in &references {
            assert_eq!(
                loc.file_id, file1_id,
                "Local variable references should not cross file boundaries"
            );
        }
    }

    #[test]
    fn export_method_uses_name_usage_index_to_narrow_scope() {
        let mut db = RootDatabaseImpl::default();
        let file_a = FileId(0);
        let file_b = FileId(1);
        let file_c = FileId(2);

        let file_a_src = r#"
Процедура МояПроцедура() Экспорт
    МояПроцедура();
КонецПроцедуры
"#;
        let file_b_src = r#"
Процедура МояПроцедура()
    МояПроцедура();
КонецПроцедуры
"#;
        let file_c_src = r#"
Процедура НеПохожийМетод()
КонецПроцедуры
"#;

        let mut file_set = FileSet::new();
        file_set.insert(file_a, VfsPath::new("/a.bsl"));
        file_set.insert(file_b, VfsPath::new("/b.bsl"));
        file_set.insert(file_c, VfsPath::new("/c.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_a, SourceRootId(0));
        db.set_file_source_root(file_b, SourceRootId(0));
        db.set_file_source_root(file_c, SourceRootId(0));
        db.set_file_text(file_a, file_a_src);
        db.set_file_text(file_b, file_b_src);
        db.set_file_text(file_c, file_c_src);

        let def_offset = file_a_src.find("МояПроцедура").unwrap();
        let references = find_references(&db, file_a, TextSize::from(def_offset as u32));

        assert_eq!(references.len(), 2, "expected definition + 1 call in file A");
        for loc in &references {
            assert_eq!(loc.file_id, file_a);
        }
    }

    #[test]
    fn non_export_method_stays_file_local() {
        let mut db = RootDatabaseImpl::default();

        let file1_id = FileId(0);
        let file1_source = r#"
Процедура Помощник()
КонецПроцедуры

Процедура Тест1()
    Помощник();
КонецПроцедуры
"#;
        let file2_id = FileId(1);
        let file2_source = r#"
Процедура Помощник()
КонецПроцедуры

Процедура Тест2()
    Помощник();
КонецПроцедуры
"#;

        let mut file_set = FileSet::new();
        file_set.insert(file1_id, VfsPath::new("/a.bsl"));
        file_set.insert(file2_id, VfsPath::new("/b.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file1_id, SourceRootId(0));
        db.set_file_source_root(file2_id, SourceRootId(0));
        db.set_file_text(file1_id, file1_source);
        db.set_file_text(file2_id, file2_source);

        let def_offset = file1_source.find("Помощник").unwrap();
        let references = find_references(&db, file1_id, TextSize::from(def_offset as u32));

        assert_eq!(references.len(), 2, "definition + 1 call in file 1");
        for loc in &references {
            assert_eq!(
                loc.file_id, file1_id,
                "non-export procedure references must not cross file boundaries"
            );
        }
    }
}
