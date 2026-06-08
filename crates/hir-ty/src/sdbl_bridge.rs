use std::sync::Arc;

use bsl_metadata::MdoType;
use bsl_types::builders::Builders;
use bsl_types::facet::{DateComponent, SdblTypeShadowFacet, TableSource};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{
    Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin, TypeId,
};
use bsl_types::testing::RootConfigCtx;
use hir_def::ty::MetadataKind;
use hir_def::Name;

#[allow(dead_code, reason = "Phase 3 §4.C producer — projection callers migrate in 4.C.2")]
pub fn sdbl_type_to_typeid(db: &dyn TypeKernelDb, t: &sdbl_hir::SdblType) -> TypeId {
    use sdbl_hir::SdblType as S;
    match t {
        S::Boolean => db.boolean(),
        S::String { .. } => db.string(None, false),
        S::Number { .. } => db.number(None, None),
        S::Date | S::DateTime => db.date(DateComponent::DateTime),
        S::Ref(mdo) => mdo_ref_to_typeid(db, mdo),
        S::AnyRef => db.any_ref(),
        S::AnyObjectRef { mdo_type } => db.any_metadata_ref(*mdo_type),
        S::Uuid => db.platform_object("УникальныйИдентификатор".to_string()),
        S::ValueStorage => db.platform_object("ХранилищеЗначения".to_string()),
        S::DefinedType { underlying_type, .. } => underlying_type
            .as_deref()
            .map(|inner| sdbl_type_to_typeid(db, inner))
            .unwrap_or_else(|| db.unknown()),
        S::ValueTable => db.value_table(None, TableSource::Unknown),
        S::Null => db.null(),
        S::Aggregate(inner) => sdbl_type_to_typeid(db, inner),
        S::Composite { types } => {
            db.union(types.iter().map(|t| sdbl_type_to_typeid(db, t)).collect())
        }
        S::TabularSectionRef { parent_mdo_type, parent_mdo_name, ts_name } => db.metadata_ref(
            MetadataKind::TabularSection { parent: *parent_mdo_type },
            format!("{parent_mdo_name}.{ts_name}"),
            &RootConfigCtx,
        ),
        S::Unknown | S::Error => db.unknown(),
    }
}

fn mdo_ref_to_typeid(db: &dyn TypeKernelDb, mdo: &sdbl_hir::MdoRef) -> TypeId {
    match ref_kind_for(mdo.mdo_type) {
        Some(kind) => db.metadata_ref(kind, mdo.name.clone(), &RootConfigCtx),
        None => db.any_metadata_ref(mdo.mdo_type),
    }
}

fn ref_kind_for(mdo: MdoType) -> Option<MetadataKind> {
    Some(match mdo {
        MdoType::Catalog => MetadataKind::CatalogRef,
        MdoType::Document => MetadataKind::DocumentRef,
        MdoType::Enum => MetadataKind::EnumRef,
        MdoType::Task => MetadataKind::TaskRef,
        MdoType::BusinessProcess => MetadataKind::BusinessProcessRef,
        MdoType::ExchangePlan => MetadataKind::ExchangePlanRef,
        MdoType::ChartOfAccounts => MetadataKind::ChartOfAccountsRef,
        MdoType::InformationRegister => MetadataKind::InformationRegisterRef,
        MdoType::AccumulationRegister => MetadataKind::AccumulationRegisterRef,
        MdoType::AccountingRegister => MetadataKind::AccountingRegisterRef,
        MdoType::CalculationRegister => MetadataKind::CalculationRegisterRef,
        _ => return None,
    })
}

