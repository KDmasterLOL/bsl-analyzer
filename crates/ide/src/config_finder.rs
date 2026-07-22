use std::path::{Path, PathBuf};

pub fn find_configuration_path(workspace_root: &Path) -> Option<PathBuf> {
    let project = match project_model::Project::new(workspace_root) {
        Ok(project) => project,
        Err(e) => {
            tracing::error!(root = %workspace_root.display(), error = %e, "invalid project");
            return None;
        }
    };
    project.configuration_path().map(|p| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_configuration_path_with_fixtures() {
        let fixtures_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures");

        let result = find_configuration_path(Path::new(fixtures_path));

        assert!(result.is_some(), "Should find Configuration.xml in fixtures");

        let config_path = result.unwrap();
        assert!(config_path.join("Configuration.xml").exists());
    }
}
