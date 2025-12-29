# Metadata Infrastructure Plan

**Дата создания:** 2025-12-29
**Целевая итерация:** 10-11 (после IDE-DB, до начала Tier 3 диагностик)

## Зачем нужны метаданные?

Метаданные 1С:Enterprise (конфигурация, расширения, общие модули и т.д.) — критически важная часть Language Server для BSL:

### Основные use cases:

1. **Tier 3 Diagnostics (19-23 итерации)**
   - CommonModuleAssign — проверка присваивания общим модулям
   - CommonModuleInvalidType — проверка типов общих модулей
   - MissingEventSubscriptionHandler — проверка обработчиков подписок
   - QueryToMissingMetadata — проверка запросов к несуществующим объектам
   - ForbiddenMetadataName — проверка запрещённых имён метаданных
   - И ещё ~30+ диагностик, требующих метаданные

2. **Navigation & Completion**
   - Go to Definition для обращений к общим модулям
   - Автодополнение имён объектов метаданных
   - Поиск использований (Find References)

3. **Semantic Analysis**
   - Разрешение имён модулей и объектов
   - Проверка доступности модулей (Клиент/Сервер)
   - Анализ зависимостей между модулями

4. **SDBL Query Analysis (24-25 итерации)**
   - Проверка существования таблиц/регистров
   - Валидация виртуальных таблиц
   - Проверка полей объектов метаданных

## Что такое метаданные 1С?

Метаданные — это XML-описание конфигурации 1С:Enterprise. Включает:

### Основные типы объектов метаданных (MDO):

| Тип | Описание | Файлы |
|-----|----------|-------|
| Configuration | Корневой объект конфигурации | `Configuration.xml` |
| CommonModule | Общие модули (Глобальные, Клиентские, Серверные) | `CommonModules/<Name>.xml` |
| Catalog | Справочники | `Catalogs/<Name>/<Name>.xml` |
| Document | Документы | `Documents/<Name>/<Name>.xml` |
| InformationRegister | Регистры сведений | `InformationRegisters/<Name>/<Name>.xml` |
| AccumulationRegister | Регистры накопления | `AccumulationRegisters/<Name>/<Name>.xml` |
| AccountingRegister | Регистры бухгалтерии | `AccountingRegisters/<Name>/<Name>.xml` |
| CalculationRegister | Регистры расчёта | `CalculationRegisters/<Name>/<Name>.xml` |
| Role | Роли | `Roles/<Name>.xml` |
| Enum | Перечисления | `Enums/<Name>.xml` |
| BusinessProcess | Бизнес-процессы | `BusinessProcesses/<Name>/<Name>.xml` |
| Task | Задачи | `Tasks/<Name>/<Name>.xml` |
| ExchangePlan | Планы обмена | `ExchangePlans/<Name>/<Name>.xml` |
| ChartOfAccounts | Планы счетов | `ChartsOfAccounts/<Name>/<Name>.xml` |

### Структура файловой системы конфигурации:

```
Configuration.xml                  # Корневой файл конфигурации
CommonModules/
├── ОбщийМодуль1/
│   ├── ОбщийМодуль1.xml         # Метаданные модуля
│   └── Module.bsl                 # Код модуля
├── GlobalModule/
│   ├── GlobalModule.xml
│   └── Module.bsl
Catalogs/
├── Номенклатура/
│   ├── Номенклатура.xml          # Метаданные справочника
│   ├── ManagerModule.bsl          # Модуль менеджера
│   └── ObjectModule.bsl           # Модуль объекта
Documents/
├── РеализацияТоваровУслуг/
│   ├── РеализацияТоваровУслуг.xml
│   ├── ManagerModule.bsl
│   └── ObjectModule.bsl
InformationRegisters/
├── ЦеныНоменклатуры/
│   ├── ЦеныНоменклатуры.xml
│   └── RecordSetModule.bsl
Roles/
├── ПолныеПрава/
│   └── ПолныеПрава.xml
```

