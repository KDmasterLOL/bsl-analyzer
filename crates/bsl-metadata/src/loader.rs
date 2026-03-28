//! Metadata loader for Designer format
//!
//! Loads 1C:Enterprise metadata from Designer format directory structure.
//!
//! ## Designer Format Structure
//!
//! **CRITICAL:** XML files are NEXT TO object folders, code files are inside Ext/ subdirectories:
//!
//! ```text
//! Configuration.xml                      # Root configuration
//! ConfigDumpInfo.xml                     # Dump information
//!
//! CommonModules/
//! ├── <Name>.xml                         # XML NEXT TO folder!
//! └── <Name>/                            # Object folder
//!     └── Ext/
//!         └── Module.bsl                 # Code inside Ext/
//!
//! Catalogs/
//! ├── <Name>.xml                         # XML NEXT TO folder!
//! └── <Name>/                            # Object folder
//!     └── Ext/
//!         ├── ManagerModule.bsl          # Code inside Ext/
//!         └── ObjectModule.bsl
//!
//! InformationRegisters/
//! ├── <Name>.xml                         # XML NEXT TO folder!
//! └── <Name>/                            # Object folder
//!     └── Ext/
//!         └── ManagerModule.bsl          # Code inside Ext/
//! ```

use crate::configuration::Configuration;
use crate::error::Result;
use crate::metadata_object::{MdoType, MetadataObject};
use crate::traits::MdObject;
use crate::xml_parser;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

/// Load configuration from Designer format directory
///
/// # Arguments
///
/// * `path` - Path to configuration root directory
///
/// # Returns
///
/// Loaded `Configuration` with all metadata objects
///
/// # Example
///
/// ```no_run
/// # use bsl_metadata::load_from_directory;
/// let config = load_from_directory("/path/to/configuration")?;
/// println!("Loaded {} common modules", config.common_modules().len());
/// # Ok::<(), bsl_metadata::MetadataError>(())
/// ```
pub fn load_from_directory(path: impl AsRef<Path>) -> Result<Configuration> {
    let path = path.as_ref();
    let _span = tracing::info_span!("load_from_directory", ?path).entered();

    let loaded = load_all_metadata_parallel(path);
    let config = build_configuration(loaded);

    tracing::info!(
        common_modules = config.common_modules().len(),
        metadata_objects = config.metadata_objects().len(),
        registers = config.registers().len(),
        event_subscriptions = config.event_subscriptions().len(),
        scheduled_jobs = config.scheduled_jobs().len(),
        roles = config.roles().len(),
        defined_types = config.defined_types().len(),
        http_services = config.http_services().len(),
        web_services = config.web_services().len(),
        "configuration loaded"
    );

    Ok(config)
}

/// All metadata loaded from disk, before assembling into Configuration.
struct LoadedMetadata {
    common_modules: Vec<crate::common_module::CommonModule>,
    catalogs: Vec<MetadataObject>,
    documents: Vec<MetadataObject>,
    info_registers: Vec<crate::register::Register>,
    accum_registers: Vec<crate::register::Register>,
    account_registers: Vec<crate::register::Register>,
    calc_registers: Vec<crate::register::Register>,
    event_subscriptions: Vec<crate::event_subscription::EventSubscription>,
    scheduled_jobs: Vec<crate::scheduled_job::ScheduledJob>,
    roles: Vec<crate::role::Role>,
    defined_types: Vec<crate::defined_type::DefinedType>,
    charts_char_types: Vec<MetadataObject>,
    constants: Vec<MetadataObject>,
    exchange_plans: Vec<MetadataObject>,
    business_processes: Vec<MetadataObject>,
    enums: Vec<MetadataObject>,
    tasks: Vec<MetadataObject>,
    charts_accounts: Vec<MetadataObject>,
    charts_calc_types: Vec<MetadataObject>,
    external_data_sources: Vec<MetadataObject>,
    http_services: Vec<crate::http_service::HTTPService>,
    web_services: Vec<crate::web_service::WebService>,
}

