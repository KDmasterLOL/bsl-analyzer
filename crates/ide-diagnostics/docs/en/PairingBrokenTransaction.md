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
