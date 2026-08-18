//! What a name occurrence does at its site: declare, call, write, or read.
//!
//! Purely syntactic — the decision is taken from the token's ancestors, so it
//! belongs to the syntax layer and is shared by every consumer of a reference
//! list. `document_highlight` projects it onto the three-valued LSP vocabulary;
//! the `references` surface publishes it as is.
//!
//! Event handlers are deliberately absent: a name is bound to an event in form
//! XML and in event subscriptions, not in the BSL text, so no walk of the
//! syntax tree can decide it. That fact is an edge of the call graph, with its
//! own freshness, and is not guessed here.

use syntax::ast_utils::field_tail_name_token;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReferenceKind {
    /// The declaring occurrence: a `Перем`, a parameter, a method name.
    Declaration,
    /// The callee of a call.
    Call,
    /// The assignment target.
    Write,
    /// Everything else.
    Read,
}

impl ReferenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ReferenceKind::Declaration => "declaration",
            ReferenceKind::Call => "call",
            ReferenceKind::Write => "write",
            ReferenceKind::Read => "read",
        }
    }

    /// Parse the wire spelling. Not `FromStr`: there is no error to describe —
    /// a value outside the closed vocabulary is simply not one of these four.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "declaration" => ReferenceKind::Declaration,
            "call" => ReferenceKind::Call,
            "write" => ReferenceKind::Write,
            "read" => ReferenceKind::Read,
            _ => return None,
        })
    }
}

/// Classify one name token by its position in the tree.
///
/// The order is fixed: a declared name is a declaration even when it sits in an
/// assignment (`Перем А = 1` is not a write of an existing binding), and a write
/// outranks a call so that the projection onto `DocumentHighlightKind` keeps
/// today's answer for every token that is already `Write`.
pub fn classify_reference_token(token: &SyntaxToken) -> ReferenceKind {
    if is_declaration_name_token(token) {
        return ReferenceKind::Declaration;
    }
    if is_assignment_write_target(token) {
        return ReferenceKind::Write;
    }
    if is_call_callee_token(token) {
        return ReferenceKind::Call;
    }
    ReferenceKind::Read
}

fn is_declaration_name_token(token: &SyntaxToken) -> bool {
    token.parent_ancestors().any(|node| match node.kind() {
        SyntaxKind::VAR_DEF => node
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .any(|candidate| candidate.kind() == SyntaxKind::IDENT && candidate == *token),
        SyntaxKind::PARAM | SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF => node
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|candidate| candidate.kind() == SyntaxKind::IDENT)
            .map(|candidate| candidate == *token)
            .unwrap_or(false),
        _ => false,
    })
}

fn is_assignment_write_target(token: &SyntaxToken) -> bool {
    let Some(assign_stmt) =
        token.parent_ancestors().find(|node| node.kind() == SyntaxKind::ASSIGN_STMT)
    else {
        return false;
    };

    assigned_target_name_token(&assign_stmt)
        .map(|target_token| target_token == *token)
        .unwrap_or(false)
}

fn assigned_target_name_token(assign_stmt: &SyntaxNode) -> Option<SyntaxToken> {
    let eq_start = assign_stmt
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .find(|token| token.kind() == SyntaxKind::EQ)?
        .text_range()
        .start();

    let lhs_node =
        assign_stmt.children().take_while(|node| node.text_range().end() <= eq_start).last()?;

    match lhs_node.kind() {
        SyntaxKind::IDENT => lhs_node
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|token| token.kind().is_name_token()),
        SyntaxKind::FIELD_EXPR => field_tail_name_token(&lhs_node),
        _ => None,
    }
}

/// The callee of a call is the first child of `CALL_EXPR`: an `IDENT` node for a
/// bare call, a `FIELD_EXPR` for a qualified one — and there only its tail,
/// so the receiver of `Объект.Метод()` stays a read.
fn is_call_callee_token(token: &SyntaxToken) -> bool {
    let Some(parent) = token.parent() else { return false };
    let callee_name = match parent.kind() {
        SyntaxKind::IDENT => Some(token.clone()),
        SyntaxKind::FIELD_EXPR => field_tail_name_token(&parent),
        _ => None,
    };
    if callee_name.map(|name| name != *token).unwrap_or(true) {
        return false;
    }

    let Some(call) = parent.parent() else { return false };
    if call.kind() != SyntaxKind::CALL_EXPR {
        return false;
    }
    call.children().next().map(|first| first == parent).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(source: &str, needle: &str) -> ReferenceKind {
        let parse = parser::parse(source);
        let root = parse.syntax_node();
        let offset = syntax::TextSize::from(source.find(needle).unwrap() as u32);
        let token = root.token_at_offset(offset).right_biased().unwrap();
        assert_eq!(token.text(), needle, "picked the wrong token");
        classify_reference_token(&token)
    }

    #[test]
    fn bare_call_is_a_call() {
        let source = "Процедура Тест()\n    Помощник();\nКонецПроцедуры\n";
        assert_eq!(classify(source, "Помощник"), ReferenceKind::Call);
    }

    #[test]
    fn call_in_expression_is_a_call() {
        let source = "Процедура Тест()\n    Рез = Помощник(1);\nКонецПроцедуры\n";
        assert_eq!(classify(source, "Помощник"), ReferenceKind::Call);
    }

    #[test]
    fn qualified_call_marks_the_tail_and_not_the_receiver() {
        let source = "Процедура Тест()\n    Объект.Метод();\nКонецПроцедуры\n";
        assert_eq!(classify(source, "Объект"), ReferenceKind::Read);
        assert_eq!(classify(source, "Метод"), ReferenceKind::Call);
    }

    #[test]
    fn argument_of_a_call_is_a_read() {
        let source = "Процедура Тест(Аргумент)\n    Помощник(Аргумент);\nКонецПроцедуры\n";
        let source_after_decl = &source[source.find("Помощник").unwrap()..];
        let offset = source.find("Помощник").unwrap() + source_after_decl.find("Аргумент").unwrap();
        let parse = parser::parse(source);
        let root = parse.syntax_node();
        let token =
            root.token_at_offset(syntax::TextSize::from(offset as u32)).right_biased().unwrap();
        assert_eq!(token.text(), "Аргумент");
        assert_eq!(classify_reference_token(&token), ReferenceKind::Read);
    }

    #[test]
    fn declaration_outranks_the_call_position() {
        let source = "Процедура Помощник()\nКонецПроцедуры\n";
        assert_eq!(classify(source, "Помощник"), ReferenceKind::Declaration);
    }

    #[test]
    fn assignment_target_is_a_write() {
        let source = "Процедура Тест()\n    Итог = 1;\nКонецПроцедуры\n";
        assert_eq!(classify(source, "Итог"), ReferenceKind::Write);
    }
}
