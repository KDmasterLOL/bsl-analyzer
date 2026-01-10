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

    // Load DefinedTypes
    load_defined_types(&path.join("DefinedTypes"), &mut config)?;

    // Load other metadata types (simplified - name only, for SDBL completion)
    load_simple_metadata_objects(
        &path.join("ChartsOfCharacteristicTypes"),
        &mut config,
        MdoType::ChartOfCharacteristicTypes,
    )?;
    load_simple_metadata_objects(&path.join("ExchangePlans"), &mut config, MdoType::ExchangePlan)?;
    load_business_processes(&path.join("BusinessProcesses"), &mut config)?;
    load_simple_metadata_objects(&path.join("Enums"), &mut config, MdoType::Enum)?;
    load_simple_metadata_objects(&path.join("Tasks"), &mut config, MdoType::Task)?;
    load_simple_metadata_objects(
        &path.join("ChartsOfAccounts"),
        &mut config,
        MdoType::ChartOfAccounts,
    )?;
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

    tracing::info!(
        common_modules = config.common_modules().len(),
        metadata_objects = config.metadata_objects().len(),
        registers = config.registers().len(),
        event_subscriptions = config.event_subscriptions().len(),
        defined_types = config.defined_types().len(),
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

                if xml_path.exists() {
                    // Parse XML to get properties
                    let xml = fs::read_to_string(&xml_path)?;
                    let mut module = xml_parser::parse_common_module_xml(&xml)?;

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
                            .build();
                    }

                    tracing::debug!(
                        module = %module.name(),
                        has_code = module_bsl_path.exists(),
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
                        "loaded business process"
                    );

                    config.add_metadata_object(business_process);
                }
            }
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

        // Should load common modules
        assert!(!config.common_modules().is_empty(), "No common modules loaded");
        assert_eq!(config.common_modules().len(), 3, "Expected 3 common modules");

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
            assert_eq!(cat.attributes.len(), 3, "Expected 3 attributes in Справочник1");

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
