//! Bridge between legacy `hir_def::Ty` values and interned type-kernel ids.

use std::sync::Arc;

use bsl_config::ConfigId;
use bsl_metadata::MdoType;
use bsl_types::builders::Builders;
use bsl_types::facet::{
    ArgArity, ArrayFacet, DateComponent, DefaultValue, FormBindingFacet, FormBindingTargetFacet,
    FormDataFacet, FunctionFacet, FunctionOrigin, MapFacet, MdoRefFacet, ParamPassing, ParamSpec,
    ProjectionFacet, ProjectionFieldSource, ProjectionSource, StructureFacet, TableFacet,
    TableSource,
};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{LiteralValue, MetadataKind, Projection, ProjectionOrigin, TypeId, TypeKind};
use bsl_types::testing::RootConfigCtx;
use hir_def::ty::{FormDataBinding, FormDataKind, FormDataTarget, SdblProjection, Ty};
use hir_def::Name;

pub fn ty_to_typeid(db: &dyn TypeKernelDb, ty: &Ty) -> TypeId {
    match ty {
        Ty::Unknown => db.unknown(),
        Ty::Number => db.number(None, None),
        Ty::String => db.string(None, false),
        Ty::Boolean => db.boolean(),
        Ty::Date => db.date(DateComponent::DateTime),
        Ty::Undefined => db.undefined(),
        Ty::Null => db.null(),
        Ty::Array => db.array(None),
        Ty::TypedArray(inner) => db.array(Some(ty_to_typeid(db, inner))),
        Ty::Structure => db.structure(None),
        Ty::Map => db.map(None, None),
        Ty::Type => db.type_descriptor(),
        Ty::ValueTable { projection } => db.value_table(
            projection.as_ref().map(|p| sdbl_projection_to_projection(db, p)),
            TableSource::Unknown,
        ),
        Ty::ValueTableRow { projection } => db.value_table_row(
            projection.as_ref().map(|p| sdbl_projection_to_projection(db, p)),
            TableSource::Unknown,
        ),
        Ty::ValueList => db.value_list(None),
        Ty::MetadataRef { kind, name } => {
            debug_loss("T→K", "MetadataRef", "defaulting config_id to Root");
            // loss-ok: Ty has no config axis; §3.6 requires Root during T→K.
            db.metadata_ref(*kind, ty_name_to_kernel(name, "MetadataRef"), &RootConfigCtx)
        }
        Ty::ManagerCollection(kind) => db.manager_collection(*kind),
        Ty::ObjectManager { kind, name } => {
            debug_loss("T→K", "ObjectManager", "defaulting config_id to Root");
            // loss-ok: Ty has no config axis; §3.6 requires Root during T→K.
            db.object_manager(*kind, ty_name_to_kernel(name, "ObjectManager"), &RootConfigCtx)
        }
        Ty::ThisObject { owner } => {
            debug_loss("T→K", "ThisObject", "defaulting config_id to Root");
            // loss-ok: Ty has no config axis; §3.6 requires Root during T→K.
            db.mk_this_object(ConfigId::Root, owner_to_mdo_ref(owner))
        }
        Ty::ThisManager { owner } => {
            debug_loss("T→K", "ThisManager", "defaulting config_id to Root");
            // loss-ok: Ty has no config axis; §3.6 requires Root during T→K.
            db.mk_this_manager(ConfigId::Root, owner_to_mdo_ref(owner))
        }
        Ty::FormData { kind, underlying } => db.mk_form_data(
            form_data_kind_to_facet(*kind),
            underlying.as_ref().map(owner_to_mdo_ref),
        ),
        Ty::FormControl { kind, binding } => {
            db.mk_form_control(*kind, binding.as_ref().map(|b| form_binding_to_facet(db, b)))
        }
        Ty::Function { params, defaults, max_args, ret } => {
            db.function(function_to_facet(db, params, defaults, *max_args, ret))
        }
        Ty::PlatformObject(name) => db.platform_object(ty_name_to_kernel(name, "PlatformObject")),
        Ty::Union(types) => db.union(types.iter().map(|ty| ty_to_typeid(db, ty)).collect()),
        Ty::Query { projections } => db.query(
            projections
                .iter()
                .map(|p| p.as_ref().map(|p| sdbl_projection_to_projection(db, p)))
                .collect(),
        ),
        Ty::QueryResult { projection } => db.query_result(
            projection.as_ref().map(|p| sdbl_projection_to_projection(db, p)),
            ProjectionSource::Unknown,
        ),
        Ty::QueryResultSelection { projection } => db.query_result_selection(
            projection.as_ref().map(|p| sdbl_projection_to_projection(db, p)),
            ProjectionSource::Unknown,
        ),
        Ty::QueryBatchResult { per_query } => db.query_batch_result(
            per_query
                .iter()
                .map(|p| p.as_ref().map(|p| sdbl_projection_to_projection(db, p)))
                .collect(),
        ),
        Ty::AnyMetadataRef { mdo_type } => db.any_metadata_ref(*mdo_type),
    }
}

