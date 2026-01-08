# План портирования автодополнения из RDT1C в bsl-analyzer

## Резюме

RDT1C содержит отличную систему автодополнения с богатой базой данных о платформе 1С. Можно переиспользовать **данные о платформе** и **алгоритмы анализа контекста**, но потребуется полная переработка кода с BSL на Rust.

## 1. Что можно переиспользовать из RDT1C

### 1.1 Данные о платформе (ЗОЛОТАЯ ЖИЛА 💎)

**Расположение:** `/src/DataProcessors/ирПлатформа/Templates/`

#### Ключевые файлы с данными:

| Файл | Строк | Описание |
|------|-------|----------|
| **ТаблицаОбщихТипов** | 51,635 | Все типы платформы 1С |
| **ТаблицаМетодовИСвойств** | 370,587 | Методы и свойства всех типов |
| **ТаблицаПараметровМетодов** | 109,776 | Параметры методов |
| **ТаблицаИменЭлементовКоллекций** | 2,917 | Элементы коллекций |

**Формат данных:** Сериализованная таблица значений 1С (внутренний формат)

#### Структура ТаблицаМетодовИСвойств:

```
{
    ТипКонтекста: "HTTPЗапрос",           // К какому типу относится
    Слово: "Заголовки",                    // Имя метода/свойства
    НСлово: "заголовки",                   // В нижнем регистре
    ПутьКОписанию: "/objects/.../",        // Путь к HTML описанию
    ТипСлова: "Свойство" | "Метод",        // Тип элемента
    ТипЗначения: "Соответствие",           // Возвращаемый тип
    ЯзыкПрограммы: 0,                      // 0=любой, 1=RU, 2=EN
    ТипЯзыка: "ИмяТипа",                   // Категория
    НомерВерсииПлатформы: 802000,          // 8.2.0 = минимальная версия
    НеТолстыйКлиент: false,                // Доступность по контексту
    НеТонкийКлиент: false,
    НеСервер: false,
    Запись: false                          // Признак записи (для set)
}
```

#### Структура ТаблицаОбщихТипов:

```
{
    БазовыйТип: "ActiveX",                 // Базовый тип (наследование)
    Слово: "HTTPЗапрос",                   // Имя типа
    НСлово: "httpзапрос",
    ПутьКОписанию: "/objects/catalog234/", // Путь к описанию
    ТипЭлементаКоллекции: "",              // Если это коллекция
    ЯзыкПрограммы: 0,
    Представление: "HTTPЗапрос",
    ЕстьКонструктор: true,                 // Новый HTTPЗапрос()
    ЕстьЧисловойИндекс: false,             // [0], [1]...
    ТипТипа: "Основной",                   // Категория типа
    ИД: "cb184d31-...",                    // UUID
    НомерВерсииПлатформы: 802000,
    НеТолстыйКлиент: false,
    НеТонкийКлиент: false,
    НеСервер: false
}
```

**Ценность данных:**
- ✅ Полное покрытие API платформы 1С (~370K записей методов/свойств)
- ✅ Информация о версиях платформы (с какой версии доступно)
- ✅ Контекстная доступность (клиент/сервер/толстый/тонкий)
- ✅ Типы параметров и возвращаемых значений
- ✅ Пути к документации

### 1.2 Архив синтакс-помощника

**Расположение:** `src/DataProcessors/ирСинтаксПомощник/`

**Источник:** `shcntx_ru.hbk` из каталога установки 1С

**Содержимое архива:**
- **FileStorage.data** - HTML страницы справки для каждого метода
- **PackBlock.data** - структура/содержание
- **IndexPackBlock.data** - индекс для быстрого поиска

**Что делает RDT1C:**
1. Распаковывает архив в кэш
2. Извлекает HTML-описание метода по пути
3. Парсит HTML с помощью регулярных выражений
4. Показывает описание в окне автодополнения

**Можно ли использовать?**
- ✅ Архив доступен в установке 1С
- ❌ Требуется парсер формата .hbk
- ❌ HTML нужно преобразовывать в markdown для LSP

### 1.3 Алгоритмы анализа контекста

**Расположение:** `src/DataProcessors/ирКлсПолеТекстаПрограммы/Ext/ObjectModule.bsl`

