# Provenance: LogicalOrInTheWhereSectionOfQuery

## Status

Rule is a good permissive candidate, but the current implementation still
depends on the broader SDBL parser and `sdbl-hir` audit.

## Why this rule exists

This diagnostic is supported by public 1C performance guidance.

The closest public sources are:

- `#std658`, which explains when `ИЛИ` is acceptable in effective query
  conditions and when it harms index usage;
- the 1C methodological material about suboptimal query performance when using
  logical `OR`.

These sources provide clear public rationale for treating many `OR` conditions
in `WHERE` as performance risks.

## Audit result

### Production code

Current handler in
`crates/ide-diagnostics/src/handlers/logical_or_in_the_where_section_of_query.rs`
is minimal and local:

- it reacts to `sdbl_hir::SdblDiagnostic::LogicalOrInWhere`;
- it maps the SDBL range back into the BSL source;
- it reports a local diagnostic message.

However, actual detection is performed below this layer in the SDBL parser /
lowering stack, which is still being audited separately.

### Documentation

Local RU/EN documentation was rewritten during this audit to reflect the real
state more honestly:

- public sources support the performance rationale;
- `UNION ALL` is only one possible rewrite, not a universally safe fix;
- the current implementation may still report some acceptable `OR` usages
  because it does not fully model the distinction from `#std658` between main
  and additional conditions.

### Tests

Current tests include:

- a large inline regression fixture with six diagnostics across multiple
  procedures;
- local smaller inline cases for simple `OR`, Russian and English spellings,
  nested subqueries, and false-positive protection for `CASE` / `JOIN`.

The large inline regression fixture appears inherited from earlier test data and
has not yet been independently rewritten during this audit.

## Important caveat

Even though the rule rationale is publicly supported, the implementation still
depends on SDBL infrastructure whose provenance is not yet fully closed:

- `crates/parser`
- SDBL-related lexer logic
- `crates/sdbl-hir`

There is also an implementation caveat beyond licensing: the current diagnostic
is broader than the ideal rule described in public guidance and may produce
false positives in some acceptable `WHERE` conditions.

So the practical current status is:

`rule is clean; implementation depends on SDBL audit; current behavior is broader than the ideal rule; large regression fixture should be reviewed later`

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader parser / SDBL audit.

## Conclusion

`LogicalOrInTheWhereSectionOfQuery` has strong public performance rationale and
locally rewritten docs, but it should not be considered fully cleared
independently from the ongoing SDBL parser audit and the pending cleanup of the
large inherited regression fixture.
