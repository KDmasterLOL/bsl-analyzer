//! `Builders` trait — the recommended construction API.
//!
//! Plain Rust trait, declared here and implemented blanket over
//! [`crate::intern::TypeKernelDb`]. Each method wraps `intern_type`
//! with the right `TypeKind` variant + facet, so callers don't have
//! to remember which sub-struct goes inside which variant.
//!
//! `Builders` is a **convenience layer**, not a security boundary —
//! canonicalisation and equality are enforced at the interning
//! gateway (`intern_type`), not at the builder. Callers that skip the
//! builders and construct `TypeKind` literals by hand still get
//! canonical `TypeId`s through `intern_type` (§4.1 rule 7 of the
//! design).
//!
//! `ConfigCtx` is the metadata-config oracle used by `metadata_ref` /
//! `metadata_object` to populate `MetaRefFacet.config_id`. Sandbox
//! provides [`crate::testing::RootConfigCtx`]; production
//! `bsl-config::VisibleConfig` will implement the same trait
//! (Phase 2).

use std::sync::Arc;

use bsl_metadata::Name;

use crate::facet::{
    ArrayFacet, DateFacet, FormBindingFacet, FormDataFacet, FormElementFacet, FunctionFacet,
    ManagerFacet, MapFacet, MdoRefFacet, MetaObjFacet, MetaRefFacet, NumberFacet,
    PlatformObjectFacet, ProjectionFacet, ProjectionSource, StringFacet, StructureFacet,
    TableFacet, TableSource,
};
use crate::intern::TypeKernelDb;
use crate::kind::{
    ConfigId, MetadataKind, Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin,
    TypeId, TypeKind,
};

/// Resolver for [`ConfigId`] given an MDO `(kind, name)`.
///
/// Total — returns a [`ConfigId`] for every input. Unresolvable names
/// produce `ConfigId::Unknown(name.clone())` (carries the name so
/// distinct unresolved names don't collide).
///
/// Sandbox impl: [`crate::testing::RootConfigCtx`] returns
/// `ConfigId::Root` for any input. Production
/// `bsl-config::VisibleConfig` (Phase 2) returns `Resolved(idx)` for
/// known MDOs and `Unknown(name)` otherwise.
pub trait ConfigCtx {
    fn resolve_config_id(&self, kind: MetadataKind, name: &Name) -> ConfigId;

    /// [`ConfigId`] resolution for an object manager (`Справочники.X`).
    ///
    /// Keyed by the MDO family rather than a [`MetadataKind`] because
    /// managers have no value-type companion (see
    /// [`crate::facet::ManagerFacet`]). The default returns
    /// [`ConfigId::Root`]; the production `bsl-config` oracle overrides
    /// it to resolve per-config managers by `(mdo, name)`.
    fn resolve_manager_config_id(&self, _mdo: bsl_metadata::MdoType, _name: &Name) -> ConfigId {
        ConfigId::Root
    }
}

impl ConfigCtx for crate::testing::RootConfigCtx {
    fn resolve_config_id(&self, _kind: MetadataKind, _name: &Name) -> ConfigId {
        ConfigId::Root
    }
}

/// Recommended construction API. Every method routes through
/// [`TypeKernelDb::intern_type`] so callers get a canonical
/// [`TypeId`].
///
/// Blanket impl below covers every concrete `TypeKernelDb`; trait
/// objects (`&dyn TypeKernelDb`) also satisfy it via `+ ?Sized`.
pub trait Builders: TypeKernelDb {
    // ── Bottom / top ────────────────────────────────────────────

    /// `Unknown` — analysis incomplete.
    fn unknown(&self) -> TypeId {
        self.intern_type(TypeKind::Unknown)
    }

    /// `Never` — proven unreachable / error sink.
    fn never(&self) -> TypeId {
        self.intern_type(TypeKind::Never)
    }

    /// `Any` — explicit `Произвольный`.
    fn any(&self) -> TypeId {
        self.intern_type(TypeKind::Any)
    }

    // ── Primitives ──────────────────────────────────────────────

    /// `Число` with optional precision + scale.
    fn number(&self, precision: Option<u8>, scale: Option<u8>) -> TypeId {
        self.intern_type(TypeKind::Number(NumberFacet { precision, scale, origin: None }))
    }

