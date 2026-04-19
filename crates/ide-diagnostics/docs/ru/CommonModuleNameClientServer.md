# Пропущен постфикс "КлиентСервер" (CommonModuleNameClientServer)

## Описание диагностики

Клиент-серверные общие модули содержат код, доступный и на клиенте, и на
сервере без использования схемы `ВызовСервера`. Для таких модулей стандарт
требует явного постфикса `КлиентСервер` (англ. `ClientServer`) в имени.

Этот постфикс помогает сразу отличить клиент-серверный модуль от чисто
клиентского, серверного или серверного для вызова с клиента варианта.

## Примеры

Допустимые имена: `РаботаСФайламиКлиентСервер`, `ОбщегоНазначенияКлиентСервер`, `UsersClientServer`

Недопустимые имена: `РаботаСФайлами`, `ОбщегоНазначения` (при клиент-серверных признаках модуля)

## Источники

- [Стандарт: #std469](https://its.1c.ru/db/v8std#content:469:hdoc:2.4)
- [v8std.ru: #std469](https://v8std.ru/std/469/)
- [v8std.ru: CommonModuleNameClientServer](https://v8std.ru/diagnostics/bslls/CommonModuleNameClientServer/)
- [v8std.ru: common-module-name-client-server](https://v8std.ru/diagnostics/v8-code-style/common-module-name-client-server/)
