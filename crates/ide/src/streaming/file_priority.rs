use std::path::Path;
use std::sync::Arc;
use stdx::case::CaseExt;

use bsl_metadata::{CommonModule, Configuration};

pub mod priority {
    pub const COMMON_MODULE_SERVER: u8 = 0;
    pub const COMMON_MODULE_SERVER_CALL: u8 = 1;
    pub const COMMON_MODULE_CLIENT_SERVER: u8 = 2;
    pub const COMMON_MODULE_CLIENT: u8 = 3;
    pub const MANAGER_MODULE: u8 = 4;
    pub const OBJECT_MODULE: u8 = 5;
    pub const FORM_MODULE: u8 = 6;
    pub const OTHER: u8 = 7;
}

pub fn compute_priority(path: &Path, configuration: Option<&Arc<Configuration>>) -> u8 {
    let path_str = path.to_string_lossy();
    let normalized = path_str.replace('\\', "/");

    if let Some(module_name) = extract_common_module_name(&normalized) {
        if let Some(config) = configuration {
            if let Some(common_module) = config.find_common_module(&module_name) {
                return common_module_priority(common_module);
            }
        }
        return priority::COMMON_MODULE_CLIENT_SERVER;
    }

    module_type_priority(&normalized)
}

fn extract_common_module_name(path: &str) -> Option<String> {
    let lower = path.fold_lower();

    let (prefix, prefix_len) = if let Some(idx) = lower.find("commonmodules/") {
        (idx, "commonmodules/".len())
    } else if let Some(idx) = lower.find("общиемодули/") {
        (idx, "общиемодули/".len())
    } else {
        return None;
    };

    if !lower.ends_with("/module.bsl") && !lower.ends_with("/ext/module.bsl") {
        return None;
    }

    let after_prefix = &path[prefix + prefix_len..];
    let name = after_prefix.split('/').next()?;

    if name.is_empty() {
        return None;
    }

    Some(name.to_string())
}

fn common_module_priority(module: &CommonModule) -> u8 {
    let is_server = module.is_server();
    let is_client =
        module.is_client_managed_application() || module.is_client_ordinary_application();
    let is_server_call = module.is_server_call();

    if is_server && !is_client && !is_server_call {
        return priority::COMMON_MODULE_SERVER;
    }

    if is_server_call {
        return priority::COMMON_MODULE_SERVER_CALL;
    }

    if is_server && is_client {
        return priority::COMMON_MODULE_CLIENT_SERVER;
    }

    if is_client {
        return priority::COMMON_MODULE_CLIENT;
    }

    priority::COMMON_MODULE_CLIENT_SERVER
}

fn module_type_priority(path: &str) -> u8 {
    let lower = path.fold_lower();

    if lower.contains("/ext/managermodule.bsl") || lower.contains("/ext/модульменеджера.bsl")
    {
        return priority::MANAGER_MODULE;
    }

    if lower.contains("/ext/objectmodule.bsl")
        || lower.contains("/ext/recordsetmodule.bsl")
        || lower.contains("/ext/модульобъекта.bsl")
        || lower.contains("/ext/модульнабора.bsl")
    {
        return priority::OBJECT_MODULE;
    }

    if (lower.contains("/forms/") || lower.contains("/формы/"))
        && (lower.ends_with("/ext/form/module.bsl") || lower.ends_with("/ext/form/модуль.bsl"))
    {
        return priority::FORM_MODULE;
    }

    priority::OTHER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_common_module_name() {
        assert_eq!(
            extract_common_module_name("CommonModules/ServerModule/Ext/Module.bsl"),
            Some("ServerModule".to_string())
        );

        assert_eq!(
            extract_common_module_name("/project/CommonModules/Utils/Ext/Module.bsl"),
            Some("Utils".to_string())
        );

        assert_eq!(
            extract_common_module_name("CommonModules/Test/Module.bsl"),
            Some("Test".to_string())
        );
    }

    #[test]
    fn test_extract_common_module_name_russian() {
        assert_eq!(
            extract_common_module_name("ОбщиеМодули/ОбщийМодуль/Ext/Module.bsl"),
            Some("ОбщийМодуль".to_string())
        );
    }

    #[test]
    fn test_extract_common_module_name_none() {
        assert_eq!(extract_common_module_name("Catalogs/Test/Ext/ObjectModule.bsl"), None);

        assert_eq!(extract_common_module_name("CommonModules//Ext/Module.bsl"), None);

        assert_eq!(extract_common_module_name("CommonModules/Test/Ext/Other.bsl"), None);
    }

    #[test]
    fn test_common_module_priority_server_only() {
        let module = CommonModule::builder().name("Test").server(true).build();

        assert_eq!(common_module_priority(&module), priority::COMMON_MODULE_SERVER);
    }

    #[test]
    fn test_common_module_priority_server_call() {
        let module = CommonModule::builder().name("Test").server(true).server_call(true).build();

        assert_eq!(common_module_priority(&module), priority::COMMON_MODULE_SERVER_CALL);
    }

    #[test]
    fn test_common_module_priority_client_server() {
        let module = CommonModule::builder()
            .name("Test")
            .server(true)
            .client_managed_application(true)
            .build();

        assert_eq!(common_module_priority(&module), priority::COMMON_MODULE_CLIENT_SERVER);
    }

    #[test]
    fn test_common_module_priority_client_only() {
        let module = CommonModule::builder().name("Test").client_managed_application(true).build();

        assert_eq!(common_module_priority(&module), priority::COMMON_MODULE_CLIENT);
    }

    #[test]
    fn test_module_type_priority() {
        assert_eq!(
            module_type_priority("Catalogs/Products/Ext/ManagerModule.bsl"),
            priority::MANAGER_MODULE
        );

        assert_eq!(
            module_type_priority("Catalogs/Products/Ext/ObjectModule.bsl"),
            priority::OBJECT_MODULE
        );

        assert_eq!(
            module_type_priority("Catalogs/Products/Forms/ListForm/Ext/Form/Module.bsl"),
            priority::FORM_MODULE
        );

        assert_eq!(module_type_priority("SessionModule.bsl"), priority::OTHER);
    }

    #[test]
    fn test_compute_priority_without_config() {
        let path = Path::new("CommonModules/TestModule/Ext/Module.bsl");

        let priority = compute_priority(path, None);

        assert_eq!(priority, priority::COMMON_MODULE_CLIENT_SERVER);
    }

    #[test]
    fn test_compute_priority_non_common_module() {
        let path = Path::new("Catalogs/Products/Ext/ManagerModule.bsl");

        let priority = compute_priority(path, None);

        assert_eq!(priority, priority::MANAGER_MODULE);
    }
}
