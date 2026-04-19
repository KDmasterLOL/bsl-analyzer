# Provenance: CompilationDirectiveNeedLess

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in public 1C guidance about where compilation
directives should and should not be used.

Primary sources:

- ITS / v8std `#std439`
- public v8-code-style materials about form-module pragmas

The rule is behavioral and architectural: compilation directives are useful in
managed form and command modules, but are redundant in module types whose
execution context is already defined by metadata.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/compilation_directive_need_less.rs` is
local and HIR-based:

- it restricts the check to module types where directives are considered
  redundant;
- it walks local method annotations from the item tree;
- it reports only actual compilation directives and ignores extension
  annotations.

This favors permissive treatment because the implementation is a straightforward
local check over public platform semantics.

### Documentation

Public documentation was rewritten during this audit to explain the rule from
public 1C materials rather than inherited short-form wording.

### Tests

Current tests are local and inline. During this audit, the explicitly named
`java fixture` test was renamed and rewritten with fresh method names while
preserving the same behavioral coverage.

Covered scenarios include:

- redundant directives in an object module;
- absence of directives;
- extension annotations not reported;
- command module exclusion;
- unknown module exclusion.

## Remaining caveats

- the diagnostic idea overlaps with public `bsl-ls` and v8-code-style checks on
  the same platform rule;
- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CompilationDirectiveNeedLess` is a reasonable permissive candidate because:

- the rule follows public `#std439` guidance;
- the current implementation is local and HIR-based;
- the active docs and tests do not require retaining copyleft treatment.
