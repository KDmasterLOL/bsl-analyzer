# Diagnostics Migration Plan

## Обзор

Миграция **180 диагностик** из bsl-language-server в bsl-analyzer.

**Стратегия:**
- ✅ Переносить **строго по алфавиту** (A→Z), по одной диагностике
- ✅ При обнаружении необходимости доработки инфраструктуры - делать **немедленно**
- ✅ Обеспечить **100% совместимость** с Java (номера строк, колонок, диапазоны)
- ✅ Копировать и портировать все тесты из Java проекта

## Детальные планы

📋 **Полные детальные планы по каждой диагностике:**

- **[diagnostics/README.md](./diagnostics/README.md)** - Индекс и обзор
- **[diagnostics/CONFIGURATION.md](./diagnostics/CONFIGURATION.md)** - Схема конфигурации
- **[diagnostics/MIGRATION_PLAN_A_C.md](./diagnostics/MIGRATION_PLAN_A_C.md)** - Диагностики A-C (30 штук)
- **[diagnostics/MIGRATION_PLAN_D_M.md](./diagnostics/MIGRATION_PLAN_D_M.md)** - Диагностики D-M (48 штук)
- **[diagnostics/MIGRATION_PLAN_N_S.md](./diagnostics/MIGRATION_PLAN_N_S.md)** - Диагностики N-S (45 штук)
- **[diagnostics/MIGRATION_PLAN_T_Z.md](./diagnostics/MIGRATION_PLAN_T_Z.md)** - Диагностики T-Z (56 штук)

## Категоризация по сложности

### Tier 1: Простые (Syntax-only) - ~60 диагностик
Работают только с синтаксисом, не требуют семантического анализа.
**Примеры:** CanonicalSpellingKeywords, LineLength, EmptyCodeBlock, SelfAssign, UsingGoto

### Tier 2: Средние (Symbol table) - ~60 диагностик
Требуют таблицу символов, разрешение имён.
**Примеры:** UnusedLocalVariable, FunctionShouldHaveReturn, DeprecatedCurrentDate

### Tier 3: Сложные (Full semantic) - ~40 диагностик
Требуют полный семантический анализ, метаданные.
**Примеры:** CommonModule* (11), ForbiddenMetadataName, ScheduledJobHandler

### Tier 4: SDBL (Query language) - ~16 диагностик
Требуют парсинг и анализ SDBL запросов.
**Примеры:** AssignAliasFieldsInQuery, JoinWithSubQuery, SelectTopWithoutOrderBy

## Быстрая статистика

| Категория | Количество | % |
|-----------|------------|---|
| **Tier 1: Syntax-only** | ~60 | 33% |
| **Tier 2: Symbol Table** | ~60 | 33% |
| **Tier 3: Metadata** | ~40 | 22% |
| **Tier 4: SDBL** | ~16 | 9% |
| **Special** | ~4 | 2% |

| По серьезности | Количество | % |
|----------------|------------|---|
| **BLOCKER** | 12 | 7% |
| **CRITICAL** | 30 | 17% |
| **MAJOR** | 90 | 50% |
| **MINOR** | 30 | 17% |
| **INFO** | 18 | 10% |

| По типу | Количество | % |
|---------|------------|---|
| **CODE_SMELL** | 109 | 61% |
| **ERROR** | 56 | 31% |
| **VULNERABILITY** | 7 | 4% |
| **SECURITY_HOTSPOT** | 8 | 4% |

**Отключены по умолчанию (~8 диагностик):**
BadWords, CodeAfterAsyncCall, DenyIncompleteValues, FieldsFromJoinsWithoutIsNull, FileSystemAccess, FunctionNameStartsWithGet, FunctionOutParameter, InternetAccess, MissingTempStorageDeletion, TernaryOperatorUsage, TooManyReturns, UseSystemInformation, UsingLikeInQuery

---

## Фазы реализации

### Фаза 1: Быстрые победы (Iterations 12-13) - ~40 диагностик

**Простые синтаксические проверки для валидации инфраструктуры:**

1. CanonicalSpellingKeywords - проверка написания ключевых слов
2. LineLength - длина строки
3. EmptyCodeBlock - пустые блоки кода
4. CodeBlockBeforeSub - код перед методами
5. SelfAssign - самоприсваивание
6. UsingGoto - использование Goto
7. YoLetterUsage - буква "ё"
8. И ещё ~33 простых диагностики

