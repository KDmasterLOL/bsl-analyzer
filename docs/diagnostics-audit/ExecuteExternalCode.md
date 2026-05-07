# ExecuteExternalCode

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

`Выполнить` / `Execute` и `Вычислить` / `Eval` на сервере опасны как
произвольное выполнение кода. Основание - `#std770`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/execute_external_code.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/ExecuteExternalCode.md`
- `docs/legal/diagnostics/ExecuteExternalCode.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/770.md`

## Как реализовано

HIR lowering эмитит diagnostic для `EXECUTE_STMT` и global calls
`Eval`/`Вычислить`, если метод не является строго client-only. Handler только
преобразует HIR diagnostic.

## Что покрыто

Тесты покрывают серверные аннотации, безконтекстные серверные методы,
клиент-сервер без контекста, методы без директивы и exemption для `&НаКлиенте`.

## Пробелы и ограничения

- Внутри common module есть отдельное правило `ExecuteExternalCodeInCommonModule`,
  возможны дубли или расхождение по условиям срабатывания.
- Контекст определяется по annotations, а не по полной metadata execution
  model для всех module types.
- Нет анализа источника строки: literal safe/unsafe не различается, что
  оправданно для security, но message не объясняет hotspot nature.

## Может ли инфраструктура улучшить качество

Нужен единый execution-context service и security API registry. Для UX полезно
разделить "critical definite server execution" и "review needed".

## Возможное объединение

С `ExecuteExternalCodeInCommonModule` лучше объединить внутренне: один detector
опасного API, разные context policies. Внешние коды можно оставить для
совместимости и настроек.

## Вывод

Основной риск ловится. Главный долг - синхронизация с common-module variant и
единая модель контекста выполнения.

