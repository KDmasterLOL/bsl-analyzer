# Server calls in form events (ServerCallsInFormEvents)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

The form events `OnActivateRow` and `OnStartChoice` are triggered during active user interaction. Server-side calls from these handlers can increase network traffic and noticeably slow down the form.

The current implementation analyzes call chains starting from these handlers and reports:

- local form methods that eventually switch to server execution with context;
- immediate qualified common-module calls when the target export method is server-only.

If the path goes through an idle-handler registration, the diagnostic is downgraded to informational severity.

## Examples

Incorrect:

```bsl
&AtClient
Procedure OnActivateRow(Element, SelectedRow, Field, NewValue, StandardProcessing)
    // Error: server procedure call from client event
    TableFormOnActivateRowAtServer();
    StandardProcessing = False;
EndProcedure

&AtServer
Procedure TableFormOnActivateRowAtServer()
    RaiseException "test";
EndProcedure
```

Correct:

```bsl
Procedure OnActivateRow(Element, SelectedRow, Field, NewValue, StandardProcessing)
    // Correct: only client-side processing
    StandardProcessing = False;
EndProcedure
```

## Sources

- [#std487: Minimizing server calls and traffic (RU)](https://its.1c.ru/db/v8std#content:487:hdoc)
- [#std630: Form module rules (RU)](https://its.1c.ru/db/v8std#content:630:hdoc)
- [Infostart: Server calls that should not be called (RU)](https://infostart.ru/1c/articles/1225834/)