**Ключевые алгоритмы:**

#### A. Разбор контекста (`РазобратьТекущийКонтекст`)
```bsl
// Определяет:
// 1. Текущее слово под кареткой
// 2. Родительский контекст (что стоит перед точкой)
// 3. Тип контекста (выражение, вызов метода, литерал)

Запрос = Новый Запрос;
Запрос.|   // <- Контекст: Тип="Запрос", ожидаем методы/свойства типа Запрос

Для Каждого Элемент Из Коллекция Цикл
    Элемент.|  // <- Контекст: Тип из определения Коллекция
```

#### B. Вычисление типа выражения (`ВычислитьТипЗначенияВыражения`)
```bsl
// Анализирует выражение и возвращает его тип:
// 1. Локальные переменные метода
// 2. Параметры метода
// 3. Переменные модуля
// 4. Глобальные функции/типы
// 5. Метаданные конфигурации
// 6. Результаты вызова методов (из ТаблицаМетодовИСвойств)

Результат = HTTPСоединение.Получить(Запрос);
Результат.|  // <- Тип вычисляется: HTTPСоединение.Получить() возвращает HTTPОтвет
```

#### C. Анализ запросов в строках (`РазобратьКонтекстЗапросаВТекстовомЛитерале`)
```bsl
Текст = "
    |ВЫБРАТЬ
    |    Справочник.|"  // <- Особый контекст: язык запросов внутри строки
```

**Применимость к bsl-analyzer:**
- ✅ Концепции универсальны (анализ AST, определение типов)
- ✅ Можно адаптировать логику на Rust
- ❌ Код на BSL не переносим напрямую
- ✅ У вас уже есть HIR и type inference - можно расширить

### 1.4 Статистический рейтинг

**Концепция:**
- Каждое использованное слово получает +1 к рейтингу
- Часто используемые слова показываются выше в списке
- Сохраняется между сеансами

**В bsl-analyzer:**
- Можно реализовать через кэш в `ide-db`
- Salsa поддерживает персистентность

## 2. Что НЕ переиспользовать

### ❌ Код на BSL
- 29,210 строк в `ирКлсПолеТекстаПрограммы`
- Специфичен для платформы 1С
- Много зависимостей от внутренних API инструментов

### ❌ UI формы
- Формы 1С не применимы в LSP
- LSP использует свой протокол для автодополнения

### ❌ Кэширование
- В RDT1C: файловый кэш + переменные модуля
- В bsl-analyzer: Salsa обеспечивает лучшее кэширование

## 3. План портирования

### Фаза 1: Экспорт данных из RDT1C ✅ КРИТИЧНО

**Цель:** Преобразовать данные из формата 1С в JSON/Binary для Rust

**Задачи:**

1. **Создать утилиту экспорта (на BSL в 1С):**
   ```bsl
   // Обработка: ирЭкспортДанныхДляRust.epf
   Процедура ЭкспортироватьВJSON()
       ТаблицаТипов = ирПлатформа.ТаблицаОбщихТипов;
       ТаблицаМетодов = ирПлатформа.ТаблицаМетодовИСвойств;
       ТаблицаПараметров = ирПлатформа.ТаблицаПараметровМетодов;

       // Экспорт в JSON
       ЗаписатьJSON("types.json", ТаблицаТипов);
       ЗаписатьJSON("methods.json", ТаблицаМетодов);
       ЗаписатьJSON("parameters.json", ТаблицаПараметров);
   КонецПроцедуры
   ```

2. **Формат JSON:**
   ```json
   // types.json
   {
     "types": [
       {
         "name": "HTTPЗапрос",
         "name_lower": "httpзапрос",
         "base_type": "ActiveX",
         "has_constructor": true,
         "has_numeric_index": false,
         "min_version": "8.2.0",
         "context_availability": {
           "thick_client": true,
           "thin_client": true,
           "server": true
         }
       }
     ]
   }

   // methods.json
   {
     "methods": [
       {
         "type_name": "HTTPЗапрос",
         "name": "Заголовки",
         "name_lower": "заголовки",
         "kind": "Property",  // Property | Method
         "return_type": "Соответствие",
         "min_version": "8.2.0",
         "context_availability": { /*...*/ }
       }
     ]
   }
   ```

