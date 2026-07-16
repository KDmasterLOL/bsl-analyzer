use super::*;
use serde_json::json;

fn base_data() -> Value {
    json!({
        "methods": [
            {
                "id": 1,
                "type_name": "DOMDocument",
                "name": "ДобавитьДочерний",
                "english_name": "AppendChild",
                "min_version": "8.1",
                "parameters": [{"name": "НовыйУзел", "param_type": "ЭлементDOM"}]
            }
        ]
    })
}

fn valid_overlay() -> String {
    include_str!("../data/platform_overlays.json").to_owned()
}

fn overlay_with_versions(min_version: Option<&str>, max_version: Option<&str>) -> String {
    let mut overlay: Value = serde_json::from_str(&valid_overlay()).unwrap();
    let entry = overlay["method_parameter_overrides"][0].as_object_mut().unwrap();

    match min_version {
        Some(version) => entry.insert("min_version".to_owned(), Value::String(version.to_owned())),
        None => entry.remove("min_version"),
    };
    match max_version {
        Some(version) => entry.insert("max_version".to_owned(), Value::String(version.to_owned())),
        None => entry.remove("max_version"),
    };

    overlay.to_string()
}

fn base_data_with_version(version: Option<&str>) -> Value {
    let mut data = base_data();
    let method = data["methods"][0].as_object_mut().unwrap();

    match version {
        Some(version) => method.insert("min_version".to_owned(), Value::String(version.to_owned())),
        None => method.remove("min_version"),
    };

    data
}

#[test]
fn applies_curated_dom_append_child_override() {
    let mut data = base_data();

    apply_method_parameter_overlays(&mut data, &valid_overlay()).unwrap();

    assert_eq!(
        data["methods"][0]["parameters"][0]["param_type"],
        "АтрибутDOM, ДокументDOM, ЭлементDOM, ОпределениеТипаДокументаDOM, НотацияDOM, АтрибутHTML, ЭлементHTML, ЭлементКнопкаHTML, ЭлементВводаHTML, ЭлементЗаголовокHTML"
    );
}

#[test]
fn rejects_missing_target_deterministically() {
    let mut data = base_data();
    let overlay = valid_overlay().replace("DOMDocument", "MissingDocument");

    let error = apply_method_parameter_overlays(&mut data, &overlay).unwrap_err();

    assert_eq!(
        error.to_string(),
        "overlay target MissingDocument.AppendChild is missing or ambiguous"
    );
}

#[test]
fn rejects_duplicate_alias_override_deterministically() {
    let mut data = base_data();
    let mut overlay: Value = serde_json::from_str(&valid_overlay()).unwrap();
    let duplicate = overlay["method_parameter_overrides"][0].clone();
    overlay["method_parameter_overrides"].as_array_mut().unwrap().push(duplicate);

    let error = apply_method_parameter_overlays(&mut data, &overlay.to_string()).unwrap_err();

    assert_eq!(error.to_string(), "duplicate override for DOMDocument.AppendChild parameter 0");
}

#[test]
fn rejects_aliases_that_resolve_to_different_methods() {
    let mut data = base_data();
    data["methods"].as_array_mut().unwrap().push(json!({
        "id": 2,
        "type_name": "DOMDocument",
        "name": "ДругоеИмя",
        "english_name": "OtherName",
        "min_version": "8.1",
        "parameters": [{"name": "НовыйУзел", "param_type": "ЭлементDOM"}]
    }));
    let overlay = valid_overlay().replace("AppendChild", "OtherName");

    let error = apply_method_parameter_overlays(&mut data, &overlay).unwrap_err();

    assert_eq!(
        error.to_string(),
        "overlay aliases for DOMDocument.OtherName resolve to different methods (0 and 1)"
    );
}

#[test]
fn rejects_out_of_range_parameter_index_deterministically() {
    let mut data = base_data();
    let overlay = valid_overlay().replace("\"parameter_index\": 0", "\"parameter_index\": 1");

    let error = apply_method_parameter_overlays(&mut data, &overlay).unwrap_err();

    assert_eq!(
        error.to_string(),
        "overlay target DOMDocument.AppendChild has no parameter at index 1"
    );
}

