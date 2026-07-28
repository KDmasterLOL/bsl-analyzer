//! SDBL (1C:Enterprise query language) grammar.
//!
//! ## Provenance
//!
//! The grammar has been re-derived from official 1C sources one area at
//! a time. Areas re-derived after the repository-wide comment prune
//! carry a `CLEAN-ROOM` banner naming the attestation that records their
//! sources.
//!
//! - Slice 12 — recovery behaviour and editor allowances:
//!   `docs/legal/sdbl-clean-room-slice12.md`.
//!
//! The query package and `SELECT` skeleton, the field list, the source
//! chains, the `JOIN` family, the expression layer, the clauses after
//! `FROM`, the `SELECT` prefix qualifiers and the virtual-table argument
//! body are attested by
//! `docs/legal/sdbl-clean-room-slice{6,7,7-addendum,8,8-addendum,9,10a,10b,11}.md`.
//! Their in-file banners were removed in that prune and are not restored
//! here, so absence of a banner above a function does not imply absence
//! of an attestation — the attestation documents are authoritative on
//! scope.
//!
//! What remains unowned is the reattachment of this surface to semantic
//! lowering, tracked as Slice 13; `crates/sdbl-hir` is read-only for
//! every slice before it.

pub mod expressions;
pub mod select;

use crate::event::NodeKind;
use crate::parser::Parser;
use crate::token_set::TokenSet;
use lexer::TokenKind;
use parser_error::{ParseError, RecoveryKind};

pub(super) const LIST_RECOVERY: TokenSet =
    TokenSet::new(&[TokenKind::RParen, TokenKind::Semicolon]);

// =====================================================================
// CLEAN-ROOM Slice 12 — the entry point's treatment of leftover input
//
// A query package is a `;`-separated sequence of queries. What the
// official grammar does not describe, and what this function therefore
// decides on its own, is where the sequence ends when the current token
// is neither `;` nor the end of input.
//
// Provenance: `docs/legal/sdbl-clean-room-slice12.md`, entry D1.
// =====================================================================

pub fn query_package(p: &mut Parser) {
    let m = p.start();

    // The loop is driven by "there is input left", not by "the separator is
    // where I left it". Recovery inside a query may consume a `;` — several
    // paths do, by reporting through a token bump — and a package whose
    // members are parsed only while the separators survive is a package that
    // loses good queries behind bad ones.
    let mut a_query_is_due = true;
    let mut member_has_tokens = false;

    p.skip_trivia();

    // A group cannot span a separator, so each member starts with none open
    // and "is this position inside a group" is simply "is anything open".
    // The parser keeps the count as tokens go by — the single place that
    // sees them all, and the only way to answer in constant time, since
    // rescanning a member's prefix before every decision is quadratic on a
    // long malformed one.

    while !p.at_end() {
        p.check_iteration_limit();
        p.skip_trivia();

        if p.at(TokenKind::Semicolon) {
            complain_about_a_missing_member(p, a_query_is_due, member_has_tokens);
            p.bump();
            a_query_is_due = true;
            member_has_tokens = false;
            p.reset_group_tracking();
            continue;
        }

        if p.at_end() {
            break;
        }

        // A query keyword at the top level starts a member. So does a clause
        // keyword when a member is due: `ИЗ Т` with no `ВЫБРАТЬ` yet is a
        // query being written, and the field-list slice guarantees it a node
        // to hang on to. What must not start one is a token that begins
        // nothing — forcing a query rule onto a `)` mints an empty member,
        // and the lowerer walks those.
        let at_top_level = p.open_group_count() == 0;
        let starts_a_member =
            at_top_level && (at_query_start(p) || (a_query_is_due && select::is_clause_keyword(p)));

        if starts_a_member {
            if !a_query_is_due {
                // A query where a separator should be. Recognising it keeps
                // the rest of the package parseable, but the package is still
                // malformed and silence would turn it into two good queries.
                p.error_custom_no_bump("ожидался разделитель ';' между запросами");
            }
            // Whatever is wrong inside the member is the query rule's to
            // report — including a missing `ВЫБРАТЬ`, which it already says.
            // The loop only speaks for members the query rule never sees.
            queries(p);
            a_query_is_due = false;
            member_has_tokens = true;
        } else if drain_to_boundary(p, a_query_is_due) {
            member_has_tokens = true;
        } else {
            break;
        }
    }

    // At the end of the input an owed member with nothing in it is just a
    // trailing separator, which is not a missing member.
    if member_has_tokens {
        complain_about_a_missing_member(p, a_query_is_due, member_has_tokens);
    }

    m.complete(p, NodeKind::SdblQueryPackage);
}

