# Reference Sources

Этот документ описывает проекты-источники информации для разработки bsl-analyzer.

> **Важно:** Перед использованием любой библиотеки изучите её актуальную документацию.
> См. [Правила разработки](../contributing/DEVELOPMENT_RULES.md).

## Проекты

### 1. rust-analyzer

**Путь:** `/Users/kiriller/src/lsp/rust-analyzer/`
**Роль:** Образец архитектуры

Rust Language Server от команды rust-lang. Используем как образец для:

- Архитектуры слоёв (Lexer → Parser → Syntax → HIR → IDE)
- Паттернов кода (Event-based parser, Marker pattern, InFile<T>)
- Инфраструктуры тестирования (test-fixture, expect-test)
- Инкрементального анализа (Salsa)
- CST/AST на базе Rowan

**Ключевые файлы:**

- `crates/parser/` - event-based парсер
- `crates/syntax/` - Rowan интеграция
- `crates/base-db/` - Salsa database
- `crates/ide-diagnostics/` - структура диагностик

---

### 2. bsl-language-server (Java)

**Путь:** `/Users/kiriller/src/lsp/bsl-language-server/`
**Роль:** Целевая совместимость

Java Language Server для BSL. Обеспечиваем 100% совместимость с:

- 181 диагностикой (коды, сообщения, severity)
- Конфигурацией (.bslls.json)
- LSP capabilities
- Форматом отчётов (JSON, SARIF, Generic Issue)

**Ключевые файлы:**

- `src/main/java/.../diagnostics/` - 181 диагностика
- `src/test/resources/diagnostics/` - тестовые данные
- `docs/diagnostics/` - документация диагностик
- `src/main/java/.../configuration/` - формат конфигурации

**Что брать:**

- Список диагностик и их параметры
- Тестовые данные для миграции тестов
- Документацию для совместимости

---

### 3. bsl-parser (Java/ANTLR4)

**Путь:** `/Users/kiriller/src/lsp/bsl-parser/`
**Роль:** Грамматика языка

ANTLR4 парсер BSL и SDBL. Референс для:

- Полной грамматики BSL (BSLParser.g4)
- Грамматики SDBL запросов (SDBLParser.g4)
- Токенов и ключевых слов (BSLLexer.g4, SDBLLexer.g4)
- Описаний методов (BSLMethodDescriptionParser.g4)

**Ключевые файлы:**

- `src/main/antlr/BSLLexer.g4` - токены BSL (40+ ключевых слов)
- `src/main/antlr/BSLParser.g4` - грамматика BSL
- `src/main/antlr/SDBLLexer.g4` - токены SDBL
- `src/main/antlr/SDBLParser.g4` - грамматика SDBL (SELECT, JOIN, виртуальные таблицы)
- `src/main/antlr/BSLMethodDescriptionParser.g4` - описания методов

**Что брать:**

- Список всех токенов и ключевых слов
- Правила грамматики для реализации парсера
- Особенности SDBL (виртуальные таблицы, типы метаданных)
- Режимы лексера (PREPROCESSOR_MODE, ANNOTATION_MODE, etc.)

---

### 4. salsa

**Путь:** `/Users/kiriller/src/lsp/salsa/`
**Роль:** Фреймворк инкрементальных вычислений

Фреймворк для инкрементальных вычислений от команды rust-analyzer. Используем для:

- Автоматической инвалидации кеша при изменениях
- Ленивых вычислений с кешированием
- Управления зависимостями между queries
- Параллельных вычислений
- **Критично для метаданных** — позволяет не перечитывать зависимости при каждом запросе

**Ключевые компоненты:**

- `#[salsa::input]` — входные данные (file_text, metadata files)
- `#[salsa::tracked]` — производные запросы с автоматическим кешированием
- `Durability` — контроль "долговечности" данных (HIGH для метаданных, LOW для исходников)
- `LRU` — автоматическое вытеснение старых результатов из кеша

**Зачем нужна для метаданных:**
Метаданные 1С (конфигурация, общие модули, роли и т.д.) — это идеальный use case для Salsa:

1. **Редко меняются** — можно пометить как `Durability::HIGH`
2. **Дорого загружать** — парсинг XML конфигурации, чтение файловой системы
3. **Часто используются** — каждая диагностика может обращаться к метаданным
4. **Имеют зависимости** — модули ссылаются на общие модули, роли на объекты метаданных

Пример:

```rust
// Input — изменяется только при изменении файлов конфигурации
#[salsa::input]
struct ConfigurationFiles {
    #[returns(as_ref)]
    root_path: PathBuf,
}

// Derived — автоматически пересчитывается только при изменении ConfigurationFiles
#[salsa::tracked(lru = 16)]
fn load_configuration(db: &dyn Db) -> Arc<Configuration> {
    let path = db.configuration_files().root_path(db);
    loader::load_from_directory(&path).unwrap()
}

// Другие queries автоматически зависят от load_configuration
#[salsa::tracked]
fn find_common_module(db: &dyn Db, name: &str) -> Option<Arc<CommonModule>> {
    db.load_configuration()  // Salsa отследит зависимость
        .common_modules()
        .find(|m| m.name() == name)
}
```

**Ключевые файлы:**

- `src/lib.rs` — основные макросы (#[salsa::input], #[salsa::tracked])
- `book/` — документация (mdBook)
- `tests/` — примеры использования
- `examples/` — hello-world примеры
- `CHANGELOG.md` — история изменений (текущая версия 0.25.2)

**Текущий статус интеграции:**

- ⚠️ Частично интегрирована в Iteration 5 через упрощенный подход (DashMap)
- 📋 Полная интеграция отложена до будущих итераций (см. `SALSA_TODO.md`)
- ✅ Критична для Metadata Infrastructure (Iteration 10+)

---

## Матрица использования по итерациям

| Итерация | Задача                  | Источники                                                                                       |
| -------- | ----------------------- | ----------------------------------------------------------------------------------------------- |
| 1        | Лексер                  | bsl-parser (BSLLexer.g4)                                                                        |
| 2-3      | Парсер                  | bsl-parser (BSLParser.g4), rust-analyzer (parser/grammar/), tree-sitter-bsl (приоритеты, тесты) |
| 3        | SDBL                    | bsl-parser (SDBLParser.g4, SDBLLexer.g4) — tree-sitter-bsl не содержит SDBL                     |
| 4        | Syntax/Rowan            | rust-analyzer (syntax/)                                                                         |
| 5        | Base-DB/Salsa           | rust-analyzer (base-db/), salsa (src/, book/, tests/)                                           |
| 6-9      | HIR/Symbols             | rust-analyzer (hir/)                                                                            |
| 10       | IDE-DB                  | rust-analyzer (ide-db/), salsa (полная интеграция)                                              |
| 10+      | Metadata Infrastructure | bsl-language-server (mdo/), salsa                                                               |
| 11-13    | Tier 1 диагностики      | bsl-language-server (diagnostics/)                                                              |
| 14-18    | Tier 2 диагностики      | bsl-language-server                                                                             |
| 19-23    | Tier 3 диагностики      | bsl-language-server                                                                             |
| 24-25    | SDBL диагностики        | bsl-language-server, bsl-parser (SDBL\*.g4)                                                     |
| 26-30    | LSP сервер              | rust-analyzer                                                                                   |

---

## Приоритет источников по компоненту

### Лексер

1. **bsl-parser/BSLLexer.g4** - полный список токенов
2. Column1. **tree-sitter-bsl/grammar.js** - альтернативный список токенов
3. **rust-analyzer** - архитектура (logos)

### Парсер

1. **bsl-parser/BSLParser.g4** - полная грамматика
2. **rust-analyzer/parser/** - event-based подход

### SDBL

1. **bsl-parser/SDBLParser.g4** - полная грамматика запросов
2. **bsl-parser/SDBLLexer.g4** - токены SDBL

### Диагностики

1. **bsl-language-server/diagnostics/** - референс реализации (181)
2. **bsl-language-server/test/resources/diagnostics/** - тестовые данные

### Salsa (Инкрементальные вычисления)

1. **salsa/src/** - исходный код библиотеки
2. **salsa/book/** - официальная документация (mdBook)
3. **rust-analyzer/base-db/** - примеры реального использования
4. **salsa/tests/** - тесты и примеры API

### Метаданные

1. **bsl-language-server** - mdclasses интеграция (Java референс)
2. **salsa** - для кеширования загрузки метаданных

### LSP

1. **bsl-language-server** - capabilities, handlers
2. **rust-analyzer** - архитектура сервера

---

## Статистика проектов

| Проект              | Язык       | Диагностик | LOC   | Особенности                          |
| ------------------- | ---------- | ---------- | ----- | ------------------------------------ |
| rust-analyzer       | Rust       | 54         | 1.5M+ | Образец архитектуры                  |
| bsl-language-server | Java       | 181        | 50K+  | Целевая совместимость                |
| bsl-parser          | Java/ANTLR | -          | 15K+  | Полная грамматика BSL+SDBL           |
| tree-sitter-bsl     | JS/C       | -          | 2K+   | Грамматика BSL (без SDBL)            |
| salsa               | Rust       | -          | 50K+  | Инкрементальные вычисления (v0.25.2) |
