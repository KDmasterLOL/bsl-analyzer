mod file_priority;
mod file_processor;
mod jsonl;
mod orchestrator;
mod worker_thread;

pub use file_processor::{DiagnosticOutput, FileProcessor, FileResult};
pub use jsonl::{DoneEvent, FileEvent, FileMetrics, JsonlSummary, StartEvent};
pub use orchestrator::{AnalysisOrchestrator, AnalysisResults, OrchestratorError};
pub use worker_thread::worker_main;