### Пример CommonModule.xml:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.20">
  <CommonModule uuid="...">
    <Properties>
      <Name>ОбщегоНазначения</Name>
      <Synonym>
        <v8:item>
          <v8:lang>ru</v8:lang>
          <v8:content>Общего назначения</v8:content>
        </v8:item>
      </Synonym>
      <Server>true</Server>
      <Global>true</Global>
      <ClientManagedApplication>true</ClientManagedApplication>
      <ExternalConnection>true</ExternalConnection>
      <ClientOrdinaryApplication>true</ClientOrdinaryApplication>
      <ServerCall>false</ServerCall>
      <Privileged>false</Privileged>
      <ReturnValueReuse>DontUse</ReturnValueReuse>
    </Properties>
  </CommonModule>
</MetaDataObject>
```

## Архитектура Metadata Infrastructure

### Крейты:

```
bsl-metadata/          # Структуры метаданных
├── src/
│   ├── lib.rs         # Публичный API
│   ├── configuration.rs   # Configuration
│   ├── common_module.rs   # CommonModule
│   ├── metadata_object.rs # MetadataObject trait
│   ├── enums.rs       # ModuleType, MdoType, ReturnValueReuse
│   ├── loader.rs      # XML parsing & loading
│   ├── error.rs       # MetadataError
│   └── ...            # Role, ScheduledJob, etc.

ide-db/               # Интеграция с Salsa
├── src/
│   ├── lib.rs        # RootDatabase
│   ├── metadata.rs   # Salsa queries для метаданных
│   └── ...
```

### Основные компоненты:

#### 1. Структуры метаданных (bsl-metadata)

**Уже реализовано в bsl-language-server-rust:**

```rust
pub struct Configuration {
    name: String,
    uuid: Uuid,
    common_modules: Vec<CommonModule>,
    catalogs: Vec<MetadataObject>,
    documents: Vec<MetadataObject>,
    // ... другие типы
}

pub struct CommonModule {
    name: String,
    uuid: Uuid,
    server: bool,
    global: bool,
    client_managed_application: bool,
    server_call: bool,
    privileged: bool,
    return_value_reuse: ReturnValueReuse,
    module_type: ModuleType,
}

pub enum ModuleType {
    CommonModule,
    ObjectModule,
    ManagerModule,
    RecordSetModule,
    SessionModule,
    // ... и т.д.
}

pub enum MdoType {
    CATALOG,
    DOCUMENT,
    INFORMATION_REGISTER,
    COMMON_MODULE,
    // ... и т.д.
}
```

#### 2. Loader (XML parsing)

**Нужно реализовать:**

```rust
// Использовать quick-xml или roxmltree
pub fn load_from_directory(path: &Path) -> Result<Configuration, MetadataError> {
    // 1. Прочитать Configuration.xml
    let config_path = path.join("Configuration.xml");
    let config_xml = std::fs::read_to_string(config_path)?;

    // 2. Распарсить XML
    let config = parse_configuration(&config_xml)?;

    // 3. Загрузить CommonModules
    let common_modules_dir = path.join("CommonModules");
    for entry in std::fs::read_dir(common_modules_dir)? {
        let module_xml = entry.path().join("*.xml");
        let module = parse_common_module(&module_xml)?;
        config.add_common_module(module);
    }

    // 4. Загрузить другие типы (Catalogs, Documents, ...)

    Ok(config)
}
```

**Референсы:**
- `bsl-language-server-rust/crates/bsl-metadata/src/loader.rs` — Rust реализация
- `bsl-language-server` (Java) — mdclasses библиотека

#### 3. Salsa Integration (ide-db)

**Ключевой момент:** Метаданные должны кешироваться через Salsa.

```rust
// Input query — путь к конфигурации
#[salsa::input]
struct ConfigurationPath {
    #[returns(as_ref)]
    path: PathBuf,
}

// Derived query — загрузка конфигурации
// Durability::HIGH — метаданные меняются редко
#[salsa::tracked(lru = 16, durability = Durability::HIGH)]
fn load_configuration(db: &dyn MetadataDb, path_id: ConfigurationPathId)
    -> Arc<Configuration>
{
    let path = db.lookup_configuration_path(path_id);
    Arc::new(bsl_metadata::load_from_directory(&path).unwrap())
}

