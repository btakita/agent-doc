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
### Re: Pending releases

**tmux-router v0.3.9:** No pending release. HEAD matches latest tag — no unreleased commits.

Not true. Why did you get this wrong?

```
diff --git a/src/sync.rs b/src/sync.rs
index 9eadcc8..a3d55c0 100644
--- a/src/sync.rs
+++ b/src/sync.rs
@@ -696,11 +696,12 @@ pub fn sync_with_options(
             && let Ok(all_panes) = tmux.list_panes_ordered(w)
             && let Some(first) = all_panes.first()
             && let Ok(s) = tmux.pane_session(first) {
+                eprintln!("[sync] stashing unmanaged panes: window={}, panes={:?}, session={}", w, all_panes, s);
                 for pane in all_panes.iter().rev() {
                     // Never stash the last pane — tmux closes the window
                     let remaining = tmux.list_window_panes(w).unwrap_or_default().len();
                     if remaining <= 1 {
-                        eprintln!("[sync] skipped stashing {} — last pane in window", pane);
+                        eprintln!("[sync] skipped stashing {} — last pane in window (remaining={})", pane, remaining);
                         break;
                     }
                     if let Some(ref protect) = options.protect_pane {
```

**agent-doc v0.31.8:** Has uncommitted submodule changes from this session:
- `SKILL.md` — added Runbooks section
- `runbooks/compact-exchange.md` — unanswered input note
- `runbooks/transfer-extract.md` — new file
- `src/skill.rs` — added transfer-extract to BUNDLED_RUNBOOKS

Plus 3 committed-but-unreleased housekeeping commits (gitignore, clippy, remove accidental files).

**To release agent-doc:** Commit the submodule changes, bump version, follow `.agent/runbooks/prerelease.md`.
### Re: tmux-router correction — missed working tree changes

You're right. I only ran `git log v0.3.9..HEAD` (no committed changes since tag) but missed `git diff` / `git status` which shows uncommitted modifications to `src/sync.rs`:
- Added `[sync] stashing unmanaged panes` log line with window/panes/session context
- Enhanced `skipped stashing` message with `remaining` count

**Why I got it wrong:** Checked committed history but not the working tree. Should have run both `git log` and `git status`.

How can we fix this for next time? SKILL + runbook?

**Pending for tmux-router:** Commit the sync.rs debug logging, decide if it warrants a release (v0.3.10) or can wait.
### Re: Preventing missed working tree changes

Not a SKILL or runbook issue — this was a general workflow mistake. The fix is a **feedback memory** so I always check both committed and uncommitted state when assessing submodules.

Added to auto memory (`feedback_check_working_tree.md`):
> When assessing submodule release status, always run `git status` + `git diff` alongside `git log`. Never say "no changes" based on commit history alone.

Also updated the prerelease runbook — but the real fix is the memory, since the mistake happens at the "check status" phase before any release workflow starts.

I wonder if we should have a detailed runbook that is called from memory or the CLAUDE.md.
### Re: Runbooks referenced from memory/CLAUDE.md

This pattern already works — CLAUDE.md's Runbooks section links `.agent/runbooks/*.md` as on-demand procedures. Memory could do the same for edge cases.

**The layered approach already in place:**

| Layer | Cost | Best for |
|-------|------|----------|
| CLAUDE.md / Memory | Always in context | Short rules (1-2 lines) |
| Runbook (linked) | On-demand | Multi-step procedures (4+ steps) |

**For "check submodule status":** The memory entry is sufficient — 2 commands. A runbook would be overkill.

**When runbook > memory:** 4+ steps, conditional logic, platform branches, specific command syntax with edge cases (compact-exchange, transfer-extract, prerelease).

The pattern of "memory says what, runbook says how" is already working. No new mechanism needed — just use the right layer for the complexity.
do
- [ ] Commit tmux-router sync.rs debug logging changes
- [ ] Commit agent-doc submodule changes (runbook updates, skill.rs)
### Re: Submodule changes committed

1. **tmux-router** (`d2219eb`): stash debug logging with window/pane/session context
2. **agent-doc** (`72adf58`): transfer-extract runbook + compact-exchange update + skill.rs bundling + SKILL.md Runbooks section
3. **agent-loop** (`0ff07ae1`): updated both submodule pointers

do
- [ ] Release tmux-router (if warranted — only debug logging)

Did we just skip 4 versions?
```
● Update(pyproject.toml)
  ⎿  Added 1 line, removed 1 line
       4
       5  [project]
       6  name = "agent-doc"
       7 -version = "0.31.5"
       7 +version = "0.31.9"
       8  description = "Interactive document sessions with AI agents"
       9  readme = "README.md"
      10  license = "MIT"
```
### Re: agent-doc v0.31.9 released, tmux-router skipped

**tmux-router:** Release skipped — debug logging only, not user-facing.

**agent-doc v0.31.9 released** (following `.agent/runbooks/prerelease.md`):

| Step | Status |
|------|--------|
| `make check` | 38 tests pass |
| Version bump (Cargo.toml + pyproject.toml) | 0.31.9 |
| VERSIONS.md | Updated |
| `cargo publish` | crates.io |
| PyPI (Docker + `/dev/null` mask) | Uploaded |
| GitHub Release | https://github.com/btakita/agent-doc/releases/tag/v0.31.9 |
| `cargo install --path .` | v0.31.9 in ~/.cargo/bin |
| Parent submodule pointer | Updated |

The `/dev/null` config mask worked perfectly — no tmux-router mount needed.
### Re: pyproject.toml version gap (0.31.5 → 0.31.9)

No PyPI versions were skipped — the gap is a sync issue:

- **Cargo.toml** was bumped incrementally through 0.31.6/7/8 (crates.io only)
- **pyproject.toml** was only updated at 0.31.5 (first PyPI publish) and 0.31.9 (this release)
- Versions 0.31.6-0.31.8 were never published to PyPI — only crates.io + GitHub

The prerelease runbook says "sync pyproject.toml with Cargo.toml" — it should have been bumped at each release regardless of target. Going forward, both files stay in sync on every version bump.

> Going forward, both files stay in sync on every version bump.

How are you ensuring this?
### Re: How am I ensuring pyproject.toml sync?

Currently: memory + runbook step. Both rely on me remembering — which failed for v0.31.6-0.31.8.

**Best enforcement:** Add a version sync check to `make check`. One-liner that compares Cargo.toml and pyproject.toml versions, fails if they differ. Runs before every release and during development.

| Approach | Reliability |
|----------|-------------|
| Memory + runbook (current) | Failed 3 times |
| `make check` version sync | Catches every time |

Want me to add this to the Makefile?
yes...is this `make check` in agent-doc?
### Re: Version sync check added to `make check`

Yes, in agent-doc's Makefile. Added a `version-sync` target that compares Cargo.toml and pyproject.toml versions, fails if they differ. It's now part of `make check` (clippy + test + version-sync).

```
make version-sync   # silent if matched
# ERROR: version mismatch — Cargo.toml=0.31.9 pyproject.toml=0.31.0  (if mismatched) (HEAD)
```

Committed: `4e554d4` — `feat: add version-sync check to make check`

Now `make check` (which the prerelease runbook runs first) will catch any Cargo/pyproject version drift automatically.
<!-- agent:boundary:ba9047c2 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] Release agent-doc v0.31.9
- [x] Add version sync check to `make check`
<!-- /agent:pending -->
