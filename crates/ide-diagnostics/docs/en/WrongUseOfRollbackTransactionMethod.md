# Not recommended using of RollbackTransaction method (WrongUseOfRollbackTransactionMethod)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
`RollbackTransaction` / `ОтменитьТранзакцию` should be called in the `Exception` branch of a transaction handling block, before any other executable statements.

The current implementation reports a call when it is placed:

- outside `Try ... Exception`;
- inside the `Try` body;
- in the `Exception` branch, but not as the first executable statement.
## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->
```bsl
BeginTransaction();
Attempt
    CommitTransaction();
Exception
    WriteLogEvent(NStr("en = 'OperationExecution'"),
EventLogLevel.Error,
        ,
        ,
        DetailedErrorPresentation(InformationAboutError()));
    RollbackTransaction();
    CallException; // there is external transaction
EndTry;
```
## Sources
- [Transactions: Rules of Use (RU)](https://its.1c.ru/db/v8std/content/783/hdoc/_top/)
- [v8std.ru: WrongUseOfRollbackTransactionMethod (RU)](https://v8std.ru/diagnostics/bslls/WrongUseOfRollbackTransactionMethod/)
