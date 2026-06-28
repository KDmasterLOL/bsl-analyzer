use crate::handlers;
use crate::metadata::{DiagnosticSeverityLevel, DiagnosticType, MetadataTag};
use crate::{DiagnosticCode, Severity};
use base_db::{DiagnosticsConfigInput, Locale};
use std::collections::HashMap;
use stdx::case::CaseExt;

#[derive(Debug, Clone, Default)]
pub struct MetadataOverride {
    pub severity: Option<DiagnosticSeverityLevel>,
    pub diagnostic_type: Option<DiagnosticType>,
    pub tags: Option<Vec<MetadataTag>>,
    pub lsp_severity: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EffectiveMetadata {
    base: &'static crate::metadata::DiagnosticMetadata,
    tags_override: Option<Vec<MetadataTag>>,
    lsp_severity_override: Option<String>,
}

impl EffectiveMetadata {
    pub fn severity_value(&self) -> Severity {
        if let Some(override_str) = &self.lsp_severity_override {
            return parse_severity(override_str);
        }
        self.base.calculate_severity()
    }

    pub fn tags(&self) -> Vec<MetadataTag> {
        self.tags_override.clone().unwrap_or_else(|| self.base.tags.to_vec())
    }
}

fn parse_severity(s: &str) -> Severity {
    match s.fold_lower().as_str() {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "information" | "info" => Severity::Information,
        "hint" => Severity::Hint,
        "blocker" => Severity::Blocker,
        "critical" => Severity::Critical,
        "major" => Severity::Major,
        _ => Severity::Warning,
    }
}

#[derive(Debug, Clone)]
pub struct DiagnosticsConfig {
    pub disabled: Vec<DiagnosticCode>,
    pub enabled: Vec<DiagnosticCode>,
    pub parameters: HashMap<DiagnosticCode, serde_json::Value>,
    pub ordinary_app_support: bool,
    pub dataflow_max_iterations: usize,
    pub metadata_overrides: HashMap<DiagnosticCode, MetadataOverride>,
    pub only_enabled: Option<Vec<DiagnosticCode>>,
    pub locale: Locale,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            disabled: Vec::new(),
            enabled: Vec::new(),
            parameters: HashMap::new(),
            ordinary_app_support: false,
            dataflow_max_iterations: hir::dataflow::DEFAULT_MAX_ITERATIONS,
            metadata_overrides: HashMap::new(),
            only_enabled: None,
            locale: Locale::default(),
        }
    }
}

impl DiagnosticsConfig {
    /// Build the effective diagnostics config from the raw `[diagnostics]` value that
    /// `project-model` loads from `bsl-analyzer.toml` / `.bsl-analyzer.json` /
    /// `.bsl-language-server.json`, then stamp the resolved `locale`.
    ///
    /// This is the single source of truth shared by every runtime mode (LSP, CLI,
    /// MCP), so a project's settings apply identically regardless of how the analyzer
    /// is driven. A malformed config logs a warning and falls back to defaults rather
    /// than failing the analysis.
    pub fn from_project_json(diagnostics: &serde_json::Value, locale: Locale) -> Self {
        let mut config: Self = serde_json::from_value(diagnostics.clone()).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "failed to deserialize project diagnostics config; using defaults");
            Self::default()
        });
        config.locale = locale;
        config
    }

    pub fn all_enabled() -> Self {
        let mut enabled = Vec::new();
        for code in [
            DiagnosticCode::BadWords,
            DiagnosticCode::CodeAfterAsyncCall,
            DiagnosticCode::DenyIncompleteValues,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            DiagnosticCode::FileSystemAccess,
            DiagnosticCode::FunctionNameStartsWithGet,
            DiagnosticCode::FunctionOutParameter,
            DiagnosticCode::InternetAccess,
            DiagnosticCode::MissingTempStorageDeletion,
            DiagnosticCode::TernaryOperatorUsage,
            DiagnosticCode::TooManyReturns,
            DiagnosticCode::TypeMismatchByDocComment,
            DiagnosticCode::UseSystemInformation,
            DiagnosticCode::UsingLikeInQuery,
        ] {
            if let Some(meta) = handlers::get_metadata(code) {
                if !meta.activated_by_default {
                    enabled.push(code);
                }
            }
        }

        Self {
            disabled: Vec::new(),
            enabled,
            parameters: HashMap::new(),
            ordinary_app_support: false,
            dataflow_max_iterations: hir::dataflow::DEFAULT_MAX_ITERATIONS,
            metadata_overrides: HashMap::new(),
            only_enabled: None,
            locale: Locale::default(),
        }
    }

    pub fn get_effective_metadata(&self, code: DiagnosticCode) -> Option<EffectiveMetadata> {
        let base = handlers::get_metadata(code)?;
        let override_data = self.metadata_overrides.get(&code);

        Some(EffectiveMetadata {
            base,
            tags_override: override_data.and_then(|o| o.tags.clone()),
            lsp_severity_override: override_data.and_then(|o| o.lsp_severity.clone()),
        })
    }
}

