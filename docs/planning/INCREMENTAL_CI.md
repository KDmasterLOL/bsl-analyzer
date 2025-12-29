# Incremental CI/CD Analysis with ModuleGraph

**Дата:** 2025-12-29
**Статус:** Планирование
**Приоритет:** ВЫСОКИЙ (для SonarQube CI/CD)

## Проблема

При проверке больших BSL проектов в GitLab CI большинство файлов не изменяется между коммитами.

**Пример: pt_erp проект**
- Всего модулей: 25,090 BSL
- Типичный commit: изменено 1-5 модулей
- Full scan: 10-15 секунд
- **Проблема:** анализируем все 25,090 модулей, хотя изменено < 0.1%

**Цель:** Анализировать только затронутые модули + их зависимости.

## Решение: ModuleGraph + Incremental CI Mode

### Архитектура

```
GitLab CI
  ↓
git diff HEAD~1 --name-only → ["CommonModules/Module1.bsl", "CommonModules/Module2.bsl"]
  ↓
bsl-analyzer --incremental --changed-files "..."
  ↓
ModuleGraph.affected_modules(changed_files) → [Module1, Module2, Module3, ..., Module20]
  ↓
Analyze ONLY affected modules (20 instead of 25,090)
  ↓
Output: sonarqube.json (только для затронутых модулей)
```

### Компоненты

#### 1. ModuleGraph (Core)

**Назначение:** Граф зависимостей BSL модулей

**Структура:**
```rust
#[derive(Debug, Clone)]
pub struct ModuleGraph {
    /// Все модули в проекте
    modules: Arena<ModuleData>,

    /// Индекс: path → ModuleId
    path_to_module: FxHashMap<VfsPath, ModuleId>,

    /// Индекс: name → [ModuleId] (может быть несколько модулей с одним именем)
    name_to_modules: FxHashMap<ModuleName, SmallVec<[ModuleId; 1]>>,
}

#[derive(Debug, Clone)]
pub struct ModuleData {
    /// ID модуля
    pub id: ModuleId,

    /// Имя модуля (из Процедура/Функция или CommonModule name)
    pub name: ModuleName,

    /// Путь к файлу
    pub file_id: FileId,

    /// Прямые зависимости (этот модуль ИСПОЛЬЗУЕТ)
    pub dependencies: Vec<Dependency>,

    /// Метаданные модуля (если есть - для CommonModule)
    pub metadata: Option<Arc<CommonModuleMetadata>>,

    /// Тип модуля
    pub kind: ModuleKind,
}

#[derive(Debug, Clone)]
pub struct Dependency {
    /// ID модуля-зависимости
    pub module_id: ModuleId,

    /// Тип зависимости
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    /// Прямой вызов функции/процедуры
    DirectCall,

    /// Импорт через #Использовать
    Import,

    /// Зависимость через метаданные
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    /// Общий модуль (CommonModule)
    CommonModule,

    /// Модуль объекта метаданных
    ObjectModule,

    /// Модуль формы
    FormModule,

    /// Модуль менеджера
    ManagerModule,

    /// Модуль команды
    CommandModule,
}
```

#### 2. ModuleGraphBuilder (Construction)

**Паттерн:** Как CrateGraphBuilder в rust-analyzer

```rust
#[derive(Default)]
pub struct ModuleGraphBuilder {
    modules: Vec<ModuleBuilder>,
    edges: Vec<(ModuleBuilderId, ModuleBuilderId, DependencyKind)>,
}

impl ModuleGraphBuilder {
    pub fn add_module(
        &mut self,
        name: ModuleName,
        file_id: FileId,
        kind: ModuleKind,
    ) -> ModuleBuilderId {
        let id = ModuleBuilderId(self.modules.len() as u32);
        self.modules.push(ModuleBuilder { name, file_id, kind, dependencies: vec![] });
        id
    }

    pub fn add_dependency(
        &mut self,
        from: ModuleBuilderId,
        to: ModuleBuilderId,
        kind: DependencyKind,
    ) -> Result<(), CyclicDependencyError> {
        // Проверка циклов (DFS)
        if self.has_path(to, from) {
            return Err(CyclicDependencyError { from, to });
        }

        self.edges.push((from, to, kind));
        Ok(())
    }

    /// Построение финального графа с валидацией
    pub fn build(self) -> ModuleGraph {
        // Конвертация builder IDs в final IDs
        // Построение индексов
        // ...
    }
}
```

