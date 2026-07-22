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

Определяет корень основной конфигурации 1С и расширений:

```toml
[source]
root = "src/cf"
extensions = [
  "src/cfe/BMS_RU_UT",
  "src/cfe/YAxUnit",
]
```

`root` — корень основной конфигурации. Если не задан, `project-model`
попытается сам найти `Configuration.xml` внутри проекта.

`extensions` — список расширений конфигурации (CFE). Каждый элемент — либо
строка-путь относительно корня проекта (в последнем сегменте допустим глоб
`*`), либо структурированная запись со стабильным именем:

```toml
[source]
root = "src/cf"
extensions = [
  "src/cfe/BMS_RU_UT",                                              # строка-путь
  { name = "yaxunit", path = "vendor/yaxunit" },                    # именованная запись
  { name = "TESTS", path = "src/cfe/TESTS", dependsOn = ["yaxunit"] },
]
```

Каждый путь должен указывать на каталог с `Configuration.xml`. Метаданные
расширений видны автокомплиту и диагностике наравне с основной конфигурацией.

Отличия двух форм:

- **Строка-путь** ведёт себя как раньше: имя выводится из имени каталога,
  отсутствующий путь или каталог без `Configuration.xml` пропускается
  с предупреждением, текстовые варианты одного пути схлопываются.
- **Структурированная запись** строгая: отсутствующий путь, каталог без
  `Configuration.xml`, глоб в `path`, дубль имени или пути — ошибка загрузки
  проекта, а не предупреждение. `dependsOn` принимает имена других расширений
  (допустима и snake_case-форма `depends_on`).

`dependsOn` объявляет направленную зависимость между расширениями
(`A dependsOn B` — «A использует API B»). Граф валидируется при загрузке:
неизвестные имена, самоссылки, дубли рёбер и циклы (с полным путём цикла)
отклоняются до начала анализа.

Видимость по зависимостям: файл расширения `E` видит базовую конфигурацию,
транзитивные зависимости `E` (в стабильном топологическом порядке) и само
`E` — последним. Обратной видимости нет (`B` не видит `A`), независимые
расширения не видят друг друга, файл базовой конфигурации не видит ни одного
расширения. При поиске имени приоритет у самого расширения, затем у ближайшей
зависимости, затем у базы; оверлеи метаданных накладываются в прямом порядке
(база → зависимости → своё расширение), так что унаследованные реквизиты
сохраняются.

Команда `bsl-analyzer-app check-config bsl-analyzer.toml` печатает секцию
`Extension topology` — разобранные узлы, рёбра, детерминированный порядок
и отпечаток топологии.

> Оба ключа должны быть внутри секции `[source]`. Если `extensions` окажется
> выше `[source]`, парсер отклонит конфиг с ошибкой — это намеренно, чтобы
> исключить тихое присваивание в «не ту» таблицу TOML.

> Существующий, но синтаксически некорректный `bsl-analyzer.toml` — ошибка
> загрузки проекта (CLI завершится с ошибкой, LSP покажет сообщение и не
> поднимет воркспейс). Отката к конфигурации по умолчанию при битом файле
> больше нет: молчаливый откат означал бы анализ «не того» проекта.

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

### `[output]`

Управляет языком пользовательских сообщений анализатора — имён примитивных
типов в диагностиках (`'Число'` vs `'Number'`), hover-карточек и
completion-detail.

```toml
[output]
display_language = "ru"   # или "en"
```

Поддерживаемые значения для `display_language`:

- русский: `"ru"`, `"ru-RU"`, `"ru_RU"`, `"russian"`;
- английский: `"en"`, `"en-US"`, `"en_US"`, `"en-GB"`, `"en_GB"`, `"english"`.

Сравнение регистронезависимое, пробелы по краям обрезаются. Неизвестное
значение логируется как `tracing::warn!` и игнорируется — анализатор
переходит к следующему сигналу локали (см. ниже), а не падает на загрузке
проекта.

Поддерживается также camelCase-алиас `displayLanguage` для совместимости с
JSON-форматом.

#### Приоритет источников локали

Эффективная локаль определяется по убыванию приоритета:

1. `[output] display_language` в `bsl-analyzer.toml` — фиксирует язык на
   уровне проекта (команда видит одинаковый язык независимо от настроек
   IDE у конкретного разработчика).
2. `InitializeParams.locale` от LSP-клиента (RFC 4646: `"ru-RU"`,
   `"en-US"`, …). Учитывается только primary-subtag: `"ru-RU"` → `ru`,
   `"de-DE"` → `en` (всё, что не русский, считается английским).
3. Дефолт анализатора — `ru`. BSL — русскоязычный язык-первого-класса:
   при отсутствии всех сигналов выводятся русские имена типов.

При CLI-запуске LSP-сигнала нет, поэтому работают только `[output]
display_language` и дефолт.

> Локализуются только **имена типов**, подставляемые в сообщения. Каркасы
> диагностик (текст «Несоответствие типов: ожидалось …, получено …»)
> сейчас захардкожены русскими — это сознательное ограничение текущего
> уровня i18n.

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