pub fn typeid_to_ty(db: &dyn TypeKernelDb, id: TypeId) -> Ty {
    match db.lookup_type(id) {
        TypeKind::Unknown => Ty::Unknown,
        TypeKind::Never => {
            debug_loss("K→T", "Never", "Ty has no unreachable/error-sink type");
            // loss-ok: legacy Ty only has Unknown for unrepresentable kernel sentinels.
            Ty::Unknown
        }
        TypeKind::Any => {
            debug_loss("K→T", "Any", "Ty has no explicit top type");
            // loss-ok: legacy Ty cannot distinguish Any from analysis-unknown.
            Ty::Unknown
        }
        TypeKind::Number(facet) => {
            if facet.precision.is_some() || facet.scale.is_some() || facet.origin.is_some() {
                debug_loss("K→T", "Number", "dropping number facet");
                // loss-ok: Ty::Number carries no precision/scale/provenance per §3.6.
            }
            Ty::Number
        }
        TypeKind::String(facet) => {
            if facet.length.is_some() || facet.fixed || facet.origin.is_some() {
                debug_loss("K→T", "String", "dropping string facet");
                // loss-ok: Ty::String carries no length/fixed/provenance per §3.6.
            }
            Ty::String
        }
        TypeKind::Date(facet) => {
            if facet.component != DateComponent::DateTime || facet.origin.is_some() {
                debug_loss("K→T", "Date", "dropping date facet");
                // loss-ok: Ty::Date carries no date/time granularity.
            }
            Ty::Date
        }
        TypeKind::Boolean => Ty::Boolean,
        TypeKind::Null => Ty::Null,
        TypeKind::Undefined => Ty::Undefined,
        TypeKind::Uuid => {
            debug_loss("K→T", "Uuid", "lowering kernel UUID wrapper to platform object");
            // loss-ok: legacy Ty has no UUID variant; platform wrapper is the existing shape.
            Ty::PlatformObject(Name::new("УникальныйИдентификатор"))
        }
        TypeKind::Array(ArrayFacet { element, .. }) => match element {
            Some(element) => Ty::TypedArray(Box::new(typeid_to_ty(db, *element))),
            None => Ty::Array,
        },
        TypeKind::Map(MapFacet { key, value, .. }) => {
            if key.is_some() || value.is_some() {
                debug_loss("K→T", "Map", "dropping map key/value facets");
                // loss-ok: Ty::Map is unparameterised per §3.6.
            }
            Ty::Map
        }
        TypeKind::Structure(StructureFacet { keys, .. }) => {
            if keys.is_some() {
                debug_loss("K→T", "Structure", "dropping structure keys facet");
                // loss-ok: Ty::Structure is unparameterised per §3.6.
            }
            Ty::Structure
        }
        TypeKind::ValueList(element) => {
            if element.is_some() {
                debug_loss("K→T", "ValueList", "dropping value-list element facet");
                // loss-ok: Ty::ValueList is unparameterised per §3.6.
            }
            Ty::ValueList
        }
        TypeKind::ValueTable(facet) => {
            Ty::ValueTable { projection: table_facet_to_sdbl_projection(db, "ValueTable", facet) }
        }
        TypeKind::ValueTableRow(facet) => Ty::ValueTableRow {
            projection: table_facet_to_sdbl_projection(db, "ValueTableRow", facet),
        },
        TypeKind::MetadataRef(facet) => {
            debug_loss("K→T", "MetadataRef", "dropping config_id");
            // loss-ok: Ty::MetadataRef has no config axis per §3.6.
            Ty::MetadataRef { kind: facet.kind, name: kernel_name_to_ty(&facet.name) }
        }
        TypeKind::AnyMetadataRef { mdo_type } => Ty::AnyMetadataRef { mdo_type: *mdo_type },
        TypeKind::MetadataObject(facet) => {
            debug_loss("K→T", "MetadataObject", "collapsing metadata object to MetadataRef");
            // loss-ok: legacy Ty has a single MetadataRef carrier for metadata receivers.
            debug_loss("K→T", "MetadataObject", "dropping config_id");
            // loss-ok: Ty::MetadataRef has no config axis.
            Ty::MetadataRef { kind: facet.kind, name: kernel_name_to_ty(&facet.name) }
        }
        TypeKind::TabularSection { parent, name } => {
            debug_loss("K→T", "TabularSection", "collapsing nested facet to composite name");
            // loss-ok: Ty encodes tabular section identity as MetadataKind + "Parent.Section".
            Ty::MetadataRef {
                kind: MetadataKind::TabularSection { parent: metadata_kind_to_mdo(parent.kind) },
                name: composite_name(&parent.name, name),
            }
        }
        TypeKind::TabularSectionRow { parent, name } => {
            debug_loss("K→T", "TabularSectionRow", "collapsing nested facet to composite name");
            // loss-ok: Ty encodes tabular-section row identity as MetadataKind + "Parent.Section".
            Ty::MetadataRef {
                kind: MetadataKind::TabularSectionRow { parent: metadata_kind_to_mdo(parent.kind) },
                name: composite_name(&parent.name, name),
            }
        }
        TypeKind::RegisterDimension { parent, name } => Ty::MetadataRef {
            kind: MetadataKind::RegisterDimension { parent: metadata_kind_to_mdo(parent.kind) },
            name: composite_name(&parent.name, name),
        },
        TypeKind::RegisterResource { parent, name } => Ty::MetadataRef {
            kind: MetadataKind::RegisterResource { parent: metadata_kind_to_mdo(parent.kind) },
            name: composite_name(&parent.name, name),
        },
        TypeKind::RegisterAttribute { parent, name } => Ty::MetadataRef {
            kind: MetadataKind::RegisterAttribute { parent: metadata_kind_to_mdo(parent.kind) },
            name: composite_name(&parent.name, name),
        },
        TypeKind::RegisterFilter { parent } => Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: metadata_kind_to_mdo(parent.kind) },
            name: kernel_name_to_ty(&parent.name),
        },
        TypeKind::Attribute { parent, name } => {
            debug_loss("K→T", "Attribute", "collapsing bare attribute to parent metadata ref");
            // loss-ok: Ty has no bare metadata-attribute variant.
            Ty::MetadataRef { kind: parent.kind, name: composite_name(&parent.name, name) }
        }
        TypeKind::FormData { kind, underlying } => Ty::FormData {
            kind: form_data_facet_to_kind(kind),
            underlying: underlying.as_ref().map(mdo_ref_to_owner),
        },
        TypeKind::FormControl { kind, binding } => Ty::FormControl {
            kind: *kind,
            binding: binding.as_ref().and_then(|b| form_binding_from_facet(db, b)),
        },
        TypeKind::ThisObject { config_id, owner } => {
            if *config_id != ConfigId::Root {
                debug_loss("K→T", "ThisObject", "dropping non-Root config_id");
                // loss-ok: Ty::ThisObject has no config axis.
            }
            Ty::ThisObject { owner: mdo_ref_to_owner(owner) }
        }
        TypeKind::ThisManager { config_id, owner } => {
            if *config_id != ConfigId::Root {
                debug_loss("K→T", "ThisManager", "dropping non-Root config_id");
                // loss-ok: Ty::ThisManager has no config axis.
            }
            Ty::ThisManager { owner: mdo_ref_to_owner(owner) }
        }
        TypeKind::PlatformObject(facet) => Ty::PlatformObject(kernel_name_to_ty(&facet.name)),
        TypeKind::ValueStorage => {
            debug_loss("K→T", "ValueStorage", "lowering kernel value storage to platform object");
            // loss-ok: legacy Ty has no ValueStorage variant; platform wrapper is existing shape.
            Ty::PlatformObject(Name::new("ХранилищеЗначения"))
        }
        TypeKind::TypeDescriptor => Ty::Type,
        TypeKind::Union(types) => Ty::union(types.iter().map(|id| typeid_to_ty(db, *id)).collect()),
        TypeKind::ManagerCollection(kind) => Ty::ManagerCollection(*kind),
        TypeKind::ObjectManager(facet) => {
            debug_loss("K→T", "ObjectManager", "dropping config_id");
            // loss-ok: Ty::ObjectManager has no config axis per §3.6.
            Ty::ObjectManager { kind: facet.mdo, name: kernel_name_to_ty(&facet.name) }
        }
        TypeKind::Function(facet) => function_from_facet(db, facet),
        TypeKind::QueryResult(facet) => Ty::QueryResult {
            projection: projection_facet_to_sdbl_projection(db, "QueryResult", facet),
        },
        TypeKind::QueryResultSelection(facet) => Ty::QueryResultSelection {
            projection: projection_facet_to_sdbl_projection(db, "QueryResultSelection", facet),
        },
        TypeKind::QueryBatchResult { per_query } => Ty::QueryBatchResult {
            per_query: per_query
                .iter()
                .map(|p| p.as_ref().map(|p| projection_to_sdbl_projection(db, p)))
                .collect(),
        },
        TypeKind::Query { projections } => Ty::Query {
            projections: projections
                .iter()
                .map(|p| p.as_ref().map(|p| projection_to_sdbl_projection(db, p)))
                .collect(),
        },
        future => {
            debug_loss("K→T", "TypeKind", "unhandled future non_exhaustive TypeKind variant");
            // loss-ok: external match on #[non_exhaustive] TypeKind must remain forward-compatible.
            let _ = future;
            Ty::Unknown
        }
    }
}