**Зависимости:** Parser, AST, Token access

---

### Фаза 2: Таблица символов (Iterations 14-18) - ~50 диагностик

**Семантический анализ:**

1. UnusedLocalVariable - неиспользуемые переменные
2. FunctionShouldHaveReturn - функция должна иметь return
3. DeprecatedCurrentDate - устаревший ТекущаяДата()
4. AllFunctionPathMustHaveReturn (требует CFG)
5. CognitiveComplexity (требует CFG)
6. И ещё ~45 диагностик

**Зависимости:** Symbol table, type inference, CFG

---

### Фаза 3: Метаданные (Iterations 19-23) - ~40 диагностик

**Интеграция с 1С метаданными:**

1. Все CommonModule* диагностики (11 штук)
2. ForbiddenMetadataName
3. ScheduledJobHandler
4. MissingCommonModuleMethod
5. И ещё ~26 диагностик

**Зависимости:** Metadata loader (Iteration 11), Configuration.xml parsing

---

### Фаза 4: SDBL (Iterations 24-25) - ~16 диагностик

**Язык запросов:**

1. AssignAliasFieldsInQuery
2. JoinWithSubQuery
3. SelectTopWithoutOrderBy
4. LogicalOrInJoinQuerySection
5. И ещё ~12 диагностик

**Зависимости:** SDBL AST completion, semantic analysis

---

### Фаза 5: Продвинутые (позже) - ~10 диагностик

**Специальная инфраструктура:**

1. CommentedCode (эвристический анализатор кода)
2. Typo (интеграция spell checker)
3. DuplicateStringLiteral (кросс-файловый анализ)
4. MultilingualString* (2 диагностики)
5. Data flow диагностики (DuplicatedInsertionIntoCollection, FunctionOutParameter, MissingTemp*, RewriteMethodParameter)

**Зависимости:** Кастомные алгоритмы, внешние инструменты

---

## Рекомендуемые "первые диагностики"

Начните с этих простых, хорошо определённых диагностик для валидации инфраструктуры:

1. **CanonicalSpellingKeywords** - простая проверка токенов
2. **LineLength** - токен + отслеживание строк
3. **EmptyCodeBlock** - простая проверка AST
4. **CodeBlockBeforeSub** - простая проверка AST
5. **SemicolonPresence** - проверка токенов
6. **MagicNumber** - проверка литералов AST
7. **SelfAssign** - проверка присваиваний AST
8. **UsingGoto** - проверка операторов AST
9. **YoLetterUsage** - сканирование текста идентификаторов
10. **UnaryPlusInConcatenation** - проверка выражений AST

---

## Tier 1: Простые диагностики (Iteration 12-13)

### Iteration 11: Code Style
| Код | Название | Сложность |
|-----|----------|-----------|
| CanonicalSpellingKeywords | Каноническое написание ключевых слов | Low |
| ConsecutiveEmptyLines | Последовательные пустые строки | Low |
| LineLength | Длина строки | Low |
| MissingSpace | Отсутствует пробел | Low |
| OneStatementPerLine | Один оператор на строку | Low |
| SemicolonPresence | Наличие точки с запятой | Low |
| SpaceAtStartComment | Пробел в начале комментария | Low |
| IncorrectLineBreak | Неправильный разрыв строки | Low |
| ExtraCommas | Лишние запятые | Low |

### Iteration 12: Empty/Commented Code
| Код | Название | Сложность |
|-----|----------|-----------|
| CommentedCode | Закомментированный код | Medium |
| EmptyCodeBlock | Пустой блок кода | Low |
| EmptyRegion | Пустая область | Low |
| EmptyStatement | Пустой оператор | Low |
| UnreachableCode | Недостижимый код | Medium |
| CodeBlockBeforeSub | Блок кода перед подпрограммой | Low |
| CodeOutOfRegion | Код вне областей | Low |

### Iteration 13: Operators & Literals
| Код | Название | Сложность |
|-----|----------|-----------|
| MagicNumber | Магическое число | Low |
| MagicDate | Магическая дата | Low |
| YoLetterUsage | Использование буквы Ё | Low |
| LatinAndCyrillicSymbolInWord | Смешение алфавитов | Low |
| InvalidCharacterInFile | Недопустимый символ | Low |
| DoubleNegatives | Двойные отрицания | Low |
| NestedTernaryOperator | Вложенный тернарный оператор | Low |
| TernaryOperatorUsage | Использование тернарного оператора | Low |
| UnaryPlusInConcatenation | Унарный плюс в конкатенации | Low |
| UselessTernaryOperator | Бесполезный тернарный оператор | Low |

