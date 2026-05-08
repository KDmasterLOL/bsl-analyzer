# UnsafeFindByCode

Статус: `done`, `needs-code-work`
Track 1 closure: D `637a6279`, M `691a751c` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Находит небезопасный `НайтиПоКоду()` / `FindByCode()` для объектов, где код не гарантированно уникален.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/unsafe_find_by_code.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/UnsafeFindByCode.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/456.md`

## Как реализовано

HIR передает manager name, object name и range. Handler загружает configuration, поддерживает `Справочники`, `ПланыВидовХарактеристик`, `ПланыСчетов`, проверяет metadata `is_find_by_code_safe()`.

## Что покрыто

Покрыты настройки `check_unique=false` и code series не whole. Без configuration diagnostic не появляется.

## Пробелы и ограничения

Поддержан ограниченный набор manager types. Нет suggestion, каким способом искать вместо кода.

## Может ли инфраструктура улучшить качество

Да. Расширить coverage на другие metadata types и предлагать альтернативные уникальные реквизиты/ссылки.

## Возможное объединение

Близко к `UsingFindElementByString`, `QueryToMissingMetadata`, metadata-aware semantic diagnostics.

## Вывод

Правило хорошо использует metadata, но покрытие ограничено типами объектов и отсутствием альтернатив.
