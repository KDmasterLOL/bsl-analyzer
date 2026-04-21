# Целевая архитектура системы типов BSL Analyzer

Этот документ фиксирует целевую модель разрешения типов данных в проекте и
описывает, чем она отличается от текущего состояния. Цель — единая картина,
на которую опираются последующие изменения в `hir-def`, `hir-ty`, `hir` и
`ide`. Референс — архитектура `rust-analyzer`, адаптированная под особенности
языка BSL.

## Исходные требования

- Тип должен иметь **одну точку разрешения** для выражения, имени и пути.
- Источники знаний о типах (`bsl-platform`, `bsl-metadata`, JSDoc, ручные
  аннотации менеджеров) должны подключаться через явные адаптеры, а не через
  глобальные singleton-ы в hot-path.
- IDE-слои (`ide`, `ide-completion`, `ide-diagnostics`, `ide-assists`) не
  должны напрямую обращаться к `bsl-platform` / `bsl-metadata` / `hir-ty`.
  Единственная публичная точка — `hir::Semantics` и `hir::Type`.
- Инвалидация — через Salsa inputs; изменение конфигурации или исходника
  должно корректно перестраивать затронутые типы.

## Текущее состояние (кратко)

| Слой | Абстракция | Состояние |
|---|---|---|
| `syntax` | Rowan CST / typed AST | полноценный |
| `hir-def::ty` | `enum Ty` из 15 вариантов | плоский, без фазы lowering |
| `hir-def::resolver` | `Resolver::resolve_path` для 1/2/3 сегментов | работает, даёт `PathResolution` до `Definition` |
| `hir-def::symbol_tree` | O(1) lookup методов / переменных | работает |
| `hir-ty::infer` | `InferenceContext`, `InferenceResult` | `Resolver` **не используется**, резолв имён дублируется |
| `hir-ty::method_resolution` | `CommonModule.Method()` | параллельный пайплайн резолва |
| `bsl-platform` | Методы платформенных типов | lookup через глобальный `PlatformData::instance()` |
| `bsl-metadata` | `Configuration`, MDO | есть, но недоступна из `hir-ty` |
| `hir` | `Semantics` + re-export | нет `hir::Type`, IDE ходит в `hir-ty` и `bsl-platform` напрямую |

Ключевой архитектурный долг — два независимых пайплайна резолва: Definition
умеет `Документы.ПКО.Создать` и возвращает `MethodId`, но type inference об
этом не знает и не умеет поднять результат до `Ty::MetadataRef`.

## Слои целевой архитектуры

```
Frameworks & Drivers   : lsp-server, salsa, tokio, lsp-types
Interface Adapters     : ide, ide-completion, ide-diagnostics, ide-assists
Application / Facade   : hir  (Semantics, Type, Method, Field, Module)
Domain Services        : hir-ty (lowering, inference), hir-def (resolver, bodies),
                         bsl-platform, bsl-metadata   — как адаптеры знаний
Entities               : Ty, TypeRef, MetadataKind, FunctionSignature, Name
```

Правила зависимостей:

- `Entities` не зависят от `db`/`salsa`/`resolver`, только от std и smol_str.
- `hir-def` зависит от `syntax` и entities.
- `hir-ty` зависит от `hir-def`, entities и от адаптеров знаний
  (`bsl-platform`, `bsl-metadata`) через trait-интерфейсы, без обращения
  к глобальным singleton-ам.
- `hir` — **единственный** крейт, зависящий одновременно от `hir-ty` и
  `hir-def`. Именно он собирает OO-фасад `hir::Type`.
- `ide*` зависят только от `hir`.
- Запрещено: `hir-def → hir-ty`, `hir-ty → hir`, `ide* → bsl-platform`
  напрямую.

## Стадии жизненного цикла типа

