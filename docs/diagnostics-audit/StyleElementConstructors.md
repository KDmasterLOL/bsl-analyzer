# StyleElementConstructors

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Запрещает прямые конструкторы элементов стиля (`Цвет`, `Рамка`, `Шрифт` / `Color`, `Border`, `Font`), предлагая получать элемент стиля.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/style_element_constructors.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/StyleElementConstructors.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/667.md`

## Как реализовано

HIR lowering находит direct и string constructors, handler получает type name и range.

## Что покрыто

Покрыты русские/английские типы, `Новый Цвет(...)`, `Новый("Цвет", ...)`, вложенные конструкторы.

## Пробелы и ограничения

Нет fix, потому что нужно знать конкретный элемент стиля, на который заменить конструктор.

## Может ли инфраструктура улучшить качество

Частично. Если metadata/style catalog доступен, можно предлагать known style constants или шаблон замены.

## Возможное объединение

Близко к `MagicNumber` и constructor style rules (`NestedConstructorsInStructureDeclaration`). Общий constructor-policy helper полезен.

## Вывод

Detection достаточный, но actionable рекомендация требует знаний о стиле конфигурации.
