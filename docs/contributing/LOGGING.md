# Logging and Profiling Guide

This guide explains how to use the logging and profiling infrastructure in bsl-analyzer.

## Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `BSL_LOG` | Filter logs using targets syntax | `hir_def=debug,cfg=warn` |
| `BSL_PROFILE` | Hierarchical profiling output | `*@3>10` |
| `BSL_PROFILE_JSON` | JSON profiling export | `*` |
| `BSL_LOG_FILE` | Write logs to file | `/tmp/bsl.log` |

## Log Levels

Use appropriate log levels for different scenarios:

| Level | Usage |
|-------|-------|
| `error` | Unrecoverable errors that affect functionality |
| `warn` | Recoverable errors, degraded functionality |
| `info` | High-level operation milestones (file loaded, analysis complete) |
| `debug` | Detailed operation information for debugging |
| `trace` | Very detailed tracing (individual AST nodes, iterations) |

## Using Tracing in Code

### Basic Logging

```rust
use tracing::{trace, debug, info, warn, error};

// Simple messages
info!("Analysis started");
debug!("Processing file");

// Structured fields
info!(file_count = files.len(), "Loading files");
debug!(file_id = ?file_id, method = %method_name, "Lowering method");
```

### Spans for Profiling

Use spans to measure operation duration:

```rust
pub fn expensive_operation(input: &str) -> Result {
    let _span = tracing::info_span!("expensive_operation", len = input.len()).entered();
    // ... operation code
}
```

Spans are automatically timed when `BSL_PROFILE` is set.

### Span Naming Conventions

- Use snake_case for span names
- Include relevant context as span fields
- Keep span names concise but descriptive

```rust
// Good
let _span = tracing::info_span!("lower_method", method_id = ?id).entered();
let _span = tracing::info_span!("parse_file", file_id = ?file_id, len = input.len()).entered();

// Bad - too verbose
let _span = tracing::info_span!("lowering_method_body_to_hir_representation").entered();
```

## Profiling Output

### Hierarchical Profiling (`BSL_PROFILE`)

Filter syntax: `pattern@depth>threshold_ms`

Examples:
```bash
# Profile all operations
BSL_PROFILE='*' cargo run -- analyze ~/project

# Profile operations taking >50ms, depth 2
BSL_PROFILE='*@2>50' cargo run -- analyze ~/project

# Profile specific operations
BSL_PROFILE='parse|analyze' cargo run -- analyze ~/project
```

Output format:
```
  112ms  cli_analyze
    45ms  load_files
    67ms  run_diagnostics
      23ms  check_file (×6540)
```

### JSON Profiling (`BSL_PROFILE_JSON`)

Outputs newline-delimited JSON for tooling:

```bash
BSL_PROFILE_JSON='*' cargo run -- analyze ~/project 2>timing.jsonl
```

Output format:
```json
{"name":"cli_analyze","elapsed_ms":112}
{"name":"load_files","elapsed_ms":45}
```

## Component-Based Filtering

Filter logs by crate/module using targets syntax:

```bash
# Only hir-def debug logs
BSL_LOG=hir_def=debug cargo run

# Multiple filters
BSL_LOG=hir_def=debug,cfg=warn,dataflow=info cargo run

# All debug with specific trace
BSL_LOG=debug,hir_def::body::lower=trace cargo run
```

## Best Practices

### DO

- Use spans for any operation that might be slow (>1ms)
- Include relevant context as span fields (counts, IDs)
- Use `debug!` for routine operations
- Use `info!` sparingly for major milestones

### DON'T

- Use `println!` or `eprintln!` for debugging (use tracing instead)
- Create spans for trivial operations (<100μs)
- Log sensitive data (file contents, credentials)
- Use `info!` for per-item operations in loops

## Memory and CPU Profiling

The `profile` crate provides `StopWatch` for detailed measurements:

```rust
use profile::StopWatch;

let sw = StopWatch::start();
// ... operation
let span = sw.elapsed();
println!("{}", span); // "123ms, 456ki, 789kb"
                      // time, instructions, memory delta
```

Features:
- CPU instructions (Linux only, via perf_event)
- Memory delta (Linux glibc, Windows, or with jemalloc feature)
- Timing (all platforms)
