# BSL Analyzer - Architecture

## Обзор

BSL Analyzer построен по образцу rust-analyzer с адаптацией под специфику BSL/1C.

```
┌─────────────────────────────────────────────────────────────┐
│                    bsl-analyzer (LSP Server)                 │
│  - JSON-RPC handling                                         │
│  - LSP protocol implementation                               │
│  - CLI interface                                             │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                         ide                                  │
│  - High-level API for IDE features                          │
│  - Coordinates all subsystems                               │
└─────────────────────────────────────────────────────────────┘
        │                     │                     │
        ▼                     ▼                     ▼
┌───────────────┐    ┌───────────────┐    ┌───────────────┐
│ ide-diagnostics│    │  ide-assists  │    │    ide-db     │
│ - 181 diagnostics│  │ - Code actions│    │ - RootDatabase│
│ - Quick fixes   │  │ - Refactorings│    │ - Queries     │
└───────────────┘    └───────────────┘    └───────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                          hir                                 │
│  - High-level Intermediate Representation                   │
│  - OOP-style API for semantic information                   │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        hir-def                               │
│  - Definitions (modules, methods, variables)                │
│  - Name resolution                                          │
│  - Symbol table                                             │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        syntax                                │
│  - CST (Concrete Syntax Tree) based on Rowan               │
│  - AST typed wrappers                                       │
│  - SyntaxNode, SyntaxToken                                  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        parser                                │
│  - BSL grammar implementation                               │
│  - Error recovery                                           │
│  - SDBL (query language) support                            │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        lexer                                 │
│  - Tokenization of BSL source code                          │
│  - Based on logos crate                                     │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                   Supporting Crates                          │
├─────────────────────────────────────────────────────────────┤
│ base-db      │ Source database, Salsa integration           │
│ vfs          │ Virtual file system                          │
│ bsl-metadata │ 1C metadata (Configuration, CommonModule)    │
│ project-model│ Project structure (configurations, etc.)     │
│ intern       │ String/ID interning                          │
│ stdx         │ Standard library extensions                  │
│ profile      │ Profiling utilities                          │
│ test-fixture │ Test fixtures                                │
│ test-utils   │ Test utilities                               │
└─────────────────────────────────────────────────────────────┘
```

## Ключевые архитектурные решения

### 1. Incremental Computation (Salsa)

**Версия:** Salsa 0.25.2
**Репозиторий:** `/Users/kiriller/src/lsp/salsa/`
**Статус:** ✅ Реализовано

Salsa — фреймворк для инкрементальных вычислений, критично важный для производительности Language Server.

#### Зачем нужна Salsa?

1. **Автоматическая инвалидация кеша**
   - Salsa отслеживает зависимости между queries
   - При изменении входных данных автоматически инвалидирует только затронутые производные queries
   - Не требуется ручное управление кешем

2. **Ленивые вычисления с кешированием**
   - Вычисления происходят только когда результат действительно нужен
   - Промежуточные результаты кешируются
   - Повторные запросы возвращают кешированный результат мгновенно (O(1))

3. **Durability (долговечность данных)**
   - `Durability::HIGH` — библиотеки, метаданные (меняются редко)
   - `Durability::MEDIUM` — зависимости проекта
   - `Durability::LOW` — исходный код пользователя (меняется часто)
   - Salsa оптимизирует проверки на основе durability

4. **Параллельные вычисления**
   - Thread-safe по дизайну
   - Автоматическое распараллеливание независимых queries

#### Типы Queries

**Input Queries** — входные данные, изменяются извне:

```rust
#[salsa::input]
struct FileText {
    file_id: FileId,
    #[returns(as_ref)]
    text: Arc<str>,
}

// Использование:
db.set_file_text(file_id, new_text);  // Изменение инвалидирует зависимые queries
```

**Tracked Queries** — производные запросы, автоматически пересчитываются:

```rust
#[salsa::tracked(lru = 128)]
fn parse(db: &dyn SourceDatabase, file_id: FileId) -> Parse {
    let text = db.file_text(file_id);  // Salsa отследит зависимость
    parser::parse(&text)
}

// При изменении file_text:
// 1. Salsa проверяет, изменился ли текст
// 2. Если да — парсит заново
// 3. Если нет — возвращает кешированный результат
```

#### Применение в BSL Analyzer

**1. Парсинг (базовые queries):**

