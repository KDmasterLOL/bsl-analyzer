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
    if meaningful_core(node.kind()) {
        return true;
    }
    if node.kind() == SyntaxKind::PRE_REGION_DIR {
        return node.descendants().any(|n| meaningful_core(n.kind()));
    }
    false
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
        let src = "#Область Х\nВызватьИсключение \"X\";\n#КонецОбласти\n";
        let node = first_kind(src, SyntaxKind::PRE_REGION_DIR);
        assert!(!is_significant_for_code_out_of_region(&node));
    }
}
