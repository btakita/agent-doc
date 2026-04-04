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
### Re: GitHub Actions failures — both expected

Both are **already-exists conflicts** from publishing manually before CI ran:

1. **Release workflow** (`23982571705`): `a release with the same tag name already exists: v0.31.9` — we created the release via `gh release create` before CI.
2. **PyPI workflow** (`23982606380`): `Trusted publishing exchange failure` — v0.31.9 was already uploaded to PyPI manually.

**No action needed.** Both artifacts were published successfully. CI fails on the duplicate because we published manually first.

**To prevent:** Either let CI handle the full release (don't publish manually) or add `skip_existing` to the workflows.
Let's have GH handle releases by default. But in special cases where we release locally, add `skip_existing`. Add to the relevant runbooks.
### Re: CI-first releases with skip_existing

Updated both CI workflows and the prerelease runbook:

**release.yml** — checks if release exists first, uploads assets with `--clobber` if it does
**pypi.yml** — added `skip-existing: true` to PyPI publish step
**prerelease runbook** — updated github-release section: CI handles releases by default (push tag), local release is the edge case

Committed: `6eedc63` — `fix: make CI release workflows idempotent`

**New default flow:**
1. Bump version, commit, tag, push tag
2. CI builds cross-platform binaries + creates GitHub release + publishes to PyPI
3. `cargo install --path .` locally for the dev binary

**Local override:** If you publish manually first, CI gracefully skips the duplicate.
Is the local override in a SKILL->runbook?
or is it deterministic binary/makefile?
### Re: Local override location

Not in SKILL — release procedures aren't a document session concern. It's in `.agent/runbooks/prerelease.md` (already updated), linked from CLAUDE.md's Runbooks section.

The path: **CLAUDE.md** → `.agent/runbooks/prerelease.md` → Target: github-release section (CI-first default + local override).

SKILL.md only covers the `/agent-doc` document session workflow. Release procedures are project-level runbooks.
### Re: Deterministic release via Makefile (HEAD)

The Makefile already has `publish`/`publish-crate`/`publish-pypi` (local publish). Could add a CI-first `release` target:

```makefile
release: check
	@version=$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/'); \
	git tag "v$$version" && git push origin main "v$$version"
	@echo "Tag pushed. CI handles GitHub Release + PyPI."
	cargo install --path .
```

**Both have a role:** Makefile automates the "how" (deterministic). Runbook documents the "why" and edge cases (when to use local vs CI, what to check first).

**Proposal:** Add `make release` (CI-first, default) alongside existing `make publish` (local, edge case). The runbook references both.
<!-- agent:boundary:a464b7fe -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Release agent-doc v0.31.9
- [x] Add version sync check to `make check`
- [x] Make CI release workflows idempotent
- [ ] Add `make release` target (CI-first flow)
<!-- /agent:pending -->
