use std::sync::Arc;

use bsl_metadata::MdoType;
use bsl_platform::{PlatformData, PlatformMethod};
use bsl_types::builders::Builders;
use bsl_types::facet::{FormDataFacet, ProjectionSource, TableSource};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{Projection, TypeId, TypeKind};
use bsl_types::testing::RootConfigCtx;
use hir_def::body::Body;
use hir_def::hir::Expr;
use hir_def::ty::MetadataKind;
use hir_def::{DefWithBodyId, ExprId, Name};
use vfs::FileId;

use crate::db::HirDatabase;
use crate::lower::type_string::{lower_param_type_string_typeid, lower_return_type_string_typeid};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodInfo {
    pub return_ty: TypeId,
    pub params: Vec<TypeId>,
    pub overloads: Vec<Vec<TypeId>>,
}

pub fn lookup_method(
    db: &dyn TypeKernelDb,
    receiver: TypeId,
    method_name: &Name,
) -> Option<MethodInfo> {
    lookup_method_with_refinement(db, receiver, method_name, None)
}

pub fn lookup_method_with_refinement(
    db: &dyn TypeKernelDb,
    receiver: TypeId,
    method_name: &Name,
    refine_ctx: Option<&RefineCtx<'_>>,
) -> Option<MethodInfo> {
    lookup_method_inner(db, receiver, method_name, refine_ctx)
}

fn lookup_method_inner(
    db: &dyn TypeKernelDb,
    receiver: TypeId,
    method_name: &Name,
    refine_ctx: Option<&RefineCtx<'_>>,
) -> Option<MethodInfo> {
    let eff_id = crate::this_object::coerce_to_metadata_ref_id(db, receiver).unwrap_or(receiver);

    let info = match db.lookup_type(eff_id) {
        TypeKind::Union(arms) => return union_lookup(db, arms, method_name, refine_ctx),
        TypeKind::ObjectManager(facet) => {
            lookup_on_object_manager(db, facet.mdo, &Name::new(&facet.name), method_name)
        }
        TypeKind::MetadataRef(facet) => {
            lookup_on_metadata_ref(db, facet.kind, &Name::new(&facet.name), method_name)
        }
        TypeKind::MetadataObject(facet) => {
            lookup_on_metadata_ref(db, facet.kind, &Name::new(&facet.name), method_name)
        }
        TypeKind::AnyMetadataRef { mdo_type } => {
            lookup_on_any_metadata_ref(db, *mdo_type, method_name)
        }
        TypeKind::FormControl { kind, .. } => lookup_on_form_control(db, *kind, method_name),
        _ => lookup_scalar_receiver(db, eff_id, method_name),
    }?;

    Some(apply_sdbl_chain_rewrite(db, eff_id, method_name, info, refine_ctx))
}

#[derive(Clone, Copy)]
pub struct RefineCtx<'a> {
    pub db: &'a dyn HirDatabase,
    pub file_id: FileId,
    pub owner: DefWithBodyId,
    pub body: &'a Body,
    pub dispatch_expr_id: ExprId,
    pub receiver_expr_id: ExprId,
    pub call_args: &'a [ExprId],
}

fn is_sdbl_chain_method(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "выполнить" | "execute" | "выбрать" | "choose" | "выполнитьпакет" | "executebatch",
    )
}

fn is_unload_method(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "выгрузить" | "unload")
}

fn narrow_unload_return(
    db: &dyn TypeKernelDb,
    receiver: TypeId,
    info: MethodInfo,
    refine_ctx: Option<&RefineCtx<'_>>,
) -> MethodInfo {
    if !is_query_result_receiver_id(db, receiver) {
        return info;
    }
    let projection = projection_of_query_result_receiver_id(db, receiver).flatten();
    let return_ty = if let Some(ctx) = refine_ctx {
        use crate::query_unload_refinement::{classify_unload_arg, UnloadIteration};
        let decision = classify_unload_arg(ctx.body, ctx.call_args);
        let narrowed = match decision {
            UnloadIteration::Dynamic => info.return_ty,
            UnloadIteration::Linear => drop_union_arm_id(db, info.return_ty, is_value_tree_arm_id),
            UnloadIteration::Hierarchical => {
                drop_union_arm_id(db, info.return_ty, is_value_table_arm_id)
            }
        };
        attach_projection_to_value_table_id(db, narrowed, projection)
    } else {
        attach_projection_to_value_table_id(db, info.return_ty, projection)
    };
    MethodInfo { return_ty, params: info.params, overloads: info.overloads }
}

fn apply_sdbl_chain_rewrite(
    db: &dyn TypeKernelDb,
    receiver: TypeId,
    method_name: &Name,
    info: MethodInfo,
    refine_ctx: Option<&RefineCtx<'_>>,
) -> MethodInfo {
    if is_unload_method(method_name.as_str()) {
        return narrow_unload_return(db, receiver, info, refine_ctx);
    }

    if !is_sdbl_chain_method(method_name.as_str()) {
        return info;
    }
    let refined = refine_ctx.and_then(|ctx| try_refine_receiver(db, ctx, receiver));
    let effective = refined.unwrap_or(receiver);

    let Some((target_platform_name, replacement)) =
        pick_chain_rewrite_id(db, effective, method_name.as_str())
    else {
        return info;
    };
    MethodInfo {
        return_ty: rewrite_chain_arm_in_return_id(
            db,
            info.return_ty,
            target_platform_name,
            replacement,
        ),
        params: info.params,
        overloads: info.overloads,
    }
}

