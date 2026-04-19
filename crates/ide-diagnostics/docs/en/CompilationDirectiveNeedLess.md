# Redundant Compilation Directive (CompilationDirectiveNeedLess)

## Description

Compilation directives such as `&AtClient`, `&AtServer`, and
`&AtServerNoContext` are intended for managed form modules and command
modules.

In other module types, the execution context is usually defined by the module
kind or by metadata flags. Adding compilation directives there is redundant and
makes the code harder to understand. In mixed client-server common modules,
directives can also obscure which methods are actually available in the final
context.

## Examples

Incorrect:

```bsl
&AtServer
Procedure RecalculateTotals()
EndProcedure
```

Correct:

```bsl
Procedure RecalculateTotals()
EndProcedure
```

## Sources

- [ITS: Use of compilation directives and preprocessor instructions (RU)](https://its.1c.ru/db/v8std#content:439:hdoc)
- [v8std: #std439 Compilation directives and preprocessor instructions](https://v8std.ru/std/439/)
- [v8std: form-module-pragma](https://v8std.ru/diagnostics/v8-code-style/form-module-pragma/)
