use std::fs;
use std::path::PathBuf;

fn extension_file(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extension/src");
    fs::read_to_string(root.join(path)).unwrap()
}

#[test]
fn extension_exposes_live_metadata_endpoints() {
    let service = extension_file("HTTPServices/BSLAnalyzerService.xml");
    let module = extension_file("HTTPServices/BSLAnalyzerService/Ext/Module.bsl");
    for (template, handler) in [
        ("/metadata-list", "СписокМетаданныхPOST"),
        ("/metadata-structure", "СтруктураМетаданныхPOST"),
    ] {
        assert!(service.contains(template), "missing endpoint {template}");
        assert!(service.contains(handler), "missing handler binding {handler}");
        assert!(module.contains(&format!("Функция {handler}(")), "missing handler {handler}");
    }
}

#[test]
fn arbitrary_bsl_requires_separate_role() {
    let basic = extension_file("Roles/BSL_ОсновнаяРоль/Ext/Rights.xml");
    let execute = extension_file("Roles/BSL_ВыполнениеКода/Ext/Rights.xml");
    for method in ["Выполнить", "Вычислить"] {
        assert!(!basic.contains(&format!("URLTemplate.{method}.Method.POST")));
        assert!(execute.contains(&format!("URLTemplate.{method}.Method.POST")));
    }
}
