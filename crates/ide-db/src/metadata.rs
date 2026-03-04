//! Metadata database queries for 1C:Enterprise configurations.
//!
//! This module provides Salsa-based caching for metadata operations.
//! Metadata is loaded from Designer format and cached efficiently using Salsa's
//! incremental computation framework.
//!
//! ## Architecture
//!
//! The metadata system uses Salsa for incremental computation:
//!
//! 1. **Input**: `ConfigurationPathInput` - path to configuration directory
//! 2. **Query**: `load_configuration` - loads and parses metadata (with LRU cache)
//! 3. **Result**: `Arc<Configuration>` - shared configuration instance
//!
//! ## Caching Strategy
//!
//! - **LRU cache**: 16 configurations (supports multi-workspace scenarios)
//! - **Incremental**: Salsa tracks dependencies and invalidates automatically
//! - **Efficient cloning**: Arc wrapper enables cheap copies
//! - **Metadata rarely changes**: Typically loaded once per workspace
//!
//! ## Performance Characteristics
//!
//! - **First load**: ~1 second for typical configuration
//! - **Cached access**: < 1 ms (returns same Arc instance)
//! - **Memory**: ~10-50 MB per configuration (shared via Arc)
//! - **LRU eviction**: Only keeps 16 most recent configurations in memory

use bsl_metadata::traits::Module;
use bsl_metadata::Configuration;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Interned: path to configuration root directory.
///
/// This is a Salsa interned value that represents the path to a 1C configuration.
/// Using `interned` instead of `input` ensures that multiple calls with the same path
/// return the same ID, enabling proper Salsa caching.
/// When the path changes, all dependent queries are invalidated.
#[salsa::interned(debug)]
pub struct ConfigurationPathInput {
    /// Path to configuration root directory (stored as String for Salsa)
    pub path: String,
}

/// Load configuration from directory.
///
/// This is a Salsa tracked query that loads metadata from Designer format.
///
/// # Performance
///
/// - LRU cache: 16 configurations (supports multi-workspace scenarios)
/// - Note: Durability is set via input setters, not tracked function
/// - First load: ~1 second for typical configuration
/// - Cached access: < 1 ms
///
/// # Arguments
///
/// * `db` - Salsa database
/// * `path_input` - Configuration path input (Salsa dependency)
///
/// # Returns
///
/// Loaded configuration wrapped in Arc for efficient cloning
#[salsa::tracked(lru = 16)]
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

