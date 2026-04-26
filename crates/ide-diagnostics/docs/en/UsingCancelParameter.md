# Using parameter "Cancel" (UsingCancelParameter)

## Description

In event handlers that receive the `Cancel` / `Отказ` parameter, you should not overwrite it in a way that can reset a previously set cancellation flag.

The safe forms are:

- `Cancel = True`
- `Cancel = Cancel Or Check()`
- `Cancel = Check() Or Cancel`

Assignments such as `Cancel = False`, `Cancel = Check()`, or expressions with `And` are unsafe because they can discard an earlier cancellation decision made by another check or another handler.

## Examples

### Incorrect

```bsl
Procedure BeforeWrite(Cancel)
  ...
  Cancel = CheckName();
  ...
EndProcedure
```

### Correct

```bsl
Procedure BeforeWrite(Cancel)
  ...
  If CheckName() Then
   Cancel = True;
  EndIf;
  ...
EndProcedure
```

or

```bsl
Cancel = Cancel or CheckName();
```

## Sources

* [Standard: Working with the "Cancel" parameter in event handlers (RU)](https://its.1c.ru/db/v8std#content:686:hdoc)
* [v8std: UsingCancelParameter](https://v8std.ru/diagnostics/bslls/UsingCancelParameter/)