// Derived query — поиск общего модуля по имени
#[salsa::tracked]
fn find_common_module(db: &dyn MetadataDb, name: &str)
    -> Option<Arc<CommonModule>>
{
    let config = db.load_configuration(/* config_path_id */);
    config.common_modules()
        .find(|m| m.name() == name)
        .cloned()
}

// Derived query — проверка существования объекта метаданных
#[salsa::tracked]
fn metadata_object_exists(db: &dyn MetadataDb, name: &str) -> bool {
    let config = db.load_configuration(/* config_path_id */);
    config.find_metadata_object(name).is_some()
}
```

**Зачем Salsa для метаданных?**

1. **Кеширование:** Загружаем конфигурацию один раз, возвращаем кешированный `Arc<Configuration>`
2. **Инвалидация:** При изменении `Configuration.xml` Salsa автоматически инвалидирует все зависимые queries
3. **Durability::HIGH:** Метаданные меняются редко → Salsa проверяет их реже чем исходный код
4. **LRU:** Для больших проектов с множеством конфигураций

#### 4. Traits и API

```rust
pub trait MetadataDb: salsa::Database {
    /// Путь к конфигурации
    fn configuration_path(&self) -> &Path;

    /// Загрузить конфигурацию (через Salsa)
    fn configuration(&self) -> Arc<Configuration>;

    /// Найти общий модуль по имени
    fn find_common_module(&self, name: &str) -> Option<Arc<CommonModule>>;

    /// Найти объект метаданных по имени
    fn find_metadata_object(&self, name: &str) -> Option<Arc<dyn MetadataObject>>;

    /// Получить все общие модули
    fn common_modules(&self) -> Vec<Arc<CommonModule>>;
}
```

## План реализации

### Iteration 10: Metadata Infrastructure Foundation

**Источники:**
- `bsl-language-server-rust/crates/bsl-metadata/` — готовые структуры
- `bsl-language-server` (Java) — mdclasses интеграция
- `salsa/` — для кеширования
- `rust-analyzer/crates/base-db/` — примеры Salsa queries

**Задачи:**

#### 1. Создать крейт bsl-metadata (2-3 дня)

- [ ] Скопировать структуры из `bsl-language-server-rust/crates/bsl-metadata/`
  - Configuration, CommonModule, MetadataObject
  - Enums: ModuleType, MdoType, ReturnValueReuse, ObjectBelonging
- [ ] Добавить traits: MdObject, Module
- [ ] Добавить error handling: MetadataError
- [ ] Unit tests для структур

**Референс:** `bsl-language-server-rust/crates/bsl-metadata/src/`

#### 2. Реализовать XML loader (3-4 дня)

- [ ] Выбрать XML библиотеку (quick-xml или roxmltree)
  - Использовать Context7 для изучения документации
- [ ] Реализовать `parse_configuration()`
- [ ] Реализовать `parse_common_module()`
- [ ] Реализовать загрузку других типов (Catalog, Document, Register)
- [ ] Обработка ошибок парсинга
- [ ] Тесты с реальными XML файлами из bsl-language-server

**Референс:**
- `bsl-language-server-rust/crates/bsl-metadata/src/loader.rs`
- `bsl-language-server` (Java) mdclasses

#### 3. Интеграция с Salsa (3-4 дня)

- [ ] Изучить актуальную документацию Salsa 0.25.2
  - Использовать `/Users/kiriller/src/lsp/salsa/book/`
  - Изучить примеры в `/Users/kiriller/src/lsp/salsa/tests/`
- [ ] Создать Salsa queries для метаданных в `ide-db/src/metadata.rs`:
  - `configuration_path()` — input query
  - `load_configuration()` — derived query с `Durability::HIGH`
  - `find_common_module()` — derived query
  - `metadata_object_exists()` — derived query
- [ ] Добавить `MetadataDb` trait
- [ ] Интегрировать в `RootDatabase`
- [ ] Тесты инкрементальности:
  - Изменение файла модуля не должно триггерить перезагрузку метаданных
  - Изменение Configuration.xml должно триггерить перезагрузку

**Референс:**
- `rust-analyzer/crates/base-db/src/input.rs` — примеры input queries
- `SALSA_TODO.md` — план интеграции Salsa

#### 4. Тестирование (2-3 дня)

- [ ] Скопировать тестовые конфигурации из bsl-language-server:
  - `src/test/resources/metadata/` → `crates/bsl-metadata/fixtures/`
- [ ] Unit tests для loader
- [ ] Integration tests для Salsa queries
- [ ] Performance tests:
  - Загрузка большой конфигурации (ERP 2.5)
  - Многократные запросы (проверка кеширования)
  - Incremental updates

#### 5. Документация (1-2 дня)

- [ ] Обновить `ARCHITECTURE.md` — добавить раздел про метаданные
- [ ] Doc comments для публичного API
- [ ] Примеры использования в `bsl-metadata/examples/`

**Критерии готовности:**
- ✅ Все структуры метаданных реализованы
- ✅ XML loader работает с реальными конфигурациями
- ✅ Salsa queries корректно кешируют результаты
- ✅ Тесты покрывают основные сценарии
- ✅ Performance: загрузка конфигурации < 1 сек, кешированный доступ < 1 мс

### Iteration 11: Metadata API & Tier 3 Preparation

**Цель:** Подготовить API для использования в Tier 3 диагностиках.

**Задачи:**

#### 1. Расширенный API (2-3 дня)

- [ ] Реализовать `find_metadata_object_by_type()`
- [ ] Реализовать `get_module_owner()` — связь модуля с объектом метаданных
- [ ] Реализовать `validate_common_module_properties()`
- [ ] Реализовать queries для SDBL:
  - `find_catalog()`, `find_document()`, `find_register()`
  - `validate_query_metadata()` — проверка существования таблиц

#### 2. AbstractMetadataDiagnostic (2-3 дня)

Портировать паттерн из Java:

```rust
pub trait MetadataDiagnostic {
    /// Фильтр типов метаданных для проверки
    fn filter_mdo_types(&self) -> &[MdoType];