```
Source          : BSL / XML / JSDoc
 ↓ parse
AST             : syntax::ast
 ↓ item_tree + body lowering
HIR             : hir-def (Body, SymbolTree, ItemTree)
 ↓ type ref extraction
TypeRef         : синтаксическое представление типа (из JSDoc, XML, `Новый`)
 ↓ TyLoweringContext
Ty              : семантический тип (резолв имён, MDO, платформа)
 ↓ InferenceContext (с Expectation, narrowing)
InferenceResult : тип каждого выражения + диагностики
 ↓ hir::Type facade
IDE features    : completion, hover, diagnostics, signature help
```

### `TypeRef`

Синтаксическое представление типа — добавляется как сущность в `hir-def`.
Источники:

- описания типов из XML-метаданных (`Справочник.Номенклатура`,
  `СправочникСсылка.Номенклатура`, `ОпределяемыйТип.Х`);
- JSDoc-комментарии к процедурам и функциям (`// Параметры:`,
  `// Возвращаемое значение:`);
- операторы `Новый Тип(...)`.

`TypeRef` — это arena-индексируемая сущность без привязки к db.

### `Ty`

Семантический тип. Расширяется относительно текущего:

- `Ty::ManagerCollection(MdoType)` — глобалы `Документы`, `Справочники`,
  `РегистрыСведений`;
- `Ty::ObjectManager { kind, name }` — конкретный менеджер
  (`Документы.ПКО`);
- `Ty::Union(Arc<[Ty]>)` — объединения из ОписаниеТипов или JSDoc;
- `Ty::ThisObject { owner }` — самоссылка в контексте модуля объекта /
  менеджера;
- существующие варианты остаются.

`Ty` не знает про `db` и остаётся entity-уровнем.

### `TyLoweringContext`

Чистый адаптер `TypeRef → Ty`. Использует `Resolver` для разрешения имён и
адаптеры знаний для доступа к `Configuration` и списку платформенных типов.
Вся логика, сейчас размазанная по `Ty::from_type_name`, `Ty::from_new_expr`
и веткам `Expr::New`, собирается здесь.

### Единая точка `infer_expr`

Inference обязан использовать `Resolver` — тот же, что и Semantics. Никаких
параллельных пайплайнов:

- `Expr::Path(name)` → `Resolver::resolve_name` → `def_to_ty`;
- `Expr::Field { base, field }` → type-directed lookup
  (`ManagerCollection` → MDO-объект, `ObjectManager` → manager prop,
  `MetadataRef` → реквизит из `Configuration`, иначе платформа);
- `Expr::MethodCall` → аналогично, с таблицей методов менеджеров и
  резолвом через `bsl-platform`.

### `hir::Type` (OO-фасад)

Новая сущность в крейте `hir`:

```rust
pub struct Type<'db, DB> { db: &'db DB, source_root: SourceRootId, inner: hir_ty::Ty }

impl<'db, DB: HirDatabase> Type<'db, DB> {
    pub fn display_name(&self) -> String;
    pub fn methods(&self) -> Vec<Method<'db, DB>>;
    pub fn fields(&self)  -> Vec<Field<'db, DB>>;
    pub fn is_assignable_to(&self, other: &Type) -> bool;
    pub fn narrow(&self, predicate: TypePredicate) -> Type;
}
```

Все IDE-фичи обращаются только к нему. `platform_completion` перестаёт знать
про `bsl_platform::PlatformData::instance()`.

## Salsa-модель (целевая)

```rust
#[salsa::db]
pub trait HirDatabase: DefDatabase {
    fn infer(&self, file_id: FileId) -> Arc<InferenceResult>;
    // ExprId is body-local; owner disambiguates across the file.
    fn type_of_expr(&self, file_id: FileId, owner: DefWithBodyId, expr: ExprId) -> Ty;

    fn ty_of_method_signature(&self, method: MethodId) -> Arc<FunctionSignature>;
    fn ty_of_mdo_attribute(&self, mdo: MdoRef, attr: Name) -> Ty;
    fn ty_of_this_object(&self, file_id: FileId) -> Ty;

    fn configuration(&self, src: SourceRootId) -> Option<Arc<Configuration>>;
    fn manager_methods(&self, kind: MdoType) -> Arc<ManagerMethodTable>;
}
```

