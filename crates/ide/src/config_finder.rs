//! Configuration file finder for 1C:Enterprise projects.
//!
//! Locates Configuration.xml and metadata directory using multiple strategies:
//! 1. Read .bsl-language-server.json or .bsl-analyzer.json (configurationRoot field)
//! 2. Search for Configuration.xml in workspace (max depth 2)
//! 3. Common patterns: src/cf, Configuration

use std::path::{Path, PathBuf};

/// Finds the path to 1C configuration directory.
///
/// Searches in this order:
/// 1. Check .bsl-language-server.json or .bsl-analyzer.json for `configurationRoot`
/// 2. Search for Configuration.xml (max depth 2 from workspace_root)
/// 3. Common patterns: src/cf, Configuration
///
/// # Arguments
///
/// * `workspace_root` - Root directory of the workspace
///
/// # Returns
///
/// Path to directory containing Configuration.xml, or None if not found.
pub fn find_configuration_path(workspace_root: &Path) -> Option<PathBuf> {
    tracing::debug!(
        workspace_root = ?workspace_root,
        "searching for 1C configuration"
    );

    // Strategy 1: Read from .bsl-language-server.json or .bsl-analyzer.json
    if let Some(path) = find_from_config_file(workspace_root) {
        tracing::info!(
            path = ?path,
            "found configuration from .bsl-language-server.json"
        );
        return Some(path);
    }

    // Strategy 2: Search for Configuration.xml (max depth 2)
    if let Some(path) = search_configuration_xml(workspace_root, 2) {
        tracing::info!(
            path = ?path,
            "found Configuration.xml"
        );
        return Some(path);
    }

    // Strategy 3: Try common patterns
    for pattern in &["src/cf", "Configuration"] {
        let path = workspace_root.join(pattern);
        if path.join("Configuration.xml").exists() {
            tracing::info!(
                path = ?path,
                pattern,
                "found configuration using common pattern"
            );
            return Some(path);
        }
    }

    tracing::warn!(
        workspace_root = ?workspace_root,
        "no 1C configuration found"
    );
    None
}

/// Reads configuration path from .bsl-language-server.json or .bsl-analyzer.json.
fn find_from_config_file(workspace_root: &Path) -> Option<PathBuf> {
    use serde_json::Value;

    for filename in &[".bsl-analyzer.json", ".bsl-language-server.json"] {
        let config_path = workspace_root.join(filename);

        if !config_path.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&config_path).ok()?;
        let json: Value = serde_json::from_str(&content).ok()?;

        if let Some(config_root) = json.get("configurationRoot").and_then(|v| v.as_str()) {
            let full_path = workspace_root.join(config_root);

            if full_path.join("Configuration.xml").exists() {
                return Some(full_path);
            } else {
                tracing::warn!(
                    config_root,
                    full_path = ?full_path,
                    "configurationRoot specified but Configuration.xml not found"
                );
            }
        }
    }

    None
}

/// Searches for Configuration.xml recursively up to max_depth.
fn search_configuration_xml(root: &Path, max_depth: usize) -> Option<PathBuf> {
    search_configuration_xml_recursive(root, max_depth, 0)
}

fn search_configuration_xml_recursive(
    dir: &Path,
    max_depth: usize,
    current_depth: usize,
) -> Option<PathBuf> {
    if current_depth > max_depth {
        return None;
    }

    // Check if Configuration.xml exists in current directory
    let config_xml = dir.join("Configuration.xml");
    if config_xml.exists() {
        return Some(dir.to_path_buf());
    }

    // If we haven't reached max depth, search subdirectories
    if current_depth < max_depth {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() {
                        if let Some(path) = search_configuration_xml_recursive(
                            &entry.path(),
                            max_depth,
                            current_depth + 1,
                        ) {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_configuration_path_with_fixtures() {
        // Use the existing fixtures from bsl-metadata
        let fixtures_path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let workspace_root = Path::new(fixtures_path).parent().unwrap();

        // Should find the designer directory
        let result = search_configuration_xml(workspace_root, 2);

        assert!(result.is_some(), "Should find Configuration.xml in fixtures");

        let config_path = result.unwrap();
        assert!(config_path.join("Configuration.xml").exists());
    }

    #[test]
    fn test_search_respects_max_depth() {
        let fixtures_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures");

        // With depth 0, should not find anything (Configuration.xml is in designer/)
        let result = search_configuration_xml(Path::new(fixtures_path), 0);
        assert!(result.is_none(), "Should not find with depth 0");

        // With depth 1, should find it
        let result = search_configuration_xml(Path::new(fixtures_path), 1);
        assert!(result.is_some(), "Should find with depth 1");
    }
}
