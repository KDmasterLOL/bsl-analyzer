# Cognitive complexity (CognitiveComplexity)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic reports procedures and functions whose cognitive complexity is
higher than the configured threshold.

Cognitive complexity is a readability metric. It grows when the reader has to
keep more execution branches, nested blocks, and boolean conditions in mind.
Large methods with deeply nested logic are usually harder to review, debug, and
change safely.

## How the score grows

The implementation follows the public SonarSource specification.

- Structural constructs such as `If`, loops, `Except`, and ternary expressions
  add complexity.
- Nested control flow adds an extra penalty on top of the base increment.
- `ElseIf`, `Else`, `Goto`, and boolean `And` / `Or` also increase the score,
  but without the same nesting penalty as full structural blocks.

## Recommendations

- split long methods into smaller routines with clear names;
- use guard clauses to reduce indentation depth;
- simplify composite conditions when they combine unrelated checks;
- move repeated branch-specific logic into helper procedures or functions.

## Example

```bsl
Функция ПодготовитьДанные(Данные)
    Если Данные = Неопределено Тогда
        Возврат Неопределено;
    КонецЕсли;

    Для Каждого Элемент Из Данные Цикл
        Если Элемент.Актуален Тогда
            Если Элемент.НужнаПроверка Тогда
                Если Элемент.Значение > 0 Тогда
                    Возврат Элемент;
                КонецЕсли;
            КонецЕсли;
        КонецЕсли;
    КонецЦикла;

    Возврат Неопределено;
КонецФункции
```

## Sources

Primary source: [SonarSource Cognitive Complexity specification v1.4](https://www.sonarsource.com/docs/CognitiveComplexity.pdf)
