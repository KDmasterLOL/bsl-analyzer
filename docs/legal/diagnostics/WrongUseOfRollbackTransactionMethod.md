# WrongUseOfRollbackTransactionMethod provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule directly follows public 1C transaction guidelines. The requirement to call `ОтменитьТранзакцию` first in the `Исключение` branch is stated in `#std783`, so the diagnostic idea is standards-based rather than an original upstream invention.

## Public sources

- `#std783` Transactions: rules of use.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It reports `ОтменитьТранзакцию` / `RollbackTransaction` when the call:

- appears outside `Попытка ... Исключение`;
- appears in the main `Попытка` body;
- is not the first executable statement in the `Исключение` branch.

The implementation is narrower than full transaction-pairing analysis and does not try to verify all transaction lifecycle requirements.

## Audit notes

- Rule idea: clean and standards-based.
- Docs were rewritten to match the actual implementation scope.
- Existing tests are local and exercise the project's own HIR-based behavior.
