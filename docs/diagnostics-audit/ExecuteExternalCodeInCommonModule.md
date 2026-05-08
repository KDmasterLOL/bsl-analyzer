# ExecuteExternalCodeInCommonModule

Статус: `done`, `needs-code-work`
Track 1 closure: D `637a6279`, M `691a751c` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.

Дата разбора: 2026-05-07

## Суть правила

Запрещает `Выполнить`/`Вычислить` в общих модулях, которые доступны на
сервере, во внешнем соединении или обычном приложении. Основание - `#std770`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/execute_external_code_in_common_module.rs`
- `crates/ide-diagnostics/src/common_module_helpers.rs`
- `crates/ide-diagnostics/docs/ru/ExecuteExternalCodeInCommonModule.md`
- `docs/legal/diagnostics/ExecuteExternalCodeInCommonModule.md`
- `<v8std mirror>/docs/std/770.md`

## Как реализовано

Handler загружает configuration через `ctx.load_configuration()`, ищет common
module по URI через `find_common_module_for_file()`, проверяет flags и затем
AST traversal ищет `EXECUTE_STMT` и global eval call.

## Что покрыто

Есть прямые тесты `detect_violations()` на `Выполнить` и `Вычислить`, а также
negative case без configuration.

## Пробелы и ограничения

- Без загруженной configuration правило молчит.
- `find_common_module_for_file()` сравнивает URI и file path буквально
  case-insensitive; возможны проблемы с относительными/нормализованными путями.
- AST detection дублирует HIR detector из `ExecuteExternalCode`.
- `should_check_module()` использует сырые flags напрямую, при этом рядом в
  `common_module_helpers.rs` уже есть `is_server` / `is_client_server` /
  `is_server_call` / `is_client` — handler их не задействует.

## Может ли инфраструктура улучшить качество

Перенести правило на общий HIR detector опасных вызовов и общий execution
context/common-module classifier. Для тестов нужен helper с реальной metadata.

## Возможное объединение

Внутренне объединить с `ExecuteExternalCode`: один source detector, разные
context filters. Внешний код можно сохранить как более metadata-specific alert.

## Вывод

Идея правильная, но реализация живет отдельно от HIR security detector и
сильно зависит от качества metadata path matching.

