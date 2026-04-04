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

gh action failures
- https://github.com/btakita/agent-doc/actions/runs/23982571705
- https://github.com/btakita/agent-doc/actions/runs/23982606380
### Re: GitHub Actions failures — both expected (HEAD)

Both are **already-exists conflicts** from publishing manually before CI ran:

1. **Release workflow** (`23982571705`): `a release with the same tag name already exists: v0.31.9` — we created the release via `gh release create` before CI.
2. **PyPI workflow** (`23982606380`): `Trusted publishing exchange failure` — v0.31.9 was already uploaded to PyPI manually.

**No action needed.** Both artifacts were published successfully. CI fails on the duplicate because we published manually first.

**To prevent:** Either let CI handle the full release (don't publish manually) or add `skip_existing` to the workflows.
<!-- agent:boundary:3b219f41 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Release agent-doc v0.31.9
- [x] Add version sync check to `make check`
<!-- /agent:pending -->
