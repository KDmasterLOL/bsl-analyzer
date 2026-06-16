# Violation of pairing using methods "BeginTransaction()" & "CommitTransaction()" / "RollbackTransaction()" (PairingBrokenTransaction)

## Description

This diagnostic reports broken transaction pairing inside a single method.

The public rule is straightforward: `BeginTransaction()` must be matched by
either `CommitTransaction()` or `RollbackTransaction()`. In the current project
the check is path-sensitive: it analyzes all execution paths and reports both
unclosed transactions and orphaned `CommitTransaction()` /
`RollbackTransaction()` calls.

The implementation also supports a local safety limit, `maxTransactionLevel`,
to avoid pathological traversal depth.

The analysis correlates simple flag conditions: when `BeginTransaction()` and
its matching `CommitTransaction()` / `RollbackTransaction()` are guarded by the
same condition (e.g. `If LocalTransaction Then …`), infeasible paths where the
flag would be both true and false are dropped, so no false positive is raised.
Correlation applies only to stable variables that are assigned at most once.

## Examples

Correct:

```bsl
Procedure SaveData()
    BeginTransaction();
    Try
        DocumentObject.Write();
        CommitTransaction();
    Except
        RollbackTransaction();
        Raise;
    EndTry;
EndProcedure
```

Incorrect:

```bsl
Procedure StartWrite()
    BeginTransaction();
    WriteDocument();
EndProcedure

Procedure WriteDocument()
    Try
        DocumentObject.Write();
        CommitTransaction();
    Except
        RollbackTransaction();
    EndTry;
EndProcedure
```

## Sources

- Source: [1C standard: Transactions, rules of use (#std783)](https://its.1c.ru/db/v8std#content:783:hdoc)
- Secondary reference: [v8std.ru: PairingBrokenTransaction](https://v8std.ru/diagnostics/bslls/PairingBrokenTransaction/)
