# PublicMethodsDescription

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что экспортные методы программного интерфейса имеют комментарий-описание.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/public_methods_description.rs`
- `<v8std mirror>/docs/diagnostics/bslls/PublicMethodsDescription.md`
- `<v8std mirror>/docs/std/453.md`

## Как реализовано

Обходит procedures/functions из `module_data`. Если метод экспортный и `ctx.method_docs` пустой, по умолчанию проверяется только root API-region `ПрограммныйИнтерфейс` / `Public`. Конфиг `checkAllRegion` расширяет проверку на все экспортные методы.

## Что покрыто

Покрыты экспортные методы без описания в API-регионах, вложенные области внутри public root и режим проверки всех регионов. Регион `СлужебныйПрограммныйИнтерфейс` (и `Internal`) при дефолтном режиме явно исключены тестами.

## Пробелы и ограничения

Проверяется только наличие raw docs, а качество секций параметров/возврата вынесено в соседние диагностики. Метод вне region при дефолтном режиме может быть пропущен.

## Может ли инфраструктура улучшить качество

Да. Общий docs analyzer должен строить цельную картину: есть ли комментарий, параметры, return, types и качество текста.

## Возможное объединение

Близко к `MissingParameterDescription`, `MissingReturnedValueDescription`, `NonExportMethodsInApiRegion`. Лучше один documentation/region engine с несколькими кодами.

## Вывод

Правило правильно не дублирует соседние docs diagnostics, но из-за этого пользователю нужно смотреть семейство целиком.

## Закрыто Track 2

**Phase B §5.3 (косвенно, commit `6d8a1eb2`, 2026-05):** Track 2 не
вносил прямых изменений в этот handler, но связанный gap «MRVD не
покрывает no-doc export при PMD-disabled» закрыт в MRVD.
Audit gap «export outside region — emit всё равно» из master plan §5.3
**отложен в Track 4 quick-fixes** (требует решения по UX и формулировке
сообщения для region-violations).
