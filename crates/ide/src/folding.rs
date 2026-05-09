//! Folding ranges implementation.

use ide_db::RootDatabase;
use line_index::LineIndex;
use syntax::{SyntaxKind, SyntaxNode, TextRange, TextSize};
use vfs::FileId;

/// A foldable text range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldingRange {
    pub range: TextRange,
    pub kind: Option<FoldingRangeKind>,
}

/// Standardized folding range kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldingRangeKind {
    Region,
}

/// Returns folding ranges for a file.
pub fn folding_ranges<DB: RootDatabase>(db: &DB, file_id: FileId) -> Vec<FoldingRange> {
    let _span = tracing::info_span!("folding_ranges", ?file_id).entered();

    let input = db.file_text_input(file_id);
    let text = input.text(db);
    let line_index = LineIndex::new(&text);
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    let mut ranges = Vec::new();
    collect_region_ranges(db, file_id, &line_index, &mut ranges);
    collect_syntax_ranges(&root, &line_index, &mut ranges);

    ranges.sort_by_key(|range| (range.range.start(), range.range.end()));
    ranges.dedup_by_key(|range| (range.range.start(), range.range.end(), range.kind));
    ranges
}

fn collect_region_ranges<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    line_index: &LineIndex,
    ranges: &mut Vec<FoldingRange>,
) {
    let region_tree = db.region_tree(file_id);
    for (_, region) in region_tree.regions() {
        push_multiline_range(ranges, line_index, region.range, Some(FoldingRangeKind::Region));
    }
}

fn collect_syntax_ranges(
    root: &SyntaxNode,
    line_index: &LineIndex,
    ranges: &mut Vec<FoldingRange>,
) {
    for node in root.descendants() {
        if is_foldable_syntax_node(node.kind()) {
            push_multiline_range(ranges, line_index, node.text_range(), None);
        }
    }
}

fn is_foldable_syntax_node(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PROCEDURE_DEF
            | SyntaxKind::FUNCTION_DEF
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::PRE_IF_DIR
            | SyntaxKind::PRE_DELETE_DIR
            | SyntaxKind::PRE_INSERT_DIR
    )
}

fn push_multiline_range(
    ranges: &mut Vec<FoldingRange>,
    line_index: &LineIndex,
    range: TextRange,
    kind: Option<FoldingRangeKind>,
) {
    if folding_lines(line_index, range).is_none() {
        return;
    }
    ranges.push(FoldingRange { range, kind });
}

fn folding_lines(line_index: &LineIndex, range: TextRange) -> Option<(u32, u32)> {
    if range.is_empty() {
        return None;
    }

    let start_line = line_index.try_line_col(range.start())?.line;
    let end_offset = range.end() - TextSize::from(1);
    let end_line = line_index.try_line_col(end_offset)?.line;
    (end_line > start_line).then_some((start_line, end_line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::vfs::{file_set::FileSet, VfsPath};
    use ide_db::RootDatabaseImpl;

    fn setup_db(code: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, code);
        (db, file_id)
    }

    fn ranges_by_lines(code: &str) -> Vec<(u32, u32, Option<FoldingRangeKind>)> {
        let (db, file_id) = setup_db(code);
        let line_index = LineIndex::new(code);
        folding_ranges(&db, file_id)
            .into_iter()
            .filter_map(|range| {
                let (start, end) = folding_lines(&line_index, range.range)?;
                Some((start, end, range.kind))
            })
            .collect()
    }

    #[test]
    fn folds_procedure_and_function() {
        let code = "Процедура Тест()\n    Сообщить(1);\nКонецПроцедуры\n\nФункция Ф()\n    Возврат 1;\nКонецФункции";

        let ranges = ranges_by_lines(code);

        assert_eq!(ranges, vec![(0, 2, None), (4, 6, None)]);
    }

    #[test]
    fn folds_regions_with_kind() {
        let code = "#Область Public\nПроцедура Тест()\nКонецПроцедуры\n#КонецОбласти";

        let ranges = ranges_by_lines(code);

        assert_eq!(ranges, vec![(0, 3, Some(FoldingRangeKind::Region)), (1, 2, None)]);
    }

    #[test]
    fn folds_control_flow_blocks() {
        let code = "Процедура Тест()\nЕсли Истина Тогда\n    Сообщить(1);\nКонецЕсли;\nДля Сч = 1 По 2 Цикл\n    Сообщить(Сч);\nКонецЦикла;\nПопытка\n    Сообщить(1);\nИсключение\n    Сообщить(2);\nКонецПопытки;\nКонецПроцедуры";

        let ranges = ranges_by_lines(code);

        assert!(ranges.contains(&(0, 12, None)));
        assert!(ranges.contains(&(1, 3, None)));
        assert!(ranges.contains(&(4, 6, None)));
        assert!(ranges.contains(&(7, 11, None)));
    }

    #[test]
    fn folds_preprocessor_blocks() {
        let code = "#Если Сервер Тогда\nПроцедура Тест()\nКонецПроцедуры\n#КонецЕсли\n#Удаление\nСообщить(1);\n#КонецУдаления\n#Вставка\nСообщить(2);\n#КонецВставки";

        let ranges = ranges_by_lines(code);

        assert!(ranges.contains(&(0, 3, None)));
        assert!(ranges.contains(&(1, 2, None)));
        assert!(ranges.contains(&(4, 6, None)));
        assert!(ranges.contains(&(7, 9, None)));
    }

    #[test]
    fn ignores_single_line_ranges() {
        let code = "Процедура Тест() КонецПроцедуры";

        let ranges = ranges_by_lines(code);

        assert!(ranges.is_empty());
    }
}
