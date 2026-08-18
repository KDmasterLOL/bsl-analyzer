use hir::Semantics;
use ide_db::RootDatabase;
use syntax::{SyntaxNode, SyntaxToken, TextRange, TextSize};
use vfs::FileId;

use crate::reference_kind::{classify_reference_token, ReferenceKind};
use crate::references;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentHighlight {
    pub range: TextRange,
    pub kind: DocumentHighlightKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentHighlightKind {
    Text,
    Read,
    Write,
}

pub fn document_highlights<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Vec<DocumentHighlight> {
    let _span = tracing::info_span!("document_highlights", ?file_id).entered();

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

    references::find_references_in_file(db, file_id, &symbol)
        .into_iter()
        .filter_map(|loc| {
            let token = token_for_range(&root, loc.range)?;
            let kind = classify_highlight_token(&token);
            Some(DocumentHighlight { range: loc.range, kind })
        })
        .collect()
}

fn token_for_range(root: &SyntaxNode, range: TextRange) -> Option<SyntaxToken> {
    root.descendants_with_tokens()
        .filter_map(|it| it.into_token())
        .find(|token| token.text_range() == range)
}

/// The LSP vocabulary has no slot for a call, so a call is a read — exactly what
/// the classifier answered before it learned to name calls apart.
fn classify_highlight_token(token: &SyntaxToken) -> DocumentHighlightKind {
    match classify_reference_token(token) {
        ReferenceKind::Declaration => DocumentHighlightKind::Text,
        ReferenceKind::Write => DocumentHighlightKind::Write,
        ReferenceKind::Call | ReferenceKind::Read => DocumentHighlightKind::Read,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
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

    fn highlight_kinds(
        db: &RootDatabaseImpl,
        file_id: FileId,
        source: &str,
        name: &str,
    ) -> Vec<(String, DocumentHighlightKind)> {
        let offset = TextSize::from(source.find(name).unwrap() as u32);
        let mut highlights = document_highlights(db, file_id, offset);
        highlights.sort_by_key(|highlight| highlight.range.start());

        highlights
            .into_iter()
            .map(|highlight| {
                let start: u32 = highlight.range.start().into();
                let end: u32 = highlight.range.end().into();
                (source[start as usize..end as usize].to_string(), highlight.kind)
            })
            .collect()
    }

    fn classify_first_token(
        db: &RootDatabaseImpl,
        file_id: FileId,
        text: &str,
    ) -> DocumentHighlightKind {
        let root = db.parse(file_id).syntax_node();
        let token = root
            .descendants_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|token| token.text() == text)
            .unwrap();

        classify_highlight_token(&token)
    }

    #[test]
    fn local_variable_highlights_text_write_and_read() {
        let source = r#"
Процедура Тест()
    Перем МояПеременная;

    МояПеременная = 10;
    Сообщить(МояПеременная);
КонецПроцедуры
"#;
        let (db, file_id) = create_db_with_file(source);

        let kinds = highlight_kinds(&db, file_id, source, "МояПеременная");

        assert_eq!(
            kinds,
            vec![
                ("МояПеременная".to_string(), DocumentHighlightKind::Text),
                ("МояПеременная".to_string(), DocumentHighlightKind::Write),
                ("МояПеременная".to_string(), DocumentHighlightKind::Read),
            ]
        );
    }

    #[test]
    fn parameter_highlights_text_write_and_read() {
        let source = r#"
Процедура Тест(МойПараметр)
    МойПараметр = 10;
    Сообщить(МойПараметр);
КонецПроцедуры
"#;
        let (db, file_id) = create_db_with_file(source);

        let kinds = highlight_kinds(&db, file_id, source, "МойПараметр");

        assert_eq!(
            kinds,
            vec![
                ("МойПараметр".to_string(), DocumentHighlightKind::Text),
                ("МойПараметр".to_string(), DocumentHighlightKind::Write),
                ("МойПараметр".to_string(), DocumentHighlightKind::Read),
            ]
        );
    }

    #[test]
    fn implicit_local_highlights_write_and_read() {
        let source = r#"
Процедура Тест()
    НаборЗаписей = 10;
    Сообщить(НаборЗаписей);
КонецПроцедуры
"#;
        let (db, file_id) = create_db_with_file(source);

        let kinds = highlight_kinds(&db, file_id, source, "НаборЗаписей");

        assert_eq!(
            kinds,
            vec![
                ("НаборЗаписей".to_string(), DocumentHighlightKind::Write),
                ("НаборЗаписей".to_string(), DocumentHighlightKind::Read),
            ]
        );
    }

    #[test]
    fn indexed_assignment_does_not_mark_index_operands_as_write() {
        let source = r#"
Процедура Тест()
    Перем Массив, Индекс;

    Массив[Индекс] = 10;
    Сообщить(Индекс);
КонецПроцедуры
"#;
        let (db, file_id) = create_db_with_file(source);

        let kinds = highlight_kinds(&db, file_id, source, "Индекс");

        assert_eq!(
            kinds,
            vec![
                ("Индекс".to_string(), DocumentHighlightKind::Text),
                ("Индекс".to_string(), DocumentHighlightKind::Read),
                ("Индекс".to_string(), DocumentHighlightKind::Read),
            ]
        );
    }

    #[test]
    fn field_assignment_marks_tail_as_write_and_receiver_as_read() {
        let source = r#"
Процедура Тест()
    Объект.Поле = 10;
КонецПроцедуры
"#;
        let (db, file_id) = create_db_with_file(source);

        assert_eq!(classify_first_token(&db, file_id, "Объект"), DocumentHighlightKind::Read);
        assert_eq!(classify_first_token(&db, file_id, "Поле"), DocumentHighlightKind::Write);
    }

    #[test]
    fn call_projects_onto_read() {
        let source = r#"
Процедура Тест()
    Помощник();
КонецПроцедуры

Процедура Помощник()
КонецПроцедуры
"#;
        let (db, file_id) = create_db_with_file(source);

        let root = db.parse(file_id).syntax_node();
        let call_offset = TextSize::from(source.find("Помощник();").unwrap() as u32);
        let call_token = root.token_at_offset(call_offset).right_biased().unwrap();
        assert_eq!(classify_reference_token(&call_token), ReferenceKind::Call);
        assert_eq!(classify_highlight_token(&call_token), DocumentHighlightKind::Read);
    }

    #[test]
    fn no_symbol_returns_empty_highlights() {
        let source = "Процедура Тест() КонецПроцедуры";
        let (db, file_id) = create_db_with_file(source);

        let offset = TextSize::from(source.find("Процедура").unwrap() as u32);
        let highlights = document_highlights(&db, file_id, offset);

        assert!(highlights.is_empty());
    }

    fn create_db_with_two_files(
        source_a: &str,
        path_a: &str,
        source_b: &str,
        path_b: &str,
    ) -> (RootDatabaseImpl, FileId, FileId) {
        let mut db = RootDatabaseImpl::default();
        let file_a = FileId(0);
        let file_b = FileId(1);

        let mut file_set = FileSet::new();
        file_set.insert(file_a, VfsPath::new(path_a));
        file_set.insert(file_b, VfsPath::new(path_b));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_a, SourceRootId(0));
        db.set_file_source_root(file_b, SourceRootId(0));
        db.set_file_text(file_a, source_a);
        db.set_file_text(file_b, source_b);

        (db, file_a, file_b)
    }

    #[test]
    fn document_highlights_do_not_cross_file_boundaries() {
        let source_a = r#"
Процедура ОбщийМетод() Экспорт
    ОбщийМетод();
КонецПроцедуры
"#;
        let source_b = r#"
Процедура ОбщийМетод() Экспорт
    ОбщийМетод();
КонецПроцедуры
"#;
        let (db, file_a, _file_b) =
            create_db_with_two_files(source_a, "/a.bsl", source_b, "/b.bsl");

        let offset = TextSize::from(source_a.find("ОбщийМетод").unwrap() as u32);
        let highlights = document_highlights(&db, file_a, offset);

        assert_eq!(
            highlights.len(),
            2,
            "expected definition + 1 call in file A, got {} highlights",
            highlights.len()
        );

        let source_len = source_a.len() as u32;
        for highlight in &highlights {
            let start: u32 = highlight.range.start().into();
            let end: u32 = highlight.range.end().into();
            assert!(
                end <= source_len,
                "highlight range {start}..{end} exceeds file A source length {source_len} — \
                 file-B range leaked into result"
            );
            assert_eq!(&source_a[start as usize..end as usize], "ОбщийМетод");
        }
    }

    #[test]
    fn document_highlights_on_call_site_stay_file_local() {
        let source_a = r#"
Процедура Вызов() Экспорт
    Помощник();
    Помощник();
КонецПроцедуры

Процедура Помощник() Экспорт
КонецПроцедуры
"#;
        let source_b = r#"
Процедура Помощник() Экспорт
    Помощник();
КонецПроцедуры
"#;
        let (db, file_a, _file_b) =
            create_db_with_two_files(source_a, "/caller.bsl", source_b, "/other.bsl");

        let first_call = source_a.find("Помощник();").unwrap();
        let offset = TextSize::from(first_call as u32);
        let highlights = document_highlights(&db, file_a, offset);

        assert_eq!(
            highlights.len(),
            3,
            "expected 2 calls + 1 definition in file A, got {} highlights",
            highlights.len()
        );

        let source_len = source_a.len() as u32;
        for highlight in &highlights {
            let start: u32 = highlight.range.start().into();
            let end: u32 = highlight.range.end().into();
            assert!(
                end <= source_len,
                "highlight range {start}..{end} exceeds file A source length {source_len}"
            );
            assert_eq!(&source_a[start as usize..end as usize], "Помощник");
        }
    }
}
