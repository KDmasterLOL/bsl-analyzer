use syntax::{SyntaxKind, SyntaxNode};

pub(crate) struct ControlFlowAnalysis {
    pub has_return: bool,
    pub call_stmts: Vec<SyntaxNode>,
}

pub(crate) fn analyze_control_flow(stmt_list: &SyntaxNode) -> ControlFlowAnalysis {
    let mut has_return = false;
    let mut call_stmts = Vec::new();

    for node in stmt_list.descendants() {
        match node.kind() {
            SyntaxKind::RETURN_STMT => {
                has_return = true;
            }
            SyntaxKind::CALL_STMT => {
                call_stmts.push(node);
            }
            _ => {}
        }
    }

    ControlFlowAnalysis { has_return, call_stmts }
}

pub(crate) fn is_statement_node(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::GOTO_STMT
            | SyntaxKind::LABEL_STMT
            | SyntaxKind::EXECUTE_STMT
            | SyntaxKind::ADD_HANDLER_STMT
            | SyntaxKind::REMOVE_HANDLER_STMT
            | SyntaxKind::VAR_DEF
            | SyntaxKind::EMPTY_STMT
            | SyntaxKind::ERROR
    )
}
