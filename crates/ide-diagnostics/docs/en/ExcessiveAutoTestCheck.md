# Excessive AutoTest Check (ExcessiveAutoTestCheck)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Legacy checks for the `"АвтоТест"` / `"AutoTest"` parameter are no longer needed.

This pattern used to appear in form-opening and fill handlers as an early `Return`, but today it only leaves dead compatibility code in the module.

## Examples
```bsl
If Parameters.Property("AutoTest") Then
    Return;
EndIf;
```

```bsl
If FillData = "AutoTest" Then
    Return;
EndIf;
```

These branches should be removed when they do nothing except return immediately.

## Sources
* [Standard: Module texts (RU)](https://its.1c.ru/db/v8std/content/456/hdoc)
* [v8std.ru: #std456](https://v8std.ru/std/456/)