#[test]
fn rejects_malformed_schema_deterministically() {
    let mut data = base_data();

    let error =
        apply_method_parameter_overlays(&mut data, "{\"schema_version\": \"1\"}").unwrap_err();

    assert_eq!(error.to_string(), "overlay schema_version must be the number 1");
}

#[test]
fn rejects_max_only_bound_above_target_version() {
    let mut data = base_data();

    let error =
        apply_method_parameter_overlays(&mut data, &overlay_with_versions(None, Some("8.0")))
            .unwrap_err();

    assert_eq!(
        error.to_string(),
        "overlay target DOMDocument.AppendChild version is outside its declared bounds"
    );
}

#[test]
fn accepts_max_only_bound_at_target_version() {
    let mut data = base_data();

    apply_method_parameter_overlays(&mut data, &overlay_with_versions(None, Some("8.1"))).unwrap();
}

#[test]
fn rejects_min_only_bound_below_target_version() {
    let mut data = base_data();

    let error =
        apply_method_parameter_overlays(&mut data, &overlay_with_versions(Some("8.2"), None))
            .unwrap_err();

    assert_eq!(
        error.to_string(),
        "overlay target DOMDocument.AppendChild version is outside its declared bounds"
    );
}

#[test]
fn accepts_min_only_bound_at_target_version() {
    let mut data = base_data();

    apply_method_parameter_overlays(&mut data, &overlay_with_versions(Some("8.1"), None)).unwrap();
}

#[test]
fn rejects_reversed_version_bounds() {
    let mut data = base_data();

    let error = apply_method_parameter_overlays(
        &mut data,
        &overlay_with_versions(Some("8.2"), Some("8.1")),
    )
    .unwrap_err();

    assert_eq!(error.to_string(), "overlay entry 0 has min_version after max_version");
}

#[test]
fn accepts_both_bounds_when_target_is_interior() {
    let mut data = base_data();

    apply_method_parameter_overlays(&mut data, &overlay_with_versions(Some("8.0"), Some("8.2")))
        .unwrap();
}

#[test]
fn accepts_both_bounds_at_inclusive_endpoint() {
    let mut data = base_data();

    apply_method_parameter_overlays(&mut data, &overlay_with_versions(Some("8.1"), Some("8.1")))
        .unwrap();
}

#[test]
fn rejects_missing_target_version_when_bounds_exist() {
    let mut data = base_data_with_version(None);

    let error =
        apply_method_parameter_overlays(&mut data, &overlay_with_versions(None, Some("8.1")))
            .unwrap_err();

    assert_eq!(
        error.to_string(),
        "overlay target DOMDocument.AppendChild has no valid minimum version"
    );
}

fn property_data() -> Value {
    json!({
        "types": [
            {"name": "ФормаКлиентскогоПриложения", "english_name": "ClientApplicationForm"}
        ],
        "properties": [
            {"id": 7, "type_name": "ClientApplicationForm", "name": "Заголовок", "english_name": "Title"}
        ]
    })
}

fn property_addition_overlay(entry: Value) -> String {
    json!({
        "schema_version": 1,
        "method_parameter_overrides": [],
        "type_property_additions": [entry]
    })
    .to_string()
}

fn window_mode_addition() -> Value {
    json!({
        "canonical_type": "ClientApplicationForm",
        "russian_name": "РежимОткрытияОкна",
        "english_name": "WindowOpeningMode",
        "property_types": ["РежимОткрытияОкнаФормы"],
        "is_readonly": false,
        "min_version": "8.2",
        "evidence_source": "v8std #std742",
        "rationale": "help archive files it under the enum type name"
    })
}