/// Load all metadata types in parallel using rayon::scope.
fn load_all_metadata_parallel(path: &Path) -> LoadedMetadata {
    let common_modules = Mutex::new(Vec::new());
    let catalogs = Mutex::new(Vec::new());
    let documents = Mutex::new(Vec::new());
    let info_registers = Mutex::new(Vec::new());
    let accum_registers = Mutex::new(Vec::new());
    let account_registers = Mutex::new(Vec::new());
    let calc_registers = Mutex::new(Vec::new());
    let event_subscriptions = Mutex::new(Vec::new());
    let scheduled_jobs = Mutex::new(Vec::new());
    let roles = Mutex::new(Vec::new());
    let defined_types = Mutex::new(Vec::new());
    let charts_char_types = Mutex::new(Vec::new());
    let constants = Mutex::new(Vec::new());
    let exchange_plans = Mutex::new(Vec::new());
    let business_processes = Mutex::new(Vec::new());
    let enums = Mutex::new(Vec::new());
    let tasks = Mutex::new(Vec::new());
    let charts_accounts = Mutex::new(Vec::new());
    let charts_calc_types = Mutex::new(Vec::new());
    let external_data_sources = Mutex::new(Vec::new());
    let http_services = Mutex::new(Vec::new());
    let web_services = Mutex::new(Vec::new());

    rayon::scope(|s| {
        s.spawn(|_| {
            *common_modules.lock().unwrap() =
                load_common_modules_parallel(&path.join("CommonModules"))
        });
        s.spawn(|_| *catalogs.lock().unwrap() = load_catalogs_parallel(&path.join("Catalogs")));
        s.spawn(|_| *documents.lock().unwrap() = load_documents_parallel(&path.join("Documents")));
        s.spawn(|_| {
            *info_registers.lock().unwrap() =
                load_information_registers_parallel(&path.join("InformationRegisters"))
        });
        s.spawn(|_| {
            *accum_registers.lock().unwrap() =
                load_accumulation_registers_parallel(&path.join("AccumulationRegisters"))
        });
        s.spawn(|_| {
            *account_registers.lock().unwrap() =
                load_accounting_registers_parallel(&path.join("AccountingRegisters"))
        });
        s.spawn(|_| {
            *calc_registers.lock().unwrap() =
                load_calculation_registers_parallel(&path.join("CalculationRegisters"))
        });
        s.spawn(|_| {
            *event_subscriptions.lock().unwrap() =
                load_event_subscriptions_parallel(&path.join("EventSubscriptions"))
        });
        s.spawn(|_| {
            *scheduled_jobs.lock().unwrap() =
                load_scheduled_jobs_parallel(&path.join("ScheduledJobs"))
        });
        s.spawn(|_| *roles.lock().unwrap() = load_roles_parallel(&path.join("Roles")));
        s.spawn(|_| {
            *defined_types.lock().unwrap() = load_defined_types_parallel(&path.join("DefinedTypes"))
        });
        s.spawn(|_| {
            *charts_char_types.lock().unwrap() = load_charts_of_characteristic_types_parallel(
                &path.join("ChartsOfCharacteristicTypes"),
            )
        });
        s.spawn(|_| *constants.lock().unwrap() = load_constants_parallel(&path.join("Constants")));
        s.spawn(|_| {
            *exchange_plans.lock().unwrap() =
                load_exchange_plans_parallel(&path.join("ExchangePlans"))
        });
        s.spawn(|_| {
            *business_processes.lock().unwrap() =
                load_business_processes_parallel(&path.join("BusinessProcesses"))
        });
        s.spawn(|_| *enums.lock().unwrap() = load_enums_parallel(&path.join("Enums")));
        s.spawn(|_| *tasks.lock().unwrap() = load_tasks_parallel(&path.join("Tasks")));
        s.spawn(|_| {
            *charts_accounts.lock().unwrap() =
                load_charts_of_accounts_parallel(&path.join("ChartsOfAccounts"))
        });
        s.spawn(|_| {
            *charts_calc_types.lock().unwrap() = load_simple_metadata_objects_parallel(
                &path.join("ChartsOfCalculationTypes"),
                MdoType::ChartOfCalculationTypes,
            )
        });
        s.spawn(|_| {
            *external_data_sources.lock().unwrap() = load_simple_metadata_objects_parallel(
                &path.join("ExternalDataSources"),
                MdoType::ExternalDataSource,
            )
        });
        s.spawn(|_| {
            *http_services.lock().unwrap() = load_http_services_parallel(&path.join("HTTPServices"))
        });
        s.spawn(|_| {
            *web_services.lock().unwrap() = load_web_services_parallel(&path.join("WebServices"))
        });
    });

    LoadedMetadata {
        common_modules: common_modules.into_inner().unwrap(),
        catalogs: catalogs.into_inner().unwrap(),
        documents: documents.into_inner().unwrap(),
        info_registers: info_registers.into_inner().unwrap(),
        accum_registers: accum_registers.into_inner().unwrap(),
        account_registers: account_registers.into_inner().unwrap(),
        calc_registers: calc_registers.into_inner().unwrap(),
        event_subscriptions: event_subscriptions.into_inner().unwrap(),
        scheduled_jobs: scheduled_jobs.into_inner().unwrap(),
        roles: roles.into_inner().unwrap(),
        defined_types: defined_types.into_inner().unwrap(),
        charts_char_types: charts_char_types.into_inner().unwrap(),
        constants: constants.into_inner().unwrap(),
        exchange_plans: exchange_plans.into_inner().unwrap(),
        business_processes: business_processes.into_inner().unwrap(),
        enums: enums.into_inner().unwrap(),
        tasks: tasks.into_inner().unwrap(),
        charts_accounts: charts_accounts.into_inner().unwrap(),
        charts_calc_types: charts_calc_types.into_inner().unwrap(),
        external_data_sources: external_data_sources.into_inner().unwrap(),
        http_services: http_services.into_inner().unwrap(),
        web_services: web_services.into_inner().unwrap(),
    }
}

