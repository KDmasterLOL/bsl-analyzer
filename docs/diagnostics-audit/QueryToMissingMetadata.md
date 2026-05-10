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

## Закрыто Track 3

**Phase C sub-slice C3 (commit `<pending>`, 2026-05):** добавлены
Configuration.xml-backed snapshot-fixtures через `check_snapshot_with_config_xml`:

- `track3_existing_common_module_reference_with_config_xml_snapshot` —
  regression guard текущего поведения: ссылка на объявленный `ОбщийМодуль` в
  SDBL пока диагностируется как отсутствующая metadata.
- `track3_missing_common_module_reference_with_config_xml_snapshot` — ссылка на
  несуществующий `ОбщийМодуль` диагностируется как `QueryToMissingMetadata`.
- `track3_bilingual_common_module_references_with_config_xml_snapshot` —
  русская `ОбщийМодуль.*` и английская `CommonModule.*` формы фиксируются в
  одном SDBL query с `ОБЪЕДИНИТЬ ВСЕ`.

Поддержка разрешения `CommonModule` через `Configuration.common_modules()`, а не
только через `metadata_objects`, оставлена в Track 4/6; Track 3 ограничен
fixtures и audit markdown.
