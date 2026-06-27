# DeprecatedMethods8310

Статус: `historical`, `folded-into-DeprecatedPlatformApi`

Дата разбора: 2026-05-07

Примечание 2026-06-27: историческая карточка. Public diagnostic code
`DeprecatedMethods8310` удален и свернут в активную диагностику
`DeprecatedPlatformApi`.

## Суть правила

Ловит глобальные методы клиентского приложения, устаревшие с 8.3.10, и
предлагает replacements через объект `КлиентскоеПриложение` /
`ClientApplication`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/deprecated_method.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/DeprecatedMethods8310.md`
- `docs/legal/diagnostics/DeprecatedMethods8310.md`

## Как реализовано

Общий handler `deprecated_method::from_hir()` получает имя из
`BodyDiagnostic::DeprecatedMethod`, ищет его в `get_8310_replacement_map()` и
возвращает code `DeprecatedMethods8310`.

## Что покрыто

Тесты в `deprecated_method.rs` проверяют отдельные 8.3.10 методы, английские
варианты и совместную фильтрацию с 8.3.17.

## Пробелы и ограничения

- Replacement map живет в коде, не генерируется из данных.
- Нет quick-fix переписывания `Метод()` в `КлиентскоеПриложение.Метод()`.
- Не проверяется контекст выполнения; client application API может быть
  недоступен в некоторых местах.
- Нет docs/source link на каждый method.

## Инфраструктурные улучшения

Общий deprecated API registry с полями version, lang aliases, replacement,
context, docs link и auto-fix capability.

## Возможное объединение

Внутренне объединить с `DeprecatedMethods8317` и другими platform deprecated
calls. Внешний код оставить для compatibility mode `8.3.10`.

## Вывод

Хороший кандидат на data-driven refactor: сейчас таблица маленькая, но уже
дублирует общий deprecated mechanism.
