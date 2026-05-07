# ParseError

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Сообщает об ошибках разбора BSL-кода.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/parse_error.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/ParseError.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/439.md`

## Как реализовано

AST-обход по `SyntaxKind::ERROR`; непустые error nodes превращаются в critical diagnostics.

## Что покрыто

Покрыты базовые parse errors: незакрытые строки, некорректное условие, bare identifier после метода, EOF-ошибки.

## Пробелы и ограничения

Сообщение общее, без expected tokens и объяснения причины. Диапазон зависит от error recovery парсера.

## Может ли инфраструктура улучшить качество

Да. Parser должен отдавать structured parse errors: expected token, actual token, recovery kind.

## Возможное объединение

Близко к `QueryParseError`, но это разные языки: BSL и SDBL. Можно унифицировать reporting format, не объединяя rule ids.

## Вывод

Диагностика базовая и необходимая, но для хорошего UX нужен richer parser error model.
