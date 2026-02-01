//! BSL Analyzer Launcher
//!
//! Минимальный бинарник (~1-2 MB) для запуска LSP сервера bsl-analyzer.
//! Автоматически скачивает, обновляет и верифицирует основное приложение.
//!
//! Архитектура:
//! ```text
//! bsl-analyzer (launcher) -> скачивает -> bsl-analyzer-app (LSP сервер)
//!                                              |
//!                                              v
//!                                     ~/.bsl-analyzer/bin/
//! ```

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sys_locale::get_locale;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(120);

// URL сервера релизов (переопределяется через BSL_RELEASE_URL)
const DEFAULT_RELEASE_URL: &str = "https://dev.runsystems.ru/releases";

// Идентификатор продукта для multi-product сервера
const PRODUCT: &str = "bsl-analyzer";

// Публичный ключ для проверки подписей (32 байта, hex-encoded)
// Общий ключ releases-server (тот же что у rtools)
const PUBLIC_KEY_HEX: &str = "a2618f20b4a0d270b627c164f5b8bcecc7559f85a25489620d7ab614cc8efbe8";

struct Messages {
    connecting: &'static str,
    downloading: &'static str,
    downloading_binary: &'static str,
    fetching_manifest: &'static str,
    fetching_signature: &'static str,
    installed: &'static str,
    up_to_date: &'static str,
    updating: &'static str,
    verifying: &'static str,
    verified: &'static str,
    no_installation: &'static str,
    ok: &'static str,
    failed: &'static str,
    self_update_downloading: &'static str,
    self_update_done: &'static str,
    self_update_up_to_date: &'static str,
    launcher_commands: &'static str,
    help_self_update: &'static str,
    help_version: &'static str,
    help_update: &'static str,
    help_verify: &'static str,
}

const MESSAGES_RU: Messages = Messages {
    connecting: "Подключение к серверу обновлений... ",
    downloading: "Загрузка bsl-analyzer {}...",
    downloading_binary: "Загрузка бинарника ({:.1} МБ)... ",
    fetching_manifest: "Получение манифеста... ",
    fetching_signature: "Получение подписи... ",
    installed: "bsl-analyzer {} установлен и проверен",
    up_to_date: "bsl-analyzer актуален ({})",
    updating: "Обновление bsl-analyzer: {:?} -> {}",
    verifying: "Проверка bsl-analyzer {}...",
    verified: "Проверка успешна!",
    no_installation: "Установка bsl-analyzer не найдена",
    ok: "ок",
    failed: "ошибка",
    self_update_downloading: "Обновление лаунчера до версии {}...",
    self_update_done: "Лаунчер обновлён. Проверьте изменения: git status",
    self_update_up_to_date: "Лаунчер актуален ({})",
    launcher_commands: "Команды лаунчера:",
    help_self_update: "Обновить лаунчер",
    help_version: "Показать версию лаунчера",
    help_update: "Обновить bsl-analyzer",
    help_verify: "Проверить целостность установки",
};

const MESSAGES_EN: Messages = Messages {
    connecting: "Connecting to update server... ",
    downloading: "Downloading bsl-analyzer {}...",
    downloading_binary: "Downloading binary ({:.1} MB)... ",
    fetching_manifest: "Fetching manifest... ",
    fetching_signature: "Fetching signature... ",
    installed: "bsl-analyzer {} installed and verified",
    up_to_date: "bsl-analyzer is up to date ({})",
    updating: "Updating bsl-analyzer: {:?} -> {}",
    verifying: "Verifying bsl-analyzer {}...",
    verified: "Verification successful!",
    no_installation: "No bsl-analyzer installation found",
    ok: "ok",
    failed: "failed",
    self_update_downloading: "Updating launcher to version {}...",
    self_update_done: "Launcher updated. Check changes: git status",
    self_update_up_to_date: "Launcher is up to date ({})",
    launcher_commands: "Launcher commands:",
    help_self_update: "Update launcher binary",
    help_version: "Show launcher version",
    help_update: "Update bsl-analyzer",
    help_verify: "Verify installation integrity",
};

