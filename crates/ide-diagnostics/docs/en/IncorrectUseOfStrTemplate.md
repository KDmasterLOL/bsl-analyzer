# Incorrect use of "StrTemplate" (IncorrectUseOfStrTemplate)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
This diagnostic validates the API contract of `StrTemplate` / `СтрШаблон`.

The current implementation checks several common problems:

- the number of passed arguments does not match the placeholders in the template;
- invalid placeholder numbers such as `%0` or `%11` are used;
- `%%` escapes are handled incorrectly;
- a template is passed indirectly through a variable that resolves to an invalid
  literal;
- misplaced parentheses around `NStr(...)` make the template expression wrong.

It is also important to remember:

- `StrTemplate` supports placeholders from `%1` to `%10`;
- if a digit must follow a substituted value immediately, the placeholder should
  use parentheses, for example `"%(1)45"`.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

Invalid: the number of arguments does not match the placeholders

```bsl
StrTemplate("Name (version %1)");
StrTemplate("%1 (version %2)", Name);
```

Invalid: `NStr` parentheses are misplaced

```bsl
StrTemplate(NStr("en='Name (version %1)'", Version()));
```

Correct:

```bsl
StrTemplate(NStr("en='Name (version %1)'"), Version());
StrTemplate("Name %(1)2", Name);
```

## Sources

This diagnostic has no direct normative 1C standard source.

Related public context:

* [ITS / v8std #std763: localization requirements for formatting strings](https://its.1c.ru/db/v8std#content:763:hdoc)
