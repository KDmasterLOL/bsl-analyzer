use bsl_search::Chunker;

fn main() {
    let path = std::env::args().nth(1).expect("Usage: debug_chunks <file.bsl>");
    let content = std::fs::read_to_string(&path).expect("failed to read file");

    println!("File: {path}");
    println!("Size: {} bytes, {} lines", content.len(), content.lines().count());
    println!();

    let chunks = Chunker::chunk(&content);
    println!("Chunks: {}", chunks.len());
    println!();

    for (i, chunk) in chunks.iter().enumerate() {
        let preview: String = chunk.text.lines().take(2).collect::<Vec<_>>().join(" | ");
        println!(
            "#{:>4} {:?} name={:30} export={} ann={:?} lines={}-{} size={} bytes",
            i + 1,
            chunk.kind,
            if chunk.name.is_empty() { "<header>" } else { &chunk.name },
            chunk.is_export,
            chunk.annotations,
            chunk.line_start,
            chunk.line_end,
            chunk.text.len(),
        );
        let limit = preview.char_indices().nth(100).map_or(preview.len(), |(i, _)| i);
        println!("      preview: {}", &preview[..limit]);
        println!();
    }
}
