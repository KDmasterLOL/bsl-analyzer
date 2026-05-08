# ProtectedModule

Статус: `done`, `needs-code-work`
Track 1 closure: D `637a6279`, M `691a751c` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Сообщает о защищенных паролем общих модулях, чей исходный код недоступен.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/protected_module.rs`
- `<v8std mirror>/docs/diagnostics/bslls/ProtectedModule.md`

## Как реализовано

Запускается только в `SessionModule` (ограничение прописано и в metadata `modules: &[SessionModule]`, и проверкой пути в handler), загружает конфигурацию и обходит common modules. Для `is_protected()` создает diagnostic с именем модуля. Метаданные помечены `can_locate_on_project: true`.

## Что покрыто

Покрыты отключение правила, отсутствие metadata, проверка только session module и несколько защищенных common modules.

## Пробелы и ограничения

Диапазон синтетический в начале session module. Правило не показывает точное место в metadata и не может предложить fix.

## Может ли инфраструктура улучшить качество

Да. Нужна project-level metadata diagnostic с range на common module XML.

## Возможное объединение

Близко к `OrdinaryAppSupport`, `ScheduledJobHandler`, `MissingEventSubscriptionHandler`, `PrivilegedModuleMethodCall`: metadata/security/project checks.

## Вывод

Проверка важная, но место диагностики сейчас техническое, а не пользовательски точное.
