# NonStandardRegion

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит области верхнего уровня, имя которых не входит в стандартный набор для типа модуля.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/non_standard_region.rs`
- `crates/ide-diagnostics/src/utils/standard_regions.rs`
- `<v8std mirror>/docs/diagnostics/bslls/NonStandardRegion.md`
- `<v8std mirror>/docs/std/455.md`

## Как реализовано

Тип модуля определяется по пути файла через metadata helper. Затем `module_level_regions()` сверяются с `standard_regions::is_standard_region`.

## Что покрыто

Покрыты module-specific стандартные области, case-insensitive сравнение, английские/русские имена и суффиксные form-table regions.

## Пробелы и ограничения

Если тип модуля не определяется из пути, диагностика молчит. Нет fix для переименования/переноса областей. Проверяются только области верхнего уровня.

## Может ли инфраструктура улучшить качество

Да. Надежнее использовать metadata-backed module type, а не только path heuristic, и добавить rename region code action.

## Возможное объединение

Близко к `DuplicateRegion`, `EmptyRegion`, `CodeOutOfRegion`, `NonExportMethodsInApiRegion`. Стоит иметь единый region analyzer.

## Вывод

Правило простое и полезное, но ограничено определением типа модуля и отсутствием автоматического исправления.

## Закрыто Track 2

**Phase C §3 Slice 1 (`effab845`) + Slice 2 (`d32f55d9`), 2026-05:**
hardcoded standard-list заменён на
`hir_def::module_structure::policy::policy_for(module_type).allowed_regions`
+ `RegionTree::module_level_regions` (Slice 2).
