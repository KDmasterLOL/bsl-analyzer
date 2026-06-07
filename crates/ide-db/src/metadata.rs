use bsl_metadata::traits::Module;
use bsl_metadata::Configuration;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[salsa::interned(debug)]
pub struct ConfigurationPathInput {
    pub path: String,
    pub root_revision: u32,
}

pub fn intern_configuration_path<'db>(
    db: &'db dyn salsa::Database,
    raw_path: &str,
    root_revision: u32,
) -> ConfigurationPathInput<'db> {
    let canonical = canonicalize_configuration_path(raw_path);
    ConfigurationPathInput::new(db, canonical, root_revision)
}

/// Per-config-root revision counter, as a Salsa input so that config-dependent
/// queries which read it (via [`intern_configuration_path`] callers running
/// inside a tracked query) record a dependency on the specific root. Bumping one
/// root's revision then invalidates only the queries that touched that root,
/// instead of a single global counter invalidating every configuration.
#[salsa::input(debug)]
pub struct ConfigRevisionInput {
    pub revision: u32,
}

pub(crate) fn canonicalize_configuration_path(raw_path: &str) -> String {
    if cfg!(windows) {
        let trimmed = raw_path.strip_prefix(r"\\?\").unwrap_or(raw_path);
        let mut s = trimmed.replace('\\', "/");
        s.make_ascii_lowercase();
        s
    } else if raw_path.contains('\\') {
        raw_path.replace('\\', "/")
    } else {
        raw_path.to_owned()
    }
}

#[salsa::input(debug)]
pub struct WorkspaceConfigsInput {
    pub paths: Vec<(Option<String>, PathBuf)>,
}

// Keyed by config root (base config + each extension), so the cache holds one entry
// per root, not per file/module — its size tracks the number of configurations, which
// is small. The cap must exceed the realistic number of extension roots: the graph
// build pre-warms every root before its parallel region (a per-root reload there would
// re-enter the metadata loader's `rayon::scope` inside a worker thread), so an eviction
// under the cap would let that load run in parallel and break the build's concurrency
// invariant. 1024 is far above any real extension count while still bounded.
#[salsa::tracked(lru = 1024)]
pub fn load_configuration<'db>(
    db: &'db dyn salsa::Database,
    path_input: ConfigurationPathInput<'db>,
) -> Arc<Configuration> {
    let _span = tracing::info_span!("load_configuration").entered();

    let path_str = path_input.path(db);
    let path = PathBuf::from(path_str);

    tracing::warn!(?path, "METADATA LOAD: loading configuration from directory");

    let config = bsl_metadata::load_from_directory(&path).unwrap_or_else(|e| {
        tracing::error!(error = %e, ?path, "failed to load configuration");
        Configuration::new("Configuration")
    });

    tracing::warn!(
        common_modules = config.common_modules().len(),
        metadata_objects = config.metadata_objects().len(),
        "METADATA LOAD: configuration loaded successfully"
    );

    Arc::new(config)
}

#[salsa::db]
pub trait MetadataDb: salsa::Database {
    fn load_configuration<'db>(
        &'db self,
        path_input: ConfigurationPathInput<'db>,
    ) -> Arc<Configuration>
    where
        Self: Sized,
    {
        load_configuration(self, path_input)
    }
}