3. **Оптимизация: Binary формат (опционально)**
   - Использовать `bincode` или `postcard` для сериализации
   - Встроить в бинарь bsl-analyzer с помощью `include_bytes!`
   - Размер ~2-5MB, загрузка мгновенная

**Оценка времени:** 1-2 дня (написать экспортер + протестировать)

### Фаза 2: Структуры данных в Rust

**Расположение:** Новый crate `bsl-platform-api`

```rust
// crates/bsl-platform-api/src/lib.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformType {
    pub name: String,
    pub name_lower: String,
    pub base_type: Option<String>,
    pub has_constructor: bool,
    pub has_numeric_index: bool,
    pub min_version: Version,
    pub context: ContextAvailability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformMethod {
    pub type_name: String,
    pub name: String,
    pub name_lower: String,
    pub kind: MemberKind,  // Property | Method
    pub return_type: Option<String>,
    pub min_version: Version,
    pub context: ContextAvailability,
    pub is_writable: bool,  // для свойств
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodParameter {
    pub method_name: String,
    pub type_name: String,
    pub param_index: usize,
    pub param_name: String,
    pub param_type: Option<String>,
    pub is_optional: bool,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct ContextAvailability {
    pub thick_client: bool,
    pub thin_client: bool,
    pub server: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum MemberKind {
    Property,
    Method,
}

// База данных платформы
pub struct PlatformDatabase {
    types: HashMap<String, PlatformType>,
    methods: HashMap<String, Vec<PlatformMethod>>,  // key = type_name
    parameters: HashMap<String, Vec<MethodParameter>>,  // key = type_name::method_name
}

impl PlatformDatabase {
    pub fn load() -> Result<Self> {
        // Загрузка из встроенных данных
        let types: Vec<PlatformType> =
            serde_json::from_slice(include_bytes!("../data/types.json"))?;
        let methods: Vec<PlatformMethod> =
            serde_json::from_slice(include_bytes!("../data/methods.json"))?;
        // ...
    }

    pub fn get_type(&self, name: &str) -> Option<&PlatformType> {
        self.types.get(&name.to_lowercase())
    }

    pub fn get_members(&self, type_name: &str) -> &[PlatformMethod] {
        self.methods.get(&type_name.to_lowercase())
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}
```

**Оценка времени:** 2-3 дня

### Фаза 3: Интеграция с Salsa

**Расположение:** `crates/ide-db/src/platform.rs`

```rust
// Добавить в RootDatabase queries

#[salsa::query_group(PlatformDatabaseStorage)]
pub trait PlatformDatabase {
    /// Загрузить базу данных платформы (вычисляется один раз)
    #[salsa::input]
    fn platform_api_version(&self) -> Version;

    /// Получить информацию о типе платформы
    fn platform_type(&self, name: String) -> Option<Arc<PlatformType>>;

    /// Получить методы/свойства типа
    fn platform_type_members(&self, type_name: String) -> Arc<Vec<PlatformMethod>>;

    /// Получить параметры метода
    fn method_parameters(&self, type_name: String, method_name: String)
        -> Arc<Vec<MethodParameter>>;
}

fn platform_type(db: &dyn PlatformDatabase, name: String) -> Option<Arc<PlatformType>> {
    let platform_db = PLATFORM_DATABASE.get_or_init(|| {
        PlatformDatabase::load().expect("Failed to load platform database")
    });

    platform_db.get_type(&name).map(|t| Arc::new(t.clone()))
}

// Аналогично для остальных queries
```

**Оценка времени:** 1-2 дня

### Фаза 4: Анализ типов выражений

**Расположение:** `crates/hir-ty/src/infer.rs` (расширение существующего)