/// Build Configuration from loaded metadata.
fn build_configuration(loaded: LoadedMetadata) -> Configuration {
    let mut config = Configuration::new("Configuration");

    for module in loaded.common_modules {
        config.add_common_module(module);
    }
    for obj in loaded.catalogs {
        config.add_metadata_object(obj);
    }
    for obj in loaded.documents {
        config.add_metadata_object(obj);
    }
    for reg in loaded.info_registers {
        config.add_register(reg);
    }
    for reg in loaded.accum_registers {
        config.add_register(reg);
    }
    for reg in loaded.account_registers {
        config.add_register(reg);
    }
    for reg in loaded.calc_registers {
        config.add_register(reg);
    }
    for sub in loaded.event_subscriptions {
        config.add_event_subscription(sub);
    }
    for job in loaded.scheduled_jobs {
        config.add_scheduled_job(job);
    }
    for role in loaded.roles {
        config.add_role(role);
    }
    for dt in loaded.defined_types {
        config.add_defined_type(dt);
    }
    for obj in loaded.charts_char_types {
        config.add_metadata_object(obj);
    }
    for obj in loaded.constants {
        config.add_metadata_object(obj);
    }
    for obj in loaded.exchange_plans {
        config.add_metadata_object(obj);
    }
    for obj in loaded.business_processes {
        config.add_metadata_object(obj);
    }
    for obj in loaded.enums {
        config.add_metadata_object(obj);
    }
    for obj in loaded.tasks {
        config.add_metadata_object(obj);
    }
    for obj in loaded.charts_accounts {
        config.add_metadata_object(obj);
    }
    for obj in loaded.charts_calc_types {
        config.add_metadata_object(obj);
    }
    for obj in loaded.external_data_sources {
        config.add_metadata_object(obj);
    }
    for svc in loaded.http_services {
        config.add_http_service(svc);
    }
    for svc in loaded.web_services {
        config.add_web_service(svc);
    }

    config
}

// ============================================================================
// Parallel loading functions
// ============================================================================

