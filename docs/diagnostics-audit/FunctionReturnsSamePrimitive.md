# FunctionReturnsSamePrimitive

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Если все ветки функции возвращают одно и то же примитивное значение, функция
скорее всего лишняя или содержит ошибочную логику.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/function_returns_same_primitive.rs`
- `crates/hir-def/src/body/lower/mod.rs`
- `crates/ide-diagnostics/docs/ru/FunctionReturnsSamePrimitive.md`
- `docs/legal/diagnostics/FunctionReturnsSamePrimitive.md`

## Как реализовано

HIR lowering собирает return primitive literals по функции и эмитит diagnostic,
если несколько return branches сходятся к одному primitive. Есть исключение для
attachable methods с prefix `Подключаемый_` / `Attachable_` (case-insensitive).

## Что покрыто

Тесты покрывают одинаковые bool/string/number/null, case-insensitive строки,
single return negative, переменную вместо literal и разные primitives.

## Пробелы и ограничения

- Проверяются только primitive literals; `Возврат Константа` и вычислимые
  константы не анализируются.
- Исключение attachable prefix зашито, не конфигурируемо.
- Не учитывается unreachable code и all-paths semantics так глубоко, как у
  return-flow diagnostics.
- Нет quick-fix replace with constant.

## Может ли инфраструктура улучшить качество

Общий return-flow/value analysis с constant folding и unreachable-code model.

## Возможное объединение

Внутренне с `FunctionShouldHaveReturn` и
`AllFunctionPathMustHaveReturn` через return-flow analyzer. Внешние коды
оставить: проблемы разные.

## Вывод

Полезная smell-диагностика, но пока ограничена literal-only анализом.

