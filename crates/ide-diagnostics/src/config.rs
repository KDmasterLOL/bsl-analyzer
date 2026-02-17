//! Diagnostics configuration.

use crate::metadata::{DiagnosticSeverityLevel, DiagnosticType, MetadataTag};
use crate::handlers;
use crate::{DiagnosticCode, Severity};
use base_db::DiagnosticsConfigInput;
use std::collections::HashMap;

/// Configuration for diagnostics.
///
/// Supports Java BSL-LS compatible format:
/// ```json
/// {
///   "diagnostics": {
///     "ordinaryAppSupport": false,
///     "dataflowMaxIterations": 10000,
///     "parameters": {
///       "EmptyCodeBlock": false,
///       "LineLength": { "maxLength": 120 }
///     }
///   }
/// }
/// ```
///
/// In `parameters`:
/// - `false` = diagnostic disabled
/// - `true` = diagnostic enabled (default)
/// - `{...}` = diagnostic parameters
///
/// Metadata override from JSON configuration.
///
/// Allows runtime override of compile-time metadata.
/// Matches Java's metadata override functionality.
#[derive(Debug, Clone, Default)]
pub struct MetadataOverride {
    pub severity: Option<DiagnosticSeverityLevel>,
    pub diagnostic_type: Option<DiagnosticType>,
    pub tags: Option<Vec<MetadataTag>>,
    pub lsp_severity: Option<String>,
}

/// Effective metadata (base + overrides).
///
/// Combines compile-time const metadata with runtime config overrides.
#[derive(Debug, Clone)]
pub struct EffectiveMetadata {
    base: &'static crate::metadata::DiagnosticMetadata,
    severity_override: Option<DiagnosticSeverityLevel>,
    type_override: Option<DiagnosticType>,
    tags_override: Option<Vec<MetadataTag>>,
    lsp_severity_override: Option<String>,
}

impl EffectiveMetadata {
    /// Get effective severity (base or override).
    pub fn severity_value(&self) -> Severity {
        if let Some(override_str) = &self.lsp_severity_override {
            return parse_severity(override_str);
        }
        self.base.calculate_severity()
    }

    /// Get effective tags (base or override).
    pub fn tags(&self) -> Vec<MetadataTag> {
        self.tags_override.clone().unwrap_or_else(|| self.base.tags.to_vec())
    }

    /// Get effective diagnostic type (base or override).
    pub fn diagnostic_type(&self) -> DiagnosticType {
        self.type_override.unwrap_or(self.base.diagnostic_type)
    }

    /// Get effective severity (base or override).
    pub fn severity(&self) -> DiagnosticSeverityLevel {
        self.severity_override.unwrap_or(self.base.severity)
    }
}

/// Parse severity from string.
fn parse_severity(s: &str) -> Severity {
    match s.to_lowercase().as_str() {
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
    /// Diagnostics explicitly enabled (for those disabled by default).
    pub enabled: Vec<DiagnosticCode>,
    pub parameters: HashMap<DiagnosticCode, serde_json::Value>,
    pub ordinary_app_support: bool,
    /// Maximum iterations for dataflow analysis (default: dataflow::DEFAULT_MAX_ITERATIONS)
    ///
    /// Controls convergence limit for liveness analysis and other dataflow algorithms.
    /// Increase this for very complex methods with deep nesting or many loops.
    /// Warning is logged if analysis exceeds this limit.
    pub dataflow_max_iterations: usize,
    /// Metadata overrides from JSON configuration.
    ///
    /// Allows runtime override of compile-time metadata (severity, type, tags, lsp_severity).
    /// Not yet fully implemented - placeholder for Phase 3 completion.
    pub metadata_overrides: HashMap<DiagnosticCode, MetadataOverride>,
    /// Exclusive mode: if Some, ONLY these diagnostics are enabled.
    /// Set via --only-diagnostic CLI flag. Overrides disabled/enabled lists.
    pub only_enabled: Option<Vec<DiagnosticCode>>,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            disabled: Vec::new(),
            enabled: Vec::new(),
            parameters: HashMap::new(),
            ordinary_app_support: false,
            dataflow_max_iterations: dataflow::DEFAULT_MAX_ITERATIONS,
            metadata_overrides: HashMap::new(),
            only_enabled: None,
        }
    }
}

