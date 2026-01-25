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

    let mut config = Configuration::new("Configuration");

    // Load CommonModules
    load_common_modules(&path.join("CommonModules"), &mut config)?;

    // Load Catalogs
    load_catalogs(&path.join("Catalogs"), &mut config)?;

    // Load Documents
    load_documents(&path.join("Documents"), &mut config)?;

    // Load all 4 register types
    load_information_registers(&path.join("InformationRegisters"), &mut config)?;
    load_accumulation_registers(&path.join("AccumulationRegisters"), &mut config)?;
    load_accounting_registers(&path.join("AccountingRegisters"), &mut config)?;
    load_calculation_registers(&path.join("CalculationRegisters"), &mut config)?;

    // Load EventSubscriptions
    load_event_subscriptions(&path.join("EventSubscriptions"), &mut config)?;

    // Load ScheduledJobs
    load_scheduled_jobs(&path.join("ScheduledJobs"), &mut config)?;

    // Load Roles
    load_roles(&path.join("Roles"), &mut config)?;

    // Load DefinedTypes
    load_defined_types(&path.join("DefinedTypes"), &mut config)?;

    // Load ChartsOfCharacteristicTypes (with full parsing for tabular sections)
    load_charts_of_characteristic_types(&path.join("ChartsOfCharacteristicTypes"), &mut config)?;

    // Load Constants
    load_constants(&path.join("Constants"), &mut config)?;

    // Load other metadata types
    load_exchange_plans(&path.join("ExchangePlans"), &mut config)?;
    load_business_processes(&path.join("BusinessProcesses"), &mut config)?;
    load_enums(&path.join("Enums"), &mut config)?;
    load_tasks(&path.join("Tasks"), &mut config)?;
    load_charts_of_accounts(&path.join("ChartsOfAccounts"), &mut config)?;
    load_simple_metadata_objects(
        &path.join("ChartsOfCalculationTypes"),
        &mut config,
        MdoType::ChartOfCalculationTypes,
    )?;
    load_simple_metadata_objects(
        &path.join("ExternalDataSources"),
        &mut config,
        MdoType::ExternalDataSource,
    )?;

    // Load HTTPServices
    load_http_services(&path.join("HTTPServices"), &mut config)?;

    // Load WebServices (SOAP)
    load_web_services(&path.join("WebServices"), &mut config)?;

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

/// Load CommonModules from directory
///
/// Designer format structure:
/// - XML: `CommonModules/<Name>.xml` (рядом с папкой!)
/// - Code: `CommonModules/<Name>/Ext/Module.bsl` (внутри Ext/)
fn load_common_modules(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_common_modules", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let module_dir = entry.path();

        // Look for directories
        if module_dir.is_dir() {
            if let Some(name) = module_dir.file_name().and_then(|n| n.to_str()) {
                // Designer format structure:
                // - XML: CommonModules/<Name>.xml (NEXT TO folder)
                // - Code: CommonModules/<Name>/Ext/Module.bsl (inside Ext/)

                let xml_path = dir.join(format!("{}.xml", name));
                let module_bsl_path = module_dir.join("Ext/Module.bsl");
                let module_bin_path = module_dir.join("Ext/Module.bin");

                if xml_path.exists() {
                    // Parse XML to get properties
                    let xml = fs::read_to_string(&xml_path)?;
                    let mut module = xml_parser::parse_common_module_xml(&xml)?;

                    // Module is protected if .bin exists but .bsl does not
                    let is_protected = module_bin_path.exists() && !module_bsl_path.exists();

                    // Build URI to .bsl file if it exists
                    if module_bsl_path.exists() {
                        // URI relative to configuration root
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
                        // Protected module - has .bin without .bsl
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

                    tracing::debug!(
                        module = %module.name(),
                        has_code = module_bsl_path.exists(),
                        is_protected = is_protected,
                        "loaded common module"
                    );

                    config.add_common_module(module);
                } else {
                    tracing::warn!(
                        name = %name,
                        "CommonModule directory found but no XML file"
                    );
                }
            }
        }
    }

    Ok(())
}

