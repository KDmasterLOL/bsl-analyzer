# Provenance: OneStatementPerLine

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic has a public standards-based rationale.

`#std456` ("Module texts") supports the general expectation that module code
should be formatted clearly, with statements separated for readability. The
current implementation uses a strict local interpretation of that guidance:
multiple statements starting on the same line are reported.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/one_statement_per_line.rs`
is local and thin:

- HIR lowering detects multiple statements starting on the same line;
- the handler simply converts that local body diagnostic into a user-facing
  diagnostic;
- exclusions for empty statements, preprocessor cases, and parse-error cases are
  handled in local lowering logic.

This strongly favors permissive treatment because the implementation is a local
formatting check built on top of a public style recommendation.

### Documentation

RU/EN documentation was rewritten during this audit to match the actual current
behavior. In particular, the old wording about allowing same-type assignment
chains was removed because current tests show that such cases are reported.

### Tests

Current tests are local inline fixtures covering:

- several statements on one line;
- multiple statements at end of file;
- ordinary one-statement-per-line code;
- exclusion of statements that contain preprocessor directives.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- the exact strictness of the rule is a local implementation choice layered on
  top of the public style guidance;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`OneStatementPerLine` is a strong permissive candidate because it is a local
implementation of a public 1C formatting recommendation, with local tests and
now-local documentation.
