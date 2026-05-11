//! Shared "real, observable code" predicate consumed by both
//! `CodeOutOfRegion` (module-level direct children) and
//! `RegionTree::is_region_empty` (region interior).
//!
//! Track 2 Phase C §3 Slice 1: two PUBLIC entry points cover two
//! different consumer contexts via a single private kind-level core.
//!
//! - [`is_significant_for_code_out_of_region`] — takes a `SyntaxNode`
//!   so it can descend into nested `#Область`/`#region` directives
//!   the way the existing handler does.
//! - [`is_significant_for_region_emptiness`] — pure `SyntaxKind` check
//!   for region-interior descendants.
//!
//! ## Why the two predicates carry different RAISE/LABEL membership
//!
//! `RegionTree::is_region_empty` counts `RAISE_STMT` and `LABEL_STMT`
//! as meaningful: a region with a single raise or label is not "empty"
//! — it has observable behaviour the user wrote on purpose.
//!
//! `CodeOutOfRegion` deliberately *excludes* both: a bare
//! `ВызватьИсключение …;` outside any region is the canonical way to
//! make a CommonModule "server-only" through the
//! `#Если Сервер … #Иначе ВызватьИсключение …; #КонецЕсли` guard idiom
//! (see `code_out_of_region::tests::test_standard_preproc`), and a
//! solitary label outside a region is meaningless without an
//! enclosing `Goto`. Flagging either as out-of-region would punish the
//! standard preprocessor guard pattern. Closing the divergence as an
//! audit-gap fix would require preprocessor-aware logic and belongs
//! to a separate track.

use syntax::{SyntaxKind, SyntaxNode};

/// Kind-level core shared by both consumer predicates: declarations
/// (`PROCEDURE_DEF`, `FUNCTION_DEF`, `VAR_DEF`) and the executable
/// statement kinds that *both* contexts agree are meaningful.
fn meaningful_core(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PROCEDURE_DEF
            | SyntaxKind::FUNCTION_DEF
            | SyntaxKind::VAR_DEF
            | SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::EXECUTE_STMT
            | SyntaxKind::GOTO_STMT
            | SyntaxKind::ADD_HANDLER_STMT
            | SyntaxKind::REMOVE_HANDLER_STMT
    )
}

/// Returns `true` when `node` is a module-level child that should sit
/// inside a `#Область`/`#region`. Direct kind hits use
/// [`meaningful_core`]; a `PRE_REGION_DIR` matches when it itself
/// encloses any meaningful content (descendants). Excludes
/// `RAISE_STMT` / `LABEL_STMT` deliberately — see module-level docs.
pub fn is_significant_for_code_out_of_region(node: &SyntaxNode) -> bool {
    if meaningful_core(node.kind()) {
        return true;
    }
    if node.kind() == SyntaxKind::PRE_REGION_DIR {
        return node.descendants().any(|n| meaningful_core(n.kind()));
    }
    false
}

/// Returns `true` when a syntax node of `kind`, found anywhere inside
/// a `#Область` block, makes that region non-empty. Adds
/// `RAISE_STMT` and `LABEL_STMT` on top of [`meaningful_core`] —
/// inside a region those are observable content even when bare.
pub fn is_significant_for_region_emptiness(kind: SyntaxKind) -> bool {
    meaningful_core(kind) || matches!(kind, SyntaxKind::RAISE_STMT | SyntaxKind::LABEL_STMT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_kind(text: &str, kind: SyntaxKind) -> SyntaxNode {
        let parse = parser::parse(text);
        parse
            .syntax_node()
            .descendants()
            .find(|n| n.kind() == kind)
            .unwrap_or_else(|| panic!("no node of kind {kind:?} in:\n{text}"))
    }

    #[test]
    fn region_emptiness_covers_declarations() {
        for kind in [SyntaxKind::PROCEDURE_DEF, SyntaxKind::FUNCTION_DEF, SyntaxKind::VAR_DEF] {
            assert!(is_significant_for_region_emptiness(kind), "{kind:?} should be meaningful");
        }
    }

    #[test]
    fn region_emptiness_covers_statements_including_raise_and_label() {
        for kind in [
            SyntaxKind::ASSIGN_STMT,
            SyntaxKind::CALL_STMT,
            SyntaxKind::RETURN_STMT,
            SyntaxKind::IF_STMT,
            SyntaxKind::WHILE_STMT,
            SyntaxKind::FOR_STMT,
            SyntaxKind::FOR_EACH_STMT,
            SyntaxKind::TRY_STMT,
            SyntaxKind::RAISE_STMT,
            SyntaxKind::BREAK_STMT,
            SyntaxKind::CONTINUE_STMT,
            SyntaxKind::EXECUTE_STMT,
            SyntaxKind::GOTO_STMT,
            SyntaxKind::LABEL_STMT,
            SyntaxKind::ADD_HANDLER_STMT,
            SyntaxKind::REMOVE_HANDLER_STMT,
        ] {
            assert!(is_significant_for_region_emptiness(kind), "{kind:?} should be meaningful");
        }
    }

    #[test]
    fn code_out_of_region_excludes_raise_and_label() {
        // Mirrors the existing `code_out_of_region::tests::test_standard_preproc`
        // expectation: the standard preprocessor guard idiom
        // `#Если Сервер … #Иначе ВызватьИсключение …; #КонецЕсли` must
        // not be flagged. Likewise a solitary `LABEL_STMT` outside a
        // region is not significant on its own.
        let raise = first_kind("ВызватьИсключение \"x\";", SyntaxKind::RAISE_STMT);
        assert!(!is_significant_for_code_out_of_region(&raise));
        // RegionTree-side predicate disagrees on purpose.
        assert!(is_significant_for_region_emptiness(SyntaxKind::RAISE_STMT));
        assert!(is_significant_for_region_emptiness(SyntaxKind::LABEL_STMT));
    }

    #[test]
    fn comment_and_whitespace_are_not_meaningful() {
        assert!(!is_significant_for_region_emptiness(SyntaxKind::COMMENT));
        assert!(!is_significant_for_region_emptiness(SyntaxKind::WHITESPACE));
    }

    #[test]
    fn code_out_of_region_matches_direct_kinds() {
        let node = first_kind("Перем Х;", SyntaxKind::VAR_DEF);
        assert!(is_significant_for_code_out_of_region(&node));
    }

    #[test]
    fn code_out_of_region_descends_into_pre_region_dir() {
        let src = "#Область Х\nПерем Y;\n#КонецОбласти\n";
        let node = first_kind(src, SyntaxKind::PRE_REGION_DIR);
        assert!(is_significant_for_code_out_of_region(&node));
    }

    #[test]
    fn empty_pre_region_dir_is_not_significant() {
        let src = "#Область Х\n#КонецОбласти\n";
        let node = first_kind(src, SyntaxKind::PRE_REGION_DIR);
        assert!(!is_significant_for_code_out_of_region(&node));
    }

    #[test]
    fn pre_region_dir_containing_only_raise_is_not_significant() {
        // Mirrors `contains_executable_code`'s historical RAISE
        // exclusion — a nested region whose body is only
        // `ВызватьИсключение …` is classified the same way as the
        // bare-module-level case.
        let src = "#Область Х\nВызватьИсключение \"X\";\n#КонецОбласти\n";
        let node = first_kind(src, SyntaxKind::PRE_REGION_DIR);
        assert!(!is_significant_for_code_out_of_region(&node));
    }
}
