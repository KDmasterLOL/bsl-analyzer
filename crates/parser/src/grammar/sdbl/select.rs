//! SELECT query parsing for SDBL
//!
//! Implements parsing for SELECT queries including:
//! - Field lists with aliases
//! - FROM clauses with data sources
//! - WHERE clauses
//! - UNION queries
//! - Subqueries

use crate::event::NodeKind;
use crate::parser::Parser;
use lexer::TokenKind;

use super::expressions;

/// Helper to check for bilingual SDBL keywords (English or Russian).
pub(super) fn at_sdbl_keyword(p: &Parser, en: &str, ru: &str) -> bool {
    p.at_keyword(en) || p.at_keyword(ru)
}

/// Helper to consume bilingual SDBL keywords (English or Russian).
pub(super) fn eat_sdbl_keyword(p: &mut Parser, en: &str, ru: &str) -> bool {
    p.eat_keyword(en) || p.eat_keyword(ru)
}

/// Recover from unexpected tokens in selected field to alias or delimiter.
///
/// Called when expression parsing stopped early (e.g., didn't understand CASE in arithmetic).
/// Consumes all tokens until we find:
/// - AS/КАК keyword (alias start)
/// - Comma (next field)
/// - Clause keyword (FROM, WHERE, etc.)
///
/// **Important:** Handles nested constructs like CASE...END by tracking keywords.
/// Only creates ERROR node if actually consumed at least one token.
///
/// # Example
///
/// ```ignore
/// // After parsing "name" in: name + ВЫБОР КОГДА x ТОГДА y КОНЕЦ КАК alias
/// // Current position: +
/// recover_field_to_alias_or_delimiter(p);  // Consumes: + ВЫБОР ... КОНЕЦ
/// // Current position: КАК (alias start)
/// ```
fn recover_field_to_alias_or_delimiter(p: &mut Parser) {
    let err = p.start();
    let mut case_depth = 0i32; // Track nested CASE expressions
    let mut paren_depth = 0i32; // Track nested parentheses
    let mut consumed_any = false; // Track if we consumed at least one token

    loop {
        p.check_iteration_limit(); // Prevent infinite loops

        // Track CASE/ВЫБОР nesting (CASE can contain commas)
        if p.at_keyword("CASE") || p.at_keyword("ВЫБОР") {
            case_depth += 1;
            p.bump();
            consumed_any = true;
            continue;
        }

        if (p.at_keyword("END") || p.at_keyword("КОНЕЦ")) && case_depth > 0 {
            case_depth -= 1;
            p.bump();
            consumed_any = true;
            continue;
        }

        // Track parenthesis nesting
        if p.at(TokenKind::LParen) {
            paren_depth += 1;
            p.bump();
            consumed_any = true;
            continue;
        }

        if p.at(TokenKind::RParen) && paren_depth > 0 {
            paren_depth -= 1;
            p.bump();
            consumed_any = true;
            continue;
        }

        // Only check delimiters when not inside nested constructs
        if case_depth == 0 && paren_depth == 0 {
            // Stop at alias keyword
            if at_sdbl_keyword(p, "AS", "КАК") {
                break;
            }

            // Stop at field delimiter (comma)
            if p.at(TokenKind::Comma) {
                break;
            }

            // Stop at semicolon (end of query)
            if p.at(TokenKind::Semicolon) {
                break;
            }

            // Stop at closing parenthesis (end of subquery in FROM)
            if p.at(TokenKind::RParen) {
                break;
            }

            // Stop at clause keywords
            if is_clause_keyword(p) {
                break;
            }

            // Stop at EOF
            if p.at_end() {
                break;
            }
        }

        // Consume one token
        p.bump();
        consumed_any = true;
    }

    // Only create ERROR node if we actually consumed tokens
    if consumed_any {
        err.complete(p, NodeKind::Error);
    } else {
        err.abandon(p);
    }
}

/// Recover to next delimiter by consuming unexpected tokens in virtual table arguments.
///
/// Similar to expressions::recover_to_delimiter but for virtual table method args context.
/// Tracks parenthesis balance to handle nested calls.
fn recover_to_delimiter_vt(p: &mut Parser) {
    let err = p.start();
    let mut paren_depth = 0i32; // Track nested parentheses

    loop {
        p.check_iteration_limit(); // Prevent infinite loops

        // Track parenthesis nesting
        if p.at(TokenKind::LParen) {
            paren_depth += 1;
            p.bump();
            continue;
        }

        if p.at(TokenKind::RParen) {
            if paren_depth > 0 {
                // This is a closing paren for a nested call - consume it
                paren_depth -= 1;
                p.bump();
                continue;
            } else {
                // This is the closing paren for our function - stop here
                break;
            }
        }

        // Stop at top-level delimiters (when not inside nested parens)
        if paren_depth == 0 {
            if p.at(TokenKind::Comma) || p.at(TokenKind::Semicolon) {
                break;
            }

            // Stop at clause keywords (FROM, WHERE, etc.)
            if is_clause_keyword(p) {
                break;
            }
        }

        // Stop at EOF
        if p.at_end() {
            break;
        }

        // Consume one token
        p.bump();
    }

    err.complete(p, NodeKind::Error);
}

// ============================================================================
// CLEAN-ROOM Slice 6 — select entry wrapper, subquery, UNION clause
// ============================================================================
//
// See `docs/legal/sdbl-clean-room-slice6.md` for authorship and source
// citations. Per-function provenance comments are attached at C2.

/// Parse a SELECT query.
///
/// Grammar: `select-query := subquery trailing-select-clauses*`
///
/// Opens the `SdblSelectQuery` marker around the `subquery` body and the
/// AUTOORDER / ORDER BY / TOTALS BY tail-clause loop. The tail-clause loop
/// itself lives under the LEGACY banner in `select_tail_clauses` because its
/// clean-room rewrite belongs to Slice 11 (clauses after FROM).
pub fn select_query(p: &mut Parser) {
    // local: event-parser entry shell; opens SdblSelectQuery around the
    // subquery body (main query + UNION chain) and the AUTOORDER / ORDER BY /
    // TOTALS BY tail-clause loop. The wrapper itself is glue; the tail-clause
    // loop is Tier B (see select_tail_clauses under the LEGACY banner).
    let m = p.start();
    subquery(p);
    select_tail_clauses(p);
    m.complete(p, NodeKind::SdblSelectQuery);
}

/// Parse a subquery (main query plus any UNION clauses).
///
/// Grammar: `subquery := query (union-clause)*`
pub(super) fn subquery(p: &mut Parser) {
    // ITS pubqlang/10 — a subquery is a single query body optionally followed
    // by UNION / UNION ALL clauses. The UNION loop terminates on a
    // package-level ';' or on any non-UNION token (including EOF);
    // parenthesised subqueries are closed by the caller (data_source) so ')'
    // is handled there, not here.
    let m = p.start();

    query(p);

    loop {
        p.skip_trivia();

        if p.at(TokenKind::Semicolon) {
            break;
        }

        if !at_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ") {
            break;
        }

        union_clause(p);
    }

    m.complete(p, NodeKind::SdblSubquery);
}