impl DiagnosticsConfig {
    /// Create a config with all diagnostics enabled (including those disabled by default).
    /// Useful for testing.
    pub fn all_enabled() -> Self {
        // Collect all diagnostics that are disabled by default from metadata registry
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
            dataflow_max_iterations: dataflow::DEFAULT_MAX_ITERATIONS,
            metadata_overrides: HashMap::new(),
            only_enabled: None,
        }
    }

    /// Get effective metadata (base + overrides).
    ///
    /// Returns `None` if no metadata is defined for this diagnostic yet.
    pub fn get_effective_metadata(&self, code: DiagnosticCode) -> Option<EffectiveMetadata> {
        let base = handlers::get_metadata(code)?;
        let override_data = self.metadata_overrides.get(&code);

        Some(EffectiveMetadata {
            base,
            severity_override: override_data.and_then(|o| o.severity),
            type_override: override_data.and_then(|o| o.diagnostic_type),
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
                let mut dataflow_max_iterations = dataflow::DEFAULT_MAX_ITERATIONS;

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
                                        _ => {
                                            // ignore other values
                                        }
                                    }
                                }
                                // Unknown diagnostic codes are silently ignored
                            }
                        }
                        _ => {
                            // Skip unknown fields
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
                })
            }
        }

        deserializer.deserialize_map(DiagnosticsConfigVisitor)
    }
}

impl DiagnosticsConfig {
    /// Check if ANY of the given diagnostics is enabled.
    ///
    /// Used for early exit in collectors - if none of the collector's diagnostics
    /// are enabled, the entire collector can be skipped.
    ///
    /// Returns `true` if at least one diagnostic from the list is enabled.
    #[inline]
    pub fn any_enabled(&self, codes: &[DiagnosticCode]) -> bool {
        // Fast path: if only_enabled is set, check intersection
        if let Some(ref only) = self.only_enabled {
            return codes.iter().any(|code| only.contains(code));
        }

        // Normal mode: at least one must not be disabled
        codes.iter().any(|code| !self.is_disabled(*code))
    }

    /// Check if a diagnostic is disabled.
    ///
    /// A diagnostic is disabled if:
    /// 1. only_enabled is set and code is NOT in that list (exclusive mode from --only-diagnostic), OR
    /// 2. It's explicitly disabled via configuration, OR
    /// 3. Has metadata with activatedByDefault=false AND not explicitly enabled AND has no parameters
    pub fn is_disabled(&self, code: DiagnosticCode) -> bool {
        // Exclusive mode: if only_enabled is set, ONLY those diagnostics are active
        if let Some(ref only) = self.only_enabled {
            return !only.contains(&code);
        }

        if self.disabled.contains(&code) {
            return true;
        }

        // Check metadata for activatedByDefault
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

    /// Get a boolean parameter for a diagnostic.
    pub fn get_bool(&self, code: DiagnosticCode, param: &str) -> Option<bool> {
        self.parameters.get(&code).and_then(|v| v.get(param)).and_then(|v| v.as_bool())
    }

    /// Get an integer parameter for a diagnostic.
    pub fn get_int(&self, code: DiagnosticCode, param: &str) -> Option<i64> {
        self.parameters.get(&code).and_then(|v| v.get(param)).and_then(|v| v.as_i64())
    }

    /// Get a string parameter for a diagnostic.
    pub fn get_string(&self, code: DiagnosticCode, param: &str) -> Option<&str> {
        self.parameters.get(&code).and_then(|v| v.get(param)).and_then(|v| v.as_str())
    }

    /// Get a string parameter for a diagnostic (owned version).
    pub fn get_string_param(&self, code: DiagnosticCode, param: &str) -> Option<String> {
        self.parameters
            .get(&code)
            .and_then(|v| v.get(param))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Get a string array parameter for a diagnostic.
    pub fn get_string_array(&self, code: DiagnosticCode, param: &str) -> Option<Vec<String>> {
        self.parameters
            .get(&code)
            .and_then(|v| v.get(param))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
    }

    /// Get a float parameter for a diagnostic.
    pub fn get_float(&self, code: DiagnosticCode, param: &str) -> Option<f64> {
        self.parameters.get(&code).and_then(|v| v.get(param)).and_then(|v| v.as_f64())
    }

    /// Convert from Salsa-hashable DiagnosticsConfigInput.
    ///
    /// This converts the string-based config (used in Salsa for hashability)
    /// to the typed config (used by diagnostic handlers).
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
        }
    }

    /// Apply CLI filter flags to this config.
    ///
    /// - `only_diagnostic`: If non-empty, sets exclusive mode - only these diagnostics run
    /// - `disable_diagnostic`: Adds these codes to the disabled list
    ///
    /// The `only_diagnostic` flag takes precedence over everything else.
    pub fn apply_cli_filters(&mut self, only_diagnostic: &[String], disable_diagnostic: &[String]) {
        // Apply --only-diagnostic (exclusive mode)
        if !only_diagnostic.is_empty() {
            let codes: Vec<DiagnosticCode> =
                only_diagnostic.iter().filter_map(|s| s.parse().ok()).collect();
            if !codes.is_empty() {
                self.only_enabled = Some(codes);
            }
        }

        // Apply --disable-diagnostic (add to disabled list)
        for code_str in disable_diagnostic {
            if let Ok(code) = code_str.parse::<DiagnosticCode>() {
                if !self.disabled.contains(&code) {
                    self.disabled.push(code);
                }
            }
        }
    }
}
