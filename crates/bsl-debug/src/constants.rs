// Fixed property UUIDs for 1C module types.
// These are hardcoded in the 1C platform and never change.

/// CommonModule, WebService, HTTPService main module
pub const PROPERTY_MODULE: &str = "d5963243-262e-4398-b4d7-fb16d06484f6";

/// Form module (Module.bsl inside Form directory)
pub const PROPERTY_FORM_MODULE: &str = "32e087ab-1491-49b6-aba7-43571b41ac2b";

/// Command module
pub const PROPERTY_COMMAND_MODULE: &str = "078a6af8-d22c-4248-9c33-7e90075a3d2c";

/// Object module (ObjectModule.bsl)
pub const PROPERTY_OBJECT_MODULE: &str = "a637f77f-3840-441d-a1c3-699c8c5cb7e0";

/// Manager module (ManagerModule.bsl)
pub const PROPERTY_MANAGER_MODULE: &str = "d1b64a2c-8078-4982-8190-8f81aefda192";

/// RecordSet module (RecordSetModule.bsl)
pub const PROPERTY_RECORDSET_MODULE: &str = "9f36fd70-4bf4-47f6-b235-935f73aab43f";

/// Value manager module (ValueManagerModule.bsl)
pub const PROPERTY_VALUE_MANAGER_MODULE: &str = "3e58c91f-9aaa-4f42-8999-4baf33907b75";

/// Managed application module
pub const PROPERTY_MANAGED_APP_MODULE: &str = "d22e852a-cf8a-4f77-8ccb-3548e7792bea";

/// Session module
pub const PROPERTY_SESSION_MODULE: &str = "9b7bbbae-9771-46f2-9e4d-2489e0ffc702";

/// External connection module
pub const PROPERTY_EXTERNAL_CONNECTION_MODULE: &str = "a4a9c1e2-1e54-4c7f-af06-4ca341198fac";

/// Ordinary application module
pub const PROPERTY_ORDINARY_APP_MODULE: &str = "a78d9ce3-4e0c-48d5-9863-ae7342eedf94";

/// Directories that use the generic module property ID (not file-name-based).
const SINGLE_MODULE_DIRS: &[&str] = &["CommonModules", "WebServices", "HTTPServices"];

/// Resolves property ID from directory name and module file stem.
pub fn property_id_for_module(dir_name: &str, module_stem: &str) -> Option<&'static str> {
    if SINGLE_MODULE_DIRS.contains(&dir_name) {
        return Some(PROPERTY_MODULE);
    }
    match module_stem {
        "Module" => Some(PROPERTY_FORM_MODULE),
        "CommandModule" => Some(PROPERTY_COMMAND_MODULE),
        "ObjectModule" => Some(PROPERTY_OBJECT_MODULE),
        "ManagerModule" => Some(PROPERTY_MANAGER_MODULE),
        "RecordSetModule" => Some(PROPERTY_RECORDSET_MODULE),
        "ValueManagerModule" => Some(PROPERTY_VALUE_MANAGER_MODULE),
        "ManagedApplicationModule" => Some(PROPERTY_MANAGED_APP_MODULE),
        "SessionModule" => Some(PROPERTY_SESSION_MODULE),
        "ExternalConnectionModule" => Some(PROPERTY_EXTERNAL_CONNECTION_MODULE),
        "OrdinaryApplicationModule" => Some(PROPERTY_ORDINARY_APP_MODULE),
        _ => None,
    }
}

/// Returns human-readable Russian name for a property ID.
pub fn module_kind_label(property_id: &str) -> &'static str {
    match property_id {
        PROPERTY_MODULE => "Модуль",
        PROPERTY_FORM_MODULE => "МодульФормы",
        PROPERTY_COMMAND_MODULE => "МодульКоманды",
        PROPERTY_OBJECT_MODULE => "МодульОбъекта",
        PROPERTY_MANAGER_MODULE => "МодульМенеджера",
        PROPERTY_RECORDSET_MODULE => "МодульНабораЗаписей",
        PROPERTY_VALUE_MANAGER_MODULE => "МодульМенеджераЗначения",
        PROPERTY_MANAGED_APP_MODULE => "МодульУправляемогоПриложения",
        PROPERTY_SESSION_MODULE => "МодульСеанса",
        PROPERTY_EXTERNAL_CONNECTION_MODULE => "МодульВнешнегоСоединения",
        PROPERTY_ORDINARY_APP_MODULE => "МодульОбычногоПриложения",
        _ => "Неизвестный",
    }
}
