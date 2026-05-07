# UsingSynchronousCalls

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит синхронные клиентские вызовы, для которых есть асинхронные аналоги.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/using_synchronous_calls.rs`
- `crates/hir-def/src/body/lower/diagnostics.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/UsingSynchronousCalls.md`

## Как реализовано

HIR lowering сверяет имя вызова со списком sync-methods и передает replacement. В серверных методах (`&НаСервере`, `&НаСервереБезКонтекста`, `&AtServer`) диагностика не эмитится.

## Что покрыто

Покрыты модальные методы, файловые операции, получение каталогов, разрешение пользователя, запуск приложения, установка/подключение расширений. Есть русские/английские имена и многострочные вызовы.

## Пробелы и ограничения

Есть пересечение с `UsingModalWindows`: 12 модальных методов полностью входят в 28-элементный sync-список (`MODAL_METHODS ⊂ SYNCHRONOUS_METHODS` в `diagnostics.rs:1007/1067`), такие вызовы как `Вопрос`/`Предупреждение` получают по два предупреждения. Список API hardcoded; `compatibility_mode: 8.3.3` гейтит правило целиком, но не фильтрует отдельные API по версии их асинхронного аналога.

## Может ли инфраструктура улучшить качество

Да. Нужен общий registry sync/modal APIs с compatibility metadata, deduplication и генерацией рекомендаций. Для fix потребуется анализ продолжения выполнения и callback/notification patterns.

## Возможное объединение

Стоит объединить с `UsingModalWindows` на уровне источника фактов или пользовательской диагностики. Более широкая `UsingSynchronousCalls` может покрывать modal subset с дополнительным признаком `modal`, чтобы не дублировать сообщения.

## Вывод

Покрытие API широкое, но текущее дублирование с `UsingModalWindows` требует архитектурного решения.