/// Load Catalogs from directory
///
/// Designer format structure:
/// - XML: `Catalogs/<Name>.xml` (рядом с папкой!)
/// - Code: `Catalogs/<Name>/Ext/ManagerModule.bsl` and `ObjectModule.bsl` (внутри Ext/)
fn load_catalogs(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_catalogs", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let catalog_dir = entry.path();

        // Look for directories
        if catalog_dir.is_dir() {
            if let Some(name) = catalog_dir.file_name().and_then(|n| n.to_str()) {
                let xml_path = dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    // Parse XML to get catalog with attributes
                    let xml = fs::read_to_string(&xml_path)?;
                    let catalog = xml_parser::parse_catalog_xml(&xml)?;

                    tracing::debug!(
                        catalog = %name,
                        attributes = catalog.attributes.len(),
                        "loaded catalog"
                    );

                    config.add_metadata_object(catalog);
                }
            }
        }
    }

    Ok(())
}

/// Load Documents from directory
///
/// Designer format structure:
/// - XML: `Documents/<Name>.xml` (рядом с папкой!)
/// - Code: `Documents/<Name>/Ext/ManagerModule.bsl` and `ObjectModule.bsl` (внутри Ext/)
fn load_documents(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_documents", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let document_dir = entry.path();

        // Look for directories
        if document_dir.is_dir() {
            if let Some(name) = document_dir.file_name().and_then(|n| n.to_str()) {
                let xml_path = dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    // Parse XML to get document with attributes
                    let xml = fs::read_to_string(&xml_path)?;
                    let document = xml_parser::parse_document_xml(&xml)?;

                    tracing::debug!(
                        document = %name,
                        attributes = document.attributes.len(),
                        "loaded document"
                    );

                    config.add_metadata_object(document);
                }
            }
        }
    }

    Ok(())
}

/// Load BusinessProcesses from directory
fn load_business_processes(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_business_processes", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let business_process_dir = entry.path();

        // Look for directories
        if business_process_dir.is_dir() {
            if let Some(name) = business_process_dir.file_name().and_then(|n| n.to_str()) {
                let xml_path = dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    // Parse XML to get business process with attributes
                    let xml = fs::read_to_string(&xml_path)?;
                    let business_process = xml_parser::parse_business_process_xml(&xml)?;

                    tracing::debug!(
                        business_process = %name,
                        attributes = business_process.attributes.len(),
                        "loaded business process with attributes"
                    );

                    config.add_metadata_object(business_process);
                }
            }
        }
    }

    Ok(())
}

/// Load Tasks from directory
fn load_tasks(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_tasks", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let task_dir = entry.path();

        // Look for directories
        if task_dir.is_dir() {
            if let Some(name) = task_dir.file_name().and_then(|n| n.to_str()) {
                let xml_path = dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    // Parse XML to get task with attributes and tabular sections
                    let xml = fs::read_to_string(&xml_path)?;
                    let task = xml_parser::parse_task_xml(&xml)?;

                    tracing::debug!(
                        task = %name,
                        attributes = task.attributes.len(),
                        tabular_sections = task.tabular_sections.len(),
                        "loaded task"
                    );

                    config.add_metadata_object(task);
                }
            }
        }
    }

    Ok(())
}

/// Load ExchangePlans from directory
fn load_exchange_plans(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_exchange_plans", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let exchange_plan_dir = entry.path();

        // Look for directories
        if exchange_plan_dir.is_dir() {
            if let Some(name) = exchange_plan_dir.file_name().and_then(|n| n.to_str()) {
                let xml_path = dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    // Parse XML to get exchange plan with attributes and tabular sections
                    let xml = fs::read_to_string(&xml_path)?;
                    let exchange_plan = xml_parser::parse_exchange_plan_xml(&xml)?;

                    tracing::debug!(
                        exchange_plan = %name,
                        attributes = exchange_plan.attributes.len(),
                        tabular_sections = exchange_plan.tabular_sections.len(),
                        "loaded exchange plan"
                    );

                    config.add_metadata_object(exchange_plan);
                }
            }
        }
    }

    Ok(())
}

