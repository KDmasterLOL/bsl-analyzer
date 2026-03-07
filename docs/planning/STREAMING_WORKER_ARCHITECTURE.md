# Streaming Worker Architecture

## Обзор

Детальная архитектура worker pool для streaming analyze mode. Этот документ описывает как workers обрабатывают файлы параллельно с минимальным потреблением памяти и без блокировок на циклических зависимостях.

**Связанный документ:** [STREAMING_ANALYZE_ARCHITECTURE.md](./STREAMING_ANALYZE_ARCHITECTURE.md) — верхнеуровневая архитектура.

## Ключевой принцип

**SymbolTree публикуется ДО начала диагностик.**

Это устраняет проблему циклических зависимостей, потому что:
- SymbolTree строится только из собственного ItemTree (нет внешних зависимостей)
- Cross-module зависимости появляются только на этапе диагностик
- К моменту диагностик SymbolTree уже доступен другим workers

```
Parse(X) → AST(X) → ItemTree(X) → SymbolTree(X) → PUBLISH
                         ↑
              Только локальные данные:
              - Имена методов
              - Сигнатуры (параметры, экспорт)
              - Имена переменных

              Нет зависимостей от других модулей!
```

## Фазы обработки файла

```
┌─────────────────────────────────────────────────────────────────┐
│                    ОБРАБОТКА ФАЙЛА X                            │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ ФАЗА 1: Построение SymbolTree (без зависимостей)         │  │
│  │                                                          │  │
│  │  1. Read text                                            │  │
│  │  2. Parse → AST                                          │  │
│  │  3. AST → ItemTree (только сигнатуры)                    │  │
│  │  4. ItemTree → SymbolTree                                │  │
│  │  5. PUBLISH SymbolTree(X) ───────────────────────────────────▶ Shared Cache
│  │                                                          │  │
│  │  ✓ Теперь X доступен для ВСЕХ (включая себя)            │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ ФАЗА 2: Диагностики (могут требовать другие модули)      │  │
│  │                                                          │  │
│  │  6. AST → ModuleBodies + BodyDiagnostics                 │  │
│  │  7. Для каждой cross-module диагностики:                 │  │
│  │     └─ Нужен SymbolTree(Y)?                              │  │
│  │        ├─ Есть в cache → используем                      │  │
│  │        └─ Нет → обрабатываем Y (рекурсия в Фазу 1)       │  │
│  │  8. Валидация, генерация Diagnostic                      │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ ФАЗА 3: Cleanup                                          │  │
│  │                                                          │  │
│  │  9. DROP AST                                             │  │
│  │ 10. DROP ModuleBodies                                    │  │
│  │ 11. Mark file completed                                  │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Пример: циклическая зависимость A ↔ B

```
Модули: A вызывает B.Method(), B вызывает A.Method()

Worker начинает с A:

A: Фаза 1
├─ Parse A
├─ Build SymbolTree(A)
└─ PUBLISH SymbolTree(A) ✓         ← A теперь доступен всем

A: Фаза 2
├─ Build ModuleBodies(A)
├─ Диагностика: A вызывает B.Method()
│   └─ Нужен SymbolTree(B) — нет в cache
│       │
│       └─ РЕКУРСИЯ: обрабатываем B
│           │
│           B: Фаза 1
│           ├─ Parse B
│           ├─ Build SymbolTree(B)
│           └─ PUBLISH SymbolTree(B) ✓    ← B теперь доступен
│           │
│           B: Фаза 2
│           ├─ Build ModuleBodies(B)
│           ├─ Диагностика: B вызывает A.Method()
│           │   └─ Нужен SymbolTree(A)
│           │       └─ ЕСТЬ В CACHE! ✓    ← Опубликован выше
│           │       └─ Валидация OK
│           │
│           B: Фаза 3
│           └─ DROP, mark completed
│
├─ Получили SymbolTree(B) ✓
├─ Валидация OK
│
A: Фаза 3
└─ DROP, mark completed

