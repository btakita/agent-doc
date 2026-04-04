---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.31.5 released. Commit-on-claim, auto-setup, VCS refresh, --diff-only, skill runbooks, .ai/runbooks convention.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. agent-doc v0.31.9 released (all targets). `make check` now includes version-sync (Cargo.toml vs pyproject.toml). tmux-router sync.rs debug logging committed, release skipped. Feedback memory: always check git status + git log for submodule state. Memory/runbook layering confirmed.*
<!-- agent:boundary:9f5a5f4c -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Release agent-doc v0.31.9
- [x] Add version sync check to `make check`
<!-- /agent:pending -->
