# CommitTransactionOutsideTryCatch provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule directly follows public 1C transaction guidelines. `#std783` requires `ЗафиксироватьТранзакцию` to appear as the last statement in the `Попытка` branch before `Исключение`, so the diagnostic idea is standards-based rather than an original upstream invention.

## Public sources

- `#std783` Transactions: rules of use.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It reports `ЗафиксироватьТранзакцию` / `CommitTransaction` when the call:

- appears outside `Попытка ... Исключение`;
- appears in the `Исключение` branch;
- is followed by executable code in the `Попытка` branch.

The implementation is narrower than full transaction-lifecycle analysis and does not attempt to validate all pairing requirements.

## Audit notes

- Rule idea: clean and standards-based.
- Handler comments were reduced to a local description.
- Docs were rewritten to match the actual implementation scope.
- Existing tests are local and exercise the project's own HIR-based behavior.
