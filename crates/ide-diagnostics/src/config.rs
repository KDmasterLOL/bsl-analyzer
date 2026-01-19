//! Diagnostics configuration.

use crate::DiagnosticCode;
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
#[derive(Debug, Clone)]
pub struct DiagnosticsConfig {
    pub disabled: Vec<DiagnosticCode>,
    pub parameters: HashMap<DiagnosticCode, serde_json::Value>,
    pub ordinary_app_support: bool,
    /// Maximum iterations for dataflow analysis (default: 10000)
    ///
    /// Controls convergence limit for liveness analysis and other dataflow algorithms.
    /// Increase this for very complex methods with deep nesting or many loops.
    /// Warning is logged if analysis exceeds this limit.
    pub dataflow_max_iterations: usize,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            disabled: Vec::new(),
            parameters: HashMap::new(),
            ordinary_app_support: false,
            dataflow_max_iterations: 10000,
        }
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
                let mut parameters = HashMap::new();
                let mut ordinary_app_support = false;
                let mut dataflow_max_iterations = 10000usize;

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
                                            // enabled = default, skip
                                        }
                                        serde_json::Value::Object(_) => {
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
                    parameters,
                    ordinary_app_support,
                    dataflow_max_iterations,
                })
            }
        }

        deserializer.deserialize_map(DiagnosticsConfigVisitor)
    }
}

impl DiagnosticsConfig {
    /// Check if a diagnostic is disabled.
    pub fn is_disabled(&self, code: DiagnosticCode) -> bool {
        self.disabled.contains(&code)
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
            parameters,
            ordinary_app_support: input.ordinary_app_support,
            dataflow_max_iterations: input.dataflow_max_iterations,
        }
    }
}
