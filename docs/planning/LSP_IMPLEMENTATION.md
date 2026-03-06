# LSP Implementation Plan

## Обзор

Реализация Language Server Protocol для полной совместимости с bsl-language-server.

## Поддерживаемые возможности (по приоритету)

### P0: Критические (Iteration 26)

| Capability | Описание | Сложность |
|------------|----------|-----------|
| `initialize` | Инициализация сервера | Medium |
| `textDocument/didOpen` | Открытие документа | Low |
| `textDocument/didChange` | Изменение документа | Medium |
| `textDocument/didClose` | Закрытие документа | Low |
| `textDocument/didSave` | Сохранение документа | Low |
| `textDocument/publishDiagnostics` | Публикация диагностик | High |

### P1: Важные (Iteration 27)

| Capability | Описание | Сложность |
|------------|----------|-----------|
| `textDocument/definition` | Переход к определению | High |
| `textDocument/references` | Поиск использований | High |
| `textDocument/documentSymbol` | Символы документа | Medium |
| `textDocument/hover` | Подсказки при наведении | Medium |

### P2: Стандартные (Iteration 28)

| Capability | Описание | Сложность |
|------------|----------|-----------|
| `workspace/symbol` | Поиск символов | Medium |
| `textDocument/codeAction` | Code Actions | High |
| `codeAction/resolve` | Разрешение Code Action | Medium |
| `textDocument/codeLens` | Code Lens | Medium |

### P3: Продвинутые (Iteration 29)

| Capability | Описание | Сложность |
|------------|----------|-----------|
| `textDocument/formatting` | Форматирование | High |
| `textDocument/rangeFormatting` | Форматирование диапазона | Medium |
| `textDocument/rename` | Переименование | High |
| `textDocument/foldingRange` | Сворачивание | Low |

### P4: Дополнительные (Iteration 30)

| Capability | Описание | Сложность |
|------------|----------|-----------|
| `textDocument/semanticTokens` | Семантические токены | High |
| `textDocument/inlayHint` | Встроенные подсказки | Medium |
| `textDocument/documentColor` | Цвета в документе | Low |
| `textDocument/selectionRange` | Расширение выделения | Low |
| `callHierarchy/*` | Иерархия вызовов | High |
| `textDocument/documentLink` | Ссылки в документе | Low |

---

## Структура LSP сервера

```rust
// crates/bsl-analyzer/src/main.rs
use lsp_server::{Connection, Message};

fn main() -> anyhow::Result<()> {
    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        // ...
    })?;

    let init_params = connection.initialize(server_capabilities)?;
    main_loop(connection, init_params)?;
    io_threads.join()?;
    Ok(())
}
```

### Обработка запросов

```rust
// crates/bsl-analyzer/src/dispatch.rs
pub fn dispatch(
    req: lsp_server::Request,
    global_state: &mut GlobalState,
) -> Option<lsp_server::Response> {
    RequestDispatcher::new(req, global_state)
        .on::<lsp_types::request::GotoDefinition>(handlers::handle_goto_definition)?
        .on::<lsp_types::request::References>(handlers::handle_references)?
        .on::<lsp_types::request::DocumentSymbolRequest>(handlers::handle_document_symbol)?
        .on::<lsp_types::request::HoverRequest>(handlers::handle_hover)?
        .on::<lsp_types::request::CodeActionRequest>(handlers::handle_code_action)?
        .finish()
}
```

### Handlers

```rust
// crates/bsl-analyzer/src/handlers/
pub fn handle_goto_definition(
    snap: GlobalStateSnapshot,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let file_id = snap.url_to_file_id(&params.text_document.uri)?;
    let position = snap.offset(file_id, params.position)?;

    let nav_info = snap.analysis.goto_definition(file_id, position)?;

    Ok(nav_info.map(|nav| to_lsp_location(&nav)))
}
```

---

## Конфигурация

### Формат .bsl-analyzer.json (совместимость)

