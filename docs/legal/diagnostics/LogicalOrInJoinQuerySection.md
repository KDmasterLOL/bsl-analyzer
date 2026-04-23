# Provenance: LogicalOrInJoinQuerySection

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

These sources do not define this exact diagnostic one-to-one, but they provide
clear public rationale for warning about `OR` over different fields in join
conditions.

## Audit result

### Production code

Current handler in
`crates/ide-diagnostics/src/handlers/logical_or_in_join_query_section.rs` is
minimal and local:

- it reacts to `sdbl_hir::SdblDiagnostic::LogicalOrInJoin`;
- it maps the SDBL range back into the BSL source;
- it reports a local diagnostic message.

However, actual detection is performed below this layer in the SDBL parser /
lowering stack, which is still being audited separately.

### Documentation

Local RU/EN documentation was rewritten during this audit to tie the rule to
public query-performance guidance and to describe the current scope honestly:

- same-field `OR` is intentionally not reported;
- different-field `OR` in join conditions is reported;
- rewrite options such as `UNION ALL` are examples, not universally safe
  automatic transformations.

### Tests

Current tests include:

- a large inline regression fixture with eight diagnostics in nested joins;
- local smaller inline cases for same-field `OR`, `OR` outside join conditions,
  different-field `OR`, and Russian/English spellings.

The large inline regression fixture appears inherited from earlier test data and
has not yet been independently rewritten during this audit.

## Important caveat

Even though the rule rationale is publicly supported, the implementation still
depends on SDBL infrastructure whose provenance is not yet fully closed:

- `crates/parser`
- SDBL-related lexer logic
- `crates/sdbl-hir`

Also, the large regression fixture remains a cleanup target if the goal is to
maximize confidence in fully permissive provenance for the whole diagnostic
package.

So the practical current status is:

`rule is clean; implementation depends on SDBL audit; large regression fixture should be reviewed later`

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader parser / SDBL audit.

## Conclusion

`LogicalOrInJoinQuerySection` has strong public performance rationale and
locally rewritten docs, but it should not be considered fully cleared
independently from the ongoing SDBL parser audit and the pending cleanup of the
large inherited regression fixture.