/// Says once, at the end of a member, that no query was found in it.
///
/// The loop speaks only for members no query rule ever saw. When one did
/// run, whatever was wrong inside is that rule's to report — a missing
/// `ВЫБРАТЬ` included — and a second voice here would double every
/// incomplete query's diagnosis.
fn complain_about_a_missing_member(p: &mut Parser, a_query_is_due: bool, had_tokens: bool) {
    if !a_query_is_due {
        return;
    }

    if had_tokens {
        p.error_custom_no_bump("ожидалось 'ВЫБРАТЬ' / 'SELECT'");
    } else {
        p.error_custom_no_bump("ожидался запрос между разделителями");
    }
}

/// Whether the current token can be the last one of a name a dot may
/// qualify: a word, a parameter, or the closing paren of a cast.
///
/// None of those is settled by the token kind alone. A keyword shares its
/// kind with every name, and one standing on its own is a clause the
/// leftover walked past, not a table waiting for its field. A parameter
/// arrives whole, sigil and name in one token, so its kind is shared with
/// a sigil that names nothing. And a paren that closes nothing closes no
/// cast either. Each spells a name only in the leftover's imagination, and
/// a dot behind it would hide the query behind that.
///
/// `is_property` is the one thing the kind cannot see: after a dot, a
/// keyword is the name of a field and does end one.
fn ends_a_name(p: &Parser, is_property: bool) -> bool {
    match p.current() {
        Some(TokenKind::Ident) => is_property || !select::is_clause_keyword(p),
        Some(TokenKind::Ampersand) => p.current_text().chars().count() > 1,
        // A cast has something in it, and its closing paren is the end of
        // the name only then: `()` closes a group and names nothing.
        Some(TokenKind::RParen) => {
            p.open_group_count() > 0 && p.prev_significant() != Some(TokenKind::LParen)
        }
        // A few words carry a kind of their own and are still names after a
        // dot; the expression layer names that set, and the leftover reads a
        // chain the same way or it will cut one in half.
        _ => is_property && expressions::at_property_name(p),
    }
}

fn at_trivia(p: &Parser) -> bool {
    matches!(
        p.current(),
        Some(TokenKind::Whitespace | TokenKind::Comment | TokenKind::Newline | TokenKind::Bom)
    )
}

/// The start of a query is a boundary for the whole grammar: no rule may
/// report an error by taking it, or the package loses the member it begins.
pub fn at_query_boundary(p: &Parser) -> bool {
    at_query_start(p)
}

fn at_query_start(p: &Parser) -> bool {
    select::at_sdbl_keyword(p, "SELECT", "ВЫБРАТЬ")
        || select::at_sdbl_keyword(p, "DROP", "УНИЧТОЖИТЬ")
}

/// A word standing where a name may stand.
///
/// Every keyword of this language reaches the parser as an identifier, so
/// a rule that asks only for the kind will accept the word that begins the
/// next query as a table, an alias or a type — and the package then loses
/// that query. Rules that read a bare name ask this instead.
pub(super) fn at_name(p: &Parser) -> bool {
    p.at(TokenKind::Ident) && !at_query_start(p)
}

