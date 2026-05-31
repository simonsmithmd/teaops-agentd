# teaops-agentd — DEPRECATED / 已废弃

This supervisor has been **merged into [teaops-agent](https://github.com/simonsmithmd/Teaops-agent)**.

`teaops-agent` is now a single self-splitting binary: running it with no
arguments starts the supervisor role, which spawns the agent role (itself, with
the `agent` subcommand) and keeps it alive. There is no separate `teaops-agentd`
binary anymore.

## Migration

Replace any `teaops-agentd` process with the no-arg `teaops-agent`:

```bash
# before:  ./teaops-agentd        (supervised ./teaops-agent)
# after:   ./teaops-agent         (supervisor role; self-splits the agent role)
```

Drop the `TEAOPS_AGENTD_*` / `TEAOPS_GUARD_AGENTD` env vars; see the
teaops-agent README for the current configuration.

This repository is archived and no longer built or released.