Инвалидация корректна: `infer` зависит от `module_bodies` и
`configuration`; изменение XML или BSL-исходника автоматически сбрасывает
кэш вниз по графу.

## Таблица резолва выражений

| Выражение BSL | Целевой `Ty` |
|---|---|
| литералы | Number / String / Date / Boolean / Undefined / Null |
| `Новый Массив` | `Ty::Array` |
| `Новый Запрос` | `Ty::PlatformObject("Запрос")` |
| `Запрос.Выполнить()` | `Ty::PlatformObject("РезультатЗапроса")` |
| `Документы` | `Ty::ManagerCollection(Document)` |
| `Документы.ПКО` | `Ty::ObjectManager { Document, "ПКО" }` |
| `Документы.ПКО.СоздатьДокумент()` | `Ty::MetadataRef { DocumentObject, "ПКО" }` |
| `Документы.ПКО.ПустаяСсылка()` | `Ty::MetadataRef { DocumentRef, "ПКО" }` |
| `Документ.Сумма` | тип реквизита из `Configuration` |
| `Документ.Товары` | `Ty::MetadataRef { TabularSection, "ПКО.Товары" }` |
| `Документ.Товары.Добавить()` | `Ty::MetadataRef { TabularSectionRow, "ПКО.Товары" }` |
| `ТипЗнч(Х)` | `Ty::Type` |
| `ЭтотОбъект` в модуле объекта справочника | `Ty::MetadataRef { CatalogObject, <имя> }` |
| параметр функции с JSDoc | результат lowering `TypeRef` из JSDoc |
| переменная после `Если ТипЗнч(Х) = Тип("Массив")` | `Ty::Array` внутри блока (narrowing) |

## Что не заимствуем из rust-analyzer

- `Canonical<T>` — не нужен: в BSL нет trait-based полиморфизма;
- trait solver / `chalk` / `rustc_type_ir` — язык не имеет трейтов и дженериков;
- autoderef, lifetime regions, `Projection`, `Placeholder` — семантически
  отсутствуют в BSL.

## Источники знаний о типах (не дублировать)

`hir-ty` **не** владеет данными о типах — он оркестрирует lowering и резолв
через существующие источники. Правило: новых «параллельных» таблиц в
`hir-ty` не создаём, только thin-адаптеры над существующим.

| Источник | Что даёт | Как подключается |
|---|---|---|
| `bsl-platform::PlatformData::get_manager_methods` | Методы менеджеров (`СоздатьДокумент`, `ПустаяСсылка`, …) на все MdoType | Через `TyLoweringContext`, возвратный тип типизируется lowering-ом |
| `bsl-platform::PlatformData::get_method` | Методы платформенных объектов (Запрос, ТаблицаЗначений, …) | Через `resolve_method_return_type` |
| `bsl-metadata::MdoType::{from_plural, is_plural_form, manager_type_prefix}` | Маппинг глобалов `Документы`/`Справочники`/… → `MdoType` (RU/EN, case-insensitive) | Прямой вызов в `infer` |
| `bsl-metadata::metadata_object::StandardAttributeKind` | Стандартные реквизиты с учётом Hierarchical/CodeLength/Periodicity | Прямой вызов при lowering полей `MetadataRef` |
| `bsl-metadata::xml_parser::type_parser` | Парсинг XML-типов (`Справочник.Х`, `ОпределяемыйТип.Х`, `Composite`, `AnyRef`) | Выход мигрирует с `AttributeType` на `TypeRef` |
| `hir-def::ty::doc_types` | Парсер типов из JSDoc | Выход мигрирует на `TypeRef` |
| `hir-def::resolver::Resolver` | Name → `PathResolution` / `Resolution` | Единая точка резолва для Definition и Ty слоёв |
| `ide-db::provider::AnalysisProvider::visible_configurations` | main + CFE extensions для файла | Поднимается в `TypeDatabase` trait |

## Архитектурные решения

Следующие решения приняты до начала реализации и фиксируют выбор между
альтернативами:

1. **Доступ `hir-ty` к Configuration — через trait `TypeDatabase`.**
   В `hir-ty` заводится `TypeDatabase: DefDatabase` с методом
   `configurations(file_id) -> Vec<VisibleConfig>`. Реализация в `ide-db`
   делегирует в существующий `AnalysisProvider`. `hir-ty` не зависит от
   `ide-db` напрямую, только от абстракции.

2. **`Resolver` расширяется до итерации по `visible_configurations`.**
   `resolve_cross_module` и `resolve_three_level` ходят по main → extensions
   в порядке, заданном provider-ом. Definition- и Type-слои видят одинаковый
   граф.

3. **Union-семантика для расширений CFE.**
   Методы одноимённых CommonModule / менеджерных модулей из main и
   extensions объединяются. При совпадении имени выигрывает расширение
   (соответствует runtime-семантике 1С).

4. **Универсальный `TyLoweringContext` на все источники XML-типов.**
   Реквизиты MDO, `Тип("СправочникСсылка.Х")`, `ОписаниеТипов`, JSDoc —
   одна pipeline: источник → `TypeRef` → `Ty`. Никаких частных lowering-ов.

5. **Встроенные функции — `Scope::Builtins` в `Resolver`.**
   Shadowing builtin > var > module > workspace закреплён в Resolver, а не
   копипастится в `Semantics` и `infer`. Это BSL-специфика: локальная
   переменная с именем встроенной функции не затеняет её вызов.

## Path-sensitive анализ (narrowing) — отдельная подсистема

Narrowing типов через `Если ТипЗнч(Х) = Тип("...")` или
`Если Х <> Неопределено` — **не расширение** текущего inference, а
самостоятельный path-sensitive анализ поверх CFG. Текущий
`InferenceContext` идёт линейным проходом без merge состояний в ветках.
Narrowing проектируется отдельным ADR и реализуется после стабилизации
базовой системы типов.

## Ключевые инварианты

1. Один способ резолвить имя — `Resolver`, используется и в Definition, и в
   Ty-слое. Shadowing builtin>var>module встроен в Resolver.
2. Entities (`Ty`, `TypeRef`) не зависят от `db`/`salsa`.
3. Один публичный API для IDE — `hir::Semantics` + `hir::Type`.
4. Источники знаний о типах подключаются через trait-адаптеры, не через
   глобальные singleton-ы. `hir-ty` оркестрирует, не владеет данными.
5. Инвалидация — через Salsa inputs (`file_text`, `configuration`), без
   обходов кэша.
6. Диагностики типов коллектятся внутри lowering и inference и форвардятся
   единым каналом в `ide-diagnostics`.
7. `hir-ty` видит main + CFE через `visible_configurations`, не только
   текущий SourceRoot.

## Покрытие инвариантов по вехам

