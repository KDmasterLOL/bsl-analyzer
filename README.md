# BSL Analyzer

Высокопроизводительный Language Server для языка BSL (1С:Предприятие), написанный на Rust.

## Возможности

- **180+ диагностик** качества кода BSL
- **LSP** — поддержка Language Server Protocol для IDE
- **SonarQube** — отчёты SARIF, потоковый режим для крупных проектов
- **Совместимость** с форматом конфигурации `.bsl-language-server.json`
- **Кроссплатформенность** — Linux, Windows, macOS (Apple Silicon)

## Установка

### Linux

```bash
curl -fsSL https://github.com/itrous/bsl-analyzer/releases/latest/download/bsl-launcher-linux-amd64 -o ~/.local/bin/bsl-analyzer && chmod +x ~/.local/bin/bsl-analyzer
```

### Windows (PowerShell)

```powershell
Invoke-WebRequest "https://github.com/itrous/bsl-analyzer/releases/latest/download/bsl-launcher-windows-amd64.exe" -OutFile bsl-analyzer.exe
```

### macOS (Apple Silicon)

```bash
curl -fsSL https://github.com/itrous/bsl-analyzer/releases/latest/download/bsl-launcher-darwin-arm64 -o /usr/local/bin/bsl-analyzer && chmod +x /usr/local/bin/bsl-analyzer
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

# Потоковый режим (низкое потребление памяти)
bsl-analyzer analyze -s ./my-project --streaming --format=jsonl > report.jsonl
```

### Фиксация версии (CI/CD)

```bash
BSL_ANALYZER_VERSION=0.1.33 bsl-analyzer analyze -s ./src
```

## Конфигурация

Файл `.bsl-analyzer.json` (или `.bsl-language-server.json`):

```json
{
    "diagnostics": {
        "skip": ["CommentedCode"],
        "parameters": {
            "CyclomaticComplexity": {
                "complexityThreshold": 20
            }
        }
    }
}
```

## Сборка из исходников

**Требования:** Rust 1.91+

```bash
git clone https://github.com/itrous/bsl-analyzer.git
cd bsl-analyzer
cargo build --release
```

## Производительность

Сравнительный бенчмарк с [bsl-language-server](https://github.com/1c-syntax/bsl-language-server) на реальном проекте.

**Тестовый проект:** Управление торговлей 11.5.22.134 (12 578 BSL-файлов, 500 MB кода)

**Конфигурация:** настройки по умолчанию, отключена только Typo. Без skipSupport.

**Система:** AMD Ryzen 5 5600X (6 ядер / 12 потоков), 32 GB RAM, Linux 6.19, NVMe SSD

**Методика:** `/usr/bin/time -v` (GNU time), холодный запуск без кэша. bsl-analyzer в streaming-режиме (`--streaming`), bsl-language-server в стандартном режиме. Оба инструмента используют все доступные ядра.

| Метрика | bsl-language-server 0.28.5 | bsl-analyzer 0.1.35 | Разница |
|---------|---------------------------|---------------------|---------|
| **Wall time** | 137.3s | 90.0s | 1.5x быстрее |
| **CPU time** | 1428.6s | 641.7s | 2.2x меньше CPU |
| **Peak RSS** | 4 576 MB | 1 394 MB | 3.3x меньше памяти |
| **System time** | 8.85s | 1.67s | 5.3x меньше I/O |
| **Files/sec** | 91 | 139 | 1.5x пропускная способность |
| **Диагностик** | 552 111 | 724 961 | — |

**О разнице в количестве диагностик.** bsl-analyzer нашёл на 31% больше срабатываний. Это объясняется несколькими факторами:

- Ряд SDBL-диагностик (запросы к базе данных) в bsl-analyzer работают точечно — выделяют каждое проблемное поле или выражение в запросе отдельной диагностикой, тогда как bsl-language-server отмечает запрос целиком одним предупреждением.
- Возможны ложные срабатывания — проект молодой, качество детекции активно улучшается.
- Некоторые диагностики реализованы с отличиями в логике — это штатная ситуация для независимой реализации.

Мы работаем над повышением точности диагностик и уменьшением числа ложных срабатываний.

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

Подробнее: [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md)

## Участие в разработке

См. [CONTRIBUTING.md](CONTRIBUTING.md).

## Благодарности

Проект вдохновлён [BSL Language Server](https://github.com/1c-syntax/bsl-language-server) — инструментом статического анализа BSL на Java. Спасибо авторам за огромную работу по формализации диагностик и стандартов качества кода 1С.

- [bsl-language-server](https://github.com/1c-syntax/bsl-language-server) — статический анализатор BSL (Java, LGPL-3.0)
- [1c-syntax](https://github.com/1c-syntax) — сообщество разработчиков инструментов для 1С

## Лицензия

MIT или Apache-2.0, на выбор. См. [LICENSE-MIT](LICENSE-MIT) и [LICENSE-APACHE](LICENSE-APACHE).
