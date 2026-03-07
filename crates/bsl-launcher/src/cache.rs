use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

pub fn get_cache_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let cache_dir = home.join(".bsl-analyzer").join("bin");
    fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir)
}

pub fn get_current_version(cache_dir: &Path) -> Option<String> {
    let current_link = cache_dir.join("current");
    let target = fs::read_link(&current_link).ok()?;
    let file_name = target.file_name()?.to_str()?;

    file_name.strip_prefix("bsl-analyzer-").map(|s| s.to_string())
}

pub fn update_current_link(cache_dir: &Path, target: &Path) -> Result<()> {
    let link_path = cache_dir.join("current");

    let _ = fs::remove_file(&link_path);

    let target_name = target.file_name().context("Target has no filename")?;

    #[cfg(unix)]
    std::os::unix::fs::symlink(target_name, &link_path)?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target_name, &link_path)?;

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
