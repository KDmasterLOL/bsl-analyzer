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

### 4. HIR-based диагностики (архитектура rust-analyzer)

**Принцип:** Диагностики собираются как побочный продукт HIR lowering, не как отдельные AST traversals.

#### Структура файлов

```
crates/hir-def/src/body.rs          ← BodyDiagnostic enum (собирается при lowering)
crates/ide-diagnostics/src/lib.rs   ← Dispatch к handlers по типу BodyDiagnostic
crates/ide-diagnostics/src/handlers/
├── function_should_have_return.rs  ← Отдельный файл для каждой диагностики
├── empty_code_block.rs
└── ...
```

#### Правила для HIR диагностик

**✅ ПРАВИЛЬНО:**
```rust
// handlers/function_should_have_return.rs

/// Creates diagnostic from HIR BodyDiagnostic (called from lib.rs dispatch)
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::FunctionShouldHaveReturn) {
        return None;
    }
    Some(Diagnostic { ... })
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_from_fixture() {
        let code = include_str!("../../tests/fixtures/FunctionShouldHaveReturnDiagnostic.bsl");
        // ...
    }
}
```

**❌ ЗАПРЕЩЕНО:**
```rust
// НЕ собирать все HIR диагностики в одном файле hir_diagnostics.rs
// Каждая диагностика должна быть в своём файле!
```

#### Когда использовать HIR vs AST

| Тип проверки | Подход | Пример |
|--------------|--------|--------|
| Структура метода (return/empty) | HIR lowering | FunctionShouldHaveReturn, EmptyCodeBlock |
| Значения литералов | HIR lowering | MagicNumber |
| Flow-sensitive (достижимость) | HIR + CFG | UnreachableCode, MissingReturn |
| Синтаксис (форматирование) | AST | LineLength, MissingSpace |
| Метаданные | AST + Metadata | CommonModuleAssign |

#### Тестирование HIR диагностик

**Обязательно:**
- Тесты с реальными фикстурами из `tests/fixtures/`
- Тесты через helper `check_hir_diagnostic()` или аналогичный
- Проверка что диагностика НЕ срабатывает на правильном коде

```rust
#[test]
fn test_function_should_have_return_fixture() {
    let code = include_str!("../../tests/fixtures/FunctionShouldHaveReturnDiagnostic.bsl");
    let diagnostics = check_diagnostic(code);
    assert_eq!(diagnostics.len(), 1);
    assert_diagnostic_range(&code, &diagnostics[0], 0, 8, 26);
}
```

---

### 5. Качество кода

```bash
# Обязательно перед коммитом
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

**Допустимо:** `#[allow(...)]` только с комментарием-обоснованием.

---

### 5. Оптимизация алгоритмов (избегать O(n²) и выше)

**Цель проекта:** 5-6x быстрее Java версии на реальных проектах (100+ MB, 6K+ файлов).

**Правило:** Все алгоритмы должны быть оптимальной сложности. Избегать вложенных циклов O(n²) или выше.

#### Запрещенные паттерны

```rust
// ❌ ПЛОХО: O(n²) - вложенный descendants() в цикле
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let root = parse.syntax_node();
    for node in root.descendants() {           // O(n)
        node.descendants()...                  // O(n) × n = O(n²)
    }
}

// ❌ ПЛОХО: O(n×m) - проверка всех элементов для каждого
for region in &regions {                       // O(n)
    for method in &methods {                   // O(m) × n = O(n×m)
        if region.contains(method) { ... }
    }
}

// ❌ ПЛОХО: Множественные обходы дерева
let has_eq = node.descendants_with_tokens()    // O(n)
    .any(|t| t.kind() == SyntaxKind::EQ);
let has_dot = node.descendants_with_tokens()   // O(n) - второй обход!
    .any(|t| t.kind() == SyntaxKind::DOT);
```

#### Правильные паттерны

