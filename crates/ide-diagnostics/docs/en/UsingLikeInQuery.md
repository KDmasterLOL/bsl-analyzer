# Using 'LIKE' in query (UsingLikeInQuery)

## Description

This diagnostic reports any usage of the `LIKE` / `ПОДОБНО` operator in query text.

The rule is conservative: even when pattern matching looks acceptable in a specific case, the behavior of `LIKE` can still depend on DBMS details and query semantics. For that reason, the current implementation flags all occurrences and leaves the final decision to the developer.

## Examples

Reported:

```bsl
Property LIKE "123%"
```

Reported:

```bsl
Property LIKE Table.Template
```

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->
<!-- Примеры источников

* Источник: [Стандарт: Тексты модулей](https://its.1c.ru/db/v8std#content:456:hdoc)
* Полезная информация: [Отказ от использования модальных окон](https://its.1c.ru/db/metod8dev#content:5272:hdoc)
* Источник: [Cognitive complexity, ver. 1.4](https://www.sonarsource.com/docs/CognitiveComplexity.pdf) -->

- [Standard. Features of use in operator requests LIKE (RU)](https://its.1c.ru/db/v8std#content:726:hdoc)
- [Developers guide. Pattern-like string validation operator  (RU)](https://its.1c.ru/db/v8318doc#bookmark:dev:TI000000506)
