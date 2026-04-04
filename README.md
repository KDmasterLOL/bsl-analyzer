# BSL Analyzer

Высокопроизводительный Language Server для языка BSL (1С:Предприятие), написанный на Rust.

## Возможности

- **180+ диагностик** качества кода BSL
- **LSP** — поддержка Language Server Protocol для IDE
- **MCP** — встроенный сервер Model Context Protocol для AI-агентов
- **SonarQube** — отчёты SARIF, потоковый режим для крупных проектов
- **Совместимость** с форматом конфигурации `bsl-analyzer.toml`
- **Кроссплатформенность** — Linux, Windows, macOS (Apple Silicon)

## Установка

### Linux / macOS

<!-- INSTALL_URL:gitlab -->
```bash
curl -fsSL https://dev.runsystems.ru/releases/static/install.sh | bash
```

Или с указанием версии:

```bash
curl -fsSL https://dev.runsystems.ru/releases/static/install.sh | bash -s -- --version 0.1.38
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

### Анализ (SonarQube)

```bash
# Консольный вывод
bsl-analyzer analyze -s ./my-project

# SARIF-отчёт
bsl-analyzer analyze -s ./my-project -r sarif -o ./reports

# JSONL-вывод (для SonarQube)
bsl-analyzer analyze -s ./my-project --format=jsonl > report.jsonl
```

### Интеграция с AI (MCP-сервер)

BSL Analyzer включает встроенный [MCP-сервер](https://modelcontextprotocol.io/) (Model Context Protocol), который позволяет AI-агентам (Claude, Cursor, Gemini, Codex) взаимодействовать с кодом 1С, справкой платформы, а также выполнять запросы и отладку.

Самый простой способ подключить инструменты к вашим AI-агентам — выполнить автоустановку:

```bash
# Устанавливает глобальную справку (reference) и проектный скоуп (workspace)
bsl-analyzer mcp install \
  --target all \
  --preset recommended \
  --source-dir ./my-project
```

Подробную информацию о ручном запуске, интеграции с 1С (HTTP-расширение), настройке семантического поиска и списках доступных инструментов читайте в документации:

- 📖 **[Инструкция по настройке MCP и профилей](docs/mcp/README.md)**
- 🛠️ **[Инструменты AI и расширение для 1С (query, execute, debug)](docs/mcp/TOOLS_AND_EXTENSION.md)**

## Конфигурация

Файл настройки проекта `bsl-analyzer.toml` располагается в корне проекта. Пример конфигурации диагностик:

```toml
[diagnostics]
skip = ["CommentedCode"]

[diagnostics.parameters.CyclomaticComplexity]
complexityThreshold = 20
```

> **Для продвинутых пользователей**: BSL Analyzer поддерживает централизованный распределённый индекс поиска на базе PostgreSQL, чтобы не индексировать крупные конфигурации локально. Подробнее об архитектуре и CLI-командах (`sync-pg`, `gc-pg`) читайте в разделе [Central Postgres Search](docs/central-postgres-search/).

## Сборка из исходников

**Требования:** Rust 1.91+

```bash
git clone https://github.com/itrous/bsl-analyzer.git
cd bsl-analyzer
cargo build --release
```

## Производительность

Сравнительный бенчмарк с [bsl-language-server](https://github.com/1c-syntax/bsl-language-server) на реальном проекте.

**Тестовый проект:** Управление торговлей 11.5.22.134 (12 519 BSL-файлов, 500 MB кода)  
**Конфигурация:** настройки по умолчанию, отключена только Typo. Без skipSupport.  
**Система:** AMD Ryzen 5 5600X (6 ядер / 12 потоков), 32 GB RAM, Linux 6.19, NVMe SSD

| Метрика | bsl-language-server 0.28.5 | bsl-analyzer 0.1.111 | Разница |
|---------|---------------------------|----------------------|---------|
| **Wall time** | 132.6s | 52s | 2.6x быстрее |
| **Peak RSS** | 4 566 MB | 1 553 MB | 2.9x меньше памяти |
| **Files/sec** | 94 | 263 | 2.8x пропускная способность |
| **Диагностик** | 552 111 | 610 477 | На 11% больше точных диагностик |

## Архитектура

```
bsl-analyzer (LSP-сервер)
    └── ide (API верхнего уровня)
        ├── ide-diagnostics (180+ диагностик)
        ├── ide-assists (Quick-fix действия)
        └── ide-db (Salsa — инкрементальные вычисления)
            └── hir (семантический анализ)
                └── syntax (CST, Rowan)
                    └── parser → lexer
```

Подробнее об архитектуре компилятора и пайплайнах читайте в [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md).

## Участие в разработке

Инструкции для разработчиков и правила приёма Pull Requests описаны в [CONTRIBUTING.md](CONTRIBUTING.md).

## Благодарности

Проект вдохновлён [BSL Language Server](https://github.com/1c-syntax/bsl-language-server) — инструментом статического анализа BSL на Java. Спасибо авторам за огромную работу по формализации диагностик и стандартов качества кода 1С.

- [bsl-language-server](https://github.com/1c-syntax/bsl-language-server) — статический анализатор BSL (Java, LGPL-3.0)
- [1c-syntax](https://github.com/1c-syntax) — сообщество разработчиков инструментов для 1С
- [RDT1C](https://github.com/Segate-ekb/1c_RDT) — «Инструменты разработчика» для 1С. Реализация интеграции с API 1С:Напарник (FIM completion, context/update) использована как референс для модуля `naparnik`.

## Лицензия

MIT или Apache-2.0, на выбор. См. [LICENSE-MIT](LICENSE-MIT) и [LICENSE-APACHE](LICENSE-APACHE).
