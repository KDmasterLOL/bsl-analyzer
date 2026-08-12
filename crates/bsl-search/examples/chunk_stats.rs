use bsl_search::Chunker;

fn main() {
    let root = std::env::args().nth(1).expect("Usage: chunk_stats <config_dir>");
    let mut sizes: Vec<(usize, String, String)> = Vec::new();

    for entry in walkdir::WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
        if !bsl_conventions::has_extension(entry.path(), bsl_conventions::BSL_EXTENSION) {
            continue;
        }
        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
        let chunks = Chunker::chunk(&content);
        for chunk in &chunks {
            let label = if chunk.name.is_empty() {
                format!("{:?}", chunk.kind)
            } else {
                chunk.name.clone()
            };
            sizes.push((chunk.text.len(), rel.to_string_lossy().to_string(), label));
        }
    }

    sizes.sort_by_key(|(s, _, _)| *s);
    let n = sizes.len();

    println!("Total chunks: {n}");
    if n == 0 {
        return;
    }

    println!("Min:    {:>8} bytes", sizes[0].0);
    println!("Median: {:>8} bytes", sizes[n / 2].0);
    println!("P95:    {:>8} bytes", sizes[n * 95 / 100].0);
    println!("P99:    {:>8} bytes", sizes[n * 99 / 100].0);
    println!("Max:    {:>8} bytes", sizes[n - 1].0);

    println!("\nTop 20 largest chunks:");
    for (size, path, name) in sizes.iter().rev().take(20) {
        println!("  {:>8} bytes  ({:>7.1} KB)  {} :: {}", size, *size as f64 / 1024.0, path, name);
    }

    let buckets = [1024, 4096, 16384, 65536, 262144, 1048576];
    println!("\nSize distribution:");
    let mut prev = 0;
    for &limit in &buckets {
        let count = sizes.iter().filter(|(s, _, _)| *s >= prev && *s < limit).count();
        println!("  {:>7}-{:<7}: {:>6} chunks", prev, limit, count);
        prev = limit;
    }
    let big = sizes.iter().filter(|(s, _, _)| *s >= *buckets.last().unwrap()).count();
    println!("  {:>7}+      : {:>6} chunks", buckets.last().unwrap(), big);
}