/// Metadata database trait.
///
/// Provides access to 1C:Enterprise metadata with Salsa-based caching.
///
/// # Example
///
/// ```no_run
/// use ide_db::{RootDatabaseImpl, metadata::*};
///
/// let mut db = RootDatabaseImpl::new();
/// let path_input = ConfigurationPathInput::new(&db, "/path/to/configuration".to_string());
/// let config = db.load_configuration(path_input);
///
/// println!("Loaded {} common modules", config.common_modules().len());
/// ```
#[salsa::db]
pub trait MetadataDb: salsa::Database {
    /// Load configuration from directory.
    ///
    /// This method is cached by Salsa. The same path will return the same Arc,
    /// avoiding redundant file I/O and parsing.
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

/// Определяет тип модуля по URI файла.
///
/// Парсит путь к файлу для определения типа модуля по структуре:
/// - `CommonModules/<Name>/Ext/Module.bsl` → CommonModule
/// - `Catalogs/<Name>/Commands/<Cmd>/Ext/CommandModule.bsl` → CommandModule
/// - `Catalogs/<Name>/Forms/<Form>/Ext/Form/Module.bsl` → FormModule
/// - `Catalogs/<Name>/Ext/ObjectModule.bsl` → ObjectModule
///
/// # Returns
///
/// Тип модуля если распознан, `None` в противном случае.
pub fn get_module_type_from_uri(file_uri: &str) -> Option<bsl_metadata::ModuleType> {
    let parts: Vec<&str> = file_uri.split('/').collect();

    if parts.is_empty() {
        return None;
    }

    // Ext/ManagedApplicationModule.bsl → ManagedApplicationModule
    if parts.len() >= 2
        && parts[parts.len() - 2] == "Ext"
        && parts[parts.len() - 1] == "ManagedApplicationModule.bsl"
    {
        return Some(bsl_metadata::ModuleType::ManagedApplicationModule);
    }

    // CommonModules/<Name>/Ext/Module.bsl
    // Works with both relative and absolute paths
    if let Some(cm_idx) = parts.iter().position(|&p| p == "CommonModules") {
        if parts.len() >= cm_idx + 4 {
            return Some(bsl_metadata::ModuleType::CommonModule);
        }
    }

    // HTTPServices/<Name>/Ext/Module.bsl
    if let Some(idx) = parts.iter().position(|&p| p == "HTTPServices") {
        if parts.len() >= idx + 4 {
            return Some(bsl_metadata::ModuleType::HTTPServiceModule);
        }
    }

    // WebServices/<Name>/Ext/Module.bsl
    if let Some(idx) = parts.iter().position(|&p| p == "WebServices") {
        if parts.len() >= idx + 4 {
            return Some(bsl_metadata::ModuleType::WebServiceModule);
        }
    }

    // CommonCommands/<Name>/Ext/CommandModule.bsl (top-level common commands)
    if let Some(idx) = parts.iter().position(|&p| p == "CommonCommands" || p == "ОбщиеКоманды")
    {
        if parts.len() >= idx + 4 && parts[parts.len() - 1] == "CommandModule.bsl" {
            return Some(bsl_metadata::ModuleType::CommandModule);
        }
    }

    // <TypePlural>/<Name>/Commands/<Cmd>/Ext/CommandModule.bsl (subordinate commands)
    if let Some(cmd_idx) = parts.iter().position(|&p| p == "Commands") {
        if parts.len() >= cmd_idx + 4 && parts[parts.len() - 1].ends_with("CommandModule.bsl") {
            return Some(bsl_metadata::ModuleType::CommandModule);
        }
    }

    // <TypePlural>/<Name>/Forms/<Form>/Ext/Form/Module.bsl
    // Check for Forms in path and /Ext/Form/Module.bsl at end
    if let Some(forms_idx) = parts.iter().position(|&p| p == "Forms") {
        // Need at least: Forms/<FormName>/Ext/Form/Module.bsl (5 parts after Forms idx)
        if parts.len() >= forms_idx + 5
            && parts[parts.len() - 1] == "Module.bsl"
            && parts[parts.len() - 2] == "Form"
            && parts[parts.len() - 3] == "Ext"
        {
            return Some(bsl_metadata::ModuleType::FormModule);
        }
    }

    // Простые модули (check last file name)
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

/// Information extracted from a module file path.
///
/// Contains the parsed components of a Designer format module path.
#[derive(Debug, Clone)]
pub struct ModulePathInfo {
    /// Type of metadata object (Catalog, Document, Register, etc.)
    pub mdo_type: Option<bsl_metadata::MdoType>,
    /// Name of the metadata object
    pub name: Option<String>,
    /// Type of the module file
    pub module_type: bsl_metadata::ModuleType,
}

/// Parse module file path to extract MDO type and name.
///
/// Parses Designer format paths like:
/// - `Catalogs/<Name>/Ext/ManagerModule.bsl` → (Catalog, Name, ManagerModule)
/// - `InformationRegisters/<Name>/Ext/RecordSetModule.bsl` → (InformationRegister, Name, RecordSetModule)
///
/// # Arguments
/// * `file_uri` - Relative path to the module file
///
/// # Returns
/// Parsed path information or None if path format is unrecognized.
pub fn parse_module_path(file_uri: &str) -> Option<ModulePathInfo> {
    let parts: Vec<&str> = file_uri.split('/').collect();

    if parts.len() < 4 {
        return None;
    }

    // Find the type folder by scanning parts (handles paths with prefix like ./src/cf/)
    let type_idx =
        parts.iter().position(|&p| mdo_type_from_plural(p).is_some() || p == "CommonModules")?;

    // Need at least type + name + Ext + module file
    if parts.len() < type_idx + 4 {
        return None;
    }

    let type_plural = parts[type_idx];
    let name = parts[type_idx + 1].to_string();

    let mdo_type = mdo_type_from_plural(type_plural);

    // Determine module type from file name
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

/// Map plural type folder name to MdoType.
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

/// Find a metadata object by type and name.
///
/// This is a convenience function for looking up metadata objects.
/// It loads the configuration and searches for an object with the given type and name.
///
/// # Arguments
///
/// * `db` - Database with metadata access
/// * `path_input` - Configuration path input
/// * `mdo_type` - Type of metadata object to find
/// * `name` - Name of the object (case-sensitive)
///
/// # Returns
///
/// The metadata object if found, `None` otherwise.
///
/// # Example
///
/// ```no_run
/// # use ide_db::{RootDatabaseImpl, metadata::*};
/// # use bsl_metadata::MdoType;
/// # let db = RootDatabaseImpl::new();
/// # let path_input = ConfigurationPathInput::new(&db, "/path/to/config".to_string());
/// let catalog = find_metadata_object(&db, path_input, MdoType::Catalog, "Products");
/// ```
pub fn find_metadata_object<DB: MetadataDb>(
    db: &DB,
    path_input: ConfigurationPathInput,
    mdo_type: bsl_metadata::MdoType,
    name: &str,
) -> Option<bsl_metadata::MetadataObject> {
    let config = db.load_configuration(path_input);

    // First try to find in metadata_objects
    if let Some(mdo) =
        config.metadata_objects().iter().find(|mdo| mdo.mdo_type == mdo_type && mdo.name == name)
    {
        return Some(mdo.clone());
    }

    // For register types, also search in registers collection
    use bsl_metadata::MdoType;
    if matches!(
        mdo_type,
        MdoType::InformationRegister
            | MdoType::AccumulationRegister
            | MdoType::AccountingRegister
            | MdoType::CalculationRegister
    ) {
        // Find in registers and convert to MetadataObject
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

/// Find a common module by name.
///
/// This is a convenience function for looking up common modules.
///
/// # Arguments
///
/// * `db` - Database with metadata access
/// * `path_input` - Configuration path input
/// * `name` - Name of the common module
///
/// # Returns
///
/// The common module if found, `None` otherwise.
///
/// # Example
///
/// ```no_run
/// # use ide_db::{RootDatabaseImpl, metadata::*};
/// # let db = RootDatabaseImpl::new();
/// # let path_input = ConfigurationPathInput::new(&db, "/path/to/config".to_string());
/// let module = find_common_module(&db, path_input, "ОбщегоНазначения");
/// ```
pub fn find_common_module<DB: MetadataDb>(
    db: &DB,
    path_input: ConfigurationPathInput,
    name: &str,
) -> Option<bsl_metadata::CommonModule> {
    let config = db.load_configuration(path_input);
    config.find_common_module(name).cloned()
}

/// Get the metadata object that owns a module file.
///
/// Parses the file URI to determine which metadata object owns the module.
/// Supports Designer format paths like:
/// - `CommonModules/<Name>/Ext/Module.bsl` → CommonModule "<Name>"
/// - `Catalogs/<Name>/Ext/ManagerModule.bsl` → Catalog "<Name>"
/// - `Catalogs/<Name>/Ext/ObjectModule.bsl` → Catalog "<Name>"
///
/// # Arguments
///
/// * `db` - Database with metadata and file system access
/// * `path_input` - Configuration path input
/// * `file_uri` - URI of the module file (relative to configuration root)
///
/// # Returns
///
/// The owning metadata object if identified, `None` otherwise.
///
/// # Example
///
/// ```no_run
/// # use ide_db::{RootDatabaseImpl, metadata::*};
/// # let db = RootDatabaseImpl::new();
/// # let path_input = ConfigurationPathInput::new(&db, "/path/to/config".to_string());
/// let owner = get_module_owner(&db, path_input, "Catalogs/Products/Ext/ObjectModule.bsl");
/// ```
pub fn get_module_owner<DB: MetadataDb>(
    db: &DB,
    path_input: ConfigurationPathInput,
    file_uri: &str,
) -> Option<ModuleOwner> {
    let _span = tracing::debug_span!("get_module_owner", file_uri).entered();

    // Parse the URI to extract type and name
    // Expected format: <TypePlural>/<Name>/Ext/<ModuleName>.bsl
    let parts: Vec<&str> = file_uri.split('/').collect();

    if parts.len() < 3 {
        tracing::debug!("URI too short, expected at least 3 parts");
        return None;
    }

    let type_plural = parts[0];
    let name = parts[1];

    // Special case: CommonModules
    if type_plural == "CommonModules" {
        return find_common_module(db, path_input, name).map(ModuleOwner::CommonModule);
    }

    // Map plural form to MdoType
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

/// Module owner - either a CommonModule or a MetadataObject.
///
/// This enum represents the possible owners of BSL module files.
#[derive(Debug, Clone, PartialEq)]
pub enum ModuleOwner {
    /// Common module (global, server, client, etc.)
    CommonModule(bsl_metadata::CommonModule),
    /// Other metadata object (Catalog, Document, Register, etc.)
    MetadataObject(bsl_metadata::MetadataObject),
}

impl ModuleOwner {
    /// Get the name of the owning object.
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

/// Find CommonModule in configuration by matching file URI.
///
/// Matches the file path against CommonModule URIs from metadata.
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
                // Normalize paths for comparison (case-insensitive on some systems)
                module_uri.to_lowercase() == file_uri.to_lowercase()
            } else {
                false
            }
        })
        .cloned()
}

/// Load Form metadata from BSL module path.
///
/// Given a FormModule BSL path like:
/// `Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form/Module.bsl`
///
/// Loads form metadata from corresponding XML:
/// `Catalogs/Справочник1/Forms/ФормаЭлемента.xml`
///
/// Returns None if:
/// - Path doesn't match FormModule pattern
/// - XML file doesn't exist
/// - XML parsing fails
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

/// Build module metadata from file path and optional configuration.
///
/// This is the single source of truth for creating ModuleMetadata.
/// Used by both Salsa queries and StreamingProvider.
///
/// # Arguments
/// * `file_path` - Path to the BSL module file
/// * `configuration` - Optional configuration for resolving module-specific metadata
///
/// # Returns
/// ModuleMetadata with all available information filled in.
pub fn build_module_metadata(
    file_path: &Path,
    configuration: Option<&bsl_metadata::Configuration>,
) -> hir_def::ModuleMetadata {
    let uri = file_path.to_string_lossy().to_string();

    // Parse module path to get MDO type and name
    let path_info = parse_module_path(&uri);

    // Determine module type from file URI
    let module_type = get_module_type_from_uri(&uri).unwrap_or(bsl_metadata::ModuleType::Unknown);

    tracing::debug!(uri = %uri, module_type = ?module_type, "build_module_metadata");

    // Initialize result fields
    let mut execution_context = None;
    let mut common_module = None;
    let mut mdo = None;
    let mut register = None;
    let mut form = None;
    let mut http_service = None;
    let mut web_service = None;

    // Load metadata based on module type
    if let Some(config) = configuration {
        match module_type {
            bsl_metadata::ModuleType::CommonModule => {
                // Find CommonModule by URI
                if let Some(cm) = find_common_module_by_uri(config, file_path) {
                    execution_context = Some(hir_def::compute_execution_context(&cm));
                    common_module = Some(Arc::new(cm));
                }
            }
            bsl_metadata::ModuleType::ManagerModule
            | bsl_metadata::ModuleType::ObjectModule
            | bsl_metadata::ModuleType::RecordSetModule => {
                // Load MDO or Register based on path info
                if let Some(ref info) = path_info {
                    if let (Some(mdo_type), Some(ref name)) = (info.mdo_type, &info.name) {
                        // Check if this is a register type
                        if matches!(
                            mdo_type,
                            bsl_metadata::MdoType::InformationRegister
                                | bsl_metadata::MdoType::AccumulationRegister
                                | bsl_metadata::MdoType::AccountingRegister
                                | bsl_metadata::MdoType::CalculationRegister
                        ) {
                            // Find register by type and name
                            if let Some(reg) = config.find_register_by_type_and_name(mdo_type, name)
                            {
                                register = Some(Arc::new(reg.clone()));
                            }
                        } else {
                            // Find metadata object
                            if let Some(obj) = config.find_metadata_object(mdo_type, name) {
                                mdo = Some(Arc::new(obj.clone()));
                            }
                        }
                    }
                }
            }
            bsl_metadata::ModuleType::FormModule => {
                // Load Form metadata for FormModule
                form = load_form_from_path(file_path);
            }
            bsl_metadata::ModuleType::HTTPServiceModule => {
                // Find HTTP service by path
                http_service = find_http_service_by_path(config, file_path);
            }
            bsl_metadata::ModuleType::WebServiceModule => {
                // Find Web service by path
                web_service = find_web_service_by_path(config, file_path);
            }
            _ => {}
        }
    }

    // For FormModule without configuration, try to load form from XML directly
    if module_type == bsl_metadata::ModuleType::FormModule && form.is_none() {
        form = load_form_from_path(file_path);
    }

    hir_def::ModuleMetadata {
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

/// Find HTTP service by BSL module path.
///
/// Given an HTTPServiceModule path like:
/// `HTTPServices/HTTPСервис1/Ext/Module.bsl`
///
/// Extracts the service name and looks it up in configuration.
pub(crate) fn find_http_service_by_path(
    configuration: &bsl_metadata::Configuration,
    file_path: &Path,
) -> Option<Arc<bsl_metadata::HTTPService>> {
    let file_str = file_path.to_string_lossy();

    // Extract HTTP service name from path: HTTPServices/<Name>/Ext/Module.bsl
    let parts: Vec<&str> = file_str.split('/').collect();
    let parts_backslash: Vec<&str> = file_str.split('\\').collect();
    let parts = if parts.len() > parts_backslash.len() { parts } else { parts_backslash };

    // Find HTTPServices in path
    let http_idx = parts.iter().position(|&p| p == "HTTPServices")?;

    // Name should be the next element after HTTPServices
    let name = parts.get(http_idx + 1)?;

    configuration.find_http_service(name).map(|hs| Arc::new(hs.clone()))
}

/// Find Web service (SOAP) metadata by file path.
///
/// Given a WebServiceModule path like:
/// `WebServices/WebСервис1/Ext/Module.bsl`
///
/// Extracts the service name and looks it up in configuration.
pub(crate) fn find_web_service_by_path(
    configuration: &bsl_metadata::Configuration,
    file_path: &Path,
) -> Option<Arc<bsl_metadata::WebService>> {
    let file_str = file_path.to_string_lossy();

    // Extract Web service name from path: WebServices/<Name>/Ext/Module.bsl
    let parts: Vec<&str> = file_str.split('/').collect();
    let parts_backslash: Vec<&str> = file_str.split('\\').collect();
    let parts = if parts.len() > parts_backslash.len() { parts } else { parts_backslash };

    // Find WebServices in path
    let ws_idx = parts.iter().position(|&p| p == "WebServices")?;

    // Name should be the next element after WebServices
    let name = parts.get(ws_idx + 1)?;

    configuration.find_web_service(name).map(|ws| Arc::new(ws.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test database for testing metadata queries
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

        // Create input with test fixtures path
        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path);

        // Load configuration twice
        let config1 = db.load_configuration(path_input);
        let config2 = db.load_configuration(path_input);

        // Should return same Arc (pointer equality)
        assert!(Arc::ptr_eq(&config1, &config2), "Salsa should cache configuration");

        // Verify loaded data
        assert!(!config1.common_modules().is_empty(), "Should load common modules");
    }

    #[test]
    fn test_load_configuration_different_paths() {
        let db = TestDatabase::default();

        let input1 = ConfigurationPathInput::new(&db, "/path/to/config1".to_string());
        let input2 = ConfigurationPathInput::new(&db, "/path/to/config2".to_string());

        // Different inputs should be different
        assert_ne!(input1, input2, "Different paths should create different inputs");
    }

    #[test]
    fn test_find_metadata_object() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path);

        // Find existing catalog
        let catalog =
            find_metadata_object(&db, path_input, bsl_metadata::MdoType::Catalog, "Справочник1");
        assert!(catalog.is_some(), "Should find Справочник1");
        assert_eq!(catalog.unwrap().name, "Справочник1");

        // Try to find non-existent object
        let not_found =
            find_metadata_object(&db, path_input, bsl_metadata::MdoType::Catalog, "NonExistent");
        assert!(not_found.is_none(), "Should not find non-existent object");
    }

    #[test]
    fn test_find_common_module() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path);

        // Find existing common module
        let module = find_common_module(&db, path_input, "ГлобальныйСерверныйМодуль");
        assert!(module.is_some(), "Should find ГлобальныйСерверныйМодуль");

        use bsl_metadata::traits::MdObject;
        assert_eq!(module.unwrap().name(), "ГлобальныйСерверныйМодуль");

        // Try to find non-existent module
        let not_found = find_common_module(&db, path_input, "NonExistent");
        assert!(not_found.is_none(), "Should not find non-existent module");
    }

    #[test]
    fn test_get_module_owner_common_module() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path);