/// Parse a single UNION clause.
///
/// Grammar: `union-clause := UNION [ALL] query`
fn union_clause(p: &mut Parser) {
    // ITS pubqlang/10 — UNION clause: UNION | UNION ALL followed by a query.
    // UNION and UNION ALL share SdblUnionClause; the optional ALL modifier is
    // carried as an IDENT token inside the node, detected post-parse by
    // SdblUnionClause::has_all() (see sdbl-clean-room-slice6.md §Preserved
    // pre-refactor behaviours — the split into SdblUnionAllClause is deferred
    // to Slice 13).
    let m = p.start();

    eat_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ");

    p.skip_trivia();
    eat_sdbl_keyword(p, "ALL", "ВСЕ");

    p.skip_trivia();
    query(p);

    m.complete(p, NodeKind::SdblUnionClause);
}

// ============================================================================
// CLEAN-ROOM Slice 7 — SELECT prefix: field list, aliases, INTO
// ============================================================================
//
// See `docs/legal/sdbl-clean-room-slice7.md` for authorship and source
// citations (landed with C3). Per-function provenance comments are attached
// at C2.

/// Parse a single SELECT query.
///
/// Grammar: `query := SELECT limitations? selected-fields into-clause?
/// query-body-clauses`.
///
/// This wrapper owns the SELECT prefix (keyword, limitations dispatch, field
/// list, INTO). Remaining clauses (FROM / JOIN / WHERE / GROUP / HAVING /
/// FOR UPDATE / INDEX BY / ORDER BY) are delegated to `query_body_clauses`
/// under the LEGACY banner — rewrite deferred to Slices 8 / 9 / 11.
fn query(p: &mut Parser) {
    // ITS pubqlang/10 — SELECT header: SELECT keyword, optional limitations,
    // selected fields, optional INTO; remainder delegated to
    // query_body_clauses (Tier B — Slices 8 / 9 / 11 pending).
    let m = p.start();

    if !eat_sdbl_keyword(p, "SELECT", "ВЫБРАТЬ") {
        p.error();
        m.complete(p, NodeKind::SdblQuery);
        return;
    }

    p.skip_trivia();
    if is_limitation_keyword(p) {
        limitations(p);
        p.skip_trivia();
    }

    selected_fields(p);

    p.skip_trivia();
    if at_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ") {
        into_clause(p);
    }

    query_body_clauses(p);

    m.complete(p, NodeKind::SdblQuery);
}

/// Parse selected fields list.
///
/// Grammar: `selected-fields := selected-field (',' selected-field)*`.
///
/// Delegates to the Tier B event-parser helper `parse_delimited_list` with
/// `LIST_RECOVERY` so incomplete / empty / unrecognised list elements produce
/// recoverable error nodes rather than abort the surrounding parse (IDE
/// recovery contract). The helper is Slice 10 project prior art.
pub(super) fn selected_fields(p: &mut Parser) {
    // ITS pubqlang/10 — selected field list: selectedField (COMMA selectedField)*;
    // uses event-parser parse_delimited_list with LIST_RECOVERY.
    let m = p.start();

    super::expressions::parse_delimited_list(
        p,
        TokenKind::Comma,
        &super::LIST_RECOVERY,
        is_field_start,
        selected_field,
    );

    m.complete(p, NodeKind::SdblFieldList);
}

/// Parse a single selected field.
///
/// Grammar: `selected-field := asterisk-field | expression alias?`.
///
/// Alias detection is guarded against clause keywords (FROM, WHERE, ...) so
/// that a bare-identifier after an expression is only consumed as an implicit
/// alias when the identifier is not the start of the next clause. After
/// expression parsing, if the next token is neither an alias start, a list
/// delimiter, nor the end of a clause, `recover_field_to_alias_or_delimiter`
/// (local Tier B recovery helper) consumes the unexpected span into an ERROR
/// node so the rest of the field list can still parse.
fn selected_field(p: &mut Parser) {
    // ITS pubqlang/10 — selected field: asteriskField | expression alias?;
    // recover_field_to_alias_or_delimiter is local event-parser recovery glue
    // (see attestation §Preserved pre-refactor behaviours).
    let m = p.start();

    if is_asterisk_start(p) {
        asterisk_field(p);
    } else {
        expressions::expression(p);
        p.skip_trivia();

        let at_expected_position = at_sdbl_keyword(p, "AS", "КАК")
            || (is_identifier_token(p) && !is_clause_keyword(p))
            || p.at(TokenKind::Comma)
            || p.at(TokenKind::Semicolon)
            || is_clause_keyword(p)
            || p.at_end();

        if !at_expected_position {
            recover_field_to_alias_or_delimiter(p);
            p.skip_trivia();
        }
    }

    p.skip_trivia();
    if (at_sdbl_keyword(p, "AS", "КАК") || is_identifier_token(p)) && !is_clause_keyword(p) {
        selected_field_alias(p);
    }

    m.complete(p, NodeKind::SdblSelectedField);
}

/// Predicate: can the current token start a selected field?
///
/// Returns `true` for asterisk-start tokens (`*`, `Ident . *`) or any
/// expression-start token. Used by `parse_delimited_list` to tell apart a
/// missing-element recovery from a clause boundary.
fn is_field_start(p: &Parser) -> bool {
    // local: event-parser predicate for field-list head detection; returns
    // true for asterisk-start or expression-start tokens.
    if is_asterisk_start(p) {
        return true;
    }
    super::expressions::is_expression_start(p)
}

/// Predicate: can the current token start an asterisk field?
///
/// Returns `true` for a literal `*` or a single-segment `Ident . *` prefix.
/// Multi-segment qualified asterisks (`Catalog.Products.*`) and
/// temp-table-prefixed asterisks (`#Temp.*`) are NOT recognised here; they
/// either arrive through expression parsing or fall through to the field
/// recovery path. See the Slice 7 attestation §Preserved pre-refactor
/// behaviours.
fn is_asterisk_start(p: &Parser) -> bool {
    // ITS pubqlang/10 — asterisk field start detection: literal * or Ident
    // followed by Dot Asterisk. Multi-segment and #Temp. prefixes are not
    // detected here and rely on expression-parsing entry (see attestation
    // §Preserved pre-refactor behaviours).
    if p.at(TokenKind::Star) {
        return true;
    }

    if p.at(TokenKind::Ident) {
        if let Some(TokenKind::Dot) = p.nth(1) {
            if let Some(TokenKind::Star) = p.nth(2) {
                return true;
            }
        }
    }

    false
}

/// Parse an asterisk field.
///
/// Grammar: `asterisk-field := (identifier '.')* '*'`.
///
/// Entry is gated by `is_asterisk_start`, which only recognises the
/// zero-prefix (`*`) and single-prefix (`Ident . *`) forms. Once inside, the
/// loop consumes any number of `Ident .` pairs before the mandatory `*` so
/// multi-segment prefixes still parse correctly if another path reaches
/// this function.
fn asterisk_field(p: &mut Parser) {
    // ITS pubqlang/10 — asterisk field: * | Ident . * at the predicate level;
    // once inside, consumes arbitrary prefix segments before the trailing .*.
    let m = p.start();

    while p.at(TokenKind::Ident) {
        if let Some(TokenKind::Dot) = p.nth(1) {
            p.bump();
            p.bump();
        } else {
            break;
        }
    }

    p.expect(TokenKind::Star);

    m.complete(p, NodeKind::SdblAsteriskField);
}