```rust
// Input — текст файла
#[salsa::input]
struct FileText { /* ... */ }

// Derived — парсинг с LRU кешированием
#[salsa::tracked(lru = 128)]
fn parse(db: &dyn Db, file_id: FileId) -> Parse {
    let text = db.file_text(file_id);
    parser::parse(&text)
}
```

**2. Метаданные (Durability::HIGH):**

```rust
// Input — путь к конфигурации
#[salsa::input]
struct ConfigurationPath {
    #[returns(as_ref)]
    path: PathBuf,
}

// Derived — загрузка метаданных (редко меняются)
#[salsa::tracked(lru = 16, durability = Durability::HIGH)]
fn load_configuration(db: &dyn MetadataDb) -> Arc<Configuration> {
    let path = db.configuration_path();
    bsl_metadata::load_from_directory(&path).unwrap()
}

// Derived — поиск общего модуля
#[salsa::tracked]
fn find_common_module(db: &dyn Db, name: &str) -> Option<Arc<CommonModule>> {
    db.load_configuration()  // Автоматическая зависимость
        .common_modules()
        .find(|m| m.name() == name)
}
```

**3. Semantic Analysis:**

```rust
#[salsa::tracked]
fn module_tree(db: &dyn Db, file_id: FileId) -> Arc<ModuleTree> {
    let ast = db.parse(file_id);  // Зависит от parse
    let config = db.load_configuration();  // Зависит от метаданных
    analyze_module(ast, config)
}
```

#### Преимущества для BSL Analyzer

| Сценарий | Без Salsa | С Salsa |
|----------|-----------|---------|
| Редактирование файла | Пересчёт всех зависимых модулей | Пересчёт только если интерфейс изменился |
| Загрузка метаданных | Каждый запрос парсит XML | Парсинг один раз, далее кеш (< 1ms) |
| Incremental update | 500ms+ | < 50ms (цель: 10x улучшение) |
| Память | Неограниченный рост кеша | LRU автоматически вытесняет старые записи |

#### Производительность (реальные данные + экстраполяция)

**✅ Реальный проект pt_erp (121 MB, 111K файлов):**

| Операция | Java (bsl-ls) | Rust (bsl-analyzer) | Улучшение |
|----------|---------------|---------------------|-----------|
| **SonarQube scan** | 1 час | **10-15 секунд** | **240x-360x** |
| **LSP cold start** | 45-100 сек | **3-5 секунд** | **10x-30x** |
| **LSP incremental** | 500-5000 ms | **< 50 ms** | **10x-100x** |
| **Память** | 2-4 GB | **~500 MB** | **4x-8x меньше** |

**Экстраполяция на 4GB проект:**

| Операция | Java (bsl-ls) | Rust (bsl-analyzer) | Улучшение |
|----------|---------------|---------------------|-----------|
| **SonarQube scan** | ~33 часа | **6-10 минут** | **200x-330x** |
| **Память** | ~99 GB ❌ | **~4-5 GB** ✅ | **20x меньше** |

**Ключевые факторы:**
- ✅ Текущая скорость парсера: 225 MB/s (превышает цель 4.5x)
- ✅ Salsa LRU: держим только 128-512 последних файлов в памяти
- ✅ Durability: метаданные (HIGH) проверяются реже, чем код (LOW)
- ✅ Rayon: автоматическое распараллеливание на все ядра

См. `docs/planning/PERFORMANCE_ESTIMATES.md` для детальных расчетов.

### 2. Rowan для Syntax Trees

Red-green trees для эффективного представления синтаксиса:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntaxKind {
    // Tokens
    L_PAREN,
    R_PAREN,
    IDENT,
    NUMBER,
    STRING,
    // ...

    // Nodes
    SOURCE_FILE,
    FUNCTION_DEF,
    PROCEDURE_DEF,
    STATEMENT,
    EXPRESSION,
    // ...
}

pub type SyntaxNode = rowan::SyntaxNode<BslLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<BslLanguage>;
```

### 3. Diagnostic Infrastructure

Каждая диагностика - отдельный модуль с единообразным интерфейсом:

```rust
pub trait Diagnostic: Send + Sync {
    fn code(&self) -> DiagnosticCode;
    fn message(&self) -> String;
    fn severity(&self) -> Severity;
    fn range(&self) -> TextRange;
    fn fixes(&self) -> Option<Vec<Assist>>;
}