/// Steps over the dot joining two parts of a qualified name, wherever the
/// whitespace around it falls.
///
/// Seven rules read such a chain, and each one that spelled the step out
/// for itself got some of it wrong: the space before the dot, the space
/// after it, or the word beyond it. They ask this instead, and the next
/// rule to read a chain inherits the answer rather than re-deriving it.
/// Takes the word standing where only a name may stand, whatever it spells.
///
/// The boundary that keeps rules off the next query's first word is a guess
/// made from the word alone, and after an explicit `КАК` there is nothing to
/// guess: a name is the only thing the position admits. This says so.
///
/// What it must not do is take something that is not a word at all. A
/// missing alias is a gap, and consuming the comma or paren that revealed
/// the gap costs the enclosing list everything after it.
pub(super) fn eat_name_here(p: &mut Parser, expected: &'static str) {
    if p.at(TokenKind::Ident) || expressions::at_property_name(p) {
        p.bump();
        return;
    }

    p.error_custom_no_bump(expected);
}

/// A word standing where a field or a table name belongs.
///
/// The source is explicit that this position is closed to keywords, and only
/// this one: «Имена таблиц и полей не могут совпадать с ключевыми словами
/// языка запросов». An alias is a different position — its spelling is
/// delegated to the rules for variable identifiers, and a keyword landing
/// there is documented as being read as the alias — so this must not reach
/// one, and `КАК ИТОГИ` stays a name.
///
/// The two separators are refused here as well, and neither is among the
/// clause keywords: that predicate also says which word ends a list, and a
/// word named there is one no rule inside the list may consume. Saying both
/// with one bit is what cost every alias its name once already.
pub(super) fn at_field_name(p: &Parser) -> bool {
    at_name(p)
        && !select::is_clause_keyword(p)
        && !at_the_alias_separator(p)
        && !at_a_list_separator(p)
}

/// The word that introduces an alias.
///
/// Refused where a name is required, and nowhere else: this is also reached
/// where an expression has legitimately ended and its alias follows, so a
/// position that merely wants an operand must not consult it.
pub(super) fn at_the_alias_separator(p: &Parser) -> bool {
    select::at_sdbl_keyword(p, "AS", "КАК")
}

/// The word that separates a list from what it is keyed by.
///
/// `ПО` is already among the clause keywords, as the Russian for `ON`. Its
/// English spelling is here instead, because that predicate would also make it
/// a boundary — and `BY` is a legal alias and a legal qualifier
/// (`SELECT A FROM T BY`, `SELECT BY.A FROM T AS BY`), which a boundary
/// forbids. Only a position that must hold a field or a table is closed to it.
pub(super) fn at_a_list_separator(p: &Parser) -> bool {
    p.at_keyword("BY")
}

/// Whether the word here can be the next part of a table's name.
///
/// A field chain is open to keywords: `Объект.Итоги` names a field, and after
/// a dot the language does not reserve them, which is why the expression layer
/// reads a chain by asking only for a property name. A table's name is not
/// open to them, and the dot exempts no component of it.
///
/// Stated once because four rules walk such a chain — the source of an `ИЗ`,
/// the table of a `ССЫЛКА`, the type of a `ВЫРАЗИТЬ`, the target of
/// `ДЛЯ ИЗМЕНЕНИЯ` — and each of them checked only its first component, so
/// each let a different set of words through the rest of the name.
///
/// What it does NOT refuse is a word carrying a kind of its own — `В`, `И`,
/// `НЕ`, `ИСТИНА`. A configuration may name an object `В`, and
/// `ВЫБРАТЬ * ИЗ Справочник.В` is a query slice 10a attested as clean. Only
/// the words that open a clause or separate its parts are closed here, and
/// only after a dot: the first component of a name is read as an identifier,
/// where `В` could not be told from the operator.
pub(super) fn at_table_name_component(p: &Parser) -> bool {
    expressions::at_property_name(p)
        && !at_query_boundary(p)
        && !select::is_clause_keyword(p)
        && !at_the_alias_separator(p)
        && !at_a_list_separator(p)
}

pub(super) fn at_a_qualifying_dot(p: &Parser) -> bool {
    p.at(TokenKind::Dot) || (at_trivia(p) && p.nth_non_trivia(0) == Some(TokenKind::Dot))
}