fn sdbl_projection_to_projection(
    db: &dyn TypeKernelDb,
    projection: &Arc<SdblProjection>,
) -> Arc<Projection> {
    // Phase 3 §4.D: preserve `raw_sdbl_types` through the bridge so
    // hover keeps rendering precision-bearing labels (`Число(15,2)`,
    // `Строка(50)`). Length invariant: when both `fields` and
    // `raw_sdbl_types` are present they index parallel.
    let fields: Arc<[bsl_types::kind::ProjectionField]> = projection
        .fields
        .iter()
        .map(|(name, ty)| {
            bsl_types::kind::ProjectionField::new(
                ty_name_to_kernel(name, "Projection"),
                ty_to_typeid(db, ty),
                ProjectionFieldSource::Unknown,
            )
        })
        .collect();
    let raw_sdbl_types = projection.raw_sdbl_types.as_ref().map(|shadows| {
        shadows
            .iter()
            .map(|s| bsl_types::facet::SdblTypeShadowFacet::new(s.display.clone()))
            .collect::<Arc<[_]>>()
    });
    Arc::new(Projection::new(fields, ProjectionOrigin::SdblQuery, raw_sdbl_types))
}

fn projection_to_sdbl_projection(
    db: &dyn TypeKernelDb,
    projection: &Arc<Projection>,
) -> Arc<SdblProjection> {
    if projection.origin != ProjectionOrigin::Unknown {
        debug_loss("K→T", "Projection", "dropping projection origin");
        // loss-ok: SdblProjection carries no kernel ProjectionOrigin field.
    }
    let fields = projection
        .fields
        .iter()
        .map(|field| {
            if field.source != ProjectionFieldSource::Unknown {
                debug_loss("K→T", "Projection", "dropping projection field source");
                // loss-ok: SdblProjection carries field name and Ty only.
            }
            (kernel_name_to_ty(&field.name), typeid_to_ty(db, field.ty))
        })
        .collect();
    // Phase 3 §4.D: copy kernel-side shadows back into the legacy
    // `SdblTypeShadow` shape so hover renders precision-aware labels.
    let raw_sdbl_types = projection.raw_sdbl_types.as_ref().map(|shadows| {
        shadows
            .iter()
            .map(|s| hir_def::ty::SdblTypeShadow { display: s.display.clone() })
            .collect::<Arc<[_]>>()
    });
    Arc::new(SdblProjection { fields, raw_sdbl_types })
}

