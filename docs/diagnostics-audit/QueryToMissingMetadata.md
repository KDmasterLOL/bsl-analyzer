# QueryToMissingMetadata

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит обращения в запросе к несуществующим объектам метаданных.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/query_to_missing_metadata.rs`
- `<v8std mirror>/docs/diagnostics/bslls/QueryToMissingMetadata.md`

## Как реализовано

SDBL diagnostic `QueryToMissingMetadata` содержит имя таблицы и range; handler мапит range и формирует blocker diagnostic.

## Что покрыто

Покрытие зависит от наличия metadata context. В тестах явно зафиксировано, что без metadata diagnostic не появляется.

## Пробелы и ограничения

Без загруженной конфигурации правило молчит. Нет fuzzy suggestion для похожего объекта метаданных.

## Может ли инфраструктура улучшить качество

Да. Нужна надежная metadata загрузка в workspace и fuzzy lookup по именам объектов для подсказок.

## Возможное объединение

Близко к `UnresolvedField`, `UnresolvedMethodCall`, `WrongDataPathForFormElements`: все проверяют ссылки на модель приложения. Общая name-resolution инфраструктура нужна.

## Вывод

Сильная semantic query diagnostic, но полностью зависит от metadata availability.

## Закрыто Track 2

**Phase C §4 audit pass (task #84, 2026-05):** Track A — без изменений.
