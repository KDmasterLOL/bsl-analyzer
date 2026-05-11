# Exception block silently swallows the error (MissingCodeTryCatchEx)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

It is unacceptable to catch any exception, without any trace for system administrator.

*Incorrect*

```bsl
Try
    // code causing exception
    ....
Raise // catch any exception
EndTry;

```

As a rule, such a design hides a real problem, which is subsequently impossible to diagnose.

The current implementation classifies the `Exception` body (Track 2 Phase D
§2.2) and fires for three cases:

- the block is empty (no statements);
- the block silently swallows the failure — it has statements, but none of
  them re-raise (`Raise` / `ВызватьИсключение`) or call a logging API from
  the platform registry (`Message`, `WriteLogEvent`, `Сообщить`,
  `ЗаписьЖурналаРегистрации`);
- the block only rolls back the transaction (`RollbackTransaction` /
  `ОтменитьТранзакцию`) without recording or propagating the failure —
  this case emits a rollback-specific message recommending to add a log
  call or `Raise`.

Proper recovery paths — re-raise, logging, or a combination — do not
trigger the diagnostic.

There is one configuration nuance:

- `commentAsCode = true` changes the behavior so that a comment-only `Exception` block is not reported.

*Correct*

```bsl
Try
    // code causing exception
    ....
Raise
    // Explanation why catching all exceptions untraceable for enduser.
    // ....
    // Write to log for system administrator.
    WriteLogEvent(NStr("en = 'Action'"),
       EventLogLevel.Error,,,
       DetailErrorDescription(ErrorInfo()));
EndTry;
```

## Sources

* [Catching Exceptions in Code (RU)](https://its.1c.ru/db/v8std#content:499:hdoc)
* [v8std.ru: MissingCodeTryCatchEx (RU)](https://v8std.ru/diagnostics/bslls/MissingCodeTryCatchEx/)
