# Правила разработки

> См. также: [Политика версионирования](./VERSIONING.md)

## Общие правила

### 1. Изучение библиотек перед использованием

**Правило:** Прежде чем использовать любую внешнюю библиотеку (crate), необходимо изучить её актуальную документацию.

**Для AI-ассистентов (Claude Code):**
- Использовать плагин **Context7** для получения актуальной документации
- Команда: вызвать `resolve-library-id` для поиска библиотеки, затем `get-library-docs` для получения документации
- Пример: перед использованием `rowan`, `salsa`, `logos` и других crates — запросить актуальную документацию

**Для разработчиков:**
- Проверить документацию на docs.rs или официальном сайте библиотеки
- Обратить внимание на версию библиотеки и breaking changes
- Изучить примеры использования в проектах-источниках (см. [SOURCES.md](../planning/SOURCES.md))

**Обоснование:**
- API библиотек может меняться между версиями
- Документация в обучающих данных AI может быть устаревшей
- Избегаем ошибок типа "метод не найден" или "тип не существует"

---

### 2. Сверка с проектами-источниками

**Правило:** При реализации компонента сверяться с соответствующими проектами-источниками.

**Приоритет источников:**
1. **rust-analyzer** — архитектура и паттерны
2. **bsl-language-server** — совместимость (диагностики, конфигурация)
3. **bsl-parser** — грамматика BSL/SDBL
4. **tree-sitter-bsl** — приоритеты операторов, тесты
5. **bsl-language-server-rust** — готовые Rust компоненты

См. [SOURCES.md](../planning/SOURCES.md) для детального описания источников.

---

### 3. Тестирование

**Правило:** Каждый новый функционал должен быть покрыт тестами. Нельзя ломать существующие тесты.

**Типы тестов:**
- Unit-тесты для отдельных функций
- Integration-тесты для взаимодействия компонентов
- Snapshot-тесты для парсера и AST (expect-test)
- Адаптировать тесты из bsl-language-server где возможно

**Тестирование диагностик на BSL файлах:**

При тестировании диагностик **ОБЯЗАТЕЛЬНО** использовать вспомогательные методы для проверки позиций:

```rust
use crate::test_utils::assert_diagnostic_range_multiline;

#[test]
fn test_my_diagnostic() {
    let code = include_str!("../../test_data/MyDiagnostic.bsl");
    let diagnostics = check_diagnostic(code);

    // ✅ ПРАВИЛЬНО: используем assert_diagnostic_range_multiline
    assert_eq!(diagnostics.len(), 1);
    assert_diagnostic_range_multiline(&code, &diagnostics[0], 3, 0, 5, 13);
    // Проверяет что диагностика на строке 3, колонка 0 до строки 5, колонка 13
}
```

**Запрещено:**
```rust
// ❌ НЕПРАВИЛЬНО: ручная проверка TextRange
assert_eq!(diagnostics[0].range, TextRange::new(42.into(), 156.into()));
// Магические числа, сложно понять где именно ошибка в BSL файле
```

**Обоснование:**
- `assert_diagnostic_range_multiline` показывает читаемые номера строк/колонок
- При падении теста сразу понятно где проблема в BSL файле
- Легко сверить с Java тестами (которые тоже используют line/column)
- Избегаем магических чисел для byte offsets

**Доступные helper методы** (см. `crates/ide-diagnostics/src/test_utils.rs`):
- `assert_diagnostic_range_multiline(code, diag, start_line, start_col, end_line, end_col)`
- `assert_diagnostic_range(code, diag, line, start_col, end_col)` — для однострочных

**Если сломались существующие тесты:**

1. **Понять причину** — не игнорировать, не удалять тест
2. **Если тест устарел** (изменились требования, API, поведение):
   - Доработать тест под новое поведение
   - Добавить комментарий с обоснованием изменения
3. **Если тест корректен** (выявил регрессию):
   - Исправить код, не тест
   - Тест защищает от ошибок — это его работа

```bash
# Проверка всех тестов
cargo test --all

# Обновление snapshot-тестов (только после анализа!)
UPDATE_EXPECT=1 cargo test
```

