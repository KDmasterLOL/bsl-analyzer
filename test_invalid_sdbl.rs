use parser::parse_sdbl;

fn main() {
    // Test cases
    let queries = vec![
        "ВЫБРАТЬ ИЗ",          // Missing fields
        "это вообще не запрос", // Garbage
        "SELECT",              // Incomplete
        "SELECT * FROM",       // Missing table
    ];

    for query in queries {
        println!("\n=== Query: {:?} ===", query);
        let parse = parse_sdbl(query);
        println!("Has errors: {}", parse.has_errors());
        println!("Error count: {}", parse.errors().len());
        for error in parse.errors() {
            println!("  - {}", error.message());
        }
        
        let tree = format!("{:#?}", parse.syntax_node());
        println!("Tree has ERROR nodes: {}", tree.contains("ERROR"));
        if tree.contains("ERROR") {
            let lines: Vec<&str> = tree.lines()
                .filter(|line| line.contains("ERROR"))
                .take(5)
                .collect();
            for line in lines {
                println!("  {}", line);
            }
        }
    }
}
