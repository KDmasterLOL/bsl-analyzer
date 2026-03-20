//! DAP (Debug Adapter Protocol) server for bsl-debug.
//!
//! Translates DAP protocol messages to [`crate::session::DebugSession`] calls.
//! Reads from stdin and writes to stdout via the `dap` crate.

use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use serde::Deserialize;

use dap::prelude::*;
use dap::server::ServerOutput;
use dap::types::{Breakpoint, Scope, ScopePresentationhint, Source, StackFrame, Thread, Variable};

use crate::session::{DebugConfig, DebugSession, StopReason};
use crate::types::base::{CalcPathItem, StepAction, ViewInterface};

const THREAD_ID: i64 = 1;
const VAR_REF_BASE: i64 = 1000;

/// Information needed to expand a variable.
#[derive(Clone)]
struct ExpandInfo {
    path: Vec<CalcPathItem>,
    stack_level: u32,
}

/// State shared between the main loop and the event listener thread.
struct DapState {
    session: Option<DebugSession>,
    var_refs: HashMap<i64, ExpandInfo>,
    next_var_ref: i64,
    /// Breakpoints keyed by file path.
    breakpoints_by_file: HashMap<String, Vec<u32>>,
}

impl DapState {
    fn new() -> Self {
        Self {
            session: None,
            var_refs: HashMap::new(),
            next_var_ref: VAR_REF_BASE,
            breakpoints_by_file: HashMap::new(),
        }
    }

    fn next_var_ref(&mut self) -> i64 {
        let id = self.next_var_ref;
        self.next_var_ref += 1;
        id
    }

    fn reset_var_refs(&mut self) {
        self.var_refs.clear();
        self.next_var_ref = VAR_REF_BASE;
    }

    fn session_mut(&mut self) -> Result<&mut DebugSession, &'static str> {
        self.session.as_mut().ok_or("no active debug session; send Attach first")
    }
}

/// Custom attach arguments for 1C debugger.
#[derive(Deserialize, Debug)]
struct AttachArgs {
    host: String,
    port: u16,
    infobase: String,
    #[serde(default)]
    config_root: Option<String>,
}

/// Run the DAP adapter loop on stdio.
///
/// Blocks until the client disconnects or sends a Disconnect request.
pub fn run_dap_stdio() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut server = Server::new(BufReader::new(stdin), BufWriter::new(stdout));

    let state = Arc::new(Mutex::new(DapState::new()));
    let stop_flag = Arc::new(AtomicBool::new(false));

    loop {
        let req = match server.poll_request() {
            Ok(Some(r)) => r,
            Ok(None) => {
                tracing::info!("DAP client closed connection");
                break;
            }
            Err(e) => {
                tracing::error!(error = %e, "DAP poll_request error");
                break;
            }
        };

        tracing::debug!(command = ?req.command, seq = req.seq, "DAP request");

        let result = handle_request(req, &mut server, &state, &stop_flag);
        if let Err(e) = result {
            tracing::error!(error = %e, "DAP handle_request error");
            break;
        }
        if stop_flag.load(Ordering::Relaxed) {
            tracing::info!("DAP disconnect received, stopping");
            break;
        }
    }
}

