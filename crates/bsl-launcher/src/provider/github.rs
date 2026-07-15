use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::entities::{FileInfo, Manifest};
use crate::messages::messages;
use crate::provider::ReleaseProvider;

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

pub struct GitHubProvider {
    repo: String,
}

impl GitHubProvider {
    pub fn new(repo: &str) -> Self {
        Self { repo: repo.to_string() }
    }
}

impl ReleaseProvider for GitHubProvider {
    fn fetch_latest_version(&self, client: &reqwest::blocking::Client) -> Result<String> {
        let url = format!("https://api.github.com/repos/{}/releases/latest", self.repo);
        let response = client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "bsl-analyzer-launcher")
            .send()
            .with_context(|| format!("Failed to connect to {}", url))?;

        if !response.status().is_success() {
            bail!("Failed to get latest version: HTTP {}", response.status());
        }

        let text = response.text()?;
        let release: GitHubRelease =
            serde_json::from_str(&text).context("Failed to parse GitHub release JSON")?;

        let version = release.tag_name.strip_prefix('v').unwrap_or(&release.tag_name).to_string();

        Ok(version)
    }

    fn fetch_manifest(
        &self,
        client: &reqwest::blocking::Client,
        version: &str,
    ) -> Result<Manifest> {
        let m = messages();

        eprint!("{}", m.fetching_release_info);
        let url = format!("https://api.github.com/repos/{}/releases/tags/v{}", self.repo, version);
        let response = client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "bsl-analyzer-launcher")
            .send()
            .with_context(|| format!("Failed to fetch release from {}", url))?;

        if !response.status().is_success() {
            eprintln!("{}", m.failed);
            bail!("Failed to fetch release: HTTP {}", response.status());
        }

        let text = response.text()?;
        let release: GitHubRelease =
            serde_json::from_str(&text).context("Failed to parse GitHub release JSON")?;
        eprintln!("{}", m.ok);

        let checksums_asset = release
            .assets
            .iter()
            .find(|a| a.name == "checksums.txt")
            .context("checksums.txt not found in release assets")?;

        eprint!("{}", m.fetching_checksums);
        let checksums_response = client
            .get(&checksums_asset.browser_download_url)
            .header("User-Agent", "bsl-analyzer-launcher")
            .send()
            .context("Failed to download checksums.txt")?;

        if !checksums_response.status().is_success() {
            eprintln!("{}", m.failed);
            bail!("Failed to download checksums.txt: HTTP {}", checksums_response.status());
        }

        let checksums_text = checksums_response.text()?;
        eprintln!("{}", m.ok);

        let checksums: HashMap<String, String> = checksums_text
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(2, |c: char| c.is_whitespace());
                let hash = parts.next()?.trim();
                let name = parts.next()?.trim().trim_start_matches('*');
                if hash.is_empty() || name.is_empty() {
                    return None;
                }
                Some((name.to_string(), hash.to_string()))
            })
            .collect();

        let mut files = HashMap::new();
        for asset in &release.assets {
            if asset.name == "checksums.txt" {
                continue;
            }
            if let Some(sha256) = checksums.get(&asset.name) {
                files.insert(
                    asset.name.clone(),
                    FileInfo { sha256: sha256.clone(), size: asset.size },
                );
            }
        }

        Ok(Manifest { version: version.to_string(), files })
    }

    fn download_url(&self, version: &str, file_name: &str) -> String {
        format!("https://github.com/{}/releases/download/v{}/{}", self.repo, version, file_name)
    }
}
