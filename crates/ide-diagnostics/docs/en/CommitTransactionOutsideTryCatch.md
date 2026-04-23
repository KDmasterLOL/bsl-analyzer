# Violating transaction rules for the 'CommitTransaction' method (CommitTransactionOutsideTryCatch)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

`CommitTransaction` / `ЗафиксироватьТранзакцию` should be the last executable statement in the `Try` branch, immediately before `Exception`.

The current implementation reports a call when it is placed:

- outside `Try ... Exception`;
- inside the `Exception` branch;
- in the `Try` branch, but followed by more executable code.

This diagnostic focuses on the position of `CommitTransaction`. It does not replace full transaction-pairing analysis.

## Examples

### Incorrect

```bsl
BeginTransaction();
Attempt
    WriteData();
    CommitTransaction();
    LogMessage("Saved");
Exception
    RollbackTransaction();
EndTry;
```

### Correct

```bsl
BeginTransaction();
Attempt
    WriteData();
    CommitTransaction();
Exception
    RollbackTransaction();
    Raise;
EndTry;
```

## Sources

* [Transactions: terms of use (RU)](https://its.1c.ru/db/v8std/content/783/hdoc/_top/)
* [v8std.ru: CommitTransactionOutsideTryCatch (RU)](https://v8std.ru/diagnostics/bslls/CommitTransactionOutsideTryCatch/)
