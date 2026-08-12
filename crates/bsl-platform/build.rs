use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "src/overlays.rs"]
mod overlays;

fn find_1c_help_file_paths() -> Option<(PathBuf, PathBuf)> {
    if let Ok(path) = env::var("BSL_PLATFORM_PATH") {
        let base_path = PathBuf::from(path);
        let shcntx_path = base_path.join("shcntx_ru.hbk");
        let shlang_path = base_path.join("shlang_ru.hbk");

        if shcntx_path.exists() && shlang_path.exists() {
            return Some((shcntx_path, shlang_path));
        }
    }

    find_1c_help_files()
}

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

    let crate_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let timestamp_path = crate_root.join(".platform_cache_timestamp");

    let committed_data_path = crate_root.join("data/platform_data.json");
    let overlays_path = crate_root.join("data/platform_overlays.json");
    let global_catalog_path = crate_root.join("data/global_catalog.json");
    println!("cargo:rerun-if-changed=data/platform_data.json");
    println!("cargo:rerun-if-changed=data/platform_overlays.json");
    println!("cargo:rerun-if-changed=data/global_catalog.json");

    if committed_data_path.exists() {
        generate_code_from_json(
            &committed_data_path,
            &overlays_path,
            &global_catalog_path,
            &generated_path,
            true,
        );
        println!("cargo:rustc-cfg=feature=\"platform_docs\"");
        return;
    }

    let platform_found = find_1c_help_file_paths().is_some();

    if let Some(hbk_files) = find_1c_help_file_paths() {
        let (shcntx_path, shlang_path) = hbk_files;

        println!("cargo:rerun-if-changed={}", shcntx_path.display());
        println!("cargo:rerun-if-changed={}", shlang_path.display());

        if generated_path.exists() && timestamp_path.exists() {
            if let Ok(stored_timestamp) = fs::read_to_string(&timestamp_path) {
                let current_timestamp = get_files_timestamp(&shcntx_path, &shlang_path);
                if stored_timestamp == current_timestamp {
                    println!("cargo:rustc-cfg=feature=\"platform_docs\"");
                    return;
                }
            }
        }

        if let Some(json_path) = extract_both_help_files(&shcntx_path, &shlang_path) {
            println!("cargo:rustc-cfg=feature=\"platform_docs\"");
            generate_code_from_json(
                &json_path,
                &overlays_path,
                &global_catalog_path,
                &generated_path,
                true,
            );

            let timestamp = get_files_timestamp(&shcntx_path, &shlang_path);
            let _ = fs::write(&timestamp_path, timestamp);
            return;
        }
    }

    if !platform_found {
        println!("cargo:warning=1C platform not found and data/platform_data.json missing");
        println!("cargo:warning=See docs/contributing/DEVELOPMENT_RULES.md for instructions");
    }

    generate_empty_structures(&global_catalog_path, &generated_path);
}