pub struct DiagnosticsContext<'a> {
    pub db: &'a dyn RootDatabase,
    pub config: &'a DiagnosticsConfig,
    pub file_id: FileId,
}
```

### 4. Metadata Infrastructure

**Статус:** ✅ **Реализовано**
**Крейты:** `bsl-metadata`, `ide-db/metadata`

Инфраструктура для работы с метаданными 1С:Enterprise — критически важная часть для полноценного Language Server.

#### Что такое метаданные 1С?

Метаданные — это XML-описание конфигурации 1С:Enterprise, включающее:
- **Configuration** — корневой объект конфигурации
- **CommonModule** — общие модули (Глобальные, Клиентские, Серверные)
- **Catalog/Document** — справочники и документы
- **Registers** — регистры (Information, Accumulation, Accounting, Calculation)
- **Role, Enum** — роли и перечисления
- И другие типы объектов метаданных (~14+ типов)

#### Зачем нужны метаданные?

**1. Tier 3 Diagnostics (~40 диагностик)**
```rust
// Примеры диагностик, требующих метаданные:
- CommonModuleAssign          // Проверка присваивания общему модулю
- CommonModuleInvalidType     // Проверка типов общих модулей
- MissingEventSubscriptionHandler  // Проверка обработчиков подписок
- QueryToMissingMetadata      // Запрос к несуществующим объектам
- ForbiddenMetadataName       // Запрещённые имена метаданных
```

**2. Navigation & Completion**
- Go to Definition для обращений к общим модулям
- Автодополнение имён объектов метаданных
- Поиск использований (Find References)

**3. Semantic Analysis**
- Разрешение имён модулей и объектов
- Проверка доступности модулей (Клиент/Сервер)
- Анализ зависимостей между модулями

**4. SDBL Query Analysis**
- Проверка существования таблиц/регистров в запросах
- Валидация виртуальных таблиц
- Проверка полей объектов метаданных

#### Designer Format Structure

**КРИТИЧЕСКИ ВАЖНО:** XML файлы находятся **РЯДОМ** с папками, не внутри!

```text
Configuration.xml                      # Корневой файл
ConfigDumpInfo.xml                     # Информация о выгрузке

CommonModules/
├── <Name>.xml                         # XML NEXT TO folder
└── <Name>/                            # Folder with code
    └── Ext/
        └── Module.bsl                 # Code INSIDE Ext/

Catalogs/
├── <Name>.xml                         # XML NEXT TO folder
└── <Name>/                            # Folder with code
    └── Ext/
        ├── ManagerModule.bsl
        └── ObjectModule.bsl

InformationRegisters/
├── <Name>.xml                         # XML NEXT TO folder
└── <Name>/                            # Folder with code
    └── Ext/
        └── ManagerModule.bsl
