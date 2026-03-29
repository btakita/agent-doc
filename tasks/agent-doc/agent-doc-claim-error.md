---
agent_doc_session: bef800bc-950e-4f9e-b861-92c249853397
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->
v0.27.3 released. Auto-kill reverted, stash rescue stable.
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
There was an active claude run in tasks/software/joinhandshake.md
When I moved to another file in the right editor and back to joinhandshake, a new claude session was created.
What happened to the active session?
### v0.27.2 released

**Published everywhere:**
- **crates.io:** agent-doc v0.27.2 ✓
- **PyPI:** agent_doc v0.27.2 ✓
- **GitHub Release:** [v0.27.2](https://github.com/btakita/agent-doc/releases/tag/v0.27.2) with binary ✓
- **cargo install:** updated ✓

**Changelog:**
- Auto-kill idle stashed Claude sessions with no return target
- Remove stale boundary disk write (from v0.27.1)
### Root cause: auto-cleanup killed active sessions

The v0.27.2 auto-cleanup (`is_idle_claude_pane()`) was **too aggressive**. It killed your active joinhandshake session because:

1. User switches away → joinhandshake pane stashed → Claude sitting at `❯` prompt
2. Resync runs → sees `❯` in last 3 lines → classifies as "idle orphan" → **kills it**
3. User switches back → pane gone → creates new session (context lost)

**The `❯` prompt is the normal state of a stashed session waiting to be rescued.** It's NOT an orphan.

**Fix:** Reverted the auto-kill logic from `return_stashed_panes_bulk()`. Back to v0.27.0 behavior:
- Idle shells (zsh/bash) → deregister
- Active processes (agent-doc/claude) → keep registered, let `rescue_from_stash()` handle them

**Committed:** `4fa3377` — revert: remove auto-kill of idle stashed Claude sessions
**Installed:** `~/.cargo/bin/agent-doc` updated

The orphaned pane accumulation problem remains unsolved but is less harmful than killing active sessions. A better approach would be a separate `agent-doc gc` command that cleans up panes with no registered file, rather than inline cleanup during resync.

Ready to release v0.27.3 with the revert.

release
### v0.27.3 released

**Published everywhere:**
- **crates.io:** v0.27.3 ✓
- **PyPI:** v0.27.3 ✓
- **GitHub Release:** [v0.27.3](https://github.com/btakita/agent-doc/releases/tag/v0.27.3) ✓
- **cargo install:** updated ✓

Reverts the v0.27.2 auto-kill. Back to stable stash rescue behavior.
<!-- agent:boundary:7442f0ed -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] v0.27.0: Fix stash rescue deregistration
- [x] v0.27.1: Fix "externally modified" popup
- [x] v0.27.3: Revert auto-kill (was killing active sessions)
- All shipped. Future: `agent-doc gc` for safe orphan cleanup.
<!-- /agent:pending -->
