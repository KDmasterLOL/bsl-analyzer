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
            cleanup_cached_versions(&cache_dir, std::slice::from_ref(&resolved_version), 1);
            return Ok(binary_path);
        }
        let _ = fs::remove_file(&binary_path);
    }

    let path = download_version_without_linking(provider, &cache_dir, &resolved_version)?;
    cleanup_cached_versions(&cache_dir, &[resolved_version], 1);
    Ok(path)
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
        cleanup_cached_versions(&cache_dir, &[], 1);
        return Ok(());
    }

    eprintln!(
        "{}",
        m.updating
            .replace("{:?}", &format!("{:?}", current_version))
            .replace("{}", &latest_version)
    );
    download_version(provider, &cache_dir, &latest_version)?;
    cleanup_cached_versions(&cache_dir, &[], 1);

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
            cleanup_cached_versions(cache_dir, &[], 1);
            return Ok(binary_path);
        }
        let _ = fs::remove_file(&binary_path);
    }

    let path = do_download(provider, cache_dir, version)?;
    update_current_link(cache_dir, &path)?;

    let m = messages();
    eprintln!("{}", m.installed.replace("{}", version));

    cleanup_cached_versions(cache_dir, &[], 1);

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

fn cleanup_cached_versions(cache_dir: &Path, preserve_versions: &[String], keep_count: usize) {
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

    let ordered_versions: Vec<String> =
        versions.iter().map(|(version, _)| version.clone()).collect();
    let versions_to_remove = select_versions_to_remove(
        &ordered_versions,
        current_version.as_deref(),
        preserve_versions,
        keep_count,
    );

    for (version, path) in &versions {
        if versions_to_remove.contains(version) {
            let _ = fs::remove_file(path);
        }
    }
}

fn select_versions_to_remove(
    ordered_versions: &[String],
    current_version: Option<&str>,
    preserve_versions: &[String],
    keep_count: usize,
) -> std::collections::HashSet<String> {
    let mut preserve: std::collections::HashSet<&str> =
        preserve_versions.iter().map(String::as_str).collect();
    if let Some(current) = current_version {
        preserve.insert(current);
    }

    let mut kept = 0;
    let mut versions_to_remove = std::collections::HashSet::new();

    for version in ordered_versions {
        if preserve.contains(version.as_str()) {
            continue;
        }

        if kept < keep_count {
            kept += 1;
        } else {
            versions_to_remove.insert(version.clone());
        }
    }

    versions_to_remove
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::thread::sleep;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let unique = format!(
                "bsl-launcher-test-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system time before unix epoch")
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("failed to create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn create_version_file(cache_dir: &Path, version: &str) -> PathBuf {
        let path = cache_dir.join(format!("bsl-analyzer-{version}"));
        fs::write(&path, version).expect("failed to create version file");
        sleep(Duration::from_millis(20));
        path
    }

    fn version_exists(cache_dir: &Path, version: &str) -> bool {
        cache_dir.join(format!("bsl-analyzer-{version}")).exists()
    }

    #[test]
    fn select_versions_to_remove_keeps_current_and_latest_previous() {
        let versions =
            vec!["0.1.2", "0.1.1", "0.1.0"].into_iter().map(str::to_string).collect::<Vec<_>>();

        let to_remove = select_versions_to_remove(&versions, Some("0.1.2"), &[], 1);

        assert!(!to_remove.contains("0.1.2"));
        assert!(!to_remove.contains("0.1.1"));
        assert!(to_remove.contains("0.1.0"));
    }

    #[test]
    fn select_versions_to_remove_preserves_requested_version_for_pinned_run() {
        let versions = vec!["0.1.2", "0.1.1", "0.1.0", "0.0.9"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let preserve = ["0.1.2".to_string()];

        let to_remove = select_versions_to_remove(&versions, Some("0.1.0"), &preserve, 1);

        assert!(!to_remove.contains("0.1.2"));
        assert!(!to_remove.contains("0.1.1"));
        assert!(!to_remove.contains("0.1.0"));
        assert!(to_remove.contains("0.0.9"));
    }

    #[test]
    fn cleanup_keeps_current_and_latest_previous() {
        let dir = TestDir::new();
        create_version_file(dir.path(), "0.1.0");
        create_version_file(dir.path(), "0.1.1");
        let current = create_version_file(dir.path(), "0.1.2");
        update_current_link(dir.path(), &current).expect("failed to update current link");

        cleanup_cached_versions(dir.path(), &[], 1);

        assert!(version_exists(dir.path(), "0.1.2"));
        assert!(version_exists(dir.path(), "0.1.1"));
        assert!(!version_exists(dir.path(), "0.1.0"));
    }

    #[test]
    fn cleanup_preserves_requested_version_for_pinned_run() {
        let dir = TestDir::new();
        create_version_file(dir.path(), "0.0.9");
        let current = create_version_file(dir.path(), "0.1.0");
        create_version_file(dir.path(), "0.1.1");
        create_version_file(dir.path(), "0.1.2");
        update_current_link(dir.path(), &current).expect("failed to update current link");

        cleanup_cached_versions(dir.path(), &["0.1.2".to_string()], 1);

        assert!(version_exists(dir.path(), "0.1.0"));
        assert!(version_exists(dir.path(), "0.1.2"));
        assert!(version_exists(dir.path(), "0.1.1"));
        assert!(!version_exists(dir.path(), "0.0.9"));
    }
}
