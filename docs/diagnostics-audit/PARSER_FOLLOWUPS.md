# Parser Follow-Up Audit Cards

Surfaced by Track 6.1 §5.2 structured-error plumbing.

## PARSER-BUG-001: Nested subquery source alias boundary

**Symptom**: valid nested FROM-subquery inputs such as `SELECT * FROM (SELECT * FROM (SELECT 1) AS Inner) AS Outer` trigger `Unexpected { found: Some(RParen), recovery: BumpToken }`, followed by `Expected { expected: [RParen], found: Some(Ident), recovery: BumpToken }` at the outer alias boundary.
**Affected tests**: `crates/parser/tests/sdbl_parser_tests.rs:test_subquery_nested`; `crates/parser/tests/sdbl_slice8_sources.rs:test_from_subquery_source_nested`.
**Root cause hypothesis**: subquery-source parsing consumes or recovers around the inner closing parenthesis in a way that leaves the outer subquery-source state expecting another `)` when the alias identifier is reached.
**Suggested fix**: audit SDBL subquery-source closing delimiter ownership and alias handoff so nested `(SELECT ... ) AS Alias` sources close exactly once before parsing the source alias.
**Track**: Track 6.1 §5.2 surfaced this via structured-error plumbing.

## PARSER-BUG-002: Expression parser clause-boundary recovery

**Symptom**: valid SDBL expressions in SELECT fields or WHERE clauses trigger `Unexpected { found: Some(KwIn), recovery: BumpToken }` when the expression parser reaches a following clause-boundary keyword such as `FROM` / `ИЗ`.
**Affected tests**: `crates/parser/tests/sdbl_parser_tests.rs:test_slice10a_flat_additive_associativity`; `crates/parser/tests/sdbl_slice10a_backbone.rs:test_slice10a_precedence_and_binds_tighter_than_or`; `crates/parser/tests/sdbl_slice10a_backbone.rs:test_slice10a_precedence_mul_binds_tighter_than_add`; `crates/parser/tests/sdbl_slice10a_backbone.rs:test_slice10a_flat_additive_three_operands`; `crates/parser/tests/sdbl_slice10a_backbone.rs:test_slice10a_flat_logical_or_three_operands`; `crates/parser/tests/sdbl_slice10a_backbone.rs:test_slice10a_bilingual_or_and_not`.
**Root cause hypothesis**: SDBL expression parsing does not consistently include clause-boundary tokens in the stop/recovery set, so a valid expression body is parsed and then the boundary keyword is reported as unexpected.
**Suggested fix**: thread the caller's follow set through expression parsing or add clause-boundary tokens to the expression parser's stop set for SELECT-field and WHERE-clause contexts.
**Track**: Track 6.1 §5.2 surfaced this via structured-error plumbing.
