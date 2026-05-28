use std::{error::Error, fs, path::PathBuf};

pub fn run_format(
    file: PathBuf,
    write: bool,
    spaces: bool,
    indent_size: u32,
    check: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::time::Instant;

    use ide::formatting::{format_file, FormattingConfig};

    let content = fs::read_to_string(&file)?;
    let file_size = content.len();
    let line_count = content.lines().count();

    if !check {
        eprintln!("Formatting: {:?}", file);
        eprintln!("File size: {} bytes, {} lines", file_size, line_count);
    }

    let start = Instant::now();
    let parsed = parser::parse(&content);
    let parse_time = start.elapsed();
    if !check {
        eprintln!("Parse time: {:?}", parse_time);
    }

    let root = parsed.syntax_node();

    let config = if spaces {
        FormattingConfig::with_spaces(indent_size)
    } else {
        FormattingConfig::default()
    };

    let start = Instant::now();
    let result = format_file(&root, &config);
    let format_time = start.elapsed();
    if !check {
        eprintln!("Format time: {:?}", format_time);
        eprintln!("Total time: {:?}", parse_time + format_time);
    }

    if check {
        if result.text == content {
            return Ok(());
        }
        eprintln!("would reformat: {}", file.display());
        std::process::exit(1);
    }

    if write {
        fs::write(&file, &result.text)?;
        eprintln!("Written to: {:?}", file);
    } else {
        print!("{}", result.text);
    }

    Ok(())
}
