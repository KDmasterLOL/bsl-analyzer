use serde::Serialize;

use ide_diagnostics::DiagnosticOutput;

#[derive(Debug, Clone, Serialize)]
pub struct StartEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,

    pub total_files: usize,

    pub version: &'static str,
}

impl StartEvent {
    pub fn new(total_files: usize) -> Self {
        Self { event_type: "start", total_files, version: env!("CARGO_PKG_VERSION") }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FileMetrics {
    pub functions: usize,

    pub complexity: u32,

    pub cognitive_complexity: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,

    pub path: String,

    pub diagnostics: Vec<DiagnosticOutput>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<FileMetrics>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl FileEvent {
    pub fn new(
        path: String,
        diagnostics: Vec<DiagnosticOutput>,
        metrics: Option<FileMetrics>,
        error: Option<String>,
    ) -> Self {
        Self { event_type: "file", path, diagnostics, metrics, error }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DoneEvent {
    #[serde(rename = "type")]
    pub event_type: &'static str,

    pub elapsed_secs: f64,

    pub total_files: usize,

    pub total_diagnostics: usize,

    pub failed_files: usize,
}

impl DoneEvent {
    pub fn new(
        elapsed_secs: f64,
        total_files: usize,
        total_diagnostics: usize,
        failed_files: usize,
    ) -> Self {
        Self { event_type: "done", elapsed_secs, total_files, total_diagnostics, failed_files }
    }
}

#[derive(Debug, Clone, Default)]
pub struct JsonlSummary {
    pub total_files: usize,

    pub total_diagnostics: usize,

    pub failed_files: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_event_serialization() {
        let event = StartEvent::new(100);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"start""#));
        assert!(json.contains(r#""total_files":100"#));
        assert!(json.contains(r#""version":"#));
    }

    #[test]
    fn test_file_event_serialization() {
        let event = FileEvent::new(
            "src/Module.bsl".to_string(),
            vec![DiagnosticOutput {
                code: "LineLength".to_string(),
                message: "Line too long".to_string(),
                severity: "Warning".to_string(),
                start_line: 10,
                start_column: 0,
                end_line: 10,
                end_column: 150,
                tags: vec![],
            }],
            Some(FileMetrics { functions: 5, complexity: 10, cognitive_complexity: 8 }),
            None,
        );
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"file""#));
        assert!(json.contains(r#""path":"src/Module.bsl""#));
        assert!(json.contains(r#""diagnostics":"#));
        assert!(json.contains(r#""metrics":"#));
        assert!(!json.contains(r#""error""#));
    }

    #[test]
    fn test_file_event_without_metrics() {
        let event = FileEvent::new("src/Module.bsl".to_string(), vec![], None, None);
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains(r#""metrics""#));
    }

    #[test]
    fn test_file_event_with_error() {
        let event = FileEvent::new(
            "src/Module.bsl".to_string(),
            vec![],
            None,
            Some("Parse error".to_string()),
        );
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""error":"Parse error""#));
    }

    #[test]
    fn test_done_event_serialization() {
        let event = DoneEvent::new(11.2, 6540, 1234, 5);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"done""#));
        assert!(json.contains(r#""elapsed_secs":11.2"#));
        assert!(json.contains(r#""total_files":6540"#));
        assert!(json.contains(r#""total_diagnostics":1234"#));
        assert!(json.contains(r#""failed_files":5"#));
    }
}
