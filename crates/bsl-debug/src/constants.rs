pub const PROPERTY_MODULE: &str = "d5963243-262e-4398-b4d7-fb16d06484f6";

pub const PROPERTY_FORM_MODULE: &str = "32e087ab-1491-49b6-aba7-43571b41ac2b";

pub const PROPERTY_COMMAND_MODULE: &str = "078a6af8-d22c-4248-9c33-7e90075a3d2c";

pub const PROPERTY_OBJECT_MODULE: &str = "a637f77f-3840-441d-a1c3-699c8c5cb7e0";

pub const PROPERTY_MANAGER_MODULE: &str = "d1b64a2c-8078-4982-8190-8f81aefda192";

pub const PROPERTY_RECORDSET_MODULE: &str = "9f36fd70-4bf4-47f6-b235-935f73aab43f";

pub const PROPERTY_VALUE_MANAGER_MODULE: &str = "3e58c91f-9aaa-4f42-8999-4baf33907b75";

pub const PROPERTY_MANAGED_APP_MODULE: &str = "d22e852a-cf8a-4f77-8ccb-3548e7792bea";

pub const PROPERTY_SESSION_MODULE: &str = "9b7bbbae-9771-46f2-9e4d-2489e0ffc702";

pub const PROPERTY_EXTERNAL_CONNECTION_MODULE: &str = "a4a9c1e2-1e54-4c7f-af06-4ca341198fac";

pub const PROPERTY_ORDINARY_APP_MODULE: &str = "a78d9ce3-4e0c-48d5-9863-ae7342eedf94";

const SINGLE_MODULE_DIRS: &[&str] = &["CommonModules", "WebServices", "HTTPServices"];

pub fn property_id_for_module(
    dir_name: &str,
    module_kind: bsl_conventions::ConventionalName,
) -> Option<&'static str> {
    use bsl_conventions::ConventionalName as Conv;
    if SINGLE_MODULE_DIRS.contains(&dir_name) {
        return Some(PROPERTY_MODULE);
    }
    match module_kind {
        Conv::Module => Some(PROPERTY_FORM_MODULE),
        Conv::CommandModule => Some(PROPERTY_COMMAND_MODULE),
        Conv::ObjectModule => Some(PROPERTY_OBJECT_MODULE),
        Conv::ManagerModule => Some(PROPERTY_MANAGER_MODULE),
        Conv::RecordSetModule => Some(PROPERTY_RECORDSET_MODULE),
        Conv::ValueManagerModule => Some(PROPERTY_VALUE_MANAGER_MODULE),
        Conv::ManagedApplicationModule => Some(PROPERTY_MANAGED_APP_MODULE),
        Conv::SessionModule => Some(PROPERTY_SESSION_MODULE),
        Conv::ExternalConnectionModule => Some(PROPERTY_EXTERNAL_CONNECTION_MODULE),
        Conv::OrdinaryApplicationModule => Some(PROPERTY_ORDINARY_APP_MODULE),
        _ => None,
    }
}

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
