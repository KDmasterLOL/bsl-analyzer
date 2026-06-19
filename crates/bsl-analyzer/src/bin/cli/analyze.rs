use std::{
    error::Error,
    fs,
    path::PathBuf,
    sync::{atomic::AtomicUsize, Arc},
};

use clap::ValueEnum;
use ide_db::metadata;

#[derive(Debug, Clone, Default, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Console,
    Jsonl,
}

struct FileTiming {
    path: PathBuf,
    duration: std::time::Duration,
}

struct ProfilingStats {
    diagnostic_name: String,
    total: std::time::Duration,
    count: usize,
    max: std::time::Duration,
    max_file: PathBuf,
}

impl ProfilingStats {
    fn from_timings(diagnostic_name: String, timings: Vec<FileTiming>) -> Option<Self> {
        if timings.is_empty() {
            return None;
        }

        let total: std::time::Duration = timings.iter().map(|t| t.duration).sum();
        let count = timings.len();
        let max_timing = timings.iter().max_by_key(|t| t.duration)?;

        Some(Self {
            diagnostic_name,
            total,
            count,
            max: max_timing.duration,
            max_file: max_timing.path.clone(),
        })
    }

    fn average(&self) -> std::time::Duration {
        if self.count == 0 {
            std::time::Duration::ZERO
        } else {
            self.total / self.count as u32
        }
    }

    fn print(&self) {
        println!();
        println!("=== Diagnostic Profiling: {} ===", self.diagnostic_name);
        println!("Files analyzed: {}", self.count);
        println!("Total time:     {:.3}ms", self.total.as_secs_f64() * 1000.0);
        println!("Average time:   {:.3}ms", self.average().as_secs_f64() * 1000.0);
        println!("Max time:       {:.3}ms", self.max.as_secs_f64() * 1000.0);
        println!("Max file:       {}", self.max_file.display());
    }
}