pub fn get_module_type_from_uri(file_uri: &str) -> Option<bsl_metadata::ModuleType> {
    let parts: Vec<&str> = file_uri.split('/').collect();

    if parts.is_empty() {
        return None;
    }

    if parts.len() >= 2
        && parts[parts.len() - 2] == "Ext"
        && parts[parts.len() - 1] == "ManagedApplicationModule.bsl"
    {
        return Some(bsl_metadata::ModuleType::ManagedApplicationModule);
    }

    if let Some(cm_idx) = parts.iter().position(|&p| p == "CommonModules") {
        if parts.len() >= cm_idx + 4 {
            return Some(bsl_metadata::ModuleType::CommonModule);
        }
    }

    if let Some(idx) = parts.iter().position(|&p| p == "HTTPServices") {
        if parts.len() >= idx + 4 {
            return Some(bsl_metadata::ModuleType::HTTPServiceModule);
        }
    }

    if let Some(idx) = parts.iter().position(|&p| p == "WebServices") {
        if parts.len() >= idx + 4 {
            return Some(bsl_metadata::ModuleType::WebServiceModule);
        }
    }

    if let Some(idx) = parts.iter().position(|&p| p == "CommonCommands" || p == "ОбщиеКоманды")
    {
        if parts.len() >= idx + 4 && parts[parts.len() - 1] == "CommandModule.bsl" {
            return Some(bsl_metadata::ModuleType::CommandModule);
        }
    }

    if let Some(cmd_idx) = parts.iter().position(|&p| p == "Commands") {
        if parts.len() >= cmd_idx + 4 && parts[parts.len() - 1].ends_with("CommandModule.bsl") {
            return Some(bsl_metadata::ModuleType::CommandModule);
        }
    }

    if let Some(idx) = parts.iter().rposition(|&p| p == "CommonForms" || p == "ОбщиеФормы")
    {
        if parts.len() == idx + 5
            && parts[parts.len() - 1] == "Module.bsl"
            && parts[parts.len() - 2] == "Form"
            && parts[parts.len() - 3] == "Ext"
        {
            return Some(bsl_metadata::ModuleType::FormModule);
        }
    }

    if let Some(forms_idx) = parts.iter().position(|&p| p == "Forms") {
        if parts.len() >= forms_idx + 5
            && parts[parts.len() - 1] == "Module.bsl"
            && parts[parts.len() - 2] == "Form"
            && parts[parts.len() - 3] == "Ext"
        {
            return Some(bsl_metadata::ModuleType::FormModule);
        }
    }

    if parts.len() >= 4 {
        let module_file = parts[parts.len() - 1];
        return match module_file {
            "ObjectModule.bsl" => Some(bsl_metadata::ModuleType::ObjectModule),
            "ManagerModule.bsl" => Some(bsl_metadata::ModuleType::ManagerModule),
            "RecordSetModule.bsl" => Some(bsl_metadata::ModuleType::RecordSetModule),
            _ => None,
        };
    }

    None
}

#[derive(Debug, Clone)]
pub struct ModulePathInfo {
    pub mdo_type: Option<bsl_metadata::MdoType>,
    pub name: Option<String>,
    pub module_type: bsl_metadata::ModuleType,
}

pub fn parse_module_path(file_uri: &str) -> Option<ModulePathInfo> {
    let parts: Vec<&str> = file_uri.split('/').collect();

    if parts.len() < 4 {
        return None;
    }

    let type_idx =
        parts.iter().rposition(|&p| mdo_type_from_plural(p).is_some() || p == "CommonModules")?;

    if parts.len() < type_idx + 4 {
        return None;
    }

    let type_plural = parts[type_idx];
    let name = parts[type_idx + 1].to_string();

    let mdo_type = mdo_type_from_plural(type_plural);

    let module_file = parts[parts.len() - 1];
    let module_type = match module_file {
        "ObjectModule.bsl" => bsl_metadata::ModuleType::ObjectModule,
        "ManagerModule.bsl" => bsl_metadata::ModuleType::ManagerModule,
        "RecordSetModule.bsl" => bsl_metadata::ModuleType::RecordSetModule,
        "Module.bsl" if type_plural == "CommonModules" => bsl_metadata::ModuleType::CommonModule,
        _ => bsl_metadata::ModuleType::Unknown,
    };

    Some(ModulePathInfo { mdo_type, name: Some(name), module_type })
}