impl<'de> serde::Deserialize<'de> for DiagnosticsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor};
        use std::fmt;

        struct DiagnosticsConfigVisitor;

        impl<'de> Visitor<'de> for DiagnosticsConfigVisitor {
            type Value = DiagnosticsConfig;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a diagnostics configuration object")
            }

            fn visit_map<M>(self, mut map: M) -> Result<DiagnosticsConfig, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut disabled = Vec::new();
                let mut enabled = Vec::new();
                let mut parameters = HashMap::new();
                let mut ordinary_app_support = false;
                let mut dataflow_max_iterations = hir::dataflow::DEFAULT_MAX_ITERATIONS;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "ordinaryAppSupport" => {
                            ordinary_app_support = map.next_value()?;
                        }
                        "dataflowMaxIterations" => {
                            dataflow_max_iterations = map.next_value()?;
                        }
                        "parameters" => {
                            let params: HashMap<String, serde_json::Value> = map.next_value()?;
                            for (code_str, value) in params {
                                if let Ok(code) = code_str.parse::<DiagnosticCode>() {
                                    match &value {
                                        serde_json::Value::Bool(false) => {
                                            disabled.push(code);
                                        }
                                        serde_json::Value::Bool(true) => {
                                            enabled.push(code);
                                        }
                                        serde_json::Value::Object(_) => {
                                            enabled.push(code);
                                            parameters.insert(code, value);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        _ => {
                            let _: serde_json::Value = map.next_value()?;
                        }
                    }
                }

                Ok(DiagnosticsConfig {
                    disabled,
                    enabled,
                    parameters,
                    ordinary_app_support,
                    dataflow_max_iterations,
                    metadata_overrides: HashMap::new(),
                    only_enabled: None,
                    locale: Locale::default(),
                })
            }
        }

        deserializer.deserialize_map(DiagnosticsConfigVisitor)
    }
}

impl DiagnosticsConfig {
    #[inline]
    pub fn any_enabled(&self, codes: &[DiagnosticCode]) -> bool {
        if let Some(ref only) = self.only_enabled {
            return codes.iter().any(|code| only.contains(code));
        }

        codes.iter().any(|code| !self.is_disabled(*code))
    }

    pub fn is_disabled(&self, code: DiagnosticCode) -> bool {
        if let Some(ref only) = self.only_enabled {
            return !only.contains(&code);
        }

        if self.disabled.contains(&code) {
            return true;
        }

        if let Some(metadata) = handlers::get_metadata(code) {
            if !metadata.activated_by_default
                && !self.enabled.contains(&code)
                && !self.parameters.contains_key(&code)
            {
                return true;
            }
        }

        false
    }

    pub fn get_bool(&self, code: DiagnosticCode, param: &str) -> Option<bool> {
        self.parameters.get(&code).and_then(|v| v.get(param)).and_then(|v| v.as_bool())
    }