```rust
// ✅ ХОРОШО: O(n) - один обход с токен-стримом
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let root = parse.syntax_node();

    // Построить токен-лист один раз
    let tokens: Vec<_> = root
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .collect();  // O(n) однократно

    // Обрабатывать с lookahead
    for (i, token) in tokens.iter().enumerate() {
        let next = tokens.get(i + 1);          // O(1)
        // ... pattern matching
    }
}

// ✅ ХОРОШО: O(n + m log m) - сортировка + бинарный поиск
methods.sort_by_key(|r| r.start());            // O(m log m)
for region in &regions {                       // O(n)
    let start_idx = methods
        .binary_search_by_key(&region.start(), |r| r.start())  // O(log m)
        .unwrap_or_else(|idx| idx);

    // Проверяем только релевантные методы
    let has_methods = methods[start_idx..]
        .iter()
        .take_while(|m| m.start() < region.end())  // Early exit
        .any(|m| region.contains(m));
}

// ✅ ХОРОШО: Один обход с множественными проверками
let tokens: Vec<_> = node.descendants_with_tokens()
    .filter_map(|el| el.into_token())
    .collect();  // O(n) один раз

let has_eq = tokens.iter().any(|t| t.kind() == SyntaxKind::EQ);    // O(n)
let has_dot = tokens.iter().any(|t| t.kind() == SyntaxKind::DOT);  // O(n)
// Итого: O(n), а не O(2n) с двумя обходами дерева
```

#### Стратегии оптимизации

1. **Pre-collect данные** в один проход:
   ```rust
   // Собрать все узлы/токены один раз
   let nodes: Vec<_> = root.descendants().filter(...).collect();
   let tokens: Vec<_> = root.descendants_with_tokens()...collect();
   ```

2. **Кешировать вычисления**:
   ```rust
   // HashMap для O(1) lookup вместо O(n) поиска
   let mut node_info = HashMap::new();
   for node in &nodes {
       let info = compute_info(&node);  // Один раз
       node_info.insert(node.id(), info);
   }
   ```

3. **Сортировка + бинарный поиск** вместо вложенных циклов:
   ```rust
   items.sort_by_key(|item| item.position());
   let idx = items.binary_search_by_key(&target, |i| i.position());
   ```

4. **Early exit** при поиске:
   ```rust
   // Остановиться когда нашли или вышли за границы
   .take_while(|item| item.start() < region.end())
   ```

5. **Проверять дешевые условия ПЕРЕД дорогими**:
   ```rust
   // ✅ Быстрая проверка синтаксиса → дорогая загрузка метаданных
   if !has_public_regions(&root) {
       return Vec::new();  // Early exit
   }
   let metadata = load_metadata(ctx);  // Только если нужно
   ```

#### Измерение производительности

```bash
# Включить логирование медленных диагностик (>100ms)
BSL_LOG=warn cargo run

# Проверить, что ваша диагностика не в списке медленных
./target/release/bsl-analyzer analyze -s=/path/to/large/project
```

**Threshold:** Диагностика на файле > 100ms считается медленной и требует оптимизации.

#### Реальные примеры (из истории проекта)

**До оптимизации:**
- `double_negatives`: O(n²) - 7 вызовов descendants()
- `create_query_in_cycle`: O(n²) - 9 вызовов descendants()
- `cached_public`: 300-316ms (загрузка метаданных перед синтаксической проверкой)

**После оптимизации:**
- Все < 100ms
- doc3 (6.5K файлов): **11.2s → 10.0s** общее время

**Commit examples:** `cfe8491`, `4e6793d`

#### Исключения

Допустимо O(n²) ТОЛЬКО если:
1. n гарантированно мало (< 10 элементов) и это документировано
2. Оптимизация усложнит код без реальной пользы (профилирование показало < 1ms)
3. Есть комментарий с обоснованием и бенчмарком

```rust
// ✅ Допустимо: n < 5 (только аннотации метода)
// Benchmark: < 0.1ms на типичных методах (1-3 аннотации)
for ann in annotations {  // n = 1-3
    for param in ann.params {  // m = 0-2
        check_param(param);
    }
}
```

---

### 6. Независимость от внешних проектов

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

### 7. Логирование: только tracing

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

### 8. Self-documenting code (минимум комментариев)

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

### 9. Исправление инфраструктуры вместо костылей

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

### 10. Инкрементальная разработка

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
# - Нет O(n²) паттернов (вложенные descendants() и т.д.)
# - Диагностики < 100ms на файл (проверить: BSL_LOG=warn cargo run)
```