fn handle_request(
    req: requests::Request,
    server: &mut Server<std::io::Stdin, std::io::Stdout>,
    state: &Arc<Mutex<DapState>>,
    stop_flag: &Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match &req.command {
        Command::Initialize(_args) => {
            let rsp = req.success(ResponseBody::Initialize(types::Capabilities {
                supports_configuration_done_request: Some(true),
                supports_function_breakpoints: Some(false),
                supports_conditional_breakpoints: Some(true),
                supports_evaluate_for_hovers: Some(true),
                ..Default::default()
            }));
            server.respond(rsp)?;
            server.send_event(Event::Initialized)?;
        }

        Command::Attach(attach_args) => {
            let result = do_attach(attach_args, state, server);
            match result {
                Ok(()) => {
                    let rsp = req.success(ResponseBody::Attach);
                    server.respond(rsp)?;
                    spawn_event_listener(state.clone(), server.output.clone(), stop_flag.clone());
                }
                Err(e) => {
                    let rsp = req.error(&e.to_string());
                    server.respond(rsp)?;
                }
            }
        }

        Command::ConfigurationDone => {
            let rsp = req.success(ResponseBody::ConfigurationDone);
            server.respond(rsp)?;
        }

        Command::SetBreakpoints(args) => {
            let result = do_set_breakpoints(args, state);
            match result {
                Ok(bps) => {
                    let rsp = req.success(ResponseBody::SetBreakpoints(
                        responses::SetBreakpointsResponse { breakpoints: bps },
                    ));
                    server.respond(rsp)?;
                }
                Err(e) => {
                    let rsp = req.error(&e.to_string());
                    server.respond(rsp)?;
                }
            }
        }

        Command::Threads => {
            let rsp = req.success(ResponseBody::Threads(responses::ThreadsResponse {
                threads: vec![Thread { id: THREAD_ID, name: "Main Thread".to_string() }],
            }));
            server.respond(rsp)?;
        }

        Command::StackTrace(_args) => {
            let result = do_stack_trace(state);
            match result {
                Ok(frames) => {
                    let total = frames.len() as i64;
                    let rsp =
                        req.success(ResponseBody::StackTrace(responses::StackTraceResponse {
                            stack_frames: frames,
                            total_frames: Some(total),
                        }));
                    server.respond(rsp)?;
                }
                Err(e) => {
                    let rsp = req.error(&e.to_string());
                    server.respond(rsp)?;
                }
            }
        }

        Command::Scopes(args) => {
            // frame_id encodes stack level: frame 0 = stack level 0, etc.
            let variables_reference = VAR_REF_BASE - 1 + args.frame_id;
            let rsp = req.success(ResponseBody::Scopes(responses::ScopesResponse {
                scopes: vec![Scope {
                    name: "Locals".to_string(),
                    presentation_hint: Some(ScopePresentationhint::Locals),
                    variables_reference,
                    expensive: false,
                    ..Default::default()
                }],
            }));
            server.respond(rsp)?;
        }

        Command::Variables(args) => {
            let result = do_variables(args.variables_reference, state);
            match result {
                Ok(vars) => {
                    let rsp = req.success(ResponseBody::Variables(responses::VariablesResponse {
                        variables: vars,
                    }));
                    server.respond(rsp)?;
                }
                Err(e) => {
                    let rsp = req.error(&e.to_string());
                    server.respond(rsp)?;
                }
            }
        }

        Command::Continue(_args) => {
            let result = state.lock().unwrap().session_mut().and_then(|s| {
                s.continue_execution()
                    .map_err(|e| Box::leak(e.to_string().into_boxed_str()) as &'static str)
            });
            match result {
                Ok(()) => {
                    state.lock().unwrap().reset_var_refs();
                    let rsp = req.success(ResponseBody::Continue(responses::ContinueResponse {
                        all_threads_continued: Some(true),
                    }));
                    server.respond(rsp)?;
                }
                Err(e) => {
                    let rsp = req.error(e);
                    server.respond(rsp)?;
                }
            }
        }

        Command::Next(_args) => {
            let result = do_step(StepAction::Next, state);
            match result {
                Ok(()) => {
                    state.lock().unwrap().reset_var_refs();
                    let rsp = req.success(ResponseBody::Next);
                    server.respond(rsp)?;
                }
                Err(e) => {
                    let rsp = req.error(&e);
                    server.respond(rsp)?;
                }
            }
        }

        Command::StepIn(_args) => {
            let result = do_step(StepAction::StepIn, state);
            match result {
                Ok(()) => {
                    state.lock().unwrap().reset_var_refs();
                    let rsp = req.success(ResponseBody::StepIn);
                    server.respond(rsp)?;
                }
                Err(e) => {
                    let rsp = req.error(&e);
                    server.respond(rsp)?;
                }
            }
        }

        Command::StepOut(_args) => {
            let result = do_step(StepAction::StepOut, state);
            match result {
                Ok(()) => {
                    state.lock().unwrap().reset_var_refs();
                    let rsp = req.success(ResponseBody::StepOut);
                    server.respond(rsp)?;
                }
                Err(e) => {
                    let rsp = req.error(&e);
                    server.respond(rsp)?;
                }
            }
        }

        Command::Evaluate(args) => {
            let stack_level = 0u32;
            let result = {
                let mut guard = state.lock().unwrap();
                guard
                    .session_mut()
                    .map_err(|e| e.to_string())
                    .and_then(|s| s.eval(&args.expression, stack_level).map_err(|e| e.to_string()))
            };
            match result {
                Ok(eval_result) => {
                    let rsp = req.success(ResponseBody::Evaluate(responses::EvaluateResponse {
                        result: eval_result.value,
                        type_field: Some(eval_result.type_name),
                        variables_reference: 0,
                        ..Default::default()
                    }));
                    server.respond(rsp)?;
                }
                Err(e) => {
                    let rsp = req.error(&e);
                    server.respond(rsp)?;
                }
            }
        }

        Command::Disconnect(_args) => {
            {
                let mut guard = state.lock().unwrap();
                if let Some(session) = guard.session.take() {
                    if let Err(e) = session.disconnect() {
                        tracing::warn!(error = %e, "disconnect error");
                    }
                }
            }
            stop_flag.store(true, Ordering::Relaxed);
            let rsp = req.success(ResponseBody::Disconnect);
            server.respond(rsp)?;
        }

        other => {
            tracing::debug!(command = ?other, "unhandled DAP command");
            let rsp = req.error("command not supported");
            server.respond(rsp)?;
        }
    }

    Ok(())
}

fn do_attach(
    attach_args: &requests::AttachRequestArguments,
    state: &Arc<Mutex<DapState>>,
    _server: &mut Server<std::io::Stdin, std::io::Stdout>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let additional = attach_args
        .additional_data
        .as_ref()
        .ok_or("attach requires additional_data with host/port/infobase")?;

    let args: AttachArgs = serde_json::from_value(additional.clone())?;

    let config_root = args
        .config_root
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let config = DebugConfig {
        host: args.host,
        port: args.port,
        infobase: args.infobase,
        config_root,
        extensions: Vec::new(),
        auto_attach: vec!["Client".to_string(), "Server".to_string()],
    };

    let session = DebugSession::connect(config)?;
    state.lock().unwrap().session = Some(session);
    Ok(())
}

fn do_set_breakpoints(
    args: &requests::SetBreakpointsArguments,
    state: &Arc<Mutex<DapState>>,
) -> Result<Vec<Breakpoint>, String> {
    let path = args.source.path.clone().unwrap_or_default();
    let breakpoints = args.breakpoints.as_deref().unwrap_or(&[]);

    let mut guard = state.lock().unwrap();

    // Remove old breakpoints for this file first.
    let old_lines = guard.breakpoints_by_file.remove(&path).unwrap_or_default();

    if let Some(session) = guard.session.as_mut() {
        let file_path = Path::new(&path);

        for line in &old_lines {
            let _ = session.remove_breakpoint_by_path(file_path, *line);
        }

        // Set new breakpoints.
        let mut new_lines = Vec::new();
        for bp in breakpoints {
            let line = bp.line as u32;
            let condition = bp.condition.as_deref();
            session
                .set_breakpoint_by_path(file_path, line, condition)
                .map_err(|e| e.to_string())?;
            new_lines.push(line);
        }
        guard.breakpoints_by_file.insert(path.clone(), new_lines);
    }

    let result_bps: Vec<Breakpoint> = breakpoints
        .iter()
        .map(|bp| Breakpoint {
            verified: true,
            line: Some(bp.line),
            source: Some(Source { path: Some(path.clone()), ..Default::default() }),
            ..Default::default()
        })
        .collect();

    Ok(result_bps)
}

fn do_stack_trace(state: &Arc<Mutex<DapState>>) -> Result<Vec<StackFrame>, String> {
    let mut guard = state.lock().unwrap();
    let session = guard.session_mut().map_err(|e| e.to_string())?;
    let frames = session.call_stack().map_err(|e| e.to_string())?;

    let dap_frames = frames
        .into_iter()
        .enumerate()
        .map(|(idx, f)| {
            let source = if f.module_path.is_empty() {
                None
            } else {
                Some(Source { path: Some(f.module_path), ..Default::default() })
            };
            StackFrame {
                id: idx as i64,
                name: f.presentation,
                source,
                line: f.line as i64,
                column: 1,
                ..Default::default()
            }
        })
        .collect();

    Ok(dap_frames)
}

fn do_variables(
    variables_reference: i64,
    state: &Arc<Mutex<DapState>>,
) -> Result<Vec<Variable>, String> {
    let mut guard = state.lock().unwrap();

    // Check if this is a locals scope reference (VAR_REF_BASE - 1 + frame_id)
    // Frame 0 → VAR_REF_BASE - 1, Frame 1 → VAR_REF_BASE, etc.
    // Locals scope references start below VAR_REF_BASE.
    if variables_reference < VAR_REF_BASE {
        let stack_level = (variables_reference - (VAR_REF_BASE - 1)).max(0) as u32;
        let session = guard.session_mut().map_err(|e| e.to_string())?;
        let vars = session.locals(stack_level).map_err(|e| e.to_string())?;

        let mut result = Vec::with_capacity(vars.len());
        let mut new_refs: Vec<(i64, ExpandInfo)> = Vec::new();

        for v in vars {
            let var_ref = if v.expandable {
                let ref_id = guard.next_var_ref();
                new_refs.push((
                    ref_id,
                    ExpandInfo {
                        path: vec![CalcPathItem::Expression(v.name.clone())],
                        stack_level,
                    },
                ));
                ref_id
            } else {
                0
            };
            result.push(Variable {
                name: v.name,
                value: v.value,
                type_field: Some(v.type_name),
                variables_reference: var_ref,
                ..Default::default()
            });
        }

        for (ref_id, info) in new_refs {
            guard.var_refs.insert(ref_id, info);
        }

        return Ok(result);
    }

    // Expand a structured variable.
    let expand_info = guard.var_refs.get(&variables_reference).cloned();
    let Some(info) = expand_info else {
        return Ok(Vec::new());
    };

    let session = guard.session_mut().map_err(|e| e.to_string())?;
    let vars = session
        .expand(&info.path, ViewInterface::Context, info.stack_level)
        .map_err(|e| e.to_string())?;

    let mut result = Vec::with_capacity(vars.len());
    let mut new_refs: Vec<(i64, ExpandInfo)> = Vec::new();

    for v in vars {
        let var_ref = if v.expandable {
            let ref_id = guard.next_var_ref();
            let mut child_path = info.path.clone();
            child_path.push(CalcPathItem::Property(v.name.clone()));
            new_refs.push((ref_id, ExpandInfo { path: child_path, stack_level: info.stack_level }));
            ref_id
        } else {
            0
        };
        result.push(Variable {
            name: v.name,
            value: v.value,
            type_field: Some(v.type_name),
            variables_reference: var_ref,
            ..Default::default()
        });
    }

    for (ref_id, info) in new_refs {
        guard.var_refs.insert(ref_id, info);
    }

    Ok(result)
}

fn do_step(action: StepAction, state: &Arc<Mutex<DapState>>) -> Result<(), String> {
    let guard = state.lock().unwrap();
    let session = guard.session.as_ref().ok_or("no active debug session")?;
    session.step(action).map_err(|e| e.to_string())
}

/// Spawns a background thread that polls for debug events and sends DAP events.
fn spawn_event_listener(
    state: Arc<Mutex<DapState>>,
    output: Arc<Mutex<ServerOutput<std::io::Stdout>>>,
    stop_flag: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        tracing::debug!("DAP event listener started");
        loop {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            let stop_event = {
                let mut guard = state.lock().unwrap();
                if let Some(session) = guard.session.as_mut() {
                    match session.wait_for_stop(Duration::from_millis(100)) {
                        Ok(Some(ev)) => Some(ev),
                        Ok(None) => None,
                        Err(e) => {
                            tracing::warn!(error = %e, "wait_for_stop error");
                            None
                        }
                    }
                } else {
                    None
                }
            };

            if let Some(ev) = stop_event {
                tracing::debug!(
                    reason = ?ev.reason,
                    module = %ev.module,
                    line = ev.line,
                    "execution stopped"
                );

                // Reset variable references on each new stop.
                state.lock().unwrap().reset_var_refs();

                let (reason, text, description) = match &ev.reason {
                    StopReason::Breakpoint => (
                        types::StoppedEventReason::Breakpoint,
                        None,
                        Some("Paused on breakpoint".to_string()),
                    ),
                    StopReason::Step => {
                        (types::StoppedEventReason::Step, None, Some("Stepped".to_string()))
                    }
                    StopReason::Exception { message } => (
                        types::StoppedEventReason::Exception,
                        Some(message.clone()),
                        Some("Paused on exception".to_string()),
                    ),
                };

                let dap_event = Event::Stopped(events::StoppedEventBody {
                    reason,
                    description,
                    thread_id: Some(THREAD_ID),
                    text,
                    all_threads_stopped: Some(true),
                    preserve_focus_hint: None,
                    hit_breakpoint_ids: None,
                });

                let mut out = output.lock().unwrap();
                if let Err(e) = out.send_event(dap_event) {
                    tracing::warn!(error = %e, "failed to send Stopped event");
                    break;
                }
            }
        }
        tracing::debug!("DAP event listener stopped");
    });
}
