# Конфигурация проекта

Этот документ описывает общую структуру `bsl-analyzer.toml`: где лежит файл,
какие верхнеуровневые секции поддерживаются и как проверять конфигурацию.

Параметры конкретных диагностик вынесены отдельно в
`docs/configuration/DIAGNOSTICS.md`.

## Какие форматы поддерживаются

При автоматической загрузке конфигурации используется такой приоритет:

1. `bsl-analyzer.toml`
2. `.bsl-analyzer.json`
3. `.bsl-language-server.json`

Важно:

- если `bsl-analyzer.toml` найден, но не парсится, fallback на JSON не
  выполняется;
- legacy JSON поддерживается только как слой совместимости со старым tooling;
- для новых проектов основным форматом считается `bsl-analyzer.toml`.

## Где должен лежать файл

Обычно конфигурация лежит в корне проекта:

```text
my-project/
├─ bsl-analyzer.toml
├─ src/
└─ ...
```

Для CLI можно передать явный путь:

```bash
bsl-analyzer analyze -s ./my-project -c ./configs/bsl-analyzer.toml
```

## Минимальный пример

```toml
[source]
root = "src/cf"

[diagnostics]
ordinaryAppSupport = false
dataflowMaxIterations = 10000

[diagnostics.parameters]
CommentedCode = false

[formatting]
use_tabs = true
indent_size = 1
```

## Верхнеуровневая структура `bsl-analyzer.toml`

### `[source]`

Определяет корень конфигурации 1С.

```toml
[source]
root = "src/cf"
```

Если параметр не задан, `project-model` пытается сам найти `Configuration.xml`
внутри проекта.

### `extensions`

Список путей к расширениям конфигурации, относительных корню проекта:

```toml
extensions = [
  "src/cfe/BMS_RU_UT",
  "src/cfe/YAxUnit",
]
```

Каждый путь должен указывать на каталог, где присутствует `Configuration.xml`.

### `[diagnostics]` и `[diagnostics.parameters]`

В этой секции задаются общие флаги диагностики и параметры конкретных правил.

Базовый вид:

```toml
[diagnostics]
ordinaryAppSupport = false
dataflowMaxIterations = 10000

[diagnostics.parameters]
LineLength = { maxLineLength = 140 }
BadWords = true
CommentedCode = false
```

Полный список поддерживаемых ключей и параметров — в
`docs/configuration/DIAGNOSTICS.md`.

### `[code_lens]`

Поддерживаются два булевых флага:

```toml
[code_lens]
show_cognitive_complexity = true
show_cyclomatic_complexity = true
```

В JSON-совместимом формате им соответствуют camelCase-ключи
`showCognitiveComplexity` и `showCyclomaticComplexity`.

### `[formatting]`

Настройки форматтера:

```toml
[formatting]
use_tabs = true
indent_size = 1
```

Поддерживаемые поля:

- `use_tabs` — использовать табы;
- `indent_size` — размер одного уровня отступа.

### `[search.baseline]`

Секция описывает настройки централизованного baseline для поиска.

Минимальный пример для PostgreSQL backend:

```toml
[search.baseline]
backend = "postgres"

[search.baseline.postgres]
schema = "bsl_search"
url_env = "BSL_SEARCH_BASELINE_PG_URL"

[search.baseline.workspace_code.policy]
publish_branches = ["vendor", "develop"]
```

Подробная схема, branch policy и CLI-команды вынесены в:

- `docs/central-postgres-search/README.md`
- `docs/central-postgres-search/13-cli-commands.md`

## Проверка конфигурации

### Быстрая проверка через `check-config`

Команда `check-config` умеет читать и TOML, и legacy JSON:

```bash
bsl-analyzer check-config --config ./bsl-analyzer.toml
```

```bash
bsl-analyzer check-config --config ./.bsl-analyzer.json
```

В сводке можно увидеть:

- `source.root` и подключённые extensions;
- сводку по diagnostics;
- отключённые, явно включённые и параметризованные правила;
- code lens;
- formatting;
- workspace/reference baseline selection.

### Проверка в реальном запуске

Для end-to-end проверки можно запустить анализ с явным конфигом:

```bash
bsl-analyzer analyze -s ./my-project -c ./bsl-analyzer.toml
```

## Совместимость с legacy JSON

Legacy-формат всё ещё поддерживается. В нём встречаются camelCase-ключи:

```json
{
  "configurationRoot": "src/cf",
  "diagnostics": {
    "ordinaryAppSupport": false,
    "dataflowMaxIterations": 10000,
    "parameters": {
      "LineLength": { "maxLineLength": 120 }
    }
  }
}
```

Это нужно только для совместимости со старой экосистемой. Для новых проектов
рекомендуется `bsl-analyzer.toml`.
