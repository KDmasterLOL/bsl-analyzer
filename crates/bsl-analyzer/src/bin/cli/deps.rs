use std::{
    error::Error,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::ValueEnum;

/// Output format for the `deps` subcommand.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum DepsOutputFormat {
    #[default]
    Csv,
    Json,
}

struct UnionSummary {
    size: usize,
    bytes: u64,
    /// Per-root BFS panics encountered while traversing for the union.
    panicked: usize,
    /// At least one root or transitively reached file was unreadable, so
    /// the union may be silently truncated.
    hit_unreadable: bool,
}

struct DepsRow {
    path: PathBuf,
    file_id: u32,
    levels: Vec<usize>,
    closure: usize,
    /// Sum of file sizes (bytes on disk) for every file in the closure,
    /// including the root. `0` when `--bytes` is off.
    closure_bytes: u64,
    /// `None` = BFS completed successfully; `Some(reason)` = row excluded
    /// from aggregate stats (file unreadable at load or BFS panicked).
    error: Option<&'static str>,
}

/// Best-effort `VmRSS` from `/proc/self/status` in kilobytes. Linux-only;
/// returns `None` on other platforms or when the proc entry can't be parsed.
fn read_vmrss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

/// RFC-4180 field quoting: wrap in `"` and double internal `"` when the
/// field contains a comma, quote, or newline. Returned `Cow` borrows when
/// no escaping is needed.
fn csv_field(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        std::borrow::Cow::Owned(format!("\"{}\"", s.replace('"', "\"\"")))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_deps(
    source_dir: PathBuf,
    depth: u32,
    sample: usize,
    format: DepsOutputFormat,
    quiet: bool,
    bytes: bool,
    report_mem: bool,
    bench: Option<PathBuf>,
    multi_open: Vec<PathBuf>,
    bench_index: bool,
    index_workers: Option<usize>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::collections::{HashMap, HashSet};
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use base_db::{FileIdInput, SourceDatabase};
    use hir::file_dependencies_query;
    use ide::RootDatabaseImpl;
    use indicatif::{ProgressBar, ProgressStyle};
    use rayon::prelude::*;
    use vfs::FileId;
    use walkdir::WalkDir;

    let _span = tracing::info_span!("cli_deps", ?source_dir, depth, sample).entered();
    let start = Instant::now();
    let rss_baseline = if report_mem { read_vmrss_kb() } else { None };

    // Canonicalize the workspace root up-front so `path_to_id` and CLI args
    // (`--bench`/`--multi-open`) share one canonical key space; otherwise
    // `--source-dir .` produces relative WalkDir paths that CLI canonicalize
    // can't match.
    let source_dir = source_dir.canonicalize().unwrap_or(source_dir);

    let mut bsl_entries: Vec<(PathBuf, u64)> = Vec::new();
    for entry in WalkDir::new(&source_dir).follow_links(true) {
        let entry = entry?;
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|e| e.to_str()) == Some("bsl")
        {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let path = entry.path().canonicalize().unwrap_or_else(|_| entry.path().to_path_buf());
            bsl_entries.push((path, size));
        }
    }
    bsl_entries.sort_by(|a, b| a.0.cmp(&b.0));
    let total = bsl_entries.len();
    tracing::info!(total, "discovered .bsl files");
    if total == 0 {
        return Err(format!("no .bsl files under {}", source_dir.display()).into());
    }

    // Short-circuit before the eager workspace-into-Salsa load so the
    // measurement times only the lexer-only index build, not the surrounding
    // ingest.
    if bench_index {
        return run_deps_bench_index(bsl_entries, index_workers, rss_baseline, start);
    }

    let mut db = RootDatabaseImpl::default();
    let mut file_set = vfs::FileSet::new();
    let mut all_file_ids: Vec<(FileId, PathBuf)> = Vec::with_capacity(total);
    let mut file_sizes: HashMap<FileId, u64> = HashMap::with_capacity(total);
    let mut path_to_id: HashMap<PathBuf, FileId> = HashMap::with_capacity(total);
    for (idx, (path, size)) in bsl_entries.iter().enumerate() {
        let file_id = FileId(idx as u32);
        file_set.insert(file_id, vfs::VfsPath::new(path.clone()));
        all_file_ids.push((file_id, path.clone()));
        file_sizes.insert(file_id, *size);
        path_to_id.insert(path.clone(), file_id);
    }
    let source_root_id = base_db::SourceRootId(0);
    db.set_source_root(source_root_id, base_db::SourceRoot::new_local(file_set));
    let mut unreadable: HashSet<FileId> = HashSet::new();
    let mut read_errors: Vec<(PathBuf, std::io::Error)> = Vec::new();
    for (file_id, path) in &all_file_ids {
        db.set_file_source_root(*file_id, source_root_id);
        match fs::read_to_string(path) {
            Ok(content) => db.set_file_text(*file_id, &content),
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "failed to read file");
                unreadable.insert(*file_id);
                read_errors.push((path.clone(), err));
                // Leave file_text empty so `file_dependencies_query` on this
                // file returns an empty deps list; we mark roots that hit
                // this state with an `error` and exclude them from aggregates.
                db.set_file_text(*file_id, "");
            }
        }
    }
    if !read_errors.is_empty() {
        tracing::warn!(count = read_errors.len(), "files unreadable, excluded from aggregates");
    }
    let load_elapsed = start.elapsed();
    tracing::info!(elapsed_ms = load_elapsed.as_millis() as u64, "workspace loaded");
    let rss_after_load = if report_mem { read_vmrss_kb() } else { None };

    if let Some(bench_path) = bench.as_ref() {
        return run_deps_bench(
            &db,
            bench_path,
            &path_to_id,
            &file_sizes,
            rss_baseline,
            rss_after_load,
            load_elapsed,
        );
    }

    let roots: Vec<(FileId, PathBuf)> = if !multi_open.is_empty() {
        let mut resolved: Vec<(FileId, PathBuf)> = Vec::with_capacity(multi_open.len());
        for raw in &multi_open {
            let canonical = raw.canonicalize().unwrap_or_else(|_| raw.clone());
            let Some(file_id) = path_to_id.get(&canonical).copied() else {
                return Err(format!(
                    "--multi-open: file not found in workspace: {}",
                    raw.display()
                )
                .into());
            };
            resolved.push((file_id, canonical));
        }
        resolved
    } else if sample == 0 || sample >= total {
        all_file_ids.clone()
    } else {
        // Integer stride sampling: `i * total / sample` avoids the f64
        // precision wobble at workspace scales beyond 2^53.
        (0..sample)
            .map(|i| {
                let idx = (i.saturating_mul(total) / sample).min(total - 1);
                all_file_ids[idx].clone()
            })
            .collect()
    };
    // Dedupe roots (for `--multi-open`) preserving first occurrence so the
    // union arithmetic later cannot underflow when the user repeats a path.
    let roots: Vec<(FileId, PathBuf)> = {
        let mut seen: HashSet<FileId> = HashSet::new();
        roots.into_iter().filter(|(fid, _)| seen.insert(*fid)).collect()
    };
    let sampled = roots.len();
    tracing::info!(sampled, total, depth, "starting BFS");

    let progress = (!quiet).then(|| {
        let pb = ProgressBar::new(sampled as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );
        pb
    });

    let max_depth = depth as usize;
    let processed = Arc::new(AtomicUsize::new(0));
    let bfs_start = Instant::now();
    let unreadable = Arc::new(unreadable);
    let file_sizes_arc = Arc::new(file_sizes);

    let rows: Vec<DepsRow> = roots
        .par_iter()
        .map_with(db.clone(), |snapshot, (root_id, root_path)| {
            let row = if unreadable.contains(root_id) {
                DepsRow {
                    path: root_path.clone(),
                    file_id: root_id.0,
                    levels: Vec::new(),
                    closure: 0,
                    closure_bytes: 0,
                    error: Some("unreadable"),
                }
            } else {
                let sizes = Arc::clone(&file_sizes_arc);
                let unread_set = Arc::clone(&unreadable);
                catch_unwind(AssertUnwindSafe(|| {
                    let mut levels: Vec<usize> = Vec::with_capacity(max_depth);
                    let mut visited: HashSet<FileId> = HashSet::new();
                    visited.insert(*root_id);
                    let mut frontier: Vec<FileId> = vec![*root_id];
                    // Stop-but-record: if BFS reaches a file whose text we
                    // couldn't read at load time, the closure beyond that
                    // point is unknown — mark the root row as errored so it
                    // doesn't pollute aggregates with a falsely-truncated
                    // closure.
                    let mut hit_unreadable = false;

                    for _ in 0..max_depth {
                        let mut next: Vec<FileId> = Vec::new();
                        for fid in &frontier {
                            let input = FileIdInput::new(&*snapshot, *fid);
                            let deps = file_dependencies_query(&*snapshot, input);
                            for d in deps.iter() {
                                if visited.insert(*d) {
                                    if unread_set.contains(d) {
                                        hit_unreadable = true;
                                    }
                                    next.push(*d);
                                }
                            }
                        }
                        levels.push(next.len());
                        if next.is_empty() {
                            break;
                        }
                        frontier = next;
                    }
                    let closure = visited.len() - 1;
                    let closure_bytes = if bytes {
                        visited.iter().fold(0u64, |acc, f| {
                            acc.saturating_add(sizes.get(f).copied().unwrap_or(0))
                        })
                    } else {
                        0
                    };
                    let error = if hit_unreadable { Some("transitive_unreadable") } else { None };
                    DepsRow {
                        path: root_path.clone(),
                        file_id: root_id.0,
                        levels,
                        closure,
                        closure_bytes,
                        error,
                    }
                }))
                .unwrap_or_else(|_| {
                    tracing::warn!(
                        path = %root_path.display(),
                        file_id = root_id.0,
                        "BFS panicked, excluding from aggregates",
                    );
                    DepsRow {
                        path: root_path.clone(),
                        file_id: root_id.0,
                        levels: Vec::new(),
                        closure: 0,
                        closure_bytes: 0,
                        error: Some("panic"),
                    }
                })
            };

            let count = processed.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(ref pb) = progress {
                pb.set_position(count as u64);
            }
            row
        })
        .collect();

    if let Some(ref pb) = progress {
        pb.finish_with_message("done");
    }
    let bfs_elapsed = bfs_start.elapsed();
    let rss_after_bfs = if report_mem { read_vmrss_kb() } else { None };

    // Multi-open: synthesize a union row covering the union of all roots'
    // closures. Compute each root's closure with its OWN visited set, then
    // union the sets — a single shared visited set under-counts because a
    // file first reached at a deeper depth from one root would not be
    // re-expanded when reached at a shallower depth from another root.
    let union_summary: Option<UnionSummary> = if !multi_open.is_empty() {
        let snapshot = db.clone();
        let root_ids: HashSet<FileId> = roots.iter().map(|(fid, _)| *fid).collect();
        let mut union: HashSet<FileId> = HashSet::new();
        let mut union_panicked: usize = 0;
        let mut union_hit_unreadable: bool = false;
        for (root_id, root_path) in &roots {
            // A root that itself failed to load at workspace ingestion has
            // an empty `file_text` in Salsa; its closure will be empty and
            // misleading. Surface that explicitly instead of silently
            // contributing a 1-file (root-only) singleton to the union.
            if unreadable.contains(root_id) {
                union_hit_unreadable = true;
                union.insert(*root_id);
                continue;
            }
            let unread = Arc::clone(&unreadable);
            let mut local_hit_unreadable = false;
            let closure = catch_unwind(AssertUnwindSafe(|| {
                let mut visited: HashSet<FileId> = HashSet::new();
                visited.insert(*root_id);
                let mut frontier: Vec<FileId> = vec![*root_id];
                for _ in 0..max_depth {
                    let mut next: Vec<FileId> = Vec::new();
                    for fid in &frontier {
                        let input = FileIdInput::new(&snapshot, *fid);
                        let deps = file_dependencies_query(&snapshot, input);
                        for d in deps.iter() {
                            if visited.insert(*d) {
                                if unread.contains(d) {
                                    local_hit_unreadable = true;
                                }
                                next.push(*d);
                            }
                        }
                    }
                    if next.is_empty() {
                        break;
                    }
                    frontier = next;
                }
                visited
            }))
            .unwrap_or_else(|_| {
                tracing::warn!(
                    path = %root_path.display(),
                    "multi-open: BFS panicked, skipping root in union",
                );
                union_panicked += 1;
                HashSet::new()
            });
            if local_hit_unreadable {
                union_hit_unreadable = true;
            }
            union.extend(closure);
        }
        let union_bytes = if bytes {
            union.iter().fold(0u64, |acc, f| {
                acc.saturating_add(file_sizes_arc.get(f).copied().unwrap_or(0))
            })
        } else {
            0
        };
        // "Files beyond the roots" = union members that are NOT themselves
        // roots. Counting `union.len() - roots.len()` underflows when
        // panicked roots are absent from `union`, so filter explicitly.
        let union_size = union.iter().filter(|f| !root_ids.contains(f)).count();
        Some(UnionSummary {
            size: union_size,
            bytes: union_bytes,
            panicked: union_panicked,
            hit_unreadable: union_hit_unreadable,
        })
    } else {
        None
    };

    let max_levels = rows.iter().map(|r| r.levels.len()).max().unwrap_or(0);
    match format {
        DepsOutputFormat::Csv => {
            let mut header = String::from("file,file_id,error,closure");
            if bytes {
                header.push_str(",closure_bytes");
            }
            for i in 0..max_levels {
                let _ = write!(header, ",l{}", i + 1);
            }
            println!("{}", header);
            for r in &rows {
                let err = r.error.unwrap_or("");
                let path_str = r.path.display().to_string();
                let path_field = csv_field(&path_str);
                let mut line = format!("{},{},{},{}", path_field, r.file_id, err, r.closure);
                if bytes {
                    let _ = write!(line, ",{}", r.closure_bytes);
                }
                for i in 0..max_levels {
                    let _ = write!(line, ",{}", r.levels.get(i).copied().unwrap_or(0));
                }
                println!("{}", line);
            }
        }
        DepsOutputFormat::Json => {
            for r in &rows {
                let mut obj = serde_json::json!({
                    "file": r.path.display().to_string(),
                    "file_id": r.file_id,
                    "error": r.error,
                    "closure": r.closure,
                    "levels": r.levels,
                });
                if bytes {
                    obj["closure_bytes"] = serde_json::json!(r.closure_bytes);
                }
                println!("{}", obj);
            }
        }
    }

    // Aggregate over successful rows only. Failed rows (unreadable file or
    // panicked BFS) carry closure=0 by construction and would otherwise pull
    // every percentile and the mean downward.
    let ok_rows: Vec<&DepsRow> = rows.iter().filter(|r| r.error.is_none()).collect();
    let mut closures: Vec<usize> = ok_rows.iter().map(|r| r.closure).collect();
    closures.sort_unstable();
    let pct = |p: f64| -> usize {
        if closures.is_empty() {
            0
        } else {
            let idx = ((closures.len() as f64 - 1.0) * p).round() as usize;
            closures[idx]
        }
    };
    let avg = if closures.is_empty() {
        0.0
    } else {
        closures.iter().sum::<usize>() as f64 / closures.len() as f64
    };
    let l1_avg = if ok_rows.is_empty() {
        0.0
    } else {
        ok_rows.iter().map(|r| r.levels.first().copied().unwrap_or(0)).sum::<usize>() as f64
            / ok_rows.len() as f64
    };
    let share_pct = if total > 0 { avg / total as f64 * 100.0 } else { 0.0 };
    let ok = ok_rows.len();
    let panicked = rows.iter().filter(|r| r.error == Some("panic")).count();
    let unread = rows.iter().filter(|r| r.error == Some("unreadable")).count();
    let trans_unread = rows.iter().filter(|r| r.error == Some("transitive_unreadable")).count();

    eprintln!();
    eprintln!("=== Dependency closure summary ===");
    eprintln!("workspace files (.bsl):  {}", total);
    eprintln!("unreadable at load:      {}", read_errors.len());
    eprintln!("sampled roots:           {}", sampled);
    eprintln!("  ok:                    {}", ok);
    eprintln!("  unreadable (root):     {}", unread);
    eprintln!("  transitive unreadable: {}", trans_unread);
    eprintln!("  panicked BFS:          {}", panicked);
    eprintln!("BFS depth:               {}", depth);
    eprintln!("workspace load:          {:.1}s", load_elapsed.as_secs_f64());
    eprintln!("BFS elapsed:             {:.1}s", bfs_elapsed.as_secs_f64());
    if ok == 0 {
        eprintln!("(no successful rows — aggregates omitted)");
    } else {
        eprintln!("avg L1 (direct deps):    {:.1}", l1_avg);
        eprintln!("closure size — avg:      {:.1}", avg);
        eprintln!("closure size — p50:      {}", pct(0.50));
        eprintln!("closure size — p90:      {}", pct(0.90));
        eprintln!("closure size — p95:      {}", pct(0.95));
        eprintln!("closure size — max:      {}", closures.last().copied().unwrap_or(0));
        eprintln!("avg closure / workspace: {:.2}%", share_pct);

        if bytes {
            let mut byte_vals: Vec<u64> = ok_rows.iter().map(|r| r.closure_bytes).collect();
            byte_vals.sort_unstable();
            let byte_pct = |p: f64| -> u64 {
                if byte_vals.is_empty() {
                    0
                } else {
                    let idx = ((byte_vals.len() as f64 - 1.0) * p).round() as usize;
                    byte_vals[idx]
                }
            };
            let byte_sum = byte_vals.iter().copied().fold(0u64, |acc, v| acc.saturating_add(v));
            let byte_avg = byte_sum as f64 / byte_vals.len() as f64;
            let mb = |b: u64| b as f64 / 1024.0 / 1024.0;
            eprintln!("closure bytes — avg:     {:.1} MB", mb(byte_avg as u64));
            eprintln!("closure bytes — p50:     {:.1} MB", mb(byte_pct(0.50)));
            eprintln!("closure bytes — p90:     {:.1} MB", mb(byte_pct(0.90)));
            eprintln!("closure bytes — p95:     {:.1} MB", mb(byte_pct(0.95)));
            eprintln!(
                "closure bytes — max:     {:.1} MB",
                mb(byte_vals.last().copied().unwrap_or(0)),
            );
        }
    }

    if let Some(union) = union_summary {
        eprintln!();
        eprintln!("=== Multi-open union closure ===");
        eprintln!("roots:                   {}", roots.len());
        eprintln!("union closure (files):   {}", union.size);
        let union_share = if total > 0 { union.size as f64 / total as f64 * 100.0 } else { 0.0 };
        eprintln!("union / workspace:       {:.2}%", union_share);
        if bytes {
            eprintln!("union closure bytes:     {:.1} MB", union.bytes as f64 / 1024.0 / 1024.0,);
        }
        if union.panicked > 0 {
            eprintln!("panicked roots (skipped):{}", union.panicked);
        }
        if union.hit_unreadable {
            eprintln!("WARNING: union touched unreadable files; closure may be truncated");
        }
    }

    if report_mem {
        let fmt = |kb: Option<u64>| -> String {
            kb.map(|k| format!("{:.1} MB", k as f64 / 1024.0)).unwrap_or_else(|| "n/a".to_string())
        };
        eprintln!();
        eprintln!("=== RSS snapshots (VmRSS) ===");
        eprintln!("baseline (pre-load):     {}", fmt(rss_baseline));
        eprintln!("after workspace load:    {}", fmt(rss_after_load));
        eprintln!("after BFS:               {}", fmt(rss_after_bfs));
    }

    Ok(())
}