#### 3. Salsa Integration

**Queries:**

```rust
/// Input: конфигурация проекта
#[salsa::input]
pub struct ProjectConfig {
    pub root_path: PathBuf,
    pub metadata_path: PathBuf,
}

/// Derived: список всех модулей
#[salsa::tracked(lru = 1)]
pub fn all_modules(db: &dyn Db) -> Arc<ModuleGraph> {
    let config = db.project_config();

    // 1. Сканируем все .bsl файлы
    // 2. Парсим каждый файл (через db.parse)
    // 3. Извлекаем зависимости (вызовы функций, #Использовать)
    // 4. Строим ModuleGraph

    Arc::new(module_graph)
}

/// Derived: зависимости конкретного модуля
#[salsa::tracked]
pub fn module_dependencies(db: &dyn Db, module_id: ModuleId) -> Arc<Vec<ModuleId>> {
    let graph = db.all_modules();
    Arc::new(graph.dependencies(module_id).to_vec())
}

/// Derived: обратные зависимости (кто зависит от этого модуля)
#[salsa::tracked]
pub fn module_reverse_dependencies(db: &dyn Db, module_id: ModuleId) -> Arc<Vec<ModuleId>> {
    let graph = db.all_modules();
    Arc::new(graph.reverse_dependencies(module_id).to_vec())
}
```

**Durability:**
- ModuleGraph: `Durability::LOW` (меняется при изменении любого файла)
- Отдельные module_dependencies: кешируются Salsa, инвалидируются при изменении файла модуля

#### 4. Incremental Analysis Engine

**Алгоритм поиска затронутых модулей:**

```rust
impl ModuleGraph {
    /// Найти все модули, затронутые изменениями
    pub fn affected_modules(&self, changed_files: &[FileId]) -> Vec<ModuleId> {
        let mut affected = FxHashSet::default();
        let mut queue = VecDeque::new();

        // 1. Добавляем измененные модули
        for &file_id in changed_files {
            if let Some(module_id) = self.file_to_module(file_id) {
                affected.insert(module_id);
                queue.push_back(module_id);
            }
        }

        // 2. BFS: добавляем все зависимые модули
        while let Some(module_id) = queue.pop_front() {
            for &dependent in self.reverse_dependencies(module_id) {
                if affected.insert(dependent) {
                    queue.push_back(dependent);
                }
            }
        }

        // 3. Также добавляем прямые зависимости (для контекста диагностик)
        let mut with_deps = affected.clone();
        for &module_id in &affected {
            for &dep in self.dependencies(module_id) {
                with_deps.insert(dep);
            }
        }

        with_deps.into_iter().collect()
    }

    /// Транзитивные зависимости (этот модуль зависит от...)
    pub fn transitive_dependencies(&self, module_id: ModuleId) -> Vec<ModuleId> {
        let mut visited = FxHashSet::default();
        let mut result = Vec::new();
        self.transitive_deps_impl(module_id, &mut visited, &mut result);
        result
    }

    fn transitive_deps_impl(
        &self,
        module_id: ModuleId,
        visited: &mut FxHashSet<ModuleId>,
        result: &mut Vec<ModuleId>,
    ) {
        if !visited.insert(module_id) {
            return;
        }

        for dep in &self[module_id].dependencies {
            result.push(dep.module_id);
            self.transitive_deps_impl(dep.module_id, visited, result);
        }
    }

    /// Обратные транзитивные зависимости (кто зависит от этого модуля)
    pub fn transitive_reverse_dependencies(&self, module_id: ModuleId) -> Vec<ModuleId> {
        let mut visited = FxHashSet::default();
        let mut result = Vec::new();
        self.transitive_rev_deps_impl(module_id, &mut visited, &mut result);
        result
    }

    fn transitive_rev_deps_impl(
        &self,
        module_id: ModuleId,
        visited: &mut FxHashSet<ModuleId>,
        result: &mut Vec<ModuleId>,
    ) {
        if !visited.insert(module_id) {
            return;
        }

        for &rev_dep in self.reverse_dependencies(module_id) {
            result.push(rev_dep);
            self.transitive_rev_deps_impl(rev_dep, visited, result);
        }
    }
}
```

