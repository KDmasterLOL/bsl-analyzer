# DeprecatedMessage

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Глобальный `Сообщить()` / `Message()` нежелателен; стандарт `#std418`
рекомендует более явные пользовательские сообщения или журналирование.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/deprecated_message.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/DeprecatedMessage.md`
- `docs/legal/diagnostics/DeprecatedMessage.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/418.md`

## Как реализовано

HIR lowering диагностирует global call `Сообщить`/`Message`, исключая
qualified method calls. Handler подставляет фиксированный replacement text.

## Что покрыто

Тесты покрывают русский/английский вызов, object method exclusion,
case-insensitive variants, вызов внутри `Если` и module-level call.

## Пробелы и ограничения

- Replacement зависит от сценария: пользователю, журнал регистрации, отказ,
  исключение. Сейчас совет один.
- Нет quick-fix.
- Нет связи с правилами логирования/пользовательских сообщений.

## Инфраструктурные улучшения

Нужен context-aware replacement: UI/client context, server context, exception
path, logging path. Минимум - несколько suggested alternatives в docs/message.

## Возможное объединение

Внутренне с deprecated global method registry. Внешне оставить отдельно из-за
стандарта `#std418` и специфичных рекомендаций.

## Вывод

Detection хорошая, но remediation слишком упрощенная.