fn mdo_type_from_plural(type_plural: &str) -> Option<bsl_metadata::MdoType> {
    match type_plural {
        "Catalogs" | "Справочники" => Some(bsl_metadata::MdoType::Catalog),
        "Documents" | "Документы" => Some(bsl_metadata::MdoType::Document),
        "BusinessProcesses" | "БизнесПроцессы" => {
            Some(bsl_metadata::MdoType::BusinessProcess)
        }
        "Tasks" | "Задачи" => Some(bsl_metadata::MdoType::Task),
        "ExchangePlans" | "ПланыОбмена" => Some(bsl_metadata::MdoType::ExchangePlan),
        "ChartsOfAccounts" | "ПланыСчетов" => {
            Some(bsl_metadata::MdoType::ChartOfAccounts)
        }
        "ChartsOfCalculationTypes" | "ПланыВидовРасчета" => {
            Some(bsl_metadata::MdoType::ChartOfCalculationTypes)
        }
        "ChartsOfCharacteristicTypes" | "ПланыВидовХарактеристик" => {
            Some(bsl_metadata::MdoType::ChartOfCharacteristicTypes)
        }
        "InformationRegisters" | "РегистрыСведений" => {
            Some(bsl_metadata::MdoType::InformationRegister)
        }
        "AccumulationRegisters" | "РегистрыНакопления" => {
            Some(bsl_metadata::MdoType::AccumulationRegister)
        }
        "AccountingRegisters" | "РегистрыБухгалтерии" => {
            Some(bsl_metadata::MdoType::AccountingRegister)
        }
        "CalculationRegisters" | "РегистрыРасчета" => {
            Some(bsl_metadata::MdoType::CalculationRegister)
        }
        "DataProcessors" | "Обработки" => Some(bsl_metadata::MdoType::DataProcessor),
        "Reports" | "Отчеты" => Some(bsl_metadata::MdoType::Report),
        _ => None,
    }
}

pub fn find_metadata_object<DB: MetadataDb>(
    db: &DB,
    path_input: ConfigurationPathInput,
    mdo_type: bsl_metadata::MdoType,
    name: &str,
) -> Option<bsl_metadata::MetadataObject> {
    let config = db.load_configuration(path_input);

    if let Some(mdo) =
        config.metadata_objects().iter().find(|mdo| mdo.mdo_type == mdo_type && mdo.name == name)
    {
        return Some(mdo.clone());
    }

    use bsl_metadata::MdoType;
    if matches!(
        mdo_type,
        MdoType::InformationRegister
            | MdoType::AccumulationRegister
            | MdoType::AccountingRegister
            | MdoType::CalculationRegister
    ) {
        #[allow(unused_imports)]
        use bsl_metadata::traits::MdObject;
        config
            .registers()
            .iter()
            .find(|reg| reg.mdo_type() == mdo_type && reg.name() == name)
            .map(|reg| bsl_metadata::MetadataObject::new(mdo_type, reg.name()))
    } else {
        None
    }
}

pub fn find_common_module<DB: MetadataDb>(
    db: &DB,
    path_input: ConfigurationPathInput,
    name: &str,
) -> Option<bsl_metadata::CommonModule> {
    let config = db.load_configuration(path_input);
    config.find_common_module(name).cloned()
}

