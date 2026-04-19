# Provenance: CompilationDirectiveLost

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in public 1C guidance about compilation
directives in managed form and command modules.

Primary sources:

- ITS `pubv8devui`
- v8std `#std439`
- public v8-code-style materials about form-module pragmas

The rule is behavioral and architectural: in form and command modules, method
execution context should be made explicit through a compilation directive.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/compilation_directive_lost.rs` is local
and HIR-based:

- it checks only form modules and command modules;
- it reads procedure/function annotations from the local item tree;
- it reports methods whose annotation list is empty.

This favors permissive treatment because the implementation is a small local
check over public language and platform concepts rather than a copied parser
or visitor structure.

### Documentation

Public documentation was rewritten during this audit to explain explicit
execution-context requirements in form and command modules using public 1C
materials.

### Tests

Current tests are local and inline. During this audit, the most upstream-like
fixture names were rewritten to use fresh scenarios while preserving the same
behavioral coverage:

- method with directive;
- method without directive;
- mixed module with one missing directive;
- English-keyword variant;
- multiple missing directives;
- regular module exclusion.

## Remaining caveats

- the diagnostic idea overlaps with public `bsl-ls` and v8-code-style checks on
  the same platform behavior;
- repository history may still contain earlier upstream-aligned wording and
  fixture names;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CompilationDirectiveLost` is a reasonable permissive candidate because:

- the rule follows public 1C guidance on compilation directives;
- the current implementation is local and HIR-based;
- the active docs and tests no longer need copyleft treatment on their face.
