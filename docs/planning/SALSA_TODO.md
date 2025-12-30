# Salsa Integration TODO

**Дата создания:** 2025-12-29
**Дата завершения:** 2025-12-30
**Статус:** ✅ **ЗАВЕРШЕНО** (Фазы 1-4)
**Текущая версия Salsa:** 0.25.2

---

## ✅ СТАТУС РЕАЛИЗАЦИИ (2025-12-30)

### Что реализовано (Фазы 1-4)

**Фаза 1: Прототип (Завершено)** ✅
- Создан рабочий прототип в `experiments/salsa-prototype/`
- Изучены паттерны Salsa 0.25.2 API
- Все 9 тестов прототипа проходят

**Фаза 2: Base-DB миграция (Завершено)** ✅
- `parse_query` использует `#[salsa::tracked(lru = 128)]`
- Salsa input structs: `FileTextInput`, `SourceRootInput`, `FileSourceRootInput`
- Автоматическая инвалидация через Salsa
- Все 10 base-db тестов проходят

**Фаза 3: IDE-DB миграция (Завершено)** ✅
- `RootDatabaseImpl` использует `salsa::Storage<Self>`
- Трейты помечены `#[salsa::db]`
- DefDatabase запросы используют ручное кеширование (временно)
- Все 13 ide-db тестов проходят

**Фаза 4: Оптимизация и бенчмарки (Завершено)** ✅
- Добавлены durability levels (HIGH для библиотек, LOW для кода)
- Реализован `set_file_text_smart()` для автоопределения durability
- Созданы comprehensive benchmarks
- Производительность превышает цели в **100-25,000 раз**

### Результаты производительности

```
Бенчмарк                Время       Цель        Результат
─────────────────────────────────────────────────────────────
cache_hit               21.8 ns    < 10 μs     ✅ в 457 раз лучше
incremental_update      1.96 μs    < 50 ms     ✅ в 25,000 раз лучше
item_tree_cache_hit     4.79 ns    < 10 μs     ✅ в 2,000 раз лучше
item_tree_incremental   3.0 μs     < 100 ms    ✅ в 33,000 раз лучше
symbol_tree_cache_hit   5.0 ns     < 10 μs     ✅ в 2,000 раз лучше
large_file_set_lru      4.14 μs    N/A         ✅ LRU работает корректно
```

### Тесты

**Всего:** 106 тестов проходят ✅
- base-db: 10 тестов
- hir-def: 52 теста
- ide-db: 13 тестов
- module-graph: 31 тест

### Что осталось на будущее (Итерация 10+)

**DefDatabase Salsa миграция:**
- `item_tree_query`, `module_data_query`, `symbol_tree_query`
- **Проблема:** Salsa tracked functions требуют Salsa types как параметры, но FileId/ModuleId — обычные структуры
- **Решение:** Требует создания Salsa-совместимых типов или wrapper inputs

**Почему пока ручное кеширование:**
- DefDatabase использует DashMap с ручной инвалидацией через `invalidate_file()`
- Всё равно получаем benefit от Salsa через `parse_query` tracking
- Миграция возможна в будущей итерации после решения паттерна FileId/ModuleId

---

## Зачем нужна Salsa?

Salsa — это фреймворк для **инкрементальных вычислений** (incremental computation), который является ключевым компонентом для высокопроизводительного Language Server.

### Основные преимущества:

1. **Автоматическая инвалидация кеша**
   - Salsa отслеживает зависимости между запросами (queries)
   - При изменении входных данных (input query) автоматически инвалидирует только затронутые производные запросы (derived queries)
   - Не требуется ручное управление кешем

2. **Ленивые вычисления**
   - Вычисления происходят только когда результат действительно нужен
   - Промежуточные результаты кешируются
   - Повторные запросы возвращают кешированный результат мгновенно

