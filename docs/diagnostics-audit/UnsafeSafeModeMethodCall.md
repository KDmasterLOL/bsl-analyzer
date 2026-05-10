# UnsafeSafeModeMethodCall

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Требует явно сравнивать результат `БезопасныйРежим()` / `SafeMode()` с булевым значением.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/unsafe_safe_mode_method_call.rs`
- `<v8std mirror>/docs/diagnostics/bslls/UnsafeSafeModeMethodCall.md`

## Как реализовано

HIR находит вызовы `SafeMode` в булевых условиях без явного сравнения; handler создает blocker diagnostic. Метаданные привязаны к `CompatibilityMode8_3_1` — правило целится в платформу 8.3.1+, где `БезопасныйРежим()` возвращает строку.

## Что покрыто

Покрыты прямые условия, `Не БезопасныйРежим()`, сложные `И/ИЛИ`, присваивание булевого выражения. Не срабатывает на присваивание результата, аргумент метода и явное сравнение.

## Пробелы и ограничения

Сообщение на английском. Нет fix для добавления `= Истина` или `= Ложь`.

## Может ли инфраструктура улучшить качество

Да. Добавить локализованное сообщение и context-aware fix для явного сравнения.

## Возможное объединение

Близко к `SetPrivilegedMode`, `DisableSafeMode`, `IsInRoleMethod`. Общий security-mode analyzer полезен.

## Вывод

Паттерн покрыт хорошо, но UX просит локализацию и quick fix.

## Закрыто Track 2

**Phase A §1.6 Group B (commit `9588c13e`, 2026-05):** registry-driven
distinction unsafe-mode method names (`bsl_platform::security::registry`,
`Category::SafeModeMethodCall`) — hardcoded equality pattern сохранён
структурно, но имя метода теперь приходит из реестра.