#### 5. CLI Integration

**Команда:**

```bash
bsl-analyzer analyze \
  --project /path/to/project \
  --incremental \
  --changed-files "CommonModules/Module1.bsl,CommonModules/Module2.bsl" \
  --output sonarqube.json
```

**Или через git:**

```bash
bsl-analyzer analyze \
  --project /path/to/project \
  --incremental \
  --git-diff HEAD~1 \
  --output sonarqube.json
```

**Реализация:**

```rust
pub struct AnalyzeCommand {
    pub project: PathBuf,
    pub incremental: bool,
    pub changed_files: Option<Vec<PathBuf>>,
    pub git_diff: Option<String>,
    pub output: PathBuf,
}

impl AnalyzeCommand {
    pub fn run(&self, db: &mut RootDatabase) -> Result<()> {
        // 1. Загружаем проект
        db.set_project_config(ProjectConfig {
            root_path: self.project.clone(),
            metadata_path: self.project.join("Configuration.xml"),
        });

        // 2. Строим ModuleGraph
        let graph = db.all_modules();

        // 3. Определяем, какие модули анализировать
        let modules_to_analyze = if self.incremental {
            let changed_files = self.resolve_changed_files()?;
            graph.affected_modules(&changed_files)
        } else {
            graph.all_module_ids().collect()
        };

        info!(
            modules_total = graph.modules.len(),
            modules_affected = modules_to_analyze.len(),
            "Analyzing modules"
        );

        // 4. Анализируем только затронутые модули
        let diagnostics = modules_to_analyze
            .par_iter()
            .flat_map(|&module_id| {
                let file_id = graph[module_id].file_id;
                db.diagnostics(file_id)
            })
            .collect::<Vec<_>>();

        // 5. Экспортируем результаты
        export_sonarqube(&diagnostics, &self.output)?;

        Ok(())
    }

    fn resolve_changed_files(&self) -> Result<Vec<FileId>> {
        if let Some(ref files) = self.changed_files {
            // Прямой список файлов
            files.iter()
                .map(|path| self.path_to_file_id(path))
                .collect()
        } else if let Some(ref git_ref) = self.git_diff {
            // git diff
            let output = Command::new("git")
                .args(&["diff", "--name-only", git_ref])
                .current_dir(&self.project)
                .output()?;

            let paths = String::from_utf8(output.stdout)?
                .lines()
                .filter(|line| line.ends_with(".bsl"))
                .map(PathBuf::from)
                .collect::<Vec<_>>();

            paths.iter()
                .map(|path| self.path_to_file_id(path))
                .collect()
        } else {
            bail!("Either --changed-files or --git-diff must be specified in incremental mode");
        }
    }
}
```

### GitLab CI Integration

**Example `.gitlab-ci.yml`:**

```yaml
bsl-analysis-incremental:
  stage: test
  image: bsl-analyzer:latest
  script:
    # Получаем список измененных файлов
    - CHANGED_FILES=$(git diff --name-only $CI_COMMIT_BEFORE_SHA...$CI_COMMIT_SHA | grep '\.bsl$' | tr '\n' ',')

    # Запускаем инкрементальный анализ
    - |
      if [ -n "$CHANGED_FILES" ]; then
        bsl-analyzer analyze \
          --project . \
          --incremental \
          --changed-files "$CHANGED_FILES" \
          --output sonarqube.json
      else
        echo "No BSL files changed, skipping analysis"
      fi

    # Отправляем в SonarQube
    - sonar-scanner \
        -Dsonar.externalIssuesReportPaths=sonarqube.json
  only:
    - merge_requests
  cache:
    key: bsl-analyzer-cache
    paths:
      - .bsl-analyzer-cache/

bsl-analysis-full:
  stage: test
  image: bsl-analyzer:latest
  script:
    # Full scan для main ветки
    - bsl-analyzer analyze \
        --project . \
        --output sonarqube.json
    - sonar-scanner \
        -Dsonar.externalIssuesReportPaths=sonarqube.json
  only:
    - main
  cache:
    key: bsl-analyzer-cache
    paths:
      - .bsl-analyzer-cache/
```

