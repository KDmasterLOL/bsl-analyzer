# UsingServiceTag

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит служебные теги и заготовочные комментарии в коде.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/using_service_tag.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/UsingServiceTag.md`

## Как реализовано

AST проходит по `COMMENT` токенам. По умолчанию ищет `todo`, `fixme`, `!!`, `mrg`, `@`, `отладка`, `debug`, конструкторские блоки, MRG-блоки и типовые "insert/paste handler content" комментарии. Через параметр `serviceTags` можно заменить паттерн.

## Что покрыто

Покрыты русские и английские теги, регистронезависимость, inline-комментарии, конструкторские блоки запроса и заготовки обработчиков 1С.

## Пробелы и ограничения

Default-паттерны смешивают разные сущности: TODO, merge-маркеры, constructor comments и handler stubs. При custom `serviceTags` логика default полностью заменяется, а не дополняется.

## Может ли инфраструктура улучшить качество

Да. Нужна категоризация тегов и настройки по категориям: TODO/FIXME, generated stubs, merge leftovers, debug markers. Это позволит задавать разные severity и suppression.

## Возможное объединение

Близко к cleanup-диагностикам и к возможной отдельной `GeneratedHandlerStub`/`DebugCode` группе. Можно разделить текущее правило на подкатегории, а не объединять с другими.

## Вывод

Правило практически полезное, но слишком широкое. Лучшее развитие - разнести срабатывания по категориям.