    /// `Строка` with optional length and `fixed` (Фиксированная) flag.
    fn string(&self, length: Option<u32>, fixed: bool) -> TypeId {
        self.intern_type(TypeKind::String(StringFacet { length, fixed, origin: None }))
    }

    /// Date / Time / DateTime.
    fn date(&self, component: crate::facet::DateComponent) -> TypeId {
        self.intern_type(TypeKind::Date(DateFacet { component, origin: None }))
    }

    /// `Булево`.
    fn boolean(&self) -> TypeId {
        self.intern_type(TypeKind::Boolean)
    }

    /// `Null`.
    fn null(&self) -> TypeId {
        self.intern_type(TypeKind::Null)
    }

    /// `Неопределено`.
    fn undefined(&self) -> TypeId {
        self.intern_type(TypeKind::Undefined)
    }

    // ── Collections ────────────────────────────────────────────

    /// `Массив` with optional element type. `None` produces an
    /// unparameterised array (legacy `Массив` without element info).
    fn array(&self, element: Option<TypeId>) -> TypeId {
        self.intern_type(TypeKind::Array(ArrayFacet { element }))
    }

    /// `Соответствие` with optional key/value types.
    fn map(&self, key: Option<TypeId>, value: Option<TypeId>) -> TypeId {
        self.intern_type(TypeKind::Map(MapFacet { key, value }))
    }

    /// `Структура` with optional keys. `None` means keys aren't
    /// known statically (`Новый Структура()`).
    fn structure(&self, keys: Option<Arc<[Name]>>) -> TypeId {
        self.intern_type(TypeKind::Structure(StructureFacet { keys }))
    }

    /// `ТаблицаЗначений`. `projection` carries field info when known
    /// (e.g. derived from `Запрос.Выполнить().Выгрузить()`).
    fn value_table(&self, projection: Option<Arc<Projection>>, source: TableSource) -> TypeId {
        self.intern_type(TypeKind::ValueTable(TableFacet { projection, source }))
    }

    /// Row of a projected `ТаблицаЗначений`.
    fn value_table_row(&self, projection: Option<Arc<Projection>>, source: TableSource) -> TypeId {
        self.intern_type(TypeKind::ValueTableRow(TableFacet { projection, source }))
    }

    /// `СписокЗначений` with optional element type.
    fn value_list(&self, element: Option<TypeId>) -> TypeId {
        self.intern_type(TypeKind::ValueList(element))
    }

    /// `УникальныйИдентификатор`.
    fn uuid(&self) -> TypeId {
        self.intern_type(TypeKind::Uuid)
    }

    /// `ХранилищеЗначения`.
    fn value_storage(&self) -> TypeId {
        self.intern_type(TypeKind::ValueStorage)
    }

    /// `Тип` reflection wrapper (returned by `ТипЗнч`).
    fn type_descriptor(&self) -> TypeId {
        self.intern_type(TypeKind::TypeDescriptor)
    }

    // ── Metadata references ────────────────────────────────────

    /// `MetadataRef` — concrete metadata reference. Always routes
    /// `config_id` through [`ConfigCtx`] so the invariant
    /// "config_id is required" (design §4.3) is enforced.
    fn metadata_ref(&self, kind: MetadataKind, name: Name, cfg: &dyn ConfigCtx) -> TypeId {
        let config_id = cfg.resolve_config_id(kind, &name);
        self.intern_type(TypeKind::MetadataRef(MetaRefFacet { kind, name, config_id }))
    }

    /// `MetadataObject` — concrete metadata object. Same
    /// `config_id` routing as `metadata_ref`.
    fn metadata_object(&self, kind: MetadataKind, name: Name, cfg: &dyn ConfigCtx) -> TypeId {
        let config_id = cfg.resolve_config_id(kind, &name);
        self.intern_type(TypeKind::MetadataObject(MetaObjFacet { kind, name, config_id }))
    }

    /// `AnyMetadataRef { mdo_type }` — coarser than [`Builders::metadata_ref`];
    /// represents "any reference of this MDO flavour" without
    /// binding to a specific name.
    fn any_metadata_ref(&self, mdo_type: bsl_metadata::MdoType) -> TypeId {
        self.intern_type(TypeKind::AnyMetadataRef { mdo_type })
    }

