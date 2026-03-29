---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.27.2 released. Idle stash cleanup + boundary fix shipped.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/4674cbdd322fce519fe66718682225f86b088adb9a6e63aa6f5df754c59a4ef1-20260329-003821.md`*

I got another agent-doc claim error.
When calling `Claim for Tmux Pane` on tasks/software/joinhandshake.md
![img.png](img.png)
### Claim error analysis

The claim itself **succeeded** — pane %110 was created for `joinhandshake.md`. The errors you see are resync warnings about 7 orphaned stashed panes that can't be returned to a visible window.

All 7 stashed panes (%104, %109, %106, %108, %102, %107, %111) are idle Claude sessions sitting at the `❯` prompt. They're pre-fix leftovers that accumulate because:
1. v0.27.0 correctly keeps them registered (doesn't deregister)
2. But resync has no way to return them — "no valid target found"
3. They stay in stash forever, growing the warning list

**Two-part fix needed:**
1. **Immediate:** Kill the 7 orphaned panes now
2. **Permanent:** Add auto-cleanup to resync — when a stashed pane has an idle Claude session and no document claim, kill it instead of keeping it registered

Want me to kill the orphaned panes and implement the auto-cleanup in resync?
yes.
### Done

**Killed 7 orphaned panes:** %104, %109, %106, %108, %102, %107, %111 — all idle Claude sessions.

**Implemented auto-cleanup** in `return_stashed_panes_bulk()` (resync.rs):

When a stashed pane meets ALL of these conditions:
1. Running `agent-doc` or `claude` (not an idle shell like zsh/bash)
2. Shows `❯` in the last 3 non-empty lines (idle Claude prompt)
3. `find_return_target_bulk()` returns `None` (no window to return to)

→ Kill the pane via `tmux.kill_pane()` and deregister it.

Previously these panes accumulated forever because tmux reported them as running `agent-doc` (not an idle shell), so the existing idle-shell deregistration didn't apply.

**Committed:** `634985c` — fix: auto-kill idle stashed Claude sessions with no return target
**Installed:** `~/.cargo/bin/agent-doc` updated

Ready to release when you want, or test first.
release
### v0.27.2 released (HEAD)

**Published everywhere:**
- **crates.io:** agent-doc v0.27.2 ✓
- **PyPI:** agent_doc v0.27.2 ✓
- **GitHub Release:** [v0.27.2](https://github.com/btakita/agent-doc/releases/tag/v0.27.2) with binary ✓
- **cargo install:** updated ✓

**Changelog:**
- Auto-kill idle stashed Claude sessions with no return target
- Remove stale boundary disk write (from v0.27.1)
<!-- agent:boundary:52208247 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] v0.27.0: Fix stash rescue deregistration
- [x] v0.27.1: Fix "externally modified" popup
- [x] v0.27.2: Auto-kill idle stashed Claude sessions
- All tasks complete.
<!-- /agent:pending -->
