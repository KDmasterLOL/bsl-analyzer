use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, info};
use uuid::Uuid;

use crate::client::{ClientError, DebugClient};
use crate::listener::EventListener;
use crate::module_index::ModuleIndex;
use crate::types::base::{CalcPathItem, ModuleId, StepAction, ViewInterface};
use crate::types::events::DebugEvent;
use crate::types::responses::{self, VarValue};
use crate::types::xml::BreakpointDef;

/// High-level debug session — the main API for AI agents and CLI.
pub struct DebugSession {
    client: Arc<DebugClient>,
    _listener: EventListener,
    events: mpsc::UnboundedReceiver<DebugEvent>,
    index: ModuleIndex,
    attached_targets: HashMap<String, String>, // id -> type
    breakpoints: Vec<BreakpointDef>,
    stopped_target: Option<String>,
    last_stop: Option<StopEvent>,
    watches: Vec<String>,
    /// Buffered async eval results keyed by expressionResultID.
    pending_eval_results: HashMap<String, Vec<u8>>,
}

/// Configuration for connecting to the debug server.
pub struct DebugConfig {
    pub host: String,
    pub port: u16,
    pub infobase: String,
    pub config_root: PathBuf,
    pub extensions: Vec<(String, PathBuf)>,
    pub auto_attach: Vec<String>,
}

/// Reason execution stopped.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StopEvent {
    pub reason: StopReason,
    pub target_id: String,
    pub module: String,
    pub line: u32,
    pub stack: Vec<FrameInfo>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum StopReason {
    Breakpoint,
    Step,
    Exception { message: String },
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FrameInfo {
    pub module_path: String,
    pub module_name: String,
    pub line: u32,
    pub presentation: String,
}

/// A variable with its value.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Variable {
    pub name: String,
    pub type_name: String,
    pub value: String,
    pub expandable: bool,
}

/// Result of expression evaluation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EvalResult {
    pub value: String,
    pub type_name: String,
}

/// An active debug target.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TargetInfo {
    pub id: String,
    pub target_type: String,
    pub user_name: String,
}

/// Result of a single watch expression evaluation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WatchResult {
    pub expression: String,
    pub value: Option<EvalResult>,
    pub error: Option<String>,
}

/// A line of source code with metadata.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceLine {
    pub line: u32,
    pub text: String,
    pub is_current: bool,
}

/// Everything the agent sees when execution stops — the "debugger screen".
#[derive(Debug, Clone, serde::Serialize)]
pub struct StopSnapshot {
    pub reason: StopReason,
    pub target_id: String,
    pub module: String,
    pub line: u32,
    pub source_context: Vec<SourceLine>,
    pub stack: Vec<FrameInfo>,
    pub locals: Vec<Variable>,
    pub watches: Vec<WatchResult>,
}

impl DebugSession {
    /// Connects to the debug server and prepares the session.
    pub fn connect(config: DebugConfig) -> Result<Self, ClientError> {
        let debugger_id = Uuid::new_v4().to_string();

        // Build module index
        let ext_refs: Vec<(&str, &Path)> =
            config.extensions.iter().map(|(n, p)| (n.as_str(), p.as_path())).collect();
        let index =
            ModuleIndex::scan(&config.config_root, &ext_refs).map_err(ClientError::Debug)?;
        info!(modules = index.len(), "module index built");

        // Create client
        let client =
            Arc::new(DebugClient::new(&config.host, config.port, &debugger_id, &config.infobase));

        // Attach to debug server
        let result = client.attach()?;
        match &result {
            crate::types::responses::AttachResult::Ok => {
                info!("attached to debug server");
            }
            crate::types::responses::AttachResult::IBInDebug => {
                info!("infobase already in debug mode — proceeding");
            }
            other => {
                return Err(ClientError::AttachFailed(
                    other.error_message().unwrap_or("unknown error").to_string(),
                ));
            }
        }

        // Init settings
        client.init_settings()?;

        // Set auto-attach
        if !config.auto_attach.is_empty() {
            let types: Vec<&str> = config.auto_attach.iter().map(|s| s.as_str()).collect();
            client.set_auto_attach(&types)?;
        }

        // Start event listener
        let (listener, events) = EventListener::start(client.clone(), 50);

        Ok(Self {
            client,
            _listener: listener,
            events,
            index,
            attached_targets: HashMap::new(),
            breakpoints: Vec::new(),
            stopped_target: None,
            last_stop: None,
            watches: Vec::new(),
            pending_eval_results: HashMap::new(),
        })
    }