---

## Tier 2: Средние диагностики (Iteration 14-18)

### Iteration 14: Functions & Procedures
| Код | Название | Зависимости |
|-----|----------|-------------|
| AllFunctionPathMustHaveReturn | Все пути функции должны возвращать | CFG |
| FunctionShouldHaveReturn | Функция должна иметь return | AST |
| ProcedureReturnsValue | Процедура возвращает значение | AST |
| FunctionReturnsSamePrimitive | Функция возвращает тот же примитив | AST |
| FunctionNameStartsWithGet | Имя функции начинается с Get | AST |
| TooManyReturns | Слишком много return | AST |

### Iteration 15: Parameters
| Код | Название | Зависимости |
|-----|----------|-------------|
| NumberOfParams | Количество параметров | Symbol Table |
| NumberOfOptionalParams | Количество опциональных параметров | Symbol Table |
| OrderOfParams | Порядок параметров | Symbol Table |
| MissedRequiredParameter | Пропущен обязательный параметр | Symbol Table |
| FunctionOutParameter | Выходной параметр функции | Symbol Table |
| UnusedParameters | Неиспользуемые параметры | Symbol Table |
| MissingParameterDescription | Отсутствует описание параметра | Symbol Table |
| MissingReturnedValueDescription | Отсутствует описание возвращаемого значения | Symbol Table |
| RewriteMethodParameter | Перезапись параметра метода | Data Flow |

### Iteration 16: Variables
| Код | Название | Зависимости |
|-----|----------|-------------|
| UnusedLocalVariable | Неиспользуемая локальная переменная | Symbol Table |
| UnusedLocalMethod | Неиспользуемый локальный метод | Symbol Table |
| ExportVariables | Экспортируемые переменные | Symbol Table |
| MissingVariablesDescription | Отсутствует описание переменных | Symbol Table |
| SelfAssign | Самоприсваивание | AST |
| ThisObjectAssign | Присваивание ThisObject | AST |

### Iteration 17: Complexity
| Код | Название | Зависимости |
|-----|----------|-------------|
| CyclomaticComplexity | Цикломатическая сложность | CFG |
| CognitiveComplexity | Когнитивная сложность | CFG |
| NestedStatements | Вложенные операторы | AST |
| MethodSize | Размер метода | AST |
| IfConditionComplexity | Сложность условия if | AST |

### Iteration 18: Control Flow
| Код | Название | Зависимости |
|-----|----------|-------------|
| MissingCodeTryCatchEx | Отсутствует код в try/catch | AST |
| UsingGoto | Использование Goto | AST |
| BeginTransactionBeforeTryCatch | BeginTransaction перед try/catch | AST |
| CommitTransactionOutsideTryCatch | CommitTransaction вне try/catch | AST |
| PairingBrokenTransaction | Нарушенное парирование транзакции | CFG |
| WrongUseOfRollbackTransactionMethod | Неправильное использование Rollback | CFG |

---

## Tier 3: Сложные диагностики (Iteration 19-23)

### Iteration 19: Common Module Diagnostics
| Код | Название | Зависимости |
|-----|----------|-------------|
| CommonModuleAssign | Присваивание общему модулю | Metadata |
| CommonModuleInvalidType | Неверный тип общего модуля | Metadata |
| CommonModuleMissingAPI | Отсутствует API в общем модуле | Metadata |
| CommonModuleNameCached | Название общего модуля (Cached) | Metadata |
| CommonModuleNameClient | Название общего модуля (Client) | Metadata |
| CommonModuleNameClientServer | Название общего модуля (ClientServer) | Metadata |
| CommonModuleNameFullAccess | Название общего модуля (FullAccess) | Metadata |
| CommonModuleNameGlobal | Название общего модуля (Global) | Metadata |
| CommonModuleNameGlobalClient | Название общего модуля (GlobalClient) | Metadata |
| CommonModuleNameServerCall | Название общего модуля (ServerCall) | Metadata |
| CommonModuleNameWords | Слова в названии общего модуля | Metadata |

