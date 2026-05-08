# MissingCommonModuleMethod

Статус: `done`, `deprecated`
Дата разбора: 2026-05-07

## Суть правила

Исторически правило сообщало об обращении к отсутствующему методу общего модуля.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missing_common_module_method.rs`
- `crates/ide-diagnostics/src/hir_dispatch.rs`
- `<v8std mirror>/docs/diagnostics/bslls/MissingCommonModuleMethod.md`

## Как реализовано

Обработчик является deprecated no-op stub с явным комментарием: диагностика заменена на `UnresolvedMethodCall` (`MethodNotFound` или `ReceiverNotResolved`). `from_hir` всегда возвращает `None`, код и metadata оставлены для совместимости.

## Что покрыто

Новые случаи должны покрываться `UnresolvedMethodCall`. Тесты в файле фиксируют, что legacy-code больше не появляется и что receiver/method resolution уходит в новый канал.

## Пробелы и ограничения

Публичная диагностика существует в списках и конфиге, но фактически не срабатывает. Это может путать пользователей и аудит rule coverage.

## Может ли инфраструктура улучшить качество

Качество нужно улучшать в `UnresolvedMethodCall`, а не здесь. Для совместимости стоит явно пометить rule как deprecated в docs/export metadata, если формат это поддерживает.

## Возможное объединение

Фактически уже объединена в `UnresolvedMethodCall`. Дальше возможны два пути: оставить alias навсегда или удалить код после breaking-change окна.

## Вывод

Это совместимый пустой фасад. В аудите его нужно считать не активной диагностикой, а legacy identifier.