| Инвариант | Статус | Подтверждающие тесты |
|---|---|---|
| 1. Единый `Resolver` для Definition и Ty-слоёв | ✅ M1 + M2 | `Expr::Path` → `Resolver::resolve_name` (M1); `Expr::New`, `Expr::Call(QualifiedPath)` (2 и 3 сегмента) → `TyLoweringContext` + `Resolver::resolve_qualified_method` / `resolve_three_level_method` (M2). Regression: `crates/ide/tests/type_system_invariants.rs::single_resolver_cascade_*`. |
| 2. Entities (`Ty`, `TypeRef`) не зависят от `db`/`salsa` | ✅ M2 | `TypeRef` и новые варианты `Ty::ManagerCollection` / `Ty::ObjectManager` — plain data (`crates/hir-def/src/type_ref.rs`, `crates/hir-def/src/ty.rs`). |
| 3. Один публичный API для IDE — `hir::Semantics` + `hir::Type` | ✅ M3 | Фасад `hir::Type` (`crates/hir/src/type_facade.rs`) с `.methods()`, `.fields()`, `.method_return_type()`, `.field_type()`, `.is_ref_type()`, `.manager()`, `.display_name()`. `Semantics::type_of_expr(SyntaxNode) -> Ty` даёт IDE одну точку входа поверх `InferenceResult::expr_types_by_body`. Миграция consumers: `platform_completion`, `mdo_completion`. Гейт CI: `scripts/check-invariants.sh` (keyword docs — исключение). |
| 4. Источники знаний о типах через trait-адаптеры | ✅ M3 | `ConfigsDatabase` (M1), bridge `AttributeType → TypeRef` (M2); в M3 добавились `hir_ty::method_lookup::lookup_method` (Task 5) и `hir_ty::field_lookup::lookup_field` (Task 7) — единственные путь `(receiver_ty, name) → Ty` для методов и полей. `bsl_platform::manager_methods_query` вынес последний IDE-side `PlatformData::instance()` за Salsa-gate. |
| 5. Инвалидация через Salsa inputs | ✅ M1 + M2 | `db.infer` транзитивно зависит от `db.configurations`; 2-level и 3-level ре-resolving подтверждены `infer_invalidation::infer_invalidates_when_config_set_changes` + `infer_three_level::three_level_invalidates_on_config_change`. |
| 6. Диагностики типов коллектятся единым каналом | 🟡 частично | `InferenceDiagnostic::{UnresolvedMethodCall, MismatchedArgCount, TypeMismatch}` покрывают method calls; narrowing / field-unresolved — M4+. |
| 7. `hir-ty` видит main + CFE через `visible_configurations` | ✅ M1 | `ConfigsDatabase::configurations`, visibility-gate в resolver, invalidation test. |

## Что M2 закрыл, и что осталось

**M2 выполнено** (ветка `feature/type-system-m2-ty-lowering`, 11 коммитов):

- `TypeRef` — синтаксический слой, bridge с XML `AttributeType` без reverse dep;
- `TyLoweringContext` — единая pipeline `TypeRef → Ty` для `Expr::New`, JSDoc, `Тип("…")`, XML;
- Новые варианты `Ty::ManagerCollection(MdoType)` и `Ty::ObjectManager { kind, name }` с factory-guard;
- 3-сегментные qualified calls в inference через `Resolver::resolve_three_level_method` + `resolve_three_level_call`;
- `Expr::Path(plural)` → `Ty::ManagerCollection` (с сохранением semantic shadowing через `var_types`);
- JSDoc wiring: `doc_types` → `TypeRef`, `MethodSymbol`/`ParamSymbol` получили `type_ref` поля, `materialise_signature` lower-ит их через `TyLoweringContext` в `FunctionSignature` — **первый user-visible эффект типовой системы**;
- Intergation-регрессионный набор: `infer_new_expr`, `infer_three_level`, `infer_plural_managers`, `infer_jsdoc_types`, `type_system_invariants` (≈30 behavioral кейсов).

## Что M3 закрыл, и что осталось

**M3 выполнено** (ветка `feature/type-system-m3-hir-type-facade`, 15 коммитов + Codex fix-ups):

