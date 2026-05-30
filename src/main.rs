//! teaops-agentd — a tiny local supervisor for teaops-agent.
//!
//! Why: the agent self-updates by replacing its own binary and exiting; it then
//! needs to be relaunched. Relying on systemd doesn't work in Docker or
//! permission-constrained environments. agentd is a dependency-light supervisor
//! that keeps the agent running everywhere.
//!
//! Safe bidirectional supervision:
//! - agentd supervises agent (spawn + restart on exit).
//! - the agent watches agentd's heartbeat and relaunches it (under a file lock)
//!   if it dies — see the agent's `guardian` module.
//!
//! Duplicate protection:
//! - a per-role flock ensures only one agentd runs.
//! - on startup, if a fresh agent heartbeat is found, agentd adopts the running
//!   agent (monitor-only) instead of spawning a second one.

mod config;
mod fetch;
mod procfile;

use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use tokio::process::{Child, Command};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::DaemonConfig;
use crate::procfile::{
    heartbeat_fresh, pid_alive, read_pid, try_lock, write_heartbeat, write_pid, RolePaths,
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,teaops_agentd=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cfg = DaemonConfig::from_env();
    std::fs::create_dir_all(&cfg.runtime_dir).ok();

    let me = RolePaths::new(&cfg.runtime_dir, "agentd");
    let agent = RolePaths::new(&cfg.runtime_dir, "agent");

    // Single-instance guard: hold the agentd lock for our whole lifetime.
    let _lock = match try_lock(&me.lock)? {
        Some(g) => g,
        None => {
            tracing::info!("another teaops-agentd is already running; exiting");
            return Ok(());
        }
    };
    write_pid(&me.pid)?;
    write_heartbeat(&me.heartbeat)?;
    tracing::info!(pid = std::process::id(), agent_bin = ?cfg.agent_bin, "teaops-agentd started");

    // Heartbeat task: keep our heartbeat fresh so the agent's guardian sees us.
    {
        let hb = me.heartbeat.clone();
        let interval = cfg.supervise_interval_secs.max(1);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(interval));
            loop {
                ticker.tick().await;
                let _ = write_heartbeat(&hb);
            }
        });
    }

    // Graceful shutdown on SIGTERM/SIGINT: stop our managed child too.
    let mut child: Option<Child> = None;
    let mut shutdown = signal_stream()?;

    // If an agent is already alive (e.g. started by hand or by the previous
    // agentd), adopt it: don't spawn a duplicate, just monitor until it dies.
    // This only applies at startup; once we own the child, its exit always
    // triggers an immediate respawn.
    if agent_is_alive(&agent, cfg.heartbeat_timeout_secs) {
        tracing::info!("found a live agent on startup; adopting (monitor-only)");
        tokio::select! {
            _ = wait_until_agent_dead(&agent, &cfg) => {}
            _ = shutdown.recv() => {
                tracing::info!("shutdown signal received");
                return Ok(());
            }
        }
    }

    loop {
        // Spawn the agent if we don't have a managed child. Reaching here with
        // no child means either first launch or our child just exited, so we
        // always (re)spawn — no adoption re-check (the just-exited agent's
        // heartbeat may still be within the timeout window).
        if child.is_none() {
            // Make sure the agent binary exists (re-fetch if missing/deleted),
            // GitHub-first with a download-service fallback.
            if let Err(e) = fetch::ensure_agent_binary(&cfg).await {
                tracing::error!("agent binary unavailable: {e}");
                tokio::time::sleep(Duration::from_secs(cfg.restart_backoff_secs.max(1))).await;
                continue;
            }

            match spawn_agent(&cfg) {
                Ok(c) => {
                    tracing::info!(pid = c.id(), "spawned teaops-agent");
                    child = Some(c);
                }
                Err(e) => {
                    tracing::error!("failed to spawn agent: {e}");
                    tokio::time::sleep(Duration::from_secs(cfg.restart_backoff_secs.max(1))).await;
                    continue;
                }
            }
        }

        // Wait for the child to exit or a shutdown signal.
        let c = child.as_mut().unwrap();
        tokio::select! {
            status = c.wait() => {
                match status {
                    Ok(s) => tracing::warn!("agent exited with {s}; restarting"),
                    Err(e) => tracing::warn!("agent wait error: {e}; restarting"),
                }
                child = None;
                // Backoff to avoid a restart storm.
                tokio::time::sleep(Duration::from_secs(cfg.restart_backoff_secs)).await;
            }
            _ = shutdown.recv() => {
                tracing::info!("shutdown signal received; terminating agent");
                let _ = c.start_kill();
                let _ = c.wait().await;
                break;
            }
        }
    }

    Ok(())
}

/// Spawn the agent as a child process, inheriting stdio so its logs show up.
fn spawn_agent(cfg: &DaemonConfig) -> Result<Child> {
    let child = Command::new(&cfg.agent_bin)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(false)
        .spawn()?;
    Ok(child)
}

/// An agent is "alive" if its pid is running or its heartbeat is fresh.
fn agent_is_alive(agent: &RolePaths, timeout: u64) -> bool {
    if let Some(pid) = read_pid(&agent.pid) {
        if pid_alive(pid) {
            return true;
        }
    }
    heartbeat_fresh(&agent.heartbeat, timeout)
}

/// Poll until the (adopted) agent is no longer alive.
async fn wait_until_agent_dead(agent: &RolePaths, cfg: &DaemonConfig) {
    let mut ticker = tokio::time::interval(Duration::from_secs(cfg.supervise_interval_secs.max(1)));
    loop {
        ticker.tick().await;
        if !agent_is_alive(agent, cfg.heartbeat_timeout_secs) {
            tracing::warn!("adopted agent is no longer alive");
            return;
        }
    }
}

/// Combined SIGTERM/SIGINT receiver.
fn signal_stream() -> Result<tokio::sync::mpsc::Receiver<()>> {
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    tokio::spawn(async move {
        loop {
            let send: Result<(), _> = tokio::select! {
                _ = term.recv() => tx.send(()).await,
                _ = int.recv() => tx.send(()).await,
            };
            if send.is_err() {
                break;
            }
        }
    });
    Ok(rx)
}