Результат: ОБА файла обработаны, НИКАКИХ блокировок!
```

## Shared State

```
┌─────────────────────────────────────────────────────────────────────┐
│                         SHARED STATE                                │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ file_statuses: Vec<AtomicU8>                                │   │
│  │                                                             │   │
│  │   Единый источник правды о состоянии файла:                │   │
│  │   - 0 = NotStarted      (никто не начал)                   │   │
│  │   - 1 = Parsing         (парсим, ST ещё не готов)          │   │
│  │   - 2 = SymbolTreeReady (ST опубликован, диагностики идут) │   │
│  │   - 3 = Completed       (полностью завершён)               │   │
│  │                                                             │   │
│  │   Переходы состояний:                                       │   │
│  │   NotStarted ──▶ Parsing ──▶ SymbolTreeReady ──▶ Completed │   │
│  │                                                             │   │
│  │   Ожидание SymbolTree: wait пока status >= SymbolTreeReady │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ symbol_trees: DashMap<FileId, Arc<SymbolTree>>              │   │
│  │                                                             │   │
│  │   Заполняется по мере обработки файлов                      │   │
│  │   Финальный размер: ~292 MB для ERP (25K файлов)           │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ sorted_files: Vec<FileId>                                   │   │
│  │                                                             │   │
│  │   Pre-sorted by module type (высший приоритет первым):     │   │
│  │   1. CommonModules (Server)                                 │   │
│  │   2. CommonModules (CallServer)                             │   │
│  │   3. CommonModules (ClientServer)                           │   │
│  │   4. CommonModules (Client)                                 │   │
│  │   5. Manager Modules                                        │   │
│  │   6. Object Modules                                         │   │
│  │   7. Form Modules                                           │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ next_file_idx: AtomicUsize                                  │   │
│  │                                                             │   │
│  │   Указатель на следующий файл для обработки                 │   │
│  │   Workers атомарно инкрементируют для получения работы      │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ condvars: Vec<Condvar> + mutexes: Vec<Mutex<()>>           │   │
│  │                                                             │   │
│  │   Для ожидания готовности SymbolTree конкретного файла      │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Реализация

### Структуры данных

```rust
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use dashmap::DashMap;
use parking_lot::{Condvar, Mutex};

/// Состояние обработки файла (единый источник правды)
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FileStatus {
    NotStarted = 0,       // Никто не начал
    Parsing = 1,          // Парсим, SymbolTree ещё не готов
    SymbolTreeReady = 2,  // SymbolTree опубликован, диагностики идут
    Completed = 3,        // Всё завершено
}

struct SharedState {
    /// Единый статус файла (включает информацию о готовности SymbolTree)
    file_statuses: Vec<AtomicU8>,

    /// Готовые SymbolTrees
    symbol_trees: DashMap<FileId, Arc<SymbolTree>>,

    /// Отсортированный список файлов
    sorted_files: Vec<FileId>,

    /// Индекс следующего файла
    next_file_idx: AtomicUsize,

    /// Для ожидания готовности SymbolTree
    condvars: Vec<Condvar>,
    mutexes: Vec<Mutex<()>>,

    /// Индекс модулей (имя → FileId)
    module_index: Arc<ModuleIndex>,

    /// Metadata конфигурации
    configuration: Option<Arc<Configuration>>,
}
```

### Основной цикл worker'а

```rust
fn worker_main(ctx: Arc<SharedState>, results_tx: Sender<FileResults>) {
    loop {
        // Атомарно получаем следующий файл
        let idx = ctx.next_file_idx.fetch_add(1, Ordering::SeqCst);

        if idx >= ctx.sorted_files.len() {
            break; // Все файлы обработаны
        }

        let file_id = ctx.sorted_files[idx];

        // Пробуем захватить файл
        match ctx.try_claim(file_id) {
            ClaimResult::ByUs => {
                // Мы захватили — обрабатываем
                let results = process_file(file_id, &ctx);
                results_tx.send(FileResults { file_id, diagnostics: results });
            }
            ClaimResult::ByOther | ClaimResult::AlreadyDone => {
                // Кто-то другой уже делает/сделал — пропускаем
                continue;
            }
        }
    }
}
```

### Обработка файла

