# ParseError

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Сообщает об ошибках разбора BSL-кода.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/parse_error.rs`
- `<v8std mirror>/docs/diagnostics/bslls/ParseError.md`
- `<v8std mirror>/docs/std/439.md`

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

## Закрыто Track 6.1

Audit-card requirement «structured parse errors: expected token, actual token, recovery kind» закрыт:
- `ParseError` enum (`crates/parser-error/src/lib.rs`): variants `Expected { expected, found, recovery }`, `Unexpected { found, recovery }`, `Custom { message, recovery }`
- `RecoveryKind` taxonomy: `BumpToken` / `MissingToken` / `RecoverySpan` / `Custom`
- Handler (`crates/ide-diagnostics/src/handlers/parse_error.rs`) consumes `parse.errors()` and renders via `ParseError::format_ru()`
- Track 6.1 commits: A.1 `8ddf8ccb`, A.2 `703300c4`, A.3 `565918fb`, B.1 `f762aec7`, B.2 `d3f1543c`+`f1fc00ff`, B.3 `512adc79`+`21199d55`, D.1 `78c208e8`
