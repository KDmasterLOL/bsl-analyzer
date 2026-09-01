use crate::tools::response::text_within_budget;
use bsl_debug::session::{DebugConfig, DebugSession};
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::ErrorData as McpError;
use std::fmt::Write;
use std::sync::{Arc, Mutex};

const DEFAULT_AUTO_ATTACH: &[&str] = &["Client", "Server", "HTTPService"];

/// A deep stack, a frame full of locals, or an evaluated collection has no size of its own,
/// so the state-reading actions render through the output budget.
const STACK_NOTE: &str =
    "\n-- stack truncated to fit max_output_tokens; raise the budget to see deeper frames --\n";
const LOCALS_NOTE: &str = "\n-- locals truncated to fit max_output_tokens; raise the budget, or \
                           read single names with action=eval --\n";
const EVAL_NOTE: &str = "\n-- value truncated to fit max_output_tokens; raise the budget or \
                         evaluate a narrower expression --\n";
const TARGETS_NOTE: &str = "\n-- target list truncated to fit max_output_tokens; raise the \
                            budget to see the rest --\n";

pub struct AttachParams<'a> {
    pub host: &'a str,
    pub port: u16,
    pub infobase: &'a str,
    pub config_root: Option<&'a str>,
    pub workspace_root: Option<&'a std::path::Path>,
    pub extensions: &'a [[String; 2]],
    pub auto_attach: &'a [String],
}

pub fn debug_attach(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    params: AttachParams<'_>,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    if guard.is_some() {
        return Err(McpError::invalid_params(
            "Debug session already active. Call debug_disconnect first.",
            None,
        ));
    }

    let AttachParams { host, port, infobase, config_root, workspace_root, extensions, auto_attach } =
        params;

    let root = if let Some(cr) = config_root {
        std::path::PathBuf::from(cr)
    } else if let Some(wr) = workspace_root {
        wr.to_path_buf()
    } else {
        std::path::PathBuf::from(".")
    };

    let ext_pairs: Vec<(String, std::path::PathBuf)> =
        extensions.iter().map(|e| (e[0].clone(), std::path::PathBuf::from(&e[1]))).collect();

    let attach_types: Vec<String> = if auto_attach.is_empty() {
        DEFAULT_AUTO_ATTACH.iter().map(|s| s.to_string()).collect()
    } else {
        auto_attach.to_vec()
    };

    let config = DebugConfig {
        host: host.to_string(),
        port,
        infobase: infobase.to_string(),
        config_root: root.clone(),
        extensions: ext_pairs,
        auto_attach: attach_types.clone(),
    };
    let session = DebugSession::connect(config)
        .map_err(|e| McpError::internal_error(format!("Failed to connect: {e}"), None))?;
    let targets = session.targets().map_err(|e| {
        McpError::internal_error(format!("Connected but failed to get targets: {e}"), None)
    })?;
    let mut out = format!("Connected to debug server {host}:{port}, infobase: {infobase}\n");
    let _ = writeln!(out, "Config root: {}", root.display());
    let _ = writeln!(out, "Modules indexed: {}", session.module_count());
    let _ = writeln!(out, "Auto-attach: {}\n", attach_types.join(", "));
    if targets.is_empty() {
        out.push_str(
            "No debug targets available yet. \
             Start a 1C client or trigger an HTTP service request to begin debugging.\n",
        );
    } else {
        let _ = writeln!(out, "Available targets: {}", targets.len());
        for t in &targets {
            let _ = writeln!(out, "- {} ({})", t.user_name, t.id);
        }
    }
    *guard = Some(session);
    Ok(text_within_budget(out, max_output_tokens, TARGETS_NOTE))
}

pub fn debug_disconnect(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.take().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;
    let _ = session.disconnect();
    Ok(CallToolResult::success(vec![ContentBlock::text("Debug session disconnected")]))
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
    if let Err(e) = session.set_breakpoint(module, line, condition) {
        let mut msg = format!("Failed to set breakpoint: {e}");
        let names = session.module_names();
        if names.is_empty() {
            msg.push_str("\n\nModule index is empty — check config_root in debug_attach.");
        } else {
            msg.push_str(&format!("\n\nAvailable modules ({}):", names.len()));
            for name in names.iter().take(20) {
                msg.push_str(&format!("\n  - {name}"));
            }
            if names.len() > 20 {
                msg.push_str(&format!("\n  ... and {} more", names.len() - 20));
            }
        }
        return Err(McpError::internal_error(msg, None));
    }
    let msg = if let Some(cond) = condition {
        format!("Breakpoint set: {module}:{line} (condition: {cond})")
    } else {
        format!("Breakpoint set: {module}:{line}")
    };
    Ok(CallToolResult::success(vec![ContentBlock::text(msg)]))
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
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "Breakpoint removed: {module}:{line}"
        ))]))
    } else {
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
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
    Ok(CallToolResult::success(vec![ContentBlock::text("Execution continued")]))
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
    Ok(CallToolResult::success(vec![ContentBlock::text(format!("Step {action}"))]))
}

pub fn debug_wait_stop(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    timeout_secs: Option<u64>,
    max_output_tokens: usize,
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
            Ok(text_within_budget(out, max_output_tokens, STACK_NOTE))
        }
        None => Ok(CallToolResult::success(vec![ContentBlock::text(
            "Timeout — no stop event received",
        )])),
    }
}