/// Parse a field alias (selected-field site).
///
/// Grammar: `alias := (AS | КАК)? identifier`.
///
/// The AS / КАК keyword is optional; a bare identifier after an expression
/// is a valid implicit alias — accepted structurally so the IDE can observe
/// it, with diagnostic enforcement (e.g. `AssignAliasFieldsInQuery`) layered
/// on top of the lossless tree. If the keyword is present but the identifier
/// is missing (`КАК ИЗ …`), an empty ERROR sub-node stands in for the alias
/// name and the outer parse continues.
///
/// Split from the former `alias()` helper in Slice 7 C1 (pure refactor).
/// The twin `source_alias` (born as `source_alias_legacy` in Slice 7 C1,
/// renamed in Slice 8 C1) sits under the Slice 8 clean-room banner — see
/// the Slice 7 attestation §Preserved pre-refactor behaviours.
fn selected_field_alias(p: &mut Parser) {
    // ITS pubqlang/10 — alias: (AS | КАК)? identifier; bare-identifier alias
    // preserved as IDE-recovery behaviour per attestation §Preserved
    // pre-refactor behaviours. Dual-use alias() was split in C1 —
    // source-alias rewrite deferred to Slice 8.
    let m = p.start();

    eat_sdbl_keyword(p, "AS", "КАК");
    p.skip_trivia();

    if is_clause_keyword(p) {
        let err = p.start();
        err.complete(p, NodeKind::Error);
        m.complete(p, NodeKind::SdblAlias);
        return;
    }

    let _ = p.expect(TokenKind::Ident);

    m.complete(p, NodeKind::SdblAlias);
}

/// Parse INTO clause for a temporary table destination.
///
/// Grammar: `into-clause := (INTO | ПОМЕСТИТЬ) identifier`.
///
/// The identifier names the temporary table receiving the query result and
/// is wrapped in a dedicated `SdblTempTableName` node so downstream
/// (sdbl-hir) can resolve it against the temporary-table scope. A missing
/// identifier produces a recoverable parse error — the IDE still observes
/// the INTO keyword.
fn into_clause(p: &mut Parser) {
    // ITS pubqlang/10 + /51 h47 — (INTO | ПОМЕСТИТЬ) identifier;
    // identifier-recovery path is local-preserved per attestation.
    let m = p.start();

    eat_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ");
    p.skip_trivia();

    if p.at(TokenKind::Ident) {
        let table_m = p.start();
        p.bump();
        table_m.complete(p, NodeKind::SdblTempTableName);
    } else {
        p.error();
    }

    m.complete(p, NodeKind::SdblIntoClause);
}

// ============================================================================
// CLEAN-ROOM Slice 8 — FROM sources and source chains
// ============================================================================
//
// See `docs/legal/sdbl-clean-room-slice8.md` for authorship and source
// citations (landed with C3). Per-function provenance comments are
// attached below.

/// Predicate: does the current token open a data-source?
///
/// Consulted by the FROM-clause delimited-list loop to decide whether the
/// next comma-separated element is a parseable data-source or a stray
/// token that should be skipped by the list recovery path.
fn is_data_source_start(p: &Parser) -> bool {
    // local: event-parser predicate for FROM-list head detection per
    // mini-spec §FROM / primary-source (subquery-source | table-ref |
    // parameter-source). Accepts LParen for subquery-source, Ampersand
    // for parameter-source, and Ident (provided it is not a clause
    // keyword) for table-ref.
    match p.current() {
        Some(TokenKind::LParen) => true,
        Some(TokenKind::Ampersand) => true,
        Some(TokenKind::Ident) => !is_clause_keyword(p),
        _ => false,
    }
}

/// Parse a FROM clause.
///
/// Grammar: `from-clause := (FROM|ИЗ) data-source (',' data-source)*`.
fn from_clause(p: &mut Parser) {
    // ITS pubqlang/10 + mini-spec §FROM clause — (FROM|ИЗ) data-source
    // (COMMA data-source)*. Keyword is bilingual per pubqlang/12.
    // Delegates the comma-separated list body to the Tier B
    // parse_delimited_list helper with LIST_RECOVERY so an incomplete
    // source node at any position can be recovered (§Recovery
    // requirements #7) without aborting the enclosing query.
    let m = p.start();

    eat_sdbl_keyword(p, "FROM", "ИЗ");
    p.skip_trivia();

    super::expressions::parse_delimited_list(
        p,
        TokenKind::Comma,
        &super::LIST_RECOVERY,
        is_data_source_start,
        data_source,
    );

    m.complete(p, NodeKind::SdblFromClause);
}

/// Parse a single data source in a FROM clause.
///
/// Grammar: `data-source := primary-source alias? join-clause*`,
/// `primary-source := subquery-source | table-ref | parameter-source`,
/// `subquery-source := '(' subquery ')' alias?`.
///
/// The parameter-source branch is dispatched inside [`table_ref`]: a bare
/// `&Ident` enters the identifier branch at the top of `table_ref`, which
/// owns the `SdblParameter` sub-node shape. An LParen at the data-source
/// head always enters the subquery-source branch regardless of inner
/// token shape.
fn data_source(p: &mut Parser) {
    // ITS pubqlang/10 + mini-spec §Data source — primary-source alias?
    // join-clause*. Primary-source is either '(' subquery ')' (the
    // subquery-source branch) or table-ref (which itself dispatches the
    // parameter-source branch on leading Ampersand). Alias is optional
    // at this level; the combined guard
    // (is_AS/КАК || is_identifier_token) && !is_clause_keyword prevents
    // an immediately following clause keyword (WHERE / GROUP / etc.)
    // from being captured as an implicit bare alias, matching
    // mini-spec §Recovery requirements #3. JOIN attachment delegates
    // into the Slice 9 `join_clause` helper; the attachment-point
    // loop is owned by Slice 8 per mini-spec §Data source join-clause*.
    let m = p.start();

    if p.at(TokenKind::LParen) {
        p.bump();
        p.skip_trivia();
        subquery(p);
        p.expect(TokenKind::RParen);
    } else {
        table_ref(p);
    }

    p.skip_trivia();
    if (at_sdbl_keyword(p, "AS", "КАК") || is_identifier_token(p)) && !is_clause_keyword(p) {
        source_alias(p);
    }

    p.skip_trivia();
    while is_join_keyword(p) {
        join_clause(p);
        p.skip_trivia();
    }

    m.complete(p, NodeKind::SdblDataSource);
}

