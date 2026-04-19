# Query Execution Inside a Loop (CreateQueryInCycle)

## Description

Executing a query repeatedly inside a loop is a serious performance problem.
Each iteration adds an extra database round-trip, increases server load, and
often turns a linear task into an expensive N-query pattern.

In most cases, similar data should be fetched with one query by using list
parameters, temporary tables, `IN`, `UNION ALL`, or a batch query.

## Examples

Incorrect:

```bsl
// BanksToProcess contains a list of banks

IndividualQuery = New Query("
  |SELECT
  |   BankAccounts.Ref AS Account
  |FROM
  |   Catalog.BankAccounts AS BankAccounts
  |WHERE
  |   BankAccounts.Bank = &Bank");

For Each Bank From BanksToProcess Do
  IndividualQuery.SetParameter("Bank", Bank);
  AccountsSelection = IndividualQuery.Execute().Select();
  While AccountsSelection.Next() Do
    ProcessBankAccount(AccountsSelection.Account);
  EndDo;
EndDo;
```

Correct:

```bsl
// BanksToProcess contains a list of banks

MergedQuery = New Query("
  |SELECT
  |   BankAccounts.Ref AS Account
  |FROM
  |   Catalog.BankAccounts AS BankAccounts
  |WHERE
  |   BankAccounts.Bank IN (&BanksToProcess)");

MergedQuery.SetParameter("BanksToProcess", BanksToProcess);
AccountsSelection = MergedQuery.Execute().Select();
While AccountsSelection.Next() Do
  ProcessBankAccount(AccountsSelection.Account);
EndDo;
```

## Sources

- [ITS: Repeated execution of similar queries (RU)](https://its.1c.ru/db/v8std#content:436)
- [v8std: #std436 Repeated execution of similar queries](https://v8std.ru/std/436/)
