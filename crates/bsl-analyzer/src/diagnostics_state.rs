use base_db::{DiagnosticsConfigId, DiagnosticsConfigInput, Locale};

use crate::global_state::GlobalState;
use crate::locale::resolve_locale;

impl GlobalState {
    pub fn diagnostics_config_id(&self) -> DiagnosticsConfigId<'_> {
        DiagnosticsConfigId::new(self.analysis_host.raw_database(), self.diagnostics_config.clone())
    }

    pub fn diagnostics_config(&self) -> &DiagnosticsConfigInput {
        &self.diagnostics_config
    }

    pub fn update_diagnostics_config(&mut self) {
        let project_locale = self.project.as_ref().and_then(|p| Self::project_locale(&p.config));
        let locale = resolve_locale(project_locale, self.lsp_locale);

        self.diagnostics_config =
            self.project.as_ref().map(|p| Self::config_from_project(p, locale)).unwrap_or_else(
                || {
                    DiagnosticsConfigInput::from_raw(
                        Vec::<String>::new(),
                        Vec::<String>::new(),
                        Vec::<(String, String)>::new(),
                        false,
                        hir::dataflow::DEFAULT_MAX_ITERATIONS,
                        locale,
                        true,
                    )
                },
            );

        // The input was rebuilt from the project config; re-attach the current
        // vendor-diff scope so a config reload does not silently drop the filter.
        self.apply_scope_to_config();

        tracing::info!(
            disabled_count = self.diagnostics_config.disabled.len(),
            enabled_count = self.diagnostics_config.enabled.len(),
            params_count = self.diagnostics_config.parameters.len(),
            scope = self.diagnostics_config.scope.is_some(),
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

    fn project_locale(config: &project_model::ProjectConfig) -> Option<Locale> {
        config.output.resolve_locale()
    }

    fn config_from_project(
        project: &project_model::Project,
        locale: Locale,
    ) -> DiagnosticsConfigInput {
        let diagnostics = project.config.diagnostics.rules_json();
        let config = ide::DiagnosticsConfig::from_project_json(&diagnostics, locale);

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
            config.bslls_suppression_compat,
        )
    }
}