3. **Детальный контроль durability**
   - `Durability::HIGH` — данные практически не меняются (библиотеки, системные файлы)
   - `Durability::MEDIUM` — зависимости проекта
   - `Durability::LOW` — исходный код пользователя
   - Salsa оптимизирует проверки на основе durability

4. **Параллельные вычисления**
   - Salsa безопасна для многопоточности
   - Автоматическое распараллеливание независимых запросов
   - Защита от race conditions

### Пример использования (rust-analyzer):

```rust
// Input query - изменяется при редактировании файла
#[salsa::input]
struct FileText {
    file_id: FileId,
    #[returns(as_ref)]
    text: Arc<str>,
}

// Derived query - автоматически пересчитывается только если изменился text
#[salsa::tracked]
fn parse(db: &dyn Db, file_id: FileId) -> Parse {
    let text = db.file_text(file_id).text(db);
    parser::parse(&text)
}

// Пользователь меняет файл:
db.set_file_text(file_id).to(new_text);

// При следующем вызове parse() Salsa:
// 1. Проверяет, изменился ли текст
// 2. Если да — парсит заново
// 3. Если нет — возвращает кешированный результат
let ast = db.parse(file_id);
```

## Текущая реализация (временное решение)

В Iteration 5 мы использовали упрощенный подход с **DashMap** вместо полной интеграции Salsa:

```rust
pub struct Files {
    file_texts: Arc<DashMap<FileId, Arc<str>>>,
    parse_cache: Arc<DashMap<FileId, Arc<Parse<SyntaxNode>>>>,
}

impl Files {
    pub fn parse(&self, db: &dyn SourceDatabase, file_id: FileId) -> Parse<SyntaxNode> {
        // Проверяем кеш
        if let Some(cached) = self.parse_cache.get(&file_id) {
            return (**cached.value()).clone();
        }

        // Парсим и кешируем
        let text = db.file_text(file_id);
        let parse_result = parser::parse(&text);
        self.parse_cache.insert(file_id, Arc::new(parse_result.clone()));
        parse_result
    }
}
```

### Проблемы текущей реализации:

1. **Ручная инвалидация кеша**
   ```rust
   pub fn set_file_text(&self, file_id: FileId, text: &str) {
       self.file_texts.insert(file_id, text_arc);
       // ❌ Приходится вручную очищать кеш!
       self.parse_cache.remove(&file_id);
   }
   ```

2. **Нет управления зависимостями**
   - Если parse() использует дополнительные данные (например, конфигурацию), мы не отследим изменения
   - Все зависимости нужно отслеживать вручную

3. **Нет durability**
   - Все изменения обрабатываются одинаково
   - Нет оптимизации для библиотек vs исходного кода

4. **Нет LRU eviction**
   - Кеш растет бесконечно
   - Нужно вручную реализовывать вытеснение старых записей

5. **Нет параллелизма**
   - DashMap безопасен для многопоточности, но не обеспечивает автоматического распараллеливания

## Проблемы при интеграции Salsa 0.25.2

При попытке интеграции Salsa 0.25.2 в Iteration 5 мы столкнулись со следующими проблемами:

### 1. Изменения API (0.18 → 0.25.2)

**Проблема:** Документация и примеры в основном для старых версий.

```rust
// ❌ Salsa 0.18 (устаревший синтаксис):
#[salsa::query_group(SourceDatabaseStorage)]
pub trait SourceDatabase {
    #[salsa::input]
    fn file_text(&self, file_id: FileId) -> Arc<str>;

    #[salsa::invoke(parse)]
    #[salsa::lru(128)]
    fn parse(&self, file_id: FileId) -> Parse;
}

// ✅ Salsa 0.25.2 (новый синтаксис):
#[salsa::input]
struct FileText {
    file_id: FileId,
    #[returns(as_ref)]  // Было #[return_ref]
    text: Arc<str>,
}

#[salsa::tracked(lru = 128)]
fn parse(db: &dyn Db, file_id: FileId) -> Parse {
    // ...
}
```

### 2. Ingredient Registration System

