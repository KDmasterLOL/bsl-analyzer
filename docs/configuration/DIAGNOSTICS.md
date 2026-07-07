# Настройка диагностик

Этот документ описывает только диагностики: общие флаги анализа, включение и
отключение правил, а также параметры конкретных диагностических правил.

Общая структура `bsl-analyzer.toml`, секции `source`, `formatting`,
`code_lens`, `extensions` и `search.baseline` описаны в
`docs/configuration/PROJECT_CONFIGURATION.md`.

## Базовая схема

Все настройки диагностик живут в двух местах:

- `[diagnostics]` — общие флаги подсистемы;
- `[diagnostics.parameters]` — включение, выключение и параметры отдельных правил.

Минимальный пример:

```toml
[diagnostics]
ordinaryAppSupport = false
dataflowMaxIterations = 10000

[diagnostics.parameters]
CommentedCode = false
BadWords = true

[diagnostics.parameters.CyclomaticComplexity]
complexityThreshold = 20
```

## Общие настройки `[diagnostics]`

В этом разделе поддерживаются только подтверждённые кодом поля:

```toml
[diagnostics]
ordinaryAppSupport = false
dataflowMaxIterations = 10000
```

Поддерживаемые ключи:

- `ordinaryAppSupport` — учитывать проверки для обычных форм и обычного приложения;
- `dataflowMaxIterations` — верхняя граница итераций для dataflow-анализа;
- `bsllsSuppressionCompat` — распознавать директивы подавления bsl-language-server
  (`// BSLLS:Ключ-выкл`) как алиасы наших. Включено по умолчанию; см. раздел
  «Подавление в коде».

Значение по умолчанию для `dataflowMaxIterations` — `10000`, для
`bsllsSuppressionCompat` — `true`.

### `ordinaryAppSupport`

```toml
[diagnostics]
ordinaryAppSupport = true
```

Нужно только для части диагностик, которые учитывают поддержку обычного
приложения в общих модулях.

### `dataflowMaxIterations`

```toml
[diagnostics]
dataflowMaxIterations = 20000
```

Используется flow-sensitive диагностикой и влияет, например, на liveness и
reaching definitions. Обычно значения по умолчанию достаточно; увеличивать его
имеет смысл только для действительно тяжёлых методов со сложным CFG.

## Настройка отдельных правил: `[diagnostics.parameters]`

Для каждой диагностики поддерживаются три формы значения:

- `false` — отключить правило;
- `true` — явно включить правило, которое по умолчанию выключено;
- `{ ... }` — передать параметры конкретной диагностике.

Пример:

```toml
[diagnostics.parameters]
CommentedCode = false
BadWords = true
LineLength = { maxLineLength = 140 }
MethodSize = { maxMethodSize = 250 }
```

## Часто используемые параметры

Ниже перечислены параметры, которые подтверждены реализацией в
`crates/ide-diagnostics/src/handlers/`.

| Диагностика | Параметр | Значение по умолчанию |
|-------------|----------|-----------------------|
| `LineLength` | `maxLineLength` | `120` |
| `MethodSize` | `maxMethodSize` | `200` |
| `CyclomaticComplexity` | `complexityThreshold` | `20` |
| `CognitiveComplexity` | `complexityThreshold` | `15` |
| `NestedStatements` | `maxAllowedLevel` | `4` |
| `NumberOfParams` | `maxParamsCount` | `7` |
| `NumberOfOptionalParams` | `maxOptionalParamsCount` | `3` |
| `NumberOfValuesInStructureConstructor` | `maxValuesCount` | `3` |
| `TooManyReturns` | `maxReturnsCount` | `3` |
| `IfConditionComplexity` | `maxIfConditionComplexity` | `3` |
| `MagicNumber` | `authorizedNumbers` | `-1,0,1` |
| `MagicDate` | `authorizedDates` | встроенный набор разрешённых дат |
| `BadWords` | `badWords` | пусто |
| `CommentedCode` | `threshold` | `0.9` |
| `ConsecutiveEmptyLines` | `allowedEmptyLinesCount` | `1` |

Примеры:

```toml
[diagnostics.parameters.LineLength]
maxLineLength = 140

[diagnostics.parameters.MethodSize]
maxMethodSize = 250

[diagnostics.parameters.CognitiveComplexity]
complexityThreshold = 12

[diagnostics.parameters.MagicNumber]
authorizedNumbers = "-1,0,1,2,10,100"

[diagnostics.parameters.BadWords]
badWords = "хрень,костыль,тупой"
```

## Как включать правила, выключенные по умолчанию

Некоторые diagnostics не активны по умолчанию. Их можно явно включить через
`true` или через объект параметров:

```toml
[diagnostics.parameters]
BadWords = true
TooManyReturns = { maxReturnsCount = 5 }
```

## Подавление в коде (директивы-комментарии)

Кроме конфига, отдельные срабатывания можно подавлять прямо в коде — это нужно,
чтобы довести проект до нуля замечаний и включить строгий CI-гейт, не выключая
полезные правила целиком.

### Нативные директивы

```bsl
// bsl-analyzer:off ИмяКода1, ИмяКода2   // начать подавление диапазона
…
// bsl-analyzer:on ИмяКода1              // закончить подавление диапазона

// bsl-analyzer:disable-next-line ИмяКода   // подавить следующую строку
Значение = Функция();  // bsl-analyzer:disable-line ИмяКода   // подавить эту строку
```

- `off` без парного `on` действует до конца файла (поставьте его на первую строку —
  подавите весь файл);
- перечисляйте конкретные коды; директива **без кодов** подавляет все диагностики
  в области и сама помечается замечанием `SuppressionWithoutCode`;
- опечатка в имени кода не подавляет ничего и помечается `UnknownSuppressionCode`.

Директивы работают одинаково в LSP, MCP и CLI (`analyze`, все репортёры, включая
SARIF — подавлённые не попадают в отчёт).

### Совместимость с bsl-language-server

По умолчанию распознаются и директивы bsl-language-server, чтобы существующие
комментарии проекта продолжали работать без массовой правки:

```bsl
// BSLLS:ИмяКода-off   … // BSLLS:ИмяКода-on     // диапазон для одного правила
// BSLLS-off           … // BSLLS-on             // диапазон для всех правил
```

Поддерживаются локализованные `-выкл`/`-вкл` и trailing-форма (директива `-off`
на строке с кодом подавляет только эту строку). Отключить распознавание:

```toml
[diagnostics]
bsllsSuppressionCompat = false
```

**Важно про совпадение имён.** Директива адресует правило по имени кода. Имена
наших диагностик в основном совпадают с ключами bsl-language-server, поэтому
`// BSLLS:Ключ-off` для такого правила сработает. Если ключ bsl-language-server
называется иначе, чем наш код, подавление сработает только когда для этой пары
заведён алиас (таблица `BSLLS_KEY_ALIASES` в `crates/ide-diagnostics/src/suppression.rs`;
сейчас, например, bslls `AssignToReadOnlyProperty` → наш `ReadOnlyPropertyAssignment`).
Ключи правил, которых у анализатора нет, игнорируются молча — подавлять нечего.

## Что больше неактуально

Следующие вещи не стоит использовать в новой документации и в новых примерах:

- `validate-config` — такой команды в CLI нет;
- `skipSupport`, `computeTrigger`, `mode`, `skip` — эти поля не поддерживаются
  текущим TOML-конфигом проекта;
- примеры, где JSON выступает как основной формат конфигурации.

## Проверка diagnostic-конфига

Надёжный способ проверить разбор конфигурации — `check-config`:

```bash
bsl-analyzer check-config --config ./bsl-analyzer.toml
```

Команда печатает сводку по общим настройкам диагностик и показывает:

- `ordinaryAppSupport`;
- `dataflowMaxIterations`;
- количество правил;
- список отключённых, явно включённых и параметризованных диагностик.

Для end-to-end проверки можно запустить анализ с явным конфигом:

```bash
bsl-analyzer analyze -s ./my-project -c ./bsl-analyzer.toml
```

## Совместимость с legacy JSON

Legacy-формат всё ещё поддерживается. В нём используются camelCase-ключи:

```json
{
  "diagnostics": {
    "ordinaryAppSupport": false,
    "dataflowMaxIterations": 10000,
    "parameters": {
      "LineLength": { "maxLineLength": 120 }
    }
  }
}
```

Для новых проектов рекомендуется `bsl-analyzer.toml`.
