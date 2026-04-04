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
### Re: tmux-router correction — missed working tree changes (HEAD)

You're right. I only ran `git log v0.3.9..HEAD` (no committed changes since tag) but missed `git diff` / `git status` which shows uncommitted modifications to `src/sync.rs`:
- Added `[sync] stashing unmanaged panes` log line with window/panes/session context
- Enhanced `skipped stashing` message with `remaining` count

**Why I got it wrong:** Checked committed history but not the working tree. Should have run both `git log` and `git status`.

**Pending for tmux-router:** Commit the sync.rs debug logging, decide if it warrants a release (v0.3.10) or can wait.
<!-- agent:boundary:fbab23ec -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Commit tmux-router sync.rs debug logging changes
- [ ] Commit agent-doc submodule changes (runbook updates, skill.rs)
- [ ] Release agent-doc (version TBD)
- [ ] Release tmux-router (if warranted)
<!-- /agent:pending -->
