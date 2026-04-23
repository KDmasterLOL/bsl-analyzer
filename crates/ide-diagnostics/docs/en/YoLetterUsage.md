# Using Russian character "yo" ("ё") in code (YoLetterUsage)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

According to the module text style guidelines, the Russian letter `ё` should not be used in source code identifiers.

The current implementation checks only identifiers such as variable names, procedure names, function names, and references to them. It does not report string literals, comments, or other user-facing texts.

This keeps interface messages and other display texts outside the scope of the diagnostic.

## Examples

### Reported

```bsl
Перем ПодсчётИтогов;
```

### Not reported

```bsl
Сообщить("Подсчёт итогов завершён");
```

## Sources

- [Standard: Module texts (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
- [v8std.ru: YoLetterUsage (RU)](https://v8std.ru/diagnostics/bslls/YoLetterUsage/)
