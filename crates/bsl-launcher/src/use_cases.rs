use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::cache::{
    compute_sha256, get_cache_dir, get_current_version, read_current_link, update_current_link,
    verify_file_checksum,
};
use crate::entities::{get_platform_binary, FileInfo};
use crate::messages::messages;
use crate::provider::ReleaseProvider;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(120);
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_millis(300);

const LAUNCHER_MAPPINGS: &[(&str, &str)] = &[
    ("bsl-analyzer", "bsl-analyzer-linux-amd64"),
    ("bsl-analyzer.exe", "bsl-analyzer-windows-amd64.exe"),
    ("bsl-analyzer-mac", "bsl-analyzer-darwin-arm64"),
];

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

fn fetch_version_verbose(
    provider: &dyn ReleaseProvider,
    client: &reqwest::blocking::Client,
) -> Result<String> {
    let m = messages();
    eprint!("{}", m.connecting);
    match provider.fetch_latest_version(client) {
        Ok(v) => {
            eprintln!("{}", m.ok);
            Ok(v)
        }
        Err(e) => {
            eprintln!("{}", m.failed);
            Err(e)
        }
    }
}

fn try_fetch_latest_version_fast(provider: &dyn ReleaseProvider) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(UPDATE_CHECK_TIMEOUT)
        .timeout(UPDATE_CHECK_TIMEOUT)
        .build()?;
    provider.fetch_latest_version(&client)
}

pub fn ensure_analyzer(provider: &dyn ReleaseProvider, sync_update: bool) -> Result<PathBuf> {
    let cache_dir = get_cache_dir()?;

    if let Some(target) = read_current_link(&cache_dir) {
        let full_path = if target.is_absolute() { target } else { cache_dir.join(&target) };

        if full_path.exists() {
            check_updates_if_needed(provider, &cache_dir, sync_update)?;
            // После синхронного обновления current мог измениться
            if sync_update {
                if let Some(new_target) = read_current_link(&cache_dir) {
                    let new_path = if new_target.is_absolute() {
                        new_target
                    } else {
                        cache_dir.join(&new_target)
                    };
                    if new_path.exists() {
                        return Ok(new_path);
                    }
                }
            }
            return Ok(full_path);
        }
    }

    download_latest(provider, &cache_dir)
}

pub fn ensure_specific_version(provider: &dyn ReleaseProvider, version: &str) -> Result<PathBuf> {
    let cache_dir = get_cache_dir()?;

    let resolved_version = if version == "latest" {
        let http_client = create_http_client()?;
        fetch_version_verbose(provider, &http_client)?
    } else {
        version.to_string()
    };

    let binary_name = format!("bsl-analyzer-{}", resolved_version);
    let binary_path = cache_dir.join(&binary_name);

    if binary_path.exists() {
        if verify_existing_binary(provider, &resolved_version, &binary_path).is_ok() {
            return Ok(binary_path);
        }
        let _ = fs::remove_file(&binary_path);
    }

    download_version_without_linking(provider, &cache_dir, &resolved_version)
}

fn check_updates_if_needed(
    provider: &dyn ReleaseProvider,
    cache_dir: &Path,
    sync_update: bool,
) -> Result<()> {
    if sync_update {
        let Ok(latest_version) = try_fetch_latest_version_fast(provider) else {
            return Ok(());
        };

        let current_version = get_current_version(cache_dir);
        if Some(&latest_version) == current_version.as_ref() {
            return Ok(());
        }

        download_version(provider, cache_dir, &latest_version)?;
    } else if let Ok(exe) = env::current_exe() {
        let _ = Command::new(exe)
            .arg("--launcher-update")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    Ok(())
}

pub fn update_analyzer(provider: &dyn ReleaseProvider) -> Result<()> {
    let cache_dir = get_cache_dir()?;
    let http_client = create_http_client()?;

    let latest_version = fetch_version_verbose(provider, &http_client)?;
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
    download_version(provider, &cache_dir, &latest_version)?;

    Ok(())
}

pub fn verify_installation(provider: &dyn ReleaseProvider) -> Result<()> {
    let cache_dir = get_cache_dir()?;

    let m = messages();
    let target = match read_current_link(&cache_dir) {
        Some(t) => t,
        None => bail!("{}", m.no_installation),
    };
    let version = target
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("bsl-analyzer-"))
        .context("Invalid current link")?;

    eprintln!("{}", m.verifying.replace("{}", version));

    let http_client = create_http_client()?;
    let manifest = provider.fetch_manifest(&http_client, version)?;

    let binary_path = if target.is_absolute() { target } else { cache_dir.join(&target) };

    let platform = get_platform_binary();
    let expected = manifest.files.get(platform).context("Platform not found in manifest")?;

    verify_file_checksum(&binary_path, &expected.sha256)?;

    eprintln!("{}", m.verified);
    Ok(())
}