pub fn query_to_projection(
    db: &dyn TypeKernelDb,
    q: &sdbl_hir::SdblQuery,
) -> Option<Arc<Projection>> {
    let initial_cap = q.hir.select.fields.len();
    let mut named_fields: Vec<ProjectionField> = Vec::with_capacity(initial_cap);
    let mut shadows: Vec<SdblTypeShadowFacet> = Vec::with_capacity(initial_cap);
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let push_unique = |name: Name,
                       alt_keys: &[&str],
                       ty: TypeId,
                       shadow: SdblTypeShadowFacet,
                       named_fields: &mut Vec<ProjectionField>,
                       shadows: &mut Vec<SdblTypeShadowFacet>,
                       seen: &mut std::collections::HashSet<String>|
     -> bool {
        let primary_key = name.as_str().to_lowercase();
        if !seen.contains(&primary_key)
            && !alt_keys.iter().any(|k| seen.contains(&k.to_lowercase()))
        {
            seen.insert(primary_key);
            for k in alt_keys {
                seen.insert(k.to_lowercase());
            }
            named_fields.push(ProjectionField::new(
                name.as_str().to_string(),
                ty,
                ProjectionFieldSource::Column,
            ));
            shadows.push(shadow);
            true
        } else {
            false
        }
    };

    for field in &q.hir.select.fields {
        if field.has_parse_error {
            continue;
        }
        if field.is_asterisk {
            for (name, alt_en, ty, shadow) in
                expand_asterisk(db, field.asterisk_qualifier.as_deref(), &q.hir)
            {
                let alt_keys: Vec<&str> = alt_en.as_deref().into_iter().collect();
                push_unique(
                    name,
                    &alt_keys,
                    ty,
                    shadow,
                    &mut named_fields,
                    &mut shadows,
                    &mut seen,
                );
            }
            continue;
        }
        let Some(name) = field.alias_or_name() else {
            continue;
        };
        let bridged = sdbl_type_to_typeid(db, &field.ty);
        let shadow = SdblTypeShadowFacet::new(field.ty.to_string());
        push_unique(
            Name::new(name.as_str()),
            &[],
            bridged,
            shadow,
            &mut named_fields,
            &mut shadows,
            &mut seen,
        );
    }

    if named_fields.is_empty() {
        return None;
    }

    debug_assert_eq!(
        named_fields.len(),
        shadows.len(),
        "Projection invariant: raw_sdbl_types.len() must equal fields.len()",
    );

    Some(Arc::new(Projection::new(
        named_fields.into(),
        ProjectionOrigin::SdblQuery,
        Some(shadows.into()),
    )))
}

fn expand_asterisk(
    db: &dyn TypeKernelDb,
    qualifier: Option<&str>,
    hir: &sdbl_hir::SdblHir,
) -> Vec<(Name, Option<String>, TypeId, SdblTypeShadowFacet)> {
    let qualifier_lower = qualifier.map(|q| q.to_lowercase());
    let mut out = Vec::new();
    for table in hir.all_tables() {
        if let Some(q_lower) = qualifier_lower.as_deref() {
            let effective = table.effective_name().to_lowercase();
            let full = table.full_name.to_lowercase();
            if effective != q_lower && full != q_lower {
                continue;
            }
        }
        let Some(resolved) = &table.metadata else {
            continue;
        };
        for field_def in resolved.fields() {
            out.push((
                Name::new(&field_def.name),
                field_def.name_en.clone(),
                sdbl_type_to_typeid(db, &field_def.ty),
                SdblTypeShadowFacet::new(field_def.ty.to_string()),
            ));
        }
    }
    out
}

