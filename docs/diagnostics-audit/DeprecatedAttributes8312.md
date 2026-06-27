# DeprecatedAttributes8312

Статус: `historical`, `folded-into-DeprecatedPlatformApi`

Дата разбора: 2026-05-07

Примечание 2026-06-27: историческая карточка. Public diagnostic code
`DeprecatedAttributes8312` удален и свернут в активную диагностику
`DeprecatedPlatformApi`.

## Суть правила

Ловит устаревшие с платформы 8.3.12 chart attributes/methods, global methods,
enum names и enum values. Compatibility mode: `8.3.12`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/deprecated_attributes_8312.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/hir-def/src/body/lower/diagnostics.rs`
- `crates/ide-diagnostics/docs/ru/DeprecatedAttributes8312.md`
- `docs/legal/diagnostics/DeprecatedAttributes8312.md`

## Как реализовано

HIR lowering эмитит `BodyDiagnostic::DeprecatedAttribute8312` для глобальных
методов, field calls/attributes и enum-like constructs. Handler выбирает
сообщение по `DeprecatedKind8312` и replacement из локальной HashMap.

## Что покрыто

Файл handler содержит большой набор тестов на русские/английские имена,
attributes, methods, enum names/values и negative cases.

## Пробелы и ограничения

- Список replacement зашит в коде, не переиспользуется с другими deprecated
  diagnostics.
- Нет единой таблицы "deprecated API item -> platform version -> kind ->
  replacement -> docs link".
- В некоторых случаях replacement пустой fallback все равно попадет в текст
  `Используйте:`.
- Detection зависит от эвристики имени объекта/поля, без полноценного type
  resolution chart objects.

## Инфраструктурные улучшения

Создать общий deprecated API registry и генерировать из него handler maps,
доки и тесты. Для object attributes добавить type-aware resolution, когда
будет достаточно metadata/type info.

## Возможное объединение

Сильный кандидат на внутреннее объединение с `DeprecatedMethods8310`,
`DeprecatedMethods8317`, `DeprecatedCurrentDate`, `DeprecatedFind`,
`DeprecatedMessage`, `DeprecatedTypeManagedForm`. Внешние коды можно оставить
для compatibility mode и точной настройки.

## Вывод

Правило функционально широкое, но список API живет изолированно. Главный долг -
единый registry устаревших API.