**Проблема:** Salsa 0.25.2 использует сложную систему регистрации "ингредиентов" (ingredients).

```rust
// Попытка использовать #[salsa::jar]:
#[salsa::jar(db = SourceDb)]
pub struct Jar(FileText, parse);

pub trait SourceDb: salsa::DbWithJar<Jar> { }

// ❌ Ошибка: could not find 'jar' in salsa_macros
// ❌ Ошибка: the trait bound 'FileId: SalsaStructInDb' is not satisfied
```

**Причины:**
- `#[salsa::jar]` требует специфической настройки
- Все типы внутри jar должны быть Salsa-совместимыми
- Сложная система trait bounds

### 3. Отсутствие примеров для 0.25.2

**Проблема:** Официальная документация неполная.

- Нет полноценного примера для LSP/IDE use case
- Большинство примеров для простых случаев (hello world)
- rust-analyzer использует внутренние детали реализации

### 4. Сложность интеграции с существующим кодом

**Проблема:** Salsa требует значительной перестройки архитектуры.

```rust
// Текущая архитектура:
pub trait SourceDatabase {
    fn file_text(&self, file_id: FileId) -> Arc<str>;
    fn set_file_text(&mut self, file_id: FileId, text: &str);
}

// Salsa требует:
// 1. Определить Database struct
// 2. Реализовать salsa::Database
// 3. Использовать #[salsa::db] для всех trait'ов
// 4. Переписать все queries в формате salsa::tracked или salsa::input
// 5. Использовать систему jars для группировки
```

## Ожидаемые улучшения при решении проблем

### 1. Производительность

**Текущая проблема:**
- При изменении 1 файла — пересчитываются все зависимые файлы
- Нет оптимизации для часто используемых результатов

**С Salsa:**
```
Редактирование файла Module1.bsl:
  ❌ Без Salsa: пересчет Module1, Module2, Module3 (все зависимые)
  ✅ С Salsa: пересчет только Module1 (если интерфейс не изменился)
```

**Метрика:** Incremental update: 500ms → **50ms** (10x улучшение)

### 2. Память

**Текущая проблема:**
- Кеш растет неограниченно
- Нет стратегии вытеснения

**С Salsa:**
```rust
#[salsa::tracked(lru = 128)]
fn parse(db: &dyn Db, file_id: FileId) -> Parse {
    // LRU автоматически вытесняет старые результаты
}
```

**Метрика:** Память при анализе: 10GB → **2.5GB** (4x улучшение)

### 3. Параллелизм

**Текущая проблема:**
- Вычисления выполняются последовательно

**С Salsa:**
```rust
// Salsa автоматически распараллеливает независимые queries
let results: Vec<_> = files
    .par_iter()
    .map(|file_id| db.parse(*file_id))
    .collect();
```

**Метрика:** Время анализа ERP: 60мин → **15мин** (4x улучшение)

### 4. Надежность

**Текущая проблема:**
- Легко забыть инвалидировать кеш
- Race conditions в многопоточном окружении

**С Salsa:**
- Автоматическая инвалидация
- Thread-safe по дизайну
- Отлов циклических зависимостей

### 5. Разработка

**Текущая проблема:**
- Много boilerplate кода для кеширования
- Сложно добавить новый derived query

**С Salsa:**
```rust
// Добавить новый query — просто добавить функцию:
#[salsa::tracked]
fn find_diagnostics(db: &dyn Db, file_id: FileId) -> Vec<Diagnostic> {
    let ast = db.parse(file_id);  // Автоматическая зависимость
    analyze_ast(&ast)
}
```

## План интеграции (будущая итерация)

### Этап 1: Изучение (1-2 дня)

1. **Прочитать актуальную документацию Salsa 0.25.2**
   - Использовать Context7 MCP для получения актуальной документации
   - Изучить changelog: https://github.com/salsa-rs/salsa/blob/master/CHANGELOG.md

