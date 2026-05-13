# Type inference roadmap

Что уже работает и где остаются дыры. Текущий снимок состояния по
крейтам см. в `crates/hir-ty/src/`.

## Покрытие сейчас

### Базовое инференция

- Литералы, бинарные/унарные операторы, `Новый`, `Тип("…")`, `ТипЗнч`.
- Каскадная резолюция Builtins → Local → Module → Workspace.
- `is_assignable_to` (`subtype.rs`): gradual top/bottom через `Unknown`,
  `Null ≤ *Ref`, union distribute, ThisObject one-way coerce, function
  variance (контравариантные параметры, ковариантный return).
- `Ty::Union` smart constructor (флэт + сортировка + дедуп).
- Двуязычность RU/EN, case-insensitive везде.

### Workspace-методы

- `CommonModule.Method()` с проверкой export.
- 3-сегментные пути через manager: `Документы.ПКО.НайтиПоНомеру()`,
  promotion `ManagerCollection → ObjectManager → MetadataRef`.
- `proc_signature_query` (`proc_signature.rs`): сигнатура из docstring
  + Body-walk fallback для return-типа.
- `proc_signature_lookup` adapter (`proc_signature_lookup.rs`) —
  готовая обвязка под `MethodInfo`, ждёт consumer-wiring через
  `infer` (см. Tier 1 ниже).

### Метаданные (MDO)

- Field lookup для атрибутов справочников/документов/планов
  обмена/планов счетов, табличных секций → `TabularSection`/`Row`,
  размерностей/ресурсов/реквизитов регистров, predefined items / enum
  values.
- Method lookup для платформенных value-типов через `bsl-platform`,
  fluent chains (`Запрос.Выполнить().Выбрать()`).
- DefinedType resolution (`<v8:TypeSet>cfg:DefinedType.X</…>`) c
  двумя слоями cycle protection.
- Register record-set / record platform kinds
  (`InformationRegisterRecordSet`, `AccumulationRegisterRecordSet`,
  `AccountingRegisterRecordSet`, `CalculationRegisterRecordSet` + Record).
- Extension wins main через `configs.iter().rev()` везде.

### `ЭтотОбъект` self-resolution

- **ObjectModule** (`Ty::ThisObject`) — Catalog/Document/ExchangePlan/
  ChartOfAccounts через `resolve_this_object` + `MetadataKind::object_kind_for`.
- **ManagerModule** (`Ty::ThisManager`) — отдельный вариант + sibling
  `resolve_this_manager` (resolver.rs:257-302), coerce_to_metadata_ref
  обрабатывает оба.
- **Managed-form** (`form_self.rs`) — платформенные свойства/методы
  формы (`Элементы`, `Команды`, `Параметры`, `Активизировать()`, …)
  через `platform_property_lookup`.

### Managed-форма: реквизиты и элементы

- **Реквизиты формы** (`form_attr.rs`) — `<Attributes>` из `Form.xml`
  резолвятся в типы:
  - **MainAttribute** (`cfg:CatalogObject.X` / `DocumentObject.Y` / …)
    → `Ty::FormData { kind: Structure, underlying: Some((mdo, name)) }`
    с peel-семантикой для дальнейшего field/method lookup,
  - **ValueTable** (`<Columns>`) → `Ty::FormData { kind: Collection }`,
  - **остальное** (примитивы, рефы, DefinedType, composite) через
    общий `attribute_type_to_ty`.
- **Элементы формы** (`form_items.rs`) — `Элементы.<имя>` →
  `Ty::FormControl { kind, binding }` с разбором `<ChildItems>` из
  `Form.xml` и таксономии тегов.

### Narrowing

ADR-01 MUST-grammar (`narrow.rs`):

- `ТипЗнч(X) = Тип("…")` — сужение по платформенному типу.
- `X = Неопределено` / `X <> Неопределено` — между `Ty::Undefined` и
  `Union \ Undefined`.
- `ЗначениеЗаполнено(X)` — снятие `Undefined`/`Null` на true-ветке.
- Симметричные ориентации (`Тип("…") = ТипЗнч(X)` и т.п.).

Branch-aware `Transfer::transfer_edge` — есть.

### Salsa-кэширование

`parse` (LRU 512) → `item_tree` → `symbol_tree` →
`module_bodies` (128) → `infer` (256) → `module_metadata` (128).

## Tier 1 — закрывает самые частые «почему hover молчит»

### Wiring `proc_signature_lookup` через `infer`

**Текущее ограничение.** Adapter (`proc_signature_lookup.rs`)
готов: умеет лоуэрить workspace-метод в `MethodInfo`-шейп, который
ожидает arg-checking. Но **ни один call-site `lookup_method` не
ходит через него во время инференции**. Это значит: для workspace-
методов (включая `CommonModule.Method()` после type-driven dispatch)
проверка количества и типов аргументов **молча пропускается** —
вызовы выглядят валидными даже при несовпадении.

**Подход.** Подключить `resolve_workspace_method` к
соответствующим точкам в `infer`/`method_resolution`. Одновременно
добавить `salsa::cycle_fn` к `proc_signature_query`,
возвращающий `ProcSignature { params: <doc-derived>, return_ty: Unknown }`
на случай цикла `infer → lookup_method → proc_signature_query`
(см. cycle status note в обоих файлах). До этого цикл структурно
unreachable, после подключения — обязателен.