fn messages() -> &'static Messages {
    get_locale()
        .map(|l| {
            if l.starts_with("ru") {
                &MESSAGES_RU
            } else {
                &MESSAGES_EN
            }
        })
        .unwrap_or(&MESSAGES_RU)
}

fn get_release_url() -> String {
    std::env::var("BSL_RELEASE_URL").unwrap_or_else(|_| DEFAULT_RELEASE_URL.to_string())
}

fn create_http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(READ_TIMEOUT)
        .build()
        .context("Failed to create HTTP client")
}

fn create_download_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .context("Failed to create HTTP client")
}

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    #[allow(dead_code)]
    timestamp: String,
    files: std::collections::HashMap<String, FileInfo>,
}

#[derive(Debug, Deserialize)]
struct FileInfo {
    sha256: String,
    size: u64,
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    // Специальные команды launcher
    match args.first().map(|s| s.as_str()) {
        Some("--launcher-update") => return update_analyzer(),
        Some("--launcher-version") => {
            println!("bsl-analyzer-launcher {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--launcher-verify") => return verify_installation(),
        Some("--launcher-self-update") => return self_update_launcher(),
        Some("--help" | "-h") if args.len() == 1 => {
            return show_help_with_launcher_commands();
        }
        _ => {}
    }

    // Находим или скачиваем bsl-analyzer
    let analyzer_path = ensure_analyzer()?;

    // Запускаем bsl-analyzer с переданными аргументами
    // Важно: для LSP нужно наследовать stdin/stdout/stderr
    let status = Command::new(&analyzer_path)
        .args(&args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to execute bsl-analyzer at {:?}", analyzer_path))?;

    std::process::exit(status.code().unwrap_or(1));
}

fn show_help_with_launcher_commands() -> Result<()> {
    let analyzer_path = ensure_analyzer()?;
    let m = messages();

    let output = Command::new(&analyzer_path)
        .arg("--help")
        .output()
        .with_context(|| format!("Failed to execute bsl-analyzer at {:?}", analyzer_path))?;

    print!("{}", String::from_utf8_lossy(&output.stdout));

    println!("\n{}", m.launcher_commands);
    println!("  {:30} {}", "--launcher-self-update", m.help_self_update);
    println!("  {:30} {}", "--launcher-version", m.help_version);
    println!("  {:30} {}", "--launcher-update", m.help_update);
    println!("  {:30} {}", "--launcher-verify", m.help_verify);

    Ok(())
}

fn ensure_analyzer() -> Result<PathBuf> {
    let cache_dir = get_cache_dir()?;
    let current_link = cache_dir.join("current");

    if current_link.exists() {
        if let Ok(target) = fs::read_link(&current_link) {
            let full_path = if target.is_absolute() {
                target
            } else {
                cache_dir.join(&target)
            };

            if full_path.exists() {
                check_updates_if_needed(&cache_dir);
                return Ok(full_path);
            }
        }
    }

    download_latest(&cache_dir)
}

fn get_cache_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let cache_dir = home.join(".bsl-analyzer").join("bin");
    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}

fn check_updates_if_needed(cache_dir: &Path) {
    let marker = cache_dir.join(".last_check");

    if let Ok(metadata) = fs::metadata(&marker) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(elapsed) = modified.elapsed() {
                if elapsed.as_secs() < 86400 {
                    return;
                }
            }
        }
    }

    if let Ok(exe) = env::current_exe() {
        let _ = Command::new(exe)
            .arg("--launcher-update")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    let _ = fs::write(&marker, "");
}

