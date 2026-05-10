# CommitTransactionOutsideTryCatch

Статус: `done`, `needs-code-work`
Track 1 closure: scope-included, no code change (kept syntactic in `hir-def/body/lower` per plan §1.8) — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.

Дата разбора: 2026-05-07

## Суть правила

Диагностика проверяет позицию `ЗафиксироватьТранзакцию()` /
`CommitTransaction()` в стандартном паттерне транзакций. По `#std783` фиксация
должна быть последним исполняемым оператором в ветке `Попытка`,
непосредственно перед `Исключение`. Если после фиксации выполнится код и он
упадет, обработчик `Исключение` может попытаться откатить уже зафиксированную
транзакцию.

Правило не доказывает полную корректность lifecycle транзакции: наличие
`НачатьТранзакцию`, парность begin/commit/rollback и первый `Rollback` в
`Исключение` проверяются соседними диагностиками.

## Проверенные источники

- Handler:
  `crates/ide-diagnostics/src/handlers/commit_transaction_outside_try_catch.rs`.
- Эмиссия HIR diagnostic:
  `crates/hir-def/src/body/lower/stmt.rs`,
  `crates/hir-def/src/body/lower/diagnostics.rs`,
  `crates/hir-def/src/body/lower/platform_helpers.rs`,
  `crates/hir-def/src/body.rs`.
- Dispatch:
  `crates/ide-diagnostics/src/hir_dispatch.rs`.
- Смежные transaction diagnostics:
  `crates/ide-diagnostics/src/handlers/begin_transaction_before_try_catch.rs`,
  `crates/ide-diagnostics/src/handlers/wrong_use_of_rollback_transaction_method.rs`,
  `crates/ide-diagnostics/src/handlers/pairing_broken_transaction.rs`.
- Rule-доки:
  `crates/ide-diagnostics/docs/ru/CommitTransactionOutsideTryCatch.md`,
  `crates/ide-diagnostics/docs/en/CommitTransactionOutsideTryCatch.md`.
- Provenance:
  `docs/legal/diagnostics/CommitTransactionOutsideTryCatch.md`.
- Локальный `v8std`:
  `<v8std mirror>/docs/diagnostics/bslls/CommitTransactionOutsideTryCatch.md`,
  `<v8std mirror>/docs/diagnostics/v8-code-style/commit-transaction.md`,
  `<v8std mirror>/docs/std/783.md`,
  `<v8std mirror>/docs/std/499.md`.
- Внешние ссылки из rule-доков:
  `https://its.1c.ru/db/v8std/content/783/hdoc/_top/`,
  `https://v8std.ru/diagnostics/bslls/CommitTransactionOutsideTryCatch/`.

## Как реализовано

Распознается только глобальный неквалифицированный `CALL_STMT` на
`ЗафиксироватьТранзакцию` / `CommitTransaction`. Квалифицированные вызовы вроде
`Коннектор.ЗафиксироватьТранзакцию()` игнорируются.

Диагностика выпускается в двух местах во время lowering:

- в `lower_stmt_list_with_unreachable()`: если `CommitTransaction` найден в
  statement list без ancestor `TRY_STMT`, он считается вызовом вне
  `Попытка...Исключение`;
- в `lower_try_stmt()`: `check_commit_transaction_in_try()` проверяет сам
  `TRY_STMT`.

Внутри `TRY_STMT` проверяется:

- есть ли `EXCEPT_CLAUSE`;
- прямые executable statements первой `STMT_LIST` ветки `Попытка`;
- является ли прямой `CommitTransaction` последним direct statement;
- любые `CommitTransaction` внутри `EXCEPT_CLAUSE` через `descendants()`.

Handler `from_hir()` только создает diagnostic с фиксированным сообщением.
Quick-fix нет.

## Что покрыто

- корректный паттерн `НачатьТранзакцию(); Попытка ... ЗафиксироватьТранзакцию(); Исключение ...`;
- `CommitTransaction` вне `Попытка...Исключение`;
- `CommitTransaction` в ветке `Исключение`;
- исполняемый код после `CommitTransaction` в ветке `Попытка`;
- `CommitTransaction` внутри `try` без `except`;
- квалифицированные вызовы игнорируются;
- русское и английское имя метода;
- регистронезависимое распознавание имени;
- несколько процедур в одном файле;
- нарушение внутри цикла, когда сам `try` находится внутри цикла.

Покрытие хорошее для прямолинейного стандартного transaction pattern.

## Пробелы покрытия

- Проверка direct statements в `try` не спускается в `IF_STMT`, циклы и другие
  вложенные конструкции внутри ветки `Попытка`. `CommitTransaction` внутри
  `Если` в `Попытка` может не получить diagnostic, хотя он не является
  последним оператором перед `Исключение`.
- `is_inside_try_body()` фактически означает "есть ancestor `TRY_STMT`" и не
  различает try-body/except-body. Для текущей outside-части это работает только
  потому, что `check_commit_transaction_in_try()` отдельно смотрит except
  descendants. В transaction-анализе уже есть более точный
  `is_inside_try_body_not_except()`, но здесь он не используется.