    pub fn get_int(&self, code: DiagnosticCode, param: &str) -> Option<i64> {
        self.parameters.get(&code).and_then(|v| v.get(param)).and_then(|v| v.as_i64())
    }

    pub fn get_string(&self, code: DiagnosticCode, param: &str) -> Option<&str> {
        self.parameters.get(&code).and_then(|v| v.get(param)).and_then(|v| v.as_str())
    }

    pub fn get_string_param(&self, code: DiagnosticCode, param: &str) -> Option<String> {
        self.parameters
            .get(&code)
            .and_then(|v| v.get(param))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    pub fn get_string_array(&self, code: DiagnosticCode, param: &str) -> Option<Vec<String>> {
        self.parameters
            .get(&code)
            .and_then(|v| v.get(param))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
    }

    pub fn from_input(input: &DiagnosticsConfigInput) -> Self {
        let disabled: Vec<DiagnosticCode> =
            input.disabled.iter().filter_map(|s| s.parse().ok()).collect();

        let enabled: Vec<DiagnosticCode> =
            input.enabled.iter().filter_map(|s| s.parse().ok()).collect();

        let parameters: HashMap<DiagnosticCode, serde_json::Value> = input
            .parameters
            .iter()
            .filter_map(|(code_str, json_str)| {
                let code: DiagnosticCode = code_str.parse().ok()?;
                let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
                Some((code, value))
            })
            .collect();

        Self {
            disabled,
            enabled,
            parameters,
            ordinary_app_support: input.ordinary_app_support,
            dataflow_max_iterations: input.dataflow_max_iterations,
            metadata_overrides: HashMap::new(),
            only_enabled: None,
            locale: input.locale,
        }
    }

    pub fn apply_cli_filters(&mut self, only_diagnostic: &[String], disable_diagnostic: &[String]) {
        if !only_diagnostic.is_empty() {
            let codes: Vec<DiagnosticCode> =
                only_diagnostic.iter().filter_map(|s| s.parse().ok()).collect();
            if !codes.is_empty() {
                self.only_enabled = Some(codes);
            }
        }

        for code_str in disable_diagnostic {
            if let Ok(code) = code_str.parse::<DiagnosticCode>() {
                if !self.disabled.contains(&code) {
                    self.disabled.push(code);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shared project-config parser turns the raw `[diagnostics]` value into the
    /// same effective config every runtime mode consumes: a `parameters` entry of
    /// `false` disables a code, an object enables it with parameters, and the resolved
    /// locale is stamped over the default.
    #[test]
    fn from_project_json_parses_params_and_stamps_locale() {
        let raw = json!({
            "parameters": {
                "Typo": false,
                "LineLength": { "maxLineLength": 150 },
            }
        });
        let config = DiagnosticsConfig::from_project_json(&raw, Locale::En);

        assert!(config.is_disabled(DiagnosticCode::Typo), "a `false` param disables the code");
        assert_eq!(
            config.get_int(DiagnosticCode::LineLength, "maxLineLength"),
            Some(150),
            "an object param carries the project threshold"
        );
        assert_eq!(config.locale, Locale::En, "the resolved locale overrides the default");
    }

    /// A malformed config (not an object) must not fail the analysis: it falls back to
    /// defaults while still stamping the locale.
    #[test]
    fn from_project_json_falls_back_on_garbage() {
        let config = DiagnosticsConfig::from_project_json(&json!("not an object"), Locale::Ru);
        assert!(config.disabled.is_empty());
        assert!(config.parameters.is_empty());
        assert_eq!(config.locale, Locale::Ru);
    }

    /// An absent `[diagnostics]` section (serde null) yields defaults, not a panic.
    #[test]
    fn from_project_json_handles_null() {
        let config =
            DiagnosticsConfig::from_project_json(&serde_json::Value::Null, Locale::default());
        assert!(config.disabled.is_empty());
        assert!(config.enabled.is_empty());
    }
}
