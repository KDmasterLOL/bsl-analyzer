# Parser Follow-Up Audit Cards

Surfaced by Track 6.1 §5.2 structured-error plumbing.

## PARSER-BUG-001: Nested subquery source alias boundary

**Symptom**: valid nested FROM-subquery inputs such as `SELECT * FROM (SELECT * FROM (SELECT 1) AS Inner) AS Outer` trigger `Unexpected { found: Some(RParen), recovery: BumpToken }`, followed by `Expected { expected: [RParen], found: Some(Ident), recovery: BumpToken }` at the outer alias boundary.
**Affected tests**: `crates/parser/tests/sdbl_parser_tests.rs:test_subquery_nested`; `crates/parser/tests/sdbl_slice8_sources.rs:test_from_subquery_source_nested`.
**Root cause hypothesis**: subquery-source parsing consumes or recovers around the inner closing parenthesis in a way that leaves the outer subquery-source state expecting another `)` when the alias identifier is reached.
**Suggested fix**: audit SDBL subquery-source closing delimiter ownership and alias handoff so nested `(SELECT ... ) AS Alias` sources close exactly once before parsing the source alias.
**Track**: Track 6.1 §5.2 surfaced this via structured-error plumbing. Bandaid: `is_known_nested_subquery_alias_recovery` in both test files whitelists the four known error patterns so the suite stays green while the structural fix is deferred.

## PARSER-BUG-002: SDBL test inputs collide with single-char keyword `В`

The "expression clause-boundary recovery" symptom originally documented here decomposes into three independent issues. Only 002a is a test-design fix; 002b and 002c are real grammar / display bugs whose blast radius is much smaller than originally feared.

### 002a — Test fixtures use `В` as a fake identifier

**Symptom**: 6 tests construct fixtures of the form `А + Б + В` / `НЕ А И Б ИЛИ В` to exercise FLAT operator wrappers and precedence ladders with "three Russian-letter operands". `В` is the SDBL keyword for the set-membership operator (Russian twin of `IN`, per ITS pubqlang/10), so the lexer tokenizes it as `KwIn` and the parser treats `+ В` / `ИЛИ В` as an unterminated `IN` predicate (expects `(...)` afterwards). Realistic inputs like `ВЫБРАТЬ Имя + Фамилия ИЗ Сотрудники` or `ВЫБРАТЬ 1 + 2 + 3 ИЗ Т` parse cleanly — the bug only appears because the test author picked a reserved letter.

**Affected tests** (all in `crates/parser/tests/`):
- `sdbl_parser_tests.rs:test_slice10a_flat_additive_associativity`
- `sdbl_slice10a_backbone.rs:test_slice10a_precedence_and_binds_tighter_than_or`
- `sdbl_slice10a_backbone.rs:test_slice10a_precedence_mul_binds_tighter_than_add`
- `sdbl_slice10a_backbone.rs:test_slice10a_flat_additive_three_operands`
- `sdbl_slice10a_backbone.rs:test_slice10a_flat_logical_or_three_operands`
- `sdbl_slice10a_backbone.rs:test_slice10a_bilingual_or_and_not`

**Fix**: rewrite the 6 fixtures to use a single-char Russian operand that is not an SDBL keyword (e.g. `Г` instead of `В` — `Г` is the next letter after `В` and is not reserved). After the rewrite, delete the `is_known_clause_boundary_recovery` whitelist in both files and make the local `parse_no_errors` helper assert `!parse.has_errors()` strictly.

**Track**: Track 6.1 §5.2 surfaced this via structured-error plumbing. Not a parser bug — a test design choice that became visible once errors were structured.

### 002b — Post-`Dot` property-name slot rejects soft keywords

**Symptom**: realistic input `ВЫБРАТЬ Т.А + Т.Б + Т.В ИЗ Т КАК Т` emits two errors:
- `Ожидалось 'идентификатор', встречено Из` — after `Т.` the parser expects `Ident`, lexer hands it `KwIn` (because `В` is the IN-operator keyword), parser refuses.
- `Ожидалось '(', встречено идентификатор` — cascade follow-on.

`crates/parser/src/sdbl_token_converter.rs:55` maps SDBL `S::KwIn → T::KwIn` (keeps keyword status) while most other SDBL keywords (`KwSelect`, `KwFrom`, `KwAs`, …) downgrade to `T::Ident`. The retained-keyword set includes `KwIn`, `KwAnd`, `KwOr`, `KwNot`, `KwTrue`, `KwFalse`, `KwUndefined`. Of these, `KwIn` (`В`) is realistically a column name; the others (`И`, `ИЛИ`, `НЕ`, `Истина`, `Ложь`, `Неопределено`) are unusual but technically valid identifiers.

**Affected sites** (4 `Dot`-loop branches in `grammar/sdbl/expressions.rs` + 2 in `grammar/sdbl/select.rs`):
- `expressions.rs` REFS predicate MDO path
- `expressions.rs` CAST type MDO chain
- `expressions.rs` column reference dotted-path
- `expressions.rs` member access after `CAST(...)` result
- `select.rs::table_ref` FROM-side MDO chain
- `select.rs::for_update_clause` dotted column path

**Fix**: introduce a `pub(super) fn at_property_name(p)` helper in `expressions.rs` that accepts `Ident` plus the full retained-keyword set (`KwIn`/`KwAnd`/`KwOr`/`KwNot`/`KwTrue`/`KwFalse`/`KwUndefined`). Apply at every post-`Dot` property-name slot in both files. Add regression tests for `Т.В`, `Т.И`, `Т.Не`, `Т.Истина`, `Справочник.В` (FROM-side), `ДЛЯ ИЗМЕНЕНИЯ Т.В` (for-update) as column references.

**Track**: Track 6.1 §5.2 surfaced this; locally fixable in a single follow-up slice.

### 002c — SDBL `В` displays as `'Из'` in error messages

**Symptom**: SDBL parse errors involving `KwIn` token (the SDBL `В` / `IN` operator) render as `Неожиданный токен 'Из'` in IDE diagnostics. The SDBL-to-parser-token converter at `crates/parser/src/sdbl_token_converter.rs:55` routes SDBL `S::KwIn` to the shared `lexer::TokenKind::KwIn` enum value, and `parser-error::format_ru` maps that variant to `"Из"` — correct for the BSL `Для Каждого X Из Список` construct (where `KwIn` matches the regex `(?i)из|(?i)in` per `crates/lexer/src/lib.rs:76`), but wrong for the SDBL `В` operator that reuses the same `TokenKind` slot. The other retained-keyword tokens (`KwAnd`/`KwOr`/`KwNot` — SDBL `И`/`ИЛИ`/`НЕ` mapped from `OpAnd`/`OpOr`/`OpNot`) happen to render correctly because the display string coincides between BSL and SDBL semantics. `KwIn` is the only token where the BSL/SDBL display strings diverge.

**Affected files**: `crates/parser-error/src/lib.rs:94` (`TokenKind::KwIn => "Из"`).

**Fix options** (pick one in a dedicated slice — non-trivial):
1. **Per-context display**: format_ru learns to take an optional source-text override (Sink passes `tokens[i].text` when constructing SyntaxError). Most robust, ~30 LOC plumbing.
2. **Split TokenKind**: introduce SDBL-side TokenKind alongside BSL TokenKind in parser-error. Largest blast radius, ~3-day refactor.
3. **Live with bilingual ambiguity**: render as `'Из / В'` in the table. Smallest change, ugly UX.

**Track**: Track 6.1 §5.2 surfaced this; depends on choice of fix shape — own follow-up slice with pair-mode review.