### Iteration 20: Deprecated & Platform
| Код | Название | Зависимости |
|-----|----------|-------------|
| DeprecatedCurrentDate | Устаревший CurrentDate | AST |
| DeprecatedFind | Устаревший Find | AST |
| DeprecatedMessage | Устаревший Message | AST |
| DeprecatedMethodCall | Устаревший вызов метода | Type Info |
| DeprecatedMethods8310 | Устаревшие методы 8.3.10 | Platform Version |
| DeprecatedMethods8317 | Устаревшие методы 8.3.17 | Platform Version |
| DeprecatedAttributes8312 | Устаревшие атрибуты 8.3.12 | Platform Version |
| DeprecatedTypeManagedForm | Устаревший тип ManagedForm | Type Info |
| GlobalContextMethodCollision8312 | Конфликт методов 8.3.12 | Platform Version |

### Iteration 21: Security
| Код | Название | Зависимости |
|-----|----------|-------------|
| ExecuteExternalCode | Выполнение внешнего кода | Data Flow |
| ExecuteExternalCodeInCommonModule | Выполнение внешнего кода в общем модуле | Metadata |
| DisableSafeMode | Отключение безопасного режима | AST |
| SetPrivilegedMode | Установка привилегированного режима | AST |
| UsingHardcodeNetworkAddress | Жестко закодированный сетевой адрес | AST |
| UsingHardcodePathDiagnostic | Жестко закодированный путь | AST |
| UsingHardcodeSecretInformation | Жестко закодированная секретная информация | AST |
| ExternalAppStarting | Запуск внешнего приложения | AST |
| FileSystemAccess | Доступ к файловой системе | AST |
| InternetAccess | Доступ в интернет | AST |
| TimeoutsInExternalResources | Таймауты во внешних ресурсах | AST |
| UnsafeSafeModeMethodCall | Небезопасный вызов в безопасном режиме | Type Info |

### Iteration 22: Code Quality
| Код | Название | Зависимости |
|-----|----------|-------------|
| CreateQueryInCycle | Создание запроса в цикле | CFG |
| DeletingCollectionItem | Удаление элемента коллекции | CFG |
| DuplicatedInsertionIntoCollection | Дублированное добавление в коллекцию | CFG |
| DuplicateStringLiteral | Дублированный строковый литерал | Cross-file |
| IdenticalExpressions | Идентичные выражения | AST |
| IfElseDuplicatedCodeBlock | Дублированный блок в if/else | AST |
| IfElseDuplicatedCondition | Дублированное условие в if/else | AST |
| IfElseIfEndsWithElse | if/elseif должны заканчиваться else | AST |
| RefOveruse | Чрезмерное использование ссылок | Data Flow |

### Iteration 23: Metadata Dependent
| Код | Название | Зависимости |
|-----|----------|-------------|
| MissingEventSubscriptionHandler | Отсутствует обработчик подписки | Metadata |
| MissingCommonModuleMethod | Отсутствует метод общего модуля | Metadata |
| ScheduledJobHandler | Обработчик запланированного задания | Metadata |
| QueryToMissingMetadata | Запрос к отсутствующей метаданной | Metadata |
| WrongHttpServiceHandler | Неверный обработчик HTTP | Metadata |
| WrongWebServiceHandler | Неверный обработчик веб-сервиса | Metadata |
| ForbiddenMetadataName | Запрещенное имя метаданных | Metadata |
| MetadataObjectNameLength | Длина имени объекта метаданных | Metadata |
| SameMetadataObjectAndChildNames | Совпадающие имена метаданных | Metadata |

---

## Tier 4: SDBL диагностики (Iteration 24-25)

### Iteration 24: Query Syntax
| Код | Название | Зависимости |
|-----|----------|-------------|
| QueryParseError | Ошибка парсинга запроса | SDBL Parser |
| MultilineStringInQuery | Многострочная строка в запросе | SDBL AST |
| SelectTopWithoutOrderBy | SELECT TOP без ORDER BY | SDBL AST |
| UnionAllDiagnostic | UNION ALL | SDBL AST |
| UsingLikeInQuery | Использование LIKE в запросе | SDBL AST |
| IncorrectUseLikeInQuery | Неправильное использование LIKE | SDBL AST |