/// Parse a table reference inside a data source.
///
/// Grammar: `table-ref := parameter-source | identifier ('.' identifier)*`,
/// `parameter-source := '&' identifier`. The identifier chain covers the
/// simple-name, MDO path, and MDO-with-virtual-table-trail forms.
/// Virtual-table method-call arguments are delegated to the Tier B
/// [`virtual_table_args_legacy`] helper (Slice 5 target).
fn table_ref(p: &mut Parser) {
    // ITS pubqlang/10 + /12 + mini-spec §FROM / primary-source +
    // parameter-source. The body is either a parameter-source
    // (Ampersand + identifier) wrapped in an SdblParameter sub-node, or
    // an identifier ('.' identifier)* chain (simple, MDO path, or
    // MDO-with-virtual-table-trail). DOT-recovery inserts an Error
    // sub-node when the next token after '.' is not an identifier or is
    // a clause / AS / КАК keyword, so an incomplete qualified name is
    // recoverable for IDE use without aborting the enclosing source
    // (§Recovery requirements #4). The parameter-source branch admits a
    // bare `&` without identifier as an IDE-recovery allowance: the
    // identifier bump is guarded by `if p.at(Ident)`, not required by
    // `p.expect`, so the user can keep typing the parameter name with
    // the SdblParameter marker already open. VT method-call arguments
    // are delegated to Tier B `virtual_table_args_legacy` (Slice 5
    // target — virtual-table and external-source handling).
    let m = p.start();

    if p.at(TokenKind::Ampersand) {
        let pm = p.start();
        p.bump();
        if p.at(TokenKind::Ident) {
            p.bump();
        }
        pm.complete(p, NodeKind::SdblParameter);
        m.complete(p, NodeKind::SdblTableRef);
        return;
    }

    if !p.expect(TokenKind::Ident) {
        m.complete(p, NodeKind::SdblTableRef);
        return;
    }

    while p.eat(TokenKind::Dot) {
        p.check_iteration_limit();
        p.skip_trivia();

        if !p.at(TokenKind::Ident) {
            let err = p.start();
            err.complete(p, NodeKind::Error);
            break;
        }

        if is_clause_keyword(p) || p.at_keyword("AS") || p.at_keyword("КАК") {
            let err = p.start();
            err.complete(p, NodeKind::Error);
            break;
        }

        p.bump();
    }

    p.skip_trivia();
    virtual_table_args_legacy(p);

    m.complete(p, NodeKind::SdblTableRef);
}

/// Parse a source alias at a FROM-clause data-source site.
///
/// Grammar: `alias := (AS|КАК)? identifier`.
///
/// Behaviorally equivalent to [`selected_field_alias`] but kept as a
/// distinct helper so each call-site chain stays under its own
/// clean-room slice. Re-merge is a Slice 12 decision.
fn source_alias(p: &mut Parser) {
    // ITS pubqlang/10 + mini-spec §Alias — (AS|КАК)? identifier at the
    // FROM-clause data-source / table-ref site. The AS/КАК keyword is
    // optional: the implicit (bare) alias form is accepted for
    // syntax-tree parity with the explicit form, per mini-spec
    // §Selected fields → Alias and §Recovery requirements #3. A clause
    // keyword appearing where the alias name is expected (e.g. AS
    // followed by WHERE) yields an empty Error sub-node inside
    // SdblAlias and then completes the marker, so the enclosing clause
    // loop can consume the keyword at the next level up without
    // aborting the data source.
    let m = p.start();

    eat_sdbl_keyword(p, "AS", "КАК");
    p.skip_trivia();

    if is_clause_keyword(p) {
        let err = p.start();
        err.complete(p, NodeKind::Error);
        m.complete(p, NodeKind::SdblAlias);
        return;
    }

    p.expect(TokenKind::Ident);

    m.complete(p, NodeKind::SdblAlias);
}

// ============================================================================
// CLEAN-ROOM Slice 9 — JOIN family
// ============================================================================
//
// `is_join_keyword` and `join_clause` are the JOIN-family surface reached
// by Slice 8's `data_source` join-attachment loop. They were re-authored
// in C2 from ITS pubqlang chapters 44–48 (RU listings + chapter 48
// chained / nested examples) and the SELECT mini-spec §JOIN clauses
// (lines 297–319) + §Recovery requirements item #6 (line 410); each
// function body carries a tiered provenance comment. Bilingual EN/RU
// keyword pairs are recognised per the lexer's Slice 2 attestation.
// See `docs/legal/sdbl-clean-room-slice9.md` for the attestation
// (landed with C3).

/// Predicate: does the current token open a JOIN clause?
///
/// Consulted by Slice 8's `data_source` join-attachment loop to decide
/// whether the next position should be parsed as a `SdblJoinClause`.
/// Returns true for any of the five JOIN-clause starters in EN or RU:
///
/// - `LEFT` / `ЛЕВОЕ`     — chapter 45 outer-join family;
/// - `RIGHT` / `ПРАВОЕ`   — chapter 46 outer-join family;
/// - `FULL` / `ПОЛНОЕ`    — chapter 47 outer-join family;
/// - `INNER` / `ВНУТРЕННЕЕ` — chapter 44 inner-join family;
/// - `JOIN` / `СОЕДИНЕНИЕ` — chapter 44 standalone reference (bare
///   `СОЕДИНЕНИЕ` is implicit INNER per mini-spec §JOIN clauses
///   behavioural note line 318).
///
/// `OUTER` / `ВНЕШНЕЕ` is **not** a starter — it is only consumed
/// inside `join_clause` after the optional join-type Ident. Adding
/// it to the starter set would change Slice 8's join-attachment
/// boundary and the alias-/recovery boundary in `is_clause_keyword`.
//
// Provenance:
//   Tier A1 — ITS pubqlang chapters 44 (`ВНУТРЕННЕЕ СОЕДИНЕНИЕ` +
//             standalone `СОЕДИНЕНИЕ`), 45 (`ЛЕВОЕ ВНЕШНЕЕ
//             СОЕДИНЕНИЕ`), 46 (`ПРАВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ`),
//             47 (`ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ`); chapter 48
//             chained / nested examples.
//   Tier B  — bilingual EN/RU keyword pairs per the lexer's
//             Slice 2 attestation
//             (`docs/legal/sdbl-clean-room-slice2.md`).
//   Tier C  — mini-spec §JOIN clauses §Shape line 302 ff.
fn is_join_keyword(p: &Parser) -> bool {
    p.at_keyword("LEFT")
        || p.at_keyword("ЛЕВОЕ")
        || p.at_keyword("RIGHT")
        || p.at_keyword("ПРАВОЕ")
        || p.at_keyword("FULL")
        || p.at_keyword("ПОЛНОЕ")
        || p.at_keyword("INNER")
        || p.at_keyword("ВНУТРЕННЕЕ")
        || p.at_keyword("JOIN")
        || p.at_keyword("СОЕДИНЕНИЕ")
}