/// Load CommonModules in parallel, returning a Vec instead of mutating config.
fn load_common_modules_parallel(dir: &Path) -> Vec<crate::common_module::CommonModule> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let module_dir = entry.path();
            if !module_dir.is_dir() {
                return None;
            }

            let name = module_dir.file_name()?.to_str()?;
            let xml_path = dir.join(format!("{}.xml", name));
            let module_bsl_path = module_dir.join("Ext/Module.bsl");
            let module_bin_path = module_dir.join("Ext/Module.bin");

            if !xml_path.exists() {
                return None;
            }

            let xml = fs::read_to_string(&xml_path).ok()?;
            let mut module = xml_parser::parse_common_module_xml(&xml).ok()?;

            let is_protected = module_bin_path.exists() && !module_bsl_path.exists();

            if module_bsl_path.exists() {
                let uri = format!("CommonModules/{}/Ext/Module.bsl", name);
                module = crate::common_module::CommonModule::builder()
                    .uuid(*module.uuid())
                    .name(module.name())
                    .uri(Some(uri))
                    .server(module.is_server())
                    .global(module.is_global())
                    .client_managed_application(module.is_client_managed_application())
                    .client_ordinary_application(module.is_client_ordinary_application())
                    .external_connection(module.is_external_connection())
                    .server_call(module.is_server_call())
                    .privileged(module.is_privileged())
                    .return_values_reuse(module.return_values_reuse())
                    .protected(false)
                    .build();
            } else if is_protected {
                module = crate::common_module::CommonModule::builder()
                    .uuid(*module.uuid())
                    .name(module.name())
                    .uri(None::<String>)
                    .server(module.is_server())
                    .global(module.is_global())
                    .client_managed_application(module.is_client_managed_application())
                    .client_ordinary_application(module.is_client_ordinary_application())
                    .external_connection(module.is_external_connection())
                    .server_call(module.is_server_call())
                    .privileged(module.is_privileged())
                    .return_values_reuse(module.return_values_reuse())
                    .protected(true)
                    .build();
            }

            Some(module)
        })
        .collect()
}

/// Load Catalogs in parallel.
fn load_catalogs_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_catalog_xml)
}

/// Load Documents in parallel.
fn load_documents_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_document_xml)
}

/// Load BusinessProcesses in parallel.
fn load_business_processes_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_business_process_xml)
}

/// Load Tasks in parallel.
fn load_tasks_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_task_xml)
}

/// Load ExchangePlans in parallel.
fn load_exchange_plans_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_exchange_plan_xml)
}

/// Load ChartsOfCharacteristicTypes in parallel.
fn load_charts_of_characteristic_types_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_chart_of_characteristic_types_xml)
}

/// Load ChartsOfAccounts in parallel.
fn load_charts_of_accounts_parallel(dir: &Path) -> Vec<MetadataObject> {
    load_metadata_objects_parallel(dir, xml_parser::parse_chart_of_accounts_xml)
}

/// Generic parallel loader for metadata objects.
///
/// Loads metadata from XML files. Supports two cases:
/// 1. Directory + XML file (e.g., `Catalogs/Номенклатура/` + `Catalogs/Номенклатура.xml`)
/// 2. XML file only (e.g., `Catalogs/ПоставляемыеДополнительныеОтчетыИОбработки.xml` without directory)
///
/// Some metadata objects (catalogs, documents without forms/modules) may only have XML files.
fn load_metadata_objects_parallel<F>(dir: &Path, parser: F) -> Vec<MetadataObject>
where
    F: Fn(&str) -> Result<MetadataObject> + Sync,
{
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    // Collect names of directories (to avoid duplicates when processing XML files)
    let dir_names: std::collections::HashSet<String> = entries
        .iter()
        .filter_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                path.file_name()?.to_str().map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();

            if path.is_dir() {
                // Case 1: Directory exists, look for corresponding XML file
                let name = path.file_name()?.to_str()?;
                let xml_path = dir.join(format!("{}.xml", name));

                if !xml_path.exists() {
                    return None;
                }

                let xml = fs::read_to_string(&xml_path).ok()?;
                let mut mdo = parser(&xml).ok()?;

                // Load predefined items from Ext/Predefined.xml if exists
                let predefined_path = path.join("Ext").join("Predefined.xml");
                if predefined_path.exists() {
                    if let Ok(predefined_xml) = fs::read_to_string(&predefined_path) {
                        mdo.predefined_items = xml_parser::parse_predefined_xml(&predefined_xml);
                        tracing::debug!(
                            name = %name,
                            count = mdo.predefined_items.len(),
                            "Loaded predefined items"
                        );
                    }
                }

                Some(mdo)
            } else if path.extension().and_then(|e| e.to_str()) == Some("xml") {
                // Case 2: XML file without directory
                let file_stem = path.file_stem()?.to_str()?;

                // Skip if there's already a directory with this name (already processed above)
                if dir_names.contains(file_stem) {
                    return None;
                }

                let xml = fs::read_to_string(&path).ok()?;
                parser(&xml).ok()
            } else {
                None
            }
        })
        .collect()
}

