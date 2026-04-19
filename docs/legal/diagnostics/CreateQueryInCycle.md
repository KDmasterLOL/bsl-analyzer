# Provenance: CreateQueryInCycle

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in published 1C guidance on repeated execution of
similar queries.

Primary sources:

- ITS / v8std `#std436`

The rule is architectural and performance-oriented: repeated query execution in
loops usually causes avoidable database round-trips and should be replaced with
single-query or batched access patterns where practical.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/create_query_in_cycle.rs` is local and
HIR-based:

- the user-facing diagnostic is created from a local HIR body diagnostic;
- detection is tied to local lowering logic in `hir-def`;
- the handler itself only maps that local diagnostic to an IDE diagnostic.

This favors permissive treatment because the active code path is based on local
HIR infrastructure rather than an upstream AST visitor.

### Documentation

Public documentation was rewritten during this audit to explain the rule from
`#std436` and to avoid inherited terse wording.

### Tests

Current tests are local and inline:

- query execution in a `for each` loop;
- query created outside the loop but executed inside it;
- English-keyword variant;
- case-insensitive variant;
- query builder variant.

During this audit, a misleading test name was clarified to match the actual
behavior being checked.

## Remaining caveats

- the diagnostic idea overlaps with public `bsl-ls` material and with the
  public 1C standard on repeated queries;
- deeper provenance of the HIR body-diagnostic origin still depends on the
  broader audit of `hir-def` lowering logic;
- repository history may still contain earlier upstream-aligned wording.

## Conclusion

`CreateQueryInCycle` is a reasonable permissive candidate because:

- the rule directly follows from `#std436`;
- the active implementation is integrated through local HIR infrastructure;
- the current docs and tests do not require retaining copyleft treatment on
  their face.
