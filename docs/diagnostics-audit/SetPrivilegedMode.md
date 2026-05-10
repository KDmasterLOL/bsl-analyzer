# SetPrivilegedMode

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Помечает включение привилегированного режима как security hotspot.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/set_privileged_mode.rs`
- `crates/hir-def/src/body/lower/expr.rs` (логика «Ложь — safe» в `SetPrivilegedModeCall`)
- `crates/hir-def/src/body/lower/diagnostics.rs::is_set_privileged_mode`
- `<v8std mirror>/docs/diagnostics/bslls/SetPrivilegedMode.md`
- `<v8std mirror>/docs/std/678.md`

## Как реализовано

HIR определяет опасный вызов `УстановитьПривилегированныйРежим` / `SetPrivilegedMode`; handler создает simple diagnostic. По тесту `Ложь` не срабатывает, `Истина` и переменная срабатывают.

## Что покрыто

Покрыто прямое включение и включение через выражение/переменную.

## Пробелы и ограничения

Нет анализа пары включение/выключение, области действия и обоснованности использования. Это hotspot, а не доказанная ошибка.

## Может ли инфраструктура улучшить качество

Да. Нужен path-sensitive privilege lifetime analysis и связь с проверками ролей/безопасного режима.

## Возможное объединение

Близко к `IsInRoleMethod`, `PrivilegedModuleMethodCall`, `DisableSafeMode`, `UnsafeSafeModeMethodCall`. Стоит иметь общий security-mode analyzer.

## Вывод

Текущее правило полезно как сигнал, но для качества нужна модель жизненного цикла привилегированного режима.

## Закрыто Track 2

**Phase A §1.6 Group C (commit `f0c617e1`, 2026-05):** hardcoded
`True`/non-literal проверка заменена на lattice-driven дисptch:
`SecurityModeState` saturating-counter (Phase A §1.2,
`crates/dataflow/src/security_state.rs`), `value_state` const-fold
(§1.3), Salsa-обёртка через `AnalysisProvider::module_security_state`
(§1.4b+c, `72b7c3eb`). Detection переехал в `check()` runner-шага;
HIR-side detection полностью удалён (§1.6-C-5). Полная
inter-procedural прозрачность для вызова с произвольной переменной
без known value — Track 6.