fn table_facet_to_sdbl_projection(
    db: &dyn TypeKernelDb,
    variant: &'static str,
    facet: &TableFacet,
) -> Option<Arc<SdblProjection>> {
    if facet.source != TableSource::Unknown {
        debug_loss("K→T", variant, "dropping table source facet");
        // loss-ok: Ty table variants carry projection only per §3.6.
    }
    facet.projection.as_ref().map(|p| projection_to_sdbl_projection(db, p))
}

fn projection_facet_to_sdbl_projection(
    db: &dyn TypeKernelDb,
    variant: &'static str,
    facet: &ProjectionFacet,
) -> Option<Arc<SdblProjection>> {
    if facet.source != ProjectionSource::Unknown {
        debug_loss("K→T", variant, "dropping projection source facet");
        // loss-ok: Ty query result variants carry projection only per §3.6.
    }
    facet.projection.as_ref().map(|p| projection_to_sdbl_projection(db, p))
}

fn form_binding_to_facet(db: &dyn TypeKernelDb, binding: &FormDataBinding) -> FormBindingFacet {
    let path = binding.path().iter().map(|name| ty_name_to_kernel(name, "FormControl")).collect();
    let target = match binding.target() {
        FormDataTarget::TabularSection { mdo_type, owner, section } => {
            FormBindingTargetFacet::TabularSection {
                mdo_ref: make_mdo_ref_facet(*mdo_type, ty_name_to_kernel(owner, "FormControl")),
                section: ty_name_to_kernel(section, "FormControl"),
            }
        }
        FormDataTarget::Attribute { ty } => {
            FormBindingTargetFacet::Attribute { ty: ty_to_typeid(db, ty) }
        }
    };
    make_form_binding_facet(path, target)
}

fn form_binding_from_facet(
    db: &dyn TypeKernelDb,
    binding: &FormBindingFacet,
) -> Option<FormDataBinding> {
    let path: Box<[Name]> = binding.path.iter().map(kernel_name_to_ty).collect();
    let target = match &binding.target {
        FormBindingTargetFacet::TabularSection { mdo_ref, section } => {
            FormDataTarget::TabularSection {
                mdo_type: mdo_ref.mdo_type,
                owner: kernel_name_to_ty(&mdo_ref.name),
                section: kernel_name_to_ty(section),
            }
        }
        FormBindingTargetFacet::Attribute { ty } => {
            FormDataTarget::Attribute { ty: Box::new(typeid_to_ty(db, *ty)) }
        }
        future => {
            debug_loss("K→T", "FormControl", "dropping future form binding target");
            // loss-ok: external match on #[non_exhaustive] target must remain forward-compatible.
            let _ = future;
            return None;
        }
    };
    let out = FormDataBinding::new(path, target);
    if out.is_none() {
        debug_loss("K→T", "FormControl", "dropping empty form binding path");
        // loss-ok: FormDataBinding enforces non-empty paths; empty kernel paths are unrepresentable.
    }
    out
}