```

#### Архитектура

**Крейт `bsl-metadata`:**

```rust
// Основные структуры (портированы из bsl-language-server-rust)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Configuration {
    uuid: Uuid,
    name: String,
    common_modules: Vec<CommonModule>,
    metadata_objects: Vec<MetadataObject>,
    // HashMap caches (excluded from PartialEq)
    #[serde(skip)]
    uri_to_module: HashMap<String, usize>,
    #[serde(skip)]
    name_to_common_module: HashMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommonModule {
    uuid: Uuid,
    name: String,
    uri: Option<String>,          // Path to .bsl file
    server: bool,                 // Серверный
    global: bool,                 // Глобальный
    client_managed_application: bool,  // Клиент (управляемое приложение)
    server_call: bool,            // Серверный вызов
    privileged: bool,             // Привилегированный
    return_values_reuse: ReturnValueReuse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MdoType {
    Catalog,
    Document,
    InformationRegister,
    // ... ~14+ типов
}
```

**XML Loader (с правильными путями):**

```rust
// Загрузка метаданных из Designer format
pub fn load_from_directory(path: impl AsRef<Path>) -> Result<Configuration> {
    let mut config = Configuration::new("Configuration");

    // Load CommonModules
    for entry in fs::read_dir(path.join("CommonModules"))? {
        let path = entry.path();
        // XML files are NEXT TO folders!
        if path.is_file() && path.extension() == Some("xml") {
            let name = path.file_stem().unwrap().to_str().unwrap();
            let xml = fs::read_to_string(&path)?;
            let mut module = xml_parser::parse_common_module_xml(&xml)?;

            // Build URI to .bsl file (INSIDE Ext/)
            let module_bsl = format!("CommonModules/{}/Ext/Module.bsl", name);
            config.add_common_module(module);
        }
    }

    Ok(config)
}
```

**Интеграция с Salsa (критично для производительности):**

```rust
// Input query — путь к конфигурации
#[salsa::input(debug)]
pub struct ConfigurationPathInput {
    pub path: String,  // Stored as String for Salsa
}

// Tracked query — загрузка конфигурации
// LRU cache: 16 configurations (multi-workspace support)
#[salsa::tracked(lru = 16)]
pub fn load_configuration(
    db: &dyn salsa::Database,
    path_input: ConfigurationPathInput,
) -> Arc<Configuration> {
    let path = PathBuf::from(path_input.path(db));
    let config = bsl_metadata::load_from_directory(&path)
        .unwrap_or_else(|_| Configuration::new("Configuration"));
    Arc::new(config)
}

// Database trait
#[salsa::db]
pub trait MetadataDb: salsa::Database {
    fn load_configuration(&self, path_input: ConfigurationPathInput) -> Arc<Configuration> {
        load_configuration(self, path_input)
    }
}
```

**Почему Salsa критична для метаданных:**

1. **Редко меняются** — загружаются 1 раз при открытии workspace
2. **Дорого загружать** — XML парсинг + file I/O (~1 секунда)
3. **Часто используются** — каждая Tier 3 диагностика запрашивает метаданные
4. **PartialEq requirement** — все структуры реализуют PartialEq для Salsa caching
5. **Результат:** Загрузка 1 раз (~1 сек), далее кеширование (< 1ms)

#### AbstractMetadataDiagnostic Pattern

Портирован из bsl-language-server (Java):

```rust
pub trait MetadataDiagnostic {
    /// Фильтр типов метаданных для проверки
    fn filter_mdo_types(&self) -> &[MdoType];

    /// Проверка объекта метаданных
    fn check_metadata(&self, ctx: &DiagnosticContext, mdo: &dyn MetadataObject);
}

// Пример использования:
impl MetadataDiagnostic for CommonModuleAssignDiagnostic {
    fn filter_mdo_types(&self) -> &[MdoType] {
        &[MdoType::COMMON_MODULE]
    }

    fn check_metadata(&self, ctx: &DiagnosticContext, mdo: &dyn MetadataObject) {
        let common_module = mdo.as_common_module().unwrap();
        if common_module.global() {
            // Проверка присваивания глобальному модулю
            // ...
        }
    }
}
```

#### Метрики производительности

| Операция | Результат | Обоснование |
|----------|-----------|-------------|
| Загрузка конфигурации | < 1 сек | Холодный старт LSP сервера |
| Кешированный доступ | < 1 мс | Каждая диагностика может запрашивать метаданные |
| Память (ERP 2.5) | < 100 MB | Не должны доминировать в потреблении памяти |

#### Реализация

- ✅ **Крейт bsl-metadata** — все базовые структуры (Configuration, CommonModule, MetadataObject)
- ✅ **XML Loader** — парсинг Designer format (CommonModules, InformationRegisters, Catalogs, Documents)
- ✅ **Salsa Integration** — MetadataDb trait, load_configuration query с кешированием
- ✅ **PartialEq support** — все структуры поддерживают Salsa caching
- ✅ **Тесты** — 14 unit tests в bsl-metadata, 2 integration tests в ide-db
- ✅ **Производительность** — загрузка < 1 сек, кешированный доступ < 1 мс

### 5. LSP Compatibility

Полная совместимость с bsl-language-server:

- Те же коды диагностик
- Те же severity levels
- Те же параметры конфигурации
- Тот же формат .bslls.json

## Структура крейтов

### bsl-analyzer
Главный бинарник, LSP сервер:
- `main.rs` - точка входа
- `config.rs` - конфигурация
- `handlers/` - обработчики LSP requests
- `dispatch.rs` - роутинг запросов

### lexer
Лексический анализатор:
- `lib.rs` - токенизация
- `token_kind.rs` - виды токенов
- `tests.rs` - тесты

### parser
Парсер BSL:
- `grammar/` - грамматика BSL
  - `expressions.rs`
  - `statements.rs`
  - `items.rs`
- `event.rs` - события парсинга
- `parser.rs` - основной парсер

### syntax
Синтаксические деревья:
- `syntax_kind.rs` - виды узлов
- `ast/` - типизированные обёртки
- `algo.rs` - алгоритмы обхода

### hir / hir-def
Семантический анализ:
- `module.rs` - представление модуля
- `symbols.rs` - таблица символов
- `resolver.rs` - разрешение имён

### ide-diagnostics
Диагностики:
- `lib.rs` - инфраструктура
- `handlers/` - 181 диагностика

### bsl-metadata
Метаданные 1С:
- `lib.rs` - публичный API с примерами использования
- `configuration.rs` - Configuration (с PartialEq для Salsa)
- `common_module.rs` - CommonModule (все execution context flags)
- `metadata_object.rs` - MetadataObject + MdoType enum
- `loader.rs` - загрузка из Designer format
- `xml_parser.rs` - парсинг XML с quick-xml + serde
- `traits.rs` - MdObject, Module traits
- `enums.rs` - ReturnValueReuse, ModuleType, ObjectBelonging, SupportVariant
- `error.rs` - MetadataError + Result type

## Потоки данных

### Parsing Flow
```
Source Text → Lexer → Tokens → Parser → GreenNode → SyntaxNode → AST
```

### Diagnostic Flow

**Tier 1-2 (Syntax/Semantic):**
```
AST → HIR → DiagnosticContext → [Diagnostics] → LSP Diagnostics
```

**Tier 3 (Metadata-dependent):**
```
                     ┌─→ load_configuration() (Salsa, Durability::HIGH)
                     │   └─→ Arc<Configuration> (cached)
                     │
AST → HIR → DiagnosticContext ─→ MetadataDiagnostic::check_metadata()
                                 └─→ [Diagnostics] → LSP Diagnostics
```

### Metadata Loading Flow
```
Configuration Path (Input Query)
         │
         ▼
    Salsa Check (changed?)
         │
         ├─→ NO  → Return Cached Arc<Configuration> (< 1ms)
         │
         └─→ YES → XML Loader:
                   ├─→ Parse Configuration.xml
                   ├─→ Parse CommonModules/*.xml
                   ├─→ Parse Catalogs/*/...xml
                   ├─→ Parse Documents/*/...xml
                   └─→ Parse Registers/*/...xml
                         │
                         ▼
                   Arc<Configuration> → Salsa Cache (Durability::HIGH)
```

### Incremental Update
```
File Change → VFS Update → Salsa Invalidation → Recompute Affected Queries
                                │
                                ├─→ .bsl file changed → Invalidate parse(), HIR
                                │                        (Metadata NOT invalidated)
                                │
                                └─→ Configuration.xml changed → Invalidate load_configuration()
                                                                → Invalidate dependent queries
```

## Паттерны кода

### Context Pattern
```rust
pub struct AnalysisContext<'db> {
    pub db: &'db dyn RootDatabase,
    pub file_id: FileId,
}

