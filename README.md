# TeaOps Agent Supervisor (teaops-agentd)

A tiny, dependency-light supervisor for the
[TeaOps agent](https://github.com/simonsmithmd/Teaops-agent).

## Why

The agent self-updates by downloading a new binary, replacing its own
executable, and exiting — it then needs to be relaunched. Relying on systemd's
`Restart=always` doesn't work in Docker or permission-constrained environments.
`teaops-agentd` keeps the agent running everywhere, without root or systemd.

If the agent binary is **missing** (never downloaded or deleted), agentd fetches
it before spawning — **GitHub-first** (parses the upstream `releases.atom`,
downloads the latest release asset), falling back to the download/CDN service.

## Mutual (bidirectional) supervision

- **agentd → agent**: agentd spawns the agent as a child and restarts it
  whenever it exits (including after a self-update).
- **agent → agentd**: the agent watches agentd's heartbeat and relaunches it if
  it dies (the agent's `guardian`, enabled with `TEAOPS_GUARD_AGENTD=1`).

Both sides are made safe against the classic "two supervisors fighting"
problem:

- A per-role **flock** ensures only one agentd runs at a time.
- pid + heartbeat files let each side detect the other's liveness.
- An **adoption protocol**: when (re)started, if a live agent is already
  present, agentd monitors it instead of spawning a duplicate. The agent's
  relaunch of agentd is guarded by a lock and re-checks liveness after locking.

## Run

```bash
cp .env.example .env
# Put the agent (and this binary) somewhere and point at them:
TEAOPS_AGENT_BIN=./teaops-agent ./teaops-agentd
```

Starting `teaops-agentd` is enough — it launches and supervises the agent. You
do not need a separate systemd unit for the agent.

## Configuration (env)

| Var | Default | Notes |
|-----|---------|-------|
| `TEAOPS_AGENT_BIN` | `./teaops-agent` | agent binary to supervise |
| `TEAOPS_RUNTIME_DIR` | `.` | shared dir for pid/heartbeat/lock files |
| `TEAOPS_HEARTBEAT_TIMEOUT_SECS` | `15` | staleness threshold for liveness |
| `TEAOPS_SUPERVISE_INTERVAL_SECS` | `3` | supervision/heartbeat tick |
| `TEAOPS_RESTART_BACKOFF_SECS` | `2` | min delay between agent restarts |
| `TEAOPS_AGENT_REPO` | `simonsmithmd/Teaops-agent` | upstream repo for fetching a missing agent binary |
| `TEAOPS_DOWNLOAD_URL` | `https://download.agent.dn7.cn` | download/CDN fallback source |

The agent must share the same `TEAOPS_RUNTIME_DIR` and, to guard agentd back,
be run with `TEAOPS_GUARD_AGENTD=1` and `TEAOPS_AGENTD_BIN` pointing at this
binary. agentd sets neither for its child by default — enable mutual guarding
explicitly if you want it.

## Deployment

### systemd (when available)

Supervise **agentd** (not the agent); agentd handles the agent.

```ini
# /etc/systemd/system/teaops-agentd.service
[Unit]
Description=TeaOps Agent Supervisor
After=network-online.target

[Service]
WorkingDirectory=/var/lib/teaops
Environment=TEAOPS_AGENT_BIN=/usr/local/bin/teaops-agent
Environment=TEAOPS_RUNTIME_DIR=/var/lib/teaops
Environment=TEAOPS_BACKEND_URL=https://wxapi.dn7.cn
ExecStart=/usr/local/bin/teaops-agentd
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### Docker / no systemd

Make `teaops-agentd` the container entrypoint (PID 1). It keeps the agent alive
across self-updates without any external service manager.

```dockerfile
COPY teaops-agentd teaops-agent /app/
WORKDIR /app
ENV TEAOPS_AGENT_BIN=/app/teaops-agent TEAOPS_RUNTIME_DIR=/app
ENTRYPOINT ["/app/teaops-agentd"]
```

## License

**Proprietary — All Rights Reserved.** See [LICENSE](./LICENSE).
