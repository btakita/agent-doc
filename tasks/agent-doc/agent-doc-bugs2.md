---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
**Bugs fixed:** race condition from multiple stash panes claiming same doc; streaming checkpoint orphans; multi-session doc accumulation. **Safeguards implemented (all 4 layers):** L1 registry uniqueness (already done), L2 preflight CONFLICT hard-block, L3 cycle lock across full preflight→commit span, L4 orphaned checkpoint auto-discard. **git.rs change:** `agent-doc commit` now stages document file (HEAD stripped) so responses don't accumulate across sessions — blue gutter marks cycle boundary. **Files cleaned up:** agent-doc-bugs.md and claudescore.md committed.
❯ Why?
```
● Bash(agent-doc preflight tasks/agent-doc/agent-doc-feat.md)
  ⎿  [preflight] step 0: layout check
     [preflight] layout issue: stash window 'stash' has non-idle pane %1 running 'agent-doc'
     [preflight] layout issue: stash window 'stash' has non-idle pane %36 running 'agent-doc'
```
### Re: stash pane layout issues persisting

The safeguards (L1–L4) prevent **new** races — they don't retroactively kill existing stash panes.

**Why panes %1, %34, %36, %39, %40, %41 are still there:**

These panes were spawned by `route.rs` before the fixes, parked in the stash window, and are still sitting there running active `agent-doc` sessions on their respective documents. Nothing in the implementation evicts pre-existing stash panes.

**Why the preflight for `agent-doc-feat.md` shows %1 and %36:**

`check_layout()` scans all tmux panes. Any pane in the stash window running `agent-doc` gets flagged as a layout issue. These are informational unless one of them actually claims the same document — then it becomes a `CONFLICT:` and the cycle aborts (L2).

For `agent-doc-feat.md`, neither %1 nor %36 claims that document, so preflight continues normally.

**To clean these up now:** kill the stash panes manually:

```bash
tmux kill-pane -t %1
tmux kill-pane -t %34
tmux kill-pane -t %36
tmux kill-pane -t %39
tmux kill-pane -t %40
tmux kill-pane -t %41
```

**Missing safeguard — stash pane eviction:** The existing fixes don't address *how panes get stashed in the first place*. A follow-up fix in `route.rs` could: when routing a new session for document X, if an existing stash pane already claims X, kill the stash pane and reuse the route rather than adding another one.

❯ ran the command. all ok?
### Re: ran the command — all ok?

Not fully. %40 and %41 are gone, but **%43 and %46 are still running `agent-doc` in the stash window** (visible in this preflight's layout issues).

Kill those two:

```bash
tmux kill-pane -t %43
tmux kill-pane -t %46
```

After that the stash window should be clean.

❯ Why do I need to kill these panes? The previous killing 6 tmux panes resulting in already running panes being killed.
❯ Why are there now 2 kill-panes recommended?
### Re: Why kill panes? Why only 2? (HEAD)

**Why you keep having to kill panes:**

The routing code in `route.rs` spawns a new tmux pane into the stash window every time a document session is opened, *without checking if one already exists for that document*. So:

1. You open `/agent-doc agent-doc-bugs2.md` → new stash pane spawned
2. You kill it
3. You open it again → another new stash pane spawned

Killing manually is whack-a-mole — the root bug keeps recreating them.

**Why the count keeps changing (6 → 2 → 3):**

The numbers I reported were just whatever happened to be in the stash window at that moment. Some of the original 6 may have been from older orphaned sessions. New ones appeared because the bug ran again between cycles.

**The real fix:**

Stash pane eviction in `route.rs` — before spawning a new pane for document X, check if a stash pane already claims X and kill it. That's what's in the pending list. Until that's in, killing panes manually achieves nothing lasting.
<!-- agent:boundary:3ce84976 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Implement stash pane eviction in `route.rs` — before spawning, kill any existing stash pane claiming the same document (this is the root fix; manual kills are whack-a-mole without it)
<!-- /agent:pending -->
