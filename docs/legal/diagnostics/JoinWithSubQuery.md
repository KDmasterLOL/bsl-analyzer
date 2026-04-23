# Provenance: JoinWithSubQuery

## Status

Rule is a good permissive candidate, but the current implementation still
depends on the broader SDBL parser and `sdbl-hir` audit.

## Why this rule exists

This diagnostic is directly grounded in public 1C guidance.

The primary source is `#std655`, which explicitly says not to use joins with
subqueries and recommends temporary tables instead.

So the rule idea, rationale, and the general remediation strategy are public.

## Audit result

### Production code

Current handler in
`crates/ide-diagnostics/src/handlers/join_with_sub_query.rs` is minimal and
local:

- it reacts to `sdbl_hir::SdblDiagnostic::JoinWithSubQuery`;
- it maps the SDBL range back into the BSL source;
- it reports a local diagnostic message.

However, the actual detection of problematic joins happens below this layer in
the SDBL parser / lowering stack. That stack is still under separate audit.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
through `#std655` and the temporary-table rewrite pattern, without relying on
upstream wording.

### Tests

The largest inline multi-case fixture was rewritten during this audit to use new
local query scenarios and names. The remaining tests are local inline Rust
cases for:

- left, right, and inner joins with subqueries;
- subquery sources in `FROM` together with joins;
- multiline queries;
- a negative case where subqueries exist but no join is used.

## Important caveat

Even though the rule itself is public and strong, the implementation still
depends on SDBL infrastructure whose provenance is not yet fully closed:

- `crates/parser`
- SDBL-related lexer logic
- `crates/sdbl-hir`

So this diagnostic should currently be treated as:

`rule is clean, implementation depends on SDBL audit`

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader parser / SDBL audit.

## Conclusion

`JoinWithSubQuery` has a strong public standards basis and locally rewritten
docs/tests, but it cannot be treated as fully cleared independently from the
ongoing SDBL parser audit.
