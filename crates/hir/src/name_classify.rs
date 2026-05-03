//! Name-position classifier for tokens.
//!
//! Single funnel for IDE consumers (hover, goto-definition, references,
//! completion, `symbol-info` callee resolution): given a `SyntaxToken`,
//! return what *kind of name slot* it sits in. Downstream resolvers
//! then dispatch on the [`NameClass`] variant instead of inspecting
//! token kind themselves.
//!
//! ## Why this exists
//!
//! BSL keywords can collide with method names — the parser accepts any
//! `is_keyword()` token as a field tail (see
//! `crates/parser/src/grammar/expressions.rs::is_ident_or_keyword`).
//! The headline case is `Запрос.Выполнить()`, where `Выполнить` lexes
//! as `KW_EXECUTE` (BSL has a global eval-style statement of the same
//! name). Every IDE handler that opens with
//! `if token.kind() != SyntaxKind::IDENT { return None; }` silently
//! kills hover/goto/references on that slot.
//!
//! Centralising the kind-vs-slot decision here lets every consumer
//! `match` over [`NameClass`] without re-deriving the rule. The
//! [`SyntaxKind::is_name_token`] predicate stays in
//! `crates/syntax/src/syntax_kind.rs` as the single source of truth
//! for "what counts as a name token at all"; this module composes it
//! with positional structure (`FIELD_EXPR`, `NEW_EXPR`, …) into the
//! richer slot taxonomy.

use syntax::ast_utils::{field_tail_name_token, new_expr_type_name_token};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// What kind of name slot a token sits in.
///
/// `match` over this enum is the only legal way to ask "should I
/// resolve this token?" in the IDE layer. Variants intentionally carry
/// the receiver / token they refer to so consumers don't re-walk the
/// CST.
#[derive(Debug, Clone)]
pub enum NameClass {
    /// Free name in expression position. Examples: `Х`, `MyFunc`,
    /// `Справочники`, the `КомпоновщикНастроек` in `КомпоновщикНастроек = …`.
    /// Resolves through builtins / locals / MDO plurals / module
    /// symbols (the existing [`crate::Semantics::resolve_name_to_definition`]
    /// pipeline).
    FreeName { token: SyntaxToken },

    /// Name to the right of `.` in a `FIELD_EXPR`. Either a method or a
    /// property of `receiver`. The token can be `IDENT` or any
    /// `is_keyword()` variant — callers must use `token.text()`, never
    /// `token.kind()`.
    ///
    /// `is_call` is `true` iff the enclosing expression is a
    /// `CALL_EXPR` whose callee is the parent `FIELD_EXPR`. Used by
    /// hover to break property-vs-method ties on names that exist in
    /// both slots on the same type.
    FieldName { receiver: SyntaxNode, token: SyntaxToken, is_call: bool },

    /// Type-reference position. Currently `Новый X` (constructor name).
    /// `Тип("X")` literals are *string contents*, not tokens, so the
    /// classifier doesn't see them — they're handled by string-literal
    /// hover separately.
    TypeRef { token: SyntaxToken },

    /// Boolean / null / undefined literal. Lexed as keyword
    /// (`KW_TRUE`, `KW_FALSE`, `KW_UNDEFINED`, `KW_NULL`) but
    /// semantically a value, not a name. Hover should treat these as
    /// literals (no work today — variant exists so the IDE layer's
    /// `match` is a closed dispatch).
    Literal { token: SyntaxToken },

    /// Plain keyword in a non-name position. Examples: `Если`,
    /// `Тогда`, `КонецПроцедуры`, `Возврат`. Only `hover_keyword`
    /// cares — every other consumer ignores.
    Keyword { token: SyntaxToken },

    /// Trivia, punctuation, label markers (`~Метка`), preprocessor
    /// symbols, and anything outside the IDE-layer's lookup vocabulary.
    /// Each of these has its own handler upstream (highlighter,
    /// preprocessor, label resolver) — the classifier deliberately
    /// hands them back as `Other` so the IDE-layer dispatch is a
    /// closed match, not a fall-through chain.
    Other,
}

