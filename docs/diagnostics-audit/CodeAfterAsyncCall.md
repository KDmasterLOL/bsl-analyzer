# CodeAfterAsyncCall

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Диагностика ищет код, который будет выполнен после вызова асинхронного метода,
не дожидаясь завершения асинхронной операции. Для старого callback-подхода это
типичная ошибка: разработчик пишет следующую строку так, будто результат уже
получен, хотя управление вернулось сразу.

Корректные варианты: перенести зависимый код в обработчик
`ОписаниеОповещения` / `NotifyDescription` или использовать `Ждать` / `Await`
для async-методов, которые возвращают обещание.

## Проверенные источники

- Handler:
  `crates/ide-diagnostics/src/handlers/code_after_async_call.rs`.
- Эмиссия HIR diagnostic:
  `crates/hir-def/src/body/lower/diagnostics.rs`,
  `crates/hir-def/src/body/lower/control_flow.rs`,
  `crates/hir-def/src/body/lower/mod.rs`,
  `crates/hir-def/src/body.rs`.
- Dispatch:
  `crates/ide-diagnostics/src/hir_dispatch.rs`.
- Platform lookup:
  `crates/hir-def/src/body/lower/platform_helpers.rs`.
- Смежные списки async/sync/modal методов:
  `crates/ide-diagnostics/src/handlers/using_synchronous_calls.rs`,
  `crates/hir-def/src/body/lower/diagnostics.rs`.
- Rule-доки:
  `crates/ide-diagnostics/docs/ru/CodeAfterAsyncCall.md`,
  `crates/ide-diagnostics/docs/en/CodeAfterAsyncCall.md`.
- Provenance:
  `docs/legal/diagnostics/CodeAfterAsyncCall.md`.
- Локальный `v8std`:
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/CodeAfterAsyncCall.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/v8-code-style/code-after-async-call.md`,
  `/home/itrous/src/tools_migration/lsp/v8std/docs/lang/index.md`.
- Внешние ссылки из rule-доков:
  `https://its.1c.ru/db/v8319doc#bookmark:dev:TI000001505`,
  `https://1c-syntax.github.io/bsl-language-server/diagnostics/CodeAfterAsyncCall/`,
  `https://github.com/1C-Company/v8-code-style/blob/master/bundles/com.e1c.v8codestyle.bsl/markdown/ru/code-after-async-call.md`.

## Как реализовано

Во время lowering метода `control_flow::analyze_control_flow()` одним обходом
собирает все `CALL_STMT`. Затем `check_code_after_async_call()`:

- оставляет только глобальные вызовы из списка `ASYNC_ENGLISH_NAMES`;
- русские имена распознаются через `bsl-platform` lookup по английскому имени;
- квалифицированные вызовы вроде `Форма.ПоказатьВводЧисла()` игнорируются;
- проверяет, есть ли executable code после async-call в том же блоке или после
  родительского `Если`, цикла или `Попытка`;
- `Возврат` после async-call считается безопасным выходом;
- `Прервать` после async-call считается безопасным только локально, но родительские
  блоки потом проверяются дальше.

Handler `from_hir()` только применяет настройки диагностики и строит сообщение.
Quick-fix нет. Диагностика выключена по умолчанию.

## Что покрыто

- async-call без последующего кода, только с комментарием;
- async-call и сразу исполняемая строка после него;
- `Возврат` после async-call как безопасный выход;
- async-call внутри `Если` и код после `КонецЕсли`;
- async-call внутри той же ветки `Если` и код ниже в этой ветке;
- вложенные `Если`;
- циклы `Пока`, `Для каждого`, `Для ... По` с кодом после цикла;
- `Попытка/Исключение` с кодом после `КонецПопытки`;
- английские имена async-методов;
- квалифицированный вызов игнорируется;
- `Прервать` после async-call внутри цикла;
- два async-call во взаимоисключающих ветках без общего кода после них.

Покрытие хорошее для локальных AST-паттернов вокруг callback-style глобальных
async-методов.

## Пробелы покрытия

- Диагностика основана на AST-соседях и подъеме по родителям, а не на CFG.
  Поэтому она не доказывает достижимость и не различает все реальные пути
  выполнения.
- Безопасными терминаторами считаются только `Возврат` и частично `Прервать`.
  `Продолжить`, `ВызватьИсключение`, `Перейти` и блок `Если`, у которого все
  ветки завершаются, сейчас могут выглядеть как "код после async" и давать
  ложные срабатывания.