/// Parse a single `SdblJoinClause` attached to the preceding data source.
///
/// Grammar (mini-spec §JOIN clauses §Shape, lines 301–307):
///
/// ```text
/// join-clause :=
///     [LEFT|RIGHT|FULL|INNER|ЛЕВОЕ|ПРАВОЕ|ПОЛНОЕ|ВНУТРЕННЕЕ]
///     [OUTER|ВНЕШНЕЕ]
///     (JOIN|СОЕДИНЕНИЕ)
///     data-source
///     (ON|ПО)
///     logical-expression
/// ```
///
/// Bare `JOIN` / `СОЕДИНЕНИЕ` without an explicit type is accepted
/// and treated as implicit INNER (mini-spec §JOIN clauses behavioural
/// note line 318; chapter 44 standalone `СОЕДИНЕНИЕ` example).
///
/// `data_source` (Slice 8) and `logical_expression` (Slice 10a) are
/// the cross-slice dispatch boundaries reached from this function.
//
// Provenance:
//   Tier A1 — ITS pubqlang chapters 44/45/46/47 RU canonical
//             listings (ВНУТРЕННЕЕ / ЛЕВОЕ ВНЕШНЕЕ / ПРАВОЕ
//             ВНЕШНЕЕ / ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ); chapter 44
//             standalone `СОЕДИНЕНИЕ`; chapter 48 chained /
//             nested example listings.
//   Tier B  — bilingual EN/RU keyword pairs (LEFT/ЛЕВОЕ,
//             RIGHT/ПРАВОЕ, FULL/ПОЛНОЕ, INNER/ВНУТРЕННЕЕ,
//             JOIN/СОЕДИНЕНИЕ, OUTER/ВНЕШНЕЕ, ON/ПО) per
//             the lexer's Slice 2 attestation.
//   Tier C  — mini-spec §JOIN clauses (lines 297–319) +
//             §Recovery requirements item #6 (line 410, the
//             incomplete-join-condition-after-ON recovery
//             policy); §Behavioral note line 318 (bare JOIN
//             without explicit type is implicit INNER).
//   Tier D  — local IDE-recovery allowances:
//             (a) bare LEFT/RIGHT/FULL/INNER (or RU
//                 equivalents) without OUTER/ВНЕШНЕЕ — accepted
//                 because the type-Ident bump is unconditional;
//                 no ITS prose-note attests OUTER optionality
//                 in chapters 45/46/47, so this is a parser-
//                 accepted local allowance;
//             (b) `Parser::error()`-bumps for missing JOIN
//                 keyword (after type Ident) and missing ON/ПО
//                 keyword (after data_source). Preserved
//                 behaviour locked by audit-gate tests
//                 `test_slice9_missing_join_keyword_current_behavior`
//                 and `test_slice9_missing_on_current_behavior`
//                 in `crates/parser/tests/sdbl_parser_tests.rs`.
//                 Recovery improvement (zero-width error,
//                 mirroring Slice 10b `column_or_function`) is
//                 deferred to the Slice 12 IDE-recovery rewrite
//                 — both options leave bad recovery trees in
//                 error cases, and Slice 9's clean-room scope
//                 is the happy-path grammar.
fn join_clause(p: &mut Parser) {
    let m = p.start();

    // Optional join-type Ident: LEFT/ЛЕВОЕ, RIGHT/ПРАВОЕ,
    // FULL/ПОЛНОЕ, INNER/ВНУТРЕННЕЕ. Tier A1 chapters 44/45/46/47.
    // The Ident is bumped into SdblJoinClause direct tokens so
    // SdblJoinClause::join_type() can substring-match it back
    // (consumer-side AST-shape invariant #1).
    let has_type = p.at_keyword("LEFT")
        || p.at_keyword("ЛЕВОЕ")
        || p.at_keyword("RIGHT")
        || p.at_keyword("ПРАВОЕ")
        || p.at_keyword("FULL")
        || p.at_keyword("ПОЛНОЕ")
        || p.at_keyword("INNER")
        || p.at_keyword("ВНУТРЕННЕЕ");
    if has_type {
        p.bump();
        p.skip_trivia();
    }

    // Optional OUTER/ВНЕШНЕЕ. Tier A1 chapters 45/46/47 each
    // list a single OUTER form (`ЛЕВОЕ/ПРАВОЕ/ПОЛНОЕ ВНЕШНЕЕ
    // СОЕДИНЕНИЕ`); chapter 44 INNER does not. The parser
    // accepts OUTER after any preceding type Ident.
    if p.at_keyword("OUTER") || p.at_keyword("ВНЕШНЕЕ") {
        p.bump();
        p.skip_trivia();
    }

    // Mandatory JOIN/СОЕДИНЕНИЕ keyword. Tier A1 anchors all
    // join forms on this keyword; chapter 44 standalone
    // `СОЕДИНЕНИЕ` is the bare-JOIN implicit-INNER form. On
    // miss: Tier D allowance — `p.error()` bumps the offending
    // token into an ERROR child and the marker still completes
    // (audit-gate `test_slice9_missing_join_keyword_current_behavior`).
    if !p.at_keyword("JOIN") && !p.at_keyword("СОЕДИНЕНИЕ") {
        p.error();
        m.complete(p, NodeKind::SdblJoinClause);
        return;
    }
    p.bump();
    p.skip_trivia();

    // Joined data source — table reference, parameter source,
    // or subquery; alias is optional. Slice 8 owns this helper.
    data_source(p);
    p.skip_trivia();

    // Mandatory ON/ПО keyword. Tier A1 chapters 44–47 + 48 all
    // gate the join condition on this keyword. On miss: Tier D
    // allowance — `p.error()` bumps the offending token into an
    // ERROR child of SdblJoinClause and parsing falls through
    // to the logical-expression body so the user's typed
    // condition still lands inside the JOIN node (audit-gate
    // `test_slice9_missing_on_current_behavior`; mini-spec
    // §Recovery requirement #6).
    if !eat_sdbl_keyword(p, "ON", "ПО") {
        p.error();
    }
    p.skip_trivia();

    // Join condition — full logical expression (Slice 10a/10b
    // surface: OR / AND / NOT / comparison / additive). Chapter
    // 48 chained example uses a single equality
    // (`ПО Т1.А = Т2.А`); chapter 47 example shows arithmetic
    // and parenthesised compounds.
    expressions::logical_expression(p);

    m.complete(p, NodeKind::SdblJoinClause);
}

// ============================================================================
// LEGACY (Slices 5, 11 pending)
// ============================================================================
//
// Everything below this banner — `select_tail_clauses` (Slice 11 target),
// `query_body_clauses` (Slice 11 target — clause-body dispatcher),
// `virtual_table_args_legacy` (Slice 5 target — virtual-table and
// external-source handling; extracted from `table_ref` during Slice 8 C1 as
// a pure refactor), `where_clause` / `group_by_clause` / `having_clause` /
// `order_by_clause` / `for_update_clause` / `index_by_clause` /
// `autoorder_clause` / `totals_by_clause` (Slice 11), and the
// `limitations` / `top_clause` dispatchers remains Tier B pre-refactor code
// until the corresponding clean-room slice rewrites it. No per-function
// provenance comments here.

/// Parse the optional AUTOORDER / ORDER BY / TOTALS BY tail-clause loop.
///
/// Extracted verbatim from the pre-C1 `select_query` body as a pure refactor
/// so the Slice 6 `select_query` wrapper earlier in this file can be attested
/// under the Slice 6 clean-room banner without dragging clause-body scope in.
/// Rewrite of this helper is deferred to Slice 11 (clauses after FROM).
fn select_tail_clauses(p: &mut Parser) {
    // Parse AUTOORDER, ORDER BY, and TOTALS BY in any order
    // These clauses are all optional and can appear in any combination
    let mut parsed_autoorder = false;
    let mut parsed_order_by = false;
    let mut parsed_totals_by = false;

    loop {
        p.skip_trivia();

        // Check for AUTOORDER
        if !parsed_autoorder && at_sdbl_keyword(p, "AUTOORDER", "АВТОУПОРЯДОЧИВАНИЕ")
        {
            autoorder_clause(p);
            parsed_autoorder = true;
            continue;
        }

        // Check for ORDER BY
        if !parsed_order_by && at_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ") {
            order_by_clause(p);
            parsed_order_by = true;
            continue;
        }

        // Check for TOTALS BY
        if !parsed_totals_by && at_sdbl_keyword(p, "TOTALS", "ИТОГИ") {
            totals_by_clause(p);
            parsed_totals_by = true;
            continue;
        }

        // No more clauses to parse
        break;
    }
}

