# agent-doc Functional Specification

> Language-independent specification for the agent-doc interactive document session tool.
> This document captures the exact behavior a port must reproduce.

Individual specs are in `specs/`. This file is the index.

| # | File | Description |
|---|------|-------------|
| 1 | [Overview](specs/01-overview.md) | What agent-doc does and how sessions work |
| 2 | [Document Format](specs/02-document-format.md) | Frontmatter fields, components, and template structure |
| 3 | [Snapshot System](specs/03-snapshot-system.md) | Snapshot storage, lifecycle, and diff baseline |
| 4 | [Diff Computation](specs/04-diff-computation.md) | Line-level unified diff and comment stripping |
| 5 | [Agent Backend](specs/05-agent-backend.md) | Agent trait, resolution order, Claude backend |
| 6 | [Config](specs/06-config.md) | Global/project config, IPC, document state model |
| 7 | [Commands](specs/07-commands.md) | All CLI commands (run, init, route, sync, write, etc.) |
| 8 | [Session Routing](specs/08-session-routing.md) | Registry, claim semantics, stash routing, binding invariant |
| 9 | [Git Integration](specs/09-git-integration.md) | Commit/branch/squash and hook system |
| 10 | [Security](specs/10-security.md) | Threat model, known risks, recommendations |
| 11 | [Debounce](specs/11-debounce.md) | Debounce system gaps, limitations, and improvements |
| 12 | [Codex Support](specs/codex-support.md) | Harness-specific differences for Codex vs Claude Code |
