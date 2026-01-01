# Правила разработки

> См. также: [Политика версионирования](./VERSIONING.md)

## Критичные правила (обязательно для всех)

### 1. Изучение библиотек перед использованием

**Перед использованием любого crate:**
- **AI (Claude Code):** Использовать Context7 → `resolve-library-id` + `query-docs`
- **Люди:** Проверить docs.rs и примеры в проектах-источниках ([SOURCES.md](../planning/SOURCES.md))

**Почему:** API меняются между версиями, документация в обучающих данных может быть устаревшей.

---

### 2. Сверка с проектами-источниками

**Приоритет источников:**
1. **rust-analyzer** — архитектура, паттерны (Rowan, Salsa)
2. **bsl-language-server** — совместимость (диагностики, конфигурация, метаданные)
3. **bsl-parser** — грамматика BSL/SDBL
4. **tree-sitter-bsl** — приоритеты операторов, тесты
5. **bsl-language-server-rust** — готовые Rust компоненты

Детали: [SOURCES.md](../planning/SOURCES.md)

---

### 3. Тестирование

**Требования:**
- Каждый новый функционал покрыт тестами
- Нельзя ломать существующие тесты без обоснования
- Unit / Integration / Snapshot (expect-test)

**Тестирование диагностик - ОБЯЗАТЕЛЬНО:**
```rust
// ✅ ПРАВИЛЬНО: используем helper методы
assert_diagnostic_range_multiline(&code, &diagnostics[0], 3, 0, 5, 13);

// ❌ ЗАПРЕЩЕНО: магические числа для TextRange
assert_eq!(diagnostics[0].range, TextRange::new(42.into(), 156.into()));
```

**Доступные helpers** (`crates/ide-diagnostics/src/test_utils.rs`):
- `assert_diagnostic_range_multiline(code, diag, start_line, start_col, end_line, end_col)`
- `assert_diagnostic_range(code, diag, line, start_col, end_col)` — однострочные

**Если тест сломался:**
1. Понять причину (не удалять тест!)
2. Если устарел — обновить с комментарием-обоснованием
3. Если выявил регрессию — исправить код, не тест

```bash
cargo test --all                    # Все тесты
UPDATE_EXPECT=1 cargo test         # Обновить snapshots (только после анализа!)
```

**Запрещено:**
- Удалять тесты без обоснования
- Коммитить с падающими тестами
- `#[ignore]` без TODO с планом
- Магические числа для TextRange

---

### 4. Качество кода

```bash
# Обязательно перед коммитом
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

**Допустимо:** `#[allow(...)]` только с комментарием-обоснованием.

---

### 5. Независимость от внешних проектов

**Все тестовые файлы копируются в наш проект:**

```rust
// ✅ ПРАВИЛЬНО
let input = include_str!("fixtures/Module.bsl");

// ❌ ЗАПРЕЩЕНО
let path = "/Users/.../bsl-parser/src/test/resources/Module.bsl";
```

**Структура:**
```
crates/parser/tests/fixtures/  # BSL файлы с указанием источника в комментариях
```

**Источники:** bsl-parser, bsl-language-server, tree-sitter-bsl (см. комментарии в файлах).

---

### 6. Логирование: только tracing

**ВСЕГДА используйте:**
```rust
use tracing::{trace, debug, info, warn, error};

info!("parsing started");
debug!(file_id = ?file_id, "parsing file");

// Spans для профилирования
pub fn parse_file(input: &str) -> Parse {
    let _span = tracing::info_span!("parse_file", len = input.len()).entered();
    // ... logic
}
```

**ЗАПРЕЩЕНО:**
```rust
println!("Debug: {:?}", value);  // ❌
eprintln!("Error: {}", error);   // ❌
dbg!(value);                     // ❌
log::info!("message");           // ❌
```

**Исключения:** `println!` только для CLI вывода в бинарниках (не для отладки).

**Управление:**
```bash
BSL_LOG=parser=debug cargo run      # Debug логи для модуля
BSL_LOG=trace cargo run             # Все логи
BSL_PROFILE=* cargo run             # Профилирование
BSL_LOG_FILE=/tmp/bsl.log cargo run # Запись в файл
```