fn update_analyzer() -> Result<()> {
    let cache_dir = get_cache_dir()?;

    let latest_version = fetch_latest_version()?;
    let current_version = get_current_version(&cache_dir);

    let m = messages();
    if Some(&latest_version) == current_version.as_ref() {
        eprintln!("{}", m.up_to_date.replace("{}", &latest_version));
        return Ok(());
    }

    eprintln!(
        "{}",
        m.updating
            .replace("{:?}", &format!("{:?}", current_version))
            .replace("{}", &latest_version)
    );
    download_version(&cache_dir, &latest_version)?;

    Ok(())
}

fn verify_installation() -> Result<()> {
    let cache_dir = get_cache_dir()?;
    let current_link = cache_dir.join("current");

    let m = messages();
    if !current_link.exists() {
        bail!("{}", m.no_installation);
    }

    let target = fs::read_link(&current_link)?;
    let version = target
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("bsl-analyzer-"))
        .context("Invalid current link")?;

    eprintln!("{}", m.verifying.replace("{}", version));

    let manifest = fetch_and_verify_manifest(version)?;

    let binary_path = if target.is_absolute() {
        target
    } else {
        cache_dir.join(&target)
    };

    let platform = get_platform_binary();
    let expected = manifest
        .files
        .get(platform)
        .context("Platform not found in manifest")?;

    verify_file_checksum(&binary_path, &expected.sha256)?;

    eprintln!("{}", m.verified);
    Ok(())
}

const LAUNCHER_MAPPINGS: &[(&str, &str)] = &[
    ("bsl-analyzer", "bsl-launcher-linux-amd64"),
    ("bsl-analyzer.exe", "bsl-launcher-windows-amd64.exe"),
    ("bsl-analyzer-mac", "bsl-launcher-darwin-arm64"),
];

fn self_update_launcher() -> Result<()> {
    let m = messages();

    let current_exe = env::current_exe().context("Cannot determine current executable path")?;
    let launcher_dir = current_exe
        .parent()
        .context("Cannot determine launcher directory")?;

    let latest_version = fetch_latest_version()?;
    let manifest = fetch_and_verify_manifest(&latest_version)?;

    eprintln!(
        "{}",
        m.self_update_downloading.replace("{}", &latest_version)
    );

    let client = create_download_client()?;
    let mut updated_count = 0;

    for (local_name, remote_name) in LAUNCHER_MAPPINGS {
        let local_path = launcher_dir.join(local_name);

        if !local_path.exists() {
            continue;
        }

        let file_info = match manifest.files.get(*remote_name) {
            Some(info) => info,
            None => {
                eprintln!(
                    "  {} -> {} (not in manifest, skipped)",
                    local_name, remote_name
                );
                continue;
            }
        };

        if verify_file_checksum(&local_path, &file_info.sha256).is_ok() {
            eprintln!(
                "  {} ({:.1} MB) ... up to date",
                local_name,
                file_info.size as f64 / 1_048_576.0
            );
            continue;
        }

        eprint!(
            "  {} ({:.1} MB) ... ",
            local_name,
            file_info.size as f64 / 1_048_576.0
        );

        let bytes = download_launcher_binary(&client, &latest_version, remote_name, file_info)?;

        let is_current_exe = local_path == current_exe;
        update_launcher_file(&local_path, &bytes, is_current_exe)?;

        eprintln!("{}", m.ok);
        updated_count += 1;
    }

    if updated_count > 0 {
        eprintln!("{}", m.self_update_done);
    } else {
        eprintln!(
            "{}",
            m.self_update_up_to_date.replace("{}", &latest_version)
        );
    }

    Ok(())
}