## Оценка производительности

### pt_erp проект (121 MB, 25,090 BSL модулей)

**Сценарии:**

#### 1. Full Scan (baseline)
```
Модулей: 25,090
Время: 10-15 секунд
Память: ~500 MB
```

#### 2. Incremental: 1 модуль изменен

**Расчет:**
- Изменен: 1 модуль
- Прямые зависимости: ~5-10 модулей (среднее)
- Обратные зависимости: ~10-20 модулей (кто использует)
- **Итого**: 15-30 модулей

**Время:**
```
Построение ModuleGraph: 2-3 секунды (1 раз, кешируется)
Фильтрация затронутых: < 0.1 секунды
Анализ 30 модулей: (10 сек / 25090) * 30 = 0.012 сек ≈ 10-50 мс
Overhead (I/O, координация): 0.3-0.5 сек
──────────────────────────────────
TOTAL: 0.5-1 секунда ✅

Экономия: 10-15 секунд → 0.5-1 сек = 10x-30x
```

#### 3. Incremental: 5 модулей изменено

**Расчет:**
- Изменено: 5 модулей
- Затронуто: ~100-200 модулей (с зависимостями)

**Время:**
```
Построение ModuleGraph: 2-3 секунды (кешировано)
Анализ 150 модулей: (10 сек / 25090) * 150 = 0.06 сек
Overhead: 0.5 сек
──────────────────────────────────
TOTAL: 1-2 секунды ✅

Экономия: 10-15 секунд → 1-2 сек = 5x-15x
```

#### 4. Incremental: 100 модулей изменено (большой MR)

**Расчет:**
- Изменено: 100 модулей
- Затронуто: ~1000-2000 модулей

**Время:**
```
Анализ 1500 модулей: (10 сек / 25090) * 1500 = 0.6 сек
Overhead: 1 сек
──────────────────────────────────
TOTAL: 3-5 секунд ✅

Экономия: 10-15 секунд → 3-5 сек = 2x-5x
```

### Проект 4 GB (экстраполяция)

**Параметры:**
- BSL модулей: ~80,000
- Full scan: 6-10 минут

**Incremental (1 модуль):**
```
Затронуто: ~50 модулей
Время: 1-2 секунды
Экономия: 180x-600x ✅
```

## Дополнительные возможности ModuleGraph

### 1. Диагностики на основе графа

**UnusedModule (DG001):**
```rust
pub fn unused_modules(graph: &ModuleGraph) -> Vec<Diagnostic> {
    graph.modules()
        .filter(|module| {
            // Модуль неиспользуемый, если:
            // 1. Нет обратных зависимостей
            // 2. Не экспортный модуль (не Global, не в метаданных)
            graph.reverse_dependencies(module.id).is_empty()
                && !module.is_exported()
        })
        .map(|module| Diagnostic {
            code: "UnusedModule",
            message: format!("Модуль {} не используется", module.name),
            severity: DiagnosticSeverity::WARNING,
            range: module.name_range(),
        })
        .collect()
}
```

