use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub files: HashMap<String, FileInfo>,
}

#[derive(Debug, Deserialize)]
pub struct FileInfo {
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseConfig {
    pub provider: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
}

pub fn get_platform_binary() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "bsl-analyzer-app-linux-amd64";

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "bsl-analyzer-app-linux-arm64";

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "bsl-analyzer-app-darwin-amd64";

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "bsl-analyzer-app-darwin-arm64";

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "bsl-analyzer-app-windows-amd64.exe";

    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
    )))]
    compile_error!("Unsupported platform");
}
