//! Acquire the agent binary, GitHub-first.
//!
//! If the supervised agent binary is missing (never downloaded or deleted),
//! agentd fetches it before spawning. Order:
//!   1. GitHub: parse `releases.atom` for the highest version, fetch the asset.
//!   2. Fallback: the download/CDN service (`/download/agent/latest`).

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::config::DaemonConfig;

const AGENT_BIN: &str = "teaops-agent";

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("teaops-agentd/fetch")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("http client")
}

/// Ensure the agent binary exists at `cfg.agent_bin`; fetch it if missing.
pub async fn ensure_agent_binary(cfg: &DaemonConfig) -> Result<()> {
    if cfg.agent_bin.exists() {
        return Ok(());
    }
    tracing::warn!(path = ?cfg.agent_bin, "agent binary missing; fetching");
    let bytes = fetch_latest(cfg).await?;
    install_bytes(&bytes, &cfg.agent_bin).await?;
    tracing::info!(path = ?cfg.agent_bin, bytes = bytes.len(), "fetched missing agent binary");
    Ok(())
}

/// Download the latest agent binary into memory, GitHub-first.
async fn fetch_latest(cfg: &DaemonConfig) -> Result<Vec<u8>> {
    match fetch_from_github(cfg).await {
        Ok(b) => {
            tracing::info!("fetched agent binary from GitHub");
            return Ok(b);
        }
        Err(e) => tracing::warn!("GitHub fetch failed ({e}); falling back to download service"),
    }
    let b = fetch_from_download_service(cfg).await?;
    tracing::info!("fetched agent binary from download service");
    Ok(b)
}

async fn fetch_from_github(cfg: &DaemonConfig) -> Result<Vec<u8>> {
    let repo = &cfg.agent_repo;
    let client = http();
    let body = client
        .get(format!("https://github.com/{repo}/releases.atom"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let version =
        highest_version(&body).ok_or_else(|| anyhow!("no version in releases.atom"))?;
    let asset = format!("{AGENT_BIN}-linux-x86_64-v{version}");
    let url = format!("https://github.com/{repo}/releases/download/v{version}/{asset}");
    let bytes = client.get(&url).send().await?.error_for_status()?.bytes().await?;
    if bytes.is_empty() {
        return Err(anyhow!("downloaded asset is empty"));
    }
    Ok(bytes.to_vec())
}

async fn fetch_from_download_service(cfg: &DaemonConfig) -> Result<Vec<u8>> {
    let url = format!("{}/download/agent/latest", cfg.download_url);
    let bytes = http().get(&url).send().await?.error_for_status()?.bytes().await?;
    if bytes.is_empty() {
        return Err(anyhow!("download service returned empty body"));
    }
    Ok(bytes.to_vec())
}

/// Write `bytes` to `target` atomically with executable permissions.
async fn install_bytes(bytes: &[u8], target: &Path) -> Result<()> {
    if bytes.is_empty() {
        return Err(anyhow!("refusing to install empty binary"));
    }
    let dir = target
        .parent()
        .ok_or_else(|| anyhow!("target has no parent dir"))?;
    tokio::fs::create_dir_all(dir).await.ok();
    let tmp = dir.join(format!(
        ".{}.dl",
        target.file_name().and_then(|n| n.to_str()).unwrap_or("bin")
    ));
    tokio::fs::write(&tmp, bytes).await.context("write temp binary")?;
    tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .await
        .context("chmod temp binary")?;
    tokio::fs::rename(&tmp, target).await.context("install binary")?;
    Ok(())
}

/// Find the highest `1.0.x`-style version in a releases Atom feed.
fn highest_version(xml: &str) -> Option<String> {
    let mut best: Option<((u64, u64, u64), String)> = None;
    for raw in xml.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
        if !raw.starts_with('v') {
            continue;
        }
        let v = raw.trim_start_matches('v');
        if let Some(parsed) = parse_ver(v) {
            if best.as_ref().map(|(b, _)| parsed > *b).unwrap_or(true) {
                best = Some((parsed, v.to_string()));
            }
        }
    }
    best.map(|(_, v)| v)
}

fn parse_ver(s: &str) -> Option<(u64, u64, u64)> {
    let mut it = s.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next().unwrap_or("0").parse().ok()?;
    Some((a, b, c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_highest() {
        let xml = "x/v1.0.3 y v1.0.10 z v1.0.2";
        assert_eq!(highest_version(xml).as_deref(), Some("1.0.10"));
    }
}
