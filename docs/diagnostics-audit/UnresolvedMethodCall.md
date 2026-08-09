# UnresolvedMethodCall

Статус: `done`, `needs-code-work`
Track 1 closure: G1 `27fb95ec`, G2 `1e5230fd`, G4 `ece5d404` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Сообщает о квалифицированных вызовах методов, которые semantic resolver не смог разрешить.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/unresolved_method_call.rs`
- `crates/ide-diagnostics/src/hir_inference_dispatch.rs`
- `crates/ide-diagnostics/src/handlers/missing_common_module_method.rs`

## Как реализовано

Handler принимает receiver, method, `UnresolvedMethodKind` и range. Сообщения различают `MethodNotFound`, `MethodNotExport`, `ReceiverNotResolved`. Пятый вид, `BodyUnread`, сообщения не имеет вовсе: тело вызываемого модуля существует, но прочитать его не удалось, и любое утверждение о вызове было бы догадкой, выписанной вызывающему файлу вместо нечитаемого. Диагностика в этом случае не выдаётся; сам факт сообщается по своему адресу — `diagnostics file` отвечает `unreadable`, а рабочая область считает такие файлы в `files_unread`. Это фактическая замена deprecated `MissingCommonModuleMethod`.

## Что покрыто

Покрыты отсутствующий метод, неэкспортный метод, нерешенный receiver и отсутствие исходника общего модуля. Локальные переменные/параметры, затеняющие receiver, подавляют diagnostic.

## Пробелы и ограничения

Качество зависит от module index, symbol tree и extension semantics. Нет quick fix/import/suggestion.

## Может ли инфраструктура улучшить качество

Да. Нужны fuzzy suggestions, better extension-aware resolution и suppression cascade после parse/type errors.

## Возможное объединение

Близко к `UnresolvedField`, `MismatchedArgCount`, `MissedRequiredParameter`. `MissingCommonModuleMethod` уже deprecated (v0.1.176) и заменён этим правилом — не кандидат на объединение, а историческая корка. Общий resolver diagnostics UX нужен.

## Вывод

Ключевая typed diagnostic, уже стала заменой старого common-module правила. Следующий шаг - suggestions и снижение каскадов.