/// Load Enums from directory with EnumValue elements
///
/// Designer format structure:
/// - XML: `Enums/<Name>.xml` (next to folder)
/// - Folder: `Enums/<Name>/` (may exist but has no code files for Enums)
fn load_enums(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_enums", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    // Collect all XML files to avoid duplicates
    // (some enums have both directory and XML file with same name)
    let mut processed = std::collections::HashSet::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Only process XML files
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("xml") {
            if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                // Skip if already processed
                if !processed.insert(name.to_string()) {
                    continue;
                }

                let xml = fs::read_to_string(&path)?;
                let enum_obj = xml_parser::parse_enum_xml(&xml)?;

                tracing::debug!(
                    enum_name = %name,
                    enum_values = enum_obj.enum_values.len(),
                    "loaded enum"
                );

                config.add_metadata_object(enum_obj);
            }
        }
    }

    Ok(())
}

/// Load ChartsOfCharacteristicTypes from directory
fn load_charts_of_characteristic_types(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_charts_of_characteristic_types", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let chart_dir = entry.path();

        // Look for directories
        if chart_dir.is_dir() {
            if let Some(name) = chart_dir.file_name().and_then(|n| n.to_str()) {
                let xml_path = dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    // Parse XML to get chart with attributes and tabular sections
                    let xml = fs::read_to_string(&xml_path)?;
                    let chart = xml_parser::parse_chart_of_characteristic_types_xml(&xml)?;

                    tracing::debug!(
                        chart = %name,
                        attributes = chart.attributes.len(),
                        tabular_sections = chart.tabular_sections.len(),
                        "loaded chart of characteristic types"
                    );

                    config.add_metadata_object(chart);
                }
            }
        }
    }

    Ok(())
}

/// Load ChartsOfAccounts from directory
fn load_charts_of_accounts(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_charts_of_accounts", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let chart_dir = entry.path();

        if chart_dir.is_dir() {
            if let Some(name) = chart_dir.file_name().and_then(|n| n.to_str()) {
                let xml_path = dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    let xml = fs::read_to_string(&xml_path)?;
                    let chart = xml_parser::parse_chart_of_accounts_xml(&xml)?;

                    tracing::debug!(
                        chart = %name,
                        attributes = chart.attributes.len(),
                        check_unique = chart.check_unique,
                        code_series = ?chart.code_series,
                        "loaded chart of accounts"
                    );

                    config.add_metadata_object(chart);
                }
            }
        }
    }

    Ok(())
}

/// Load Constants from directory
///
/// Constants are stored as individual XML files directly in the Constants folder,
/// without subdirectories.
fn load_constants(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_constants", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Constants are stored as XML files directly (no subdirectories)
        if path.is_file() && path.extension().is_some_and(|ext| ext == "xml") {
            let xml = fs::read_to_string(&path)?;
            let constant = xml_parser::parse_constant_xml(&xml)?;

            tracing::debug!(
                constant = %constant.name,
                "loaded constant"
            );

            config.add_metadata_object(constant);
        }
    }

    Ok(())
}

/// Load InformationRegisters from directory
fn load_information_registers(dir: &Path, config: &mut Configuration) -> Result<()> {
    load_registers(dir, config, xml_parser::parse_information_register_xml)
}

/// Load AccumulationRegisters from directory
fn load_accumulation_registers(dir: &Path, config: &mut Configuration) -> Result<()> {
    load_registers(dir, config, xml_parser::parse_accumulation_register_xml)
}

/// Load AccountingRegisters from directory
fn load_accounting_registers(dir: &Path, config: &mut Configuration) -> Result<()> {
    load_registers(dir, config, xml_parser::parse_accounting_register_xml)
}

