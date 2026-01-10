//! Build script for bsl-platform.
//!
//! Detects installed 1C:Enterprise platform and extracts documentation from shcntx_ru.hbk.
//! If platform is not found, generates minimal data structure without documentation.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=data/platform_minimal.json");
    println!("cargo:rerun-if-env-changed=BSL_PLATFORM_PATH");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let generated_path = out_dir.join("generated.rs");

    // Try to find 1C platform and extract documentation
    match find_and_extract_platform_data() {
        Some(json_path) => {
            println!("cargo:warning=Found 1C platform, building with full documentation");
            println!("cargo:rustc-cfg=feature=\"platform_docs\"");
            generate_code_from_json(&json_path, &generated_path, true);
        }
        None => {
            println!("cargo:warning=1C platform not found, building without documentation");
            println!("cargo:warning=Install 1C:Enterprise to enable platform documentation");

            // Check if minimal data exists
            let minimal_path = PathBuf::from("data/platform_minimal.json");
            if minimal_path.exists() {
                generate_code_from_json(&minimal_path, &generated_path, false);
            } else {
                // Generate empty structures
                generate_empty_structures(&generated_path);
            }
        }
    }
}

/// Finds 1C platform installation and extracts documentation.
fn find_and_extract_platform_data() -> Option<PathBuf> {
    // Check environment variable first
    if let Ok(path) = env::var("BSL_PLATFORM_PATH") {
        let base_path = PathBuf::from(path);
        let shcntx_path = base_path.join("shcntx_ru.hbk");
        let shlang_path = base_path.join("shlang_ru.hbk");

        if shcntx_path.exists() && shlang_path.exists() {
            return extract_both_help_files(&shcntx_path, &shlang_path);
        }
    }

    // Try to find 1C installation
    let (shcntx_path, shlang_path) = find_1c_help_files()?;
    extract_both_help_files(&shcntx_path, &shlang_path)
}

/// Searches for 1C help files in standard installation locations.
/// Returns (shcntx_ru.hbk, shlang_ru.hbk) paths.
fn find_1c_help_files() -> Option<(PathBuf, PathBuf)> {
    // Linux: /opt/1cv8/x86_64/*/
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = fs::read_dir("/opt/1cv8/x86_64") {
            for entry in entries.flatten() {
                let shcntx_path = entry.path().join("shcntx_ru.hbk");
                let shlang_path = entry.path().join("shlang_ru.hbk");

                if shcntx_path.exists() && shlang_path.exists() {
                    println!("cargo:warning=Found 1C help files at: {}", entry.path().display());
                    return Some((shcntx_path, shlang_path));
                }
            }
        }
    }

    // macOS: /opt/1cv8/*/
    #[cfg(target_os = "macos")]
    {
        if let Ok(entries) = fs::read_dir("/opt/1cv8") {
            for entry in entries.flatten() {
                let path = entry.path();

                // Skip common, conf, etc. - only process version directories
                if !path.is_dir() {
                    continue;
                }

                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if dir_name == "common" || dir_name == "conf" {
                    continue;
                }

                let shcntx_path = path.join("shcntx_ru.hbk");
                let shlang_path = path.join("shlang_ru.hbk");

                if shcntx_path.exists() && shlang_path.exists() {
                    println!("cargo:warning=Found 1C help files at: {}", path.display());
                    return Some((shcntx_path, shlang_path));
                }
            }
        }
    }

    // Windows: C:\Program Files\1cv8\*\
    #[cfg(target_os = "windows")]
    {
        let program_files =
            env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
        let base_path = PathBuf::from(program_files).join("1cv8");

        if let Ok(entries) = fs::read_dir(&base_path) {
            for entry in entries.flatten() {
                let shcntx_path = entry.path().join("shcntx_ru.hbk");
                let shlang_path = entry.path().join("shlang_ru.hbk");

                if shcntx_path.exists() && shlang_path.exists() {
                    println!("cargo:warning=Found 1C help files at: {}", entry.path().display());
                    return Some((shcntx_path, shlang_path));
                }
            }
        }
    }

    None
}

/// Extracts a single .hbk file using 7z.
fn extract_hbk_file(hbk_path: &Path, name: &str) -> Option<PathBuf> {
    // Create temp directory for extraction
    let temp_dir = env::temp_dir().join(format!("bsl-platform-{}", name));
    let _ = fs::remove_dir_all(&temp_dir); // Clean up previous extraction
    fs::create_dir_all(&temp_dir).ok()?;

    // Extract with 7z: 7z x -y -o<output_dir> <input_file>
    // Note: Some .hbk files have minor errors but still extract most files
    let output_arg = format!("-o{}", temp_dir.display());
    let _output = Command::new("7z").args(["x", "-y", &output_arg]).arg(hbk_path).output().ok()?;

    // Don't check exit code - 7z returns error for corrupted archives even if most files extracted
    // Instead, check if any files were actually extracted
    if let Ok(entries) = fs::read_dir(&temp_dir) {
        let file_count = entries.count();
        if file_count > 0 {
            println!("cargo:warning=Extracted {} files from {}", file_count, name);
            return Some(temp_dir);
        }
    }

    println!("cargo:warning=No files extracted from {}", name);
    None
}