**CircularDependency (DG002):**
```rust
pub fn circular_dependencies(graph: &ModuleGraph) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut visited = FxHashSet::default();
    let mut stack = Vec::new();

    for module_id in graph.all_module_ids() {
        if !visited.contains(&module_id) {
            find_cycles(graph, module_id, &mut visited, &mut stack, &mut diagnostics);
        }
    }

    diagnostics
}

fn find_cycles(
    graph: &ModuleGraph,
    module_id: ModuleId,
    visited: &mut FxHashSet<ModuleId>,
    stack: &mut Vec<ModuleId>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    visited.insert(module_id);
    stack.push(module_id);

    for dep in graph.dependencies(module_id) {
        if let Some(cycle_start) = stack.iter().position(|&id| id == dep.module_id) {
            // Найден цикл!
            let cycle = &stack[cycle_start..];
            diagnostics.push(Diagnostic {
                code: "CircularDependency",
                message: format!(
                    "Обнаружена циклическая зависимость: {}",
                    cycle.iter()
                        .map(|&id| graph[id].name.as_str())
                        .collect::<Vec<_>>()
                        .join(" → ")
                ),
                severity: DiagnosticSeverity::ERROR,
                range: graph[module_id].name_range(),
            });
        } else if !visited.contains(&dep.module_id) {
            find_cycles(graph, dep.module_id, visited, stack, diagnostics);
        }
    }

    stack.pop();
}
```

**ModuleCoupling (DG003):**
```rust
pub fn module_coupling_metrics(graph: &ModuleGraph, module_id: ModuleId) -> CouplingMetrics {
    CouplingMetrics {
        afferent_coupling: graph.reverse_dependencies(module_id).len(),  // Ca
        efferent_coupling: graph.dependencies(module_id).len(),          // Ce
        instability: calculate_instability(graph, module_id),            // I = Ce / (Ce + Ca)
    }
}

fn calculate_instability(graph: &ModuleGraph, module_id: ModuleId) -> f64 {
    let ce = graph.dependencies(module_id).len() as f64;
    let ca = graph.reverse_dependencies(module_id).len() as f64;

    if ce + ca == 0.0 {
        0.0
    } else {
        ce / (ce + ca)
    }
}
```

### 2. LSP Navigation

**Find Usages:**
```rust
pub fn find_usages(
    db: &dyn Db,
    module_id: ModuleId,
) -> Vec<Location> {
    let graph = db.all_modules();

    graph.reverse_dependencies(module_id)
        .iter()
        .flat_map(|&dependent_id| {
            let file_id = graph[dependent_id].file_id;
            find_references_in_file(db, file_id, module_id)
        })
        .collect()
}
```

**Call Hierarchy:**
```rust
pub fn incoming_calls(
    db: &dyn Db,
    module_id: ModuleId,
) -> Vec<CallHierarchyItem> {
    let graph = db.all_modules();

    graph.reverse_dependencies(module_id)
        .iter()
        .map(|&caller_id| {
            let module = &graph[caller_id];
            CallHierarchyItem {
                name: module.name.to_string(),
                kind: SymbolKind::MODULE,
                uri: file_id_to_uri(db, module.file_id),
                range: module.full_range(),
                selection_range: module.name_range(),
            }
        })
        .collect()
}

pub fn outgoing_calls(
    db: &dyn Db,
    module_id: ModuleId,
) -> Vec<CallHierarchyItem> {
    let graph = db.all_modules();

    graph.dependencies(module_id)
        .iter()
        .map(|dep| {
            let module = &graph[dep.module_id];
            CallHierarchyItem {
                name: module.name.to_string(),
                kind: SymbolKind::MODULE,
                uri: file_id_to_uri(db, module.file_id),
                range: module.full_range(),
                selection_range: module.name_range(),
            }
        })
        .collect()
}
```

## Ограничения и caveats

### 1. Метаданные все равно нужны

