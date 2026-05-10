# PrivilegedModuleMethodCall

Статус: `done`, `needs-code-work`
Track 1 closure: D `637a6279`, M `691a751c` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Находит вызовы экспортных методов привилегированных общих модулей.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/privileged_module_method_call.rs`
- `<v8std mirror>/docs/diagnostics/bslls/PrivilegedModuleMethodCall.md`

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

## Закрыто Track 2

**Phase A §1.4 (commit `72b7c3eb`) + §1.5 (`3a847187`) + §1.6 Group D
(`0862379b`), 2026-05:** detection переписан на `effect_summary`
транзитивную эффект-пропагацию (`crates/dataflow/src/effect_summary.rs`,
Salsa-cycle-aware через `module_effect_summaries_query` /
`method_effect_summary_query`); guard-predicate detector
(`crates/dataflow/src/guard_predicates.rs`) подавляет diagnostic, если
вызов окружён `РольДоступна(...)`/role-check guard. Cross-module
recursion обрабатывается Salsa cycle handlers
(`module_effect_summaries_initial`/`_cycle`).

Known-limitation: cross-module SCC walk for `is_recursive` propagation
вынесен как pending follow-up (task #50, не блокирует Phase A acceptance).
