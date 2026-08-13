//! Обход токенов назад, не спотыкающийся о пустой узел.
//!
//! `SyntaxToken::prev_token` из rowan берёт соседний элемент и спрашивает у
//! него краевой токен, а `SyntaxNode::last_token` спускается ровно в
//! последнего ребёнка. Пустой узел отдаёт `None`, и обход обрывается на
//! границе, за которой текст есть.
//!
//! Пустые узлы стоят на краях постоянно: ими помечен пропущенный элемент, и
//! `ERROR` в конце незавершённой процедуры — обычное состояние кода, который
//! ещё печатают.
//!
//! Вперёд не ходит никто, поэтому обратного направления здесь нет: ходят
//! назад — за словом, к которому относится позиция.

use crate::{NodeOrToken, SyntaxElement, SyntaxToken};

/// Предыдущий токен дерева, включая случай, когда путь к нему лежит через
/// узел без единого токена.
pub fn prev_token_past_empty(token: &SyntaxToken) -> Option<SyntaxToken> {
    let mut from: SyntaxElement = NodeOrToken::Token(token.clone());
    loop {
        if let Some(neighbour) = from.prev_sibling_or_token() {
            if let Some(found) = last_token_past_empty(neighbour) {
                return Some(found);
            }
        }
        let parent = match &from {
            NodeOrToken::Token(token) => token.parent(),
            NodeOrToken::Node(node) => node.parent(),
        }?;
        from = NodeOrToken::Node(parent);
    }
}

/// Последний токен, найденный от `start` влево: сам элемент, если он токен,
/// иначе спуск в него, иначе предыдущий сосед.
fn last_token_past_empty(start: SyntaxElement) -> Option<SyntaxToken> {
    let mut current = Some(start);
    while let Some(element) = current {
        match &element {
            NodeOrToken::Token(token) => return Some(token.clone()),
            NodeOrToken::Node(node) => {
                if let Some(found) = node.last_child_or_token().and_then(last_token_past_empty) {
                    return Some(found);
                }
            }
        }
        current = element.prev_sibling_or_token();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SyntaxKind, SyntaxNode, SyntaxTreeBuilder};

    /// `Процедура П() ... ; <ERROR> \n` — незавершённая процедура: пустой
    /// `ERROR` последним ребёнком, перевод строки уже за нею.
    fn tree_with_empty_node_at_the_edge() -> SyntaxNode {
        let mut builder = SyntaxTreeBuilder::new();
        builder.start_node(SyntaxKind::SOURCE_FILE);
        builder.start_node(SyntaxKind::PROCEDURE_DEF);
        builder.token(SyntaxKind::SEMICOLON, ";");
        builder.start_node(SyntaxKind::ERROR);
        builder.finish_node();
        builder.finish_node();
        builder.token(SyntaxKind::NEWLINE, "\n");
        builder.finish_node();
        builder.finish().syntax_node()
    }

    #[test]
    fn empty_node_does_not_stop_the_walk() {
        let root = tree_with_empty_node_at_the_edge();
        let newline = root.last_token().expect("перевод строки");
        assert_eq!(newline.kind(), SyntaxKind::NEWLINE);

        // Положительный контроль: без обхода пустого узла ответа нет вовсе.
        assert!(newline.prev_token().is_none());

        let prev = prev_token_past_empty(&newline).expect("точка с запятой перед пустым узлом");
        assert_eq!(prev.kind(), SyntaxKind::SEMICOLON);
    }

    #[test]
    fn the_start_of_the_tree_has_no_neighbour() {
        let root = tree_with_empty_node_at_the_edge();
        let first = root.first_token().expect("точка с запятой");

        assert!(prev_token_past_empty(&first).is_none());
    }
}
