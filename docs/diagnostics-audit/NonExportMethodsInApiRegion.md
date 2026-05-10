# NonExportMethodsInApiRegion

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит неэкспортные методы внутри API-областей `ПрограммныйИнтерфейс` / `Public` / `СлужебныйПрограммныйИнтерфейс` / `Internal`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/non_export_methods_in_api_region.rs`
- `<v8std mirror>/docs/diagnostics/bslls/NonExportMethodsInApiRegion.md`
- `<v8std mirror>/docs/std/455.md`

## Как реализовано

Через `item_tree` берутся процедуры/функции, через `region_tree.root_api_region_for_range` определяется API-регион. Экспортные методы пропускаются. Конфиг `skipAnnotatedMethods` позволяет пропускать методы с аннотациями.

## Что покрыто

Покрыты русские и английские API-регионы, вложенные области внутри API-региона, функции и процедуры.

## Пробелы и ограничения

Правило не предлагает перенос или добавление `Экспорт`. `skipAnnotatedMethods` грубый: любая распознанная аннотация исключает метод, без проверки ее смысла.

## Может ли инфраструктура улучшить качество

Да. Нужны region-aware code actions: переместить метод в private region или добавить `Экспорт`, если это реально API.

## Возможное объединение

Близко к `PublicMethodsDescription`, `CommonModuleMissingAPI`, `CodeOutOfRegion`, `NonStandardRegion`. Лучше объединить region/module-structure infrastructure, оставив отдельные коды.

## Вывод

Правило хорошо закрывает прямой structural smell, но пока не помогает привести модуль к стандартной структуре автоматически.

## Закрыто Track 2

**Phase C §3 Slice 1 (commit `effab845`, 2026-05):** hardcoded API
region-names заменены на
`hir_def::module_structure::policy::policy_for(module_type).api_region_names`.
