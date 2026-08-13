//! Обход токенов, не спотыкающийся о пустой узел.
//!
//! `SyntaxToken::prev_token` и `next_token` из rowan берут соседний элемент и
//! спрашивают у него краевой токен, а `SyntaxNode::last_token` спускается
//! ровно в последнего ребёнка. Пустой узел отдаёт `None`, и обход
//! обрывается на границе, за которой текст есть.
//!
//! Пустые узлы стоят на краях постоянно: ими помечен пропущенный элемент, и
//! `ERROR` в конце незавершённой процедуры — обычное состояние кода, который
//! ещё печатают. Отсюда пара функций, обходящих такой узел, а не
//! останавливающихся на нём.

use crate::{NodeOrToken, SyntaxElement, SyntaxToken};

#[derive(Clone, Copy)]
enum Side {
    Prev,
    Next,
}

/// Предыдущий токен дерева, включая случай, когда путь к нему лежит через
/// узел без единого токена.
pub fn prev_token_past_empty(token: &SyntaxToken) -> Option<SyntaxToken> {
    walk(token, Side::Prev)
}

/// Следующий токен дерева, включая случай, когда путь к нему лежит через
/// узел без единого токена.
pub fn next_token_past_empty(token: &SyntaxToken) -> Option<SyntaxToken> {
    walk(token, Side::Next)
}

fn walk(token: &SyntaxToken, side: Side) -> Option<SyntaxToken> {
    let mut from: SyntaxElement = NodeOrToken::Token(token.clone());
    loop {
        if let Some(neighbour) = sibling(&from, side) {
            if let Some(found) = edge_token(neighbour, side) {
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

/// Первый токен, найденный от `start` в сторону `side`: сам элемент, если он
/// токен, иначе спуск в него, иначе следующий сосед.
fn edge_token(start: SyntaxElement, side: Side) -> Option<SyntaxToken> {
    let mut current = Some(start);
    while let Some(element) = current {
        match &element {
            NodeOrToken::Token(token) => return Some(token.clone()),
            NodeOrToken::Node(node) => {
                let inner = match side {
                    Side::Prev => node.last_child_or_token(),
                    Side::Next => node.first_child_or_token(),
                };
                if let Some(found) = inner.and_then(|inner| edge_token(inner, side)) {
                    return Some(found);
                }
            }
        }
        current = sibling(&element, side);
    }
    None
}

fn sibling(element: &SyntaxElement, side: Side) -> Option<SyntaxElement> {
    match side {
        Side::Prev => element.prev_sibling_or_token(),
        Side::Next => element.next_sibling_or_token(),
    }
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

        assert_eq!(
            next_token_past_empty(&prev).map(|token| token.kind()),
            Some(SyntaxKind::NEWLINE)
        );
    }

    #[test]
    fn ends_of_the_tree_have_no_neighbour() {
        let root = tree_with_empty_node_at_the_edge();
        let first = root.first_token().expect("точка с запятой");
        let last = root.last_token().expect("перевод строки");

        assert!(prev_token_past_empty(&first).is_none());
        assert!(next_token_past_empty(&last).is_none());
    }
}