fn function_to_facet(
    db: &dyn TypeKernelDb,
    params: &[Ty],
    defaults: &[bool],
    max_args: Option<u32>,
    ret: &Ty,
) -> FunctionFacet {
    let params_len = params.len();
    let params: Arc<[ParamSpec]> = params
        .iter()
        .enumerate()
        .map(|(idx, ty)| {
            make_param_spec(
                format!("p{}", idx + 1),
                ty_to_typeid(db, ty),
                ParamPassing::ByRef,
                max_args.is_none() && idx + 1 == params_len,
            )
        })
        .collect();
    let defaults_arc = defaults
        .iter()
        .map(|has_default| has_default.then(|| DefaultValue::Literal(LiteralValue::Undefined)))
        .collect();
    let min_args = min_args_from_defaults(defaults);
    let max_args = match max_args {
        Some(max) if max > u16::MAX as u32 => {
            debug_loss("T→K", "Function", "clamping max_args to u16::MAX");
            // loss-ok: FunctionFacet stores fixed arity as u16; legacy Ty used u32.
            ArgArity::Fixed(u16::MAX)
        }
        Some(max) => ArgArity::Fixed(max as u16),
        None => ArgArity::Variadic,
    };
    make_function_facet(
        params,
        defaults_arc,
        min_args,
        max_args,
        ty_to_typeid(db, ret),
        FunctionOrigin::Unknown,
    )
}

fn function_from_facet(db: &dyn TypeKernelDb, facet: &FunctionFacet) -> Ty {
    if facet.origin != FunctionOrigin::Unknown {
        debug_loss("K→T", "Function", "dropping function origin");
        // loss-ok: Ty::Function has no provenance field.
    }
    if facet.min_args != min_args_from_default_options(&facet.defaults) {
        debug_loss("K→T", "Function", "dropping explicit min_args");
        // loss-ok: Ty recomputes requiredness from the per-parameter defaults mask.
    }
    if facet.params.iter().any(|p| p.passing != ParamPassing::ByRef || p.variadic) {
        debug_loss("K→T", "Function", "dropping parameter passing/variadic flags");
        // loss-ok: Ty::Function carries only parameter types and max_args.
    }
    if facet
        .defaults
        .iter()
        .flatten()
        .any(|default| !matches!(default, DefaultValue::Literal(LiteralValue::Undefined)))
    {
        debug_loss("K→T", "Function", "dropping default-value payloads");
        // loss-ok: Ty::Function preserves only whether a parameter has a default.
    }
    if !facet.params.is_empty() {
        debug_loss("K→T", "Function", "dropping parameter names");
        // loss-ok: Ty::Function carries positional parameter types only.
    }
    let params = facet.params.iter().map(|param| typeid_to_ty(db, param.ty)).collect();
    let defaults = facet.defaults.iter().map(Option::is_some).collect();
    let max_args = match facet.max_args {
        ArgArity::Fixed(max) => Some(u32::from(max)),
        ArgArity::Variadic => None,
        future => {
            debug_loss("K→T", "Function", "dropping future function arity variant");
            // loss-ok: external match on #[non_exhaustive] arity must remain forward-compatible.
            let _ = future;
            None
        }
    };
    Ty::Function { params, defaults, max_args, ret: Box::new(typeid_to_ty(db, facet.returns)) }
}

fn min_args_from_defaults(defaults: &[bool]) -> u16 {
    defaults
        .iter()
        .rposition(|has_default| !has_default)
        .map_or(0, |idx| (idx + 1).min(u16::MAX as usize) as u16)
}

fn min_args_from_default_options(defaults: &[Option<DefaultValue>]) -> u16 {
    defaults
        .iter()
        .rposition(Option::is_none)
        .map_or(0, |idx| (idx + 1).min(u16::MAX as usize) as u16)
}

fn owner_to_mdo_ref((mdo_type, name): &(MdoType, Name)) -> MdoRefFacet {
    make_mdo_ref_facet(*mdo_type, ty_name_to_kernel(name, "MdoRef"))
}

fn mdo_ref_to_owner(owner: &MdoRefFacet) -> (MdoType, Name) {
    (owner.mdo_type, kernel_name_to_ty(&owner.name))
}

fn form_data_kind_to_facet(kind: FormDataKind) -> FormDataFacet {
    match kind {
        FormDataKind::Structure => FormDataFacet::Structure,
        FormDataKind::Collection => FormDataFacet::Collection,
        FormDataKind::StructureWithCollection => FormDataFacet::StructureWithCollection,
    }
}