    /// Disconnects from the debug server.
    pub fn disconnect(self) -> Result<(), ClientError> {
        self._listener.stop();
        self.client.detach()?;
        info!("detached from debug server");
        Ok(())
    }

    /// Sets a breakpoint by human-readable module name and line number.
    ///
    /// Name format: "ОбщийМодуль.МойМодуль.Модуль" or "Справочник.Товары.МодульОбъекта"
    pub fn set_breakpoint(
        &mut self,
        module_name: &str,
        line: u32,
        condition: Option<&str>,
    ) -> Result<(), ClientError> {
        let (module_id, _path) = self.index.resolve_name(module_name).ok_or_else(|| {
            ClientError::Debug(crate::error::DebugError::ModuleNotFound(module_name.to_string()))
        })?;

        self.breakpoints.push(BreakpointDef {
            extension: module_id.extension.clone(),
            object_id: module_id.object_id.clone(),
            property_id: module_id.property_id.clone(),
            line,
            condition: condition.map(|s| s.to_string()),
        });

        self.client.set_breakpoints(&self.breakpoints)?;
        info!(module_name, line, "breakpoint set");
        Ok(())
    }

    /// Sets a breakpoint by file path and line number.
    pub fn set_breakpoint_by_path(
        &mut self,
        path: &Path,
        line: u32,
        condition: Option<&str>,
    ) -> Result<(), ClientError> {
        let module_id = self.index.module_by_path(path).ok_or_else(|| {
            ClientError::Debug(crate::error::DebugError::ModuleNotFound(path.display().to_string()))
        })?;

        self.breakpoints.push(BreakpointDef {
            extension: module_id.extension.clone(),
            object_id: module_id.object_id.clone(),
            property_id: module_id.property_id.clone(),
            line,
            condition: condition.map(|s| s.to_string()),
        });

        self.client.set_breakpoints(&self.breakpoints)?;
        Ok(())
    }