pub fn self_update_launcher(provider: &dyn ReleaseProvider) -> Result<()> {
    let m = messages();
    let http_client = create_http_client()?;

    let latest_version = fetch_version_verbose(provider, &http_client)?;
    let manifest = provider.fetch_manifest(&http_client, &latest_version)?;

    eprintln!("{}", m.self_update_downloading.replace("{}", &latest_version));

    let current_exe = env::current_exe().context("Cannot determine current executable path")?;
    let launcher_dir = current_exe.parent().context("Cannot determine launcher directory")?;

    let download_client = create_download_client()?;
    let mut updated_count = 0;

    for (local_name, remote_name) in LAUNCHER_MAPPINGS {
        let local_path = launcher_dir.join(local_name);

        if !local_path.exists() {
            continue;
        }

        let file_info = match manifest.files.get(*remote_name) {
            Some(info) => info,
            None => {
                eprintln!("  {} -> {} (not in manifest, skipped)", local_name, remote_name);
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

        eprint!("  {} ({:.1} MB) ... ", local_name, file_info.size as f64 / 1_048_576.0);

        let bytes = download_launcher_binary(
            &download_client,
            provider,
            &latest_version,
            remote_name,
            file_info,
        )?;

        let is_current_exe = local_path == current_exe;
        update_launcher_file(&local_path, &bytes, is_current_exe)?;

        eprintln!("{}", m.ok);
        updated_count += 1;
    }

    if updated_count > 0 {
        eprintln!("{}", m.self_update_done);
    } else {
        eprintln!("{}", m.self_update_up_to_date.replace("{}", &latest_version));
    }

    Ok(())
}

pub fn cleanup_versions(args: &[String]) -> Result<()> {
    let cache_dir = get_cache_dir()?;
    let m = messages();

    let keep_count = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--keep="))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1);

    let current_version = get_current_version(&cache_dir);

    let mut versions: Vec<(String, PathBuf)> = Vec::new();
    for entry in fs::read_dir(&cache_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if let Some(version) = name.to_str().and_then(|n| n.strip_prefix("bsl-analyzer-")) {
            versions.push((version.to_string(), entry.path()));
        }
    }

    versions.sort_by(|a, b| {
        let time_a = fs::metadata(&a.1).and_then(|m| m.modified()).ok();
        let time_b = fs::metadata(&b.1).and_then(|m| m.modified()).ok();
        time_b.cmp(&time_a)
    });

    let mut kept = 0;
    let mut removed = 0;
    for (version, path) in &versions {
        let is_current = current_version.as_ref() == Some(version);

        if is_current || kept < keep_count {
            eprintln!("  {} {}", version, if is_current { "(current)" } else { "" });
            if !is_current {
                kept += 1;
            }
        } else if let Err(e) = fs::remove_file(path) {
            eprintln!("  {} - {} {}: {}", version, m.cleanup_error, path.display(), e);
        } else {
            eprintln!("  {} - {}", version, m.cleanup_removed);
            removed += 1;
        }
    }

    eprintln!("{}", m.cleanup_done.replace("{}", &removed.to_string()));
    Ok(())
}

fn download_latest(provider: &dyn ReleaseProvider, cache_dir: &Path) -> Result<PathBuf> {
    let http_client = create_http_client()?;
    let version = fetch_version_verbose(provider, &http_client)?;
    download_version(provider, cache_dir, &version)
}

fn download_version(
    provider: &dyn ReleaseProvider,
    cache_dir: &Path,
    version: &str,
) -> Result<PathBuf> {
    let binary_name = format!("bsl-analyzer-{}", version);
    let binary_path = cache_dir.join(&binary_name);

    if binary_path.exists() {
        if verify_existing_binary(provider, version, &binary_path).is_ok() {
            update_current_link(cache_dir, &binary_path)?;
            return Ok(binary_path);
        }
        let _ = fs::remove_file(&binary_path);
    }

    let path = do_download(provider, cache_dir, version)?;
    update_current_link(cache_dir, &path)?;

    let m = messages();
    eprintln!("{}", m.installed.replace("{}", version));

    auto_cleanup(cache_dir);

    Ok(path)
}

fn download_version_without_linking(
    provider: &dyn ReleaseProvider,
    cache_dir: &Path,
    version: &str,
) -> Result<PathBuf> {
    let path = do_download(provider, cache_dir, version)?;

    let m = messages();
    eprintln!("{}", m.installed.replace("{}", version));
    Ok(path)
}

fn auto_cleanup(cache_dir: &Path) {
    let current_version = get_current_version(cache_dir);

    let mut versions: Vec<(String, PathBuf)> = Vec::new();
    let Ok(entries) = fs::read_dir(cache_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if let Some(version) = name.to_str().and_then(|n| n.strip_prefix("bsl-analyzer-")) {
            versions.push((version.to_string(), entry.path()));
        }
    }

    versions.sort_by(|a, b| {
        let time_a = fs::metadata(&a.1).and_then(|m| m.modified()).ok();
        let time_b = fs::metadata(&b.1).and_then(|m| m.modified()).ok();
        time_b.cmp(&time_a)
    });

    // Оставляем текущую + 1 предыдущую
    let mut kept = 0;
    for (version, path) in &versions {
        let is_current = current_version.as_ref() == Some(version);
        if is_current || kept < 1 {
            if !is_current {
                kept += 1;
            }
        } else {
            let _ = fs::remove_file(path);
        }
    }
}

fn do_download(provider: &dyn ReleaseProvider, cache_dir: &Path, version: &str) -> Result<PathBuf> {
    let binary_name = format!("bsl-analyzer-{}", version);
    let binary_path = cache_dir.join(&binary_name);

    let m = messages();
    eprintln!("{}", m.downloading.replace("{}", version));

    let http_client = create_http_client()?;
    let manifest = provider.fetch_manifest(&http_client, version)?;

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
    let download_client = create_download_client()?;
    let url = provider.download_url(version, platform);
    let response = download_client
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
        bail!("Size mismatch: expected {}, got {}", file_info.size, bytes.len());
    }

    let hash = compute_sha256(&bytes);
    if hash != file_info.sha256 {
        bail!("Checksum mismatch!\nExpected: {}\nGot: {}", file_info.sha256, hash);
    }

    fs::write(&binary_path, &bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&binary_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&binary_path, perms)?;
    }

    Ok(binary_path)
}

