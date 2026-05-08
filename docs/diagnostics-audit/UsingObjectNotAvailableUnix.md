# UsingObjectNotAvailableUnix

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит создание объектов, недоступных в Unix/Linux окружении: `COMОбъект` / `COMObject`, `Почта` / `Mail`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/using_object_not_available_unix.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `<v8std mirror>/docs/diagnostics/bslls/UsingObjectNotAvailableUnix.md`

## Как реализовано

HIR lowering проверяет `Новый Тип` и `Новый("Тип")`. Если тип входит в hardcoded список и lowering сейчас не находится внутри platform guard, эмитится diagnostic с именем типа.

## Что покрыто

Покрыты direct и string constructors, `COMОбъект`/`COMObject`, `Почта`/`Mail`, а также простые guard-условия по платформам Windows/Linux/MacOS.

## Пробелы и ограничения

Platform guard определяется по тексту условия с платформенными ключевыми словами, без точной логики выражения. Нет расширяемого списка Windows-only объектов и нет связи с целевыми платформами проекта.

## Может ли инфраструктура улучшить качество

Да. Нужны нормализованный анализ условий, project target platforms и registry объектов платформы с доступностью по ОС.

## Возможное объединение

Близко к `UseSystemInformation` и security/portability diagnostics. Объединять можно общий слой platform-awareness, но пользовательский код диагностики лучше оставить отдельным.

## Вывод

Правило полезное, но качество сильно упирается в точность platform-guard анализа и полноту списка объектов.
