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

pub(crate) fn is_control_flow_terminator(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::RETURN_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::GOTO_STMT
    )
}

pub(crate) fn if_all_branches_terminate(node: &SyntaxNode) -> bool {
    let has_else = node.children().any(|n| n.kind() == SyntaxKind::ELSE_CLAUSE);
    if !has_else {
        return false;
    }

    let then_stmt_list = node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);
    if !then_stmt_list.is_some_and(|n| stmt_list_terminates(&n)) {
        return false;
    }

    for elsif in node.children().filter(|n| n.kind() == SyntaxKind::ELSIF_CLAUSE) {
        let elsif_stmt_list = elsif.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);
        if !elsif_stmt_list.is_some_and(|n| stmt_list_terminates(&n)) {
            return false;
        }
    }

    let else_clause = node.children().find(|n| n.kind() == SyntaxKind::ELSE_CLAUSE);
    if let Some(else_node) = else_clause {
        let else_stmt_list = else_node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);
        if !else_stmt_list.is_some_and(|n| stmt_list_terminates(&n)) {
            return false;
        }
    }

    true
}

pub(crate) fn stmt_list_terminates(stmt_list: &SyntaxNode) -> bool {
    let last_stmt = stmt_list
        .children()
        .filter(|n| {
            is_statement_node(n)
                || n.kind() == SyntaxKind::PRE_IF_DIR
                || n.kind() == SyntaxKind::PRE_REGION_DIR
        })
        .last();

    match last_stmt {
        Some(node) => {
            if is_control_flow_terminator(&node) {
                true
            } else if node.kind() == SyntaxKind::IF_STMT {
                if_all_branches_terminate(&node)
            } else if node.kind() == SyntaxKind::PRE_IF_DIR {
                false
            } else if node.kind() == SyntaxKind::PRE_REGION_DIR {
                preproc_region_terminates(&node)
            } else {
                false
            }
        }
        None => false,
    }
}

fn preproc_region_terminates(region: &SyntaxNode) -> bool {
    let last = region
        .children()
        .filter(|n| {
            is_statement_node(n)
                || n.kind() == SyntaxKind::PRE_IF_DIR
                || n.kind() == SyntaxKind::PRE_REGION_DIR
                || n.kind() == SyntaxKind::STMT_LIST
        })
        .last();

    match last {
        Some(node) if node.kind() == SyntaxKind::STMT_LIST => stmt_list_terminates(&node),
        Some(node) if is_control_flow_terminator(&node) => true,
        Some(node) if node.kind() == SyntaxKind::IF_STMT => if_all_branches_terminate(&node),
        Some(node) if node.kind() == SyntaxKind::PRE_REGION_DIR => preproc_region_terminates(&node),
        Some(node) if node.kind() == SyntaxKind::PRE_IF_DIR => {
            super::preproc::preproc_if_all_branches_terminate(&node)
        }
        _ => false,
    }
}