fn download_launcher_binary(
    client: &reqwest::blocking::Client,
    version: &str,
    remote_name: &str,
    file_info: &FileInfo,
) -> Result<Vec<u8>> {
    let m = messages();
    let url = format!(
        "{}/{}/{}/{}",
        get_release_url(),
        PRODUCT,
        version,
        remote_name
    );

    let response = client
        .get(&url)
        .send()
        .with_context(|| format!("Failed to download from {}", url))?;

    if !response.status().is_success() {
        eprintln!("{}", m.failed);
        bail!("Download failed: HTTP {}", response.status());
    }

    let bytes = response.bytes()?.to_vec();

    if bytes.len() as u64 != file_info.size {
        bail!(
            "Size mismatch: expected {}, got {}",
            file_info.size,
            bytes.len()
        );
    }

    let hash = compute_sha256(&bytes);
    if hash != file_info.sha256 {
        bail!(
            "Checksum mismatch!\nExpected: {}\nGot: {}",
            file_info.sha256,
            hash
        );
    }

    Ok(bytes)
}

fn update_launcher_file(path: &Path, bytes: &[u8], is_current_exe: bool) -> Result<()> {
    if is_current_exe {
        let temp_path = path.with_extension("new");
        fs::write(&temp_path, bytes)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&temp_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&temp_path, perms)?;
        }

        self_replace::self_replace(&temp_path).context("Failed to replace executable")?;

        let _ = fs::remove_file(&temp_path);
    } else {
        fs::write(path, bytes)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms)?;
        }
    }

    Ok(())
}

fn download_latest(cache_dir: &Path) -> Result<PathBuf> {
    let version = fetch_latest_version()?;
    download_version(cache_dir, &version)
}

fn download_version(cache_dir: &Path, version: &str) -> Result<PathBuf> {
    let binary_name = format!("bsl-analyzer-{}", version);
    let binary_path = cache_dir.join(&binary_name);

    if binary_path.exists() {
        if verify_existing_binary(version, &binary_path).is_ok() {
            update_current_link(cache_dir, &binary_path)?;
            return Ok(binary_path);
        }
        let _ = fs::remove_file(&binary_path);
    }

    let m = messages();
    eprintln!("{}", m.downloading.replace("{}", version));

    let manifest = fetch_and_verify_manifest(version)?;

    let platform = get_platform_binary();
    let file_info = manifest
        .files
        .get(platform)
        .context(format!("Platform {} not found in manifest", platform))?;

    eprint!(
        "{}",
        m.downloading_binary
            .replace("{:.1}", &format!("{:.1}", file_info.size as f64 / 1_048_576.0))
    );
    let client = create_download_client()?;
    let url = format!(
        "{}/{}/{}/{}",
        get_release_url(),
        PRODUCT,
        version,
        platform
    );
    let response = client
        .get(&url)
        .send()
        .with_context(|| format!("Failed to download from {}", url))?;

    if !response.status().is_success() {
        eprintln!("{}", m.failed);
        bail!("Download failed: HTTP {}", response.status());
    }

    let bytes = response.bytes()?;
    eprintln!("{}", m.ok);

    if bytes.len() as u64 != file_info.size {
        bail!(
            "Size mismatch: expected {}, got {}",
            file_info.size,
            bytes.len()
        );
    }

    let hash = compute_sha256(&bytes);
    if hash != file_info.sha256 {
        bail!(
            "Checksum mismatch!\nExpected: {}\nGot: {}",
            file_info.sha256,
            hash
        );
    }

    fs::write(&binary_path, &bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&binary_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&binary_path, perms)?;
    }

    update_current_link(cache_dir, &binary_path)?;

    eprintln!("{}", m.installed.replace("{}", version));
    Ok(binary_path)
}

