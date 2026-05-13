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

## Закрыто Track 2

**Phase C §4 audit pass (task #84, 2026-05):** Track A — без изменений.

## Закрыто Track 6.1

Audit-card requirement «structured errors and precise ranges inside query text» закрыт:
- SDBL grammar sites emit structured `ParseError` payloads (C.1: `a019462a`+`7d4ba69a`)
- SDBL→BSL inverse projection helper `map_range_query_to_literal` (C.2: `ee69cddf`+`a5223483`)
- `SdblQueryInfo::error_ranges_in_bsl` populated in hir-def lowering with structured ParseError + trailing-dot synthetic detection (C.3: `65202959`)
- Handler (`crates/ide-diagnostics/src/handlers/query_parse_error.rs`) consumes `error_ranges_in_bsl` and renders precise BSL-coordinate diagnostics (D.2: `a040b9a1`)
- 9 snapshot tests rebased — every new range is a sub-range of the old whole-literal range
