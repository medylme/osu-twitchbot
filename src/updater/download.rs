use std::path::Path;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::core::UpdateError;

pub async fn download_file<F>(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    total_size: u64,
    mut on_progress: F,
) -> Result<(), UpdateError>
where
    F: FnMut(f32),
{
    let response = client
        .get(url)
        .header("User-Agent", format!("osu-twitchbot/{}", env!("CARGO_PKG_VERSION")))
        .send()
        .await?
        .error_for_status()?;

    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        if total_size > 0 {
            on_progress(downloaded as f32 / total_size as f32);
        }
    }

    file.flush().await?;
    Ok(())
}

pub fn parse_checksum_file(content: &str, filename: &str) -> Option<String> {
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == filename {
            return Some(parts[0].to_lowercase());
        }
    }
    None
}

pub async fn calculate_sha256(path: &Path) -> Result<String, UpdateError> {
    let bytes = tokio::fs::read(path).await?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

pub async fn verify_checksum(file_path: &Path, expected_hash: &str) -> Result<bool, UpdateError> {
    let actual = calculate_sha256(file_path).await?;
    Ok(actual.to_lowercase() == expected_hash.to_lowercase())
}
