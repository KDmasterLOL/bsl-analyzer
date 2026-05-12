//! Document highlights implementation.

use hir::Semantics;
use ide_db::RootDatabase;
use syntax::ast_utils::field_tail_name_token;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextRange, TextSize};
use vfs::FileId;

use crate::references;

/// A same-document symbol highlight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentHighlight {
    pub range: TextRange,
    pub kind: DocumentHighlightKind,
}

/// Access kind for a document highlight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentHighlightKind {
    Text,
    Read,
    Write,
}

/// Returns same-document highlights for the symbol at the given position.
///
/// LSP `textDocument/documentHighlight` is defined as same-document only, so this
/// function never crosses file boundaries — no workspace fan-out, no `find_references`
/// orchestration. The per-file traversal lives in
/// [`crate::references::find_references_in_file`] and is shared with `find_references`.
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

fn classify_highlight_token(token: &SyntaxToken) -> DocumentHighlightKind {
    if is_declaration_name_token(token) {
        return DocumentHighlightKind::Text;
    }

    if is_assignment_write_target(token) {
        DocumentHighlightKind::Write
    } else {
        DocumentHighlightKind::Read
    }
}

fn is_declaration_name_token(token: &SyntaxToken) -> bool {
    token.parent_ancestors().any(|node| match node.kind() {
        SyntaxKind::VAR_DEF => node
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .any(|candidate| candidate.kind() == SyntaxKind::IDENT && candidate == *token),
        SyntaxKind::PARAM | SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF => node
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|candidate| candidate.kind() == SyntaxKind::IDENT)
            .map(|candidate| candidate == *token)
            .unwrap_or(false),
        _ => false,
    })
}

fn is_assignment_write_target(token: &SyntaxToken) -> bool {
    let Some(assign_stmt) =
        token.parent_ancestors().find(|node| node.kind() == SyntaxKind::ASSIGN_STMT)
    else {
        return false;
    };

    assigned_target_name_token(&assign_stmt)
        .map(|target_token| target_token == *token)
        .unwrap_or(false)
}

fn assigned_target_name_token(assign_stmt: &SyntaxNode) -> Option<SyntaxToken> {
    let eq_start = assign_stmt
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .find(|token| token.kind() == SyntaxKind::EQ)?
        .text_range()
        .start();

    let lhs_node =
        assign_stmt.children().take_while(|node| node.text_range().end() <= eq_start).last()?;

    match lhs_node.kind() {
        SyntaxKind::IDENT => lhs_node
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|token| token.kind().is_name_token()),
        SyntaxKind::FIELD_EXPR => field_tail_name_token(&lhs_node),
        _ => None,
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
        // Same-named export procedure in both files. Cursor on the definition in A.
        // documentHighlight is same-document by LSP spec — must not return ranges
        // that belong to file B's source text.
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
        // Cursor on a call site (not the definition). Same name exists in a neighbour
        // module — must not surface in highlights for the current document.
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
