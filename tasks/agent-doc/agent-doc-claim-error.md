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
*Compacted. Runbook updates: compact-exchange (unanswered input note), transfer-extract (new, bundled). v0.31.5 PyPI published (manylinux Docker). Security: Docker mount rule in CLAUDE.md, mask `.cargo/config.toml` with `/dev/null`. Prerelease runbook updated with PyPI Docker build. Stash panes cleaned via `resync --fix`. No autokill — warn only.*
<!-- agent:boundary:ba8069ba -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Release v0.31.5
- [x] Update compact-exchange runbook: note about preserving uncommitted user input
- [x] Document `agent-doc transfer`/`extract` in SKILL.md or runbook
- [x] PyPI publish (manylinux Docker build)
- [x] Add Docker mount security rule to CLAUDE.md
- [x] Add PyPI Docker build to prerelease runbook
<!-- /agent:pending -->
