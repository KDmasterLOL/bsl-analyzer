//! ScheduledJob metadata object
//!
//! Represents 1C:Enterprise ScheduledJob metadata.
//! ScheduledJobs define handlers for scheduled tasks that run on the server.
//!
//! ## Structure
//!
//! - Name: Unique job name
//! - MethodName: Format `CommonModule.ModuleName.MethodName`
//! - Predefined: Whether the job is predefined (cannot have parameters)
//! - Use: Whether the job is enabled
//!
//! ## Note
//!
//! Unlike CommonModules, ScheduledJobs have NO code files - only XML metadata.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// ScheduledJob metadata object
///
/// Java equivalent: `com.github._1c_syntax.bsl.mdo.ScheduledJob`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledJob {
    /// UUID
    #[serde(rename = "uuid")]
    pub(crate) uuid: Uuid,

    /// Job name
    #[serde(rename = "name")]
    pub(crate) name: String,

    /// Handler path: "CommonModule.ModuleName.MethodName"
    /// Can be empty if not configured
    #[serde(rename = "methodName", default)]
    pub(crate) method_name: String,

    /// Whether the job is predefined
    #[serde(rename = "predefined", default)]
    pub(crate) predefined: bool,

    /// Whether the job is enabled
    #[serde(rename = "use", default)]
    pub(crate) use_flag: bool,
}

/// Parsed handler (CommonModule.ModuleName.MethodName)
///
/// Represents a parsed scheduled job handler path.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledJobHandler {
    /// Common module name
    pub module_name: String,

    /// Method name (can be empty if handler is malformed)
    pub method_name: String,
}

impl ScheduledJob {
    /// Create new ScheduledJob
    #[cfg(test)]
    pub fn new(name: impl Into<String>, method_name: impl Into<String>) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            name: name.into(),
            method_name: method_name.into(),
            predefined: false,
            use_flag: true,
        }
    }

    /// Create new predefined ScheduledJob
    #[cfg(test)]
    pub fn new_predefined(name: impl Into<String>, method_name: impl Into<String>) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            name: name.into(),
            method_name: method_name.into(),
            predefined: true,
            use_flag: true,
        }
    }

    /// Get job name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get method name string (handler path)
    pub fn method_name(&self) -> &str {
        &self.method_name
    }

    /// Check if job is predefined
    pub fn is_predefined(&self) -> bool {
        self.predefined
    }

    /// Check if job is enabled
    pub fn is_enabled(&self) -> bool {
        self.use_flag
    }

    /// Parse handler string into components
    ///
    /// Returns:
    /// - `None` if handler is empty
    /// - `Some(Handler)` with empty method_name if malformed (e.g., "CommonModule.Module")
    /// - `Some(Handler)` with full data if valid (e.g., "CommonModule.Module.Method")
    ///
    /// ## Examples
    ///
    /// ```ignore
    /// // Valid handler: "CommonModule.MyModule.MyMethod"
    /// let handler = job.parse_handler().unwrap();
    /// assert_eq!(handler.module_name, "MyModule");
    /// assert_eq!(handler.method_name, "MyMethod");
    ///
    /// // Malformed (missing method): module_name set, method_name empty
    /// // Empty handler string: returns None
    /// ```
    pub fn parse_handler(&self) -> Option<ScheduledJobHandler> {
        if self.method_name.is_empty() {
            return None;
        }

        let parts: Vec<&str> = self.method_name.split('.').collect();

        // Must start with "CommonModule" and have at least module name
        if parts.len() < 2 || parts[0] != "CommonModule" {
            return None;
        }

        Some(ScheduledJobHandler {
            module_name: parts[1].to_string(),
            method_name: parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_handler_full() {
        let job =
            ScheduledJob::new("TestJob", "CommonModule.ПервыйОбщийМодуль.НеУстаревшаяПроцедура");

        let handler = job.parse_handler().unwrap();
        assert_eq!(handler.module_name, "ПервыйОбщийМодуль");
        assert_eq!(handler.method_name, "НеУстаревшаяПроцедура");
    }

    #[test]
    fn test_parse_handler_malformed_missing_method() {
        let job = ScheduledJob::new("TestJob", "CommonModule.ОбщийМодуль");

        let handler = job.parse_handler().unwrap();
        assert_eq!(handler.module_name, "ОбщийМодуль");
        assert_eq!(handler.method_name, ""); // Empty!
    }

    #[test]
    fn test_parse_handler_empty() {
        let job = ScheduledJob::new("TestJob", "");
        assert!(job.parse_handler().is_none());
    }

    #[test]
    fn test_parse_handler_invalid_prefix() {
        let job = ScheduledJob::new("TestJob", "InvalidPrefix.Module.Method");
        assert!(job.parse_handler().is_none());
    }

    #[test]
    fn test_parse_handler_only_common_module() {
        let job = ScheduledJob::new("TestJob", "CommonModule");
        assert!(job.parse_handler().is_none());
    }

    #[test]
    fn test_scheduled_job_accessors() {
        let job = ScheduledJob::new("TestJob", "CommonModule.M.F");
        assert_eq!(job.name(), "TestJob");
        assert_eq!(job.method_name(), "CommonModule.M.F");
        assert!(!job.is_predefined());
        assert!(job.is_enabled());
    }

    #[test]
    fn test_predefined_job() {
        let job = ScheduledJob::new_predefined("PredefinedJob", "CommonModule.M.F");
        assert!(job.is_predefined());
    }
}