    /// Removes a specific breakpoint by module name and line.
    pub fn remove_breakpoint(&mut self, module_name: &str, line: u32) -> Result<bool, ClientError> {
        let (module_id, _path) = match self.index.resolve_name(module_name) {
            Some(v) => v,
            None => return Ok(false),
        };
        let before = self.breakpoints.len();
        self.breakpoints.retain(|bp| {
            !(bp.object_id == module_id.object_id
                && bp.property_id == module_id.property_id
                && bp.extension == module_id.extension
                && bp.line == line)
        });
        if self.breakpoints.len() != before {
            self.client.set_breakpoints(&self.breakpoints)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Removes a specific breakpoint by file path and line.
    pub fn remove_breakpoint_by_path(
        &mut self,
        path: &Path,
        line: u32,
    ) -> Result<bool, ClientError> {
        let module_id = match self.index.module_by_path(path) {
            Some(v) => v,
            None => return Ok(false),
        };
        let before = self.breakpoints.len();
        self.breakpoints.retain(|bp| {
            !(bp.object_id == module_id.object_id
                && bp.property_id == module_id.property_id
                && bp.extension == module_id.extension
                && bp.line == line)
        });
        if self.breakpoints.len() != before {
            self.client.set_breakpoints(&self.breakpoints)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Removes all breakpoints.
    pub fn clear_breakpoints(&mut self) -> Result<(), ClientError> {
        self.breakpoints.clear();
        self.client.set_breakpoints(&self.breakpoints)?;
        Ok(())
    }

    /// Enables break on runtime errors, optionally filtering by error text.
    pub fn break_on_error(&self, filter: Option<&str>) -> Result<(), ClientError> {
        self.client.set_break_on_error(true, filter)
    }

    /// Disables break on runtime errors.
    pub fn ignore_errors(&self) -> Result<(), ClientError> {
        self.client.set_break_on_error(false, None)
    }

    /// Number of modules in the index.
    pub fn module_count(&self) -> usize {
        self.index.len()
    }

    /// All registered human-readable module names.
    pub fn module_names(&self) -> Vec<&str> {
        self.index.all_names()
    }

    /// Lists all available debug targets.
    pub fn targets(&self) -> Result<Vec<TargetInfo>, ClientError> {
        let targets = self.client.get_targets()?;
        Ok(targets
            .into_iter()
            .map(|t| TargetInfo { id: t.id, target_type: t.target_type, user_name: t.user_name })
            .collect())
    }

    /// Returns the currently stopped target ID, if any.
    pub fn stopped_target(&self) -> Option<&str> {
        self.stopped_target.as_deref()
    }

    /// Returns the last stop event, if any.
    pub fn last_stop(&self) -> Option<&StopEvent> {
        self.last_stop.as_ref()
    }

    /// Waits for execution to stop (breakpoint, step, or exception).
    ///
    /// Blocks until a stop event is received or timeout expires.
    pub fn wait_for_stop(&mut self, timeout: Duration) -> Result<Option<StopEvent>, ClientError> {
        let deadline = std::time::Instant::now() + timeout;

        loop {
            // Process pending events
            match self.events.try_recv() {
                Ok(event) => {
                    if let Some(stop) = self.handle_event(event)? {
                        self.last_stop = Some(stop.clone());
                        return Ok(Some(stop));
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    if std::time::Instant::now() >= deadline {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Ok(None);
                }
            }
        }
    }

    /// Steps execution (next line).
    pub fn step(&self, action: StepAction) -> Result<(), ClientError> {
        let target_id = self.require_stopped_target()?;

        let action_str = match action {
            StepAction::Next => "Step",
            StepAction::StepIn => "StepIn",
            StepAction::StepOut => "StepOut",
            StepAction::Continue => "Continue",
        };

        self.client.step(target_id, action_str)
    }

    /// Continues execution.
    pub fn continue_execution(&self) -> Result<(), ClientError> {
        self.step(StepAction::Continue)
    }

    /// Gets the call stack of the currently stopped target.
    /// Returns the target ID of the stopped target, falling back to the last stop event.
    fn require_stopped_target(&self) -> Result<&str, ClientError> {
        if let Some(ref id) = self.stopped_target {
            return Ok(id);
        }
        if let Some(ref stop) = self.last_stop {
            return Ok(&stop.target_id);
        }
        Err(ClientError::AttachFailed("no stopped target".to_string()))
    }

    pub fn call_stack(&self) -> Result<Vec<FrameInfo>, ClientError> {
        let target_id = self.require_stopped_target()?;

        let frames = self.client.get_call_stack(target_id)?;

        Ok(frames
            .into_iter()
            .map(|f| {
                let module_id = ModuleId {
                    extension: f.module_extension,
                    object_id: f.module_object_id,
                    property_id: f.module_property_id,
                };
                let module_path = self
                    .index
                    .path_by_module(&module_id)
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();

                FrameInfo {
                    module_path,
                    module_name: module_id.to_string(),
                    line: f.line_no,
                    presentation: f.presentation,
                }
            })
            .collect())
    }

    /// Gets local variables at the given stack level.
    pub fn locals(&mut self, stack_level: u32) -> Result<Vec<Variable>, ClientError> {
        let target_id = self.require_stopped_target()?.to_string();

        let result_id = Uuid::new_v4().to_string();
        let vars = self.client.eval_local_vars(&target_id, stack_level as i64, &result_id)?;

        // evalLocalVariables results typically arrive async via ExprEvaluated event
        let vars = if vars.is_empty() {
            self.wait_for_eval_result(&result_id, Duration::from_secs(5))
        } else {
            vars
        };

        Ok(vars
            .into_iter()
            .map(|v| Variable {
                name: v.name,
                type_name: v.type_name,
                value: v.presentation,
                expandable: v.is_expandable,
            })
            .collect())
    }

    /// Waits for an async eval result to arrive via the ExprEvaluated event.
    ///
    /// The 1C debug server returns evalExpr results asynchronously — the HTTP response
    /// is typically empty, and the actual result arrives via the ping event loop.
    fn wait_for_eval_result(&mut self, result_id: &str, timeout: Duration) -> Vec<VarValue> {
        let deadline = std::time::Instant::now() + timeout;

        loop {
            if let Some(xml) = self.pending_eval_results.remove(result_id) {
                return responses::parse_eval_result(&xml);
            }

            match self.events.try_recv() {
                Ok(event) => {
                    let _ = self.handle_event(event);
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    if std::time::Instant::now() >= deadline {
                        debug!(result_id, "eval result timeout");
                        return Vec::new();
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Vec::new();
                }
            }
        }
    }

    /// Evaluates a 1C expression at the given stack level.
    ///
    /// The result may arrive synchronously in the HTTP response or asynchronously
    /// via the ExprEvaluated event from the ping loop.
    pub fn eval(&mut self, expression: &str, stack_level: u32) -> Result<EvalResult, ClientError> {
        let target_id = self.require_stopped_target()?.to_string();

        let result_id = Uuid::new_v4().to_string();
        let vars = self.client.eval_expr(&target_id, expression, stack_level as i64, &result_id)?;

        // evalExpr results typically arrive async via ExprEvaluated event
        let vars = if vars.is_empty() {
            self.wait_for_eval_result(&result_id, Duration::from_secs(5))
        } else {
            vars
        };

        let first = vars.into_iter().next().unwrap_or(VarValue {
            name: String::new(),
            type_name: String::new(),
            presentation: String::new(),
            is_expandable: false,
            error: None,
        });

        Ok(EvalResult { value: first.presentation, type_name: first.type_name })
    }

    /// Expands a variable to show its children (properties or collection elements).
    ///
    /// For objects/structures, use `ViewInterface::Context`.
    /// For arrays/collections, use `ViewInterface::Collection`.
    pub fn expand(
        &mut self,
        path: &[CalcPathItem],
        view: ViewInterface,
        stack_level: u32,
    ) -> Result<Vec<Variable>, ClientError> {
        let target_id = self.require_stopped_target()?.to_string();

        let result_id = Uuid::new_v4().to_string();
        let vars =
            self.client.eval_expand(&target_id, path, view, stack_level as i64, &result_id)?;

        // expand results may also arrive async
        let vars = if vars.is_empty() {
            self.wait_for_eval_result(&result_id, Duration::from_secs(5))
        } else {
            vars
        };

        Ok(vars
            .into_iter()
            .map(|v| Variable {
                name: v.name,
                type_name: v.type_name,
                value: v.presentation,
                expandable: v.is_expandable,
            })
            .collect())
    }

    /// Adds an expression to the watch list.
    pub fn add_watch(&mut self, expression: &str) {
        if !self.watches.contains(&expression.to_string()) {
            self.watches.push(expression.to_string());
        }
    }

    /// Removes an expression from the watch list.
    pub fn remove_watch(&mut self, expression: &str) {
        self.watches.retain(|w| w != expression);
    }

    /// Returns the current watch list.
    pub fn watch_list(&self) -> &[String] {
        &self.watches
    }

    /// Returns the number of active breakpoints.
    pub fn breakpoint_count(&self) -> usize {
        self.breakpoints.len()
    }

    /// Evaluates all watch expressions at the given stack level.
    pub fn eval_watches(&mut self, stack_level: u32) -> Vec<WatchResult> {
        let expressions: Vec<String> = self.watches.clone();
        expressions
            .iter()
            .map(|expr| match self.eval(expr, stack_level) {
                Ok(result) => {
                    WatchResult { expression: expr.clone(), value: Some(result), error: None }
                }
                Err(e) => WatchResult {
                    expression: expr.clone(),
                    value: None,
                    error: Some(e.to_string()),
                },
            })
            .collect()
    }

    /// Reads source code around the given file path and line.
    ///
    /// Returns ±`context` lines around the target line.
    pub fn source_context(
        &self,
        file_path: &str,
        current_line: u32,
        context: u32,
    ) -> Result<Vec<SourceLine>, ClientError> {
        let path = std::path::Path::new(file_path);
        let content = std::fs::read_to_string(path)
            .map_err(|e| ClientError::Debug(crate::error::DebugError::Io(e)))?;

        let start = current_line.saturating_sub(context).max(1);
        let end = current_line + context;

        Ok(content
            .lines()
            .enumerate()
            .filter_map(|(i, text)| {
                let line_num = (i + 1) as u32;
                if line_num >= start && line_num <= end {
                    Some(SourceLine {
                        line: line_num,
                        text: text.to_string(),
                        is_current: line_num == current_line,
                    })
                } else {
                    None
                }
            })
            .collect())
    }

    /// Builds a complete snapshot of the current stop state from a StopEvent.
    ///
    /// This is the primary API for AI agents — one call to get everything.
    pub fn snapshot_from_stop(
        &mut self,
        stop: &StopEvent,
        source_context_lines: u32,
    ) -> Result<StopSnapshot, ClientError> {
        let locals = self.locals(0).unwrap_or_default();
        let watches = self.eval_watches(0);

        let source_context = if !stop.module.is_empty() && stop.line > 0 {
            self.source_context(&stop.module, stop.line, source_context_lines).unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(StopSnapshot {
            reason: stop.reason.clone(),
            target_id: stop.target_id.clone(),
            module: stop.module.clone(),
            line: stop.line,
            source_context,
            stack: stop.stack.clone(),
            locals,
            watches,
        })
    }

    /// Returns the module index for external use.
    pub fn module_index(&self) -> &ModuleIndex {
        &self.index
    }

    fn handle_event(&mut self, event: DebugEvent) -> Result<Option<StopEvent>, ClientError> {
        match event {
            DebugEvent::TargetStarted { target_id, target_type } => {
                info!(target_id, target_type, "target started");
                self.client.attach_targets(&[&target_id])?;
                self.attached_targets.insert(target_id.clone(), target_type);
                Ok(None)
            }
            DebugEvent::TargetQuit { target_id } => {
                info!(target_id, "target quit");
                self.attached_targets.remove(&target_id);
                if self.stopped_target.as_deref() == Some(&target_id) {
                    self.stopped_target = None;
                }
                Ok(None)
            }
            DebugEvent::CallStackFormed {
                target_id,
                stop_by_bp,
                line_no,
                module_extension,
                module_object_id,
                module_property_id,
                send_message_only,
                call_stack,
                ..
            } => {
                if send_message_only {
                    // Logpoint — continue execution
                    self.client.step(&target_id, "Continue")?;
                    return Ok(None);
                }

                self.stopped_target = Some(target_id.clone());

                let module_id = ModuleId {
                    extension: module_extension,
                    object_id: module_object_id,
                    property_id: module_property_id,
                };
                let module_path = self
                    .index
                    .path_by_module(&module_id)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| module_id.to_string());

                let reason = if stop_by_bp { StopReason::Breakpoint } else { StopReason::Step };

                // Build call stack from event data
                let stack: Vec<FrameInfo> = call_stack
                    .into_iter()
                    .map(|f| {
                        let mid = ModuleId {
                            extension: f.extension,
                            object_id: f.object_id,
                            property_id: f.property_id,
                        };
                        let path = self
                            .index
                            .path_by_module(&mid)
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        FrameInfo {
                            module_path: path,
                            module_name: mid.to_string(),
                            line: f.line_no,
                            presentation: f.presentation,
                        }
                    })
                    .collect();

                // Use deepest stack frame for stop location if top-level is empty
                let (stop_module, stop_line) = if line_no > 0 && !module_path.is_empty() {
                    (module_path, line_no)
                } else if let Some(last) = stack.last() {
                    (last.module_path.clone(), last.line)
                } else {
                    (module_path, line_no)
                };

                Ok(Some(StopEvent {
                    reason,
                    target_id,
                    module: stop_module,
                    line: stop_line,
                    stack,
                }))
            }
            DebugEvent::RuntimeException {
                target_id,
                description,
                line_no,
                module_extension,
                module_object_id,
                module_property_id,
            } => {
                self.stopped_target = Some(target_id.clone());

                let module_id = ModuleId {
                    extension: module_extension,
                    object_id: module_object_id,
                    property_id: module_property_id,
                };
                let module_path = self
                    .index
                    .path_by_module(&module_id)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| module_id.to_string());

                let stack = self.call_stack().unwrap_or_default();

                Ok(Some(StopEvent {
                    reason: StopReason::Exception { message: description },
                    target_id,
                    module: module_path,
                    line: line_no,
                    stack,
                }))
            }
            DebugEvent::ExprEvaluated { result_id, raw_xml } => {
                debug!(result_id, "async expression evaluated, buffering");
                self.pending_eval_results.insert(result_id, raw_xml);
                Ok(None)
            }
        }
    }
}