---

### 7. Self-documenting code (минимум комментариев)

**Философия:** Код должен объяснять себя сам. Комментарии — только для "почему", не "что".

**Запрещено:**
```rust
// ❌ Дублирование кода
// Парсим выражение
let expr = parse_expression(p);

// ❌ Устаревшие TODO без контекста
// TODO: fix this

// ❌ Закомментированный код
// let old = calculate();

// ❌ Очевидное
counter += 1; // Increment counter

// ❌ Декоративное
// ==========================================
```

**Допустимо:**
```rust
// ✅ Неочевидная логика + ссылка на спецификацию
// BSL allows implicit conversions in ternary (legacy 1C behavior)
// See: https://github.com/1c-syntax/bsl-language-server/issues/1234

// ✅ SAFETY для unsafe
// SAFETY: pointer guaranteed valid by caller

// ✅ Doc comments для публичного API
/// Parses BSL source into syntax tree.
pub fn parse(input: &str) -> Parse { ... }

// ✅ TODO с автором и планом
// TODO(username): Implement after #123 merged (Iteration 10)

// ✅ Источник тестовых данных
// Source: bsl-parser/src/test/resources/Module.bsl
```

**Альтернативы комментариям:**
- Выразительные имена (`duration_ms` вместо `x`)
- Извлечение функций (`is_keyword(token)`)
- Описательные типы (`ByteOffset(usize)`)
- Enum вместо if-else с комментариями

---

### 8. Исправление инфраструктуры вместо костылей

**Правило:** Всегда предпочитать фундаментальное исправление временному решению.

**Временное решение допустимо ТОЛЬКО если выполнены ВСЕ условия:**
1. Критическая блокировка релиза
2. Есть issue с планом правильного решения
3. Определён срок удаления workaround
4. Явная пометка:
   ```rust
   // TEMPORARY WORKAROUND: Issue #123
   // TODO: Remove in Iteration 10 after Salsa migration
   // Proper fix: implement DirectoryWatcher (see SALSA_TODO.md)
   ```
5. Изолирован (не копируется в несколько мест)
6. Согласован с ревьюером в PR

**Запрещено:**
- "Временный" код без плана исправления
- `unsafe`/глобальное состояние для обхода архитектуры
- TODO без issue и срока

**Правило большого пальца:**
- Workaround в > 3 местах → исправлять инфраструктуру
- Workaround использует `unsafe` → исправлять инфраструктуру
- Правильное решение < 1 дня → исправлять инфраструктуру
- Сомневаешься → спросить в PR

**Примеры:**
```rust
// ❌ Костыль: ручная очистка кэша
cache.clear();

// ✅ Правильно: добавить Salsa query
#[salsa::tracked]
fn module_dependencies(db: &dyn Db, module: ModuleId) -> Vec<ModuleId> { ... }
```

**Обоснование:** Мы строим production tool для проектов 100K+ файлов / 4GB кода. Технический долг блокирует будущие улучшения. Один раз потратить 2 дня на рефакторинг лучше, чем годами терять время на обходы.

---

### 9. Инкрементальная разработка

```bash
# После каждого изменения
cargo check
cargo clippy
cargo test
```

---

## Специфичные правила по технологиям

### Rowan (CST/AST)
- Изучить документацию rowan
- Паттерны: rust-analyzer/crates/syntax/

### Salsa (инкрементальный анализ)
- Изучить документацию salsa 0.25.2
- Паттерны: rust-analyzer/crates/base-db/

### Logos (лексер)
- Изучить документацию logos
- Учитывать case-insensitivity BSL ключевых слов

---

## Чек-лист перед коммитом

```bash
# 1. Форматирование
cargo fmt --all

# 2. Линтер (обязательно без warnings!)
cargo clippy --all-targets --all-features -- -D warnings

# 3. Тесты
cargo test --all

# 4. Проверка
# - Нет println!/dbg!/eprintln! (только tracing!)
# - Нет абсолютных путей к внешним проектам
# - Нет магических чисел в assert для TextRange
# - Комментарии только для "почему", не "что"
# - TODO с контекстом (автор, issue, срок)
```
