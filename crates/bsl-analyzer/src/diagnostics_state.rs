//! Diagnostics configuration state management.
//!
//! Methods on `GlobalState` for managing diagnostics configuration:
//! - Loading config from project settings
//! - Converting to Salsa-compatible format
//! - Cache invalidation via generation counter

use base_db::{DiagnosticsConfigId, DiagnosticsConfigInput, Locale};

use crate::global_state::GlobalState;
use crate::locale::resolve_locale;

impl GlobalState {
    /// Gets the Salsa-interned diagnostics config ID.
    ///
    /// This ID is used in the `file_diagnostics_query` for Salsa caching.
    /// The same config produces the same ID (Salsa interning).
    pub fn diagnostics_config_id(&self) -> DiagnosticsConfigId<'_> {
        DiagnosticsConfigId::new(self.analysis_host.raw_database(), self.diagnostics_config.clone())
    }

    /// Gets a reference to the current diagnostics config.
    pub fn diagnostics_config(&self) -> &DiagnosticsConfigInput {
        &self.diagnostics_config
    }

    /// Updates diagnostics config from project settings.
    ///
    /// Called when project is loaded or config file changes.
    /// This invalidates all cached diagnostics (new config ID = new hash).
    pub fn update_diagnostics_config(&mut self) {
        let project_locale = self.project.as_ref().and_then(|p| Self::project_locale(&p.config));
        let locale = resolve_locale(project_locale, self.lsp_locale);

        self.diagnostics_config =
            self.project.as_ref().map(|p| Self::config_from_project(p, locale)).unwrap_or_else(
                || {
                    let mut input = DiagnosticsConfigInput::new();
                    input.locale = locale;
                    input
                },
            );

        tracing::info!(
            disabled_count = self.diagnostics_config.disabled.len(),
            enabled_count = self.diagnostics_config.enabled.len(),
            params_count = self.diagnostics_config.parameters.len(),
            ?locale,
            "updated diagnostics config"
        );

        if !self.diagnostics_config.disabled.is_empty() {
            tracing::debug!(
                disabled = ?self.diagnostics_config.disabled,
                "disabled diagnostics from config"
            );
        }
    }

    /// Resolve the project-level output locale from `[output] display_language`.
    ///
    /// Thin delegator to [`project_model::OutputConfig::resolve_locale`] —
    /// the actual parsing/warning lives in `project-model` so the streaming
    /// CLI path uses the exact same logic.
    fn project_locale(config: &project_model::ProjectConfig) -> Option<Locale> {
        config.output.resolve_locale()
    }

    /// Converts project diagnostics config to hashable DiagnosticsConfigInput.
    ///
    /// Deserializes the raw JSON into `ide::DiagnosticsConfig`,
    /// then converts to the Salsa-compatible `DiagnosticsConfigInput`.
    fn config_from_project(
        project: &project_model::Project,
        locale: Locale,
    ) -> DiagnosticsConfigInput {
        let config: ide::DiagnosticsConfig = match serde_json::from_value(
            project.config.diagnostics.clone(),
        ) {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!(error = %e, "failed to deserialize diagnostics config, using defaults");
                ide::DiagnosticsConfig::default()
            }
        };

        let disabled: Vec<String> = config.disabled.iter().map(|code| code.to_string()).collect();
        let enabled: Vec<String> = config.enabled.iter().map(|code| code.to_string()).collect();

        let parameters: Vec<(String, String)> = config
            .parameters
            .iter()
            .map(|(code, value)| {
                (code.to_string(), serde_json::to_string(value).unwrap_or_default())
            })
            .collect();

        DiagnosticsConfigInput::from_raw(
            disabled,
            enabled,
            parameters,
            config.ordinary_app_support,
            config.dataflow_max_iterations,
            locale,
        )
    }
}