**Запрещено:**
- Удалять тесты без обоснования
- Коммитить с падающими тестами
- Использовать `#[ignore]` без TODO с планом исправления
- Использовать магические числа для TextRange в тестах диагностик

---

### 4. Качество кода (clippy, rustfmt)

**Правило:** Код должен соответствовать best practices Rust и не иметь замечаний от анализатора.

**Требования:**
- Код не должен иметь warnings от `cargo clippy`
- Код должен быть отформатирован через `cargo fmt`
- Нельзя коммитить код с замечаниями анализатора

```bash
# Перед коммитом обязательно
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

**Допустимые исключения:**
- `#[allow(...)]` только с комментарием-обоснованием
- Временные `TODO` и `FIXME` в процессе разработки (убрать перед merge)

---

### 5. Инкрементальная разработка

**Правило:** Разрабатывать итеративно, проверяя компиляцию и тесты после каждого значимого изменения.

```bash
# После каждого изменения
cargo check
cargo clippy
cargo test
```

---

### 6. Независимость от внешних проектов

**Правило:** Проект должен быть полностью автономным и не зависеть от внешних проектов-источников во время сборки, тестирования и работы.

**Требования:**
- Все необходимые тестовые файлы должны быть скопированы в наш проект
- Пути к файлам должны быть относительными внутри проекта
- Запрещено использовать абсолютные пути к внешним проектам типа `/Users/.../bsl-parser/...`

**Структура тестовых данных:**
```
crates/
  parser/
    tests/
      fixtures/          # Тестовые BSL файлы
        Module.bsl
        simple.bsl
        ...
      integration_tests.rs
```

**Правильно:**
```rust
let input = include_str!("fixtures/Module.bsl");
// или
let path = "tests/fixtures/Module.bsl";
```

**Неправильно:**
```rust
let path = "/Users/kiriller/src/lsp/bsl-parser/src/test/resources/Module.bsl";
```

**Обоснование:**
- Проект должен собираться на любой машине без дополнительных зависимостей
- CI/CD должен работать без клонирования внешних проектов
- Упрощает воспроизведение результатов тестов

**Источники тестовых данных:**
- bsl-parser/src/test/resources/ — грамматика BSL/SDBL
- bsl-language-server/src/test/resources/ — реальные BSL файлы
- tree-sitter-bsl/test/corpus/ — тестовые случаи

Копировать с указанием источника в комментариях.

---

### 7. Единообразное логирование

**Правило:** Использовать только `tracing` ecosystem для логирования и профилирования. Запрещено использовать другие способы вывода отладочной информации.

**Требования:**
- Использовать только `tracing` macros: `trace!`, `debug!`, `info!`, `warn!`, `error!`
- Использовать spans для профилирования значимых операций
- Следовать паттернам из rust-analyzer

**Разрешено:**
```rust
use tracing::{trace, debug, info, warn, error};

// Логирование с полями (structured logging)
info!("parsing started");
debug!(file_id = ?file_id, "parsing file");
warn!(line = line_number, column = column, "syntax error");
error!("failed to parse");

// Spans для профилирования
pub fn parse_file(input: &str) -> Parse {
    let _span = tracing::info_span!("parse_file", len = input.len()).entered();
    // ... parsing logic ...
}
```

**Запрещено:**
```rust
// ❌ println! / eprintln! для отладки
println!("Debug: {:?}", value);
eprintln!("Error: {}", error);

// ❌ dbg! macro
dbg!(some_value);

// ❌ log crate
log::info!("message");

// ❌ Временный отладочный код
// TODO: remove debug print
println!("value = {:?}", value);
```

**Обоснование:**
- Единообразие кодовой базы
- Возможность управления уровнем логирования через `BSL_LOG`
- Профилирование через `BSL_PROFILE` без изменения кода
- Структурированное логирование с полями
- Нет необходимости удалять отладочный код

**Исключения:**
- `println!` в тестах допустимо (но лучше использовать assertions)
- `println!` в бинарниках для CLI вывода (не для отладки)

**Управление логированием:**
```bash
# Включить debug логи для парсера
BSL_LOG=parser=debug cargo run

# Включить все логи
BSL_LOG=trace cargo run

# Профилирование
BSL_PROFILE=* cargo run

# Запись в файл
BSL_LOG_FILE=/tmp/bsl.log BSL_LOG=debug cargo run
```