### Iteration 25: Query Performance
| Код | Название | Зависимости |
|-----|----------|-------------|
| FullOuterJoinQuery | Полное внешнее объединение | SDBL AST |
| JoinWithSubQuery | Объединение с подзапросом | SDBL AST |
| JoinWithVirtualTable | Объединение с виртуальной таблицей | SDBL AST |
| LogicalOrInJoinQuerySection | ИЛИ в секции JOIN | SDBL AST |
| LogicalOrInTheWhereSectionOfQuery | ИЛИ в секции WHERE | SDBL AST |
| FieldsFromJoinsWithoutIsNull | Поля из объединений без IsNull | SDBL AST |
| AssignAliasFieldsInQuery | Присвоение полей с alias | SDBL AST |
| QueryNestedFieldsByDot | Вложенные поля через точку | SDBL AST |
| VirtualTableCallWithoutParameters | Вызов виртуальной таблицы без параметров | SDBL AST |

---

## Тестирование диагностик

### Структура тестов

```
crates/ide-diagnostics/
├── src/
│   ├── lib.rs
│   ├── handlers/
│   │   ├── canonical_spelling_keywords.rs
│   │   ├── consecutive_empty_lines.rs
│   │   └── ...
│   └── tests/
│       ├── canonical_spelling_keywords.rs
│       ├── consecutive_empty_lines.rs
│       └── ...
└── test_data/
    ├── canonical_spelling_keywords/
    │   ├── simple.bsl
    │   └── complex.bsl
    └── ...
```

### Формат тестов

```rust
#[test]
fn test_canonical_spelling_keywords() {
    check_diagnostics(r#"
Процедура Тест()
    если Истина тогда  // error: CanonicalSpellingKeywords
    //   ^^^^^ expected "Если"
    КонецЕсли;
КонецПроцедуры
"#);
}
```

### Миграция тестов из Java

Для каждой диагностики:
1. Найти тесты в `bsl-language-server/src/test/java/diagnostics/`
2. Найти тестовые данные в `bsl-language-server/src/test/resources/diagnostics/`
3. Адаптировать под формат Rust
4. Обеспечить 100% покрытие

---

## Конфигурация

Все диагностики настраиваются через `.bsl-language-server.json`.

**Пример:**
```json
{
  "diagnostics": {
    "parameters": {
      "LineLength": {
        "maxLineLength": 140
      },
      "MethodSize": false,
      "CyclomaticComplexity": {
        "complexityThreshold": 15
      }
    }
  }
}
```

**Режимы:**
- `"on"` (по умолчанию): все включённые диагностики работают
- `"off"`: все диагностики отключены
- `"only"`: только указанные диагностики
- `"except"`: все кроме указанных

См. [diagnostics/CONFIGURATION.md](./diagnostics/CONFIGURATION.md) для полной документации.

---

## Источники

**Java проект:** `~/src/lsp/bsl-language-server/`

- **Код диагностик:** `src/main/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/`
- **Тесты:** `src/test/java/com/github/_1c_syntax/bsl/languageserver/diagnostics/`
- **Тестовые данные:** `src/test/resources/diagnostics/`
- **Документация:** https://1c-syntax.github.io/bsl-language-server/diagnostics/

---

## Трекинг прогресса

| Фаза | Всего | Готово | Прогресс |
|------|-------|--------|----------|
| Фаза 1 (Syntax) | 40 | 0 | 0% |
| Фаза 2 (Semantic) | 50 | 0 | 0% |
| Фаза 3 (Metadata) | 40 | 0 | 0% |
| Фаза 4 (SDBL) | 16 | 0 | 0% |
| Фаза 5 (Advanced) | 10 | 0 | 0% |
| **Итого** | **156** | **0** | **0%** |

*Примечание: 24 диагностики отложены на более поздние итерации (metadata, data flow)*

---

## Связанная документация

- [diagnostics/README.md](./diagnostics/README.md) - Индекс детальных планов
- [diagnostics/CONFIGURATION.md](./diagnostics/CONFIGURATION.md) - Схема конфигурации
- [ARCHITECTURE.md](../architecture/ARCHITECTURE.md) - Архитектура системы
- [ROADMAP.md](./ROADMAP.md) - План на 30 итераций
- [METADATA_PLAN.md](./METADATA_PLAN.md) - Инфраструктура метаданных (Iteration 11)
- [SALSA_TODO.md](./SALSA_TODO.md) - Интеграция Salsa (Iteration 10)