```json
{
    "$schema": "https://raw.githubusercontent.com/1c-syntax/bsl-language-server/master/docs/configuration/schema.json",
    "diagnostics": {
        "parameters": {
            "CyclomaticComplexity": {
                "complexityThreshold": 20
            }
        },
        "skip": [
            "CommentedCode"
        ]
    },
    "codeLens": {
        "showCognitiveComplexity": true,
        "showCyclomaticComplexity": true
    }
}
```

### Rust структуры

```rust
#[derive(Debug, Deserialize)]
pub struct LanguageServerConfiguration {
    #[serde(default)]
    pub diagnostics: DiagnosticsConfiguration,
    #[serde(default)]
    pub code_lens: CodeLensConfiguration,
    #[serde(default)]
    pub formatting: FormattingConfiguration,
}

#[derive(Debug, Default, Deserialize)]
pub struct DiagnosticsConfiguration {
    #[serde(default)]
    pub parameters: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub skip: Vec<String>,
}
```

---

## Тестирование LSP

### Unit тесты обработчиков

```rust
#[test]
fn test_goto_definition() {
    let (snap, file_id) = fixture(r#"
//- /main.bsl
Процедура Тест()
    МояФункция();  // <- cursor here
КонецПроцедуры

Функция МояФункция()
    Возврат 42;
КонецФункции
"#);

    let position = Position { line: 1, character: 4 };
    let result = handle_goto_definition(snap, file_id, position);

    expect![[r#"
        Location {
            uri: file:///main.bsl,
            range: Range { start: Position { line: 5, character: 0 }, end: ... }
        }
    "#]].assert_debug_eq(&result);
}
```

### Integration тесты

```rust
#[test]
fn test_full_lsp_session() {
    let server = TestServer::new();

    // Initialize
    let init_result = server.initialize(InitializeParams::default());
    assert!(init_result.capabilities.definition_provider.is_some());

    // Open document
    server.open_document("file:///test.bsl", "Процедура Тест() КонецПроцедуры");

    // Check diagnostics
    let diags = server.wait_for_diagnostics("file:///test.bsl");
    assert!(diags.is_empty());
}
```

---

## CLI интерфейс

```bash
# Запуск LSP сервера
bsl-analyzer

# Анализ проекта (режим SonarQube)
bsl-analyzer analyze --project ./my-project --output report.json

# Проверка конфигурации
bsl-analyzer check-config --config .bsl-analyzer.json

# Версия
bsl-analyzer --version
```

```rust
// crates/bsl-analyzer/src/cli.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bsl-analyzer")]
#[command(about = "BSL Language Server and Analyzer")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run static analysis
    Analyze {
        #[arg(short, long)]
        project: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(short, long, default_value = "json")]
        format: OutputFormat,
    },
    /// Check configuration file
    CheckConfig {
        #[arg(short, long)]
        config: PathBuf,
    },
}
```

---

## Совместимость с bsl-language-server

### Идентичные коды диагностик

```rust
// Используем те же коды что и bsl-language-server
pub enum DiagnosticCode {
    CanonicalSpellingKeywords,      // Те же названия
    ConsecutiveEmptyLines,
    CyclomaticComplexity,
    // ... все 181
}

impl DiagnosticCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CanonicalSpellingKeywords => "CanonicalSpellingKeywords",
            // ...
        }
    }
}
```

### Идентичный формат отчётов

Поддержка тех же репортеров:
- JSON
- JUnit
- TSLint
- Generic Issue (SonarQube)
- SARIF

---

## Трекинг прогресса LSP

| Категория | Capabilities | Готово | Прогресс |
|-----------|--------------|--------|----------|
| Sync | 4 | 0 | 0% |
| Diagnostics | 2 | 0 | 0% |
| Navigation | 4 | 0 | 0% |
| Code Actions | 3 | 0 | 0% |
| Formatting | 2 | 0 | 0% |
| Advanced | 6 | 0 | 0% |
| **Итого** | **21** | **0** | **0%** |