    /// `ManagerCollection(MdoType)` — `Справочники`, `Документы`, ….
    fn manager_collection(&self, mdo_type: bsl_metadata::MdoType) -> TypeId {
        self.intern_type(TypeKind::ManagerCollection(mdo_type))
    }

    /// `ObjectManager` — `Справочники.X`. Keyed by [`MdoType`] (the
    /// metadata-object family), not a [`MetadataKind`] value-companion.
    fn object_manager(
        &self,
        mdo: bsl_metadata::MdoType,
        name: Name,
        cfg: &dyn ConfigCtx,
    ) -> TypeId {
        let config_id = cfg.resolve_manager_config_id(mdo, &name);
        self.intern_type(TypeKind::ObjectManager(ManagerFacet { mdo, name, config_id }))
    }

    /// `ObjectManager` from an already-resolved [`ConfigId`].
    ///
    /// Mirrors [`Builders::object_manager`] but takes the `config_id`
    /// directly instead of consulting a [`ConfigCtx`] oracle — for
    /// callers that derive the manager from a receiver whose config
    /// identity is already known (e.g. the `manager()` facade promoting
    /// a `MetadataRef`'s `config_id`), so the manager stays bound to the
    /// same config rather than collapsing to [`ConfigId::Root`].
    fn object_manager_with_config(
        &self,
        mdo: bsl_metadata::MdoType,
        name: Name,
        config_id: ConfigId,
    ) -> TypeId {
        self.intern_type(TypeKind::ObjectManager(ManagerFacet { mdo, name, config_id }))
    }

    // ── Metadata inner shapes ─────────────────────────────────

    /// Tabular section of an MDO. `parent` identifies the owner MDO
    /// (e.g. `metadata_ref(CatalogObject, "ПКО", cfg)`'s facet);
    /// `name` is the section name (`"Товары"`).
    fn tabular_section(&self, parent: MetaRefFacet, name: Name) -> TypeId {
        self.intern_type(TypeKind::TabularSection { parent, name })
    }

    /// A single row of a tabular section.
    fn tabular_section_row(&self, parent: MetaRefFacet, name: Name) -> TypeId {
        self.intern_type(TypeKind::TabularSectionRow { parent, name })
    }

    /// `Измерение` of a register.
    fn register_dimension(&self, parent: MetaRefFacet, name: Name) -> TypeId {
        self.intern_type(TypeKind::RegisterDimension { parent, name })
    }

    /// `Ресурс` of a register.
    fn register_resource(&self, parent: MetaRefFacet, name: Name) -> TypeId {
        self.intern_type(TypeKind::RegisterResource { parent, name })
    }

    /// `Реквизит` of a register.
    fn register_attribute(&self, parent: MetaRefFacet, name: Name) -> TypeId {
        self.intern_type(TypeKind::RegisterAttribute { parent, name })
    }

    /// `Отбор` of a record set.
    fn register_filter(&self, parent: MetaRefFacet) -> TypeId {
        self.intern_type(TypeKind::RegisterFilter { parent })
    }

    /// Bare attribute of an MDO (catalog/document attribute).
    fn attribute(&self, parent: MetaRefFacet, name: Name) -> TypeId {
        self.intern_type(TypeKind::Attribute { parent, name })
    }

    /// Construct a [`MetaRefFacet`] for use with inner-shape builders
    /// like [`Builders::tabular_section`]. Routes `config_id` through
    /// `&dyn ConfigCtx` for the same invariant guarantee as
    /// [`Builders::metadata_ref`].
    fn meta_ref_facet(&self, kind: MetadataKind, name: Name, cfg: &dyn ConfigCtx) -> MetaRefFacet {
        let config_id = cfg.resolve_config_id(kind, &name);
        MetaRefFacet { kind, name, config_id }
    }

    // ── Form-specific shapes ──────────────────────────────────

    /// `ДанныеФормы*` wrapper with optional underlying MDO.
    fn mk_form_data(&self, kind: FormDataFacet, underlying: Option<MdoRefFacet>) -> TypeId {
        self.intern_type(TypeKind::FormData { kind, underlying })
    }