pub fn package_to_projections(
    db: &dyn TypeKernelDb,
    pkg: &sdbl_hir::SdblPackage,
) -> Vec<Option<Arc<Projection>>> {
    pkg.queries().iter().map(|q| query_to_projection(db, q)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::testing::InMemoryDb;
    use sdbl_hir::SdblType;

    fn boxed_number() -> Box<SdblType> {
        Box::new(SdblType::Number { precision: Some(15), scale: Some(2) })
    }

    #[test]
    fn sdbl_typeid_covers_all_variants() {
        let db = InMemoryDb::new();
        let cases: Vec<(SdblType, TypeId)> = vec![
            (SdblType::Boolean, db.boolean()),
            (SdblType::string(), db.string(None, false)),
            (SdblType::string_with_length(50), db.string(None, false)),
            (SdblType::Number { precision: Some(15), scale: Some(2) }, db.number(None, None)),
            (SdblType::Date, db.date(DateComponent::DateTime)),
            (SdblType::DateTime, db.date(DateComponent::DateTime)),
            (SdblType::Null, db.null()),
            (SdblType::ValueTable, db.value_table(None, TableSource::Unknown)),
            (SdblType::Uuid, db.platform_object("УникальныйИдентификатор".to_string())),
            (SdblType::ValueStorage, db.platform_object("ХранилищеЗначения".to_string())),
            (SdblType::AnyRef, db.any_ref()),
            (SdblType::Unknown, db.unknown()),
            (SdblType::Error, db.unknown()),
            (
                SdblType::AnyObjectRef { mdo_type: MdoType::Catalog },
                db.any_metadata_ref(MdoType::Catalog),
            ),
            (
                SdblType::Ref(sdbl_hir::MdoRef::new(MdoType::Catalog, "Товары")),
                db.metadata_ref(MetadataKind::CatalogRef, "Товары".to_string(), &RootConfigCtx),
            ),
            (
                SdblType::Ref(sdbl_hir::MdoRef::new(MdoType::CommonModule, "Х")),
                db.any_metadata_ref(MdoType::CommonModule),
            ),
            (
                SdblType::DefinedType {
                    name: "Деньги".to_string(),
                    underlying_type: Some(boxed_number()),
                },
                db.number(None, None),
            ),
            (SdblType::Aggregate(boxed_number()), db.number(None, None)),
            (
                SdblType::Composite {
                    types: vec![
                        SdblType::Number { precision: None, scale: None },
                        SdblType::Unknown,
                    ],
                },
                db.number(None, None),
            ),
            (
                SdblType::TabularSectionRef {
                    parent_mdo_type: MdoType::Catalog,
                    parent_mdo_name: "Номенклатура".to_string(),
                    ts_name: "Товары".to_string(),
                },
                db.metadata_ref(
                    MetadataKind::TabularSection { parent: MdoType::Catalog },
                    "Номенклатура.Товары".to_string(),
                    &RootConfigCtx,
                ),
            ),
        ];
        for (t, expected) in &cases {
            assert_eq!(sdbl_type_to_typeid(&db, t), *expected, "mapping for {t:?}");
        }
    }

    #[test]
    fn primitives_bridge_to_structural_ty() {
        let db = InMemoryDb::new();
        assert_eq!(sdbl_type_to_typeid(&db, &SdblType::Boolean), db.boolean());
        assert_eq!(sdbl_type_to_typeid(&db, &SdblType::string()), db.string(None, false));
        assert_eq!(
            sdbl_type_to_typeid(&db, &SdblType::string_with_length(50)),
            db.string(None, false)
        );
        assert_eq!(
            sdbl_type_to_typeid(&db, &SdblType::Number { precision: Some(15), scale: Some(2) }),
            db.number(None, None),
        );
        assert_eq!(sdbl_type_to_typeid(&db, &SdblType::Date), db.date(DateComponent::DateTime));
        assert_eq!(sdbl_type_to_typeid(&db, &SdblType::DateTime), db.date(DateComponent::DateTime));
        assert_eq!(sdbl_type_to_typeid(&db, &SdblType::Null), db.null());
        assert_eq!(
            sdbl_type_to_typeid(&db, &SdblType::ValueTable),
            db.value_table(None, TableSource::Unknown)
        );
        assert_eq!(sdbl_type_to_typeid(&db, &SdblType::AnyRef), db.any_ref());
        assert_eq!(sdbl_type_to_typeid(&db, &SdblType::Unknown), db.unknown());
        assert_eq!(sdbl_type_to_typeid(&db, &SdblType::Error), db.unknown());
    }

    #[test]
    fn uuid_and_value_storage_lower_to_platform_objects() {
        let db = InMemoryDb::new();
        assert_eq!(
            sdbl_type_to_typeid(&db, &SdblType::Uuid),
            db.platform_object("УникальныйИдентификатор".to_string()),
        );
        assert_eq!(
            sdbl_type_to_typeid(&db, &SdblType::ValueStorage),
            db.platform_object("ХранилищеЗначения".to_string()),
        );
    }

    #[test]
    fn ref_bridges_to_matching_metadata_ref_kind() {
        let db = InMemoryDb::new();
        let r = SdblType::Ref(sdbl_hir::MdoRef::new(MdoType::Catalog, "Товары"));
        assert_eq!(
            sdbl_type_to_typeid(&db, &r),
            db.metadata_ref(MetadataKind::CatalogRef, "Товары".to_string(), &RootConfigCtx),
        );

        let r = SdblType::Ref(sdbl_hir::MdoRef::new(MdoType::Document, "ПКО"));
        assert_eq!(
            sdbl_type_to_typeid(&db, &r),
            db.metadata_ref(MetadataKind::DocumentRef, "ПКО".to_string(), &RootConfigCtx),
        );
    }

    #[test]
    fn any_object_ref_lowers_to_dedicated_variant() {
        let db = InMemoryDb::new();
        let t = SdblType::AnyObjectRef { mdo_type: MdoType::Catalog };
        assert_eq!(sdbl_type_to_typeid(&db, &t), db.any_metadata_ref(MdoType::Catalog));
    }

    #[test]
    fn defined_type_with_underlying_recurses() {
        let db = InMemoryDb::new();
        let t = SdblType::DefinedType {
            name: "Деньги".to_string(),
            underlying_type: Some(boxed_number()),
        };
        assert_eq!(sdbl_type_to_typeid(&db, &t), db.number(None, None));
    }

    #[test]
    fn defined_type_without_underlying_falls_to_unknown() {
        let db = InMemoryDb::new();
        let t = SdblType::DefinedType { name: "Деньги".to_string(), underlying_type: None };
        assert_eq!(sdbl_type_to_typeid(&db, &t), db.unknown());
    }

    #[test]
    fn aggregate_strips_wrapper() {
        let db = InMemoryDb::new();
        let t = SdblType::Aggregate(boxed_number());
        assert_eq!(sdbl_type_to_typeid(&db, &t), db.number(None, None));
    }

    #[test]
    fn composite_lowers_via_union() {
        let db = InMemoryDb::new();
        let t = SdblType::Composite {
            types: vec![
                SdblType::Boolean,
                SdblType::string(),
                SdblType::Number { precision: None, scale: None },
            ],
        };
        assert_eq!(
            sdbl_type_to_typeid(&db, &t),
            db.union(vec![db.boolean(), db.string(None, false), db.number(None, None)]),
        );
    }

    #[test]
    fn tabular_section_ref_carries_parent_pair() {
        let db = InMemoryDb::new();
        let t = SdblType::TabularSectionRef {
            parent_mdo_type: MdoType::Document,
            parent_mdo_name: "ПКО".to_string(),
            ts_name: "Товары".to_string(),
        };
        assert_eq!(
            sdbl_type_to_typeid(&db, &t),
            db.metadata_ref(
                MetadataKind::TabularSection { parent: MdoType::Document },
                "ПКО.Товары".to_string(),
                &RootConfigCtx,
            ),
        );
    }

    #[test]
    fn ref_kind_for_returns_none_for_managerless_mdo() {
        assert_eq!(ref_kind_for(MdoType::CommonModule), None);
    }

    #[test]
    fn ref_without_matching_metadata_kind_falls_to_any_metadata_ref() {
        let db = InMemoryDb::new();
        let r = SdblType::Ref(sdbl_hir::MdoRef::new(
            MdoType::ChartOfCharacteristicTypes,
            "ВидыНоменклатуры",
        ));
        assert_eq!(
            sdbl_type_to_typeid(&db, &r),
            db.any_metadata_ref(MdoType::ChartOfCharacteristicTypes),
        );
    }

    use sdbl_hir::{
        ExprHir, FieldDef, FieldHir, ResolvedTable, SdblHir, SdblQuery, SelectHir, TableRef,
    };
    use syntax::MODULE_RANGE;

    fn mk_asterisk(qualifier: Option<&str>) -> FieldHir {
        FieldHir {
            expr: ExprHir::Missing { range: MODULE_RANGE },
            alias: None,
            has_as_keyword: false,
            has_parse_error: false,
            raw_name: None,
            ty: SdblType::Unknown,
            is_asterisk: true,
            asterisk_qualifier: qualifier.map(str::to_string),
            diagnostic_range: MODULE_RANGE,
            range: MODULE_RANGE,
        }
    }

    fn mk_named(name: &str, ty: SdblType) -> FieldHir {
        FieldHir {
            expr: ExprHir::ColumnRef {
                parts: vec![sdbl_hir::Name::from(name)],
                ty: ty.clone(),
                range: MODULE_RANGE,
            },
            alias: None,
            has_as_keyword: false,
            has_parse_error: false,
            raw_name: Some(sdbl_hir::Name::from(name)),
            ty,
            is_asterisk: false,
            asterisk_qualifier: None,
            diagnostic_range: MODULE_RANGE,
            range: MODULE_RANGE,
        }
    }

    fn mk_metadata_table(full_name: &str, alias: Option<&str>, fields: Vec<FieldDef>) -> TableRef {
        TableRef {
            parts: full_name.split('.').map(sdbl_hir::Name::from).collect(),
            full_name: full_name.to_string(),
            alias: alias.map(sdbl_hir::Name::from),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Catalog,
                name: full_name.to_string(),
                fields,
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            subquery: Vec::new(),
            range: MODULE_RANGE,
        }
    }

    fn mk_register_table(
        full_name: &str,
        fields: Vec<FieldDef>,
        dimensions: Vec<FieldDef>,
        resources: Vec<FieldDef>,
        attributes: Vec<FieldDef>,
    ) -> TableRef {
        TableRef {
            parts: full_name.split('.').map(sdbl_hir::Name::from).collect(),
            full_name: full_name.to_string(),
            alias: None,
            metadata: Some(ResolvedTable::Register {
                mdo_type: MdoType::AccumulationRegister,
                name: full_name.to_string(),
                fields,
                dimensions,
                resources,
                attributes,
                field_model_complete: false,
            }),
            is_virtual_table: true,
            virtual_table_params: Vec::new(),
            subquery: Vec::new(),
            range: MODULE_RANGE,
        }
    }

    fn mk_temp_table(name: &str, alias: Option<&str>, fields: Vec<FieldDef>) -> TableRef {
        TableRef {
            parts: vec![sdbl_hir::Name::from(name)],
            full_name: name.to_string(),
            alias: alias.map(sdbl_hir::Name::from),
            metadata: Some(ResolvedTable::TempTable {
                name: name.to_string(),
                fields,
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            subquery: Vec::new(),
            range: MODULE_RANGE,
        }
    }

    fn mk_query(fields: Vec<FieldHir>, from: Vec<TableRef>) -> SdblQuery {
        let mut hir = SdblHir::empty();
        hir.select = SelectHir { fields, distinct: false, top: None };
        hir.from = from;
        SdblQuery { hir, range: MODULE_RANGE }
    }

    fn projection_field_names(p: &Projection) -> Vec<String> {
        p.fields.iter().map(|f| f.name.clone()).collect()
    }

    #[test]
    fn asterisk_expands_metadata_table_fields() {
        let db = InMemoryDb::new();
        let table = mk_metadata_table(
            "Справочник.Товары",
            None,
            vec![
                FieldDef::new("Ссылка", SdblType::reference(MdoType::Catalog, "Товары")),
                FieldDef::new("Наименование", SdblType::string_with_length(150)),
                FieldDef::new("Цена", SdblType::Number { precision: Some(15), scale: Some(2) }),
            ],
        );
        let q = mk_query(vec![mk_asterisk(None)], vec![table]);
        let p = query_to_projection(&db, &q).expect("asterisk over resolved table must project");
        assert_eq!(projection_field_names(&p), vec!["Ссылка", "Наименование", "Цена"]);
        assert_eq!(p.fields[1].ty, sdbl_type_to_typeid(&db, &SdblType::String { length: None }));
        assert_eq!(
            p.fields[2].ty,
            sdbl_type_to_typeid(&db, &SdblType::Number { precision: Some(15), scale: Some(2) },),
        );
    }

    #[test]
    fn qualified_asterisk_expands_only_matching_table() {
        let db = InMemoryDb::new();
        let table_a = mk_metadata_table(
            "Справочник.A",
            Some("Т"),
            vec![FieldDef::new("Имя", SdblType::string_with_length(50))],
        );
        let table_b = mk_metadata_table(
            "Справочник.B",
            None,
            vec![FieldDef::new("Другое", SdblType::Boolean)],
        );
        let q = mk_query(vec![mk_asterisk(Some("Т"))], vec![table_a, table_b]);
        let p =
            query_to_projection(&db, &q).expect("qualified asterisk must project matching table");
        assert_eq!(projection_field_names(&p), vec!["Имя"]);
    }

    #[test]
    fn qualified_asterisk_matches_full_name_when_no_alias() {
        let db = InMemoryDb::new();
        let table = mk_metadata_table(
            "Справочник.Товары",
            None,
            vec![FieldDef::new("Код", SdblType::string_with_length(11))],
        );
        let q = mk_query(vec![mk_asterisk(Some("справочник.товары"))], vec![table]);
        let p =
            query_to_projection(&db, &q).expect("case-insensitive full_name match must project");
        assert_eq!(projection_field_names(&p), vec!["Код"]);
    }

    #[test]
    fn bare_asterisk_walks_all_tables_in_declaration_order() {
        let db = InMemoryDb::new();
        let table_a =
            mk_metadata_table("Справочник.A", None, vec![FieldDef::new("X", SdblType::Boolean)]);
        let table_b =
            mk_metadata_table("Справочник.B", None, vec![FieldDef::new("Y", SdblType::string())]);
        let q = mk_query(vec![mk_asterisk(None)], vec![table_a, table_b]);
        let p = query_to_projection(&db, &q).expect("bare asterisk with tables must project");
        assert_eq!(projection_field_names(&p), vec!["X", "Y"]);
    }

    #[test]
    fn bare_asterisk_dedupes_duplicate_names_first_wins() {
        let db = InMemoryDb::new();
        let table_a = mk_metadata_table(
            "Справочник.A",
            None,
            vec![FieldDef::new("Ссылка", SdblType::reference(MdoType::Catalog, "A"))],
        );
        let table_b = mk_metadata_table(
            "Справочник.B",
            None,
            vec![FieldDef::new("Ссылка", SdblType::reference(MdoType::Catalog, "B"))],
        );
        let q = mk_query(vec![mk_asterisk(None)], vec![table_a, table_b]);
        let p =
            query_to_projection(&db, &q).expect("bare asterisk must project at least one field");
        assert_eq!(p.fields.len(), 1);
        assert_eq!(p.fields[0].name.as_str(), "Ссылка");
        assert_eq!(
            p.fields[0].ty,
            sdbl_type_to_typeid(&db, &SdblType::reference(MdoType::Catalog, "A")),
        );
    }

    #[test]
    fn mixed_asterisk_and_named_appends_named_after_expansion() {
        let db = InMemoryDb::new();
        let table = mk_metadata_table(
            "Справочник.Товары",
            None,
            vec![
                FieldDef::new("Ссылка", SdblType::reference(MdoType::Catalog, "Товары")),
                FieldDef::new("Наименование", SdblType::string_with_length(150)),
            ],
        );
        let q = mk_query(
            vec![
                mk_asterisk(None),
                mk_named("Ссылка", SdblType::reference(MdoType::Catalog, "Товары")),
                mk_named("Новое", SdblType::Boolean),
            ],
            vec![table],
        );
        let p = query_to_projection(&db, &q).expect("mixed shape must project");
        assert_eq!(projection_field_names(&p), vec!["Ссылка", "Наименование", "Новое"]);
    }

    #[test]
    fn asterisk_against_register_walks_combined_fields() {
        let db = InMemoryDb::new();
        let table = mk_register_table(
            "РегистрНакопления.ОстаткиТоваров.Обороты",
            vec![
                FieldDef::new("Период", SdblType::Date),
                FieldDef::new(
                    "КоличествоОборот",
                    SdblType::Number { precision: None, scale: None },
                ),
                FieldDef::new(
                    "КоличествоПриход",
                    SdblType::Number { precision: None, scale: None },
                ),
                FieldDef::new(
                    "КоличествоРасход",
                    SdblType::Number { precision: None, scale: None },
                ),
            ],
            vec![FieldDef::new("Период", SdblType::Date)],
            vec![FieldDef::new("Количество", SdblType::Number { precision: None, scale: None })],
            Vec::new(),
        );
        let q = mk_query(vec![mk_asterisk(None)], vec![table]);
        let p = query_to_projection(&db, &q).expect("register virtual asterisk must project");
        assert_eq!(
            projection_field_names(&p),
            vec!["Период", "КоличествоОборот", "КоличествоПриход", "КоличествоРасход"],
        );
    }

    #[test]
    fn asterisk_against_temp_table_expands_subquery_fields() {
        let db = InMemoryDb::new();
        let table = mk_temp_table(
            "ВТ_Имена",
            Some("T"),
            vec![
                FieldDef::new("Имя", SdblType::string_with_length(50)),
                FieldDef::new("Активность", SdblType::Boolean),
            ],
        );
        let q = mk_query(vec![mk_asterisk(None)], vec![table]);
        let p = query_to_projection(&db, &q).expect("temp-table asterisk must project");
        assert_eq!(projection_field_names(&p), vec!["Имя", "Активность"]);
    }

    #[test]
    fn qualified_asterisk_with_no_matching_table_yields_none() {
        let db = InMemoryDb::new();
        let table = mk_metadata_table(
            "Справочник.A",
            Some("Т"),
            vec![FieldDef::new("Имя", SdblType::string_with_length(50))],
        );
        let q = mk_query(vec![mk_asterisk(Some("Z"))], vec![table]);
        assert!(
            query_to_projection(&db, &q).is_none(),
            "asterisk with unresolved qualifier must drop silently",
        );
    }

    #[test]
    fn asterisk_against_unresolved_table_yields_none() {
        let db = InMemoryDb::new();
        let table = TableRef {
            parts: Vec::new(),
            full_name: String::new(),
            alias: None,
            metadata: None,
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            subquery: Vec::new(),
            range: MODULE_RANGE,
        };
        let q = mk_query(vec![mk_asterisk(None)], vec![table]);
        assert!(query_to_projection(&db, &q).is_none());
    }

    #[test]
    fn bilingual_dedup_drops_named_field_reprojecting_english_spelling() {
        let db = InMemoryDb::new();
        let table = mk_metadata_table(
            "Справочник.Товары",
            None,
            vec![FieldDef::standard(
                "Ссылка",
                "Ref",
                SdblType::reference(MdoType::Catalog, "Товары"),
            )],
        );
        let q = mk_query(
            vec![
                mk_asterisk(None),
                mk_named("Ref", SdblType::reference(MdoType::Catalog, "Товары")),
            ],
            vec![table],
        );
        let p = query_to_projection(&db, &q).expect("bilingual mixed shape must project");
        assert_eq!(
            projection_field_names(&p),
            vec!["Ссылка"],
            "English spelling of an asterisk-expanded Russian field must dedup first-wins",
        );
    }

    #[test]
    fn asterisk_field_with_parse_error_is_skipped() {
        let db = InMemoryDb::new();
        let table = mk_metadata_table(
            "Справочник.Товары",
            None,
            vec![FieldDef::new("Имя", SdblType::string_with_length(50))],
        );
        let mut bad = mk_asterisk(None);
        bad.has_parse_error = true;
        let q = mk_query(vec![bad], vec![table]);
        assert!(query_to_projection(&db, &q).is_none());
    }

    #[test]
    fn cast_projection_field_carries_precise_shadow_display() {
        let db = InMemoryDb::new();
        let cast_field = FieldHir {
            expr: ExprHir::Missing { range: MODULE_RANGE },
            alias: Some(sdbl_hir::Name::from("Цена")),
            has_as_keyword: true,
            has_parse_error: false,
            raw_name: None,
            ty: SdblType::Number { precision: Some(15), scale: Some(2) },
            is_asterisk: false,
            asterisk_qualifier: None,
            diagnostic_range: MODULE_RANGE,
            range: MODULE_RANGE,
        };
        let q = mk_query(vec![cast_field], Vec::new());
        let p = query_to_projection(&db, &q).expect("CAST field must project");
        assert_eq!(p.fields.len(), 1);
        assert_eq!(p.fields[0].name.as_str(), "Цена");
        assert_eq!(
            p.fields[0].ty,
            sdbl_type_to_typeid(&db, &SdblType::Number { precision: Some(15), scale: Some(2) },),
        );
        let shadows = p.raw_sdbl_types.as_ref().expect("Phase E shadows always populated");
        assert_eq!(shadows.len(), 1);
        assert_eq!(shadows[0].display, "Число(15, 2)");
    }

    #[test]
    fn cast_projection_field_renders_precision_only_number() {
        let db = InMemoryDb::new();
        let cast_field = FieldHir {
            expr: ExprHir::Missing { range: MODULE_RANGE },
            alias: Some(sdbl_hir::Name::from("Сумма")),
            has_as_keyword: true,
            has_parse_error: false,
            raw_name: None,
            ty: SdblType::Number { precision: Some(15), scale: None },
            is_asterisk: false,
            asterisk_qualifier: None,
            diagnostic_range: MODULE_RANGE,
            range: MODULE_RANGE,
        };
        let q = mk_query(vec![cast_field], Vec::new());
        let p = query_to_projection(&db, &q).expect("CAST field must project");
        let shadows = p.raw_sdbl_types.as_ref().expect("shadows populated");
        assert_eq!(shadows[0].display, "Число(15)");
    }

    #[test]
    fn composite_with_aggregate_folds_to_single_arm() {
        let db = InMemoryDb::new();
        let t = SdblType::Composite {
            types: vec![
                SdblType::Number { precision: None, scale: None },
                SdblType::Aggregate(Box::new(SdblType::Number { precision: None, scale: None })),
            ],
        };
        assert_eq!(sdbl_type_to_typeid(&db, &t), db.number(None, None));
    }
}
