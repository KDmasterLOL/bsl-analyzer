# Weaving annotation not applicable (WeavingAnnotationNotApplicable)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

A configuration extension can intercept a base-module method with the `&Вместо`
(Around / `&Instead`), `&Перед` (Before) or `&После` (After) annotation. The
platform restricts which annotation may target a **function**: an extended
function can be extended **only** with `&Вместо`. The `&Перед` and `&После`
annotations are available for procedures only.

This diagnostic reports a `&Перед` / `&После` interceptor whose target is a
function. The platform rejects such an interception during the applicability
check, so the interceptor silently does not take effect.

## Examples

### Wrong

```bsl
// Base method: Функция ВычислитьСумму(Параметры) Экспорт ... КонецФункции

&После("ВычислитьСумму") // &После cannot target a function
Процедура Расш1_ВычислитьСумму(Параметры)
    // ...
КонецПроцедуры
```

### Correct

```bsl
&Вместо("ВычислитьСумму")
Функция Расш1_ВычислитьСумму(Параметры)
    Результат = ПродолжитьВызов(Параметры);
    // ...
    Возврат Результат;
КонецФункции
```

## Sources

- [1C:Enterprise — Configuration extensions (RU)](https://its.1c.ru/db/v8327doc#bookmark:dev:TI000001535)
