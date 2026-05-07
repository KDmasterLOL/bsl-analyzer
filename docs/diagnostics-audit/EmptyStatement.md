# EmptyStatement

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Одиночная лишняя `;` создает пустой оператор. Обычно это опечатка или след
рефакторинга.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/empty_statement.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/ide-diagnostics/docs/ru/EmptyStatement.md`
- `docs/legal/diagnostics/EmptyStatement.md`

## Как реализовано

HIR lowering эмитит `BodyDiagnostic::EmptyStatement` для `EMPTY_STMT`, если
рядом нет parser error. Handler создает diagnostic и fix, удаляющий range
пустого оператора.

## Что покрыто

Тесты проверяют `;` после `Тогда`, двойной `;;`, отсутствие лишних операторов и
suppression при parse error.

## Пробелы и ограничения

- Fix удаляет только token range; если вокруг останется лишний пробел, формат
  может быть неидеальным.
- Suppression при parse errors зависит от lowerer's эвристики.
- Нет unified formatter integration.

## Может ли инфраструктура улучшить качество

Подключить whitespace-aware text edits и общий parser-error suppression layer
для синтаксических diagnostics.

## Возможное объединение

Внутренне с `ExtraCommas` и formatting diagnostics через safe text-edit
helpers. Внешне объединять не нужно.

## Вывод

Правило маленькое и уже имеет fix. Улучшать стоит качество edit'а и общую
политику при parse errors.