Самый высокий приоритет: разблокирует `MismatchedArgCount` /
`TypeMismatch` по аргументам **для всего workspace-кода**, а не
только для платформенных вызовов.

### Параметры workspace-методов из call-sites

**Текущее ограничение.** `proc_signature_query` берёт типы
параметров **только из docstring `// Параметры:`**. Слоты без
documentation остаются `Ty::Unknown` — gradual top/bottom гасит
любую диагностику аргументов через `is_assignable`. Большая часть
реального BSL-кода идёт без полной документации.

**Подход.** Второй проход после docstring: собрать call-sites метода
через `module_index`/cross-module refs, вывести union типов
фактически передаваемых аргументов как fallback для пустых слотов.
Структурно `proc_signature_query` уже Salsa-tracked. Cycle_fn,
введённый предыдущим пунктом, обеспечит безопасность.

Имеет смысл закрывать после wiring предыдущего пункта — без него
улучшение параметров не даёт пользовательски заметного эффекта.

### Phase 5 row refinement для form items

**Текущее ограничение.** `form_items.rs` лоуэрит элемент в
`Ty::FormControl { kind, binding }` с провенансом `<DataPath>`, но
**row-aware refinement не подключён**:

- `.ВыделенныеСтроки` / `.ТекущаяСтрока` на табличной форме не
  возвращают `Ty::TypedArray(row)` / row-тип,
- итерация / индексирование `Для каждого Строка Из …Строки Цикл`
  даёт `Ty::Unknown`,
- `Строка.Колонка` внутри тела цикла — без типа.

Phase 5 ограничена MDO tabular sections (см. комментарий в
`form_items.rs:175` / Phase 4); non-MDO в первой итерации
out-of-scope.

**Подход.** Прокинуть `binding` через resolver на row-aware
properties, увязать с уже работающим `TabularSection`/`Row` lookup.

## Tier 2 — расширение существующих систем

### ADR-01 Q1 narrowing

Deferred в `narrow.rs` (документировано в комментариях):

- `Не`-инверсия (`Если Не ТипЗнч(X) = Тип("…")` /
  `Не ЗначениеЗаполнено(X)`),
- `ИЛИ`-композиция (`ТипЗнч(X) = Тип("Строка") ИЛИ ТипЗнч(X) = …`),
- `И`-композиция / nested guards (`Если A И B Тогда`),
- `X Есть Справочник`,
- narrowing на non-`Path(Name)` receivers (поля, индексы,
  qualified-пути).

MUST-grammar покрывает большинство кейсов. Расширение —
средняя ценность при низкой стоимости.

### Методы экземпляров `MetadataRef` / `ObjectManager`

**Текущее ограничение.** `lookup_method` для `Ty::MetadataRef` и
`Ty::ObjectManager` возвращает Unknown — методы экземпляров
записаны в HBK `documentation.syntax` с mangled-name полями и не
доступны через текущий `PlatformData`.

**Подход.** Либо отдельный индекс по `documentation.syntax`, либо
парсер mangled-name в `bsl-platform/tools/html-parser`. Самое
дорогое из Tier 2, но открывает «методы у Справочника/Документа»
напрямую в hover/completion.

### RecordSetModule self-resolver

**Текущее ограничение.** Платформенные kinds для record-set'ов
известны (`InformationRegisterRecordSet`, … — `platform_manager_lookup.rs`),
но **workspace `RecordSetModule.bsl`** не имеет своего
`resolve_this_*` аналога. `ЭтотОбъект` внутри модуля набора записей
остаётся Unknown.

**Подход.** По образу `resolve_this_manager`: гейт по
`ModuleType::RecordSetModule` + mapping в соответствующий
`*RecordSet` MetadataKind. Минимальный риск регрессии.

## Tier 3 — долгосрочное

- **Ordinary forms self-context.** Старые формы (`FormType::Ordinary`)
  не покрыты — managed-form gate в `form_self.rs` намеренно строгий.
  Нужен отдельный платформенный type-key + параллельный resolver-путь.
- **Движения / запись наборов записей** (`.Движения.ДобавитьРасход(…)`)
  — отдельная подсистема со своим lifecycle.
- **Generic типы, refinements.** Без явного use case пока ждёт.

## Принципы для всей type-инфраструктуры

- **`Ty::Unknown` — gradual top/bottom.** Никогда не эмитим
  диагностику при `Unknown` на любой стороне — это false-positive
  гарант.
- **Extension wins main.** Все MDO-lookup-ы итерируют конфигурации
  через `.iter().rev()` (расширения первыми).
- **Двуязычность и case-insensitivity по умолчанию.**
- **Salsa-tracked везде, где есть осмысленная инкрементальность.**
  Не плодить query на каждое промежуточное значение, но и не
  складывать всё в один монолитный pass.
- **Lowering без `db`.** `hir-def/body/lower` принимает только
  синтаксические решения; всё, что требует resolver/конфигурации/
  типа receiver'а, живёт в `hir-ty`.
- **Cycle handlers — обязательны при подключении новых consumer'ов
  к `proc_signature_query`.** Без них Salsa упадёт при первом
  цикле `infer → lookup_method → proc_signature_query`.
