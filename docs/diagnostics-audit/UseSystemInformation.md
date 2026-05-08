# UseSystemInformation

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Помечает создание `СистемнаяИнформация` / `SystemInfo` как security hotspot.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/use_system_information.rs`
- `<v8std mirror>/docs/diagnostics/bslls/UseSystemInformation.md`

## Как реализовано

HIR находит `Новый СистемнаяИнформация`, `Новый("СистемнаяИнформация")`, `New SystemInfo`, `New("SystemInfo")`; handler создает simple diagnostic. Диагностика выключена по умолчанию.

## Что покрыто

Покрыты direct и string constructors, русские/английские имена, case-insensitive совпадение. Переменная с именем типа не срабатывает.

## Пробелы и ограничения

Сообщение на английском и без объяснения риска. Нет анализа контекста использования системной информации.

## Может ли инфраструктура улучшить качество

Да. Нужны локализованное сообщение, security context и рекомендации по допустимым сценариям.

## Возможное объединение

Близко к `OSUsersMethod`, `ExternalAppStarting`, `UsingHardcodePath`, `InternetAccess`: security/privacy hotspots.

## Вывод

Правило корректно выключено по умолчанию как hotspot; для включения в строгих профилях нужен лучше объясненный риск.
