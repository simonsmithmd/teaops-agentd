use std::env;
use std::path::PathBuf;

/// Supervisor configuration loaded from the environment.
#[derive(Clone, Debug)]
pub struct DaemonConfig {
    /// Path to the teaops-agent binary to supervise.
    pub agent_bin: PathBuf,
    /// Shared runtime directory for pid/heartbeat/lock files.
    pub runtime_dir: PathBuf,
    /// Seconds without a heartbeat before a peer is considered dead.
    pub heartbeat_timeout_secs: u64,
    /// How often the supervisor checks the agent (seconds).
    pub supervise_interval_secs: u64,
    /// Minimum delay between agent restarts (seconds).
    pub restart_backoff_secs: u64,
    /// Download/CDN service base URL (fallback binary source).
    pub download_url: String,
    /// Upstream agent repo (`owner/name`) for GitHub-first downloads.
    pub agent_repo: String,
}

impl DaemonConfig {
    pub fn from_env() -> Self {
        let agent_bin = env::var("TEAOPS_AGENT_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./teaops-agent"));
        let runtime_dir = env::var("TEAOPS_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let heartbeat_timeout_secs = env::var("TEAOPS_HEARTBEAT_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15);
        let supervise_interval_secs = env::var("TEAOPS_SUPERVISE_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);
        let restart_backoff_secs = env::var("TEAOPS_RESTART_BACKOFF_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);
        let download_url = env::var("TEAOPS_DOWNLOAD_URL")
            .unwrap_or_else(|_| "https://download.agent.dn7.cn".to_string())
            .trim_end_matches('/')
            .to_string();
        let agent_repo = env::var("TEAOPS_AGENT_REPO")
            .unwrap_or_else(|_| "simonsmithmd/Teaops-agent".to_string());

        DaemonConfig {
            agent_bin,
            runtime_dir,
            heartbeat_timeout_secs,
            supervise_interval_secs,
            restart_backoff_secs,
            download_url,
            agent_repo,
        }
    }
}