/// Load CalculationRegisters from directory
fn load_calculation_registers(dir: &Path, config: &mut Configuration) -> Result<()> {
    load_registers(dir, config, xml_parser::parse_calculation_register_xml)
}

/// Generic register loader for all 4 register types
///
/// Designer format structure:
/// - XML: `<RegisterType>/<Name>.xml` (NEXT TO folder OR standalone)
/// - Code: `<RegisterType>/<Name>/Ext/ManagerModule.bsl` (inside Ext/, optional)
///
/// Note: Registers without code (no Ext/ folder) will only have XML files.
fn load_registers<F>(dir: &Path, config: &mut Configuration, parser: F) -> Result<()>
where
    F: Fn(&str) -> Result<crate::register::Register>,
{
    let _span = tracing::debug_span!("load_registers", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    // Collect all XML files (both with and without folders)
    let mut xml_files = std::collections::HashSet::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // If it's a .xml file, add it to the set
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("xml") {
            if let Some(name) = path.file_stem().and_then(|n| n.to_str()) {
                xml_files.insert(name.to_string());
            }
        }
    }

    // Load each register from its XML file
    for name in xml_files {
        let xml_path = dir.join(format!("{}.xml", name));

        if xml_path.exists() {
            let xml = fs::read_to_string(&xml_path)?;
            let register = parser(&xml)?;
            config.add_register(register);

            tracing::debug!(
                register = %name,
                "loaded register"
            );
        }
    }

    Ok(())
}

/// Load EventSubscriptions from directory
///
/// **CRITICAL:** EventSubscriptions have NO code files - only XML!
///
/// Designer format structure:
/// - XML: `EventSubscriptions/<Name>.xml` (NO folders, NO code files)
fn load_event_subscriptions(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_event_subscriptions", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Only process .xml files (EventSubscriptions have no code)
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("xml") {
            let xml = fs::read_to_string(&path)?;
            let subscription = xml_parser::parse_event_subscription_xml(&xml)?;

            tracing::debug!(
                subscription = %subscription.name(),
                handler = %subscription.handler_string(),
                "loaded event subscription"
            );

            config.add_event_subscription(subscription);
        }
    }

    Ok(())
}

/// Load ScheduledJobs from directory
///
/// **CRITICAL:** ScheduledJobs have NO code files - only XML!
///
/// Designer format structure:
/// - XML: `ScheduledJobs/<Name>.xml` (NO folders, NO code files)
fn load_scheduled_jobs(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_scheduled_jobs", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Only process .xml files (ScheduledJobs have no code)
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("xml") {
            let xml = fs::read_to_string(&path)?;
            let job = xml_parser::parse_scheduled_job_xml(&xml)?;

            tracing::debug!(
                job_name = %job.name(),
                method_name = %job.method_name(),
                predefined = job.is_predefined(),
                "loaded scheduled job"
            );

            config.add_scheduled_job(job);
        }
    }

    Ok(())
}

/// Load Roles from directory
///
/// **CRITICAL:** Roles have NO code files - only XML!
///
/// Designer format structure:
/// - XML: `Roles/<Name>.xml` - basic info (uuid, name)
/// - Rights: `Roles/<Name>/Ext/Rights.xml` - permissions data
fn load_roles(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_roles", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Only process .xml files (role definition)
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("xml") {
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                let xml = fs::read_to_string(&path)?;
                let mut role = xml_parser::parse_role_xml(&xml)?;

                // Try to load Rights.xml for this role
                let rights_path = dir.join(name).join("Ext").join("Rights.xml");
                if rights_path.exists() {
                    let rights_xml = fs::read_to_string(&rights_path)?;
                    if let Ok(rights_data) = xml_parser::parse_rights_xml(&rights_xml) {
                        role = crate::role::Role::with_data(
                            *role.uuid(),
                            role.name().to_string(),
                            rights_data,
                        );
                    }
                }

                tracing::debug!(
                    role_name = %role.name(),
                    set_for_new_objects = role.data().set_for_new_objects(),
                    "loaded role"
                );

                config.add_role(role);
            }
        }
    }

    Ok(())
}