- Логика "код после commit" не использует CFG и не различает достижимый и
  недостижимый код. Например, `Возврат` после commit считается нарушением, что
  совпадает с текущим intent, но не отделяет реальный риск исключения от любого
  statement after commit.
- Проверка не связывает `CommitTransaction` с ближайшим `BeginTransaction`.
  Вызов commit может быть синтаксически последним в `Попытка`, но не
  соответствовать транзакции из этого метода. Это зона `PairingBrokenTransaction`,
  но пользователь может ожидать единой transaction-картины.
- v8-code-style `commit-transaction` шире текущей диагностики: кроме позиции
  `Commit`, там указаны отсутствие `BeginTransaction`, отсутствие парного
  `RollbackTransaction` и другие lifecycle-нарушения.
- Rule-доки ссылаются на `#std783` и BSLLS-страницу, но не показывают
  v8-code-style `commit-transaction` и `#std499`, хотя локальный `v8std`
  связывает v8-code-style правило с обоими стандартами.
- Нет тестов на `CommitTransaction` внутри `Если` в `Попытка`, внутри
  препроцессорных веток, внутри `#Область`, внутри вложенного `try`, на `try`
  без `except`, на unreachable code after commit и на взаимодействие с
  `PairingBrokenTransaction`.
- Нет reason-specific messages. Один текст используется для "вне try", "в
  except" и "после commit есть код", хотя пользовательские исправления разные.

## Может ли инфраструктура улучшить качество

Да. Текущая HIR-lowering проверка хорошо ловит локальные syntactic cases, но
transaction-набор уже разделен между несколькими слоями:

- `BeginTransactionBeforeTryCatch` использует локальный statement-order анализ;
- `CommitTransactionOutsideTryCatch` и `WrongUseOfRollbackTransactionMethod`
  частично анализируют `TRY_STMT`;
- `PairingBrokenTransaction` уже строит CFG и проходит по execution paths.

Лучшее улучшение — общий transaction-analysis поверх HIR/CFG, который один раз
собирает Begin/Commit/Rollback/Raise, позицию в try/except, direct/nested
контекст и достижимость. Тогда текущая диагностика будет отдельным projection
"Commit стоит не в canonical position", а не самостоятельным частичным
анализатором.

## Возможное объединение

Это тот же transaction-кластер, что у `BeginTransactionBeforeTryCatch`:
`WrongUseOfRollbackTransactionMethod`, `PairingBrokenTransaction`, возможно
будущая проверка обязательного `ВызватьИсключение` в `Исключение`.

Внешние `DiagnosticCode` лучше оставить раздельными. Нарушения имеют разные
места подсветки и разные исправления: перенести `Begin`, перенести/удалить
`Commit`, поставить `Rollback` первым, добавить недостающую пару. Один общий
"TransactionRules" diagnostic стал бы менее actionable.

Внутренне их стоит объединить сильнее, чем сейчас:

- общий recognizer глобальных transaction calls;
- общая модель `TransactionCall { kind, range, stmt_id, try_context }`;
- общая классификация `try_body`, `except_body`, outside;
- общий CFG-aware результат для pairing и position diagnostics;
- единый набор tests на стандартный паттерн `#std783`.

## Варианты снятия ограничений

1. Добавить тесты на nested `CommitTransaction` внутри `Если`/цикла в ветке
   `Попытка`; затем решить expected behavior и исправить false negatives.
2. Использовать более точный try-context helper вместо `is_inside_try_body()`
   для всех transaction-shape проверок.
3. Разделить violation reasons в `BodyDiagnostic` или handler message: outside
   try, inside except, code after commit, try without except.
4. Сверить docs с v8-code-style `commit-transaction`: явно написать, какие
   lifecycle-пункты покрывает `PairingBrokenTransaction`, а какие эта
   диагностика.
5. Вынести recognizer Begin/Commit/Rollback в общий transaction helper,
   используемый lowering-диагностиками и CFG-based `PairingBrokenTransaction`.
6. Рассмотреть CFG-aware проверку "Commit должен доминировать выход из try и
   после него нет достижимого опасного кода", если потребуется меньше
   синтаксических false positives.
7. Добавить интеграционные тесты на совместную выдачу/недублирование
   `CommitTransactionOutsideTryCatch` и `PairingBrokenTransaction`.
8. Quick-fix пока не добавлять: безопасное исправление зависит от структуры
   блока и обычно требует ручного переноса кода после `Commit` за `КонецПопытки`.

## Вывод

Диагностика полезно покрывает главный запрет `#std783`: `CommitTransaction` не
должен стоять вне `try/except`, в `except` или перед дополнительным кодом в
`try`. Основной риск качества — частичный локальный анализ: direct statements
проверяются, а вложенные ветки внутри `Попытка` могут выпасть. Следующий шаг —
общий transaction-analysis слой и reason-specific diagnostics, не объединяя
внешние коды правил.

## Закрыто Track 2

**Phase D §2 audit (2026-05):** out-of-scope для Track 2. Master plan §2
включал только `BeginTransactionBeforeTryCatch` + `MissingCodeTryCatchEx`;
остальной transaction-cluster (этот, `PairingBrokenTransaction`,
`WrongUseOfRollbackTransactionMethod`, `TryNumber`) — отдельный
будущий трек по transaction-shape анализу поверх CFG.
