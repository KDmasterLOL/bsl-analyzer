# UsingHardcodePath

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит абсолютные пути к файлам и каталогам, записанные строковыми литералами.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/using_hardcode_path.rs`
- `<v8std mirror>/docs/diagnostics/bslls/UsingHardcodePath.md`

## Как реализовано

AST token-pass проверяет `STRING`: Windows drive path, UNC, `//`, Unix root path из заданного списка стандартных каталогов, `~`, `%ENV%/path`. URL исключаются. Для Unix-каталогов используется параметр `searchWordsStdPathsUnix`.

## Что покрыто

Покрыты Windows, UNC, Unix standard paths, home-relative и environment-variable paths; относительные пути и URL не диагностируются.

## Пробелы и ограничения

Нет контекста использования: строка может быть примером, тестом, шаблоном или аргументом запуска. Для Unix намеренно игнорируются нестандартные root-пути вроде `/catalog`, что снижает false positive, но оставляет пропуски.

## Может ли инфраструктура улучшить качество

Да. Нужен общий literal-classifier и контекст вызова: путь в файловом API опаснее, чем текст сообщения или документация в строке.

## Возможное объединение

Хорошо ложится в общий hardcode-анализ с `UsingHardcodeNetworkAddress` и `UsingHardcodeSecretInformation`. Можно объединить проход по литералам и конфиг исключений, сохранив отдельные коды.

## Вывод

Правило покрывает основные паттерны путей, но без контекста вызовов неизбежно балансирует между шумом и пропусками.
