# ExcessiveAutoTestCheck

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Проверки устаревшего параметра `АвтоТест` / `AutoTest`, которые сразу делают
`Возврат`, больше не нужны. Локальные источники связывают правило с `#std456`
и отмененной проверкой из `#std772`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/excessive_auto_test_check.rs`
- `crates/ide-diagnostics/docs/ru/ExcessiveAutoTestCheck.md`
- `docs/legal/diagnostics/ExcessiveAutoTestCheck.md`
- `<v8std mirror>/docs/std/456.md`

## Как реализовано

AST pass собирает `IF_STMT` и `RETURN_STMT`. Regex применяется ко всему тексту
`IF_STMT` (не только к условию) и ловит `.Свойство("АвтоТест")`,
`.Property("AutoTest")` либо сравнение со строкой. Ветка должна содержать
только `Return`; есть workaround для parser error nodes.

## Что покрыто

Тесты покрывают основные формы AutoTest check, русские/английские варианты,
условие с return и negative cases.

## Пробелы и ограничения

- Regex по тексту условия может ловить совпадения в неожиданных контекстах.
- Проверяется только branch "условие -> единственный return"; другие устаревшие
  AutoTest patterns не покрыты.
- Нет quick-fix удаления блока `Если`.
- Нет связи с module/event handler context, кроме metadata списка модулей.

## Может ли инфраструктура улучшить качество

Перейти на AST/HIR pattern matcher для property/equality expressions и
block-removal fix builder.

## Возможное объединение

Внутренне близко к dead/obsolete code cleanup diagnostics. Внешне оставить
отдельным, потому что правило исторически специфично.

## Вывод

Правило полезно для миграции, но regex делает анализ менее надежным. Лучше
перевести на структурный matcher.