        // CommonModules path
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
        let path_input = ConfigurationPathInput::new(&db, path);

        // Catalogs path (English)
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
        let path_input = ConfigurationPathInput::new(&db, path);

        // Catalogs path (Russian plural)
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
        let path_input = ConfigurationPathInput::new(&db, path);

        // InformationRegisters path
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
        let path_input = ConfigurationPathInput::new(&db, path);

        // URI too short
        let owner = get_module_owner(&db, path_input, "CommonModules/Module.bsl");
        assert!(owner.is_none(), "Should return None for URI too short");

        // Unknown type
        let owner = get_module_owner(&db, path_input, "UnknownType/Object/Ext/Module.bsl");
        assert!(owner.is_none(), "Should return None for unknown type");

        // Non-existent object
        let owner = get_module_owner(&db, path_input, "Catalogs/NonExistent/Ext/ObjectModule.bsl");
        assert!(owner.is_none(), "Should return None for non-existent object");
    }

    #[test]
    fn test_module_owner_clone_and_eq() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path);

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

        // Test Clone
        let owner1_clone = owner1.clone();
        assert_eq!(owner1, owner1_clone, "Cloned owner should be equal");

        // Test PartialEq
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
        // Relative path
        let uri = "CommonModules/ГлобальныйМодуль/Ext/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommonModule));

        // Absolute path
        let uri = "/home/user/project/src/cf/CommonModules/ГлобальныйМодуль/Ext/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommonModule));
    }

    #[test]
    fn test_get_module_type_form_module() {
        // Relative path
        let uri = "Catalogs/Номенклатура/Forms/ФормаЭлемента/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));

        // Absolute path (real-world use case)
        let uri = "/home/user/project/src/cf/BusinessProcesses/Исполнение/Forms/ВводОписанияЗадачиИсполнителя/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));
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

        // With full path
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
        // Streaming mode passes paths like ./src/cf/Catalogs/...
        let info = parse_module_path("./src/cf/Catalogs/ДействияСогласования/Ext/ObjectModule.bsl")
            .unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::Catalog));
        assert_eq!(info.name.as_deref(), Some("ДействияСогласования"));
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
