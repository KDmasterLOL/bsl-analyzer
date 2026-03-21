//! Demo: index a 1C configuration and run semantic search queries.
//!
//! Prerequisites:
//!   1. Install ollama: https://ollama.com
//!   2. Pull the model: `ollama pull qwen3-embedding`
//!   3. Ollama runs on http://localhost:11434 by default
//!
//! Usage:
//!   cargo run -p bsl-search --example search_demo -- <config_dir> [query]
//!
//! Examples:
//!   # Index and search:
//!   cargo run -p bsl-search --example search_demo -- ~/src/niagara_ut/src "обработка проведения документа"
//!
//!   # Just index (no query):
//!   cargo run -p bsl-search --example search_demo -- ~/src/niagara_ut/src
//!
//! Environment variables:
//!   EMBEDDING_URL   - embedding API base URL (default: http://localhost:11434)
//!   EMBEDDING_MODEL - model name (default: qwen3-embedding)
//!   EMBEDDING_DIM   - embedding dimension (default: 1024)
//!   BATCH_SIZE      - batch size for embedding (default: 32)

use bsl_search::{EmbedderConfig, SearchConfig, SearchEngine};
use project_model::Project;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    // Init tracing.
    tracing_subscriber::fmt().with_max_level(tracing_subscriber::filter::LevelFilter::INFO).init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: search_demo <config_dir> [query]");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  search_demo ~/src/niagara_ut/src \"обработка проведения\"");
        eprintln!("  search_demo ~/src/niagara_ut/src");
        std::process::exit(1);
    }

    let project_dir = PathBuf::from(&args[1]);
    let query = args.get(2).map(|s| s.as_str());

    if !project_dir.is_dir() {
        eprintln!("Error: {} is not a directory", project_dir.display());
        std::process::exit(1);
    }

    // Use project-model to discover the configuration source path.
    let project = Project::new(&project_dir);
    let config_dir = project.source_path().to_path_buf();

    // Configure embedding API.
    let base_url =
        std::env::var("EMBEDDING_URL").unwrap_or_else(|_| "http://localhost:11434".to_owned());
    let model = std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "qwen3-embedding".to_owned());
    let dim: usize =
        std::env::var("EMBEDDING_DIM").ok().and_then(|s| s.parse().ok()).unwrap_or(1024);
    let batch_size: usize =
        std::env::var("BATCH_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(32);

    let config = SearchConfig {
        embedder: EmbedderConfig { base_url, model: model.clone(), dim: Some(dim) },
        batch_size,
    };

    // Database stored in .build/ (typically gitignored).
    let build_dir = project_dir.join(".build");
    std::fs::create_dir_all(&build_dir).ok();
    let db_path = build_dir.join("bsl-search.db");

    println!("=== BSL Search Demo ===");
    println!("Config dir:  {}", config_dir.display());
    println!("Database:    {}", db_path.display());
    println!("Model:       {model}");
    println!("Dimension:   {dim}");
    println!("Batch size:  {batch_size}");
    println!();

    // Check embedding service.
    let embedder = bsl_search::Embedder::new(EmbedderConfig {
        base_url: config.embedder.base_url.clone(),
        model: config.embedder.model.clone(),
        dim: config.embedder.dim,
    });
    if let Err(e) = embedder.health_check() {
        eprintln!("Error: embedding service not available: {e}");
        eprintln!();
        eprintln!("Make sure ollama is running:");
        eprintln!("  ollama serve");
        eprintln!("  ollama pull qwen3-embedding");
        std::process::exit(1);
    }
    println!("Embedding service: OK");

    // Create search engine.
    let mut engine = match SearchEngine::new(&db_path, config) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error creating search engine: {e}");
            std::process::exit(1);
        }
    };

    println!(
        "Existing index: {} files, {} chunks, {} vectors",
        engine.file_count().unwrap_or(0),
        engine.chunk_count().unwrap_or(0),
        engine.vector_count(),
    );
    println!();

    // Index.
    println!("Indexing...");
    let start = Instant::now();
    match engine.index_directory(&config_dir, None) {
        Ok(indexed) => {
            let elapsed = start.elapsed();
            println!(
                "Indexed {indexed} files in {:.1}s ({} total chunks, {} vectors)",
                elapsed.as_secs_f64(),
                engine.chunk_count().unwrap_or(0),
                engine.vector_count(),
            );
        }
        Err(e) => {
            eprintln!("Indexing error: {e}");
            std::process::exit(1);
        }
    }
    println!();

    // Search.
    if let Some(query) = query {
        // Prefix "fts:" uses full-text search, otherwise semantic search.
        let (mode, actual_query) = if let Some(q) = query.strip_prefix("fts:") {
            ("FTS", q.trim())
        } else {
            ("semantic", query)
        };

        println!("Searching ({mode}): \"{actual_query}\"");
        println!("{}", "─".repeat(60));

        let start = Instant::now();
        let result = if mode == "FTS" {
            engine.text_search(actual_query, 10, None)
        } else {
            engine.search(actual_query, 10, None)
        };

        match result {
            Ok(hits) => {
                let elapsed = start.elapsed();
                println!(
                    "Found {} results in {:.1}ms\n",
                    hits.len(),
                    elapsed.as_secs_f64() * 1000.0
                );

                for (i, hit) in hits.iter().enumerate() {
                    println!(
                        "#{} [{:.3}] {} :: {} ({})",
                        i + 1,
                        hit.score,
                        hit.file_path,
                        if hit.symbol_name.is_empty() { "<header>" } else { &hit.symbol_name },
                        hit.kind,
                    );
                    println!("   Lines {}-{}", hit.line_start + 1, hit.line_end);

                    // Show first 3 lines of the chunk.
                    let preview: String = hit
                        .text
                        .lines()
                        .take(3)
                        .map(|l| format!("   │ {l}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    println!("{preview}");
                    println!();
                }
            }
            Err(e) => {
                eprintln!("Search error: {e}");
            }
        }
    } else {
        println!("No query provided. Run with a query argument to search:");
        println!(
            "  cargo run -p bsl-search --example search_demo -- {} \"your query\"",
            config_dir.display()
        );
        println!("  # Prefix 'fts:' for full-text search:");
        println!(
            "  cargo run -p bsl-search --example search_demo -- {} \"fts:ОбработкаПроведения\"",
            config_dir.display()
        );
    }
}