#[test]
fn appends_type_property_addition_with_synthetic_id() {
    let mut data = property_data();

    apply_type_property_additions(&mut data, &property_addition_overlay(window_mode_addition()))
        .unwrap();

    let properties = data["properties"].as_array().unwrap();
    assert_eq!(properties.len(), 2);
    let added = &properties[1];
    assert_eq!(added["type_name"], "ClientApplicationForm");
    assert_eq!(added["name"], "РежимОткрытияОкна");
    assert_eq!(added["english_name"], "WindowOpeningMode");
    assert_eq!(added["property_types"][0], "РежимОткрытияОкнаФормы");
    // max existing id (7) + 1
    assert_eq!(added["id"], 8);
}

#[test]
fn absent_additions_section_is_a_no_op() {
    let mut data = property_data();
    let overlay = json!({
        "schema_version": 1,
        "method_parameter_overrides": []
    })
    .to_string();

    apply_type_property_additions(&mut data, &overlay).unwrap();

    assert_eq!(data["properties"].as_array().unwrap().len(), 1);
}

#[test]
fn rejects_addition_for_unknown_type() {
    let mut data = property_data();
    let mut entry = window_mode_addition();
    entry["canonical_type"] = Value::String("NoSuchType".to_owned());

    let error =
        apply_type_property_additions(&mut data, &property_addition_overlay(entry)).unwrap_err();

    assert_eq!(
        error.to_string(),
        "type property addition target NoSuchType is not a known platform type (canonical_type must be the English type name)"
    );
}

#[test]
fn rejects_russian_canonical_type() {
    // The property index keys on the English type name, so a Russian alias would
    // produce a property `get_type_properties` can never resolve.
    let mut data = property_data();
    let mut entry = window_mode_addition();
    entry["canonical_type"] = Value::String("ФормаКлиентскогоПриложения".to_owned());

    let error =
        apply_type_property_additions(&mut data, &property_addition_overlay(entry)).unwrap_err();

    assert_eq!(
        error.to_string(),
        "type property addition target ФормаКлиентскогоПриложения is not a known platform type (canonical_type must be the English type name)"
    );
}

#[test]
fn rejects_addition_that_already_exists() {
    let mut data = property_data();
    let mut entry = window_mode_addition();
    entry["russian_name"] = Value::String("Заголовок".to_owned());
    entry["english_name"] = Value::String("Title".to_owned());

    let error =
        apply_type_property_additions(&mut data, &property_addition_overlay(entry)).unwrap_err();

    assert_eq!(
        error.to_string(),
        "type property addition ClientApplicationForm.Title already exists in the extracted data"
    );
}

#[test]
fn rejects_addition_colliding_with_existing_english_name_via_russian() {
    // property_data() has Заголовок/Title. An addition whose Russian name is
    // "Title" would shadow that property in the shared name index, so it must be
    // rejected even though no RU→RU or EN→EN pair matches directly.
    let mut data = property_data();
    let mut entry = window_mode_addition();
    entry["russian_name"] = Value::String("Title".to_owned());

    let error =
        apply_type_property_additions(&mut data, &property_addition_overlay(entry)).unwrap_err();

    assert_eq!(
        error.to_string(),
        "type property addition ClientApplicationForm.WindowOpeningMode already exists in the extracted data"
    );
}

#[test]
fn rejects_addition_with_empty_property_types() {
    let mut data = property_data();
    let mut entry = window_mode_addition();
    entry["property_types"] = json!([]);

    let error =
        apply_type_property_additions(&mut data, &property_addition_overlay(entry)).unwrap_err();

    assert_eq!(error.to_string(), "type property addition 0 property_types must not be empty");
}

#[test]
fn curated_overlay_adds_window_opening_mode() {
    let mut data = property_data();

    apply_type_property_additions(&mut data, &valid_overlay()).unwrap();

    let added = data["properties"].as_array().unwrap().iter().any(|prop| {
        prop["type_name"] == "ClientApplicationForm" && prop["name"] == "РежимОткрытияОкна"
    });
    assert!(added, "curated overlay must add ClientApplicationForm.РежимОткрытияОкна");
}