```rust
impl InferenceContext {
    /// Вычислить тип выражения для автодополнения
    pub fn infer_completion_type(&mut self, expr: ExprId) -> Option<Ty> {
        match &self.body[expr] {
            Expr::Path(path) => {
                // 1. Локальные переменные
                if let Some(ty) = self.resolve_local_var(path) {
                    return Some(ty);
                }

                // 2. Глобальные типы платформы
                if let Some(platform_type) = self.db.platform_type(path.to_string()) {
                    return Some(Ty::Platform(platform_type.name.clone()));
                }

                // 3. Метаданные
                if let Some(mdo_ty) = self.resolve_metadata_ref(path) {
                    return Some(mdo_ty);
                }

                None
            }

            Expr::MethodCall { receiver, method, .. } => {
                // Получить тип receiver
                let receiver_ty = self.infer_completion_type(*receiver)?;

                // Найти метод в базе платформы
                if let Ty::Platform(type_name) = &receiver_ty {
                    let members = self.db.platform_type_members(type_name.clone());
                    let method_info = members.iter()
                        .find(|m| m.name_lower == method.to_lowercase())?;

                    return method_info.return_type.as_ref()
                        .map(|rt| Ty::Platform(rt.clone()));
                }

                None
            }

            Expr::Literal(Literal::String(_)) => {
                // Проверить, это запрос?
                // TODO: анализ SDBL внутри строки
                Some(Ty::Builtin(BuiltinType::String))
            }

            _ => None,
        }
    }
}
```

**Оценка времени:** 3-5 дней

### Фаза 5: Completion Provider

**Расположение:** Новый `crates/ide/src/completion.rs`

```rust
use lsp_types::CompletionItem;

pub struct CompletionContext<'a> {
    pub db: &'a RootDatabase,
    pub position: FilePosition,
    pub trigger_character: Option<char>,
}

pub fn completions(ctx: &CompletionContext) -> Vec<CompletionItem> {
    let parse = ctx.db.parse(ctx.position.file_id);
    let syntax = parse.syntax_node();

    // Найти токен в позиции
    let token = syntax.token_at_offset(ctx.position.offset).left_biased()?;

    // Определить контекст
    let completion_kind = determine_completion_kind(&token);

    match completion_kind {
        CompletionKind::DotAccess => {
            // Авто�дополнение после точки: "Объект.|"
            complete_dot_access(ctx, token)
        }
        CompletionKind::NewExpression => {
            // После "Новый ": все типы с конструкторами
            complete_constructors(ctx)
        }
        CompletionKind::TypeKeyword => {
            // После "Тип(": все типы платформы
            complete_type_names(ctx)
        }
        CompletionKind::FreeIdentifier => {
            // Свободный контекст: локальные переменные + глобальные
            complete_identifiers(ctx)
        }
        CompletionKind::StringLiteral => {
            // Внутри строки (язык запросов?)
            complete_string_literal(ctx, token)
        }
    }
}

fn complete_dot_access(ctx: &CompletionContext, token: SyntaxToken) -> Vec<CompletionItem> {
    // Найти выражение слева от точки
    let expr = find_expression_before_dot(&token)?;

    // Вычислить тип выражения
    let (hir_file, source_map) = ctx.db.body_with_source_map(module_id);
    let expr_id = source_map.node_expr(expr.syntax())?;

    let inference_result = ctx.db.infer(module_id);
    let ty = inference_result[expr_id].clone();

    // Получить members из базы платформы
    if let Ty::Platform(type_name) = ty {
        let members = ctx.db.platform_type_members(type_name);

        return members.iter().map(|m| CompletionItem {
            label: m.name.clone(),
            kind: Some(match m.kind {
                MemberKind::Property => CompletionItemKind::PROPERTY,
                MemberKind::Method => CompletionItemKind::METHOD,
            }),
            detail: m.return_type.clone(),
            documentation: None,  // TODO: Фаза 6
            ..Default::default()
        }).collect();
    }

    vec![]
}

fn complete_constructors(ctx: &CompletionContext) -> Vec<CompletionItem> {
    let platform_db = get_platform_database();

    platform_db.types.values()
        .filter(|t| t.has_constructor)
        .map(|t| CompletionItem {
            label: t.name.clone(),
            kind: Some(CompletionItemKind::CONSTRUCTOR),
            insert_text: Some(format!("{}()", t.name)),
            ..Default::default()
        })
        .collect()
}
```

**Оценка времени:** 5-7 дней

### Фаза 6: Документация (Hover)

**Опция A: Использовать `shcntx_ru.hbk`**

