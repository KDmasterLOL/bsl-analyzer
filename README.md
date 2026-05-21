# BSL Analyzer

Экспериментальный Language Server для BSL (1С:Предприятие), написанный на Rust.

## Идея проекта

Проект проверяет гипотезу: что получит разработчик 1С, если применить
архитектуру [rust-analyzer](https://github.com/rust-lang/rust-analyzer) к
языку 1С:Предприятие — инкрементальные вычисления на Salsa, full-fidelity CST
на Rowan, многослойный HIR с выводом типов, flow-sensitive анализ через CFG и
dataflow.

Какие практические эффекты даёт такая архитектура:

- **Инкрементальные пересчёты.** Salsa переинвалидирует только зависимую от
  правки часть графа запросов, без полной переобработки проекта.
- **Семантический hover и completion.** Реальный вывод типов (HIR/Ty):
  `Документы.ПКО.<TAB>` фильтруется по типу receiver'а; hover показывает
  фактический тип выражения, включая union'ы из `ОписаниеТипов` и JSDoc.
  После `Если ТипЗнч(Х) = Тип("Массив")` тип внутри ветки сужается (narrowing).
- **Find references и rename по символам.** Поиск работает через
  `SemanticSymbol`, а не текстовый grep — одноимённые локальные переменные и
  экспорты модуля не путаются.
- **Flow-sensitive диагностики.** Unreachable code, неиспользованные
  присваивания, потерянные `Возврат` ищутся через CFG и reaching definitions.
- **Анализ незаконченного кода.** Recovery-узлы Rowan позволяют hover и
  completion работать на полу-набранной строке без flicker'а диагностик.

Подробнее — в [`docs/architecture/ARCHITECTURE.md`](docs/architecture/ARCHITECTURE.md)
и [`docs/architecture/TYPE_SYSTEM.md`](docs/architecture/TYPE_SYSTEM.md).

## Возможности

- Набор диагностик качества кода BSL
- LSP — поддержка Language Server Protocol для IDE
- MCP — встроенный сервер Model Context Protocol для AI-агентов
- SonarQube — отчёты SARIF, потоковый режим для крупных проектов
- Конфигурация проекта через `bsl-analyzer.toml`
- Linux, Windows, macOS (Apple Silicon)

## Сообщество

Вопросы, идеи, баги — в Telegram-чат: https://t.me/+K7CcQPNZobE3YjUy

## Установка

### Linux / macOS

<!-- INSTALL_URL:github -->
```bash
curl -fsSL https://raw.githubusercontent.com/itrous/bsl-analyzer/develop/scripts/install.sh | bash
```

Или с указанием версии:

```bash
curl -fsSL https://raw.githubusercontent.com/itrous/bsl-analyzer/develop/scripts/install.sh | bash -s -- --version 0.1.38
```
<!-- /INSTALL_URL -->

### Windows (PowerShell)

```powershell
Invoke-WebRequest "https://github.com/itrous/bsl-analyzer/releases/latest/download/bsl-analyzer-windows-amd64.exe" -OutFile bsl-analyzer.exe
```

## Использование

### LSP-сервер

```bash
bsl-analyzer lsp
```

### Анализ проекта

```bash
# Консольный вывод
bsl-analyzer analyze -s ./my-project

# SARIF-отчёт
bsl-analyzer analyze -s ./my-project -r sarif -o ./reports

# JSONL-вывод
bsl-analyzer analyze -s ./my-project --format=jsonl > report.jsonl
```

### Интеграция с AI через MCP

BSL Analyzer включает встроенный [MCP-сервер](https://modelcontextprotocol.io/) (Model Context Protocol), который позволяет AI-агентам работать с кодом 1С, справкой платформы, SDBL-запросами и отладкой.

Самый простой способ подключить инструменты к AI-клиентам — выполнить автоустановку:

```bash
# Устанавливает global reference и project workspace
bsl-analyzer mcp install \
  --target all \
  --preset recommended \
  --source-dir ./my-project
```

Подробности:

- [Настройка MCP и профилей](docs/mcp/README.md)
- [Инструменты AI и расширение для 1С](docs/mcp/TOOLS_AND_EXTENSION.md)

## Куда идти дальше

`README.md` остаётся короткой продуктовой точкой входа: установка, запуск и
основные сценарии. Детальная навигация по документации собрана в
`docs/README.md`.

Быстрые ссылки:

- [Карта документации](docs/README.md) — полный обзор пользовательских и внутренних документов
- [Конфигурация проекта](docs/configuration/PROJECT_CONFIGURATION.md) — структура `bsl-analyzer.toml`
- [Конфигурация диагностик](docs/configuration/DIAGNOSTICS.md) — параметры диагностических правил
- [Настройка MCP](docs/mcp/README.md) — профили, `mcp install` и ручной запуск
- [Архитектура](docs/architecture/ARCHITECTURE.md) — устройство анализатора и основные пайплайны

## Конфигурация

Основной файл настройки проекта — `bsl-analyzer.toml` в корне репозитория.

Минимальный пример:

```toml
[source]
root = "src/cf"

[diagnostics]
ordinaryAppSupport = false
dataflowMaxIterations = 10000

[diagnostics.parameters]
CommentedCode = false
BadWords = true

[diagnostics.parameters.CyclomaticComplexity]
complexityThreshold = 20
```

Общая структура файла описана в `docs/configuration/PROJECT_CONFIGURATION.md`,
а параметры диагностических правил — в `docs/configuration/DIAGNOSTICS.md`.

## Сборка из исходников

**Требования:** Rust 1.91+

```bash
git clone https://github.com/itrous/bsl-analyzer.git
cd bsl-analyzer
cargo build --release
```

Если вы планируете менять код проекта, процесс контрибуции и профильная
документация для разработчиков собраны в `CONTRIBUTING.md` и `docs/README.md`.

## Архитектура проекта

```text
bsl-analyzer (LSP/CLI/MCP)
    └── ide
        ├── ide-diagnostics
        ├── ide-assists
        └── ide-db
            └── hir / hir-def / hir-ty
                └── syntax / parser / lexer
```

Подробнее об архитектуре, Salsa, dataflow и centralized search — в
`docs/architecture/ARCHITECTURE.md`, `docs/architecture/DATAFLOW.md` и
`docs/central-postgres-search/README.md`.

## Благодарности

- [rust-analyzer](https://github.com/rust-lang/rust-analyzer) — архитектурный референс: Salsa, Rowan, слойная организация `syntax → hir-def → hir-ty → ide`, подход к инкрементальной компиляции и LSP-фичам. Бóльшая часть структуры крейтов в `bsl-analyzer` следует именно этому образцу.
- [bsl-language-server](https://github.com/1c-syntax/bsl-language-server) — статический анализатор BSL (Java, LGPL-3.0); источник формализации диагностических правил и стандартов качества кода 1С.
- [1c-syntax](https://github.com/1c-syntax) — сообщество разработчиков инструментов для 1С.
- [RDT1C](https://github.com/Segate-ekb/1c_RDT) — «Инструменты разработчика» для 1С; использован как референс для модуля `naparnik`.

## Лицензия

LGPL-3.0-or-later. См. `LICENSE-LGPL`, `LICENSE-GPL` и `NOTICE`.

Репозиторий содержит материалы, адаптированные по мотивам
`bsl-language-server` (`LGPL-3.0`), поэтому permissive-модель `MIT/Apache-2.0`
для всего workspace больше не заявляется. Если позже будет проведён
файловый provenance-аудит, отдельные части проекта можно будет лицензировать
отдельно.