**Проблема:**
- Tier 3 диагностики требуют метаданные (Configuration.xml, CommonModules/*.xml)
- Даже при incremental mode нужно загружать все метаданные

**Решение:**
- Метаданные загружаются 1 раз через Salsa с Durability::HIGH
- Кешируются между запусками (можно сохранять на диск)
- Для pt_erp: загрузка 102 MB XML занимает 1-2 секунды, но делается 1 раз

**Оценка:**
- С кешированием: incremental mode экономит 5x-30x
- Без кеширования: incremental mode все равно быстрее, но меньше (2x-5x)

### 2. Некоторые диагностики требуют full scan

**Примеры:**
- `DuplicateFunctionName` - нужно видеть все модули
- `UnusedModule` - нужен полный граф
- `GlobalCoupling` - метрики связности всего проекта

**Решение:**
- Incremental mode по умолчанию для MR (merge requests)
- Full scan для main ветки и scheduled pipelines
- Или: разделить диагностики на incremental-safe и full-scan-only

### 3. Определение "затронутости" - не тривиально

**Вопросы:**
- Изменился комментарий → интерфейс не изменился → нужно проверять зависимые?
- Изменилась приватная функция → интерфейс тот же → нужно проверять?

**Решение:**
- **Консервативный подход**: любое изменение файла → считаем модуль затронутым
- **Оптимистичный подход**: парсим файл, сравниваем интерфейсы (Salsa делает это через хеши)
- **Для CI/CD**: консервативный (безопаснее, все равно быстро)

### 4. Построение графа - overhead

**Проблема:**
- Для pt_erp: парсинг 25,090 файлов для построения графа занимает время
- При incremental mode парсим все файлы, хотя анализируем немного

**Решение:**
- **Кеширование графа**: сохраняем ModuleGraph на диск (JSON/MessagePack)
- **Lazy construction**: парсим файлы для графа on-demand (только по мере надобности)
- **Salsa**: ModuleGraph строится через Salsa, кешируется автоматически

**Оценка:**
- Построение графа с нуля: 2-3 секунды (pt_erp)
- С кешем: < 0.1 секунды
- **Вывод**: критически важно кешировать граф между запусками CI

## План реализации

### Iteration 9.5: ModuleGraph & Incremental CI Mode

**Цель:** Добавить граф зависимостей модулей для инкрементального анализа в CI/CD.

**Задачи:**

#### 1. Core: ModuleGraph (5-7 дней)

**Файлы:**
- `crates/base-db/src/module_graph.rs` - ModuleGraph, ModuleData, Dependency
- `crates/base-db/src/module_graph/builder.rs` - ModuleGraphBuilder
- `crates/base-db/src/lib.rs` - Salsa queries (all_modules, module_dependencies)

**Тесты:**
- `crates/base-db/src/module_graph/tests.rs` - unit tests
- Построение графа из фикстур
- Обнаружение циклических зависимостей
- Транзитивные зависимости

#### 2. Dependency Extraction (3-5 дней)

**Файлы:**
- `crates/hir-def/src/module_deps.rs` - извлечение зависимостей из AST
- Парсинг вызовов функций (прямые зависимости)
- Парсинг `#Использовать` директив (импорты)
- Метаданные: CommonModule dependencies

**Тесты:**
- Извлечение зависимостей из различных конструкций
- Edge cases (циклы, неразрешенные ссылки)

#### 3. Incremental Analysis Engine (3-5 дней)

**Файлы:**
- `crates/ide/src/incremental.rs` - affected_modules, фильтрация
- `crates/base-db/src/module_graph.rs` - affected_modules, transitive_deps

**Тесты:**
- affected_modules для различных сценариев
- Проверка корректности (не пропускаем затронутые модули)

#### 4. CLI Integration (2-3 дня)

**Файлы:**
- `crates/bsl-analyzer/src/cli/analyze.rs` - --incremental, --changed-files, --git-diff
- `crates/bsl-analyzer/src/cli/graph.rs` - команда для визуализации графа

**Тесты:**
- E2E тесты с реальными проектами
- GitLab CI mock tests

#### 5. Graph Caching (2-3 дня)

**Файлы:**
- `crates/base-db/src/module_graph/cache.rs` - сохранение/загрузка графа
- Формат: MessagePack или JSON
- Инвалидация кеша при изменении файлов

**Тесты:**
- Сохранение/загрузка
- Корректность после загрузки

#### 6. Diagnostics на основе графа (3-5 дней)

**Файлы:**
- `crates/ide-diagnostics/src/handlers/unused_module.rs` - DG001
- `crates/ide-diagnostics/src/handlers/circular_dependency.rs` - DG002
- `crates/ide-diagnostics/src/handlers/module_coupling.rs` - DG003

**Тесты:**
- Обнаружение неиспользуемых модулей
- Обнаружение циклов
- Метрики связности

#### 7. LSP Navigation (опционально, 3-5 дней)

**Файлы:**
- `crates/ide/src/call_hierarchy.rs` - incoming/outgoing calls
- `crates/ide/src/references.rs` - find usages через граф

**Тесты:**
- Call hierarchy
- Find usages cross-module

**Итого:** 20-35 дней (3-5 недель)

## Метрики успеха

### Производительность

| Метрика | Целевое значение |
|---------|-----------------|
| **pt_erp incremental (1 модуль)** | < 1 секунда (vs 10-15 сек full) |
| **pt_erp incremental (5 модулей)** | < 2 секунды (vs 10-15 сек full) |
| **4 GB incremental (1 модуль)** | < 2 секунды (vs 6-10 мин full) |
| **Построение графа (pt_erp)** | < 3 секунды |
| **Загрузка графа из кеша** | < 0.1 секунды |

### Корректность

| Метрика | Целевое значение |
|---------|-----------------|
| **False negatives** | 0% (не пропускаем затронутые модули) |
| **False positives** | < 20% (лишние модули в анализе - допустимо) |
| **Cycle detection** | 100% (все циклы обнаружены) |

### CI/CD Integration

| Метрика | Целевое значение |
|---------|-----------------|
| **Время MR pipeline (pt_erp)** | < 5 секунд (vs 10-15 сек) |
| **Время MR pipeline (4 GB)** | < 30 секунд (vs 6-10 мин) |
| **Экономия CI минут** | 5x-30x для типичного MR |

## Альтернативы

### 1. Без ModuleGraph - только Salsa

**Плюсы:**
- Salsa уже отслеживает зависимости автоматически
- Не нужна дополнительная инфраструктура

**Минусы:**
- Salsa работает ВНУТРИ анализа, не для фильтрации входных данных
- Нет способа сказать "анализируй только эти файлы"
- Нет кросс-модульных диагностик (unused modules, cycles)

**Вывод:** Salsa необходима, но недостаточна для incremental CI/CD.

### 2. Файловый уровень вместо модульного

**Идея:** `git diff` → анализируем только измененные файлы (без зависимостей)

**Плюсы:**
- Проще реализовать
- Еще быстрее

**Минусы:**
- **Некорректно:** если изменен CommonModule, нужно проверить все модули, которые его используют
- Пропускаем важные диагностики

**Вывод:** Не подходит для BSL (в отличие от изолированных языков типа Go).

### 3. SonarQube Incremental Mode (встроенный)

**Идея:** SonarQube сам умеет incremental analysis

**Плюсы:**
- Не нужно реализовывать
- SonarQube знает, какие файлы изменились

**Минусы:**
- **Проблема:** SonarQube не знает зависимостей между BSL модулями
- Отдаем SonarQube результаты только по измененным файлам → теряем диагностики в зависимых модулях

**Вывод:** SonarQube incremental mode работает только для изолированных файлов, не для BSL.

## Выводы

### ModuleGraph - критичен для production use case

**Почему:**
1. **CI/CD экономия времени**: 5x-30x для типичных MR
2. **Кросс-модульные диагностики**: unused modules, circular deps
3. **LSP navigation**: call hierarchy, find usages
4. **rust-analyzer паттерн**: проверенная архитектура

**Приоритет:**
- **ВЫСОКИЙ** для SonarQube CI/CD
- **СРЕДНИЙ** для LSP (Salsa уже кеширует, но навигация полезна)
- **СРЕДНИЙ** для диагностик

**Когда реализовывать:**
- После Iteration 10 (Salsa)
- Перед Iteration 12 (Diagnostics) - некоторые диагностики требуют граф
- **Рекомендация:** Iteration 9.5 или 11.5 (между Salsa и Diagnostics)

**Усилия:** 20-35 дней (3-5 недель)

**ROI:** Огромный! Для больших проектов экономия времени в CI/CD окупит разработку за 1-2 месяца.

---

**Следующие шаги:**
1. Добавить Iteration 9.5 в ROADMAP.md
2. Детализировать задачи в ITERATIONS.md
3. Обновить ARCHITECTURE.md с секцией ModuleGraph
