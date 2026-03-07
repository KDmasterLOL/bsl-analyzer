use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

use crate::entities::Manifest;
use crate::messages::messages;
use crate::provider::ReleaseProvider;

pub struct ServerProvider {
    base_url: String,
    product: String,
    public_key_hex: String,
}

impl ServerProvider {
    pub fn new(base_url: &str, product: &str, public_key_hex: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            product: product.to_string(),
            public_key_hex: public_key_hex.to_string(),
        }
    }

    fn verify_signature(&self, data: &[u8], signature_hex: &[u8]) -> Result<()> {
        let public_key_bytes =
            hex::decode(&self.public_key_hex).context("Invalid public key hex")?;

        let public_key_array: [u8; 32] = public_key_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Public key must be 32 bytes"))?;

        let verifying_key =
            VerifyingKey::from_bytes(&public_key_array).context("Invalid public key")?;

        let sig_hex_str =
            std::str::from_utf8(signature_hex).context("Signature is not valid UTF-8")?.trim();

        let sig_bytes = hex::decode(sig_hex_str).context("Invalid signature hex")?;

        let sig_array: [u8; 64] =
            sig_bytes.try_into().map_err(|_| anyhow::anyhow!("Signature must be 64 bytes"))?;

        let signature = Signature::from_bytes(&sig_array);

        verifying_key.verify(data, &signature).context("Signature verification failed")?;

        Ok(())
    }
}

impl ReleaseProvider for ServerProvider {
    fn fetch_latest_version(&self, client: &reqwest::blocking::Client) -> Result<String> {
        let url = format!("{}/{}/latest", self.base_url, self.product);
        let response =
            client.get(&url).send().with_context(|| format!("Failed to connect to {}", url))?;

        if !response.status().is_success() {
            bail!("Failed to get latest version: HTTP {}", response.status());
        }

        Ok(response.text()?.trim().to_string())
    }

    fn fetch_manifest(
        &self,
        client: &reqwest::blocking::Client,
        version: &str,
    ) -> Result<Manifest> {
        let m = messages();

        eprint!("{}", m.fetching_manifest);
        let manifest_url = format!("{}/{}/{}/manifest.json", self.base_url, self.product, version);
        let manifest_response = client
            .get(&manifest_url)
            .send()
            .with_context(|| format!("Failed to fetch manifest from {}", manifest_url))?;

        if !manifest_response.status().is_success() {
            eprintln!("{}", m.failed);
            bail!("Failed to fetch manifest: HTTP {}", manifest_response.status());
        }

        let manifest_bytes = manifest_response.bytes()?;
        eprintln!("{}", m.ok);

        eprint!("{}", m.fetching_signature);
        let sig_url = format!("{}/{}/{}/manifest.sig", self.base_url, self.product, version);
        let sig_response = client
            .get(&sig_url)
            .send()
            .with_context(|| format!("Failed to fetch signature from {}", sig_url))?;

        if !sig_response.status().is_success() {
            eprintln!("{}", m.failed);
            bail!("Failed to fetch signature: HTTP {}", sig_response.status());
        }

        let sig_bytes = sig_response.bytes()?;
        eprintln!("{}", m.ok);

        self.verify_signature(&manifest_bytes, &sig_bytes)?;

        let manifest: Manifest =
            serde_json::from_slice(&manifest_bytes).context("Failed to parse manifest.json")?;

        if manifest.version != version {
            bail!("Version mismatch in manifest: expected {}, got {}", version, manifest.version);
        }

        Ok(manifest)
    }

    fn download_url(&self, version: &str, file_name: &str) -> String {
        format!("{}/{}/{}/{}", self.base_url, self.product, version, file_name)
    }
}
