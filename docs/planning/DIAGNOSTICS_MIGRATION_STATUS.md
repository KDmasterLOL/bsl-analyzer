# Статус миграции диагностик

Этот документ описывает статус миграции всех реализованных диагностик на новую архитектуру.

## Архитектура диагностик

### Три типа диагностик

1. **Text-based (check_node API)** — синтаксические диагностики
   - Один проход по AST для всех диагностик
   - Не требуют семантического анализа
   - Паттерн из rust-analyzer: `crates/ide-diagnostics/src/lib.rs:336-352`

2. **HIR-based** — семантические диагностики
   - Собираются во время HIR lowering
   - Требуют control flow анализ, type inference
   - Кешируются через Salsa

3. **Metadata-based** — диагностики на основе метаданных 1C
   - Требуют загрузки Configuration.xml
   - Используют ModuleMetadata из HIR
   - Кешируются вместе с метаданными

## Сводная статистика

**Всего реализовано:** 118 диагностик (из 181 запланированных)

**Распределение по статусу:**
- **Text-based (collect_text_diagnostics):** 5/~50 (10%)
- **HIR-based (collect_hir_diagnostics):** 9/~20 (45%)
- **Metadata-based (collect_metadata_diagnostics):** 9/~10 (90%)
- **AST-based (старый API, требуют миграции):** ~78 диагностики

## Приоритеты миграции

### Phase 3.1: Text-based миграция (~30-40 диагностик)

**Цель:** Единый проход AST для всех синтаксических проверок

**Кандидаты:**
1. **Форматирование:**
   - LineLength, MissingSpace, IncorrectLineBreak
   - CanonicalSpellingKeywords

2. **Комментарии:**
   - CommentedCode, SpaceAtStartComment

3. **Синтаксис:**
   - EmptyStatement, ExtraCommas, DoubleNegatives
   - NestedTernaryOperator, NestedFunctionInParameters
   - InvalidCharacterInFile, LatinAndCyrillicSymbolInWord

4. **Регионы:**
   - EmptyRegion, DuplicateRegion, NonStandardRegion
   - CodeBlockBeforeSub, CodeOutOfRegion

5. **Литералы:**
   - MagicDate, DuplicateStringLiteral

6. **Security (простые):**
   - ExecuteExternalCode, DisableSafeMode
   - ExternalAppStarting, FileSystemAccess, InternetAccess
   - IsInRoleMethod, FormDataToValue, GetFormMethod

### Phase 3.2: HIR-based миграция (~15-20 диагностик)

**Цель:** Диагностики с control flow анализом

**Кандидаты:**
1. **Control Flow:**
   - BeginTransactionBeforeTryCatch, CommitTransactionOutsideTryCatch
   - CodeAfterAsyncCall, CreateQueryInCycle
   - DeletingCollectionItem, DataExchangeLoading

2. **Complexity:**
   - CognitiveComplexity, CyclomaticComplexity

3. **Try-Catch:**
   - MissingCodeTryCatchEx

4. **Returns:**
   - FunctionReturnsSamePrimitive

5. **Resource tracking:**
   - MissingTempStorageDeletion, MissingTemporaryFileDeletion

6. **Method calls:**
   - MissedRequiredParameter

### Phase 3.3: Metadata-based завершение (~5 диагностик)

**Кандидаты:**
- DenyIncompleteValues
- CommandModuleExportMethods
- MetadataObjectNameLength
- MissingEventSubscriptionHandler

### Phase 3.4: Hybrid диагностики (~5 диагностик)

**Требуют HIR + Metadata:**
- CachedPublic
- CommonModuleMissingAPI
- ExecuteExternalCodeInCommonModule
- GlobalContextMethodCollision8312

### Phase 3.5: SDBL диагностики (уже используют BSL HIR!)

**Статус:** ✅ Уже реализованы через `all_sdbl_in_file()` (Этап 1 из SDBL_HIR_ROADMAP.md)

**Архитектура:**
```rust
// SDBL запросы собираются во время BSL HIR lowering (нет отдельного прохода AST!)
parse() → module_bodies() → Body.sdbl_exprs
                               ↓
                  all_sdbl_in_file() → Vec<(ExprId, SdblQueryInfo)>
                               ↓
                  SDBL диагностики читают SDBL AST
```

