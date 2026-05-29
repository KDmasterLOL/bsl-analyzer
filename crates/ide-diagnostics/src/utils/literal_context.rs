use syntax::{SyntaxKind, SyntaxToken};

pub fn is_in_default_value(token: &SyntaxToken) -> bool {
    let mut node = token.parent();
    while let Some(current) = node {
        if current.kind() == SyntaxKind::PARAM {
            return true;
        }
        if matches!(current.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF) {
            return false;
        }
        node = current.parent();
    }
    false
}

pub fn is_in_simple_assignment(token: &SyntaxToken) -> bool {
    let mut node = token.parent();

    while let Some(current) = node {
        if current.kind() == SyntaxKind::ASSIGN_STMT {
            let has_binary = current.descendants().any(|d| d.kind() == SyntaxKind::BINARY_EXPR);
            let has_arg_list = current.descendants().any(|d| d.kind() == SyntaxKind::ARG_LIST);

            return !has_binary && !has_arg_list;
        }
        node = current.parent();
    }

    false
}

pub fn find_method_name(node: &syntax::SyntaxNode) -> Option<String> {
    for child in node.descendants() {
        if child.kind() == SyntaxKind::FIELD_EXPR {
            return child
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| t.kind() == SyntaxKind::IDENT)
                .last()
                .map(|t| t.text().to_string());
        }
        if child.kind() == SyntaxKind::ARG_LIST {
            break;
        }
    }

    for token in node.children_with_tokens().filter_map(|e| e.into_token()) {
        if token.kind() == SyntaxKind::IDENT {
            return Some(token.text().to_string());
        }
    }

    None
}

pub fn is_in_structure_constructor(token: &SyntaxToken, extra_types: &[&str]) -> bool {
    let mut node = token.parent();

    while let Some(current) = node {
        if current.kind() == SyntaxKind::NEW_EXPR {
            for element in current.children_with_tokens() {
                if let Some(t) = element.as_token() {
                    if t.kind() == SyntaxKind::IDENT {
                        let type_name = t.text().to_lowercase();
                        if type_name.contains("структура")
                            || type_name.contains("structure")
                            || extra_types.iter().any(|kw| type_name.contains(kw))
                        {
                            return true;
                        }
                        break;
                    }
                }
            }
        }
        node = current.parent();
    }

    false
}

pub fn is_in_property_assignment(token: &SyntaxToken) -> bool {
    let mut node = token.parent();

    let mut check_node = token.parent();
    while let Some(current) = check_node {
        if current.kind() == SyntaxKind::ARG_LIST {
            return false;
        }
        if current.kind() == SyntaxKind::ASSIGN_STMT {
            break;
        }
        check_node = current.parent();
    }

    while let Some(current) = node {
        if current.kind() == SyntaxKind::ASSIGN_STMT {
            let has_dot = current
                .descendants_with_tokens()
                .any(|e| e.as_token().is_some_and(|t| t.kind() == SyntaxKind::DOT));

            return has_dot;
        }
        node = current.parent();
    }

    false
}
