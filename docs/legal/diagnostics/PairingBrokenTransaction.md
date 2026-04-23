# Provenance: PairingBrokenTransaction

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic has a public standards-based rationale.

`#std783` ("Transactions: rules of use") supports the requirement that
transaction start and finish calls must be paired correctly. That public rule is
well established independently of any specific upstream implementation.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/pairing_broken_transaction.rs`
is substantial and clearly local:

- it builds and traverses local CFGs for each method;
- it tracks transaction state per execution path rather than by a simple stack;
- it treats both `CommitTransaction()` and `RollbackTransaction()` as valid
  closers;
- it uses local heuristics such as `maxTransactionLevel` and special handling
  for `TryExcept` / dead-code edges.

This strongly favors permissive treatment because the concrete analysis logic is
an original local implementation of a public transaction rule.

### Documentation

RU/EN documentation was rewritten during this audit to distinguish the public
transaction rule from the current local CFG-based implementation.

### Tests

Current tests are local and extensive. They cover:

- straightforward matching and mismatching cases;
- branching paths;
- `Try/Except` patterns with `Commit` vs `Rollback`;
- orphaned `Commit` / `Rollback`;
- loops and CFG edge cases;
- configuration of `maxTransactionLevel`.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`PairingBrokenTransaction` is a strong permissive candidate because it
implements a public 1C transaction rule through a substantial local CFG-based
analysis, with local tests and now-local documentation.