    /// Form control with optional resolved binding.
    fn mk_form_control(&self, kind: FormElementFacet, binding: Option<FormBindingFacet>) -> TypeId {
        self.intern_type(TypeKind::FormControl { kind, binding })
    }

    /// Contextual `ЭтотОбъект`.
    fn mk_this_object(&self, config_id: ConfigId, owner: MdoRefFacet) -> TypeId {
        self.intern_type(TypeKind::ThisObject { config_id, owner })
    }

    /// Contextual `ЭтотМенеджер`.
    fn mk_this_manager(&self, config_id: ConfigId, owner: MdoRefFacet) -> TypeId {
        self.intern_type(TypeKind::ThisManager { config_id, owner })
    }

    // ── Platform wrapper ──────────────────────────────────────

    /// `Запрос`, `ТабличныйДокумент`, … — typed platform value.
    fn platform_object(&self, name: Name) -> TypeId {
        self.intern_type(TypeKind::PlatformObject(PlatformObjectFacet { name }))
    }

    // ── Union ─────────────────────────────────────────────────

    /// `Union` of pre-interned member ids. Canonicalisation (sort,
    /// dedupe, flatten, sentinel handling) runs inside `intern_type`
    /// — this builder is a thin convenience wrapper.
    fn union(&self, members: Vec<TypeId>) -> TypeId {
        self.intern_type(TypeKind::Union(members.into()))
    }

    // ── Query results ─────────────────────────────────────────

    /// `Запрос.Выполнить()` result.
    fn query_result(
        &self,
        projection: Option<Arc<Projection>>,
        source: ProjectionSource,
    ) -> TypeId {
        self.intern_type(TypeKind::QueryResult(ProjectionFacet { projection, source }))
    }

    /// `Результат.Выбрать()` cursor.
    fn query_result_selection(
        &self,
        projection: Option<Arc<Projection>>,
        source: ProjectionSource,
    ) -> TypeId {
        self.intern_type(TypeKind::QueryResultSelection(ProjectionFacet { projection, source }))
    }

    /// `Запрос.ВыполнитьПакет()` batch result.
    fn query_batch_result(&self, per_query: Arc<[Option<Arc<Projection>>]>) -> TypeId {
        self.intern_type(TypeKind::QueryBatchResult { per_query })
    }

    /// `Запрос` value holding one or more sub-query projections.
    fn query(&self, projections: Arc<[Option<Arc<Projection>>]>) -> TypeId {
        self.intern_type(TypeKind::Query { projections })
    }

    // ── Function ──────────────────────────────────────────────

    /// User-defined function / procedure.
    fn function(&self, facet: FunctionFacet) -> TypeId {
        self.intern_type(TypeKind::Function(facet))
    }

    // ── Projection helper ─────────────────────────────────────

    /// Build a `Projection` from a list of `(name, ty)` pairs with a
    /// uniform field source. Provenance is stripped at intern time
    /// anyway, but the helper lets call sites stay readable.
    fn projection_from_fields(
        &self,
        fields: Vec<(Name, TypeId)>,
        field_source: ProjectionFieldSource,
        origin: ProjectionOrigin,
    ) -> Arc<Projection> {
        let fields: Arc<[ProjectionField]> = fields
            .into_iter()
            .map(|(name, ty)| ProjectionField { name, ty, source: field_source })
            .collect();
        Arc::new(Projection { fields, origin, raw_sdbl_types: None })
    }
}

// Blanket impl — every `TypeKernelDb` (sized or not) automatically
// satisfies `Builders`. `+ ?Sized` lets `&dyn TypeKernelDb` route
// through builders without an explicit trait object cast.
impl<T: TypeKernelDb + ?Sized> Builders for T {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use bsl_metadata::MdoType;

    use crate::facet::{
        DateComponent, FormBindingTargetFacet, FormDataFacet, FormElementFacet, NumberFacet,
    };
    use crate::testing::{InMemoryDb, RootConfigCtx};

