#[test]
fn metadata_ref_catalog_object_resolves_write_as_procedure() {
    let db = InMemoryDb::new();
    let res = resolve_platform_metadata_ref_method(
        &db,
        MetadataKind::CatalogObject,
        &Name::new("Номенклатура"),
        &Name::new("Записать"),
    )
    .expect("platform data indexes Write under CatalogObject");
    assert_eq!(res.return_ty, db.undefined());
}

#[test]
fn any_metadata_ref_resolves_common_method_without_name() {
    let db = InMemoryDb::new();
    let res = resolve_platform_any_metadata_ref_method(
        &db,
        MdoType::Catalog,
        &Name::new("Метаданные"),
    );
    assert!(res.is_some(), "Metadata() must resolve on AnyMetadataRef{{Catalog}}");
}

#[test]
fn any_metadata_ref_object_return_degrades_to_unknown() {
    let db = InMemoryDb::new();
    let res = resolve_platform_any_metadata_ref_method(
        &db,
        MdoType::Catalog,
        &Name::new("ПолучитьОбъект"),
    )
    .expect("GetObject must resolve on AnyMetadataRef{Catalog}");
    assert_eq!(res.return_ty, db.unknown(), "object return has no name to bind → Unknown");
}

#[test]
fn any_metadata_ref_unknown_method_is_none() {
    let db = InMemoryDb::new();
    assert!(resolve_platform_any_metadata_ref_method(
        &db,
        MdoType::Catalog,
        &Name::new("НесуществующийМетод"),
    )
    .is_none());
}

#[test]
fn any_metadata_ref_register_flavour_has_no_ref_surface() {
    let db = InMemoryDb::new();
    assert!(resolve_platform_any_metadata_ref_method(
        &db,
        MdoType::InformationRegister,
        &Name::new("Метаданные"),
    )
    .is_none());
}

#[test]
fn metadata_ref_register_record_manager_resolves_write() {
    let db = InMemoryDb::new();
    let res = resolve_platform_metadata_ref_method(
        &db,
        MetadataKind::InformationRegisterRecordManager,
        &Name::new("Курсы"),
        &Name::new("Записать"),
    )
    .expect("platform data indexes Write under InformationRegisterRecordManager");
    assert_eq!(res.return_ty, db.undefined());
}

#[test]
fn metadata_ref_information_register_record_set_resolves_load() {
    let db = InMemoryDb::new();
    let res = resolve_platform_metadata_ref_method(
        &db,
        MetadataKind::InformationRegisterRecordSet,
        &Name::new("Курсы"),
        &Name::new("Загрузить"),
    )
    .expect("platform data indexes Load under InformationRegisterRecordSet");
    assert_eq!(res.return_ty, db.undefined());
}
