# agent-doc Functional Specification

> Language-independent specification for the agent-doc interactive document session tool.
> This document captures the exact behavior a port must reproduce.

Individual specs are in `specs/`. This file is the index.

Notable invariants:
- `agent-doc commit` remains snapshot-selective. It may repair narrowly-classified missed agent-owned drift before staging, but it must not absorb free-form user prompts from the working tree.
- Extreme snapshot/file drift does not relax that rule for tracked documents. Wholesale snapshot re-sync from the live file is reserved for bootstrap scaffold snapshots on files with no `HEAD` entry yet; tracked documents stay selective so unanswered prompts cannot be committed during preflight.
- Post-commit cleanup keeps the committed blob clean but preserves a single visible ` (HEAD)` marker on the current response heading in the snapshot and user-facing document state as the current-response affordance. When no live editor IPC listener exists, the CLI performs that working-tree rewrite itself instead of leaving stale boundary churn behind.
- Final responses are durably captured before write-back in `.agent-doc/captures/<doc-hash>/<cycle-id>.json`, and interrupted-cycle replay must use that captured response body only when the captured snapshot/file hashes still match the current baseline.
- `agent-doc preflight` treats `response_captured` as an open cycle state, auto-attempts recovery+commit before diffing, and emits the tier/attribution contract the skill consumes: `effective_tier`, `required_tier`, `suggested_tier`, `model_switch`, `model_switch_tier`, and `agent_model`.
- Harness-specific arg aliases are explicit: `agent_args` is generic, `claude_args` applies only to Claude, and `codex_args` applies only to Codex.
- `### Re:` response headers must use the resolved model short name for attribution (for example `gpt-5`, `opus-4-6`), never the harness label (`codex`, `claude`).
- Bundled skill/install content is part of the external contract: Codex/manual-repair instructions must distinguish adding a missing user prompt from repairing a missed assistant response, and the missed-response repair path should use `agent-doc write --commit <file>` rather than direct file patching.
- Route readiness/trigger acceptance is a binary responsibility: pane prompt detection must be robust to shell startup noise and must wait for actual prompt state rather than treating echoed command text as readiness.

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