/// Parse the optional FROM → ORDER BY clause tail of a single query.
///
/// Extracted verbatim from the pre-C1 `query()` body as a pure refactor so
/// the Slice 7 `query()` wrapper earlier in this file can be attested under
/// the Slice 7 clean-room banner without dragging clause-body scope in.
/// Rewrite of this helper is deferred to Slices 8 (FROM), 9 (JOIN via
/// data_source), and 11 (WHERE / GROUP / HAVING / FOR UPDATE / INDEX BY /
/// ORDER BY).
fn query_body_clauses(p: &mut Parser) {
    // FROM clause (optional)
    p.skip_trivia(); // CRITICAL: Must skip trivia before checking for FROM
    if at_sdbl_keyword(p, "FROM", "ИЗ") {
        from_clause(p);
    }

    // WHERE clause (optional)
    p.skip_trivia(); // CRITICAL: Must skip trivia before checking for WHERE
    if at_sdbl_keyword(p, "WHERE", "ГДЕ") {
        where_clause(p);
    }

    // GROUP BY clause (optional)
    p.skip_trivia();
    if at_sdbl_keyword(p, "GROUP", "СГРУППИРОВАТЬ") {
        group_by_clause(p);
    }

    // HAVING clause (optional)
    p.skip_trivia();
    if at_sdbl_keyword(p, "HAVING", "ИМЕЮЩИЕ") {
        having_clause(p);
    }

    // FOR UPDATE clause (optional)
    // Note: We check for FOR UPDATE in one place
    // The function will handle cases where UPDATE is missing
    p.skip_trivia();
    if at_sdbl_keyword(p, "FOR", "ДЛЯ") {
        for_update_clause(p);
    }

    // INDEX BY clause (optional)
    p.skip_trivia();
    if at_sdbl_keyword(p, "INDEX", "ИНДЕКСИРОВАТЬ") {
        index_by_clause(p);
    }

    // ORDER BY clause (optional) - can appear in query()
    p.skip_trivia();
    if at_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ") {
        order_by_clause(p);
    }
}

/// Parse virtual-table method-call arguments (e.g., `.Обороты(&A, , Авто, )`).
///
/// LEGACY: body extracted verbatim from the pre-C1 `table_ref` virtual-table
/// block as a pure refactor so the Slice 8 `table_ref` wrapper can be
/// attested under the Slice 8 clean-room banner without dragging virtual-table
/// scope in. Owns the leading `if p.at(TokenKind::LParen)` guard so the call
/// site in `table_ref` is unconditional. Clean-room rewrite deferred to
/// Slice 5 (virtual table and external-source handling).
fn virtual_table_args_legacy(p: &mut Parser) {
    if p.at(TokenKind::LParen) {
        p.bump(); // (
        p.skip_trivia();

        // Parse arguments (comma-separated expressions)
        // Empty parameters are valid SDBL: .Остатки(, , Авто, ) means "use defaults"
        if !p.at(TokenKind::RParen) {
            // First argument (might be empty — valid for VT params)
            if super::expressions::is_expression_start(p) && !p.at(TokenKind::Comma) {
                super::expressions::expression(p);

                // ERROR RECOVERY: After expression, consume unexpected tokens
                p.skip_trivia();
                if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
                    recover_to_delimiter_vt(p);
                }
            } else {
                let m = p.start();
                m.complete(p, NodeKind::SdblMissingArg);
            }

            // Parse remaining arguments with error recovery
            while p.eat(TokenKind::Comma) {
                p.check_iteration_limit();
                p.skip_trivia();

                // Empty or trailing argument — valid for VT params
                if p.at(TokenKind::Comma)
                    || p.at(TokenKind::RParen)
                    || !super::expressions::is_expression_start(p)
                {
                    let m = p.start();
                    m.complete(p, NodeKind::SdblMissingArg);
                    if !p.at(TokenKind::Comma) {
                        break;
                    }
                    continue;
                }

                super::expressions::expression(p);

                // ERROR RECOVERY: After each argument expression, check for unexpected tokens
                p.skip_trivia();
                if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
                    recover_to_delimiter_vt(p);
                }
            }
        }

        p.skip_trivia();
        p.expect(TokenKind::RParen);
    }
}

/// Parse WHERE clause
///
/// Grammar: `WHERE logicalExpression`
fn where_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "WHERE", "ГДЕ");
    p.skip_trivia();

    // Parse logical expression (AND, OR, NOT, predicates)
    expressions::logical_expression(p);

    m.complete(p, NodeKind::SdblWhereClause);
}

/// Check if current token is an identifier
///
/// Note: Some keywords can be used as identifiers in SDBL
fn is_identifier_token(p: &Parser) -> bool {
    p.at(TokenKind::Ident)
}

/// Check if current token is a clause keyword (FROM, WHERE, GROUP, etc.)
///
/// Used to avoid consuming keywords when parsing aliases and for error recovery.
pub(super) fn is_clause_keyword(p: &Parser) -> bool {
    at_sdbl_keyword(p, "SELECT", "ВЫБРАТЬ")
        || at_sdbl_keyword(p, "FROM", "ИЗ")
        || at_sdbl_keyword(p, "WHERE", "ГДЕ")
        || at_sdbl_keyword(p, "GROUP", "СГРУППИРОВАТЬ")
        || at_sdbl_keyword(p, "HAVING", "ИМЕЮЩИЕ")
        || at_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ")
        || at_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ")
        || at_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ")
        || at_sdbl_keyword(p, "ON", "ПО")
        || at_sdbl_keyword(p, "FOR", "ДЛЯ") // FOR UPDATE
        || at_sdbl_keyword(p, "INDEX", "ИНДЕКСИРОВАТЬ") // INDEX BY
        || at_sdbl_keyword(p, "AUTOORDER", "АВТОУПОРЯДОЧИВАНИЕ")
        || at_sdbl_keyword(p, "TOTALS", "ИТОГИ")
        || is_join_keyword(p)
}

/// Check if current position starts a limitation keyword
///
/// Limitations: DISTINCT, TOP, ALLOWED
fn is_limitation_keyword(p: &Parser) -> bool {
    at_sdbl_keyword(p, "DISTINCT", "РАЗЛИЧНЫЕ")
        || at_sdbl_keyword(p, "TOP", "ПЕРВЫЕ")
        || at_sdbl_keyword(p, "ALLOWED", "РАЗРЕШЕННЫЕ")
}