См. [ARCHITECTURE.md](../architecture/ARCHITECTURE.md#логирование) для детальной информации.

---

### 8. Self-documenting code (минимум комментариев)

**Правило:** Код должен быть самодокументируемым. Комментарии в коде нежелательны и допустимы только для описания сложной неочевидной логики.

**Философия:**
- Хороший код не нуждается в комментариях
- Имена функций, переменных и типов должны объяснять намерение
- Если нужен комментарий - возможно код нужно рефакторить

**Запрещено:**
```rust
// ❌ Комментарии, дублирующие код
// Парсим выражение
let expr = parse_expression(p);

// ❌ Устаревшие комментарии
// TODO: this needs refactoring (без даты, без issue)
// FIXME: broken (без объяснения что и почему)

// ❌ Закомментированный код
// let old_value = calculate_old_way();
// process(old_value);

// ❌ Очевидные комментарии
// Increment counter
counter += 1;

// ❌ Разделители и декоративные комментарии
// ==========================================
// Section: Helper Functions
// ==========================================
```

**Допустимо:**
```rust
// ✅ Объяснение сложной неочевидной логики
// BSL allows implicit conversions in ternary operator right operand:
// `x = ?(cond, value1, value2)` where value2 can have different type.
// This is legacy 1C behavior that we must preserve for compatibility.
if in_ternary_context && types_mismatch {
    allow_implicit_conversion();
}

// ✅ Пояснение магических чисел или алгоритма
// UTF-8 BOM: EF BB BF
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

// ✅ Ссылка на спецификацию или issue
// See: https://github.com/1c-syntax/bsl-language-server/issues/1234
// BSL preprocessor symbols are case-insensitive (legacy behavior)

// ✅ SAFETY комментарии для unsafe кода
// SAFETY: pointer is guaranteed valid by the caller contract
unsafe { *ptr }

// ✅ NOTE/WARNING для важных ограничений
// NOTE: This function assumes input is already validated.
// Calling with invalid input leads to panic.

// ✅ Doc comments (///) для публичного API
/// Parses a BSL source file into a syntax tree.
///
/// # Errors
/// Returns error if iteration limit exceeded.
pub fn parse(input: &str) -> Result<Parse> { ... }
```

**Альтернативы комментариям:**

Вместо комментариев используйте:

1. **Выразительные имена:**
   ```rust
   // ❌ До
   let x = t * 1000; // convert to milliseconds

   // ✅ После
   let duration_ms = duration_secs * 1000;
   ```

2. **Извлечение функций:**
   ```rust
   // ❌ До
   // Check if token is a keyword
   if matches!(token, Token::KwFunction | Token::KwProcedure | ...) {
       ...
   }

   // ✅ После
   if is_keyword(token) {
       ...
   }

   fn is_keyword(token: Token) -> bool {
       matches!(token, Token::KwFunction | Token::KwProcedure | ...)
   }
   ```

3. **Описательные типы:**
   ```rust
   // ❌ До
   fn process(offset: usize) -> usize { ... } // returns new offset

   // ✅ После
   struct ByteOffset(usize);
   fn process(offset: ByteOffset) -> ByteOffset { ... }
   ```

4. **Pattern matching вместо if-else с комментариями:**
   ```rust
   // ❌ До
   if state == 0 {
       // Initial state - waiting for input
   } else if state == 1 {
       // Processing state
   } else {
       // Error state
   }

   // ✅ После
   enum ParserState {
       WaitingForInput,
       Processing,
       Error,
   }

   match state {
       ParserState::WaitingForInput => { ... }
       ParserState::Processing => { ... }
       ParserState::Error => { ... }
   }
   ```

**Исключения:**

1. **Атрибуты с объяснением:**
   ```rust
   #[allow(clippy::too_many_arguments)]
   // Required for LSP protocol compatibility - matches TextDocumentPositionParams
   pub fn goto_definition(...) { ... }
   ```

2. **Временные маркеры (с обязательным контекстом):**
   ```rust
   // TODO(username): Implement after #123 is merged
   // FIXME(username): Temporary workaround for upstream bug rust-lang/rust#12345
   // HACK(username): Required for 1C:Enterprise 8.3.10 compatibility, remove in v2.0
   ```
   - Обязательно указывать автора и контекст
   - Желательно ссылка на issue
   - Обязательно план исправления

3. **Комментарии для источников тестовых данных:**
   ```rust
   // Source: bsl-parser/src/test/resources/Module.bsl
   let input = include_str!("fixtures/Module.bsl");
   ```

**Обоснование:**
- Комментарии быстро устаревают и вводят в заблуждение
- Self-documenting код улучшает читаемость
- Рефакторинг для ясности лучше комментария
- Doc comments (///) остаются для документации API
- Код должен говорить "что" и "как", комментарии - только "почему"

**Code review:**
- Каждый комментарий в PR должен быть обоснован
- Рецензент может попросить убрать комментарий и переписать код

---

### 9. Исправление инфраструктуры вместо костылей

**Правило:** При выборе между временным решением (workaround) и фундаментальным исправлением инфраструктуры всегда предпочитать исправление инфраструктуры, даже если это требует значительных доработок.

**Философия:**
- Технический долг накапливается быстрее, чем кажется
- Временные решения становятся постоянными
- Правильная архитектура окупается в долгосрочной перспективе
- Один раз исправить инфраструктуру лучше, чем годами обходить проблему

**Примеры:**

**Сценарий 1: Проблема с кэшированием**
```rust
// ❌ НЕПРАВИЛЬНО: Костыль
// HACK: Clear cache manually because Salsa doesn't track this dependency
fn update_module(db: &mut dyn Db, module: ModuleId) {
    cache.clear(); // Очищаем весь кэш вручную
    process_module(db, module);
}
```

```rust
// ✅ ПРАВИЛЬНО: Исправление инфраструктуры
// 1. Добавить query в Salsa для отслеживания зависимостей
#[salsa::tracked]
fn module_dependencies(db: &dyn Db, module: ModuleId) -> Vec<ModuleId> {
    // Salsa автоматически отслеживает изменения
}

// 2. Использовать query вместо ручного управления
fn update_module(db: &dyn Db, module: ModuleId) {
    // Salsa автоматически инвалидирует зависимые результаты
    process_module(db, module);
}
```

**Обоснование:** Да, добавление Salsa query требует больше работы (изменение трейтов, миграция тестов), но это решает проблему навсегда для всех модулей.

---

**Сценарий 2: Неудобный API**
```rust
// ❌ НЕПРАВИЛЬНО: Обходное решение
// WORKAROUND: Parse text manually because parser doesn't expose this
fn get_function_name(node: &SyntaxNode) -> Option<String> {
    let text = node.text().to_string();
    // Regex parsing of BSL syntax
    let re = Regex::new(r"Функция\s+(\w+)").unwrap();
    re.captures(&text)?.get(1).map(|m| m.as_str().to_string())
}
```

```rust
// ✅ ПРАВИЛЬНО: Расширение AST API
// 1. Добавить typed accessor в syntax/ast.rs
impl ast::FunctionDecl {
    pub fn name(&self) -> Option<ast::Name> {
        support::child(&self.syntax)
    }
}

// 2. Использовать типизированный API
fn get_function_name(node: &SyntaxNode) -> Option<String> {
    ast::FunctionDecl::cast(node)?
        .name()?
        .text()
        .to_string()
        .into()
}
```

**Обоснование:** Добавление метода в AST требует времени, но улучшает API для всех будущих пользователей.

---

**Сценарий 3: Ограничение архитектуры**
```rust
// ❌ НЕПРАВИЛЬНО: Глобальное состояние
// HACK: Use global state because we can't pass context here
static mut DIAGNOSTIC_CONFIG: Option<DiagnosticConfig> = None;

fn check_diagnostic(node: &SyntaxNode) -> Vec<Diagnostic> {
    let config = unsafe { DIAGNOSTIC_CONFIG.as_ref().unwrap() };
    // ...
}
```

```rust
// ✅ ПРАВИЛЬНО: Рефакторинг архитектуры
// 1. Изменить сигнатуру, добавив DiagnosticContext
pub struct DiagnosticContext<'a> {
    config: &'a DiagnosticConfig,
    db: &'a dyn Db,
}

// 2. Передавать контекст явно
fn check_diagnostic(ctx: &DiagnosticContext, node: &SyntaxNode) -> Vec<Diagnostic> {
    let config = ctx.config;
    // ...
}

// 3. Обновить все вызовы (да, это много работы)
impl Diagnostic {
    pub fn check(ctx: &DiagnosticContext) -> Vec<Diagnostic> {
        check_diagnostic(ctx, root_node)
    }
}
```

**Обоснование:** Рефакторинг 181 диагностики для добавления контекста - большая работа, но это делает код тестируемым, безопасным и расширяемым.

---

**Когда допустимы временные решения:**

Временное решение допустимо **только** если выполнены **все** условия:

1. **Критическая блокировка:** Фундаментальное исправление блокирует релиз/критический функционал
2. **Чёткий план исправления:** Есть задача (issue) с описанием правильного решения
3. **Временные рамки:** Определён срок удаления workaround (например, "убрать в Iteration 15")
4. **Явная пометка в коде:**
   ```rust
   // TEMPORARY WORKAROUND: Issue #123
   // TODO: Remove after Salsa 0.25.2 migration (Iteration 10)
   // This hack exists because current VFS doesn't track directory changes.
   // Proper fix: implement DirectoryWatcher in VFS (see SALSA_TODO.md)
   ```
5. **Изолированность:** Workaround не распространяется на другие компоненты
6. **Обсуждение в PR:** Ревьюер согласен с временным решением

**Запрещено:**
- Писать "временный" код без плана исправления
- Использовать `unsafe`, глобальное состояние или другие опасные паттерны для обхода архитектурных ограничений
- Копировать workaround в несколько мест ("если уж делаем костыль, то один раз")
- Оставлять TODO без issue и срока

---

**Процесс принятия решения:**

1. **Оценить масштаб проблемы:**
   - Сколько мест затронет workaround?
   - Сколько раз придётся его копировать?
   - Какой технический долг это создаст?

2. **Оценить стоимость правильного решения:**
   - Сколько времени займёт исправление инфраструктуры?
   - Какие компоненты нужно изменить?
   - Есть ли риски сломать существующий код?

3. **Правило большого пальца:**
   - Если workaround нужен в > 3 местах → исправлять инфраструктуру
   - Если workaround использует `unsafe`/глобальное состояние → исправлять инфраструктуру
   - Если правильное решение займёт < 1 дня → исправлять инфраструктуру
   - Если сомневаешься → спросить в PR review

4. **В случае сомнений:**
   - Использовать AskUserQuestion для обсуждения подхода
   - Написать оба варианта в комментарии к PR
   - Предпочесть качество скорости

**Обоснование:**
- Мы строим production-ready инструмент, не прототип
- Целевая аудитория - большие проекты (100K+ файлов, 4GB+ кода)
- Технический долг в инфраструктуре блокирует будущие улучшения
- Правильная архитектура критична для performance (Salsa, incremental analysis)
- Один раз потратить 2 дня на рефакторинг лучше, чем годами терять 10 минут на каждое новое изменение

**Примеры из истории проекта:**
- ✅ **Salsa 0.17 → 0.25.2:** Отложили на Iteration 10 вместо костылей с ручным кэшированием
- ✅ **DiagnosticContext:** Рефакторинг всех диагностик вместо глобальной конфигурации
- ✅ **ModuleGraph:** Полноценная инфраструктура для инкрементальности вместо "просто пропускать файлы"

---

## Специфичные правила

### Работа с Rowan (CST/AST)

- Изучить документацию rowan перед работой с syntax trees
- Использовать паттерны из rust-analyzer/crates/syntax/

### Работа с Salsa (инкрементальный анализ)

- Изучить документацию salsa (версия 0.17 или актуальная)
- Использовать паттерны из rust-analyzer/crates/base-db/

### Работа с logos (лексер)

- Изучить документацию logos
- Учитывать case-insensitivity для BSL ключевых слов
