# DataExchangeLoading

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

В обработчиках записи/удаления объектных модулей нужна ранняя проверка
`ОбменДанными.Загрузка`, чтобы не выполнять бизнес-логику при обмене данными.
Основание - стандарты интеграции/обмена данных из локальной документации.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/data_exchange_loading.rs`
- `crates/ide-diagnostics/docs/ru/DataExchangeLoading.md`
- `docs/legal/diagnostics/DataExchangeLoading.md`

## Как реализовано

Handler применим к object/recordset/value-manager modules. Через ItemTree и HIR
берет процедуры с именами `ПередЗаписью`, `ПриЗаписи`, `ПередУдалением` /
English variants. Проверяет наличие `If ОбменДанными.Загрузка Then Return` в
первом executable statement или где угодно, в зависимости от `findFirst`.

## Что покрыто

Тесты охватывают отсутствующий guard, валидные русский/английский guards,
`Если ... Тогда` без `Возврат`, не-monitored процедуры, case-insensitive,
сложное условие с `Или`, неверное условие (`Отказ`), неверное поле
(`.Recipients`), `Возврат` во вложенной логике, `findFirst` true/false и
негированный guard `НЕ ОбменДанными.Загрузка И ...`. Helper'ы HIR pattern
matching и module applicability через path/metadata проверяются опосредованно.

## Пробелы и ограничения

- Guard считается валидным даже при `НЕ ОбменДанными.Загрузка` с `Return`,
  потому что условие проверяется на наличие field, а не на полярность.
- Не проверяется, что `Return` действительно прекращает нужный путь до
  бизнес-логики во всех ветках.
- Не распознаются aliases/wrappers вроде `Если ЭтоЗагрузкаДанных() Тогда`.
- Module applicability частично fallback'ится на `true` для unknown path, что
  удобно для тестов, но может шуметь в standalone файлах.

## Инфраструктурные улучшения

Нужен небольшой control-flow/predicate analyzer: полярность условия, ранний
exit, first executable statement, wrapper whitelist.

## Возможное объединение

Близко к event-handler policy diagnostics, но сливать внешний код не надо.
Внутренне можно объединить с правилами про cancel parameter и обработчики
событий через общий event-handler analyzer.

## Вывод

Правило ловит основной паттерн, но сейчас может принять обратную проверку за
валидную. Это главный bug-risk.

