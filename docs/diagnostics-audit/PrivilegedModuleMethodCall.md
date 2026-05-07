# PrivilegedModuleMethodCall

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит вызовы экспортных методов привилегированных общих модулей.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/privileged_module_method_call.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/PrivilegedModuleMethodCall.md`

## Как реализовано

Загружает конфигурацию, собирает common modules с флагом privileged, затем смотрит `call_summary` текущего модуля. Срабатывают `DirectQualifiedModule` edges, если `resolve_qualified_path` подтверждает метод. Конфиг `validateNestedCalls`.

## Что покрыто

Покрыты прямые квалифицированные вызовы привилегированных модулей и опциональное исключение вызовов из самого привилегированного модуля.

## Пробелы и ограничения

Нет межпроцедурного security reasoning: diagnostic говорит “проверьте”, но не понимает, есть ли защитная обвязка. Динамические вызовы и aliases не покрываются.

## Может ли инфраструктура улучшить качество

Да. Нужны call graph effects, trust boundaries и модели safe wrappers.

## Возможное объединение

Близко к `IsInRoleMethod`, `SetPrivilegedMode`, `DisableSafeMode`, `UnsafeSafeModeMethodCall`, `ProtectedModule`, `OSUsersMethod`. Стоит объединить security hotspot helpers.

## Вывод

Правило хорошо использует call summary и metadata, но остается hotspot без глубокого анализа безопасности.
