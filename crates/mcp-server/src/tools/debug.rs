//! Debug tools — wrap `bsl_debug::session::DebugSession` for MCP tool handlers.
//!
//! All functions take `&Arc<Mutex<Option<DebugSession>>>` and are called from
//! `tokio::task::spawn_blocking` in the tool handlers (DebugSession uses blocking I/O).

use bsl_debug::session::{DebugConfig, DebugSession};
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use std::fmt::Write;
use std::sync::{Arc, Mutex};

pub fn debug_attach(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    host: &str,
    port: u16,
    infobase: &str,
    config_root: Option<&str>,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    if guard.is_some() {
        return Err(McpError::invalid_params(
            "Debug session already active. Call debug_disconnect first.",
            None,
        ));
    }
    let root =
        config_root.map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("."));
    let config = DebugConfig {
        host: host.to_string(),
        port,
        infobase: infobase.to_string(),
        config_root: root,
        extensions: Vec::new(),
        auto_attach: vec!["Client".to_string(), "Server".to_string()],
    };
    let session = DebugSession::connect(config)
        .map_err(|e| McpError::internal_error(format!("Failed to connect: {e}"), None))?;
    let targets = session.targets().map_err(|e| {
        McpError::internal_error(format!("Connected but failed to get targets: {e}"), None)
    })?;
    let mut out = format!("Connected to debug server {host}:{port}, infobase: {infobase}\n\n");
    if targets.is_empty() {
        out.push_str("No debug targets available yet. Start a 1C client to begin debugging.\n");
    } else {
        let _ = writeln!(out, "Available targets: {}", targets.len());
        for t in &targets {
            let _ = writeln!(out, "- {} ({})", t.user_name, t.id);
        }
    }
    *guard = Some(session);
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

pub fn debug_disconnect(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.take().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;
    let _ = session.disconnect();
    Ok(CallToolResult::success(vec![Content::text("Debug session disconnected")]))
}

pub fn debug_set_breakpoint(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    module: &str,
    line: u32,
    condition: Option<&str>,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session = guard.as_mut().ok_or_else(|| {
        McpError::invalid_params("No active debug session. Call debug_attach first.", None)
    })?;
    session
        .set_breakpoint(module, line, condition)
        .map_err(|e| McpError::internal_error(format!("Failed to set breakpoint: {e}"), None))?;
    let msg = if let Some(cond) = condition {
        format!("Breakpoint set: {module}:{line} (condition: {cond})")
    } else {
        format!("Breakpoint set: {module}:{line}")
    };
    Ok(CallToolResult::success(vec![Content::text(msg)]))
}

pub fn debug_remove_breakpoint(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    module: &str,
    line: u32,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.as_mut().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;
    let removed = session
        .remove_breakpoint(module, line)
        .map_err(|e| McpError::internal_error(format!("Failed to remove breakpoint: {e}"), None))?;
    if removed {
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Breakpoint removed: {module}:{line}"
        ))]))
    } else {
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Breakpoint not found: {module}:{line}"
        ))]))
    }
}

pub fn debug_continue(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.as_mut().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;
    session
        .continue_execution()
        .map_err(|e| McpError::internal_error(format!("Failed to continue: {e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text("Execution continued")]))
}

pub fn debug_step(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    action: &str,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.as_mut().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;
    let step_action = match action.to_lowercase().as_str() {
        "next" | "over" => bsl_debug::types::base::StepAction::Next,
        "in" | "into" | "stepin" => bsl_debug::types::base::StepAction::StepIn,
        "out" | "stepout" => bsl_debug::types::base::StepAction::StepOut,
        _ => {
            return Err(McpError::invalid_params(
                format!("Unknown step action: '{action}'. Use: next, in, out"),
                None,
            ))
        }
    };
    session
        .step(step_action)
        .map_err(|e| McpError::internal_error(format!("Failed to step: {e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(format!("Step {action}"))]))
}

pub fn debug_wait_stop(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    timeout_secs: Option<u64>,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.as_mut().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;
    let timeout = std::time::Duration::from_secs(timeout_secs.unwrap_or(30));
    let stop = session
        .wait_for_stop(timeout)
        .map_err(|e| McpError::internal_error(format!("Error waiting for stop: {e}"), None))?;
    match stop {
        Some(event) => {
            let mut out = String::new();
            let reason = match &event.reason {
                bsl_debug::session::StopReason::Breakpoint => "breakpoint",
                bsl_debug::session::StopReason::Step => "step",
                bsl_debug::session::StopReason::Exception { message } => {
                    let _ = writeln!(out, "Exception: {message}\n");
                    "exception"
                }
            };
            let _ = writeln!(out, "Stopped (reason: {reason})");
            let _ = writeln!(out, "- Module: {}", event.module);
            let _ = writeln!(out, "- Line: {}", event.line);
            if !event.stack.is_empty() {
                let _ = writeln!(out, "\nCall stack:");
                for (i, frame) in event.stack.iter().enumerate() {
                    let _ = writeln!(out, "  {}: {} (line {})", i, frame.presentation, frame.line);
                }
            }
            Ok(CallToolResult::success(vec![Content::text(out)]))
        }
        None => {
            Ok(CallToolResult::success(vec![Content::text("Timeout — no stop event received")]))
        }
    }
}

pub fn debug_stack_trace(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.as_mut().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;
    let frames = session
        .call_stack()
        .map_err(|e| McpError::internal_error(format!("Failed to get call stack: {e}"), None))?;
    if frames.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "Call stack is empty (not stopped)",
        )]));
    }
    let mut out = format!("# Call Stack ({} frames)\n\n", frames.len());
    for (i, frame) in frames.iter().enumerate() {
        let _ = writeln!(out, "{}. {} (line {})", i, frame.presentation, frame.line);
    }
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

pub fn debug_locals(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    stack_level: Option<u32>,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.as_mut().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;
    let level = stack_level.unwrap_or(0);
    let vars = session
        .locals(level)
        .map_err(|e| McpError::internal_error(format!("Failed to get locals: {e}"), None))?;
    if vars.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No local variables")]));
    }
    let mut out = format!("# Local Variables (stack level {})\n\n", level);
    let _ = writeln!(out, "| Name | Type | Value |");
    let _ = writeln!(out, "|------|------|-------|");
    for v in &vars {
        let expandable = if v.expandable { " >" } else { "" };
        let _ = writeln!(out, "| {}{expandable} | {} | {} |", v.name, v.type_name, v.value);
    }
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

pub fn debug_eval(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    expression: &str,
    stack_level: Option<u32>,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.as_mut().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;
    let level = stack_level.unwrap_or(0);
    let result = session
        .eval(expression, level)
        .map_err(|e| McpError::internal_error(format!("Failed to evaluate: {e}"), None))?;
    let mut out = format!("**{}** = `{}`", expression, result.value);
    if !result.type_name.is_empty() {
        let _ = write!(out, " ({})", result.type_name);
    }
    Ok(CallToolResult::success(vec![Content::text(out)]))
}