Сложность: Средняя-Высокая
- Нужен парсер формата .hbk (вероятно, proprietary)
- Парсинг HTML → Markdown
- Кэширование распакованных данных

**Опция B: Генерировать из структурированных данных**

Сложность: Низкая
- Использовать только данные из JSON (имя, тип, параметры)
- Простое форматирование в Markdown

```rust
fn generate_method_documentation(method: &PlatformMethod, params: &[MethodParameter]) -> String {
    let mut doc = String::new();

    doc.push_str(&format!("## {}\n\n", method.name));
    doc.push_str(&format!("**Тип:** {}\n\n",
        if method.kind == MemberKind::Method { "Метод" } else { "Свойство" }));

    if let Some(ret_ty) = &method.return_type {
        doc.push_str(&format!("**Возвращает:** `{}`\n\n", ret_ty));
    }

    if !params.is_empty() {
        doc.push_str("**Параметры:**\n\n");
        for p in params {
            doc.push_str(&format!("- `{}`: {}{}\n",
                p.param_name,
                p.param_type.as_deref().unwrap_or("?"),
                if p.is_optional { " (необязательный)" } else { "" }
            ));
        }
    }

    doc.push_str(&format!("\n**Доступно с:** {}\n", method.min_version));
    doc.push_str(&format!("**Контекст:** {}\n", format_context(&method.context)));

    doc
}
```

**Рекомендация:** Начать с Опции B, затем при необходимости добавить Опцию A

**Оценка времени:**
- Опция B: 2-3 дня
- Опция A: 5-7 дней (исследование формата + реализация)

### Фаза 7: Расширенные возможности

**7.1 Автодополнение в строках запросов**

```rust
fn complete_string_literal(ctx: &CompletionContext, token: SyntaxToken) -> Vec<CompletionItem> {
    let string_content = token.text();

    // Проверить, это похоже на запрос?
    if string_content.contains("ВЫБРАТЬ") || string_content.contains("SELECT") {
        // Парсинг SDBL (уже есть в crates/parser/src/sdbl.rs!)
        let sdbl_parse = parse_sdbl_query(string_content);

        // Найти контекст в запросе
        let cursor_offset_in_string = calculate_offset_in_string(ctx.position, &token);
        let sdbl_ctx = determine_sdbl_context(&sdbl_parse, cursor_offset_in_string);

        match sdbl_ctx {
            SdblContext::TableName => {
                // Список таблиц из метаданных
                complete_metadata_tables(ctx)
            }
            SdblContext::FieldName { table } => {
                // Поля указанной таблицы
                complete_table_fields(ctx, table)
            }
            SdblContext::Keyword => {
                // Ключевые слова SDBL
                complete_sdbl_keywords()
            }
        }
    } else {
        vec![]
    }
}
```

**Оценка времени:** 3-5 дней

**7.2 Статистический рейтинг**

```rust
// В crates/ide-db

#[salsa::query_group(CompletionStatsStorage)]
pub trait CompletionStatsDatabase {
    #[salsa::input]
    fn completion_usage_stats(&self) -> Arc<HashMap<String, u32>>;

    fn increment_completion_usage(&mut self, item: String);
}

// Персистентность
fn load_stats_from_cache() -> HashMap<String, u32> {
    let cache_path = get_cache_dir().join("completion_stats.json");
    if cache_path.exists() {
        serde_json::from_reader(File::open(cache_path).unwrap()).unwrap()
    } else {
        HashMap::new()
    }
}

fn save_stats_to_cache(stats: &HashMap<String, u32>) {
    let cache_path = get_cache_dir().join("completion_stats.json");
    serde_json::to_writer(File::create(cache_path).unwrap(), stats).unwrap();
}

// При сортировке completions
fn sort_completions_by_relevance(items: &mut [CompletionItem], stats: &HashMap<String, u32>) {
    items.sort_by_cached_key(|item| {
        let usage_count = stats.get(&item.label).copied().unwrap_or(0);
        std::cmp::Reverse(usage_count)  // Большее значение = выше
    });
}
```

**Оценка времени:** 1-2 дня

## 4. Оценки и риски

### Временные оценки (один разработчик)

