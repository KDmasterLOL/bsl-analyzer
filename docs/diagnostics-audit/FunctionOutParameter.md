# FunctionOutParameter

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Функция не должна изменять входной параметр по ссылке как выходной параметр;
для результата нужно использовать возвращаемое значение.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/function_out_parameter.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/ide-diagnostics/docs/ru/FunctionOutParameter.md`
- `docs/legal/diagnostics/FunctionOutParameter.md`

## Как реализовано

HIR lowering отслеживает assignment в простой parameter path внутри function.
`Знач` parameters и procedures исключаются. Handler создает diagnostic на имя
параметра.

## Что покрыто

Тесты проверяют function/procedure distinction, `Знач`, case-insensitive,
property assignment negative и несколько параметров.

## Пробелы и ограничения

- Ловится только simple assignment `Параметр = ...`; мутации через
  `Параметр.Свойство = ...` или методы не считаются.
- Нет анализа alias: `Лок = Параметр; Лок.Свойство = ...`.
- Diagnostic отключен по умолчанию, нужно сверить policy.
- Нет quick-fix, потому что требуется менять signature и call sites.

## Может ли инфраструктура улучшить качество

Нужен parameter mutation/dataflow analyzer и signature refactoring support.

## Возможное объединение

Внутренне близко к assignment diagnostics (`UsingCancelParameter`,
`CommonModuleAssign`, `ThisObjectAssign`), но внешний код отдельный.

## Вывод

Правило покрывает безопасный минимум. Для полноценного контроля нужны mutation
analysis и refactoring.