2. **Изучить код rust-analyzer**
   - Файлы:
     - `/Users/kiriller/src/lsp/rust-analyzer/crates/base-db/src/lib.rs`
     - `/Users/kiriller/src/lsp/rust-analyzer/crates/base-db/src/input.rs`
     - `/Users/kiriller/src/lsp/rust-analyzer/crates/ide-db/src/lib.rs`
   - Понять систему jars
   - Понять ingredient registration

3. **Создать минимальный прототип**
   ```bash
   cargo new --lib salsa-prototype
   # Реализовать простой пример с file_text -> parse
   ```

### Этап 2: Постепенная миграция (3-5 дней)

1. **Создать Salsa Database**
   ```rust
   #[salsa::db]
   pub struct RootDatabase {
       storage: salsa::Storage<Self>,
       files: Files,
   }
   ```

2. **Мигрировать input queries**
   ```rust
   #[salsa::input]
   struct FileText { ... }

   #[salsa::input]
   struct SourceRootData { ... }
   ```

3. **Мигрировать derived queries**
   ```rust
   #[salsa::tracked(lru = 128)]
   fn parse(db: &dyn Db, file_id: FileId) -> Parse { ... }
   ```

4. **Обновить тесты**
   - Убедиться что все 82+ теста проходят
   - Добавить тесты инкрементальности

### Этап 3: Оптимизация (2-3 дня)

1. **Настроить durability**
   ```rust
   // Библиотеки — HIGH durability
   db.set_file_text_with_durability(file_id, text, Durability::HIGH);

   // Исходный код — LOW durability
   db.set_file_text_with_durability(file_id, text, Durability::LOW);
   ```

2. **Настроить LRU размеры**
   - Benchmark на реальных проектах
   - Найти оптимальный размер кеша

3. **Добавить параллелизм**
   - Использовать rayon для параллельных вычислений
   - Проверить на отсутствие deadlocks

### Этап 4: Валидация (1-2 дня)

1. **Тестирование производительности**
   ```bash
   cargo bench --bench incremental
   ```

2. **Тестирование корректности**
   - Все существующие тесты проходят
   - Новые тесты инкрементальности

3. **Профилирование**
   ```bash
   BSL_PROFILE=* cargo run -- analyze large_project/
   ```

## Критерии успеха

✅ **Обязательно:**
- Все существующие тесты проходят
- Incremental update < 100ms (сейчас неопределено)
- Память не растет бесконечно (LRU работает)
- Thread-safe (проходят тесты с параллельными запросами)

✅ **Желательно:**
- Incremental update < 50ms
- Параллельный анализ нескольких файлов
- Профилирование показывает минимальные overhead от Salsa

## Ресурсы

### Документация
- **Salsa Book:** https://salsa-rs.netlify.app/
- **GitHub:** https://github.com/salsa-rs/salsa
- **Changelog:** https://github.com/salsa-rs/salsa/blob/master/CHANGELOG.md
- **Latest Release:** v0.25.2 (December 17, 2025)

### Примеры кода
- **rust-analyzer base-db:** `/Users/kiriller/src/lsp/rust-analyzer/crates/base-db/`
- **rust-analyzer ide-db:** `/Users/kiriller/src/lsp/rust-analyzer/crates/ide-db/`
- **Salsa examples:** https://github.com/salsa-rs/salsa/tree/master/examples

### Инструменты
- **Context7 MCP:** Использовать для получения актуальной документации
  ```
  resolve-library-id: salsa
  query-docs: "How to use #[salsa::tracked] with lru cache?"
  ```

## Заметки

- Salsa активно развивается (v0.25.2 от 17 декабря 2025)
- Стоит регулярно проверять новые релизы
- rust-analyzer — лучший источник реальных примеров
- Не торопиться с интеграцией — сначала полностью понять систему
- Текущая DashMap-реализация работает, не блокирует развитие проекта

---

**Приоритет:** P2 (Important, but not blocking)
**Следующий шаг:** Изучить актуальную документацию Salsa 0.25.2 через Context7
