# Missing Compilation Directive on a Method (CompilationDirectiveLost)

## Description

Methods in managed form modules and command modules should explicitly declare
their execution context with a compilation directive such as `&AtClient`,
`&AtServer`, or `&AtServerNoContext`.

If a method in one of these module types has no compilation directive, the
execution context becomes implicit. This makes the code harder to read and can
lead to errors, especially in web-client scenarios and client-server
interaction.

## Examples

Incorrect:

```bsl
Procedure ProcessData(Cancel)
    // The execution context is not stated explicitly
EndProcedure
```

Correct:

```bsl
&AtServer
Procedure ProcessData(Cancel)
    // The method runs on the server
EndProcedure
```

## Sources

- [ITS: Developing the UI of applied solutions on the 1C:Enterprise platform (RU)](https://its.1c.ru/db/pubv8devui#content:189:1)
- [v8std: #std439 Compilation directives and preprocessor instructions](https://v8std.ru/std/439/)
- [v8std: form-module-pragma](https://v8std.ru/diagnostics/v8-code-style/form-module-pragma/)
- [v8std: form-module-missing-pragma](https://v8std.ru/diagnostics/v8-code-style/form-module-missing-pragma/)
