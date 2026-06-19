# Interceptor signature mismatch (WeavingSignatureMismatch)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

A configuration extension can intercept a base-module method with the `&Вместо`
(Around / `&Instead`), `&Перед` (Before) or `&После` (After) annotation. The
platform applies such an interceptor only when its signature matches the extended
method exactly:

- the **number of parameters** must be the same;
- each parameter must agree on the by-value keyword `Знач` (`Val`);
- a `&Вместо` replacement must be the same **kind** as the base method —
  a function may only be replaced by a function, a procedure by a procedure
  (an extended function can be extended only with `&Вместо`).

If the signatures diverge, the extension fails the applicability check and the
interception silently does not take effect, so the divergence is a real defect.

Parameter names may differ (a prefix is recommended), and default values are not
repeated in the interceptor — they are taken from the base method, so they are
not compared.

## Examples

### Wrong

```bsl
// Base method: Процедура ПриЗаписи(Отказ, ПараметрыЗаписи)

&Перед("ПриЗаписи")
Процедура Расш1_ПриЗаписи(Отказ) // one parameter instead of two
    // ...
КонецПроцедуры
```

### Correct

```bsl
&Перед("ПриЗаписи")
Процедура Расш1_ПриЗаписи(Отказ, ПараметрыЗаписи)
    // ...
КонецПроцедуры
```

## Sources

- [1C:Enterprise — Configuration extensions (RU)](https://its.1c.ru/db/v8327doc#bookmark:dev:TI000001535)