/// Bench mode for `deps --bench <file>`: time the cold cost of bringing a
/// single file's text into Salsa-driven IDE state
/// (read → parse → item_tree → module_bodies).
#[allow(clippy::too_many_arguments)]
fn run_deps_bench(
    db: &ide::RootDatabaseImpl,
    bench_path: &Path,
    path_to_id: &std::collections::HashMap<PathBuf, vfs::FileId>,
    file_sizes: &std::collections::HashMap<vfs::FileId, u64>,
    rss_baseline: Option<u64>,
    rss_after_load: Option<u64>,
    load_elapsed: std::time::Duration,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::time::Instant;

    use base_db::{FileIdInput, RootQueryDb};
    use hir::{DefDatabase, ModuleId};

    let canonical = bench_path.canonicalize().unwrap_or_else(|_| bench_path.to_path_buf());
    let Some(&file_id) = path_to_id.get(&canonical) else {
        return Err(format!("--bench: file not in workspace: {}", bench_path.display()).into());
    };
    let size = file_sizes.get(&file_id).copied().unwrap_or(0);

    let read_start = Instant::now();
    let read_result = std::fs::read_to_string(&canonical);
    let read_elapsed = read_start.elapsed();
    let read_bytes = read_result.map(|s| s.len()).unwrap_or(0);

    // Cold from Salsa's POV — the eager workspace load above only set inputs,
    // it didn't trigger parse/item_tree/module_bodies for this file_id yet.
    let parse_start = Instant::now();
    let _parse = db.parse(file_id);
    let parse_elapsed = parse_start.elapsed();

    let item_tree_start = Instant::now();
    let _item_tree = db.item_tree(file_id);
    let item_tree_elapsed = item_tree_start.elapsed();

    let module_bodies_start = Instant::now();
    let _bodies = db.module_bodies(ModuleId::new(file_id));
    let module_bodies_elapsed = module_bodies_start.elapsed();

    let deps_start = Instant::now();
    let deps = hir::file_dependencies_query(db, FileIdInput::new(db, file_id));
    let deps_elapsed = deps_start.elapsed();

    eprintln!("=== Bench: {} ===", canonical.display());
    eprintln!("file_id:               {}", file_id.0);
    eprintln!("file size (disk):      {} bytes ({:.1} KB)", size, size as f64 / 1024.0);
    // These timings are staged, not isolated: each step observes Salsa
    // memoization populated by earlier phases (item_tree builds on parse,
    // module_bodies on item_tree, …). The numbers reflect the marginal
    // wall-clock seen by an LSP request that triggers each phase for the
    // first time *after* the prior phases ran — which is the realistic
    // cold-cost of a hover crossing the closure boundary.
    eprintln!("---- staged cold phases (each marginal over prior) ----");
    eprintln!(
        "read_to_string:        {:.1} ms ({} bytes)",
        read_elapsed.as_secs_f64() * 1000.0,
        read_bytes,
    );
    eprintln!("parse:                 {:.1} ms", parse_elapsed.as_secs_f64() * 1000.0);
    eprintln!("item_tree (+parse):    {:.1} ms", item_tree_elapsed.as_secs_f64() * 1000.0);
    eprintln!("module_bodies (+above):{:.1} ms", module_bodies_elapsed.as_secs_f64() * 1000.0,);
    eprintln!("file_dependencies:     {:.1} ms", deps_elapsed.as_secs_f64() * 1000.0);
    eprintln!("L1 deps:               {}", deps.len());
    eprintln!("workspace load:        {:.1}s", load_elapsed.as_secs_f64());

    let fmt = |kb: Option<u64>| -> String {
        kb.map(|k| format!("{:.1} MB", k as f64 / 1024.0)).unwrap_or_else(|| "n/a".to_string())
    };
    if rss_baseline.is_some() || rss_after_load.is_some() {
        eprintln!();
        eprintln!("=== RSS snapshots (VmRSS) ===");
        eprintln!("baseline (pre-load):   {}", fmt(rss_baseline));
        eprintln!("after workspace load:  {}", fmt(rss_after_load));
        eprintln!("after bench:           {}", fmt(read_vmrss_kb()));
    }

    Ok(())
}

