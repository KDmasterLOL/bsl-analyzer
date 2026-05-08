# QueryParseError

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Сообщает об ошибках разбора текста запроса.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/query_parse_error.rs`
- `<v8std mirror>/docs/diagnostics/bslls/QueryParseError.md`
- `<v8std mirror>/docs/std/437.md`

## Как реализовано

Берет `ctx.all_sdbl_in_file()`, проверяет `ERROR` nodes в SDBL AST и отдельный паттерн trailing dot в `ССЫЛКА Документ.`. Если AST отсутствует, это считается parse error.

## Что покрыто

Покрыты неполный `JOIN`, неполный `WHERE`, неполный `FROM`, trailing dot в refs и валидные запросы без diagnostic.

## Пробелы и ограничения

Диапазон ставится на весь BSL literal запроса, а не на конкретную ошибочную часть. Сообщение не содержит expected token.

## Может ли инфраструктура улучшить качество

Да. SDBL parser должен отдавать structured errors и точные ranges внутри query text.

## Возможное объединение

Близко к `ParseError` как parser-level диагностика, но язык другой. Можно унифицировать формат сообщений и expected-token UX.

## Вывод

Необходимая диагностика, но сейчас скорее “запрос сломан”, чем точная подсказка, где и почему.
