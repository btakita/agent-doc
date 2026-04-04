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
*Compacted. CI workflows idempotent (release.yml --clobber, pypi.yml skip-existing). `make release` added (CI-first: check→tag→push). Prerelease runbook updated. Gitignore fixed (.bin/ .cargo/ separate). All pushed.*

The jetbrains editor has performance issues navigating and editing tasks/agent-doc/agent-doc-bugs.md. Please look into why.
### Re: JetBrains performance with agent-doc-bugs.md

**Not the file** (125 lines / 6.5KB). It's the git repo state:

1. **Orphaned 3.9 GB blob** in git history — cleaned up via `git gc --prune=now --aggressive` (4.2 GB → 2.4 GB, remaining is submodule data)
2. **7,691 total commits**, 208 for this file — frequent `agent-doc commit` calls
3. **`git status` ~300ms** — JetBrains runs this + annotations on every VCS refresh signal

Each `/agent-doc` cycle triggers commit + VCS refresh → JetBrains re-indexes. With 7K+ commits, annotation/blame/log operations are heavier.

**Mitigations:**
- GC done (removed 1.8 GB orphaned blob)
- `agent-doc clean` could squash old task commits to reduce history depth
- JetBrains plugin could throttle VCS refresh signals
- Disable git annotations for `tasks/` directory in IDE settings

> - `agent-doc clean` could squash old task commits to reduce history depth

I would want to archive the repo with the old history. Are there options to do this?

> - Disable git annotations for `tasks/` directory in IDE settings

Wa can do this.
### Re: Archiving history before squash (HEAD)

Options:
1. **Archive branch**: `git branch archive/pre-squash` → preserves full history, push to remote for backup
2. **Tag**: `git tag archive/pre-squash-2026-04-04` — lighter, same effect
3. **Separate bare clone**: `git clone --bare . ~/archive/agent-loop-full-history.git`
4. **`agent-doc clean`** squashes only agent-doc task commits (not all), single squash commit remains

**Recommended:** Tag pre-squash + squash. Tag costs nothing, preserves full history if needed.

### Re: Disabling git annotations for tasks/ (HEAD)

JetBrains options:
- Per-file: right-click gutter → Annotate → uncheck
- Directory: Settings → Version Control → Directory Mappings (heavy-handed)
- **Better: throttle VCS refresh in the agent-doc plugin** — the plugin fires `VirtualFileManager.getInstance().asyncRefresh()` on every commit. Could skip or debounce for task documents.
<!-- agent:boundary:8474bb2e -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] GC orphaned 3.9 GB blob from git history
- [ ] Archive history + squash old task commits
- [ ] Throttle VCS refresh in JetBrains plugin for task documents
<!-- /agent:pending -->