/// Bench for `deps --bench-index`: lexer-only sweep over every `.bsl`
/// file in `bsl_entries`, accumulate identifier tokens (lowercased) into
/// a `HashMap<Name, Vec<FileId>>`, and report wall-clock, unique-name count,
/// (name,file) pair count, and a rough byte-size estimate.
fn run_deps_bench_index(
    bsl_entries: Vec<(PathBuf, u64)>,
    index_workers: Option<usize>,
    rss_baseline: Option<u64>,
    walk_start: std::time::Instant,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;

    use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

    let walk_elapsed = walk_start.elapsed();
    let total = bsl_entries.len();
    let total_bytes: u64 =
        bsl_entries.iter().map(|(_, sz)| *sz).fold(0u64, |a, b| a.saturating_add(b));

    eprintln!("=== Bench: persistent name-index build ===");
    eprintln!("workspace files (.bsl):  {}", total);
    eprintln!("total bytes on disk:     {:.1} MB", total_bytes as f64 / 1024.0 / 1024.0);
    eprintln!("walk elapsed:            {:.1} ms", walk_elapsed.as_secs_f64() * 1000.0);

    let pool = if let Some(n) = index_workers {
        rayon::ThreadPoolBuilder::new().num_threads(n).build()?
    } else {
        rayon::ThreadPoolBuilder::new().build()?
    };
    let worker_count = pool.current_num_threads();
    eprintln!("workers:                 {}", worker_count);

    let build_start = Instant::now();

    // Parallel phase: per-file lex → unique identifier set. No locking,
    // no global state — each task is pure, results merged later.
    let per_file: Vec<(u32, HashSet<String>)> = pool.install(|| {
        bsl_entries
            .par_iter()
            .enumerate()
            .map(|(idx, (path, _))| {
                let content = std::fs::read_to_string(path).unwrap_or_default();
                let tokens = lexer::tokenize(&content);
                let mut names: HashSet<String> = HashSet::new();
                for t in &tokens {
                    if t.kind == lexer::TokenKind::Ident {
                        names.insert(t.text.as_str().to_lowercase());
                    }
                }
                (idx as u32, names)
            })
            .collect()
    });
    let lex_elapsed = build_start.elapsed();

    // Serial merge into the final `by_name` map. Could be parallelized
    // with a sharded reduce, but at 25k files this is sub-second and
    // keeps the measurement honest about the canonical-merge step.
    let mut by_name: HashMap<String, Vec<u32>> = HashMap::new();
    for (file_id, names) in &per_file {
        for n in names {
            by_name.entry(n.clone()).or_default().push(*file_id);
        }
    }
    let total_elapsed = build_start.elapsed();
    let merge_elapsed = total_elapsed - lex_elapsed;

    let unique_names = by_name.len();
    let total_pairs: usize = by_name.values().map(|v| v.len()).sum();
    // Rough lower bound on `by_name` memory: assume 24 B `String`
    // overhead per key + key bytes, plus 24 B `Vec` overhead per value
    // + 4 B per `FileId` entry. HashMap bucket overhead not counted.
    let est_key_bytes: usize = by_name.keys().map(|k| k.len() + 24).sum();
    let est_value_bytes: usize = by_name.values().map(|v| v.len() * 4 + 24).sum();
    let est_total = est_key_bytes + est_value_bytes;

    eprintln!();
    eprintln!("---- build phases ----");
    eprintln!("lex + per-file dedupe:   {:.2} s", lex_elapsed.as_secs_f64());
    eprintln!("merge into HashMap:      {:.2} s", merge_elapsed.as_secs_f64());
    eprintln!("total build:             {:.2} s", total_elapsed.as_secs_f64());
    eprintln!();
    eprintln!("---- index statistics ----");
    eprintln!("unique names:            {}", unique_names);
    eprintln!("(name, file) pairs:      {}", total_pairs);
    eprintln!(
        "avg names per file:      {:.1}",
        if total > 0 { total_pairs as f64 / total as f64 } else { 0.0 },
    );
    eprintln!("est. key bytes:          {:.1} MB", est_key_bytes as f64 / 1024.0 / 1024.0);
    eprintln!("est. value bytes:        {:.1} MB", est_value_bytes as f64 / 1024.0 / 1024.0);
    eprintln!("est. total index size:   {:.1} MB", est_total as f64 / 1024.0 / 1024.0,);

    if rss_baseline.is_some() {
        let fmt = |kb: Option<u64>| {
            kb.map(|k| format!("{:.1} MB", k as f64 / 1024.0)).unwrap_or_else(|| "n/a".to_string())
        };
        eprintln!();
        eprintln!("=== RSS snapshots (VmRSS) ===");
        eprintln!("baseline (pre-walk):     {}", fmt(rss_baseline));
        eprintln!("after index build:       {}", fmt(read_vmrss_kb()));
    }

    Ok(())
}
