# Lines of code after the asynchronous method call (CodeAfterAsyncCall)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
Code placed immediately after an asynchronous method call runs right away,
before the asynchronous operation finishes.

This often leads to a mistaken assumption that the next line already sees the
result of the async action. In practice, such code must usually be moved either:

- into the notification handler passed through `NotifyDescription`;
- or into an `await`-style flow such as `Wait SomeAsyncMethod(...)`.

## Examples

### Incorrect

```bsl
&AtClient
Procedure ChooseFile(Command)
    Notification = New NotifyDescription("AfterFileChoice", ThisObject);
    StartPutFile(Notification, , , True);

    Message("The file has already been selected"); // Executes immediately
EndProcedure
```

### Correct

```bsl
&AtClient
Procedure ChooseFile(Command)
    Notification = New NotifyDescription("AfterFileChoice", ThisObject);
    StartPutFile(Notification, , , True);
EndProcedure

&AtClient
Procedure AfterFileChoice(Result, Address, SelectedName, ExtraParameters) Export
    If Result Then
        Message("The file has been selected: " + SelectedName);
    EndIf;
EndProcedure
```

### Async call inside a branch

```bsl
&AtClient
Procedure ProcessData(Command)
    If NeedConfirmation Then
        Notification = New NotifyDescription("AfterConfirmation", ThisObject);
        ShowQueryBox(Notification, "Continue?", QuestionDialogMode.YesNo);
    Else
        RunWithoutConfirmation();
    EndIf;

    RefreshInterface(); // May run before the user answers the question
EndProcedure
```

## Sources

Primary source: [Developer Guide: Built-in language, chapter 4. Sync and async methods (RU)](https://its.1c.ru/db/v8319doc#bookmark:dev:TI000001505)
