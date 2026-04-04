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
*Compacted. Features: commit-on-claim, auto-setup untracked files, --diff-only flag, skill runbook bundling. Conventions: `.ai/runbooks/` (project root, vendor-neutral), `runbooks/` (skill-scoped). v0.31.5 released (cargo + GitHub). Open: PyPI manylinux build, `.ai/runbooks` blog post.*
<!-- agent:boundary:a4e6c722 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Release v0.31.5
- [ ] PyPI publish (needs manylinux Docker build)
<!-- /agent:pending -->
