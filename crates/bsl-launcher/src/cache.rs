use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

pub const STABLE_APP_FILENAME: &str =
    if cfg!(windows) { "bsl-analyzer-app.exe" } else { "bsl-analyzer-app" };

pub const STABLE_APP_TEMP_PREFIX: &str = "bsl-analyzer-app.new-";

pub const STABLE_VERSION_SIDECAR: &str = ".app-version";

pub const SWAP_LOCK_FILENAME: &str = ".swap.lock";

pub const CURRENT_LINK_TEMP_PREFIX: &str = "current.new-";

pub fn get_cache_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let cache_dir = home.join(".bsl-analyzer").join("bin");
    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}

pub fn stable_app_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(STABLE_APP_FILENAME)
}

pub fn unique_stable_temp_path(cache_dir: &Path) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    cache_dir.join(format!("{}{}-{}", STABLE_APP_TEMP_PREFIX, std::process::id(), nanos))
}

const STALE_TEMP_MIN_AGE: std::time::Duration = std::time::Duration::from_secs(300);

pub fn sweep_stale_swap_temps(cache_dir: &Path) {
    let Ok(entries) = fs::read_dir(cache_dir) else { return };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else { continue };
        if !(name_str.starts_with(STABLE_APP_TEMP_PREFIX)
            || name_str.starts_with(CURRENT_LINK_TEMP_PREFIX))
        {
            continue;
        }

        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        let age = now.duration_since(mtime).unwrap_or_default();
        if age >= STALE_TEMP_MIN_AGE {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub fn read_stable_version(cache_dir: &Path) -> Option<String> {
    let raw = fs::read_to_string(cache_dir.join(STABLE_VERSION_SIDECAR)).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn write_stable_version(cache_dir: &Path, version: &str) -> Result<()> {
    fs::write(cache_dir.join(STABLE_VERSION_SIDECAR), version)
        .context("Failed to write stable app version sidecar")
}

pub fn is_reserved_cache_entry(name: &str) -> bool {
    name == STABLE_APP_FILENAME
        || name.starts_with(STABLE_APP_TEMP_PREFIX)
        || name == STABLE_VERSION_SIDECAR
        || name == SWAP_LOCK_FILENAME
        || name == "current"
        || name.starts_with(CURRENT_LINK_TEMP_PREFIX)
}

pub struct SwapLock {
    _file: fs::File,
    #[cfg(windows)]
    handle: std::os::windows::io::RawHandle,
}

impl SwapLock {
    pub fn acquire(cache_dir: &Path) -> Result<Self> {
        let path = cache_dir.join(SWAP_LOCK_FILENAME);
        let file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("Failed to open swap lock {}", path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if rc != 0 {
                return Err(std::io::Error::last_os_error())
                    .context("flock(LOCK_EX) on swap lock failed");
            }
            Ok(SwapLock { _file: file })
        }

        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{LockFileEx, LOCKFILE_EXCLUSIVE_LOCK};
            let handle = file.as_raw_handle();
            let mut overlapped: windows_sys::Win32::System::IO::OVERLAPPED =
                unsafe { std::mem::zeroed() };
            let ok = unsafe {
                LockFileEx(
                    handle as windows_sys::Win32::Foundation::HANDLE,
                    LOCKFILE_EXCLUSIVE_LOCK,
                    0,
                    u32::MAX,
                    u32::MAX,
                    &mut overlapped,
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error())
                    .context("LockFileEx exclusive on swap lock failed");
            }
            Ok(SwapLock { _file: file, handle })
        }
    }
}

#[cfg(windows)]
impl Drop for SwapLock {
    fn drop(&mut self) {
        use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
        let mut overlapped: windows_sys::Win32::System::IO::OVERLAPPED =
            unsafe { std::mem::zeroed() };
        let _ = unsafe {
            UnlockFileEx(
                self.handle as windows_sys::Win32::Foundation::HANDLE,
                0,
                u32::MAX,
                u32::MAX,
                &mut overlapped,
            )
        };
    }
}

pub fn read_current_link(cache_dir: &Path) -> Option<PathBuf> {
    let link_path = cache_dir.join("current");

    #[cfg(unix)]
    {
        fs::read_link(&link_path).ok()
    }

    #[cfg(windows)]
    {
        let content = fs::read_to_string(&link_path).ok()?;
        let trimmed = content.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    }
}

pub fn get_current_version(cache_dir: &Path) -> Option<String> {
    let target = read_current_link(cache_dir)?;
    let file_name = target.file_name()?.to_str()?;

    file_name.strip_prefix("bsl-analyzer-").map(|s| s.to_string())
}

pub fn update_current_link(cache_dir: &Path, target: &Path) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    let link_path = cache_dir.join("current");
    let target_name = target.file_name().context("Target has no filename")?;

    static UNIQUE: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let seq = UNIQUE.fetch_add(1, Ordering::Relaxed);
    let temp_path = cache_dir.join(format!(
        "{}{}-{}-{}",
        CURRENT_LINK_TEMP_PREFIX,
        std::process::id(),
        nanos,
        seq,
    ));
    let _ = fs::remove_file(&temp_path);

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target_name, &temp_path)
            .context("Failed to create temp current symlink")?;
    }

    #[cfg(windows)]
    {
        fs::write(&temp_path, target_name.to_str().context("Invalid filename")?)
            .context("Failed to write temp current marker")?;
    }

    if let Err(e) = fs::rename(&temp_path, &link_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(e).context("Failed to atomically install current link");
    }

    Ok(())
}

pub fn verify_file_checksum(path: &Path, expected_sha256: &str) -> Result<()> {
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

pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}
