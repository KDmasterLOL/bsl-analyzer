# Provenance: BeginTransactionBeforeTryCatch

## Status

Candidate for `MIT OR Apache-2.0` after removal of obvious upstream-specific
docs/tests residue.

## Why this rule exists

This diagnostic is grounded in official 1C guidance for transaction handling.

Relevant standards and public references:

- ITS / v8std `#std783`: `Транзакции: правила использования`
- public `v8std.ru` rule page `v8cs:begin-transaction`

The core requirement is standards-based: `НачатьТранзакцию()` should stand
immediately before `Попытка`, and executable code should not appear between them.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/begin_transaction_before_try_catch.rs`
is integrated into the local HIR/body-diagnostics pipeline.

Favorable factors:

- diagnostics are created from local HIR/body analysis;
- the current Rust entry point is shaped around local diagnostic conversion;
- this file no longer contains explicit `ported from` wording.

### Documentation

Local English and Russian documentation were rewritten to refer to the rule
through official 1C standards and public `v8std` references instead of upstream
wording.

### Tests

The previous large combined fixture was structurally close to the upstream
`BeginTransactionBeforeTryCatchDiagnostic.bsl` scenario set.

During this audit, that combined fixture was replaced with a new independently
written multi-case module. Smaller targeted tests for specific transaction
patterns remain local and generic.

## Remaining caveats

- the diagnostic idea is standards-based, but earlier repository history still
  contains upstream-aligned material;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`BeginTransactionBeforeTryCatch` is a solid permissive candidate because:

- the rule follows directly from official 1C transaction guidance;
- the current implementation uses local HIR infrastructure;
- the most obvious borrowed docs and test fixture were replaced.