fn form_data_facet_to_kind(kind: &FormDataFacet) -> FormDataKind {
    match kind {
        FormDataFacet::Structure => FormDataKind::Structure,
        FormDataFacet::Collection => FormDataKind::Collection,
        FormDataFacet::StructureWithCollection => FormDataKind::StructureWithCollection,
    }
}

fn metadata_kind_to_mdo(kind: MetadataKind) -> MdoType {
    match kind {
        MetadataKind::CatalogRef | MetadataKind::CatalogObject => MdoType::Catalog,
        MetadataKind::DocumentRef | MetadataKind::DocumentObject => MdoType::Document,
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRecordSet
        | MetadataKind::InformationRegisterRecord
        | MetadataKind::InformationRegisterRef
        | MetadataKind::RegisterDimension { parent: MdoType::InformationRegister }
        | MetadataKind::RegisterResource { parent: MdoType::InformationRegister }
        | MetadataKind::RegisterAttribute { parent: MdoType::InformationRegister }
        | MetadataKind::RegisterFilter { parent: MdoType::InformationRegister } => {
            MdoType::InformationRegister
        }
        MetadataKind::AccumulationRegisterRecordSet
        | MetadataKind::AccumulationRegisterRecord
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::RegisterDimension { parent: MdoType::AccumulationRegister }
        | MetadataKind::RegisterResource { parent: MdoType::AccumulationRegister }
        | MetadataKind::RegisterAttribute { parent: MdoType::AccumulationRegister }
        | MetadataKind::RegisterFilter { parent: MdoType::AccumulationRegister } => {
            MdoType::AccumulationRegister
        }
        MetadataKind::AccountingRegisterRecordSet
        | MetadataKind::AccountingRegisterRecord
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::RegisterDimension { parent: MdoType::AccountingRegister }
        | MetadataKind::RegisterResource { parent: MdoType::AccountingRegister }
        | MetadataKind::RegisterAttribute { parent: MdoType::AccountingRegister }
        | MetadataKind::RegisterFilter { parent: MdoType::AccountingRegister } => {
            MdoType::AccountingRegister
        }
        MetadataKind::CalculationRegisterRecordSet
        | MetadataKind::CalculationRegisterRecord
        | MetadataKind::CalculationRegisterRef
        | MetadataKind::RegisterDimension { parent: MdoType::CalculationRegister }
        | MetadataKind::RegisterResource { parent: MdoType::CalculationRegister }
        | MetadataKind::RegisterAttribute { parent: MdoType::CalculationRegister }
        | MetadataKind::RegisterFilter { parent: MdoType::CalculationRegister } => {
            MdoType::CalculationRegister
        }
        MetadataKind::EnumRef => MdoType::Enum,
        MetadataKind::TaskRef | MetadataKind::TaskObject => MdoType::Task,
        MetadataKind::BusinessProcessRef | MetadataKind::BusinessProcessObject => {
            MdoType::BusinessProcess
        }
        MetadataKind::DataProcessorObject => MdoType::DataProcessor,
        MetadataKind::ReportObject => MdoType::Report,
        MetadataKind::ExchangePlanRef | MetadataKind::ExchangePlanObject => MdoType::ExchangePlan,
        MetadataKind::ChartOfAccountsRef | MetadataKind::ChartOfAccountsObject => {
            MdoType::ChartOfAccounts
        }
        MetadataKind::TabularSection { parent } | MetadataKind::TabularSectionRow { parent } => {
            parent
        }
        MetadataKind::RegisterDimension { parent }
        | MetadataKind::RegisterResource { parent }
        | MetadataKind::RegisterAttribute { parent }
        | MetadataKind::RegisterFilter { parent } => parent,
    }
}

fn ty_name_to_kernel(name: &Name, variant: &'static str) -> bsl_metadata::Name {
    debug_loss("T→K", variant, "converting hir_def::Name to String");
    // loss-ok: §3.6 says only SmolStr inlining/performance is lost.
    name.as_str().to_string()
}

fn kernel_name_to_ty(name: &bsl_metadata::Name) -> Name {
    Name::new(name)
}

fn composite_name(parent: &bsl_metadata::Name, child: &bsl_metadata::Name) -> Name {
    Name::new(&format!("{parent}.{child}"))
}

fn debug_loss(direction: &'static str, variant: &'static str, reason: &'static str) {
    tracing::debug!(direction, variant, "{reason}");
}

fn make_mdo_ref_facet(mdo_type: MdoType, name: bsl_metadata::Name) -> MdoRefFacet {
    MdoRefFacet::new(mdo_type, name)
}

fn make_form_binding_facet(
    path: Arc<[bsl_metadata::Name]>,
    target: FormBindingTargetFacet,
) -> FormBindingFacet {
    FormBindingFacet::new(path, target)
}

fn make_param_spec(
    name: bsl_metadata::Name,
    ty: TypeId,
    passing: ParamPassing,
    variadic: bool,
) -> ParamSpec {
    ParamSpec::new(name, ty, passing, variadic)
}