/// Best-effort human-readable message from a caught panic payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic during analysis".to_string()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn analyze(
    source_dir: PathBuf,
    workspace_dir: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    config_path: Option<PathBuf>,
    reporters: Vec<String>,
    quiet: bool,
    _incremental: bool,
    _changed_files: Option<Vec<PathBuf>>,
    _git_diff: Option<String>,
    streaming: bool,
    workers: Option<usize>,
    format: OutputFormat,
    only_diagnostic: Option<String>,
    diff_filter_path: Option<PathBuf>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let diff_filter = if let Some(ref path) = diff_filter_path {
        Some(bsl_analyzer::diff_filter::DiffFilter::load(path)?)
    } else {
        None
    };

    if streaming {
        tracing::warn!(
            "The --streaming analyzer is deprecated and will be removed; Salsa is the default."
        );
        analyze_streaming(
            source_dir,
            workspace_dir,
            output_dir,
            config_path,
            reporters,
            quiet,
            workers,
            format,
            only_diagnostic,
            diff_filter,
        )
    } else {
        analyze_salsa(
            source_dir,
            workspace_dir,
            output_dir,
            config_path,
            reporters,
            quiet,
            workers,
            format,
            only_diagnostic,
            diff_filter,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn analyze_salsa(
    source_dir: PathBuf,
    workspace_dir: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    config_path: Option<PathBuf>,
    reporters: Vec<String>,
    quiet: bool,
    workers: Option<usize>,
    format: OutputFormat,
    only_diagnostic: Option<String>,
    diff_filter: Option<bsl_analyzer::diff_filter::DiffFilter>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::{
        panic::{catch_unwind, AssertUnwindSafe},
        sync::atomic::Ordering,
        time::Instant,
    };

    use base_db::SourceDatabase;
    use ide::streaming::FileMetrics;
    use ide::{DiagnosticsConfig, DiagnosticsContext, RootDatabaseImpl};
    use ide_db::AnalysisProvider;
    use indicatif::{ProgressBar, ProgressStyle};
    use rayon::prelude::*;
    use vfs::FileId;
    use walkdir::WalkDir;

    use bsl_analyzer::reporters::{AnalysisResults, FileAnalysis, ReporterRegistry};

    let _span = tracing::info_span!("cli_analyze").entered();

    let profiling_enabled = only_diagnostic.is_some();
    let jsonl = matches!(format, OutputFormat::Jsonl);

    // Honour `--workers` by sizing the global rayon pool before any rayon use
    // (metadata load and the per-chunk `par_iter` both draw from it). Must run
    // before `load_metadata`, which initialises the pool on first parallel read;
    // best-effort because the pool can only be built once per process.
    if let Some(w) = workers.filter(|w| *w > 0) {
        if let Err(e) = rayon::ThreadPoolBuilder::new().num_threads(w).build_global() {
            tracing::warn!("Failed to set worker count to {w}: {e}");
        }
    }

    tracing::info!("Analyzing project: {:?}", source_dir);
    tracing::info!("Reporters: {:?}", reporters);
    tracing::info!("Quiet mode: {}", quiet);
    if let Some(ref diag) = only_diagnostic {
        tracing::info!("Profiling diagnostic: {}", diag);
    }
    if diff_filter.is_some() {
        tracing::info!("Diff filter enabled");
    }

    let start = Instant::now();

    let workspace_dir = workspace_dir.unwrap_or_else(|| source_dir.clone());
    let output_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));

    tracing::info!("Loading project configuration");
    let proj_config = if let Some(ref cfg) = config_path {
        project_model::ProjectConfig::load_from_file(cfg).unwrap_or_default()
    } else {
        project_model::ProjectConfig::load(&source_dir).unwrap_or_default()
    };

    let _metadata = proj_config.load_metadata(&source_dir);
    let configuration_path = proj_config.configuration_path(&source_dir);

    // Scope the file walk to the configuration source root (+ extension roots)
    // instead of the raw `-s` dir, so vendored/build copies such as
    // `.build/vendor` are not analyzed as a duplicate configuration.
    let project = project_model::Project::with_config(&source_dir, proj_config.clone());
    let source_roots = project.source_roots();

    tracing::info!("Creating database");
    let mut db = RootDatabaseImpl::default();

    // Register the base + extension configuration roots so per-file resolution — and the
    // `&ИзменениеИКонтроль` effective merge (`effective_target` → `pair_base_module_path`) —
    // can pair an extension module to its base. Mirrors the LSP workspace loader; without
    // this `all_config_paths` is empty and extension merging silently never activates.
    {
        let mut config_paths: Vec<(Option<String>, PathBuf)> =
            vec![(None, project.source_path().to_path_buf())];
        for (name, ext_path) in project.extension_paths() {
            config_paths.push((Some(name.clone()), ext_path.clone()));
        }
        db.set_all_config_paths(config_paths);
    }

    tracing::info!("Finding BSL files in {:?}", source_roots);
    let mut bsl_files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in &source_roots {
        for entry in WalkDir::new(root).follow_links(true) {
            let entry = entry?;
            if entry.file_type().is_file()
                && entry.path().extension().is_some_and(|ext| ext == "bsl")
            {
                let path = entry.path().to_path_buf();
                if seen.insert(path.clone()) {
                    bsl_files.push(path);
                }
            }
        }
    }

    tracing::info!(
        "Found {} BSL files across {} source root(s)",
        bsl_files.len(),
        source_roots.len()
    );

    tracing::info!("Loading files into database");
    let mut file_set = vfs::FileSet::new();
    let mut all_file_ids: Vec<(FileId, PathBuf)> = Vec::new();

    for (idx, path) in bsl_files.iter().enumerate() {
        let file_id = FileId(idx as u32);
        let vfs_path = vfs::VfsPath::new(path.clone());
        file_set.insert(file_id, vfs_path);
        all_file_ids.push((file_id, path.clone()));
    }

    let file_ids: Vec<(FileId, PathBuf)> = if let Some(ref filter) = diff_filter {
        let filtered: Vec<_> = all_file_ids
            .iter()
            .filter(|(_, path)| {
                let rel_path = path.strip_prefix(&source_dir).unwrap_or(path);
                filter.should_analyze(rel_path)
            })
            .cloned()
            .collect();
        tracing::info!(
            "After diff filter: {} files to analyze (from {} total in VFS)",
            filtered.len(),
            all_file_ids.len()
        );
        filtered
    } else {
        all_file_ids.clone()
    };

    let file_set_arc = Arc::new(file_set);
    let source_root_id = base_db::SourceRootId(0);
    let source_root = base_db::SourceRoot::new_local((*file_set_arc).clone());
    db.set_source_root(source_root_id, source_root);

    // Disk-backed: record each file's content revision and drop the text (re-read on
    // demand by `file_text_query` under the salsa LRU, verified against the revision),
    // via the shared resident-load primitive. A file that cannot be read aborts the
    // run, preserving the previous `?` behaviour.
    let unreadable =
        ide_host_core::register_files_disk_backed(&mut db, source_root_id, &all_file_ids);
    if let Some((path, err)) = unreadable.into_iter().next() {
        return Err(format!("failed to read BSL file {}: {err}", path.display()).into());
    }

    tracing::info!(
        "Files loaded, starting parallel diagnostics (threads: {})",
        rayon::current_num_threads()
    );

    let progress = if !quiet && !jsonl {
        let pb = ProgressBar::new(file_ids.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    let mut config = DiagnosticsConfig::from_project_json(
        &proj_config.diagnostics,
        proj_config.output.resolve_locale().unwrap_or_default(),
    );

    if let Some(ref diag_name) = only_diagnostic {
        config.apply_cli_filters(std::slice::from_ref(diag_name), &[]);
    }

    tracing::info!(
        disabled = config.disabled.len(),
        only_enabled = ?config.only_enabled.as_ref().map(|v| v.len()),
        params = config.parameters.len(),
        locale = ?config.locale,
        "Loaded DiagnosticsConfig"
    );
    let config = Arc::new(config);
    let processed = Arc::new(AtomicUsize::new(0));
    let progress_arc = Arc::new(progress);
    let workspace_dir_arc = Arc::new(workspace_dir.clone());
    let source_dir_arc = Arc::new(source_dir.clone());
    let diff_filter_arc = Arc::new(diff_filter);
    // Process in chunks, trimming salsa's LRU caches and the parser's green-node
    // cache between chunks. A single-revision batch never trips salsa's automatic
    // (revision-boundary) eviction, so without this every file's heavy memos
    // (parse/HIR/inference) stay resident at once. Each query is trimmed only to
    // its own `lru` cap, so the durable cross-module currency (method return
    // types, generously capped) is largely retained while the heavy per-file
    // memos fall out once their chunk is done.
    // Chunk size is the peak-memory <-> wall-time knob (the in-chunk working set
    // is the peak, bounded only by this). On ERP 500 trims peak RSS ~25% vs 1000
    // (~6.0 GB vs ~8.1 GB) for ~+5% wall, so it is the default; override via
    // `BSL_SALSA_CHUNK` (larger = faster + more RSS, smaller = leaner + slower).
    let chunk_size = std::env::var("BSL_SALSA_CHUNK")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(500);
    // Only read complexity into the JSONL `metrics` field when BOTH complexity
    // diagnostics are enabled: then both queries are warm during the chunk and
    // reading them is a salsa cache hit (free). Requiring both (not either) is
    // deliberate — `module_cyclomatic_query` builds CFGs, so if only the
    // cognitive diagnostic ran the cyclomatic read would be a cold recompute.
    // When either is off (e.g. `--only-diagnostic`, or disabled in config) the
    // whole metrics block is skipped, keeping the run cost flat.
    let emit_metrics = jsonl
        && !config.is_disabled(ide::DiagnosticCode::CognitiveComplexity)
        && !config.is_disabled(ide::DiagnosticCode::CyclomaticComplexity);

    type FileResult =
        (Option<FileAnalysis>, Option<FileTiming>, Option<FileMetrics>, Option<String>);
    let mut results: Vec<FileResult> = Vec::with_capacity(file_ids.len());
    let report_mem = std::env::var_os("BSL_MEM_REPORT").is_some();
    let chunk_profile = std::env::var_os("BSL_CHUNK_PROFILE").is_some();
    // For modules with >= N methods, warm body inference in parallel before
    // computing diagnostics, so a giant module does not stall its chunk
    // single-threaded. On by default: union ordering is deterministic, so the
    // output is identical, wall drops ~24% on large configs and peak RSS is
    // unchanged. `BSL_PRIME_GIANTS=<N>` overrides the threshold, `0` disables.
    let prime_giants: Option<usize> = match std::env::var("BSL_PRIME_GIANTS") {
        Ok(value) => value.parse::<usize>().ok().filter(|n| *n > 0),
        Err(_) => Some(700),
    };
    let num_chunks = file_ids.len().div_ceil(chunk_size);

    // The JSONL contract opens with a `start` event before any analysis. Salsa
    // emits the `file`/`done` events as a batch once the chunked run finishes
    // (it collects and evicts per chunk rather than streaming live), so only the
    // opener is hoisted here; content and ordering still mirror the streaming path.
    if jsonl {
        use ide::streaming::StartEvent;

        println!("{}", serde_json::to_string(&StartEvent::new(file_ids.len()))?);
    }

    for (chunk_idx, chunk) in file_ids.chunks(chunk_size).enumerate() {
        // Per-chunk timing to separate the parallel phase from the serial tail
        // (the slowest single file's straggler wait) and the single-threaded
        // `enforce_lru` trim — the two work-starvation sources between chunks.
        let max_file_us = std::sync::atomic::AtomicU64::new(0);
        let par_start = std::time::Instant::now();
        let mut chunk_results: Vec<FileResult> = {
            let config_path_input = configuration_path
                .as_ref()
                .map(|path| metadata::intern_configuration_path(&db, &path.to_string_lossy(), 0));
            chunk
                .par_iter()
                .map_with(db.clone(), |db_snapshot, (file_id, path)| {
                    if let Some(min) = prime_giants {
                        db_snapshot.prime_module_inference(*file_id, min);
                    }
                    let provider = ide_db::SalsaProvider::with_file_set(
                        db_snapshot,
                        config_path_input,
                        Some(&file_set_arc),
                    );
                    let ctx = DiagnosticsContext::new(&config, *file_id, &provider);

                    let file_start = std::time::Instant::now();
                    // Standalone pass, then the `&ИзменениеИКонтроль` effective merge — both
                    // reuse the run-global `config_path_input` + `file_set` so the effective
                    // pass resolves metadata/cross-module context exactly as the standalone one.
                    let diagnostics = match catch_unwind(AssertUnwindSafe(|| {
                        let standalone = ide::compute_diagnostics(&ctx);
                        ide::apply_extension_merge(
                            db_snapshot,
                            *file_id,
                            &config,
                            config_path_input,
                            Some(&file_set_arc),
                            standalone,
                        )
                    })) {
                        Ok(diags) => diags,
                        Err(e) => {
                            let message = panic_message(e.as_ref());
                            tracing::error!("Panic analyzing {:?}: {}", path, message);
                            return (None, None, None, Some(message));
                        }
                    };

                    // Read the already-memoised complexity for the JSONL `metrics`
                    // field: when the complexity diagnostics are enabled they have
                    // just run, so these queries are warm here (a salsa cache hit).
                    // Computed before the per-chunk LRU eviction, since at the
                    // output stage the memos are gone. `emit_metrics` is false for
                    // non-JSONL output and when complexity is off, so neither the
                    // reporter path nor a filtered profiling run pays anything.
                    // Wrapped in its own panic boundary (like the diagnostics pass)
                    // so a query panic drops only this file's metrics, not the chunk.
                    let metrics = if emit_metrics {
                        catch_unwind(AssertUnwindSafe(|| {
                            let hir = provider.module_hir_metrics(*file_id);
                            let cyclomatic = provider.module_cyclomatic(*file_id);
                            FileMetrics {
                                functions: hir.len(),
                                complexity: cyclomatic.total(),
                                cognitive_complexity: hir.total_cognitive(),
                            }
                        }))
                        .ok()
                    } else {
                        None
                    };
                    let elapsed = file_start.elapsed();
                    max_file_us.fetch_max(elapsed.as_micros() as u64, Ordering::Relaxed);

                    let timing = if profiling_enabled {
                        Some(FileTiming { path: path.clone(), duration: elapsed })
                    } else {
                        None
                    };

                    let count = processed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(ref pb) = &*progress_arc {
                        pb.set_position(count as u64);
                        pb.set_message(format!(
                            "{:.0} files/sec",
                            count as f64 / start.elapsed().as_secs_f64()
                        ));
                    }

                    let file_analysis = if !diagnostics.is_empty() {
                        let file_text = match fs::read_to_string(path) {
                            Ok(text) => text,
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to read file {:?} for diagnostic conversion: {}",
                                    path,
                                    e
                                );
                                return (
                                    None,
                                    timing,
                                    metrics,
                                    Some(format!(
                                        "failed to read file for diagnostic conversion: {e}"
                                    )),
                                );
                            }
                        };

                        let file_line_index = line_index::LineIndex::new(&file_text);
                        let mut diagnostic_outputs: Vec<_> = diagnostics
                            .iter()
                            .map(|d| d.to_output_with_index(&file_text, &file_line_index))
                            .collect();

                        if let Some(ref filter) = *diff_filter_arc {
                            let rel_path = path.strip_prefix(&*source_dir_arc).unwrap_or(path);
                            diagnostic_outputs.retain(|d| {
                                filter.diagnostic_in_diff(
                                    rel_path,
                                    d.start_line as u32,
                                    d.end_line as u32,
                                )
                            });
                        }

                        if diagnostic_outputs.is_empty() {
                            None
                        } else {
                            Some(FileAnalysis {
                                path: path.clone(),
                                relative_path: path
                                    .strip_prefix(&*workspace_dir_arc)
                                    .unwrap_or(path)
                                    .to_path_buf(),
                                diagnostics: diagnostic_outputs,
                            })
                        }
                    } else {
                        None
                    };

                    (file_analysis, timing, metrics, None)
                })
                .collect()
        };
        let par_wall = par_start.elapsed();
        results.append(&mut chunk_results);

        // Snapshot the salsa memory map at its high-water mark: the final chunk's
        // memos are all still live here, before the trim below evicts them. Without
        // this the only report (post-loop) reflects the post-eviction trough, which
        // understates the working set by an order of magnitude.
        if report_mem && chunk_idx + 1 == num_chunks {
            eprintln!("\n##### salsa memory at PEAK (last chunk, pre-eviction) #####");
            report_salsa_memory(&db);
        }

        // `config_path_input` (which borrows `&db`) is now out of scope, so the
        // exclusive `&mut db` borrow is free: trim the salsa memos beyond their
        // caps, then release the parser's thread-local green-node caches (which
        // salsa does not own) on the driver and every rayon worker.
        let evict_start = std::time::Instant::now();
        db.enforce_lru();
        syntax::clear_shared_node_cache();
        rayon::broadcast(|_| syntax::clear_shared_node_cache());
        if chunk_profile {
            eprintln!(
                "chunk {:>3}/{}: files={} par={:.2}s straggler_max={:.2}s evict={:.2}s",
                chunk_idx + 1,
                num_chunks,
                chunk.len(),
                par_wall.as_secs_f64(),
                max_file_us.load(Ordering::Relaxed) as f64 / 1e6,
                evict_start.elapsed().as_secs_f64(),
            );
        }
    }

    if let Some(ref pb) = &*progress_arc {
        pb.finish_with_message("Analysis complete");
    }

    let elapsed = start.elapsed();

    if report_mem {
        eprintln!("\n##### salsa memory at TROUGH (post-eviction) #####");
        report_salsa_memory(&db);
    }

    // Results are appended chunk-by-chunk in `file_ids` order, and rayon's
    // indexed `par_iter().collect()` preserves within-chunk order, so
    // `file_analyses[i]` / `metrics_list[i]` / `errors_list[i]` correspond to
    // `file_ids[i]` — the JSONL path relies on this alignment to pair each file
    // with its metrics and error.
    let mut file_analyses: Vec<Option<FileAnalysis>> = Vec::with_capacity(results.len());
    let mut all_timings: Vec<FileTiming> = Vec::new();
    let mut metrics_list: Vec<Option<FileMetrics>> = Vec::with_capacity(results.len());
    let mut errors_list: Vec<Option<String>> = Vec::with_capacity(results.len());
    for (analysis, timing, metrics, error) in results {
        file_analyses.push(analysis);
        if let Some(timing) = timing {
            all_timings.push(timing);
        }
        metrics_list.push(metrics);
        errors_list.push(error);
    }

    let total_diagnostics: usize =
        file_analyses.iter().flatten().map(|f| f.diagnostics.len()).sum();

    // JSONL emits one `file` event per analyzed file (clean files included),
    // closed by `done`, mirroring the streaming `--format jsonl` contract (the
    // `start` event was already emitted before analysis). `metrics` carries the
    // real cyclomatic/cognitive complexity; `error` is set for files whose
    // analysis panicked or whose text could not be read (so a crashed file is
    // not silently reported as clean), and those are tallied into `done`'s
    // `failed_files`.
    if jsonl {
        use ide::streaming::{DoneEvent, FileEvent};

        let mut failed_files = 0;
        for (i, (_, path)) in file_ids.iter().enumerate() {
            let diagnostics =
                file_analyses[i].as_ref().map(|f| f.diagnostics.clone()).unwrap_or_default();
            let error = errors_list[i].clone();
            if error.is_some() {
                failed_files += 1;
            }
            let file_event = FileEvent::new(
                path.display().to_string(),
                diagnostics,
                metrics_list[i].clone(),
                error,
            );
            println!("{}", serde_json::to_string(&file_event)?);
        }

        let done_event =
            DoneEvent::new(elapsed.as_secs_f64(), file_ids.len(), total_diagnostics, failed_files);
        println!("{}", serde_json::to_string(&done_event)?);

        tracing::info!("JSONL analysis complete");
        return Ok(());
    }

    let all_diagnostics: Vec<FileAnalysis> = file_analyses.into_iter().flatten().collect();

    let analysis_results = AnalysisResults {
        files_analyzed: bsl_files.len(),
        files_with_issues: all_diagnostics.len(),
        total_diagnostics,
        elapsed_secs: elapsed.as_secs_f64(),
        diagnostics: all_diagnostics,
        source_dir: source_dir.clone(),
        workspace_dir: workspace_dir.clone(),
    };

    std::fs::create_dir_all(&output_dir)?;

    let registry = ReporterRegistry::new();
    let reporter_keys = if reporters.is_empty() { vec!["console".to_string()] } else { reporters };

    for key in &reporter_keys {
        match registry.get(key) {
            Some(reporter) => {
                if let Err(e) = reporter.report(&analysis_results, &output_dir) {
                    tracing::error!("Reporter '{}' failed: {}", key, e);
                    eprintln!("Error: Reporter '{}' failed: {}", key, e);
                }
            }
            None => {
                eprintln!("Error: Unknown reporter '{}'", key);
                eprintln!("Valid reporters: {}", registry.keys().join(", "));
                return Err(format!("Unknown reporter: {}", key).into());
            }
        }
    }

    if let Some(diag_name) = only_diagnostic {
        if let Some(stats) = ProfilingStats::from_timings(diag_name, all_timings) {
            stats.print();
        }
    }

    tracing::info!("Analysis complete");

    Ok(())
}

