//! Build script for bsl-platform.
//!
//! Detects installed 1C:Enterprise platform and extracts documentation from shcntx_ru.hbk.
//! If platform is not found, generates minimal data structure without documentation.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Finds paths to 1C help files without extracting them.
fn find_1c_help_file_paths() -> Option<(PathBuf, PathBuf)> {
    // Check environment variable first
    if let Ok(path) = env::var("BSL_PLATFORM_PATH") {
        let base_path = PathBuf::from(path);
        let shcntx_path = base_path.join("shcntx_ru.hbk");
        let shlang_path = base_path.join("shlang_ru.hbk");

        if shcntx_path.exists() && shlang_path.exists() {
            return Some((shcntx_path, shlang_path));
        }
    }

    // Try to find 1C installation
    find_1c_help_files()
}

/// Gets a timestamp string for the .hbk files (modification times).
fn get_files_timestamp(shcntx_path: &Path, shlang_path: &Path) -> String {
    let shcntx_time = fs::metadata(shcntx_path)
        .and_then(|m| m.modified())
        .map(|t| format!("{:?}", t))
        .unwrap_or_default();

    let shlang_time = fs::metadata(shlang_path)
        .and_then(|m| m.modified())
        .map(|t| format!("{:?}", t))
        .unwrap_or_default();

    format!("{}|{}", shcntx_time, shlang_time)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=BSL_PLATFORM_PATH");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let generated_path = out_dir.join("generated.rs");

    // Store timestamp in crate root (stable location across builds)
    let crate_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let timestamp_path = crate_root.join(".platform_cache_timestamp");

    // Check if we can skip regeneration
    if let Some(hbk_files) = find_1c_help_file_paths() {
        let (shcntx_path, shlang_path) = hbk_files;

        // Tell Cargo to rerun if .hbk files change
        println!("cargo:rerun-if-changed={}", shcntx_path.display());
        println!("cargo:rerun-if-changed={}", shlang_path.display());

        // Check if generated.rs exists and .hbk files haven't changed
        if generated_path.exists() && timestamp_path.exists() {
            if let Ok(stored_timestamp) = fs::read_to_string(&timestamp_path) {
                let current_timestamp = get_files_timestamp(&shcntx_path, &shlang_path);
                if stored_timestamp == current_timestamp {
                    // No changes detected, skip regeneration
                    println!("cargo:rustc-cfg=feature=\"platform_docs\"");
                    return;
                }
            }
        }

        // Extract and generate
        if let Some(json_path) = extract_both_help_files(&shcntx_path, &shlang_path) {
            println!("cargo:warning=Found 1C platform, building with full documentation");
            println!("cargo:rustc-cfg=feature=\"platform_docs\"");
            generate_code_from_json(&json_path, &generated_path, true);

            // Save timestamp for future builds
            let timestamp = get_files_timestamp(&shcntx_path, &shlang_path);
            let _ = fs::write(&timestamp_path, timestamp);
            return;
        }
    }

    // Fallback: no platform found or extraction failed
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

/// Searches for 1C help files in standard installation locations.
/// Returns (shcntx_ru.hbk, shlang_ru.hbk) paths without printing anything.
fn find_1c_help_files() -> Option<(PathBuf, PathBuf)> {
    // Linux: /opt/1cv8/x86_64/*/
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = fs::read_dir("/opt/1cv8/x86_64") {
            for entry in entries.flatten() {
                let shcntx_path = entry.path().join("shcntx_ru.hbk");
                let shlang_path = entry.path().join("shlang_ru.hbk");

                if shcntx_path.exists() && shlang_path.exists() {
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
    // Get parent directory for display
    let platform_dir = shcntx_path.parent().map(|p| p.display().to_string()).unwrap_or_default();

    println!("cargo:warning=Found 1C platform at: {}", platform_dir);
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
        // Clean up temp directories
        let _ = fs::remove_dir_all(&shlang_dir);
        let _ = fs::remove_dir_all(&shcntx_dir);
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

    // Clean up temporary extraction directories
    println!("cargo:warning=Cleaning up temporary files...");
    let _ = fs::remove_dir_all(&shlang_dir);
    let _ = fs::remove_dir_all(&shcntx_dir);

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

    // Generate context availability constants
    let mut context_counter = 0;
    let mut type_context_names = Vec::new();

    if let Some(types) = data.get("types").and_then(|v| v.as_array()) {
        for ty in types {
            if let Some(context) = ty.get("context").and_then(|v| v.as_object()) {
                let context_name = format!("TYPE_CONTEXT_{}", context_counter);
                context_counter += 1;

                code.push_str(&format!(
                    "const {}: RawContextAvailability = RawContextAvailability {{\n",
                    context_name
                ));
                code.push_str(&format!(
                    "    thick_client: {},\n",
                    context.get("thick_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    thin_client: {},\n",
                    context.get("thin_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    web_client: {},\n",
                    context.get("web_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    server: {},\n",
                    context.get("server").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    mobile_client: {},\n",
                    context.get("mobile_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    external_connection: {},\n",
                    context.get("external_connection").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str("};\n\n");

                type_context_names.push(Some(context_name));
            } else {
                type_context_names.push(None);
            }
        }
    }

    // Generate method parameter arrays and context availability constants
    let mut param_counter = 0;
    let mut method_param_names = Vec::new();
    let mut method_context_names = Vec::new();
    let mut global_function_param_names: Vec<Option<String>> = Vec::new();
    let mut global_function_context_names: Vec<Option<String>> = Vec::new();

    if let Some(methods) = data.get("methods").and_then(|v| v.as_array()) {
        for method in methods {
            // Generate parameters array
            if let Some(params) = method.get("parameters").and_then(|v| v.as_array()) {
                if !params.is_empty() {
                    let param_name = format!("METHOD_PARAMS_{}", param_counter);
                    param_counter += 1;

                    code.push_str(&format!("const {}: &[RawMethodParam] = &[\n", param_name));
                    for param in params {
                        code.push_str("    RawMethodParam {\n");
                        code.push_str(&format!(
                            "        name: {:?},\n",
                            param.get("name").and_then(|v| v.as_str()).unwrap_or("")
                        ));

                        if let Some(param_type) = param.get("param_type").and_then(|v| v.as_str()) {
                            code.push_str(&format!(
                                "        param_type: Some({:?}),\n",
                                param_type
                            ));
                        } else {
                            code.push_str("        param_type: None,\n");
                        }

                        code.push_str(&format!(
                            "        is_optional: {},\n",
                            param.get("is_optional").and_then(|v| v.as_bool()).unwrap_or(false)
                        ));
                        code.push_str("    },\n");
                    }
                    code.push_str("];\n\n");

                    method_param_names.push(Some(param_name));
                } else {
                    method_param_names.push(None);
                }
            } else {
                method_param_names.push(None);
            }

            // Generate context availability
            if let Some(context) = method.get("context").and_then(|v| v.as_object()) {
                let context_name = format!("METHOD_CONTEXT_{}", context_counter);
                context_counter += 1;

                code.push_str(&format!(
                    "const {}: RawContextAvailability = RawContextAvailability {{\n",
                    context_name
                ));
                code.push_str(&format!(
                    "    thick_client: {},\n",
                    context.get("thick_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    thin_client: {},\n",
                    context.get("thin_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    web_client: {},\n",
                    context.get("web_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    server: {},\n",
                    context.get("server").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    mobile_client: {},\n",
                    context.get("mobile_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    external_connection: {},\n",
                    context.get("external_connection").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str("};\n\n");

                method_context_names.push(Some(context_name));
            } else {
                method_context_names.push(None);
            }
        }
    }

    // Generate global function parameter arrays and context availability constants
    if let Some(global_functions) = data.get("global_functions").and_then(|v| v.as_array()) {
        for function in global_functions {
            // Generate parameters array
            if let Some(params) = function.get("parameters").and_then(|v| v.as_array()) {
                if !params.is_empty() {
                    let param_name = format!("GLOBAL_FUNC_PARAMS_{}", param_counter);
                    param_counter += 1;

                    code.push_str(&format!("const {}: &[RawMethodParam] = &[\n", param_name));
                    for param in params {
                        code.push_str("    RawMethodParam {\n");
                        code.push_str(&format!(
                            "        name: {:?},\n",
                            param.get("name").and_then(|v| v.as_str()).unwrap_or("")
                        ));

                        if let Some(param_type) = param.get("param_type").and_then(|v| v.as_str()) {
                            code.push_str(&format!(
                                "        param_type: Some({:?}),\n",
                                param_type
                            ));
                        } else {
                            code.push_str("        param_type: None,\n");
                        }

                        code.push_str(&format!(
                            "        is_optional: {},\n",
                            param.get("is_optional").and_then(|v| v.as_bool()).unwrap_or(false)
                        ));
                        code.push_str("    },\n");
                    }
                    code.push_str("];\n\n");

                    global_function_param_names.push(Some(param_name));
                } else {
                    global_function_param_names.push(None);
                }
            } else {
                global_function_param_names.push(None);
            }

            // Generate context availability
            if let Some(context) = function.get("context").and_then(|v| v.as_object()) {
                let context_name = format!("GLOBAL_FUNC_CONTEXT_{}", context_counter);
                context_counter += 1;

                code.push_str(&format!(
                    "const {}: RawContextAvailability = RawContextAvailability {{\n",
                    context_name
                ));
                code.push_str(&format!(
                    "    thick_client: {},\n",
                    context.get("thick_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    thin_client: {},\n",
                    context.get("thin_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    web_client: {},\n",
                    context.get("web_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    server: {},\n",
                    context.get("server").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    mobile_client: {},\n",
                    context.get("mobile_client").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str(&format!(
                    "    external_connection: {},\n",
                    context.get("external_connection").and_then(|v| v.as_bool()).unwrap_or(false)
                ));
                code.push_str("};\n\n");

                global_function_context_names.push(Some(context_name));
            } else {
                global_function_context_names.push(None);
            }
        }
    }

    // Generate platform types array
    if let Some(types) = data.get("types").and_then(|v| v.as_array()) {
        code.push_str("pub const PLATFORM_TYPES: &[RawPlatformType] = &[\n");
        for (idx, ty) in types.iter().enumerate() {
            let name = ty.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let english_name = ty.get("english_name").and_then(|v| v.as_str()).unwrap_or("");

            code.push_str("    RawPlatformType {\n");
            code.push_str(&format!("        name: {:?},\n", name));
            code.push_str(&format!("        english_name: {:?},\n", english_name));

            // min_version
            if let Some(min_version) = ty.get("min_version").and_then(|v| v.as_str()) {
                code.push_str(&format!("        min_version: Some({:?}),\n", min_version));
            } else {
                code.push_str("        min_version: None,\n");
            }

            // context
            if let Some(context_name) = &type_context_names[idx] {
                code.push_str(&format!("        context: Some({}),\n", context_name));
            } else {
                code.push_str("        context: None,\n");
            }

            code.push_str("    },\n");
        }
        code.push_str("];\n\n");
    } else {
        code.push_str("pub const PLATFORM_TYPES: &[RawPlatformType] = &[];\n\n");
    }

    // Generate platform methods array
    if let Some(methods) = data.get("methods").and_then(|v| v.as_array()) {
        code.push_str("pub const PLATFORM_METHODS: &[RawPlatformMethod] = &[\n");
        for (idx, method) in methods.iter().enumerate() {
            let id = method.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let type_name = method.get("type_name").and_then(|v| v.as_str()).unwrap_or("");
            let name = method.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let english_name = method.get("english_name").and_then(|v| v.as_str()).unwrap_or("");

            code.push_str("    RawPlatformMethod {\n");
            code.push_str(&format!("        id: {},\n", id));
            code.push_str(&format!("        type_name: {:?},\n", type_name));
            code.push_str(&format!("        name: {:?},\n", name));
            code.push_str(&format!("        english_name: {:?},\n", english_name));

            // return_type
            if let Some(return_type) = method.get("return_type").and_then(|v| v.as_str()) {
                code.push_str(&format!("        return_type: Some({:?}),\n", return_type));
            } else {
                code.push_str("        return_type: None,\n");
            }

            // parameters
            if let Some(param_name) = &method_param_names[idx] {
                code.push_str(&format!("        parameters: {},\n", param_name));
            } else {
                code.push_str("        parameters: &[],\n");
            }

            // min_version
            if let Some(min_version) = method.get("min_version").and_then(|v| v.as_str()) {
                code.push_str(&format!("        min_version: Some({:?}),\n", min_version));
            } else {
                code.push_str("        min_version: None,\n");
            }

            // context
            if let Some(context_name) = &method_context_names[idx] {
                code.push_str(&format!("        context: Some({}),\n", context_name));
            } else {
                code.push_str("        context: None,\n");
            }

            code.push_str("    },\n");
        }
        code.push_str("];\n\n");
    } else {
        code.push_str("pub const PLATFORM_METHODS: &[RawPlatformMethod] = &[];\n\n");
    }

    // Generate platform global functions array
    if let Some(global_functions) = data.get("global_functions").and_then(|v| v.as_array()) {
        code.push_str("pub const PLATFORM_GLOBAL_FUNCTIONS: &[RawGlobalFunction] = &[\n");
        for (idx, function) in global_functions.iter().enumerate() {
            let id = function.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let name = function.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let english_name = function.get("english_name").and_then(|v| v.as_str()).unwrap_or("");

            code.push_str("    RawGlobalFunction {\n");
            code.push_str(&format!("        id: {},\n", id));
            code.push_str(&format!("        name: {:?},\n", name));
            code.push_str(&format!("        english_name: {:?},\n", english_name));

            // return_type
            if let Some(return_type) = function.get("return_type").and_then(|v| v.as_str()) {
                code.push_str(&format!("        return_type: Some({:?}),\n", return_type));
            } else {
                code.push_str("        return_type: None,\n");
            }

            // parameters
            if let Some(param_name) = &global_function_param_names[idx] {
                code.push_str(&format!("        parameters: {},\n", param_name));
            } else {
                code.push_str("        parameters: &[],\n");
            }

            // min_version
            if let Some(min_version) = function.get("min_version").and_then(|v| v.as_str()) {
                code.push_str(&format!("        min_version: Some({:?}),\n", min_version));
            } else {
                code.push_str("        min_version: None,\n");
            }

            // context
            if let Some(context_name) = &global_function_context_names[idx] {
                code.push_str(&format!("        context: Some({}),\n", context_name));
            } else {
                code.push_str("        context: None,\n");
            }

            code.push_str("    },\n");
        }
        code.push_str("];\n");
    } else {
        code.push_str("pub const PLATFORM_GLOBAL_FUNCTIONS: &[RawGlobalFunction] = &[];\n");
    }

    // Generate method documentation arrays
    if let Some(methods) = data.get("methods").and_then(|v| v.as_array()) {
        // First pass: generate nested arrays for params and examples
        let mut param_docs_names = Vec::new();
        let mut examples_names = Vec::new();
        let mut param_docs_counter = 0;
        let mut examples_counter = 0;

        for method in methods {
            if let Some(docs) = method.get("documentation").and_then(|v| v.as_object()) {
                // Generate param docs array
                if let Some(params) = docs.get("param_descriptions").and_then(|v| v.as_array()) {
                    if !params.is_empty() {
                        let param_array_name = format!("METHOD_PARAM_DOCS_{}", param_docs_counter);
                        param_docs_counter += 1;

                        code.push_str(&format!(
                            "const {}: &[RawParamDocs] = &[\n",
                            param_array_name
                        ));
                        for param in params {
                            let name = param.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let description =
                                param.get("description").and_then(|v| v.as_str()).unwrap_or("");
                            code.push_str("    RawParamDocs {\n");
                            code.push_str(&format!("        name: {:?},\n", name));
                            code.push_str(&format!("        description: {:?},\n", description));
                            code.push_str("    },\n");
                        }
                        code.push_str("];\n\n");

                        param_docs_names.push(Some(param_array_name));
                    } else {
                        param_docs_names.push(None);
                    }
                } else {
                    param_docs_names.push(None);
                }

                // Generate examples array
                if let Some(examples) = docs.get("examples").and_then(|v| v.as_array()) {
                    if !examples.is_empty() {
                        let examples_array_name = format!("METHOD_EXAMPLES_{}", examples_counter);
                        examples_counter += 1;

                        code.push_str(&format!(
                            "const {}: &[RawCodeExample] = &[\n",
                            examples_array_name
                        ));
                        for example in examples {
                            let code_text =
                                example.get("code").and_then(|v| v.as_str()).unwrap_or("");
                            let description = example.get("description").and_then(|v| v.as_str());
                            code.push_str("    RawCodeExample {\n");
                            code.push_str(&format!("        code: {:?},\n", code_text));
                            if let Some(desc) = description {
                                code.push_str(&format!("        description: Some({:?}),\n", desc));
                            } else {
                                code.push_str("        description: None,\n");
                            }
                            code.push_str("    },\n");
                        }
                        code.push_str("];\n\n");

                        examples_names.push(Some(examples_array_name));
                    } else {
                        examples_names.push(None);
                    }
                } else {
                    examples_names.push(None);
                }
            } else {
                param_docs_names.push(None);
                examples_names.push(None);
            }
        }

        // Second pass: generate METHOD_DOCS array
        code.push_str("pub const METHOD_DOCS: &[RawMethodDocs] = &[\n");
        let mut docs_idx = 0;
        for method in methods {
            if let Some(docs) = method.get("documentation").and_then(|v| v.as_object()) {
                let method_id = method.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                let syntax = docs.get("syntax").and_then(|v| v.as_str()).unwrap_or("");
                let description = docs.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let notes = docs.get("notes").and_then(|v| v.as_str());

                code.push_str("    RawMethodDocs {\n");
                code.push_str(&format!("        method_id: {},\n", method_id));
                code.push_str(&format!("        syntax: {:?},\n", syntax));
                code.push_str(&format!("        description: {:?},\n", description));

                // params
                if let Some(param_name) = &param_docs_names[docs_idx] {
                    code.push_str(&format!("        params: {},\n", param_name));
                } else {
                    code.push_str("        params: &[],\n");
                }

                // examples
                if let Some(examples_name) = &examples_names[docs_idx] {
                    code.push_str(&format!("        examples: {},\n", examples_name));
                } else {
                    code.push_str("        examples: &[],\n");
                }

                // notes
                if let Some(notes_text) = notes {
                    code.push_str(&format!("        notes: Some({:?}),\n", notes_text));
                } else {
                    code.push_str("        notes: None,\n");
                }

                // see_also
                code.push_str("        see_also: &[],\n");

                code.push_str("    },\n");
                docs_idx += 1;
            }
        }
        code.push_str("];\n\n");
    } else {
        code.push_str("pub const METHOD_DOCS: &[RawMethodDocs] = &[];\n\n");
    }

    // Generate global function documentation arrays
    if let Some(global_functions) = data.get("global_functions").and_then(|v| v.as_array()) {
        // First pass: generate nested arrays for params and examples
        let mut gf_param_docs_names = Vec::new();
        let mut gf_examples_names = Vec::new();
        let mut gf_param_docs_counter = 0;
        let mut gf_examples_counter = 0;

        for function in global_functions {
            if let Some(docs) = function.get("documentation").and_then(|v| v.as_object()) {
                // Generate param docs array
                if let Some(params) = docs.get("param_descriptions").and_then(|v| v.as_array()) {
                    if !params.is_empty() {
                        let param_array_name = format!("GF_PARAMS_{}", gf_param_docs_counter);
                        gf_param_docs_counter += 1;

                        code.push_str(&format!(
                            "const {}: &[RawParamDocs] = &[\n",
                            param_array_name
                        ));
                        for param in params {
                            let name = param.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let description =
                                param.get("description").and_then(|v| v.as_str()).unwrap_or("");
                            code.push_str("    RawParamDocs {\n");
                            code.push_str(&format!("        name: {:?},\n", name));
                            code.push_str(&format!("        description: {:?},\n", description));
                            code.push_str("    },\n");
                        }
                        code.push_str("];\n\n");

                        gf_param_docs_names.push(Some(param_array_name));
                    } else {
                        gf_param_docs_names.push(None);
                    }
                } else {
                    gf_param_docs_names.push(None);
                }

                // Generate examples array
                if let Some(examples) = docs.get("examples").and_then(|v| v.as_array()) {
                    if !examples.is_empty() {
                        let examples_array_name = format!("GF_EXAMPLES_{}", gf_examples_counter);
                        gf_examples_counter += 1;

                        code.push_str(&format!(
                            "const {}: &[RawCodeExample] = &[\n",
                            examples_array_name
                        ));
                        for example in examples {
                            let code_text =
                                example.get("code").and_then(|v| v.as_str()).unwrap_or("");
                            let description = example.get("description").and_then(|v| v.as_str());
                            code.push_str("    RawCodeExample {\n");
                            code.push_str(&format!("        code: {:?},\n", code_text));
                            if let Some(desc) = description {
                                code.push_str(&format!("        description: Some({:?}),\n", desc));
                            } else {
                                code.push_str("        description: None,\n");
                            }
                            code.push_str("    },\n");
                        }
                        code.push_str("];\n\n");

                        gf_examples_names.push(Some(examples_array_name));
                    } else {
                        gf_examples_names.push(None);
                    }
                } else {
                    gf_examples_names.push(None);
                }
            } else {
                gf_param_docs_names.push(None);
                gf_examples_names.push(None);
            }
        }

        // Second pass: generate GLOBAL_FUNCTION_DOCS array
        code.push_str("pub const GLOBAL_FUNCTION_DOCS: &[RawMethodDocs] = &[\n");
        let mut docs_idx = 0;
        for function in global_functions {
            if let Some(docs) = function.get("documentation").and_then(|v| v.as_object()) {
                let function_id = function.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                let syntax = docs.get("syntax").and_then(|v| v.as_str()).unwrap_or("");
                let description = docs.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let notes = docs.get("notes").and_then(|v| v.as_str());

                code.push_str("    RawMethodDocs {\n");
                code.push_str(&format!("        method_id: {},\n", function_id));
                code.push_str(&format!("        syntax: {:?},\n", syntax));
                code.push_str(&format!("        description: {:?},\n", description));

                // params
                if let Some(param_name) = &gf_param_docs_names[docs_idx] {
                    code.push_str(&format!("        params: {},\n", param_name));
                } else {
                    code.push_str("        params: &[],\n");
                }

                // examples
                if let Some(examples_name) = &gf_examples_names[docs_idx] {
                    code.push_str(&format!("        examples: {},\n", examples_name));
                } else {
                    code.push_str("        examples: &[],\n");
                }

                // notes
                if let Some(notes_text) = notes {
                    code.push_str(&format!("        notes: Some({:?}),\n", notes_text));
                } else {
                    code.push_str("        notes: None,\n");
                }

                // see_also
                code.push_str("        see_also: &[],\n");

                code.push_str("    },\n");
                docs_idx += 1;
            }
        }
        code.push_str("];\n\n");
    } else {
        code.push_str("pub const GLOBAL_FUNCTION_DOCS: &[RawMethodDocs] = &[];\n\n");
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
pub const PLATFORM_GLOBAL_FUNCTIONS: &[RawGlobalFunction] = &[];
pub const METHOD_DOCS: &[RawMethodDocs] = &[];
pub const GLOBAL_FUNCTION_DOCS: &[RawMethodDocs] = &[];
"#;

    fs::write(output_path, code).expect("Failed to write generated.rs");
    println!("cargo:warning=Generated empty structures (no platform data)");
}