/// Load DefinedTypes from directory
///
/// **CRITICAL:** DefinedTypes have NO code files - only XML!
///
/// Designer format structure:
/// - XML: `DefinedTypes/<Name>.xml` (NO folders, NO code files)
fn load_defined_types(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_defined_types", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Only process .xml files (DefinedTypes have no code)
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("xml") {
            let xml = fs::read_to_string(&path)?;
            let defined_type = xml_parser::parse_defined_type_xml(&xml)?;

            tracing::debug!(
                defined_type_name = %defined_type.name(),
                underlying_type = ?defined_type.underlying_type(),
                "loaded defined type"
            );

            config.add_defined_type(defined_type);
        }
    }

    Ok(())
}

/// Load HTTPServices from directory
///
/// Designer format structure:
/// - XML: `HTTPServices/<Name>.xml` (next to folder)
/// - Code: `HTTPServices/<Name>/Ext/Module.bsl` (inside Ext/)
fn load_http_services(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_http_services", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let service_dir = entry.path();

        // Look for directories (each HTTPService is a folder)
        if service_dir.is_dir() {
            if let Some(name) = service_dir.file_name().and_then(|n| n.to_str()) {
                // XML is next to the folder
                let xml_path = dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    let xml = fs::read_to_string(&xml_path)?;
                    let http_service = xml_parser::parse_http_service_xml(&xml, name)?;

                    tracing::debug!(
                        service_name = %http_service.name(),
                        root_url = %http_service.root_url(),
                        url_templates = http_service.url_templates().len(),
                        "loaded HTTP service"
                    );

                    config.add_http_service(http_service);
                }
            }
        }
    }

    Ok(())
}

/// Load WebServices (SOAP) from directory
///
/// Designer format structure:
/// - XML: `WebServices/<Name>.xml` (next to folder)
/// - Code: `WebServices/<Name>/Ext/Module.bsl` (inside Ext/)
fn load_web_services(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_web_services", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let service_dir = entry.path();

        // Look for directories (each WebService is a folder)
        if service_dir.is_dir() {
            if let Some(name) = service_dir.file_name().and_then(|n| n.to_str()) {
                // XML is next to the folder
                let xml_path = dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    let xml = fs::read_to_string(&xml_path)?;
                    let web_service = xml_parser::parse_web_service_xml(&xml, name)?;

                    tracing::debug!(
                        service_name = %web_service.name(),
                        namespace = %web_service.namespace(),
                        operations = web_service.operations().len(),
                        "loaded web service"
                    );

                    config.add_web_service(web_service);
                }
            }
        }
    }

    Ok(())
}

/// Load metadata objects of any type (simplified - name only)
///
/// This is a generic loader for metadata types that don't have full parsers yet.
/// It simply reads directory names and creates MetadataObject with name only.
/// This is sufficient for SDBL completion to work.
///
/// Designer format structure:
/// - XML: `<Type>/<Name>.xml` (next to folder)
/// - Folder: `<Type>/<Name>/` (may have Ext/ inside)
fn load_simple_metadata_objects(
    dir: &Path,
    config: &mut Configuration,
    mdo_type: MdoType,
) -> Result<()> {
    let _span = tracing::debug_span!("load_simple_metadata_objects", ?dir, ?mdo_type).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let object_dir = entry.path();

        // Look for directories
        if object_dir.is_dir() {
            if let Some(name) = object_dir.file_name().and_then(|n| n.to_str()) {
                let xml_path = dir.join(format!("{}.xml", name));

                // Only add if corresponding XML exists (standard Designer format)
                if xml_path.exists() {
                    let metadata_obj = MetadataObject::new(mdo_type, name);

                    tracing::debug!(
                        mdo_type = ?mdo_type,
                        name = %name,
                        "loaded metadata object (simplified)"
                    );

                    config.add_metadata_object(metadata_obj);
                }
            }
        }
    }

    Ok(())
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
}