/// Dump salsa's per-ingredient memory snapshot plus process RSS, so the gap
/// between RSS and salsa-tracked bytes exposes untracked residency (parser
/// green-node cache, file-text inputs, allocator retention). Sorted by live
/// entry count: a query whose count tracks the file total is retaining
/// everything (LRU not evicting).
fn report_salsa_memory(db: &ide::RootDatabaseImpl) {
    let mut rows = db.memory_report();
    rows.sort_by_key(|(_, count, ..)| std::cmp::Reverse(*count));

    let (mut tc, mut tm, mut tf, mut th) = (0usize, 0usize, 0usize, 0usize);
    for (_, c, m, f, h) in &rows {
        tc += c;
        tm += m;
        tf += f;
        th += h.unwrap_or(0);
    }

    let proc_kb = |key: &str| -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        status
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
    };

    eprintln!("\n=== salsa memory_usage (top 40 ingredients by live entry count) ===");
    eprintln!(
        "{:<48}{:>10}{:>12}{:>12}{:>12}",
        "ingredient", "count", "meta_KB", "fields_KB", "heap_KB"
    );
    for (name, c, m, f, h) in rows.iter().take(40) {
        let heap_s =
            h.map(|x| format!("{:.1}", x as f64 / 1024.0)).unwrap_or_else(|| "-".to_string());
        eprintln!(
            "{:<48}{:>10}{:>12.1}{:>12.1}{:>12}",
            name,
            c,
            *m as f64 / 1024.0,
            *f as f64 / 1024.0,
            heap_s
        );
    }
    eprintln!(
        "--- salsa totals: entries={} meta={:.1}MB fields={:.1}MB heap={:.1}MB (heap reported only for some ingredients) ---",
        tc,
        tm as f64 / 1048576.0,
        tf as f64 / 1048576.0,
        th as f64 / 1048576.0
    );
    if let (Some(hwm), Some(rss)) = (proc_kb("VmHWM:"), proc_kb("VmRSS:")) {
        let salsa_mb = (tm + tf + th) as f64 / 1048576.0;
        eprintln!(
            "--- process: VmHWM(peak)={:.1}MB VmRSS(now)={:.1}MB | salsa-tracked={:.1}MB | untracked(node-cache+text+alloc)~={:.1}MB ---",
            hwm as f64 / 1024.0,
            rss as f64 / 1024.0,
            salsa_mb,
            rss as f64 / 1024.0 - salsa_mb
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn analyze_streaming(
    source_dir: PathBuf,
    workspace_dir: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    config_path: Option<PathBuf>,
    reporters: Vec<String>,
    quiet: bool,
    workers: Option<usize>,
    format: OutputFormat,
    only_diagnostic: Option<String>,
    diff_filter: Option<bsl_analyzer::diff_filter::DiffFilter>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use ide::{streaming::AnalysisOrchestrator, DiagnosticsConfig};
    use vfs::FileId;
    use walkdir::WalkDir;

    let _span = tracing::info_span!("cli_analyze_streaming").entered();

    let source_dir = std::fs::canonicalize(&source_dir).unwrap_or(source_dir);

    let profiling_enabled = only_diagnostic.is_some();
    if let Some(ref diag) = only_diagnostic {
        tracing::info!("Profiling diagnostic (streaming mode): {}", diag);
    }
    if diff_filter.is_some() {
        tracing::info!("Diff filter enabled (streaming mode)");
    }

    tracing::info!("Analyzing project (streaming mode): {:?}", source_dir);
    tracing::info!("Workers: {:?}", workers);
    tracing::info!("Format: {:?}", format);

    let workspace_dir = workspace_dir.unwrap_or_else(|| source_dir.clone());
    let output_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));

    let proj_config = if let Some(ref cfg) = config_path {
        project_model::ProjectConfig::load_from_file(cfg).unwrap_or_default()
    } else {
        project_model::ProjectConfig::load(&source_dir).unwrap_or_default()
    };

    let mut diag_config = DiagnosticsConfig::from_project_json(
        &proj_config.diagnostics,
        proj_config.output.resolve_locale().unwrap_or_default(),
    );

    if let Some(ref diag_name) = only_diagnostic {
        diag_config.apply_cli_filters(std::slice::from_ref(diag_name), &[]);
    }

    tracing::info!(
        disabled = diag_config.disabled.len(),
        only_enabled = ?diag_config.only_enabled.as_ref().map(|v| v.len()),
        params = diag_config.parameters.len(),
        locale = ?diag_config.locale,
        "Loaded DiagnosticsConfig (streaming mode)"
    );

    // Scope the file walk to the configuration source root (+ extension roots)
    // instead of the raw `-s` dir, so vendored/build copies such as
    // `.build/vendor` are not analyzed as a duplicate configuration.
    let project = project_model::Project::with_config(&source_dir, proj_config.clone());
    let source_roots = project.source_roots();

    tracing::info!("Finding BSL files in {:?}", source_roots);
    let mut bsl_files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in &source_roots {
        for entry in WalkDir::new(root).follow_links(true) {
            let entry = entry?;
            if entry.file_type().is_file()
                && entry.path().extension().is_some_and(|ext| ext == "bsl")
            {
                let path = entry.path().to_path_buf();
                if seen.insert(path.clone()) {
                    bsl_files.push(path);
                }
            }
        }
    }

    tracing::info!(
        "Found {} BSL files across {} source root(s)",
        bsl_files.len(),
        source_roots.len()
    );

    if bsl_files.is_empty() {
        tracing::warn!("No BSL files found in {:?}", source_roots);
        if !matches!(format, OutputFormat::Jsonl) {
            println!("No BSL files found!");
        }
        return Ok(());
    }

    let mut file_set = vfs::FileSet::new();
    let mut all_file_ids: Vec<(FileId, PathBuf)> = Vec::new();

    for (idx, path) in bsl_files.iter().enumerate() {
        let file_id = FileId(idx as u32);
        let vfs_path = vfs::VfsPath::new(path.clone());
        file_set.insert(file_id, vfs_path);
        all_file_ids.push((file_id, path.clone()));
    }

    let file_ids: Vec<(FileId, PathBuf)> = if let Some(ref filter) = diff_filter {
        let filtered: Vec<_> = all_file_ids
            .iter()
            .filter(|(_, path)| {
                let rel_path = path.strip_prefix(&source_dir).unwrap_or(path);
                filter.should_analyze(rel_path)
            })
            .cloned()
            .collect();
        tracing::info!(
            "After diff filter: {} files to analyze (from {} total in VFS)",
            filtered.len(),
            all_file_ids.len()
        );
        filtered
    } else {
        all_file_ids.clone()
    };

    let mut builder =
        AnalysisOrchestrator::builder().workspace_root(&source_dir).diagnostics_config(diag_config);

    if let Some(w) = workers {
        builder = builder.num_workers(w);
    }

    if let Some(ref cfg) = config_path {
        builder = builder.configuration_path(cfg);
    }

    let orchestrator = builder.build()?;

    match format {
        OutputFormat::Jsonl if diff_filter.is_some() => {
            use std::time::Instant;

            use ide::streaming::{DoneEvent, FileEvent, StartEvent};

            let start = Instant::now();
            let file_id_vec: Vec<FileId> = file_ids.iter().map(|(id, _)| *id).collect();
            let total_files = file_id_vec.len();

            let start_event = StartEvent::new(total_files);
            println!("{}", serde_json::to_string(&start_event).unwrap());

            let streaming_results =
                orchestrator.analyze_with_progress(file_id_vec, file_set, |_, _| {})?;

            let filter = diff_filter.as_ref().unwrap();
            let mut total_diagnostics = 0;

            for file_result in &streaming_results.file_results {
                if let Some((_, path)) = file_ids.iter().find(|(id, _)| *id == file_result.file_id)
                {
                    let rel_path = path.strip_prefix(&source_dir).unwrap_or(path);
                    let filtered: Vec<_> = file_result
                        .diagnostics
                        .iter()
                        .filter(|d| {
                            filter.diagnostic_in_diff(
                                rel_path,
                                d.start_line as u32,
                                d.end_line as u32,
                            )
                        })
                        .cloned()
                        .collect();

                    total_diagnostics += filtered.len();
                    let file_event = FileEvent::new(
                        path.display().to_string(),
                        filtered,
                        file_result.metrics.clone(),
                        file_result.error.as_ref().map(|e| e.to_string()),
                    );
                    println!("{}", serde_json::to_string(&file_event).unwrap());
                }
            }

            let done_event = DoneEvent::new(
                start.elapsed().as_secs_f64(),
                streaming_results.file_results.len(),
                total_diagnostics,
                streaming_results.file_results.iter().filter(|r| r.error.is_some()).count(),
            );
            println!("{}", serde_json::to_string(&done_event).unwrap());

            tracing::info!("JSONL streaming analysis complete (with diff filter)");
        }
        OutputFormat::Jsonl => {
            let file_id_vec: Vec<FileId> = file_ids.iter().map(|(id, _)| *id).collect();
            let _summary = orchestrator.analyze_jsonl(file_id_vec, file_set)?;
            tracing::info!("JSONL streaming analysis complete");
        }
        OutputFormat::Console => {
            use std::time::Instant;

            use indicatif::{ProgressBar, ProgressStyle};

            use bsl_analyzer::reporters::{AnalysisResults, FileAnalysis, ReporterRegistry};

            let start = Instant::now();
            let total_files = file_ids.len();

            let progress = if !quiet {
                let pb = ProgressBar::new(total_files as u64);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template(
                            "[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}",
                        )
                        .unwrap()
                        .progress_chars("#>-"),
                );
                Some(pb)
            } else {
                None
            };

            let progress_clone = progress.clone();
            let start_clone = start;

            let file_id_vec: Vec<FileId> = file_ids.iter().map(|(id, _)| *id).collect();
            let streaming_results = orchestrator.analyze_with_progress(
                file_id_vec,
                file_set,
                |processed, _total| {
                    if let Some(ref pb) = progress_clone {
                        pb.set_position(processed as u64);
                        let elapsed_secs = start_clone.elapsed().as_secs_f64();
                        if elapsed_secs > 0.0 {
                            pb.set_message(format!(
                                "{:.0} files/sec",
                                processed as f64 / elapsed_secs
                            ));
                        }
                    }
                },
            )?;

            if let Some(ref pb) = progress {
                pb.finish_with_message("Analysis complete");
            }

            let elapsed = start.elapsed();

            let mut all_diagnostics = Vec::new();

            for file_result in &streaming_results.file_results {
                if !file_result.diagnostics.is_empty() {
                    if let Some((_, path)) =
                        file_ids.iter().find(|(id, _)| *id == file_result.file_id)
                    {
                        let filtered_diagnostics = if let Some(ref filter) = diff_filter {
                            let rel_path = path.strip_prefix(&source_dir).unwrap_or(path);
                            file_result
                                .diagnostics
                                .iter()
                                .filter(|d| {
                                    filter.diagnostic_in_diff(
                                        rel_path,
                                        d.start_line as u32,
                                        d.end_line as u32,
                                    )
                                })
                                .cloned()
                                .collect()
                        } else {
                            file_result.diagnostics.clone()
                        };

                        if !filtered_diagnostics.is_empty() {
                            all_diagnostics.push(FileAnalysis {
                                path: path.clone(),
                                relative_path: path
                                    .strip_prefix(&workspace_dir)
                                    .unwrap_or(path)
                                    .to_path_buf(),
                                diagnostics: filtered_diagnostics,
                            });
                        }
                    }
                }
            }

            let results = AnalysisResults {
                files_analyzed: bsl_files.len(),
                files_with_issues: all_diagnostics.len(),
                total_diagnostics: streaming_results.total_diagnostics,
                elapsed_secs: elapsed.as_secs_f64(),
                diagnostics: all_diagnostics,
                source_dir: source_dir.clone(),
                workspace_dir: workspace_dir.clone(),
            };

            std::fs::create_dir_all(&output_dir)?;

            let registry = ReporterRegistry::new();
            let reporter_keys =
                if reporters.is_empty() { vec!["console".to_string()] } else { reporters };

            for key in &reporter_keys {
                match registry.get(key) {
                    Some(reporter) => {
                        if let Err(e) = reporter.report(&results, &output_dir) {
                            tracing::error!("Reporter '{}' failed: {}", key, e);
                            eprintln!("Error: Reporter '{}' failed: {}", key, e);
                        }
                    }
                    None => {
                        eprintln!("Error: Unknown reporter '{}'", key);
                        eprintln!("Valid reporters: {}", registry.keys().join(", "));
                        return Err(format!("Unknown reporter: {}", key).into());
                    }
                }
            }

            if profiling_enabled {
                if let Some(diag_name) = only_diagnostic {
                    let timings: Vec<FileTiming> = streaming_results
                        .file_results
                        .iter()
                        .filter_map(|fr| {
                            file_ids.iter().find(|(id, _)| *id == fr.file_id).map(|(_, path)| {
                                FileTiming { path: path.clone(), duration: fr.duration }
                            })
                        })
                        .collect();

                    if let Some(stats) = ProfilingStats::from_timings(diag_name, timings) {
                        stats.print();
                    }
                }
            }

            tracing::info!("Streaming analysis complete");
        }
    }

    Ok(())
}
