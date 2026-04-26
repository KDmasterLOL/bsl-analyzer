# Wrong use of ProceedWithCall function (WrongUseFunctionProceedWithCall)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
`ProceedWithCall` / `ПродолжитьВызов` is intended for extension interception methods. Calling it outside a method marked with `&Around` (`&Вместо`) leads to incorrect extension behavior and usually ends with a runtime error.

The current implementation reports global calls to `ProceedWithCall` / `ПродолжитьВызов` when the enclosing method is not marked with `&Вместо`. It also reports calls from `&Before` / `&After` methods and from ordinary module procedures and functions.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

```bsl
Procedure Test()
    ProceedWithCall(); // Reported here
EndProcedure
```

## Sources
- [Extensions. Functionality -> Modules (RU)](https://its.1c.ru/db/pubextensions#content:54:1)
- [v8std.ru: WrongUseFunctionProceedWithCall (RU)](https://v8std.ru/diagnostics/bslls/WrongUseFunctionProceedWithCall/)