| Фаза | Описание | Время |
|------|----------|-------|
| 1 | Экспорт данных из RDT1C | 1-2 дня |
| 2 | Структуры данных в Rust | 2-3 дня |
| 3 | Интеграция с Salsa | 1-2 дня |
| 4 | Анализ типов выражений | 3-5 дней |
| 5 | Completion Provider | 5-7 дней |
| 6 | Документация (Hover) | 2-3 дня (опция B) |
| 7.1 | Автодополнение в запросах | 3-5 дней |
| 7.2 | Статистический рейтинг | 1-2 дня |
| **ИТОГО** | Базовая функциональность (Фазы 1-6) | **~3-4 недели** |
| **ИТОГО** | С расширенными возможностями | **~4-5 недель** |

### Риски и сложности

**🔴 ВЫСОКИЙ РИСК:**
- Формат данных 1С может оказаться сложным для парсинга
  - **Митигация:** Написать простой экспортер в самой 1С (BSL код)

**🟡 СРЕДНИЙ РИСК:**
- Интеграция с существующим type inference
  - **Митигация:** Расширить существующую систему, не переписывать

**🟢 НИЗКИЙ РИСК:**
- LSP протокол для автодополнения стандартный
- Salsa обеспечивает кэширование out-of-the-box
- У вас уже есть парсер BSL и SDBL

## 5. Альтернативные источники данных

Если экспорт из RDT1C не сработает:

### Опция A: bsl-language-server
- Java проект, данные в JSON
- Расположение: `src/main/resources/`
- Неполная информация о типах

### Опция B: 1C Syntax Helper напрямую
- Парсить `shcntx_ru.hbk` без RDT1C
- Более сложно, но возможно
- Reverse engineering формата

### Опция C: Собрать вручную
- Долго и подвержено ошибкам
- **НЕ РЕКОМЕНДУЕТСЯ**

## 6. Рекомендуемый путь

**Минимально жизнеспособный продукт (MVP):**

1. ✅ **Фаза 1** - Экспорт данных (критично)
2. ✅ **Фаза 2** - Структуры данных
3. ✅ **Фаза 3** - Salsa интеграция
4. ✅ **Фаза 5** - Базовое автодополнение (без type inference)
   - Только глобальные функции и типы с конструкторами
5. ✅ **Фаза 6B** - Простая документация

**Время на MVP: ~2 недели**

**Затем итеративно:**
- Фаза 4 - Полноценный type inference
- Фаза 7.1 - Автодополнение в запросах
- Фаза 7.2 - Статистика
- Фаза 6A - Полная документация из shcntx_ru.hbk

## 7. Пример использования

После реализации:

```rust
// В редакторе VSCode с bsl-analyzer

// 1. Автодополнение типов после "Новый"
Запрос = Новый |
// ▼ Показывает: HTTPЗапрос, HTTPСоединение, HTTPОтвет...

// 2. Автодополнение методов/свойств
Запрос = Новый HTTPЗапрос();
Запрос.|
// ▼ Показывает: Заголовки (Свойство), УстановитьИмяФайлаТела (Метод), ...

// 3. Документация при hover
Запрос.Заголовки
// ▼ Показывает Markdown popup:
// ## Заголовки
// **Тип:** Свойство
// **Возвращает:** `Соответствие`
// **Доступно с:** 8.2.0

// 4. Автодополнение в запросах
ТекстЗапроса = "
    |ВЫБРАТЬ
    |    Справочник.|"
// ▼ Показывает: СправочникСсылка.Номенклатура, СправочникСсылка.Контрагенты...
```

## 8. Следующие шаги

1. **Принять решение** - Стоит ли портировать?
   - ✅ ДА: Богатая база данных, улучшит DX
   - ❌ НЕТ: Можно обойтись базовым автодополнением

2. **Если ДА:**
   - Начать с Фазы 1 (экспорт данных)
   - Проверить качество экспортированных данных
   - Продолжить по MVP плану

3. **Создать issue в bsl-analyzer:**
   - Метка: `feature/completion`
   - Прикрепить этот документ
   - Разбить на под-задачи

---

**Автор плана:** Claude Code
**Дата:** 2026-01-08
**Версия:** 1.0
