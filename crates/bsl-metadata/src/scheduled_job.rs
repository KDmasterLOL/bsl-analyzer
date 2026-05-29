use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledJob {
    #[serde(rename = "uuid")]
    pub(crate) uuid: Uuid,

    #[serde(rename = "name")]
    pub(crate) name: String,

    #[serde(rename = "methodName", default)]
    pub(crate) method_name: String,

    #[serde(rename = "predefined", default)]
    pub(crate) predefined: bool,

    #[serde(rename = "use", default)]
    pub(crate) use_flag: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledJobHandler {
    pub module_name: String,
    pub method_name: String,
}

impl ScheduledJob {
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

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn method_name(&self) -> &str {
        &self.method_name
    }

    pub fn is_predefined(&self) -> bool {
        self.predefined
    }

    pub fn is_enabled(&self) -> bool {
        self.use_flag
    }

    pub fn parse_handler(&self) -> Option<ScheduledJobHandler> {
        if self.method_name.is_empty() {
            return None;
        }

        let parts: Vec<&str> = self.method_name.split('.').collect();

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
        assert_eq!(handler.method_name, "");
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
