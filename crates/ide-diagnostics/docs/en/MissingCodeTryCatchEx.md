# Missing code in Raise block in "Try ... Raise ... EndTry" (MissingCodeTryCatchEx)

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

The current implementation reports `Try ... Exception` blocks whose `Exception` branch has no executable statements. By default, a branch that contains only comments is still treated as empty.

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