/// Classify `token`'s name-slot. Pure syntactic analysis — does **not**
/// touch type inference or salsa.
///
/// The order of the rules is load-bearing:
///
/// 1. **Literal** wins before any other keyword interpretation. Without
///    it, `KW_TRUE` after a dot (which is invalid BSL but recoverable)
///    would land in `FieldName` rather than `Literal`. The current
///    parser doesn't accept literals after a dot, but the rule
///    documents intent: a token whose semantic role is "value" is
///    never a name.
/// 2. **FieldName** — the token is the *tail* of a `FIELD_EXPR`. The
///    [`field_tail_name_token`] helper guarantees the token sits
///    strictly after the dot inside the field-expression node.
///    `is_call` is checked here so callers (hover) can break
///    property-vs-method ties.
/// 3. **TypeRef** — the token is the type-name child of `NEW_EXPR`,
///    sitting strictly after `KW_NEW`.
/// 4. **Keyword** — `is_keyword()` and not in any name slot.
/// 5. **FreeName** — `IDENT` and not in any name slot.
/// 6. **Other** — everything else (trivia, punctuation, labels,
///    preprocessor symbols).
pub fn classify_token(token: &SyntaxToken) -> NameClass {
    // 1. Literal — beats keyword-as-name for `Истина`/`Ложь`/`Неопределено`/`Null`.
    if token.kind().is_literal() {
        return NameClass::Literal { token: token.clone() };
    }

    // 2. Field-tail of a `FIELD_EXPR`.
    if let Some((field_expr, name_token)) = field_expr_for_tail(token) {
        if name_token == *token {
            // Receiver is the first child of FIELD_EXPR; classifier
            // hands it to consumers so they can `type_of_expr` it
            // without re-walking the tree.
            if let Some(receiver) = field_expr.children().next() {
                let is_call = is_call_callee(&field_expr);
                return NameClass::FieldName { receiver, token: token.clone(), is_call };
            }
        }
    }

    // 3. Type-name child of a `NEW_EXPR`.
    if let Some((new_expr, type_token)) = new_expr_for_type_name(token) {
        if type_token == *token {
            let _ = new_expr; // structurally validated; node not needed downstream
            return NameClass::TypeRef { token: token.clone() };
        }
    }

    // 4. Keyword in non-name position.
    if token.kind().is_keyword() {
        return NameClass::Keyword { token: token.clone() };
    }

    // 5. Free name.
    if token.kind() == SyntaxKind::IDENT {
        return NameClass::FreeName { token: token.clone() };
    }

    // 6. Anything else.
    NameClass::Other
}

/// If `token` is the field-tail of some `FIELD_EXPR` ancestor, return
/// `(field_expr_node, tail_token)`.
fn field_expr_for_tail(token: &SyntaxToken) -> Option<(SyntaxNode, SyntaxToken)> {
    let parent = token.parent()?;
    // Walk at most one ancestor up — the parser places the tail token
    // as a direct child of `FIELD_EXPR`, with no intervening name-ref
    // wrapper.
    let field_expr = if parent.kind() == SyntaxKind::FIELD_EXPR {
        parent
    } else {
        // Defensive: some parser variants might wrap the tail in a
        // single-child node. Step up once and re-check, never deeper —
        // we must not pick up a `FIELD_EXPR` that contains the token in
        // its receiver slot.
        let grand = parent.parent()?;
        if grand.kind() != SyntaxKind::FIELD_EXPR {
            return None;
        }
        // Reject when the token's range falls inside the receiver
        // (first child) — that would mean `token` is *part of* the
        // receiver expression, not the tail.
        let receiver = grand.children().next()?;
        if receiver.text_range().contains_range(token.text_range()) {
            return None;
        }
        grand
    };

    let tail = field_tail_name_token(&field_expr)?;
    Some((field_expr, tail))
}

/// True iff `field_expr` is the callee of an enclosing `CALL_EXPR`.
/// I.e. distinguishes `recv.M` (property access) from `recv.M(...)`
/// (method call).
fn is_call_callee(field_expr: &SyntaxNode) -> bool {
    let Some(parent) = field_expr.parent() else { return false };
    if parent.kind() != SyntaxKind::CALL_EXPR {
        return false;
    }
    // The CALL_EXPR's first child is the callee. We're the callee iff
    // we are that first child.
    parent.children().next().map(|n| n == *field_expr).unwrap_or(false)
}

/// If `token` is the type-name child of a `NEW_EXPR`, return
/// `(new_expr_node, type_name_token)`.
fn new_expr_for_type_name(token: &SyntaxToken) -> Option<(SyntaxNode, SyntaxToken)> {
    let parent = token.parent()?;
    if parent.kind() != SyntaxKind::NEW_EXPR {
        return None;
    }
    let type_token = new_expr_type_name_token(&parent)?;
    Some((parent, type_token))
}