    #[test]
    fn builders_round_trip_primitives() {
        let db = InMemoryDb::new();
        // Same construction via builder twice → same TypeId.
        let a = db.number(Some(15), Some(2));
        let b = db.number(Some(15), Some(2));
        assert_eq!(a, b);
        // Builder result equals hand-built TypeKind via intern_type.
        let manual = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));
        assert_eq!(a, manual);
    }

    #[test]
    fn builder_unknown_returns_sentinel() {
        // `Builders::unknown` routes through `intern_type(TypeKind::Unknown)`,
        // which canonicalises to the preseeded sentinel that
        // `InMemoryDb::unknown` (the inherent accessor) also exposes.
        // Disambiguate the name via UFCS — both names exist on the same
        // value.
        let db = InMemoryDb::new();
        let via_builder = <InMemoryDb as Builders>::unknown(&db);
        let via_sentinel_accessor = InMemoryDb::unknown(&db);
        assert_eq!(via_builder, via_sentinel_accessor);
    }

    #[test]
    fn builder_string_with_length() {
        let db = InMemoryDb::new();
        let a = db.string(Some(50), false);
        let b = db.string(Some(50), false);
        assert_eq!(a, b);
        // Different length → different id.
        let c = db.string(Some(100), false);
        assert_ne!(a, c);
        // Fixed flag matters.
        let d = db.string(Some(50), true);
        assert_ne!(a, d);
    }

    #[test]
    fn builder_metadata_ref_routes_through_config_ctx() {
        let db = InMemoryDb::new();
        let cfg = RootConfigCtx;
        let cat_a = db.metadata_ref(MetadataKind::CatalogRef, "Номенклатура".to_string(), &cfg);
        let cat_b = db.metadata_ref(MetadataKind::CatalogRef, "Номенклатура".to_string(), &cfg);
        assert_eq!(cat_a, cat_b);

        // Different name → different id.
        let cat_c = db.metadata_ref(MetadataKind::CatalogRef, "Контрагенты".to_string(), &cfg);
        assert_ne!(cat_a, cat_c);

        // Different kind → different id even with same name.
        let doc = db.metadata_ref(MetadataKind::DocumentRef, "Номенклатура".to_string(), &cfg);
        assert_ne!(cat_a, doc);

        // Verify the stored facet has config_id::Root.
        match db.lookup_type(cat_a) {
            TypeKind::MetadataRef(facet) => {
                assert_eq!(facet.kind, MetadataKind::CatalogRef);
                assert_eq!(facet.name, "Номенклатура");
                assert_eq!(facet.config_id, ConfigId::Root);
            }
            other => panic!("expected MetadataRef; got {:?}", other),
        }
    }

    #[test]
    fn builder_union_canonicalises() {
        let db = InMemoryDb::new();
        let n = db.number(Some(15), Some(2));
        let s = db.string(None, false);
        // Different argument orders → same canonical id.
        let u1 = db.union(vec![n, s]);
        let u2 = db.union(vec![s, n]);
        assert_eq!(u1, u2);
    }

    #[test]
    fn builders_dyn_compatible() {
        // Object-safety: `&dyn TypeKernelDb` must satisfy `Builders`
        // via the blanket impl with `+ ?Sized`.
        let db = InMemoryDb::new();
        let dyn_db: &dyn TypeKernelDb = &db;
        let id = dyn_db.number(Some(15), Some(2));
        assert_eq!(dyn_db.lookup_type(id), &TypeKind::Number(NumberFacet::with_scale(15, 2)));
    }

    #[test]
    fn builder_date_round_trip() {
        let db = InMemoryDb::new();
        let a = db.date(DateComponent::DateTime);
        let b = db.date(DateComponent::DateTime);
        assert_eq!(a, b);
        let c = db.date(DateComponent::Date);
        assert_ne!(a, c);
    }

    #[test]
    fn builder_array_with_and_without_element() {
        let db = InMemoryDb::new();
        let n = db.number(None, None);
        let arr_typed = db.array(Some(n));
        let arr_typed_2 = db.array(Some(n));
        let arr_untyped = db.array(None);
        assert_eq!(arr_typed, arr_typed_2);
        assert_ne!(arr_typed, arr_untyped);
    }

    #[test]
    fn builder_form_data_round_trip() {
        let db = InMemoryDb::new();
        let owner =
            MdoRefFacet { mdo_type: MdoType::Catalog, name: "Контрагенты".to_string() };
        let a = db.mk_form_data(FormDataFacet::Structure, Some(owner.clone()));
        let b = db.mk_form_data(FormDataFacet::Structure, Some(owner.clone()));
        assert_eq!(a, b);

        match db.lookup_type(a) {
            TypeKind::FormData { kind, underlying } => {
                assert_eq!(*kind, FormDataFacet::Structure);
                assert_eq!(underlying.as_ref(), Some(&owner));
            }
            other => panic!("expected FormData; got {:?}", other),
        }
    }

    #[test]
    fn builder_form_data_distinguishes_kind_and_underlying() {
        let db = InMemoryDb::new();
        let owner =
            MdoRefFacet { mdo_type: MdoType::Catalog, name: "Контрагенты".to_string() };
        let structure = db.mk_form_data(FormDataFacet::Structure, Some(owner.clone()));
        let collection = db.mk_form_data(FormDataFacet::Collection, Some(owner.clone()));
        let bare_structure = db.mk_form_data(FormDataFacet::Structure, None);

        assert_ne!(structure, collection);
        assert_ne!(structure, bare_structure);
    }

    #[test]
    fn builder_form_control_round_trip() {
        let db = InMemoryDb::new();
        let ty = db.string(Some(30), false);
        let binding = FormBindingFacet {
            path: Arc::from(["Объект".to_string(), "Наименование".to_string()]),
            target: FormBindingTargetFacet::Attribute { ty },
        };
        let id = db.mk_form_control(FormElementFacet::Field, Some(binding.clone()));

        match db.lookup_type(id) {
            TypeKind::FormControl { kind, binding: stored } => {
                assert_eq!(*kind, FormElementFacet::Field);
                assert_eq!(stored.as_ref(), Some(&binding));
            }
            other => panic!("expected FormControl; got {:?}", other),
        }
    }

    #[test]
    fn builder_form_control_distinguishes_kind_and_binding() {
        let db = InMemoryDb::new();
        let owner =
            MdoRefFacet { mdo_type: MdoType::Catalog, name: "Контрагенты".to_string() };
        let binding = FormBindingFacet {
            path: Arc::from(["Объект".to_string(), "Товары".to_string()]),
            target: FormBindingTargetFacet::TabularSection {
                mdo_ref: owner,
                section: "Товары".to_string(),
            },
        };
        let table = db.mk_form_control(FormElementFacet::Table, Some(binding.clone()));
        let field = db.mk_form_control(FormElementFacet::Field, Some(binding));
        let bare_table = db.mk_form_control(FormElementFacet::Table, None);

        assert_ne!(table, field);
        assert_ne!(table, bare_table);
    }

    #[test]
    fn builder_this_object_and_manager_round_trip() {
        let db = InMemoryDb::new();
        let owner = MdoRefFacet { mdo_type: MdoType::Document, name: "Заказ".to_string() };
        let object = db.mk_this_object(ConfigId::Root, owner.clone());
        let manager = db.mk_this_manager(ConfigId::Root, owner.clone());
        assert_ne!(object, manager);

        assert_eq!(
            db.lookup_type(object),
            &TypeKind::ThisObject { config_id: ConfigId::Root, owner: owner.clone() }
        );
        assert_eq!(
            db.lookup_type(manager),
            &TypeKind::ThisManager { config_id: ConfigId::Root, owner }
        );
    }

    #[test]
    fn builder_this_variants_distinguish_config_id() {
        let db = InMemoryDb::new();
        let owner = MdoRefFacet { mdo_type: MdoType::Document, name: "Заказ".to_string() };
        let root = db.mk_this_object(ConfigId::Root, owner.clone());
        let resolved = db.mk_this_object(ConfigId::Resolved(1), owner);

        assert_ne!(root, resolved);
    }

    #[test]
    fn projection_helper_builds_arc_projection() {
        let db = InMemoryDb::new();
        let n = db.number(Some(15), Some(2));
        let s = db.string(None, false);
        let proj = db.projection_from_fields(
            vec![("Цена".to_string(), n), ("Наименование".to_string(), s)],
            ProjectionFieldSource::Column,
            ProjectionOrigin::SdblQuery,
        );
        assert_eq!(proj.fields.len(), 2);
        assert_eq!(proj.fields[0].name, "Цена");
        assert_eq!(proj.fields[1].ty, s);
    }
}