pub fn debug_stack_trace(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.as_mut().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;

    let call_stack_result = session.call_stack();
    let frames = match call_stack_result {
        Ok(ref f) if !f.is_empty() => f.clone(),
        _ => {
            let err_info = match &call_stack_result {
                Ok(f) => format!("call_stack returned {} frames", f.len()),
                Err(e) => format!("call_stack error: {e}"),
            };
            let has_stopped = session.stopped_target().is_some();
            let has_last_stop = session.last_stop().is_some();
            let last_stop_stack_len = session.last_stop().map(|s| s.stack.len()).unwrap_or(0);

            if let Some(stop) = session.last_stop() {
                if !stop.stack.is_empty() {
                    stop.stack.clone()
                } else {
                    return Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                        "Call stack is empty.\nDiag: {err_info}, stopped_target={has_stopped}, \
                         last_stop={has_last_stop}, last_stop_stack={last_stop_stack_len}"
                    ))]));
                }
            } else {
                return Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                    "Call stack is empty.\nDiag: {err_info}, stopped_target={has_stopped}, \
                     last_stop={has_last_stop}"
                ))]));
            }
        }
    };

    let mut out = format!("# Call Stack ({} frames)\n\n", frames.len());
    for (i, frame) in frames.iter().enumerate() {
        let _ = writeln!(out, "{}. {} (line {})", i, frame.presentation, frame.line);
    }
    Ok(text_within_budget(out, max_output_tokens, STACK_NOTE))
}

pub fn debug_locals(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    stack_level: Option<u32>,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.as_mut().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;
    let level = stack_level.unwrap_or(0);
    let vars = session
        .locals(level)
        .map_err(|e| McpError::internal_error(format!("Failed to get locals: {e}"), None))?;
    if vars.is_empty() {
        return Ok(CallToolResult::success(vec![ContentBlock::text("No local variables")]));
    }
    let mut out = format!("# Local Variables (stack level {})\n\n", level);
    let _ = writeln!(out, "| Name | Type | Value |");
    let _ = writeln!(out, "|------|------|-------|");
    for v in &vars {
        let expandable = if v.expandable { " >" } else { "" };
        let _ = writeln!(out, "| {}{expandable} | {} | {} |", v.name, v.type_name, v.value);
    }
    Ok(text_within_budget(out, max_output_tokens, LOCALS_NOTE))
}

pub fn debug_eval(
    session_mutex: &Arc<Mutex<Option<DebugSession>>>,
    expression: &str,
    stack_level: Option<u32>,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let mut guard = session_mutex.lock().unwrap();
    let session =
        guard.as_mut().ok_or_else(|| McpError::invalid_params("No active debug session", None))?;
    let level = stack_level.unwrap_or(0);
    let result = session
        .eval(expression, level)
        .map_err(|e| McpError::internal_error(format!("Failed to evaluate: {e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(format_eval(
        expression,
        &result.value,
        &result.type_name,
        max_output_tokens,
    ))]))
}

/// Both the evaluated value and the echoed expression are caller/runtime-sized. The value is
/// clipped first with the type suffix reserved out of the budget — a clipped value must not
/// cost the reader the type it evaluated to — and the composed body then passes a hard ceiling
/// that also catches an oversized expression.
fn format_eval(expression: &str, value: &str, type_name: &str, max_output_tokens: usize) -> String {
    let suffix = if type_name.is_empty() { String::new() } else { format!(" ({type_name})") };
    let reserved = (format!("**{expression}** = ``{suffix}").len() + EVAL_NOTE.len()).div_ceil(4);
    let mut clipped = value.to_string();
    let cut = crate::tools::response::truncate_text_to_budget(
        &mut clipped,
        max_output_tokens.saturating_sub(reserved).max(1),
        " …",
    );
    let mut out = format!("**{expression}** = `{clipped}`{suffix}");
    let ceiling_hit =
        crate::tools::response::truncate_text_to_budget(&mut out, max_output_tokens, EVAL_NOTE);
    if cut && !ceiling_hit {
        out.push_str(EVAL_NOTE);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_within_budget_is_untouched() {
        assert_eq!(format_eval("Итог", "42", "Число", 6000), "**Итог** = `42` (Число)");
    }

    #[test]
    fn eval_clips_a_huge_value_but_never_the_type() {
        let out = format_eval("Таблица", &"v".repeat(10_000), "ТаблицаЗначений", 200);
        assert!(out.contains("` (ТаблицаЗначений)"), "type must survive: {out}");
        assert!(out.ends_with(EVAL_NOTE), "must say it clipped: {out}");
        assert!(out.len() <= 200 * 4, "must stay inside the budget: {}", out.len());
    }

    #[test]
    fn eval_ceiling_also_catches_an_oversized_expression() {
        // A short result leaves the value untouched, so only the echoed expression can blow
        // the budget — it must still be cut and marked, never read as a complete answer.
        let out = format_eval(&"э".repeat(10_000), "1", "Число", 100);
        assert!(out.len() <= 100 * 4, "must stay inside the budget: {}", out.len());
        assert!(out.ends_with(EVAL_NOTE), "an expression-driven cut must be marked: {out}");
    }
}