- Квалифицированные вызовы полностью игнорируются. Это правильно для текущего
  контракта "глобальные методы", но если async API доступен через объектную
  форму или обертку, диагностика его не увидит.
- Список async-методов жестко задан через `ASYNC_ENGLISH_NAMES`. Он пересекается
  со списками `SYNCHRONOUS_METHODS` и `MODAL_METHODS`, но не выводится из них и
  не проверяется на согласованность.
- Диагностика не пытается понять, зависит ли следующий код от результата async
  операции. Любой executable statement после async-call считается suspicious.
  Это консервативно, но может шуметь, если код действительно независим.
- Проверяется только `CALL_STMT`. Async-вызовы в выражениях, присваиваниях или
  `Ждать`-цепочках не являются целью этой диагностики. Это нормально для
  callback-style методов, но границу стоит явно держать в документации.
- Логика `try/except` хрупкая: есть ручное отслеживание `Исключение` /
  `КонецПопытки` через descendants, без структурной модели try-body и
  except-body.
- Нет тестов на `Продолжить`, `ВызватьИсключение`, `Перейти`, `Если/Иначе` с
  терминаторами после async, препроцессорные ветки и region-блоки.
- В тестах для циклов есть места с `assert!(!diagnostics.is_empty())`, но не
  фиксируется точное количество и диапазоны diagnostics. Это оставляет место для
  незаметного изменения поведения.

## Может ли инфраструктура улучшить качество

Да. Для более точной проверки у проекта уже есть нужные компоненты:

- HIR body и module CFG могут моделировать пути выполнения лучше, чем AST
  siblings;
- `stmt_list_terminates()` уже знает больше терминаторов, чем текущая проверка
  `CodeAfterAsyncCall`;
- `bsl-platform` lookup уже решает bilingual matching глобальных функций;
- рядом есть lists/mappings для sync/modal/async методов, которые можно
  привести к одному источнику правды.

Самый полезный шаг — перевести "есть живой код после async-call" в CFG-свойство:
от async-call существует достижимый путь к следующему executable statement без
немедленного завершения метода/итерации/исключения.

## Возможное объединение

Ближайший кластер: `UsingSynchronousCalls`, `UsingModalWindows` и
`ExternalAppStarting` для async-вариантов запуска приложения. На уровне
реализации рядом находятся replacement tables `SYNCHRONOUS_METHODS` и
`MODAL_METHODS`, а также список `ASYNC_ENGLISH_NAMES`.

Объединять `DiagnosticCode` не стоит:

- `UsingSynchronousCalls` говорит "замените sync API на async API";
- `UsingModalWindows` говорит про запрет модальности;
- `CodeAfterAsyncCall` говорит "после выбранного async API нельзя писать код,
  зависящий от результата".

Но внутренне эти правила должны опираться на общий каталог UI/file/extension
sync-async пар: sync-name, async callback-name, promise/await-name, replacement,
контекст доступности и допустимость qualified calls. Тогда исчезнет дублирование
списков и будет проще поддерживать новые платформенные методы.

## Варианты снятия ограничений

1. Вынести async/sync/modal method catalog в один модуль и использовать его в
   `CodeAfterAsyncCall`, `UsingSynchronousCalls`, `UsingModalWindows` и
   документации.
2. Переписать проверку "код после async" на CFG или хотя бы переиспользовать
   общий `stmt_list_terminates()` для терминаторов.
3. Добавить тесты на `Продолжить`, `ВызватьИсключение`, `Перейти`, all-branches
   terminating `Если`, препроцессорные ветки и region-блоки.
4. Уточнить контракт по qualified calls: оставить явное ограничение на
   глобальный контекст или добавить поддержку известных object/form async API.
5. Добавить точные assert'ы по количеству и диапазонам diagnostics для циклов,
   где сейчас проверяется только непустой результат.
6. В rule-доках явно написать, что диагностика не анализирует зависимость
   следующего кода от результата async-call и поэтому работает как
   консервативное suspicious-правило.
7. Рассмотреть future-правило для misuse `Ждать` / `Await` отдельно, не
   смешивая его с callback-style `CodeAfterAsyncCall`.

## Вывод

Диагностика полезная и хорошо покрывает основные ошибки после callback-style
асинхронных глобальных вызовов. Главные ограничения — AST-эвристика вместо CFG и
дублирование каталога async/sync методов. Следующий практичный шаг — общий
каталог платформенных sync/async пар и тесты на терминаторы, после чего можно
перенести проверку на CFG без изменения внешнего `DiagnosticCode`.
