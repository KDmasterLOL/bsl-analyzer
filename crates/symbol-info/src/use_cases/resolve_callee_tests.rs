use super::*;
use ide_db::base_db::{RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use ide_db::vfs::{file_set::FileSet, VfsPath};
use ide_db::RootDatabaseImpl;
use vfs::FileId;

fn parse(source: &str) -> SyntaxNode {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, SourceRootId(0));
    db.set_file_text(file_id, source);
    db.parse(file_id).syntax_node()
}

fn slot_at(source: &str, needle: &str) -> Option<usize> {
    let root = parse(source);
    let offset = TextSize::from(source.find(needle).expect("needle present") as u32);
    resolve_callee_at(&root, offset).map(|active| active.index)
}

const CALLS: &str = r#"
Функция Внешняя(Первый, Второй)
    Возврат Первый;
КонецФункции

Процедура Тест()
    Внешняя(10, 20);
    Новый Массив(30);
    Справочники.Товары.ПолучитьФорму(40);
КонецПроцедуры
"#;

#[test]
fn argument_slot_counts_commas_for_call_and_constructor_forms() {
    assert_eq!(slot_at(CALLS, "10"), Some(0));
    assert_eq!(slot_at(CALLS, "20"), Some(1));
    assert_eq!(slot_at(CALLS, "30"), Some(0), "constructor argument lists count too");
    assert_eq!(slot_at(CALLS, "40"), Some(0), "a qualified chain is still just an argument list");
}

#[test]
fn trailing_slot_resolves_and_closing_paren_does_not() {
    let source = r#"
Функция Локальная(Первый, Второй)
    Возврат Первый;
КонецФункции

Процедура Тест()
    Локальная(1, );
КонецПроцедуры
"#;
    let root = parse(source);
    let comma_offset = TextSize::from(source.find(", )").expect("trailing comma") as u32 + 1);
    let closing_offset = TextSize::from(
        source.find("Локальная(1, )").expect("call") as u32 + "Локальная(1, )".len() as u32,
    );

    assert_eq!(resolve_callee_at(&root, comma_offset), Some(ActiveParam { index: 1 }));
    assert_eq!(resolve_callee_at(&root, closing_offset), None);
}

#[test]
fn rejects_an_argument_list_that_belongs_to_no_call() {
    let mut builder = syntax::SyntaxTreeBuilder::new();
    builder.start_node(SyntaxKind::SOURCE_FILE);
    builder.start_node(SyntaxKind::ARG_LIST);
    builder.token(SyntaxKind::L_PAREN, "(");
    builder.token(SyntaxKind::IDENT, "Х");
    builder.token(SyntaxKind::R_PAREN, ")");
    builder.finish_node();
    builder.finish_node();
    let root = builder.finish().syntax_node();

    assert_eq!(resolve_callee_at(&root, TextSize::from(1)), None);
}

#[test]
fn is_safe_on_malformed_recovery_call() {
    // Truncated call with a trailing comma and no closing paren: the parser
    // produces a recovery CALL_EXPR/ARG_LIST. The slot is still well defined —
    // whether anything is actually callable there is inference's answer, and it
    // records no binding for a recovered expression.
    let source = "Процедура Тест()\n    Несуществующая(1,\nКонецПроцедуры";
    assert_eq!(slot_at(source, "1,"), Some(0));
}
