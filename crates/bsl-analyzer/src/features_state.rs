use ide_db::RootDatabaseImpl;
use project_model::{FeaturesConfig, Project};

use crate::global_state::GlobalState;

impl GlobalState {
    pub fn update_features_config(&mut self) {
        let features = resolve_features(self.project.as_ref());
        apply_features_to_db(self.analysis_host.raw_database_mut(), &features);
    }
}

fn resolve_features(project: Option<&Project>) -> FeaturesConfig {
    project.map(|p| p.config.features.clone()).unwrap_or_default()
}

fn apply_features_to_db(db: &mut RootDatabaseImpl, features: &FeaturesConfig) {
    tracing::info!(type_narrowing = features.type_narrowing, "updated feature flags");
    db.set_type_narrowing_enabled(features.type_narrowing);
}

#[cfg(test)]
mod tests {
    use super::{apply_features_to_db, resolve_features};
    use ide_db::RootDatabaseImpl;
    use project_model::{FeaturesConfig, Project, ProjectConfig};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn resolve_features_returns_defaults_without_project() {
        let features = resolve_features(None);
        assert!(
            features.type_narrowing,
            "missing project must fall back to `FeaturesConfig::default` (narrowing on)"
        );
    }

    #[test]
    fn apply_features_propagates_disable_to_database() {
        let mut db = RootDatabaseImpl::new();
        assert!(db.type_narrowing_enabled(), "fresh database defaults to narrowing on");

        let disabled = FeaturesConfig { type_narrowing: false, ..FeaturesConfig::default() };
        apply_features_to_db(&mut db, &disabled);
        assert!(!db.type_narrowing_enabled(), "apply must flip the Salsa input to false");

        let enabled = FeaturesConfig::default();
        apply_features_to_db(&mut db, &enabled);
        assert!(db.type_narrowing_enabled(), "apply must flip the Salsa input back to true");
    }

    #[test]
    fn update_features_pipeline_threads_toml_flag_end_to_end() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("bsl-analyzer.toml"),
            r#"
[features]
type_narrowing = false
"#,
        )
        .unwrap();

        let project = Project::new(dir.path());
        assert!(
            !project.config.features.type_narrowing,
            "Project::new must surface the disabled flag from the TOML"
        );

        let mut db = RootDatabaseImpl::new();
        apply_features_to_db(&mut db, &resolve_features(Some(&project)));
        assert!(
            !db.type_narrowing_enabled(),
            "full pipeline must land the disabled flag on the Salsa input"
        );

        let default_project_config = ProjectConfig::default();
        assert!(default_project_config.features.type_narrowing);
        apply_features_to_db(&mut db, &default_project_config.features);
        assert!(db.type_narrowing_enabled());
    }
}