pub(super) fn eat_qualifying_dot(p: &mut Parser) -> bool {
    if p.at(TokenKind::Dot) {
        p.bump();
        return true;
    }

    if at_trivia(p) && p.nth_non_trivia(0) == Some(TokenKind::Dot) {
        p.skip_trivia();
        p.bump();
        return true;
    }

    false
}

/// Takes whatever the query rules refused, up to the next boundary, so that
/// the tree's text is the source text. Returns whether anything was taken.
///
/// A query stops short whenever a clause is out of order, a modifier is one
/// this grammar does not know, or the text is simply wrong. Without this the
/// remainder is not merely unparsed — it is absent from the tree, and the
/// parse reports success, so a consumer cannot tell a clean query from half
/// of one.
///
/// A separator cannot belong to a group in this language, so it ends the
/// leftover at any depth. A query start can belong to one — inside parens it
/// is a subquery, inside braces it is extension text — and is a boundary only
/// where nothing this member opened is still open. While a member is still
/// owed, a clause keyword is a boundary too: it is where an incomplete query
/// begins, and swallowing it would cost that query the node it is promised.
fn drain_to_boundary(p: &mut Parser, member_is_due: bool) -> bool {
    p.skip_trivia();

    if p.at_end() || p.at(TokenKind::Semicolon) {
        return false;
    }

    let leftover = p.start();
    let mut took_anything = false;
    let mut after_a_dot = false;
    let mut after_a_receiver = false;

    while !p.at_end() && !p.at(TokenKind::Semicolon) {
        p.check_iteration_limit();

        // After a dot the word is a property name, whatever it spells. `T.ИЗ`
        // is one fragment being skipped, not a fragment and then a query.
        let could_begin_a_member =
            !after_a_dot && (at_query_start(p) || (member_is_due && select::is_clause_keyword(p)));

        if took_anything && could_begin_a_member && p.open_group_count() == 0 {
            break;
        }

        // The dot reaches across a space but not across a line break, which
        // is how the expression layer reads a qualified name: `Т . ИЗ` is one
        // property access, `Т.\nИЗ` is a name and then a clause.
        if p.at(TokenKind::Newline) {
            after_a_dot = false;
        } else if !at_trivia(p) {
            // A dot qualifies the name in front of it. A dot with no name
            // there — a second dot, an operator, the start of the leftover —
            // is junk of its own and must not shield what follows.
            let is_property = after_a_dot;
            after_a_dot = p.at(TokenKind::Dot) && after_a_receiver;
            after_a_receiver = ends_a_name(p, is_property);
        }

        p.bump();
        took_anything = true;
    }

    if took_anything {
        p.emit_error_at_marker(
            leftover,
            ParseError::Custom {
                message: "не разобран остаток текста запроса",
                recovery: RecoveryKind::RecoverySpan,
            },
        );
    } else {
        leftover.abandon(p);
    }

    took_anything
}

fn queries(p: &mut Parser) {
    if select::at_sdbl_keyword(p, "DROP", "УНИЧТОЖИТЬ") {
        drop_table_query(p);
    } else {
        select::select_query(p);
    }
}

fn drop_table_query(p: &mut Parser) {
    let m = p.start();
    select::eat_sdbl_keyword(p, "DROP", "УНИЧТОЖИТЬ");
    p.skip_trivia();
    // Every keyword reaches the parser as an `Ident`, so a bare kind check
    // would take the next query's `ВЫБРАТЬ` for the name of the table being
    // dropped — and the package would then lose that query entirely.
    if at_field_name(p) {
        p.bump();
    } else if p.at(TokenKind::Ident) {
        // A keyword here belongs to whatever comes next — the following
        // query, the following clause — so report without taking it, or the
        // package loses what that word begins.
        p.error_custom_no_bump("ожидалось имя таблицы после 'УНИЧТОЖИТЬ' / 'DROP'");
    } else {
        p.error_custom("ожидалось имя таблицы после 'УНИЧТОЖИТЬ' / 'DROP'");
    }
    m.complete(p, NodeKind::SdblDropQuery);
}