fn fetch_and_verify_manifest(version: &str) -> Result<Manifest> {
    let m = messages();
    let client = create_http_client()?;

    eprint!("{}", m.fetching_manifest);
    let manifest_url = format!(
        "{}/{}/{}/manifest.json",
        get_release_url(),
        PRODUCT,
        version
    );
    let manifest_response = client
        .get(&manifest_url)
        .send()
        .with_context(|| format!("Failed to fetch manifest from {}", manifest_url))?;

    if !manifest_response.status().is_success() {
        eprintln!("{}", m.failed);
        bail!(
            "Failed to fetch manifest: HTTP {}",
            manifest_response.status()
        );
    }

    let manifest_bytes = manifest_response.bytes()?;
    eprintln!("{}", m.ok);

    eprint!("{}", m.fetching_signature);
    let sig_url = format!(
        "{}/{}/{}/manifest.sig",
        get_release_url(),
        PRODUCT,
        version
    );
    let sig_response = client
        .get(&sig_url)
        .send()
        .with_context(|| format!("Failed to fetch signature from {}", sig_url))?;

    if !sig_response.status().is_success() {
        eprintln!("{}", m.failed);
        bail!(
            "Failed to fetch signature: HTTP {}",
            sig_response.status()
        );
    }

    let sig_bytes = sig_response.bytes()?;
    eprintln!("{}", m.ok);

    verify_signature(&manifest_bytes, &sig_bytes)?;

    let manifest: Manifest =
        serde_json::from_slice(&manifest_bytes).context("Failed to parse manifest.json")?;

    if manifest.version != version {
        bail!(
            "Version mismatch in manifest: expected {}, got {}",
            version,
            manifest.version
        );
    }

    Ok(manifest)
}

fn verify_signature(data: &[u8], signature_hex: &[u8]) -> Result<()> {
    let public_key_bytes = hex::decode(PUBLIC_KEY_HEX).context("Invalid public key hex")?;

    let public_key_array: [u8; 32] = public_key_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Public key must be 32 bytes"))?;

    let verifying_key =
        VerifyingKey::from_bytes(&public_key_array).context("Invalid public key")?;

    let sig_hex_str = std::str::from_utf8(signature_hex)
        .context("Signature is not valid UTF-8")?
        .trim();

    let sig_bytes = hex::decode(sig_hex_str).context("Invalid signature hex")?;

    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Signature must be 64 bytes"))?;

    let signature = Signature::from_bytes(&sig_array);

    verifying_key
        .verify(data, &signature)
        .context("Signature verification failed")?;

    Ok(())
}

fn verify_existing_binary(version: &str, path: &Path) -> Result<()> {
    let manifest = fetch_and_verify_manifest(version)?;
    let platform = get_platform_binary();
    let expected = manifest
        .files
        .get(platform)
        .context("Platform not found in manifest")?;

    verify_file_checksum(path, &expected.sha256)
}

fn verify_file_checksum(path: &Path, expected_sha256: &str) -> Result<()> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let hash = hex::encode(hasher.finalize());

    if hash != expected_sha256 {
        bail!("Checksum mismatch for {:?}", path);
    }

    Ok(())
}

fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn update_current_link(cache_dir: &Path, target: &Path) -> Result<()> {
    let link_path = cache_dir.join("current");

    let _ = fs::remove_file(&link_path);

    let target_name = target.file_name().context("Target has no filename")?;

    #[cfg(unix)]
    std::os::unix::fs::symlink(target_name, &link_path)?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target_name, &link_path)?;

    Ok(())
}

fn fetch_latest_version() -> Result<String> {
    let m = messages();
    eprint!("{}", m.connecting);

    let client = create_http_client()?;
    let url = format!("{}/{}/latest", get_release_url(), PRODUCT);
    let response = client
        .get(&url)
        .send()
        .with_context(|| format!("Failed to connect to {}", url))?;

    if !response.status().is_success() {
        eprintln!("{}", m.failed);
        bail!("Failed to get latest version: HTTP {}", response.status());
    }

    let version = response.text()?.trim().to_string();
    eprintln!("{}", m.ok);
    Ok(version)
}

fn get_current_version(cache_dir: &Path) -> Option<String> {
    let current_link = cache_dir.join("current");
    let target = fs::read_link(&current_link).ok()?;
    let file_name = target.file_name()?.to_str()?;

    file_name
        .strip_prefix("bsl-analyzer-")
        .map(|s| s.to_string())
}

fn get_platform_binary() -> &'static str {
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
