# Function name shouldn't start with "Получить" (FunctionNameStartsWithGet)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

According to the 1C naming standard, a function name should describe the
returned value. The prefix `Получить` duplicates the fact that the routine is a
function and therefore already returns something.

Prefer a name derived from the result instead of the action of obtaining it.

Current implementation checks function names that start with the Russian prefix
`Получить` (case-insensitive). Procedures and English `Get...` names are not
reported by this diagnostic.

## Examples
```bsl
// Incorrect:
Функция ПолучитьДатуДокумента()
    Возврат ТекущийДокумент.Дата;
КонецФункции

// Correct:
Функция ДатаДокумента()
    Возврат ТекущийДокумент.Дата;
КонецФункции
```


## Sources
* Primary source: [ITS / v8std #std647: Names of procedures and functions, section 6.1 (RU)](https://its.1c.ru/db/v8std#content:647:hdoc)
* Secondary source: [v8std.ru: #std647](https://v8std.ru/std/647/)
* Secondary source: [v8std.ru: FunctionNameStartsWithGet](https://v8std.ru/diagnostics/bslls/FunctionNameStartsWithGet/)
