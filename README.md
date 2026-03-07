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

## Лицензия

MIT или Apache-2.0, на выбор. См. [LICENSE-MIT](LICENSE-MIT) и [LICENSE-APACHE](LICENSE-APACHE).
