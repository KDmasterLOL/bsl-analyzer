use ide_db::RootDatabase;
use syntax::{SyntaxNode, TextRange, TextSize};
use vfs::FileId;

/// For each requested offset, the ascending chain of syntactic ranges — the
/// token under the cursor first, then each enclosing node out to the root.
///
/// Purely syntactic: reads only the (salsa-cached) parse tree, no semantics.
pub fn selection_ranges<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    offsets: &[TextSize],
) -> Vec<Vec<TextRange>> {
    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    offsets.iter().map(|&offset| chain_at(&root, offset)).collect()
}

/// The nested ranges covering `offset`, innermost first, with consecutive
/// duplicates collapsed (a node whose span equals its only child's).
fn chain_at(root: &SyntaxNode, offset: TextSize) -> Vec<TextRange> {
    let token = root
        .token_at_offset(offset)
        .right_biased()
        .or_else(|| root.token_at_offset(offset).left_biased());

    let Some(token) = token else {
        // An empty tree still owes the client one range for the position.
        return vec![root.text_range()];
    };

    let mut ranges: Vec<TextRange> = Vec::new();
    let spans =
        std::iter::once(token.text_range()).chain(token.parent_ancestors().map(|n| n.text_range()));
    for span in spans {
        if ranges.last() != Some(&span) {
            ranges.push(span);
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{file_set::FileSet, VfsPath};

    fn single_file(source: &str) -> (RootDatabaseImpl, FileId) {
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

    fn offset_of(source: &str, needle: &str) -> TextSize {
        TextSize::from(source.find(needle).unwrap() as u32)
    }

    #[test]
    fn chain_nests_outward_from_the_token() {
        let source = "Процедура Тест()\n    Итог = Первое + Второе;\nКонецПроцедуры\n";
        let (db, file_id) = single_file(source);
        let chains = selection_ranges(&db, file_id, &[offset_of(source, "Первое")]);
        assert_eq!(chains.len(), 1);
        let chain = &chains[0];

        // Innermost range is the token the cursor sits on.
        assert_eq!(&source[chain[0]], "Первое");
        // Each range strictly contains the previous one.
        for pair in chain.windows(2) {
            assert!(
                pair[1].contains_range(pair[0]) && pair[1] != pair[0],
                "range {:?} must strictly contain {:?}",
                pair[1],
                pair[0]
            );
        }
        // The outermost range spans the whole file.
        assert_eq!(*chain.last().unwrap(), TextRange::up_to(TextSize::from(source.len() as u32)));
    }

    #[test]
    fn produces_one_chain_per_position() {
        let source = "Процедура Тест()\n    А = 1;\nКонецПроцедуры\n";
        let (db, file_id) = single_file(source);
        let offsets = [offset_of(source, "А"), offset_of(source, "Тест")];
        let chains = selection_ranges(&db, file_id, &offsets);
        assert_eq!(chains.len(), 2);
        assert!(chains.iter().all(|c| !c.is_empty()));
        assert_eq!(&source[chains[1][0]], "Тест");
    }

    #[test]
    fn empty_offsets_yield_no_chains() {
        let (db, file_id) = single_file("Процедура Тест()\nКонецПроцедуры\n");
        assert!(selection_ranges(&db, file_id, &[]).is_empty());
    }
}
