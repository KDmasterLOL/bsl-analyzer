//! Integration test for doc3 project metadata loading.

use ide::config_finder;
use std::path::PathBuf;

#[test]
#[ignore] // Requires doc3 project to be present
fn test_load_doc3_metadata() {
    let doc3_root = PathBuf::from(env!("HOME")).join("src/doc3");

    if !doc3_root.exists() {
        println!("doc3 project not found at {:?}, skipping test", doc3_root);
        return;
    }

    // Find configuration path
    let config_path = config_finder::find_configuration_path(&doc3_root)
        .expect("Failed to find configuration in doc3");

    println!("Found configuration at: {:?}", config_path);

    // Load metadata
    let config =
        bsl_metadata::load_from_directory(&config_path).expect("Failed to load doc3 metadata");

    // Verify metadata was loaded
    let metadata_objects = config.metadata_objects();

    println!("Loaded {} metadata objects", metadata_objects.len());

    let catalogs: Vec<_> = metadata_objects
        .iter()
        .filter(|obj| obj.mdo_type == bsl_metadata::MdoType::Catalog)
        .collect();

    let documents: Vec<_> = metadata_objects
        .iter()
        .filter(|obj| obj.mdo_type == bsl_metadata::MdoType::Document)
        .collect();

    let registers: Vec<_> = metadata_objects
        .iter()
        .filter(|obj| obj.mdo_type == bsl_metadata::MdoType::InformationRegister)
        .collect();

    println!("Catalogs: {}", catalogs.len());
    println!("Documents: {}", documents.len());
    println!("Registers: {}", registers.len());

    // Print first 5 catalogs
    println!("\nFirst 5 catalogs:");
    for catalog in catalogs.iter().take(5) {
        println!("  - {}", catalog.name);
    }

    // Print first 5 documents
    println!("\nFirst 5 documents:");
    for doc in documents.iter().take(5) {
        println!("  - {}", doc.name);
    }

    // Verify we have meaningful data
    assert!(!metadata_objects.is_empty(), "Should have metadata objects");
    assert!(!catalogs.is_empty(), "Should have catalogs");
    assert!(!documents.is_empty(), "Should have documents");
}
