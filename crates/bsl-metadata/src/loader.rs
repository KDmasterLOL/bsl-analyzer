//! Metadata loader for Designer format
//!
//! Loads 1C:Enterprise metadata from Designer format directory structure.
//!
//! ## Designer Format Structure
//!
//! **CRITICAL:** XML files are INSIDE object folders, code files are inside Ext/ subdirectories:
//!
//! ```text
//! Configuration.xml                      # Root configuration
//! ConfigDumpInfo.xml                     # Dump information
//!
//! CommonModules/
//! ├── <Name>/                            # Object folder
//! │   ├── <Name>.xml                     # XML INSIDE folder!
//! │   └── Ext/
//! │       └── Module.bsl                 # Code inside Ext/!
//!
//! Catalogs/
//! ├── <Name>/                            # Object folder
//! │   ├── <Name>.xml                     # XML INSIDE folder!
//! │   └── Ext/
//! │       ├── ManagerModule.bsl          # Code inside Ext/!
//! │       └── ObjectModule.bsl
//!
//! InformationRegisters/
//! ├── <Name>/                            # Object folder
//! │   ├── <Name>.xml                     # XML INSIDE folder!
//! │   └── Ext/
//! │       └── ManagerModule.bsl          # Code inside Ext/!
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

    // Load InformationRegisters
    load_information_registers(&path.join("InformationRegisters"), &mut config)?;

    tracing::info!(
        common_modules = config.common_modules().len(),
        metadata_objects = config.metadata_objects().len(),
        "configuration loaded"
    );

    Ok(config)
}

/// Load CommonModules from directory
///
/// Designer format structure:
/// - XML: `CommonModules/<Name>/<Name>.xml` (ВНУТРИ папки!)
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
                // ПРАВИЛЬНАЯ СТРУКТУРА:
                // - XML: CommonModules/<Name>/<Name>.xml (внутри папки!)
                // - Код: CommonModules/<Name>/Ext/Module.bsl (внутри Ext/)

                let xml_path = module_dir.join(format!("{}.xml", name));
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
/// - XML: `Catalogs/<Name>/<Name>.xml` (ВНУТРИ папки!)
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
                let xml_path = catalog_dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    let obj = MetadataObject::new(MdoType::Catalog, name);
                    config.add_metadata_object(obj);

                    tracing::debug!(catalog = %name, "loaded catalog");
                }
            }
        }
    }

    Ok(())
}

/// Load Documents from directory
///
/// Designer format structure:
/// - XML: `Documents/<Name>/<Name>.xml` (ВНУТРИ папки!)
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
                let xml_path = document_dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    let obj = MetadataObject::new(MdoType::Document, name);
                    config.add_metadata_object(obj);

                    tracing::debug!(document = %name, "loaded document");
                }
            }
        }
    }

    Ok(())
}

/// Load InformationRegisters from directory
///
/// Designer format structure:
/// - XML: `InformationRegisters/<Name>/<Name>.xml` (ВНУТРИ папки!)
/// - Code: `InformationRegisters/<Name>/Ext/ManagerModule.bsl` (внутри Ext/)
fn load_information_registers(dir: &Path, config: &mut Configuration) -> Result<()> {
    let _span = tracing::debug_span!("load_information_registers", ?dir).entered();

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
                let xml_path = register_dir.join(format!("{}.xml", name));

                if xml_path.exists() {
                    let obj = MetadataObject::new(MdoType::InformationRegister, name);
                    config.add_metadata_object(obj);

                    tracing::debug!(register = %name, "loaded information register");
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

    // Note: These tests require test fixtures to be copied
    // See Step 1.4.1 in the plan

    #[test]
    #[ignore = "requires test fixtures"]
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

        // Check InformationRegisters loaded
        assert!(!config.metadata_objects().is_empty(), "No metadata objects loaded");
    }
}
