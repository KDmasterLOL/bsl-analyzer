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
        let hbk_path = PathBuf::from(path).join("shcntx_ru.hbk");
        if hbk_path.exists() {
            return extract_platform_data(&hbk_path);
        }
    }

    // Try to find 1C installation
    let hbk_path = find_1c_help_file()?;
    extract_platform_data(&hbk_path)
}

/// Searches for shcntx_ru.hbk in standard installation locations.
fn find_1c_help_file() -> Option<PathBuf> {
    // Linux: /opt/1cv8/x86_64/*/shcntx_ru.hbk
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = fs::read_dir("/opt/1cv8/x86_64") {
            for entry in entries.flatten() {
                let hbk_path = entry.path().join("shcntx_ru.hbk");
                if hbk_path.exists() {
                    println!("cargo:warning=Found 1C help at: {}", hbk_path.display());
                    return Some(hbk_path);
                }
            }
        }
    }

    // Windows: C:\Program Files\1cv8\*\shcntx_ru.hbk
    #[cfg(target_os = "windows")]
    {
        let program_files =
            env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
        let base_path = PathBuf::from(program_files).join("1cv8");

        if let Ok(entries) = fs::read_dir(&base_path) {
            for entry in entries.flatten() {
                let hbk_path = entry.path().join("shcntx_ru.hbk");
                if hbk_path.exists() {
                    println!("cargo:warning=Found 1C help at: {}", hbk_path.display());
                    return Some(hbk_path);
                }
            }
        }
    }

    None
}

/// Extracts platform documentation from .hbk file using 7z.
fn extract_platform_data(hbk_path: &Path) -> Option<PathBuf> {
    println!("cargo:warning=Extracting platform data from: {}", hbk_path.display());

    // Create temp directory for extraction
    let temp_dir = env::temp_dir().join("bsl-platform-extract");
    let _ = fs::remove_dir_all(&temp_dir); // Clean up previous extraction
    fs::create_dir_all(&temp_dir).ok()?;

    // Extract with 7z
    let output =
        Command::new("7z").args(["x", "-y", "-o"]).arg(&temp_dir).arg(hbk_path).output().ok()?;

    if !output.status.success() {
        println!("cargo:warning=Failed to extract .hbk file with 7z");
        return None;
    }

    println!("cargo:warning=Extracted to: {}", temp_dir.display());

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
    let output = Command::new(&parser_binary).arg(&temp_dir).arg(&json_output).output().ok()?;

    if !output.status.success() {
        println!("cargo:warning=Failed to parse HTML files");
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
    code.push_str("use smol_str::SmolStr;\n\n");

    // Generate platform types array
    if let Some(types) = data.get("types").and_then(|v| v.as_array()) {
        code.push_str("pub const PLATFORM_TYPES: &[PlatformType] = &[\n");
        for ty in types {
            let name = ty.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let english_name = ty.get("english_name").and_then(|v| v.as_str()).unwrap_or("");

            code.push_str("    PlatformType {\n");
            code.push_str(&format!("        name: SmolStr::new_inline({:?}),\n", name));
            code.push_str(&format!(
                "        english_name: SmolStr::new_inline({:?}),\n",
                english_name
            ));
            code.push_str("    },\n");
        }
        code.push_str("];\n\n");
    } else {
        code.push_str("pub const PLATFORM_TYPES: &[PlatformType] = &[];\n\n");
    }

    // Generate platform methods array (stub for now)
    code.push_str("pub const PLATFORM_METHODS: &[PlatformMethod] = &[];\n");

    fs::write(output_path, code).expect("Failed to write generated.rs");
    println!("cargo:warning=Generated code at: {}", output_path.display());
}

/// Generates empty structures when no data is available.
fn generate_empty_structures(output_path: &Path) {
    let code = r#"// Auto-generated by build.rs
// NO PLATFORM DATA AVAILABLE

use smol_str::SmolStr;

pub const PLATFORM_TYPES: &[PlatformType] = &[];
pub const PLATFORM_METHODS: &[PlatformMethod] = &[];
"#;

    fs::write(output_path, code).expect("Failed to write generated.rs");
    println!("cargo:warning=Generated empty structures (no platform data)");
}
