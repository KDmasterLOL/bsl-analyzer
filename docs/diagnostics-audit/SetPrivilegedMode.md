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
