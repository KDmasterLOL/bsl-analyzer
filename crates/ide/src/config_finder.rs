//! Configuration file finder for 1C:Enterprise projects.
//!
//! This module provides a convenience wrapper around [`project_model::Project`]
//! for finding 1C configuration directories.
//!
//! **Note:** For new code, prefer using [`project_model::Project`] directly,
//! which provides the same discovery logic plus access to project configuration.

use std::path::{Path, PathBuf};

/// Finds the path to 1C configuration directory.
///
/// This is a convenience wrapper around [`project_model::Project::configuration_path`].
///
/// Searches in this order:
/// 1. Check .bsl-analyzer.json or .bsl-language-server.json for `configurationRoot`
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
    let project = project_model::Project::new(workspace_root);
    project.configuration_path().map(|p| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_configuration_path_with_fixtures() {
        // Use the existing fixtures from bsl-metadata
        let fixtures_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures");

        let result = find_configuration_path(Path::new(fixtures_path));

        assert!(result.is_some(), "Should find Configuration.xml in fixtures");

        let config_path = result.unwrap();
        assert!(config_path.join("Configuration.xml").exists());
    }
}
