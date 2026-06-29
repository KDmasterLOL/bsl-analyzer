//! Salsa memory snapshot formatting, shared by the batch analyzer and the smoke
//! `session` scenario. Reads salsa's `memory_usage` introspection via
//! [`ide::RootDatabaseImpl::memory_report`] and prints a per-ingredient table
//! sorted by live entry count, plus a process RSS cross-check. The strongest
//! signal is `count` — whether an ingredient's LRU is actually evicting — since
//! absolute bytes are reported only for ingredients with a `heap_size` hook.

/// One ingredient's snapshot row: `(name, live entry count, salsa metadata
/// bytes, field-stack bytes, optional heap bytes)`, sorted by descending count.
pub type SalsaMemoryRow = (&'static str, usize, usize, usize, Option<usize>);

/// Per-ingredient salsa memory rows, sorted by descending live entry count.
pub fn salsa_memory_rows(db: &ide::RootDatabaseImpl) -> Vec<SalsaMemoryRow> {
    let mut rows = db.memory_report();
    rows.sort_by_key(|(_, count, ..)| std::cmp::Reverse(*count));
    rows
}

fn proc_kb(key: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok())
}

/// Print the salsa memory map (top 40 ingredients by live entry count) under a
/// caller-supplied `label`, followed by salsa totals and a process RSS/HWM
/// cross-check. Emitted to stderr so it never pollutes machine-readable stdout.
pub fn print_salsa_memory_report(db: &ide::RootDatabaseImpl, label: &str) {
    let rows = salsa_memory_rows(db);

    let (mut tc, mut tm, mut tf, mut th) = (0usize, 0usize, 0usize, 0usize);
    for (_, c, m, f, h) in &rows {
        tc += c;
        tm += m;
        tf += f;
        th += h.unwrap_or(0);
    }

    eprintln!(
        "\n##### salsa memory_usage — {label} (top 40 ingredients by live entry count) #####"
    );
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