impl<'db> AnalysisContext<'db> {
    pub fn syntax_tree(&self) -> &SyntaxNode {
        self.db.parse(self.file_id).syntax()
    }
}
```

### Visitor Pattern (для диагностик)
```rust
pub trait SyntaxVisitor {
    fn visit_function(&mut self, func: &ast::Function) {}
    fn visit_procedure(&mut self, proc: &ast::Procedure) {}
    fn visit_statement(&mut self, stmt: &ast::Statement) {}
    // ...
}
```

### InFile<T> для привязки к файлу
```rust
pub struct InFile<T> {
    pub file_id: FileId,
    pub value: T,
}
```

## Сравнение с bsl-language-server

| Аспект | BSL-LS (Java) | BSL Analyzer (Rust) |
|--------|---------------|---------------------|
| Парсер | ANTLR 4 | Hand-written + Rowan |
| Инкрементальность | Нет | Salsa (0.25.2) |
| Метаданные | mdclasses (рефлексия) | bsl-metadata + Salsa (кеширование) |
| Memory model | GC (JVM) | Manual (Rust) |
| Concurrency | ForkJoinPool | Rayon / async |
| AST | ANTLR generated | Rowan + typed wrappers |
| Metadata кеш | Нет | Salsa (Durability::HIGH, LRU) |

## Логирование

### Трассировка и профилирование (tracing)

Используем **tracing ecosystem** по примеру rust-analyzer:

```toml
[workspace.dependencies]
tracing = { version = "0.1", default-features = false, features = ["std"] }
tracing-subscriber = { version = "0.3", default-features = false, features = [
    "registry",
    "fmt",
    "std",
    "tracing-log",
] }
tracing-tree = "0.4"
```

### Архитектура логирования

```
┌─────────────────────────────────────────────────────────┐
│                   Application Code                       │
│  tracing::info!("message", field = ?value)              │
│  let _span = tracing::info_span!("operation").entered() │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────┐
│                  tracing Registry                        │
│  (координирует несколько слоев)                         │
└─────────────────────────────────────────────────────────┘
            │               │               │
            ▼               ▼               ▼
┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│Format Layer │  │Profiler Layer│  │JSON Layer  │
│(stderr/file)│  │(timing tree) │  │(metrics)   │
└─────────────┘  └─────────────┘  └─────────────┘
```

### Инициализация логирования

Инициализация в `bsl-analyzer/src/bin/main.rs`:

```rust
fn setup_logging(log_file: Option<PathBuf>) -> anyhow::Result<()> {
    let writer = match log_file {
        Some(file) => BoxMakeWriter::new(Arc::new(fs::File::create(&file)?)),
        None => BoxMakeWriter::new(std::io::stderr),
    };

    bsl_analyzer::tracing::Config {
        writer,
        filter: env::var("BSL_LOG").ok().unwrap_or_else(|| "warn".to_owned()),
        profile_filter: env::var("BSL_PROFILE").ok(),
    }
    .init()?;

    Ok(())
}
```

### Environment Variables

- **BSL_LOG** - уровень логирования (default: "warn")
  - `BSL_LOG=debug` - весь debug output
  - `BSL_LOG=info` - общая информация
  - `BSL_LOG=parser=debug` - debug только для parser
  - Синтаксис: `target=level` (EnvFilter)

- **BSL_LOG_FILE** - запись логов в файл
  - `BSL_LOG_FILE=/tmp/bsl-analyzer.log`

- **BSL_PROFILE** - иерархическое профилирование
  - `BSL_PROFILE=*` - все spans
  - `BSL_PROFILE=parse|analyze` - только parse и analyze spans
  - `BSL_PROFILE=*@3>10` - глубина 3, > 10ms

### Паттерны использования

#### Базовое логирование

```rust
use tracing::{trace, debug, info, warn, error};

// Простое сообщение
info!("parsing started");

// С полями (structured logging)
debug!(file_id = ?file_id, "parsing file");

// С несколькими полями
warn!(
    line = line_number,
    column = column,
    "syntax error"
);
```

#### Spans для профилирования

```rust
// Span на время выполнения функции
pub fn parse_file(input: &str) -> Parse {
    let _span = tracing::info_span!("parse_file", len = input.len()).entered();

    // ... parsing logic ...
}

// Span для измерения производительности
pub fn run_diagnostics(&self, db: &dyn RootDatabase) {
    let _p = tracing::info_span!("run_diagnostics").entered();

    for diagnostic in &self.diagnostics {
        let _d = tracing::debug_span!("diagnostic", code = %diagnostic.code()).entered();
        diagnostic.check(db);
    }
}
```

#### Вложенные spans (иерархия)

```rust
fn analyze_project() {
    let _p = tracing::info_span!("analyze_project").entered();

    for module in modules {
        let _m = tracing::debug_span!("analyze_module", name = %module.name()).entered();

        for function in module.functions() {
            let _f = tracing::trace_span!("analyze_function").entered();
            // ...
        }
    }
}
```

Вывод при `BSL_PROFILE=*`:
```
1234ms    analyze_project
  456ms    analyze_module name="CommonModule"
    12ms    analyze_function
    23ms    analyze_function
  234ms    analyze_module name="ObjectModule"
```

### Рекомендации

1. **Использовать spans для всех значимых операций:**
   - Parsing
   - Semantic analysis
   - Diagnostics
   - LSP requests

2. **Уровни логирования:**
   - `error!` - критические ошибки
   - `warn!` - предупреждения (default level)
   - `info!` - общая информация (start/stop operations)
   - `debug!` - детальная отладочная информация
   - `trace!` - очень детальная информация (loop iterations)

3. **Structured logging:**
   - Использовать поля вместо string formatting
   - `debug!(file = ?file_id, "parsing")` вместо `debug!("parsing {:?}", file_id)`

4. **Guard pattern:**
   - `let _span = ...` для автоматического drop при выходе из scope
   - Имя переменной с `_` чтобы clippy не ругался

## Тестирование

### Unit Tests
Каждый крейт содержит тесты в `tests/` или inline `#[cfg(test)]`

### Integration Tests
Фикстуры в формате:
```
//- /main.bsl
Процедура Тест()
    // код
КонецПроцедуры
```

### Snapshot Tests
Используем `expect-test` для snapshot тестирования