```rust
fn process_file(file_id: FileId, ctx: &SharedState) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // === ФАЗА 1: SymbolTree (без зависимостей) ===

    let text = read_file(file_id);
    let ast = parse(&text);
    let item_tree = lower_item_tree(&ast);
    let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree));

    // PUBLISH — теперь другие могут использовать
    ctx.symbol_trees.insert(file_id, Arc::clone(&symbol_tree));
    ctx.mark_symbol_tree_ready(file_id);

    // === ФАЗА 2: Диагностики ===

    let module_id = ModuleId::new(file_id);
    let bodies = lower_module_bodies(&ast, &item_tree, module_id);

    // Локальные диагностики
    for body_diag in bodies.all_diagnostics() {
        if let Some(diag) = process_local_diagnostic(body_diag, &ctx) {
            diagnostics.push(diag);
        }
    }

    // Cross-module диагностики
    for body_diag in bodies.cross_module_diagnostics() {
        if let Some(diag) = process_cross_module_diagnostic(body_diag, file_id, &ctx) {
            diagnostics.push(diag);
        }
    }

    // === ФАЗА 3: Cleanup ===

    drop(ast);
    drop(bodies);
    ctx.mark_completed(file_id);

    diagnostics
}
```

### Получение SymbolTree с рекурсивной обработкой

```rust
fn get_or_process_symbol_tree(
    target_file: FileId,
    ctx: &SharedState
) -> Arc<SymbolTree> {
    // 1. Уже готов?
    if ctx.is_symbol_tree_ready(target_file) {
        return ctx.symbol_trees.get(&target_file).unwrap().clone();
    }

    // 2. Пробуем захватить
    match ctx.try_claim(target_file) {
        ClaimResult::ByUs => {
            // Мы захватили — обрабатываем рекурсивно
            // (результаты диагностик отправим позже)
            process_file_for_symbol_tree(target_file, ctx);
        }
        ClaimResult::ByOther => {
            // Кто-то обрабатывает — ждём только SymbolTree
            ctx.wait_for_symbol_tree(target_file);
        }
        ClaimResult::AlreadyDone => {
            // Уже готов (гонка между проверкой и claim)
        }
    }

    ctx.symbol_trees.get(&target_file).unwrap().clone()
}

/// Обработка файла только до SymbolTree (для рекурсивных вызовов)
fn process_file_for_symbol_tree(file_id: FileId, ctx: &SharedState) {
    let text = read_file(file_id);
    let ast = parse(&text);
    let item_tree = lower_item_tree(&ast);
    let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree));

    ctx.symbol_trees.insert(file_id, symbol_tree);
    ctx.mark_symbol_tree_ready(file_id);

    // НЕ делаем диагностики здесь — только SymbolTree
    // Полная обработка будет когда worker дойдёт до этого файла
    // или уже была выполнена
}
```

### Примитивы синхронизации

```rust
impl SharedState {
    fn try_claim(&self, file_id: FileId) -> ClaimResult {
        let idx = file_id.index();

        // Атомарный CAS: NotStarted → Parsing
        match self.file_statuses[idx].compare_exchange(
            FileStatus::NotStarted as u8,
            FileStatus::Parsing as u8,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => ClaimResult::ByUs,
            Err(current) => {
                if current >= FileStatus::Completed as u8 {
                    ClaimResult::AlreadyDone
                } else {
                    ClaimResult::ByOther
                }
            }
        }
    }

    fn mark_symbol_tree_ready(&self, file_id: FileId) {
        let idx = file_id.index();

        // Parsing → SymbolTreeReady
        self.file_statuses[idx].store(FileStatus::SymbolTreeReady as u8, Ordering::SeqCst);

        // Будим ожидающих
        self.condvars[idx].notify_all();
    }

    fn mark_completed(&self, file_id: FileId) {
        let idx = file_id.index();

        // SymbolTreeReady → Completed
        self.file_statuses[idx].store(FileStatus::Completed as u8, Ordering::SeqCst);
    }

    fn is_symbol_tree_ready(&self, file_id: FileId) -> bool {
        // SymbolTree готов если status >= SymbolTreeReady
        self.file_statuses[file_id.index()].load(Ordering::SeqCst)
            >= FileStatus::SymbolTreeReady as u8
    }

    fn wait_for_symbol_tree(&self, file_id: FileId) {
        let idx = file_id.index();

        // Быстрая проверка без блокировки
        if self.is_symbol_tree_ready(file_id) {
            return;
        }

        // Ждём пока status >= SymbolTreeReady
        let guard = self.mutexes[idx].lock();
        while !self.is_symbol_tree_ready(file_id) {
            self.condvars[idx].wait(&mut guard);
        }
    }
}

enum ClaimResult {
    ByUs,        // Мы захватили файл
    ByOther,     // Кто-то другой обрабатывает
    AlreadyDone, // Уже завершён
}
```

## Когда возникает ожидание?