**Syntax-only (8 диагностик) - ✅ Готовы:**
- AssignAliasFieldsInQuery
- FullOuterJoinQuery
- LogicalOrInJoinQuerySection
- LogicalOrInTheWhereSectionOfQuery
- MultilineStringInQuery
- JoinWithSubQuery
- FieldsFromJoinsWithoutIsNull
- IncorrectUseLikeInQuery

**Semantic (3-5 диагностик) - ⚠️ Требуют SDBL HIR (Этап 3, будущее):**
- QueryToMissingMetadata (требует metadata)
- JoinWithVirtualTable (требует metadata)
- VirtualTableCallWithoutParameters (требует metadata)

**См.** `docs/planning/SDBL_HIR_ROADMAP.md` для деталей

## Детальная таблица

| # | Диагностика | Текущий API | Рекомендация | Причина |
|---|-------------|-------------|--------------|---------|
| 1 | **BadWords** | Text-based ✅ | Оставить | Regex по тексту |
| 2 | **CanonicalSpellingKeywords** | AST-based | → Text-based | Регистр ключевых слов |
| 3 | **CommentedCode** | AST-based | → Text-based | Анализ комментариев |
| 4 | **DoubleNegatives** | AST-based | → Text-based | Два NOT подряд |
| 5 | **DuplicateStringLiteral** | AST-based | → Text-based | Подсчет литералов |
| 6 | **DuplicateRegion** | AST-based | → Text-based | Дубликаты регионов |
| 7 | **NonStandardRegion** | AST-based | → Text-based | Нестандартные имена |
| 8 | **DuplicatedInsertionIntoCollection** | AST-based | → HIR-based | Data flow |
| 9 | **EmptyCodeBlock** | HIR-based ✅ | Оставить | CFG анализ |
| 10 | **EmptyRegion** | AST-based | → Text-based | Синтаксис |
| 11 | **EmptyStatement** | AST-based | → Text-based | Синтаксис |
| 12 | **ExtraCommas** | AST-based | → Text-based | Синтаксис |
| 13 | **ExcessiveAutoTestCheck** | AST-based | → HIR-based | CFG условий |
| 14 | **IdenticalExpressions** | AST-based | → Text-based | AST сравнение |
| 15 | **IfConditionComplexity** | AST-based | → Text-based | Подсчет сложности |
| 16 | **IfElseDuplicatedCodeBlock** | AST-based | → Text-based | AST сравнение |
| 17 | **IfElseDuplicatedCondition** | AST-based | → Text-based | AST сравнение |
| 18 | **IfElseIfEndsWithElse** | AST-based | → Text-based | Наличие Else |
| 19 | **IncorrectLineBreak** | Text-based ✅ | Оставить | Позиция переносов |
| 20 | **IncorrectUseOfStrTemplate** | AST-based | → Text-based | Использование СтрШаблон |
| 21 | **InvalidCharacterInFile** | AST-based | → Text-based | Недопустимые символы |
| 22 | **LineLength** | Text-based ✅ | Оставить | Длина строк |
| 23 | **MagicDate** | AST-based | → Text-based | Даты-литералы |
| 24 | **MagicNumber** | HIR-based ✅ | Оставить | Числа в выражениях |
| 25 | **MissingSpace** | Text-based ✅ | Оставить | Пробелы |
| 26 | **MultilingualStringHasAllDeclaredLanguages** | AST-based | → Text-based | НСтр() языки |
| 27 | **MultilingualStringUsingWithTemplate** | AST-based | → Text-based | НСтр() с шаблонами |
| 28 | **NestedConstructorsInStructureDeclaration** | AST-based | → Text-based | Вложенные конструкторы |
| 29 | **NestedFunctionInParameters** | AST-based | → Text-based | Вызовы в параметрах |
| 30 | **NestedTernaryOperator** | AST-based | → Text-based | Вложенные тернарные |
| 31 | **NonExportMethodsInApiRegion** | AST-based | → Text-based | Методы в API регионе |
| 32 | **BeginTransactionBeforeTryCatch** | AST-based | → HIR-based | CFG транзакций |
| 33 | **CommitTransactionOutsideTryCatch** | AST-based | → HIR-based | CFG транзакций |
| 34 | **CreateQueryInCycle** | AST-based | → HIR-based | Детекция циклов |
| 35 | **DataExchangeLoading** | AST-based | → HIR-based | CFG условий |
| 36 | **DeletingCollectionItem** | AST-based | → HIR-based | CFG + модификация |
| 37 | **DeprecatedCurrentDate** | AST-based | → Text-based | Вызов ТекущаяДата() |
| 38 | **DeprecatedFind** | AST-based | → Text-based | Вызов Найти() |
| 39 | **DeprecatedMessage** | AST-based | → Text-based | Вызов Сообщить() |
| 40 | **DeprecatedTypeManagedForm** | AST-based | → Text-based | Тип УправляемаяФорма |
| 41 | **DeprecatedMethods8310** | HIR-based ✅ | Оставить | Объединён в DeprecatedMethod |
| 42 | **DeprecatedMethods8317** | HIR-based ✅ | Оставить | Объединён в DeprecatedMethod |
| 43 | **DeprecatedAttributes8312** | AST-based | → Text-based | Атрибуты объектов |
| 44 | **DisableSafeMode** | AST-based | → Text-based | УстановитьБезопасныйРежим(Ложь) |
| 45 | **ExecuteExternalCode** | AST-based | → Text-based | Выполнить(), Eval() |
| 46 | **ExternalAppStarting** | AST-based | → Text-based | ЗапуститьПриложение() |
| 47 | **FileSystemAccess** | AST-based | → Text-based | Файловая система |
| 48 | **InternetAccess** | AST-based | → Text-based | Доступ к интернету |
| 49 | **IsInRoleMethod** | AST-based | → Text-based | РольДоступна() |
| 50 | **FormDataToValue** | AST-based | → Text-based | ДанныеФормыВЗначение() |
| 51 | **GetFormMethod** | AST-based | → Text-based | ПолучитьФорму() |
| 52 | **GlobalContextMethodCollision8312** | AST-based | → Hybrid | Методы + глобальный контекст |
| 53 | **ExportVariables** | AST-based | → Text-based | Экспортные переменные |
| 54 | **CodeAfterAsyncCall** | AST-based | → HIR-based | CFG async-вызовов |
| 55 | **CodeBlockBeforeSub** | AST-based | → Text-based | Код перед процедурами |
| 56 | **CodeOutOfRegion** | AST-based | → Text-based | Код вне регионов |
| 57 | **CognitiveComplexity** | AST-based | → HIR-based | CFG сложности |
| 58 | **CyclomaticComplexity** | AST-based | → HIR-based | CFG сложности |
| 59 | **MethodSize** | AST-based | → Text-based | Подсчет строк |
| 60 | **NestedStatements** | AST-based | → Text-based | Уровень вложенности |
| 61 | **MissedRequiredParameter** | AST-based | → HIR-based | Анализ вызовов |
| 62 | **MissingCodeTryCatchEx** | AST-based | → HIR-based | CFG Try-Catch |
| 63 | **MissingTempStorageDeletion** | AST-based | → HIR-based | Resource tracking |
| 64 | **MissingTemporaryFileDeletion** | AST-based | → HIR-based | Resource tracking |
| 65 | **FunctionNameStartsWithGet** | AST-based | → Text-based | Имя функции |
| 66 | **FunctionReturnsSamePrimitive** | AST-based | → HIR-based | Анализ return |
| 67 | **FunctionShouldHaveReturn** | HIR-based ✅ | Оставить | CFG анализ |
| 68 | **CachedPublic** | AST-based | → Hybrid | Metadata + атрибуты |
| 69 | **CommandModuleExportMethods** | AST-based | → Metadata-based | Тип модуля команды |
| 70 | **CommonModuleAssign** | AST-based | → HIR-based | Анализ присваиваний |
| 71 | **CommonModuleInvalidType** | Metadata-based ✅ | Оставить | Флаг ReturnValuesReuse |
| 72 | **CommonModuleMissingAPI** | AST-based | → Hybrid | Metadata + методы |
| 73 | **CommonModuleNameCached** | Metadata-based ✅ | Оставить | Флаг Cached + имя |
| 74 | **CommonModuleNameClient** | Metadata-based ✅ | Оставить | Флаг ClientManaged + имя |
| 75 | **CommonModuleNameClientServer** | Metadata-based ✅ | Оставить | Флаги Client/Server + имя |
| 76 | **CommonModuleNameFullAccess** | Metadata-based ✅ | Оставить | Флаг FullAccess + имя |
| 77 | **CommonModuleNameGlobal** | Metadata-based ✅ | Оставить | Флаг Global + имя |
| 78 | **CommonModuleNameGlobalClient** | Metadata-based ✅ | Оставить | Флаги Global/Client + имя |
| 79 | **CommonModuleNameServerCall** | Metadata-based ✅ | Оставить | Флаг ServerCall + имя |
| 80 | **CommonModuleNameWords** | Metadata-based ✅ | Оставить | Слова в имени |
| 81 | **DenyIncompleteValues** | AST-based | → Metadata-based | Свойство DenyIncompleteValues |
| 82 | **ExecuteExternalCodeInCommonModule** | AST-based | → Hybrid | Metadata Server + Выполнить() |
| 83 | **MetadataObjectNameLength** | AST-based | → Metadata-based | Длина имен объектов |
| 84 | **MissingCommonModuleMethod** | HIR-based ✅ | Оставить | Вызовы методов ОМ |
| 85 | **MissingReturnedValueDescription** | AST-based | → Text-based | Описание в комментарии |
| 86 | **SelfAssign** | HIR-based ✅ | Оставить | Анализ присваиваний |
| 87 | **UnreachableCode** | HIR-based ✅ | Оставить | CFG достижимость |
| 88 | **UnusedLocalVariable** | HIR-based ✅ | Оставить | Использование переменных |
| 89 | **MissingReturn** | HIR-based ✅ | Оставить | CFG все пути |
| 90 | **DeprecatedMethod** | HIR-based ✅ | Оставить | Устаревшие методы |
| 91 | **AssignAliasFieldsInQuery** | AST-based | → Text-based (SDBL) | SDBL алиасы |
| 92 | **FieldsFromJoinsWithoutIsNull** | AST-based | → HIR-based (SDBL) | SDBL анализ JOIN |
| 93 | **FullOuterJoinQuery** | AST-based | → Text-based (SDBL) | SDBL FULL OUTER JOIN |
| 94 | **JoinWithSubQuery** | AST-based | → Text-based (SDBL) | SDBL вложенные запросы |
| 95 | **LogicalOrInJoinQuerySection** | AST-based | → Text-based (SDBL) | SDBL OR в JOIN |
| 96 | **LogicalOrInTheWhereSectionOfQuery** | AST-based | → Text-based (SDBL) | SDBL OR в WHERE |
| 97 | **MultilineStringInQuery** | AST-based | → Text-based (SDBL) | SDBL многострочные литералы |
| 98 | **LatinAndCyrillicSymbolInWord** | AST-based | → Text-based | Смешение латиницы/кириллицы |

## Следующие шаги

1. ✅ **Phase 1:** Откат HIR инфраструктуры для BadWords
2. ✅ **Phase 2:** Создание единого text-based прохода
3. 🚧 **Phase 3.1:** Миграция 30-40 text-based диагностик (5/40 готово: BadWords, LineLength, MissingSpace, IncorrectLineBreak, SpaceAtStartComment)
4. ⏳ **Phase 3.2:** Миграция 15-20 HIR-based диагностик
5. ⏳ **Phase 3.3:** Завершение metadata-based диагностик
6. ⏳ **Phase 3.4:** Реализация hybrid диагностик

## Производительность

**Текущее состояние:**
- 78 отдельных прохода по AST (каждая AST-based диагностика)
- 1 единый проход для текстовых диагностик (5 диагностики: BadWords, LineLength, MissingSpace, IncorrectLineBreak, SpaceAtStartComment)

**После Phase 3.1:**
- 1 единый проход для ~30-40 текстовых диагностик
- ~40-50 отдельных проходов (только для сложных диагностик)
- **Ожидаемое ускорение:** 30-40x для text-based диагностик
