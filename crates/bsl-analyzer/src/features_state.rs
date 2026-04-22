//! Feature flag state management.
//!
//! Propagates workspace feature flags from `project_model::ProjectConfig`
//! into the Salsa input hosted by `ide_db`. Kept in its own module to
//! mirror `diagnostics_state.rs`.

use ide_db::RootDatabaseImpl;
use project_model::{FeaturesConfig, Project};

use crate::global_state::GlobalState;

impl GlobalState {
    /// Push feature flags from the current project config into the Salsa
    /// `FeaturesInput`. Falls back to `FeaturesConfig::default` when no
    /// project is loaded (e.g., single-file mode).
    pub fn update_features_config(&mut self) {
        let features = resolve_features(self.project.as_ref());
        apply_features_to_db(self.analysis_host.raw_database_mut(), &features);
    }
}

/// Pure resolver: pick the project's [`FeaturesConfig`] when available,
/// else fall back to defaults. Split from [`GlobalState::update_features_config`]
/// to keep it testable without standing up a real LSP `GlobalState`.
fn resolve_features(project: Option<&Project>) -> FeaturesConfig {
    project.map(|p| p.config.features.clone()).unwrap_or_default()
}

/// Stateless DB bridge: apply a [`FeaturesConfig`] to a [`RootDatabaseImpl`].
/// All runtime-observable side effects of feature-flag propagation live
/// here, so tests can exercise the config → DB path without the
/// surrounding LSP plumbing.
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
        // Covers the single-file / no-workspace branch: `update_features_config`
        // is called during init before any project exists (e.g., first run
        // with a scratch file). Must yield the shipped defaults so the
        // analyzer doesn't sit in an unexpected disabled state.
        let features = resolve_features(None);
        assert!(
            features.type_narrowing,
            "missing project must fall back to `FeaturesConfig::default` (narrowing on)"
        );
    }

    #[test]
    fn apply_features_propagates_disable_to_database() {
        // Bridge between the project config and the Salsa input — the
        // specific unit Codex flagged as untested. The DB starts with
        // the default `true` (set in `RootDatabaseImpl::new`); applying
        // a `type_narrowing = false` config must flip the observable
        // reader to `false`.
        let mut db = RootDatabaseImpl::new();
        assert!(db.type_narrowing_enabled(), "fresh database defaults to narrowing on");

        let disabled = FeaturesConfig { type_narrowing: false };
        apply_features_to_db(&mut db, &disabled);
        assert!(!db.type_narrowing_enabled(), "apply must flip the Salsa input to false");

        // Round-trip: re-enable and confirm the bridge is symmetric.
        let enabled = FeaturesConfig::default();
        apply_features_to_db(&mut db, &enabled);
        assert!(db.type_narrowing_enabled(), "apply must flip the Salsa input back to true");
    }

    #[test]
    fn update_features_pipeline_threads_toml_flag_end_to_end() {
        // End-to-end through the real loaders: write a `bsl-analyzer.toml`
        // with `type_narrowing = false`, build a `Project`, run the same
        // `resolve_features` + `apply_features_to_db` sequence that
        // `GlobalState::update_features_config` runs. Guards against
        // regressions in any link of the chain
        // (TOML → TomlConfig → ProjectConfig → Project.config.features →
        //  resolve_features → apply_features_to_db → RootDatabaseImpl).
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

        // Reset via default-only config to mirror "project removed the flag":
        // the bridge must re-raise `type_narrowing` to `true`.
        let default_project_config = ProjectConfig::default();
        assert!(default_project_config.features.type_narrowing);
        apply_features_to_db(&mut db, &default_project_config.features);
        assert!(db.type_narrowing_enabled());
    }
}
