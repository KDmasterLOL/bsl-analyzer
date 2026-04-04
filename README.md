# BSL Analyzer

Высокопроизводительный Language Server для языка BSL (1С:Предприятие), написанный на Rust.

## Возможности

- **180 диагностик** качества кода BSL
- **LSP** — поддержка Language Server Protocol для IDE
- **MCP** — встроенный сервер Model Context Protocol для AI-агентов
- **SonarQube** — отчёты SARIF, потоковый режим для крупных проектов
- **Конфигурация проекта** через `bsl-analyzer.toml`
- **Кроссплатформенность** — Linux, Windows, macOS (Apple Silicon)

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

- 📖 [Настройка MCP и профилей](docs/mcp/README.md)
- 🛠️ [Инструменты AI и расширение для 1С](docs/mcp/TOOLS_AND_EXTENSION.md)

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

## Производительность

Сравнительный бенчмарк с [bsl-language-server](https://github.com/1c-syntax/bsl-language-server) на реальном проекте.

**Тестовый проект:** Управление торговлей 11.5.22.134 (12 519 BSL-файлов, 500 MB кода)  
**Конфигурация:** настройки по умолчанию, отключена только `Typo`, без дополнительных фильтров диагностик.  
**Система:** AMD Ryzen 5 5600X (6 ядер / 12 потоков), 32 GB RAM, Linux 6.19, NVMe SSD

| Метрика | bsl-language-server 0.28.5 | bsl-analyzer 0.1.111 | Разница |
|---------|---------------------------|----------------------|---------|
| **Wall time** | 132.6s | 52s | 2.6x быстрее |
| **Peak RSS** | 4 566 MB | 1 553 MB | 2.9x меньше памяти |
| **Files/sec** | 94 | 263 | 2.8x пропускная способность |
| **Диагностик** | 552 111 | 610 477 | На 11% больше точных диагностик |

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

Проект вдохновлён [BSL Language Server](https://github.com/1c-syntax/bsl-language-server) — инструментом статического анализа BSL на Java. Спасибо авторам за огромную работу по формализации диагностик и стандартов качества кода 1С.

- [bsl-language-server](https://github.com/1c-syntax/bsl-language-server) — статический анализатор BSL (Java, LGPL-3.0)
- [1c-syntax](https://github.com/1c-syntax) — сообщество разработчиков инструментов для 1С
- [RDT1C](https://github.com/Segate-ekb/1c_RDT) — «Инструменты разработчика» для 1С; использован как референс для модуля `naparnik`

## Лицензия

MIT или Apache-2.0, на выбор. См. `LICENSE-MIT` и `LICENSE-APACHE`.