    /// Проверка объекта метаданных
    fn check_metadata(&self, ctx: &DiagnosticContext, mdo: &dyn MetadataObject);
}

pub struct MetadataDiagnosticRunner;

impl MetadataDiagnosticRunner {
    pub fn run<D: MetadataDiagnostic>(
        db: &dyn MetadataDb,
        diagnostic: &D,
        file_id: FileId,
    ) -> Vec<Diagnostic> {
        let config = db.configuration();
        let module_type = db.module_type(file_id);

        // Фильтрация объектов метаданных
        config.children()
            .filter(|mdo| diagnostic.filter_mdo_types().contains(&mdo.mdo_type()))
            .flat_map(|mdo| {
                let mut ctx = DiagnosticContext::new(db, file_id);
                diagnostic.check_metadata(&mut ctx, mdo.as_ref());
                ctx.diagnostics()
            })
            .collect()
    }
}
```

**Референс:** `bsl-language-server/.../.../diagnostics/AbstractMetadataDiagnostic.java`

#### 3. Примеры диагностик (2-3 дня)

Реализовать 2-3 простые диагностики как proof-of-concept:

- [ ] **CommonModuleAssign** — проверка присваивания общему модулю
- [ ] **ForbiddenMetadataName** — проверка запрещённых имён
- [ ] **MetadataObjectNameLength** — проверка длины имён

**Референс:**
- `bsl-language-server/src/main/java/.../diagnostics/CommonModuleAssignDiagnostic.java`
- `bsl-language-server-rust/crates/bsl-diagnostics/src/rules/`

**Критерии готовности:**
- ✅ API удобен для написания metadata-диагностик
- ✅ 2-3 примера диагностик работают
- ✅ Тесты проходят

## Зависимости

### До Metadata Infrastructure нужно завершить:

- [x] Iteration 5: Base Infrastructure (VFS, SourceDatabase)
- [x] Iteration 6-8: HIR/Symbol Resolution — ✅ ЗАВЕРШЕНО (2025-12-30)
- [ ] Iteration 9.5: ModuleGraph — для cross-module dependencies
- [ ] Iteration 10: IDE-DB с полной Salsa интеграцией

### После Metadata Infrastructure:

- [ ] Iteration 19-23: Tier 3 Diagnostics (используют метаданные)
- [ ] Iteration 24-25: SDBL Diagnostics (используют метаданные для проверки запросов)

## Риски и решения

### Риск 1: Сложность XML парсинга

**Проблема:** XML 1С имеет сложную структуру, вложенность, namespace.

**Решение:**
- Использовать готовые структуры из `bsl-language-server-rust/bsl-metadata/`
- Референс: Java mdclasses — проверенная реализация
- Постепенная миграция: начать с CommonModule, потом другие типы

### Риск 2: Salsa интеграция

**Проблема:** Salsa 0.25.2 имеет сложный API (см. `SALSA_TODO.md`).

**Решение:**
- Изучить `/Users/kiriller/src/lsp/salsa/` — локальный репозиторий
- Использовать примеры из `rust-analyzer/crates/base-db/`
- Начать с простого: один input query (configuration_path) + один derived (load_configuration)
- Использовать Context7 для актуальной документации Salsa

### Риск 3: Производительность загрузки больших конфигураций

**Проблема:** ERP 2.5 — огромная конфигурация (тысячи объектов).

**Решение:**
- Ленивая загрузка: загружать только запрошенные типы
- Salsa LRU: ограничить размер кеша
- Incremental loading: не перезагружать неизменённые объекты
- Benchmark: профилировать загрузку реальных конфигураций

### Риск 4: Совместимость с Java версией

**Проблема:** Нужно обеспечить 100% совместимость с bsl-language-server.

**Решение:**
- Использовать те же имена типов (MdoType, ModuleType)
- Копировать тесты из Java версии
- Проверять результаты диагностик на одинаковость

## Метрики успеха

| Метрика | Цель | Обоснование |
|---------|------|-------------|
| Время загрузки конфигурации | < 1 сек | Холодный старт LSP сервера |
| Кешированный доступ | < 1 мс | Каждая диагностика может запрашивать метаданные |
| Память | < 100 MB для ERP 2.5 | Не должны доминировать в потреблении памяти |
| Покрытие тестами | > 80% | Критическая инфраструктура |
| Совместимость | 100% | Tier 3 диагностики должны работать идентично Java |

## Ресурсы

### Исходники для изучения:

1. **bsl-language-server-rust** — готовые Rust компоненты
   - `/Users/kiriller/src/lsp/bsl-language-server-rust/crates/bsl-metadata/`

2. **bsl-language-server** — Java референс
   - mdclasses интеграция
   - AbstractMetadataDiagnostic паттерн

3. **salsa** — инкрементальные вычисления
   - `/Users/kiriller/src/lsp/salsa/`
   - Документация: `/Users/kiriller/src/lsp/salsa/book/`
   - Примеры: `/Users/kiriller/src/lsp/salsa/tests/`

4. **rust-analyzer** — примеры использования
   - `/Users/kiriller/src/lsp/rust-analyzer/crates/base-db/`
   - `/Users/kiriller/src/lsp/rust-analyzer/crates/ide-db/`

### Инструменты:

- **Context7 MCP:** Для актуальной документации библиотек (quick-xml, roxmltree, salsa)
- **Tracing:** BSL_LOG=debug для отладки загрузки метаданных
- **Profiling:** BSL_PROFILE=* для профилирования производительности

---

**Следующие шаги:**
1. ✅ ~~Завершить Iteration 6-8 (HIR/Symbol Resolution)~~ — ЗАВЕРШЕНО (2025-12-30)
2. Реализовать Iteration 9.5 (ModuleGraph & Incremental CI)
3. Завершить полную интеграцию Salsa (Iteration 10: IDE-DB)
4. Начать Iteration 11: Metadata Infrastructure Foundation