fn find_1c_help_files() -> Option<(PathBuf, PathBuf)> {
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

    #[cfg(target_os = "macos")]
    {
        if let Ok(entries) = fs::read_dir("/opt/1cv8") {
            for entry in entries.flatten() {
                let path = entry.path();

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

fn extract_hbk_file(hbk_path: &Path, name: &str) -> Option<PathBuf> {
    let temp_dir = env::temp_dir().join(format!("bsl-platform-{}", name));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).ok()?;

    let output_arg = format!("-o{}", temp_dir.display());
    let _output = Command::new("7z").args(["x", "-y", &output_arg]).arg(hbk_path).output().ok()?;

    if let Ok(entries) = fs::read_dir(&temp_dir) {
        let file_count = entries.count();
        if file_count > 0 {
            return Some(temp_dir);
        }
    }

    println!("cargo:warning=Failed to extract files from {}", name);
    None
}

fn extract_both_help_files(shcntx_path: &Path, shlang_path: &Path) -> Option<PathBuf> {
    let shlang_dir = extract_hbk_file(shlang_path, "shlang")?;

    let shcntx_dir = extract_hbk_file(shcntx_path, "shcntx")?;

    let parser_binary = PathBuf::from("tools/html-parser/target/release/html-parser");
    if !parser_binary.exists() {
        println!("cargo:warning=HTML parser not built, skipping documentation extraction");
        println!(
            "cargo:warning=Run: cargo build --release --manifest-path tools/html-parser/Cargo.toml"
        );
        let _ = fs::remove_dir_all(&shlang_dir);
        let _ = fs::remove_dir_all(&shcntx_dir);
        return None;
    }

    let json_output = PathBuf::from(env::var("OUT_DIR").unwrap()).join("platform_data.json");
    let output = Command::new(&parser_binary)
        .arg(&shlang_dir)
        .arg(&shcntx_dir)
        .arg(&json_output)
        .output()
        .ok()?;

    let _ = fs::remove_dir_all(&shlang_dir);
    let _ = fs::remove_dir_all(&shcntx_dir);

    if !output.status.success() {
        println!("cargo:warning=Failed to parse platform HTML files");
        if !output.stderr.is_empty() {
            println!("cargo:warning=Parser error: {}", String::from_utf8_lossy(&output.stderr));
        }
        return None;
    }

    Some(json_output)
}

fn generate_code_from_json(
    json_path: &Path,
    overlays_path: &Path,
    global_catalog_path: &Path,
    output_path: &Path,
    _with_docs: bool,
) {
    let json_content = fs::read_to_string(json_path).expect("Failed to read JSON file");

    let mut data: serde_json::Value =
        serde_json::from_str(&json_content).expect("Failed to parse JSON");
    let overlay_content =
        fs::read_to_string(overlays_path).expect("Failed to read platform overlays");
    overlays::apply_method_parameter_overlays(&mut data, &overlay_content)
        .unwrap_or_else(|error| panic!("Invalid platform overlay: {error}"));
    overlays::apply_type_property_additions(&mut data, &overlay_content)
        .unwrap_or_else(|error| panic!("Invalid platform overlay: {error}"));

    let mut code = String::new();
    code.push_str("// Auto-generated by build.rs\n");
    code.push_str("// DO NOT EDIT MANUALLY\n\n");
    code.push_str("use super::types::*;\n\n");

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

    let mut param_counter = 0;
    let mut method_param_names = Vec::new();
    let mut method_context_names = Vec::new();
    let mut global_function_param_names: Vec<Option<String>> = Vec::new();
    let mut global_function_context_names: Vec<Option<String>> = Vec::new();

    let mut method_variants_names: Vec<Option<String>> = Vec::new();
    if let Some(methods) = data.get("methods").and_then(|v| v.as_array()) {
        for method in methods {
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
                        code.push_str(&format!(
                            "        is_variadic: {},\n",
                            param.get("is_variadic").and_then(|v| v.as_bool()).unwrap_or(false)
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

            if let Some(variants) =
                method.get("variants").and_then(|v| v.as_array()).filter(|v| !v.is_empty())
            {
                let variants_array_name =
                    format!("METHOD_VARIANTS_{}", method_variants_names.len());

                let mut per_variant_param_names: Vec<Option<String>> = Vec::new();
                for variant in variants {
                    if let Some(vparams) = variant
                        .get("parameters")
                        .and_then(|v| v.as_array())
                        .filter(|v| !v.is_empty())
                    {
                        let vparam_name = format!("METHOD_VARIANT_PARAMS_{}", param_counter);
                        param_counter += 1;
                        code.push_str(&format!("const {}: &[RawMethodParam] = &[\n", vparam_name));
                        for param in vparams {
                            code.push_str("    RawMethodParam {\n");
                            code.push_str(&format!(
                                "        name: {:?},\n",
                                param.get("name").and_then(|v| v.as_str()).unwrap_or("")
                            ));
                            if let Some(param_type) =
                                param.get("param_type").and_then(|v| v.as_str())
                            {
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
                            code.push_str(&format!(
                                "        is_variadic: {},\n",
                                param.get("is_variadic").and_then(|v| v.as_bool()).unwrap_or(false)
                            ));
                            code.push_str("    },\n");
                        }
                        code.push_str("];\n\n");
                        per_variant_param_names.push(Some(vparam_name));
                    } else {
                        per_variant_param_names.push(None);
                    }
                }

                code.push_str(&format!(
                    "const {}: &[RawMethodVariant] = &[\n",
                    variants_array_name
                ));
                for (variant, params_const) in variants.iter().zip(per_variant_param_names.iter()) {
                    code.push_str("    RawMethodVariant {\n");
                    if let Some(name) = variant.get("variant_name").and_then(|v| v.as_str()) {
                        code.push_str(&format!("        variant_name: Some({:?}),\n", name));
                    } else {
                        code.push_str("        variant_name: None,\n");
                    }
                    if let Some(name) = params_const {
                        code.push_str(&format!("        parameters: {},\n", name));
                    } else {
                        code.push_str("        parameters: &[],\n");
                    }
                    code.push_str("    },\n");
                }
                code.push_str("];\n\n");

                method_variants_names.push(Some(variants_array_name));
            } else {
                method_variants_names.push(None);
            }

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

    let mut global_function_variants_names: Vec<Option<String>> = Vec::new();
    if let Some(global_functions) = data.get("global_functions").and_then(|v| v.as_array()) {
        for function in global_functions {
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
                        code.push_str(&format!(
                            "        is_variadic: {},\n",
                            param.get("is_variadic").and_then(|v| v.as_bool()).unwrap_or(false)
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

            if let Some(variants) =
                function.get("variants").and_then(|v| v.as_array()).filter(|v| !v.is_empty())
            {
                let variants_array_name =
                    format!("GLOBAL_FUNC_VARIANTS_{}", global_function_variants_names.len());

                let mut per_variant_param_names: Vec<Option<String>> = Vec::new();
                for variant in variants {
                    if let Some(vparams) = variant
                        .get("parameters")
                        .and_then(|v| v.as_array())
                        .filter(|v| !v.is_empty())
                    {
                        let vparam_name = format!("GLOBAL_FUNC_VARIANT_PARAMS_{}", param_counter);
                        param_counter += 1;
                        code.push_str(&format!("const {}: &[RawMethodParam] = &[\n", vparam_name));
                        for param in vparams {
                            code.push_str("    RawMethodParam {\n");
                            code.push_str(&format!(
                                "        name: {:?},\n",
                                param.get("name").and_then(|v| v.as_str()).unwrap_or("")
                            ));
                            if let Some(param_type) =
                                param.get("param_type").and_then(|v| v.as_str())
                            {
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
                            code.push_str(&format!(
                                "        is_variadic: {},\n",
                                param.get("is_variadic").and_then(|v| v.as_bool()).unwrap_or(false)
                            ));
                            code.push_str("    },\n");
                        }
                        code.push_str("];\n\n");
                        per_variant_param_names.push(Some(vparam_name));
                    } else {
                        per_variant_param_names.push(None);
                    }
                }

                code.push_str(&format!(
                    "const {}: &[RawGlobalFunctionVariant] = &[\n",
                    variants_array_name
                ));
                for (variant, params_const) in variants.iter().zip(per_variant_param_names.iter()) {
                    code.push_str("    RawGlobalFunctionVariant {\n");
                    if let Some(name) = variant.get("variant_name").and_then(|v| v.as_str()) {
                        code.push_str(&format!("        variant_name: Some({:?}),\n", name));
                    } else {
                        code.push_str("        variant_name: None,\n");
                    }
                    if let Some(name) = params_const {
                        code.push_str(&format!("        parameters: {},\n", name));
                    } else {
                        code.push_str("        parameters: &[],\n");
                    }
                    code.push_str("    },\n");
                }
                code.push_str("];\n\n");

                global_function_variants_names.push(Some(variants_array_name));
            } else {
                global_function_variants_names.push(None);
            }

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

    if let Some(types) = data.get("types").and_then(|v| v.as_array()) {
        let mut type_iter_elem_names: Vec<Option<String>> = Vec::with_capacity(types.len());
        let mut type_iter_elem_counter = 0usize;

        for ty in types.iter() {
            if let Some(elems) = ty.get("iter_element_types").and_then(|v| v.as_array()) {
                if !elems.is_empty() {
                    let arr = format!("TYPE_ITER_ELEMENTS_{}", type_iter_elem_counter);
                    type_iter_elem_counter += 1;
                    code.push_str(&format!("const {}: &[&str] = &[", arr));
                    for (i, e) in elems.iter().enumerate() {
                        if i > 0 {
                            code.push_str(", ");
                        }
                        code.push_str(&format!("{:?}", e.as_str().unwrap_or("")));
                    }
                    code.push_str("];\n\n");
                    type_iter_elem_names.push(Some(arr));
                } else {
                    type_iter_elem_names.push(None);
                }
            } else {
                type_iter_elem_names.push(None);
            }
        }

        code.push_str("pub const PLATFORM_TYPES: &[RawPlatformType] = &[\n");
        for (idx, ty) in types.iter().enumerate() {
            let name = ty.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let english_name = ty.get("english_name").and_then(|v| v.as_str()).unwrap_or("");

            code.push_str("    RawPlatformType {\n");
            code.push_str(&format!("        name: {:?},\n", name));
            code.push_str(&format!("        english_name: {:?},\n", english_name));

            if let Some(min_version) = ty.get("min_version").and_then(|v| v.as_str()) {
                code.push_str(&format!("        min_version: Some({:?}),\n", min_version));
            } else {
                code.push_str("        min_version: None,\n");
            }

            if let Some(context_name) = &type_context_names[idx] {
                code.push_str(&format!("        context: Some({}),\n", context_name));
            } else {
                code.push_str("        context: None,\n");
            }

            if let Some(arr) = &type_iter_elem_names[idx] {
                code.push_str(&format!("        iter_element_types: {},\n", arr));
            } else {
                code.push_str("        iter_element_types: &[],\n");
            }

            if let Some(xdto) = ty.get("xdto_name").and_then(|v| v.as_str()) {
                code.push_str(&format!("        xdto_name: Some({:?}),\n", xdto));
            } else {
                code.push_str("        xdto_name: None,\n");
            }

            code.push_str("    },\n");
        }
        code.push_str("];\n\n");
    } else {
        code.push_str("pub const PLATFORM_TYPES: &[RawPlatformType] = &[];\n\n");
    }

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

            if let Some(return_type) = method.get("return_type").and_then(|v| v.as_str()) {
                code.push_str(&format!("        return_type: Some({:?}),\n", return_type));
            } else {
                code.push_str("        return_type: None,\n");
            }

            if let Some(param_name) = &method_param_names[idx] {
                code.push_str(&format!("        parameters: {},\n", param_name));
            } else {
                code.push_str("        parameters: &[],\n");
            }

            if let Some(variants_name) = &method_variants_names[idx] {
                code.push_str(&format!("        variants: {},\n", variants_name));
            } else {
                code.push_str("        variants: &[],\n");
            }

            if let Some(min_version) = method.get("min_version").and_then(|v| v.as_str()) {
                code.push_str(&format!("        min_version: Some({:?}),\n", min_version));
            } else {
                code.push_str("        min_version: None,\n");
            }

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

            if let Some(return_type) = function.get("return_type").and_then(|v| v.as_str()) {
                code.push_str(&format!("        return_type: Some({:?}),\n", return_type));
            } else {
                code.push_str("        return_type: None,\n");
            }

            if let Some(param_name) = &global_function_param_names[idx] {
                code.push_str(&format!("        parameters: {},\n", param_name));
            } else {
                code.push_str("        parameters: &[],\n");
            }

            if let Some(variants_name) = &global_function_variants_names[idx] {
                code.push_str(&format!("        variants: {},\n", variants_name));
            } else {
                code.push_str("        variants: &[],\n");
            }

            if let Some(min_version) = function.get("min_version").and_then(|v| v.as_str()) {
                code.push_str(&format!("        min_version: Some({:?}),\n", min_version));
            } else {
                code.push_str("        min_version: None,\n");
            }

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

    if let Some(methods) = data.get("methods").and_then(|v| v.as_array()) {
        let mut param_docs_names = Vec::new();
        let mut examples_names = Vec::new();
        let mut param_docs_counter = 0;
        let mut examples_counter = 0;

        for method in methods {
            if let Some(docs) = method.get("documentation").and_then(|v| v.as_object()) {
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
                            let default_value = param.get("default_value").and_then(|v| v.as_str());
                            code.push_str("    RawParamDocs {\n");
                            code.push_str(&format!("        name: {:?},\n", name));
                            code.push_str(&format!("        description: {:?},\n", description));
                            if let Some(dv) = default_value {
                                code.push_str(&format!("        default_value: Some({:?}),\n", dv));
                            } else {
                                code.push_str("        default_value: None,\n");
                            }
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

                if let Some(param_name) = &param_docs_names[docs_idx] {
                    code.push_str(&format!("        params: {},\n", param_name));
                } else {
                    code.push_str("        params: &[],\n");
                }

                if let Some(examples_name) = &examples_names[docs_idx] {
                    code.push_str(&format!("        examples: {},\n", examples_name));
                } else {
                    code.push_str("        examples: &[],\n");
                }

                if let Some(notes_text) = notes {
                    code.push_str(&format!("        notes: Some({:?}),\n", notes_text));
                } else {
                    code.push_str("        notes: None,\n");
                }

                if let Some(see_also) = docs.get("see_also").and_then(|v| v.as_array()) {
                    if see_also.is_empty() {
                        code.push_str("        see_also: &[],\n");
                    } else {
                        code.push_str("        see_also: &[");
                        for (i, item) in see_also.iter().enumerate() {
                            if i > 0 {
                                code.push_str(", ");
                            }
                            let s = item.as_str().unwrap_or("");
                            code.push_str(&format!("{:?}", s));
                        }
                        code.push_str("],\n");
                    }
                } else {
                    code.push_str("        see_also: &[],\n");
                }

                code.push_str("    },\n");
                docs_idx += 1;
            }
        }
        code.push_str("];\n\n");
    } else {
        code.push_str("pub const METHOD_DOCS: &[RawMethodDocs] = &[];\n\n");
    }

    if let Some(global_functions) = data.get("global_functions").and_then(|v| v.as_array()) {
        let mut gf_param_docs_names = Vec::new();
        let mut gf_examples_names = Vec::new();
        let mut gf_param_docs_counter = 0;
        let mut gf_examples_counter = 0;

        for function in global_functions {
            if let Some(docs) = function.get("documentation").and_then(|v| v.as_object()) {
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
                            let default_value = param.get("default_value").and_then(|v| v.as_str());
                            code.push_str("    RawParamDocs {\n");
                            code.push_str(&format!("        name: {:?},\n", name));
                            code.push_str(&format!("        description: {:?},\n", description));
                            if let Some(dv) = default_value {
                                code.push_str(&format!("        default_value: Some({:?}),\n", dv));
                            } else {
                                code.push_str("        default_value: None,\n");
                            }
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

                if let Some(param_name) = &gf_param_docs_names[docs_idx] {
                    code.push_str(&format!("        params: {},\n", param_name));
                } else {
                    code.push_str("        params: &[],\n");
                }

                if let Some(examples_name) = &gf_examples_names[docs_idx] {
                    code.push_str(&format!("        examples: {},\n", examples_name));
                } else {
                    code.push_str("        examples: &[],\n");
                }

                if let Some(notes_text) = notes {
                    code.push_str(&format!("        notes: Some({:?}),\n", notes_text));
                } else {
                    code.push_str("        notes: None,\n");
                }

                if let Some(see_also) = docs.get("see_also").and_then(|v| v.as_array()) {
                    if see_also.is_empty() {
                        code.push_str("        see_also: &[],\n");
                    } else {
                        code.push_str("        see_also: &[");
                        for (i, item) in see_also.iter().enumerate() {
                            if i > 0 {
                                code.push_str(", ");
                            }
                            let s = item.as_str().unwrap_or("");
                            code.push_str(&format!("{:?}", s));
                        }
                        code.push_str("],\n");
                    }
                } else {
                    code.push_str("        see_also: &[],\n");
                }

                code.push_str("    },\n");
                docs_idx += 1;
            }
        }
        code.push_str("];\n\n");
    } else {
        code.push_str("pub const GLOBAL_FUNCTION_DOCS: &[RawMethodDocs] = &[];\n\n");
    }

    if let Some(constructors) = data.get("constructors").and_then(|v| v.as_array()) {
        let mut ctor_param_names: Vec<Option<String>> = Vec::with_capacity(constructors.len());
        let mut ctor_context_names: Vec<Option<String>> = Vec::with_capacity(constructors.len());
        let mut ctor_param_docs_names: Vec<Option<String>> = Vec::with_capacity(constructors.len());
        let mut ctor_examples_names: Vec<Option<String>> = Vec::with_capacity(constructors.len());

        let mut ctor_param_counter = 0usize;
        let mut ctor_context_counter = 0usize;
        let mut ctor_param_docs_counter = 0usize;
        let mut ctor_examples_counter = 0usize;

        for ctor in constructors {
            if let Some(params) = ctor.get("parameters").and_then(|v| v.as_array()) {
                if !params.is_empty() {
                    let arr = format!("CTOR_PARAMS_{}", ctor_param_counter);
                    ctor_param_counter += 1;
                    code.push_str(&format!("const {}: &[RawMethodParam] = &[\n", arr));
                    for param in params {
                        code.push_str("    RawMethodParam {\n");
                        code.push_str(&format!(
                            "        name: {:?},\n",
                            param.get("name").and_then(|v| v.as_str()).unwrap_or("")
                        ));
                        if let Some(t) = param.get("param_type").and_then(|v| v.as_str()) {
                            code.push_str(&format!("        param_type: Some({:?}),\n", t));
                        } else {
                            code.push_str("        param_type: None,\n");
                        }
                        code.push_str(&format!(
                            "        is_optional: {},\n",
                            param.get("is_optional").and_then(|v| v.as_bool()).unwrap_or(false)
                        ));
                        code.push_str(&format!(
                            "        is_variadic: {},\n",
                            param.get("is_variadic").and_then(|v| v.as_bool()).unwrap_or(false)
                        ));
                        code.push_str("    },\n");
                    }
                    code.push_str("];\n\n");
                    ctor_param_names.push(Some(arr));
                } else {
                    ctor_param_names.push(None);
                }
            } else {
                ctor_param_names.push(None);
            }

            if let Some(ctx) = ctor.get("context").and_then(|v| v.as_object()) {
                let name = format!("CTOR_CONTEXT_{}", ctor_context_counter);
                ctor_context_counter += 1;
                code.push_str(&format!(
                    "const {}: RawContextAvailability = RawContextAvailability {{\n",
                    name
                ));
                for field in &[
                    "thick_client",
                    "thin_client",
                    "web_client",
                    "server",
                    "mobile_client",
                    "external_connection",
                ] {
                    code.push_str(&format!(
                        "    {}: {},\n",
                        field,
                        ctx.get(*field).and_then(|v| v.as_bool()).unwrap_or(false)
                    ));
                }
                code.push_str("};\n\n");
                ctor_context_names.push(Some(name));
            } else {
                ctor_context_names.push(None);
            }

            if let Some(docs) = ctor.get("documentation").and_then(|v| v.as_object()) {
                if let Some(params) = docs.get("param_descriptions").and_then(|v| v.as_array()) {
                    if !params.is_empty() {
                        let arr = format!("CTOR_PARAM_DOCS_{}", ctor_param_docs_counter);
                        ctor_param_docs_counter += 1;
                        code.push_str(&format!("const {}: &[RawParamDocs] = &[\n", arr));
                        for p in params {
                            let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let description =
                                p.get("description").and_then(|v| v.as_str()).unwrap_or("");
                            let default_value = p.get("default_value").and_then(|v| v.as_str());
                            code.push_str("    RawParamDocs {\n");
                            code.push_str(&format!("        name: {:?},\n", name));
                            code.push_str(&format!("        description: {:?},\n", description));
                            if let Some(dv) = default_value {
                                code.push_str(&format!("        default_value: Some({:?}),\n", dv));
                            } else {
                                code.push_str("        default_value: None,\n");
                            }
                            code.push_str("    },\n");
                        }
                        code.push_str("];\n\n");
                        ctor_param_docs_names.push(Some(arr));
                    } else {
                        ctor_param_docs_names.push(None);
                    }
                } else {
                    ctor_param_docs_names.push(None);
                }

                if let Some(examples) = docs.get("examples").and_then(|v| v.as_array()) {
                    if !examples.is_empty() {
                        let arr = format!("CTOR_EXAMPLES_{}", ctor_examples_counter);
                        ctor_examples_counter += 1;
                        code.push_str(&format!("const {}: &[RawCodeExample] = &[\n", arr));
                        for e in examples {
                            let code_text = e.get("code").and_then(|v| v.as_str()).unwrap_or("");
                            let description = e.get("description").and_then(|v| v.as_str());
                            code.push_str("    RawCodeExample {\n");
                            code.push_str(&format!("        code: {:?},\n", code_text));
                            if let Some(d) = description {
                                code.push_str(&format!("        description: Some({:?}),\n", d));
                            } else {
                                code.push_str("        description: None,\n");
                            }
                            code.push_str("    },\n");
                        }
                        code.push_str("];\n\n");
                        ctor_examples_names.push(Some(arr));
                    } else {
                        ctor_examples_names.push(None);
                    }
                } else {
                    ctor_examples_names.push(None);
                }
            } else {
                ctor_param_docs_names.push(None);
                ctor_examples_names.push(None);
            }
        }

        code.push_str("pub const PLATFORM_CONSTRUCTORS: &[RawPlatformConstructor] = &[\n");
        for (idx, ctor) in constructors.iter().enumerate() {
            let id = ctor.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let type_name = ctor.get("type_name").and_then(|v| v.as_str()).unwrap_or("");
            let variant_name = ctor.get("variant_name").and_then(|v| v.as_str());

            code.push_str("    RawPlatformConstructor {\n");
            code.push_str(&format!("        id: {},\n", id));
            code.push_str(&format!("        type_name: {:?},\n", type_name));
            if let Some(v) = variant_name {
                code.push_str(&format!("        variant_name: Some({:?}),\n", v));
            } else {
                code.push_str("        variant_name: None,\n");
            }
            if let Some(arr) = &ctor_param_names[idx] {
                code.push_str(&format!("        parameters: {},\n", arr));
            } else {
                code.push_str("        parameters: &[],\n");
            }
            if let Some(v) = ctor.get("min_version").and_then(|v| v.as_str()) {
                code.push_str(&format!("        min_version: Some({:?}),\n", v));
            } else {
                code.push_str("        min_version: None,\n");
            }
            if let Some(name) = &ctor_context_names[idx] {
                code.push_str(&format!("        context: Some({}),\n", name));
            } else {
                code.push_str("        context: None,\n");
            }
            code.push_str("    },\n");
        }
        code.push_str("];\n\n");

        code.push_str("pub const CONSTRUCTOR_DOCS: &[RawConstructorDocs] = &[\n");
        for (idx, ctor) in constructors.iter().enumerate() {
            let Some(docs) = ctor.get("documentation").and_then(|v| v.as_object()) else {
                continue;
            };
            let constructor_id = ctor.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let syntax = docs.get("syntax").and_then(|v| v.as_str()).unwrap_or("");
            let description = docs.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let notes = docs.get("notes").and_then(|v| v.as_str());

            code.push_str("    RawConstructorDocs {\n");
            code.push_str(&format!("        constructor_id: {},\n", constructor_id));
            code.push_str(&format!("        syntax: {:?},\n", syntax));
            code.push_str(&format!("        description: {:?},\n", description));
            if let Some(arr) = &ctor_param_docs_names[idx] {
                code.push_str(&format!("        params: {},\n", arr));
            } else {
                code.push_str("        params: &[],\n");
            }
            if let Some(arr) = &ctor_examples_names[idx] {
                code.push_str(&format!("        examples: {},\n", arr));
            } else {
                code.push_str("        examples: &[],\n");
            }
            if let Some(n) = notes {
                code.push_str(&format!("        notes: Some({:?}),\n", n));
            } else {
                code.push_str("        notes: None,\n");
            }
            if let Some(see_also) = docs.get("see_also").and_then(|v| v.as_array()) {
                if see_also.is_empty() {
                    code.push_str("        see_also: &[],\n");
                } else {
                    code.push_str("        see_also: &[");
                    for (i, item) in see_also.iter().enumerate() {
                        if i > 0 {
                            code.push_str(", ");
                        }
                        code.push_str(&format!("{:?}", item.as_str().unwrap_or("")));
                    }
                    code.push_str("],\n");
                }
            } else {
                code.push_str("        see_also: &[],\n");
            }
            code.push_str("    },\n");
        }
        code.push_str("];\n\n");
    } else {
        code.push_str("pub const PLATFORM_CONSTRUCTORS: &[RawPlatformConstructor] = &[];\n");
        code.push_str("pub const CONSTRUCTOR_DOCS: &[RawConstructorDocs] = &[];\n\n");
    }

    if let Some(properties) = data.get("properties").and_then(|v| v.as_array()) {
        let mut prop_types_names: Vec<Option<String>> = Vec::with_capacity(properties.len());
        let mut prop_context_names: Vec<Option<String>> = Vec::with_capacity(properties.len());

        let mut prop_types_counter = 0usize;
        let mut prop_context_counter = 0usize;

        for prop in properties {
            if let Some(types) = prop.get("property_types").and_then(|v| v.as_array()) {
                if !types.is_empty() {
                    let arr = format!("PROP_TYPES_{}", prop_types_counter);
                    prop_types_counter += 1;
                    code.push_str(&format!("const {}: &[&str] = &[", arr));
                    for (i, t) in types.iter().enumerate() {
                        if i > 0 {
                            code.push_str(", ");
                        }
                        code.push_str(&format!("{:?}", t.as_str().unwrap_or("")));
                    }
                    code.push_str("];\n\n");
                    prop_types_names.push(Some(arr));
                } else {
                    prop_types_names.push(None);
                }
            } else {
                prop_types_names.push(None);
            }

            if let Some(ctx) = prop.get("context").and_then(|v| v.as_object()) {
                let name = format!("PROP_CONTEXT_{}", prop_context_counter);
                prop_context_counter += 1;
                code.push_str(&format!(
                    "const {}: RawContextAvailability = RawContextAvailability {{\n",
                    name
                ));
                for field in &[
                    "thick_client",
                    "thin_client",
                    "web_client",
                    "server",
                    "mobile_client",
                    "external_connection",
                ] {
                    code.push_str(&format!(
                        "    {}: {},\n",
                        field,
                        ctx.get(*field).and_then(|v| v.as_bool()).unwrap_or(false)
                    ));
                }
                code.push_str("};\n\n");
                prop_context_names.push(Some(name));
            } else {
                prop_context_names.push(None);
            }
        }

        code.push_str("pub const PLATFORM_PROPERTIES: &[RawPlatformProperty] = &[\n");
        for (idx, prop) in properties.iter().enumerate() {
            let id = prop.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let type_name = prop.get("type_name").and_then(|v| v.as_str()).unwrap_or("");
            let name = prop.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let english_name = prop.get("english_name").and_then(|v| v.as_str()).unwrap_or("");
            let is_readonly = prop.get("is_readonly").and_then(|v| v.as_bool()).unwrap_or(false);

            code.push_str("    RawPlatformProperty {\n");
            code.push_str(&format!("        id: {},\n", id));
            code.push_str(&format!("        type_name: {:?},\n", type_name));
            code.push_str(&format!("        name: {:?},\n", name));
            code.push_str(&format!("        english_name: {:?},\n", english_name));
            if let Some(arr) = &prop_types_names[idx] {
                code.push_str(&format!("        property_types: {},\n", arr));
            } else {
                code.push_str("        property_types: &[],\n");
            }
            code.push_str(&format!("        is_readonly: {},\n", is_readonly));
            if let Some(v) = prop.get("min_version").and_then(|v| v.as_str()) {
                code.push_str(&format!("        min_version: Some({:?}),\n", v));
            } else {
                code.push_str("        min_version: None,\n");
            }
            if let Some(n) = &prop_context_names[idx] {
                code.push_str(&format!("        context: Some({}),\n", n));
            } else {
                code.push_str("        context: None,\n");
            }
            code.push_str("    },\n");
        }
        code.push_str("];\n\n");

        code.push_str("pub const PROPERTY_DOCS: &[RawPropertyDocs] = &[\n");
        for prop in properties.iter() {
            let Some(docs) = prop.get("documentation").and_then(|v| v.as_object()) else {
                continue;
            };
            let property_id = prop.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let description = docs.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let notes = docs.get("notes").and_then(|v| v.as_str());

            code.push_str("    RawPropertyDocs {\n");
            code.push_str(&format!("        property_id: {},\n", property_id));
            code.push_str(&format!("        description: {:?},\n", description));
            if let Some(n) = notes {
                code.push_str(&format!("        notes: Some({:?}),\n", n));
            } else {
                code.push_str("        notes: None,\n");
            }
            if let Some(see_also) = docs.get("see_also").and_then(|v| v.as_array()) {
                if see_also.is_empty() {
                    code.push_str("        see_also: &[],\n");
                } else {
                    code.push_str("        see_also: &[");
                    for (i, item) in see_also.iter().enumerate() {
                        if i > 0 {
                            code.push_str(", ");
                        }
                        code.push_str(&format!("{:?}", item.as_str().unwrap_or("")));
                    }
                    code.push_str("],\n");
                }
            } else {
                code.push_str("        see_also: &[],\n");
            }
            code.push_str("    },\n");
        }
        code.push_str("];\n\n");
    } else {
        code.push_str("pub const PLATFORM_PROPERTIES: &[RawPlatformProperty] = &[];\n");
        code.push_str("pub const PROPERTY_DOCS: &[RawPropertyDocs] = &[];\n\n");
    }

    append_global_catalog(&mut code, global_catalog_path);
    fs::write(output_path, code).expect("Failed to write generated.rs");
}

fn append_global_catalog(code: &mut String, manifest_path: &Path) {
    if !manifest_path.exists() {
        code.push_str(
            "pub const PLATFORM_GLOBAL_CATALOG_METADATA: Option<RawPlatformGlobalCatalogMetadata> = None;\n",
        );
        code.push_str("pub const PLATFORM_GLOBAL_CATALOG: &[RawPlatformGlobalSymbol] = &[];\n");
        return;
    }

    let source = fs::read_to_string(manifest_path).expect("Failed to read global catalog manifest");
    let manifest: serde_json::Value =
        serde_json::from_str(&source).expect("Failed to parse global catalog manifest");
    assert_eq!(
        manifest.get("schema_version").and_then(|value| value.as_u64()),
        Some(1),
        "Unsupported global catalog schema"
    );
    let source = manifest
        .get("source")
        .and_then(|value| value.as_object())
        .expect("Global catalog source metadata is missing");
    let required = |name: &str| {
        source
            .get(name)
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| panic!("Global catalog source field {name:?} is missing"))
    };
    let platform_version = required("platform_version");
    let edt_version = required("edt_version");
    let environment_mask_bits = source
        .get("environment_mask_bits")
        .and_then(|value| value.as_object())
        .expect("Global catalog environment_mask_bits metadata is missing");
    let expected_environment_mask_bits = [
        ("thick_client", 1_u64),
        ("thin_client", 2),
        ("web_client", 4),
        ("server", 8),
        ("mobile_client", 16),
        ("external_connection", 32),
    ];
    assert_eq!(
        environment_mask_bits.len(),
        expected_environment_mask_bits.len(),
        "Global catalog environment_mask_bits contains an unknown or missing environment"
    );
    for (name, expected) in expected_environment_mask_bits {
        assert_eq!(
            environment_mask_bits.get(name).and_then(|value| value.as_u64()),
            Some(expected),
            "Global catalog environment bit for {name:?} disagrees with context_from_mask"
        );
    }
    let resources = source
        .get("resources")
        .and_then(|value| value.as_object())
        .expect("Global catalog resource provenance is missing");
    let resource_hash = |name: &str| {
        let hash = resources
            .get(name)
            .and_then(|value| value.get("sha256"))
            .and_then(|value| value.as_str())
            .unwrap_or_else(|| panic!("Global catalog resource hash {name:?} is missing"));
        assert!(
            hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Global catalog resource hash {name:?} is invalid"
        );
        hash
    };
    let global_context_sha256 = resource_hash("global_context");
    let system_enums_sha256 = resource_hash("system_enums");
    let completeness = manifest
        .get("completeness")
        .and_then(|value| value.as_object())
        .expect("Global catalog completeness metadata is missing");
    let complete_global_context =
        completeness.get("global_context").and_then(|value| value.as_bool()).unwrap_or(false);
    let complete_system_enums =
        completeness.get("system_enums").and_then(|value| value.as_bool()).unwrap_or(false);

    code.push_str("\npub const PLATFORM_GLOBAL_CATALOG_METADATA: Option<RawPlatformGlobalCatalogMetadata> = Some(RawPlatformGlobalCatalogMetadata {\n");
    code.push_str("    schema_version: 1,\n");
    code.push_str(&format!("    platform_version: {:?},\n", platform_version));
    code.push_str(&format!("    edt_version: {:?},\n", edt_version));
    code.push_str(&format!("    global_context_sha256: {:?},\n", global_context_sha256));
    code.push_str(&format!("    system_enums_sha256: {:?},\n", system_enums_sha256));
    code.push_str(&format!("    complete_global_context: {},\n", complete_global_context));
    code.push_str(&format!("    complete_system_enums: {},\n", complete_system_enums));
    code.push_str("});\n\n");

    let symbols = manifest
        .get("symbols")
        .and_then(|value| value.as_array())
        .expect("Global catalog symbols are missing");
    assert!(!symbols.is_empty(), "Attested global catalog must not be empty");
    if complete_global_context || complete_system_enums {
        let expected = manifest
            .get("attestation")
            .and_then(|value| value.get("expected_symbol_counts"))
            .and_then(|value| value.as_object())
            .expect("A complete global catalog requires independently recorded symbol counts");
        for kind in ["function", "property", "system_enum"] {
            let expected_count = expected
                .get(kind)
                .and_then(|value| value.as_u64())
                .filter(|count| *count > 0)
                .unwrap_or_else(|| panic!("Attested symbol count for {kind:?} is missing"));
            let actual_count = symbols
                .iter()
                .filter(|symbol| symbol.get("kind").and_then(|value| value.as_str()) == Some(kind))
                .count() as u64;
            assert_eq!(
                actual_count, expected_count,
                "Global catalog {kind} count does not match its attestation"
            );
        }
    }

    // `lookup()` has one deterministic winner per folded alias. Collisions are
    // safe only when every claimant has the same use capabilities: the current
    // property↔system-enum compatibility aliases are all readable/non-callable.
    // A future function↔value collision would otherwise make one valid use look
    // absent depending on manifest sort order, so reject it during the build.
    let mut alias_capabilities = std::collections::HashMap::<String, (bool, bool)>::new();
    for symbol in symbols {
        let kind = symbol.get("kind").and_then(|value| value.as_str());
        let capabilities = match kind {
            Some("function") => (true, false),
            Some("property" | "system_enum") => (false, true),
            other => panic!("Unknown global catalog symbol kind: {other:?}"),
        };
        for alias in ["ru", "en"]
            .into_iter()
            .filter_map(|field| symbol.get(field).and_then(|value| value.as_str()))
            .filter(|alias| !alias.is_empty())
        {
            let folded = alias.to_lowercase();
            if let Some(previous) = alias_capabilities.insert(folded.clone(), capabilities) {
                assert_eq!(
                    previous, capabilities,
                    "Global catalog alias {alias:?} has incompatible call/read claimants"
                );
            }
        }
    }

    code.push_str("pub const PLATFORM_GLOBAL_CATALOG: &[RawPlatformGlobalSymbol] = &[\n");
    for symbol in symbols {
        let kind = match symbol.get("kind").and_then(|value| value.as_str()) {
            Some("function") => "RawPlatformGlobalKind::Function",
            Some("property") => "RawPlatformGlobalKind::Property",
            Some("system_enum") => "RawPlatformGlobalKind::SystemEnum",
            other => panic!("Unknown global catalog symbol kind: {other:?}"),
        };
        let ru = symbol.get("ru").and_then(|value| value.as_str()).unwrap_or("");
        let en = symbol.get("en").and_then(|value| value.as_str()).unwrap_or("");
        assert!(!ru.is_empty() || !en.is_empty(), "Global catalog symbol has no alias");
        let environment_mask = symbol
            .get("environment_mask")
            .and_then(|value| value.as_u64())
            .filter(|value| *value <= u8::MAX as u64)
            .expect("Global catalog environment mask is invalid");
        let writable = symbol.get("writable").and_then(|value| value.as_bool()).unwrap_or(false);
        code.push_str("    RawPlatformGlobalSymbol {\n");
        code.push_str(&format!("        canonical_ru: {:?},\n", ru));
        code.push_str(&format!("        canonical_en: {:?},\n", en));
        code.push_str(&format!("        kind: {},\n", kind));
        code.push_str(&format!("        environment_mask: {},\n", environment_mask));
        code.push_str(&format!("        writable: {},\n", writable));
        code.push_str("    },\n");
    }
    code.push_str("];\n");
}

fn generate_empty_structures(global_catalog_path: &Path, output_path: &Path) {
    let mut code = r#"// Auto-generated by build.rs
// NO PLATFORM DATA AVAILABLE

use super::types::*;

pub const PLATFORM_TYPES: &[RawPlatformType] = &[];
pub const PLATFORM_METHODS: &[RawPlatformMethod] = &[];
pub const PLATFORM_GLOBAL_FUNCTIONS: &[RawGlobalFunction] = &[];
pub const METHOD_DOCS: &[RawMethodDocs] = &[];
pub const GLOBAL_FUNCTION_DOCS: &[RawMethodDocs] = &[];
pub const PLATFORM_CONSTRUCTORS: &[RawPlatformConstructor] = &[];
pub const CONSTRUCTOR_DOCS: &[RawConstructorDocs] = &[];
pub const PLATFORM_PROPERTIES: &[RawPlatformProperty] = &[];
pub const PROPERTY_DOCS: &[RawPropertyDocs] = &[];
"#
    .to_string();

    append_global_catalog(&mut code, global_catalog_path);
    fs::write(output_path, code).expect("Failed to write generated.rs");
}