fn make_function_facet(
    params: Arc<[ParamSpec]>,
    defaults: Arc<[Option<DefaultValue>]>,
    min_args: u16,
    max_args: ArgArity,
    returns: TypeId,
    origin: FunctionOrigin,
) -> FunctionFacet {
    FunctionFacet::new(params, defaults, min_args, max_args, returns, origin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::FormElementKind;
    use bsl_types::facet::{FormBindingTargetFacet, NumberFacet, ProjectionSource, StringFacet};
    use bsl_types::testing::InMemoryDb;

    fn db() -> InMemoryDb {
        InMemoryDb::new()
    }

    fn round_trip(ty: Ty) {
        let db = db();
        let id = ty_to_typeid(&db, &ty);
        assert_eq!(typeid_to_ty(&db, id), ty);
    }

    fn name(s: &str) -> Name {
        Name::new(s)
    }

    fn projection() -> Arc<SdblProjection> {
        Arc::new(SdblProjection {
            fields: Arc::new([(name("A"), Ty::Number), (name("B"), Ty::String)]),
            raw_sdbl_types: None,
        })
    }

    fn binding_attr() -> FormDataBinding {
        FormDataBinding::new(
            Box::new([name("Объект"), name("Дата")]),
            FormDataTarget::Attribute { ty: Box::new(Ty::Date) },
        )
        .unwrap()
    }

    macro_rules! rt_test {
        ($test_name:ident, $ty:expr) => {
            #[test]
            fn $test_name() {
                round_trip($ty);
            }
        };
    }

    rt_test!(rt_unknown, Ty::Unknown);
    rt_test!(rt_number, Ty::Number);
    rt_test!(rt_string, Ty::String);
    rt_test!(rt_boolean, Ty::Boolean);
    rt_test!(rt_date, Ty::Date);
    rt_test!(rt_undefined, Ty::Undefined);
    rt_test!(rt_null, Ty::Null);
    rt_test!(rt_array, Ty::Array);
    rt_test!(rt_typed_array, Ty::TypedArray(Box::new(Ty::String)));
    rt_test!(rt_structure, Ty::Structure);
    rt_test!(rt_map, Ty::Map);
    rt_test!(rt_type, Ty::Type);
    rt_test!(rt_value_table, Ty::ValueTable { projection: None });
    rt_test!(rt_value_table_row, Ty::ValueTableRow { projection: None });
    rt_test!(rt_value_list, Ty::ValueList);
    rt_test!(
        rt_metadata_ref,
        Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: name("Номенклатура") }
    );
    rt_test!(rt_manager_collection, Ty::ManagerCollection(MdoType::Document));
    rt_test!(
        rt_object_manager,
        Ty::ObjectManager { kind: MdoType::Catalog, name: name("Номенклатура") }
    );

    /// §4.E.2b-i: these manager families have no `MetadataKind`
    /// value-companion. Before `ManagerFacet`, T→K collapsed them to
    /// `CatalogRef`, so the round-trip rewrote the MDO family. Now they
    /// survive losslessly.
    #[test]
    fn object_manager_without_metadata_kind_companion_round_trips() {
        for mdo in [
            MdoType::Constant,
            MdoType::CommonModule,
            MdoType::ChartOfCharacteristicTypes,
            MdoType::ChartOfCalculationTypes,
            MdoType::ExternalDataSource,
            MdoType::Cube,
            MdoType::DimensionTable,
        ] {
            round_trip(Ty::ObjectManager { kind: mdo, name: name("X") });
        }
    }
    rt_test!(rt_this_object, Ty::ThisObject { owner: (MdoType::Catalog, name("Номенклатура")) });
    rt_test!(rt_this_manager, Ty::ThisManager { owner: (MdoType::Document, name("Заказ")) });
    rt_test!(
        rt_form_data,
        Ty::FormData {
            kind: FormDataKind::Structure,
            underlying: Some((MdoType::Catalog, name("Номенклатура"))),
        }
    );
    rt_test!(
        rt_form_control,
        Ty::FormControl { kind: FormElementKind::Table, binding: Some(binding_attr()) }
    );
    rt_test!(
        rt_function,
        Ty::Function {
            params: Box::new([Ty::Number, Ty::String]),
            defaults: Box::new([false, true]),
            max_args: Some(2),
            ret: Box::new(Ty::Boolean),
        }
    );
    rt_test!(rt_platform_object, Ty::PlatformObject(name("Запрос")));
    rt_test!(rt_union, Ty::union(vec![Ty::String, Ty::Number]));
    rt_test!(rt_query, Ty::Query { projections: Arc::new([None]) });
    rt_test!(rt_query_result, Ty::QueryResult { projection: None });
    rt_test!(rt_query_result_selection, Ty::QueryResultSelection { projection: None });
    rt_test!(rt_query_batch_result, Ty::QueryBatchResult { per_query: Arc::new([None]) });
    rt_test!(rt_any_metadata_ref, Ty::AnyMetadataRef { mdo_type: MdoType::Catalog });

    #[test]
    fn typed_array_recurses() {
        round_trip(Ty::TypedArray(Box::new(Ty::TypedArray(Box::new(Ty::Number)))));
    }

    #[test]
    fn value_table_projection_preserves_fields() {
        round_trip(Ty::ValueTable { projection: Some(projection()) });
    }

    #[test]
    fn value_table_row_projection_preserves_fields() {
        round_trip(Ty::ValueTableRow { projection: Some(projection()) });
    }

    #[test]
    fn query_projection_preserves_per_query_shape() {
        round_trip(Ty::Query { projections: Arc::new([Some(projection()), None]) });
    }

    #[test]
    fn query_result_projection_preserves_fields() {
        round_trip(Ty::QueryResult { projection: Some(projection()) });
    }

    #[test]
    fn query_result_selection_projection_preserves_fields() {
        round_trip(Ty::QueryResultSelection { projection: Some(projection()) });
    }

    #[test]
    fn query_batch_projection_preserves_per_query_shape() {
        round_trip(Ty::QueryBatchResult { per_query: Arc::new([Some(projection()), None]) });
    }

    #[test]
    fn form_binding_tabular_section_preserves_shape() {
        let binding = FormDataBinding::new(
            Box::new([name("Объект"), name("Товары")]),
            FormDataTarget::TabularSection {
                mdo_type: MdoType::Document,
                owner: name("Заказ"),
                section: name("Товары"),
            },
        )
        .unwrap();
        round_trip(Ty::FormControl { kind: FormElementKind::Table, binding: Some(binding) });
    }

    #[test]
    fn union_recursion_uses_ty_canonical_shape() {
        round_trip(Ty::union(vec![
            Ty::Union(Arc::new([Ty::String, Ty::Number])),
            Ty::String,
            Ty::Boolean,
        ]));
    }

    #[test]
    fn k_number_facets_drop_to_ty_number() {
        let db = db();
        let id = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));
        assert_eq!(typeid_to_ty(&db, id), Ty::Number);
    }

    #[test]
    fn k_string_facets_drop_to_ty_string() {
        let db = db();
        let id = db.intern_type(TypeKind::String(StringFacet::with_length(50)));
        assert_eq!(typeid_to_ty(&db, id), Ty::String);
    }

    #[test]
    fn k_map_facets_drop_to_untyped_map() {
        let db = db();
        let key = ty_to_typeid(&db, &Ty::String);
        let value = ty_to_typeid(&db, &Ty::Number);
        let id = db.map(Some(key), Some(value));
        assert_eq!(typeid_to_ty(&db, id), Ty::Map);
    }

    #[test]
    fn k_value_list_element_drops_to_untyped_value_list() {
        let db = db();
        let element = ty_to_typeid(&db, &Ty::String);
        let id = db.intern_type(TypeKind::ValueList(Some(element)));
        assert_eq!(typeid_to_ty(&db, id), Ty::ValueList);
    }

    #[test]
    fn k_form_control_empty_path_binding_drops_binding() {
        let db = db();
        let binding = make_form_binding_facet(
            Arc::new([]),
            FormBindingTargetFacet::Attribute { ty: ty_to_typeid(&db, &Ty::Number) },
        );
        let id = db.intern_type(TypeKind::FormControl {
            kind: FormElementKind::Field,
            binding: Some(binding),
        });
        assert_eq!(
            typeid_to_ty(&db, id),
            Ty::FormControl { kind: FormElementKind::Field, binding: None }
        );
    }

    #[test]
    fn k_projection_field_source_drops_but_fields_survive() {
        let db = db();
        let ty = ty_to_typeid(&db, &Ty::Boolean);
        let projection = db.projection_from_fields(
            vec![("Flag".to_string(), ty)],
            ProjectionFieldSource::Cast,
            ProjectionOrigin::SdblQuery,
        );
        let id = db.query_result(Some(projection), ProjectionSource::Sdbl);
        assert_eq!(
            typeid_to_ty(&db, id),
            Ty::QueryResult {
                projection: Some(Arc::new(SdblProjection {
                    fields: Arc::new([(name("Flag"), Ty::Boolean)]),
                    raw_sdbl_types: None,
                })),
            }
        );
    }

    #[test]
    fn k_uuid_maps_to_platform_object() {
        let db = db();
        let id = db.intern_type(TypeKind::Uuid);
        assert_eq!(typeid_to_ty(&db, id), Ty::PlatformObject(name("УникальныйИдентификатор")));
    }

    #[test]
    fn k_value_storage_maps_to_platform_object() {
        let db = db();
        let id = db.intern_type(TypeKind::ValueStorage);
        assert_eq!(typeid_to_ty(&db, id), Ty::PlatformObject(name("ХранилищеЗначения")));
    }
}
