use syntax::{SyntaxKind, SyntaxNode};

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

pub fn is_significant_for_code_out_of_region(node: &SyntaxNode) -> bool {
    // Region directives are flat folding markers; any code that used to live
    // inside a region container is now a direct sibling and is matched here on
    // its own kind.
    meaningful_core(node.kind())
}

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
        let raise = first_kind("ВызватьИсключение \"x\";", SyntaxKind::RAISE_STMT);
        assert!(!is_significant_for_code_out_of_region(&raise));
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
    fn flat_region_marker_is_not_significant() {
        // The marker is a flat leaf; the code inside the region is a sibling.
        let src = "#Область Х\nПерем Y;\n#КонецОбласти\n";
        let marker = first_kind(src, SyntaxKind::PRE_REGION_DIR);
        assert!(!is_significant_for_code_out_of_region(&marker));

        let var = first_kind(src, SyntaxKind::VAR_DEF);
        assert!(is_significant_for_code_out_of_region(&var));
    }

    #[test]
    fn empty_pre_region_dir_marker_is_not_significant() {
        let src = "#Область Х\n#КонецОбласти\n";
        let marker = first_kind(src, SyntaxKind::PRE_REGION_DIR);
        assert!(!is_significant_for_code_out_of_region(&marker));
    }

    #[test]
    fn raise_stmt_is_not_code_out_of_region_significant() {
        // ВызватьИсключение is meaningful for region-emptiness but is not a
        // "code out of region" element.
        let src = "Вызватьисключение \"X\";\n";
        let raise = first_kind(src, SyntaxKind::RAISE_STMT);
        assert!(!is_significant_for_code_out_of_region(&raise));
        assert!(is_significant_for_region_emptiness(raise.kind()));
    }
}
