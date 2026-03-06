# Правила разработки

## Архитектура диагностик

### Выбор типа диагностики

| Тип | Когда использовать | Сигнатура | Пример |
|-----|-------------------|-----------|--------|
| **HIR-based** | Семантика: return, присваивания, deprecated, транзакции | `from_hir(range, ctx)` | UnreachableCode, SelfAssign |
| **CFG/Dataflow** | Flow-sensitive: все пути, reaching definitions | `from_hir(range, method_id, ctx)` | AllFunctionPathMustHaveReturn |
| **AST-based** | Синтаксис: форматирование, паттерны в коде | `check(ctx)` | DoubleNegatives, MagicDate |
| **SDBL-based** | Запросы: таблицы, поля, алиасы | `check(ctx)` + `sdbl_hir_in_file()` | AssignAliasFieldsInQuery |
| **Metadata** | Бизнес-правила 1С: имена модулей, контексты | `from_metadata(metadata, config)` | CommonModuleNameClient |
| **Text-based** | Текст: длина строк, пустые строки | `check(ctx)` | LineLength, ConsecutiveEmptyLines |

### Критерии выбора

```
Нужна информация о потоке выполнения (все пути, использование до определения)?
  → CFG/Dataflow-based

Проверка собирается при построении HIR (return, deprecated, транзакции)?
  → HIR-based (добавить в BodyDiagnostic)

Нужны метаданные 1С (Configuration, CommonModule)?
  → Metadata-based

Анализ SDBL запросов?
  → SDBL-based

Проверка синтаксических паттернов без семантики?
  → AST-based

Проверка текста (не AST)?
  → Text-based
```

### Структура файлов

```
crates/hir-def/src/body.rs              ← BodyDiagnostic enum
crates/hir-def/src/body/lower/*.rs      ← Сбор HIR диагностик при lowering
crates/ide-diagnostics/src/lib.rs       ← Dispatch
crates/ide-diagnostics/src/handlers/    ← Один файл = одна диагностика
```

### Шаблоны

**HIR-based (предпочтительный для семантики):**
```rust
// handlers/my_diagnostic.rs
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::MyDiagnostic) {
        return None;
    }
    Some(Diagnostic {
        code: DiagnosticCode::MyDiagnostic,
        message: "...".into(),
        severity: Severity::Warning,
        range,
        tags: vec![],
        fixes: vec![],
    })
}
```

**AST-based:**
```rust
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::MyDiagnostic) {
        return Vec::new();
    }
    let root = ctx.parse().syntax_node();
    // Один проход по descendants()
    root.descendants()
        .filter_map(|node| check_node(&node))
        .collect()
}
```

### Производительность

**Обязательно:**
- Один проход по AST (не вложенные `descendants()`)
- Early exit при `is_disabled()`
- Дешёвые проверки перед дорогими

**Запрещено:**
```rust
// ❌ O(n²) - вложенные descendants
for node in root.descendants() {
    for child in node.descendants() { ... }
}

// ❌ Множественные обходы
let has_a = root.descendants().any(|n| ...);
let has_b = root.descendants().any(|n| ...);
```

**Правильно:**
```rust
// ✅ Один проход, множественные проверки
let nodes: Vec<_> = root.descendants().collect();
let has_a = nodes.iter().any(|n| ...);
let has_b = nodes.iter().any(|n| ...);
```

---

## Критичные правила

### 1. Тестирование

```rust
// ✅ Helper методы для позиций
assert_diagnostic_range(&code, &diag, line, start_col, end_col);
assert_diagnostic_range_multiline(&code, &diag, start_line, start_col, end_line, end_col);

// ❌ Магические числа
assert_eq!(diag.range, TextRange::new(42.into(), 156.into()));
```

```bash
cargo test --all
UPDATE_EXPECT=1 cargo test  # Обновить snapshots после анализа
```

### 2. Качество кода

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

### 3. Логирование

```rust
// ✅ tracing
use tracing::{debug, info, warn};
info!("parsing started");
debug!(file_id = ?file_id, "processing");

// ❌ Запрещено
println!(); dbg!(); eprintln!();
```

### 4. Документация

- Код должен быть самодокументируемым
- Комментарии только для "почему", не "что"
- TODO с автором и issue: `// TODO(username): #123`

### 5. Библиотеки

Перед использованием crate → Context7 (`resolve-library-id` + `query-docs`)

---

## DiagnosticsContext API

```rust
ctx.parse()                    // AST
ctx.module_bodies()            // HIR bodies + BodyDiagnostic
ctx.module_cfgs()              // Control Flow Graphs
ctx.module_reaching_defs()     // Reaching definitions
ctx.sdbl_hir_in_file()         // SDBL HIR
ctx.file_text()                // Исходный текст
ctx.load_configuration()       // Метаданные 1С
ctx.config.is_disabled(code)   // Проверка включения
```

---

## Чек-лист перед коммитом

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

- [ ] Нет `println!/dbg!/eprintln!`
- [ ] Нет магических чисел в assert для TextRange
- [ ] Нет O(n²) паттернов
- [ ] Early exit при `is_disabled()`
- [ ] Тесты покрывают positive и negative cases

---

## Обновление справки платформы 1С

Справка платформы хранится в `crates/bsl-platform/data/platform_data.json` и содержит информацию о типах, методах и глобальных функциях 1С.

### Когда обновлять

- При выходе новой версии платформы 1С с новыми методами/типами
- При обнаружении ошибок в справке

### Требования

- Установленная платформа 1С:Предприятие (Linux: `/opt/1cv8/x86_64/*/`)
- Утилита `7z` для распаковки .hbk файлов

### Процедура обновления

```bash
# 1. Удалить текущий файл (чтобы build.rs извлёк заново)
rm crates/bsl-platform/data/platform_data.json

# 2. Собрать html-parser (если ещё не собран)
cargo build --release -p html-parser \
  --manifest-path crates/bsl-platform/tools/html-parser/Cargo.toml

# 3. Пересобрать bsl-platform (извлечёт данные из 1С)
cargo clean -p bsl-platform
cargo build -p bsl-platform

# 4. Скопировать сгенерированный JSON в репозиторий
cp target/debug/build/bsl-platform-*/out/platform_data.json \
   crates/bsl-platform/data/platform_data.json

# 5. Проверить тесты
cargo test -p hir-def --lib -- platform_helpers

# 6. Закоммитить
git add crates/bsl-platform/data/platform_data.json
git commit -m "chore: update platform data to version X.X.X"
```

### Приоритет источников данных (build.rs)

1. `data/platform_data.json` — из репозитория (предпочтительный)
2. Извлечение из 1С — требует установленную платформу и 7z
3. Пустые структуры — fallback, тесты platform_helpers упадут
