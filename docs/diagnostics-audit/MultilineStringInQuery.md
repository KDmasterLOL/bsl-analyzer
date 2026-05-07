# MultilineStringInQuery

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит подозрительные многострочные строковые литералы внутри SDBL-запросов.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/multiline_string_in_query.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/MultilineStringInQuery.md`

## Как реализовано

`sdbl_hir` отдает `SdblDiagnostic::MultilineString`, но handler дополнительно фильтрует false positives: из-за особенностей SDBL lexer все непустые строки могут выглядеть как multi-string nodes. Поэтому обработчик сканирует исходный `query_text` и реально проверяет, есть ли строковый литерал, пересекающий newline.

## Что покрыто

Покрыты ошибки с `""` внутри текста запроса, корректные escaped strings вроде `""""`, single-line литералы в `CASE` и многострочные query strings в BSL.

## Пробелы и ограничения

Фильтр работает на весь `query_text`, а не на конкретный range SDBL diagnostic. Если в одном запросе есть один настоящий многострочный литерал, теоретически может быть сложнее точно отфильтровать соседние false positives.

## Может ли инфраструктура улучшить качество

Да. Нужно исправить SDBL lexer/HIR ranges для строковых литералов так, чтобы handler не делал глобальный fallback scan.

## Возможное объединение

Близко к `QueryParseError`, `IncorrectUseOfStrTemplate`, `MultilingualString*` только по работе со строками, но смысл отдельный. Инфраструктурно связано с SDBL string parsing.

## Вывод

Правило важное, но наличие workaround в handler показывает инфраструктурный долг в SDBL lexer/range model.
