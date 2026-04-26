# Mismatched argument count (MismatchedArgCount)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic reports calls where the number of passed arguments does not
match the resolved callee signature.

The current implementation works for calls that were successfully resolved to a
specific target, for example:

- qualified module calls such as `CommonModule.Method(...)`;
- built-in platform functions and methods with known signatures.

This is a semantic correctness check: even if the code is syntactically valid,
the call may still be wrong if too few or too many arguments are passed.

## Examples

Invalid:

```bsl
Процедура Сложение(Левый, Правый) Экспорт
КонецПроцедуры

Процедура Тест()
    ОбщийМодуль.Сложение(1);
КонецПроцедуры
```

Correct:

```bsl
Процедура Сложение(Левый, Правый) Экспорт
КонецПроцедуры

Процедура Тест()
    ОбщийМодуль.Сложение(1, 2);
КонецПроцедуры
```

## Sources

This diagnostic has no direct normative 1C standard source.