- **`MetadataKind` расширен** до 15 вариантов: добавлены `EnumRef`, `TaskRef`, `BusinessProcessRef`, `InformationRegisterRef`, `AccumulationRegisterRef`, `AccountingRegisterRef`, `CalculationRegisterRef`, `TabularSection { parent }`, `TabularSectionRow { parent }`. Tabular-section варианты несут `parent: MdoType` (Codex MAJOR fix) — снимает неоднозначность `Catalog "X".Товары` vs `Document "X".Товары`.
- **`Ty::Union(Arc<[Ty]>)`** со smart-constructor (`flatten`, `sort+dedup`, collapse singletons) — даёт канонический, Eq-стабильный union. XML `AttributeType::Composite` теперь лоуверится в `TypeRef::Union` и дальше в `Ty::union`, вместо `Ty::Unknown`.
- **JSDoc union parser**: `// Возвращаемое значение: Число, Строка` → `TypeRef::Union([Number, String])`.
- **`MethodLookup` адаптер** (`crates/hir-ty/src/method_lookup.rs`) — `(receiver_ty, method_name) → Option<MethodInfo>`. Единственный вход для `Expr::MethodCall` (и для Call-where-callee-is-Field после Task 14 fix). Замещает удалённый `resolve_method_return_type`.
- **`FieldLookup` адаптер** (`crates/hir-ty/src/field_lookup.rs`) — `(configs, receiver_ty, field_name) → Option<FieldInfo>`. Покрывает MDO attributes (custom + standard через `mdo.attributes`), tabular section promotion (с `parent: MdoType`), tabular row attributes. `Expr::Field` теперь реально резолвится (был `Ty::Unknown`-stub).
- **Per-body `expr_types_by_body`** (Task 9, Codex Q5 HIGH): `InferenceResult` сохраняет per-body inferred types после merge, keyed по `DefWithBodyId`. `Semantics::type_of_expr(SyntaxNode) -> Ty` — IDE-facing bridge через `BodySourceMap::expr_at_range`.
- **`hir::Type` фасад** (`crates/hir/src/type_facade.rs`): `.methods()`, `.fields()`, `.method_return_type()`, `.field_type()`, `.is_ref_type()`, `.manager()`, `.display_name()`. `Method` / `Field` — лёгкие DTO с Russian + English именами и typed return/params. Fields dedup по обеим алиасам (Codex MAJOR fix).
- **IDE миграция**: `platform_completion` и `mdo_completion` больше не дёргают `PlatformData::instance()` на type-path. Остались два whitelist-ed callsite для keyword docs. Манагер-методы теперь через `manager_methods_query` (Salsa).
- **Invariants CI-gate**: `scripts/check-invariants.sh` — grep-based facade-boundary check со skip-in-comments и allow-marker ("allow: keyword docs (M3 exception)").
- **Регрессионный набор**: `infer_field_lookup.rs` (5 behavioral E2E через designer fixture), `type_of_expr.rs` (6 acceptance для ExprId-bridge), `type_system_invariants.rs` расширен до 4 тестов (включая single-method-lookup и single-field-lookup invariants).
- **Task 14 bonus fix**: `infer_call` теперь детектит `Expr::Call { callee: Expr::Field { base, field } }` и роутит через `MethodLookup` — иначе fluent chains (`Запрос.Выполнить().Выбрать()`) возвращали `Ty::Unknown` после Task 11 убрал syntax-fallback.

**Оставлено для M4**:

- **Narrowing** (`Если ТипЗнч(Х) = Тип("Массив")` сужает `Х: Union(..., Array)` до `Array` внутри блока). Требует смены `InferenceContext` с линейной модели на CFG-driven merge — отдельный ADR (`ADR-01-narrowing.md`, stub).
- **`Ty::ThisObject { owner }`** — редкая фича модуля объекта; отдельный ADR, блокирован на стабилизации field-lookup для `CatalogObject`.
- **Полная миграция `bsl_metadata::AttributeType` консьюмеров** → `TypeRef` (40+ производственных ссылок).
- **Полноценный FieldLookup для регистров/планов/задач**: `MetadataKind::{AccumulationRegisterRef, AccountingRegisterRef, …}` узнаются как типы, но `.Измерения.X` / `.Движения.ДобавитьРасход()` — пока `Ty::Unknown` (register storage в отдельном `Configuration.registers`).
- **Предопределённые элементы / значения перечислений** на `Ty::ObjectManager` (`Перечисления.Состояния.Активен`, `Справочники.Валюты.Доллар`). Требует manager-side adapter.
- **`hir::Type::is_assignable_to`** — Codex Q4 MEDIUM: без narrowing / union-subtyping лжёт callers'ам. Ждёт M4 ADR.
- **`UnresolvedField`-диагностика** в `InferenceDiagnostic` (сейчас FieldLookup просто молча отдаёт `Ty::Unknown`).

## Связанные документы

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — общая архитектура проекта;
- [`DATAFLOW.md`](DATAFLOW.md) — flow-sensitive анализ и инфраструктура CFG;
- [`adr/ADR-01-narrowing.md`](adr/ADR-01-narrowing.md) — M4 narrowing scope & open questions (stub).