/// Extracts both help files and combines data.
fn extract_both_help_files(shcntx_path: &Path, shlang_path: &Path) -> Option<PathBuf> {
    println!("cargo:warning=Extracting platform help files...");
    println!("cargo:warning=  shcntx_ru.hbk: {}", shcntx_path.display());
    println!("cargo:warning=  shlang_ru.hbk: {}", shlang_path.display());

    // Extract shlang_ru.hbk (keywords) - small file, extract first
    let shlang_dir = extract_hbk_file(shlang_path, "shlang")?;
    println!("cargo:warning=Extracted shlang to: {}", shlang_dir.display());

    // Extract shcntx_ru.hbk (platform types/methods) - large file
    let shcntx_dir = extract_hbk_file(shcntx_path, "shcntx")?;
    println!("cargo:warning=Extracted shcntx to: {}", shcntx_dir.display());

    // Check if HTML parser tool exists
    let parser_binary = PathBuf::from("tools/html-parser/target/release/html-parser");
    if !parser_binary.exists() {
        println!("cargo:warning=HTML parser not built, skipping documentation extraction");
        println!(
            "cargo:warning=Run: cargo build --release --manifest-path tools/html-parser/Cargo.toml"
        );
        return None;
    }

    // Parse HTML files to JSON
    let json_output = PathBuf::from(env::var("OUT_DIR").unwrap()).join("platform_data.json");
    let output = Command::new(&parser_binary)
        .arg(&shlang_dir)
        .arg(&shcntx_dir)
        .arg(&json_output)
        .output()
        .ok()?;

    if !output.status.success() {
        println!("cargo:warning=Failed to parse HTML files");
        println!("cargo:warning=Parser stderr: {}", String::from_utf8_lossy(&output.stderr));
        return None;
    }

    println!("cargo:warning=Generated platform data: {}", json_output.display());
    Some(json_output)
}

/// Generates Rust code from JSON data.
fn generate_code_from_json(json_path: &Path, output_path: &Path, _with_docs: bool) {
    let json_content = fs::read_to_string(json_path).expect("Failed to read JSON file");

    let data: serde_json::Value =
        serde_json::from_str(&json_content).expect("Failed to parse JSON");

    let mut code = String::new();
    code.push_str("// Auto-generated by build.rs\n");
    code.push_str("// DO NOT EDIT MANUALLY\n\n");
    code.push_str("use super::types::*;\n\n");

    // Generate platform types array
    if let Some(types) = data.get("types").and_then(|v| v.as_array()) {
        code.push_str("pub const PLATFORM_TYPES: &[RawPlatformType] = &[\n");
        for ty in types {
            let name = ty.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let english_name = ty.get("english_name").and_then(|v| v.as_str()).unwrap_or("");

            code.push_str("    RawPlatformType {\n");
            code.push_str(&format!("        name: {:?},\n", name));
            code.push_str(&format!("        english_name: {:?},\n", english_name));
            code.push_str("    },\n");
        }
        code.push_str("];\n\n");
    } else {
        code.push_str("pub const PLATFORM_TYPES: &[RawPlatformType] = &[];\n\n");
    }

    // Generate platform methods array
    if let Some(methods) = data.get("methods").and_then(|v| v.as_array()) {
        code.push_str("pub const PLATFORM_METHODS: &[RawPlatformMethod] = &[\n");
        for method in methods {
            let id = method.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let type_name = method.get("type_name").and_then(|v| v.as_str()).unwrap_or("");
            let name = method.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let english_name = method.get("english_name").and_then(|v| v.as_str()).unwrap_or("");

            code.push_str("    RawPlatformMethod {\n");
            code.push_str(&format!("        id: {},\n", id));
            code.push_str(&format!("        type_name: {:?},\n", type_name));
            code.push_str(&format!("        name: {:?},\n", name));
            code.push_str(&format!("        english_name: {:?},\n", english_name));
            code.push_str("    },\n");
        }
        code.push_str("];\n");
    } else {
        code.push_str("pub const PLATFORM_METHODS: &[RawPlatformMethod] = &[];\n");
    }

    fs::write(output_path, code).expect("Failed to write generated.rs");
    println!("cargo:warning=Generated code at: {}", output_path.display());
}

/// Generates empty structures when no data is available.
fn generate_empty_structures(output_path: &Path) {
    let code = r#"// Auto-generated by build.rs
// NO PLATFORM DATA AVAILABLE

use super::types::*;

pub const PLATFORM_TYPES: &[RawPlatformType] = &[];
pub const PLATFORM_METHODS: &[RawPlatformMethod] = &[];
"#;

    fs::write(output_path, code).expect("Failed to write generated.rs");
    println!("cargo:warning=Generated empty structures (no platform data)");
}
