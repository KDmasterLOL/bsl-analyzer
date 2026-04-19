# Provenance: DataExchangeLoading

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in published 1C guidance for object event handlers
that participate in data exchange.

Primary sources:

- ITS / v8std `#std773`
- ITS / v8std `#std464`
- ITS / v8std `#std465`
- ITS / v8std `#std752`

The rule is behavioral and standards-based: event handlers such as
`ПередЗаписью`, `ПриЗаписи`, and `ПередУдалением` should check
`ОбменДанными.Загрузка` before running business logic.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/data_exchange_loading.rs` is local and
HIR-based:

- it applies only to relevant module types and monitored procedure names;
- it detects the `DataExchange.Load` / `ОбменДанными.Загрузка` guard through
  local expression matching;
- it validates that the guarded branch contains a `Return`.

This favors permissive treatment because the rule follows published 1C
standards and the implementation is a local HIR walk rather than an upstream
visitor.

### Documentation

Public documentation was rewritten during this audit to explain the rule from
the public 1C standards and to avoid inherited English wording.

### Tests

Current tests are local and inline. During this audit, two upstream-like guard
conditions were replaced with neutral local examples while preserving the same
coverage:

- guard with a combined condition;
- negated guard with an additional boolean predicate.

Covered scenarios include:

- missing guard;
- valid Russian and English guards;
- missing return in guard branch;
- irrelevant procedure names;
- case-insensitive names;
- wrong field and wrong condition;
- `findFirst` configuration behavior.

## Remaining caveats

- the diagnostic idea overlaps with public `bsl-ls`, v8-code-style, and ACC
  checks on the same published standard;
- repository history may still contain earlier upstream-aligned wording or test
  material;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`DataExchangeLoading` is a strong permissive candidate because:

- the rule directly follows from published 1C standards;
- the active implementation is local and HIR-based;
- the current docs and tests do not require retaining copyleft treatment on
  their face.