pub fn get_module_owner<DB: MetadataDb>(
    db: &DB,
    path_input: ConfigurationPathInput,
    file_uri: &str,
) -> Option<ModuleOwner> {
    let _span = tracing::debug_span!("get_module_owner", file_uri).entered();

    let parts: Vec<&str> = file_uri.split('/').collect();

    if parts.len() < 3 {
        tracing::debug!("URI too short, expected at least 3 parts");
        return None;
    }

    let type_plural = parts[0];
    let name = parts[1];

    if type_plural == "CommonModules" {
        return find_common_module(db, path_input, name).map(ModuleOwner::CommonModule);
    }

    let mdo_type = match type_plural {
        "Catalogs" | "Справочники" => bsl_metadata::MdoType::Catalog,
        "Documents" | "Документы" => bsl_metadata::MdoType::Document,
        "InformationRegisters" | "РегистрыСведений" => {
            bsl_metadata::MdoType::InformationRegister
        }
        "AccumulationRegisters" | "РегистрыНакопления" => {
            bsl_metadata::MdoType::AccumulationRegister
        }
        "AccountingRegisters" | "РегистрыБухгалтерии" => {
            bsl_metadata::MdoType::AccountingRegister
        }
        "CalculationRegisters" | "РегистрыРасчета" => {
            bsl_metadata::MdoType::CalculationRegister
        }
        _ => {
            tracing::debug!(?type_plural, "Unknown metadata type");
            return None;
        }
    };

    find_metadata_object(db, path_input, mdo_type, name).map(ModuleOwner::MetadataObject)
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModuleOwner {
    CommonModule(bsl_metadata::CommonModule),
    MetadataObject(bsl_metadata::MetadataObject),
}

impl ModuleOwner {
    pub fn name(&self) -> &str {
        match self {
            ModuleOwner::CommonModule(m) => {
                use bsl_metadata::traits::MdObject;
                m.name()
            }
            ModuleOwner::MetadataObject(m) => &m.name,
        }
    }
}

pub(crate) fn find_common_module_by_uri(
    configuration: &bsl_metadata::Configuration,
    file_path: &Path,
) -> Option<bsl_metadata::CommonModule> {
    let file_uri = file_path.to_string_lossy().to_string();

    configuration
        .common_modules()
        .iter()
        .find(|module| {
            if let Some(module_uri) = module.uri() {
                module_uri.to_lowercase() == file_uri.to_lowercase()
            } else {
                false
            }
        })
        .cloned()
}

pub(crate) fn load_form_from_path(file_path: &Path) -> Option<Arc<bsl_metadata::Form>> {
    use bsl_metadata::xml_parser::parse_form_from_bsl_path;

    tracing::debug!(path = %file_path.display(), "Attempting to load form metadata");

    match parse_form_from_bsl_path(file_path) {
        Ok(form) => {
            tracing::debug!(
                form_name = %form.name(),
                form_type = ?form.form_type(),
                event_handlers = form.event_handlers().len(),
                command_handlers = form.command_handlers().len(),
                "Loaded form metadata"
            );
            Some(Arc::new(form))
        }
        Err(e) => {
            tracing::debug!(?e, path = %file_path.display(), "Could not load form metadata");
            None
        }
    }
}

pub fn build_module_metadata(
    file_path: &Path,
    configuration: Option<&bsl_metadata::Configuration>,
) -> hir::ModuleMetadata {
    let uri = file_path.to_string_lossy().to_string();

    let path_info = parse_module_path(&uri);

    let module_type = get_module_type_from_uri(&uri).unwrap_or(bsl_metadata::ModuleType::Unknown);

    tracing::debug!(uri = %uri, module_type = ?module_type, "build_module_metadata");

    let mut execution_context = None;
    let mut common_module = None;
    let mut mdo = None;
    let mut register = None;
    let mut form = None;
    let mut http_service = None;
    let mut web_service = None;

    if let Some(config) = configuration {
        match module_type {
            bsl_metadata::ModuleType::CommonModule => {
                if let Some(cm) = find_common_module_by_uri(config, file_path) {
                    execution_context = Some(hir::compute_execution_context(&cm));
                    common_module = Some(Arc::new(cm));
                }
            }
            bsl_metadata::ModuleType::ManagerModule
            | bsl_metadata::ModuleType::ObjectModule
            | bsl_metadata::ModuleType::RecordSetModule => {
                if let Some(ref info) = path_info {
                    if let (Some(mdo_type), Some(ref name)) = (info.mdo_type, &info.name) {
                        if matches!(
                            mdo_type,
                            bsl_metadata::MdoType::InformationRegister
                                | bsl_metadata::MdoType::AccumulationRegister
                                | bsl_metadata::MdoType::AccountingRegister
                                | bsl_metadata::MdoType::CalculationRegister
                        ) {
                            if let Some(reg) = config.find_register_by_type_and_name(mdo_type, name)
                            {
                                register = Some(Arc::new(reg.clone()));
                            }
                        } else {
                            if let Some(obj) = config.find_metadata_object(mdo_type, name) {
                                mdo = Some(Arc::new(obj.clone()));
                            }
                        }
                    }
                }
            }
            bsl_metadata::ModuleType::FormModule => {
                form = load_form_from_path(file_path);
            }
            bsl_metadata::ModuleType::HTTPServiceModule => {
                http_service = find_http_service_by_path(config, file_path);
            }
            bsl_metadata::ModuleType::WebServiceModule => {
                web_service = find_web_service_by_path(config, file_path);
            }
            _ => {}
        }
    }

    if module_type == bsl_metadata::ModuleType::FormModule && form.is_none() {
        form = load_form_from_path(file_path);
    }

    hir::ModuleMetadata {
        module_type,
        execution_context,
        common_module,
        mdo,
        register,
        form,
        http_service,
        web_service,
    }
}

pub(crate) fn find_http_service_by_path(
    configuration: &bsl_metadata::Configuration,
    file_path: &Path,
) -> Option<Arc<bsl_metadata::HTTPService>> {
    let file_str = file_path.to_string_lossy().replace('\\', "/");

    let parts: Vec<&str> = file_str.split('/').collect();

    let http_idx = parts.iter().position(|&p| p == "HTTPServices")?;

    let name = parts.get(http_idx + 1)?;

    configuration.find_http_service(name).map(|hs| Arc::new(hs.clone()))
}

pub(crate) fn find_web_service_by_path(
    configuration: &bsl_metadata::Configuration,
    file_path: &Path,
) -> Option<Arc<bsl_metadata::WebService>> {
    let file_str = file_path.to_string_lossy().replace('\\', "/");

    let parts: Vec<&str> = file_str.split('/').collect();

    let ws_idx = parts.iter().position(|&p| p == "WebServices")?;

    let name = parts.get(ws_idx + 1)?;

    configuration.find_web_service(name).map(|ws| Arc::new(ws.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[salsa::db]
    #[derive(Default, Clone)]
    struct TestDatabase {
        storage: salsa::Storage<Self>,
    }

    #[salsa::db]
    impl salsa::Database for TestDatabase {}

    #[salsa::db]
    impl MetadataDb for TestDatabase {}

    #[test]
    fn test_load_configuration_caching() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let config1 = db.load_configuration(path_input);
        let config2 = db.load_configuration(path_input);

        assert!(Arc::ptr_eq(&config1, &config2), "Salsa should cache configuration");

        assert!(!config1.common_modules().is_empty(), "Should load common modules");
    }

    #[test]
    fn test_load_configuration_different_paths() {
        let db = TestDatabase::default();

        let input1 = ConfigurationPathInput::new(&db, "/path/to/config1".to_string(), 0);
        let input2 = ConfigurationPathInput::new(&db, "/path/to/config2".to_string(), 0);

        assert_ne!(input1, input2, "Different paths should create different inputs");
    }

    #[test]
    fn intern_configuration_path_collapses_separator_variants() {
        let db = TestDatabase::default();
        let backslash = intern_configuration_path(&db, r"C:\foo\bar", 0);
        let forward = intern_configuration_path(&db, "C:/foo/bar", 0);
        assert_eq!(
            backslash, forward,
            "backslash and forward-slash paths must intern as the same Salsa key",
        );
    }

    #[cfg(windows)]
    #[test]
    fn intern_configuration_path_is_case_insensitive_on_windows() {
        let db = TestDatabase::default();
        let upper = intern_configuration_path(&db, r"C:\Foo\Bar", 0);
        let lower = intern_configuration_path(&db, r"c:\foo\bar", 0);
        assert_eq!(upper, lower);
    }

    #[cfg(windows)]
    #[test]
    fn intern_configuration_path_strips_extended_prefix_on_windows() {
        let db = TestDatabase::default();
        let extended = intern_configuration_path(&db, r"\\?\C:\foo\bar", 0);
        let plain = intern_configuration_path(&db, r"C:\foo\bar", 0);
        assert_eq!(extended, plain);
    }

    #[cfg(not(windows))]
    #[test]
    fn intern_configuration_path_preserves_case_on_posix() {
        let db = TestDatabase::default();
        let upper = intern_configuration_path(&db, "/Foo/Bar", 0);
        let lower = intern_configuration_path(&db, "/foo/bar", 0);
        assert_ne!(upper, lower, "POSIX file systems are case-sensitive");
    }

    #[test]
    fn test_find_metadata_object() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let catalog =
            find_metadata_object(&db, path_input, bsl_metadata::MdoType::Catalog, "Справочник1");
        assert!(catalog.is_some(), "Should find Справочник1");
        assert_eq!(catalog.unwrap().name, "Справочник1");

        let not_found =
            find_metadata_object(&db, path_input, bsl_metadata::MdoType::Catalog, "NonExistent");
        assert!(not_found.is_none(), "Should not find non-existent object");
    }

    #[test]
    fn test_find_common_module() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let module = find_common_module(&db, path_input, "ГлобальныйСерверныйМодуль");
        assert!(module.is_some(), "Should find ГлобальныйСерверныйМодуль");

        use bsl_metadata::traits::MdObject;
        assert_eq!(module.unwrap().name(), "ГлобальныйСерверныйМодуль");

        let not_found = find_common_module(&db, path_input, "NonExistent");
        assert!(not_found.is_none(), "Should not find non-existent module");
    }

    #[test]
    fn test_get_module_owner_common_module() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner = get_module_owner(
            &db,
            path_input,
            "CommonModules/ГлобальныйСерверныйМодуль/Ext/Module.bsl",
        );

        assert!(owner.is_some(), "Should find module owner");
        let owner = owner.unwrap();

        match &owner {
            ModuleOwner::CommonModule(m) => {
                use bsl_metadata::traits::MdObject;
                assert_eq!(m.name(), "ГлобальныйСерверныйМодуль");
            }
            _ => panic!("Should be CommonModule"),
        }

        assert_eq!(owner.name(), "ГлобальныйСерверныйМодуль");
    }

    #[test]
    fn test_get_module_owner_catalog_english() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner = get_module_owner(&db, path_input, "Catalogs/Справочник1/Ext/ObjectModule.bsl");

        assert!(owner.is_some(), "Should find catalog owner");
        let owner = owner.unwrap();

        match &owner {
            ModuleOwner::MetadataObject(m) => {
                assert_eq!(m.name, "Справочник1");
                assert_eq!(m.mdo_type, bsl_metadata::MdoType::Catalog);
            }
            _ => panic!("Should be MetadataObject"),
        }

        assert_eq!(owner.name(), "Справочник1");
    }

    #[test]
    fn test_get_module_owner_catalog_russian() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner =
            get_module_owner(&db, path_input, "Справочники/Справочник1/Ext/ManagerModule.bsl");

        assert!(owner.is_some(), "Should find catalog owner with Russian plural");
        assert_eq!(owner.unwrap().name(), "Справочник1");
    }

    #[test]
    fn test_get_module_owner_register() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner = get_module_owner(
            &db,
            path_input,
            "InformationRegisters/РегистрСведений1/Ext/ManagerModule.bsl",
        );

        assert!(owner.is_some(), "Should find register owner");
        let owner = owner.unwrap();

        match &owner {
            ModuleOwner::MetadataObject(m) => {
                assert_eq!(m.name, "РегистрСведений1");
                assert_eq!(m.mdo_type, bsl_metadata::MdoType::InformationRegister);
            }
            _ => panic!("Should be MetadataObject"),
        }
    }

    #[test]
    fn test_get_module_owner_invalid_uri() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner = get_module_owner(&db, path_input, "CommonModules/Module.bsl");
        assert!(owner.is_none(), "Should return None for URI too short");

        let owner = get_module_owner(&db, path_input, "UnknownType/Object/Ext/Module.bsl");
        assert!(owner.is_none(), "Should return None for unknown type");

        let owner = get_module_owner(&db, path_input, "Catalogs/NonExistent/Ext/ObjectModule.bsl");
        assert!(owner.is_none(), "Should return None for non-existent object");
    }

    #[test]
    fn test_module_owner_clone_and_eq() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner1 = get_module_owner(
            &db,
            path_input,
            "CommonModules/ГлобальныйСерверныйМодуль/Ext/Module.bsl",
        );
        let owner2 = get_module_owner(
            &db,
            path_input,
            "CommonModules/ГлобальныйСерверныйМодуль/Ext/Module.bsl",
        );

        assert!(owner1.is_some() && owner2.is_some());

        let owner1_clone = owner1.clone();
        assert_eq!(owner1, owner1_clone, "Cloned owner should be equal");

        assert_eq!(owner1, owner2, "Same module owner should be equal");
    }

    #[test]
    fn test_get_module_type_command_module() {
        let uri = "Catalogs/Справочник1/Commands/Команда1/Ext/CommandModule.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommandModule));
    }

    #[test]
    fn test_get_module_type_common_command_module() {
        let uri = "CommonCommands/АвтономнаяРабота/Ext/CommandModule.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommandModule));

        let uri = "src/cf/CommonCommands/АвтономнаяРабота/Ext/CommandModule.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommandModule));

        let uri = "ОбщиеКоманды/ВыполнитьДействие/Ext/CommandModule.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommandModule));
    }

    #[test]
    fn test_get_module_type_common_module() {
        let uri = "CommonModules/ГлобальныйМодуль/Ext/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommonModule));

        let uri = "/home/user/project/src/cf/CommonModules/ГлобальныйМодуль/Ext/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommonModule));
    }

    #[test]
    fn test_get_module_type_form_module() {
        let uri = "Catalogs/Номенклатура/Forms/ФормаЭлемента/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));

        let uri = "/home/user/project/src/cf/BusinessProcesses/Исполнение/Forms/ВводОписанияЗадачиИсполнителя/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));
    }

    #[test]
    fn test_get_module_type_common_form_module() {
        let uri = "CommonForms/ТестоваяФорма/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));

        let uri = "/home/user/project/src/cf/CommonForms/ТестоваяФорма/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));

        let uri = "ОбщиеФормы/ТестоваяФорма/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));

        let uri = "CommonForms/ТестоваяФорма/Ext/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), None);
    }

    #[test]
    fn test_build_module_metadata_loads_common_form_without_configuration() {
        let fixture_root = std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../bsl-metadata/fixtures/designer"
        ));
        let bsl_path = fixture_root.join("CommonForms/ТестоваяФорма/Ext/Form/Module.bsl");

        let metadata = build_module_metadata(&bsl_path, None);

        assert_eq!(metadata.module_type, bsl_metadata::ModuleType::FormModule);
        let form = metadata.form.as_ref().expect("common form metadata should be loaded");
        assert_eq!(form.name(), "ТестоваяФорма");
        assert!(form.is_handler("ПриСозданииНаСервере"));
    }

    #[test]
    fn test_get_module_type_object_module() {
        let uri = "Catalogs/Номенклатура/Ext/ObjectModule.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::ObjectModule));
    }

    #[test]
    fn test_get_module_type_unknown() {
        let uri = "SomeRandomPath/File.bsl";
        assert_eq!(get_module_type_from_uri(uri), None);
    }

    #[test]
    fn test_get_module_type_managed_application_module() {
        let uri = "Ext/ManagedApplicationModule.bsl";
        assert_eq!(
            get_module_type_from_uri(uri),
            Some(bsl_metadata::ModuleType::ManagedApplicationModule)
        );

        let uri = "Configuration/Ext/ManagedApplicationModule.bsl";
        assert_eq!(
            get_module_type_from_uri(uri),
            Some(bsl_metadata::ModuleType::ManagedApplicationModule)
        );
    }

    #[test]
    fn test_parse_module_path_simple() {
        let info = parse_module_path("Catalogs/Справочник1/Ext/ObjectModule.bsl").unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::Catalog));
        assert_eq!(info.name.as_deref(), Some("Справочник1"));
        assert_eq!(info.module_type, bsl_metadata::ModuleType::ObjectModule);
    }

    #[test]
    fn test_parse_module_path_with_prefix() {
        let info = parse_module_path("./src/cf/Catalogs/ДействияСогласования/Ext/ObjectModule.bsl")
            .unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::Catalog));
        assert_eq!(info.name.as_deref(), Some("ДействияСогласования"));
        assert_eq!(info.module_type, bsl_metadata::ModuleType::ObjectModule);
    }

    #[test]
    fn test_parse_module_path_with_absolute_documents_prefix() {
        let info = parse_module_path(
            "/Users/test/Documents/git/project/Catalogs/Справочник1/Ext/ObjectModule.bsl",
        )
        .unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::Catalog));
        assert_eq!(info.name.as_deref(), Some("Справочник1"));
        assert_eq!(info.module_type, bsl_metadata::ModuleType::ObjectModule);
    }

    #[test]
    fn test_parse_module_path_document() {
        let info =
            parse_module_path("src/cf/Documents/ПриходнаяНакладная/Ext/ObjectModule.bsl").unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::Document));
        assert_eq!(info.name.as_deref(), Some("ПриходнаяНакладная"));
    }

    #[test]
    fn test_parse_module_path_register() {
        let info = parse_module_path(
            "src/cf/InformationRegisters/НастройкиОбмена/Ext/RecordSetModule.bsl",
        )
        .unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::InformationRegister));
        assert_eq!(info.name.as_deref(), Some("НастройкиОбмена"));
        assert_eq!(info.module_type, bsl_metadata::ModuleType::RecordSetModule);
    }

    #[test]
    fn test_parse_module_path_data_processor() {
        let info = parse_module_path("DataProcessors/ЗагрузкаДанных/Ext/ObjectModule.bsl").unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::DataProcessor));
        assert_eq!(info.name.as_deref(), Some("ЗагрузкаДанных"));
    }
}
