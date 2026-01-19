use std::sync::OnceLock;

use serde::Deserialize;
use thiserror::Error;

use crate::VERSION;

const GITHUB_LATEST_RELEASE_URL: Option<&str> = option_env!("GITHUB_LATEST_RELEASE_URL");
static AUTO_UPDATE_ENABLED: OnceLock<bool> = OnceLock::new();

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum UpdateError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Failed to parse version: {0}")]
    VersionParse(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Checksum not found for binary")]
    ChecksumNotFound,

    #[error("Checksum verification failed")]
    ChecksumMismatch,

    #[error("No binary available for this platform")]
    UnsupportedPlatform,

    #[error("No releases found")]
    NoReleases,

    #[error("Failed to restart: {0}")]
    Restart(String),

    #[error("User declined update")]
    UserDeclined,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ReleaseInfo {
    pub version: semver::Version,
    pub tag_name: String,
    pub binary_url: String,
    pub binary_name: String,
    pub checksum_url: Option<String>,
    pub checksum_name: Option<String>,
    pub size: u64,
}

pub fn parse_version(version_str: &str) -> Result<semver::Version, UpdateError> {
    let clean = version_str.strip_prefix('v').unwrap_or(version_str);
    semver::Version::parse(clean).map_err(|e| UpdateError::VersionParse(e.to_string()))
}

pub fn current_version() -> Result<semver::Version, UpdateError> {
    parse_version(VERSION)
}

pub async fn check_for_updates(
    client: &reqwest::Client,
) -> Result<Option<ReleaseInfo>, UpdateError> {
    let Some(url) = GITHUB_LATEST_RELEASE_URL else {
        return Ok(None);
    };

    let response = client
        .get(url)
        .header("User-Agent", format!("osu-twitchbot/{}", VERSION))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    let release: GitHubRelease = response.error_for_status()?.json().await?;

    if release.draft {
        return Ok(None);
    }

    let remote_version = parse_version(&release.tag_name)?;
    let current = current_version()?;

    if remote_version <= current {
        return Ok(None);
    }

    let binary_asset = get_platform_asset(&release).ok_or(UpdateError::UnsupportedPlatform)?;
    let checksum_asset = get_checksum_asset(&release, &binary_asset.name);

    Ok(Some(ReleaseInfo {
        version: remote_version,
        tag_name: release.tag_name.clone(),
        binary_url: binary_asset.browser_download_url.clone(),
        binary_name: binary_asset.name.clone(),
        checksum_url: checksum_asset.map(|a| a.browser_download_url.clone()),
        checksum_name: checksum_asset.map(|a| a.name.clone()),
        size: binary_asset.size,
    }))
}

fn get_platform_asset(release: &GitHubRelease) -> Option<&GitHubAsset> {
    let suffix = if cfg!(target_os = "windows") {
        "windows-x86_64.exe"
    } else if cfg!(target_os = "linux") {
        "linux-x86_64"
    } else {
        return None;
    };

    release.assets.iter().find(|a| a.name.ends_with(suffix))
}

fn get_checksum_asset<'a>(
    release: &'a GitHubRelease,
    binary_name: &str,
) -> Option<&'a GitHubAsset> {
    let checksum_name = format!("{}.sha256", binary_name);
    release.assets.iter().find(|a| a.name == checksum_name)
}

pub fn is_auto_update_enabled() -> bool {
    GITHUB_LATEST_RELEASE_URL.is_some() && AUTO_UPDATE_ENABLED.get().copied().unwrap_or(true)
}

pub fn set_auto_update_enabled(enabled: bool) {
    let _ = AUTO_UPDATE_ENABLED.set(enabled);
}
