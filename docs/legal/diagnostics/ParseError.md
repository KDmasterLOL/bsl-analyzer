# Provenance: ParseError

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is fundamentally a generic parser-error rule.

The underlying idea is not unique to any particular upstream project: when the
parser produces non-empty error nodes, the analyzer should surface them to the
user. Public 1C guidance such as `#std439` is relevant for some parse-error
cases, especially around preprocessor usage, but the actual rule is broader than
that single standard section.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/parse_error.rs`
is very small and local:

- it traverses the local syntax tree;
- it selects non-empty `SyntaxKind::ERROR` nodes;
- it emits a local `ParseError` diagnostic for each such node.

This strongly favors permissive treatment because the implementation is a direct
local wrapper around the parser output, with no meaningful dependence on copied
upstream expression.

### Documentation

RU/EN documentation was rewritten during this audit to match the actual parser
behavior instead of narrowing the rule to preprocessor-only scenarios.

### Tests

Current tests are local and broad. They cover:

- malformed `If` conditions;
- unterminated strings;
- stray identifiers at EOF;
- BOM handling;
- async procedures with `Ждать`;
- Unicode identifiers;
- cases that should parse without errors.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- repository-wide relicensing still depends on the broader audit of shared
  parser infrastructure;
- because this diagnostic directly reflects parser output, final confidence for
  repo-wide permissive relicensing still depends in part on the parser audit.

## Conclusion

`ParseError` is a strong permissive candidate at the rule and handler level
because it is a generic parser-error surface over local parser output, with
local tests and now-local documentation. The broader parser relicensing question
still remains part of the separate parser audit.
