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

    // Load other metadata types (simplified - name only, for SDBL completion)
    load_simple_metadata_objects(
        &path.join("ChartsOfCharacteristicTypes"),
        &mut config,
        MdoType::ChartOfCharacteristicTypes,
    )?;
    load_simple_metadata_objects(&path.join("ExchangePlans"), &mut config, MdoType::ExchangePlan)?;
    load_simple_metadata_objects(
        &path.join("BusinessProcesses"),
        &mut config,
        MdoType::BusinessProcess,
    )?;
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
/// - XML: `<RegisterType>/<Name>.xml` (NEXT TO folder)
/// - Code: `<RegisterType>/<Name>/Ext/ManagerModule.bsl` (inside Ext/)
fn load_registers<F>(dir: &Path, config: &mut Configuration, parser: F) -> Result<()>
where
    F: Fn(&str) -> Result<crate::register::Register>,
{
    let _span = tracing::debug_span!("load_registers", ?dir).entered();

    if !dir.exists() {
        tracing::debug!("directory does not exist, skipping");
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let register_dir = entry.path();

        // Look for directories
        if register_dir.is_dir() {
            if let Some(name) = register_dir.file_name().and_then(|n| n.to_str()) {
                // XML is NEXT TO folder: <RegisterType>/<Name>.xml
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
}
