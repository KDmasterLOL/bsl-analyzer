pub mod github;
pub mod server;

use anyhow::Result;

use crate::entities::{Manifest, ReleaseConfig};

pub trait ReleaseProvider {
    fn fetch_latest_version(&self, client: &reqwest::blocking::Client) -> Result<String>;
    fn fetch_manifest(&self, client: &reqwest::blocking::Client, version: &str)
        -> Result<Manifest>;
    fn download_url(&self, version: &str, file_name: &str) -> String;
}

pub fn create_provider(config: &ReleaseConfig) -> Box<dyn ReleaseProvider> {
    match config.provider.as_str() {
        "github" => Box::new(github::GitHubProvider::new(
            config.repo.as_deref().expect("GitHub provider requires 'repo' field"),
        )),
        _ => {
            let url = std::env::var("BSL_RELEASE_URL").unwrap_or_else(|_| {
                config.url.clone().expect("Server provider requires 'url' field")
            });
            Box::new(server::ServerProvider::new(
                &url,
                config.product.as_deref().expect("Server provider requires 'product' field"),
                config.public_key.as_deref().expect("Server provider requires 'public_key' field"),
            ))
        }
    }
}