/// Load Enums in parallel.
fn load_enums_parallel(dir: &Path) -> Vec<MetadataObject> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            xml_parser::parse_enum_xml(&xml).ok()
        })
        .collect()
}

/// Load Constants in parallel.
fn load_constants_parallel(dir: &Path) -> Vec<MetadataObject> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            xml_parser::parse_constant_xml(&xml).ok()
        })
        .collect()
}

/// Load registers in parallel (generic for all register types).
fn load_registers_parallel<F>(dir: &Path, parser: F) -> Vec<crate::register::Register>
where
    F: Fn(&str) -> Result<crate::register::Register> + Sync,
{
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            parser(&xml).ok()
        })
        .collect()
}

fn load_information_registers_parallel(dir: &Path) -> Vec<crate::register::Register> {
    load_registers_parallel(dir, xml_parser::parse_information_register_xml)
}

fn load_accumulation_registers_parallel(dir: &Path) -> Vec<crate::register::Register> {
    load_registers_parallel(dir, xml_parser::parse_accumulation_register_xml)
}

fn load_accounting_registers_parallel(dir: &Path) -> Vec<crate::register::Register> {
    load_registers_parallel(dir, xml_parser::parse_accounting_register_xml)
}

fn load_calculation_registers_parallel(dir: &Path) -> Vec<crate::register::Register> {
    load_registers_parallel(dir, xml_parser::parse_calculation_register_xml)
}

/// Load EventSubscriptions in parallel.
fn load_event_subscriptions_parallel(
    dir: &Path,
) -> Vec<crate::event_subscription::EventSubscription> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            xml_parser::parse_event_subscription_xml(&xml).ok()
        })
        .collect()
}

/// Load ScheduledJobs in parallel.
fn load_scheduled_jobs_parallel(dir: &Path) -> Vec<crate::scheduled_job::ScheduledJob> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            xml_parser::parse_scheduled_job_xml(&xml).ok()
        })
        .collect()
}

/// Load Roles in parallel.
fn load_roles_parallel(dir: &Path) -> Vec<crate::role::Role> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let name = path.file_stem()?.to_str()?;
            let xml = fs::read_to_string(&path).ok()?;
            let mut role = xml_parser::parse_role_xml(&xml).ok()?;

            // Try to load Rights.xml
            let rights_path = dir.join(name).join("Ext").join("Rights.xml");
            if rights_path.exists() {
                if let Ok(rights_xml) = fs::read_to_string(&rights_path) {
                    if let Ok(rights_data) = xml_parser::parse_rights_xml(&rights_xml) {
                        role = crate::role::Role::with_data(
                            *role.uuid(),
                            role.name().to_string(),
                            rights_data,
                        );
                    }
                }
            }

            Some(role)
        })
        .collect()
}

/// Load DefinedTypes in parallel.
fn load_defined_types_parallel(dir: &Path) -> Vec<crate::defined_type::DefinedType> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("xml") {
                return None;
            }

            let xml = fs::read_to_string(&path).ok()?;
            xml_parser::parse_defined_type_xml(&xml).ok()
        })
        .collect()
}