/// Parse query limitations (DISTINCT, TOP, ALLOWED)
///
/// Grammar (simplified):
/// ```text
/// limitations: (DISTINCT | TOP count | ALLOWED)+
/// ```
///
/// Keywords are accepted in any order to keep the parser tolerant; strict
/// ordering (where required) is enforced by semantic diagnostics, not here.
fn limitations(p: &mut Parser) {
    let m = p.start();

    // Parse keywords in any order until no more limitation keywords found
    while is_limitation_keyword(p) {
        if at_sdbl_keyword(p, "DISTINCT", "РАЗЛИЧНЫЕ") {
            eat_sdbl_keyword(p, "DISTINCT", "РАЗЛИЧНЫЕ");
        } else if at_sdbl_keyword(p, "TOP", "ПЕРВЫЕ") {
            top_clause(p);
        } else if at_sdbl_keyword(p, "ALLOWED", "РАЗРЕШЕННЫЕ") {
            eat_sdbl_keyword(p, "ALLOWED", "РАЗРЕШЕННЫЕ");
        }
        p.skip_trivia();
    }

    m.complete(p, NodeKind::SdblLimitations);
}

/// Parse TOP clause
///
/// Grammar: `TOP count=DECIMAL`
fn top_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "TOP", "ПЕРВЫЕ");
    p.skip_trivia();

    // Expect a number (count)
    if !p.expect(TokenKind::Decimal) {
        // Error recovery: complete anyway
    }

    m.complete(p, NodeKind::SdblTopClause);
}

