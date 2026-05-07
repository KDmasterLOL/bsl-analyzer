# UsingHardcodeSecretInformation

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит хранение паролей и похожих секретов в коде.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/using_hardcode_secret_information.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/UsingHardcodeSecretInformation.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/740.md`

## Как реализовано

HIR проверяет присваивания в переменные/поля/индексы с именем по regex `searchWords` (`Пароль|Password` по умолчанию), `Вставить/Insert` в структуры и карты, конструкторы `Структура`/`Соответствие` и 4-й аргумент `HTTPСоединение`/`FTPСоединение`.

## Что покрыто

Покрыты прямые присваивания, поля, индексный доступ, `Вставить`, конструктор структуры/карты, HTTP/FTP соединение, пустые строки и строки из одних `*` исключены.

## Пробелы и ограничения

Не покрыты вычисляемые секреты, конкатенация, вложенные структуры глубже простых случаев, параметры конструкторов через переменные и другие секретные ключи без настройки. Regex по имени поля может пропускать token/api-key сценарии.

## Может ли инфраструктура улучшить качество

Да. Нужны расширяемая taxonomy секретов, taint/constant propagation и понимание типов контейнеров. Полезен единый механизм подавления тестовых фикстур.

## Возможное объединение

Инфраструктурно близко к hardcode-группе (`UsingHardcodePath`, `UsingHardcodeNetworkAddress`), но как security rule должно остаться отдельным из-за другой severity и remediation.

## Вывод

Покрытие типовых password-кейсов хорошее; для modern secrets стоит расширить словарь и добавить dataflow.