/// Load HTTPServices in parallel.
fn load_http_services_parallel(dir: &Path) -> Vec<crate::http_service::HTTPService> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let service_dir = entry.path();
            if !service_dir.is_dir() {
                return None;
            }

            let name = service_dir.file_name()?.to_str()?;
            let xml_path = dir.join(format!("{}.xml", name));

            if !xml_path.exists() {
                return None;
            }

            let xml = fs::read_to_string(&xml_path).ok()?;
            xml_parser::parse_http_service_xml(&xml, name).ok()
        })
        .collect()
}

/// Load WebServices in parallel.
fn load_web_services_parallel(dir: &Path) -> Vec<crate::web_service::WebService> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let service_dir = entry.path();
            if !service_dir.is_dir() {
                return None;
            }

            let name = service_dir.file_name()?.to_str()?;
            let xml_path = dir.join(format!("{}.xml", name));

            if !xml_path.exists() {
                return None;
            }

            let xml = fs::read_to_string(&xml_path).ok()?;
            xml_parser::parse_web_service_xml(&xml, name).ok()
        })
        .collect()
}

/// Load simple metadata objects in parallel.
fn load_simple_metadata_objects_parallel(dir: &Path, mdo_type: MdoType) -> Vec<MetadataObject> {
    if !dir.exists() {
        return Vec::new();
    }

    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let obj_dir = entry.path();
            if !obj_dir.is_dir() {
                return None;
            }

            let name = obj_dir.file_name()?.to_str()?;
            let xml_path = dir.join(format!("{}.xml", name));

            if !xml_path.exists() {
                return None;
            }

            Some(MetadataObject::new(mdo_type, name))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Module;

    #[test]
    fn test_load_from_directory() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/designer");
        let config = load_from_directory(path).unwrap();

        // Should load common modules (3 with .bsl + 1 protected with .bin)
        assert!(!config.common_modules().is_empty(), "No common modules loaded");
        assert_eq!(config.common_modules().len(), 4, "Expected 4 common modules");

        // Check specific modules exist
        let global_server = config.find_common_module("ГлобальныйСерверныйМодуль");
        assert!(global_server.is_some(), "ГлобальныйСерверныйМодуль not found");
        let module = global_server.unwrap();
        assert!(module.is_server(), "Should be server module");
        assert!(module.is_global(), "Should be global module");
        assert!(module.uri().is_some(), "Should have URI");
        assert_eq!(module.uri().unwrap(), "CommonModules/ГлобальныйСерверныйМодуль/Ext/Module.bsl");

        // Check registers loaded
        assert!(!config.registers().is_empty(), "No registers loaded");

        // Check InformationRegisters loaded as full Register objects
        let register = config.find_register("РегистрСведений1");
        if let Some(reg) = register {
            assert!(reg.is_information_register(), "Should be InformationRegister");
            assert_eq!(reg.dimensions().len(), 1, "Should have 1 dimension");
            assert_eq!(reg.dimensions()[0].name(), "Справочник1", "Dimension name should match");
            assert!(
                !reg.dimensions()[0].is_deny_incomplete_values(),
                "DenyIncompleteValues should be false"
            );
        }

        // Check Catalogs and Documents loaded as metadata objects
        assert!(!config.metadata_objects().is_empty(), "No metadata objects loaded");

        // Check that Catalog has attributes loaded
        let catalog = config.metadata_objects().iter().find(|obj| {
            obj.mdo_type == crate::metadata_object::MdoType::Catalog && obj.name == "Справочник1"
        });

        if let Some(cat) = catalog {
            // Should have 3 custom attributes + standard attributes
            assert!(
                cat.attributes.len() >= 3,
                "Expected at least 3 custom attributes in Справочник1"
            );

            assert!(cat.find_attribute("Реквизит1").is_some(), "Expected Реквизит1");
            assert!(cat.find_attribute("Реквизит2").is_some(), "Expected Реквизит2");
            assert!(cat.find_attribute("Реквизит3").is_some(), "Expected Реквизит3");

            let attr1 = cat.find_attribute("Реквизит1").unwrap();
            assert!(
                matches!(attr1.attr_type, crate::metadata_object::AttributeType::String { .. }),
                "Реквизит1 should be String type"
            );

            assert_eq!(cat.tabular_sections.len(), 1, "Expected 1 tabular section");
            let ts = &cat.tabular_sections[0];
            assert_eq!(ts.name(), "ТабличнаяЧасть1");
        }
    }

    #[test]
    fn test_load_protected_module() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/designer");
        let config = load_from_directory(path).unwrap();

        // Check protected module is loaded
        let protected_module = config.find_common_module("ЗащищенныйМодуль");
        assert!(protected_module.is_some(), "ЗащищенныйМодуль not found");

        let module = protected_module.unwrap();
        assert!(module.is_protected(), "Module should be protected");
        assert!(module.uri().is_none(), "Protected module should not have URI");
        assert!(module.is_server(), "Should be server module");
        assert!(module.is_server_call(), "Should have server call");
    }

    /// Test loading enum values from doc3 project
    #[test]
    #[ignore] // Only run when doc3 project is available
    fn test_load_enum_values_from_doc3() {
        let doc3_path = concat!(env!("HOME"), "/src/doc3/src/cf");

        if !std::path::Path::new(doc3_path).exists() {
            eprintln!("Skipping test: doc3 project not found at {}", doc3_path);
            return;
        }

        let config = load_from_directory(doc3_path).expect("Failed to load doc3 configuration");

        // Find Enums
        let enums: Vec<_> = config
            .metadata_objects()
            .iter()
            .filter(|obj| obj.mdo_type == crate::metadata_object::MdoType::Enum)
            .collect();

        println!("Total Enums loaded: {}", enums.len());
        assert!(!enums.is_empty(), "No enums loaded");

        // Print first 10 enums
        println!("\nFirst 10 Enums:");
        for (i, enum_obj) in enums.iter().take(10).enumerate() {
            println!("  {}: {} (values: {})", i + 1, enum_obj.name, enum_obj.enum_values.len());
        }

        // Check for specific enum
        let target_name = "СпособыУстановкиКурсаВалюты";
        let target_enum_specific = enums.iter().find(|e| e.name == target_name);

        if let Some(enum_obj) = target_enum_specific {
            println!("\n✅ Found target enum: {}", target_name);
            println!("  EnumValues count: {}", enum_obj.enum_values.len());
            for (i, ev) in enum_obj.enum_values.iter().enumerate() {
                println!("    {}: {}", i + 1, ev.name);
            }
        } else {
            println!("\n❌ Target enum '{}' NOT FOUND", target_name);
            println!("\nAll enum names:");
            for (i, e) in enums.iter().enumerate() {
                println!("  {}: {}", i + 1, e.name);
            }
        }

        // Find specific enum - use first one with values
        let target_enum = enums.iter().find(|e| !e.enum_values.is_empty());

        if let Some(enum_obj) = target_enum {
            println!("✅ Found enum: {}", enum_obj.name);
            println!("  EnumValues count: {}", enum_obj.enum_values.len());

            // Check that enum values are loaded
            assert!(!enum_obj.enum_values.is_empty(), "EnumValues should not be empty");

            // Print first 5 enum values
            println!("  First 5 EnumValues:");
            for (i, ev) in enum_obj.enum_values.iter().take(5).enumerate() {
                println!("    {}: {} (uuid: {})", i + 1, ev.name, ev.uuid);
            }

            // Test find_enum_value method
            if let Some(first_value) = enum_obj.enum_values.first() {
                let found = enum_obj.find_enum_value(&first_value.name);
                assert!(found.is_some(), "find_enum_value should work");

                // Test case-insensitive search
                let found_lower = enum_obj.find_enum_value(&first_value.name.to_lowercase());
                assert!(found_lower.is_some(), "find_enum_value should be case-insensitive");
            }
        } else {
            panic!("❌ Enum 'ЗаданияОчередиОбновленияПрав' not found");
        }
    }

    /// Test loading from doc3 project (only run when doc3 is available)
    #[test]
    #[ignore] // Only run when doc3 project is available
    fn test_load_from_doc3_project() {
        let doc3_path = concat!(env!("HOME"), "/src/doc3/src/cf");

        if !std::path::Path::new(doc3_path).exists() {
            eprintln!("Skipping test: doc3 project not found at {}", doc3_path);
            return;
        }

        let config = load_from_directory(doc3_path).expect("Failed to load doc3 configuration");

        println!("Total registers loaded: {}", config.registers().len());

        // Find InformationRegisters
        let info_registers: Vec<_> =
            config.registers().iter().filter(|r| r.is_information_register()).collect();

        println!("InformationRegisters count: {}", info_registers.len());

        // List first 20 registers to see what's loaded
        println!("\nFirst 20 InformationRegisters:");
        for (i, reg) in info_registers.iter().take(20).enumerate() {
            println!("  {}: {}", i + 1, reg.name());
        }

        // Search for registers containing "Значения" or "Действий" or "Писем"
        println!("\nRegisters containing 'Значения':");
        for reg in info_registers.iter() {
            if reg.name().contains("Значения") {
                println!("  - {}", reg.name());
            }
        }

        println!("\nRegisters containing 'Действий':");
        for reg in info_registers.iter() {
            if reg.name().contains("Действий") {
                println!("  - {}", reg.name());
            }
        }

        println!("\nRegisters containing 'Писем':");
        for reg in info_registers.iter() {
            if reg.name().contains("Писем") {
                println!("  - {}", reg.name());
            }
        }

        // Look for the specific register user asked about
        let target_register = config.find_register("ЗначенияДействийПриОбработкеПисем");

        if let Some(register) = target_register {
            println!("✅ Found register: {}", register.name());
            println!("  Type: {:?}", register.mdo_type());
            println!("  Dimensions: {}", register.dimensions().len());
            println!("  Resources: {}", register.resources().len());
            println!("  Attributes: {}", register.attributes().len());

            for dim in register.dimensions() {
                println!("    Dimension: {}", dim.name());
            }

            for res in register.resources() {
                println!("    Resource: {} - Type: {:?}", res.name(), res.attr_type());
            }

            for attr in register.attributes() {
                println!("    Attribute: {} - Type: {:?}", attr.name(), attr.attr_type());
            }

            assert!(register.is_information_register(), "Should be InformationRegister");
        } else {
            panic!("❌ Register 'ЗначенияДействийПриОбработкеПисем' not found in loaded configuration!");
        }
    }

    /// Test to verify that catalogs without directories (XML-only) are loaded correctly.
    ///
    /// Some metadata objects like catalogs without forms/modules exist only as XML files.
    /// The loader must handle this case.
    #[test]
    #[ignore] // Only run when doc3 project is available
    fn test_catalog_xml_only_without_directory() {
        let doc3_path = concat!(env!("HOME"), "/src/doc3/src/cf");

        if !std::path::Path::new(doc3_path).exists() {
            eprintln!("Skipping test: doc3 project not found at {}", doc3_path);
            return;
        }

        let config = load_from_directory(doc3_path).expect("Failed to load doc3 configuration");

        // This catalog exists only as XML file without a subdirectory
        let catalog_name = "ПоставляемыеДополнительныеОтчетыИОбработки";

        // Verify XML exists but directory doesn't
        let xml_path = format!("{}/Catalogs/{}.xml", doc3_path, catalog_name);
        let dir_path = format!("{}/Catalogs/{}", doc3_path, catalog_name);
        assert!(std::path::Path::new(&xml_path).exists(), "XML file should exist");
        assert!(
            !std::path::Path::new(&dir_path).exists(),
            "Directory should NOT exist (this is the test case)"
        );

        // Catalog should be loaded from XML file only
        let exists =
            config.has_metadata_object(crate::metadata_object::MdoType::Catalog, catalog_name);
        assert!(exists, "Catalog '{}' should be loaded from XML-only file", catalog_name);
    }
}
