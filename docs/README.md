# Documentation README

This is the entry point for agent-doc documentation, specs, and product-planning
material.

## Start Here

- [README](../README.md) - project overview, install commands, quick start,
  architecture summary, editor setup, and security model.
- [Introduction](introduction.md) - concise user-facing introduction.
- [Quick Start](getting-started/quick-start.md) - shortest path from a markdown
  file to an agent-doc cycle.
- [Installation](getting-started/installation.md) - install paths for the CLI
  and editor integrations.
- [Configuration](getting-started/configuration.md) - project and document
  configuration.

## Guides

- [Commands](guide/commands.md) - user-facing command guide.
- [Document Format](guide/document-format.md) - session frontmatter and
  component layout.
- [Components](guide/components.md) - named component patching model.
- [Dashboard-as-Document](guide/dashboard.md) - dashboard workflow.
- [Run Flow](guide/submit-flow.md) - normal edit, diff, response, write, and
  commit flow.
- [Editor Integration](guide/editor-integration.md) - JetBrains, VS Code, and
  other editor integration notes.
- [Agent Backends](guide/agent-backends.md) - Claude, Codex, OpenCode, and
  direct harness behavior.

## Reference

- [Specification](reference/specs.md) - includes the root functional
  specification.
- [Flow Map](reference/flow-map.md) - FlowCore ownership map and typed event
  migration plan.
- [Active Turn Lifecycle And Replay Paths](reference/active-turn-lifecycle-and-replay.md)
  - generated diagrams for active turns, route readiness, prompt ownership,
  stale cache/conflict replay, and late fallback replay.
- [IPC](reference/ipc.md) - editor IPC architecture and fallback behavior.
- [Full-Document IPC Corruption Chain](reference/full-document-ipc-corruption-chain.md)
  - separate Mermaid logic chain for repeated full-document IPC corruption and
  the end-to-end disabled path.
- [Prompt Duplicate Closeout Repair](reference/prompt-duplicate-closeout-repair.md)
  - Mermaid process diagram for the disabled full-content IPC path and
  duplicate-prompt closeout repair.
- [Race Condition Analysis](reference/race-conditions.md) - concurrency hazards
  and mitigations.
- [Reactive Stream](reference/reactive-stream.md) - reactive stream notes.
- [Changelog](reference/changelog.md) - version history.

## Specs

The canonical spec entry point is [SPEC.md](../SPEC.md). Split specs live under
[`specs/`](../specs/) for focused review:

- [Overview](../specs/01-overview.md)
- [Document Format](../specs/02-document-format.md)
- [Snapshot System](../specs/03-snapshot-system.md)
- [Diff Computation](../specs/04-diff-computation.md)
- [Agent Backend](../specs/05-agent-backend.md)
- [Config](../specs/06-config.md)
- [Commands](../specs/07-commands.md)
- [Core Commands](../specs/07-core-commands.md)
- [Closeout Commands](../specs/07-closeout-commands.md)
- [Orchestration Commands](../specs/07-orchestration-commands.md)
- [Session Tmux Commands](../specs/07-session-tmux-commands.md)
- [Session Routing](../specs/08-session-routing.md)
- [Session Actor Contract](../specs/08a-session-actor-contract.md)
- [Git Integration](../specs/09-git-integration.md)
- [Security](../specs/10-security.md)
- [Debounce](../specs/11-debounce.md)
- [Deterministic Simulation](../specs/12-deterministic-simulation.md)
- [Pending System](../specs/pending-system.md)
- [Supervisor](../specs/supervisor.md)
- [Codex Support](../specs/codex-support.md)

## Development

- [Building](development/building.md)
- [Conventions](development/conventions.md)
- [Editor specs](../editors/SPEC.md)
- [VS Code README](../editors/vscode/README.md)
- [JetBrains spec](../editors/jetbrains/SPEC.md)

## Examples And Ontology

- [Example session docs](../examples/agent-doc/README.md)
- [Ontology README](../ontology/README.md)

## PRDs

No PRD or product-requirements documents are currently present in this repo.
When PRDs are added, link them from this section and keep this README as the
documentation entry point.