/// Parse GROUP BY clause
///
/// Grammar: `GROUP BY expression (, expression)*`
fn group_by_clause(p: &mut Parser) {
    let m = p.start();

    // GROUP/СГРУППИРОВАТЬ keyword
    eat_sdbl_keyword(p, "GROUP", "СГРУППИРОВАТЬ");
    p.skip_trivia();

    // BY/ПО keyword
    if !at_sdbl_keyword(p, "BY", "ПО") {
        // Error recovery: expected BY after GROUP
        m.complete(p, NodeKind::SdblGroupClause);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    // Parse expressions (comma-separated list)
    super::expressions::expression(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        super::expressions::expression(p);
    }

    m.complete(p, NodeKind::SdblGroupClause);
}

/// Parse ORDER BY clause
///
/// Grammar: `ORDER BY orderByItem (, orderByItem)*`
/// orderByItem: expression (ASC | DESC)?
fn order_by_clause(p: &mut Parser) {
    let m = p.start();

    // ORDER/УПОРЯДОЧИТЬ keyword
    eat_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ");
    p.skip_trivia();

    // BY/ПО keyword
    if !at_sdbl_keyword(p, "BY", "ПО") {
        // Error recovery: expected BY after ORDER
        m.complete(p, NodeKind::SdblOrderClause);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    // Parse order by items (comma-separated list)
    order_by_item(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        order_by_item(p);
    }

    m.complete(p, NodeKind::SdblOrderClause);
}

/// Parse single ORDER BY item
///
/// Grammar: `expression (ASC | DESC | ВОЗР | УБЫВ)?`
fn order_by_item(p: &mut Parser) {
    // Parse expression
    super::expressions::expression(p);
    p.skip_trivia();

    // Optional ASC/DESC/ВОЗР/УБЫВ modifier
    if p.at_keyword("ASC") || p.at_keyword("ВОЗР") || p.at_keyword("DESC") || p.at_keyword("УБЫВ")
    {
        p.bump(); // Consume ASC/DESC
        p.skip_trivia();
    }
}

/// Parse HAVING clause
///
/// Grammar: `HAVING logicalExpression`
fn having_clause(p: &mut Parser) {
    let m = p.start();

    // HAVING/ИМЕЮЩИЕ keyword
    eat_sdbl_keyword(p, "HAVING", "ИМЕЮЩИЕ");
    p.skip_trivia();

    // Parse logical expression
    super::expressions::expression(p);

    m.complete(p, NodeKind::SdblHavingClause);
}

/// Parse FOR UPDATE clause
///
/// Grammar: `FOR UPDATE [mdo]`
fn for_update_clause(p: &mut Parser) {
    let m = p.start();

    // FOR/ДЛЯ keyword
    eat_sdbl_keyword(p, "FOR", "ДЛЯ");
    p.skip_trivia();

    // UPDATE/ИЗМЕНЕНИЯ keyword
    eat_sdbl_keyword(p, "UPDATE", "ИЗМЕНЕНИЯ");
    p.skip_trivia();

    // Optional MDO reference
    // If we see an identifier, it might be an MDO reference
    if p.at(TokenKind::Ident) && !is_clause_keyword(p) {
        // Parse MDO reference (Справочник.Контрагенты)
        // This is a simple dot-separated identifier chain
        p.bump(); // First part
        while p.at(TokenKind::Dot) {
            p.check_iteration_limit();
            p.bump(); // Dot
            if p.at(TokenKind::Ident) {
                p.bump();
            } else {
                break;
            }
        }
    }

    m.complete(p, NodeKind::SdblForUpdate);
}

/// Parse INDEX BY clause
///
/// Grammar: `INDEX BY indexingItem (, indexingItem)*`
/// indexingItem: expression
fn index_by_clause(p: &mut Parser) {
    let m = p.start();

    // INDEX/ИНДЕКСИРОВАТЬ keyword
    eat_sdbl_keyword(p, "INDEX", "ИНДЕКСИРОВАТЬ");
    p.skip_trivia();

    // BY/ПО keyword
    if !at_sdbl_keyword(p, "BY", "ПО") {
        // Error recovery: expected BY after INDEX
        m.complete(p, NodeKind::SdblIndexBy);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    // Parse indexing items (comma-separated expressions)
    super::expressions::expression(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        super::expressions::expression(p);
    }

    m.complete(p, NodeKind::SdblIndexBy);
}

/// Parse AUTOORDER clause
///
/// Grammar: `AUTOORDER`
fn autoorder_clause(p: &mut Parser) {
    let m = p.start();

    // AUTOORDER/АВТОУПОРЯДОЧИВАНИЕ keyword
    eat_sdbl_keyword(p, "AUTOORDER", "АВТОУПОРЯДОЧИВАНИЕ");

    m.complete(p, NodeKind::SdblAutoorder);
}

/// Parse TOTALS BY clause
///
/// Grammar: `TOTALS [selectedFields] BY totalsGroup (, totalsGroup)*`
/// totalsGroup: `OVERALL | expression [ONLY? HIERARCHY] [alias]`
///
/// Simplified implementation: parse as comma-separated expressions
fn totals_by_clause(p: &mut Parser) {
    let m = p.start();

    // TOTALS/ИТОГИ keyword
    eat_sdbl_keyword(p, "TOTALS", "ИТОГИ");
    p.skip_trivia();

    // Check if we have selected fields before BY
    // If we see identifiers/expressions before BY, parse them as fields
    // This is a simplified approach - we parse everything as expressions
    // until we hit BY keyword
    while !p.at_end() {
        p.skip_trivia();

        // Check for BY keyword
        if at_sdbl_keyword(p, "BY", "ПО") {
            break;
        }

        // Check for clause keywords (stop parsing if we hit another clause)
        if is_clause_keyword(p) {
            break;
        }

        // Parse expression/field
        if super::expressions::is_expression_start(p) {
            super::expressions::expression(p);

            // Check for comma
            p.skip_trivia();
            if !p.at(TokenKind::Comma) {
                // No comma, check for BY
                continue;
            }
            p.bump(); // Comma
        } else {
            break;
        }
    }

    // BY/ПО keyword (required)
    if !at_sdbl_keyword(p, "BY", "ПО") {
        // Error recovery: expected BY
        m.complete(p, NodeKind::SdblTotalsBy);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    // Parse totals groups (comma-separated)
    // For now, we parse as expressions
    // TODO: Add proper support for OVERALL, HIERARCHY, PERIODS
    super::expressions::expression(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        super::expressions::expression(p);
    }

    m.complete(p, NodeKind::SdblTotalsBy);
}

#[cfg(test)]
mod tests {
    use crate::parse_sdbl;

    #[test]
    fn test_error_recovery_incomplete_field_list() {
        // Test that FROM clause is parsed even when SELECT field list is incomplete
        let input = r#"ВЫБРАТЬ
    Очередь.
ИЗ
    РегистрСведений.ОчередьОбновленияКэширующихДанных КАК Очередь"#;

        let parse = parse_sdbl(input);
        let tree_text = format!("{:#?}", parse.syntax_node());

        // Should have ERROR node marking incomplete field
        assert!(
            tree_text.contains("ERROR"),
            "Expected ERROR node for incomplete field.\nTree: {}",
            tree_text
        );

        // But FROM clause should still be parsed!
        assert!(
            tree_text.contains("SDBL_FROM_CLAUSE"),
            "FROM clause should be parsed despite incomplete field list.\nTree: {}",
            tree_text
        );

        // Should have SDBL_DATA_SOURCE (table reference)
        assert!(
            tree_text.contains("SDBL_DATA_SOURCE"),
            "Data source should be in FROM clause.\nTree: {}",
            tree_text
        );
    }

    #[test]
    fn test_error_recovery_complete_query_after_incomplete_field() {
        // More complete test: incomplete field, but FROM and WHERE both present
        let input = r#"ВЫБРАТЬ
    Очередь.
ИЗ
    РегистрСведений.Тест КАК Очередь
ГДЕ
    Очередь.Попыток < 3"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        // Should have ERROR node for incomplete field
        assert!(
            text.contains("ERROR"),
            "Expected ERROR node for incomplete field.\nTree: {}",
            text
        );

        // Should have FROM clause
        assert!(text.contains("SDBL_FROM_CLAUSE"), "FROM clause should be parsed.\nTree: {}", text);

        // Should have WHERE clause
        assert!(
            text.contains("SDBL_WHERE_CLAUSE"),
            "WHERE clause should be parsed.\nTree: {}",
            text
        );
    }

    #[test]
    fn test_error_recovery_incomplete_field_in_middle_of_list() {
        // Real-world case: incomplete field IN THE MIDDLE of field list (not at the end)
        // User types: "Очередь.," - comma after dot without field name
        let input = r#"ВЫБРАТЬ ПЕРВЫЕ 500
    Очередь.,
    Очередь.ЗависимыйОбъектМетаданных КАК ЗависимыйОбъектМетаданных
ИЗ
    РегистрСведений.ОчередьОбновленияКэширующихДанных КАК Очередь
ГДЕ
    Очередь.Попыток < 3"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        // Should have ERROR node for incomplete field
        assert!(
            text.contains("ERROR"),
            "Expected ERROR node for incomplete field.\nTree: {}",
            text
        );

        // But FROM clause should still be parsed!
        assert!(
            text.contains("SDBL_FROM_CLAUSE"),
            "FROM clause should be parsed despite incomplete field in middle of list.\nTree: {}",
            text
        );

        // Should have SDBL_DATA_SOURCE (table reference)
        assert!(
            text.contains("SDBL_DATA_SOURCE"),
            "Data source should be in FROM clause.\nTree: {}",
            text
        );

        // Should have WHERE clause
        assert!(
            text.contains("SDBL_WHERE_CLAUSE"),
            "WHERE clause should be parsed.\nTree: {}",
            text
        );

        // Should have multiple SDBL_SELECTED_FIELD (both incomplete and complete fields)
        let field_count = text.matches("SDBL_SELECTED_FIELD").count();
        assert!(
            field_count >= 2,
            "Should have at least 2 selected fields (incomplete + complete). Got: {}",
            field_count
        );
    }

    #[test]
    fn test_tuple_in_in_predicate() {
        // Tuple on left side of IN predicate - row-wise comparison
        let input = r#"ВЫБРАТЬ *
ИЗ Документ.Заказ КАК Заказ
ГДЕ (Заказ.Партнер, Заказ.Контрагент, Заказ.Организация) В
    (ВЫБРАТЬ Т.Партнер, Т.Контрагент, Т.Организация ИЗ ВТ_Данные КАК Т)"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        // Should have TUPLE_EXPR node
        assert!(
            text.contains("SDBL_TUPLE_EXPR"),
            "Expected SDBL_TUPLE_EXPR for tuple in IN predicate.\nTree: {}",
            text
        );

        // Should have IN expression
        assert!(text.contains("SDBL_IN_EXPR"), "Expected SDBL_IN_EXPR.\nTree: {}", text);

        // Should have subquery inside IN
        assert!(
            text.contains("SDBL_SUBQUERY"),
            "Expected subquery inside IN predicate.\nTree: {}",
            text
        );

        // No parse errors expected
        assert!(!parse.has_errors(), "Should parse without errors: {:?}", parse.errors());
    }

    #[test]
    fn test_simple_tuple() {
        // Simple tuple with multiple elements
        // Note: using "Г" instead of "В" because "В" is IN keyword in Russian
        let input = r#"ВЫБРАТЬ * ИЗ Т ГДЕ (А, Б, Г) = (1, 2, 3)"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        // Should have 2 TUPLE_EXPR nodes (left and right of =)
        let tuple_count = text.matches("SDBL_TUPLE_EXPR").count();
        assert!(
            tuple_count >= 2,
            "Expected at least 2 SDBL_TUPLE_EXPR nodes. Got: {}\nTree: {}",
            tuple_count,
            text
        );
    }

    #[test]
    fn test_paren_expr_not_tuple() {
        // Single expression in parentheses should NOT be a tuple
        let input = r#"ВЫБРАТЬ (А + Б) КАК Сумма ИЗ Т"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        // Should NOT have TUPLE_EXPR
        assert!(
            !text.contains("SDBL_TUPLE_EXPR"),
            "Single parenthesized expression should not create SDBL_TUPLE_EXPR.\nTree: {}",
            text
        );

        // Should have PAREN_EXPR
        assert!(
            text.contains("SDBL_PAREN_EXPR"),
            "Expected SDBL_PAREN_EXPR for single parenthesized expression.\nTree: {}",
            text
        );
    }

    #[test]
    fn test_tuple_in_virtual_table_params() {
        // Tuple inside virtual table parameters
        let input = r#"ВЫБРАТЬ *
ИЗ РегистрНакопления.Расчеты.Обороты(
    &Начало,
    &Конец,
    ,
    (Аналитика.Партнер, Аналитика.Контрагент) В
        (ВЫБРАТЬ Т.Партнер, Т.Контрагент ИЗ ВТ_Данные КАК Т)
) КАК Обороты"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        // Should have TUPLE_EXPR
        assert!(
            text.contains("SDBL_TUPLE_EXPR"),
            "Expected SDBL_TUPLE_EXPR in virtual table params.\nTree: {}",
            text
        );

        // Should have TABLE_REF (virtual table)
        assert!(text.contains("SDBL_TABLE_REF"), "Expected SDBL_TABLE_REF.\nTree: {}", text);
    }
}