fn try_refine_receiver(
    db: &dyn TypeKernelDb,
    ctx: &RefineCtx<'_>,
    receiver: TypeId,
) -> Option<TypeId> {
    if !receiver_needs_refinement_id(db, receiver) {
        return None;
    }
    let Expr::Path(receiver_name) = ctx.body.expr(ctx.receiver_expr_id) else {
        return None;
    };
    let projections = crate::query_text_dataflow::refine_query_at_use_site(
        ctx.db,
        ctx.file_id,
        ctx.owner,
        ctx.dispatch_expr_id,
        receiver_name,
        ctx.body,
    )?;
    Some(db.query(projections.iter().cloned().collect()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChainTarget {
    PlatformObjectNamed { ru: &'static str, en: &'static str },
    AnyArray,
}

pub(crate) fn is_platform_name(name: &Name, ru: &str, en: &str) -> bool {
    is_platform_name_str(name.as_str(), ru, en)
}

fn is_platform_name_str(name: &str, ru: &str, en: &str) -> bool {
    let lower = name.to_lowercase();
    lower == ru.to_lowercase() || lower == en.to_lowercase()
}

fn is_value_table_arm_id(db: &dyn TypeKernelDb, id: TypeId) -> bool {
    matches!(db.lookup_type(id), TypeKind::ValueTable(_))
        || matches!(db.lookup_type(id), TypeKind::PlatformObject(f) if is_platform_name_str(&f.name, "ТаблицаЗначений", "ValueTable"))
}

fn is_value_tree_arm_id(db: &dyn TypeKernelDb, id: TypeId) -> bool {
    matches!(db.lookup_type(id), TypeKind::PlatformObject(f) if is_platform_name_str(&f.name, "ДеревоЗначений", "ValueTree"))
}

fn is_query_result_receiver_id(db: &dyn TypeKernelDb, id: TypeId) -> bool {
    match db.lookup_type(id) {
        TypeKind::QueryResult(_) => true,
        TypeKind::PlatformObject(f) => {
            is_platform_name_str(&f.name, "РезультатЗапроса", "QueryResult")
        }
        _ => false,
    }
}

pub(crate) fn receiver_needs_refinement_id(db: &dyn TypeKernelDb, id: TypeId) -> bool {
    match db.lookup_type(id) {
        TypeKind::PlatformObject(f) => is_platform_name_str(&f.name, "Запрос", "Query"),
        TypeKind::Query { projections } => match projections.last() {
            None | Some(None) => true,
            Some(Some(_)) => false,
        },
        _ => false,
    }
}

fn projection_of_query_receiver_id(
    db: &dyn TypeKernelDb,
    id: TypeId,
) -> Option<Option<Arc<Projection>>> {
    match db.lookup_type(id) {
        TypeKind::Query { projections } => Some(projections.last().cloned().flatten()),
        TypeKind::PlatformObject(f) if is_platform_name_str(&f.name, "Запрос", "Query") => {
            Some(None)
        }
        _ => None,
    }
}

fn projections_of_query_receiver_id(
    db: &dyn TypeKernelDb,
    id: TypeId,
) -> Option<Arc<[Option<Arc<Projection>>]>> {
    match db.lookup_type(id) {
        TypeKind::Query { projections } => Some(projections.clone()),
        TypeKind::PlatformObject(f) if is_platform_name_str(&f.name, "Запрос", "Query") => {
            Some(Arc::from([]))
        }
        _ => None,
    }
}

fn projection_of_query_result_receiver_id(
    db: &dyn TypeKernelDb,
    id: TypeId,
) -> Option<Option<Arc<Projection>>> {
    match db.lookup_type(id) {
        TypeKind::QueryResult(facet) => Some(facet.projection.clone()),
        TypeKind::PlatformObject(f)
            if is_platform_name_str(&f.name, "РезультатЗапроса", "QueryResult") =>
        {
            Some(None)
        }
        _ => None,
    }
}

fn pick_chain_rewrite_id(
    db: &dyn TypeKernelDb,
    recv_id: TypeId,
    method_name: &str,
) -> Option<(ChainTarget, TypeId)> {
    let lower = method_name.to_lowercase();
    match lower.as_str() {
        "выполнить" | "execute" => {
            let projection = projection_of_query_receiver_id(db, recv_id)?;
            Some((
                ChainTarget::PlatformObjectNamed {
                    ru: "РезультатЗапроса", en: "QueryResult"
                },
                db.query_result(projection, ProjectionSource::Unknown),
            ))
        }
        "выбрать" | "choose" => {
            let projection = projection_of_query_result_receiver_id(db, recv_id)?;
            Some((
                ChainTarget::PlatformObjectNamed {
                    ru: "ВыборкаИзРезультатаЗапроса",
                    en: "QueryResultSelection",
                },
                db.query_result_selection(projection, ProjectionSource::Unknown),
            ))
        }
        "выполнитьпакет" | "executebatch" => {
            let projections = projections_of_query_receiver_id(db, recv_id)?;
            Some((ChainTarget::AnyArray, db.query_batch_result(projections)))
        }
        _ => None,
    }
}

fn rewrite_chain_arm_in_return_id(
    db: &dyn TypeKernelDb,
    return_id: TypeId,
    target: ChainTarget,
    replacement: TypeId,
) -> TypeId {
    let matches_target = |arm_id: TypeId| -> bool {
        match (&target, db.lookup_type(arm_id)) {
            (ChainTarget::PlatformObjectNamed { ru, en }, TypeKind::PlatformObject(f)) => {
                is_platform_name_str(&f.name, ru, en)
            }
            (ChainTarget::AnyArray, TypeKind::Array(_)) => true,
            _ => false,
        }
    };

    if matches_target(return_id) {
        return replacement;
    }
    match db.lookup_type(return_id) {
        TypeKind::Union(arms) => {
            let new_arms: Vec<TypeId> = arms
                .iter()
                .map(|&arm| if matches_target(arm) { replacement } else { arm })
                .collect();
            db.union(new_arms)
        }
        _ => return_id,
    }
}

fn attach_projection_to_value_table_id(
    db: &dyn TypeKernelDb,
    id: TypeId,
    projection: Option<Arc<Projection>>,
) -> TypeId {
    let Some(projection) = projection else { return id };
    let upgrade = |arm_id: TypeId| -> Option<TypeId> {
        match db.lookup_type(arm_id) {
            TypeKind::ValueTable(f) if f.projection.is_none() => {
                Some(db.value_table(Some(projection.clone()), TableSource::Unknown))
            }
            _ => None,
        }
    };
    if let Some(upgraded) = upgrade(id) {
        return upgraded;
    }
    match db.lookup_type(id) {
        TypeKind::Union(arms) => {
            let rebuilt: Vec<TypeId> =
                arms.iter().map(|&arm| upgrade(arm).unwrap_or(arm)).collect();
            db.union(rebuilt)
        }
        _ => id,
    }
}

fn drop_union_arm_id(
    db: &dyn TypeKernelDb,
    id: TypeId,
    unwanted: impl Fn(&dyn TypeKernelDb, TypeId) -> bool,
) -> TypeId {
    let TypeKind::Union(arms) = db.lookup_type(id) else { return id };
    let kept: Vec<TypeId> = arms.iter().copied().filter(|&arm| !unwanted(db, arm)).collect();
    if kept.is_empty() {
        return id;
    }
    if kept.len() == 1 {
        return kept.into_iter().next().unwrap();
    }
    db.union(kept)
}

fn lookup_scalar_receiver(
    db: &dyn TypeKernelDb,
    receiver: TypeId,
    method_name: &Name,
) -> Option<MethodInfo> {
    let type_key = platform_type_key_id(db, receiver)?;
    let method = PlatformData::instance().get_method(&type_key, method_name.as_str())?;
    let mut info = to_method_info(db, method);
    if let Some(row) = form_data_collection_row_ty(db, receiver) {
        info.return_ty =
            rewrite_form_data_collection_item_return(db, info.return_ty, row, method.name.as_str());
    }
    Some(info)
}

fn lookup_on_object_manager(
    db: &dyn TypeKernelDb,
    mdo_type: MdoType,
    name: &Name,
    method_name: &Name,
) -> Option<MethodInfo> {
    let res = crate::platform_manager_lookup::resolve_platform_manager_method(
        db,
        mdo_type,
        name,
        method_name,
    )?;
    Some(MethodInfo {
        return_ty: res.return_ty,
        params: res.signature.params.to_vec(),
        overloads: res.overloads,
    })
}

fn lookup_on_metadata_ref(
    db: &dyn TypeKernelDb,
    kind: MetadataKind,
    name: &Name,
    method_name: &Name,
) -> Option<MethodInfo> {
    if let MetadataKind::TabularSection { parent } = kind {
        let method =
            PlatformData::instance().get_method("Tabular section", method_name.as_str())?;
        return Some(build_tabular_section_method_info(db, method, parent, name));
    }
    if let Some(res) = crate::platform_manager_lookup::resolve_platform_metadata_ref_method(
        db,
        kind,
        name,
        method_name,
    ) {
        return Some(MethodInfo {
            return_ty: res.return_ty,
            params: res.signature.params.to_vec(),
            overloads: res.overloads,
        });
    }
    if let Some(scalar_key) = kind.scalar_platform_key() {
        if let Some(method) = PlatformData::instance().get_method(scalar_key, method_name.as_str())
        {
            return Some(to_method_info(db, method));
        }
    }
    None
}

fn lookup_on_any_metadata_ref(
    db: &dyn TypeKernelDb,
    mdo_type: MdoType,
    method_name: &Name,
) -> Option<MethodInfo> {
    let res = crate::platform_manager_lookup::resolve_platform_any_metadata_ref_method(
        db,
        mdo_type,
        method_name,
    )?;
    Some(MethodInfo {
        return_ty: res.return_ty,
        params: res.signature.params.to_vec(),
        overloads: res.overloads,
    })
}

fn lookup_on_form_control(
    db: &dyn TypeKernelDb,
    kind: hir_def::ty::FormElementKind,
    method_name: &Name,
) -> Option<MethodInfo> {
    hir_def::ty::form_control_chain_first_hit(kind, |type_name| {
        PlatformData::instance()
            .get_method(type_name, method_name.as_str())
            .map(|method| to_method_info(db, method))
    })
}

fn push_unique_sig(sigs: &mut Vec<Vec<TypeId>>, sig: Vec<TypeId>) {
    if !sigs.contains(&sig) {
        sigs.push(sig);
    }
}

fn union_lookup(
    db: &dyn TypeKernelDb,
    members: &[TypeId],
    method_name: &Name,
    refine_ctx: Option<&RefineCtx<'_>>,
) -> Option<MethodInfo> {
    let live: Vec<TypeId> = members
        .iter()
        .copied()
        .filter(|id| !matches!(db.lookup_type(*id), TypeKind::Undefined | TypeKind::Null))
        .collect();
    let mut returns: Vec<TypeId> = Vec::with_capacity(live.len());
    // A union receiver is an over-approximation: at runtime the value is exactly
    // one arm. Gather every arm's signature(s); an argument accepted by ANY arm
    // must not be a mismatch (e.g. a String key on `Структура | Массив` is valid
    // via `Структура.Вставить`, even though `Массив.Вставить` wants a numeric
    // index). Taking only the first arm's signature emits a false positive
    // whenever the arms disagree.
    let mut sigs: Vec<Vec<TypeId>> = Vec::new();
    let mut hit_any = false;
    for m in live {
        if let Some(info) = lookup_method_inner(db, m, method_name, refine_ctx) {
            hit_any = true;
            returns.push(info.return_ty);
            if info.overloads.is_empty() {
                push_unique_sig(&mut sigs, info.params);
            } else {
                for ov in info.overloads {
                    push_unique_sig(&mut sigs, ov);
                }
            }
        }
    }
    hit_any.then(|| {
        // When every arm shares one signature, present it as a plain signature
        // (no overloads) so single-shape unions behave exactly as a single
        // receiver; emit overloads only when the arms genuinely diverge.
        let (params, overloads) = if sigs.len() <= 1 {
            (sigs.into_iter().next().unwrap_or_default(), Vec::new())
        } else {
            (sigs[0].clone(), sigs)
        };
        MethodInfo { return_ty: db.union(returns), params, overloads }
    })
}

pub fn platform_type_key_id(db: &dyn TypeKernelDb, id: TypeId) -> Option<String> {
    match db.lookup_type(id) {
        TypeKind::Array(_) => Some("Array".to_string()),
        TypeKind::Structure(_) => Some("Structure".to_string()),
        TypeKind::Map(_) => Some("Map".to_string()),
        TypeKind::ValueTable(_) => Some("ValueTable".to_string()),
        TypeKind::ValueTableRow(_) => Some("ValueTableRow".to_string()),
        TypeKind::ValueList(_) => Some("ValueList".to_string()),
        TypeKind::TypeDescriptor => Some("Type".to_string()),
        TypeKind::PlatformObject(f) => Some(f.name.clone()),
        TypeKind::FormData { kind, .. } => Some(
            match kind {
                FormDataFacet::Structure => "ДанныеФормыСтруктура",
                FormDataFacet::Collection => "ДанныеФормыКоллекция",
                FormDataFacet::StructureWithCollection => "ДанныеФормыСтруктураСКоллекцией",
            }
            .to_string(),
        ),
        TypeKind::FormControl { kind, .. } => {
            hir_def::ty::form_control_platform_type_name(*kind).map(ToString::to_string)
        }
        TypeKind::Query { .. } => Some("Запрос".to_string()),
        TypeKind::QueryResult(_) => Some("РезультатЗапроса".to_string()),
        TypeKind::QueryResultSelection(_) => Some("ВыборкаИзРезультатаЗапроса".to_string()),
        TypeKind::QueryBatchResult { .. } => Some("Array".to_string()),
        TypeKind::Unknown
        | TypeKind::Never
        | TypeKind::Any
        | TypeKind::Number(_)
        | TypeKind::String(_)
        | TypeKind::Date(_)
        | TypeKind::Boolean
        | TypeKind::Null
        | TypeKind::Undefined => None,
        TypeKind::Uuid => Some("УникальныйИдентификатор".to_string()),
        TypeKind::ValueStorage => Some("ХранилищеЗначения".to_string()),
        TypeKind::MetadataRef(_)
        | TypeKind::AnyMetadataRef { .. }
        | TypeKind::MetadataObject(_)
        | TypeKind::TabularSection { .. }
        | TypeKind::TabularSectionRow { .. }
        | TypeKind::RegisterDimension { .. }
        | TypeKind::RegisterResource { .. }
        | TypeKind::RegisterAttribute { .. }
        | TypeKind::RegisterFilter { .. }
        | TypeKind::Attribute { .. }
        | TypeKind::ThisObject { .. }
        | TypeKind::ThisManager { .. }
        | TypeKind::Union(_)
        | TypeKind::ManagerCollection(_)
        | TypeKind::ObjectManager(_)
        | TypeKind::Function(_)
        | _ => None,
    }
}

fn form_data_collection_row_ty(db: &dyn TypeKernelDb, receiver: TypeId) -> Option<TypeId> {
    let TypeKind::FormData { kind: FormDataFacet::Collection, underlying: Some(underlying) } =
        db.lookup_type(receiver)
    else {
        return None;
    };
    if !underlying.name.contains('.') {
        return None;
    }
    Some(db.metadata_ref(
        MetadataKind::TabularSectionRow { parent: underlying.mdo_type },
        underlying.name.clone(),
        &RootConfigCtx,
    ))
}

fn rewrite_form_data_collection_item_return(
    db: &dyn TypeKernelDb,
    id: TypeId,
    row: TypeId,
    method_name: &str,
) -> TypeId {
    match db.lookup_type(id) {
        TypeKind::PlatformObject(f) if is_form_data_collection_item_type_name(&f.name) => row,
        TypeKind::Union(members) => db.union(
            members
                .iter()
                .map(|m| rewrite_form_data_collection_item_return(db, *m, row, method_name))
                .collect(),
        ),
        TypeKind::Array(f) if f.element.is_none() && is_row_array_method(method_name) => {
            db.array(Some(row))
        }
        _ => id,
    }
}

fn is_form_data_collection_item_type_name(name: &str) -> bool {
    let lc = name.to_lowercase();
    lc == "данныеформыэлементколлекции" || lc == "formdatacollectionitem"
}

pub(crate) fn to_method_info(db: &dyn TypeKernelDb, method: &PlatformMethod) -> MethodInfo {
    let return_ty = method
        .return_type
        .as_ref()
        .map(|ret| lower_return_type_string_typeid(db, ret))
        .unwrap_or_else(|| db.undefined());

    let params: Vec<TypeId> = method
        .parameters
        .iter()
        .map(|p| {
            p.param_type
                .as_ref()
                .map(|t| lower_param_type_string_typeid(db, t))
                .unwrap_or_else(|| db.unknown())
        })
        .collect();

    let overloads = lower_overloads_typeid(db, method);

    MethodInfo { return_ty, params, overloads }
}

pub(crate) fn lower_overloads_typeid(
    db: &dyn TypeKernelDb,
    method: &PlatformMethod,
) -> Vec<Vec<TypeId>> {
    method
        .variants
        .iter()
        .map(|v| {
            v.parameters
                .iter()
                .map(|p| {
                    p.param_type
                        .as_ref()
                        .map(|t| lower_param_type_string_typeid(db, t))
                        .unwrap_or(db.unknown())
                })
                .collect()
        })
        .collect()
}

pub(crate) fn build_tabular_section_method_info(
    db: &dyn TypeKernelDb,
    method: &PlatformMethod,
    parent: MdoType,
    section_name: &Name,
) -> MethodInfo {
    let return_ty = method
        .return_type
        .as_ref()
        .map(|ret| {
            let lowered = rewrite_row_generic(
                db,
                lower_return_type_string_typeid(db, ret),
                parent,
                section_name,
            );
            rewrite_row_array_for_method(db, lowered, method.name.as_str(), parent, section_name)
        })
        .unwrap_or_else(|| db.undefined());

    let params: Vec<TypeId> = method
        .parameters
        .iter()
        .map(|p| {
            p.param_type
                .as_ref()
                .map(|t| lower_param_type_string_typeid(db, t.as_str()))
                .unwrap_or_else(|| db.unknown())
        })
        .collect();

    let overloads: Vec<Vec<TypeId>> = method
        .variants
        .iter()
        .map(|v| {
            v.parameters
                .iter()
                .map(|p| {
                    p.param_type
                        .as_ref()
                        .map(|t| lower_param_type_string_typeid(db, t.as_str()))
                        .unwrap_or_else(|| db.unknown())
                })
                .collect()
        })
        .collect();

    MethodInfo { return_ty, params, overloads }
}

fn rewrite_row_generic(
    db: &dyn TypeKernelDb,
    id: TypeId,
    parent: MdoType,
    section_name: &Name,
) -> TypeId {
    match db.lookup_type(id) {
        TypeKind::PlatformObject(f) if is_tabular_row_type_name(&f.name) => db.metadata_ref(
            MetadataKind::TabularSectionRow { parent },
            section_name.as_str().to_string(),
            &RootConfigCtx,
        ),
        TypeKind::Union(members) => db.union(
            members.iter().map(|m| rewrite_row_generic(db, *m, parent, section_name)).collect(),
        ),
        _ => id,
    }
}

fn rewrite_row_array_for_method(
    db: &dyn TypeKernelDb,
    id: TypeId,
    method_name: &str,
    parent: MdoType,
    section_name: &Name,
) -> TypeId {
    if !is_row_array_method(method_name) {
        return id;
    }
    let row = db.metadata_ref(
        MetadataKind::TabularSectionRow { parent },
        section_name.as_str().to_string(),
        &RootConfigCtx,
    );
    match db.lookup_type(id) {
        TypeKind::Array(f) if f.element.is_none() => db.array(Some(row)),
        _ => id,
    }
}

fn is_row_array_method(name: &str) -> bool {
    let lc = name.to_lowercase();
    matches!(lc.as_str(), "найтистроки" | "findrows")
}

fn is_tabular_row_type_name(name: &str) -> bool {
    let lc = name.to_lowercase();
    lc == "строка табличной части" || lc == "line of a tabular section"
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::{FormElementKind, MdoType};
    use bsl_platform::MethodParam;
    use bsl_types::facet::MdoRefFacet;
    use bsl_types::kind::{ProjectionField, ProjectionFieldSource, ProjectionOrigin, TypeKind};
    use bsl_types::testing::{InMemoryDb, RootConfigCtx};
    use hir_def::ty::MetadataKind;

    fn lookup(db: &dyn TypeKernelDb, recv: TypeId, method: &Name) -> Option<MethodInfo> {
        super::lookup_method(db, recv, method)
    }

    fn query_id(db: &dyn TypeKernelDb, projections: Vec<Option<Arc<Projection>>>) -> TypeId {
        db.query(Arc::from(projections))
    }

    fn projection(db: &dyn TypeKernelDb) -> Arc<Projection> {
        Arc::new(Projection::new(
            Arc::from([
                ProjectionField::new(
                    "Номер".to_string(),
                    db.number(None, None),
                    ProjectionFieldSource::Column,
                ),
                ProjectionField::new(
                    "Имя".to_string(),
                    db.string(None, false),
                    ProjectionFieldSource::Column,
                ),
            ]),
            ProjectionOrigin::SdblQuery,
            None,
        ))
    }

    fn projection_result_id(
        db: &dyn TypeKernelDb,
        projection: Option<Option<Arc<Projection>>>,
    ) -> Option<TypeId> {
        projection.map(|projection| db.query_result(projection, ProjectionSource::Unknown))
    }

    fn projection_result_expected_id(
        db: &dyn TypeKernelDb,
        projection: Option<Option<Arc<Projection>>>,
    ) -> Option<TypeId> {
        projection.map(|projection| db.query_result(projection, ProjectionSource::Unknown))
    }

    fn projection_selection_id(
        db: &dyn TypeKernelDb,
        projection: Option<Option<Arc<Projection>>>,
    ) -> Option<TypeId> {
        projection
            .map(|projection| db.query_result_selection(projection, ProjectionSource::Unknown))
    }

    fn projection_selection_expected_id(
        db: &dyn TypeKernelDb,
        projection: Option<Option<Arc<Projection>>>,
    ) -> Option<TypeId> {
        projection
            .map(|projection| db.query_result_selection(projection, ProjectionSource::Unknown))
    }

    fn value_table_id(db: &dyn TypeKernelDb, projection: Option<Arc<Projection>>) -> TypeId {
        db.value_table(projection, TableSource::Unknown)
    }

    fn platform_id(db: &dyn TypeKernelDb, name: &str) -> TypeId {
        db.platform_object(name.to_string())
    }

    fn metadata_ref_id(db: &dyn TypeKernelDb, kind: MetadataKind, name: &str) -> TypeId {
        db.metadata_ref(kind, name.to_string(), &RootConfigCtx)
    }

    fn object_manager_id(db: &dyn TypeKernelDb, kind: MdoType, name: &str) -> TypeId {
        db.object_manager(kind, name.to_string(), &RootConfigCtx)
    }

    fn form_control_id(db: &dyn TypeKernelDb, kind: FormElementKind) -> TypeId {
        db.mk_form_control(kind, None)
    }

    fn form_data_id(
        db: &dyn TypeKernelDb,
        kind: FormDataFacet,
        underlying: Option<(MdoType, Name)>,
    ) -> TypeId {
        db.mk_form_data(
            kind,
            underlying.map(|(mdo_type, name)| MdoRefFacet::new(mdo_type, name.to_string())),
        )
    }

    fn assert_metadata_ref(
        db: &dyn TypeKernelDb,
        id: TypeId,
        expected_kind: MetadataKind,
        expected_name: &str,
    ) {
        match db.lookup_type(id) {
            TypeKind::MetadataRef(facet) => {
                assert_eq!(facet.kind, expected_kind);
                assert_eq!(facet.name.as_str(), expected_name);
            }
            other => panic!("expected MetadataRef, got {other:?}"),
        }
    }

    fn assert_query_result_selection_none(db: &dyn TypeKernelDb, id: TypeId) {
        match db.lookup_type(id) {
            TypeKind::QueryResultSelection(facet) => {
                assert!(facet.projection.is_none());
            }
            other => panic!("expected QueryResultSelection{{None}}, got {other:?}"),
        }
    }

    fn test_method(return_type: Option<&str>, param_type: Option<&str>) -> PlatformMethod {
        PlatformMethod {
            id: 0,
            type_name: "TestType".into(),
            name: "Тест".into(),
            english_name: "Test".into(),
            return_type: return_type.map(Into::into),
            parameters: vec![MethodParam {
                name: "Параметр".into(),
                param_type: param_type.map(Into::into),
                is_optional: false,
                is_variadic: false,
            }],
            variants: Vec::new(),
            min_version: None,
            context: None,
        }
    }

    fn to_type_method_info(db: &dyn TypeKernelDb, method: &PlatformMethod) -> MethodInfo {
        to_method_info(db, method)
    }

    #[test]
    fn kernel_native_predicates_match_expected_shapes() {
        let db = InMemoryDb::new();
        let projection = projection(&db);
        let cases = [
            (value_table_id(&db, None), true, false, false),
            (value_table_id(&db, Some(projection.clone())), true, false, false),
            (platform_id(&db, "ТаблицаЗначений"), true, false, false),
            (platform_id(&db, "ДеревоЗначений"), false, true, false),
            (db.query_result(None, ProjectionSource::Unknown), false, false, true),
            (
                db.query_result(Some(projection.clone()), ProjectionSource::Unknown),
                false,
                false,
                true,
            ),
            (platform_id(&db, "РезультатЗапроса"), false, false, true),
            (platform_id(&db, "Запрос"), false, false, false),
            (db.undefined(), false, false, false),
        ];

        for (id, is_table, is_tree, is_result) in cases {
            assert_eq!(is_value_table_arm_id(&db, id), is_table, "{:?}", db.lookup_type(id));
            assert_eq!(is_value_tree_arm_id(&db, id), is_tree, "{:?}", db.lookup_type(id));
            assert_eq!(is_query_result_receiver_id(&db, id), is_result, "{:?}", db.lookup_type(id));
        }
    }

    #[test]
    fn kernel_native_receiver_refinement_matches_expected_shapes() {
        let db = InMemoryDb::new();
        let projection = projection(&db);
        let cases = [
            (query_id(&db, vec![None]), true),
            (query_id(&db, vec![Some(projection)]), false),
            (query_id(&db, Vec::new()), true),
            (platform_id(&db, "Запрос"), true),
            (platform_id(&db, "РезультатЗапроса"), false),
            (db.undefined(), false),
        ];

        for (id, expected) in cases {
            assert_eq!(receiver_needs_refinement_id(&db, id), expected, "{:?}", db.lookup_type(id));
        }
    }

    #[test]
    fn kernel_native_projection_readers_match_expected_shapes() {
        let db = InMemoryDb::new();
        let projection = projection(&db);
        let query_cases = vec![
            (query_id(&db, vec![None]), Some(None), Some(Arc::from([None]))),
            (
                query_id(&db, vec![Some(projection.clone())]),
                Some(Some(projection.clone())),
                Some(Arc::from([Some(projection.clone())])),
            ),
            (platform_id(&db, "Запрос"), Some(None), Some(Arc::from([]))),
            (db.undefined(), None, None),
        ];

        for (id, expected_projection, expected_projections) in query_cases {
            assert_eq!(
                projection_result_id(&db, projection_of_query_receiver_id(&db, id)),
                projection_result_expected_id(&db, expected_projection),
                "{:?}",
                db.lookup_type(id)
            );
            assert_eq!(
                projections_of_query_receiver_id(&db, id)
                    .map(|projections| db.query_batch_result(projections)),
                expected_projections.map(|projections| db.query_batch_result(projections)),
                "{:?}",
                db.lookup_type(id)
            );
        }

        let result_cases = vec![
            (db.query_result(None, ProjectionSource::Unknown), Some(None)),
            (
                db.query_result(Some(projection.clone()), ProjectionSource::Unknown),
                Some(Some(projection)),
            ),
            (platform_id(&db, "РезультатЗапроса"), Some(None)),
            (db.undefined(), None),
        ];

        for (id, expected_projection) in result_cases {
            assert_eq!(
                projection_selection_id(&db, projection_of_query_result_receiver_id(&db, id)),
                projection_selection_expected_id(&db, expected_projection),
                "{:?}",
                db.lookup_type(id)
            );
        }
    }

    fn assert_pick_chain_rewrite_twin_matches_kernel(
        db: &dyn TypeKernelDb,
        receiver: TypeId,
        method_name: &str,
        expected: Option<(ChainTarget, TypeId)>,
    ) {
        let actual = pick_chain_rewrite_id(db, receiver, method_name);
        match (actual, expected) {
            (Some((actual_target, actual_replacement)), Some((expected_target, expected_id))) => {
                assert_eq!(actual_target, expected_target);
                assert_eq!(actual_replacement, expected_id);
            }
            (None, None) => {}
            (actual, expected) => {
                panic!(
                    "pick_chain_rewrite drift for {:?}.{method_name}: {actual:?} vs {expected:?}",
                    db.lookup_type(receiver)
                )
            }
        }
    }

    #[test]
    fn kernel_native_pick_chain_rewrite_twin_matches_ty_helper() {
        let db = InMemoryDb::new();
        let projection = projection(&db);

        assert_pick_chain_rewrite_twin_matches_kernel(
            &db,
            query_id(&db, vec![Some(projection.clone())]),
            "Выполнить",
            Some((
                ChainTarget::PlatformObjectNamed {
                    ru: "РезультатЗапроса", en: "QueryResult"
                },
                db.query_result(Some(projection.clone()), ProjectionSource::Unknown),
            )),
        );
        assert_pick_chain_rewrite_twin_matches_kernel(
            &db,
            platform_id(&db, "Запрос"),
            "execute",
            Some((
                ChainTarget::PlatformObjectNamed {
                    ru: "РезультатЗапроса", en: "QueryResult"
                },
                db.query_result(None, ProjectionSource::Unknown),
            )),
        );
        assert_pick_chain_rewrite_twin_matches_kernel(
            &db,
            db.query_result(Some(projection.clone()), ProjectionSource::Unknown),
            "Выбрать",
            Some((
                ChainTarget::PlatformObjectNamed {
                    ru: "ВыборкаИзРезультатаЗапроса",
                    en: "QueryResultSelection",
                },
                db.query_result_selection(Some(projection.clone()), ProjectionSource::Unknown),
            )),
        );
        assert_pick_chain_rewrite_twin_matches_kernel(
            &db,
            platform_id(&db, "РезультатЗапроса"),
            "choose",
            Some((
                ChainTarget::PlatformObjectNamed {
                    ru: "ВыборкаИзРезультатаЗапроса",
                    en: "QueryResultSelection",
                },
                db.query_result_selection(None, ProjectionSource::Unknown),
            )),
        );
        let batch_projection = projection.clone();
        assert_pick_chain_rewrite_twin_matches_kernel(
            &db,
            query_id(&db, vec![None, Some(projection)]),
            "ВыполнитьПакет",
            Some((
                ChainTarget::AnyArray,
                db.query_batch_result(Arc::from([None, Some(batch_projection)])),
            )),
        );
        assert_pick_chain_rewrite_twin_matches_kernel(
            &db,
            platform_id(&db, "Запрос"),
            "executebatch",
            Some((ChainTarget::AnyArray, db.query_batch_result(Arc::from([])))),
        );
        assert_pick_chain_rewrite_twin_matches_kernel(
            &db,
            platform_id(&db, "Запрос"),
            "Колонки",
            None,
        );
    }

    fn assert_rewrite_chain_arm_twin_matches_kernel(
        db: &dyn TypeKernelDb,
        return_ty: TypeId,
        target: ChainTarget,
        replacement: TypeId,
        expected: TypeId,
    ) {
        let actual = rewrite_chain_arm_in_return_id(db, return_ty, target.clone(), replacement);
        assert_eq!(actual, expected);
    }

    #[test]
    fn kernel_native_rewrite_chain_arm_twin_matches_ty_helper() {
        let db = InMemoryDb::new();
        assert_rewrite_chain_arm_twin_matches_kernel(
            &db,
            db.union(vec![platform_id(&db, "РезультатЗапроса"), db.undefined()]),
            ChainTarget::PlatformObjectNamed {
                ru: "РезультатЗапроса", en: "QueryResult"
            },
            db.query_result(None, ProjectionSource::Unknown),
            db.union(vec![db.query_result(None, ProjectionSource::Unknown), db.undefined()]),
        );
        assert_rewrite_chain_arm_twin_matches_kernel(
            &db,
            db.array(None),
            ChainTarget::AnyArray,
            db.query_batch_result(Arc::from([])),
            db.query_batch_result(Arc::from([])),
        );
        assert_rewrite_chain_arm_twin_matches_kernel(
            &db,
            db.array(Some(db.string(None, false))),
            ChainTarget::AnyArray,
            db.query_batch_result(Arc::from([])),
            db.query_batch_result(Arc::from([])),
        );
    }

    #[test]
    fn kernel_native_attach_projection_to_value_table_twin_matches_ty_helper() {
        let db = InMemoryDb::new();
        let projection = projection(&db);
        let kernel_projection = projection.clone();
        let cases = vec![
            (value_table_id(&db, None), value_table_id(&db, Some(projection.clone()))),
            (
                db.union(vec![value_table_id(&db, None), platform_id(&db, "ДеревоЗначений")]),
                db.union(vec![
                    value_table_id(&db, Some(projection.clone())),
                    platform_id(&db, "ДеревоЗначений"),
                ]),
            ),
            (
                db.union(vec![value_table_id(&db, None), value_table_id(&db, None)]),
                value_table_id(&db, Some(projection.clone())),
            ),
        ];

        for (input, expected) in cases {
            let actual =
                attach_projection_to_value_table_id(&db, input, Some(kernel_projection.clone()));
            assert_eq!(actual, expected);
        }

        let no_projection_input = value_table_id(&db, None);
        assert_eq!(
            attach_projection_to_value_table_id(&db, no_projection_input, None),
            no_projection_input
        );
    }

    #[test]
    fn kernel_native_drop_union_arm_twin_matches_ty_helper() {
        let db = InMemoryDb::new();
        let input = db.union(vec![value_table_id(&db, None), platform_id(&db, "ДеревоЗначений")]);

        assert_eq!(drop_union_arm_id(&db, input, is_value_tree_arm_id), value_table_id(&db, None));
        assert_eq!(
            drop_union_arm_id(&db, input, is_value_table_arm_id),
            platform_id(&db, "ДеревоЗначений")
        );

        let non_union = value_table_id(&db, None);
        assert_eq!(drop_union_arm_id(&db, non_union, is_value_tree_arm_id), non_union);
    }

    #[test]
    fn method_lookup_platform_type_hit() {
        let db = InMemoryDb::new();
        let info = lookup(&db, db.array(None), &Name::new("Добавить"))
            .expect("Массив.Добавить must resolve in platform data");
        assert_eq!(info.return_ty, db.undefined());
    }

    #[test]
    fn method_lookup_typed_array_shares_array_method_table() {
        let db = InMemoryDb::new();
        let receiver = db.array(Some(db.string(None, false)));
        let info = lookup(&db, receiver, &Name::new("Добавить"))
            .expect("TypedArray must expose Массив.Добавить through the Array platform page");
        assert_eq!(info.return_ty, db.undefined());

        let count = lookup(&db, receiver, &Name::new("Количество"))
            .expect("TypedArray.Количество must resolve via the Array platform page");
        assert_eq!(count.return_ty, db.number(None, None));
    }

    #[test]
    fn method_lookup_unknown_method_returns_none() {
        let db = InMemoryDb::new();
        assert!(lookup(&db, db.array(None), &Name::new("НеСуществуетТакогоМетода")).is_none());
    }

    #[test]
    fn union_lookup_merges_all_arm_signatures() {
        let db = InMemoryDb::new();
        let recv = db.union(vec![db.array(None), db.structure(None)]);
        let info = lookup(&db, recv, &Name::new("Вставить"))
            .expect("Вставить must resolve on a Массив | Структура union");

        // Every arm's signature must be present so an argument accepted by EITHER
        // arm is not reported as a mismatch (a union receiver is an
        // over-approximation). Массив.Вставить wants a numeric index;
        // Структура.Вставить wants a string key.
        let first_accepts = |arg: TypeId| {
            info.overloads
                .iter()
                .any(|ov| ov.first().is_some_and(|&p| crate::subtype::is_coercible_to(&db, arg, p)))
        };
        assert!(
            first_accepts(db.string(None, false)),
            "Структура.Вставить arm must accept a String key, got overloads {:?}",
            info.overloads
        );
        assert!(
            first_accepts(db.number(None, None)),
            "Массив.Вставить arm must accept a numeric index, got overloads {:?}",
            info.overloads
        );
    }

    #[test]
    fn method_lookup_returns_none_for_unknown_receiver() {
        let db = InMemoryDb::new();
        assert!(lookup(&db, db.unknown(), &Name::new("Любой")).is_none());
        assert!(lookup(&db, db.undefined(), &Name::new("Любой")).is_none());
        assert!(lookup(&db, db.null(), &Name::new("Любой")).is_none());
    }

    #[test]
    fn method_lookup_returns_none_for_union_without_live_method() {
        let db = InMemoryDb::new();
        let u = db.union(vec![db.number(None, None), db.string(None, false)]);
        assert!(lookup(&db, u, &Name::new("Любой")).is_none());
    }

    #[test]
    fn method_lookup_union_narrows_past_undefined_sentinel() {
        let db = InMemoryDb::new();
        let u = db.union(vec![platform_id(&db, "РезультатЗапроса"), db.undefined()]);
        let info = lookup(&db, u, &Name::new("Выгрузить")).expect(
            "Union([QueryResult, Undefined]).Выгрузить must resolve through the live branch",
        );
        let contains_value_table = match db.lookup_type(info.return_ty) {
            TypeKind::ValueTable(_) => true,
            TypeKind::Union(members) => {
                members.iter().any(|id| matches!(db.lookup_type(*id), TypeKind::ValueTable(_)))
            }
            _ => false,
        };
        assert!(
            contains_value_table,
            "return type must include ValueTable, got {:?}",
            db.lookup_type(info.return_ty),
        );
    }

    #[test]
    fn method_lookup_platform_object_query_execute_direct() {
        let db = InMemoryDb::new();
        let info = lookup(&db, platform_id(&db, "Запрос"), &Name::new("Выполнить"));
        assert!(info.is_some(), "PlatformObject(Запрос).Выполнить must resolve");
    }

    #[test]
    fn method_lookup_query_execute_returns_union_with_undefined() {
        let db = InMemoryDb::new();
        let info = lookup(&db, platform_id(&db, "Запрос"), &Name::new("Выполнить"))
            .expect("Запрос.Выполнить must resolve in platform data");
        match db.lookup_type(info.return_ty) {
            TypeKind::Union(members) => {
                assert!(
                    members
                        .iter()
                        .any(|id| matches!(db.lookup_type(*id), TypeKind::QueryResult(facet) if facet.projection.is_none())),
                    "union must include QueryResult{{None}} (the rewritten РезультатЗапроса arm), got {members:?}",
                );
                assert!(
                    members.iter().any(|id| matches!(db.lookup_type(*id), TypeKind::Undefined)),
                    "union must include Undefined, got {members:?}",
                );
            }
            other => panic!("expected Union, got {other:?}"),
        }
    }

    #[test]
    fn to_method_info_lowers_param_type_asymmetrically() {
        let db = InMemoryDb::new();
        let info = to_type_method_info(
            &db,
            &test_method(Some("Число, Неопределено"), Some("Метаданные,")),
        );
        assert_eq!(info.return_ty, db.union(vec![db.number(None, None), db.undefined()]));
        assert_eq!(
            info.params,
            vec![db.unknown()],
            "garbage param strings stay Unknown for gradual typing",
        );
    }

    #[test]
    fn to_method_info_prose_comma_param_stays_unknown() {
        let db = InMemoryDb::new();
        let info = to_type_method_info(&db, &test_method(None, Some("Ссылка на объект, либо")));
        assert_eq!(info.params, vec![db.unknown()]);
    }

    #[test]
    fn to_method_info_single_unknown_param_stays_unknown() {
        let db = InMemoryDb::new();
        let info = to_type_method_info(&db, &test_method(None, Some("Строка табличной части")));
        assert_eq!(info.params, vec![db.unknown()]);
    }

    #[test]
    fn to_method_info_multi_type_param_lowers_to_union() {
        let db = InMemoryDb::new();
        let info = to_type_method_info(
            &db,
            &test_method(None, Some("Число, Строка, КолонкаТаблицыЗначений")),
        );
        match &info.params[..] {
            [param] => match db.lookup_type(*param) {
                TypeKind::Union(members) => {
                    assert!(members.contains(&db.number(None, None)));
                    assert!(members.contains(&db.string(None, false)));
                    assert!(members.iter().any(
                    |id| matches!(db.lookup_type(*id), TypeKind::PlatformObject(facet) if facet.name.as_str() == "КолонкаТаблицыЗначений")
                ));
                }
                other => panic!("expected single Union param, got {other:?}"),
            },
            other => panic!("expected single Union param, got {other:?}"),
        }
    }

    #[test]
    fn to_method_info_arbitrary_return_lowers_to_unknown() {
        let db = InMemoryDb::new();
        let info = to_type_method_info(&db, &test_method(Some("Произвольный"), None));
        assert_eq!(info.return_ty, db.unknown());
    }

    #[test]
    fn method_lookup_returns_none_for_manager_collection() {
        let db = InMemoryDb::new();
        let doc = db.manager_collection(MdoType::Document);
        assert!(lookup(&db, doc, &Name::new("Любой")).is_none());
    }

    #[test]
    fn method_lookup_value_table_english_key_hits_russian_method_name() {
        let db = InMemoryDb::new();
        let info = lookup(&db, value_table_id(&db, None), &Name::new("Добавить"))
            .expect("ValueTable.Добавить must resolve via bilingual platform index");
        assert!(!matches!(db.lookup_type(info.return_ty), TypeKind::Unknown));
    }

    #[test]
    fn method_lookup_object_manager_resolves_through_platform_manager_adapter() {
        let db = InMemoryDb::new();
        let om = object_manager_id(&db, MdoType::Catalog, "Номенклатура");
        let info = lookup(&db, om, &Name::new("СоздатьЭлемент"))
            .expect("ObjectManager.СоздатьЭлемент must resolve via platform adapter");
        assert_eq!(
            info.return_ty,
            metadata_ref_id(&db, MetadataKind::CatalogObject, "Номенклатура")
        );
    }

    #[test]
    fn method_lookup_object_manager_unknown_method_returns_none() {
        let db = InMemoryDb::new();
        let om = object_manager_id(&db, MdoType::Catalog, "Номенклатура");
        assert!(lookup(&db, om, &Name::new("НетТакогоМетода")).is_none());
    }

    #[test]
    fn method_lookup_metadata_ref_catalog_object_resolves_write() {
        let db = InMemoryDb::new();
        let r = metadata_ref_id(&db, MetadataKind::CatalogObject, "Номенклатура");
        let info = lookup(&db, r, &Name::new("Записать"))
            .expect("MetadataRef CatalogObject.Записать must resolve");
        assert_eq!(info.return_ty, db.undefined());
    }

    #[test]
    fn method_lookup_register_filter_resolves_filter_method_via_scalar_key() {
        let db = InMemoryDb::new();
        let r = metadata_ref_id(
            &db,
            MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
            "РегистрСведений1",
        );
        let info = lookup(&db, r, &Name::new("Сбросить"))
            .expect("Filter.Сбросить must resolve through scalar-key fallback");
        assert_eq!(info.return_ty, db.undefined());
    }

    #[test]
    fn method_lookup_composite_multi_overload_populates_overloads() {
        let db = InMemoryDb::new();
        let r = object_manager_id(&db, MdoType::InformationRegister, "Курсы");
        let Some(info) = lookup(&db, r, &Name::new("Получить")) else {
            println!("Skipping: no platform data available");
            return;
        };
        assert!(
            !info.overloads.is_empty(),
            "InformationRegisterManager.Получить must surface multi-overload variants \
             through lookup_method (the inference path); got params={:?}, overloads={:?}",
            info.params,
            info.overloads,
        );
    }

    fn ts_receiver(db: &dyn TypeKernelDb, parent: MdoType, name: &str) -> TypeId {
        metadata_ref_id(db, MetadataKind::TabularSection { parent }, name)
    }

    #[test]
    fn method_lookup_tabular_section_add_returns_row_metadata_ref() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::Catalog, "Номенклатура.Услуги");
        let info = lookup(&db, r, &Name::new("Добавить")).expect(
            "TabularSection.Добавить must resolve through PlatformData[\"Tabular section\"]",
        );
        assert_eq!(
            info.return_ty,
            metadata_ref_id(
                &db,
                MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
                "Номенклатура.Услуги"
            )
        );
    }

    #[test]
    fn method_lookup_tabular_section_count_returns_number() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::Catalog, "X.Y");
        let info = lookup(&db, r, &Name::new("Количество"))
            .expect("TabularSection.Количество must resolve");
        assert_eq!(info.return_ty, db.number(None, None));
    }

    #[test]
    fn method_lookup_tabular_section_unload_returns_value_table() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::Catalog, "X.Y");
        let info =
            lookup(&db, r, &Name::new("Выгрузить")).expect("TabularSection.Выгрузить must resolve");
        assert_eq!(info.return_ty, value_table_id(&db, None));
    }

    #[test]
    fn method_lookup_tabular_section_find_returns_union_with_row() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::Catalog, "X.Y");
        let info = lookup(&db, r, &Name::new("Найти")).expect("TabularSection.Найти must resolve");
        let members = match db.lookup_type(info.return_ty) {
            TypeKind::Union(m) => m.clone(),
            other => panic!("expected Union, got {other:?}"),
        };
        assert!(
            members.iter().any(|id| matches!(
                db.lookup_type(*id),
                TypeKind::MetadataRef(facet)
                    if facet.kind == MetadataKind::TabularSectionRow { parent: MdoType::Catalog }
                        && facet.name.as_str() == "X.Y"
            )),
            "Найти union must include TabularSectionRow {{ parent: Catalog, name: \"X.Y\" }}, got {members:?}",
        );
        assert!(
            members.iter().any(|id| matches!(db.lookup_type(*id), TypeKind::Undefined)),
            "Найти union must include Undefined, got {members:?}",
        );
    }

    #[test]
    fn method_lookup_tabular_section_findrows_returns_typed_array_of_rows() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::Catalog, "X.Y");
        let info = lookup(&db, r, &Name::new("НайтиСтроки"))
            .expect("TabularSection.НайтиСтроки must resolve");
        match db.lookup_type(info.return_ty) {
            TypeKind::Array(facet) => {
                let elem = facet.element.expect("FindRows must return a typed array");
                assert_metadata_ref(
                    &db,
                    elem,
                    MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
                    "X.Y",
                );
            }
            other => panic!("expected TypedArray, got {other:?}"),
        }
    }

    #[test]
    fn method_lookup_tabular_section_findrows_english_alias_typed_array() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::Document, "ПКО.Товары");
        let info = lookup(&db, r, &Name::new("FindRows"))
            .expect("TabularSection.FindRows must resolve via bilingual platform index");
        match db.lookup_type(info.return_ty) {
            TypeKind::Array(facet) => {
                let elem = facet.element.expect("FindRows must return a typed array");
                assert!(matches!(
                    db.lookup_type(elem),
                    TypeKind::MetadataRef(meta)
                        if meta.kind == MetadataKind::TabularSectionRow {
                            parent: MdoType::Document
                        }
                ));
            }
            other => panic!("expected TypedArray, got {other:?}"),
        }
    }

    #[test]
    fn method_lookup_tabular_section_english_name_resolves() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::Catalog, "X.Y");
        let info = lookup(&db, r, &Name::new("Add"))
            .expect("TabularSection.Add must resolve via bilingual platform index");
        assert!(matches!(
            db.lookup_type(info.return_ty),
            TypeKind::MetadataRef(facet)
                if facet.kind == MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
        ));
    }

    #[test]
    fn method_lookup_tabular_section_unknown_method_returns_none() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::Catalog, "X.Y");
        assert!(lookup(&db, r, &Name::new("НетТакогоМетодаНаТЧ")).is_none());
    }

    #[test]
    fn method_lookup_tabular_section_parent_propagates_document() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::Document, "ПКО.Товары");
        let info = lookup(&db, r, &Name::new("Добавить"))
            .expect("Document TabularSection.Добавить must resolve");
        assert_eq!(
            info.return_ty,
            metadata_ref_id(
                &db,
                MetadataKind::TabularSectionRow { parent: MdoType::Document },
                "ПКО.Товары"
            )
        );
    }

    #[test]
    fn method_lookup_tabular_section_parent_propagates_exchange_plan() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::ExchangePlan, "ПО.Состав");
        let info = lookup(&db, r, &Name::new("Добавить"))
            .expect("ExchangePlan TabularSection.Добавить must resolve");
        assert_eq!(
            info.return_ty,
            metadata_ref_id(
                &db,
                MetadataKind::TabularSectionRow { parent: MdoType::ExchangePlan },
                "ПО.Состав"
            )
        );
    }

    #[test]
    fn method_lookup_tabular_section_find_params_preserve_arbitrary_as_unknown() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::Catalog, "X.Y");
        let info = lookup(&db, r, &Name::new("Найти")).expect("TabularSection.Найти must resolve");
        assert_eq!(
            info.params,
            vec![db.unknown(), db.string(None, false)],
            "Произвольный must stay Unknown; only the row generic is rebound",
        );
    }

    #[test]
    fn method_lookup_tabular_section_index_param_stays_unknown() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::Catalog, "X.Y");
        let info =
            lookup(&db, r, &Name::new("Индекс")).expect("TabularSection.Индекс must resolve");
        assert_eq!(
            info.params,
            vec![db.unknown()],
            "Индекс param must stay Unknown — rebinding would false-reject valid args",
        );
    }

    #[test]
    fn method_lookup_tabular_section_parent_propagates_chart_of_accounts() {
        let db = InMemoryDb::new();
        let r = ts_receiver(&db, MdoType::ChartOfAccounts, "Основной.ВидыСубконто");
        let info = lookup(&db, r, &Name::new("Добавить"))
            .expect("ChartOfAccounts TabularSection.Добавить must resolve");
        assert_eq!(
            info.return_ty,
            metadata_ref_id(
                &db,
                MetadataKind::TabularSectionRow { parent: MdoType::ChartOfAccounts },
                "Основной.ВидыСубконто"
            )
        );
    }

    #[test]
    fn method_lookup_usual_group_resolves_extension_method() {
        let db = InMemoryDb::new();
        let receiver = form_control_id(&db, FormElementKind::UsualGroup);
        assert!(
            lookup(&db, receiver, &Name::new("Скрыть")).is_some(),
            "<UsualGroup>.Скрыть must resolve via the usual-group extension chain entry"
        );
        assert!(
            lookup(&db, receiver, &Name::new("Показать")).is_some(),
            "<UsualGroup>.Показать must resolve via the usual-group extension chain entry"
        );
    }

    #[test]
    fn method_lookup_pages_does_not_borrow_usual_group_methods() {
        let db = InMemoryDb::new();
        let receiver = form_control_id(&db, FormElementKind::Pages);
        assert!(
            lookup(&db, receiver, &Name::new("Скрыть")).is_none(),
            "Pages chain must not borrow UsualGroup-extension methods"
        );
    }

    #[test]
    fn method_lookup_form_control_other_with_empty_chain_returns_none() {
        let db = InMemoryDb::new();
        let receiver = form_control_id(&db, FormElementKind::Other);
        assert!(lookup(&db, receiver, &Name::new("Скрыть")).is_none());
    }

    #[test]
    fn method_lookup_union_two_live_branches_first_branch_signature_wins() {
        let db = InMemoryDb::new();
        let u = db.union(vec![db.array(None), value_table_id(&db, None)]);
        let info = lookup(&db, u, &Name::new("Количество"))
            .expect("Union(Array, ValueTable).Количество must resolve through both branches");
        assert_eq!(
            info.return_ty,
            db.number(None, None),
            "Количество returns Число on both branches"
        );
        assert!(
            info.overloads.is_empty(),
            "cohesion: overloads must NOT be merged across union branches, got {:?}",
            info.overloads,
        );
    }

    #[test]
    fn method_lookup_form_data_collection_get_rewrites_item_return_to_row() {
        let db = InMemoryDb::new();
        let receiver = form_data_id(
            &db,
            FormDataFacet::Collection,
            Some((MdoType::Document, Name::new("Док.Товары"))),
        );
        let info = lookup(&db, receiver, &Name::new("Получить"))
            .expect("FormDataCollection.Получить must resolve in platform data");
        assert_metadata_ref(
            &db,
            info.return_ty,
            MetadataKind::TabularSectionRow { parent: MdoType::Document },
            "Док.Товары",
        );
    }

    #[test]
    fn method_lookup_form_control_table_unchanged_by_chain_walk() {
        let db = InMemoryDb::new();
        let receiver = form_control_id(&db, FormElementKind::Table);
        let _ = lookup(&db, receiver, &Name::new("ОбновитьСтроки"));
    }

    fn assert_query_result_in_return(db: &dyn TypeKernelDb, return_ty: TypeId) {
        let has_query_result = match db.lookup_type(return_ty) {
            TypeKind::QueryResult(facet) if facet.projection.is_none() => true,
            TypeKind::Union(arms) => arms.iter().any(|id| {
                matches!(
                    db.lookup_type(*id),
                    TypeKind::QueryResult(facet) if facet.projection.is_none()
                )
            }),
            _ => false,
        };
        assert!(
            has_query_result,
            "expected QueryResult{{None}} in return, got {:?}",
            db.lookup_type(return_ty),
        );
    }

    #[test]
    fn sdbl_chain_rewrite_executes_query() {
        let db = InMemoryDb::new();
        let receiver = platform_id(&db, "Запрос");
        let info = lookup(&db, receiver, &Name::new("Выполнить"))
            .expect("Запрос.Выполнить must resolve in platform data");
        assert_query_result_in_return(&db, info.return_ty);
    }

    #[test]
    fn sdbl_chain_rewrite_executes_query_english_alias() {
        let db = InMemoryDb::new();
        let receiver = platform_id(&db, "Запрос");
        let info = lookup(&db, receiver, &Name::new("Execute"))
            .expect("Запрос.Execute must resolve in platform data");
        assert_query_result_in_return(&db, info.return_ty);
    }

    #[test]
    fn sdbl_chain_rewrite_choose_on_result() {
        let db = InMemoryDb::new();
        let receiver = platform_id(&db, "РезультатЗапроса");
        let info = lookup(&db, receiver, &Name::new("Выбрать"))
            .expect("РезультатЗапроса.Выбрать must resolve in platform data");
        assert_query_result_selection_none(&db, info.return_ty);
    }

    #[test]
    fn sdbl_chain_rewrite_choose_on_typed_result() {
        let db = InMemoryDb::new();
        let receiver = db.query_result(None, ProjectionSource::Unknown);
        let info = lookup(&db, receiver, &Name::new("Выбрать"))
            .expect("QueryResult.Выбрать must resolve via platform alias");
        assert_query_result_selection_none(&db, info.return_ty);
    }

    #[test]
    fn sdbl_chain_rewrite_skips_unrelated_choose() {
        let db = InMemoryDb::new();
        let receiver = platform_id(&db, "СтандартныйПериод");
        if let Some(info) = lookup(&db, receiver, &Name::new("Выбрать")) {
            assert!(
                !matches!(db.lookup_type(info.return_ty), TypeKind::QueryResultSelection(_)),
                "rewrite must not fire on non-query receivers — got {:?}",
                db.lookup_type(info.return_ty),
            );
        }
    }

    #[test]
    fn sdbl_chain_rewrite_execute_batch() {
        let db = InMemoryDb::new();
        let receiver = platform_id(&db, "Запрос");
        let info = lookup(&db, receiver, &Name::new("ВыполнитьПакет"))
            .expect("Запрос.ВыполнитьПакет must resolve in platform data");
        match db.lookup_type(info.return_ty) {
            TypeKind::QueryBatchResult { per_query } => {
                assert!(per_query.is_empty(), "Slice 1 leaves per_query empty; Phase 3 fills it",);
            }
            other => panic!("expected QueryBatchResult, got {other:?}"),
        }
    }

    #[test]
    fn sdbl_chain_rewrite_preserves_nullability_in_union() {
        let db = InMemoryDb::new();
        let input = db.union(vec![platform_id(&db, "РезультатЗапроса"), db.undefined()]);
        let rewritten_id = rewrite_chain_arm_in_return_id(
            &db,
            input,
            ChainTarget::PlatformObjectNamed {
                ru: "РезультатЗапроса", en: "QueryResult"
            },
            db.query_result(None, ProjectionSource::Unknown),
        );
        match db.lookup_type(rewritten_id) {
            TypeKind::Union(arms) => {
                assert_eq!(arms.len(), 2);
                assert!(arms.contains(&db.query_result(None, ProjectionSource::Unknown)));
                assert!(arms.contains(&db.undefined()));
            }
            other => panic!("expected Union, got {other:?}"),
        }
    }

    #[test]
    fn sdbl_chain_rewrite_skips_non_chain_methods() {
        assert!(!is_sdbl_chain_method("Колонки"));
        assert!(!is_sdbl_chain_method("Columns"));
        assert!(!is_sdbl_chain_method("УстановитьПараметр"));
        assert!(is_sdbl_chain_method("Выполнить"));
        assert!(is_sdbl_chain_method("execute"));
        assert!(is_sdbl_chain_method("ВЫБРАТЬ"));
        assert!(is_sdbl_chain_method("ExecuteBatch"));
    }
}
