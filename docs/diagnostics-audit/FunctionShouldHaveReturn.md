# FunctionShouldHaveReturn

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Функция должна содержать хотя бы один `Возврат` / `Return`. Это более простая
проверка, чем "все пути возвращают значение".

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/function_should_have_return.rs`
- `crates/hir-def/src/body/lower/mod.rs`
- `crates/ide-diagnostics/docs/ru/FunctionShouldHaveReturn.md`
- `docs/legal/diagnostics/FunctionShouldHaveReturn.md`

## Как реализовано

HIR lowering через `cf_analysis.has_return` эмитит `FunctionShouldHaveReturn`,
если в функции вообще нет `Возврат`; иначе эмитит дуальный кандидат
`MissingReturn` для `AllFunctionPathMustHaveReturn`. Handler ставит diagnostic
на имя функции.

## Что покрыто

Тесты проверяют function without return, with return, procedure negative,
conditional returns, несколько функций, английский синтаксис и fixture-сценарий
с function/procedure ± `Возврат` в одном модуле.

## Пробелы и ограничения

- Наличие одного return достаточно; функция с return только в одной ветке
  проходит и должна ловиться `AllFunctionPathMustHaveReturn`.
- Нет объединенного UX с `AllFunctionPathMustHaveReturn`: пользователь может
  видеть разные уровни одной return-flow проблемы.
- Не учитываются throw/raise как завершение.

## Может ли инфраструктура улучшить качество

Общий return-flow analyzer должен отдавать facts: no return, partial return,
same primitive, unreachable paths.

## Возможное объединение

Внутренне объединить с return-flow diagnostics. Внешне отдельный код полезен
как более простая и точная причина.

## Вывод

Проверка корректна для грубого случая, но должна жить в одном return-flow
слое с более строгими правилами.

