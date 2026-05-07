# IncorrectUseOfStrTemplate

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

`СтрШаблон` / `StrTemplate` должен иметь корректные placeholders `%1..%10`
(также поддерживается форма `%(N)` для случаев типа `%(1)07`) и достаточное
число параметров; `%%` экранируется.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/incorrect_use_of_str_template.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/IncorrectUseOfStrTemplate.md`
- `docs/legal/diagnostics/IncorrectUseOfStrTemplate.md`

## Как реализовано

Две фазы: literal templates проверяются в HIR lowering, а post-HIR check
использует reaching definitions, чтобы разрешать template variable до строкового
literal. Есть quick pre-check по тексту файла и depth limit `10`.

## Что покрыто

Тесты покрывают invalid `%0`, `%11+`, недостаток параметров, escaped `%%`,
literal и variable/transitive assignment scenarios.

## Пробелы и ограничения

- Reaching definitions не всегда сходятся; тогда candidate пропускается.
- Если разные definitions дают разные template values, правило молчит.
- Нет quick-fix добавления аргументов/исправления placeholder.
- Message для post-HIR cases на английском.

## Может ли инфраструктура улучшить качество

Улучшить constant/string propagation и унифицировать literal/post-HIR
validation в одном StrTemplate analyzer.

## Возможное объединение

Внутренне с API-usage diagnostics и string-literal analyzers. Внешне оставить:
это конкретная API correctness ошибка.

## Вывод

Диагностика уже использует dataflow и покрывает больше literal-only сценария.
Главный долг - единый analyzer и русскоязычные/точные сообщения.

