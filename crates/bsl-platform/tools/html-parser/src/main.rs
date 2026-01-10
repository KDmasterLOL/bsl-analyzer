//! HTML parser for extracting BSL platform documentation.
//!
//! Parses extracted .hbk files and generates JSON with:
//! - BSL keywords from shlang_ru.hbk
//! - Platform types and methods from shcntx_ru.hbk

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct PlatformData {
    keywords: Vec<KeywordInfo>,
    types: Vec<TypeInfo>,
    methods: Vec<MethodInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct KeywordInfo {
    /// Russian name (e.g., "Функция")
    russian: String,
    /// English name (e.g., "Function")
    english: String,
    /// Snippet template in Russian
    snippet_ru: String,
    /// Snippet template in English
    snippet_en: String,
    /// HTML documentation
    documentation: String,
    /// Category (def, struct, root, etc.)
    category: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TypeInfo {
    /// Russian name (e.g., "Массив")
    name: String,
    /// English name (e.g., "Array")
    english_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MethodInfo {
    /// Method ID
    id: u32,
    /// Type name
    type_name: String,
    /// Russian method name
    name: String,
    /// English method name
    english_name: String,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 4 {
        eprintln!("Usage: {} <shlang_dir> <shcntx_dir> <output_json>", args[0]);
        std::process::exit(1);
    }

    let shlang_dir = PathBuf::from(&args[1]);
    let shcntx_dir = PathBuf::from(&args[2]);
    let output_path = PathBuf::from(&args[3]);

    println!("Parsing shlang from: {}", shlang_dir.display());
    println!("Parsing shcntx from: {}", shcntx_dir.display());

    // Parse keywords from shlang
    let keywords = parse_shlang_keywords(&shlang_dir)
        .context("Failed to parse shlang keywords")?;

    println!("Parsed {} keywords", keywords.len());

    // Parse platform types and methods from shcntx
    let (types, methods) = parse_shcntx_data(&shcntx_dir)
        .context("Failed to parse shcntx data")?;

    println!("Parsed {} types and {} methods", types.len(), methods.len());

    // Generate JSON
    let platform_data = PlatformData {
        keywords,
        types,
        methods,
    };

    let json = serde_json::to_string_pretty(&platform_data)
        .context("Failed to serialize to JSON")?;

    fs::write(&output_path, json)
        .context("Failed to write output JSON")?;

    println!("Generated: {}", output_path.display());
    Ok(())
}

/// Parses shlang directory for BSL keywords.
fn parse_shlang_keywords(shlang_dir: &Path) -> Result<Vec<KeywordInfo>> {
    let mut keywords = Vec::new();

    // Find all .st files (snippet templates)
    for entry in fs::read_dir(shlang_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("st") {
            if let Some(keyword) = parse_keyword_file(&path)? {
                keywords.push(keyword);
            }
        }
    }

    Ok(keywords)
}

/// Parses a single keyword file (.st + corresponding HTML file).
fn parse_keyword_file(st_path: &Path) -> Result<Option<KeywordInfo>> {
    let file_name = st_path.file_stem().unwrap().to_string_lossy();

    // Parse category and base name from filename
    // Examples: "def_Func", "struct_For", "root_New"
    let parts: Vec<&str> = file_name.split('_').collect();
    if parts.len() < 2 {
        return Ok(None);
    }

    let category = parts[0].to_string();
    let _base_name = parts[1..].join("_");

    // Parse .st file to get snippets
    let st_content = fs::read_to_string(st_path)?;
    let (snippet_ru, snippet_en, russian, english) = parse_st_snippet(&st_content)?;

    // Load corresponding HTML documentation
    let html_path = st_path.with_extension("");
    let documentation = if html_path.exists() {
        fs::read_to_string(&html_path).unwrap_or_default()
    } else {
        String::new()
    };

    Ok(Some(KeywordInfo {
        russian,
        english,
        snippet_ru,
        snippet_en,
        documentation,
        category,
    }))
}

/// Parses .st file to extract snippet templates.
///
/// Format:
/// ```
/// {1,{2,
/// {"",1,0,"",""},
/// {0,{"ru",0,0,"","Функция <?>()..."}},
/// {0,{"en",0,0,"","Function <?>()..."}}
/// }}
/// ```
fn parse_st_snippet(content: &str) -> Result<(String, String, String, String)> {
    // Simple regex-like parsing of the structured format
    let mut snippet_ru = String::new();
    let mut snippet_en = String::new();
    let mut russian_name = String::new();
    let mut english_name = String::new();

    // Find Russian snippet: {"ru",0,0,"","..."}
    // The snippet is the 5th element (after 4 commas)
    if let Some(ru_start) = content.find(r#"{"ru",0,0,""#) {
        let after_pattern = &content[ru_start + r#"{"ru",0,0,""#.len()..];
        // Skip opening comma-quote: ,"
        if let Some(snippet_start) = after_pattern.find(r#","#) {
            let snippet_text = &after_pattern[snippet_start + 2..]; // Skip ,"
            if let Some(snippet_end) = snippet_text.find(r#""}"#) {
                snippet_ru = snippet_text[..snippet_end].to_string();
                // Extract first word as Russian name
                russian_name = snippet_ru
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
        }
    }

    // Find English snippet: {"en",0,0,"","..."}
    if let Some(en_start) = content.find(r#"{"en",0,0,""#) {
        let after_pattern = &content[en_start + r#"{"en",0,0,""#.len()..];
        // Skip opening comma-quote: ,"
        if let Some(snippet_start) = after_pattern.find(r#","#) {
            let snippet_text = &after_pattern[snippet_start + 2..]; // Skip ,"
            if let Some(snippet_end) = snippet_text.find(r#""}"#) {
                snippet_en = snippet_text[..snippet_end].to_string();
                // Extract first word as English name
                english_name = snippet_en
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
        }
    }

    Ok((snippet_ru, snippet_en, russian_name, english_name))
}

/// Parses shcntx directory for platform types and methods.
fn parse_shcntx_data(_shcntx_dir: &Path) -> Result<(Vec<TypeInfo>, Vec<MethodInfo>)> {
    // TODO: Implement shcntx parsing in next iteration
    // For now, return empty data
    Ok((Vec::new(), Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_st_snippet() {
        let content = r#"{1,
{2,
{"",1,0,"",""},
{0,
{"ru",0,0,"","Функция <?>()
Возврат ;
КонецФункции "}
},
{0,
{"en",0,0,"","Function <?>()
Return ;
EndFunction"}
}
}
}"#;

        let (snippet_ru, snippet_en, russian, english) = parse_st_snippet(content).unwrap();

        assert_eq!(russian, "Функция");
        assert_eq!(english, "Function");
        assert!(snippet_ru.contains("Функция <?>()"));
        assert!(snippet_en.contains("Function <?>()"));
    }
}
