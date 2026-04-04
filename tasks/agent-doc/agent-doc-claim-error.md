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

any pending releases for?
- tmux-router
- agent-doc
### Re: Pending releases (HEAD)

**tmux-router v0.3.9:** No pending release. HEAD matches latest tag — no unreleased commits.

**agent-doc v0.31.8:** Has uncommitted submodule changes from this session:
- `SKILL.md` — added Runbooks section
- `runbooks/compact-exchange.md` — unanswered input note
- `runbooks/transfer-extract.md` — new file
- `src/skill.rs` — added transfer-extract to BUNDLED_RUNBOOKS

Plus 3 committed-but-unreleased housekeeping commits (gitignore, clippy, remove accidental files).

**To release agent-doc:** Commit the submodule changes, bump version, follow `.agent/runbooks/prerelease.md`.
<!-- agent:boundary:e4403bd2 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Commit agent-doc submodule changes (runbook updates, skill.rs)
- [ ] Release agent-doc (version TBD)
<!-- /agent:pending -->
