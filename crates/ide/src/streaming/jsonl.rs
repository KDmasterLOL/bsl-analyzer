//! JSON Lines event structures for streaming output.
//!
//! This module provides structures for streaming analysis results in JSON Lines format.
//! Each event is serialized as a single JSON object on one line, enabling real-time
//! processing and low memory consumption.
//!
//! ## Format
//!
//! ```jsonl
//! {"type":"start","total_files":6540,"version":"0.2.0"}
//! {"type":"file","path":"src/Module.bsl","diagnostics":[...],"metrics":{...}}
//! {"type":"done","elapsed_secs":11.2,"total_files":6540,"total_diagnostics":1234,"failed_files":0}
//! ```
//!
//! ## Usage
//!
//! This format is designed for:
//! - SonarQube integration (streaming import)
//! - Real-time monitoring of analysis progress
//! - Memory-efficient processing of large codebases

use serde::Serialize;

use ide_diagnostics::DiagnosticOutput;

/// Start event emitted at the beginning of analysis.
#[derive(Debug, Clone, Serialize)]
pub struct StartEvent {
    /// Event type marker ("start").
    #[serde(rename = "type")]
    pub event_type: &'static str,

    /// Total number of files to analyze.
    pub total_files: usize,

    /// Analyzer version.
    pub version: &'static str,
}

impl StartEvent {
    /// Create a new start event.
    pub fn new(total_files: usize) -> Self {
        Self { event_type: "start", total_files, version: env!("CARGO_PKG_VERSION") }
    }
}

/// File metrics (optional, computed during analysis).
#[derive(Debug, Clone, Default, Serialize)]
pub struct FileMetrics {
    /// Number of functions/procedures in the file.
    pub functions: usize,

    /// Cyclomatic complexity sum for all methods.
    pub complexity: u32,

    /// Cognitive complexity sum for all methods.
    pub cognitive_complexity: u32,
}

/// File event emitted for each processed file.
#[derive(Debug, Clone, Serialize)]
pub struct FileEvent {
    /// Event type marker ("file").
    #[serde(rename = "type")]
    pub event_type: &'static str,

    /// File path (relative or absolute).
    pub path: String,

    /// Diagnostics found in this file.
    pub diagnostics: Vec<DiagnosticOutput>,

    /// Optional file metrics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<FileMetrics>,

    /// Optional error message if processing failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl FileEvent {
    /// Create a new file event.
    pub fn new(
        path: String,
        diagnostics: Vec<DiagnosticOutput>,
        metrics: Option<FileMetrics>,
        error: Option<String>,
    ) -> Self {
        Self { event_type: "file", path, diagnostics, metrics, error }
    }
}

/// Done event emitted at the end of analysis.
#[derive(Debug, Clone, Serialize)]
pub struct DoneEvent {
    /// Event type marker ("done").
    #[serde(rename = "type")]
    pub event_type: &'static str,

    /// Total elapsed time in seconds.
    pub elapsed_secs: f64,

    /// Total number of files processed.
    pub total_files: usize,

    /// Total number of diagnostics found.
    pub total_diagnostics: usize,

    /// Number of files that failed to process.
    pub failed_files: usize,
}

impl DoneEvent {
    /// Create a new done event.
    pub fn new(
        elapsed_secs: f64,
        total_files: usize,
        total_diagnostics: usize,
        failed_files: usize,
    ) -> Self {
        Self { event_type: "done", elapsed_secs, total_files, total_diagnostics, failed_files }
    }
}

/// Summary of JSONL streaming analysis.
///
/// Returned by `analyze_jsonl()` for programmatic access to statistics.
#[derive(Debug, Clone, Default)]
pub struct JsonlSummary {
    /// Total number of files processed.
    pub total_files: usize,

    /// Total number of diagnostics found.
    pub total_diagnostics: usize,

    /// Number of files that failed to process.
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
        assert!(!json.contains(r#""error""#)); // Should be skipped when None
    }

    #[test]
    fn test_file_event_without_metrics() {
        let event = FileEvent::new("src/Module.bsl".to_string(), vec![], None, None);
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains(r#""metrics""#)); // Should be skipped when None
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