fn verify_existing_binary(
    provider: &dyn ReleaseProvider,
    version: &str,
    path: &Path,
) -> Result<()> {
    let http_client = create_http_client()?;
    let manifest = provider.fetch_manifest(&http_client, version)?;
    let platform = get_platform_binary();
    let expected = manifest.files.get(platform).context("Platform not found in manifest")?;
    verify_file_checksum(path, &expected.sha256)
}

fn download_launcher_binary(
    client: &reqwest::blocking::Client,
    provider: &dyn ReleaseProvider,
    version: &str,
    remote_name: &str,
    file_info: &FileInfo,
) -> Result<Vec<u8>> {
    let m = messages();
    let url = provider.download_url(version, remote_name);

    let response =
        client.get(&url).send().with_context(|| format!("Failed to download from {}", url))?;

    if !response.status().is_success() {
        eprintln!("{}", m.failed);
        bail!("Download failed: HTTP {}", response.status());
    }

    let bytes = response.bytes()?.to_vec();

    if bytes.len() as u64 != file_info.size {
        bail!("Size mismatch: expected {}, got {}", file_info.size, bytes.len());
    }

    let hash = compute_sha256(&bytes);
    if hash != file_info.sha256 {
        bail!("Checksum mismatch!\nExpected: {}\nGot: {}", file_info.sha256, hash);
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