**Единственный случай:** два worker'а одновременно хотят один файл.

```
Worker A: обрабатывает X, нужен Y
Worker B: обрабатывает Z, нужен Y

Timeline:
├─ A: try_claim(Y) = ByUs    ← A успел первым
├─ B: try_claim(Y) = ByOther ← B видит что кто-то делает
├─ B: wait_for_symbol_tree(Y) ← B блокируется
├─ A: публикует SymbolTree(Y)
├─ A: notify_all()
├─ B: просыпается
├─ B: получает SymbolTree(Y)
└─ Оба продолжают работу
```

**Ожидание НЕ возникает** для циклических зависимостей — SymbolTree публикуется до диагностик.

## Предварительная сортировка файлов

Минимизирует ожидания и рекурсивные вызовы:

```rust
fn sort_files_by_priority(
    files: Vec<FileId>,
    module_index: &ModuleIndex,
    configuration: &Configuration,
) -> Vec<FileId> {
    let mut sorted = files;

    sorted.sort_by_key(|file_id| {
        match module_index.get_module_type(*file_id) {
            // CommonModules — наивысший приоритет (большинство зависимостей)
            ModuleType::CommonModule(CommonModuleType::Server) => 0,
            ModuleType::CommonModule(CommonModuleType::CallServer) => 1,
            ModuleType::CommonModule(CommonModuleType::ClientServer) => 2,
            ModuleType::CommonModule(CommonModuleType::Client) => 3,

            // Manager modules — средний приоритет
            ModuleType::ManagerModule => 4,

            // Object modules
            ModuleType::ObjectModule => 5,

            // Form modules — низший приоритет (зависят от всего выше)
            ModuleType::FormModule => 6,

            // Остальные
            _ => 7,
        }
    });

    sorted
}
```

При такой сортировке:
- Когда Form модуль нужен CommonModule → он уже обработан
- Минимум рекурсивных вызовов `get_or_process_symbol_tree`
- Минимум ожиданий `wait_for_symbol_tree`

## Память

| Компонент | Размер | Когда освобождается |
|-----------|--------|---------------------|
| Metadata | 31 MB | Конец анализа |
| ModuleIndex | ~5 MB | Конец анализа |
| SymbolTrees (все) | 292 MB | Конец анализа |
| File statuses | ~100 KB | Конец анализа |
| Condvars + Mutexes | ~200 KB | Конец анализа |
| **Постоянно:** | **~330 MB** | |
| | | |
| AST (1 файл) | ~70 KB | После Фазы 3 файла |
| ModuleBodies (1 файл) | ~160 KB | После Фазы 3 файла |
| Текст файла | ~25 KB | После Фазы 3 файла |
| **На worker:** | **~255 KB** | |
| | | |
| Рекурсия (макс глубина ~10) | ~2.5 MB | После возврата |

**Peak память (8 workers):** ~330 MB + 8 × 255 KB + 2.5 MB = **~335 MB**

vs текущие:
- Salsa full: 26.6 GB
- Batch mode: 4.2 GB

## Зависимости (Cargo.toml)

```toml
[dependencies]
dashmap = "6.1.0"
parking_lot = "0.12.5"
crossbeam-channel = "0.5.15"
```

## Метрики для мониторинга

```rust
struct WorkerMetrics {
    /// Количество обработанных файлов
    files_processed: AtomicUsize,

    /// Количество рекурсивных вызовов get_or_process_symbol_tree
    recursive_calls: AtomicUsize,

    /// Количество ожиданий wait_for_symbol_tree
    waits: AtomicUsize,

    /// Суммарное время ожидания (наносекунды)
    wait_time_ns: AtomicU64,
}
```

## Тестирование

### Unit tests

1. **Базовая обработка** — один файл без зависимостей
2. **Линейная зависимость** — A → B → C
3. **Циклическая зависимость** — A ↔ B
4. **Множественные зависимости** — A → [B, C, D]
5. **Гонка за файл** — два worker'а хотят один файл

### Integration tests

1. **doc3 (6,540 файлов)** — реальный проект, проверка корректности
2. **ERP (25,000 файлов)** — stress test, проверка памяти
3. **Synthetic циклы** — искусственные циклические зависимости

### Benchmarks

1. **Время обработки** vs Salsa batch mode
2. **Peak память** vs Salsa batch mode
3. **Распределение времени** по фазам (parse, lower, diagnostics)
