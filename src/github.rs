use std::path::Path;

use anyhow::{Context, ensure};
use md5::Md5;
use reqwest::header::ACCEPT;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{fs, io::AsyncWriteExt};

const API: &str = "https://api.github.com";

#[derive(Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub body: Option<String>,
    pub published_at: Option<String>,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
pub struct Asset {
    pub id: i64,
    pub name: String,
    pub size: i64,
    /// `sha256:<hex>`; GitHub fills it for assets uploaded since mid-2025, older ones have none.
    pub digest: Option<String>,
}

impl Asset {
    /// The hex SHA-256 GitHub computed, if it did.
    pub fn sha256(&self) -> Option<&str> {
        self.digest.as_deref()?.strip_prefix("sha256:")
    }
}

pub struct GitHub {
    pub http: reqwest::Client,
    /// Fine-grained token with "Contents: read" on the private repos; none for public ones.
    pub token: Option<String>,
}

impl GitHub {
    fn get(&self, url: String, accept: &str) -> reqwest::RequestBuilder {
        let req = self
            .http
            .get(url)
            .header(ACCEPT, accept)
            .header("X-GitHub-Api-Version", "2022-11-28");
        match &self.token {
            Some(token) => req.bearer_auth(token),
            None => req,
        }
    }

    /// The 30 newest releases; older ones are not offered to devices.
    pub async fn releases(&self, repo: &str) -> anyhow::Result<Vec<Release>> {
        Ok(self
            .get(
                format!("{API}/repos/{repo}/releases?per_page=30"),
                "application/vnd.github+json",
            )
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Streams an asset to `dest` (through a `.part` file, so a cut download never looks
    /// cached) and returns its hex SHA-256 and MD5.
    pub async fn download(
        &self,
        repo: &str,
        asset_id: i64,
        size: i64,
        dest: &Path,
    ) -> anyhow::Result<(String, String)> {
        let part = dest.with_extension("part");
        let result = self.download_to(repo, asset_id, size, &part).await;
        match result {
            Ok(hashes) => {
                fs::rename(&part, dest).await?;
                Ok(hashes)
            }
            Err(e) => {
                let _ = fs::remove_file(&part).await;
                Err(e)
            }
        }
    }

    async fn download_to(
        &self,
        repo: &str,
        asset_id: i64,
        size: i64,
        part: &Path,
    ) -> anyhow::Result<(String, String)> {
        // GitHub redirects to a signed storage URL; reqwest drops the token on that hop.
        let res = self
            .get(
                format!("{API}/repos/{repo}/releases/assets/{asset_id}"),
                "application/octet-stream",
            )
            .send()
            .await?
            .error_for_status()?;
        save(res, size, part).await
    }
}

/// Writes the body to `part`, refusing it as soon as it outgrows `size`, so a wrong or hostile
/// response cannot fill the disk.
async fn save(
    mut res: reqwest::Response,
    size: i64,
    part: &Path,
) -> anyhow::Result<(String, String)> {
    let mut file = fs::File::create(part)
        .await
        .with_context(|| format!("creating {}", part.display()))?;
    let (mut sha, mut md5, mut written) = (Sha256::new(), Md5::new(), 0i64);
    while let Some(chunk) = res.chunk().await? {
        written += chunk.len() as i64;
        ensure!(written <= size, "got more than the expected {size} bytes");
        sha.update(&chunk);
        md5.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.sync_all().await?;
    ensure!(written == size, "got {written} bytes, expected {size}");
    Ok((hex::encode(sha.finalize()), hex::encode(md5.finalize())))
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn save_stops_at_the_expected_size() {
        let part = std::env::temp_dir().join(format!(
            "beacon-save-{}.part",
            hex::encode(rand::random::<[u8; 8]>())
        ));
        let res = |body: &'static [u8]| reqwest::Response::from(axum::http::Response::new(body));
        assert!(super::save(res(b"0123456789"), 5, &part).await.is_err());
        // Refused before anything was written.
        assert_eq!(std::fs::metadata(&part).unwrap().len(), 0);
        assert!(super::save(res(b"012"), 5, &part).await.is_err());
        let (sha, _) = super::save(res(b"hello"), 5, &part).await.unwrap();
        assert!(sha.starts_with("2cf24dba"));
        std::fs::remove_file(part).unwrap();
    }
}
